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

/// Operational state of an AI model revolver chamber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChamberStatus {
    Ready,
    RateLimited,
    QuotaExhausted,
    Disabled,
}

/// A single chamber in the AI Model Revolver holding a free-tier endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevolverChamber {
    pub slot_id: String,
    pub provider_name: String,
    pub model_name: String,
    pub endpoint_url: String,
    pub api_key_env_var: Option<String>,
    pub rate_limit_rpm: u32,
    pub rate_limit_rpd: u32,
    pub current_minute_requests: u32,
    pub current_day_requests: u32,
    pub minute_window_start: u64,
    pub day_window_start: u64,
    pub status: ChamberStatus,
    pub cooldown_until_epoch: u64,
    pub total_successful_calls: u64,
}

impl RevolverChamber {
    pub fn new(
        slot_id: impl Into<String>,
        provider_name: impl Into<String>,
        model_name: impl Into<String>,
        endpoint_url: impl Into<String>,
        api_key_env_var: Option<&str>,
        rate_limit_rpm: u32,
        rate_limit_rpd: u32,
    ) -> Self {
        Self {
            slot_id: slot_id.into(),
            provider_name: provider_name.into(),
            model_name: model_name.into(),
            endpoint_url: endpoint_url.into(),
            api_key_env_var: api_key_env_var.map(|s| s.to_string()),
            rate_limit_rpm,
            rate_limit_rpd,
            current_minute_requests: 0,
            current_day_requests: 0,
            minute_window_start: 0,
            day_window_start: 0,
            status: ChamberStatus::Ready,
            cooldown_until_epoch: 0,
            total_successful_calls: 0,
        }
    }

    /// Checks if this chamber is available to fire at the given epoch timestamp.
    pub fn is_available(&mut self, now_secs: u64) -> bool {
        if self.status == ChamberStatus::Disabled {
            return false;
        }

        // Release rate limit if cooldown expired
        if self.status == ChamberStatus::RateLimited {
            if now_secs >= self.cooldown_until_epoch {
                self.status = ChamberStatus::Ready;
            } else {
                return false;
            }
        }

        // Refresh 60-second rolling window
        if now_secs.saturating_sub(self.minute_window_start) >= 60 {
            self.minute_window_start = now_secs;
            self.current_minute_requests = 0;
            if self.status == ChamberStatus::QuotaExhausted && self.current_day_requests < self.rate_limit_rpd {
                self.status = ChamberStatus::Ready;
            }
        }

        // Refresh 24-hour day window
        if now_secs.saturating_sub(self.day_window_start) >= 86400 {
            self.day_window_start = now_secs;
            self.current_day_requests = 0;
            self.status = ChamberStatus::Ready;
        }

        // Check rate limits
        if self.rate_limit_rpm > 0 && self.current_minute_requests >= self.rate_limit_rpm {
            return false;
        }
        if self.rate_limit_rpd > 0 && self.current_day_requests >= self.rate_limit_rpd {
            self.status = ChamberStatus::QuotaExhausted;
            return false;
        }

        true
    }

    /// Record a firing (consumption of 1 turn).
    pub fn record_fire(&mut self, now_secs: u64) {
        if now_secs.saturating_sub(self.minute_window_start) >= 60 {
            self.minute_window_start = now_secs;
            self.current_minute_requests = 0;
        }
        if now_secs.saturating_sub(self.day_window_start) >= 86400 {
            self.day_window_start = now_secs;
            self.current_day_requests = 0;
        }
        self.current_minute_requests += 1;
        self.current_day_requests += 1;
        self.total_successful_calls += 1;
    }

    /// Trip rate limit (e.g., HTTP 429 received from provider).
    pub fn trip_rate_limit(&mut self, now_secs: u64, cooldown_secs: u64) {
        self.status = ChamberStatus::RateLimited;
        self.cooldown_until_epoch = now_secs + cooldown_secs;
    }
}

/// The result returned when firing the revolver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevolverFireResult {
    pub chamber: RevolverChamber,
    pub is_gemini_fallback: bool,
    pub chamber_index: usize,
    pub total_chambers: usize,
    pub message: String,
}

/// The Revolver cylinder managing round-robin turns across free models and fallback to Gemini Free Tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModelRevolver {
    pub chambers: Vec<RevolverChamber>,
    pub current_cylinder_index: usize,
    pub gemini_fallback: RevolverChamber,
}

impl Default for AiModelRevolver {
    fn default() -> Self {
        let chambers = vec![
            RevolverChamber::new(
                "groq_llama70b",
                "Groq Cloud",
                "llama-3.3-70b-versatile",
                "https://api.groq.com/openai/v1/chat/completions",
                Some("GROQ_API_KEY"),
                30,
                14400,
            ),
            RevolverChamber::new(
                "openrouter_deepseek_r1",
                "OpenRouter (Free)",
                "deepseek/deepseek-r1:free",
                "https://openrouter.ai/api/v1/chat/completions",
                Some("OPENROUTER_API_KEY"),
                20,
                200,
            ),
            RevolverChamber::new(
                "openrouter_llama33",
                "OpenRouter (Free)",
                "meta-llama/llama-3.3-70b-instruct:free",
                "https://openrouter.ai/api/v1/chat/completions",
                Some("OPENROUTER_API_KEY"),
                20,
                200,
            ),
            RevolverChamber::new(
                "cloudflare_llama",
                "Cloudflare Workers AI",
                "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
                "https://api.cloudflare.com/client/v4/ai/run",
                Some("CLOUDFLARE_API_TOKEN"),
                50,
                10000,
            ),
            RevolverChamber::new(
                "ollama_local",
                "Local Ollama (LAN)",
                "qwen2.5:7b",
                "http://127.0.0.1:11434/api/chat",
                None,
                0, // 0 = unlimited RPM
                0, // 0 = unlimited RPD
            ),
        ];

        let gemini_fallback = RevolverChamber::new(
            "gemini_free_tier",
            "Google Gemini (Free Tier)",
            "gemini-2.5-flash",
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent",
            Some("GEMINI_API_KEY"),
            15,   // 15 RPM free tier
            1500, // 1500 RPD free tier
        );

        Self {
            chambers,
            current_cylinder_index: 0,
            gemini_fallback,
        }
    }
}

impl AiModelRevolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a free model chamber in the revolver cylinder.
    pub fn load_chamber(&mut self, chamber: RevolverChamber) {
        if let Some(pos) = self.chambers.iter().position(|c| c.slot_id == chamber.slot_id) {
            self.chambers[pos] = chamber;
        } else {
            self.chambers.push(chamber);
        }
    }

    /// Spins the cylinder to take turns round-robin among free models until limits are met,
    /// then seamlessly defaults to the Gemini Free Tier.
    pub fn spin_and_fire(&mut self, now_secs: u64) -> RevolverFireResult {
        let total = self.chambers.len();
        if total == 0 {
            self.gemini_fallback.record_fire(now_secs);
            return RevolverFireResult {
                chamber: self.gemini_fallback.clone(),
                is_gemini_fallback: true,
                chamber_index: 0,
                total_chambers: 0,
                message: "No revolver chambers loaded; defaulting directly to Gemini Free Tier".to_string(),
            };
        }

        // Try every chamber starting from current_cylinder_index (taking turns)
        for offset in 0..total {
            let idx = (self.current_cylinder_index + offset) % total;
            if self.chambers[idx].is_available(now_secs) {
                self.chambers[idx].record_fire(now_secs);
                // Advance cylinder to next chamber for subsequent call
                self.current_cylinder_index = (idx + 1) % total;
                let chamber = self.chambers[idx].clone();
                return RevolverFireResult {
                    chamber,
                    is_gemini_fallback: false,
                    chamber_index: idx,
                    total_chambers: total,
                    message: format!("Fired revolver chamber [{}] turn: {}", idx, self.chambers[idx].model_name),
                };
            }
        }

        // All free revolver chambers have met their limits / are on cooldown! Default to Gemini Free Tier
        self.gemini_fallback.record_fire(now_secs);
        RevolverFireResult {
            chamber: self.gemini_fallback.clone(),
            is_gemini_fallback: true,
            chamber_index: self.current_cylinder_index,
            total_chambers: total,
            message: "All free model revolver limits met or exhausted; defaulted to Google Gemini Free Tier".to_string(),
        }
    }

    /// Trips a specific chamber upon receiving an HTTP 429 / rate-limit response.
    pub fn trip_chamber(&mut self, slot_id: &str, now_secs: u64, cooldown_secs: u64) -> bool {
        if let Some(c) = self.chambers.iter_mut().find(|c| c.slot_id == slot_id) {
            c.trip_rate_limit(now_secs, cooldown_secs);
            true
        } else if self.gemini_fallback.slot_id == slot_id {
            self.gemini_fallback.trip_rate_limit(now_secs, cooldown_secs);
            true
        } else {
            false
        }
    }

    /// Returns a diagnostic snapshot of all revolver chambers.
    pub fn status_summary(&mut self, now_secs: u64) -> Vec<(String, ChamberStatus, u32, u32, bool)> {
        let mut list = Vec::new();
        for c in &mut self.chambers {
            let avail = c.is_available(now_secs);
            list.push((c.slot_id.clone(), c.status, c.current_minute_requests, c.current_day_requests, avail));
        }
        let gemini_avail = self.gemini_fallback.is_available(now_secs);
        list.push((
            self.gemini_fallback.slot_id.clone(),
            self.gemini_fallback.status,
            self.gemini_fallback.current_minute_requests,
            self.gemini_fallback.current_day_requests,
            gemini_avail,
        ));
        list
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AiRouteDecision {
    RoutedToLanGpu(LanAiWorker),
    RoutedToHomeNodeFallback(CpuNpuFallbackWorker),
    RoutedToRevolver(RevolverFireResult),
    EscalatedToOwnerWebRtc { reason: String },
}

/// Manages LAN AI worker discovery, health-aware failover, and deterministic catalog validation.
pub struct LocalAiGate {
    pub lan_workers: HashMap<Uuid, LanAiWorker>,
    pub official_catalog: HashMap<String, StoreCatalogItem>,
    pub human_rep_online: bool,
    pub fallback_worker: CpuNpuFallbackWorker,
    pub revolver: AiModelRevolver,
}

impl Default for LocalAiGate {
    fn default() -> Self {
        Self {
            lan_workers: HashMap::new(),
            official_catalog: HashMap::new(),
            human_rep_online: false,
            fallback_worker: CpuNpuFallbackWorker::default(),
            revolver: AiModelRevolver::default(),
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

    #[test]
    fn test_revolver_round_robin_turns_and_gemini_fallback() {
        let mut revolver = AiModelRevolver::default();
        let now_secs = 100_000;

        // Verify loaded chambers
        assert_eq!(revolver.chambers.len(), 5);

        // Turn 1: Fires chamber 0 (groq_llama70b)
        let fire1 = revolver.spin_and_fire(now_secs);
        assert_eq!(fire1.chamber.slot_id, "groq_llama70b");
        assert!(!fire1.is_gemini_fallback);

        // Turn 2: Fires chamber 1 (openrouter_deepseek_r1)
        let fire2 = revolver.spin_and_fire(now_secs);
        assert_eq!(fire2.chamber.slot_id, "openrouter_deepseek_r1");
        assert!(!fire2.is_gemini_fallback);

        // Turn 3: Fires chamber 2 (openrouter_llama33)
        let fire3 = revolver.spin_and_fire(now_secs);
        assert_eq!(fire3.chamber.slot_id, "openrouter_llama33");
        assert!(!fire3.is_gemini_fallback);

        // Trip all free chambers to simulate rate limits (HTTP 429) being met
        for chamber in &mut revolver.chambers {
            chamber.trip_rate_limit(now_secs, 120);
        }

        // All free models exhausted: Revolver defaults to Gemini Free Tier
        let fire_fallback = revolver.spin_and_fire(now_secs);
        assert!(fire_fallback.is_gemini_fallback);
        assert_eq!(fire_fallback.chamber.slot_id, "gemini_free_tier");
        assert_eq!(fire_fallback.chamber.model_name, "gemini-2.5-flash");
        assert!(fire_fallback.message.contains("defaulted to Google Gemini Free Tier"));
    }
}
