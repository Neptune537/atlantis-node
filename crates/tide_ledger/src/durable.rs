//! Native append-only settlement store. A request, postings, escrow transition and
//! proof identity share one SQLite transaction. No in-memory state is authoritative.
//! Hosts must authenticate economic permission and cryptographic proof separately.
use crate::v2::{Batch, Ledger, Request};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::OpenOptions,
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use uuid::Uuid;

const SCHEMA: i64 = 3;
const MAX_EVIDENCE_BYTES: usize = 16 * 1024;
const MAX_BATCH_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Ledger(#[from] crate::v2::Error),
    #[error("database is busy; request was not committed")]
    Busy,
    #[error("temporal guard rejected commit; transaction rolled back")]
    GuardRejected,
    #[error("proof identity already belongs to another request")]
    ProofReplay,
    #[error("stored proof history or incoming chain is not trusted")]
    EvidenceRejected,
    #[error("durable journal or schema is invalid")]
    Corrupt,
    #[error("legacy/snapshot storage requires an explicit migration")]
    MigrationRequired,
    #[error("storage error: {0}; recover by request ID before retrying an uncertain commit")]
    Storage(String),
    #[error("discard this connection and reopen the store before reconciling or retrying")]
    RecoveryRequired,
}

/// Opaque proof bytes are interpreted and authenticated by the trusted host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub tile: [u8; 32],
    pub proof_id: Uuid,
    pub evidence: Vec<u8>,
}
impl Context {
    fn validate(&self) -> Result<(), Error> {
        if self.proof_id.is_nil()
            || self.evidence.is_empty()
            || self.evidence.len() > MAX_EVIDENCE_BYTES
        {
            Err(Error::Corrupt)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub revision: u64,
    pub batch: Batch,
    pub context: Context,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub record: Record,
    pub replayed: bool,
}

pub struct DurableLedger {
    connection: Connection,
    usable: bool,
}

impl DurableLedger {
    /// New files only. Never upgrades or overwrites a legacy/snapshot database.
    pub fn create(path: impl AsRef<Path>) -> Result<Self, Error> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path.as_ref())
            .map_err(storage)?;
        let mut connection = connect(path.as_ref())?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        tx.execute_batch("CREATE TABLE tide_durable_meta (id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL);
            INSERT INTO tide_durable_meta VALUES (1,3);
            CREATE TABLE tide_durable_batches (
                revision INTEGER PRIMARY KEY CHECK(revision>0),
                request_id TEXT NOT NULL UNIQUE,
                proof_id TEXT NOT NULL UNIQUE,
                batch_json TEXT NOT NULL,
                context_json TEXT NOT NULL
            );
            CREATE TRIGGER tide_durable_no_update BEFORE UPDATE ON tide_durable_batches BEGIN SELECT RAISE(ABORT, 'immutable journal'); END;
            CREATE TRIGGER tide_durable_no_delete BEFORE DELETE ON tide_durable_batches BEGIN SELECT RAISE(ABORT, 'immutable journal'); END;")
            .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(Self {
            connection,
            usable: true,
        })
    }

    /// Existing durable schema only. Recovery reconstructs and audits all state.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        // Validate before changing connection settings on an unknown database.
        let connection =
            Connection::open_with_flags(path.as_ref(), OpenFlags::SQLITE_OPEN_READ_WRITE)
                .map_err(sql)?;
        validate_schema(&connection)?;
        configure(&connection)?;
        recover(&connection)?;
        Ok(Self {
            connection,
            usable: true,
        })
    }

    pub fn snapshot(&self) -> Result<Ledger, Error> {
        self.check_ready()?;
        Ok(recover(&self.connection)?.0)
    }
    pub fn records(&self) -> Result<Vec<Record>, Error> {
        self.check_ready()?;
        Ok(recover(&self.connection)?.1)
    }
    /// Reconcile a lost acknowledgement from committed, audited state. Callers
    /// must still authorize access to the requested economic record.
    pub fn lookup_request(&self, id: Uuid) -> Result<Option<Record>, Error> {
        Ok(self
            .records()?
            .into_iter()
            .find(|record| record.batch.request.id == id))
    }

    /// Re-read under BEGIN IMMEDIATE; never commit from a stale process-local cache.
    /// `guard` runs inside SQLite's commit hook, after SQL writes but before commit
    /// becomes irreversible. Return true to permit, false to roll back. Panics veto.
    /// FULL disk synchronization follows the hook and can exceed a real-time budget.
    /// An identical retry returns its original receipt without invoking the guard.
    pub fn execute_guarded<F>(
        &mut self,
        request: Request,
        context: Context,
        guard: F,
    ) -> Result<Receipt, Error>
    where
        F: FnMut() -> bool + Send + 'static,
    {
        self.execute_validated(request, context, |_| Ok(()), guard)
    }

    /// A trusted host can validate cryptographic history under the same write lock,
    /// before replay lookup or a new insert. This closes history-check/write races.
    pub fn execute_validated<H, F>(
        &mut self,
        request: Request,
        context: Context,
        validate: H,
        guard: F,
    ) -> Result<Receipt, Error>
    where
        H: FnOnce(&[Record]) -> Result<(), Error>,
        F: FnMut() -> bool + Send + 'static,
    {
        self.check_ready()?;
        let result = self.execute_inner(request, context, validate, guard);
        // Reconcile uncertain storage outcomes through a fresh connection, never
        // through a possibly uncommitted local read on this connection.
        if matches!(&result, Err(Error::Storage(_))) || !self.connection.is_autocommit() {
            self.usable = false;
        }
        if result.is_ok() && !self.usable {
            return Err(Error::RecoveryRequired);
        }
        result
    }

    fn check_ready(&self) -> Result<(), Error> {
        if self.usable && self.connection.is_autocommit() {
            Ok(())
        } else {
            Err(Error::RecoveryRequired)
        }
    }

    fn execute_inner<H, F>(
        &mut self,
        request: Request,
        context: Context,
        validate: H,
        mut guard: F,
    ) -> Result<Receipt, Error>
    where
        H: FnOnce(&[Record]) -> Result<(), Error>,
        F: FnMut() -> bool + Send + 'static,
    {
        context.validate()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let (mut ledger, records) = recover(&tx)?;
        validate(&records)?;
        if let Some(old) = records.iter().find(|r| r.batch.request.id == request.id) {
            if old.batch.request != request || old.context.tile != context.tile {
                return Err(crate::v2::Error::ReplayConflict.into());
            }
            return Ok(Receipt {
                record: old.clone(),
                replayed: true,
            });
        }
        if records
            .iter()
            .any(|r| r.context.proof_id == context.proof_id)
        {
            return Err(Error::ProofReplay);
        }
        let batch = ledger.execute(request)?;
        ledger.audit()?;
        let revision = i64::try_from(records.len())
            .map_err(|_| Error::Corrupt)?
            .checked_add(1)
            .ok_or(Error::Corrupt)?;
        let batch_json = serde_json::to_string(&batch).map_err(storage)?;
        if batch_json.len() > MAX_BATCH_BYTES {
            return Err(Error::Corrupt);
        }
        let context_json = serde_json::to_string(&context).map_err(storage)?;
        tx.execute(
            "INSERT INTO tide_durable_batches VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![
                revision,
                batch.request.id.to_string(),
                context.proof_id.to_string(),
                batch_json,
                context_json
            ],
        )
        .map_err(sql)?;
        let rejected = Arc::new(AtomicBool::new(false));
        let hook_rejected = rejected.clone();
        tx.commit_hook(Some(move || {
            let reject = !catch_unwind(AssertUnwindSafe(&mut guard)).unwrap_or(false);
            hook_rejected.store(reject, Ordering::SeqCst);
            reject
        }));
        let result = tx.commit();
        self.connection.commit_hook(None::<fn() -> bool>);
        if rejected.load(Ordering::SeqCst) {
            return Err(Error::GuardRejected);
        }
        result.map_err(sql)?;
        Ok(Receipt {
            record: Record {
                revision: revision as u64,
                batch,
                context,
            },
            replayed: false,
        })
    }
}

fn connect(path: &Path) -> Result<Connection, Error> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(sql)?;
    configure(&conn)?;
    Ok(conn)
}
fn configure(conn: &Connection) -> Result<(), Error> {
    // Fail immediately on writer contention; never sleep through a temporal budget.
    conn.busy_timeout(Duration::ZERO).map_err(sql)?;
    let mode: String = conn
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .map_err(sql)?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(Error::Corrupt);
    }
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(sql)?;
    let sync: i64 = conn
        .query_row("PRAGMA synchronous", [], |r| r.get(0))
        .map_err(sql)?;
    if sync != 2 {
        return Err(Error::Corrupt);
    }
    Ok(())
}
fn validate_schema(conn: &Connection) -> Result<(), Error> {
    let legacy: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name IN ('accounts','transactions','metadata','tide_v2'))", [], |r| r.get(0)).map_err(sql)?;
    if legacy {
        return Err(Error::MigrationRequired);
    }
    let version: i64 = conn
        .query_row(
            "SELECT schema_version FROM tide_durable_meta WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if version != SCHEMA {
        return Err(Error::Corrupt);
    }
    Ok(())
}
fn recover(conn: &Connection) -> Result<(Ledger, Vec<Record>), Error> {
    validate_schema(conn)?;
    let mut statement = conn.prepare("SELECT revision,request_id,proof_id,batch_json,context_json FROM tide_durable_batches ORDER BY revision").map_err(sql)?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(sql)?;
    let mut records = Vec::new();
    let mut journal = Vec::new();
    let mut requests = HashSet::new();
    let mut proofs = HashSet::new();
    for row in rows {
        let (revision, request_id, proof_id, batch_json, context_json) = row.map_err(sql)?;
        if revision != i64::try_from(records.len()).map_err(|_| Error::Corrupt)? + 1
            || batch_json.len() > MAX_BATCH_BYTES
            || context_json.len() > MAX_EVIDENCE_BYTES * 5 + 1024
        {
            return Err(Error::Corrupt);
        }
        let batch: Batch = serde_json::from_str(&batch_json).map_err(storage)?;
        let context: Context = serde_json::from_str(&context_json).map_err(storage)?;
        context.validate()?;
        if request_id != batch.request.id.to_string()
            || proof_id != context.proof_id.to_string()
            || !requests.insert(batch.request.id)
            || !proofs.insert(context.proof_id)
        {
            return Err(Error::Corrupt);
        }
        journal.push(batch.clone());
        records.push(Record {
            revision: revision as u64,
            batch,
            context,
        });
    }
    Ok((Ledger::from_journal(journal)?, records))
}
fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}
fn sql(error: rusqlite::Error) -> Error {
    match error {
        rusqlite::Error::SqliteFailure(ref failure, _)
            if matches!(
                failure.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            Error::Busy
        }
        _ => storage(error),
    }
}
