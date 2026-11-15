//! Bounded regression checks of the real engine; these are not a formal proof.
use tide_ledger::{TideLedgerEngine, LedgerTxType};
use atlantis_protocol::AtlantisAsset;
use uuid::Uuid;

#[test]
fn invalid_transfers_leave_every_balance_and_journal_unchanged() {
    for amount in [0.0, -1.0, f64::NAN, f64::INFINITY, 101.0] {
        let mut ledger = TideLedgerEngine::new();
        let a = Uuid::new_v4(); let b = Uuid::new_v4();
        ledger.seed_account(a, 100.0);
        let before = ledger.balances.clone();
        assert!(ledger.record_double_entry(LedgerTxType::Withdrawal, Uuid::new_v4(), a, b, AtlantisAsset::Cyan, amount, 0).is_err());
        assert_eq!(ledger.balances, before);
        assert!(ledger.transactions.is_empty());
    }
}

#[test]
fn bounded_transfer_sequences_conserve_opening_supply() {
    // Exact integer values in f64 deliberately isolate journal behavior from rounding.
    for initial in 1..=32 {
        for amount in 1..=initial {
            let mut ledger = TideLedgerEngine::new();
            let a = Uuid::new_v4(); let b = Uuid::new_v4();
            ledger.seed_account(a, initial as f64);
            for (from, to) in [(a, b), (b, a)] {
                ledger.record_double_entry(LedgerTxType::Withdrawal, Uuid::new_v4(), from, to, AtlantisAsset::Cyan, amount as f64, 0).unwrap();
                assert_eq!(ledger.balances.values().sum::<f64>(), initial as f64);
                ledger.verify_audit_invariants().unwrap();
            }
            assert_eq!(ledger.balances[&a], initial as f64);
        }
    }
}

#[test]
fn self_transfer_and_recipient_overflow_are_atomic_errors() {
    let mut ledger = TideLedgerEngine::new();
    let a = Uuid::new_v4(); let b = Uuid::new_v4();
    ledger.seed_account(a, f64::MAX); ledger.seed_account(b, f64::MAX);
    let before = ledger.balances.clone();
    for to in [a, b] {
        assert!(ledger.record_double_entry(LedgerTxType::Withdrawal, Uuid::new_v4(), a, to, AtlantisAsset::Cyan, f64::MAX, 0).is_err());
        assert_eq!(ledger.balances, before);
        assert!(ledger.transactions.is_empty());
    }
}

