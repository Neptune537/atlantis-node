//! Real migration subprocess, synthetic SQLite fixtures only; no Python.
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command};
use uuid::Uuid;

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        for name in [
            "legacy.sqlite",
            "reviewed.json",
            "v2.sqlite",
            "rejected.sqlite",
        ] {
            let _ = fs::remove_file(self.0.join(name));
        }
        let _ = fs::remove_dir(&self.0);
    }
}

#[test]
fn migration_checks_source_identity_coverage_overwrite_and_preservation() {
    let fixture =
        Fixture(std::env::temp_dir().join(format!("tide-rust-migration-{}", Uuid::new_v4())));
    fs::create_dir(&fixture.0).unwrap();
    let source = fixture.0.join("legacy.sqlite");
    let manifest = fixture.0.join("reviewed.json");
    let output = fixture.0.join("v2.sqlite");
    let account = Uuid::new_v4();
    let conn = rusqlite::Connection::open(&source).unwrap();
    conn.execute_batch("CREATE TABLE accounts (id TEXT PRIMARY KEY, balance REAL NOT NULL);")
        .unwrap();
    conn.execute(
        "INSERT INTO accounts VALUES (?1, ?2)",
        rusqlite::params![account.to_string(), 100.0_f64],
    )
    .unwrap();
    drop(conn);
    let original = fs::read(&source).unwrap();
    let mut review = json!({"source_sha256":format!("{:x}",Sha256::digest(&original)),"reconciliation_reference":"synthetic Rust fixture only","unit_policy_reference":"synthetic 100 units per token",
        "accounts":[{"account":account,"legacy_balance_bits":format!("{:016x}",100.0_f64.to_bits()),"allocations":[{"asset":"Cyan","units":10000}]}],
        "escrows":[{"escrow":Uuid::new_v4(),"owner":account,"asset":"Cyan","units":2500}]});
    fs::write(&manifest, review.to_string()).unwrap();
    let invoke = |destination: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_migrate_v2"))
            .arg(&source)
            .arg(&manifest)
            .arg(destination)
            .output()
            .unwrap()
    };
    let result = invoke(&output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let ledger = tide_ledger::v2::Ledger::load(output.to_str().unwrap()).unwrap();
    ledger.audit().unwrap();
    assert_eq!(ledger.journal().len(), 2);
    assert_eq!(ledger.balance(tide_ledger::v2::Asset::Cyan, account), 7500);
    let written = fs::read(&output).unwrap();
    assert!(!invoke(&output).status.success());
    assert_eq!(fs::read(&output).unwrap(), written);
    review["source_sha256"] = json!("0".repeat(64));
    fs::write(&manifest, review.to_string()).unwrap();
    let rejected = fixture.0.join("rejected.sqlite");
    assert!(!invoke(&rejected).status.success());
    assert!(!rejected.exists());
    review["source_sha256"] = json!(format!("{:x}", Sha256::digest(&original)));
    review["accounts"][0]["legacy_balance_bits"] = json!("0000000000000000");
    fs::write(&manifest, review.to_string()).unwrap();
    assert!(!invoke(&rejected).status.success());
    assert!(!rejected.exists());
    assert_eq!(fs::read(&source).unwrap(), original);
}
