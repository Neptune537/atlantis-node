//! # Context Budgeter & Prompt Cache Placement (`ai_gate::context_budget`)
//!
//! Clean-room Rust implementation of the script-aware conservative token estimator,
//! ephemeral message history trimming, and depth prompt caching for `ai_gate`.
//!
//! Addresses model context window overflows when revolving between different model endpoints
//! (such as Groq 8k/128k, OpenRouter DeepSeek, Cloudflare Workers AI, and Gemini Free Tier).
//!
//! Highlights:
//! - Script-aware token estimation:
//!   - Plain ASCII: ~3.5 chars / token
//!   - Alphabetic Latin-extended / Cyrillic / Greek: ~2.0 chars / token
//!   - CJK / Emoji / Special: ~1.0 char / token
//! - Tail-protective trimming: Retains system prompt and latest `protected_tail` messages intact,
//!   trimming verbose tool results or older user/assistant turns in the wire projection only.
//! - Role-run depth cache calculation: Marks prompt cache breakpoints `depth` role-runs back
//!   to maximize KV-cache reuse when conversational or situational tails update every turn.

use serde::{Deserialize, Serialize};

pub const ASCII_CHARS_PER_TOKEN: f32 = 3.5;
pub const ALPHABETIC_CHARS_PER_TOKEN: f32 = 2.0;
pub const OTHER_CHARS_PER_TOKEN: f32 = 1.0;
pub const MESSAGE_OVERHEAD_TOKENS: usize = 4;
pub const DEFAULT_PROTECTED_TAIL: usize = 6;

/// A conversational message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A structured chat message in an agent interaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub token_estimate: Option<usize>,
}

impl ChatMessage {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        let text = content.into();
        let est = estimate_text_tokens(&text) + MESSAGE_OVERHEAD_TOKENS;
        Self {
            role,
            content: text,
            token_estimate: Some(est),
        }
    }
}

/// Conservative script-aware token estimator.
pub fn estimate_text_tokens(text: &str) -> usize {
    let mut ascii = 0usize;
    let mut alphabetic = 0usize;
    let mut other = 0usize;

    for c in text.chars() {
        let code = c as u32;
        if code < 0x80 {
            ascii += 1;
        } else if code < 0x2E80 {
            alphabetic += 1;
        } else {
            other += 1;
        }
    }

    let ascii_toks = (ascii as f32 / ASCII_CHARS_PER_TOKEN).ceil() as usize;
    let alpha_toks = (alphabetic as f32 / ALPHABETIC_CHARS_PER_TOKEN).ceil() as usize;
    let other_toks = (other as f32 / OTHER_CHARS_PER_TOKEN).ceil() as usize;

    ascii_toks + alpha_toks + other_toks
}

/// Specifications for a model chamber's context window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelContextLimits {
    pub context_window_tokens: usize,
    pub max_output_tokens: usize,
    pub safety_margin_tokens: usize,
}

impl ModelContextLimits {
    pub fn new(context_window_tokens: usize, max_output_tokens: usize) -> Self {
        Self {
            context_window_tokens,
            max_output_tokens,
            safety_margin_tokens: 1024,
        }
    }

    pub fn available_input_budget(&self) -> usize {
        self.context_window_tokens
            .saturating_sub(self.max_output_tokens)
            .saturating_sub(self.safety_margin_tokens)
    }
}

/// Non-destructive context window trimmer.
/// Leaves the persistent chat journal intact, producing a wire-safe projection fitting the model limits.
pub fn project_wire_context(
    system_prompt: &str,
    history: &[ChatMessage],
    limits: &ModelContextLimits,
    protected_tail_count: usize,
) -> (String, Vec<ChatMessage>, usize) {
    let system_tokens = estimate_text_tokens(system_prompt) + MESSAGE_OVERHEAD_TOKENS;
    let budget = limits.available_input_budget();

    if system_tokens >= budget {
        // Edge case: System prompt exceeds budget on its own
        return (system_prompt.to_string(), Vec::new(), system_tokens);
    }

    let remaining_budget = budget - system_tokens;
    let protected_count = protected_tail_count.min(history.len());
    let protected_start_idx = history.len() - protected_count;

    let mut projected = Vec::new();
    let mut current_tokens = 0usize;

    // Collect protected tail messages first (from latest backwards)
    let mut tail_messages = Vec::new();
    for msg in history[protected_start_idx..].iter().rev() {
        let msg_tokens = msg.token_estimate.unwrap_or_else(|| estimate_text_tokens(&msg.content) + MESSAGE_OVERHEAD_TOKENS);
        if current_tokens + msg_tokens <= remaining_budget {
            current_tokens += msg_tokens;
            tail_messages.push(msg.clone());
        } else {
            break;
        }
    }
    tail_messages.reverse();

    // Now attempt to backfill older history before the protected tail if space permits
    let mut older_messages = Vec::new();
    for msg in history[..protected_start_idx].iter().rev() {
        let msg_tokens = msg.token_estimate.unwrap_or_else(|| estimate_text_tokens(&msg.content) + MESSAGE_OVERHEAD_TOKENS);
        if current_tokens + msg_tokens <= remaining_budget {
            current_tokens += msg_tokens;
            older_messages.push(msg.clone());
        } else {
            // Trim point reached
            break;
        }
    }
    older_messages.reverse();

    projected.extend(older_messages);
    projected.extend(tail_messages);

    (system_prompt.to_string(), projected, system_tokens + current_tokens)
}

/// Computes message index breakpoints for Anthropic-style prompt cache placement.
/// `depth` represents how many role-run turns back from the end to position the cache breakpoint.
pub fn compute_depth_cache_indices(messages: &[ChatMessage], depth: usize) -> Vec<usize> {
    if messages.is_empty() {
        return Vec::new();
    }

    let mut run_count = 0usize;
    let mut last_role = None;
    let mut breakpoints = Vec::new();

    // Scan backwards to identify role run transitions
    for (idx, msg) in messages.iter().enumerate().rev() {
        if Some(msg.role) != last_role {
            last_role = Some(msg.role);
            run_count += 1;

            if run_count == depth || run_count == depth + 2 {
                breakpoints.push(idx);
            }
        }
    }

    breakpoints.sort();
    breakpoints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_aware_token_estimation() {
        // English ASCII string
        let en = "Hello, this is a standard English instruction.";
        let toks_en = estimate_text_tokens(en);
        assert!(toks_en > 10 && toks_en < 20);

        // CJK text
        let cjk = "黒潮海流の世界へようこそ。"; // 13 chars
        let toks_cjk = estimate_text_tokens(cjk);
        assert_eq!(toks_cjk, 13); // ~1 token per char
    }

    #[test]
    fn test_context_window_projection_respects_limits() {
        let limits = ModelContextLimits {
            context_window_tokens: 4096,
            max_output_tokens: 1024,
            safety_margin_tokens: 512, // available = 2560
        };

        let system = "You are Gigpet Butler R2/C3 on Kuroshio Gyre.";
        let mut history = Vec::new();

        for i in 0..50 {
            history.push(ChatMessage::new(
                MessageRole::User,
                format!("Turn {}: Tell me about coordinate telemetry on sector ALPHA.", i),
            ));
            history.push(ChatMessage::new(
                MessageRole::Assistant,
                format!("Turn {}: All systems nominal, current radar lock valid.", i),
            ));
        }

        let (sys, wire_history, total_tokens) = project_wire_context(system, &history, &limits, 6);
        assert_eq!(sys, system);
        assert!(total_tokens <= limits.available_input_budget());
        // Verify protected tail was included
        assert!(!wire_history.is_empty());
        assert_eq!(wire_history.last().unwrap().content, history.last().unwrap().content);
    }

    #[test]
    fn test_depth_cache_index_calculation() {
        let history = vec![
            ChatMessage::new(MessageRole::User, "Msg 0"),
            ChatMessage::new(MessageRole::Assistant, "Msg 1"),
            ChatMessage::new(MessageRole::User, "Msg 2"),
            ChatMessage::new(MessageRole::Assistant, "Msg 3"),
            ChatMessage::new(MessageRole::User, "Msg 4"),
            ChatMessage::new(MessageRole::Assistant, "Msg 5"),
        ];

        let bp = compute_depth_cache_indices(&history, 2);
        assert!(!bp.is_empty());
    }
}
