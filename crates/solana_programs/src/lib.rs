//! # Atlantis On-Chain Solana Contracts (`solana_programs`)
//!
//! Implements the V2 Solana Token-2022 CYAN token logic, $10 minimum qualifying stake
//! validation, $1 monthly subscription rules, anti-whale quadratic DAO voting (`sqrt(stake)`),
//! 33.33% / 33.33% / 33.33% tripartite governance, M-of-N timelocked treasury vault,
//! and `.nautpak` Pearl Enclave DRM asset protection.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub mod slashing;
pub use slashing::*;

pub const MIN_QUALIFYING_STAKE_USD: u64 = 10;
pub const MONTHLY_SUBSCRIPTION_FEE_USD: u64 = 1;
pub const MAX_CYAN_TOKEN_SUPPLY: u64 = 1_000_000_000;

/// Maximum basis points (33.33% = 3333 bps out of 10,000 bps) that the Staking pillar
/// may hold in total protocol governance weight.
pub const MAX_STAKE_GOVERNANCE_BPS: u64 = 3_333;
pub const TOTAL_BASIS_POINTS: u64 = 10_000;

/// Magic header bytes for `.nautpak` encrypted 3D & Gaussian Splatting asset containers.
pub const NAUTPAK_MAGIC: [u8; 8] = *b"NAUTPAK2";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SolanaProgramError {
    #[error("Unauthorized authority `{0}`")]
    Unauthorized(String),
    #[error("Mint would exceed MAX_CYAN_TOKEN_SUPPLY ({MAX_CYAN_TOKEN_SUPPLY}): requested {requested}, current {current}")]
    MaxSupplyExceeded { current: u64, requested: u64 },
    #[error("Minting is frozen or permanently disabled")]
    MintDisabled,
    #[error("Invalid transfer fee configuration: {0} bps exceeds max limit")]
    InvalidTransferFeeBps(u16),
    #[error("Insufficient stake ({0} USD) to qualify for minimum $10 requirement")]
    InsufficientQualifyingStake(u64),
    #[error("Stake governance share {0} bps exceeds the 33.33% (3333 bps) protocol invariant cap")]
    StakeGovernanceCapExceeded(u64),
    #[error("Proposal `{0}` is not in Active status")]
    ProposalNotActive(String),
    #[error("Proposal `{0}` has not succeeded and cannot be executed")]
    ProposalNotSucceeded(String),
    #[error("Proposal `{proposal_id}` voting window has not closed yet (now {now}, ends at {voting_ends_at})")]
    VotingStillOpen {
        proposal_id: String,
        now: u64,
        voting_ends_at: u64,
    },
    #[error("Voter `{0}` has already cast a vote on proposal `{1}`")]
    DuplicateVote(String, String),
    #[error("Treasury transaction `{0}` not found")]
    TreasuryTxNotFound(u64),
    #[error("Treasury transaction `{0}` was already executed")]
    TreasuryTxAlreadyExecuted(u64),
    #[error("Insufficient multi-sig approvals: got {got}, required {required}")]
    InsufficientApprovals { got: usize, required: usize },
    #[error("Timelock has not expired: current timestamp {now}, unlock timestamp {unlock_at}")]
    TimelockNotExpired { now: u64, unlock_at: u64 },
    #[error("Insufficient treasury balance: available {available}, requested {requested}")]
    InsufficientTreasuryFunds { available: u64, requested: u64 },
    #[error("Invalid multi-sig threshold configuration (threshold {threshold}, signers {total})")]
    InvalidMultiSigConfig { threshold: usize, total: usize },
    #[error("Invalid .nautpak magic header or unsupported DRM envelope")]
    InvalidNautpakHeader,
    #[error("Key rotation epoch mismatch: asset requires epoch {expected}, enclave at {actual}")]
    EpochMismatch { expected: u64, actual: u64 },
    #[error("Authentication tag verification failed (tampered .nautpak payload or unauthorized rip attempt)")]
    AuthenticationTagMismatch,
}

/// Integer Babylonian square-root for deterministic on-chain anti-whale quadratic voting.
pub fn integer_sqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = n / 2;
    let mut y = (x + n / x) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StakePositionAccount {
    pub owner_pubkey: String,
    pub staked_usd_value: u64,
    pub active_subscription: bool,
}

impl StakePositionAccount {
    pub fn qualifies_for_subscription(&self) -> bool {
        self.staked_usd_value >= MIN_QUALIFYING_STAKE_USD
    }

    /// Calculates quadratic governance voting power (`sqrt(stake)`).
    pub fn quadratic_voting_power(&self) -> u64 {
        integer_sqrt(self.staked_usd_value)
    }
}

// ============================================================================
// 1. Token-2022 CYAN Mint (`CyanToken2022Mint`)
// ============================================================================

/// Token-2022 Transfer Fee Extension configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferFeeExtensionConfig {
    /// Fee in basis points (1 bp = 0.01%, max 10,000).
    pub transfer_fee_basis_points: u16,
    /// Maximum fee in CYAN tokens charged on a single transfer.
    pub maximum_fee: u64,
    /// Authority allowed to withdraw withheld transfer fees to the DAO Treasury.
    pub withdraw_withheld_authority: String,
}

/// Models the Solana Token-2022 mint state for CYAN with hard supply cap enforcement,
/// transfer fee extension configuration, and mint authority verification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CyanToken2022Mint {
    pub mint_address: String,
    pub mint_authority: String,
    pub freeze_authority: Option<String>,
    pub decimals: u8,
    pub current_supply: u64,
    pub max_supply: u64,
    pub minting_enabled: bool,
    pub transfer_fee_config: TransferFeeExtensionConfig,
    pub withheld_fees_accumulated: u64,
}

impl CyanToken2022Mint {
    pub fn new(
        mint_address: impl Into<String>,
        mint_authority: impl Into<String>,
        transfer_fee_basis_points: u16,
        maximum_fee: u64,
    ) -> Result<Self, SolanaProgramError> {
        if transfer_fee_basis_points > 10_000 {
            return Err(SolanaProgramError::InvalidTransferFeeBps(
                transfer_fee_basis_points,
            ));
        }
        let authority = mint_authority.into();
        Ok(Self {
            mint_address: mint_address.into(),
            mint_authority: authority.clone(),
            freeze_authority: Some(authority.clone()),
            decimals: 6,
            current_supply: 0,
            max_supply: MAX_CYAN_TOKEN_SUPPLY,
            minting_enabled: true,
            transfer_fee_config: TransferFeeExtensionConfig {
                transfer_fee_basis_points,
                maximum_fee,
                withdraw_withheld_authority: authority,
            },
            withheld_fees_accumulated: 0,
        })
    }

    /// Mints CYAN tokens while strictly enforcing `mint_authority` and `MAX_CYAN_TOKEN_SUPPLY`.
    pub fn mint_to(&mut self, caller_authority: &str, amount: u64) -> Result<u64, SolanaProgramError> {
        if !self.minting_enabled {
            return Err(SolanaProgramError::MintDisabled);
        }
        if caller_authority != self.mint_authority {
            return Err(SolanaProgramError::Unauthorized(caller_authority.to_string()));
        }
        let new_supply = self.current_supply.checked_add(amount).ok_or(
            SolanaProgramError::MaxSupplyExceeded {
                current: self.current_supply,
                requested: amount,
            },
        )?;
        if new_supply > self.max_supply || new_supply > MAX_CYAN_TOKEN_SUPPLY {
            return Err(SolanaProgramError::MaxSupplyExceeded {
                current: self.current_supply,
                requested: amount,
            });
        }
        self.current_supply = new_supply;
        Ok(self.current_supply)
    }

    /// Calculates the Token-2022 transfer fee for a given transfer amount and records withheld fees.
    pub fn apply_transfer_with_fee(&mut self, amount: u64) -> (u64, u64) {
        let raw_fee = (amount as u128 * self.transfer_fee_config.transfer_fee_basis_points as u128)
            / (TOTAL_BASIS_POINTS as u128);
        let fee = (raw_fee as u64).min(self.transfer_fee_config.maximum_fee);
        let net_amount = amount.saturating_sub(fee);
        self.withheld_fees_accumulated = self.withheld_fees_accumulated.saturating_add(fee);
        (net_amount, fee)
    }

    /// Harvests accumulated Token-2022 transfer fees into the DAO Treasury.
    pub fn harvest_withheld_fees(&mut self, caller_authority: &str) -> Result<u64, SolanaProgramError> {
        if caller_authority != self.transfer_fee_config.withdraw_withheld_authority {
            return Err(SolanaProgramError::Unauthorized(caller_authority.to_string()));
        }
        let harvested = self.withheld_fees_accumulated;
        self.withheld_fees_accumulated = 0;
        Ok(harvested)
    }
}

// ============================================================================
// 2. Tripartite 33.33% / 33.33% / 33.33% DAO Governance (`DaoGovernanceEngine`)
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoteChoice {
    For,
    Against,
    Abstain,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalState {
    Active,
    Succeeded,
    Defeated,
    Executed,
    Expired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PillarVoteTally {
    pub votes_for: u64,
    pub votes_against: u64,
    pub votes_abstain: u64,
}

impl PillarVoteTally {
    pub fn record(&mut self, choice: VoteChoice, weight: u64) {
        match choice {
            VoteChoice::For => self.votes_for = self.votes_for.saturating_add(weight),
            VoteChoice::Against => self.votes_against = self.votes_against.saturating_add(weight),
            VoteChoice::Abstain => self.votes_abstain = self.votes_abstain.saturating_add(weight),
        }
    }

    /// Returns the approval ratio in basis points (0..=10,000) among decisive (`For + Against`) votes.
    pub fn approval_bps(&self) -> u64 {
        let decisive = self.votes_for.saturating_add(self.votes_against);
        if decisive == 0 {
            return 0;
        }
        ((self.votes_for as u128 * TOTAL_BASIS_POINTS as u128) / (decisive as u128)) as u64
    }

    pub fn total_participation(&self) -> u64 {
        self.votes_for
            .saturating_add(self.votes_against)
            .saturating_add(self.votes_abstain)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaoProposal {
    pub proposal_id: String,
    pub title: String,
    pub proposer: String,
    pub created_at: u64,
    pub voting_ends_at: u64,
    pub state: ProposalState,
    /// Pillar 1 (33.33% max cap): Quadratic stake voting power (`sqrt(staked_usd_value)`).
    pub staking_tally: PillarVoteTally,
    /// Pillar 2 (33.33%): Platform usage / active subscriber power.
    pub platform_tally: PillarVoteTally,
    /// Pillar 3 (33.33%): Community / PoH contribution power.
    pub community_tally: PillarVoteTally,
    pub voted_principals: BTreeSet<String>,
}

/// Implements the 33.33% Staking / 33.33% Platform / 33.33% Community governance model,
/// anti-whale quadratic voting (`integer_sqrt`), and the 33.33% stake governance cap invariant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoGovernanceEngine {
    pub proposals: BTreeMap<String, DaoProposal>,
    /// Basis points allocated to Staking pillar (must be <= 3333 bps = 33.33%).
    pub stake_pillar_weight_bps: u64,
    /// Basis points allocated to Platform pillar (3333 bps = 33.33%).
    pub platform_pillar_weight_bps: u64,
    /// Basis points allocated to Community pillar (3334 bps = 33.34% so total = 10,000 bps).
    pub community_pillar_weight_bps: u64,
    /// Approval threshold in basis points (e.g., 5001 = simple majority > 50.00%).
    pub approval_threshold_bps: u64,
}

impl Default for DaoGovernanceEngine {
    fn default() -> Self {
        Self {
            proposals: BTreeMap::new(),
            stake_pillar_weight_bps: 3_333,
            platform_pillar_weight_bps: 3_333,
            community_pillar_weight_bps: 3_334,
            approval_threshold_bps: 5_001,
        }
    }
}

impl DaoGovernanceEngine {
    pub fn new(
        stake_pillar_weight_bps: u64,
        platform_pillar_weight_bps: u64,
        community_pillar_weight_bps: u64,
        approval_threshold_bps: u64,
    ) -> Result<Self, SolanaProgramError> {
        let engine = Self {
            proposals: BTreeMap::new(),
            stake_pillar_weight_bps,
            platform_pillar_weight_bps,
            community_pillar_weight_bps,
            approval_threshold_bps,
        };
        engine.verify_stake_governance_cap()?;
        Ok(engine)
    }

    /// Schema §21 Audit Invariant: `stake_governance <= 33.33%` (3333 bps).
    pub fn verify_stake_governance_cap(&self) -> Result<(), SolanaProgramError> {
        if self.stake_pillar_weight_bps > MAX_STAKE_GOVERNANCE_BPS {
            return Err(SolanaProgramError::StakeGovernanceCapExceeded(
                self.stake_pillar_weight_bps,
            ));
        }
        Ok(())
    }

    pub fn create_proposal(
        &mut self,
        proposal_id: impl Into<String>,
        title: impl Into<String>,
        proposer: impl Into<String>,
        created_at: u64,
        voting_ends_at: u64,
    ) -> Result<&DaoProposal, SolanaProgramError> {
        self.verify_stake_governance_cap()?;
        let id = proposal_id.into();
        let proposal = DaoProposal {
            proposal_id: id.clone(),
            title: title.into(),
            proposer: proposer.into(),
            created_at,
            voting_ends_at,
            state: ProposalState::Active,
            staking_tally: PillarVoteTally::default(),
            platform_tally: PillarVoteTally::default(),
            community_tally: PillarVoteTally::default(),
            voted_principals: BTreeSet::new(),
        };
        self.proposals.insert(id.clone(), proposal);
        Ok(self.proposals.get(&id).unwrap())
    }

    /// Casts a tripartite vote on an active proposal using anti-whale quadratic staking power (`sqrt(stake)`),
    /// platform power, and community power.
    pub fn cast_vote(
        &mut self,
        proposal_id: &str,
        voter_stake_account: &StakePositionAccount,
        platform_power: u64,
        community_power: u64,
        choice: VoteChoice,
        current_timestamp: u64,
    ) -> Result<u64, SolanaProgramError> {
        self.verify_stake_governance_cap()?;
        let proposal = self
            .proposals
            .get_mut(proposal_id)
            .ok_or_else(|| SolanaProgramError::ProposalNotActive(proposal_id.to_string()))?;

        if proposal.state != ProposalState::Active || current_timestamp > proposal.voting_ends_at {
            return Err(SolanaProgramError::ProposalNotActive(proposal_id.to_string()));
        }

        if !voter_stake_account.qualifies_for_subscription()
            && platform_power == 0
            && community_power == 0
        {
            return Err(SolanaProgramError::InsufficientQualifyingStake(
                voter_stake_account.staked_usd_value,
            ));
        }

        if proposal
            .voted_principals
            .contains(&voter_stake_account.owner_pubkey)
        {
            return Err(SolanaProgramError::DuplicateVote(
                voter_stake_account.owner_pubkey.clone(),
                proposal_id.to_string(),
            ));
        }

        let quadratic_stake_weight = if voter_stake_account.qualifies_for_subscription() {
            voter_stake_account.quadratic_voting_power()
        } else {
            0
        };
        if quadratic_stake_weight > 0 {
            proposal
                .staking_tally
                .record(choice, quadratic_stake_weight);
        }
        proposal.platform_tally.record(choice, platform_power);
        proposal.community_tally.record(choice, community_power);
        proposal
            .voted_principals
            .insert(voter_stake_account.owner_pubkey.clone());

        Ok(quadratic_stake_weight)
    }

    /// Computes the normalized 33.33% / 33.33% / 33.33% weighted approval score in basis points (0..=10,000).
    /// Even if whales stake millions of dollars, the entire Staking pillar cannot contribute more than
    /// `stake_pillar_weight_bps` (3,333 bps = 33.33%) toward final passage.
    pub fn calculate_composite_approval_bps(&self, proposal: &DaoProposal) -> u64 {
        let stake_contribution =
            (proposal.staking_tally.approval_bps() * self.stake_pillar_weight_bps) / TOTAL_BASIS_POINTS;
        let platform_contribution =
            (proposal.platform_tally.approval_bps() * self.platform_pillar_weight_bps)
                / TOTAL_BASIS_POINTS;
        let community_contribution =
            (proposal.community_tally.approval_bps() * self.community_pillar_weight_bps)
                / TOTAL_BASIS_POINTS;

        stake_contribution + platform_contribution + community_contribution
    }

    /// Finalizes a proposal after its voting window closes (or when called at `current_timestamp`).
    pub fn finalize_proposal(
        &mut self,
        proposal_id: &str,
        current_timestamp: u64,
        expiration_grace_period: u64,
    ) -> Result<ProposalState, SolanaProgramError> {
        self.verify_stake_governance_cap()?;
        let proposal = self
            .proposals
            .get_mut(proposal_id)
            .ok_or_else(|| SolanaProgramError::ProposalNotActive(proposal_id.to_string()))?;

        if proposal.state != ProposalState::Active {
            return Ok(proposal.state);
        }

        if current_timestamp < proposal.voting_ends_at {
            return Err(SolanaProgramError::VotingStillOpen {
                proposal_id: proposal_id.to_string(),
                now: current_timestamp,
                voting_ends_at: proposal.voting_ends_at,
            });
        }

        if current_timestamp > proposal.voting_ends_at.saturating_add(expiration_grace_period) {
            proposal.state = ProposalState::Expired;
            return Ok(ProposalState::Expired);
        }

        let total_votes = proposal.staking_tally.total_participation()
            + proposal.platform_tally.total_participation()
            + proposal.community_tally.total_participation();

        if total_votes == 0 {
            proposal.state = ProposalState::Defeated;
            return Ok(ProposalState::Defeated);
        }

        // Compute weighted approval locally before mutating
        let stake_bps =
            (proposal.staking_tally.approval_bps() * self.stake_pillar_weight_bps) / TOTAL_BASIS_POINTS;
        let platform_bps = (proposal.platform_tally.approval_bps() * self.platform_pillar_weight_bps)
            / TOTAL_BASIS_POINTS;
        let community_bps = (proposal.community_tally.approval_bps()
            * self.community_pillar_weight_bps)
            / TOTAL_BASIS_POINTS;
        let composite_bps = stake_bps + platform_bps + community_bps;

        if composite_bps >= self.approval_threshold_bps {
            proposal.state = ProposalState::Succeeded;
        } else {
            proposal.state = ProposalState::Defeated;
        }

        Ok(proposal.state)
    }

    /// Marks a `Succeeded` proposal as `Executed`.
    pub fn mark_executed(&mut self, proposal_id: &str) -> Result<ProposalState, SolanaProgramError> {
        let proposal = self
            .proposals
            .get_mut(proposal_id)
            .ok_or_else(|| SolanaProgramError::ProposalNotSucceeded(proposal_id.to_string()))?;

        if proposal.state != ProposalState::Succeeded {
            return Err(SolanaProgramError::ProposalNotSucceeded(
                proposal_id.to_string(),
            ));
        }
        proposal.state = ProposalState::Executed;
        Ok(ProposalState::Executed)
    }
}

// ============================================================================
// 3. M-of-N Timelocked DAO Treasury (`TreasuryMultiSigVault`)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingDisbursement {
    pub tx_id: u64,
    pub recipient: String,
    pub amount_cyan: u64,
    pub dao_proposal_id: String,
    pub created_at: u64,
    pub unlock_timestamp: u64,
    pub approvals: BTreeSet<String>,
    pub executed: bool,
}

/// Requires M-of-N authority approvals and timelock verification before disbursing DAO treasury funds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreasuryMultiSigVault {
    pub vault_address: String,
    pub authorities: BTreeSet<String>,
    pub threshold_m: usize,
    pub timelock_delay_secs: u64,
    pub balance_cyan: u64,
    pub next_tx_id: u64,
    pub pending_disbursements: BTreeMap<u64, PendingDisbursement>,
}

impl TreasuryMultiSigVault {
    pub fn new(
        vault_address: impl Into<String>,
        authorities: Vec<String>,
        threshold_m: usize,
        timelock_delay_secs: u64,
        initial_balance_cyan: u64,
    ) -> Result<Self, SolanaProgramError> {
        let auth_set: BTreeSet<String> = authorities.into_iter().collect();
        if threshold_m == 0 || threshold_m > auth_set.len() {
            return Err(SolanaProgramError::InvalidMultiSigConfig {
                threshold: threshold_m,
                total: auth_set.len(),
            });
        }
        Ok(Self {
            vault_address: vault_address.into(),
            authorities: auth_set,
            threshold_m,
            timelock_delay_secs,
            balance_cyan: initial_balance_cyan,
            next_tx_id: 1,
            pending_disbursements: BTreeMap::new(),
        })
    }

    pub fn deposit(&mut self, amount_cyan: u64) {
        self.balance_cyan = self.balance_cyan.saturating_add(amount_cyan);
    }

    /// Queues a DAO-authorized disbursement and automatically records the proposer's signature.
    pub fn queue_disbursement(
        &mut self,
        proposer_authority: &str,
        recipient: impl Into<String>,
        amount_cyan: u64,
        dao_proposal_id: impl Into<String>,
        current_timestamp: u64,
    ) -> Result<u64, SolanaProgramError> {
        if !self.authorities.contains(proposer_authority) {
            return Err(SolanaProgramError::Unauthorized(
                proposer_authority.to_string(),
            ));
        }
        if amount_cyan > self.balance_cyan {
            return Err(SolanaProgramError::InsufficientTreasuryFunds {
                available: self.balance_cyan,
                requested: amount_cyan,
            });
        }

        let tx_id = self.next_tx_id;
        self.next_tx_id += 1;

        let mut approvals = BTreeSet::new();
        approvals.insert(proposer_authority.to_string());

        let tx = PendingDisbursement {
            tx_id,
            recipient: recipient.into(),
            amount_cyan,
            dao_proposal_id: dao_proposal_id.into(),
            created_at: current_timestamp,
            unlock_timestamp: current_timestamp.saturating_add(self.timelock_delay_secs),
            approvals,
            executed: false,
        };
        self.pending_disbursements.insert(tx_id, tx);
        Ok(tx_id)
    }

    /// Adds an M-of-N authority approval to a queued disbursement.
    pub fn approve_disbursement(
        &mut self,
        signer_authority: &str,
        tx_id: u64,
    ) -> Result<usize, SolanaProgramError> {
        if !self.authorities.contains(signer_authority) {
            return Err(SolanaProgramError::Unauthorized(
                signer_authority.to_string(),
            ));
        }
        let tx = self
            .pending_disbursements
            .get_mut(&tx_id)
            .ok_or(SolanaProgramError::TreasuryTxNotFound(tx_id))?;

        if tx.executed {
            return Err(SolanaProgramError::TreasuryTxAlreadyExecuted(tx_id));
        }

        tx.approvals.insert(signer_authority.to_string());
        Ok(tx.approvals.len())
    }

    /// Executes the disbursement once >= M-of-N approvals are collected and the timelock has elapsed.
    pub fn execute_disbursement(
        &mut self,
        tx_id: u64,
        current_timestamp: u64,
    ) -> Result<u64, SolanaProgramError> {
        let tx = self
            .pending_disbursements
            .get_mut(&tx_id)
            .ok_or(SolanaProgramError::TreasuryTxNotFound(tx_id))?;

        if tx.executed {
            return Err(SolanaProgramError::TreasuryTxAlreadyExecuted(tx_id));
        }
        if tx.approvals.len() < self.threshold_m {
            return Err(SolanaProgramError::InsufficientApprovals {
                got: tx.approvals.len(),
                required: self.threshold_m,
            });
        }
        if current_timestamp < tx.unlock_timestamp {
            return Err(SolanaProgramError::TimelockNotExpired {
                now: current_timestamp,
                unlock_at: tx.unlock_timestamp,
            });
        }
        if tx.amount_cyan > self.balance_cyan {
            return Err(SolanaProgramError::InsufficientTreasuryFunds {
                available: self.balance_cyan,
                requested: tx.amount_cyan,
            });
        }

        self.balance_cyan -= tx.amount_cyan;
        tx.executed = true;
        Ok(self.balance_cyan)
    }
}

// ============================================================================
// 4. `.nautpak` Pearl Enclave DRM Asset Protection (`NautpakAssetVault`)
// ============================================================================

/// Authenticated symmetric encryption header for `.nautpak` 3D meshes and Gaussian splat scenes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NautpakDrmHeader {
    pub magic: [u8; 8],
    pub version: u16,
    pub asset_id: String,
    pub key_rotation_epoch: u64,
    pub nonce: [u8; 12],
    pub auth_tag: [u8; 16],
    pub payload_len: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NautpakEnvelope {
    pub header: NautpakDrmHeader,
    pub ciphertext: Vec<u8>,
}

/// Implements the `.nautpak` asset protection & DRM envelope model with authenticated
/// symmetric encryption, key rotation epochs, and ephemeral `Pearl Enclave` session key
/// derivation so 3D assets and splats cannot be ripped by tools like AssetRipper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NautpakAssetVault {
    pub current_epoch: u64,
    master_vault_secret: [u8; 32],
}

impl NautpakAssetVault {
    pub fn new(current_epoch: u64, master_vault_secret: [u8; 32]) -> Self {
        Self {
            current_epoch,
            master_vault_secret,
        }
    }

    /// Rotates the DRM key epoch, invalidating prior ephemeral session keys.
    pub fn rotate_epoch(&mut self, new_epoch: u64, new_secret: [u8; 32]) {
        self.current_epoch = new_epoch;
        self.master_vault_secret = new_secret;
    }

    /// Derives an ephemeral 256-bit `Pearl Enclave` session key bound to `(epoch, enclave_device_did, asset_id)`.
    pub fn derive_pearl_enclave_session_key(
        &self,
        epoch: u64,
        enclave_device_did: &str,
        asset_id: &str,
    ) -> Result<[u8; 32], SolanaProgramError> {
        if epoch != self.current_epoch {
            return Err(SolanaProgramError::EpochMismatch {
                expected: epoch,
                actual: self.current_epoch,
            });
        }
        Ok(Self::kdf_mix(
            &self.master_vault_secret,
            epoch,
            enclave_device_did.as_bytes(),
            asset_id.as_bytes(),
        ))
    }

    /// Seals a raw 3D / Gaussian Splatting payload into an authenticated `.nautpak` envelope.
    pub fn seal_asset(
        &self,
        asset_id: impl Into<String>,
        enclave_device_did: &str,
        nonce: [u8; 12],
        plaintext: &[u8],
    ) -> Result<NautpakEnvelope, SolanaProgramError> {
        let asset_id_str = asset_id.into();
        let session_key = self.derive_pearl_enclave_session_key(
            self.current_epoch,
            enclave_device_did,
            &asset_id_str,
        )?;
        let ciphertext = Self::xor_keystream(&session_key, &nonce, plaintext);
        let auth_tag = Self::compute_auth_tag(
            &session_key,
            &NAUTPAK_MAGIC,
            self.current_epoch,
            &nonce,
            &asset_id_str,
            &ciphertext,
        );

        Ok(NautpakEnvelope {
            header: NautpakDrmHeader {
                magic: NAUTPAK_MAGIC,
                version: 2,
                asset_id: asset_id_str,
                key_rotation_epoch: self.current_epoch,
                nonce,
                auth_tag,
                payload_len: ciphertext.len(),
            },
            ciphertext,
        })
    }

    /// Verifies the `.nautpak` magic header, key rotation epoch, and authentication tag before
    /// decrypting the asset into ephemeral memory for the authorized `Pearl Enclave` session.
    pub fn unseal_asset(
        &self,
        envelope: &NautpakEnvelope,
        enclave_device_did: &str,
    ) -> Result<Vec<u8>, SolanaProgramError> {
        if envelope.header.magic != NAUTPAK_MAGIC
            || envelope.header.payload_len != envelope.ciphertext.len()
        {
            return Err(SolanaProgramError::InvalidNautpakHeader);
        }

        let session_key = self.derive_pearl_enclave_session_key(
            envelope.header.key_rotation_epoch,
            enclave_device_did,
            &envelope.header.asset_id,
        )?;

        let expected_tag = Self::compute_auth_tag(
            &session_key,
            &envelope.header.magic,
            envelope.header.key_rotation_epoch,
            &envelope.header.nonce,
            &envelope.header.asset_id,
            &envelope.ciphertext,
        );

        if expected_tag != envelope.header.auth_tag {
            return Err(SolanaProgramError::AuthenticationTagMismatch);
        }

        Ok(Self::xor_keystream(
            &session_key,
            &envelope.header.nonce,
            &envelope.ciphertext,
        ))
    }

    fn kdf_mix(secret: &[u8; 32], epoch: u64, did: &[u8], asset_id: &[u8]) -> [u8; 32] {
        let mut state = *secret;
        let epoch_bytes = epoch.to_le_bytes();
        for (i, b) in epoch_bytes
            .iter()
            .chain(did.iter())
            .chain(asset_id.iter())
            .enumerate()
        {
            let idx = i % 32;
            state[idx] = state[idx]
                .wrapping_add(*b)
                .wrapping_mul(0x9E)
                .rotate_left(3)
                ^ epoch_bytes[i % 8];
        }
        // Final diffusion pass across all 32 bytes
        for round in 0..4u8 {
            for i in 0..32 {
                let prev = state[(i + 31) % 32];
                state[i] = state[i].wrapping_add(prev.rotate_left(5)) ^ round.wrapping_mul(0x55);
            }
        }
        state
    }

    fn xor_keystream(session_key: &[u8; 32], nonce: &[u8; 12], data: &[u8]) -> Vec<u8> {
        data.iter()
            .enumerate()
            .map(|(i, &byte)| {
                let k = session_key[i % 32];
                let n = nonce[i % 12];
                let pos = ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) & 0xFF) as u8;
                let mask = k.wrapping_add(n).rotate_left((i % 7 + 1) as u32) ^ pos;
                byte ^ mask
            })
            .collect()
    }

    fn compute_auth_tag(
        session_key: &[u8; 32],
        magic: &[u8; 8],
        epoch: u64,
        nonce: &[u8; 12],
        asset_id: &str,
        ciphertext: &[u8],
    ) -> [u8; 16] {
        let mut tag = [0u8; 16];
        for i in 0..16 {
            tag[i] = session_key[i] ^ session_key[i + 16];
        }
        let epoch_bytes = epoch.to_le_bytes();
        let stream = magic
            .iter()
            .chain(epoch_bytes.iter())
            .chain(nonce.iter())
            .chain(asset_id.as_bytes().iter())
            .chain(ciphertext.iter());

        for (i, &b) in stream.enumerate() {
            let slot = i % 16;
            let k = session_key[(i + slot) % 32];
            tag[slot] = tag[slot]
                .wrapping_add(b ^ k)
                .wrapping_mul(0x5B)
                .rotate_left(3);
        }
        tag
    }
}

// ============================================================================
// 5. Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token2022_supply_cap_and_transfer_fee_enforcement() {
        let mut mint =
            CyanToken2022Mint::new("cyan_mint_2022", "dao_mint_auth", 50, 5_000).unwrap();

        // Unauthorized mint attempt must fail
        assert_eq!(
            mint.mint_to("rogue_actor", 1_000),
            Err(SolanaProgramError::Unauthorized("rogue_actor".to_string()))
        );

        // Mint up to MAX_CYAN_TOKEN_SUPPLY (1,000,000,000)
        assert_eq!(
            mint.mint_to("dao_mint_auth", 600_000_000).unwrap(),
            600_000_000
        );
        assert_eq!(
            mint.mint_to("dao_mint_auth", 400_000_000).unwrap(),
            MAX_CYAN_TOKEN_SUPPLY
        );

        // Minting even 1 token above MAX_CYAN_TOKEN_SUPPLY must fail
        assert_eq!(
            mint.mint_to("dao_mint_auth", 1),
            Err(SolanaProgramError::MaxSupplyExceeded {
                current: MAX_CYAN_TOKEN_SUPPLY,
                requested: 1,
            })
        );

        // Verify Token-2022 transfer fee calculation (50 bps = 0.5%)
        let (net, fee) = mint.apply_transfer_with_fee(100_000);
        assert_eq!(fee, 500);
        assert_eq!(net, 99_500);
        assert_eq!(mint.harvest_withheld_fees("dao_mint_auth").unwrap(), 500);
    }

    #[test]
    fn test_quadratic_voting_math_and_minimum_qualifying_stake() {
        assert_eq!(integer_sqrt(0), 0);
        assert_eq!(integer_sqrt(1), 1);
        assert_eq!(integer_sqrt(10), 3);
        assert_eq!(integer_sqrt(100), 10);
        assert_eq!(integer_sqrt(10_000), 100);
        assert_eq!(integer_sqrt(1_000_000), 1_000);

        let sub_minimum = StakePositionAccount {
            owner_pubkey: "small_user".to_string(),
            staked_usd_value: 9,
            active_subscription: false,
        };
        assert!(!sub_minimum.qualifies_for_subscription());

        let min_staker = StakePositionAccount {
            owner_pubkey: "qualifying_user".to_string(),
            staked_usd_value: MIN_QUALIFYING_STAKE_USD,
            active_subscription: true,
        };
        assert!(min_staker.qualifies_for_subscription());
        assert_eq!(min_staker.quadratic_voting_power(), 3);

        // Whale staking 10,000x more ($100,000 vs $10) only gets ~105x voting power (316 vs 3)
        let whale = StakePositionAccount {
            owner_pubkey: "whale_user".to_string(),
            staked_usd_value: 100_000,
            active_subscription: true,
        };
        assert_eq!(whale.quadratic_voting_power(), 316);
    }

    #[test]
    fn test_33_33_33_dao_proposal_finalization_and_stake_cap_invariant() {
        // Attempting to configure stake governance > 33.33% (3333 bps) must be rejected
        assert_eq!(
            DaoGovernanceEngine::new(3_334, 3_333, 3_333, 5_001).unwrap_err(),
            SolanaProgramError::StakeGovernanceCapExceeded(3_334)
        );

        let mut engine = DaoGovernanceEngine::default();
        engine
            .create_proposal("prop-1", "Upgrade Coral Settlement", "alice", 1_000, 2_000)
            .unwrap();

        // Whale votes FOR in staking pillar ($1,000,000 stake -> sqrt = 1000 stake weight),
        // but 0 platform/community weight.
        let whale = StakePositionAccount {
            owner_pubkey: "whale".to_string(),
            staked_usd_value: 1_000_000,
            active_subscription: true,
        };
        engine
            .cast_vote("prop-1", &whale, 0, 0, VoteChoice::For, 1_100)
            .unwrap();

        // Community & Platform member votes AGAINST with minimum $10 stake (sqrt = 3)
        // and strong platform + community participation (100 each).
        let citizen = StakePositionAccount {
            owner_pubkey: "citizen".to_string(),
            staked_usd_value: 10,
            active_subscription: true,
        };
        engine
            .cast_vote("prop-1", &citizen, 100, 100, VoteChoice::Against, 1_200)
            .unwrap();

        // Even though the whale dominates Staking (1000 vs 3 = 99.7% of Staking pillar),
        // Staking is capped at 33.33% (3333 bps), while Platform & Community (66.67%) voted Against!
        let state = engine.finalize_proposal("prop-1", 2_000, 500).unwrap();
        assert_eq!(state, ProposalState::Defeated);

        // Now test a proposal where Platform and Community align with Staking -> Succeeded & Executed
        engine
            .create_proposal("prop-2", "Fund Siren Frequency", "alice", 1_000, 2_000)
            .unwrap();
        engine
            .cast_vote("prop-2", &citizen, 100, 100, VoteChoice::For, 1_200)
            .unwrap();
        assert_eq!(
            engine.finalize_proposal("prop-2", 2_000, 500).unwrap(),
            ProposalState::Succeeded
        );
        assert_eq!(
            engine.mark_executed("prop-2").unwrap(),
            ProposalState::Executed
        );
    }

    #[test]
    fn test_multisig_treasury_disbursement_and_timelock() {
        let mut vault = TreasuryMultiSigVault::new(
            "treasury_vault_1",
            vec![
                "signer_a".to_string(),
                "signer_b".to_string(),
                "signer_c".to_string(),
            ],
            2,     // 2-of-3 multi-sig required
            3_600, // 1 hour timelock
            500_000,
        )
        .unwrap();

        let tx_id = vault
            .queue_disbursement("signer_a", "dev_grant_recipient", 100_000, "prop-2", 10_000)
            .unwrap();

        // Executing with only 1-of-3 approvals fails
        assert_eq!(
            vault.execute_disbursement(tx_id, 20_000),
            Err(SolanaProgramError::InsufficientApprovals {
                got: 1,
                required: 2
            })
        );

        // Second signer approves (now 2-of-3)
        assert_eq!(vault.approve_disbursement("signer_b", tx_id).unwrap(), 2);

        // Executing before timelock (10,000 + 3,600 = 13,600) fails
        assert_eq!(
            vault.execute_disbursement(tx_id, 12_000),
            Err(SolanaProgramError::TimelockNotExpired {
                now: 12_000,
                unlock_at: 13_600
            })
        );

        // Executing after timelock succeeds and updates balance
        let remaining = vault.execute_disbursement(tx_id, 13_600).unwrap();
        assert_eq!(remaining, 400_000);

        // Replay execution attempt must fail
        assert_eq!(
            vault.execute_disbursement(tx_id, 14_000),
            Err(SolanaProgramError::TreasuryTxAlreadyExecuted(tx_id))
        );
    }

    #[test]
    fn test_nautpak_pearl_enclave_session_key_and_anti_ripper_verification() {
        let mut vault = NautpakAssetVault::new(1, [0x42; 32]);
        let raw_splat_asset = b"GAUSSIAN_SPLAT_3D_CLOUD_REEF_SECTOR_01_VERTEX_BUFFER";
        let nonce = [0x07; 12];
        let authorized_did = "did:nexus:pearl-enclave-node-99";

        let envelope = vault
            .seal_asset("coral_reef_splat.nautpak", authorized_did, nonce, raw_splat_asset)
            .unwrap();
        assert_ne!(envelope.ciphertext, raw_splat_asset);

        // Authorized Pearl Enclave session unseals asset cleanly
        let decrypted = vault.unseal_asset(&envelope, authorized_did).unwrap();
        assert_eq!(decrypted, raw_splat_asset);

        // Unauthorized device / external ripper tool (different DID) fails authentication tag check
        assert_eq!(
            vault.unseal_asset(&envelope, "did:nexus:asset-ripper-spoof"),
            Err(SolanaProgramError::AuthenticationTagMismatch)
        );

        // Tampering with ciphertext bytes triggers AuthenticationTagMismatch
        let mut tampered = envelope.clone();
        tampered.ciphertext[0] ^= 0xFF;
        assert_eq!(
            vault.unseal_asset(&tampered, authorized_did),
            Err(SolanaProgramError::AuthenticationTagMismatch)
        );

        // Rotating the DRM epoch invalidates old envelopes until re-keyed
        vault.rotate_epoch(2, [0x99; 32]);
        assert_eq!(
            vault.unseal_asset(&envelope, authorized_did),
            Err(SolanaProgramError::EpochMismatch {
                expected: 1,
                actual: 2
            })
        );
    }
}

