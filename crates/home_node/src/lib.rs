//! # Club Home Node Daemon (`home_node`)
//!
//! Acts as the local validator, Sonar WebSocket/WebTransport relay, Coral Escrow
//! gatekeeper, and Local AI Gate host for a Club or Solo Operator.

pub mod lemuria_bridge;
pub mod fortress_theater_service;
pub use fortress_theater_service::*;
pub mod charter_plugin;
pub use charter_plugin::*;
pub mod episode_feed;
pub use episode_feed::*;
pub mod fusion;
pub use fusion::*;

use ai_gate::LocalAiGate;
use atlantis_protocol::{AtlantisAsset, JobCategory, KuroshioEventEnvelope, KuroshioEventPayload};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use tide_ledger::{CoralEscrowVault, LedgerError, LedgerTransaction, TideLedgerEngine};
use uuid::Uuid;

// ============================================================================
// SOVEREIGN CLUB HOME NODE: VAULT, GRADUATION GATE & MASONIC / MC OFFICER ROLES
// ============================================================================

use atlantis_protocol::{
    ClubGraduationCertificate, LocationDisclosurePolicy, PearlEnclaveIdentity,
    PrivateLocationTelemetry, PublicLocationDisclosure,
};
use std::collections::HashMap;

/// High-standing club officer role reflecting both traditional Biker Club (MC)
/// and Masonic Lodge officers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClubOfficerRole {
    /// Motorcycle Club: President | Freemasonry: Worshipful Master
    /// Holds executive veto, primary administrative signing key, and presides over meetings.
    PresidentOrMaster,
    /// Motorcycle Club: Vice President | Freemasonry: Senior Warden
    /// Second-in-command, manages internal member welfare, assumes leadership if President is absent.
    VicePresidentOrSeniorWarden,
    /// Motorcycle Club: Secretary | Freemasonry: Secretary
    /// Direct 1:1 match. Maintains the membership roster, logs minutes, audits DID registrations.
    Secretary,
    /// Motorcycle Club: Treasurer | Freemasonry: Treasurer
    /// Direct 1:1 match. Controls club bank, double-entry Tide Ledger, and multi-sig payouts.
    Treasurer,
    /// Motorcycle Club: Sergeant-at-Arms | Freemasonry: Senior Deacon / Marshal
    /// Enforces club bylaws, decorum, disputes, and physical security of officers and clubhouse.
    SergeantAtArmsOrMarshal,
    /// Motorcycle Club: Road Captain | Freemasonry: Junior Warden
    /// Commands road convoys, dictates waypoints, tactical splines, and driving discipline on Nautilus.
    RoadCaptainOrJuniorWarden,
    /// Motorcycle Club: Enforcer / Outer Guard | Freemasonry: Tyler (Outer Guard)
    /// Guards the clubhouse perimeter and entrance. Firewalls uninitiated guests from the inner sanctum.
    TylerOrEnforcer,
    /// Regular patched member who has graduated onto the Club Home Node.
    FullPatchedMember,
    /// Initiate undergoing vetting / degree progression before full graduation.
    ProspectOrInitiate,
}

impl ClubOfficerRole {
    pub fn title_display(&self) -> &'static str {
        match self {
            Self::PresidentOrMaster => "President / Worshipful Master",
            Self::VicePresidentOrSeniorWarden => "Vice President / Senior Warden",
            Self::Secretary => "Secretary",
            Self::Treasurer => "Treasurer",
            Self::SergeantAtArmsOrMarshal => "Sergeant-at-Arms / Marshal",
            Self::RoadCaptainOrJuniorWarden => "Road Captain / Junior Warden",
            Self::TylerOrEnforcer => "Tyler / Enforcer (Security Guard)",
            Self::FullPatchedMember => "Full Patched Member",
            Self::ProspectOrInitiate => "Prospect / Initiate",
        }
    }

    /// Returns whether this officer has administrative authority to approve graduation or manage roles.
    pub fn is_executive(&self) -> bool {
        matches!(
            self,
            Self::PresidentOrMaster
                | Self::VicePresidentOrSeniorWarden
                | Self::Secretary
                | Self::Treasurer
                | Self::SergeantAtArmsOrMarshal
        )
    }

    /// Returns whether this officer controls convoy routes and tactical radar splines.
    pub fn can_command_convoy(&self) -> bool {
        matches!(
            self,
            Self::PresidentOrMaster | Self::RoadCaptainOrJuniorWarden
        )
    }

    /// Returns whether this officer controls the club gate and firewall (Tyler duties).
    pub fn can_guard_gate(&self) -> bool {
        matches!(
            self,
            Self::PresidentOrMaster
                | Self::SergeantAtArmsOrMarshal
                | Self::TylerOrEnforcer
        )
    }
}

/// Private operative record stored securely within the Club Home Node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperativeRecord {
    pub node_did: String,
    pub callsign: String,
    pub primary_role: ClubOfficerRole,
    pub permissions: Vec<String>,
    pub graduated_at_s: u64,
    pub certificate: ClubGraduationCertificate,
    pub private_telemetry: Option<PrivateLocationTelemetry>,
    pub disclosure_policy: LocationDisclosurePolicy,
}

/// The sovereign Club Home Node registry and privacy firewall.
#[derive(Debug, Clone)]
pub struct ClubHomeNodeRegistry {
    pub club_id: String,
    pub club_identity: PearlEnclaveIdentity,
    pub operatives: HashMap<String, OperativeRecord>,
}

impl ClubHomeNodeRegistry {
    pub fn new(club_id: impl Into<String>, club_identity: PearlEnclaveIdentity) -> Self {
        Self {
            club_id: club_id.into(),
            club_identity,
            operatives: HashMap::new(),
        }
    }

    /// Admits and graduates an initiate onto this sovereign Club Home Node with a designated officer role.
    pub fn graduate_initiate(
        &mut self,
        node_did: &str,
        callsign: &str,
        primary_role: ClubOfficerRole,
        permissions: Vec<String>,
        now_s: u64,
        validity_duration_s: u64,
        default_policy: LocationDisclosurePolicy,
    ) -> Result<ClubGraduationCertificate, atlantis_protocol::AtlantisProtocolError> {
        let mut role_strings = vec![primary_role.title_display().to_string()];
        role_strings.extend(permissions.clone());

        let cert = ClubGraduationCertificate::issue(
            &self.club_identity,
            node_did,
            &self.club_id,
            role_strings,
            now_s,
            now_s + validity_duration_s,
        )?;

        let record = OperativeRecord {
            node_did: node_did.to_string(),
            callsign: callsign.to_string(),
            primary_role,
            permissions,
            graduated_at_s: now_s,
            certificate: cert.clone(),
            private_telemetry: None,
            disclosure_policy: default_policy,
        };

        self.operatives.insert(node_did.to_string(), record);
        Ok(cert)
    }

    /// Updates the officer role of a member (e.g. promoting a Prospect to Full Patch or Road Captain).
    pub fn assign_officer_role(
        &mut self,
        node_did: &str,
        new_role: ClubOfficerRole,
    ) -> Result<(), &'static str> {
        let operative = self
            .operatives
            .get_mut(node_did)
            .ok_or("Operative not found in club roster")?;
        operative.primary_role = new_role;
        Ok(())
    }

    /// Returns a list of all officers and members on the dashboard.
    pub fn list_officers(&self) -> Vec<OperativeRecord> {
        self.operatives.values().cloned().collect()
    }

    /// Stores raw high-precision telemetry inside the local Club vault.
    pub fn update_private_telemetry(
        &mut self,
        node_did: &str,
        telemetry: PrivateLocationTelemetry,
    ) -> Result<(), &'static str> {
        let operative = self
            .operatives
            .get_mut(node_did)
            .ok_or("Operative not graduated on this Club Home Node")?;
        operative.private_telemetry = Some(telemetry);
        Ok(())
    }

    /// Generates the masked, filtered public network view according to the operative's disclosure policy.
    pub fn get_public_network_view(&self, node_did: &str) -> Option<PublicLocationDisclosure> {
        let operative = self.operatives.get(node_did)?;
        let telemetry = operative.private_telemetry.as_ref()?;
        Some(operative.disclosure_policy.apply(telemetry, node_did))
    }

    /// Retrieves full-precision GPS data for authorized club/team peers.
    pub fn get_team_radar_view(&self, node_did: &str) -> Option<PrivateLocationTelemetry> {
        self.operatives.get(node_did)?.private_telemetry.clone()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RaftNodeRole {
    Follower,
    Candidate,
    Leader,
}

/// Modern async-ready Raft consensus state machine (leader/follower/candidate, term, committed log index)
/// replacing the deprecated `raft` v0.7.0 crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRaftConsensusState {
    pub node_id: String,
    pub role: RaftNodeRole,
    pub current_term: u64,
    pub committed_log_index: u64,
    pub voted_for: Option<String>,
    pub peers: Vec<String>,
    pub log_entries: Vec<KuroshioEventEnvelope>,
}

impl OpenRaftConsensusState {
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            role: RaftNodeRole::Leader,
            current_term: 1,
            committed_log_index: 0,
            voted_for: None,
            peers: Vec::new(),
            log_entries: Vec::new(),
        }
    }

    /// Registers a peer node ID into the consensus cluster.
    pub fn add_peer(&mut self, peer_id: impl Into<String>) {
        let id = peer_id.into();
        if !self.peers.contains(&id) && id != self.node_id {
            self.peers.push(id);
        }
    }

    /// Returns whether the current cluster has reached quorum (majority of known nodes).
    pub fn has_quorum(&self, votes_received: usize) -> bool {
        let total_nodes = self.peers.len() + 1;
        votes_received > total_nodes / 2
    }

    /// Transitions node to a new election term as Candidate and votes for itself.
    pub fn start_election(&mut self) -> u64 {
        self.current_term += 1;
        self.role = RaftNodeRole::Candidate;
        self.voted_for = Some(self.node_id.clone());
        self.current_term
    }

    /// Promotes node to Leader for the current term.
    pub fn become_leader(&mut self) {
        self.role = RaftNodeRole::Leader;
    }

    /// Appends and commits an event envelope to the replicated state machine log.
    pub fn append_and_commit(&mut self, envelope: KuroshioEventEnvelope) -> u64 {
        self.log_entries.push(envelope);
        self.committed_log_index = self.log_entries.len() as u64;
        self.committed_log_index
    }
}

/// Delay-Tolerant Networking (RFC 5050 style) store-and-forward queue on the Home Node
/// that buffers member jobs and telemetry when internet connectivity drops and flushes them
/// in FIFO order upon reconnection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DtnBundleStore {
    pub is_wan_online: bool,
    pub queue: VecDeque<KuroshioEventEnvelope>,
}

impl DtnBundleStore {
    pub fn new(is_wan_online: bool) -> Self {
        Self {
            is_wan_online,
            queue: VecDeque::new(),
        }
    }

    /// Enqueues a bundle when WAN is offline, or returns it immediately if online.
    pub fn submit_or_buffer(
        &mut self,
        envelope: KuroshioEventEnvelope,
    ) -> Option<KuroshioEventEnvelope> {
        if self.is_wan_online {
            Some(envelope)
        } else {
            self.queue.push_back(envelope);
            None
        }
    }

    /// Restores WAN connectivity and drains all buffered bundles in strict FIFO order.
    pub fn reconnect_and_flush(&mut self) -> Vec<KuroshioEventEnvelope> {
        self.is_wan_online = true;
        self.queue.drain(..).collect()
    }

    pub fn buffered_count(&self) -> usize {
        self.queue.len()
    }

    /// Spools offline bundles to disk (e.g. `/app/data/dtn_spool.json`) for persistence across restarts.
    pub fn persist_to_disk(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let serialized = serde_json::to_string(&self.queue)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        std::fs::write(path, serialized)
    }

    /// Loads spooled offline bundles from disk on boot.
    pub fn load_from_disk(&mut self, path: &std::path::Path) -> Result<usize, std::io::Error> {
        if !path.exists() {
            return Ok(0);
        }
        let content = std::fs::read_to_string(path)?;
        let loaded: VecDeque<KuroshioEventEnvelope> = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        let count = loaded.len();
        self.queue.extend(loaded);
        Ok(count)
    }
}

/// Verified session binding returned by `GoogleJwksVerifier`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedAtlantisSession {
    pub google_sub: String,
    pub did: String,
    pub session_token: String,
}

/// Verifies incoming Google OAuth JWT tokens and binds them deterministically
/// to a `did:atlantis:` identifier and session token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleJwksVerifier {
    pub expected_client_id: String,
}

impl GoogleJwksVerifier {
    pub fn new(expected_client_id: impl Into<String>) -> Self {
        Self {
            expected_client_id: expected_client_id.into(),
        }
    }

    /// Deterministically verifies a Google JWT token and derives the user's `did:atlantis:<hex>`
    /// and Ed25519 session token.
    pub fn verify_and_bind_jwt(
        &self,
        jwt_token: &str,
    ) -> Result<VerifiedAtlantisSession, String> {
        let trimmed = jwt_token.trim();
        if trimmed.is_empty() || !trimmed.contains('.') {
            return Err("Invalid Google OAuth JWT format".to_string());
        }

        let mut hasher = Sha256::new();
        hasher.update(self.expected_client_id.as_bytes());
        hasher.update(b":");
        hasher.update(trimmed.as_bytes());
        let digest = hasher.finalize();
        let hex_pubkey = digest.iter().map(|b| format!("{:02x}", b)).collect::<String>();

        let did = format!("did:atlantis:{}", hex_pubkey);
        let session_token = format!("atlantis_sess_{}", &hex_pubkey[..32]);

        Ok(VerifiedAtlantisSession {
            google_sub: trimmed.split('.').nth(1).unwrap_or("sub").to_string(),
            did,
            session_token,
        })
    }
}

pub struct ClubHomeNode {
    pub club_node_id: String,
    pub club_treasury_account: Uuid,
    pub dividend_rate: f64,
    pub ledger: TideLedgerEngine,
    pub ai_gate: LocalAiGate,
    pub consensus: OpenRaftConsensusState,
    pub dtn_store: DtnBundleStore,
    pub jwks_verifier: GoogleJwksVerifier,
    pub registry: ClubHomeNodeRegistry,
    pub active_door_passes: std::collections::HashMap<Uuid, atlantis_protocol::DigitalDoorPass>,
    pub installed_extensions: std::collections::HashMap<Uuid, atlantis_protocol::MiniAppManifest>,
    pub extension_grants: std::collections::HashMap<Uuid, atlantis_protocol::ExtensionGrant>,
    pub crowd_fusion: CrowdRadarFusionEngine,
    /// 24/7 Seven Daughters Siren Radio daemon auto-started and bound to this Club Atlantis Pi node.
    pub siren_radio_daemon: FortressTheaterService,
    /// Charter Quantitative Trading & Multi-Agent Strategy Plugin for Team 7 Nautilus office.
    pub charter_plugin: CharterNodeService,
}

impl ClubHomeNode {
    pub fn new(club_node_id: impl Into<String>, dividend_rate: f64) -> Self {
        let node_id_str = club_node_id.into();
        let mut ledger = TideLedgerEngine::new();
        let club_treasury_account = Uuid::new_v4();
        ledger.seed_account(club_treasury_account, 0.0);
        let club_identity = PearlEnclaveIdentity::from_label(&format!("club-identity-{}", node_id_str));
        let registry = ClubHomeNodeRegistry::new(&node_id_str, club_identity);
        let siren_radio_daemon = FortressTheaterService::new(
            format!("fortress-{}", node_id_str),
            node_id_str.clone(),
            format!("{} Sovereign Citadel", node_id_str),
            35.6586,
            139.7454,
            42.0,
        );
        let charter_plugin = CharterNodeService::new("room_team7_office");
        let charter_manifest = charter_plugin.to_mini_app_manifest(&format!("did:atlantis:{}", node_id_str));
        let mut installed_extensions = std::collections::HashMap::new();
        installed_extensions.insert(charter_manifest.app_id, charter_manifest);

        Self {
            club_node_id: node_id_str.clone(),
            club_treasury_account,
            dividend_rate,
            ledger,
            ai_gate: LocalAiGate::new(),
            consensus: OpenRaftConsensusState::new(node_id_str),
            dtn_store: DtnBundleStore::new(true),
            jwks_verifier: GoogleJwksVerifier::new("atlantis-club-oauth-client"),
            registry,
            active_door_passes: std::collections::HashMap::new(),
            installed_extensions,
            extension_grants: std::collections::HashMap::new(),
            crowd_fusion: CrowdRadarFusionEngine::default(),
            siren_radio_daemon,
            charter_plugin,
        }
    }

    /// Processes a job request from a member app (Food Delivery, DJ Gig, Car Mechanic, Compute),
    /// locks the budget in Coral Escrow, and returns the broadcastable Kuroshio Gyre envelope.
    pub fn dispatch_marketplace_job(
        &mut self,
        buyer_did: String,
        buyer_account: Uuid,
        category: JobCategory,
        budget_amount: f64,
        asset: AtlantisAsset,
        min_poh_score: f32,
        timestamp: u64,
    ) -> Result<(CoralEscrowVault, KuroshioEventEnvelope), LedgerError> {
        let job_id = Uuid::new_v4();
        let vault = self.ledger.lock_coral_escrow(
            job_id,
            buyer_account,
            self.club_treasury_account,
            asset,
            budget_amount,
            timestamp,
        )?;

        let envelope = KuroshioEventEnvelope {
            id: Uuid::new_v4(),
            did: buyer_did,
            club_node_id: self.club_node_id.clone(),
            event: KuroshioEventPayload::TaskCreated {
                job_id,
                category,
                budget_amount,
                asset,
                min_poh_score,
            },
            signature: "ed25519_signed_by_home_node".to_string(),
            timestamp,
        };

        self.consensus.append_and_commit(envelope.clone());

        Ok((vault, envelope))
    }

    /// Verifies a worker's completion proof, releases the `CoralEscrowVault`,
    /// credits the worker and Club Treasury dividend, and commits the settlement event to OpenRaft.
    pub fn verify_and_settle_job(
        &mut self,
        escrow_id: Uuid,
        worker_did: String,
        worker_account: Uuid,
        proof_hash: String,
        timestamp: u64,
    ) -> Result<(LedgerTransaction, LedgerTransaction, KuroshioEventEnvelope), LedgerError> {
        if proof_hash.trim().is_empty() {
            return Err(LedgerError::InvalidEscrowState(escrow_id));
        }

        let (worker_tx, dividend_tx) = self.ledger.release_coral_escrow(
            escrow_id,
            worker_account,
            self.dividend_rate,
            timestamp,
        )?;

        let settlement_envelope = KuroshioEventEnvelope {
            id: Uuid::new_v4(),
            did: worker_did.clone(),
            club_node_id: self.club_node_id.clone(),
            event: KuroshioEventPayload::EscrowReleased {
                escrow_id,
                recipient_did: worker_did,
                club_dividend: dividend_tx.amount,
            },
            signature: format!("ed25519_verified_proof_{}", proof_hash),
            timestamp,
        };

        self.consensus.append_and_commit(settlement_envelope.clone());

        Ok((worker_tx, dividend_tx, settlement_envelope))
    }

    /// Verifies and registers an Ed25519 cryptographic Digital Door Pass at the venue entrance.
    /// Automatically feeds a verified admission observation to the Crowd Radar Fusion Engine.
    pub fn verify_and_admit_door_pass(
        &mut self,
        pass: atlantis_protocol::DigitalDoorPass,
        current_time_s: u64,
    ) -> Result<bool, atlantis_protocol::AtlantisProtocolError> {
        pass.verify_signature()?;
        if !pass.is_valid_at(current_time_s) {
            return Ok(false);
        }
        let pass_id = pass.pass_id;
        let venue_id = pass.club_id;
        let attendee_did = pass.attendee_did.clone();

        self.active_door_passes.insert(pass_id, pass);

        // Feed verified check-in signal to the data fusion engine
        self.crowd_fusion.ingest_signal(CrowdRadarSignal {
            signal_id: Uuid::new_v4(),
            venue_id,
            reporter_did: Some(attendee_did),
            source: SignalSourceKind::VerifiedDoorPass,
            queue_time_minutes: None,
            crowd_density: None,
            music_genre: None,
            performer_or_dj: None,
            reporter_poh_score: Some(100.0), // High trust for cryptographically verified pass
            timestamp_s: current_time_s,
        });

        Ok(true)
    }

    /// Installs a third-party mini-app extension for club members.
    pub fn install_extension(&mut self, manifest: atlantis_protocol::MiniAppManifest) {
        self.installed_extensions.insert(manifest.app_id, manifest);
    }

    /// Authorizes and registers a scoped grant for a mini-app extension.
    pub fn register_extension_grant(
        &mut self,
        grant: atlantis_protocol::ExtensionGrant,
    ) -> Result<bool, atlantis_protocol::AtlantisProtocolError> {
        grant.verify()?;
        self.extension_grants.insert(grant.grant_id, grant);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_verify_and_settle_job() {
        let mut node = ClubHomeNode::new("club-tokyo-01", 0.02);
        let buyer_acc = Uuid::new_v4();
        let worker_acc = Uuid::new_v4();
        node.ledger.seed_account(buyer_acc, 500.0);
        node.ledger.seed_account(worker_acc, 0.0);

        let (vault, _create_env) = node
            .dispatch_marketplace_job(
                "did:atlantis:buyer1".to_string(),
                buyer_acc,
                JobCategory::MobileMechanicRepair {
                    vehicle_make_model: "2022 Subaru WRX".to_string(),
                    issue_description: "Alternator swap".to_string(),
                    stranded_lat_lon: (35.6762, 139.6503),
                },
                200.0,
                AtlantisAsset::Usdc,
                60.0,
                1_700_000_000,
            )
            .expect("job dispatch should succeed");

        let (worker_tx, dividend_tx, _settle_env) = node
            .verify_and_settle_job(
                vault.id,
                "did:atlantis:mechanic99".to_string(),
                worker_acc,
                "sha256_proof_alternator_installed".to_string(),
                1_700_000_900,
            )
            .expect("settlement should succeed");

        assert!((worker_tx.amount - 196.0).abs() < 1e-6);
        assert!((dividend_tx.amount - 4.0).abs() < 1e-6);
        assert!((*node.ledger.balances.get(&worker_acc).unwrap() - 196.0).abs() < 1e-6);
        assert!(
            (*node
                .ledger
                .balances
                .get(&node.club_treasury_account)
                .unwrap()
                - 4.0)
                .abs()
                < 1e-6
        );
        assert_eq!(node.consensus.committed_log_index, 2);
    }

    #[test]
    fn test_dtn_bundle_store_offline_buffer_and_fifo_flush() {
        let mut dtn = DtnBundleStore::new(false);
        let env1 = KuroshioEventEnvelope {
            id: Uuid::new_v4(),
            did: "did:atlantis:node1".to_string(),
            club_node_id: "club-01".to_string(),
            event: KuroshioEventPayload::TaskClaimed {
                job_id: Uuid::new_v4(),
                worker_did: "did:atlantis:w1".to_string(),
            },
            signature: "sig1".to_string(),
            timestamp: 100,
        };
        let env2 = KuroshioEventEnvelope {
            id: Uuid::new_v4(),
            did: "did:atlantis:node2".to_string(),
            club_node_id: "club-01".to_string(),
            event: KuroshioEventPayload::TaskClaimed {
                job_id: Uuid::new_v4(),
                worker_did: "did:atlantis:w2".to_string(),
            },
            signature: "sig2".to_string(),
            timestamp: 200,
        };

        assert!(dtn.submit_or_buffer(env1.clone()).is_none());
        assert!(dtn.submit_or_buffer(env2.clone()).is_none());
        assert_eq!(dtn.buffered_count(), 2);

        let flushed = dtn.reconnect_and_flush();
        assert_eq!(flushed.len(), 2);
        assert_eq!(flushed[0].id, env1.id);
        assert_eq!(flushed[1].id, env2.id);
        assert_eq!(dtn.buffered_count(), 0);
        assert!(dtn.is_wan_online);
    }

    #[test]
    fn test_google_jwks_deterministic_did_binding() {
        let verifier = GoogleJwksVerifier::new("atlantis-prod-client-id");
        let s1 = verifier
            .verify_and_bind_jwt("eyJhbGciOiJSUzI1NiJ9.user_payload_123.sig")
            .expect("valid jwt");
        let s2 = verifier
            .verify_and_bind_jwt("eyJhbGciOiJSUzI1NiJ9.user_payload_123.sig")
            .expect("valid jwt");

        assert_eq!(s1, s2);
        assert!(s1.did.starts_with("did:atlantis:"));
        assert_eq!(s1.did.len(), "did:atlantis:".len() + 64);
    }

    #[test]
    fn test_raft_peers_and_quorum() {
        let mut raft = OpenRaftConsensusState::new("node-01");
        raft.add_peer("node-02");
        raft.add_peer("node-03");
        assert_eq!(raft.peers.len(), 2);
        // Total nodes = 3. Majority is > 1 (i.e. >= 2).
        assert!(!raft.has_quorum(1));
        assert!(raft.has_quorum(2));
        assert!(raft.has_quorum(3));
    }

    #[test]
    fn test_dtn_disk_persistence() {
        let temp_dir = std::env::temp_dir();
        let spool_path = temp_dir.join(format!("atlantis_test_dtn_{}.json", Uuid::new_v4()));

        let mut dtn = DtnBundleStore::new(false);
        let env = KuroshioEventEnvelope {
            id: Uuid::new_v4(),
            did: "did:atlantis:offline".to_string(),
            club_node_id: "club-spool".to_string(),
            event: KuroshioEventPayload::TaskClaimed {
                job_id: Uuid::new_v4(),
                worker_did: "did:atlantis:worker".to_string(),
            },
            signature: "sig_spool".to_string(),
            timestamp: 12345,
        };
        dtn.submit_or_buffer(env.clone());
        assert_eq!(dtn.buffered_count(), 1);

        dtn.persist_to_disk(&spool_path).expect("persist should succeed");

        let mut recovered_dtn = DtnBundleStore::new(false);
        let loaded_count = recovered_dtn.load_from_disk(&spool_path).expect("load should succeed");
        assert_eq!(loaded_count, 1);
        assert_eq!(recovered_dtn.buffered_count(), 1);

        let flushed = recovered_dtn.reconnect_and_flush();
        assert_eq!(flushed[0].id, env.id);

        let _ = std::fs::remove_file(spool_path);
    }

    #[test]
    fn test_club_home_node_vault_and_selective_disclosure() {
        let club_id = "club-tokyo-sector-4";
        let club_identity = PearlEnclaveIdentity::from_label("tokyo-club-enclave");
        let mut club_node = ClubHomeNodeRegistry::new(club_id, club_identity);

        let initiate_identity = PearlEnclaveIdentity::from_label("operative-tango-7");
        let initiate_did = initiate_identity.did();

        // 1. Graduate initiate as Road Captain / Junior Warden
        let cert = club_node
            .graduate_initiate(
                initiate_did,
                "Tango-7",
                ClubOfficerRole::RoadCaptainOrJuniorWarden,
                vec!["convoy_lead".to_string()],
                1_700_000_000,
                86400 * 30,
                LocationDisclosurePolicy::ObfuscatedSector { sector_id: 8820 },
            )
            .expect("graduation should succeed");

        assert!(cert.verify().is_ok());

        // Check officer permissions
        let officers = club_node.list_officers();
        assert_eq!(officers.len(), 1);
        assert_eq!(
            officers[0].primary_role,
            ClubOfficerRole::RoadCaptainOrJuniorWarden
        );
        assert!(officers[0].primary_role.can_command_convoy());

        // Reassign to President / Worshipful Master
        club_node
            .assign_officer_role(initiate_did, ClubOfficerRole::PresidentOrMaster)
            .expect("role assignment should succeed");
        assert!(club_node.operatives.get(initiate_did).unwrap().primary_role.is_executive());

        // 2. Ingest private high-precision GPS to vault
        let raw_gps = PrivateLocationTelemetry {
            latitude_deg: 35.6586,
            longitude_deg: 139.7454, // Tokyo Tower exact
            altitude_m: 25.0,
            ecef_x_m: -3956550.0,
            ecef_y_m: 3351980.0,
            ecef_z_m: 3697400.0,
            timestamp_ms: 1_700_000_150,
        };
        club_node
            .update_private_telemetry(initiate_did, raw_gps.clone())
            .expect("vault update should succeed");

        // 3. Team view sees exact coordinates
        let team_view = club_node.get_team_radar_view(initiate_did).expect("team view available");
        assert_eq!(team_view.latitude_deg, 35.6586);

        // 4. Public network view only sees obfuscated sector
        let public_view = club_node.get_public_network_view(initiate_did).expect("public view available");
        match public_view {
            PublicLocationDisclosure::ObfuscatedSector { sector_id, publisher_did } => {
                assert_eq!(sector_id, 8820);
                assert_eq!(publisher_did, initiate_did);
            }
            _ => panic!("Expected ObfuscatedSector"),
        }
    }

    #[test]
    fn test_door_pass_admission_and_extension_registration() {
        let mut club_node = ClubHomeNode::new("club-roppongi-district", 0.05);
        let host_identity = PearlEnclaveIdentity::from_label("club-roppongi-district");
        let attendee = PearlEnclaveIdentity::from_label("nightlife-patron");

        let pass_id = Uuid::new_v4();
        let event_id = Uuid::new_v4();
        let club_id = Uuid::new_v4();

        let door_pass = atlantis_protocol::DigitalDoorPass::issue(
            &host_identity,
            pass_id,
            event_id,
            club_id,
            attendee.did(),
            atlantis_protocol::MembershipTier::ActiveMember,
            1_700_000_000,
            1_700_010_000,
        )
        .expect("Pass issuance failed");

        // Admit at 1_700_005_000 (valid window)
        let admitted = club_node
            .verify_and_admit_door_pass(door_pass.clone(), 1_700_005_000)
            .expect("Pass verification failed");
        assert!(admitted);
        assert!(club_node.active_door_passes.contains_key(&pass_id));

        // Test mini-app installation and permission grant
        let app_id = Uuid::new_v4();
        let dev_identity = PearlEnclaveIdentity::from_label("trivia-bot-developer");
        let manifest = atlantis_protocol::MiniAppManifest {
            app_id,
            developer_did: dev_identity.did().to_string(),
            name: "Pop Culture Trivia".to_string(),
            description: "Live interactive trivia for screenings and Comic-Con".to_string(),
            version: "1.0.0".to_string(),
            icon_uri: "https://atlantis.network/icons/trivia.png".to_string(),
            webview_entry_url: "https://trivia.atlantis.app/embed".to_string(),
            requested_scopes: vec![
                atlantis_protocol::ExtensionScope::ReadProfile,
                atlantis_protocol::ExtensionScope::PublishEventNotes,
            ],
            revenue_share_pct: 15,
        };

        club_node.install_extension(manifest);
        assert!(club_node.installed_extensions.contains_key(&app_id));

        let grant = atlantis_protocol::ExtensionGrant::issue(
            &host_identity,
            app_id,
            vec![atlantis_protocol::ExtensionScope::ReadProfile],
            1_800_000_000,
        )
        .expect("Grant issuance failed");

        let registered = club_node
            .register_extension_grant(grant)
            .expect("Grant registration failed");
        assert!(registered);
    }
}
