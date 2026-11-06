//! # Atlantis Club Fortress & Spatial Theater Protocol
//!
//! Models the sovereign Club Fortress anchored into the Nautilus 3D Earth digital twin (WGS84/ECEF),
//! its internal rooms, Sabine RT60 spatial acoustics, and the 24/7 Theater / Siren Radio feature.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Categories of rooms located within a sovereign Club Fortress on Nautilus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FortressRoomType {
    /// 24/7 Siren Frequency radio theater, turntable decks, soundboard & acoustic pulpit.
    TheaterBroadcastingChamber,
    /// Grand Council hall for officer deliberations and multi-sig voting.
    CouncilChamber,
    /// Vault room housing high-precision telemetry, private ledger and Coral assets.
    VaultRoom,
    /// Outer Tyler security gate and entrance firewall.
    OuterGuardTylerGate,
    /// Chamber of Reflection for initiate vetting and quiet listening.
    ChamberOfReflection,
    /// Team Office & Command Sanctum for daemon collaboration in Nautilus (Team 7 Cornerstone daemons).
    TeamOfficeSanctum,
}

/// Physical acoustic properties of a room within the Club Fortress,
/// governing Sabine RT60 reverberation and spatial audio attenuation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomAcousticProfile {
    /// Room volume in cubic meters ($V$).
    pub volume_m3: f32,
    /// Total interior surface area in square meters ($A$).
    pub surface_area_m2: f32,
    /// Average acoustic absorption coefficient ($\bar{\alpha} \in (0, 1)$).
    /// E.g., 0.10 for hard reflective concrete, 0.35 for acoustic wood & baffles, 0.60 for heavy drapery.
    pub absorption_coefficient: f32,
    /// Base spatial audio roll-off exponent (typically 1.0 for inverse square law).
    pub distance_rolloff: f32,
    /// Maximum audible radius in meters outside the room doorway before complete sound isolation.
    pub max_audible_radius_m: f32,
}

impl RoomAcousticProfile {
    pub fn new_treated_theater_studio() -> Self {
        Self {
            volume_m3: 650.0,
            surface_area_m2: 480.0,
            absorption_coefficient: 0.35, // Treated acoustic studio panels + heavy bass traps
            distance_rolloff: 1.0,
            max_audible_radius_m: 65.0,
        }
    }

    /// Computes the Sabine Reverberation Time $RT_{60} = \frac{0.161 \cdot V}{A \cdot \bar{\alpha}}$ in seconds.
    pub fn sabine_rt60_seconds(&self) -> f32 {
        let effective_absorption = self.surface_area_m2 * self.absorption_coefficient.max(0.01);
        (0.161 * self.volume_m3) / effective_absorption
    }

    /// Computes the spatial audio listener gain (0.0 to 1.0) and wet reverb send at a given distance in meters.
    pub fn evaluate_listener_spatial_mix(&self, distance_m: f32) -> (f32, f32) {
        if distance_m <= 1.0 {
            // Near-field sweet spot in front of vinyl monitors
            return (1.0, 0.15);
        }
        if distance_m >= self.max_audible_radius_m {
            // Outside fortress room acoustic boundary
            return (0.0, 0.0);
        }

        // Distance attenuation
        let norm_dist = (distance_m - 1.0) / (self.max_audible_radius_m - 1.0);
        let dry_gain = (1.0 - norm_dist.powf(self.distance_rolloff)).clamp(0.0, 1.0);

        // Wet reverb increases slightly with distance as diffuse field overtakes direct sound
        let rt60 = self.sabine_rt60_seconds();
        let wet_send = (0.15 + (norm_dist * 0.35 * (rt60 / 1.5))).clamp(0.0, 0.85);

        (dry_gain, wet_send)
    }
}

/// A physical room within the Club Fortress anchored to Nautilus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FortressRoom {
    pub room_id: String,
    pub name: String,
    pub room_type: FortressRoomType,
    /// Offset [dx, dy, dz] in meters from the Fortress ground anchor in ECEF frame.
    pub local_offset_m: [f64; 3],
    /// Interior dimensions [width, length, height] in meters.
    pub dimensions_m: [f32; 3],
    /// Acoustic profile for spatial audio engine.
    pub acoustics: RoomAcousticProfile,
    /// Required officer role or permission tag to take over the console in this room.
    pub required_control_permission: String,
}

/// Sovereign spatial anchor of the Club Fortress on the Nautilus planetary digital twin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClubFortressAnchor {
    pub fortress_id: String,
    pub club_id: String,
    pub name: String,
    /// WGS84 Geodetic Coordinates
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_meters: f64,
    /// 64-bit ECEF coordinates in Nautilus planetary frame
    pub ecef_coords: [f64; 3],
    /// Rooms contained within the Fortress
    pub rooms: HashMap<String, FortressRoom>,
}

impl ClubFortressAnchor {
    pub fn new(
        fortress_id: impl Into<String>,
        club_id: impl Into<String>,
        name: impl Into<String>,
        lat_deg: f64,
        lon_deg: f64,
        alt_meters: f64,
    ) -> Self {
        let ecef = wgs84_to_ecef(lat_deg, lon_deg, alt_meters);
        Self {
            fortress_id: fortress_id.into(),
            club_id: club_id.into(),
            name: name.into(),
            lat_deg,
            lon_deg,
            alt_meters,
            ecef_coords: ecef,
            rooms: HashMap::new(),
        }
    }

    pub fn with_default_fortress_layout(mut self) -> Self {
        let theater_room = FortressRoom {
            room_id: "room_theater_sanctuary".to_string(),
            name: "Siren Theater & Vinyl Conductor Deck".to_string(),
            room_type: FortressRoomType::TheaterBroadcastingChamber,
            local_offset_m: [12.0, 8.0, 2.5], // Second floor east wing of the fortress
            dimensions_m: [14.0, 20.0, 5.0],
            acoustics: RoomAcousticProfile::new_treated_theater_studio(),
            required_control_permission: "THEATER_BROADCASTER".to_string(),
        };

        let council_room = FortressRoom {
            room_id: "room_council_hall".to_string(),
            name: "Officers Grand Council Chamber".to_string(),
            room_type: FortressRoomType::CouncilChamber,
            local_offset_m: [0.0, 0.0, 0.0],
            dimensions_m: [25.0, 30.0, 8.0],
            acoustics: RoomAcousticProfile {
                volume_m3: 6000.0,
                surface_area_m2: 2380.0,
                absorption_coefficient: 0.18,
                distance_rolloff: 1.0,
                max_audible_radius_m: 80.0,
            },
            required_control_permission: "EXECUTIVE_COUNCIL".to_string(),
        };

        let team_office = FortressRoom {
            room_id: "room_team7_office".to_string(),
            name: "Team 7 Nautilus Command Office".to_string(),
            room_type: FortressRoomType::TeamOfficeSanctum,
            local_offset_m: [-10.0, 15.0, 4.0], // Third floor north wing executive suite
            dimensions_m: [18.0, 16.0, 4.5],
            acoustics: RoomAcousticProfile {
                volume_m3: 1296.0,
                surface_area_m2: 882.0,
                absorption_coefficient: 0.28,
                distance_rolloff: 1.0,
                max_audible_radius_m: 50.0,
            },
            required_control_permission: "TEAM_7_DAEMON".to_string(),
        };

        self.rooms.insert(theater_room.room_id.clone(), theater_room);
        self.rooms.insert(council_room.room_id.clone(), council_room);
        self.rooms.insert(team_office.room_id.clone(), team_office);
        self
    }

    /// Computes absolute ECEF coordinates for a specific room within this fortress.
    pub fn get_room_absolute_ecef(&self, room_id: &str) -> Option<[f64; 3]> {
        let room = self.rooms.get(room_id)?;
        Some([
            self.ecef_coords[0] + room.local_offset_m[0],
            self.ecef_coords[1] + room.local_offset_m[1],
            self.ecef_coords[2] + room.local_offset_m[2],
        ])
    }
}

/// Live on-air state of the Theater Feature embedded in the Club Fortress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FortressTheaterState {
    pub fortress_id: String,
    pub room_id: String,
    pub siren_channel: String,
    pub stream_url: String,
    pub active_deck: String,
    pub next_deck: String,
    pub fader_position: f32,
    pub now_playing_title: String,
    pub now_playing_artist: String,
    pub next_cued_title: String,
    pub next_cued_artist: String,
    pub live_broadcaster: String,
    pub is_quarterly_bulletin_active: bool,
    pub bulletin_countdown_s: u32,
    pub weather_summary: String,
    pub active_listeners_count: u32,
    pub coral_escrow_tips_total_cyan: f64,
    pub node_division: TheaterNodeDivision,
    pub visual_telemetry: TheaterVisualTelemetry,
    pub sound_telemetry: TheaterSoundTelemetry,
}

/// Canonical node assignments for the decoupled Theater architecture:
/// - Sound aspect governed by Sebastian in Atlantis.
/// - Visual aspect governed by Ursula in Lumeria (Lemuria) where graphical tools reside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TheaterNodeDivision {
    /// Sound Daemon: Sebastian in Atlantis (Tier 3 Edge Conductor)
    pub sound_worker_did: String,
    pub sound_worker_role: String,
    /// Visual Engine: Ursula in Lumeria (Tier 2 Node Manager & Graphical Orchestrator)
    pub visual_engine_did: String,
    pub visual_engine_role: String,
    /// Canonical 3D engine target
    pub visual_engine_target: String,
}

impl Default for TheaterNodeDivision {
    fn default() -> Self {
        Self {
            sound_worker_did: "did:atlantis:sebastian-edgecrab-tier3".to_string(),
            sound_worker_role: "Sebastian (Court Conductor & Atlantis Audio Daemon)".to_string(),
            visual_engine_did: "did:atlantis:ursula-node-manager".to_string(),
            visual_engine_role: "Ursula (Pelagic Node Manager & Lumeria Graphical Orchestrator)".to_string(),
            visual_engine_target: "Lumeria (Lemuria WebGPU / Cook-Torrance GGX PBR / Nautilus World)".to_string(),
        }
    }
}

/// 3D Visual telemetry emitted to Ursula in Lumeria for graphical rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TheaterVisualTelemetry {
    pub room_id: String,
    /// Camera-relative 32-bit position offset of the theater turntables in Nautilus
    pub camera_relative_turntable_pos: [f32; 3],
    /// Platter rotation angles in degrees for Deck A and Deck B
    pub deck_a_rotation_deg: f32,
    pub deck_b_rotation_deg: f32,
    /// Tonearm engaged status
    pub tonearm_a_engaged: bool,
    pub tonearm_b_engaged: bool,
    /// Real-time FFT spectrum bins (4-band or 16-band) driving dynamic froxel lighting
    pub frequency_bands: [f32; 4],
    /// Active 3D Gaussian Splat asset URI for the room totems
    pub room_splat_uri: Option<String>,
}

impl Default for TheaterVisualTelemetry {
    fn default() -> Self {
        Self {
            room_id: "room_theater_sanctuary".to_string(),
            camera_relative_turntable_pos: [0.0, 1.2, -2.5],
            deck_a_rotation_deg: 0.0,
            deck_b_rotation_deg: 0.0,
            tonearm_a_engaged: true,
            tonearm_b_engaged: false,
            frequency_bands: [0.0; 4],
            room_splat_uri: Some("splat://fortress/citadel/theater_deck.ply".to_string()),
        }
    }
}

/// Sound telemetry emitted to Sebastian in Atlantis for audio stream processing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TheaterSoundTelemetry {
    pub sound_conductor: String,
    pub siren_channel: String,
    pub stream_endpoint: String,
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub active_deck: String,
    pub fader_position: f32,
    pub sabine_rt60_s: f32,
    pub volume: f32,
    pub is_ducked_for_bulletin: bool,
}

impl Default for TheaterSoundTelemetry {
    fn default() -> Self {
        Self {
            sound_conductor: "Sebastian (Court Conductor)".to_string(),
            siren_channel: "siren:frequency:fortress_theater".to_string(),
            stream_endpoint: "http://localhost:8080/live.mp3".to_string(),
            sample_rate_hz: 48000,
            channels: 2,
            active_deck: "A".to_string(),
            fader_position: 0.0,
            sabine_rt60_s: 0.62,
            volume: 0.8,
            is_ducked_for_bulletin: false,
        }
    }
}

impl Default for FortressTheaterState {
    fn default() -> Self {
        Self {
            fortress_id: "fortress_oceanic_citadel".to_string(),
            room_id: "room_theater_sanctuary".to_string(),
            siren_channel: "siren:frequency:fortress_theater".to_string(),
            stream_url: "http://localhost:8080/live.mp3".to_string(),
            active_deck: "A".to_string(),
            next_deck: "B".to_string(),
            fader_position: 0.0,
            now_playing_title: "Burn (feat. Ava Max)".to_string(),
            now_playing_artist: "BigXthaPlug".to_string(),
            next_cued_title: "Chineke".to_string(),
            next_cued_artist: "Tobe Nwigwe".to_string(),
            live_broadcaster: "Sebastian (Court Conductor)".to_string(),
            is_quarterly_bulletin_active: false,
            bulletin_countdown_s: 900,
            weather_summary: "Coastal Region: 74°F, Clear skies".to_string(),
            active_listeners_count: 1,
            coral_escrow_tips_total_cyan: 0.0,
            node_division: TheaterNodeDivision::default(),
            visual_telemetry: TheaterVisualTelemetry::default(),
            sound_telemetry: TheaterSoundTelemetry::default(),
        }
    }
}

/// Helper function to convert WGS84 Geodetic to 64-bit ECEF coordinates.
pub fn wgs84_to_ecef(lat_deg: f64, lon_deg: f64, alt_m: f64) -> [f64; 3] {
    const A: f64 = 6_378_137.0; // WGS84 Semi-major axis
    const E2: f64 = 0.00669437999014; // WGS84 Eccentricity squared

    let lat_rad = lat_deg.to_radians();
    let lon_rad = lon_deg.to_radians();
    let sin_lat = lat_rad.sin();
    let cos_lat = lat_rad.cos();
    let sin_lon = lon_rad.sin();
    let cos_lon = lon_rad.cos();

    let n = A / (1.0 - E2 * sin_lat * sin_lat).sqrt();
    let x = (n + alt_m) * cos_lat * cos_lon;
    let y = (n + alt_m) * cos_lat * sin_lon;
    let z = (n * (1.0 - E2) + alt_m) * sin_lat;

    [x, y, z]
}
