//! # Two-Phase Synchronous WASM Host-Call Protocol (`atlantis_protocol::sandbox_protocol`)
//!
//! Clean-room Rust implementation of the two-phase synchronous WASM host-call protocol.
//! Replaces heavy, crash-prone Asyncify mechanisms with a deterministic yield-and-resume exchange.
//!
//! Invariant:
//! 1. The sandboxed guest execution is pure and synchronous.
//! 2. When the guest needs asynchronous host I/O (LLM completion, embedding, or allowlisted HTTP),
//!    it queues the requests on the execution context and returns `GuestExecutionOutcome::PendingHostIo`.
//! 3. Private host credentials (API keys, private location vaults, root filesystem handles) NEVER enter the WASM memory space.
//! 4. The host completes the I/O externally, verifies safety through `EgressNetworkGuard` and `PriceGuard`,
//!    and re-enters the guest with results populated.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Request from a sandboxed guest to execute model inference through the host's `ai_gate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuestLlmRequest {
    pub call_id: String,
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub max_tokens: Option<usize>,
    pub temperature: Option<f32>,
}

/// Result returned by host to guest after model generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostLlmResult {
    pub call_id: String,
    pub text: String,
    pub finish_reason: String,
    pub error: Option<String>,
}

/// Request from a sandboxed guest to perform allowlisted external network I/O.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuestNetRequest {
    pub call_id: String,
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub timeout_ms: u64,
}

/// Result returned by host to guest after outbound network request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostNetResult {
    pub call_id: String,
    pub status_code: u16,
    pub body: Vec<u8>,
    pub error: Option<String>,
}

/// Outcome of a single synchronous guest execution pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GuestExecutionOutcome {
    /// Guest finished execution and yielded its final result.
    Completed {
        status_code: u16,
        response_payload: Vec<u8>,
    },
    /// Guest yielded for asynchronous host I/O.
    PendingHostIo {
        llm_requests: Vec<GuestLlmRequest>,
        net_requests: Vec<GuestNetRequest>,
        /// Opaque serialization of guest state passed back on next re-entry.
        stash: Vec<u8>,
    },
    /// Guest encountered a fatal sandboxed error.
    Fault {
        error_message: String,
    },
}

/// Host payload provided when re-entering the guest on pass 2 or 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HostIoPassResults {
    pub llm_results: HashMap<String, HostLlmResult>,
    pub net_results: HashMap<String, HostNetResult>,
    pub stash: Vec<u8>,
}

impl HostIoPassResults {
    pub fn new(stash: Vec<u8>) -> Self {
        Self {
            llm_results: HashMap::new(),
            net_results: HashMap::new(),
            stash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_two_phase_roundtrip_data_structures() {
        let req = GuestLlmRequest {
            call_id: "order_analysis_01".into(),
            prompt: "Summarize 3D parts manifest".into(),
            system_prompt: Some("You are an assistant".into()),
            max_tokens: Some(256),
            temperature: Some(0.7),
        };

        let outcome = GuestExecutionOutcome::PendingHostIo {
            llm_requests: vec![req],
            net_requests: Vec::new(),
            stash: vec![1, 2, 3, 4],
        };

        match outcome {
            GuestExecutionOutcome::PendingHostIo { llm_requests, stash, .. } => {
                assert_eq!(llm_requests.len(), 1);
                assert_eq!(stash, vec![1, 2, 3, 4]);
            }
            _ => panic!("Expected PendingHostIo"),
        }
    }
}
