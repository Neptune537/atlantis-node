//! # Atlantis SavedEpisode & Daily Feed Wire Contracts
//!
//! Provides the canonical DTOs for the 24/7 Daily Episode Engine, OpenMontage,
//! and Proof-of-Health ($H_n$) pre-flight spillover dispatch routing between personal
//! edge nodes (EpipelagicEdge) and sovereign Club Home Nodes (AbyssalAnchor / PelagicWorker).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Narrative act categorization within a canonical 1-hour episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NarrativeAct {
    Act1Intro,
    Act2Complication,
    Act3Climax,
    Act4Resolution,
}

/// A micro-chunk of synthesized or real-time audio/video content within an Act.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeMediaChunk {
    /// Unique identifier for this media chunk (e.g. `chunk_0042_vance_monologue`).
    pub chunk_id: String,
    /// Offset in seconds from start of the episode.
    pub start_offset_secs: f64,
    /// Duration of this chunk in seconds.
    pub duration_secs: f64,
    /// URI or content hash of the micro-chunk (e.g. `ipfs://...`, `https://...`, or local cache path).
    pub media_uri: String,
    /// Audio stem configuration (vocal, ambient, sfx, eas_stinger).
    pub stem_type: String,
    /// Bitrate requirement in kbps.
    pub bitrate_kbps: u32,
}

/// Dynamic 3D avatar and spatial actor staging instructions for Lumeria viewport execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialStageDirection {
    /// Actor or avatar DID / identifier (e.g. `did:atlantis:kenji_vance`, `did:atlantis:dexter_sloan`).
    pub actor_id: String,
    /// World or room anchor position [x, y, z] in camera-relative meters.
    pub target_position: [f32; 3],
    /// Orientation quaternion [x, y, z, w].
    pub target_rotation: [f32; 4],
    /// Character emotion, pose, or motion prompt for Dual Quaternion kinematic blending.
    pub motion_prompt: String,
    /// Spatial audio emitter gain and RT60 send override.
    pub audio_gain: f32,
}

/// State delta applied to the operative or club ledger upon consuming an episode segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeStateDelta {
    /// Target recipient DID.
    pub target_did: String,
    /// Micro-CYAN reward or tip amount.
    pub cyan_amount: u64,
    /// Unlocked narrative memory flags or knowledge tags.
    pub unlocked_flags: Vec<String>,
    /// Digital property or .nautpak asset voucher ID if any.
    pub asset_voucher_id: Option<String>,
}

/// A single narrative block within an Act.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NarrativeBlock {
    pub block_id: String,
    pub act: NarrativeAct,
    pub title: String,
    pub synopsis: String,
    pub media_chunks: Vec<EpisodeMediaChunk>,
    pub stage_directions: Vec<SpatialStageDirection>,
    pub state_deltas: Vec<EpisodeStateDelta>,
}

/// Compute and hardware requirements profile for rendering and simulating an episode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeHardwareProfile {
    /// Required VRAM in Megabytes for local 3D Gaussian Splats and textures.
    pub min_vram_mb: u64,
    /// Required system RAM in Megabytes for OpenMontage timeline buffering.
    pub min_ram_mb: u64,
    /// Minimum Proof-of-Health capability score $H_n$ required for local host rendering (default: 0.65).
    pub min_poh_score: f64,
    /// Whether local AI dialogue inference (Ollama/vLLM) is required.
    pub requires_local_llm: bool,
}

impl Default for EpisodeHardwareProfile {
    fn default() -> Self {
        Self {
            min_vram_mb: 4096,   // 4GB VRAM
            min_ram_mb: 8192,    // 8GB RAM
            min_poh_score: 0.65, // Atlantis PoH operational threshold
            requires_local_llm: true,
        }
    }
}

/// Canonical SavedEpisode payload produced by the Daily Episode Engine / OpenMontage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedEpisode {
    /// Unique episode identifier (e.g. `ep_2026_10_24_kaiju_breach`).
    pub episode_id: String,
    /// Title of the broadcast episode.
    pub title: String,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// Total episode length in seconds (typically 3600.0 for a 1-hour broadcast).
    pub total_duration_secs: f64,
    /// Hardware demand profile.
    pub hardware_profile: EpisodeHardwareProfile,
    /// Ordered list of narrative blocks across Acts 1 to 4.
    pub narrative_blocks: Vec<NarrativeBlock>,
    /// Metadata tags (e.g. tags: ["breaking_news", "kaiju", "houston_sector"]).
    pub metadata: HashMap<String, String>,
}

/// Target dispatch routing decision calculated during pre-flight PoH evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EpisodeProcessingRoute {
    /// Hardware capacity is verified ($H_n \ge 0.65$): process and render directly on the local Home Node.
    LocalExecution {
        vram_allocated_mb: u64,
        local_ai_model: String,
    },
    /// Hardware constrained ($H_n < 0.65$): offload processing to accepted Club Home Node.
    ClubSpillover {
        target_club_node_did: String,
        target_club_endpoint: String,
        stream_delivery_mode: EpisodeStreamDeliveryMode,
    },
}

/// Delivery mechanism when an episode execution is offloaded to a Club Node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EpisodeStreamDeliveryMode {
    /// Low-Latency HLS video stream with micro-chunks and synchronized JSON metadata stream.
    LowLatencyHls { stream_url: String },
    /// Compressed .nautpak DRM asset packet stream for client-side low-spec rasterization.
    NautpakAssetStream { package_url: String },
    /// Interactive WebRTC remote frame stream for mobile/VR thin client.
    WebRtcInteractive { signaling_room_id: String },
}

/// Status report returned by a node processing an episode feed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeFeedStatus {
    pub episode_id: String,
    pub is_spillover: bool,
    pub active_act: NarrativeAct,
    pub playback_offset_secs: f64,
    pub current_poh_score: f64,
    pub state_deltas_applied: usize,
}
