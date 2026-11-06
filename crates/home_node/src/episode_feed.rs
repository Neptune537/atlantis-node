//! # Atlantis Episode Feed Service & PoH Spillover Engine
//!
//! Evaluates local hardware capability scores ($H_n$) against incoming `SavedEpisode` profiles.
//! Automatically determines whether to process and simulate episodes locally on the node's
//! local GPU/AI engine, or offload execution to a Tier 1 `AbyssalAnchor` Club Home Node via LL-HLS / .nautpak streaming.

use atlantis_protocol::{
    EpisodeProcessingRoute, EpisodeStreamDeliveryMode,
    NarrativeAct, SavedEpisode, EpisodeFeedStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Local node hardware resource snapshot for pre-flight capability assessment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalHardwareMetrics {
    /// Dedicated VRAM available in Megabytes.
    pub vram_available_mb: u64,
    /// System RAM available in Megabytes.
    pub ram_available_mb: u64,
    /// Number of physical CPU cores.
    pub cpu_cores: usize,
    /// Whether local AI inference daemon is online and responsive via ai_gate.
    pub is_ai_gate_online: bool,
    /// Active Proof-of-Health capability score $H_n \in [0.0, 1.0]$.
    pub poh_capability_score: f64,
}

impl Default for LocalHardwareMetrics {
    fn default() -> Self {
        Self {
            vram_available_mb: 8192,
            ram_available_mb: 16384,
            cpu_cores: 8,
            is_ai_gate_online: true,
            poh_capability_score: 0.85,
        }
    }
}

/// Sovereign service managing the Daily Episode Feed pipeline and Club Node spillover logic.
#[derive(Debug, Clone)]
pub struct EpisodeFeedService {
    pub node_did: String,
    pub accepted_club_did: Option<String>,
    pub accepted_club_endpoint: Option<String>,
    hardware_metrics: Arc<Mutex<LocalHardwareMetrics>>,
    cached_episodes: Arc<Mutex<HashMap<String, SavedEpisode>>>,
    active_statuses: Arc<Mutex<HashMap<String, EpisodeFeedStatus>>>,
}

impl EpisodeFeedService {
    pub fn new(node_did: impl Into<String>) -> Self {
        Self {
            node_did: node_did.into(),
            accepted_club_did: None,
            accepted_club_endpoint: None,
            hardware_metrics: Arc::new(Mutex::new(LocalHardwareMetrics::default())),
            cached_episodes: Arc::new(Mutex::new(HashMap::new())),
            active_statuses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Configures the accepted Club Home Node fallback destination for spillover jobs.
    pub fn with_club_node_anchor(
        mut self,
        club_node_did: impl Into<String>,
        club_endpoint: impl Into<String>,
    ) -> Self {
        self.accepted_club_did = Some(club_node_did.into());
        self.accepted_club_endpoint = Some(club_endpoint.into());
        self
    }

    /// Updates the local node hardware telemetry metrics.
    pub fn update_hardware_metrics(&self, metrics: LocalHardwareMetrics) {
        let mut lock = self.hardware_metrics.lock().unwrap();
        *lock = metrics;
    }

    /// Evaluates pre-flight eligibility for an episode, deciding whether to render locally
    /// or dispatch spillover execution to the Club Home Node.
    pub fn evaluate_preflight_route(&self, episode: &SavedEpisode) -> EpisodeProcessingRoute {
        let metrics = self.hardware_metrics.lock().unwrap().clone();

        let vram_ok = metrics.vram_available_mb >= episode.hardware_profile.min_vram_mb;
        let ram_ok = metrics.ram_available_mb >= episode.hardware_profile.min_ram_mb;
        let poh_ok = metrics.poh_capability_score >= episode.hardware_profile.min_poh_score;
        let ai_ok = !episode.hardware_profile.requires_local_llm || metrics.is_ai_gate_online;

        let can_execute_locally = vram_ok && ram_ok && poh_ok && ai_ok;

        if can_execute_locally {
            EpisodeProcessingRoute::LocalExecution {
                vram_allocated_mb: episode.hardware_profile.min_vram_mb,
                local_ai_model: "ollama:llama3.3:70b-instruct".into(),
            }
        } else {
            let club_did = self.accepted_club_did.clone().unwrap_or_else(|| {
                "did:atlantis:club_abyssal_anchor_primary".into()
            });
            let endpoint = self.accepted_club_endpoint.clone().unwrap_or_else(|| {
                "https://node.atlantis.club/stream".into()
            });

            EpisodeProcessingRoute::ClubSpillover {
                target_club_node_did: club_did,
                target_club_endpoint: endpoint.clone(),
                stream_delivery_mode: EpisodeStreamDeliveryMode::LowLatencyHls {
                    stream_url: format!("{}/live/{}.m3u8", endpoint, episode.episode_id),
                },
            }
        }
    }

    /// Ingests a new SavedEpisode payload into the local service, caching narrative blocks
    /// and initializing playback status.
    pub fn ingest_episode(&self, episode: SavedEpisode) -> EpisodeFeedStatus {
        let route = self.evaluate_preflight_route(&episode);
        let is_spillover = matches!(route, EpisodeProcessingRoute::ClubSpillover { .. });

        let status = EpisodeFeedStatus {
            episode_id: episode.episode_id.clone(),
            is_spillover,
            active_act: NarrativeAct::Act1Intro,
            playback_offset_secs: 0.0,
            current_poh_score: self.hardware_metrics.lock().unwrap().poh_capability_score,
            state_deltas_applied: 0,
        };

        self.cached_episodes
            .lock()
            .unwrap()
            .insert(episode.episode_id.clone(), episode);
        self.active_statuses
            .lock()
            .unwrap()
            .insert(status.episode_id.clone(), status.clone());

        status
    }

    /// Advances playback timeline and applies narrative state deltas.
    pub fn tick_playback(
        &self,
        episode_id: &str,
        delta_seconds: f64,
    ) -> Option<EpisodeFeedStatus> {
        let mut status_map = self.active_statuses.lock().unwrap();
        let status = status_map.get_mut(episode_id)?;

        status.playback_offset_secs += delta_seconds;

        // Transition acts based on typical 1-hour schedule (900s / 15m per act)
        if status.playback_offset_secs < 900.0 {
            status.active_act = NarrativeAct::Act1Intro;
        } else if status.playback_offset_secs < 1800.0 {
            status.active_act = NarrativeAct::Act2Complication;
        } else if status.playback_offset_secs < 2700.0 {
            status.active_act = NarrativeAct::Act3Climax;
        } else {
            status.active_act = NarrativeAct::Act4Resolution;
        }

        Some(status.clone())
    }

    /// Retrieves an active episode playback status.
    pub fn get_status(&self, episode_id: &str) -> Option<EpisodeFeedStatus> {
        self.active_statuses.lock().unwrap().get(episode_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlantis_protocol::{EpisodeHardwareProfile, NarrativeBlock};

    fn make_test_episode(vram_mb: u64, ram_mb: u64, min_poh: f64) -> SavedEpisode {
        SavedEpisode {
            episode_id: "ep_test_kaiju_breach_01".into(),
            title: "Operation Abyssal Breach".into(),
            created_at: "2026-10-24T18:00:00Z".into(),
            total_duration_secs: 3600.0,
            hardware_profile: EpisodeHardwareProfile {
                min_vram_mb: vram_mb,
                min_ram_mb: ram_mb,
                min_poh_score: min_poh,
                requires_local_llm: false,
            },
            narrative_blocks: vec![NarrativeBlock {
                block_id: "block_01_intro".into(),
                act: NarrativeAct::Act1Intro,
                title: "Opening Dispatch".into(),
                synopsis: "Kenji Vance signals Houston sector".into(),
                media_chunks: Vec::new(),
                stage_directions: Vec::new(),
                state_deltas: Vec::new(),
            }],
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_local_execution_when_hardware_sufficient() {
        let service = EpisodeFeedService::new("did:atlantis:node_desktop_abyssal");
        let episode = make_test_episode(4096, 8192, 0.65);

        let route = service.evaluate_preflight_route(&episode);
        assert!(matches!(route, EpisodeProcessingRoute::LocalExecution { .. }));

        let status = service.ingest_episode(episode);
        assert!(!status.is_spillover);
        assert_eq!(status.active_act, NarrativeAct::Act1Intro);
    }

    #[test]
    fn test_club_spillover_when_poh_or_vram_insufficient() {
        let service = EpisodeFeedService::new("did:atlantis:node_rpi_epipelagic")
            .with_club_node_anchor(
                "did:atlantis:club_houston_citadel",
                "https://citadel.atlantis.club/api",
            );

        // Constrained device (low VRAM and low PoH score)
        service.update_hardware_metrics(LocalHardwareMetrics {
            vram_available_mb: 512,
            ram_available_mb: 2048,
            cpu_cores: 4,
            is_ai_gate_online: false,
            poh_capability_score: 0.45,
        });

        let heavy_episode = make_test_episode(8192, 16384, 0.65);
        let route = service.evaluate_preflight_route(&heavy_episode);

        match route {
            EpisodeProcessingRoute::ClubSpillover {
                target_club_node_did,
                target_club_endpoint,
                stream_delivery_mode,
            } => {
                assert_eq!(target_club_node_did, "did:atlantis:club_houston_citadel");
                assert_eq!(target_club_endpoint, "https://citadel.atlantis.club/api");
                assert!(matches!(stream_delivery_mode, EpisodeStreamDeliveryMode::LowLatencyHls { .. }));
            }
            _ => panic!("Expected ClubSpillover route"),
        }

        let status = service.ingest_episode(heavy_episode);
        assert!(status.is_spillover);
    }

    #[test]
    fn test_episode_playback_timeline_progression() {
        let service = EpisodeFeedService::new("did:atlantis:node_tester");
        let episode = make_test_episode(2048, 4096, 0.5);
        service.ingest_episode(episode);

        // Advance 1000s -> Act 2
        let s2 = service.tick_playback("ep_test_kaiju_breach_01", 1000.0).unwrap();
        assert_eq!(s2.active_act, NarrativeAct::Act2Complication);

        // Advance another 1000s (2000s total) -> Act 3
        let s3 = service.tick_playback("ep_test_kaiju_breach_01", 1000.0).unwrap();
        assert_eq!(s3.active_act, NarrativeAct::Act3Climax);

        // Advance another 1000s (3000s total) -> Act 4
        let s4 = service.tick_playback("ep_test_kaiju_breach_01", 1000.0).unwrap();
        assert_eq!(s4.active_act, NarrativeAct::Act4Resolution);
    }
}
