//! Authenticated challenge-bound temporal anchoring. This is a host API, not consensus.
//! Pinned authority/checkpoint provisioning is out-of-band. No remote wall clock is trusted.
use crate::oracle_verification::{Evidence, OraclePolicy, Resolution};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use uuid::Uuid;

const DOMAIN: &[u8] = b"nautilus:temporal-anchor:v1\0";
const MAX_WORK: u32 = 4096;
const MAX_PENDING: usize = 8;
const CHALLENGE_TTL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Challenge {
    pub nonce: Uuid,
    pub tile: [u8; 32],
    pub epoch: u64,
    pub sequence: u64,
    pub previous: [u8; 32],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Canary {
    pub challenge: Challenge,
    pub iterations: u32,
    pub digest: [u8; 32],
    pub signature: Vec<u8>,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("verification queue full or invalid budget")]
    Capacity,
    #[error("unknown, expired or replayed challenge")]
    Replay,
    #[error("invalid authority signature, chain, epoch or tile binding")]
    Invalid,
    #[error("stale evidence or visual QoS cannot authorize commit")]
    Stale,
}

struct Pending {
    challenge: Challenge,
    issued: Instant,
    policy: OraclePolicy,
    uncertainty: Duration,
}
/// Deliberately not Clone, Serialize or publicly constructible. Commit consumes it.
pub struct Authorization {
    host: Uuid,
    tile: [u8; 32],
    issued: Instant,
    budget: Duration,
    uncertainty: Duration,
}
pub struct Verdict {
    pub resolution: Resolution,
    pub elapsed: Duration,
    authorization: Option<Authorization>,
}
#[derive(Debug, thiserror::Error)]
pub enum CommitError {
    #[error(transparent)]
    Anchor(#[from] Error),
    #[error(transparent)]
    Ledger(#[from] tide_ledger::v2::Error),
    #[cfg(not(target_arch = "wasm32"))]
    #[error(transparent)]
    Durable(#[from] tide_ledger::durable::Error),
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableEvidence {
    pub authority_key: [u8; 32],
    pub canary: Canary,
    pub budget_ns: u64,
    pub uncertainty_ns: u64,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct DurableCommit {
    pub receipt: tide_ledger::durable::Receipt,
    /// None on an idempotent read: no new commit hook is run.
    pub age_at_commit_check: Option<Duration>,
    pub acknowledgement_age: Duration,
    /// A successful durable write is never reported as a rollback merely because
    /// disk synchronization or acknowledgement completed after the temporal budget.
    pub acknowledged_after_budget: bool,
}
impl Verdict {
    pub fn into_authorization(self) -> Option<Authorization> {
        self.authorization
    }
}

pub struct OracleHost {
    key: VerifyingKey,
    epoch: u64,
    sequence: u64,
    tip: [u8; 32],
    id: Uuid,
    pending: HashMap<Uuid, Pending>,
    trust: Option<([u8; 32], crate::temporal_trust::TrustedFloor)>,
    epoch_keys: std::collections::BTreeMap<u64, VerifyingKey>,
    revoked: bool,
}
impl Canary {
    fn prefix(&self) -> Vec<u8> {
        let c = &self.challenge;
        let mut bytes = DOMAIN.to_vec();
        bytes.extend(c.nonce.as_bytes());
        bytes.extend(c.tile);
        bytes.extend(c.epoch.to_le_bytes());
        bytes.extend(c.sequence.to_le_bytes());
        bytes.extend(c.previous);
        bytes.extend(self.iterations.to_le_bytes());
        bytes
    }
    /// Canonical bytes for an independently operated authority to sign.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut bytes = self.prefix();
        bytes.extend(self.digest);
        bytes
    }
    pub fn compute_digest(&self) -> Result<[u8; 32], Error> {
        if self.iterations == 0 || self.iterations > MAX_WORK {
            return Err(Error::Invalid);
        }
        let mut hash: [u8; 32] = Sha256::digest(self.prefix()).into();
        for _ in 0..self.iterations {
            hash = Sha256::digest(hash).into();
        }
        Ok(hash)
    }
}

impl OracleHost {
    pub fn new(key: VerifyingKey, epoch: u64, sequence: u64, checkpoint: [u8; 32]) -> Self {
        Self {
            key,
            epoch,
            sequence,
            tip: checkpoint,
            id: Uuid::new_v4(),
            pending: HashMap::new(),
            trust: None,
            epoch_keys: std::collections::BTreeMap::from([(epoch, key)]),
            revoked: false,
        }
    }
    /// Managed construction from an authenticated checkpoint journal. Provisioning
    /// the root and obtaining a fresh external rollback floor remain host duties.
    pub fn from_trust(journal: &crate::temporal_trust::TrustJournal) -> Self {
        let current = journal.current();
        let mut host = Self::new(
            journal.epoch_keys[&current.epoch],
            current.epoch,
            current.sequence,
            current.tip,
        );
        host.epoch_keys = journal.epoch_keys.clone();
        host.trust = Some((journal.root(), journal.floor()));
        host.revoked = current.revoked;
        host
    }
    /// Apply a descendant root-authorized state. Every update invalidates outstanding
    /// challenges and permits. Revocation remains applicable after local chain progress.
    pub fn apply_trust(
        &mut self,
        journal: &crate::temporal_trust::TrustJournal,
    ) -> Result<(), Error> {
        let (root, floor) = self.trust.ok_or(Error::Invalid)?;
        let next = journal.current();
        if journal.root() != root || !journal.contains(floor) || next.revision <= floor.revision {
            return Err(Error::Invalid);
        }
        if next.epoch == self.epoch
            && !next.revoked
            && (next.sequence < self.sequence
                || (next.sequence == self.sequence && next.tip != self.tip))
        {
            return Err(Error::Invalid);
        }
        self.key = journal.epoch_keys[&next.epoch];
        if next.epoch != self.epoch || !next.revoked {
            self.sequence = next.sequence;
            self.tip = next.tip;
        }
        self.epoch = next.epoch;
        self.revoked = next.revoked;
        self.epoch_keys = journal.epoch_keys.clone();
        self.trust = Some((root, journal.floor()));
        self.id = Uuid::new_v4();
        self.pending.clear();
        Ok(())
    }
    /// Called by the trusted host, never with client-selected criticality or uncertainty.
    pub fn challenge(
        &mut self,
        tile: [u8; 32],
        policy: OraclePolicy,
        uncertainty: Duration,
    ) -> Result<Challenge, Error> {
        if self.revoked {
            return Err(Error::Invalid);
        }
        self.pending
            .retain(|_, pending| pending.issued.elapsed() < CHALLENGE_TTL);
        if self.pending.len() >= MAX_PENDING
            || policy.budget.is_zero()
            || policy.budget > CHALLENGE_TTL
        {
            return Err(Error::Capacity);
        }
        let sequence = self.sequence.checked_add(1).ok_or(Error::Invalid)?;
        let challenge = Challenge {
            nonce: Uuid::new_v4(),
            tile,
            epoch: self.epoch,
            sequence,
            previous: self.tip,
        };
        self.pending.insert(
            challenge.nonce,
            Pending {
                challenge: challenge.clone(),
                issued: Instant::now(),
                policy,
                uncertainty,
            },
        );
        Ok(challenge)
    }
    /// Run on a bounded host verification worker. Signature and work limits are checked
    /// before any chain mutation; an attempted challenge is consumed even on failure.
    pub fn verify(&mut self, canary: Canary) -> Result<Verdict, Error> {
        if self.revoked {
            return Err(Error::Invalid);
        }
        let pending = self
            .pending
            .remove(&canary.challenge.nonce)
            .ok_or(Error::Replay)?;
        let c = &canary.challenge;
        let expected = &pending.challenge;
        if pending.issued.elapsed() >= CHALLENGE_TTL {
            return Err(Error::Replay);
        }
        if c.tile != expected.tile
            || c.epoch != expected.epoch
            || c.sequence != expected.sequence
            || c.previous != expected.previous
            || c.epoch != self.epoch
            || c.sequence != self.sequence.checked_add(1).ok_or(Error::Invalid)?
            || c.previous != self.tip
        {
            return Err(Error::Invalid);
        }
        let signature = Signature::from_slice(&canary.signature).map_err(|_| Error::Invalid)?;
        self.key
            .verify_strict(&canary.signing_bytes(), &signature)
            .map_err(|_| Error::Invalid)?;
        if canary.compute_digest()? != canary.digest {
            return Err(Error::Invalid);
        }
        let elapsed = pending.issued.elapsed();
        let resolution =
            pending
                .policy
                .oracle_verification(Evidence::Verified, elapsed, pending.uncertainty);
        // Authenticated stale evidence can advance chain continuity, never settlement.
        self.sequence = c.sequence;
        self.tip = canary.digest;
        let authorization = if resolution == Resolution::Anchored {
            Some(Authorization {
                host: self.id,
                tile: c.tile,
                issued: pending.issued,
                budget: pending.policy.budget.min(Duration::from_millis(250)),
                uncertainty: pending.uncertainty,
            })
        } else {
            None
        };
        Ok(Verdict {
            resolution,
            elapsed,
            authorization,
        })
    }
    /// Stage an authorized operation, then recheck freshness immediately before
    /// replacing live in-memory state. Errors leave the original ledger untouched.
    /// Callers must bind their economic authorization to the same tile. Durable
    /// commit requires an equivalent check inside the service's database transaction.
    pub fn commit_ledger(
        &self,
        authorization: Authorization,
        tile: [u8; 32],
        ledger: &mut tide_ledger::v2::Ledger,
        request: tide_ledger::v2::Request,
    ) -> Result<tide_ledger::v2::Batch, CommitError> {
        self.check_authorization(&authorization, tile)?;
        let mut staged = ledger.clone();
        let batch = staged.execute(request)?;
        self.check_authorization(&authorization, tile)?;
        *ledger = staged;
        Ok(batch)
    }

    /// Native durable integration. Economic permission is still the caller's
    /// responsibility. Verified evidence gates COMMIT, not just a memory update.
    /// SQLite's FULL sync follows the last rollback-capable freshness check.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn verify_and_commit_durable(
        &mut self,
        canary: Canary,
        tile: [u8; 32],
        store: &mut tide_ledger::durable::DurableLedger,
        request: tide_ledger::v2::Request,
    ) -> Result<DurableCommit, CommitError> {
        use std::sync::{Arc, Mutex};
        use tide_ledger::durable::{Context, Error as StoreError};
        if canary.challenge.tile != tile {
            return Err(Error::Invalid.into());
        }
        let retained = canary.clone();
        let verdict = self.verify(canary)?;
        let authorization = verdict.into_authorization().ok_or(Error::Stale)?;
        self.check_authorization(&authorization, tile)?;
        let issued = authorization.issued;
        let budget = authorization.budget;
        let uncertainty = authorization.uncertainty;
        let evidence = DurableEvidence {
            authority_key: self.key.to_bytes(),
            canary: retained,
            budget_ns: u64::try_from(budget.as_nanos()).map_err(|_| Error::Invalid)?,
            uncertainty_ns: u64::try_from(uncertainty.as_nanos()).map_err(|_| Error::Invalid)?,
        };
        let context = Context {
            tile,
            proof_id: evidence.canary.challenge.nonce,
            evidence: serde_json::to_vec(&evidence)
                .map_err(|e| StoreError::Storage(e.to_string()))?,
        };
        let observed = Arc::new(Mutex::new(None));
        let at_hook = observed.clone();
        let incoming = evidence.canary.clone();
        let receipt = store
            .execute_validated(
                request,
                context,
                |records| {
                    let mut previous: Option<DurableEvidence> = None;
                    for record in records {
                        self.audit_durable_record(record)
                            .map_err(|_| StoreError::EvidenceRejected)?;
                        let proof: DurableEvidence =
                            serde_json::from_slice(&record.context.evidence)
                                .map_err(|_| StoreError::EvidenceRejected)?;
                        if let Some(old) = previous.as_ref() {
                            if !follows(&old.canary, &proof.canary) {
                                return Err(StoreError::EvidenceRejected);
                            }
                        }
                        previous = Some(proof);
                    }
                    if let Some(old) = previous.as_ref() {
                        if !follows(&old.canary, &incoming) {
                            return Err(StoreError::EvidenceRejected);
                        }
                    }
                    Ok(())
                },
                move || {
                    let age = issued.elapsed().checked_add(uncertainty);
                    let fresh = age.is_some_and(|value| value < budget);
                    // Failure to observe the check must veto, rather than publish a false receipt.
                    match at_hook.lock() {
                        Ok(mut observed) => {
                            *observed = age;
                            fresh
                        }
                        Err(_) => false,
                    }
                },
            )
            .map_err(|error| match error {
                StoreError::GuardRejected => CommitError::Anchor(Error::Stale),
                other => CommitError::Durable(other),
            })?;
        // No fallible operation may turn a completed COMMIT into a false rollback.
        let age_at_commit_check = observed.lock().ok().and_then(|value| *value);
        let acknowledgement_age = issued
            .elapsed()
            .checked_add(uncertainty)
            .unwrap_or(Duration::MAX);
        Ok(DurableCommit {
            receipt,
            age_at_commit_check,
            acknowledgement_age,
            acknowledged_after_budget: acknowledgement_age >= budget,
        })
    }

    /// Cryptographic/binding audit against this host's pinned authority. Does not
    /// recreate expired permits or prove historical physical commit timing.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn audit_durable_record(&self, record: &tide_ledger::durable::Record) -> Result<(), Error> {
        let evidence: DurableEvidence =
            serde_json::from_slice(&record.context.evidence).map_err(|_| Error::Invalid)?;
        let canary = &evidence.canary;
        let key = self
            .epoch_keys
            .get(&canary.challenge.epoch)
            .ok_or(Error::Invalid)?;
        if evidence.authority_key != key.to_bytes()
            || canary.challenge.tile != record.context.tile
            || canary.challenge.nonce != record.context.proof_id
            || evidence.budget_ns == 0
            || evidence.budget_ns > 250_000_000
            || evidence.uncertainty_ns >= evidence.budget_ns
        {
            return Err(Error::Invalid);
        }
        let signature = Signature::from_slice(&canary.signature).map_err(|_| Error::Invalid)?;
        key.verify_strict(&canary.signing_bytes(), &signature)
            .map_err(|_| Error::Invalid)?;
        if canary.compute_digest()? != canary.digest {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    fn check_authorization(
        &self,
        authorization: &Authorization,
        tile: [u8; 32],
    ) -> Result<(), Error> {
        if self.revoked || authorization.host != self.id || authorization.tile != tile {
            return Err(Error::Invalid);
        }
        if !authorization
            .issued
            .elapsed()
            .checked_add(authorization.uncertainty)
            .is_some_and(|age| age < authorization.budget)
        {
            return Err(Error::Stale);
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn follows(previous: &Canary, next: &Canary) -> bool {
    let old = &previous.challenge;
    let new = &next.challenge;
    if new.epoch < old.epoch {
        return false;
    }
    if new.epoch > old.epoch {
        return true;
    } // A new trusted epoch needs out-of-band provisioning.
    if new.sequence <= old.sequence {
        return false;
    }
    if old.sequence.checked_add(1) == Some(new.sequence) {
        new.previous == previous.digest
    } else {
        true
    } // Verified but uncommitted canaries may occupy the intervening sequence.
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    fn signed(c: Challenge, key: &SigningKey) -> Canary {
        let mut canary = Canary {
            challenge: c,
            iterations: 2,
            digest: [0; 32],
            signature: vec![],
        };
        canary.digest = canary.compute_digest().unwrap();
        canary.signature = key.sign(&canary.signing_bytes()).to_bytes().to_vec();
        canary
    }
    #[test]
    fn signature_replay_binding_and_single_commit() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let tile = [9; 32];
        let mut host = OracleHost::new(key.verifying_key(), 3, 0, [0; 32]);
        let policy = OraclePolicy {
            non_critical: false,
            budget: Duration::from_millis(250),
        };
        let c = host.challenge(tile, policy, Duration::ZERO).unwrap();
        let canary = signed(c, &key);
        let verdict = host.verify(canary.clone()).unwrap();
        assert!(matches!(host.verify(canary), Err(Error::Replay)));
        let mut ledger = tide_ledger::v2::Ledger::default();
        let request = tide_ledger::v2::Request {
            id: Uuid::new_v4(),
            command: tide_ledger::v2::Command::Fund {
                account: Uuid::new_v4(),
                asset: tide_ledger::v2::Asset::Cyan,
                units: 1,
            },
        };
        host.commit_ledger(
            verdict.into_authorization().unwrap(),
            tile,
            &mut ledger,
            request,
        )
        .unwrap();
        ledger.audit().unwrap();
        let c = host.challenge(tile, policy, Duration::ZERO).unwrap();
        let mut bad = signed(c, &key);
        bad.challenge.tile = [8; 32];
        assert!(matches!(host.verify(bad), Err(Error::Invalid)));
        let c = host.challenge(tile, policy, Duration::ZERO).unwrap();
        let bad_key = SigningKey::from_bytes(&[8; 32]);
        assert!(matches!(
            host.verify(signed(c, &bad_key)),
            Err(Error::Invalid)
        ));
    }
    #[test]
    fn stale_visual_and_queued_commit_never_settle() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let tile = [9; 32];
        let mut host = OracleHost::new(key.verifying_key(), 3, 0, [0; 32]);
        let policy = OraclePolicy {
            non_critical: false,
            budget: Duration::from_millis(250),
        };
        let c = host
            .challenge(tile, policy, Duration::from_millis(250))
            .unwrap();
        assert!(host
            .verify(signed(c, &key))
            .unwrap()
            .into_authorization()
            .is_none());
        let c = host
            .challenge(
                tile,
                OraclePolicy {
                    non_critical: true,
                    ..policy
                },
                Duration::ZERO,
            )
            .unwrap();
        let verdict = host.verify(signed(c, &key)).unwrap();
        assert_eq!(verdict.resolution, Resolution::VisualAnchored);
        assert!(verdict.into_authorization().is_none());
        let c = host.challenge(tile, policy, Duration::ZERO).unwrap();
        let mut permit = host
            .verify(signed(c, &key))
            .unwrap()
            .into_authorization()
            .unwrap();
        permit.issued = Instant::now() - Duration::from_secs(1);
        assert!(matches!(
            host.check_authorization(&permit, tile),
            Err(Error::Stale)
        ));
    }
    #[test]
    fn bounded_pending_queue_and_work() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let mut host = OracleHost::new(key.verifying_key(), 1, 0, [0; 32]);
        let policy = OraclePolicy {
            non_critical: false,
            budget: Duration::from_millis(250),
        };
        for _ in 0..MAX_PENDING {
            host.challenge([0; 32], policy, Duration::ZERO).unwrap();
        }
        assert!(matches!(
            host.challenge([0; 32], policy, Duration::ZERO),
            Err(Error::Capacity)
        ));
        let c = host.pending.values().next().unwrap().challenge.clone();
        let mut canary = signed(c, &key);
        canary.iterations = MAX_WORK + 1;
        assert!(matches!(canary.compute_digest(), Err(Error::Invalid)));
    }

    #[test]
    fn failed_commit_preserves_ledger_and_wrong_tile_never_mutates() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let tile = [9; 32];
        let mut host = OracleHost::new(key.verifying_key(), 3, 0, [0; 32]);
        let policy = OraclePolicy {
            non_critical: false,
            budget: Duration::from_millis(250),
        };
        let mut ledger = tide_ledger::v2::Ledger::default();
        let before = ledger.clone();
        let invalid = tide_ledger::v2::Request {
            id: Uuid::new_v4(),
            command: tide_ledger::v2::Command::Fund {
                account: Uuid::new_v4(),
                asset: tide_ledger::v2::Asset::Cyan,
                units: 0,
            },
        };
        let challenge = host.challenge(tile, policy, Duration::ZERO).unwrap();
        let permit = host
            .verify(signed(challenge, &key))
            .unwrap()
            .into_authorization()
            .unwrap();
        assert!(matches!(
            host.commit_ledger(permit, tile, &mut ledger, invalid.clone()),
            Err(CommitError::Ledger(_))
        ));
        assert_eq!(ledger, before);
        let challenge = host.challenge(tile, policy, Duration::ZERO).unwrap();
        let permit = host
            .verify(signed(challenge, &key))
            .unwrap()
            .into_authorization()
            .unwrap();
        assert!(matches!(
            host.commit_ledger(permit, [8; 32], &mut ledger, invalid),
            Err(CommitError::Anchor(Error::Invalid))
        ));
        assert_eq!(ledger, before);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn durable_host_restart_and_tampered_proof_audit() {
        use tide_ledger::{
            durable::DurableLedger,
            v2::{Asset, Command, Request},
        };
        let path = std::env::temp_dir().join(format!("oracle-durable-{}.sqlite", Uuid::new_v4()));
        let key = SigningKey::from_bytes(&[7; 32]);
        let tile = [9; 32];
        let policy = OraclePolicy {
            non_critical: false,
            budget: Duration::from_millis(250),
        };
        let mut host = OracleHost::new(key.verifying_key(), 3, 0, [0; 32]);
        let mut store = DurableLedger::create(&path).unwrap();
        let account = Uuid::new_v4();
        let request = Request {
            id: Uuid::new_v4(),
            command: Command::Fund {
                account,
                asset: Asset::Cyan,
                units: 100,
            },
        };
        let challenge = host.challenge(tile, policy, Duration::ZERO).unwrap();
        let canary = signed(challenge, &key);
        let tip = canary.digest;
        let commit = host
            .verify_and_commit_durable(canary, tile, &mut store, request.clone())
            .unwrap();
        assert!(!commit.receipt.replayed);
        assert!(commit.age_at_commit_check.unwrap() < Duration::from_millis(250));
        host.audit_durable_record(&commit.receipt.record).unwrap();
        drop(store);
        // This test supplies its known trusted checkpoint explicitly; production
        // checkpoint provisioning/rotation is not inferred from untrusted DB bytes.
        let mut restarted = OracleHost::new(key.verifying_key(), 3, 1, tip);
        let mut store = DurableLedger::open(&path).unwrap();
        let stored = store.records().unwrap();
        restarted.audit_durable_record(&stored[0]).unwrap();
        assert_eq!(store.snapshot().unwrap().balance(Asset::Cyan, account), 100);
        let challenge = restarted.challenge(tile, policy, Duration::ZERO).unwrap();
        let retry = restarted
            .verify_and_commit_durable(signed(challenge, &key), tile, &mut store, request)
            .unwrap();
        assert!(retry.receipt.replayed);
        assert!(retry.age_at_commit_check.is_none());
        assert_eq!(retry.receipt.record, commit.receipt.record);
        let mut old_host = OracleHost::new(key.verifying_key(), 3, 0, [0; 32]);
        let old_challenge = old_host.challenge(tile, policy, Duration::ZERO).unwrap();
        let conflicting = Request {
            id: Uuid::new_v4(),
            command: Command::Fund {
                account,
                asset: Asset::Cyan,
                units: 1,
            },
        };
        assert!(matches!(
            old_host.verify_and_commit_durable(
                signed(old_challenge, &key),
                tile,
                &mut store,
                conflicting
            ),
            Err(CommitError::Durable(
                tide_ledger::durable::Error::EvidenceRejected
            ))
        ));
        assert_eq!(store.snapshot().unwrap().balance(Asset::Cyan, account), 100);
        let mut bad = stored[0].clone();
        let mut evidence: DurableEvidence = serde_json::from_slice(&bad.context.evidence).unwrap();
        evidence.canary.digest = [0; 32];
        bad.context.evidence = serde_json::to_vec(&evidence).unwrap();
        assert!(restarted.audit_durable_record(&bad).is_err());
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn stale_and_visual_evidence_never_enter_durable_journal() {
        use tide_ledger::{
            durable::DurableLedger,
            v2::{Asset, Command, Request},
        };
        let path = std::env::temp_dir().join(format!("oracle-stale-{}.sqlite", Uuid::new_v4()));
        let key = SigningKey::from_bytes(&[7; 32]);
        let tile = [9; 32];
        let mut host = OracleHost::new(key.verifying_key(), 3, 0, [0; 32]);
        let mut store = DurableLedger::create(&path).unwrap();
        let request = Request {
            id: Uuid::new_v4(),
            command: Command::Fund {
                account: Uuid::new_v4(),
                asset: Asset::Cyan,
                units: 100,
            },
        };
        for visual in [false, true] {
            let policy = OraclePolicy {
                non_critical: visual,
                budget: Duration::from_millis(250),
            };
            let challenge = host.challenge(tile, policy, Duration::ZERO).unwrap();
            if !visual {
                host.pending.get_mut(&challenge.nonce).unwrap().issued =
                    Instant::now() - Duration::from_secs(1);
            }
            assert!(matches!(
                host.verify_and_commit_durable(
                    signed(challenge, &key),
                    tile,
                    &mut store,
                    request.clone()
                ),
                Err(CommitError::Anchor(Error::Stale))
            ));
            assert!(store.records().unwrap().is_empty());
        }
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn slow_sql_write_expires_at_commit_hook_and_rolls_back() {
        use tide_ledger::{
            durable::DurableLedger,
            v2::{Asset, Command, Request},
        };
        let path = std::env::temp_dir().join(format!("oracle-slow-sql-{}.sqlite", Uuid::new_v4()));
        let key = SigningKey::from_bytes(&[7; 32]);
        let tile = [9; 32];
        let mut host = OracleHost::new(key.verifying_key(), 3, 0, [0; 32]);
        let mut store = DurableLedger::create(&path).unwrap();
        // Real SQLite work after admission, before COMMIT. No production fault hook.
        let fault = rusqlite::Connection::open(&path).unwrap();
        fault.execute_batch("CREATE TRIGGER synthetic_slow_insert BEFORE INSERT ON tide_durable_batches BEGIN
            SELECT sum(x) FROM (WITH RECURSIVE count(x) AS (SELECT 0 UNION ALL SELECT x+1 FROM count WHERE x<10000000) SELECT x FROM count);
            END;").unwrap();
        let policy = OraclePolicy {
            non_critical: false,
            budget: Duration::from_millis(250),
        };
        let challenge = host.challenge(tile, policy, Duration::ZERO).unwrap();
        let request = Request {
            id: Uuid::new_v4(),
            command: Command::Fund {
                account: Uuid::new_v4(),
                asset: Asset::Cyan,
                units: 100,
            },
        };
        let started = Instant::now();
        assert!(matches!(
            host.verify_and_commit_durable(signed(challenge, &key), tile, &mut store, request),
            Err(CommitError::Anchor(Error::Stale))
        ));
        assert!(
            started.elapsed() >= Duration::from_millis(250),
            "synthetic database delay did not cross the deadline"
        );
        assert!(store.records().unwrap().is_empty());
        fault
            .execute_batch("DROP TRIGGER synthetic_slow_insert;")
            .unwrap();
        drop(fault);
        drop(store);
        let recovered = DurableLedger::open(&path).unwrap();
        assert!(recovered.records().unwrap().is_empty());
        drop(recovered);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
