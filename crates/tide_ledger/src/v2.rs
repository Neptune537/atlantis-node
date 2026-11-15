//! Integer base-unit staging ledger. No decimals, network authentication or implicit migration.
//! Caller supplies authenticated operation IDs; keep this engine behind an authorized host.
use atlantis_protocol::AtlantisAsset;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

pub const RESERVE: Uuid = Uuid::from_u128(0);
pub const BURN: Uuid = Uuid::from_u128(1);
pub const CUSTODY: Uuid = Uuid::from_u128(2);
const VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Asset {
    Usd,
    Usdc,
    Cyan,
    Pearl,
    Credit,
}
impl From<AtlantisAsset> for Asset {
    fn from(value: AtlantisAsset) -> Self {
        match value {
            AtlantisAsset::Usd => Self::Usd,
            AtlantisAsset::Usdc => Self::Usdc,
            AtlantisAsset::Cyan => Self::Cyan,
            AtlantisAsset::Pearl => Self::Pearl,
            AtlantisAsset::Credit => Self::Credit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    pub to: Uuid,
    pub units: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transfer {
    pub from: Uuid,
    pub to: Uuid,
    pub asset: Asset,
    pub units: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Authorized funding event, also used for explicitly reconciled opening balances.
    Fund {
        account: Uuid,
        asset: Asset,
        units: u64,
    },
    Batch {
        transfers: Vec<Transfer>,
    },
    Lock {
        escrow: Uuid,
        owner: Uuid,
        asset: Asset,
        units: u64,
    },
    Settle {
        escrow: Uuid,
        payments: Vec<Payment>,
        burn: u64,
    },
    Refund {
        escrow: Uuid,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: Uuid,
    pub command: Command,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batch {
    pub request: Request,
    pub entries: Vec<Transfer>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscrowState {
    Locked,
    Settled,
    Refunded,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Escrow {
    pub owner: Uuid,
    pub asset: Asset,
    pub units: u64,
    pub state: EscrowState,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid operation, amount, account or payout")]
    Invalid,
    #[error("insufficient asset balance")]
    Insufficient,
    #[error("integer overflow or account limit")]
    Overflow,
    #[error("request ID reused with different content")]
    ReplayConflict,
    #[error("escrow does not exist or is no longer locked")]
    EscrowState,
    #[error("journal reconciliation failed")]
    Audit,
    #[error("storage error: {0}")]
    Storage(String),
    #[error("legacy database requires an explicit reconciled migration")]
    LegacyDatabase,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    balances: BTreeMap<(Asset, Uuid), i128>,
    escrows: BTreeMap<Uuid, Escrow>,
    journal: Vec<Batch>,
}

fn user(id: Uuid) -> bool {
    ![RESERVE, BURN, CUSTODY].contains(&id)
}
impl Ledger {
    pub fn balance(&self, asset: Asset, account: Uuid) -> i128 {
        self.balances.get(&(asset, account)).copied().unwrap_or(0)
    }
    pub fn journal(&self) -> &[Batch] {
        &self.journal
    }
    pub fn escrow(&self, id: Uuid) -> Option<&Escrow> {
        self.escrows.get(&id)
    }

    /// All-or-nothing, including journal, escrow state, and replay identity.
    /// Identical retries return the original batch without a second mutation.
    pub fn execute(&mut self, request: Request) -> Result<Batch, Error> {
        if request.id.is_nil() {
            return Err(Error::Invalid);
        }
        if let Some(old) = self.journal.iter().find(|b| b.request.id == request.id) {
            return if old.request == request {
                Ok(old.clone())
            } else {
                Err(Error::ReplayConflict)
            };
        }
        let mut next = self.clone();
        let entries = next.apply(&request.command)?;
        let batch = Batch { request, entries };
        next.journal.push(batch.clone());
        *self = next;
        Ok(batch)
    }

    fn move_units(&mut self, entry: &Transfer, funding: bool) -> Result<(), Error> {
        if entry.units == 0
            || entry.from == entry.to
            || entry.to == RESERVE
            || entry.from == BURN
            || (entry.from == RESERVE && !funding)
        {
            return Err(Error::Invalid);
        }
        let units = i128::from(entry.units);
        let debit = self
            .balance(entry.asset, entry.from)
            .checked_sub(units)
            .ok_or(Error::Overflow)?;
        let credit = self
            .balance(entry.asset, entry.to)
            .checked_add(units)
            .ok_or(Error::Overflow)?;
        if entry.from != RESERVE && debit < 0 {
            return Err(Error::Insufficient);
        }
        if credit > i128::from(u64::MAX) {
            return Err(Error::Overflow);
        }
        self.balances.insert((entry.asset, entry.from), debit);
        self.balances.insert((entry.asset, entry.to), credit);
        Ok(())
    }

    fn apply(&mut self, command: &Command) -> Result<Vec<Transfer>, Error> {
        let (entries, funding) = match command {
            Command::Fund {
                account,
                asset,
                units,
            } => {
                if !user(*account) {
                    return Err(Error::Invalid);
                }
                (
                    vec![Transfer {
                        from: RESERVE,
                        to: *account,
                        asset: *asset,
                        units: *units,
                    }],
                    true,
                )
            }
            Command::Batch { transfers } => {
                if transfers.is_empty()
                    || transfers.len() > 256
                    || transfers.iter().any(|t| !user(t.from) || !user(t.to))
                {
                    return Err(Error::Invalid);
                }
                (transfers.clone(), false)
            }
            Command::Lock {
                escrow,
                owner,
                asset,
                units,
            } => {
                if escrow.is_nil() || !user(*owner) || self.escrows.contains_key(escrow) {
                    return Err(Error::Invalid);
                }
                self.escrows.insert(
                    *escrow,
                    Escrow {
                        owner: *owner,
                        asset: *asset,
                        units: *units,
                        state: EscrowState::Locked,
                    },
                );
                (
                    vec![Transfer {
                        from: *owner,
                        to: CUSTODY,
                        asset: *asset,
                        units: *units,
                    }],
                    false,
                )
            }
            Command::Settle {
                escrow,
                payments,
                burn,
            } => {
                let vault = self.escrows.get(escrow).ok_or(Error::EscrowState)?.clone();
                if vault.state != EscrowState::Locked {
                    return Err(Error::EscrowState);
                }
                if payments.len() > 256 || payments.iter().any(|p| !user(p.to) || p.units == 0) {
                    return Err(Error::Invalid);
                }
                let sum = payments.iter().try_fold(*burn, |sum, p| {
                    sum.checked_add(p.units).ok_or(Error::Overflow)
                })?;
                if sum != vault.units {
                    return Err(Error::Invalid);
                }
                let mut entries: Vec<_> = payments
                    .iter()
                    .map(|p| Transfer {
                        from: CUSTODY,
                        to: p.to,
                        asset: vault.asset,
                        units: p.units,
                    })
                    .collect();
                if *burn > 0 {
                    entries.push(Transfer {
                        from: CUSTODY,
                        to: BURN,
                        asset: vault.asset,
                        units: *burn,
                    });
                }
                self.escrows.get_mut(escrow).unwrap().state = EscrowState::Settled;
                (entries, false)
            }
            Command::Refund { escrow } => {
                let vault = self.escrows.get(escrow).ok_or(Error::EscrowState)?.clone();
                if vault.state != EscrowState::Locked {
                    return Err(Error::EscrowState);
                }
                self.escrows.get_mut(escrow).unwrap().state = EscrowState::Refunded;
                (
                    vec![Transfer {
                        from: CUSTODY,
                        to: vault.owner,
                        asset: vault.asset,
                        units: vault.units,
                    }],
                    false,
                )
            }
        };
        for entry in &entries {
            self.move_units(entry, funding)?;
        }
        Ok(entries)
    }

    /// Independently sum journal postings, then replay commands to validate posting
    /// authorization and escrow state. No tolerances and no shared asset balances.
    pub fn audit(&self) -> Result<(), Error> {
        let mut balances: BTreeMap<(Asset, Uuid), i128> = BTreeMap::new();
        let mut ids = HashSet::new();
        let mut replay = Self::default();
        for batch in &self.journal {
            if !ids.insert(batch.request.id) {
                return Err(Error::Audit);
            }
            for entry in &batch.entries {
                if entry.units == 0 || entry.from == entry.to {
                    return Err(Error::Audit);
                }
                let debit = balances.entry((entry.asset, entry.from)).or_default();
                *debit = debit
                    .checked_sub(i128::from(entry.units))
                    .ok_or(Error::Audit)?;
                let credit = balances.entry((entry.asset, entry.to)).or_default();
                *credit = credit
                    .checked_add(i128::from(entry.units))
                    .ok_or(Error::Audit)?;
            }
            let expected = replay
                .execute(batch.request.clone())
                .map_err(|_| Error::Audit)?;
            if expected != *batch {
                return Err(Error::Audit);
            }
        }
        if balances != self.balances || replay.escrows != self.escrows {
            return Err(Error::Audit);
        }
        for asset in [
            Asset::Usd,
            Asset::Usdc,
            Asset::Cyan,
            Asset::Pearl,
            Asset::Credit,
        ] {
            let liabilities = self
                .escrows
                .values()
                .filter(|v| v.asset == asset && v.state == EscrowState::Locked)
                .try_fold(0_i128, |sum, v| {
                    sum.checked_add(i128::from(v.units)).ok_or(Error::Audit)
                })?;
            if liabilities != self.balance(asset, CUSTODY) {
                return Err(Error::Audit);
            }
            let total = self
                .balances
                .iter()
                .filter(|((a, _), _)| *a == asset)
                .try_fold(0_i128, |sum, (_, v)| {
                    sum.checked_add(*v).ok_or(Error::Audit)
                })?;
            if total != 0 {
                return Err(Error::Audit);
            }
        }
        Ok(())
    }

    /// Rebuild from an immutable journal, including idempotency and escrow status.
    pub fn from_journal(journal: Vec<Batch>) -> Result<Self, Error> {
        let mut ledger = Self::default();
        for batch in journal {
            if ledger
                .journal
                .iter()
                .any(|old| old.request.id == batch.request.id)
            {
                return Err(Error::Audit);
            }
            if ledger.execute(batch.request.clone())? != batch {
                return Err(Error::Audit);
            }
        }
        ledger.audit()?;
        Ok(ledger)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn persist(&self, path: &str) -> Result<(), Error> {
        self.audit()?;
        let mut conn = rusqlite::Connection::open(path).map_err(storage)?;
        refuse_legacy(&conn)?;
        let tx = conn.transaction().map_err(storage)?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS tide_v2 (id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL, journal TEXT NOT NULL);").map_err(storage)?;
        // Compare persisted history to prevent stale writers from dropping committed batches.
        let old: Option<(u32, String)> = tx
            .query_row("SELECT version, journal FROM tide_v2 WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
            .map_err(storage)?;
        if let Some((version, json)) = old {
            if version != VERSION {
                return Err(Error::Storage("unknown schema version".into()));
            }
            let old: Vec<Batch> = serde_json::from_str(&json).map_err(storage)?;
            Self::from_journal(old.clone())?;
            if !self.journal.starts_with(&old) {
                return Err(Error::ReplayConflict);
            }
        }
        let json = serde_json::to_string(&self.journal).map_err(storage)?;
        tx.execute("INSERT INTO tide_v2 VALUES (1, ?1, ?2) ON CONFLICT(id) DO UPDATE SET version=excluded.version, journal=excluded.journal", rusqlite::params![VERSION, json]).map_err(storage)?;
        tx.commit().map_err(storage)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(path: &str) -> Result<Self, Error> {
        let conn =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(storage)?;
        refuse_legacy(&conn)?;
        let (version, json): (u32, String) = conn
            .query_row("SELECT version, journal FROM tide_v2 WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(storage)?;
        if version != VERSION {
            return Err(Error::Storage("unknown schema version".into()));
        }
        Self::from_journal(serde_json::from_str(&json).map_err(storage)?)
    }
}

/// Split in integer basis points. Remainder goes to final recipient; no dust creation.
pub fn split(units: u64, recipients: &[(Uuid, u16)]) -> Result<Vec<Payment>, Error> {
    if units == 0
        || recipients.is_empty()
        || recipients.len() > 256
        || recipients
            .iter()
            .map(|(_, bps)| u64::from(*bps))
            .sum::<u64>()
            != 10_000
        || recipients.iter().any(|(id, bps)| !user(*id) || *bps == 0)
    {
        return Err(Error::Invalid);
    }
    let mut remaining = units;
    let mut payments = Vec::new();
    for (index, (to, bps)) in recipients.iter().enumerate() {
        let amount = if index + 1 == recipients.len() {
            remaining
        } else {
            (u128::from(units) * u128::from(*bps) / 10_000) as u64
        };
        remaining = remaining.checked_sub(amount).ok_or(Error::Overflow)?;
        if amount > 0 {
            payments.push(Payment {
                to: *to,
                units: amount,
            });
        }
    }
    Ok(payments)
}

#[cfg(not(target_arch = "wasm32"))]
use rusqlite::OptionalExtension;
#[cfg(not(target_arch = "wasm32"))]
fn storage(err: impl std::fmt::Display) -> Error {
    Error::Storage(err.to_string())
}
#[cfg(not(target_arch = "wasm32"))]
fn refuse_legacy(conn: &rusqlite::Connection) -> Result<(), Error> {
    let found: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name IN ('accounts', 'transactions', 'metadata'))", [], |r| r.get(0)).map_err(storage)?;
    if found {
        Err(Error::LegacyDatabase)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(command: Command) -> Request {
        Request {
            id: Uuid::new_v4(),
            command,
        }
    }
    #[test]
    fn tampering_is_detected() {
        let mut ledger = Ledger::default();
        let account = Uuid::new_v4();
        ledger
            .execute(request(Command::Fund {
                account,
                asset: Asset::Cyan,
                units: 100,
            }))
            .unwrap();
        ledger.balances.insert((Asset::Cyan, account), 101);
        assert_eq!(ledger.audit(), Err(Error::Audit));
    }
}
