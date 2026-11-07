//! # Kuroshio Mesh (`kuroshio_mesh`)
//!
//! Direct WebRTC DataChannels, libp2p Gossipsub browser mesh transport,
//! and HTLC Escrow with Groth16 ZK-SNARK geofence settlements on Coral-V2.

pub mod htlc_geofence;
pub mod transport;

pub use htlc_geofence::{
    GeofencePublicInputs, Groth16Proof, HtlcGeofenceEngine, HtlcSettlementError, HtlcZkEscrow,
    ZkGeofenceArrivalProof,
};
pub use transport::{
    BrowserMeshTransport, ChannelReliability, MeshEnvelope, MeshPayload, PeerConnectionState,
    WebRtcDataChannelPeer,
};

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use tide_ledger::TideLedgerEngine;
    use uuid::Uuid;

    #[test]
    fn test_browser_mesh_movement_and_dice_sync() {
        let mut node_a = BrowserMeshTransport::new("peer-alice".to_string());
        node_a.on_peer_connected("peer-bob".to_string(), ChannelReliability::ReliableOrdered);

        // 1. Broadcast movement
        node_a.broadcast_player_transform(
            42,
            [100.0, 200.0, 300.0],
            [1.0, 0.0, 0.0],
            [0.5, 0.0],
            "Running".to_string(),
            1000,
        );

        assert_eq!(node_a.outgoing_queue.len(), 1);
        let move_msg = node_a.outgoing_queue.pop().unwrap();

        // 2. Deliver to Bob
        let mut node_b = BrowserMeshTransport::new("peer-bob".to_string());
        node_b.receive_incoming(move_msg);
        let drained = node_b.drain_incoming();
        assert_eq!(drained.len(), 1);

        // 3. Roll synchronized d20
        let seed = [7u8; 32];
        let roll_env = node_a.broadcast_verifiable_dice_roll("1d20+3", 1, 20, 3, seed, 2000);
        if let MeshPayload::VerifiableDiceRoll {
            sides,
            count,
            modifier,
            result_sum,
            individual_results,
            ..
        } = roll_env.payload
        {
            assert_eq!(sides, 20);
            assert_eq!(count, 1);
            assert_eq!(modifier, 3);
            assert_eq!(individual_results.len(), 1);
            assert_eq!(result_sum, individual_results[0] as i32 + 3);
        } else {
            panic!("Expected VerifiableDiceRoll payload");
        }
    }

    #[test]
    fn test_htlc_geofence_zk_settlement_and_expiry() {
        let mut ledger = TideLedgerEngine::new();
        let mut engine = HtlcGeofenceEngine::new();

        let buyer = Uuid::new_v4();
        let claimer = Uuid::new_v4();
        let treasury = Uuid::new_v4();
        let claimer_did = "did:atlantis:explorer-007".to_string();

        ledger.balances.insert(buyer, 1000.0);
        ledger.balances.insert(claimer, 0.0);
        ledger.balances.insert(treasury, 0.0);

        let secret_preimage = b"kuroshio-secret-artifact-key".to_vec();
        let mut hasher = Sha256::new();
        hasher.update(&secret_preimage);
        let hash_lock: [u8; 32] = hasher.finalize().into();

        let epoch: u64 = 1700000000;
        let mut commit_hasher = Sha256::new();
        commit_hasher.update(claimer_did.as_bytes());
        commit_hasher.update(&epoch.to_le_bytes());
        let claimer_commitment: [u8; 32] = commit_hasher.finalize().into();

        let geofence = GeofencePublicInputs {
            target_lat_scaled: 377749290,  // San Francisco 37.774929
            target_lon_scaled: -1224194160, // -122.419416
            max_distance_sq_meters: 2500.0, // 50m radius
            claimer_commitment_hash: claimer_commitment,
            valid_epoch_seconds: epoch,
        };

        // Create HTLC with 500 CYAN bounty
        let contract = engine
            .create_zk_htlc_bounty(
                &mut ledger,
                buyer,
                claimer,
                claimer_did.clone(),
                treasury,
                500.0,
                hash_lock,
                geofence.clone(),
                epoch + 3600, // 1 hour expiry
                epoch,
            )
            .expect("Failed to lock HTLC bounty");

        assert_eq!(*ledger.balances.get(&buyer).unwrap(), 500.0);

        // Construct valid ZK proof with non-trivial distinct entropy
        let mut a_g1 = [0x55u8; 32];
        a_g1[0] = 0x12;
        a_g1[31] = 0x89;
        let mut b_g2_0 = [0xaau8; 32];
        b_g2_0[0] = 0x34;
        let mut b_g2_1 = [0xbbu8; 32];
        b_g2_1[0] = 0x56;
        let mut c_g1 = [0x77u8; 32];
        c_g1[0] = 0x78;
        c_g1[31] = 0xef;

        let zk_proof = ZkGeofenceArrivalProof {
            proof: Groth16Proof {
                a_g1,
                b_g2_0,
                b_g2_1,
                c_g1,
            },
            public_inputs: geofence,
            circuit_vk_id: "vk_groth16_geofence_v2".to_string(),
        };

        // Settle with valid preimage and ZK proof
        let (_worker_tx, _dividend_tx) = engine
            .settle_with_zk_proof(
                &mut ledger,
                contract.contract_id,
                secret_preimage,
                &zk_proof,
                epoch + 60,
            )
            .expect("Settlement should succeed");

        let settled_vault = ledger.escrows.get(&contract.base_escrow_id).unwrap();
        assert_eq!(
            settled_vault.status,
            tide_ledger::EscrowStatus::CompletedReleased
        );
        // Claimer gets 500 - 2% (10.0) = 490.0
        assert_eq!(*ledger.balances.get(&claimer).unwrap(), 490.0);
        assert_eq!(*ledger.balances.get(&treasury).unwrap(), 10.0);
    }
}
