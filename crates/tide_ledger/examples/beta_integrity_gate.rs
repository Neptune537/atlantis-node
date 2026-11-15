//! Fail-closed release probe. A successful regression suite alone cannot release beta.
use atlantis_protocol::AtlantisAsset;
use tide_ledger::{LedgerTxType, TideLedgerEngine};
use uuid::Uuid;

fn main() {
    let mut blockers = Vec::new();
    let mut ledger = TideLedgerEngine::new();
    let owner = Uuid::new_v4();
    ledger.seed_account(owner, 100.0);
    // Independent mutation must be detected by reconciliation to opening balances.
    *ledger.balances.get_mut(&owner).unwrap() += 1.0;
    if ledger.verify_audit_invariants().is_ok() {
        blockers.push("audit accepts an unjournaled balance increase");
    }
    let mut ledger = TideLedgerEngine::new();
    ledger.seed_account(owner, 100.0);
    let receiver = Uuid::new_v4();
    // A CYAN balance cannot also fund USDC. The current API cannot express this.
    if ledger.record_double_entry(LedgerTxType::Withdrawal, Uuid::new_v4(), owner,
        receiver, AtlantisAsset::Cyan, 10.0, 0).is_ok()
        && ledger.record_double_entry(LedgerTxType::Withdrawal, Uuid::new_v4(), owner,
            receiver, AtlantisAsset::Usdc, 10.0, 0).is_ok() {
        blockers.push("balances have no asset denomination; cross-asset isolation cannot be established");
    }
    for blocker in &blockers { eprintln!("BETA BLOCKED: {blocker}"); }
    if !blockers.is_empty() { std::process::exit(1); }
    // Removing these counterexamples does not establish the other acceptance gates.
    eprintln!("Ledger probes passed; formal proof, Reticulum and hardware gates still required.");
}
