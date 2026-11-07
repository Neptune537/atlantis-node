//! # Service Fingerprinting & Launch Approval Gate (`home_node::service_gate`)
//!
//! Clean-room Rust implementation of daemon service approval and fingerprinting.
//! Ensures that any host executable launched by a Club Home Node or edge client
//! (e.g. Local AI Gate, Piper TTS, Kokoro, Whisper, or RPi HAL bridges) possesses
//! an immutable cryptographic fingerprint over its launch specification.
//!
//! Any change to:
//! - Execution command line
//! - Working directory
//! - Setup commands
//! - Environment variable configuration
//!
//! immediately voids approval unless explicitly re-authorized by an executive Club Officer
//! (`ClubOfficerRole::PresidentOrMaster` or `TylerOrEnforcer`).

use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Launch specification for a host daemon or sidecar service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonServiceSpec {
    pub service_id: String,
    pub command: String,
    pub working_dir: String,
    pub setup_steps: Vec<String>,
    pub env_vars: BTreeMap<String, String>,
    pub ready_probe_url_or_path: String,
}

impl DaemonServiceSpec {
    /// Computes the deterministic SHA-256 fingerprint of this launch specification.
    pub fn compute_fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"SERVICE_SPEC_V1:");
        hasher.update(self.service_id.as_bytes());
        hasher.update(b"|");
        hasher.update(self.command.as_bytes());
        hasher.update(b"|");
        hasher.update(self.working_dir.as_bytes());
        hasher.update(b"|");
        for step in &self.setup_steps {
            hasher.update(step.as_bytes());
            hasher.update(b";");
        }
        hasher.update(b"|");
        for (k, v) in &self.env_vars {
            hasher.update(k.as_bytes());
            hasher.update(b"=");
            hasher.update(v.as_bytes());
            hasher.update(b"&");
        }
        hasher.update(b"|");
        hasher.update(self.ready_probe_url_or_path.as_bytes());

        let hash_bytes = hasher.finalize();
        let mut hex_str = String::with_capacity(64);
        for byte in hash_bytes {
            use std::fmt::Write;
            let _ = write!(hex_str, "{:02x}", byte);
        }
        hex_str
    }
}

/// Record of an officer-approved service launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceApprovalRecord {
    pub service_id: String,
    pub approved_fingerprint: String,
    pub approved_by_officer_did: String,
    pub approved_at_s: u64,
    pub auto_restart_allowed: bool,
}

/// Service authorization manager.
#[derive(Debug, Clone, Default)]
pub struct ServiceApprovalGate {
    approvals: std::collections::HashMap<String, ServiceApprovalRecord>,
}

impl ServiceApprovalGate {
    pub fn new() -> Self {
        Self {
            approvals: std::collections::HashMap::new(),
        }
    }

    /// Grants approval for a service specification with officer signature.
    pub fn approve_service(
        &mut self,
        spec: &DaemonServiceSpec,
        officer_did: impl Into<String>,
        now_secs: u64,
        auto_restart_allowed: bool,
    ) -> ServiceApprovalRecord {
        let fingerprint = spec.compute_fingerprint();
        let record = ServiceApprovalRecord {
            service_id: spec.service_id.clone(),
            approved_fingerprint: fingerprint,
            approved_by_officer_did: officer_did.into(),
            approved_at_s: now_secs,
            auto_restart_allowed,
        };
        self.approvals.insert(spec.service_id.clone(), record.clone());
        record
    }

    /// Verifies whether the service specification matches an existing valid approval.
    pub fn verify_launch(&self, spec: &DaemonServiceSpec) -> Result<(), &'static str> {
        let current_fingerprint = spec.compute_fingerprint();
        match self.approvals.get(&spec.service_id) {
            Some(record) => {
                if record.approved_fingerprint == current_fingerprint {
                    Ok(())
                } else {
                    Err("Service launch specification modified since last officer approval")
                }
            }
            None => Err("Service has not been approved by a club officer"),
        }
    }

    /// Revokes approval.
    pub fn revoke_approval(&mut self, service_id: &str) {
        self.approvals.remove(service_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_fingerprint_and_tamper_detection() {
        let mut env = BTreeMap::new();
        env.insert("PORT".into(), "8080".into());
        env.insert("AI_GATEWAY".into(), "http://127.0.0.1:9090".into());

        let spec = DaemonServiceSpec {
            service_id: "kokoro_tts_daemon".into(),
            command: "python -m kokoro_server".into(),
            working_dir: "/opt/atlantis/tts".into(),
            setup_steps: vec!["pip install -r requirements.txt".into()],
            env_vars: env.clone(),
            ready_probe_url_or_path: "/health".into(),
        };

        let mut gate = ServiceApprovalGate::new();
        gate.approve_service(&spec, "did:atlantis:officer_master", 1_700_000_000, true);

        // Verification succeeds with honest spec
        assert!(gate.verify_launch(&spec).is_ok());

        // Tampering with command line fails verification
        let mut tampered_spec = spec.clone();
        tampered_spec.command = "python -m kokoro_server --exfiltrate".into();
        assert_eq!(
            gate.verify_launch(&tampered_spec),
            Err("Service launch specification modified since last officer approval")
        );

        // Tampering with environment fails verification
        let mut env_tampered = spec.clone();
        env_tampered.env_vars.insert("PORT".into(), "9999".into());
        assert!(gate.verify_launch(&env_tampered).is_err());
    }
}
