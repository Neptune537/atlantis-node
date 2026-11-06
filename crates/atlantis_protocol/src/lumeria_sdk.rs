//! # Atlantis Lumeria App SDK & Sandboxed Plugin Contracts
//!
//! Defines the capability-based security model, permission manifests,
//! and WebAssembly plugin traits for third-party Lumeria 3D widgets, games,
//! and storefront extensions, backed by .nautpak hardware DRM and ai_gate price guards.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Declared capability permissions that a third-party Lumeria WASM application can request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppCapability {
    /// Access to coarse/masked regional geohash (Never raw GPS).
    LocationCoarse,
    /// Permission to send encrypted peer-to-peer chat packets over Kuroshio mesh.
    NetworkP2pChat,
    /// Permission to overlay 2D/3D spatial UI HUD panels into the Lumeria viewport.
    UiOverlay,
    /// Read-only access to local merchant storefront catalog.
    StoreCatalogReadOnly,
    /// Authority to initiate micro-escrow cart checkout requests through ai_gate Price Guard.
    StoreInitiateCheckout,
    /// Access to spatial audio Sabine RT60 acoustic environmental nodes.
    AudioSpatialStream,
    /// Access to vehicle telemetry (Pacejka slip, RPM, speed) for car club apps.
    VehicleTelemetryRead,
}

/// Verification status of a third-party application manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppSecurityTier {
    /// Sandboxed guest with strictly limited capabilities.
    SandboxedGuest,
    /// Club-verified application signed by a recognized Club Home Node DID.
    ClubVerified,
    /// Core sovereign module with full local node integration.
    CoreSovereign,
}

/// The declarative application manifest (`app.manifest.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppManifest {
    /// Unique bundle ID (e.g. `com.kage.carclub_streamer`, `com.atlantis.store_front`).
    pub app_id: String,
    /// Human-readable application name.
    pub name: String,
    /// Semver string (e.g. `1.0.0`).
    pub version: String,
    /// Publisher DID (e.g. `did:atlantis:6a8f...`).
    pub publisher_did: String,
    /// Requested capability set.
    pub capabilities: HashSet<AppCapability>,
    /// Cryptographic signature of the WASM bytecode package by the publisher.
    pub package_signature: String,
    /// Security tier granted after verification.
    pub security_tier: AppSecurityTier,
}

impl AppManifest {
    /// Checks if a required capability is granted by this manifest.
    pub fn has_capability(&self, cap: &AppCapability) -> bool {
        self.capabilities.contains(cap)
    }

    /// Verifies that no high-risk capabilities are requested without proper security tier.
    pub fn validate_permissions(&self) -> Result<(), String> {
        if self.capabilities.contains(&AppCapability::StoreInitiateCheckout)
            && self.security_tier == AppSecurityTier::SandboxedGuest
        {
            return Err("Capability StoreInitiateCheckout requires ClubVerified or higher tier".into());
        }
        Ok(())
    }
}

/// Specification for .nautpak DRM encrypted 3D/AI assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NautpakDrmHeader {
    /// Magic identifier (e.g. `NAUTPAK1`).
    pub magic: [u8; 8],
    /// Asset content hash (SHA-256).
    pub content_hash: [u8; 32],
    /// Asset type (0 = Gaussian Splat, 1 = GLB Mesh, 2 = AI Model Weights, 3 = Spatial Audio Pack).
    pub asset_type: u8,
    /// 12-byte initialization vector (ChaCha20-Poly1305).
    pub nonce: [u8; 12],
    /// Size of the encrypted payload in bytes.
    pub payload_size_bytes: u64,
    /// Target GPU buffer format when decrypted directly into VRAM (e.g. `R32G32B32A32_SFLOAT`).
    pub gpu_target_format: String,
}

/// Input event passed into the sandboxed LumeriaApp WASM guest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LumeriaInputEvent {
    /// 3D Viewport pointer click or tap at normalized coordinates [x, y].
    PointerDown { x: f32, y: f32 },
    /// VR Controller or 6-DoF raycast interaction.
    RaycastHit { hit_point: [f32; 3], normal: [f32; 3] },
    /// Spatial proximity trigger (e.g. avatar walked within 2.0m of storefront totem).
    ProximityTrigger { distance_m: f32, entity_id: String },
    /// Custom message frame received over authorized P2P channel.
    P2pMessage { sender_did: String, payload: Vec<u8> },
}

/// Output render and execution commands emitted by the sandboxed LumeriaApp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LumeriaUiCommand {
    /// Request to render a 2D floating UI panel in the 3D scene.
    RenderSpatialPanel {
        panel_id: String,
        title: String,
        dimensions: [f32; 2],
        position: [f32; 3],
    },
    /// Request to stream an audio clip via the Sabine RT60 acoustic engine.
    PlaySpatialAudio {
        audio_uri: String,
        position: [f32; 3],
        gain: f32,
    },
    /// Submit cart checkout through ai_gate Price Guard.
    SubmitCartOrder {
        store_node_did: String,
        item_id: String,
        unit_price_cyan: u64,
        quantity: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_permission_validation() {
        let mut caps = HashSet::new();
        caps.insert(AppCapability::LocationCoarse);
        caps.insert(AppCapability::UiOverlay);

        let manifest = AppManifest {
            app_id: "com.kage.carclub_streamer".into(),
            name: "KAGE Car Club Live Streamer".into(),
            version: "0.1.0".into(),
            publisher_did: "did:atlantis:6a8f11223344556677889900aabbccddeeff".into(),
            capabilities: caps,
            package_signature: "sig_ed25519_verified".into(),
            security_tier: AppSecurityTier::SandboxedGuest,
        };

        assert!(manifest.has_capability(&AppCapability::LocationCoarse));
        assert!(!manifest.has_capability(&AppCapability::StoreInitiateCheckout));
        assert!(manifest.validate_permissions().is_ok());

        // Now test privilege escalation attempt: Sandboxed guest requesting StoreInitiateCheckout
        let mut rogue_caps = HashSet::new();
        rogue_caps.insert(AppCapability::StoreInitiateCheckout);

        let rogue_manifest = AppManifest {
            app_id: "com.rogue.checkout_bypass".into(),
            name: "Rogue Checkout".into(),
            version: "0.1.0".into(),
            publisher_did: "did:atlantis:badactor".into(),
            capabilities: rogue_caps,
            package_signature: "sig_fake".into(),
            security_tier: AppSecurityTier::SandboxedGuest,
        };

        assert!(rogue_manifest.validate_permissions().is_err());
    }
}
