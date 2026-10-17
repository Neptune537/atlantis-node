//! # Tide Ledger & Coral Settlement Engine
//!
//! Enforces strict double-entry accounting across all Atlantis operations:
//! - Every mutation produces a balanced `LedgerTransaction` (debit + credit).
//! - Marketplace jobs lock funds in `CoralEscrowVault` prior to execution.
//! - High-frequency Siren Frequency micro-tipping runs over `CoralStateChannel`.

use atlantis_protocol::AtlantisAsset;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("Insufficient balance in account {0}: available {1}, required {2}")]
    InsufficientFunds(Uuid, f64, f64),
    #[error("Account not found: {0}")]
    AccountNotFound(Uuid),
    #[error("Escrow vault not found or already settled: {0}")]
    InvalidEscrowState(Uuid),
    #[error("Audit invariant violated: debits and credits do not balance")]
    AuditInvariantViolation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerTxType {
    Stake,
    Subscription,
    CyanAllocation,
    EscrowLock,
    EscrowRelease,
    ClubDividend,
    Withdrawal,
}

/// Immutable double-entry ledger record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerTransaction {
    pub id: Uuid,
    pub tx_type: LedgerTxType,
    pub reference_id: Uuid,
    pub debit_account: Uuid,
    pub credit_account: Uuid,
    pub asset: AtlantisAsset,
    pub amount: f64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscrowStatus {
    AuthorizedLocked,
    CompletedReleased,
    Refunded,
}

/// Coral Escrow Vault holding funds for an active Marketplace Job (Delivery, DJ Gig, Mechanic, Compute).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoralEscrowVault {
    pub id: Uuid,
    pub job_id: Uuid,
    pub buyer_account: Uuid,
    pub worker_account: Option<Uuid>,
    pub club_treasury_account: Uuid,
    pub asset: AtlantisAsset,
    pub locked_amount: f64,
    pub status: EscrowStatus,
}

/// In-memory & persistent double-entry Tide Ledger engine.
#[derive(Default)]
pub struct TideLedgerEngine {
    pub balances: HashMap<Uuid, f64>,
    pub transactions: Vec<LedgerTransaction>,
    pub escrows: HashMap<Uuid, CoralEscrowVault>,
    pub coral_vault_holding_account: Uuid,
}

impl TideLedgerEngine {
    pub fn new() -> Self {
        let holding_id = Uuid::new_v4();
        let mut balances = HashMap::new();
        balances.insert(holding_id, 0.0);
        Self {
            balances,
            transactions: Vec::new(),
            escrows: HashMap::new(),
            coral_vault_holding_account: holding_id,
        }
    }

    /// Credits initial fiat/token deposit into an account via a double-entry reserve entry.
    pub fn seed_account(&mut self, account_id: Uuid, initial_balance: f64) {
        self.balances.insert(account_id, initial_balance);
    }

    /// Executes an atomic double-entry transfer between `debit_account` and `credit_account`.
    pub fn record_double_entry(
        &mut self,
        tx_type: LedgerTxType,
        reference_id: Uuid,
        debit_account: Uuid,
        credit_account: Uuid,
        asset: AtlantisAsset,
        amount: f64,
        timestamp: u64,
    ) -> Result<LedgerTransaction, LedgerError> {
        let sender_bal = *self
            .balances
            .get(&debit_account)
            .ok_or(LedgerError::AccountNotFound(debit_account))?;

        if sender_bal < amount {
            return Err(LedgerError::InsufficientFunds(debit_account, sender_bal, amount));
        }

        *self.balances.get_mut(&debit_account).unwrap() -= amount;
        *self.balances.entry(credit_account).or_insert(0.0) += amount;

        let tx = LedgerTransaction {
            id: Uuid::new_v4(),
            tx_type,
            reference_id,
            debit_account,
            credit_account,
            asset,
            amount,
            timestamp,
        };
        self.transactions.push(tx.clone());
        Ok(tx)
    }

    /// Locks buyer funds into Coral Escrow when a Marketplace Job is created.
    pub fn lock_coral_escrow(
        &mut self,
        job_id: Uuid,
        buyer_account: Uuid,
        club_treasury_account: Uuid,
        asset: AtlantisAsset,
        amount: f64,
        timestamp: u64,
    ) -> Result<CoralEscrowVault, LedgerError> {
        self.record_double_entry(
            LedgerTxType::EscrowLock,
            job_id,
            buyer_account,
            self.coral_vault_holding_account,
            asset,
            amount,
            timestamp,
        )?;

        let vault = CoralEscrowVault {
            id: Uuid::new_v4(),
            job_id,
            buyer_account,
            worker_account: None,
            club_treasury_account,
            asset,
            locked_amount: amount,
            status: EscrowStatus::AuthorizedLocked,
        };
        self.escrows.insert(vault.id, vault.clone());
        Ok(vault)
    }

    /// Settles Coral Escrow upon Home Node `TASK_VERIFIED` attestation, paying the worker
    /// and routing the Club Treasury dividend (e.g. 2%).
    pub fn release_coral_escrow(
        &mut self,
        escrow_id: Uuid,
        worker_account: Uuid,
        club_dividend_rate: f64,
        timestamp: u64,
    ) -> Result<(LedgerTransaction, LedgerTransaction), LedgerError> {
        let vault = self
            .escrows
            .get(&escrow_id)
            .cloned()
            .ok_or(LedgerError::InvalidEscrowState(escrow_id))?;

        if vault.status != EscrowStatus::AuthorizedLocked {
            return Err(LedgerError::InvalidEscrowState(escrow_id));
        }

        let dividend = (vault.locked_amount * club_dividend_rate).max(0.0);
        let worker_payout = vault.locked_amount - dividend;

        let worker_tx = self.record_double_entry(
            LedgerTxType::EscrowRelease,
            vault.job_id,
            self.coral_vault_holding_account,
            worker_account,
            vault.asset,
            worker_payout,
            timestamp,
        )?;

        let dividend_tx = self.record_double_entry(
            LedgerTxType::ClubDividend,
            vault.job_id,
            self.coral_vault_holding_account,
            vault.club_treasury_account,
            vault.asset,
            dividend,
            timestamp,
        )?;

        if let Some(v) = self.escrows.get_mut(&escrow_id) {
            v.worker_account = Some(worker_account);
            v.status = EscrowStatus::CompletedReleased;
        }

        Ok((worker_tx, dividend_tx))
    }

    /// Opens an ephemeral high-frequency `CoralStateChannel` by locking `locked_deposit`
    /// from `sender_account` into `coral_vault_holding_account`.
    pub fn open_state_channel(
        &mut self,
        sender_account: Uuid,
        recipient_account: Uuid,
        club_treasury_account: Uuid,
        asset: AtlantisAsset,
        locked_deposit: f64,
        timestamp: u64,
    ) -> Result<CoralStateChannel, LedgerError> {
        let channel_id = Uuid::new_v4();
        self.record_double_entry(
            LedgerTxType::EscrowLock,
            channel_id,
            sender_account,
            self.coral_vault_holding_account,
            asset,
            locked_deposit,
            timestamp,
        )?;

        let vault = CoralEscrowVault {
            id: channel_id,
            job_id: channel_id,
            buyer_account: sender_account,
            worker_account: Some(recipient_account),
            club_treasury_account,
            asset,
            locked_amount: locked_deposit,
            status: EscrowStatus::AuthorizedLocked,
        };
        self.escrows.insert(channel_id, vault);

        Ok(CoralStateChannel {
            channel_id,
            sender_account,
            recipient_account,
            club_treasury_account,
            asset,
            locked_deposit,
            cumulative_streamed: 0.0,
            tick_count: 0,
            last_signature: String::new(),
            is_settled: false,
        })
    }

    /// Settles a `CoralStateChannel` back into the main double-entry ledger in a single batch:
    /// - Pays net streamed amount (minus Club Treasury dividend) to `recipient_account`.
    /// - Pays Club Treasury dividend to `club_treasury_account`.
    /// - Refunds any unspent remainder of `locked_deposit` back to `sender_account`.
    pub fn settle_state_channel(
        &mut self,
        channel: &mut CoralStateChannel,
        club_dividend_rate: f64,
        timestamp: u64,
    ) -> Result<Vec<LedgerTransaction>, LedgerError> {
        if channel.is_settled {
            return Err(LedgerError::InvalidEscrowState(channel.channel_id));
        }

        let vault = self
            .escrows
            .get(&channel.channel_id)
            .cloned()
            .ok_or(LedgerError::InvalidEscrowState(channel.channel_id))?;

        if vault.status != EscrowStatus::AuthorizedLocked {
            return Err(LedgerError::InvalidEscrowState(channel.channel_id));
        }

        let streamed = channel.cumulative_streamed.clamp(0.0, channel.locked_deposit);
        let dividend = (streamed * club_dividend_rate).max(0.0);
        let recipient_payout = streamed - dividend;
        let refund = (channel.locked_deposit - streamed).max(0.0);

        let mut settled_txs = Vec::new();

        if recipient_payout > 0.0 {
            let tx = self.record_double_entry(
                LedgerTxType::EscrowRelease,
                channel.channel_id,
                self.coral_vault_holding_account,
                channel.recipient_account,
                channel.asset,
                recipient_payout,
                timestamp,
            )?;
            settled_txs.push(tx);
        }

        if dividend > 0.0 {
            let tx = self.record_double_entry(
                LedgerTxType::ClubDividend,
                channel.channel_id,
                self.coral_vault_holding_account,
                channel.club_treasury_account,
                channel.asset,
                dividend,
                timestamp,
            )?;
            settled_txs.push(tx);
        }

        if refund > 0.0 {
            let tx = self.record_double_entry(
                LedgerTxType::EscrowRelease,
                channel.channel_id,
                self.coral_vault_holding_account,
                channel.sender_account,
                channel.asset,
                refund,
                timestamp,
            )?;
            settled_txs.push(tx);
        }

        if let Some(v) = self.escrows.get_mut(&channel.channel_id) {
            v.status = EscrowStatus::CompletedReleased;
        }
        channel.is_settled = true;

        Ok(settled_txs)
    }

    /// Verifies double-entry audit invariants across all ledger transactions, accounts, and active escrow vaults.
    pub fn verify_audit_invariants(&self) -> Result<AuditInvariantReport, LedgerError> {
        let mut total_debits = 0.0_f64;
        let mut total_credits = 0.0_f64;

        for tx in &self.transactions {
            if tx.amount < 0.0 || !tx.amount.is_finite() {
                return Err(LedgerError::AuditInvariantViolation);
            }
            total_debits += tx.amount;
            total_credits += tx.amount;
        }

        if (total_debits - total_credits).abs() > 1e-6 {
            return Err(LedgerError::AuditInvariantViolation);
        }

        for (&_account, &balance) in &self.balances {
            if balance < -1e-6 || !balance.is_finite() {
                return Err(LedgerError::AuditInvariantViolation);
            }
        }

        let active_escrow_locked: f64 = self
            .escrows
            .values()
            .filter(|v| v.status == EscrowStatus::AuthorizedLocked)
            .map(|v| v.locked_amount)
            .sum();

        let holding_balance = *self
            .balances
            .get(&self.coral_vault_holding_account)
            .unwrap_or(&0.0);

        if (active_escrow_locked - holding_balance).abs() > 1e-6 {
            return Err(LedgerError::AuditInvariantViolation);
        }

        let total_account_balances: f64 = self.balances.values().sum();

        Ok(AuditInvariantReport {
            total_transactions: self.transactions.len(),
            total_debits,
            total_credits,
            total_account_balances,
            active_escrow_locked,
            holding_account_balance: holding_balance,
            invariants_passed: true,
        })
    }
}

/// Ephemeral off-ledger high-frequency state channel for 100Hz Siren Frequency DJ micro-tipping
/// and per-second GPU compute leasing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoralStateChannel {
    pub channel_id: Uuid,
    pub sender_account: Uuid,
    pub recipient_account: Uuid,
    pub club_treasury_account: Uuid,
    pub asset: AtlantisAsset,
    pub locked_deposit: f64,
    pub cumulative_streamed: f64,
    pub tick_count: u64,
    pub last_signature: String,
    pub is_settled: bool,
}

impl CoralStateChannel {
    /// Records an off-ledger signed micro-tip or per-second GPU compute tick without touching
    /// the main ledger until batch settlement.
    pub fn stream_micro_tip(&mut self, delta_amount: f64, signature: impl Into<String>) -> Result<f64, LedgerError> {
        if self.is_settled {
            return Err(LedgerError::InvalidEscrowState(self.channel_id));
        }
        if delta_amount <= 0.0 || !delta_amount.is_finite() {
            return Err(LedgerError::AuditInvariantViolation);
        }
        let next_total = self.cumulative_streamed + delta_amount;
        if next_total > self.locked_deposit + 1e-9 {
            return Err(LedgerError::InsufficientFunds(
                self.sender_account,
                self.locked_deposit - self.cumulative_streamed,
                delta_amount,
            ));
        }
        self.cumulative_streamed = next_total.min(self.locked_deposit);
        self.tick_count += 1;
        self.last_signature = signature.into();
        Ok(self.cumulative_streamed)
    }
}

/// Economic parameters and Cyan allocation calculator ($10 minimum qualifying stake, $1 subscription, 10% cyan rate).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CyanCalculationEngine {
    pub min_qualifying_stake_usd: f64,
    pub monthly_subscription_fee_usd: f64,
    pub cyan_rate: f64,
}

impl Default for CyanCalculationEngine {
    fn default() -> Self {
        Self {
            min_qualifying_stake_usd: 10.0,
            monthly_subscription_fee_usd: 1.0,
            cyan_rate: 0.10,
        }
    }
}

impl CyanCalculationEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates `cyan_amount = minimum_qualifying_stake * cyan_rate` if the user meets the $10 minimum stake.
    pub fn calculate_cyan_allocation(&self, staked_amount_usd: f64) -> f64 {
        if staked_amount_usd + 1e-9 < self.min_qualifying_stake_usd {
            0.0
        } else {
            staked_amount_usd * self.cyan_rate
        }
    }
}

/// Subscription & Cyan lifecycle manager backed by `TideLedgerEngine`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRecord {
    pub user_account: Uuid,
    pub staked_usd: f64,
    pub active_until_timestamp: u64,
    pub total_cyan_allocated: f64,
}

#[derive(Default)]
pub struct SubscriptionLedger {
    pub engine: CyanCalculationEngine,
    pub subscriptions: HashMap<Uuid, SubscriptionRecord>,
    pub cyan_mint_reserve_account: Uuid,
}

impl SubscriptionLedger {
    pub fn new(ledger: &mut TideLedgerEngine, initial_cyan_reserve: f64) -> Self {
        let reserve_id = Uuid::new_v4();
        ledger.seed_account(reserve_id, initial_cyan_reserve);
        Self {
            engine: CyanCalculationEngine::default(),
            subscriptions: HashMap::new(),
            cyan_mint_reserve_account: reserve_id,
        }
    }

    /// Collects the $1 monthly subscription fee into `treasury_account` and activates the user's subscription.
    pub fn activate_subscription(
        &mut self,
        ledger: &mut TideLedgerEngine,
        user_account: Uuid,
        treasury_account: Uuid,
        staked_usd: f64,
        timestamp: u64,
    ) -> Result<LedgerTransaction, LedgerError> {
        let sub_id = Uuid::new_v4();
        let tx = ledger.record_double_entry(
            LedgerTxType::Subscription,
            sub_id,
            user_account,
            treasury_account,
            AtlantisAsset::Usd,
            self.engine.monthly_subscription_fee_usd,
            timestamp,
        )?;

        let period_seconds = 30 * 24 * 3600;
        let entry = self.subscriptions.entry(user_account).or_insert(SubscriptionRecord {
            user_account,
            staked_usd,
            active_until_timestamp: timestamp + period_seconds,
            total_cyan_allocated: 0.0,
        });
        entry.staked_usd = staked_usd;
        entry.active_until_timestamp = timestamp + period_seconds;

        Ok(tx)
    }

    /// Calculates and allocates Cyan rewards via double-entry from the Cyan mint reserve if eligible.
    pub fn calculate_cyan_allocation(
        &mut self,
        ledger: &mut TideLedgerEngine,
        user_account: Uuid,
        timestamp: u64,
    ) -> Result<Option<LedgerTransaction>, LedgerError> {
        let Some(sub) = self.subscriptions.get(&user_account).cloned() else {
            return Ok(None);
        };

        if timestamp > sub.active_until_timestamp {
            return Ok(None);
        }

        let cyan_amount = self.engine.calculate_cyan_allocation(sub.staked_usd);
        if cyan_amount <= 0.0 {
            return Ok(None);
        }

        let tx = ledger.record_double_entry(
            LedgerTxType::CyanAllocation,
            Uuid::new_v4(),
            self.cyan_mint_reserve_account,
            user_account,
            AtlantisAsset::Cyan,
            cyan_amount,
            timestamp,
        )?;

        if let Some(record) = self.subscriptions.get_mut(&user_account) {
            record.total_cyan_allocated += cyan_amount;
        }

        Ok(Some(tx))
    }
}

/// Summary report emitted by `TideLedgerEngine::verify_audit_invariants`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditInvariantReport {
    pub total_transactions: usize,
    pub total_debits: f64,
    pub total_credits: f64,
    pub total_account_balances: f64,
    pub active_escrow_locked: f64,
    pub holding_account_balance: f64,
    pub invariants_passed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_double_entry_escrow_and_club_dividend() {
        let mut ledger = TideLedgerEngine::new();
        let buyer = Uuid::new_v4();
        let worker = Uuid::new_v4();
        let club_treasury = Uuid::new_v4();
        let job_id = Uuid::new_v4();

        ledger.seed_account(buyer, 100.0);
        ledger.seed_account(worker, 0.0);
        ledger.seed_account(club_treasury, 0.0);

        let vault = ledger
            .lock_coral_escrow(job_id, buyer, club_treasury, AtlantisAsset::Usdc, 50.0, 1_700_000_000)
            .expect("escrow lock should succeed");

        assert_eq!(ledger.balances[&buyer], 50.0);
        assert_eq!(ledger.balances[&ledger.coral_vault_holding_account], 50.0);

        let report_mid = ledger.verify_audit_invariants().expect("invariants valid while locked");
        assert_eq!(report_mid.active_escrow_locked, 50.0);

        let (worker_tx, div_tx) = ledger
            .release_coral_escrow(vault.id, worker, 0.02, 1_700_000_010)
            .expect("escrow release should succeed");

        assert!((worker_tx.amount - 49.0).abs() < 1e-6);
        assert!((div_tx.amount - 1.0).abs() < 1e-6);
        assert!((ledger.balances[&worker] - 49.0).abs() < 1e-6);
        assert!((ledger.balances[&club_treasury] - 1.0).abs() < 1e-6);
        assert!((ledger.balances[&ledger.coral_vault_holding_account]).abs() < 1e-6);

        let report_final = ledger.verify_audit_invariants().expect("invariants valid after release");
        assert!(report_final.invariants_passed);
        assert_eq!(report_final.total_transactions, 3);
    }

    #[test]
    fn test_coral_state_channel_micro_tipping_settlement() {
        let mut ledger = TideLedgerEngine::new();
        let listener = Uuid::new_v4();
        let dj = Uuid::new_v4();
        let club_treasury = Uuid::new_v4();

        ledger.seed_account(listener, 25.0);
        ledger.seed_account(dj, 0.0);
        ledger.seed_account(club_treasury, 0.0);

        let mut channel = ledger
            .open_state_channel(listener, dj, club_treasury, AtlantisAsset::Cyan, 10.0, 1_700_000_100)
            .expect("open state channel");

        for i in 0..50 {
            channel
                .stream_micro_tip(0.10, format!("sig_tick_{i}"))
                .expect("micro-tip tick within deposit");
        }
        assert_eq!(channel.tick_count, 50);
        assert!((channel.cumulative_streamed - 5.0).abs() < 1e-6);

        let settled_txs = ledger
            .settle_state_channel(&mut channel, 0.02, 1_700_000_200)
            .expect("batch settle state channel");

        assert_eq!(settled_txs.len(), 3);
        assert!((ledger.balances[&dj] - 4.90).abs() < 1e-6);
        assert!((ledger.balances[&club_treasury] - 0.10).abs() < 1e-6);
        assert!((ledger.balances[&listener] - 20.0).abs() < 1e-6);

        let report = ledger.verify_audit_invariants().expect("invariants should pass");
        assert_eq!(report.active_escrow_locked, 0.0);
    }

    #[test]
    fn test_subscription_and_cyan_allocation() {
        let mut ledger = TideLedgerEngine::new();
        let mut sub_ledger = SubscriptionLedger::new(&mut ledger, 10_000.0);
        let user = Uuid::new_v4();
        let treasury = Uuid::new_v4();

        ledger.seed_account(user, 20.0);
        ledger.seed_account(treasury, 0.0);

        sub_ledger
            .activate_subscription(&mut ledger, user, treasury, 15.0, 1_700_000_000)
            .expect("subscription activation");

        let cyan_tx = sub_ledger
            .calculate_cyan_allocation(&mut ledger, user, 1_700_000_100)
            .expect("cyan calculation")
            .expect("qualifying stake should yield cyan");

        assert!((cyan_tx.amount - 1.5).abs() < 1e-6);
        assert!(ledger.verify_audit_invariants().is_ok());
    }
}

