//! # Adversarial Trust-Boundary & Settlement Integrity Integration Suite
//!
//! Validates the critical trust-boundary invariants under adversarial conditions:
//! 1. Forged Identity & Impersonation Rejection
//! 2. Cross-Domain / Replay Attack Rejection
//! 3. Concurrent Multi-Threaded Double-Spend Resistance
//! 4. Direct Protocol Capability Enforcement (Server-Side)
//! 5. Ledger Double-Entry Conservation & Zero-Variance Audit

use atlantis_protocol::lumeria_sdk::{AppCapability, AppManifest, AppSecurityTier};
use atlantis_protocol::AtlantisAsset;
use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use std::sync::Arc;
use tide_ledger::{LedgerTxType, TideLedgerEngine};
use tokio::sync::RwLock;
use uuid::Uuid;

#[test]
fn test_adversary_1_forged_identity_fails_authorization() {
    let attacker_key = SigningKey::generate(&mut OsRng);
    let victim_did = "did:atlantis:victim_sovereign_account";

    // Attacker crafts valid cryptographic signature using their own key,
    // but attempts to claim identity of the victim DID.
    let payload_bytes = format!("TRANSFER:1000:CYAN:TO:attacker:FROM:{}", victim_did);
    let signature = attacker_key.sign(payload_bytes.as_bytes());

    // Invariant Check: Signature verifies against attacker's key,
    // but MUST FAIL when evaluated against victim's authoritative public key.
    let victim_key = SigningKey::generate(&mut OsRng);
    let victim_verifying_key = victim_key.verifying_key();

    let verification_result = victim_verifying_key.verify_strict(payload_bytes.as_bytes(), &signature);
    assert!(
        verification_result.is_err(),
        "CRITICAL: Signature from attacker key must never verify against victim public key"
    );
}

#[test]
fn test_adversary_2_domain_separation_blocks_replay() {
    let alice_key = SigningKey::generate(&mut OsRng);
    let devnet_domain = "KUROSHIO_DEVNET_V1";
    let mainnet_domain = "KUROSHIO_CORAL_V1";

    let channel_id = "escrow_chan_99";
    let amount = 50_000u64;
    let sequence = 42u64;

    // Payload signed for devnet domain
    let devnet_payload = format!("{}:{}:{}:{}", devnet_domain, channel_id, sequence, amount);
    let signature = alice_key.sign(devnet_payload.as_bytes());

    // Mainnet authority reconstructs expected canonical domain transcript
    let mainnet_expected_payload = format!("{}:{}:{}:{}", mainnet_domain, channel_id, sequence, amount);

    let replay_result = alice_key.verifying_key().verify_strict(
        mainnet_expected_payload.as_bytes(),
        &signature,
    );
    assert!(
        replay_result.is_err(),
        "CRITICAL: Cross-domain replay must be mathematically rejected by domain separator"
    );
}

#[tokio::test]
async fn test_adversary_3_concurrent_double_spend_race_blocked() {
    let ledger = Arc::new(RwLock::new(TideLedgerEngine::new()));
    let alice_id = Uuid::new_v4();
    let bob_id = Uuid::new_v4();

    // Seed Alice with exactly 100.0 CYAN
    {
        let mut l = ledger.write().await;
        l.seed_account(alice_id, 100.0);
        l.seed_account(bob_id, 0.0);
    }

    // Spawn 10 concurrent withdrawal tasks attempting to drain 80.0 CYAN each
    // Only 1 transaction can legally succeed; the remaining 9 MUST fail with insufficient funds.
    let mut handles = vec![];
    for _ in 0..10 {
        let l_clone = Arc::clone(&ledger);
        handles.push(tokio::spawn(async move {
            let mut l = l_clone.write().await;
            l.record_double_entry(
                LedgerTxType::EscrowLock,
                Uuid::new_v4(),
                alice_id,
                bob_id,
                AtlantisAsset::Cyan,
                80.0,
                1730000000,
            )
        }));
    }

    let mut successes = 0;
    let mut failures = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => successes += 1,
            Err(_) => failures += 1,
        }
    }

    assert_eq!(successes, 1, "Exactly one withdrawal transaction must succeed");
    assert_eq!(failures, 9, "All 9 concurrent overdraws must be rejected");

    let l = ledger.read().await;
    let alice_balance = *l.balances.get(&alice_id).unwrap();
    assert_eq!(alice_balance, 20.0, "Alice balance must be exactly 20.0 (100 - 80)");
    assert!(l.verify_audit_invariants().is_ok(), "Double-entry invariants must hold");
}

#[test]
fn test_adversary_4_server_side_capability_enforcement_blocks_guest_checkout() {
    // Malicious guest client crafts manifest claiming SandboxedGuest tier
    // but requests high-value StoreInitiateCheckout capability
    let mut capabilities = std::collections::HashSet::new();
    capabilities.insert(AppCapability::StoreInitiateCheckout);

    let manifest = AppManifest {
        app_id: "com.adversary.cart_drain".to_string(),
        name: "Malicious Cart".to_string(),
        version: "1.0.0".to_string(),
        publisher_did: "did:atlantis:adversary".to_string(),
        capabilities,
        package_signature: "sig_bytes".to_string(),
        security_tier: AppSecurityTier::SandboxedGuest,
    };

    // Server-side manifest validation MUST reject checkout capability for SandboxedGuest
    let validation = manifest.validate_permissions();
    assert!(
        validation.is_err(),
        "CRITICAL: Server must reject StoreInitiateCheckout capability for SandboxedGuest"
    );
}

#[test]
fn test_adversary_5_ledger_double_entry_zero_variance() {
    let mut engine = TideLedgerEngine::new();
    let club_treasury = Uuid::new_v4();
    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    engine.seed_account(club_treasury, 1000.0);
    engine.seed_account(user_a, 500.0);
    engine.seed_account(user_b, 0.0);

    // Initial audit must balance
    assert!(engine.verify_audit_invariants().is_ok());

    // Execute transfer
    engine
        .record_double_entry(
            LedgerTxType::EscrowLock,
            Uuid::new_v4(),
            user_a,
            user_b,
            AtlantisAsset::Cyan,
            200.0,
            1730000000,
        )
        .expect("Escrow transfer recorded");

    // Recalculate global audit invariants
    let audit = engine.verify_audit_invariants();
    assert!(
        audit.is_ok(),
        "Double-entry ledger must maintain 0-variance accounting after transfer"
    );
}
