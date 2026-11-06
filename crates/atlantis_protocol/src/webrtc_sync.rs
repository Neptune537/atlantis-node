//! # WebRTC State Synchronization & Signaling Handler (`webrtc_sync.rs`)
//!
//! Provides WebRTC DataChannel session negotiation, STUN/TURN configuration,
//! and 60Hz tick state reconciliation for peer-to-peer multiplayer matches in HeavenlyBound.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Signaling payload exchanged via the Club Home Node broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalingEnvelope {
    pub room_id: String,
    pub sender_did: String,
    pub session_token: String,
    pub payload: SignalingPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SignalingPayload {
    Offer { sdp: String },
    Answer { sdp: String },
    IceCandidate {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u32>,
    },
    PeerJoined { peer_did: String },
    PeerLeft { peer_did: String },
}

/// STUN/TURN server configuration for NAT traversal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtcIceServerConfig {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

impl Default for RtcIceServerConfig {
    fn default() -> Self {
        Self {
            urls: vec![
                "stun:stun.l.google.com:19302".to_string(),
                "stun:relay.atlantis.mesh:3478".to_string(),
            ],
            username: None,
            credential: None,
        }
    }
}

/// A 60Hz multiplayer snapshot frame transmitted over the "kuroshio-sync" unreliable channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotFrame {
    pub tick_id: u64,
    pub timestamp_ms: u64,
    pub sender_did: String,
    pub entity_deltas: Vec<EntityStateDelta>,
    pub input_ack: u64,
    pub state_hash: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityStateDelta {
    pub entity_id: u32,
    pub position_ecef: [f32; 3], // Camera-relative offset
    pub velocity: [f32; 3],
    pub yaw_pitch_roll: [f32; 3],
    pub health: f32,
    pub shield: f32,
    pub flags: u32,
}

/// Reliable command sent over the "kuroshio-reliable" channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliableGameAction {
    pub action_id: u64,
    pub target_tick: u64,
    pub sender_did: String,
    pub action_type: ReliableActionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReliableActionType {
    PlayCard { card_id: String, target_sector_z: f32 },
    DeployStructure { structure_type: String, grid_coord: [i32; 2] },
    TriggerSacrifice { slot_index: u8, target_mana_pool: u32 },
    EquipItem { slot_index: u8, item_id: String },
}

/// Client-side 60Hz state reconciler with an 8-frame rollback ring.
pub struct StateReconciler {
    pub max_rollback_frames: usize,
    pub history_ring: BTreeMap<u64, SnapshotFrame>,
    pub current_tick: u64,
    pub last_acked_tick: u64,
}

impl StateReconciler {
    pub const DEFAULT_MAX_ROLLBACK: usize = 8; // 133ms at 60Hz

    pub fn new() -> Self {
        Self {
            max_rollback_frames: Self::DEFAULT_MAX_ROLLBACK,
            history_ring: BTreeMap::new(),
            current_tick: 0,
            last_acked_tick: 0,
        }
    }

    /// Ingests a remote frame and determines if physics rollback is required.
    pub fn ingest_remote_snapshot(&mut self, remote_frame: SnapshotFrame) -> ReconciliationResult {
        let tick = remote_frame.tick_id;
        self.history_ring.insert(tick, remote_frame.clone());

        // Trim history older than max rollback
        if tick > self.max_rollback_frames as u64 {
            let cutoff = tick - self.max_rollback_frames as u64;
            self.history_ring.retain(|&k, _| k >= cutoff);
        }

        if tick < self.current_tick {
            let frames_behind = self.current_tick - tick;
            if frames_behind <= self.max_rollback_frames as u64 {
                ReconciliationResult::RollbackAndReplay {
                    from_tick: tick,
                    target_tick: self.current_tick,
                }
            } else {
                // Drift is too high; hard snap to authoritative state
                ReconciliationResult::HardSnapToState {
                    authoritative_snapshot: remote_frame,
                }
            }
        } else {
            self.last_acked_tick = remote_frame.input_ack;
            ReconciliationResult::InterpolateSmooth
        }
    }
}

pub enum ReconciliationResult {
    InterpolateSmooth,
    RollbackAndReplay { from_tick: u64, target_tick: u64 },
    HardSnapToState { authoritative_snapshot: SnapshotFrame },
}
