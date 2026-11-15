//! Synthetic native persistence, contention and forced-process-termination checks.
#![cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{mpsc, Arc, Barrier},
    thread,
    time::Duration,
};
use tide_ledger::{
    durable::{Context, DurableLedger, Error},
    v2::{Asset, Command as Op, EscrowState, Ledger, Payment, Request, Transfer, BURN, CUSTODY},
};
use uuid::Uuid;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("tide-durable-{}.sqlite", Uuid::new_v4()));
        Self(path)
    }
    fn create(&self) -> DurableLedger {
        DurableLedger::create(&self.0).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let _ = fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}
fn request(command: Op) -> Request {
    Request {
        id: Uuid::new_v4(),
        command,
    }
}
fn context() -> Context {
    Context {
        tile: [9; 32],
        proof_id: Uuid::new_v4(),
        evidence: b"synthetic trusted-host fixture".to_vec(),
    }
}
fn fund(account: Uuid, units: u64) -> Request {
    request(Op::Fund {
        account,
        asset: Asset::Cyan,
        units,
    })
}

#[test]
fn restart_recovers_balances_escrow_burn_and_idempotency() {
    let file = Fixture::new();
    let mut store = file.create();
    let owner = Uuid::new_v4();
    let worker = Uuid::new_v4();
    let escrow = Uuid::new_v4();
    store
        .execute_guarded(fund(owner, 100), context(), || true)
        .unwrap();
    store
        .execute_guarded(
            request(Op::Lock {
                escrow,
                owner,
                asset: Asset::Cyan,
                units: 100,
            }),
            context(),
            || true,
        )
        .unwrap();
    drop(store);
    let mut store = DurableLedger::open(&file.0).unwrap();
    assert_eq!(
        store.snapshot().unwrap().escrow(escrow).unwrap().state,
        EscrowState::Locked
    );
    let settle = request(Op::Settle {
        escrow,
        payments: vec![Payment {
            to: worker,
            units: 95,
        }],
        burn: 5,
    });
    let original = store
        .execute_guarded(settle.clone(), context(), || true)
        .unwrap();
    assert!(!original.replayed);
    drop(store);
    let mut store = DurableLedger::open(&file.0).unwrap();
    let recovered = store.snapshot().unwrap();
    recovered.audit().unwrap();
    assert_eq!(recovered.balance(Asset::Cyan, worker), 95);
    assert_eq!(recovered.balance(Asset::Cyan, BURN), 5);
    assert_eq!(recovered.balance(Asset::Cyan, CUSTODY), 0);
    assert_eq!(
        recovered.escrow(escrow).unwrap().state,
        EscrowState::Settled
    );
    let retry = store
        .execute_guarded(settle.clone(), context(), || {
            panic!("retry must not run commit guard")
        })
        .unwrap();
    assert!(retry.replayed);
    assert_eq!(retry.record, original.record);
    let conflict = Request {
        id: settle.id,
        command: Op::Refund { escrow },
    };
    assert!(matches!(
        store.execute_guarded(conflict, context(), || true),
        Err(Error::Ledger(tide_ledger::v2::Error::ReplayConflict))
    ));
    assert_eq!(store.snapshot().unwrap(), recovered);
    assert_eq!(store.records().unwrap().len(), 3);
}

#[test]
fn guard_veto_and_panic_roll_back_journal_and_proof_identity() {
    let file = Fixture::new();
    let mut store = file.create();
    let operation = fund(Uuid::new_v4(), 100);
    let proof = context();
    assert!(matches!(
        store.execute_guarded(operation.clone(), proof.clone(), || false),
        Err(Error::GuardRejected)
    ));
    assert!(store.records().unwrap().is_empty());
    assert!(matches!(
        store.execute_guarded(operation.clone(), proof.clone(), || panic!(
            "synthetic guard failure"
        )),
        Err(Error::GuardRejected)
    ));
    assert!(store.records().unwrap().is_empty());
    store
        .execute_guarded(operation, proof.clone(), || true)
        .unwrap();
    assert!(matches!(
        store.execute_guarded(fund(Uuid::new_v4(), 1), proof, || true),
        Err(Error::ProofReplay)
    ));
    assert_eq!(store.records().unwrap().len(), 1);
}

#[test]
fn failed_batch_and_wrong_tile_retry_preserve_everything() {
    let file = Fixture::new();
    let mut store = file.create();
    let owner = Uuid::new_v4();
    let other = Uuid::new_v4();
    let seed = fund(owner, 100);
    store
        .execute_guarded(seed.clone(), context(), || true)
        .unwrap();
    let before = store.records().unwrap();
    let failed = request(Op::Batch {
        transfers: vec![
            Transfer {
                from: owner,
                to: other,
                asset: Asset::Cyan,
                units: 50,
            },
            Transfer {
                from: owner,
                to: other,
                asset: Asset::Cyan,
                units: 100,
            },
        ],
    });
    assert!(matches!(
        store.execute_guarded(failed, context(), || true),
        Err(Error::Ledger(_))
    ));
    let mut wrong = context();
    wrong.tile = [8; 32];
    assert!(matches!(
        store.execute_guarded(seed, wrong, || true),
        Err(Error::Ledger(tide_ledger::v2::Error::ReplayConflict))
    ));
    assert_eq!(store.records().unwrap(), before);
}

#[test]
fn independent_writers_reload_committed_state_and_retries_converge() {
    let file = Fixture::new();
    let mut first = file.create();
    let mut second = DurableLedger::open(&file.0).unwrap();
    let owner = Uuid::new_v4();
    let recipient = Uuid::new_v4();
    first
        .execute_guarded(fund(owner, 100), context(), || true)
        .unwrap();
    second
        .execute_guarded(
            request(Op::Batch {
                transfers: vec![Transfer {
                    from: owner,
                    to: recipient,
                    asset: Asset::Cyan,
                    units: 40,
                }],
            }),
            context(),
            || true,
        )
        .unwrap();
    assert_eq!(first.snapshot().unwrap().balance(Asset::Cyan, owner), 60);
    let barrier = Arc::new(Barrier::new(2));
    let operation = fund(owner, 1);
    let mut workers = Vec::new();
    for _ in 0..2 {
        let path = file.0.clone();
        let barrier = barrier.clone();
        let operation = operation.clone();
        workers.push(thread::spawn(move || {
            let mut store = DurableLedger::open(path).unwrap();
            barrier.wait();
            store.execute_guarded(operation, context(), || true)
        }));
    }
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(
        results
            .iter()
            .filter(|r| r.as_ref().is_ok_and(|r| !r.replayed))
            .count(),
        1
    );
    assert!(results
        .iter()
        .all(|r| r.is_ok() || matches!(r, Err(Error::Busy))));
    assert!(
        first
            .execute_guarded(operation, context(), || true)
            .unwrap()
            .replayed
    );
    assert_eq!(first.snapshot().unwrap().balance(Asset::Cyan, owner), 61);
    assert_eq!(first.records().unwrap().len(), 3);
}

#[test]
fn legacy_files_are_never_silently_upgraded() {
    let file = Fixture::new();
    Ledger::default().persist(file.0.to_str().unwrap()).unwrap();
    let before = fs::read(&file.0).unwrap();
    assert!(matches!(
        DurableLedger::open(&file.0),
        Err(Error::MigrationRequired)
    ));
    assert!(DurableLedger::create(&file.0).is_err());
    assert_eq!(fs::read(&file.0).unwrap(), before);
}

#[test]
fn writer_lock_and_sql_failure_leave_no_partial_operation() {
    let file = Fixture::new();
    let mut store = file.create();
    let lock = rusqlite::Connection::open(&file.0).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let guard_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = guard_calls.clone();
    assert!(matches!(
        store.execute_guarded(fund(Uuid::new_v4(), 100), context(), move || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            true
        }),
        Err(Error::Busy)
    ));
    assert_eq!(guard_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    lock.execute_batch("ROLLBACK; CREATE TRIGGER synthetic_failure BEFORE INSERT ON tide_durable_batches BEGIN SELECT RAISE(ABORT,'synthetic storage failure'); END;").unwrap();
    assert!(matches!(
        store.execute_guarded(fund(Uuid::new_v4(), 100), context(), || true),
        Err(Error::Storage(_))
    ));
    assert!(matches!(store.records(), Err(Error::RecoveryRequired)));
    drop(store);
    let mut store = DurableLedger::open(&file.0).unwrap();
    assert!(store.records().unwrap().is_empty());
    lock.execute_batch("DROP TRIGGER synthetic_failure;")
        .unwrap();
    drop(lock);
    store
        .execute_guarded(fund(Uuid::new_v4(), 100), context(), || true)
        .unwrap();
    assert_eq!(store.records().unwrap().len(), 1);
}

// Only the parent test below sets this child's environment. No production fault hooks.
#[test]
fn crash_child() {
    if std::env::var("TIDE_SYNTHETIC_CRASH_CHILD").ok().as_deref() != Some("1") {
        return;
    }
    let mut store = DurableLedger::open(std::env::var_os("TIDE_SYNTHETIC_DB").unwrap()).unwrap();
    let operation: Request =
        serde_json::from_str(&std::env::var("TIDE_SYNTHETIC_REQUEST").unwrap()).unwrap();
    let context: Context =
        serde_json::from_str(&std::env::var("TIDE_SYNTHETIC_CONTEXT").unwrap()).unwrap();
    let wait = || -> bool {
        println!("DURABLE_CRASH_BOUNDARY");
        std::io::stdout().flush().unwrap();
        loop {
            thread::park();
        }
    };
    if std::env::var("TIDE_SYNTHETIC_PHASE").unwrap() == "before_commit" {
        store.execute_guarded(operation, context, wait).unwrap();
    } else {
        store.execute_guarded(operation, context, || true).unwrap();
        wait();
    }
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn forced_exit_before_commit_rolls_back_and_after_commit_preserves_lost_ack() {
    for phase in ["before_commit", "after_commit"] {
        let file = Fixture::new();
        drop(file.create());
        let operation = fund(Uuid::new_v4(), 100);
        let proof = context();
        let mut child = ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "crash_child", "--nocapture"])
                .env("TIDE_SYNTHETIC_CRASH_CHILD", "1")
                .env("TIDE_SYNTHETIC_DB", &file.0)
                .env(
                    "TIDE_SYNTHETIC_REQUEST",
                    serde_json::to_string(&operation).unwrap(),
                )
                .env(
                    "TIDE_SYNTHETIC_CONTEXT",
                    serde_json::to_string(&proof).unwrap(),
                )
                .env("TIDE_SYNTHETIC_PHASE", phase)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = child.0.stdout.take().unwrap();
        let (ready, wait) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if line.unwrap_or_default().contains("DURABLE_CRASH_BOUNDARY") {
                    let _ = ready.send(());
                    break;
                }
            }
        });
        wait.recv_timeout(Duration::from_secs(15))
            .expect("child did not reach forced-exit boundary");
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        reader.join().unwrap();
        drop(child);
        let mut recovered = DurableLedger::open(&file.0).unwrap();
        let expected = usize::from(phase == "after_commit");
        assert_eq!(recovered.records().unwrap().len(), expected);
        assert_eq!(
            recovered.lookup_request(operation.id).unwrap().is_some(),
            phase == "after_commit"
        );
        let receipt = recovered
            .execute_guarded(operation, proof, || true)
            .unwrap();
        assert_eq!(receipt.replayed, phase == "after_commit");
        assert_eq!(recovered.records().unwrap().len(), 1);
        recovered.snapshot().unwrap().audit().unwrap();
    }
}
