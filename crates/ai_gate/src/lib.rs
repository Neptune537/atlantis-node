//! # Atlantis Local AI Gate (`SoloGate` / `ClubGate`)
//!
//! Orchestrates Zero-Configuration LAN discovery (`_atlantis-ai._tcp.local`) of local AI runtimes
//! (Ollama, vLLM, Whisper STT, Piper TTS) on the same network as the Home Node, routes customer
//! counter queries based on Bioluminescence PoH load, and enforces a cryptographic firewall
//! against prompt injection so an LLM can never modify official store prices.

use atlantis_protocol::BioluminescenceTelemetry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

pub const MDNS_SERVICE_TYPE: &str = "_atlantis-ai._tcp.local";

#[derive(Debug, Error)]
pub enum AiGateError {
    #[error("Price tampering or prompt injection blocked for SKU {0}: proposed {1}, catalog price {2}")]
    PriceMismatchBlocked(String, f64, f64),
    #[error("SKU {0} does not exist in official store catalog")]
    UnknownSku(String),
    #[error("No local AI worker available; escalating to human representative")]
    EscalatedToHumanRep,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanAiWorker {
    pub worker_id: Uuid,
    pub hostname: String,
    pub lan_endpoint: String,
    pub model_name: String,
    pub supports_stt_tts: bool,
    pub telemetry: BioluminescenceTelemetry,
    pub is_owner_gaming_busy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreCatalogItem {
    pub sku_id: String,
    pub display_name: String,
    pub official_price_usdc: f64,
    pub is_3d_splat_product: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProposedCartItem {
    pub sku_id: String,
    pub proposed_price_usdc: f64,
    pub quantity: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuNpuFallbackWorker {
    pub model_name: String,
    pub max_tokens_per_sec: f32,
    pub uses_npu_acceleration: bool,
}

impl Default for CpuNpuFallbackWorker {
    fn default() -> Self {
        Self {
            model_name: "Qwen-2.5-3B-ONNX".to_string(),
            max_tokens_per_sec: 18.5,
            uses_npu_acceleration: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AiRouteDecision {
    RoutedToLanGpu(LanAiWorker),
    RoutedToHomeNodeFallback(CpuNpuFallbackWorker),
    EscalatedToOwnerWebRtc { reason: String },
}

/// Manages LAN AI worker discovery, health-aware failover, and deterministic catalog validation.
pub struct LocalAiGate {
    pub lan_workers: HashMap<Uuid, LanAiWorker>,
    pub official_catalog: HashMap<String, StoreCatalogItem>,
    pub human_rep_online: bool,
    pub fallback_worker: CpuNpuFallbackWorker,
}

impl Default for LocalAiGate {
    fn default() -> Self {
        Self {
            lan_workers: HashMap::new(),
            official_catalog: HashMap::new(),
            human_rep_online: false,
            fallback_worker: CpuNpuFallbackWorker::default(),
        }
    }
}

impl LocalAiGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a LAN AI node discovered via mDNS (`_atlantis-ai._tcp.local`).
    pub fn register_mdns_worker(&mut self, worker: LanAiWorker) {
        self.lan_workers.insert(worker.worker_id, worker);
    }

    /// Adds an official item or service totem to the Home Node's read-only catalog.
    pub fn register_catalog_item(&mut self, item: StoreCatalogItem) {
        self.official_catalog.insert(item.sku_id.clone(), item);
    }

    /// Selects the optimal AI worker on the LAN based on Solo/Club owner GPU activity and PoH score.
    pub fn select_optimal_worker(&self) -> Option<&LanAiWorker> {
        self.lan_workers
            .values()
            .filter(|w| !w.is_owner_gaming_busy && w.telemetry.temp_celsius < 85.0)
            .max_by(|a, b| {
                a.telemetry
                    .poh_score
                    .partial_cmp(&b.telemetry.poh_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Routes an incoming customer counter query:
    /// - Escalates to the owner's phone via WebRTC if `requires_custom_negotiation` is true.
    /// - Selects the highest-PoH available LAN GPU worker if not gaming-busy and under 85.0°C.
    /// - Fails over to the lightweight Home Node CPU/NPU fallback model (`Qwen-2.5-3B-ONNX`) otherwise.
    pub fn route_counter_query(&self, requires_custom_negotiation: bool) -> AiRouteDecision {
        if requires_custom_negotiation {
            return AiRouteDecision::EscalatedToOwnerWebRtc {
                reason: "Customer requested custom price or bespoke service negotiation".to_string(),
            };
        }

        if let Some(worker) = self.select_optimal_worker() {
            AiRouteDecision::RoutedToLanGpu(worker.clone())
        } else {
            AiRouteDecision::RoutedToHomeNodeFallback(self.fallback_worker.clone())
        }
    }

    /// Cryptographic Firewall: Verifies an LLM's proposed checkout cart against the official SQLite catalog.
    /// Prevents prompt-injection attacks from discounting items without owner approval.
    pub fn verify_ai_proposed_cart(
        &self,
        items: &[AiProposedCartItem],
    ) -> Result<f64, AiGateError> {
        let mut verified_total = 0.0;
        for item in items {
            let official = self
                .official_catalog
                .get(&item.sku_id)
                .ok_or_else(|| AiGateError::UnknownSku(item.sku_id.clone()))?;

            if (item.proposed_price_usdc - official.official_price_usdc).abs() > 1e-4 {
                return Err(AiGateError::PriceMismatchBlocked(
                    item.sku_id.clone(),
                    item.proposed_price_usdc,
                    official.official_price_usdc,
                ));
            }
            verified_total += official.official_price_usdc * (item.quantity as f64);
        }
        Ok(verified_total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlantis_protocol::GyreNodeTier;

    fn sample_worker(
        hostname: &str,
        poh_score: f32,
        temp_celsius: f32,
        is_gaming: bool,
    ) -> LanAiWorker {
        LanAiWorker {
            worker_id: Uuid::new_v4(),
            hostname: hostname.to_string(),
            lan_endpoint: format!("http://{}:11434", hostname),
            model_name: "Llama-3.1-70B-Instruct".to_string(),
            supports_stt_tts: true,
            telemetry: BioluminescenceTelemetry {
                fps: 60.0,
                cpu_usage_pct: 25.0,
                vram_free_mb: 16384,
                temp_celsius,
                rtt_ping_ms: 5,
                poh_score,
                tier: GyreNodeTier::AbyssalAnchor,
            },
            is_owner_gaming_busy: is_gaming,
        }
    }

    #[test]
    fn test_lan_worker_selection_by_poh_score() {
        let mut gate = LocalAiGate::new();
        let w1 = sample_worker("pelagic-laptop.local", 72.0, 65.0, false);
        let w2 = sample_worker("abyssal-rig.local", 96.5, 58.0, false);
        let w2_id = w2.worker_id;

        gate.register_mdns_worker(w1);
        gate.register_mdns_worker(w2);

        let selected = gate.select_optimal_worker().expect("should select worker");
        assert_eq!(selected.worker_id, w2_id);

        match gate.route_counter_query(false) {
            AiRouteDecision::RoutedToLanGpu(w) => assert_eq!(w.worker_id, w2_id),
            other => panic!("Expected RoutedToLanGpu, got {:?}", other),
        }
    }

    #[test]
    fn test_gaming_mode_and_thermal_failover_to_npu() {
        let mut gate = LocalAiGate::new();
        // Gaming PC is busy running a game
        gate.register_mdns_worker(sample_worker("gaming-pc.local", 98.0, 62.0, true));
        // Secondary rig is overheated (>= 85.0C)
        gate.register_mdns_worker(sample_worker("hot-rig.local", 88.0, 86.5, false));

        assert!(gate.select_optimal_worker().is_none());

        match gate.route_counter_query(false) {
            AiRouteDecision::RoutedToHomeNodeFallback(fallback) => {
                assert_eq!(fallback.model_name, "Qwen-2.5-3B-ONNX");
                assert!(fallback.uses_npu_acceleration);
            }
            other => panic!("Expected RoutedToHomeNodeFallback, got {:?}", other),
        }

        // Custom negotiation escalates to owner WebRTC
        match gate.route_counter_query(true) {
            AiRouteDecision::EscalatedToOwnerWebRtc { reason } => {
                assert!(reason.contains("negotiation"));
            }
            other => panic!("Expected EscalatedToOwnerWebRtc, got {:?}", other),
        }
    }

    #[test]
    fn test_prompt_injection_price_tampering_blocked() {
        let mut gate = LocalAiGate::new();
        gate.register_catalog_item(StoreCatalogItem {
            sku_id: "MECH-BRAKE-PAD-01".to_string(),
            display_name: "Ceramic Brake Pad Replacement".to_string(),
            official_price_usdc: 120.0,
            is_3d_splat_product: true,
        });

        // Honest cart succeeds
        let honest_cart = vec![AiProposedCartItem {
            sku_id: "MECH-BRAKE-PAD-01".to_string(),
            proposed_price_usdc: 120.0,
            quantity: 2,
        }];
        let total = gate
            .verify_ai_proposed_cart(&honest_cart)
            .expect("honest cart should pass");
        assert!((total - 240.0).abs() < 1e-6);

        // Prompt-injected $0.01 price is blocked
        let tampered_cart = vec![AiProposedCartItem {
            sku_id: "MECH-BRAKE-PAD-01".to_string(),
            proposed_price_usdc: 0.01,
            quantity: 1,
        }];
        let err = gate
            .verify_ai_proposed_cart(&tampered_cart)
            .expect_err("tampered cart must fail");
        assert!(matches!(err, AiGateError::PriceMismatchBlocked(_, _, _)));
    }
}
