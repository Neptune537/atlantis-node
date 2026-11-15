//! Authoritative Pure-Rust Mesh, Persistence & 24/7 Broadcast Server (`kuroshio-server`).
//!
//! Replaces Node.js/Python runtime overhead with:
//! - Binary `#[repr(C)]` `PackedServerTelemetryFrame` and `SpatialAoiFilter` (zero JSON on hot paths)
//! - ACID `rusqlite` persistence for Club Atlantis storefront inventory, atomic checkouts,
//!   and the 100-tier dungeon progression registry (`ClubAtlantisStore`)
//! - Pure-Rust MP3 ID3v2 frame stripping, equal-power PCM crossfading, sidechain voice-over ducking,
//!   and 4-Act `SavedEpisode` compilation (`PureRustRadioEngine`)
//! - `axum` HTTP/WebSocket-ready router exposing `/live.mp3`, `/media/latest_reel.mp4`,
//!   `/api/status`, and `/api/aoi_binary`

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::path::Path;
use std::sync::{Arc, Mutex};

use atlantis_protocol::{
    EpisodeHardwareProfile, EpisodeMediaChunk, EpisodeStateDelta, NarrativeAct, NarrativeBlock,
    SavedEpisode, SpatialStageDirection,
};
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

/// Binary zero-JSON telemetry frame mirroring `lumeria_app_sdk::PackedTelemetryView` (`72` bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PackedServerTelemetryFrame {
    pub entity_id: u64,
    pub ecef_pos: [f64; 3],
    pub cam_rel_pos: [f32; 3],
    pub velocity_mps: [f32; 3],
    pub siren_freq_hz: f32,
    pub fader_pos: f32,
    pub dungeon_floor: u32,
    pub flags: u32,
}

impl PackedServerTelemetryFrame {
    /// Fixed wire size of a `PackedServerTelemetryFrame` in bytes.
    pub const BYTE_LEN: usize = 72;

    /// Serializes into a fixed-size little-endian 72-byte buffer without heap allocation.
    pub fn encode_le_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut out = [0u8; Self::BYTE_LEN];
        out[0..8].copy_from_slice(&self.entity_id.to_le_bytes());
        out[8..16].copy_from_slice(&self.ecef_pos[0].to_le_bytes());
        out[16..24].copy_from_slice(&self.ecef_pos[1].to_le_bytes());
        out[24..32].copy_from_slice(&self.ecef_pos[2].to_le_bytes());
        out[32..36].copy_from_slice(&self.cam_rel_pos[0].to_le_bytes());
        out[36..40].copy_from_slice(&self.cam_rel_pos[1].to_le_bytes());
        out[40..44].copy_from_slice(&self.cam_rel_pos[2].to_le_bytes());
        out[44..48].copy_from_slice(&self.velocity_mps[0].to_le_bytes());
        out[48..52].copy_from_slice(&self.velocity_mps[1].to_le_bytes());
        out[52..56].copy_from_slice(&self.velocity_mps[2].to_le_bytes());
        out[56..60].copy_from_slice(&self.siren_freq_hz.to_le_bytes());
        out[60..64].copy_from_slice(&self.fader_pos.to_le_bytes());
        out[64..68].copy_from_slice(&self.dungeon_floor.to_le_bytes());
        out[68..72].copy_from_slice(&self.flags.to_le_bytes());
        out
    }

    /// Decodes a `PackedServerTelemetryFrame` from a 72-byte little-endian slice.
    pub fn decode_le_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::BYTE_LEN {
            return None;
        }
        let read_f64 = |start: usize| -> f64 {
            f64::from_le_bytes(bytes[start..start + 8].try_into().unwrap())
        };
        let read_f32 = |start: usize| -> f32 {
            f32::from_le_bytes(bytes[start..start + 4].try_into().unwrap())
        };
        let read_u32 = |start: usize| -> u32 {
            u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap())
        };
        Some(Self {
            entity_id: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
            ecef_pos: [read_f64(8), read_f64(16), read_f64(24)],
            cam_rel_pos: [read_f32(32), read_f32(36), read_f32(40)],
            velocity_mps: [read_f32(44), read_f32(48), read_f32(52)],
            siren_freq_hz: read_f32(56),
            fader_pos: read_f32(60),
            dungeon_floor: read_u32(64),
            flags: read_u32(68),
        })
    }
}

/// Spatial Area-of-Interest (AOI) filter over 64-bit WGS84 ECEF coordinates.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpatialAoiFilter;

impl SpatialAoiFilter {
    /// Returns entities within `radius_meters` Euclidean ECEF distance of `observer_ecef`.
    pub fn filter_within_radius_ecef(
        observer_ecef: [f64; 3],
        entities: &[PackedServerTelemetryFrame],
        radius_meters: f64,
    ) -> Vec<PackedServerTelemetryFrame> {
        let r2 = radius_meters.max(0.0) * radius_meters.max(0.0);
        entities
            .iter()
            .copied()
            .filter(|ent| {
                let dx = ent.ecef_pos[0] - observer_ecef[0];
                let dy = ent.ecef_pos[1] - observer_ecef[1];
                let dz = ent.ecef_pos[2] - observer_ecef[2];
                dx * dx + dy * dy + dz * dz <= r2
            })
            .collect()
    }

    /// Packs a slice of `PackedServerTelemetryFrame` structs into a contiguous binary payload.
    pub fn encode_batch_binary(frames: &[PackedServerTelemetryFrame]) -> Vec<u8> {
        let mut out = Vec::with_capacity(frames.len() * PackedServerTelemetryFrame::BYTE_LEN);
        for frame in frames {
            out.extend_from_slice(&frame.encode_le_bytes());
        }
        out
    }

    /// Decodes a packed binary payload back into `PackedServerTelemetryFrame` structs.
    pub fn decode_batch_binary(bytes: &[u8]) -> Option<Vec<PackedServerTelemetryFrame>> {
        if bytes.len() % PackedServerTelemetryFrame::BYTE_LEN != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(bytes.len() / PackedServerTelemetryFrame::BYTE_LEN);
        for chunk in bytes.chunks_exact(PackedServerTelemetryFrame::BYTE_LEN) {
            out.push(PackedServerTelemetryFrame::decode_le_bytes(chunk)?);
        }
        Some(out)
    }
}

/// Receipt emitted after an ACID storefront checkout commit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckoutReceipt {
    pub order_id: String,
    pub buyer_did: String,
    pub sku: String,
    pub quantity: i64,
    pub remaining_stock: i64,
    pub total_cyan: f64,
}

/// ACID SQLite store (`rusqlite`) for Club Atlantis shelf inventory, checkout orders,
/// and persistent 100-tier dungeon state.
pub struct ClubAtlantisStore {
    conn: Mutex<Connection>,
}

impl ClubAtlantisStore {
    /// Creates an in-memory ACID store (ideal for deterministic unit tests and ephemeral edge workers).
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_schema(conn)
    }

    /// Opens or creates an on-disk SQLite database with WAL durability.
    pub fn open_path(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");
        Self::init_schema(conn)
    }

    fn init_schema(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS shelf_inventory (
                sku TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                stock INTEGER NOT NULL CHECK (stock >= 0),
                unit_price_cyan REAL NOT NULL CHECK (unit_price_cyan >= 0.0)
            );
            CREATE TABLE IF NOT EXISTS checkout_orders (
                order_id TEXT PRIMARY KEY,
                buyer_did TEXT NOT NULL,
                sku TEXT NOT NULL,
                quantity INTEGER NOT NULL CHECK (quantity > 0),
                total_cyan REAL NOT NULL,
                created_epoch_secs INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS dungeon_tiers (
                tier INTEGER PRIMARY KEY CHECK (tier >= 1 AND tier <= 100),
                theme TEXT NOT NULL,
                boss_name TEXT NOT NULL,
                cleared INTEGER NOT NULL DEFAULT 0,
                cleared_by_did TEXT
            );
            ",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Seeds all 100 dungeon tiers (`1..=100`) idempotently within a single transaction.
    pub fn seed_100_tier_dungeon(&self) -> rusqlite::Result<usize> {
        let mut conn = self.conn.lock().expect("sqlite mutex");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO dungeon_tiers (tier, theme, boss_name, cleared, cleared_by_did)
                 VALUES (?1, ?2, ?3, 0, NULL)",
            )?;
            for tier in 1..=100u32 {
                let theme = match tier {
                    1..=25 => "Abyssal Kelp Trench",
                    26..=50 => "Sunken Torii Vault",
                    51..=75 => "Siren Resonance Foundry",
                    _ => "Celestial Seraph Citadel",
                };
                let boss = format!("Tier-{tier:03} Warden");
                stmt.execute(params![tier, theme, boss])?;
            }
        }
        tx.commit()?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM dungeon_tiers", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Inserts or updates a physical/digital SKU on a Club Atlantis storefront shelf.
    pub fn upsert_shelf_item(
        &self,
        sku: &str,
        title: &str,
        stock: i64,
        unit_price_cyan: f64,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().expect("sqlite mutex");
        conn.execute(
            "INSERT INTO shelf_inventory (sku, title, stock, unit_price_cyan)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(sku) DO UPDATE SET
               title = excluded.title,
               stock = excluded.stock,
               unit_price_cyan = excluded.unit_price_cyan",
            params![sku, title, stock, unit_price_cyan],
        )?;
        Ok(())
    }

    /// Records clear progression for a specific dungeon tier (`1..=100`).
    pub fn record_tier_progress(
        &self,
        tier: u32,
        cleared: bool,
        cleared_by_did: Option<&str>,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().expect("sqlite mutex");
        conn.execute(
            "UPDATE dungeon_tiers SET cleared = ?2, cleared_by_did = ?3 WHERE tier = ?1",
            params![tier, if cleared { 1i64 } else { 0i64 }, cleared_by_did],
        )?;
        Ok(())
    }

    /// Returns total cleared dungeon tiers across `1..=100`.
    pub fn cleared_tier_count(&self) -> rusqlite::Result<u32> {
        let conn = self.conn.lock().expect("sqlite mutex");
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM dungeon_tiers WHERE cleared = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(count as u32)
    }

    /// Executes an ACID-atomic shelf checkout: checks positive quantity and sufficient stock,
    /// decrements `shelf_inventory`, and inserts the receipt row in one transaction.
    pub fn atomic_checkout(
        &self,
        order_id: &str,
        buyer_did: &str,
        sku: &str,
        quantity: i64,
        now_secs: i64,
    ) -> Result<CheckoutReceipt, String> {
        if quantity <= 0 {
            return Err("quantity must be positive".to_string());
        }
        let mut conn = self.conn.lock().expect("sqlite mutex");
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| format!("begin tx failed: {e}"))?;

        let row: Option<(i64, f64)> = tx
            .query_row(
                "SELECT stock, unit_price_cyan FROM shelf_inventory WHERE sku = ?1",
                params![sku],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| format!("select sku failed: {e}"))?;

        let (stock, unit_price_cyan) = row.ok_or_else(|| format!("unknown sku: {sku}"))?;
        if stock < quantity {
            return Err(format!(
                "insufficient stock for {sku}: requested {quantity}, available {stock}"
            ));
        }

        let remaining_stock = stock - quantity;
        let total_cyan = unit_price_cyan * (quantity as f64);

        tx.execute(
            "UPDATE shelf_inventory SET stock = ?2 WHERE sku = ?1",
            params![sku, remaining_stock],
        )
        .map_err(|e| format!("update stock failed: {e}"))?;

        tx.execute(
            "INSERT INTO checkout_orders (order_id, buyer_did, sku, quantity, total_cyan, created_epoch_secs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![order_id, buyer_did, sku, quantity, total_cyan, now_secs],
        )
        .map_err(|e| format!("insert order failed: {e}"))?;

        tx.commit().map_err(|e| format!("commit failed: {e}"))?;

        Ok(CheckoutReceipt {
            order_id: order_id.to_string(),
            buyer_did: buyer_did.to_string(),
            sku: sku.to_string(),
            quantity,
            remaining_stock,
            total_cyan,
        })
    }
}

/// Pure-Rust 24/7 broadcast & DSP primitives replacing Python transition helpers.
#[derive(Debug, Clone, Copy, Default)]
pub struct PureRustRadioEngine;

impl PureRustRadioEngine {
    /// Strips leading ID3v2 metadata tags (and optional 10-byte footer) from concatenated MP3 buffers
    /// so browsers and HTTP audio players never stumble on mid-stream ID3 headers.
    pub fn skip_id3v2_header(mp3_bytes: &[u8]) -> &[u8] {
        if mp3_bytes.len() < 10 || &mp3_bytes[0..3] != b"ID3" {
            return mp3_bytes;
        }
        let flags = mp3_bytes[5];
        let tag_size = (((mp3_bytes[6] & 0x7F) as usize) << 21)
            | (((mp3_bytes[7] & 0x7F) as usize) << 14)
            | (((mp3_bytes[8] & 0x7F) as usize) << 7)
            | ((mp3_bytes[9] & 0x7F) as usize);
        let footer_size = if (flags & 0x10) != 0 { 10 } else { 0 };
        let skip = (10 + tag_size + footer_size).min(mp3_bytes.len());
        &mp3_bytes[skip..]
    }

    /// Performs a seamless equal-power (`cos`/`sin`) overlap crossfade between `outgoing` and `incoming`
    /// PCM buffers, maintaining constant acoustic energy (`cos^2 + sin^2 = 1`) with zero silence dip.
    pub fn equal_power_crossfade_pcm(
        outgoing: &[f32],
        incoming: &[f32],
        crossfade_samples: usize,
    ) -> Vec<f32> {
        let overlap = crossfade_samples.min(outgoing.len()).min(incoming.len());
        if overlap == 0 {
            let mut joined = Vec::with_capacity(outgoing.len() + incoming.len());
            joined.extend_from_slice(outgoing);
            joined.extend_from_slice(incoming);
            return joined;
        }

        let head_len = outgoing.len() - overlap;
        let tail_len = incoming.len() - overlap;
        let mut out = Vec::with_capacity(head_len + overlap + tail_len);
        out.extend_from_slice(&outgoing[..head_len]);

        for i in 0..overlap {
            let t = if overlap > 1 {
                i as f32 / (overlap - 1) as f32
            } else {
                0.5
            };
            let fade_out = (t * FRAC_PI_2).cos();
            let fade_in = (t * FRAC_PI_2).sin();
            out.push(outgoing[head_len + i] * fade_out + incoming[i] * fade_in);
        }

        out.extend_from_slice(&incoming[overlap..]);
        out
    }

    /// Sidechain ducks a `music_bed` PCM buffer smoothly toward `duck_gain` whenever
    /// `speech_mask` amplitude reaches `threshold`.
    pub fn sidechain_duck_bed_pcm(
        music_bed: &[f32],
        speech_mask: &[f32],
        duck_gain: f32,
        threshold: f32,
    ) -> Vec<f32> {
        let target_duck = duck_gain.clamp(0.05, 1.0);
        let thresh = threshold.abs();
        let mut current_gain = 1.0_f32;
        let mut out = Vec::with_capacity(music_bed.len());

        for (idx, &sample) in music_bed.iter().enumerate() {
            let speech_lvl = speech_mask.get(idx).copied().unwrap_or(0.0).abs();
            let desired = if speech_lvl >= thresh {
                target_duck
            } else {
                1.0
            };
            let coeff = if desired < current_gain { 0.35 } else { 0.08 };
            current_gain += (desired - current_gain) * coeff;
            out.push(sample * current_gain);
        }
        out
    }

    /// Compiles a canonical 4-Act `atlantis_protocol::SavedEpisode` (`Act1Intro` through `Act4Resolution`)
    /// directly in Pure Rust without external Python/Node glue.
    pub fn compile_four_act_episode(episode_id: &str, title: &str, speaker: &str) -> SavedEpisode {
        let acts = [
            (
                NarrativeAct::Act1Intro,
                "Act I: Siren Horizon",
                "Kenji opens the 432 Hz horizon sweep over the Kuroshio sector.",
                0.0,
            ),
            (
                NarrativeAct::Act2Complication,
                "Act II: Thermal Breach",
                "Karman storm telemetry flags an anomalous trench reading.",
                900.0,
            ),
            (
                NarrativeAct::Act3Climax,
                "Act III: Guild Convergence",
                "Fleet shields lock into ECEF coordinates as the orbital pulse peaks.",
                1800.0,
            ),
            (
                NarrativeAct::Act4Resolution,
                "Act IV: Tide Settlement",
                "Coral escrows settle and Maia restores harbor ambient lock.",
                2700.0,
            ),
        ];

        let narrative_blocks = acts
            .into_iter()
            .enumerate()
            .map(|(i, (act, block_title, synopsis, offset))| NarrativeBlock {
                block_id: format!("{episode_id}_act_{}", i + 1),
                act,
                title: block_title.to_string(),
                synopsis: synopsis.to_string(),
                media_chunks: vec![EpisodeMediaChunk {
                    chunk_id: format!("{episode_id}_chunk_{}", i + 1),
                    start_offset_secs: offset,
                    duration_secs: 900.0,
                    media_uri: format!("/live.mp3?act={}", i + 1),
                    stem_type: "vocal".to_string(),
                    bitrate_kbps: 192,
                }],
                stage_directions: vec![SpatialStageDirection {
                    actor_id: speaker.to_string(),
                    target_position: [0.0, 1.6, -2.5],
                    target_rotation: [0.0, 0.0, 0.0, 1.0],
                    motion_prompt: format!("broadcast_act_{}", i + 1),
                    audio_gain: 1.0,
                }],
                state_deltas: vec![EpisodeStateDelta {
                    target_did: speaker.to_string(),
                    cyan_amount: 10,
                    unlocked_flags: vec![format!("act_{}_completed", i + 1)],
                    asset_voucher_id: None,
                }],
            })
            .collect();

        let mut metadata = HashMap::new();
        metadata.insert("engine".to_string(), "pure-rust-kuroshio".to_string());
        metadata.insert("station".to_string(), "seven-daughters-432hz".to_string());

        SavedEpisode {
            episode_id: episode_id.to_string(),
            title: title.to_string(),
            created_at: "2026-11-15T00:00:00Z".to_string(),
            total_duration_secs: 3600.0,
            hardware_profile: EpisodeHardwareProfile::default(),
            narrative_blocks,
            metadata,
        }
    }
}

/// Shared runtime state for `build_kuroshio_axum_router`.
pub struct KuroshioServerState {
    pub store: ClubAtlantisStore,
    pub live_mp3_buffer: Mutex<Vec<u8>>,
    pub latest_reel_mp4: Mutex<Vec<u8>>,
    pub tracked_entities: Mutex<Vec<PackedServerTelemetryFrame>>,
}

impl KuroshioServerState {
    /// Constructs an in-memory server state pre-seeded with 100 dungeon tiers.
    pub fn in_memory() -> rusqlite::Result<Self> {
        let store = ClubAtlantisStore::open_in_memory()?;
        store.seed_100_tier_dungeon()?;
        Ok(Self {
            store,
            live_mp3_buffer: Mutex::new(vec![0xFF, 0xFB, 0x90, 0x64]),
            latest_reel_mp4: Mutex::new(Vec::new()),
            tracked_entities: Mutex::new(Vec::new()),
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct AoiQuery {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
    pub radius: Option<f64>,
}

async fn get_live_mp3(State(state): State<Arc<KuroshioServerState>>) -> impl IntoResponse {
    let raw = state.live_mp3_buffer.lock().expect("mp3 mutex").clone();
    let stripped = PureRustRadioEngine::skip_id3v2_header(&raw).to_vec();
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("audio/mpeg"),
        )],
        Body::from(stripped),
    )
}

async fn get_latest_reel_mp4(State(state): State<Arc<KuroshioServerState>>) -> impl IntoResponse {
    let raw = state.latest_reel_mp4.lock().expect("reel mutex").clone();
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("video/mp4"),
        )],
        Body::from(raw),
    )
}

async fn get_api_status(State(state): State<Arc<KuroshioServerState>>) -> impl IntoResponse {
    let cleared_tiers = state.store.cleared_tier_count().unwrap_or(0);
    let tracked_count = state.tracked_entities.lock().expect("entities mutex").len();
    Json(serde_json::json!({
        "engine": "pure-rust",
        "station": "seven-daughters-432hz",
        "cleared_tiers": cleared_tiers,
        "tracked_entities": tracked_count,
    }))
}

async fn get_aoi_binary(
    State(state): State<Arc<KuroshioServerState>>,
    Query(q): Query<AoiQuery>,
) -> impl IntoResponse {
    let observer = [q.x.unwrap_or(0.0), q.y.unwrap_or(0.0), q.z.unwrap_or(0.0)];
    let radius = q.radius.unwrap_or(10_000_000.0);
    let entities = state.tracked_entities.lock().expect("entities mutex");
    let visible = SpatialAoiFilter::filter_within_radius_ecef(observer, &entities, radius);
    let bytes = SpatialAoiFilter::encode_batch_binary(&visible);
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        )],
        Body::from(bytes),
    )
}

/// Builds the Pure-Rust `axum::Router` for `/live.mp3`, `/media/latest_reel.mp4`,
/// `/api/status`, and `/api/aoi_binary`.
pub fn build_kuroshio_axum_router(state: Arc<KuroshioServerState>) -> Router {
    Router::new()
        .route("/live.mp3", get(get_live_mp3))
        .route("/media/latest_reel.mp4", get(get_latest_reel_mp4))
        .route("/api/status", get(get_api_status))
        .route("/api/aoi_binary", get(get_aoi_binary))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id3v2_header_stripping() {
        // Construct fake ID3v2.3 header of synchsafe size 4 (0x00, 0x00, 0x00, 0x04)
        let mut buf = b"ID3\x03\x00\x00\x00\x00\x00\x04TAGS\xFF\xFB\x90\x64".to_vec();
        let stripped = PureRustRadioEngine::skip_id3v2_header(&buf);
        assert_eq!(stripped, &[0xFF, 0xFB, 0x90, 0x64]);

        // Un-tagged MP3 stream passes through unchanged
        buf = vec![0xFF, 0xFB, 0x90, 0x64, 0x11, 0x22];
        assert_eq!(PureRustRadioEngine::skip_id3v2_header(&buf), &buf[..]);
    }

    #[test]
    fn test_equal_power_crossfade_no_silence_dip() {
        let outgoing = vec![1.0_f32; 64];
        let incoming = vec![1.0_f32; 64];
        let mixed = PureRustRadioEngine::equal_power_crossfade_pcm(&outgoing, &incoming, 16);
        assert_eq!(mixed.len(), 64 + 64 - 16);
        // For correlated constant unit signals blended with cos(t) + sin(t), amplitude is >= 1.0
        for &sample in &mixed[48..64] {
            assert!(sample >= 0.99, "crossfade dipped unexpectedly: {sample}");
        }
    }

    #[test]
    fn test_sidechain_ducking_reduces_bed_during_speech() {
        let bed = vec![0.8_f32; 64];
        let mut speech = vec![0.0_f32; 64];
        for s in &mut speech[16..48] {
            *s = 0.6;
        }
        let ducked = PureRustRadioEngine::sidechain_duck_bed_pcm(&bed, &speech, 0.25, 0.1);
        assert_eq!(ducked.len(), bed.len());
        assert!(ducked[32] < 0.35, "expected ducked level < 0.35, got {}", ducked[32]);
        assert!(ducked[63] > ducked[32], "expected recovery after speech ended");
    }

    #[test]
    fn test_four_act_episode_compiler() {
        let ep = PureRustRadioEngine::compile_four_act_episode(
            "ep_2026_11_15_kuroshio",
            "Kuroshio Tide Watch",
            "did:atlantis:kenji_vance",
        );
        assert_eq!(ep.narrative_blocks.len(), 4);
        assert_eq!(ep.narrative_blocks[0].act, NarrativeAct::Act1Intro);
        assert_eq!(ep.narrative_blocks[1].act, NarrativeAct::Act2Complication);
        assert_eq!(ep.narrative_blocks[2].act, NarrativeAct::Act3Climax);
        assert_eq!(ep.narrative_blocks[3].act, NarrativeAct::Act4Resolution);
        assert_eq!(ep.total_duration_secs, 3600.0);
    }

    #[test]
    fn test_club_atlantis_sqlite_acid_checkout_and_100_tiers() {
        let store = ClubAtlantisStore::open_in_memory().expect("open store");
        let tiers = store.seed_100_tier_dungeon().expect("seed 100 tiers");
        assert_eq!(tiers, 100);

        store
            .record_tier_progress(1, true, Some("did:atlantis:pilot_01"))
            .expect("record tier 1 clear");
        assert_eq!(store.cleared_tier_count().unwrap(), 1);

        store
            .upsert_shelf_item("totem-coral-v2", "Holographic Coral Totem", 3, 25.0)
            .expect("upsert sku");

        // Valid checkout decrements stock 3 -> 1
        let receipt = store
            .atomic_checkout(
                "ord-1001",
                "did:atlantis:pilot_01",
                "totem-coral-v2",
                2,
                1_763_200_000,
            )
            .expect("checkout must succeed");
        assert_eq!(receipt.remaining_stock, 1);
        assert!((receipt.total_cyan - 50.0).abs() < 1e-9);

        // Over-purchasing rolls back atomically without corrupting remaining stock
        let failed = store.atomic_checkout(
            "ord-1002",
            "did:atlantis:pilot_02",
            "totem-coral-v2",
            2,
            1_763_200_010,
        );
        assert!(failed.is_err());

        // Remaining stock of 1 can still be purchased cleanly
        let second = store
            .atomic_checkout(
                "ord-1003",
                "did:atlantis:pilot_02",
                "totem-coral-v2",
                1,
                1_763_200_020,
            )
            .expect("final unit checkout must succeed");
        assert_eq!(second.remaining_stock, 0);
    }

    #[test]
    fn test_packed_server_telemetry_and_aoi_filter() {
        let near = PackedServerTelemetryFrame {
            entity_id: 11,
            ecef_pos: [100.0, 200.0, 300.0],
            cam_rel_pos: [1.0, 2.0, 3.0],
            velocity_mps: [4.0, 0.0, -2.0],
            siren_freq_hz: 432.0,
            fader_pos: 0.5,
            dungeon_floor: 7,
            flags: 1,
        };
        let far = PackedServerTelemetryFrame {
            entity_id: 22,
            ecef_pos: [50_000.0, 200.0, 300.0],
            ..near
        };
        let visible = SpatialAoiFilter::filter_within_radius_ecef([0.0, 0.0, 0.0], &[near, far], 1_000.0);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].entity_id, 11);

        let bin = SpatialAoiFilter::encode_batch_binary(&visible);
        assert_eq!(bin.len(), PackedServerTelemetryFrame::BYTE_LEN);
        let decoded = SpatialAoiFilter::decode_batch_binary(&bin).expect("binary decode");
        assert_eq!(decoded, visible);
    }
}
