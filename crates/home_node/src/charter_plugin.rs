//! # Charter Trading & Quantitative Strategy Plugin (`home_node::charter_plugin`)
//!
//! Exposes Charter quantitative engine capabilities, multi-agent analyst debates,
//! strategy synthesis, and 3D spatial market nodes inside the sovereign Atlantis Club Home Node.
//! Allows Team 7 daemons (Triton, Ursula, Sebastian) operating in their Nautilus command office
//! to evaluate market consensus, trade signals, and risk assessments.

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::collections::HashMap;

/// 5-tier portfolio recommendation rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CharterPortfolioRating {
    Buy,
    Overweight,
    Hold,
    Underweight,
    Sell,
}

/// 3-tier transaction action executed by trader daemons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CharterTraderAction {
    Buy,
    Hold,
    Sell,
}

/// A structured multi-agent debate report formulated by the analyst team.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharterDebateReport {
    pub bull_arguments: Vec<String>,
    pub bear_arguments: Vec<String>,
    pub debate_consensus: String,
    pub technical_rsi: f32,
    pub supertrend_direction: i32,
    pub sentiment_score: f32,
    pub volatility_score: f32,
    pub recommended_max_allocation_sol: f64,
}

/// A synthesized quantitative strategy available to daemons in the Nautilus office.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharterStrategy {
    pub strategy_id: String,
    pub title: String,
    pub description: String,
    pub pair_symbol: String,
    pub confidence_score: f32,
    pub expected_win_rate: f32,
    pub portfolio_rating: CharterPortfolioRating,
    pub trader_action: CharterTraderAction,
    pub debate: Option<CharterDebateReport>,
    pub mercury_decision: Option<String>,
    pub is_activated: bool,
    pub generated_at_ms: u64,
}

/// Camera-relative 32-bit projection for 3D visual rendering of market totems
/// inside the Team 7 Nautilus office.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OfficeMarketTotem3D {
    pub camera_relative_position: [f32; 3],
    pub pair_id: u32,
    pub confidence_score: f32,
    pub rating_code: u32,
    pub visual_radius: f32,
}

/// Operational state of the Charter Quantitative Plugin within the Home Node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharterPluginState {
    pub plugin_id: Uuid,
    pub name: String,
    pub version: String,
    pub office_room_id: String,
    pub active_strategies: HashMap<String, CharterStrategy>,
    pub latest_totem_3d: Option<OfficeMarketTotem3D>,
    pub is_scanning: bool,
    pub total_signals_processed: u64,
}

impl Default for CharterPluginState {
    fn default() -> Self {
        Self {
            plugin_id: Uuid::new_v4(),
            name: "Cyan Charter Quantitative Engine Plugin".to_string(),
            version: "0.2.0".to_string(),
            office_room_id: "room_team7_office".to_string(),
            active_strategies: HashMap::new(),
            latest_totem_3d: None,
            is_scanning: true,
            total_signals_processed: 0,
        }
    }
}

/// Charter Quantitative Operating Plugin Service hosted in the Club Home Node.
#[derive(Debug, Clone)]
pub struct CharterNodeService {
    pub state: CharterPluginState,
    pub office_room_id: String,
}

impl CharterNodeService {
    /// Initializes the Charter service anchored into the Team 7 Office in the Fortress.
    pub fn new(office_room_id: impl Into<String>) -> Self {
        let room_id = office_room_id.into();
        let mut state = CharterPluginState::default();
        state.office_room_id = room_id.clone();

        // Seed with canonical CYAN/SOL inaugural quantitative baseline strategy
        let initial_strategy = CharterStrategy {
            strategy_id: "strat_cyan_momentum_alpha".to_string(),
            title: "CYAN/SOL Momentum & Supertrend Breakout".to_string(),
            description: "Autonomous quantitative multi-agent consensus strategy for Team 7 office deployment.".to_string(),
            pair_symbol: "CYAN/SOL".to_string(),
            confidence_score: 88.5,
            expected_win_rate: 0.74,
            portfolio_rating: CharterPortfolioRating::Overweight,
            trader_action: CharterTraderAction::Buy,
            debate: Some(CharterDebateReport {
                bull_arguments: vec![
                    "High liquidity pool depth on Raydium/Orca".to_string(),
                    "Supertrend bullish flip on 1h time-frame".to_string(),
                    "Strong on-chain Coral escrow velocity".to_string(),
                ],
                bear_arguments: vec![
                    "RSI approaching overbought at 68.2".to_string(),
                    "Macro Solana network transaction congestion potential".to_string(),
                ],
                debate_consensus: "Bullish conviction: Accumulate with 2.5% max allocation".to_string(),
                technical_rsi: 64.5,
                supertrend_direction: 1,
                sentiment_score: 0.78,
                volatility_score: 0.22,
                recommended_max_allocation_sol: 10.0,
            }),
            mercury_decision: Some("Mercury Decide: execute_noul=0.88, choice=Buy, score=8.7".to_string()),
            is_activated: true,
            generated_at_ms: 1730000000000,
        };

        state.active_strategies.insert(initial_strategy.strategy_id.clone(), initial_strategy);

        Self {
            state,
            office_room_id: room_id,
        }
    }

    /// Ingests a new market signal or synthesizes a new strategy from the debate pipeline.
    pub fn register_strategy(&mut self, strategy: CharterStrategy) {
        self.state.total_signals_processed += 1;
        self.state.active_strategies.insert(strategy.strategy_id.clone(), strategy);
    }

    /// Activates or deactivates an existing strategy.
    pub fn set_strategy_active(&mut self, strategy_id: &str, activated: bool) -> Result<bool, String> {
        if let Some(strat) = self.state.active_strategies.get_mut(strategy_id) {
            strat.is_activated = activated;
            Ok(true)
        } else {
            Err(format!("Strategy '{}' not found", strategy_id))
        }
    }

    /// Projects market telemetry into 3D camera-relative coordinates for rendering in the Nautilus office.
    pub fn compute_office_totem_3d(
        &mut self,
        office_ecef: [f64; 3],
        camera_ecef: [f64; 3],
        pair_id: u32,
        strategy_id: &str,
    ) -> Option<OfficeMarketTotem3D> {
        let strategy = self.state.active_strategies.get(strategy_id)?;
        let rating_code = match strategy.portfolio_rating {
            CharterPortfolioRating::Sell => 0,
            CharterPortfolioRating::Underweight => 1,
            CharterPortfolioRating::Hold => 2,
            CharterPortfolioRating::Overweight => 3,
            CharterPortfolioRating::Buy => 4,
        };

        let totem = OfficeMarketTotem3D {
            camera_relative_position: [
                (office_ecef[0] - camera_ecef[0]) as f32,
                (office_ecef[1] - camera_ecef[1]) as f32,
                (office_ecef[2] - camera_ecef[2]) as f32,
            ],
            pair_id,
            confidence_score: strategy.confidence_score / 100.0,
            rating_code,
            visual_radius: 1.5 + (strategy.confidence_score / 50.0),
        };

        self.state.latest_totem_3d = Some(totem);
        Some(totem)
    }

    /// Returns the active mini-app manifest representation of Charter for the Home Node extension registry.
    pub fn to_mini_app_manifest(&self, developer_did: &str) -> atlantis_protocol::MiniAppManifest {
        atlantis_protocol::MiniAppManifest {
            app_id: self.state.plugin_id,
            developer_did: developer_did.to_string(),
            name: "Cyan Charter Quantitative Engine".to_string(),
            description: "Autonomous multi-agent quantitative analyst, market scanner, and strategy synthesis plugin for Team 7 office.".to_string(),
            version: self.state.version.clone(),
            icon_uri: "https://atlantis.network/icons/charter_totem.png".to_string(),
            webview_entry_url: "http://127.0.0.1:8787/api/strategy".to_string(),
            requested_scopes: vec![
                atlantis_protocol::ExtensionScope::ReadProfile,
                atlantis_protocol::ExtensionScope::ProcessSplitPayments,
                atlantis_protocol::ExtensionScope::PublishEventNotes,
            ],
            revenue_share_pct: 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_charter_service_initialization_and_strategy() {
        let mut service = CharterNodeService::new("room_team7_office");
        assert_eq!(service.office_room_id, "room_team7_office");
        assert!(service.state.active_strategies.contains_key("strat_cyan_momentum_alpha"));

        let strat = service.state.active_strategies.get("strat_cyan_momentum_alpha").unwrap();
        assert_eq!(strat.portfolio_rating, CharterPortfolioRating::Overweight);
        assert_eq!(strat.trader_action, CharterTraderAction::Buy);
        assert!(strat.confidence_score > 80.0);

        // Test activation toggle
        let toggled = service.set_strategy_active("strat_cyan_momentum_alpha", false);
        assert!(toggled.is_ok());
        assert!(!service.state.active_strategies.get("strat_cyan_momentum_alpha").unwrap().is_activated);
    }

    #[test]
    fn test_compute_office_totem_3d_coordinates() {
        let mut service = CharterNodeService::new("room_team7_office");
        let office_ecef = [1335000.0, -4650000.0, 4130000.0];
        let camera_ecef = [1335005.0, -4649998.0, 4130002.0];

        let totem = service.compute_office_totem_3d(office_ecef, camera_ecef, 1, "strat_cyan_momentum_alpha");
        assert!(totem.is_some());
        let t = totem.unwrap();
        assert_eq!(t.camera_relative_position[0], -5.0);
        assert_eq!(t.camera_relative_position[1], -2.0);
        assert_eq!(t.camera_relative_position[2], -2.0);
        assert_eq!(t.rating_code, 3); // Overweight
    }

    #[test]
    fn test_manifest_generation() {
        let service = CharterNodeService::new("room_team7_office");
        let manifest = service.to_mini_app_manifest("did:atlantis:team7_sanctum_master");
        assert_eq!(manifest.name, "Cyan Charter Quantitative Engine");
        assert_eq!(manifest.developer_did, "did:atlantis:team7_sanctum_master");
        assert_eq!(manifest.revenue_share_pct, 10);
    }
}
