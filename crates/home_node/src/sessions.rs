//! Persistent opaque bearer sessions. Only token hashes are stored.
use ring::rand::{SecureRandom, SystemRandom};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct SessionStore { db: Connection }

/// Public room history is a bounded recent feed, not a permanent archive.
pub const CLUB_HISTORY_PER_SCENE: usize = 1000;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ClubPresence { pub principal_did: String, pub scene: String, pub seen_at_s: u64 }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ClubMessage { pub id: i64, pub principal_did: String, pub scene: String, pub text: String, pub sent_at_s: u64 }

fn valid_club_scene(scene: &str) -> bool {
    matches!(scene, "Atlantis:surface" | "Atlantis:dungeon:1")
}

impl SessionStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        #[cfg(unix)]
        if path != Path::new(":memory:") {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let file = std::fs::OpenOptions::new().read(true).write(true).create(true)
                .truncate(false).mode(0o600).open(path).map_err(|e| e.to_string())?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
        }
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        db.busy_timeout(std::time::Duration::from_secs(3)).map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS auth_sessions (
                token_hash TEXT PRIMARY KEY, principal TEXT NOT NULL,
                expires INTEGER NOT NULL, last_seen INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS auth_accounts (
                principal TEXT PRIMARY KEY, created INTEGER NOT NULL, last_login INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS wallet_links (
                principal TEXT PRIMARY KEY, wallet TEXT NOT NULL UNIQUE);
            CREATE TABLE IF NOT EXISTS wallet_challenges (
                nonce TEXT PRIMARY KEY, principal TEXT NOT NULL, expires INTEGER NOT NULL, payload TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS club_presence (
                principal TEXT PRIMARY KEY, scene TEXT NOT NULL, seen_at INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS club_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT, principal TEXT NOT NULL,
                scene TEXT NOT NULL, text TEXT NOT NULL, sent_at INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS club_messages_scene_id ON club_messages(scene, id);
            CREATE INDEX IF NOT EXISTS club_messages_principal_id ON club_messages(principal, id);
            CREATE TABLE IF NOT EXISTS club_message_rates (
                principal TEXT PRIMARY KEY, sent_at INTEGER NOT NULL);").map_err(|e| e.to_string())?;
        Ok(Self { db })
    }
    fn hash(token: &str) -> Result<String, String> {
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid session".into());
        }
        Ok(atlantis_protocol::bytes_to_hex(&Sha256::digest(token.as_bytes())))
    }
    pub fn issue(&mut self, principal: &str, now: u64, identity_expiry: u64) -> Result<(String, u64), String> {
        if now >= identity_expiry || !principal.starts_with("did:atlantis:") { return Err("Invalid identity".into()); }
        let expires = identity_expiry.min(now.saturating_add(3600));
        let mut secret = [0u8; 32];
        SystemRandom::new().fill(&mut secret).map_err(|_| "Session randomness unavailable")?;
        let token = atlantis_protocol::bytes_to_hex(&secret);
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM auth_sessions WHERE expires <= ?1 OR last_seen <= ?2", params![now, now.saturating_sub(900)])
            .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO auth_accounts VALUES (?1, ?2, ?2) ON CONFLICT(principal) DO UPDATE SET last_login=excluded.last_login", params![principal, now]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO auth_sessions VALUES (?1, ?2, ?3, ?4)", params![Self::hash(&token)?, principal, expires, now])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok((token, expires))
    }
    pub fn authenticate(&mut self, token: &str, now: u64) -> Result<String, String> {
        let hash = Self::hash(token)?;
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let row: Option<(String, u64, u64)> = tx.query_row(
            "SELECT principal, expires, last_seen FROM auth_sessions WHERE token_hash=?1", [&hash],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional().map_err(|e| e.to_string())?;
        let (principal, expires, last_seen) = row.ok_or("Unknown or revoked session")?;
        if now >= expires || now >= last_seen.saturating_add(900) || now < last_seen {
            return Err("Expired session".into());
        }
        tx.execute("UPDATE auth_sessions SET last_seen=?1 WHERE token_hash=?2", params![now, hash]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(principal)
    }
    pub fn wallet(&self, principal: &str) -> Result<Option<String>, String> {
        self.db.query_row("SELECT wallet FROM wallet_links WHERE principal=?1", [principal], |r| r.get(0))
            .optional().map_err(|e| e.to_string())
    }
    pub fn set_club_presence(&mut self, principal: &str, scene: &str, now: u64) -> Result<(), String> {
        if !valid_club_scene(scene) { return Err("Unknown public scene".into()); }
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM club_presence WHERE seen_at<=?1 OR seen_at>?2",
            params![now.saturating_sub(45), now]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO club_presence(principal,scene,seen_at) VALUES (?1,?2,?3)
            ON CONFLICT(principal) DO UPDATE SET scene=excluded.scene,seen_at=excluded.seen_at",
            params![principal, scene, now]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn clear_club_presence(&mut self, principal: &str) -> Result<(), String> {
        self.db.execute("DELETE FROM club_presence WHERE principal=?1", [principal]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn club_presence(&self, scene: &str, now: u64) -> Result<Vec<ClubPresence>, String> {
        if !valid_club_scene(scene) { return Err("Unknown public scene".into()); }
        let mut query = self.db.prepare("SELECT principal,scene,seen_at FROM club_presence
            WHERE scene=?1 AND seen_at>?2 AND seen_at<=?3 ORDER BY principal LIMIT 100").map_err(|e| e.to_string())?;
        let rows = query.query_map(params![scene, now.saturating_sub(45), now], |row| Ok(ClubPresence {
            principal_did: row.get(0)?, scene: row.get(1)?, seen_at_s: row.get(2)?,
        })).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e| e.to_string())
    }
    pub fn post_club_message(&mut self, principal: &str, scene: &str, text: &str, now: u64) -> Result<ClubMessage, String> {
        if !valid_club_scene(scene) { return Err("Unknown public scene".into()); }
        let body = text.trim();
        if body.is_empty() || body.chars().count() > 500 || body.chars().any(char::is_control) {
            return Err("Message must contain 1-500 printable characters".into());
        }
        // The write reservation covers presence, throttle, insertion and pruning,
        // including independent processes using the same database.
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
        let present: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM club_presence
            WHERE principal=?1 AND scene=?2 AND seen_at>?3 AND seen_at<=?4)",
            params![principal,scene,now.saturating_sub(45),now], |row| row.get(0)).map_err(|e| e.to_string())?;
        if !present { return Err("Join this scene before messaging".into()); }
        let last: Option<u64> = tx.query_row("SELECT MAX(sent_at) FROM (
            SELECT sent_at FROM club_message_rates WHERE principal=?1 UNION ALL
            SELECT sent_at FROM club_messages WHERE principal=?1 ORDER BY sent_at DESC LIMIT 1)",
            [principal], |row| row.get(0)).map_err(|e| e.to_string())?;
        if last.is_some_and(|sent| now < sent.saturating_add(2)) { return Err("Send messages at least two seconds apart".into()); }
        tx.execute("INSERT INTO club_messages(principal,scene,text,sent_at) VALUES (?1,?2,?3,?4)",
            params![principal,scene,body,now]).map_err(|e| e.to_string())?;
        let message = ClubMessage { id: tx.last_insert_rowid(), principal_did: principal.into(),
            scene: scene.into(), text: body.into(), sent_at_s: now };
        tx.execute("DELETE FROM club_message_rates WHERE sent_at<=?1", [now.saturating_sub(2)]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO club_message_rates VALUES (?1,?2) ON CONFLICT(principal)
            DO UPDATE SET sent_at=excluded.sent_at", params![principal,now]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM club_messages WHERE scene=?1 AND id NOT IN
            (SELECT id FROM club_messages WHERE scene=?1 ORDER BY id DESC LIMIT ?2)",
            params![scene,CLUB_HISTORY_PER_SCENE as i64]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(message)
    }
    pub fn club_messages(&self, scene: &str, after: i64) -> Result<Vec<ClubMessage>, String> {
        if !valid_club_scene(scene) { return Err("Unknown public scene".into()); }
        let mut query = self.db.prepare("SELECT id,principal,scene,text,sent_at FROM club_messages
            WHERE scene=?1 AND id>?2 ORDER BY id DESC LIMIT 100").map_err(|e| e.to_string())?;
        let rows = query.query_map(params![scene,after.max(0)], |row| Ok(ClubMessage {
            id: row.get(0)?, principal_did: row.get(1)?, scene: row.get(2)?,
            text: row.get(3)?, sent_at_s: row.get(4)?,
        })).map_err(|e| e.to_string())?;
        let mut messages = rows.collect::<Result<Vec<_>,_>>().map_err(|e| e.to_string())?;
        messages.reverse();
        Ok(messages)
    }
    pub fn issue_wallet_challenge(&mut self, principal: &str, wallet: &str, now: u64, ttl: u64)
        -> Result<crate::SolanaWalletLinkChallenge, String> {
        atlantis_protocol::PearlEnclaveIdentity::verifying_key_from_did(&format!("did:atlantis:{}", wallet))
            .map_err(|e| e.to_string())?;
        let challenge = crate::SolanaWalletLinkChallenge {
            principal_did: principal.into(), wallet_pubkey_hex: wallet.to_ascii_lowercase(),
            nonce: uuid::Uuid::new_v4().simple().to_string(), issued_at_s: now,
            expires_at_s: now.saturating_add(ttl.clamp(30, 300)),
        };
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let registered: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM auth_accounts WHERE principal=?1)", [principal], |r| r.get(0)).map_err(|e| e.to_string())?;
        if !registered { return Err("Unknown account".into()); }
        tx.execute("DELETE FROM wallet_challenges WHERE expires<=?1", [now]).map_err(|e| e.to_string())?;
        // One active challenge per account; replacing it invalidates the older request.
        tx.execute("DELETE FROM wallet_challenges WHERE principal=?1", [principal]).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO wallet_challenges VALUES (?1, ?2, ?3, ?4)",
            params![challenge.nonce, principal, challenge.expires_at_s, serde_json::to_string(&challenge).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(challenge)
    }
    pub fn link_wallet(&mut self, principal: &str, challenge: &crate::SolanaWalletLinkChallenge, signature: &str, now: u64)
        -> Result<String, String> {
        if principal != challenge.principal_did { return Err("Challenge owner mismatch".into()); }
        challenge.verify_wallet_signature(signature, now)?;
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let payload: Option<String> = tx.query_row("SELECT payload FROM wallet_challenges WHERE nonce=?1 AND principal=?2 AND expires>?3",
            params![challenge.nonce, principal, now], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
        if payload.as_deref() != Some(serde_json::to_string(challenge).map_err(|e| e.to_string())?.as_str()) {
            return Err("Unknown, expired or consumed wallet challenge".into());
        }
        let existing: Option<String> = tx.query_row("SELECT wallet FROM wallet_links WHERE principal=?1", [principal], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
        if existing.as_deref().is_some_and(|w| w != challenge.wallet_pubkey_hex) {
            return Err("Wallet replacement requires a separate recovery flow".into());
        }
        tx.execute("INSERT INTO wallet_links VALUES (?1, ?2) ON CONFLICT(principal) DO NOTHING", params![principal, challenge.wallet_pubkey_hex])
            .map_err(|_| "Wallet already belongs to another account".to_string())?;
        tx.execute("DELETE FROM wallet_challenges WHERE nonce=?1", [&challenge.nonce]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(challenge.wallet_pubkey_hex.clone())
    }
    pub fn revoke(&mut self, token: &str) -> Result<(), String> {
        let hash = Self::hash(token)?;
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
        // Logout withdraws this account's public opt-in; another device may
        // explicitly join again. Do not leave a logged-out user's beacon behind.
        tx.execute("DELETE FROM club_presence WHERE principal IN
            (SELECT principal FROM auth_sessions WHERE token_hash=?1)", [&hash]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM auth_sessions WHERE token_hash=?1", [&hash]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn revoke_all(&mut self, principal: &str) -> Result<(), String> {
        let tx = self.db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM club_presence WHERE principal=?1", [principal]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM wallet_challenges WHERE principal=?1", [principal]).map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM auth_sessions WHERE principal=?1", [principal]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn future_presence_is_hidden_and_cannot_authorize_messages() {
        let mut store = SessionStore::open(":memory:").unwrap();
        store.set_club_presence("did:atlantis:alice", "Atlantis:surface", 1000).unwrap();
        assert!(store.club_presence("Atlantis:surface", 900).unwrap().is_empty());
        assert!(store.post_club_message("did:atlantis:alice", "Atlantis:surface", "future", 900).is_err());
        store.set_club_presence("did:atlantis:bob", "Atlantis:surface", 900).unwrap();
        assert_eq!(store.club_presence("Atlantis:surface", 900).unwrap().len(), 1);
        assert!(store.post_club_message("did:atlantis:bob", "Atlantis:dungeon:1", "wrong scene", 900).is_err());
    }

    #[test]
    fn logout_withdraws_only_callers_presence_and_revoke_all_cancels_challenges() {
        let mut store = SessionStore::open(":memory:").unwrap();
        let (alice, _) = store.issue("did:atlantis:alice", 100, 5000).unwrap();
        let (bob, _) = store.issue("did:atlantis:bob", 100, 5000).unwrap();
        store.set_club_presence("did:atlantis:alice", "Atlantis:surface", 100).unwrap();
        store.set_club_presence("did:atlantis:bob", "Atlantis:surface", 100).unwrap();
        store.revoke(&alice).unwrap();
        let peers = store.club_presence("Atlantis:surface", 101).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].principal_did, "did:atlantis:bob");
        assert!(store.authenticate(&bob, 101).is_ok());
        let wallet = "11".repeat(32);
        store.issue_wallet_challenge("did:atlantis:bob", &wallet, 101, 300).unwrap();
        store.revoke_all("did:atlantis:bob").unwrap();
        assert!(store.club_presence("Atlantis:surface", 102).unwrap().is_empty());
        let count: i64 = store.db.query_row("SELECT COUNT(*) FROM wallet_challenges", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn concurrent_connections_cannot_bypass_message_throttle() {
        let path = std::env::temp_dir().join(format!("club-race-{}.sqlite", uuid::Uuid::new_v4()));
        let mut store = SessionStore::open(&path).unwrap();
        store.set_club_presence("did:atlantis:alice", "Atlantis:surface", 100).unwrap();
        let other = SessionStore::open(&path).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let joins: Vec<_> = [store, other].into_iter().map(|mut store| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.post_club_message("did:atlantis:alice", "Atlantis:surface", "race", 100).is_ok()
            })
        }).collect();
        let accepted = joins.into_iter().map(|join| usize::from(join.join().unwrap())).sum::<usize>();
        assert_eq!(accepted, 1);
        let store = SessionStore::open(&path).unwrap();
        assert_eq!(store.club_messages("Atlantis:surface", 0).unwrap().len(), 1);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn history_is_bounded_without_resetting_throttle_and_failures_are_atomic() {
        let mut store = SessionStore::open(":memory:").unwrap();
        store.set_club_presence("did:atlantis:alice", "Atlantis:surface", 100).unwrap();
        // Model a busy room without expensive repeated setup. Pruning must not
        // allow a sender to post again when other accounts evict their history.
        store.post_club_message("did:atlantis:alice", "Atlantis:surface", "original", 100).unwrap();
        for _ in 0..CLUB_HISTORY_PER_SCENE {
            store.db.execute("INSERT INTO club_messages(principal,scene,text,sent_at)
                VALUES ('did:atlantis:other','Atlantis:surface','fixture',100)", []).unwrap();
        }
        store.set_club_presence("did:atlantis:bob", "Atlantis:surface", 100).unwrap();
        store.post_club_message("did:atlantis:bob", "Atlantis:surface", "prune", 100).unwrap();
        let count: i64 = store.db.query_row("SELECT COUNT(*) FROM club_messages", [], |r| r.get(0)).unwrap();
        assert_eq!(count, CLUB_HISTORY_PER_SCENE as i64);
        assert!(store.post_club_message("did:atlantis:alice", "Atlantis:surface", "too soon", 101).is_err());
        store.db.execute_batch("CREATE TRIGGER fail_rate BEFORE INSERT ON club_message_rates
            BEGIN SELECT RAISE(ABORT, 'synthetic rate write failure'); END;").unwrap();
        assert!(store.post_club_message("did:atlantis:alice", "Atlantis:surface", "must roll back", 102).is_err());
        let count: i64 = store.db.query_row("SELECT COUNT(*) FROM club_messages WHERE text='must roll back'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
        store.db.execute_batch("DROP TRIGGER fail_rate").unwrap();
        assert!(store.post_club_message("did:atlantis:alice", "Atlantis:surface", "retry", 102).is_ok());
    }
    #[test]
    fn club_room_requires_real_scene_and_expires_presence_without_inventing_peers() {
        let path = std::env::temp_dir().join(format!("club-{}.sqlite", uuid::Uuid::new_v4()));
        let mut store = SessionStore::open(&path).unwrap();
        let alice = "did:atlantis:alice";
        let bob = "did:atlantis:bob";
        assert!(store.set_club_presence(alice, "private:home", 100).is_err());
        assert!(store.club_presence("Atlantis:surface", 100).unwrap().is_empty());
        store.set_club_presence(alice, "Atlantis:surface", 100).unwrap();
        store.set_club_presence(bob, "Atlantis:surface", 101).unwrap();
        assert_eq!(store.club_presence("Atlantis:surface", 102).unwrap().len(), 2);
        assert_eq!(store.club_presence("Atlantis:surface", 147).unwrap().len(), 0);
        assert!(store.post_club_message(alice, "Atlantis:surface", "too late", 200).is_err());
        store.set_club_presence(alice, "Atlantis:surface", 200).unwrap();
        store.set_club_presence(bob, "Atlantis:surface", 200).unwrap();
        assert!(store.post_club_message(alice, "Atlantis:surface", "hello", 200).is_ok());
        assert!(store.post_club_message(alice, "Atlantis:surface", "spam", 201).is_err());
        assert!(store.post_club_message(bob, "Atlantis:surface", "hi", 202).is_ok());
        assert!(store.post_club_message(bob, "Atlantis:surface", "\u{0}", 204).is_err());
        let messages = store.club_messages("Atlantis:surface", 0).unwrap();
        assert_eq!(messages.iter().map(|message| message.text.as_str()).collect::<Vec<_>>(), vec!["hello", "hi"]);
        assert!(store.club_messages("Atlantis:dungeon:1", 0).unwrap().is_empty());
        drop(store);
        let store = SessionStore::open(&path).unwrap();
        assert_eq!(store.club_messages("Atlantis:surface", messages[0].id).unwrap().len(), 1);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn wallet_challenge_and_link_survive_restart_without_replay_or_theft() {
        use ed25519_dalek::Signer;
        let path = std::env::temp_dir().join(format!("wallets-{}.sqlite", uuid::Uuid::new_v4()));
        let mut store = SessionStore::open(&path).unwrap();
        store.issue("did:atlantis:alice", 100, 5000).unwrap();
        store.issue("did:atlantis:bob", 100, 5000).unwrap();
        let key = atlantis_protocol::PearlEnclaveIdentity::from_label("persistent-wallet");
        let wallet = key.did().strip_prefix("did:atlantis:").unwrap().to_string();
        let challenge = store.issue_wallet_challenge("did:atlantis:alice", &wallet, 100, 300).unwrap();
        let signature = atlantis_protocol::bytes_to_hex(&key.signing_key().sign(challenge.signing_message().as_bytes()).to_bytes());
        drop(store);
        let mut store = SessionStore::open(&path).unwrap();
        assert!(store.link_wallet("did:atlantis:bob", &challenge, &signature, 101).is_err());
        assert!(store.link_wallet("did:atlantis:alice", &challenge, &signature, 101).is_ok());
        drop(store);
        let mut store = SessionStore::open(&path).unwrap();
        assert_eq!(store.wallet("did:atlantis:alice").unwrap(), Some(wallet.clone()));
        assert!(store.link_wallet("did:atlantis:alice", &challenge, &signature, 102).is_err());
        let theft = store.issue_wallet_challenge("did:atlantis:bob", &wallet, 102, 300).unwrap();
        let sig = atlantis_protocol::bytes_to_hex(&key.signing_key().sign(theft.signing_message().as_bytes()).to_bytes());
        assert!(store.link_wallet("did:atlantis:bob", &theft, &sig, 103).is_err());
        assert!(store.wallet("did:atlantis:bob").unwrap().is_none());
        let expired = store.issue_wallet_challenge("did:atlantis:bob", &wallet, 103, 30).unwrap();
        let sig = atlantis_protocol::bytes_to_hex(&key.signing_key().sign(expired.signing_message().as_bytes()).to_bytes());
        assert!(store.link_wallet("did:atlantis:bob", &expired, &sig, 133).is_err());
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn two_accounts_restart_expiry_and_revocation() {
        let path = std::env::temp_dir().join(format!("sessions-{}.sqlite", uuid::Uuid::new_v4()));
        let mut store = SessionStore::open(&path).unwrap();
        let (alice, _) = store.issue("did:atlantis:alice", 100, 5000).unwrap();
        let (alice2, _) = store.issue("did:atlantis:alice", 100, 5000).unwrap();
        let (bob, _) = store.issue("did:atlantis:bob", 100, 5000).unwrap();
        assert_ne!(alice, alice2);
        assert_eq!(store.authenticate(&alice, 101).unwrap(), "did:atlantis:alice");
        assert_eq!(store.authenticate(&bob, 101).unwrap(), "did:atlantis:bob");
        let raw: String = store.db.query_row("SELECT token_hash FROM auth_sessions WHERE principal='did:atlantis:bob'", [], |r| r.get(0)).unwrap();
        assert_ne!(raw, bob);
        drop(store);
        let mut store = SessionStore::open(&path).unwrap();
        assert!(store.authenticate(&alice, 102).is_ok());
        store.revoke(&alice).unwrap();
        assert!(store.authenticate(&alice, 103).is_err());
        assert!(store.authenticate(&alice2, 103).is_ok());
        store.revoke_all("did:atlantis:alice").unwrap();
        assert!(store.authenticate(&alice2, 104).is_err());
        assert!(store.authenticate(&bob, 104).is_ok());
        assert!(store.authenticate(&bob, 1004).is_err());
        let (short, _) = store.issue("did:atlantis:bob", 1100, 1102).unwrap();
        assert!(store.authenticate(&short, 1102).is_err());
        drop(store);
        let mut store = SessionStore::open(&path).unwrap();
        assert!(store.authenticate(&alice, 105).is_err());
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}
