//! # Fortress Theater Service (`home_node::fortress_theater_service`)
//!
//! Bridges the 24/7 Siren Theater broadcast engine into the sovereign Atlantis Club Home Node,
//! anchoring the radio turntables and acoustics inside a specific Room within the Club's Fortress on Nautilus.

use atlantis_protocol::{
    ClubFortressAnchor, FortressTheaterState,
};
use crate::ClubOfficerRole;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomSpatialPresence {
    pub is_inside_room: bool,
    pub is_inside_fortress: bool,
    pub distance_to_room_m: f32,
    pub audio_gain: f32,
    pub reverb_wet_send: f32,
    pub sabine_rt60_s: f32,
}

/// Canonical Seven Daughters shift assignment for 24/7 automated radio on the Club Atlantis Pi Node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SevenDaughtersPiSchedule {
    pub daughter_index: u8,
    pub name: String,
    pub callsign: String,
    pub harmonic_hz: f32,
    pub genre_title: String,
    pub shift_window: String,
    pub lan_stream_endpoint: String,
    pub upstream_relay_url: String,
}

impl SevenDaughtersPiSchedule {
    /// Resolves the active Daughter for any 24-hour clock hour (`0..=23`) on the Raspberry Pi node.
    pub fn for_hour(hour_24: u8, pi_lan_host: &str) -> Self {
        let h = hour_24 % 24;
        let (idx, name, callsign, hz, genre, window, upstream) = match h {
            4..=7 => (
                1,
                "Maia",
                "SIREN-01-MAIA",
                432.0,
                "Dawn Oceanic Ambient / Horizon Chill",
                "04:00-08:00",
                "https://stream.nightride.fm/chillsynth.mp3",
            ),
            8..=11 => (
                2,
                "Alcyone",
                "SIREN-02-ALCYONE",
                528.0,
                "High-Speed Cyberpunk D&B / Breakbeats",
                "08:00-12:00",
                "https://stream.nightride.fm/nightride.mp3",
            ),
            12..=15 => (
                3,
                "Asterope",
                "SIREN-03-ASTEROPE",
                639.0,
                "Tactical Deep House & Frontline Reports",
                "12:00-16:00",
                "https://ice2.somafm.com/secretagent-128-mp3",
            ),
            16..=19 => (
                4,
                "Celaeno",
                "SIREN-04-CELAENO",
                741.0,
                "Glitch / Industrial Bass / Storm Chaser",
                "16:00-20:00",
                "https://stream.nightride.fm/darksynth.mp3",
            ),
            20..=23 => (
                5,
                "Electra",
                "SIREN-05-ELECTRA",
                852.0,
                "Vocal Trance / Eurobeat / Club Atlantis",
                "20:00-00:00",
                "https://stream.nightride.fm/datawave.mp3",
            ),
            0..=1 => (
                6,
                "Taygete",
                "SIREN-06-TAYGETE",
                963.0,
                "Darkwave / Minimal Techno / Abyssal Drone",
                "00:00-02:00",
                "https://ice2.somafm.com/groovesalad-128-mp3",
            ),
            _ => (
                7,
                "Merope",
                "SIREN-07-MEROPE",
                396.0,
                "Lo-Fi / Starlight Harbor Acoustic",
                "02:00-04:00",
                "https://radio.plaza.one/mp3",
            ),
        };

        Self {
            daughter_index: idx,
            name: name.into(),
            callsign: callsign.into(),
            harmonic_hz: hz,
            genre_title: genre.into(),
            shift_window: window.into(),
            lan_stream_endpoint: format!("http://{}/live.mp3?daughter={}", pi_lan_host, idx),
            upstream_relay_url: upstream.into(),
        }
    }
}

/// Sovereign service managing the Club Fortress spatial anchor in Nautilus
/// and the 24/7 Siren Theater feature tied to the fortress broadcasting chamber.
#[derive(Debug, Clone)]
pub struct FortressTheaterService {
    pub fortress: ClubFortressAnchor,
    pub theater_room_id: String,
    pub state: FortressTheaterState,
    pub pi_lan_host: String,
    pub auto_daemon_running: bool,
    pub active_daughter_schedule: SevenDaughtersPiSchedule,
}

impl FortressTheaterService {
    /// Creates a new Fortress Theater Service anchored at the designated Nautilus coordinates.
    /// Automatically starts the 24/7 Seven Daughters radio daemon bound to the Club's Raspberry Pi LAN node.
    pub fn new(
        fortress_id: impl Into<String>,
        club_id: impl Into<String>,
        fortress_name: impl Into<String>,
        lat_deg: f64,
        lon_deg: f64,
        alt_meters: f64,
    ) -> Self {
        let f_id = fortress_id.into();
        let fortress = ClubFortressAnchor::new(
            f_id.clone(),
            club_id,
            fortress_name,
            lat_deg,
            lon_deg,
            alt_meters,
        )
        .with_default_fortress_layout();

        let room_id = "room_theater_sanctuary".to_string();
        let pi_lan_host = "10.0.0.237:8080".to_string();
        let active_daughter_schedule = SevenDaughtersPiSchedule::for_hour(12, &pi_lan_host);
        let mut state = FortressTheaterState::default();
        state.fortress_id = f_id;
        state.room_id = room_id.clone();
        state.live_broadcaster = format!(
            "Daughter #{}: {} [{}]",
            active_daughter_schedule.daughter_index,
            active_daughter_schedule.name,
            active_daughter_schedule.callsign
        );

        Self {
            fortress,
            theater_room_id: room_id,
            state,
            pi_lan_host,
            auto_daemon_running: true,
            active_daughter_schedule,
        }
    }

    /// Automatically ticks the 24/7 Seven Daughters schedule on the Club's Atlantis Raspberry Pi node,
    /// rotating the active Daughter shift and updating the on-air broadcast state whenever the daemon is running.
    pub fn auto_tick_seven_daughters(&mut self, current_hour_24: u8) -> &SevenDaughtersPiSchedule {
        if self.auto_daemon_running {
            let sched = SevenDaughtersPiSchedule::for_hour(current_hour_24, &self.pi_lan_host);
            self.state.now_playing_title = format!(
                "{} • {:.0} Hz",
                sched.genre_title, sched.harmonic_hz
            );
            self.state.now_playing_artist = format!(
                "The Seven Daughters // {}",
                sched.name
            );
            self.state.live_broadcaster = format!(
                "Daughter #{}: {} [{}] @ {}",
                sched.daughter_index, sched.name, sched.callsign, self.pi_lan_host
            );
            self.active_daughter_schedule = sched;
        }
        &self.active_daughter_schedule
    }

    /// Returns the active Fortress Theater room metadata and acoustic profile.
    pub fn get_theater_room(&self) -> Option<&atlantis_protocol::FortressRoom> {
        self.fortress.rooms.get(&self.theater_room_id)
    }

    /// Computes the exact 64-bit ECEF coordinates of the Theater Room on the Nautilus digital twin.
    pub fn get_theater_room_ecef(&self) -> Option<[f64; 3]> {
        self.fortress.get_room_absolute_ecef(&self.theater_room_id)
    }

    /// Evaluates spatial audio listener presence and Sabine RT60 acoustic mix
    /// based on a listener or avatar's 64-bit ECEF position in Nautilus.
    pub fn evaluate_listener_presence(&self, listener_ecef: [f64; 3]) -> RoomSpatialPresence {
        let room = match self.get_theater_room() {
            Some(r) => r,
            None => {
                return RoomSpatialPresence {
                    is_inside_room: false,
                    is_inside_fortress: false,
                    distance_to_room_m: f32::MAX,
                    audio_gain: 0.0,
                    reverb_wet_send: 0.0,
                    sabine_rt60_s: 0.0,
                }
            }
        };

        let room_ecef = self
            .get_theater_room_ecef()
            .unwrap_or(self.fortress.ecef_coords);

        let dx = listener_ecef[0] - room_ecef[0];
        let dy = listener_ecef[1] - room_ecef[1];
        let dz = listener_ecef[2] - room_ecef[2];
        let dist_m = ((dx * dx) + (dy * dy) + (dz * dz)).sqrt() as f32;

        let room_radius = (room.dimensions_m[0].max(room.dimensions_m[1])) / 2.0;
        let is_inside_room = dist_m <= room_radius;
        let is_inside_fortress = dist_m <= 100.0;

        let (gain, wet_send) = room.acoustics.evaluate_listener_spatial_mix(dist_m);
        let rt60 = room.acoustics.sabine_rt60_seconds();

        RoomSpatialPresence {
            is_inside_room,
            is_inside_fortress,
            distance_to_room_m: dist_m,
            audio_gain: gain,
            reverb_wet_send: wet_send,
            sabine_rt60_s: rt60,
        }
    }

    /// Updates live on-air broadcast telemetry from the radio streamer engine.
    pub fn sync_stream_telemetry(
        &mut self,
        title: impl Into<String>,
        artist: impl Into<String>,
        next_title: impl Into<String>,
        next_artist: impl Into<String>,
        broadcaster: impl Into<String>,
        fader_pos: f32,
        active_deck: impl Into<String>,
        next_deck: impl Into<String>,
        listeners_count: u32,
    ) {
        self.state.now_playing_title = title.into();
        self.state.now_playing_artist = artist.into();
        self.state.next_cued_title = next_title.into();
        self.state.next_cued_artist = next_artist.into();
        self.state.live_broadcaster = broadcaster.into();
        self.state.fader_position = fader_pos.clamp(0.0, 1.0);
        self.state.active_deck = active_deck.into();
        self.state.next_deck = next_deck.into();
        self.state.active_listeners_count = listeners_count;
    }

    /// Allows an authenticated club officer to take over the vinyl turntables from inside the Fortress Theater room.
    pub fn takeover_broadcast_as_officer(
        &mut self,
        caller_did: &str,
        caller_role: ClubOfficerRole,
        title: &str,
        artist: &str,
        deck: &str,
    ) -> Result<String, &'static str> {
        // Enforce club standing requirement
        if !caller_role.is_executive() && caller_role != ClubOfficerRole::FullPatchedMember {
            return Err("Unauthorized: Must be a Full Patched Member or Executive Officer to take over the Fortress Theater deck");
        }

        self.state.active_deck = deck.to_string();
        self.state.now_playing_title = title.to_string();
        self.state.now_playing_artist = artist.to_string();
        self.state.live_broadcaster = format!("{} ({})", caller_did, caller_role.title_display());
        self.state.fader_position = if deck == "A" { 0.0 } else { 1.0 };

        Ok(format!(
            "Officer {} successfully assumed on-air control in Fortress Room '{}'",
            caller_did, self.theater_room_id
        ))
    }

    /// Cues a track onto the standby deck within the Fortress room.
    pub fn cue_track_in_room(&mut self, deck: &str, title: &str, artist: &str) {
        self.state.next_deck = deck.to_string();
        self.state.next_cued_title = title.to_string();
        self.state.next_cued_artist = artist.to_string();
    }

    /// Processes a Coral micro-tip in CYAN from a room listener to the current broadcaster.
    pub fn tip_broadcaster(&mut self, amount_cyan: f64) {
        if amount_cyan > 0.0 {
            self.state.coral_escrow_tips_total_cyan += amount_cyan;
        }
    }

    /// Dispatches the sound aspect to Sebastian in Atlantis (`did:atlantis:sebastian-edgecrab-tier3`).
    pub fn export_sound_telemetry(&self) -> &atlantis_protocol::TheaterSoundTelemetry {
        &self.state.sound_telemetry
    }

    /// Dispatches the visual aspect to Ursula in Lumeria (`did:atlantis:ursula-node-manager`) where graphical tools reside.
    pub fn export_visual_telemetry(&self, camera_ecef: [f64; 3]) -> atlantis_protocol::TheaterVisualTelemetry {
        let mut visual = self.state.visual_telemetry.clone();
        let room_ecef = self.get_theater_room_ecef().unwrap_or(self.fortress.ecef_coords);

        // Convert 64-bit Nautilus ECEF to 32-bit camera-relative coordinates for Lumeria Cook-Torrance GGX PBR shaders
        visual.camera_relative_turntable_pos = [
            (room_ecef[0] - camera_ecef[0]) as f32,
            (room_ecef[1] - camera_ecef[1]) as f32,
            (room_ecef[2] - camera_ecef[2]) as f32,
        ];
        visual
    }

    /// Ingests real-time FFT spectrum bins and updates visual telemetry for Ursula.
    pub fn update_visual_spectrum(&mut self, bands: [f32; 4], deck_a_rot_deg: f32, deck_b_rot_deg: f32) {
        self.state.visual_telemetry.frequency_bands = bands;
        self.state.visual_telemetry.deck_a_rotation_deg = deck_a_rot_deg;
        self.state.visual_telemetry.deck_b_rotation_deg = deck_b_rot_deg;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlantis_protocol::FortressRoomType;

    #[test]
    fn test_fortress_theater_initialization_and_nautilus_anchor() {
        // Coordinates for Club Oceanic Citadel on Nautilus (e.g. near coastal ridge)
        let service = FortressTheaterService::new(
            "fortress_alpha_citadel",
            "club_oceanic",
            "Oceanic Fortress Citadel",
            25.7617,
            -80.1918,
            12.0,
        );

        assert_eq!(service.fortress.fortress_id, "fortress_alpha_citadel");
        assert_eq!(service.fortress.rooms.len(), 3);

        let room = service.get_theater_room().expect("Theater room must exist");
        assert_eq!(room.room_type, FortressRoomType::TheaterBroadcastingChamber);
        assert_eq!(room.name, "Siren Theater & Vinyl Conductor Deck");

        // Verify exact 64-bit ECEF coordinates computation
        let room_ecef = service.get_theater_room_ecef().expect("Room ECEF must be valid");
        assert!(room_ecef[0].abs() > 1000.0);
        assert!(room_ecef[1].abs() > 1000.0);
    }

    #[test]
    fn test_sabine_rt60_and_spatial_audio_presence() {
        let service = FortressTheaterService::new(
            "fortress_alpha_citadel",
            "club_oceanic",
            "Oceanic Fortress Citadel",
            25.7617,
            -80.1918,
            12.0,
        );

        let room_ecef = service.get_theater_room_ecef().unwrap();

        // 1. Inside sweet spot (0.5m from turntable console)
        let listener_at_deck = [room_ecef[0] + 0.3, room_ecef[1], room_ecef[2]];
        let presence_sweet_spot = service.evaluate_listener_presence(listener_at_deck);
        assert!(presence_sweet_spot.is_inside_room);
        assert!(presence_sweet_spot.is_inside_fortress);
        assert_eq!(presence_sweet_spot.audio_gain, 1.0);
        assert!((presence_sweet_spot.sabine_rt60_s - 0.62).abs() < 0.1);

        // 2. In Fortress Hallway (30m away)
        let listener_hallway = [room_ecef[0] + 30.0, room_ecef[1], room_ecef[2]];
        let presence_hallway = service.evaluate_listener_presence(listener_hallway);
        assert!(!presence_hallway.is_inside_room);
        assert!(presence_hallway.is_inside_fortress);
        assert!(presence_hallway.audio_gain > 0.0 && presence_hallway.audio_gain < 1.0);
        assert!(presence_hallway.reverb_wet_send > 0.2);

        // 3. Beyond audible fortress acoustic boundary (150m away)
        let listener_outside = [room_ecef[0] + 150.0, room_ecef[1], room_ecef[2]];
        let presence_outside = service.evaluate_listener_presence(listener_outside);
        assert!(!presence_outside.is_inside_room);
        assert!(!presence_outside.is_inside_fortress);
        assert_eq!(presence_outside.audio_gain, 0.0);
    }

    #[test]
    fn test_officer_takeover_permission_and_cue() {
        let mut service = FortressTheaterService::new(
            "fortress_alpha_citadel",
            "club_oceanic",
            "Oceanic Fortress Citadel",
            25.7617,
            -80.1918,
            12.0,
        );

        // Initiate cannot take over
        let err = service.takeover_broadcast_as_officer(
            "did:atlantis:initiate_01",
            ClubOfficerRole::ProspectOrInitiate,
            "Rogue Track",
            "Rogue Artist",
            "B",
        );
        assert!(err.is_err());

        // Full Patched Member or President can take over
        let res = service.takeover_broadcast_as_officer(
            "did:atlantis:president_01",
            ClubOfficerRole::PresidentOrMaster,
            "Catch Me If You Can",
            "Joyce Wrice",
            "B",
        );
        assert!(res.is_ok());
        assert_eq!(service.state.now_playing_title, "Catch Me If You Can");
        assert_eq!(service.state.active_deck, "B");
        assert_eq!(service.state.fader_position, 1.0);

        // Cue track in room
        service.cue_track_in_room("A", "Chineke", "Tobe Nwigwe");
        assert_eq!(service.state.next_cued_title, "Chineke");
        assert_eq!(service.state.next_deck, "A");

        // Tip broadcaster in room
        service.tip_broadcaster(5.5);
        assert_eq!(service.state.coral_escrow_tips_total_cyan, 5.5);
    }

    #[test]
    fn test_sebastian_sound_and_ursula_visual_division() {
        let mut service = FortressTheaterService::new(
            "fortress_alpha_citadel",
            "club_oceanic",
            "Oceanic Fortress Citadel",
            25.7617,
            -80.1918,
            12.0,
        );

        // 1. Verify sound aspect mapped to Sebastian in Atlantis
        let sound = service.export_sound_telemetry();
        assert_eq!(service.state.node_division.sound_worker_did, "did:atlantis:sebastian-edgecrab-tier3");
        assert_eq!(sound.sound_conductor, "Sebastian (Court Conductor)");
        assert_eq!(sound.sample_rate_hz, 48000);
        assert_eq!(sound.channels, 2);

        // 2. Verify visual aspect mapped to Ursula in Lumeria
        assert_eq!(service.state.node_division.visual_engine_did, "did:atlantis:ursula-node-manager");
        assert!(service.state.node_division.visual_engine_target.contains("Lumeria"));

        // Update real-time FFT spectrum and turntable rotation
        service.update_visual_spectrum([180.0, 120.0, 95.0, 60.0], 45.0, 0.0);
        assert_eq!(service.state.visual_telemetry.frequency_bands[0], 180.0);
        assert_eq!(service.state.visual_telemetry.deck_a_rotation_deg, 45.0);

        // Export visual telemetry with camera-relative coordinates for Lumeria
        let room_ecef = service.get_theater_room_ecef().unwrap();
        let camera_ecef = [room_ecef[0] + 5.0, room_ecef[1] + 2.0, room_ecef[2] + 1.5];
        let visual = service.export_visual_telemetry(camera_ecef);

        assert_eq!(visual.camera_relative_turntable_pos[0], -5.0);
        assert_eq!(visual.camera_relative_turntable_pos[1], -2.0);
        assert_eq!(visual.camera_relative_turntable_pos[2], -1.5);
        assert!(visual.room_splat_uri.is_some());
    }
}
