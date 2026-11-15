//! Explicit offline migration from a frozen, reconciled legacy snapshot to a NEW file.
//! Usage: migrate_v2 SOURCE.sqlite REVIEWED_MANIFEST.json NEW.sqlite
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};
use tide_ledger::v2::{Asset, Command, Ledger, Request};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    source_sha256: String,
    reconciliation_reference: String,
    /// Decimal policy is recorded for the reviewer; all ledger inputs are base units.
    unit_policy_reference: String,
    accounts: Vec<Account>,
    escrows: Vec<Vault>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Account {
    account: Uuid,
    /// Exact original f64 bits as 16 hex digits, not a rounding conversion.
    legacy_balance_bits: String,
    /// Liquid holdings before the escrow locks below; not inferred from legacy f64.
    allocations: Vec<Allocation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Allocation {
    asset: Asset,
    units: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Vault {
    escrow: Uuid,
    owner: Uuid,
    asset: Asset,
    units: u64,
}

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: migrate_v2 SOURCE.sqlite REVIEWED_MANIFEST.json NEW.sqlite".into());
    }
    let source = Path::new(&args[1]);
    let output = Path::new(&args[3]);
    if output.exists() {
        return Err("output exists; refusing overwrite".into());
    }
    for suffix in ["-wal", "-journal"] {
        let sidecar = format!("{}{suffix}", source.display());
        if Path::new(&sidecar).exists() {
            return Err(
                "source must be a frozen standalone SQLite backup without WAL/journal".into(),
            );
        }
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    if manifest.reconciliation_reference.trim().is_empty()
        || manifest.unit_policy_reference.trim().is_empty()
    {
        return Err("reviewed reconciliation and units policy references are required".into());
    }
    let bytes = std::fs::read(source)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if hash != manifest.source_sha256 {
        return Err("source checksum mismatch".into());
    }
    let conn =
        rusqlite::Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Err("legacy SQLite integrity failure".into());
    }
    let mut statement = conn.prepare("SELECT id, balance FROM accounts ORDER BY id")?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?;
    let mut original = BTreeMap::new();
    for row in rows {
        let (id, amount) = row?;
        if !amount.is_finite() || amount < 0.0 {
            return Err("invalid legacy balance".into());
        }
        original.insert(Uuid::parse_str(&id)?, format!("{:016x}", amount.to_bits()));
    }
    let mut supplied = BTreeMap::new();
    let mut ledger = Ledger::default();
    for account in &manifest.accounts {
        if supplied
            .insert(account.account, account.legacy_balance_bits.clone())
            .is_some()
        {
            return Err("duplicate account".into());
        }
        let mut assets = std::collections::BTreeSet::new();
        for allocation in &account.allocations {
            if !assets.insert(allocation.asset) {
                return Err("duplicate asset allocation".into());
            }
            if allocation.units > 0 {
                ledger.execute(Request {
                    id: Uuid::new_v4(),
                    command: Command::Fund {
                        account: account.account,
                        asset: allocation.asset,
                        units: allocation.units,
                    },
                })?;
            }
        }
    }
    if supplied != original {
        return Err(
            "manifest must account for every original balance exactly, including zero accounts"
                .into(),
        );
    }
    // Owner allocations represent reconciled pre-lock funds, not old aggregate custody.
    for vault in manifest.escrows {
        if !supplied.contains_key(&vault.owner) {
            return Err("escrow owner missing from reconciled accounts".into());
        }
        ledger.execute(Request {
            id: Uuid::new_v4(),
            command: Command::Lock {
                escrow: vault.escrow,
                owner: vault.owner,
                asset: vault.asset,
                units: vault.units,
            },
        })?;
    }
    ledger.audit()?;
    if std::fs::read(source)? != bytes {
        return Err("source changed during migration".into());
    }
    // Reserve pathname atomically; an existing file is never replaced.
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)?;
    ledger.persist(output.to_str().ok_or("invalid output path")?)?;
    if Ledger::load(output.to_str().unwrap())? != ledger {
        return Err("post-write reconciliation failed".into());
    }
    println!(
        "{}",
        serde_json::json!({"schema": 2, "source_sha256": hash,
        "reconciliation_reference": manifest.reconciliation_reference,
        "unit_policy_reference": manifest.unit_policy_reference,
        "journal_batches": ledger.journal().len(), "legacy_history_imported": false,
        "source_modified": false, "beta_approved": false})
    );
    Ok(())
}
