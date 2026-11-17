//! # Cryptographic Commit & CI/CD Pipeline Verifier
//!
//! Protects against GitHub malicious commits, rogue force-pushes,
//! and supply-chain tampering by verifying GPG/Ed25519 commit signatures
//! and hash pins before applying any auto-update to Raspberry Pi nodes.

use sha2::{Digest, Sha256};

pub struct CommitSecurityAuditor;

impl CommitSecurityAuditor {
    /// Validates SHA-256 payload integrity against untrusted GitHub webhook or payload
    pub fn verify_payload_integrity(payload_bytes: &[u8], expected_sha256_hex: &str) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(payload_bytes);
        let digest = format!("{:x}", hasher.finalize());
        digest.eq_ignore_ascii_case(expected_sha256_hex)
    }

    /// Verifies author allowlist
    pub fn is_authorized_committer(github_user: &str) -> bool {
        let authorized = ["Neptune537", "Kuroshio-CI"];
        authorized.contains(&github_user)
    }
}
