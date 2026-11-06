//! # Social Club Domain & Entrepreneur Network Schemas
//!
//! Provides the canonical schemas, validation logic, and cryptographic verification
//! for Movie & Screening Clubs, Book Clubs, Comic-Con & Cosplay Fandoms,
//! Nightlife digital passes, and the Mini-App / Entrepreneur extension framework.

use crate::{bytes_to_hex, hex_to_bytes, AtlantisProtocolError, PearlEnclaveIdentity};
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// 1. UNIVERSAL CLUB DEFINITIONS & ROLES
// ============================================================================

/// Subculture categories for clubs operating in the Atlantis network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClubCategory {
    MovieAndScreening,
    BookAndLiterature,
    ComicConAndCosplay,
    NightlifeAndMusic,
    EntrepreneurAndBuilder,
    GeneralSocial,
}

/// Member governance roles within a club.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ClubRole {
    Guest,
    Member,
    DoorStaff,
    Moderator,
    Curator,
    Host,
}

/// Membership tiers granting configurable perks and event access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MembershipTier {
    Guest,
    ActiveMember,
    VipPatron,
}

/// Profile metadata for an Atlantis Social Club.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClubProfile {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub category: ClubCategory,
    pub host_did: String,
    pub home_node_endpoint: String,
    pub enabled_modules: Vec<String>,
    pub created_at_s: u64,
}

// ============================================================================
// 2. MOVIE & SCREENING CLUBS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WatchlistItem {
    pub item_id: Uuid,
    pub title: String,
    pub year: u16,
    pub tmdb_or_imdb_id: Option<String>,
    pub proposed_by_did: String,
    pub votes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MovieScreeningSession {
    pub session_id: Uuid,
    pub club_id: Uuid,
    pub title: String,
    pub venue_name: String,
    pub start_time_s: u64,
    pub runtime_minutes: u32,
    pub group_capacity: u32,
    pub rsvp_dids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpoilerPost {
    pub post_id: Uuid,
    pub session_id: Uuid,
    pub author_did: String,
    pub content: String,
    pub created_at_s: u64,
}

impl SpoilerPost {
    /// Can only be viewed if viewer has confirmed they watched the movie.
    pub fn can_view(&self, viewer_did: &str, watched_dids: &[String]) -> bool {
        self.author_did == viewer_did || watched_dids.iter().any(|d| d == viewer_did)
    }
}

// ============================================================================
// 3. BOOK & READING CLUBS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadingCadenceMilestone {
    pub milestone_id: Uuid,
    pub target_chapter_or_page: String,
    pub due_date_s: u64,
    pub completed_by_dids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BookDiscussionPrompt {
    pub prompt_id: Uuid,
    pub club_id: Uuid,
    pub page_or_chapter_ref: String,
    pub quote_text: String,
    pub prompt_question: String,
    pub author_did: String,
    pub created_at_s: u64,
}

// ============================================================================
// 4. COMIC-CON, COSPLAY & FANDOM CLUBS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CosplayShowcase {
    pub build_id: Uuid,
    pub creator_did: String,
    pub character_name: String,
    pub series_or_franchise: String,
    pub is_wip: bool,
    pub bill_of_materials: Vec<String>,
    pub tagged_craftspeople: Vec<String>,
    pub media_uris: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConventionFloorBeacon {
    pub beacon_id: Uuid,
    pub club_id: Uuid,
    pub broadcaster_did: String,
    pub hall_or_booth: String,
    pub expires_at_s: u64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SwagExchangeListing {
    pub listing_id: Uuid,
    pub owner_did: String,
    pub title: String,
    pub item_type: String, // e.g. "Pin", "Badge", "Artist Print", "Prop"
    pub for_trade: bool,
    pub asking_price_cents: Option<u64>,
}

// ============================================================================
// 5. NIGHTLIFE & PARTIES: CRYPTOGRAPHIC DOOR PASS & TELEMETRY
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NightlifeCrowdTelemetry {
    pub venue_id: Uuid,
    pub queue_time_minutes: u32,
    pub current_genre: String,
    pub active_performer_or_dj: String,
    pub updated_at_s: u64,
}

/// Cryptographically signed digital door pass (Ed25519)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DigitalDoorPass {
    pub pass_id: Uuid,
    pub event_id: Uuid,
    pub club_id: Uuid,
    pub attendee_did: String,
    pub issuer_did: String,
    pub tier: MembershipTier,
    pub valid_from_s: u64,
    pub valid_until_s: u64,
    pub signature_hex: String,
}

impl DigitalDoorPass {
    pub fn issue(
        issuer: &PearlEnclaveIdentity,
        pass_id: Uuid,
        event_id: Uuid,
        club_id: Uuid,
        attendee_did: &str,
        tier: MembershipTier,
        valid_from_s: u64,
        valid_until_s: u64,
    ) -> Result<Self, AtlantisProtocolError> {
        let msg = format!(
            "DOOR_PASS:{}:{}:{}:{}:{:?}:{}:{}",
            pass_id,
            event_id,
            club_id,
            attendee_did,
            tier,
            valid_from_s,
            valid_until_s
        );
        let sig = issuer.signing_key().sign(msg.as_bytes());

        Ok(Self {
            pass_id,
            event_id,
            club_id,
            attendee_did: attendee_did.to_string(),
            issuer_did: issuer.did().to_string(),
            tier,
            valid_from_s,
            valid_until_s,
            signature_hex: bytes_to_hex(&sig.to_bytes()),
        })
    }

    pub fn verify_signature(&self) -> Result<(), AtlantisProtocolError> {
        let pubkey_hex = self
            .issuer_did
            .strip_prefix("did:atlantis:")
            .ok_or_else(|| AtlantisProtocolError::InvalidDidFormat(self.issuer_did.clone()))?;
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
            "DOOR_PASS:{}:{}:{}:{}:{:?}:{}:{}",
            self.pass_id,
            self.event_id,
            self.club_id,
            self.attendee_did,
            self.tier,
            self.valid_from_s,
            self.valid_until_s
        );

        verifying_key
            .verify(msg.as_bytes(), &sig)
            .map_err(|e| AtlantisProtocolError::CryptoError(e.to_string()))
    }

    pub fn is_valid_at(&self, current_time_s: u64) -> bool {
        current_time_s >= self.valid_from_s && current_time_s <= self.valid_until_s
    }
}

// ============================================================================
// 6. ENTREPRENEUR & MINI-APP EXTENSION SPECIFICATION
// ============================================================================

/// Permission scopes that a mini-app extension can request from clubs and members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionScope {
    ReadProfile,
    ReadAttendance,
    PublishEventNotes,
    ManageTicketing,
    ProcessSplitPayments,
}

/// Manifest defining a third-party mini-app extension for the Atlantis Platform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MiniAppManifest {
    pub app_id: Uuid,
    pub developer_did: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub icon_uri: String,
    pub webview_entry_url: String,
    pub requested_scopes: Vec<ExtensionScope>,
    pub revenue_share_pct: u8,
}

/// Authorization grant given by a club or member to an extension.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionGrant {
    pub grant_id: Uuid,
    pub app_id: Uuid,
    pub granter_did: String,
    pub granted_scopes: Vec<ExtensionScope>,
    pub expires_at_s: u64,
    pub signature_hex: String,
}

impl ExtensionGrant {
    pub fn issue(
        granter: &PearlEnclaveIdentity,
        app_id: Uuid,
        granted_scopes: Vec<ExtensionScope>,
        expires_at_s: u64,
    ) -> Result<Self, AtlantisProtocolError> {
        let grant_id = Uuid::new_v4();
        let scopes_str: Vec<String> = granted_scopes.iter().map(|s| format!("{:?}", s)).collect();
        let msg = format!(
            "GRANT:{}:{}:{}:{}:{}",
            grant_id,
            app_id,
            granter.did(),
            scopes_str.join(","),
            expires_at_s
        );
        let sig = granter.signing_key().sign(msg.as_bytes());

        Ok(Self {
            grant_id,
            app_id,
            granter_did: granter.did().to_string(),
            granted_scopes,
            expires_at_s,
            signature_hex: bytes_to_hex(&sig.to_bytes()),
        })
    }

    pub fn verify(&self) -> Result<(), AtlantisProtocolError> {
        let pubkey_hex = self
            .granter_did
            .strip_prefix("did:atlantis:")
            .ok_or_else(|| AtlantisProtocolError::InvalidDidFormat(self.granter_did.clone()))?;
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

        let scopes_str: Vec<String> = self
            .granted_scopes
            .iter()
            .map(|s| format!("{:?}", s))
            .collect();
        let msg = format!(
            "GRANT:{}:{}:{}:{}:{}",
            self.grant_id,
            self.app_id,
            self.granter_did,
            scopes_str.join(","),
            self.expires_at_s
        );

        verifying_key
            .verify(msg.as_bytes(), &sig)
            .map_err(|e| AtlantisProtocolError::CryptoError(e.to_string()))
    }
}


// ============================================================================
// TESTS FOR SOCIAL CLUB & MINI-APP EXTENSIONS
// ============================================================================

#[cfg(test)]
mod social_tests {
    use super::*;

    #[test]
    fn test_digital_door_pass_issuance_and_verification() {
        let club_host = PearlEnclaveIdentity::from_label("tokyo-underground-host");
        let attendee = PearlEnclaveIdentity::from_label("clubber-alice");

        let pass_id = Uuid::new_v4();
        let event_id = Uuid::new_v4();
        let club_id = Uuid::new_v4();

        let pass = DigitalDoorPass::issue(
            &club_host,
            pass_id,
            event_id,
            club_id,
            attendee.did(),
            MembershipTier::VipPatron,
            1_700_000_000,
            1_700_036_000,
        )
        .expect("Pass issuance failed");

        assert!(pass.verify_signature().is_ok());
        assert!(pass.is_valid_at(1_700_010_000));
        assert!(!pass.is_valid_at(1_700_050_000));

        // Forged pass test
        let mut tampered = pass.clone();
        tampered.tier = MembershipTier::Guest;
        assert!(tampered.verify_signature().is_err());
    }

    #[test]
    fn test_spoiler_post_gate() {
        let author_did = "did:atlantis:author";
        let user_watched = "did:atlantis:user_watched";
        let user_unwatched = "did:atlantis:user_unwatched";

        let post = SpoilerPost {
            post_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            author_did: author_did.to_string(),
            content: "The protagonist was the mastermind all along!".to_string(),
            created_at_s: 1_700_000_000,
        };

        let watched_list = vec![author_did.to_string(), user_watched.to_string()];

        assert!(post.can_view(author_did, &watched_list));
        assert!(post.can_view(user_watched, &watched_list));
        assert!(!post.can_view(user_unwatched, &watched_list));
    }

    #[test]
    fn test_mini_app_extension_grant_and_scope_verification() {
        let club_admin = PearlEnclaveIdentity::from_label("book-club-admin");
        let app_id = Uuid::new_v4();

        let grant = ExtensionGrant::issue(
            &club_admin,
            app_id,
            vec![
                ExtensionScope::ReadProfile,
                ExtensionScope::PublishEventNotes,
            ],
            1_800_000_000,
        )
        .expect("Grant issuance failed");

        assert!(grant.verify().is_ok());

        // Tamper test
        let mut forged_grant = grant.clone();
        forged_grant
            .granted_scopes
            .push(ExtensionScope::ProcessSplitPayments);
        assert!(forged_grant.verify().is_err());
    }
}
