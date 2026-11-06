//! # Nightlife & Crowd Radar Data Fusion Engine (`fusion`)
//!
//! Ingests heterogeneous venue signals (cryptographic door pass scans, member crowd reports,
//! beacon check-ins, sensor telemetry) and fuses them into a coherent, confidence-scored,
//! privacy-preserving venue crowd radar state.
//!
//! Follows the privacy & security guidelines in `AGENTS.md`:
//! - Member telemetry and exact locations remain obfuscated/aggregated.
//! - Outliers and stale reports are rejected or exponentially decayed.
//! - Cryptographic admissions and high-PoH member reports carry higher fusion weight.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Source category of an incoming crowd/venue observation signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalSourceKind {
    /// Cryptographically signed digital door pass admission at gate.
    VerifiedDoorPass,
    /// Direct check-in via localized floor beacon / NFC tap.
    BeaconTap,
    /// Real-time report from an active authenticated club member.
    MemberObservation,
    /// Automated acoustic or venue sensor (e.g. ambient dB / BPM / occupancy counter).
    VenueSensor,
}

impl SignalSourceKind {
    /// Base trust multiplier for fusion weighting.
    pub fn base_trust_weight(&self) -> f32 {
        match self {
            Self::VerifiedDoorPass => 1.0,
            Self::BeaconTap => 0.85,
            Self::VenueSensor => 0.75,
            Self::MemberObservation => 0.60,
        }
    }
}

/// An observation signal submitted for venue crowd radar data fusion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrowdRadarSignal {
    pub signal_id: Uuid,
    pub venue_id: Uuid,
    pub reporter_did: Option<String>,
    pub source: SignalSourceKind,
    pub queue_time_minutes: Option<u32>,
    pub crowd_density: Option<f32>, // Normalized 0.0 (empty) to 1.0 (at capacity)
    pub music_genre: Option<String>,
    pub performer_or_dj: Option<String>,
    pub reporter_poh_score: Option<f32>, // Proof-of-Health / reputation weight (0.0 - 100.0)
    pub timestamp_s: u64,
}

/// Consolidated, fused real-time venue telemetry state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FusedVenueCrowdState {
    pub venue_id: Uuid,
    /// Fused estimate of queue wait time in minutes.
    pub estimated_queue_minutes: u32,
    /// Fused crowd density index (0.0 = vacant, 1.0 = peak capacity).
    pub crowd_density_index: f32,
    /// Current consensus music genre.
    pub primary_genre: String,
    /// Current active performer / DJ.
    pub active_performer: String,
    /// Total verified door check-ins during current active aggregation window.
    pub verified_admissions_window: u32,
    /// Statistical confidence score of this fused observation (0.0 to 1.0).
    pub confidence_score: f32,
    /// Timestamp of most recent update.
    pub last_updated_s: u64,
}

/// Configuration parameters for the data fusion engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFusionConfig {
    /// Observation validity window in seconds (e.g. 1800s = 30 minutes). Older signals are discarded.
    pub max_signal_age_s: u64,
    /// Half-life for temporal exponential decay in seconds (e.g. 600s = 10 minutes).
    pub half_life_decay_s: f32,
    /// Minimum confidence threshold required to publish public estimates.
    pub min_confidence_threshold: f32,
}

impl Default for DataFusionConfig {
    fn default() -> Self {
        Self {
            max_signal_age_s: 1800,     // 30 min max horizon
            half_life_decay_s: 600.0,   // 10 min half-life decay
            min_confidence_threshold: 0.20,
        }
    }
}

/// In-memory Data Fusion Engine for Atlantis Nightlife and Crowd Radar.
#[derive(Debug, Clone)]
pub struct CrowdRadarFusionEngine {
    pub config: DataFusionConfig,
    /// Buffer of raw signals partitioned by venue ID.
    signals_by_venue: HashMap<Uuid, Vec<CrowdRadarSignal>>,
    /// Cached fused state per venue.
    fused_states: HashMap<Uuid, FusedVenueCrowdState>,
}

impl Default for CrowdRadarFusionEngine {
    fn default() -> Self {
        Self::new(DataFusionConfig::default())
    }
}

impl CrowdRadarFusionEngine {
    pub fn new(config: DataFusionConfig) -> Self {
        Self {
            config,
            signals_by_venue: HashMap::new(),
            fused_states: HashMap::new(),
        }
    }

    /// Ingests a new crowd observation signal into the venue's fusion pool.
    pub fn ingest_signal(&mut self, signal: CrowdRadarSignal) {
        let venue_id = signal.venue_id;
        self.signals_by_venue
            .entry(venue_id)
            .or_default()
            .push(signal);
    }

    /// Prunes expired observations outside `max_signal_age_s`.
    pub fn prune_stale_signals(&mut self, current_time_s: u64) {
        let cutoff = current_time_s.saturating_sub(self.config.max_signal_age_s);
        for signals in self.signals_by_venue.values_mut() {
            signals.retain(|s| s.timestamp_s >= cutoff);
        }
    }

    /// Performs sensor & signal fusion across all collected observations for the target venue.
    pub fn fuse_venue_signals(
        &mut self,
        venue_id: Uuid,
        current_time_s: u64,
    ) -> Option<FusedVenueCrowdState> {
        self.prune_stale_signals(current_time_s);

        let signals = self.signals_by_venue.get(&venue_id)?;
        if signals.is_empty() {
            return self.fused_states.get(&venue_id).cloned();
        }

        let mut total_queue_weight = 0.0f32;
        let mut weighted_queue_sum = 0.0f32;

        let mut total_density_weight = 0.0f32;
        let mut weighted_density_sum = 0.0f32;

        let mut verified_admissions = 0u32;
        let mut genre_votes: HashMap<String, f32> = HashMap::new();
        let mut performer_votes: HashMap<String, f32> = HashMap::new();

        let mut total_raw_weight = 0.0f32;

        for s in signals {
            let age_s = current_time_s.saturating_sub(s.timestamp_s) as f32;
            // Exponential time decay: 2^(-age / half_life)
            let time_decay = 2.0f32.powf(-age_s / self.config.half_life_decay_s.max(1.0));

            // PoH score weighting (scale 0.0 to 100.0 into 0.5 to 1.5 multiplier)
            let poh_mult = match s.reporter_poh_score {
                Some(score) => (0.5 + (score.clamp(0.0, 100.0) / 100.0)).clamp(0.5, 1.5),
                None => 1.0,
            };

            let weight = s.source.base_trust_weight() * time_decay * poh_mult;
            total_raw_weight += weight;

            if s.source == SignalSourceKind::VerifiedDoorPass {
                verified_admissions += 1;
            }

            if let Some(q) = s.queue_time_minutes {
                weighted_queue_sum += (q as f32) * weight;
                total_queue_weight += weight;
            }

            if let Some(d) = s.crowd_density {
                weighted_density_sum += d.clamp(0.0, 1.0) * weight;
                total_density_weight += weight;
            }

            if let Some(ref g) = s.music_genre {
                if !g.trim().is_empty() {
                    *genre_votes.entry(g.trim().to_string()).or_insert(0.0) += weight;
                }
            }

            if let Some(ref p) = s.performer_or_dj {
                if !p.trim().is_empty() {
                    *performer_votes.entry(p.trim().to_string()).or_insert(0.0) += weight;
                }
            }
        }

        let estimated_queue = if total_queue_weight > 0.0 {
            (weighted_queue_sum / total_queue_weight).round() as u32
        } else {
            0
        };

        // Combine weighted density with verified admissions heuristic
        let base_density = if total_density_weight > 0.0 {
            weighted_density_sum / total_density_weight
        } else {
            0.0
        };

        // If door admissions are present, ensure crowd density reflects entry momentum
        let admissions_factor = (verified_admissions as f32 * 0.05).clamp(0.0, 0.4);
        let final_density = (base_density.max(admissions_factor)).clamp(0.0, 1.0);

        let primary_genre = genre_votes
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(g, _)| g)
            .unwrap_or_else(|| "Eclectic / Open Format".to_string());

        let active_performer = performer_votes
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(p, _)| p)
            .unwrap_or_else(|| "Resident DJ".to_string());

        // Confidence score based on signal count, weights, and verified door inputs
        let signal_count = signals.len() as f32;
        let confidence = (1.0 - (-total_raw_weight / 5.0).exp())
            .min((signal_count / 10.0).clamp(0.1, 1.0));

        let state = FusedVenueCrowdState {
            venue_id,
            estimated_queue_minutes: estimated_queue,
            crowd_density_index: final_density,
            primary_genre,
            active_performer,
            verified_admissions_window: verified_admissions,
            confidence_score: (confidence * 100.0).round() / 100.0,
            last_updated_s: current_time_s,
        };

        self.fused_states.insert(venue_id, state.clone());
        Some(state)
    }

    /// Retrieves the most recent fused state for a venue if available.
    pub fn get_fused_state(&self, venue_id: &Uuid) -> Option<&FusedVenueCrowdState> {
        self.fused_states.get(venue_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ingest_and_fuse_multiple_signals() {
        let mut engine = CrowdRadarFusionEngine::default();
        let venue_id = Uuid::new_v4();
        let now_s = 1_700_000_000;

        // Signal 1: Verified Door Pass (high trust)
        engine.ingest_signal(CrowdRadarSignal {
            signal_id: Uuid::new_v4(),
            venue_id,
            reporter_did: Some("did:atlantis:pass_scanner".to_string()),
            source: SignalSourceKind::VerifiedDoorPass,
            queue_time_minutes: Some(15),
            crowd_density: Some(0.6),
            music_genre: Some("Cyberpunk Techno".to_string()),
            performer_or_dj: Some("DJ Akira".to_string()),
            reporter_poh_score: Some(95.0),
            timestamp_s: now_s - 120,
        });

        // Signal 2: Member observation with slightly higher queue estimate
        engine.ingest_signal(CrowdRadarSignal {
            signal_id: Uuid::new_v4(),
            venue_id,
            reporter_did: Some("did:atlantis:member_alice".to_string()),
            source: SignalSourceKind::MemberObservation,
            queue_time_minutes: Some(25),
            crowd_density: Some(0.7),
            music_genre: Some("Cyberpunk Techno".to_string()),
            performer_or_dj: Some("DJ Akira".to_string()),
            reporter_poh_score: Some(80.0),
            timestamp_s: now_s - 60,
        });

        // Signal 3: Stale signal beyond max_signal_age_s (should be pruned)
        engine.ingest_signal(CrowdRadarSignal {
            signal_id: Uuid::new_v4(),
            venue_id,
            reporter_did: None,
            source: SignalSourceKind::VenueSensor,
            queue_time_minutes: Some(90),
            crowd_density: Some(0.1),
            music_genre: Some("Jazz".to_string()),
            performer_or_dj: Some("Unknown".to_string()),
            reporter_poh_score: None,
            timestamp_s: now_s - 3600, // 1 hour ago
        });

        let fused = engine
            .fuse_venue_signals(venue_id, now_s)
            .expect("should produce fused state");

        assert_eq!(fused.venue_id, venue_id);
        assert_eq!(fused.primary_genre, "Cyberpunk Techno");
        assert_eq!(fused.active_performer, "DJ Akira");
        assert_eq!(fused.verified_admissions_window, 1);
        // Weighted average of queue times should be in the ~17-21 range (between 15 and 25)
        assert!(fused.estimated_queue_minutes >= 16 && fused.estimated_queue_minutes <= 22);
        assert!(fused.crowd_density_index >= 0.6 && fused.crowd_density_index <= 0.8);
        assert!(fused.confidence_score > 0.15);
    }

    #[test]
    fn test_stale_signal_pruning() {
        let mut engine = CrowdRadarFusionEngine::default();
        let venue_id = Uuid::new_v4();
        let now_s = 1_700_000_000;

        engine.ingest_signal(CrowdRadarSignal {
            signal_id: Uuid::new_v4(),
            venue_id,
            reporter_did: None,
            source: SignalSourceKind::MemberObservation,
            queue_time_minutes: Some(40),
            crowd_density: None,
            music_genre: None,
            performer_or_dj: None,
            reporter_poh_score: None,
            timestamp_s: now_s - 2000, // > 1800s default max age
        });

        engine.prune_stale_signals(now_s);
        let signals = engine.signals_by_venue.get(&venue_id).unwrap();
        assert!(signals.is_empty());
    }
}
