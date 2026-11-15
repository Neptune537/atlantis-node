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

pub mod service_gate;
pub use service_gate::*;

pub mod car_sentry;
pub use car_sentry::*;

pub mod sessions;
pub mod floor_one;
pub mod app_baseline_merge;
pub mod kuroshio_server;
pub use app_baseline_merge::*;

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

/// Supported consumer OAuth / OIDC identity providers for Kuroshio Gyre onboarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OidcProvider {
    Google,
    Apple,
}

impl OidcProvider {
    pub fn canonical_issuer(&self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com",
            Self::Apple => "https://appleid.apple.com",
        }
    }

    pub fn from_issuer(iss: &str) -> Result<Self, String> {
        match iss {
            "https://accounts.google.com" | "accounts.google.com" => Ok(Self::Google),
            "https://appleid.apple.com" | "appleid.apple.com" => Ok(Self::Apple),
            other => Err(format!("Unsupported OIDC issuer: '{}'", other)),
        }
    }
}

/// Decodes a Base64URL-encoded string (RFC 4648 §5, with or without padding) into raw bytes.
pub fn decode_base64url(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input.trim().trim_end_matches('=');
    let mut buffer = Vec::with_capacity(trimmed.len() * 3 / 4);
    let mut accum: u32 = 0;
    let mut bits: u8 = 0;

    for ch in trimmed.bytes() {
        let val: u8 = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return Err(format!("Invalid Base64URL character: '{}'", ch as char)),
        };
        accum = (accum << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buffer.push(((accum >> bits) & 0xFF) as u8);
        }
    }
    Ok(buffer)
}

/// Verified session binding returned by `GoogleJwksVerifier` (supporting both Google and Apple ID OIDC).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedAtlantisSession {
    pub provider: OidcProvider,
    pub issuer: String,
    pub subject: String,
    /// Legacy alias for compatibility with callers inspecting `google_sub`.
    pub google_sub: String,
    pub did: String,
    pub session_token: String,
    pub expires_at_s: u64,
    pub linked_solana_wallet: Option<String>,
}

/// Expiring one-time challenge used to cryptographically bind a self-custody Solana wallet
/// to an authenticated Apple ID or Google Atlantis principal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolanaWalletLinkChallenge {
    pub principal_did: String,
    pub wallet_pubkey_hex: String,
    pub nonce: String,
    pub issued_at_s: u64,
    pub expires_at_s: u64,
}

impl SolanaWalletLinkChallenge {
    /// Constructs the exact canonical ASCII message that the user's Solana wallet signs.
    pub fn signing_message(&self) -> String {
        format!(
            "KUROSHIO_GYRE_WALLET_LINK_V1\ndid:{}\nwallet:{}\nnonce:{}\nexpires:{}",
            self.principal_did, self.wallet_pubkey_hex, self.nonce, self.expires_at_s
        )
    }

    /// Verifies the wallet's Ed25519 signature over `signing_message()` and checks expiry.
    pub fn verify_wallet_signature(
        &self,
        signature_hex: &str,
        current_time_s: u64,
    ) -> Result<(), String> {
        if current_time_s > self.expires_at_s {
            return Err("Wallet link challenge has expired".to_string());
        }
        let target_did = format!("did:atlantis:{}", self.wallet_pubkey_hex);
        let verifying_key = PearlEnclaveIdentity::verifying_key_from_did(&target_did)
            .map_err(|e| format!("Invalid Solana wallet public key: {}", e))?;
        let sig_bytes = atlantis_protocol::hex_to_bytes(signature_hex)
            .map_err(|e| format!("Invalid signature hex: {}", e))?;
        let sig_arr: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| "Solana Ed25519 signature must be 64 bytes".to_string())?;
        let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
        use ed25519_dalek::Verifier;
        verifying_key
            .verify(self.signing_message().as_bytes(), &sig)
            .map_err(|_| "Solana wallet Ed25519 signature verification failed".to_string())
    }
}

/// JSON Web Key (JWK) descriptor fetched from Google (`https://www.googleapis.com/oauth2/v3/certs`)
/// or Apple ID (`https://appleid.apple.com/auth/keys`) JWKS endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OidcJwkKey {
    pub kid: String,
    pub kty: String,
    pub alg: String,
    #[serde(rename = "use", default)]
    pub key_use: Option<String>,
    /// Base64URL modulus (`RSA`) or public key (`OKP`/`EC`).
    #[serde(default)]
    pub n: Option<String>,
    #[serde(default)]
    pub e: Option<String>,
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub y: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OidcJwksKeySet {
    pub keys: Vec<OidcJwkKey>,
}

/// Verifies incoming Google OAuth and Apple ID ("Sign in with Apple") OIDC JWT tokens
/// and binds them deterministically to a stable `did:atlantis:` principal.
///
/// Key properties:
/// - Principal DID is derived from `(expected_client_id, canonical_issuer, sub)` so different
///   login tokens for the same Google or Apple account always resolve to the same Atlantis principal.
/// - Session token incorporates per-login token entropy so each login gets a fresh session token.
/// - Supports aligning both an Apple ID and a Google account to the same Atlantis principal,
///   and linking a self-custody Solana wallet via a signed one-time challenge.
/// - Production Google RS256 verification uses expiring, issuer-scoped HTTPS JWKS keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleJwksVerifier {
    pub expected_client_id: String,
    pub apple_service_id: Option<String>,
    /// Maps secondary `"<provider>:<sub>"` keys to an existing primary `did:atlantis:` principal
    /// when a consumer aligns their Apple ID and Google account.
    #[serde(default)]
    pub aligned_principals: HashMap<String, String>,
    /// Maps primary `did:atlantis:` principals to their verified Solana wallet public key.
    #[serde(default)]
    pub linked_wallets: HashMap<String, String>,
    /// Cached rotating JWKS keys indexed by `kid` from Google and Apple ID endpoints.
    #[serde(default)]
    pub cached_jwks: HashMap<String, OidcJwkKey>,
    // Never deserialize provider trust or freshness from persisted/user-supplied state.
    #[serde(skip)]
    provider_keys: HashMap<String, (u64, HashMap<String, OidcJwkKey>)>,
    /// Active nonces issued for `SolanaWalletLinkChallenge` to prevent replay.
    #[serde(default)]
    pub active_wallet_challenges: HashMap<String, SolanaWalletLinkChallenge>,
    /// Set of consumed challenge nonces to permanently block replay attacks.
    #[serde(default)]
    pub consumed_wallet_nonces: std::collections::HashSet<String>,
}

impl GoogleJwksVerifier {
    pub const GOOGLE_JWKS_URL: &'static str = "https://www.googleapis.com/oauth2/v3/certs";
    pub const APPLE_JWKS_URL: &'static str = "https://appleid.apple.com/auth/keys";

    pub fn new(expected_client_id: impl Into<String>) -> Self {
        let client_id = std::env::var("GOOGLE_OAUTH_CLIENT_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| expected_client_id.into());
        let apple_service_id = std::env::var("APPLE_SERVICE_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| Some("com.tr7n7t7.heavenlybound.web".to_string()));
        Self {
            expected_client_id: client_id,
            apple_service_id,
            aligned_principals: HashMap::new(),
            linked_wallets: HashMap::new(),
            cached_jwks: HashMap::new(),
            provider_keys: HashMap::new(),
            active_wallet_challenges: HashMap::new(),
            consumed_wallet_nonces: std::collections::HashSet::new(),
        }
    }

    pub fn with_apple_service_id(mut self, apple_service_id: impl Into<String>) -> Self {
        self.apple_service_id = Some(apple_service_id.into());
        self
    }

    /// Ingests a JSON Web Key Set (`{"keys": [...]}`) from Google or Apple ID into the verifier's key cache.
    pub fn ingest_jwks_json(&mut self, jwks_json: &str) -> Result<usize, String> {
        let keyset: OidcJwksKeySet = serde_json::from_str(jwks_json)
            .map_err(|e| format!("Invalid JWKS JSON payload: {}", e))?;
        let mut count = 0;
        for key in keyset.keys {
            if !key.kid.trim().is_empty() {
                self.cached_jwks.insert(key.kid.clone(), key);
                count += 1;
            }
        }
        Ok(count)
    }

    pub async fn ensure_google_keys(&mut self) -> Result<(), String> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?.as_secs();
        if self.provider_keys.get("https://accounts.google.com").is_some_and(|(expiry, _)| now < *expiry) {
            return Ok(());
        }
        self.refresh_google_keys().await.map(|_| ())
    }

    /// Refresh only from the fixed HTTPS provider endpoint; redirects are forbidden.
    pub async fn refresh_google_keys(&mut self) -> Result<usize, String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build().map_err(|e| e.to_string())?;
        let response = client.get(Self::GOOGLE_JWKS_URL).send().await
            .map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?;
        let ttl = response.headers().get(reqwest::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').find_map(|part| part.trim().strip_prefix("max-age=")?.parse::<u64>().ok()))
            .unwrap_or(300).min(86400);
        let age = response.headers().get(reqwest::header::AGE)
            .and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let mut response = response;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            if body.len() + chunk.len() > 131072 { return Err("Provider JWKS too large".into()); }
            body.extend_from_slice(&chunk);
        }
        let keyset: OidcJwksKeySet = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        let mut keys = HashMap::new();
        for key in keyset.keys {
            if key.alg != "RS256" || key.kty != "RSA" || key.key_use.as_deref() != Some("sig") { continue; }
            if key.kid.is_empty() || key.n.is_none() || key.e.is_none() || keys.contains_key(&key.kid) {
                return Err("Malformed or duplicate provider JWK".into());
            }
            keys.insert(key.kid.clone(), key);
        }
        if keys.is_empty() { return Err("Provider returned no signing keys".into()); }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?.as_secs();
        let count = keys.len();
        self.provider_keys.insert("https://accounts.google.com".into(), (now.saturating_add(ttl.saturating_sub(age)), keys));
        Ok(count)
    }

    /// Issues and stores a fresh expiring `SolanaWalletLinkChallenge` for an authenticated principal.
    pub fn issue_wallet_link_challenge(
        &mut self,
        principal_did: &str,
        wallet_pubkey_hex: &str,
        now_s: u64,
        ttl_s: u64,
    ) -> Result<SolanaWalletLinkChallenge, String> {
        if !principal_did.starts_with("did:atlantis:") {
            return Err("Principal DID must start with 'did:atlantis:'".to_string());
        }
        let target_did = format!("did:atlantis:{}", wallet_pubkey_hex);
        PearlEnclaveIdentity::verifying_key_from_did(&target_did)
            .map_err(|e| format!("Invalid Solana wallet public key hex: {}", e))?;

        let nonce = format!("n_{}", Uuid::new_v4().simple());
        let challenge = SolanaWalletLinkChallenge {
            principal_did: principal_did.to_string(),
            wallet_pubkey_hex: wallet_pubkey_hex.to_string(),
            nonce: nonce.clone(),
            issued_at_s: now_s,
            expires_at_s: now_s.saturating_add(ttl_s.max(30)),
        };
        self.active_wallet_challenges.insert(nonce, challenge.clone());
        Ok(challenge)
    }

    /// Deterministically verifies a Google or Apple ID OIDC JWT token and derives the user's
    /// stable `did:atlantis:<hex>` principal and session token.
    pub fn verify_and_bind_jwt(
        &self,
        jwt_token: &str,
    ) -> Result<VerifiedAtlantisSession, String> {
        self.verify_and_bind_jwt_at(jwt_token, None)
    }

    /// Verifies a signed token; an omitted clock override uses system time. RSA provider login requires fresh trusted provider keys; EC remains disabled.
    pub fn verify_and_bind_jwt_at(
        &self,
        jwt_token: &str,
        current_time_s: Option<u64>,
    ) -> Result<VerifiedAtlantisSession, String> {
        if jwt_token.len() > 16384 { return Err("OIDC token too large".into()); }
        let trimmed = jwt_token.trim();
        let parts: Vec<&str> = trimmed.split('.').collect();
        if parts.len() != 3 {
            return Err("Invalid OIDC JWT format: must contain header, payload, and cryptographic signature parts".to_string());
        }

        let header_b64 = parts[0];
        let payload_b64 = parts[1];
        let signature_b64 = parts[2];

        if header_b64.is_empty() || payload_b64.is_empty() || signature_b64.is_empty() {
            return Err("Invalid OIDC JWT format: empty token segments".to_string());
        }

        // Validate minimum cryptographic signature entropy (at least 32 bytes encoded)
        if signature_b64.len() < 32 {
            return Err("Invalid OIDC JWT: signature entropy insufficient".to_string());
        }
        let sig_bytes = decode_base64url(signature_b64)?;
        if sig_bytes.len() < 24 {
            return Err("Invalid OIDC JWT: decoded signature too short".to_string());
        }

        let header_bytes = decode_base64url(header_b64)?;
        let header_json: serde_json::Value = serde_json::from_slice(&header_bytes)
            .map_err(|_| "Invalid OIDC JWT header JSON".to_string())?;
        let alg = header_json
            .get("alg")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'alg' in OIDC JWT header".to_string())?;
        if !matches!(alg, "RS256" | "ES256" | "EdDSA") {
            return Err(format!("Unsupported or insecure JWT algorithm: '{}'", alg));
        }

        let payload_json: serde_json::Value = serde_json::from_slice(&decode_base64url(payload_b64)?)
            .map_err(|_| "Invalid OIDC payload")?;
        let issuer = payload_json.get("iss").and_then(|v| v.as_str()).ok_or("Missing issuer")?;
        let provider = OidcProvider::from_issuer(issuer)?;
        let now = current_time_s.unwrap_or(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).map_err(|_| "Invalid clock")?.as_secs());
        let kid = header_json.get("kid").and_then(|v| v.as_str())
            .filter(|v| !v.is_empty()).ok_or("Missing OIDC JWK kid")?;
        if alg == "RS256" {
            let (expires, keys) = self.provider_keys.get(provider.canonical_issuer())
                .ok_or("Trusted provider keys unavailable")?;
            if now >= *expires { return Err("Trusted provider keys expired".into()); }
            let jwk = keys.get(kid).ok_or("Unknown provider key; refresh required")?;
            if jwk.alg != "RS256" || jwk.kty != "RSA" { return Err("Invalid RSA key".into()); }
            let n = decode_base64url(jwk.n.as_deref().ok_or("Missing RSA modulus")?)?;
            let e = decode_base64url(jwk.e.as_deref().ok_or("Missing RSA exponent")?)?;
            ring::signature::RsaPublicKeyComponents { n, e }
                .verify(&ring::signature::RSA_PKCS1_2048_8192_SHA256,
                    format!("{}.{}", header_b64, payload_b64).as_bytes(), &sig_bytes)
                .map_err(|_| "RSA OIDC signature verification failed")?;
        } else if alg == "EdDSA" {
            // Local cryptographic fixtures only; provider login never trusts uploaded keys.
            if !cfg!(test) { return Err("Provider requires RS256".into()); }
            let jwk = self.cached_jwks.get(kid).ok_or("Unknown OIDC JWK kid")?;
            if jwk.alg != alg || jwk.kty != "OKP" || jwk.key_use.as_deref() != Some("sig") {
                return Err("OIDC JWK algorithm, type or use mismatch".to_string());
            }
            let pub_bytes = decode_base64url(jwk.x.as_deref().ok_or("Missing OIDC public key")?)?;
            let pub_arr: [u8; 32] = pub_bytes.try_into().map_err(|_| "Invalid OIDC public key length")?;
            let vk = ed25519_dalek::VerifyingKey::from_bytes(&pub_arr)
                .map_err(|_| "Invalid OIDC public key")?;
            let sig_arr: [u8; 64] = sig_bytes.as_slice().try_into()
                .map_err(|_| "Invalid OIDC signature length")?;
            vk.verify_strict(format!("{}.{}", header_b64, payload_b64).as_bytes(),
                &ed25519_dalek::Signature::from_bytes(&sig_arr))
                .map_err(|_| "OIDC signature verification failed")?;

        } else { return Err("Unsupported provider algorithm".into()); }

        let sub = payload_json
            .get("sub")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "Missing or empty 'sub' claim in OIDC JWT payload".to_string())?;

        let expected_aud = match provider {
            OidcProvider::Google => self.expected_client_id.as_str(),
            OidcProvider::Apple => self.apple_service_id.as_deref()
                .ok_or("Apple sign-in is not configured")?,
        };
        if expected_aud.is_empty() || payload_json.get("aud").and_then(|v| v.as_str()) != Some(expected_aud) {
            return Err("Missing or mismatched OIDC audience".to_string());
        }
        let exp = payload_json.get("exp").and_then(|v| v.as_u64())
            .ok_or("Missing or invalid OIDC expiry")?;
        if now >= exp {
            return Err("OIDC JWT token has expired".to_string());
        }
        if let Some(nbf) = payload_json.get("nbf") {
            if nbf.as_u64().ok_or("Invalid OIDC not-before")? > now {
                return Err("OIDC JWT is not yet valid".to_string());
            }
        }

        let canonical_iss = provider.canonical_issuer();
        let align_key = format!("{}:{}", canonical_iss, sub);

        // Check if this (provider, sub) has been aligned to an existing primary Atlantis DID
        let did = if let Some(aligned_did) = self.aligned_principals.get(&align_key) {
            aligned_did.clone()
        } else {
            let mut id_hasher = Sha256::new();
            id_hasher.update(b"ATLANTIS_OIDC_PRINCIPAL_V2:");
            id_hasher.update(self.expected_client_id.as_bytes());
            id_hasher.update(b":");
            id_hasher.update(canonical_iss.as_bytes());
            id_hasher.update(b":");
            id_hasher.update(sub.as_bytes());
            let id_digest = id_hasher.finalize();
            let hex_pubkey = atlantis_protocol::bytes_to_hex(&id_digest);
            format!("did:atlantis:{}", hex_pubkey)
        };

        // Session token binds principal DID + signature entropy so rotating tokens produce distinct sessions
        let mut sess_hasher = Sha256::new();
        sess_hasher.update(b"ATLANTIS_OIDC_SESSION_V2:");
        sess_hasher.update(did.as_bytes());
        sess_hasher.update(b":");
        sess_hasher.update(signature_b64.as_bytes());
        let sess_hex = atlantis_protocol::bytes_to_hex(&sess_hasher.finalize());
        let session_token = format!("atlantis_sess_{}", &sess_hex[..32]);

        let linked_solana_wallet = self.linked_wallets.get(&did).cloned();

        Ok(VerifiedAtlantisSession {
            provider,
            issuer: canonical_iss.to_string(),
            subject: sub.to_string(),
            google_sub: sub.to_string(),
            did,
            session_token,
            expires_at_s: exp,
            linked_solana_wallet,
        })
    }

    /// Aligns a consumer's secondary OIDC identity (e.g., Apple ID) to their existing primary
    /// Atlantis session (e.g., Google account) or vice versa, so both sign-in methods resolve
    /// to the exact same `did:atlantis:` principal and wallet profile.
    pub fn align_consumer_accounts(
        &mut self,
        primary_jwt: &str,
        secondary_jwt: &str,
        current_time_s: Option<u64>,
    ) -> Result<VerifiedAtlantisSession, String> {
        let primary_session = self.verify_and_bind_jwt_at(primary_jwt, current_time_s)?;
        let secondary_session = self.verify_and_bind_jwt_at(secondary_jwt, current_time_s)?;

        let sec_key = format!("{}:{}", secondary_session.issuer, secondary_session.subject);
        let pri_key = format!("{}:{}", primary_session.issuer, primary_session.subject);
        self.aligned_principals.insert(sec_key, primary_session.did.clone());
        self.aligned_principals.insert(pri_key, primary_session.did.clone());

        self.verify_and_bind_jwt_at(secondary_jwt, current_time_s)
    }

    /// Links a verified self-custody Solana wallet to an authenticated Atlantis principal
    /// after verifying the signed one-time challenge.
    pub fn link_solana_wallet(
        &mut self,
        challenge: &SolanaWalletLinkChallenge,
        signature_hex: &str,
        current_time_s: u64,
    ) -> Result<String, String> {
        challenge.verify_wallet_signature(signature_hex, current_time_s)?;
        if self.consumed_wallet_nonces.contains(&challenge.nonce) {
            return Err("Wallet link challenge nonce has already been consumed".to_string());
        }
        match self.active_wallet_challenges.get(&challenge.nonce) {
            Some(stored) if stored == challenge => {}
            _ => return Err("Wallet link challenge nonce not found or mismatched".to_string()),
        }
        // Prevent cross-account wallet hijacking unless re-linking the same principal
        for (existing_did, existing_wallet) in &self.linked_wallets {
            if existing_wallet == &challenge.wallet_pubkey_hex && existing_did != &challenge.principal_did {
                return Err("Solana wallet is already bound to a different Atlantis principal".to_string());
            }
        }
        self.active_wallet_challenges.remove(&challenge.nonce);
        self.consumed_wallet_nonces.insert(challenge.nonce.clone());
        self.linked_wallets.insert(
            challenge.principal_did.clone(),
            challenge.wallet_pubkey_hex.clone(),
        );
        Ok(challenge.wallet_pubkey_hex.clone())
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
    /// Sovereign subscription ledger managing annual store node stakes and monthly drips.
    pub subscription_ledger: tide_ledger::SubscriptionLedger,
}

impl ClubHomeNode {
    pub fn new(club_node_id: impl Into<String>, dividend_rate: f64) -> Self {
        let node_id_str = club_node_id.into();
        let mut ledger = TideLedgerEngine::new();
        let club_treasury_account = Uuid::new_v4();
        ledger.seed_account(club_treasury_account, 0.0);
        let subscription_ledger = tide_ledger::SubscriptionLedger::new(&mut ledger, 100_000.0);
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
            subscription_ledger,
        }
    }

    /// Processes a job request from a member app (Food Delivery, DJ Gig, Car Mechanic, Compute),
    /// locks the budget in Coral Escrow, and returns the broadcastable Kuroshio Gyre envelope.
    pub fn dispatch_marketplace_job(
        &mut self,
        _buyer_did: String,
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

        let event = KuroshioEventPayload::TaskCreated {
            job_id,
            category,
            budget_amount,
            asset,
            min_poh_score,
        };

        let envelope = self
            .registry
            .club_identity
            .sign_event(&self.club_node_id, event, timestamp)
            .map_err(|e| LedgerError::DatabaseError(format!("Signature generation failed: {}", e)))?;

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

        let event = KuroshioEventPayload::EscrowReleased {
            escrow_id,
            recipient_did: worker_did,
            club_dividend: dividend_tx.amount,
        };

        let settlement_envelope = self
            .registry
            .club_identity
            .sign_event(&self.club_node_id, event, timestamp)
            .map_err(|e| LedgerError::DatabaseError(format!("Signature generation failed: {}", e)))?;

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
    fn oidc_requires_signature_and_required_claims() {
        fn b64(bytes: &[u8]) -> String {
            const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
            let mut out = String::new();
            let mut bits = 0u32;
            let mut count = 0;
            for &byte in bytes {
                bits = (bits << 8) | byte as u32;
                count += 8;
                while count >= 6 {
                    count -= 6;
                    out.push(ABC[((bits >> count) & 63) as usize] as char);
                }
            }
            if count > 0 { out.push(ABC[((bits << (6-count)) & 63) as usize] as char); }
            out
        }
        use ed25519_dalek::Signer;
        let key = PearlEnclaveIdentity::from_label("oidc-signature-test");
        let mut verifier = GoogleJwksVerifier::new("client");
        verifier.expected_client_id = "client".into();
        verifier.ingest_jwks_json(&serde_json::json!({"keys":[{
            "kid":"test", "alg":"EdDSA", "kty":"OKP", "use":"sig",
            "x":b64(&key.signing_key().verifying_key().to_bytes())
        }]}).to_string()).unwrap();
        let token = |claims: serde_json::Value| {
            let input = format!("{}.{}", b64(br#"{"alg":"EdDSA","kid":"test"}"#), b64(claims.to_string().as_bytes()));
            format!("{}.{}", input, b64(&key.signing_key().sign(input.as_bytes()).to_bytes()))
        };
        let claims = serde_json::json!({"iss":"https://accounts.google.com","sub":"account","aud":"client","exp":200});
        let valid = token(claims.clone());
        assert!(verifier.verify_and_bind_jwt_at(&valid, Some(100)).is_ok());
        assert!(verifier.verify_and_bind_jwt_at(&valid, Some(200)).is_err());
        assert!(verifier.verify_and_bind_jwt(&valid).is_err());
        for field in ["iss", "aud", "exp", "sub"] {
            let mut invalid = claims.clone();
            invalid.as_object_mut().unwrap().remove(field);
            assert!(verifier.verify_and_bind_jwt_at(&token(invalid), Some(100)).is_err());
        }
        let mut tampered = valid.into_bytes();
        let last = tampered.len()-3;
        tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
        assert!(verifier.verify_and_bind_jwt_at(std::str::from_utf8(&tampered).unwrap(), Some(100)).is_err());
        verifier.cached_jwks.clear();
        assert!(verifier.verify_and_bind_jwt_at(&token(claims), Some(100)).is_err());
    }

    #[tokio::test]
    #[ignore = "requires live Google HTTPS access"]
    async fn google_jwks_live_refresh() {
        let mut verifier = GoogleJwksVerifier::new("client");
        assert!(verifier.refresh_google_keys().await.unwrap() > 0);
        assert!(verifier.provider_keys.contains_key("https://accounts.google.com"));
        verifier.ensure_google_keys().await.unwrap();
    }

    #[test]
    fn rsa_provider_signature_trust_and_expiry() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("oidc_rsa_test.json")).unwrap();
        let token = fixture["token"].as_str().unwrap();
        let jwk: OidcJwkKey = serde_json::from_value(fixture["jwk"].clone()).unwrap();
        let mut verifier = GoogleJwksVerifier::new("client");
        verifier.expected_client_id = "client".into();
        assert!(verifier.verify_and_bind_jwt_at(token, Some(100)).is_err());
        // Uploaded generic keys cannot establish provider trust.
        verifier.cached_jwks.insert(jwk.kid.clone(), jwk.clone());
        assert!(verifier.verify_and_bind_jwt_at(token, Some(100)).is_err());
        let mut keys = HashMap::new();
        keys.insert(jwk.kid.clone(), jwk.clone());
        verifier.provider_keys.insert("https://accounts.google.com".into(), (150, keys.clone()));
        assert!(verifier.verify_and_bind_jwt_at(token, Some(100)).is_ok());
        assert!(verifier.verify_and_bind_jwt_at(token, Some(150)).is_err());
        let mut tampered = token.as_bytes().to_vec();
        let index = tampered.len()-3;
        tampered[index] = if tampered[index] == b'A' { b'B' } else { b'A' };
        assert!(verifier.verify_and_bind_jwt_at(std::str::from_utf8(&tampered).unwrap(), Some(100)).is_err());
        verifier.provider_keys.clear();
        verifier.provider_keys.insert("https://appleid.apple.com".into(), (150, keys));
        assert!(verifier.verify_and_bind_jwt_at(token, Some(100)).is_err());
        let serialized = serde_json::to_string(&verifier).unwrap();
        let restored: GoogleJwksVerifier = serde_json::from_str(&serialized).unwrap();
        assert!(restored.provider_keys.is_empty());
    }

    #[test]
    fn oidc_and_wallet_fail_closed() {
        let mut verifier = GoogleJwksVerifier::new("client");
        let forged = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJhIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        assert!(verifier.verify_and_bind_jwt(forged).is_err());
        let wallet = PearlEnclaveIdentity::from_label("wallet-security-test");
        let address = wallet.did().strip_prefix("did:atlantis:").unwrap().to_string();
        let challenge = verifier.issue_wallet_link_challenge("did:atlantis:account", &address, 100, 300).unwrap();
        use ed25519_dalek::Signer;
        let signature = wallet.signing_key().sign(challenge.signing_message().as_bytes());
        let signature = atlantis_protocol::bytes_to_hex(&signature.to_bytes());
        let mut empty = GoogleJwksVerifier::new("client");
        assert!(empty.link_solana_wallet(&challenge, &signature, 101).is_err());
        assert!(verifier.link_solana_wallet(&challenge, &signature, 101).is_ok());
        assert!(verifier.link_solana_wallet(&challenge, &signature, 102).is_err());
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
