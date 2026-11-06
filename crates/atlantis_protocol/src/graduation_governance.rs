//! # Atlantis Network Graduation, Feature Proposals & Core Standards Governance
//!
//! Provides the governance and promotion contracts allowing third-party teams to build
//! custom microservices, radio stations, storefronts, and daemons locally,
//! and submit them for Core Team audit and graduation to the authoritative global Nautilus network.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use crate::lumeria_sdk::AppCapability;

/// Specific type of network feature a team can develop and request graduation for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NetworkFeatureKind {
    /// 24/7 Siren Radio broadcast station with live booth operator takeovers.
    PublicRadioStation {
        station_name: String,
        callsign: String,
        harmonic_hz: f32,
        stream_endpoint_url: String,
        booth_operator_did: String,
        sabine_rt60_s: f32,
    },
    /// Interactive 3D storefront / marketplace with Price Guard protected catalogs.
    PublicStorefront {
        store_name: String,
        catalog_root_hash: String,
        totem_asset_uri: String,
        interaction_radius_m: f32,
    },
    /// Exterior urban jumbotron / video billboard visible in city streets.
    ExteriorJumbotron {
        billboard_name: String,
        video_stream_uri: String,
        screen_dimensions_m: [f32; 2],
        emissive_lumens: f32,
    },
    /// Autonomous edge companion or worker daemon (e.g. courier, security patrol).
    AutonomousWorkerDaemon {
        daemon_name: String,
        service_did: String,
        required_canary_latency_ms: u32,
    },
}

/// The promotion lifecycle status of a network feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureGraduationStatus {
    /// Functional only inside the developer's local app or private LAN.
    DraftLocal,
    /// Submitted to the Core Team for audit, PoH verification, and safety checks.
    PendingCoreReview,
    /// Approved by the Core Team and graduated to the global authoritative Nautilus twin.
    GraduatedGlobal,
    /// Rejected due to invariant violations (e.g. coordinate drift, security risks).
    Rejected,
    /// Revoked or sunset from the global network.
    Revoked,
}

/// Formal proposal submitted by a team to request global graduation for their feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureGraduationProposal {
    pub proposal_id: String,
    pub team_did: String,
    pub team_name: String,
    /// Authoritative WGS84 coordinates where the feature's building is anchored.
    pub anchor_wgs84: [f64; 3], // [lat, lon, alt_meters]
    pub feature: NetworkFeatureKind,
    pub requested_capabilities: HashSet<AppCapability>,
    pub status: FeatureGraduationStatus,
    /// Cryptographic signature of the proposal by the submitting team.
    pub submitter_signature: String,
    /// Cryptographic signature of the Core Team authority upon approval.
    pub core_approval_signature: Option<String>,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

impl FeatureGraduationProposal {
    pub fn new(
        proposal_id: impl Into<String>,
        team_did: impl Into<String>,
        team_name: impl Into<String>,
        anchor_wgs84: [f64; 3],
        feature: NetworkFeatureKind,
        requested_capabilities: HashSet<AppCapability>,
        timestamp_unix: u64,
    ) -> Self {
        Self {
            proposal_id: proposal_id.into(),
            team_did: team_did.into(),
            team_name: team_name.into(),
            anchor_wgs84,
            feature,
            requested_capabilities,
            status: FeatureGraduationStatus::DraftLocal,
            submitter_signature: String::new(),
            core_approval_signature: None,
            created_at_unix: timestamp_unix,
            updated_at_unix: timestamp_unix,
        }
    }

    /// Submits the draft proposal for official Core Team review.
    pub fn submit_for_review(&mut self, submitter_sig: impl Into<String>, timestamp_unix: u64) -> Result<(), String> {
        if self.status != FeatureGraduationStatus::DraftLocal && self.status != FeatureGraduationStatus::Rejected {
            return Err("Proposal is already submitted or graduated".into());
        }
        self.submitter_signature = submitter_sig.into();
        self.status = FeatureGraduationStatus::PendingCoreReview;
        self.updated_at_unix = timestamp_unix;
        Ok(())
    }

    /// Evaluates proposal against Core Network Invariants before granting graduation.
    pub fn audit_invariants(&self) -> Result<(), String> {
        // Invariant 1: Valid WGS84 Geodesy
        let [lat, lon, alt] = self.anchor_wgs84;
        if lat < -90.0 || lat > 90.0 || lon < -180.0 || lon > 180.0 {
            return Err("Invalid WGS84 geodesy coordinates: latitude [-90, 90], longitude [-180, 180]".into());
        }
        if alt < -11000.0 || alt > 100000.0 {
            return Err("Altitude exceeds planetary operational bounds (-11km Mariana Trench to 100km Karman Line)".into());
        }

        // Invariant 2: Capability Permission Guard
        if self.requested_capabilities.contains(&AppCapability::StoreInitiateCheckout) {
            match &self.feature {
                NetworkFeatureKind::PublicStorefront { catalog_root_hash, .. } => {
                    if catalog_root_hash.is_empty() {
                        return Err("Storefront requesting checkout capability must provide a valid catalog_root_hash".into());
                    }
                }
                _ => {
                    return Err("Only PublicStorefront features can request StoreInitiateCheckout".into());
                }
            }
        }

        // Invariant 3: Radio Frequency Guard
        if let NetworkFeatureKind::PublicRadioStation { harmonic_hz, .. } = &self.feature {
            if *harmonic_hz < 20.0 || *harmonic_hz > 20000.0 {
                return Err("Radio station harmonic frequency must be within audible spectrum [20Hz, 20000Hz]".into());
            }
        }

        Ok(())
    }

    /// Grants core team graduation, promoting the feature to the global authoritative network.
    pub fn graduate(
        &mut self,
        core_authority_did: &str,
        core_signature: impl Into<String>,
        timestamp_unix: u64,
    ) -> Result<(), String> {
        if self.status != FeatureGraduationStatus::PendingCoreReview {
            return Err("Only proposals in PendingCoreReview can be graduated".into());
        }

        // Must strictly pass invariant audit
        self.audit_invariants()?;

        if !core_authority_did.contains("ursula-node-manager") && !core_authority_did.contains("core-authority") {
            return Err("Only authorized Core Team DIDs may approve graduation".into());
        }

        self.status = FeatureGraduationStatus::GraduatedGlobal;
        self.core_approval_signature = Some(core_signature.into());
        self.updated_at_unix = timestamp_unix;
        Ok(())
    }
}

/// Registry of all graduated global features visible on the Nautilus World Server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalFeatureRegistry {
    pub graduated_features: Vec<FeatureGraduationProposal>,
}

impl GlobalFeatureRegistry {
    pub fn new() -> Self {
        Self {
            graduated_features: Vec::new(),
        }
    }

    /// Inserts an approved proposal if it holds valid core approval.
    pub fn register_graduated(&mut self, proposal: FeatureGraduationProposal) -> Result<(), String> {
        if proposal.status != FeatureGraduationStatus::GraduatedGlobal || proposal.core_approval_signature.is_none() {
            return Err("Proposal must be GraduatedGlobal with valid core signature to register".into());
        }
        self.graduated_features.push(proposal);
        Ok(())
    }

    /// Resolves all active global radio stations on the network.
    pub fn list_global_radio_stations(&self) -> Vec<(&str, &str, f32, &str)> {
        self.graduated_features
            .iter()
            .filter_map(|p| match &p.feature {
                NetworkFeatureKind::PublicRadioStation {
                    station_name,
                    callsign,
                    harmonic_hz,
                    stream_endpoint_url,
                    ..
                } => Some((
                    station_name.as_str(),
                    callsign.as_str(),
                    *harmonic_hz,
                    stream_endpoint_url.as_str(),
                )),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_proposal_lifecycle_and_graduation() {
        let mut caps = HashSet::new();
        caps.insert(AppCapability::AudioSpatialStream);

        let feature = NetworkFeatureKind::PublicRadioStation {
            station_name: "Sub-Trench Underground Radio".into(),
            callsign: "SIREN-SUB-99".into(),
            harmonic_hz: 528.0,
            stream_endpoint_url: "http://subnode.lan:8080/live.mp3".into(),
            booth_operator_did: "did:atlantis:guest-dj-42".into(),
            sabine_rt60_s: 0.35,
        };

        let mut proposal = FeatureGraduationProposal::new(
            "prop_radio_001",
            "did:atlantis:team-nebula",
            "Team Nebula",
            [35.6762, 139.6503, 15.0], // Tokyo WGS84
            feature,
            caps,
            1729860000,
        );

        assert_eq!(proposal.status, FeatureGraduationStatus::DraftLocal);

        // Submit for review
        proposal.submit_for_review("sig_team_nebula_01", 1729860100).expect("submit");
        assert_eq!(proposal.status, FeatureGraduationStatus::PendingCoreReview);

        // Reject unauthorized approval
        let rogue_res = proposal.graduate("did:atlantis:random_user", "sig_bad", 1729860200);
        assert!(rogue_res.is_err());

        // Approve with Core Team Authority
        proposal
            .graduate(
                "did:atlantis:ursula-node-manager",
                "sig_core_ursula_approved",
                1729860300,
            )
            .expect("graduation succeeds");

        assert_eq!(proposal.status, FeatureGraduationStatus::GraduatedGlobal);

        // Register in global registry
        let mut registry = GlobalFeatureRegistry::new();
        registry.register_graduated(proposal).expect("register");
        assert_eq!(registry.list_global_radio_stations().len(), 1);
    }

    #[test]
    fn test_invariant_audit_rejections() {
        // Bad latitude
        let feature = NetworkFeatureKind::ExteriorJumbotron {
            billboard_name: "Times Square Screen".into(),
            video_stream_uri: "https://stream.example/live.m3u8".into(),
            screen_dimensions_m: [16.0, 9.0],
            emissive_lumens: 5.0,
        };

        let proposal = FeatureGraduationProposal::new(
            "prop_bad_geo",
            "did:atlantis:team-bad",
            "Team Bad",
            [120.0, 0.0, 0.0], // Invalid latitude > 90
            feature,
            HashSet::new(),
            1729860000,
        );

        assert!(proposal.audit_invariants().is_err());
    }
}
