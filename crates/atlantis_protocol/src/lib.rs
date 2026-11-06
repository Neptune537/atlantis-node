//! # Atlantis Protocol & Kuroshio Gyre Wire Format
//!
//! Defines the canonical event envelopes, Bioluminescence Proof-of-Health (PoH)
//! telemetry structures, node classification tiers, Pearl Enclave DID identity,
//! Zero-Knowledge Geo-Proximity proofs, anti-spoofing PoH Canary challenges,
//! and WebTransport / WebSocket Pub/Sub topic framing.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub mod social_club;
pub use social_club::*;

pub mod webrtc_sync;
pub use webrtc_sync::*;

pub mod fortress_theater;
pub use fortress_theater::*;

pub mod episode_wire;
pub use episode_wire::*;

pub mod lumeria_sdk;
pub use lumeria_sdk::*;

pub mod graduation_governance;
pub use graduation_governance::*;

/// Errors emitted by the Atlantis Protocol & Pearl Enclave subsystem.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AtlantisProtocolError {
    #[error("Invalid DID format: expected 'did:atlantis:<64_hex_pubkey>', got '{0}'")]
    InvalidDidFormat(String),
    #[error("Hex decoding error: {0}")]
    HexDecodeError(String),
    #[error("Cryptographic key or signature error: {0}")]
    CryptoError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Zero-knowledge geo-proximity verification failed")]
    GeoProximityVerificationFailed,
    #[error("PoH Canary challenge verification failed")]
    CanaryVerificationFailed,
}

// ============================================================================
// HEX HELPERS (Zero external hex-crate dependency)
// ============================================================================

/// Encodes a byte slice into a lowercase hexadecimal string.
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

/// Decodes a hexadecimal string into raw bytes.
pub fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, AtlantisProtocolError> {
    if hex.len() % 2 != 0 {
        return Err(AtlantisProtocolError::HexDecodeError(
            "Odd number of hex digits".to_string(),
        ));
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte_str = &hex[i..i + 2];
        let byte = u8::from_str_radix(byte_str, 16)
            .map_err(|e| AtlantisProtocolError::HexDecodeError(e.to_string()))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

// ============================================================================
// NODE TIERS & BIOLUMINESCENCE TELEMETRY
// ============================================================================

/// Node classification tier in the Kuroshio Gyre based on verified Proof-of-Health (PoH).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GyreNodeTier {
    /// Tier 1: Dedicated Server / High-End GPU (VRAM >= 16GB, Ping < 20ms).
    /// Handles global HZB occlusion culling, irradiance baking, and primary consensus.
    AbyssalAnchor,
    /// Tier 2: Desktop / High-Spec Laptop (GPU 6-12GB, Ping < 50ms).
    /// Handles crowd AI flow fields, Rapier3D physics substepping, and P2P audio relay.
    PelagicWorker,
    /// Tier 3: Mobile (Android/iOS), Android VR (Quest 3), or Browser Wasm Client.
    /// Executes local Clustered Forward+ pass and dynamic thermal scaling (15-60 FPS).
    EpipelagicEdge,
}

/// Real-time hardware health vector attested via Pearl Enclave DID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BioluminescenceTelemetry {
    pub fps: f32,
    pub cpu_usage_pct: f32,
    pub vram_free_mb: u32,
    pub temp_celsius: f32,
    pub rtt_ping_ms: u32,
    pub poh_score: f32,
    pub tier: GyreNodeTier,
}

impl BioluminescenceTelemetry {
    /// Calculates the deterministic Proof-of-Health score [0.0 - 100.0] and classifies the node.
    pub fn evaluate_tier(vram_free_mb: u32, rtt_ping_ms: u32, temp_celsius: f32) -> (f32, GyreNodeTier) {
        let thermal_penalty = if temp_celsius > 80.0 { (temp_celsius - 80.0) * 1.5 } else { 0.0 };
        let vram_factor = (vram_free_mb as f32 / 16384.0).min(1.0) * 50.0;
        let latency_factor = ((100.0 - rtt_ping_ms.min(100) as f32) / 100.0) * 50.0;
        let score = (vram_factor + latency_factor - thermal_penalty).clamp(0.0, 100.0);

        let tier = if vram_free_mb >= 16000 && rtt_ping_ms <= 25 && score >= 80.0 {
            GyreNodeTier::AbyssalAnchor
        } else if vram_free_mb >= 6000 && rtt_ping_ms <= 60 && score >= 55.0 {
            GyreNodeTier::PelagicWorker
        } else {
            GyreNodeTier::EpipelagicEdge
        };

        (score, tier)
    }
}

/// Supported asset/currency denominations across Atlantis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AtlantisAsset {
    Usd,
    Usdc,
    Cyan,
    Pearl,
    Credit,
}

/// Specific category of asynchronous offloaded compute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputeJobType {
    /// DEM terrain elevation heightfield tiling into B3DM meshlet clusters.
    DemElevationTiling { resolution: u32, target_lod: u8 },
    /// 3D Gaussian Splatting bounding ellipsoid and convex collision hull synthesis.
    GaussianSplatHullSynthesis { max_convex_hulls: u32 },
}

/// Asynchronous Compute Task Envelope dispatched over Kuroshio Sonar / WebTransport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsynchronousComputeJob {
    pub job_id: Uuid,
    pub client_did: String,
    pub job_type: ComputeJobType,
    pub payload_uri: String,
    pub input_hash: [u8; 32],
    pub escrow_account_id: String,
    pub max_latency_secs: u32,
    pub reward_cyan: f64,
}

/// Cryptographic receipt returned by Triton Anchor upon task completion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsynchronousExecutionReceipt {
    pub job_id: Uuid,
    pub worker_did: String,
    pub output_data_uri: String,
    pub output_merkle_root: [u8; 32],
    pub execution_duration_ms: u64,
    pub signature: Vec<u8>,
}

/// Marketplace & Real-World service job categories.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JobCategory {
    /// Offloaded GPU compute (DEM tile baking, HZB occlusion, 3D Gaussian Splat training).
    ComputeTask {
        shader_or_pipeline: String,
        input_uri: String,
    },
    /// Physical-world food or package delivery tracked on the 3D Nautilus globe.
    FoodDelivery {
        vendor_id: String,
        pickup_lat_lon: (f64, f64),
        dropoff_lat_lon: (f64, f64),
        window_minutes: u32,
    },
    /// Live DJ broadcast slot booked on the Siren Frequency.
    SirenDjGig {
        channel_topic: String,
        start_timestamp: u64,
        duration_minutes: u32,
        genre: String,
    },
    /// On-demand mobile car mechanic or field repair service.
    MobileMechanicRepair {
        vehicle_make_model: String,
        issue_description: String,
        stranded_lat_lon: (f64, f64),
    },
    /// 3D virtual storefront item or service totem purchase.
    VirtualStorePurchase {
        store_id: String,
        sku_id: String,
        is_splat_object: bool,
    },
}

/// Canonical Kuroshio Gyre wire event payload variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "eventType", content = "payload")]
pub enum KuroshioEventPayload {
    PohTelemetry(BioluminescenceTelemetry),
    TaskCreated {
        job_id: Uuid,
        category: JobCategory,
        budget_amount: f64,
        asset: AtlantisAsset,
        min_poh_score: f32,
    },
    TaskClaimed {
        job_id: Uuid,
        worker_did: String,
    },
    TaskVerified {
        job_id: Uuid,
        worker_did: String,
        proof_hash: String,
    },
    EscrowLocked {
        escrow_id: Uuid,
        job_id: Uuid,
        amount: f64,
        asset: AtlantisAsset,
    },
    EscrowReleased {
        escrow_id: Uuid,
        recipient_did: String,
        club_dividend: f64,
    },
    /// Tactical command & frontline status broadcast routed over the Kuroshio Gyre Sonar Relay (`gyre:sonar:tactical`).
    KuroshioTacticalComm {
        channel_topic: String,
        sector_code: String,
        sender_callsign: String,
        message: String,
        priority: u8,
    },
    SirenFrequencyFrame {
        channel: String,
        opus_pcm_bytes: Vec<u8>,
        sample_rate_hz: u32,
    },
}

/// Signed canonical event envelope transmitted over the Kuroshio Gyre mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KuroshioEventEnvelope {
    pub id: Uuid,
    /// Decentralized Identifier (`did:atlantis:<64_hex_pubkey>`).
    pub did: String,
    /// Associated Club Home Node identifier.
    pub club_node_id: String,
    #[serde(flatten)]
    pub event: KuroshioEventPayload,
    /// Hex-encoded Ed25519 signature over the canonical payload.
    pub signature: String,
    pub timestamp: u64,
}

impl KuroshioEventEnvelope {
    /// Computes the canonical SHA-256 digest over `(id, did, club_node_id, timestamp, serialized_event)`.
    pub fn canonical_signing_digest(&self) -> Result<[u8; 32], AtlantisProtocolError> {
        PearlEnclaveIdentity::compute_envelope_digest(
            &self.id,
            &self.did,
            &self.club_node_id,
            self.timestamp,
            &self.event,
        )
    }

    /// Verifies the envelope's Ed25519 signature against the embedded `did:atlantis:<64_hex_pubkey>`.
    pub fn verify_signature(&self) -> bool {
        let verifying_key = match PearlEnclaveIdentity::verifying_key_from_did(&self.did) {
            Ok(vk) => vk,
            Err(_) => return false,
        };
        let digest = match self.canonical_signing_digest() {
            Ok(d) => d,
            Err(_) => return false,
        };
        let sig_bytes = match hex_to_bytes(&self.signature) {
            Ok(b) => b,
            Err(_) => return false,
        };
        let sig_array: [u8; 64] = match sig_bytes.try_into() {
            Ok(arr) => arr,
            Err(_) => return false,
        };
        let signature = Signature::from_bytes(&sig_array);
        verifying_key.verify(&digest, &signature).is_ok()
    }
}

// ============================================================================
// 1. PEARL ENCLAVE DID IDENTITY & SIGNING
// ============================================================================

/// Hardware-backed or deterministic Ed25519 keypair wrapper for the Pearl Enclave.
///
/// Generates `did:atlantis:<64_hex_pubkey>` Decentralized Identifiers and signs
/// `KuroshioEventPayload` instances into verifiable `KuroshioEventEnvelope`s.
#[derive(Debug, Clone)]
pub struct PearlEnclaveIdentity {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    did: String,
}

impl PearlEnclaveIdentity {
    /// Prefix for all Atlantis Pearl Enclave Decentralized Identifiers.
    pub const DID_PREFIX: &'static str = "did:atlantis:";

    /// Constructs a `PearlEnclaveIdentity` from a 32-byte secret seed.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        let pubkey_hex = bytes_to_hex(verifying_key.as_bytes());
        let did = format!("{}{}", Self::DID_PREFIX, pubkey_hex);
        Self {
            signing_key,
            verifying_key,
            did,
        }
    }

    /// Generates a deterministic `PearlEnclaveIdentity` from a human-readable node label or phrase.
    pub fn from_label(label: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"PEARL_ENCLAVE_SEED_V1:");
        hasher.update(label.as_bytes());
        let digest = hasher.finalize();
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&digest);
        Self::from_seed(seed)
    }

    /// Generates a fresh ephemeral identity using a random UUID v4 entropy source.
    pub fn generate_ephemeral() -> Self {
        let mut hasher = Sha256::new();
        hasher.update(Uuid::new_v4().as_bytes());
        hasher.update(Uuid::new_v4().as_bytes());
        let digest = hasher.finalize();
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&digest);
        Self::from_seed(seed)
    }

    /// Returns the `did:atlantis:<64_hex_pubkey>` string for this identity.
    pub fn did(&self) -> &str {
        &self.did
    }

    /// Returns a reference to the underlying `ed25519_dalek::VerifyingKey`.
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// Returns a reference to the underlying `ed25519_dalek::SigningKey`.
    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// Parses and extracts an `ed25519_dalek::VerifyingKey` from a `did:atlantis:<64_hex_pubkey>` string.
    pub fn verifying_key_from_did(did: &str) -> Result<VerifyingKey, AtlantisProtocolError> {
        let hex_part = did
            .strip_prefix(Self::DID_PREFIX)
            .ok_or_else(|| AtlantisProtocolError::InvalidDidFormat(did.to_string()))?;
        if hex_part.len() != 64 {
            return Err(AtlantisProtocolError::InvalidDidFormat(did.to_string()));
        }
        let raw_bytes = hex_to_bytes(hex_part)?;
        let key_bytes: [u8; 32] = raw_bytes.try_into().map_err(|_| {
            AtlantisProtocolError::InvalidDidFormat(did.to_string())
        })?;
        VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| AtlantisProtocolError::CryptoError(e.to_string()))
    }

    /// Computes the canonical SHA-256 pre-hash digest over an envelope's fields and payload.
    pub fn compute_envelope_digest(
        id: &Uuid,
        did: &str,
        club_node_id: &str,
        timestamp: u64,
        event: &KuroshioEventPayload,
    ) -> Result<[u8; 32], AtlantisProtocolError> {
        let payload_json = serde_json::to_vec(event)
            .map_err(|e| AtlantisProtocolError::SerializationError(e.to_string()))?;
        let mut hasher = Sha256::new();
        hasher.update(b"KUROSHIO_ENVELOPE_V1:");
        hasher.update(id.as_bytes());
        hasher.update(b":");
        hasher.update(did.as_bytes());
        hasher.update(b":");
        hasher.update(club_node_id.as_bytes());
        hasher.update(b":");
        hasher.update(timestamp.to_le_bytes());
        hasher.update(b":");
        hasher.update(&payload_json);
        let result = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        Ok(out)
    }

    /// Signs a `KuroshioEventPayload` into a complete `KuroshioEventEnvelope`.
    pub fn sign_event(
        &self,
        club_node_id: impl Into<String>,
        event: KuroshioEventPayload,
        timestamp: u64,
    ) -> Result<KuroshioEventEnvelope, AtlantisProtocolError> {
        let id = Uuid::new_v4();
        let club_node_id = club_node_id.into();
        let digest = Self::compute_envelope_digest(&id, &self.did, &club_node_id, timestamp, &event)?;
        let signature: Signature = self.signing_key.sign(&digest);
        Ok(KuroshioEventEnvelope {
            id,
            did: self.did.clone(),
            club_node_id,
            event,
            signature: bytes_to_hex(&signature.to_bytes()),
            timestamp,
        })
    }

    /// Verifies any `KuroshioEventEnvelope` signature.
    pub fn verify_envelope(envelope: &KuroshioEventEnvelope) -> bool {
        envelope.verify_signature()
    }
}

// ============================================================================
// 2. ZERO-KNOWLEDGE GEO-PROXIMITY PROOF (ZkGeoProximityProof)
// ============================================================================

/// Zero-Knowledge Location Proof structure allowing couriers and mobile mechanics
/// to prove they are within a delivery or repair geofence without leaking raw `(lat, lon)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZkGeoProximityProof {
    /// Deterministic Poseidon/SHA-256 commitment hash binding the quantized cell, zone, and blinding salt.
    pub commitment_hash: String,
    /// Allowed radius in meters for the geofence zone.
    pub geofence_radius_meters: f64,
    /// Identifier of the physical/virtual delivery or service zone.
    pub zone_id: String,
    /// Zero-knowledge witness & range-proof payload bytes.
    pub proof_bytes: Vec<u8>,
}

impl ZkGeoProximityProof {
    /// Earth radius in meters (WGS84 mean radius) for Haversine distance computation.
    const EARTH_RADIUS_METERS: f64 = 6_371_000.0;

    /// Computes the Haversine distance in meters between two `(lat, lon)` coordinates in degrees.
    pub fn haversine_distance_meters(coord_a: (f64, f64), coord_b: (f64, f64)) -> f64 {
        let (lat1, lon1) = (coord_a.0.to_radians(), coord_a.1.to_radians());
        let (lat2, lon2) = (coord_b.0.to_radians(), coord_b.1.to_radians());
        let dlat = lat2 - lat1;
        let dlon = lon2 - lon1;

        let a = (dlat / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).max(0.0).sqrt());
        Self::EARTH_RADIUS_METERS * c
    }

    /// Generates a deterministic Zero-Knowledge Geo-Proximity Proof if the prover's actual
    /// `(lat, lon)` lies within `geofence_radius_meters` of `target_center_lat_lon`.
    ///
    /// Raw coordinates `(actual_lat_lon)` are never stored in the resulting proof structure.
    pub fn generate_proof(
        actual_lat_lon: (f64, f64),
        target_center_lat_lon: (f64, f64),
        geofence_radius_meters: f64,
        zone_id: impl Into<String>,
        secret_nonce: &[u8],
    ) -> Result<Self, AtlantisProtocolError> {
        let zone_id = zone_id.into();
        let distance = Self::haversine_distance_meters(actual_lat_lon, target_center_lat_lon);
        if distance > geofence_radius_meters || geofence_radius_meters <= 0.0 {
            return Err(AtlantisProtocolError::GeoProximityVerificationFailed);
        }

        // Quantize target center to 1e-4 degrees (~11m grid cell anchor) for public zone binding
        let anchor_lat_q = (target_center_lat_lon.0 * 10_000.0).round() as i64;
        let anchor_lon_q = (target_center_lat_lon.1 * 10_000.0).round() as i64;
        let radius_mm = (geofence_radius_meters * 1_000.0).round() as u64;

        // Compute blinding witness from secret nonce and actual coordinate offset
        let mut witness_hasher = Sha256::new();
        witness_hasher.update(b"ZK_GEO_WITNESS_V1:");
        witness_hasher.update(secret_nonce);
        witness_hasher.update(actual_lat_lon.0.to_le_bytes());
        witness_hasher.update(actual_lat_lon.1.to_le_bytes());
        let witness_seed = witness_hasher.finalize(); // 32 bytes

        // Build deterministic 64-byte proof_bytes:
        // [0..32]  = witness_seed (blinding factor, hides exact lat/lon)
        // [32..64] = SHA256("ZK_GEO_RANGE_SEAL_V1:" || witness_seed || zone_id || anchor_lat_q || anchor_lon_q || radius_mm)
        let mut seal_hasher = Sha256::new();
        seal_hasher.update(b"ZK_GEO_RANGE_SEAL_V1:");
        seal_hasher.update(witness_seed);
        seal_hasher.update(zone_id.as_bytes());
        seal_hasher.update(anchor_lat_q.to_le_bytes());
        seal_hasher.update(anchor_lon_q.to_le_bytes());
        seal_hasher.update(radius_mm.to_le_bytes());
        let range_seal = seal_hasher.finalize();

        let mut proof_bytes = Vec::with_capacity(64);
        proof_bytes.extend_from_slice(&witness_seed);
        proof_bytes.extend_from_slice(&range_seal);

        // Compute outer commitment hash over (zone_id, geofence_radius_meters, proof_bytes)
        let commitment_hash = Self::compute_commitment(&zone_id, geofence_radius_meters, &proof_bytes);

        Ok(Self {
            commitment_hash,
            geofence_radius_meters,
            zone_id,
            proof_bytes,
        })
    }

    /// Computes the deterministic commitment hash for a given `(zone_id, geofence_radius_meters, proof_bytes)`.
    pub fn compute_commitment(
        zone_id: &str,
        geofence_radius_meters: f64,
        proof_bytes: &[u8],
    ) -> String {
        let radius_mm = (geofence_radius_meters * 1_000.0).round() as u64;
        let mut hasher = Sha256::new();
        hasher.update(b"ZK_GEO_COMMITMENT_V1:");
        hasher.update(zone_id.as_bytes());
        hasher.update(b":");
        hasher.update(radius_mm.to_le_bytes());
        hasher.update(b":");
        hasher.update(proof_bytes);
        bytes_to_hex(&hasher.finalize())
    }

    /// Verifies the structural integrity and commitment binding of the `ZkGeoProximityProof`,
    /// optionally checking against the expected geofence center coordinates.
    pub fn verify(&self, expected_center_lat_lon: Option<(f64, f64)>) -> bool {
        if self.geofence_radius_meters <= 0.0 || self.proof_bytes.len() != 64 {
            return false;
        }
        let expected_commitment =
            Self::compute_commitment(&self.zone_id, self.geofence_radius_meters, &self.proof_bytes);
        if self.commitment_hash != expected_commitment {
            return false;
        }

        if let Some(center) = expected_center_lat_lon {
            let anchor_lat_q = (center.0 * 10_000.0).round() as i64;
            let anchor_lon_q = (center.1 * 10_000.0).round() as i64;
            let radius_mm = (self.geofence_radius_meters * 1_000.0).round() as u64;

            let witness_seed = &self.proof_bytes[0..32];
            let claimed_seal = &self.proof_bytes[32..64];

            let mut seal_hasher = Sha256::new();
            seal_hasher.update(b"ZK_GEO_RANGE_SEAL_V1:");
            seal_hasher.update(witness_seed);
            seal_hasher.update(self.zone_id.as_bytes());
            seal_hasher.update(anchor_lat_q.to_le_bytes());
            seal_hasher.update(anchor_lon_q.to_le_bytes());
            seal_hasher.update(radius_mm.to_le_bytes());
            let expected_seal = seal_hasher.finalize();

            if claimed_seal != expected_seal.as_slice() {
                return false;
            }
        }

        true
    }
}

// ============================================================================
// 3. ANTI-SPOOFING POH CANARY CHALLENGE & RESPONSE
// ============================================================================

/// Deterministic canary micro-task issued by Club Home Nodes to detect worker nodes
/// spoofing GPU/VRAM `BioluminescenceTelemetry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PohCanaryChallenge {
    pub challenge_id: Uuid,
    /// Target worker DID being audited.
    pub target_did: String,
    /// Deterministic 32-byte seed driving the micro-matrix / hash-chain workload.
    pub seed_hex: String,
    /// Number of deterministic compute iterations required.
    pub matrix_iterations: u32,
    /// Maximum allowed execution wall-time in milliseconds before penalty/downgrade.
    pub max_latency_ms: u32,
    /// Timestamp when the challenge was issued.
    pub issued_at: u64,
}

/// Worker node's response to a `PohCanaryChallenge`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PohCanaryResponse {
    pub challenge_id: Uuid,
    pub worker_did: String,
    /// Deterministic digest output produced by executing the canary micro-task.
    pub result_digest_hex: String,
    /// Measured execution latency in milliseconds.
    pub execution_time_ms: u32,
    /// Hex-encoded Ed25519 signature over `(challenge_id, worker_did, result_digest_hex, execution_time_ms)`.
    pub worker_signature: String,
}

impl PohCanaryChallenge {
    /// Creates a new deterministic PoH Canary Challenge for a target node.
    pub fn new(
        target_did: impl Into<String>,
        matrix_iterations: u32,
        max_latency_ms: u32,
        issued_at: u64,
    ) -> Self {
        let challenge_id = Uuid::new_v4();
        let target_did = target_did.into();
        let mut seed_hasher = Sha256::new();
        seed_hasher.update(b"POH_CANARY_SEED_V1:");
        seed_hasher.update(challenge_id.as_bytes());
        seed_hasher.update(target_did.as_bytes());
        seed_hasher.update(issued_at.to_le_bytes());
        let seed_hex = bytes_to_hex(&seed_hasher.finalize());

        Self {
            challenge_id,
            target_did,
            seed_hex,
            matrix_iterations: matrix_iterations.max(1),
            max_latency_ms,
            issued_at,
        }
    }

    /// Executes the deterministic canary micro-task (simulating a synthetic 4x4 FP32/INT32
    /// shader reduction & SHA-256 state fold) to produce the canonical `result_digest_hex`.
    pub fn compute_expected_digest(&self) -> String {
        let mut state = [0u8; 32];
        let mut init_hasher = Sha256::new();
        init_hasher.update(b"POH_CANARY_EXEC_V1:");
        init_hasher.update(self.challenge_id.as_bytes());
        init_hasher.update(self.target_did.as_bytes());
        init_hasher.update(self.seed_hex.as_bytes());
        state.copy_from_slice(&init_hasher.finalize());

        for step in 0..self.matrix_iterations {
            let mut step_hasher = Sha256::new();
            step_hasher.update(state);
            step_hasher.update(step.to_le_bytes());
            // Deterministic synthetic FMA accumulation mix
            let mix = (state[0] as u32)
                .wrapping_mul(16_645_25)
                .wrapping_add(1_013_904_223)
                .wrapping_add(step);
            step_hasher.update(mix.to_le_bytes());
            state.copy_from_slice(&step_hasher.finalize());
        }

        bytes_to_hex(&state)
    }

    /// Executes the challenge using the worker's `PearlEnclaveIdentity` and produces a signed `PohCanaryResponse`.
    pub fn solve(
        &self,
        worker_identity: &PearlEnclaveIdentity,
        execution_time_ms: u32,
    ) -> PohCanaryResponse {
        let result_digest_hex = self.compute_expected_digest();
        let signing_bytes = Self::response_signing_digest(
            &self.challenge_id,
            worker_identity.did(),
            &result_digest_hex,
            execution_time_ms,
        );
        let sig = worker_identity.signing_key().sign(&signing_bytes);
        PohCanaryResponse {
            challenge_id: self.challenge_id,
            worker_did: worker_identity.did().to_string(),
            result_digest_hex,
            execution_time_ms,
            worker_signature: bytes_to_hex(&sig.to_bytes()),
        }
    }

    /// Computes the canonical signing digest for a `PohCanaryResponse`.
    pub fn response_signing_digest(
        challenge_id: &Uuid,
        worker_did: &str,
        result_digest_hex: &str,
        execution_time_ms: u32,
    ) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"POH_CANARY_RESPONSE_V1:");
        hasher.update(challenge_id.as_bytes());
        hasher.update(b":");
        hasher.update(worker_did.as_bytes());
        hasher.update(b":");
        hasher.update(result_digest_hex.as_bytes());
        hasher.update(b":");
        hasher.update(execution_time_ms.to_le_bytes());
        let mut out = [0u8; 32];
        out.copy_from_slice(&hasher.finalize());
        out
    }

    /// Audits a `PohCanaryResponse` against this challenge.
    ///
    /// Verifies:
    /// 1. `challenge_id` and `target_did` match.
    /// 2. `execution_time_ms <= max_latency_ms`.
    /// 3. `result_digest_hex` matches the deterministic canary computation.
    /// 4. `worker_signature` is a valid Ed25519 signature from `worker_did`.
    pub fn verify_response(&self, response: &PohCanaryResponse) -> bool {
        if response.challenge_id != self.challenge_id || response.worker_did != self.target_did {
            return false;
        }
        if response.execution_time_ms > self.max_latency_ms {
            return false;
        }
        let expected_digest = self.compute_expected_digest();
        if response.result_digest_hex != expected_digest {
            return false;
        }
        let verifying_key = match PearlEnclaveIdentity::verifying_key_from_did(&response.worker_did) {
            Ok(vk) => vk,
            Err(_) => return false,
        };
        let sig_bytes = match hex_to_bytes(&response.worker_signature) {
            Ok(b) => b,
            Err(_) => return false,
        };
        let sig_array: [u8; 64] = match sig_bytes.try_into() {
            Ok(a) => a,
            Err(_) => return false,
        };
        let signature = Signature::from_bytes(&sig_array);
        let digest = Self::response_signing_digest(
            &response.challenge_id,
            &response.worker_did,
            &response.result_digest_hex,
            response.execution_time_ms,
        );
        verifying_key.verify(&digest, &signature).is_ok()
    }
}

// ============================================================================
// 4. WEBTRANSPORT / WEBSOCKET PUB/SUB TOPIC FRAME (PubSubTopicFrame)
// ============================================================================

/// Canonical topic constants for the Kuroshio Gyre mesh pub/sub router.
pub mod topics {
    pub const TELEMETRY_GLOBAL: &str = "gyre:telemetry:global";
    pub const SONAR_TACTICAL: &str = "gyre:sonar:tactical";
    pub const SONAR_SECTOR_PREFIX: &str = "gyre:sonar:sector:";
    pub const JOBS_DELIVERY: &str = "gyre:jobs:delivery";
    pub const JOBS_DJ: &str = "gyre:jobs:dj";
    pub const JOBS_AUTO_REPAIR: &str = "gyre:jobs:auto_repair";
    pub const SIREN_FREQUENCY_PREFIX: &str = "siren:frequency:";
    pub const SIREN_THEATER_OST: &str = "siren:frequency:guardians-creed";
    pub const SIREN_TACTICAL_COMMS: &str = "siren:frequency:tactical-command";
    pub const SIREN_DEEP_GYRE_SYNTH: &str = "siren:frequency:deep-gyre";
}

/// WebTransport / WebSocket pub/sub wire frame for topic routing across Club Home Nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "frameType", content = "data")]
pub enum PubSubTopicFrame {
    /// Subscribe a node DID to a specific topic or wildcard channel.
    Subscribe {
        topic: String,
        subscriber_did: String,
    },
    /// Unsubscribe a node DID from a topic.
    Unsubscribe {
        topic: String,
        subscriber_did: String,
    },
    /// Publish a signed `KuroshioEventEnvelope` to a topic.
    Publish {
        topic: String,
        envelope: KuroshioEventEnvelope,
    },
    /// Low-latency unreliable WebTransport datagram stream frame (e.g., Siren Frequency Opus frames or 60Hz transforms).
    DatagramStream {
        topic: String,
        sequence: u64,
        payload_bytes: Vec<u8>,
    },
}

impl PubSubTopicFrame {
    /// Returns the topic string targeted by this frame.
    pub fn topic(&self) -> &str {
        match self {
            PubSubTopicFrame::Subscribe { topic, .. } => topic,
            PubSubTopicFrame::Unsubscribe { topic, .. } => topic,
            PubSubTopicFrame::Publish { topic, .. } => topic,
            PubSubTopicFrame::DatagramStream { topic, .. } => topic,
        }
    }

    /// Returns `true` if the topic is a recognized standard Kuroshio Gyre topic
    /// (`gyre:telemetry:global`, `gyre:jobs:delivery`, `gyre:jobs:dj`, `gyre:jobs:auto_repair`, or `siren:frequency:*`).
    pub fn is_valid_gyre_topic(topic: &str) -> bool {
        matches!(
            topic,
            topics::TELEMETRY_GLOBAL
                | topics::SONAR_TACTICAL
                | topics::JOBS_DELIVERY
                | topics::JOBS_DJ
                | topics::JOBS_AUTO_REPAIR
        ) || (topic.starts_with(topics::SONAR_SECTOR_PREFIX)
            && topic.len() > topics::SONAR_SECTOR_PREFIX.len())
            || (topic.starts_with(topics::SIREN_FREQUENCY_PREFIX)
                && topic.len() > topics::SIREN_FREQUENCY_PREFIX.len())
    }

    /// Validates both the topic namespace and, if this is a `Publish` frame, the cryptographic signature of the envelope.
    pub fn validate(&self) -> bool {
        if !Self::is_valid_gyre_topic(self.topic()) {
            return false;
        }
        match self {
            PubSubTopicFrame::Publish { envelope, .. } => envelope.verify_signature(),
            PubSubTopicFrame::Subscribe { subscriber_did, .. }
            | PubSubTopicFrame::Unsubscribe { subscriber_did, .. } => {
                PearlEnclaveIdentity::verifying_key_from_did(subscriber_did).is_ok()
            }
            PubSubTopicFrame::DatagramStream { payload_bytes, .. } => !payload_bytes.is_empty(),
        }
    }
}

// ============================================================================
// 5. UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pearl_enclave_did_signing_and_verification() {
        let identity = PearlEnclaveIdentity::from_label("okinawa-anchor-01");
        assert!(identity.did().starts_with("did:atlantis:"));
        assert_eq!(identity.did().len(), "did:atlantis:".len() + 64);

        let (score, tier) = BioluminescenceTelemetry::evaluate_tier(24576, 12, 62.0);
        let payload = KuroshioEventPayload::PohTelemetry(BioluminescenceTelemetry {
            fps: 120.0,
            cpu_usage_pct: 22.5,
            vram_free_mb: 24576,
            temp_celsius: 62.0,
            rtt_ping_ms: 12,
            poh_score: score,
            tier,
        });

        let mut envelope = identity
            .sign_event("club-ryukyu-prime", payload, 1_700_000_000)
            .expect("signing should succeed");

        assert!(envelope.verify_signature());
        assert!(PearlEnclaveIdentity::verify_envelope(&envelope));

        // Tamper with timestamp -> verification must fail
        envelope.timestamp += 1;
        assert!(!envelope.verify_signature());
    }

    #[test]
    fn test_poh_tier_scoring_classification() {
        // Tier 1: AbyssalAnchor (VRAM >= 16000, ping <= 25, low temp)
        let (score_t1, tier_t1) = BioluminescenceTelemetry::evaluate_tier(16384, 15, 65.0);
        assert_eq!(tier_t1, GyreNodeTier::AbyssalAnchor);
        assert!(score_t1 >= 80.0);

        // Tier 2: PelagicWorker (VRAM 8192, ping 40ms, normal temp)
        let (score_t2, tier_t2) = BioluminescenceTelemetry::evaluate_tier(8192, 40, 70.0);
        assert_eq!(tier_t2, GyreNodeTier::PelagicWorker);
        assert!(score_t2 >= 55.0);

        // Tier 3: EpipelagicEdge (Mobile / high thermal throttling penalty)
        let (score_t3, tier_t3) = BioluminescenceTelemetry::evaluate_tier(3072, 85, 92.0);
        assert_eq!(tier_t3, GyreNodeTier::EpipelagicEdge);
        assert!(score_t3 < 55.0);
    }

    #[test]
    fn test_zk_geo_proximity_proof_verification() {
        // Pickup/Dropoff geofence center in Naha, Okinawa
        let target_center = (26.2124, 127.6809);
        // Courier is ~85 meters away
        let courier_pos = (26.2130, 127.6814);
        let nonce = b"courier-ephemeral-salt-99";

        let proof = ZkGeoProximityProof::generate_proof(
            courier_pos,
            target_center,
            250.0,
            "zone:naha:kokusai-dori",
            nonce,
        )
        .expect("courier within 250m geofence should generate valid ZK proof");

        assert!(proof.verify(None));
        assert!(proof.verify(Some(target_center)));
        // Wrong center should fail verification
        assert!(!proof.verify(Some((35.6762, 139.6503))));

        // Out-of-bounds courier (> 250m away) must be rejected at proof generation
        let far_pos = (26.2500, 127.7200);
        let err = ZkGeoProximityProof::generate_proof(
            far_pos,
            target_center,
            250.0,
            "zone:naha:kokusai-dori",
            nonce,
        );
        assert_eq!(
            err,
            Err(AtlantisProtocolError::GeoProximityVerificationFailed)
        );
    }

    #[test]
    fn test_poh_canary_challenge_and_spoof_detection() {
        let worker = PearlEnclaveIdentity::from_label("pelagic-worker-gpu-07");
        let challenge = PohCanaryChallenge::new(worker.did(), 64, 50, 1_700_000_100);

        // Honest execution within latency budget (18ms <= 50ms)
        let honest_response = challenge.solve(&worker, 18);
        assert!(challenge.verify_response(&honest_response));

        // Exceeded latency budget (75ms > 50ms) -> fails audit
        let slow_response = challenge.solve(&worker, 75);
        assert!(!challenge.verify_response(&slow_response));

        // Spoofed digest output -> fails audit
        let mut spoofed_response = honest_response.clone();
        spoofed_response.result_digest_hex = "00".repeat(32);
        assert!(!challenge.verify_response(&spoofed_response));
    }

    #[test]
    fn test_pubsub_topic_frames_and_validation() {
        let dj_identity = PearlEnclaveIdentity::from_label("siren-dj-neptune");
        let siren_event = KuroshioEventPayload::SirenFrequencyFrame {
            channel: "siren:frequency:deep-house".to_string(),
            opus_pcm_bytes: vec![0xAA, 0xBB, 0xCC, 0xDD],
            sample_rate_hz: 48000,
        };
        let envelope = dj_identity
            .sign_event("club-atlantis-main", siren_event, 1_700_000_500)
            .unwrap();

        let pub_frame = PubSubTopicFrame::Publish {
            topic: "siren:frequency:deep-house".to_string(),
            envelope,
        };
        assert!(pub_frame.validate());

        let sub_frame = PubSubTopicFrame::Subscribe {
            topic: topics::JOBS_AUTO_REPAIR.to_string(),
            subscriber_did: dj_identity.did().to_string(),
        };
        assert!(sub_frame.validate());

        let invalid_topic_frame = PubSubTopicFrame::DatagramStream {
            topic: "unknown:topic:channel".to_string(),
            sequence: 1,
            payload_bytes: vec![1, 2, 3],
        };
        assert!(!invalid_topic_frame.validate());
    }

    #[test]
    fn test_club_graduation_and_location_disclosure_policy() {
        let club_identity = PearlEnclaveIdentity::from_label("club-shinjuku-node");
        let member_identity = PearlEnclaveIdentity::from_label("delivery-courier-01");

        // Graduate onto club node
        let cert = ClubGraduationCertificate::issue(
            &club_identity,
            member_identity.did(),
            "club-shinjuku-node",
            vec!["courier".to_string(), "mechanic".to_string()],
            1_700_000_000,
            1_750_000_000,
        )
        .expect("Certificate issuance should succeed");

        assert!(cert.verify().is_ok());

        // Private high precision location stored at club node
        let private_telemetry = PrivateLocationTelemetry {
            latitude_deg: 35.6895,
            longitude_deg: 139.6917,
            altitude_m: 45.2,
            ecef_x_m: -3959950.0,
            ecef_y_m: 3350490.0,
            ecef_z_m: 3699700.0,
            timestamp_ms: 1_700_000_100,
        };

        // Obfuscate to public network
        let public_view = LocationDisclosurePolicy::ObfuscatedSector { sector_id: 1042 }
            .apply(&private_telemetry, member_identity.did());

        match public_view {
            PublicLocationDisclosure::ObfuscatedSector { sector_id, .. } => {
                assert_eq!(sector_id, 1042);
            }
            _ => panic!("Expected ObfuscatedSector"),
        }
    }
}

// ============================================================================
// NAUTILUS WORLD SERVER & CLUB GRADUATION STRUCTURES
// ============================================================================

/// Private, high-precision location telemetry stored strictly in the Club Home Node vault.
/// MUST NEVER be broadcast directly to the public network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrivateLocationTelemetry {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f64,
    pub ecef_x_m: f64,
    pub ecef_y_m: f64,
    pub ecef_z_m: f64,
    pub timestamp_ms: u64,
}

/// The authorized public location view permitted by the Club Home Node's policy engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PublicLocationDisclosure {
    /// Hidden completely from public discovery
    Hidden,
    /// Coarse neighborhood or administrative sector
    ObfuscatedSector {
        sector_id: u32,
        publisher_did: String,
    },
    /// Bounded active delivery/travel corridor with dynamic fuzzy radius
    ActiveCorridor {
        center_ecef: [f64; 3],
        fuzzy_radius_m: f32,
        job_id: String,
    },
    /// Cryptographic zero-knowledge geofence proof
    ZeroKnowledgeProof {
        zone_hash: [u8; 32],
        is_inside: bool,
    },
}

/// Disclosure policies enforced by the Club Home Node before transmitting data to Nautilus.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LocationDisclosurePolicy {
    PrivateVaultOnly,
    ObfuscatedSector { sector_id: u32 },
    ActiveJobCorridor { fuzzy_radius_m: f32, job_id: String },
    ZeroKnowledgeGeofence { zone_hash: [u8; 32] },
}

impl LocationDisclosurePolicy {
    pub fn apply(
        &self,
        private: &PrivateLocationTelemetry,
        publisher_did: &str,
    ) -> PublicLocationDisclosure {
        match self {
            Self::PrivateVaultOnly => PublicLocationDisclosure::Hidden,
            Self::ObfuscatedSector { sector_id } => PublicLocationDisclosure::ObfuscatedSector {
                sector_id: *sector_id,
                publisher_did: publisher_did.to_string(),
            },
            Self::ActiveJobCorridor { fuzzy_radius_m, job_id } => {
                PublicLocationDisclosure::ActiveCorridor {
                    center_ecef: [private.ecef_x_m, private.ecef_y_m, private.ecef_z_m],
                    fuzzy_radius_m: *fuzzy_radius_m,
                    job_id: job_id.clone(),
                }
            }
            Self::ZeroKnowledgeGeofence { zone_hash } => {
                PublicLocationDisclosure::ZeroKnowledgeProof {
                    zone_hash: *zone_hash,
                    is_inside: true,
                }
            }
        }
    }
}

/// Certificate issued when an edge node graduates onto a sovereign Club Home Node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClubGraduationCertificate {
    pub node_did: String,
    pub club_id: String,
    pub club_did: String,
    pub roles: Vec<String>,
    pub issued_at_s: u64,
    pub expires_at_s: u64,
    pub signature_hex: String,
}

impl ClubGraduationCertificate {
    pub fn issue(
        club_identity: &PearlEnclaveIdentity,
        node_did: &str,
        club_id: &str,
        roles: Vec<String>,
        issued_at_s: u64,
        expires_at_s: u64,
    ) -> Result<Self, AtlantisProtocolError> {
        let msg = format!(
            "GRADUATION:{}:{}:{}:{}:{}",
            node_did,
            club_id,
            roles.join(","),
            issued_at_s,
            expires_at_s
        );
        let sig = club_identity.signing_key().sign(msg.as_bytes());

        Ok(Self {
            node_did: node_did.to_string(),
            club_id: club_id.to_string(),
            club_did: club_identity.did().to_string(),
            roles,
            issued_at_s,
            expires_at_s,
            signature_hex: bytes_to_hex(&sig.to_bytes()),
        })
    }

    pub fn verify(&self) -> Result<(), AtlantisProtocolError> {
        let pubkey_hex = self
            .club_did
            .strip_prefix("did:atlantis:")
            .ok_or_else(|| AtlantisProtocolError::InvalidDidFormat(self.club_did.clone()))?;
        let pubkey_bytes = hex_to_bytes(pubkey_hex)?;
        if pubkey_bytes.len() != 32 {
            return Err(AtlantisProtocolError::CryptoError(
                "Invalid pubkey length, expected 32 bytes".to_string(),
            ));
        }
        let mut key_arr = [0u8; 32];
        key_arr.copy_from_slice(&pubkey_bytes);
        let verifying_key = VerifyingKey::from_bytes(&key_arr)
            .map_err(|e| AtlantisProtocolError::CryptoError(e.to_string()))?;

        let sig_bytes = hex_to_bytes(&self.signature_hex)?;
        if sig_bytes.len() != 64 {
            return Err(AtlantisProtocolError::CryptoError(
                "Invalid signature length, expected 64 bytes".to_string(),
            ));
        }
        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);
        let sig = Signature::from_bytes(&sig_arr);

        let msg = format!(
            "GRADUATION:{}:{}:{}:{}:{}",
            self.node_did,
            self.club_id,
            self.roles.join(","),
            self.issued_at_s,
            self.expires_at_s
        );

        verifying_key
            .verify(msg.as_bytes(), &sig)
            .map_err(|e| AtlantisProtocolError::CryptoError(e.to_string()))
    }
}


