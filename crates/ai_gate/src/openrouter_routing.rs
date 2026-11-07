//! # OpenRouter Advanced Provider Routing & Guardrails (`ai_gate::openrouter_routing`)
//!
//! Clean-room Rust representation of OpenRouter provider ordering, fallbacks,
//! zero data-retention (ZDR) constraints, price caps, and quantization filters.
//! Ensures `ai_gate` free and paid chambers adhere to privacy and economic invariants.

use serde::{Deserialize, Serialize};

/// Sorting criteria for OpenRouter provider routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenRouterSort {
    Price,
    Throughput,
    Latency,
}

/// Data collection preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataCollectionPreference {
    Allow,
    Deny,
}

/// Price guardrails per 1M tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OpenRouterMaxPrice {
    pub prompt_usd_per_m: f64,
    pub completion_usd_per_m: f64,
}

/// Routing constraints passed into OpenRouter requests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OpenRouterRouting {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_fallbacks: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_collection: Option<DataCollectionPreference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zdr: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<OpenRouterSort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantizations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_price: Option<OpenRouterMaxPrice>,
}

impl OpenRouterRouting {
    /// Validates internal consistency of the routing specification.
    pub fn validate(&self) -> Result<(), &'static str> {
        if let (Some(only), Some(ignore)) = (&self.only, &self.ignore) {
            for target in only {
                if ignore.contains(target) {
                    return Err("Provider cannot appear in both 'only' and 'ignore' lists");
                }
            }
        }
        if let Some(price) = &self.max_price {
            if price.prompt_usd_per_m < 0.0 || price.completion_usd_per_m < 0.0 {
                return Err("Max price must be non-negative");
            }
        }
        Ok(())
    }

    /// Creates a high-privacy zero data retention configuration suitable for Kuroshio Gyre.
    pub fn privacy_hardened() -> Self {
        Self {
            data_collection: Some(DataCollectionPreference::Deny),
            zdr: Some(true),
            allow_fallbacks: Some(true),
            sort: Some(OpenRouterSort::Latency),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openrouter_routing_validation() {
        let mut routing = OpenRouterRouting::privacy_hardened();
        assert!(routing.validate().is_ok());

        routing.only = Some(vec!["Groq".into()]);
        routing.ignore = Some(vec!["Groq".into()]);
        assert!(routing.validate().is_err());
    }
}
