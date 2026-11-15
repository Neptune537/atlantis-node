//! Root-authorized temporal checkpoints. Root provisioning and rollback floors
//! belong to an independent trusted control plane, never the network client.
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const DOMAIN: &[u8] = b"nautilus:temporal-trust:v1\0";
const MAX_UPDATES: usize = 1024;
const MAX_SNAPSHOT: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub revision: u64,
    pub previous: [u8; 32],
    pub authority: [u8; 32],
    pub epoch: u64,
    pub sequence: u64,
    pub tip: [u8; 32],
    pub revoked: bool,
    pub signature: Vec<u8>,
}
impl Update {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut bytes = DOMAIN.to_vec();
        bytes.extend(self.revision.to_le_bytes());
        bytes.extend(self.previous);
        bytes.extend(self.authority);
        bytes.extend(self.epoch.to_le_bytes());
        bytes.extend(self.sequence.to_le_bytes());
        bytes.extend(self.tip);
        bytes.push(u8::from(self.revoked));
        bytes
    }
    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.signing_bytes()).into()
    }
}

/// Must be obtained from storage/control separate from the potentially rolled-back file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustedFloor {
    pub revision: u64,
    pub digest: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid trust signature, transition, checkpoint or rollback floor")]
    Invalid,
    #[error("trust journal capacity exceeded")]
    Capacity,
    #[error("trust snapshot I/O: {0}")]
    Io(#[from] std::io::Error),
}

/// Fields are private: hosts accept only a root-verified journal, not deserialized state.
#[derive(Clone)]
pub struct TrustJournal {
    root: VerifyingKey,
    updates: Vec<Update>,
    pub(crate) epoch_keys: BTreeMap<u64, VerifyingKey>,
}
impl TrustJournal {
    pub fn provision(root: VerifyingKey, genesis: Update) -> Result<Self, Error> {
        verify(&root, &genesis)?;
        if genesis.revision != 0 || genesis.previous != [0; 32] || genesis.revoked {
            return Err(Error::Invalid);
        }
        let key = VerifyingKey::from_bytes(&genesis.authority).map_err(|_| Error::Invalid)?;
        Ok(Self {
            root,
            epoch_keys: BTreeMap::from([(genesis.epoch, key)]),
            updates: vec![genesis],
        })
    }
    pub fn append(&mut self, next: Update) -> Result<(), Error> {
        if self.updates.len() >= MAX_UPDATES {
            return Err(Error::Capacity);
        }
        verify(&self.root, &next)?;
        let old = self.current();
        if next.revision != old.revision.checked_add(1).ok_or(Error::Invalid)?
            || next.previous != old.digest()
            || next.epoch < old.epoch
        {
            return Err(Error::Invalid);
        }
        if next.epoch == old.epoch {
            // Revocation is terminal for this epoch. A root-authorized replacement
            // must use a new epoch/key. Equal sequence cannot replace the chain tip.
            if old.revoked
                || next.authority != old.authority
                || next.sequence < old.sequence
                || (next.sequence == old.sequence && next.tip != old.tip)
                || (next.revoked && (next.sequence != old.sequence || next.tip != old.tip))
            {
                return Err(Error::Invalid);
            }
        } else if next.epoch != old.epoch.checked_add(1).ok_or(Error::Invalid)?
            || next.revoked
            || next.sequence != 0
            || self
                .epoch_keys
                .values()
                .any(|key| key.to_bytes() == next.authority)
        {
            return Err(Error::Invalid);
        }
        let key = VerifyingKey::from_bytes(&next.authority).map_err(|_| Error::Invalid)?;
        self.epoch_keys.insert(next.epoch, key);
        self.updates.push(next);
        Ok(())
    }
    pub fn current(&self) -> &Update {
        self.updates.last().expect("nonempty verified journal")
    }
    pub fn floor(&self) -> TrustedFloor {
        TrustedFloor {
            revision: self.current().revision,
            digest: self.current().digest(),
        }
    }
    pub(crate) fn root(&self) -> [u8; 32] {
        self.root.to_bytes()
    }
    pub(crate) fn contains(&self, floor: TrustedFloor) -> bool {
        self.updates
            .iter()
            .any(|u| u.revision == floor.revision && u.digest() == floor.digest)
    }
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let bytes = serde_json::to_vec(&self.updates).map_err(|_| Error::Invalid)?;
        if bytes.len() > MAX_SNAPSHOT {
            return Err(Error::Capacity);
        }
        Ok(bytes)
    }
    pub fn restore(root: VerifyingKey, bytes: &[u8], floor: TrustedFloor) -> Result<Self, Error> {
        if bytes.len() > MAX_SNAPSHOT {
            return Err(Error::Capacity);
        }
        let updates: Vec<Update> = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
        if updates.len() > MAX_UPDATES {
            return Err(Error::Capacity);
        }
        let mut updates = updates.into_iter();
        let mut journal = Self::provision(root, updates.next().ok_or(Error::Invalid)?)?;
        for update in updates {
            journal.append(update)?;
        }
        if !journal.contains(floor) {
            return Err(Error::Invalid);
        }
        Ok(journal)
    }
    /// Writes a new immutable snapshot, never overwrites a prior recovery copy.
    /// Publish its floor externally only after success. A failed write can leave
    /// an incomplete new file, which restore rejects; retry at a new filename.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_new(&self, path: &std::path::Path) -> Result<TrustedFloor, Error> {
        use std::io::Write;
        let bytes = self.encode()?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(self.floor())
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(
        root: VerifyingKey,
        path: &std::path::Path,
        floor: TrustedFloor,
    ) -> Result<Self, Error> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take((MAX_SNAPSHOT + 1) as u64)
            .read_to_end(&mut bytes)?;
        Self::restore(root, &bytes, floor)
    }
}
fn verify(root: &VerifyingKey, update: &Update) -> Result<(), Error> {
    let signature = Signature::from_slice(&update.signature).map_err(|_| Error::Invalid)?;
    root.verify_strict(&update.signing_bytes(), &signature)
        .map_err(|_| Error::Invalid)?;
    let key = VerifyingKey::from_bytes(&update.authority).map_err(|_| Error::Invalid)?;
    if root.is_weak() || key.is_weak() {
        return Err(Error::Invalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    fn sign(mut update: Update, root: &SigningKey) -> Update {
        update.signature = root.sign(&update.signing_bytes()).to_bytes().to_vec();
        update
    }
    fn setup() -> (SigningKey, TrustJournal) {
        let root = SigningKey::from_bytes(&[42; 32]);
        let genesis = sign(
            Update {
                revision: 0,
                previous: [0; 32],
                authority: SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes(),
                epoch: 3,
                sequence: 0,
                tip: [0; 32],
                revoked: false,
                signature: vec![],
            },
            &root,
        );
        let journal = TrustJournal::provision(root.verifying_key(), genesis).unwrap();
        (root, journal)
    }
    fn next(journal: &TrustJournal) -> Update {
        Update {
            revision: journal.current().revision + 1,
            previous: journal.current().digest(),
            signature: vec![],
            ..journal.current().clone()
        }
    }
    #[test]
    fn signature_floor_fork_and_epoch_rejections() {
        let (root, mut journal) = setup();
        let genesis = journal.encode().unwrap();
        let mut update = next(&journal);
        update.sequence = 10;
        update.tip = [8; 32];
        let signed = sign(update.clone(), &root);
        let mut tampered = signed.clone();
        tampered.tip = [9; 32];
        assert!(journal.append(tampered).is_err());
        assert!(journal
            .append(sign(update, &SigningKey::from_bytes(&[9; 32])))
            .is_err());
        journal.append(signed).unwrap();
        assert!(TrustJournal::restore(root.verifying_key(), &genesis, journal.floor()).is_err());
        let mut invalid = next(&journal);
        invalid.sequence = 9;
        assert!(journal.append(sign(invalid, &root)).is_err());
        let mut invalid = next(&journal);
        invalid.tip = [1; 32];
        assert!(journal.append(sign(invalid, &root)).is_err());
        let mut invalid = next(&journal);
        invalid.epoch += 2;
        assert!(journal.append(sign(invalid, &root)).is_err());
        let bytes = journal.encode().unwrap();
        assert!(TrustJournal::restore(
            root.verifying_key(),
            &bytes,
            TrustedFloor {
                revision: 1,
                digest: [99; 32]
            }
        )
        .is_err());
    }
    #[test]
    fn revocation_requires_new_epoch_and_never_reuses_keys() {
        let (root, mut journal) = setup();
        let mut revoked = next(&journal);
        revoked.revoked = true;
        journal.append(sign(revoked, &root)).unwrap();
        let mut restored = next(&journal);
        restored.revoked = false;
        assert!(journal.append(sign(restored.clone(), &root)).is_err());
        restored.epoch += 1;
        assert!(journal.append(sign(restored.clone(), &root)).is_err());
        restored.authority = SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes();
        journal.append(sign(restored, &root)).unwrap();
        assert_eq!(journal.epoch_keys.len(), 2);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn immutable_snapshot_restart_and_truncation() {
        let (root, journal) = setup();
        let path = std::env::temp_dir().join(format!("trust-{}.json", uuid::Uuid::new_v4()));
        let floor = journal.save_new(&path).unwrap();
        assert!(journal.save_new(&path).is_err());
        let reopened = TrustJournal::open(root.verifying_key(), &path, floor).unwrap();
        assert_eq!(reopened.floor(), floor);
        let bytes = journal.encode().unwrap();
        assert!(
            TrustJournal::restore(root.verifying_key(), &bytes[..bytes.len() / 2], floor).is_err()
        );
        std::fs::remove_file(path).unwrap();
    }
    fn canary(
        host: &mut crate::temporal_anchor::OracleHost,
        key: &SigningKey,
    ) -> crate::temporal_anchor::Canary {
        use crate::{oracle_verification::OraclePolicy, temporal_anchor::Canary};
        let challenge = host
            .challenge(
                [9; 32],
                OraclePolicy {
                    non_critical: false,
                    budget: std::time::Duration::from_millis(250),
                },
                std::time::Duration::ZERO,
            )
            .unwrap();
        let mut canary = Canary {
            challenge,
            iterations: 2,
            digest: [0; 32],
            signature: vec![],
        };
        canary.digest = canary.compute_digest().unwrap();
        canary.signature = key.sign(&canary.signing_bytes()).to_bytes().to_vec();
        canary
    }
    #[test]
    fn host_revocation_invalidates_pending_and_permits_then_rotation_recovers() {
        use crate::temporal_anchor::OracleHost;
        let (root, mut journal) = setup();
        let mut host = OracleHost::from_trust(&journal);
        let old_key = SigningKey::from_bytes(&[7; 32]);
        let proof = canary(&mut host, &old_key);
        let permit = host.verify(proof).unwrap().into_authorization().unwrap();
        let pending = canary(&mut host, &old_key);
        // A valid root signature is insufficient to roll back live verified progress.
        journal.append(sign(next(&journal), &root)).unwrap();
        assert!(host.apply_trust(&journal).is_err());
        let mut revoked = next(&journal);
        revoked.revoked = true;
        journal.append(sign(revoked, &root)).unwrap();
        host.apply_trust(&journal).unwrap();
        assert!(host.verify(pending).is_err());
        assert!(host
            .challenge(
                [9; 32],
                crate::oracle_verification::OraclePolicy {
                    non_critical: false,
                    budget: std::time::Duration::from_millis(250)
                },
                std::time::Duration::ZERO
            )
            .is_err());
        let mut ledger = tide_ledger::v2::Ledger::default();
        let request = tide_ledger::v2::Request {
            id: uuid::Uuid::new_v4(),
            command: tide_ledger::v2::Command::Fund {
                account: uuid::Uuid::new_v4(),
                asset: tide_ledger::v2::Asset::Cyan,
                units: 1,
            },
        };
        assert!(host
            .commit_ledger(permit, [9; 32], &mut ledger, request)
            .is_err());
        let new_key = SigningKey::from_bytes(&[8; 32]);
        let mut replacement = next(&journal);
        replacement.revoked = false;
        replacement.epoch += 1;
        replacement.authority = new_key.verifying_key().to_bytes();
        journal.append(sign(replacement, &root)).unwrap();
        host.apply_trust(&journal).unwrap();
        assert!(host.apply_trust(&journal).is_err());
        let wrong = canary(&mut host, &old_key);
        assert!(host.verify(wrong).is_err());
        let good = canary(&mut host, &new_key);
        assert!(host.verify(good).unwrap().into_authorization().is_some());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rotated_host_audits_prior_epoch_and_commits_new_epoch_after_restart() {
        use crate::temporal_anchor::OracleHost;
        use tide_ledger::{
            durable::DurableLedger,
            v2::{Asset, Command, Request},
        };
        let (root, mut journal) = setup();
        let mut host = OracleHost::from_trust(&journal);
        let path =
            std::env::temp_dir().join(format!("rotated-ledger-{}.sqlite", uuid::Uuid::new_v4()));
        let mut store = DurableLedger::create(&path).unwrap();
        let account = uuid::Uuid::new_v4();
        let request = || Request {
            id: uuid::Uuid::new_v4(),
            command: Command::Fund {
                account,
                asset: Asset::Cyan,
                units: 1,
            },
        };
        let proof = canary(&mut host, &SigningKey::from_bytes(&[7; 32]));
        host.verify_and_commit_durable(proof, [9; 32], &mut store, request())
            .unwrap();
        let new_key = SigningKey::from_bytes(&[8; 32]);
        let mut replacement = next(&journal);
        replacement.epoch += 1;
        replacement.authority = new_key.verifying_key().to_bytes();
        journal.append(sign(replacement, &root)).unwrap();
        let recovered = TrustJournal::restore(
            root.verifying_key(),
            &journal.encode().unwrap(),
            journal.floor(),
        )
        .unwrap();
        let mut restarted = OracleHost::from_trust(&recovered);
        restarted
            .audit_durable_record(&store.records().unwrap()[0])
            .unwrap();
        let proof = canary(&mut restarted, &new_key);
        restarted
            .verify_and_commit_durable(proof, [9; 32], &mut store, request())
            .unwrap();
        assert_eq!(store.snapshot().unwrap().balance(Asset::Cyan, account), 2);
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
