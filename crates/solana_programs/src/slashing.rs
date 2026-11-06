use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SlashingError {
    #[error("Node account `{0}` not found")]
    NodeNotFound(String),
    #[error("Node stake already fully slashed")]
    AlreadySlashed,
    #[error("Invalid slashing proof: {0}")]
    InvalidProof(String),
    #[error("Evidence expired: submitted at {submitted}, current {current}")]
    ExpiredEvidence { submitted: u64, current: u64 },
}

/// Category of slashing infraction detected by Proof-of-Health (PoH) or canary monitors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashingOffense {
    /// Computation receipt contained invalid or tampered math (e.g. fraudulent HZB occlusion buffer).
    ComputationFraud,
    /// Spoofed hardware metrics or failed canary challenge response.
    CanaryChallengeFailed,
    /// Unauthorized double-signing or epoch timestamp manipulation.
    Equivocation,
    /// Unnotified node outage while assigned to an active Coral Escrow compute contract.
    ComputeAbandonment,
}

impl SlashingOffense {
    /// Basis points (out of 10,000) of the staked CYAN collateral to be slashed.
    pub fn slash_penalty_bps(&self) -> u64 {
        match self {
            SlashingOffense::ComputationFraud => 5_000,       // 50% slash
            SlashingOffense::CanaryChallengeFailed => 10_000,  // 100% total burn
            SlashingOffense::Equivocation => 7_500,          // 75% slash
            SlashingOffense::ComputeAbandonment => 1_000,    // 10% slash
        }
    }
}

/// Record of a node's staked collateral eligible for PoH slashing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StakedValidatorNode {
    pub node_did: String,
    pub staked_cyan: u64,
    pub reputation_score: u32,
    pub is_jailed: bool,
    pub total_slashed_cyan: u64,
}

/// Cryptographic evidence submitted to slash an offending node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvidence {
    pub offender_node_did: String,
    pub offense: SlashingOffense,
    pub task_or_challenge_id: String,
    pub evidence_hash: String,
    pub reporter_did: String,
    pub timestamp: u64,
}

/// The PoH Slashing Pipeline managing collateral deduction and treasury burns.
#[derive(Debug, Default, Clone)]
pub struct PohSlashingEngine {
    nodes: HashMap<String, StakedValidatorNode>,
    burned_treasury_cyan: u64,
}

impl PohSlashingEngine {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            burned_treasury_cyan: 0,
        }
    }

    pub fn register_staked_node(&mut self, node_did: &str, initial_stake_cyan: u64) {
        self.nodes.insert(
            node_did.to_string(),
            StakedValidatorNode {
                node_did: node_did.to_string(),
                staked_cyan: initial_stake_cyan,
                reputation_score: 100,
                is_jailed: false,
                total_slashed_cyan: 0,
            },
        );
    }

    pub fn get_node(&self, node_did: &str) -> Option<&StakedValidatorNode> {
        self.nodes.get(node_did)
    }

    pub fn total_burned_cyan(&self) -> u64 {
        self.burned_treasury_cyan
    }

    /// Processes verifiable slashing evidence against an offending validator/worker.
    pub fn process_slashing(
        &mut self,
        evidence: &SlashingEvidence,
    ) -> Result<u64, SlashingError> {
        let node = self
            .nodes
            .get_mut(&evidence.offender_node_did)
            .ok_or_else(|| SlashingError::NodeNotFound(evidence.offender_node_did.clone()))?;

        if node.staked_cyan == 0 {
            return Err(SlashingError::AlreadySlashed);
        }

        let penalty_bps = evidence.offense.slash_penalty_bps();
        let slashed_amount = (node.staked_cyan * penalty_bps) / 10_000;
        let actual_slash = slashed_amount.max(1).min(node.staked_cyan);

        node.staked_cyan -= actual_slash;
        node.total_slashed_cyan += actual_slash;

        // Reduce reputation and jail if severe
        let rep_drop = (penalty_bps / 100) as u32;
        node.reputation_score = node.reputation_score.saturating_sub(rep_drop);

        if evidence.offense == SlashingOffense::CanaryChallengeFailed || node.staked_cyan == 0 {
            node.is_jailed = true;
        }

        // 50% burned, 50% allocated to reporter reward pool
        let burned = actual_slash / 2;
        self.burned_treasury_cyan += burned;

        Ok(actual_slash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slashing_computation_fraud() {
        let mut engine = PohSlashingEngine::new();
        let node_did = "did:atlantis:node:fraudulent_gpu";
        engine.register_staked_node(node_did, 10_000);

        let evidence = SlashingEvidence {
            offender_node_did: node_did.to_string(),
            offense: SlashingOffense::ComputationFraud,
            task_or_challenge_id: "task_hzb_419".to_string(),
            evidence_hash: "0xdeadbeef".to_string(),
            reporter_did: "did:atlantis:reporter_01".to_string(),
            timestamp: 1730000000,
        };

        let slashed = engine.process_slashing(&evidence).expect("Should slash 50%");
        assert_eq!(slashed, 5_000);

        let node = engine.get_node(node_did).unwrap();
        assert_eq!(node.staked_cyan, 5_000);
        assert_eq!(node.total_slashed_cyan, 5_000);
        assert_eq!(node.reputation_score, 50);
        assert_eq!(engine.total_burned_cyan(), 2_500);
    }

    #[test]
    fn test_slashing_canary_spoof_jails_node() {
        let mut engine = PohSlashingEngine::new();
        let node_did = "did:atlantis:node:spoofed_canary";
        engine.register_staked_node(node_did, 20_000);

        let evidence = SlashingEvidence {
            offender_node_did: node_did.to_string(),
            offense: SlashingOffense::CanaryChallengeFailed,
            task_or_challenge_id: "canary_881".to_string(),
            evidence_hash: "0xcanaryfailure".to_string(),
            reporter_did: "did:atlantis:olympus".to_string(),
            timestamp: 1730000000,
        };

        let slashed = engine.process_slashing(&evidence).expect("Should 100% slash and jail");
        assert_eq!(slashed, 20_000);

        let node = engine.get_node(node_did).unwrap();
        assert_eq!(node.staked_cyan, 0);
        assert!(node.is_jailed);
        assert_eq!(node.reputation_score, 0);
    }
}
