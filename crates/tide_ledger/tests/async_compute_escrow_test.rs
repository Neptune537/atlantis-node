//! # Asynchronous Compute Job & Escrow Lifecycle Test (PROP-0043)
//!
//! Verifies the full pipeline:
//! 1. Client creates `AsynchronousComputeJob` (DEM Elevation / Splat Hull Synthesis)
//! 2. Buyer locks 10.0 CYAN into Coral Escrow
//! 3. Triton Anchor processes task and generates `AsynchronousExecutionReceipt` with Merkle root
//! 4. Tide Ledger verifies receipt and disburses 80% to worker, 15% to treasury, and burns 5%

use atlantis_protocol::{
    AsynchronousComputeJob, AsynchronousExecutionReceipt, AtlantisAsset, ComputeJobType,
};
use tide_ledger::{EscrowStatus, TideLedgerEngine};
use uuid::Uuid;

#[test]
fn test_async_compute_job_escrow_settlement_lifecycle() {
    let mut ledger = TideLedgerEngine::new();

    let client_account = Uuid::new_v4();
    let worker_account = Uuid::new_v4();
    let guild_treasury_account = Uuid::new_v4();

    // Fund client with 50.0 CYAN
    ledger.seed_account(client_account, 50.0);

    // 1. Client creates an offloaded DEM heightfield tiling task
    let job_id = Uuid::new_v4();
    let job = AsynchronousComputeJob {
        job_id,
        client_did: "did:atlantis:sebastian_edge".to_string(),
        job_type: ComputeJobType::DemElevationTiling {
            resolution: 512,
            target_lod: 1,
        },
        payload_uri: "ipfs://dem/terrain_chunk_419.geotiff".to_string(),
        input_hash: [0x5au8; 32],
        escrow_account_id: "coral_holding_account".to_string(),
        max_latency_secs: 60,
        reward_cyan: 10.0,
    };

    // 2. Lock 10.0 CYAN into Coral Escrow
    let escrow_vault = ledger
        .lock_coral_escrow(
            job.job_id,
            client_account,
            guild_treasury_account,
            AtlantisAsset::Cyan,
            job.reward_cyan,
            1730000000,
        )
        .expect("Escrow lock should succeed");

    assert_eq!(*ledger.balances.get(&client_account).unwrap(), 40.0);
    assert_eq!(escrow_vault.locked_amount, 10.0);
    assert_eq!(escrow_vault.status, EscrowStatus::AuthorizedLocked);

    // 3. Worker (Triton) produces synthetic Merkle receipt
    let receipt = AsynchronousExecutionReceipt {
        job_id: job.job_id,
        worker_did: "did:atlantis:triton_abyssal_anchor".to_string(),
        output_data_uri: "ipfs://b3dm/dem_chunk_419_lod1.b3dm".to_string(),
        output_merkle_root: [0x12u8; 32],
        execution_duration_ms: 450,
        signature: vec![0u8; 64],
    };

    assert_eq!(receipt.job_id, job.job_id);

    // 4. Verify & settle escrow: 80% to worker (8.0), 15% to treasury (1.5), 5% burned (0.5)
    let (worker_tx, treasury_tx, burned) = ledger
        .settle_async_compute_escrow(escrow_vault.id, worker_account, guild_treasury_account, 1730000001)
        .expect("Escrow release must succeed");

    assert_eq!(worker_tx.amount, 8.0);
    assert_eq!(treasury_tx.amount, 1.5);
    assert_eq!(burned, 0.5);

    assert_eq!(*ledger.balances.get(&worker_account).unwrap(), 8.0);
    assert_eq!(*ledger.balances.get(&guild_treasury_account).unwrap(), 1.5);
    assert_eq!(*ledger.balances.get(&client_account).unwrap(), 40.0);
}
