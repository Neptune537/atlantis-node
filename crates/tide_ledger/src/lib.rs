//! # Tide Ledger & Coral Settlement Engine
//!
//! Enforces strict double-entry accounting across all Atlantis operations:
//! - Every mutation produces a balanced `LedgerTransaction` (debit + credit).
//! - Marketplace jobs lock funds in `CoralEscrowVault` prior to execution.
//! - High-frequency Siren Frequency micro-tipping runs over `CoralStateChannel`.

use atlantis_protocol::AtlantisAsset;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use uuid::Uuid;

pub mod privacy_pool;
/// Opt-in integer, asset-isolated staging engine. Legacy databases are not auto-migrated.
pub mod v2;
#[cfg(not(target_arch = "wasm32"))]
pub mod durable;
pub use privacy_pool::{AtlantisPrivacyPool, PrivacyDepositNote, PrivacyPoolError, PrivacyPoolManager, ZkWithdrawProof};

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("Insufficient balance in account {0}: available {1}, required {2}")]
    InsufficientFunds(Uuid, f64, f64),
    #[error("Account not found: {0}")]
    AccountNotFound(Uuid),
    #[error("Escrow vault not found or already settled: {0}")]
    InvalidEscrowState(Uuid),
    #[error("Invalid transfer or deposit amount {0}: must be finite and positive")]
    InvalidAmount(f64),
    #[error("Audit invariant violated: debits and credits do not balance")]
    AuditInvariantViolation,
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerTxType {
    Stake,
    Subscription,
    CyanAllocation,
    EscrowLock,
    EscrowRelease,
    ClubDividend,
    EventTicketPurchase,
    EventSplitPayout,
    MiniAppDeveloperShare,
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

    /// Sets an opening balance for bootstrap/testing; this is not a journaled deposit.
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
        if !amount.is_finite() || amount <= 0.0 {
            return Err(LedgerError::InvalidAmount(amount));
        }
        if debit_account == credit_account {
            return Err(LedgerError::AuditInvariantViolation);
        }
        let sender_bal = *self
            .balances
            .get(&debit_account)
            .ok_or(LedgerError::AccountNotFound(debit_account))?;

        let recipient_bal = self.balances.get(&credit_account).copied().unwrap_or(0.0);
        if !sender_bal.is_finite() || sender_bal < 0.0
            || !recipient_bal.is_finite() || recipient_bal < 0.0
            || !(recipient_bal + amount).is_finite()
        {
            return Err(LedgerError::AuditInvariantViolation);
        }
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

    /// Persists the current in-memory ledger state into a SQLite database.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn persist_to_sqlite(&self, db_path: &str) -> Result<(), LedgerError> {
        let conn = rusqlite::Connection::open(db_path)
            .map_err(|e| LedgerError::DatabaseError(e.to_string()))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                balance REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS transactions (
                id TEXT PRIMARY KEY,
                tx_type TEXT NOT NULL,
                reference_id TEXT NOT NULL,
                debit_account TEXT NOT NULL,
                credit_account TEXT NOT NULL,
                asset TEXT NOT NULL,
                amount REAL NOT NULL,
                timestamp INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .map_err(|e| LedgerError::DatabaseError(e.to_string()))?;

        let tx = conn
            .unchecked_transaction()
            .map_err(|e| LedgerError::DatabaseError(e.to_string()))?;

        tx.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('coral_holding_account', ?1)",
            rusqlite::params![self.coral_vault_holding_account.to_string()],
        )
        .map_err(|e| LedgerError::DatabaseError(e.to_string()))?;

        for (account_id, balance) in &self.balances {
            tx.execute(
                "INSERT OR REPLACE INTO accounts (id, balance) VALUES (?1, ?2)",
                rusqlite::params![account_id.to_string(), balance],
            )
            .map_err(|e| LedgerError::DatabaseError(e.to_string()))?;
        }

        for entry in &self.transactions {
            tx.execute(
                "INSERT OR IGNORE INTO transactions (id, tx_type, reference_id, debit_account, credit_account, asset, amount, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    entry.id.to_string(),
                    format!("{:?}", entry.tx_type),
                    entry.reference_id.to_string(),
                    entry.debit_account.to_string(),
                    entry.credit_account.to_string(),
                    format!("{:?}", entry.asset),
                    entry.amount,
                    entry.timestamp as i64,
                ],
            )
            .map_err(|e| LedgerError::DatabaseError(e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| LedgerError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Loads the ledger state from an existing SQLite database.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_from_sqlite(db_path: &str) -> Result<Self, LedgerError> {
        let conn = rusqlite::Connection::open(db_path)
            .map_err(|e| LedgerError::DatabaseError(e.to_string()))?;

        let mut stmt = conn
            .prepare("SELECT key, value FROM metadata WHERE key = 'coral_holding_account'")
            .map_err(|e| LedgerError::DatabaseError(e.to_string()))?;
        let holding_id_str: Option<String> = stmt
            .query_row([], |row| row.get(1))
            .ok();

        let holding_id = holding_id_str
            .and_then(|s| Uuid::parse_str(&s).ok())
            .unwrap_or_else(Uuid::new_v4);

        let mut balances = HashMap::new();
        let mut stmt = conn
            .prepare("SELECT id, balance FROM accounts")
            .map_err(|e| LedgerError::DatabaseError(e.to_string()))?;
        let account_rows = stmt
            .query_map([], |row| {
                let id_str: String = row.get(0)?;
                let bal: f64 = row.get(1)?;
                Ok((id_str, bal))
            })
            .map_err(|e| LedgerError::DatabaseError(e.to_string()))?;

        for row in account_rows {
            if let Ok((id_str, bal)) = row {
                if let Ok(uid) = Uuid::parse_str(&id_str) {
                    balances.insert(uid, bal);
                }
            }
        }

        balances.entry(holding_id).or_insert(0.0);

        Ok(Self {
            balances,
            transactions: Vec::new(),
            escrows: HashMap::new(),
            coral_vault_holding_account: holding_id,
        })
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

    /// Refunds a Coral Escrow Vault in full back to the original buyer upon contract expiry or cancellation.
    pub fn refund_coral_escrow(
        &mut self,
        escrow_id: Uuid,
        timestamp: u64,
    ) -> Result<LedgerTransaction, LedgerError> {
        let vault = self
            .escrows
            .get(&escrow_id)
            .cloned()
            .ok_or(LedgerError::InvalidEscrowState(escrow_id))?;

        if vault.status != EscrowStatus::AuthorizedLocked {
            return Err(LedgerError::InvalidEscrowState(escrow_id));
        }

        let refund_tx = self.record_double_entry(
            LedgerTxType::EscrowRelease,
            vault.job_id,
            self.coral_vault_holding_account,
            vault.buyer_account,
            vault.asset,
            vault.locked_amount,
            timestamp,
        )?;

        if let Some(v) = self.escrows.get_mut(&escrow_id) {
            v.status = EscrowStatus::Refunded;
        }

        Ok(refund_tx)
    }

    /// Settles an asynchronous compute job per PROP-0043:
    /// - Disburses reward (e.g. 10.0 CYAN)
    /// - 80% to Worker Anchor
    /// - 15% to Guild Treasury
    /// - 5% permanently burned (retained in holding account as burned fee)
    pub fn settle_async_compute_escrow(
        &mut self,
        escrow_id: Uuid,
        worker_account: Uuid,
        guild_treasury_account: Uuid,
        timestamp: u64,
    ) -> Result<(LedgerTransaction, LedgerTransaction, f64), LedgerError> {
        let vault = self
            .escrows
            .get(&escrow_id)
            .cloned()
            .ok_or(LedgerError::InvalidEscrowState(escrow_id))?;

        if vault.status != EscrowStatus::AuthorizedLocked {
            return Err(LedgerError::InvalidEscrowState(escrow_id));
        }

        let worker_amount = vault.locked_amount * 0.80;
        let treasury_amount = vault.locked_amount * 0.15;
        let burned_amount = vault.locked_amount * 0.05;

        let worker_tx = self.record_double_entry(
            LedgerTxType::EscrowRelease,
            vault.job_id,
            self.coral_vault_holding_account,
            worker_account,
            vault.asset,
            worker_amount,
            timestamp,
        )?;

        let treasury_tx = self.record_double_entry(
            LedgerTxType::ClubDividend,
            vault.job_id,
            self.coral_vault_holding_account,
            guild_treasury_account,
            vault.asset,
            treasury_amount,
            timestamp,
        )?;

        if let Some(v) = self.escrows.get_mut(&escrow_id) {
            v.worker_account = Some(worker_account);
            v.status = EscrowStatus::CompletedReleased;
        }

        Ok((worker_tx, treasury_tx, burned_amount))
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
        let mut seen_transactions = HashSet::new();

        for tx in &self.transactions {
            if tx.amount <= 0.0 || !tx.amount.is_finite()
                || tx.debit_account == tx.credit_account
                || !seen_transactions.insert(tx.id)
                || !self.balances.contains_key(&tx.debit_account)
                || !self.balances.contains_key(&tx.credit_account)
            {
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
        if self.escrows.values().any(|v| !v.locked_amount.is_finite()
            || v.locked_amount < 0.0
            || (v.status == EscrowStatus::AuthorizedLocked && v.locked_amount == 0.0)) {
            return Err(LedgerError::AuditInvariantViolation);
        }

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

    /// Provisions a 12-month Store Node Annual Stake ($12.00 via Google Pay / USDC)
    /// and executes the initial Month 1 ($1.00) Network Pool Drip.
    pub fn provision_google_pay_annual_node_stake(
        &mut self,
        ledger: &mut TideLedgerEngine,
        node_account: Uuid,
        network_pool_treasury: Uuid,
        annual_stake_usd: f64,
        timestamp: u64,
    ) -> Result<(CoralEscrowVault, LedgerTransaction), LedgerError> {
        if annual_stake_usd + 1e-9 < 12.0 {
            return Err(LedgerError::InsufficientFunds(node_account, annual_stake_usd, 12.0));
        }

        // 1. Lock the $12.00 annual stake in Coral Escrow Vault
        let stake_job_id = Uuid::new_v4();
        let mut vault = ledger.lock_coral_escrow(
            stake_job_id,
            node_account,
            network_pool_treasury,
            AtlantisAsset::Usd,
            annual_stake_usd,
            timestamp,
        )?;

        // 2. Immediately drip Month 1 ($1.00) from the locked escrow into the Network Treasury Pool
        let monthly_drip = self.engine.monthly_subscription_fee_usd;
        let drip_tx = ledger.record_double_entry(
            LedgerTxType::Subscription,
            stake_job_id,
            ledger.coral_vault_holding_account,
            network_pool_treasury,
            AtlantisAsset::Usd,
            monthly_drip,
            timestamp,
        )?;

        // Update remaining locked escrow balance ($11.00 remaining for Months 2..12)
        if let Some(escrow) = ledger.escrows.get_mut(&vault.id) {
            escrow.locked_amount -= monthly_drip;
            vault = escrow.clone();
        }

        // Register active 365-day node subscription record
        let year_seconds = 365 * 24 * 3600;
        self.subscriptions.insert(
            node_account,
            SubscriptionRecord {
                user_account: node_account,
                staked_usd: annual_stake_usd,
                active_until_timestamp: timestamp + year_seconds,
                total_cyan_allocated: 0.0,
            },
        );

        Ok((vault, drip_tx))
    }

    /// Executes a subsequent monthly $1.00 drip from the node's locked annual stake into the Network Treasury Pool.
    pub fn drip_monthly_stake_to_network_pool(
        &mut self,
        ledger: &mut TideLedgerEngine,
        escrow_id: Uuid,
        timestamp: u64,
    ) -> Result<LedgerTransaction, LedgerError> {
        let vault = ledger
            .escrows
            .get(&escrow_id)
            .cloned()
            .ok_or(LedgerError::InvalidEscrowState(escrow_id))?;

        let monthly_drip = self.engine.monthly_subscription_fee_usd;
        if vault.status != EscrowStatus::AuthorizedLocked || vault.locked_amount + 1e-9 < monthly_drip {
            return Err(LedgerError::InvalidEscrowState(escrow_id));
        }

        let drip_tx = ledger.record_double_entry(
            LedgerTxType::Subscription,
            vault.job_id,
            ledger.coral_vault_holding_account,
            vault.club_treasury_account,
            vault.asset,
            monthly_drip,
            timestamp,
        )?;

        if let Some(escrow) = ledger.escrows.get_mut(&escrow_id) {
            escrow.locked_amount = (escrow.locked_amount - monthly_drip).max(0.0);
            if escrow.locked_amount < 1e-6 {
                escrow.status = EscrowStatus::CompletedReleased;
            }
        }

        Ok(drip_tx)
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

// ============================================================================
// EVENT TICKETING & MULTI-PARTY SPLIT PAYOUT ENGINE
// ============================================================================

/// Configuration for split payout distribution of an event's ticket revenues.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventPayoutSplitRule {
    pub recipient_account: Uuid,
    pub share_ratio: f64, // e.g. 0.60 (60% to host/venue), 0.30 (30% to talent), 0.10 (10% to dev)
    pub role_label: String,
}

impl TideLedgerEngine {
    /// Executes a ticket purchase with double-entry recording into the holding account.
    pub fn purchase_event_ticket(
        &mut self,
        buyer_account: Uuid,
        event_id: Uuid,
        ticket_price: f64,
        asset: AtlantisAsset,
        timestamp: u64,
    ) -> Result<LedgerTransaction, LedgerError> {
        self.record_double_entry(
            LedgerTxType::EventTicketPurchase,
            event_id,
            buyer_account,
            self.coral_vault_holding_account,
            asset,
            ticket_price,
            timestamp,
        )
    }

    /// Automatically distributes pooled ticket revenues among hosts, venues, performers, and developers.
    pub fn execute_event_split_payout(
        &mut self,
        event_id: Uuid,
        total_revenue: f64,
        asset: AtlantisAsset,
        split_rules: &[EventPayoutSplitRule],
        timestamp: u64,
    ) -> Result<Vec<LedgerTransaction>, LedgerError> {
        if split_rules.is_empty() {
            return Err(LedgerError::AuditInvariantViolation);
        }

        let total_ratio: f64 = split_rules.iter().map(|r| r.share_ratio).sum();
        if (total_ratio - 1.0).abs() > 0.001 {
            return Err(LedgerError::AuditInvariantViolation);
        }

        let mut txs = Vec::with_capacity(split_rules.len());
        let mut distributed_sum = 0.0;

        for (idx, rule) in split_rules.iter().enumerate() {
            let payout_amount = if idx == split_rules.len() - 1 {
                // Ensure remaining balance precisely matches to prevent rounding leaks
                (total_revenue - distributed_sum).max(0.0)
            } else {
                let amount = (total_revenue * rule.share_ratio * 100.0).round() / 100.0;
                distributed_sum += amount;
                amount
            };

            let tx_type = if rule.role_label.to_lowercase().contains("dev")
                || rule.role_label.to_lowercase().contains("app")
            {
                LedgerTxType::MiniAppDeveloperShare
            } else {
                LedgerTxType::EventSplitPayout
            };

            let tx = self.record_double_entry(
                tx_type,
                event_id,
                self.coral_vault_holding_account,
                rule.recipient_account,
                asset,
                payout_amount,
                timestamp,
            )?;
            txs.push(tx);
        }

        Ok(txs)
    }
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
    fn audit_rejects_corrupt_transaction_identity_and_escrow() {
        let mut ledger = TideLedgerEngine::new();
        let sender = Uuid::new_v4();
        let receiver = Uuid::new_v4();
        ledger.seed_account(sender, 10.0);
        ledger.seed_account(receiver, 0.0);
        let tx = ledger.record_double_entry(LedgerTxType::Stake, Uuid::new_v4(),
            sender, receiver, AtlantisAsset::Usdc, 2.0, 1).unwrap();
        assert!(ledger.verify_audit_invariants().is_ok());

        ledger.transactions.push(tx.clone());
        assert!(ledger.verify_audit_invariants().is_err(), "duplicate transaction ID");
        ledger.transactions.pop();
        ledger.transactions[0].credit_account = sender;
        assert!(ledger.verify_audit_invariants().is_err(), "self transfer");
        ledger.transactions[0].credit_account = receiver;
        ledger.transactions[0].amount = 0.0;
        assert!(ledger.verify_audit_invariants().is_err(), "zero amount");
        ledger.transactions[0].amount = tx.amount;
        ledger.balances.insert(receiver, f64::NAN);
        assert!(ledger.verify_audit_invariants().is_err(), "non-finite balance");
        ledger.balances.insert(receiver, 2.0);
        let vault = ledger.lock_coral_escrow(Uuid::new_v4(), sender, receiver,
            AtlantisAsset::Usdc, 1.0, 2).unwrap();
        assert!(ledger.verify_audit_invariants().is_ok());
        ledger.escrows.get_mut(&vault.id).unwrap().locked_amount = f64::NAN;
        assert!(ledger.verify_audit_invariants().is_err(), "non-finite escrow");
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

    #[test]
    fn test_event_ticketing_and_multi_party_split_payout() {
        let mut ledger = TideLedgerEngine::new();

        let attendee = Uuid::new_v4();
        let host_venue = Uuid::new_v4();
        let performer = Uuid::new_v4();
        let app_developer = Uuid::new_v4();
        let event_id = Uuid::new_v4();

        ledger.seed_account(attendee, 100.0);
        ledger.seed_account(host_venue, 0.0);
        ledger.seed_account(performer, 0.0);
        ledger.seed_account(app_developer, 0.0);

        // Attendee buys ticket for 100.0 USDC
        ledger
            .purchase_event_ticket(attendee, event_id, 100.0, AtlantisAsset::Usdc, 1_700_000_000)
            .expect("Ticket purchase failed");

        assert_eq!(*ledger.balances.get(&attendee).unwrap(), 0.0);
        assert_eq!(
            *ledger.balances.get(&ledger.coral_vault_holding_account).unwrap(),
            100.0
        );

        // Split payout rules: 60% Host/Venue, 30% Performer, 10% Mini-App Developer
        let rules = vec![
            EventPayoutSplitRule {
                recipient_account: host_venue,
                share_ratio: 0.60,
                role_label: "Host/Venue".to_string(),
            },
            EventPayoutSplitRule {
                recipient_account: performer,
                share_ratio: 0.30,
                role_label: "Performer".to_string(),
            },
            EventPayoutSplitRule {
                recipient_account: app_developer,
                share_ratio: 0.10,
                role_label: "Mini-App Developer".to_string(),
            },
        ];

        let txs = ledger
            .execute_event_split_payout(
                event_id,
                100.0,
                AtlantisAsset::Usdc,
                &rules,
                1_700_005_000,
            )
            .expect("Split payout execution failed");

        assert_eq!(txs.len(), 3);
        assert_eq!(*ledger.balances.get(&host_venue).unwrap(), 60.0);
        assert_eq!(*ledger.balances.get(&performer).unwrap(), 30.0);
        assert_eq!(*ledger.balances.get(&app_developer).unwrap(), 10.0);
        assert_eq!(
            *ledger.balances.get(&ledger.coral_vault_holding_account).unwrap(),
            0.0
        );

        assert!(ledger.verify_audit_invariants().is_ok());
    }

    #[test]
    fn test_google_pay_annual_node_stake_and_monthly_pool_drip() {
        let mut ledger = TideLedgerEngine::new();
        let mut sub_ledger = SubscriptionLedger::new(&mut ledger, 10_000.0);
        let store_node = Uuid::new_v4();
        let network_pool = Uuid::new_v4();

        // Store owner pays $12.00 via Google Pay for annual node stake
        ledger.seed_account(store_node, 12.0);
        ledger.seed_account(network_pool, 0.0);

        let (vault, month1_tx) = sub_ledger
            .provision_google_pay_annual_node_stake(&mut ledger, store_node, network_pool, 12.0, 1_700_000_000)
            .expect("Google Pay $12 stake provisioning should succeed");

        assert!((month1_tx.amount - 1.0).abs() < 1e-6);
        assert!((vault.locked_amount - 11.0).abs() < 1e-6);
        assert!((ledger.balances[&network_pool] - 1.0).abs() < 1e-6);
        assert!(ledger.verify_audit_invariants().is_ok());

        // Drip remaining 11 months ($1.00 each month)
        for m in 2..=12 {
            let tx = sub_ledger
                .drip_monthly_stake_to_network_pool(&mut ledger, vault.id, 1_700_000_000 + m * 2_592_000)
                .expect("Monthly drip should succeed");
            assert!((tx.amount - 1.0).abs() < 1e-6);
        }

        assert!((ledger.balances[&network_pool] - 12.0).abs() < 1e-6);
        assert!((ledger.balances[&ledger.coral_vault_holding_account]).abs() < 1e-6);
        assert!(ledger.verify_audit_invariants().is_ok());
    }
}

