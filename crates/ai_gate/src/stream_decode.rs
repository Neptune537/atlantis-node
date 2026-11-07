//! # Stream JSON Field Decoder (`ai_gate::stream_decode`)
//!
//! Clean-room Rust implementation of the wire-order streaming JSON field decoder.
//! Allows LLM generation producing structured JSON (e.g. `{"dialogue": "...", "action": "..."}`)
//! to stream plain unescaped text deltas directly to HUDs and TTS engines in real time
//! as chunks arrive over the wire, without waiting for the full JSON payload to complete.
//!
//! Handles:
//! - Streaming state machine (`Head`, `Json`, `PassThrough`, `Done`)
//! - JSON string escape sequences (`\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`)
//! - `\uXXXX` unicode code points and UTF-16 surrogate pairs
//! - Token boundary splits (e.g. escape sequence split across consecutive chunks)
//! - Optional literal substring replacements (e.g. tokenized delimiters back to newlines)

use std::collections::HashSet;

/// Specification configuring which JSON string field to stream and optional replacements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamDecodeSpec {
    pub target_fields: Vec<String>,
    pub replacements: Vec<(String, String)>,
}

impl StreamDecodeSpec {
    pub fn new(fields: &[&str]) -> Self {
        Self {
            target_fields: fields.iter().map(|s| s.to_string()).collect(),
            replacements: Vec::new(),
        }
    }

    pub fn with_replacement(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.replacements.push((from.into(), to.into()));
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecoderMode {
    Head,
    Json,
    PassThrough,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StringContext {
    Key,
    Value,
}

/// Zero-allocation-friendly streaming decoder for JSON string fields.
#[derive(Debug)]
pub struct JsonFieldStreamDecoder {
    spec: StreamDecodeSpec,
    mode: DecoderMode,
    current_key: String,
    string_context: Option<StringContext>,
    target_active: bool,
    fields_completed: HashSet<String>,
    escape_buffer: String,
    unicode_surrogate_high: Option<u16>,
}

impl JsonFieldStreamDecoder {
    pub fn new(spec: StreamDecodeSpec) -> Self {
        Self {
            spec,
            mode: DecoderMode::Head,
            current_key: String::new(),
            string_context: None,
            target_active: false,
            fields_completed: HashSet::new(),
            escape_buffer: String::new(),
            unicode_surrogate_high: None,
        }
    }

    /// Feeds an incoming token or wire chunk and returns any decoded text deltas for the target field.
    pub fn push_chunk(&mut self, chunk: &str) -> String {
        if chunk.is_empty() || (self.mode == DecoderMode::Done && !self.fields_completed.is_empty()) {
            return String::new();
        }

        let mut delta_out = String::new();

        for ch in chunk.chars() {
            match self.mode {
                DecoderMode::Head => {
                    if ch.is_whitespace() {
                        continue;
                    }
                    if ch == '{' {
                        self.mode = DecoderMode::Json;
                    } else {
                        // Non-JSON incoming stream: fall back to raw pass-through
                        self.mode = DecoderMode::PassThrough;
                        delta_out.push(ch);
                    }
                }
                DecoderMode::PassThrough => {
                    delta_out.push(ch);
                }
                DecoderMode::Done => break,
                DecoderMode::Json => {
                    self.consume_json_char(ch, &mut delta_out);
                }
            }
        }

        // Apply string replacements if configured
        if !self.spec.replacements.is_empty() && !delta_out.is_empty() {
            let mut replaced = delta_out;
            for (from, to) in &self.spec.replacements {
                replaced = replaced.replace(from, to);
            }
            replaced
        } else {
            delta_out
        }
    }

    fn consume_json_char(&mut self, ch: char, delta_out: &mut String) {
        // Handle in-flight string escape sequences
        if !self.escape_buffer.is_empty() {
            self.escape_buffer.push(ch);
            if self.escape_buffer.starts_with("\\u") {
                if self.escape_buffer.len() == 6 {
                    let hex_part = &self.escape_buffer[2..6];
                    if let Ok(code_point) = u16::from_str_radix(hex_part, 16) {
                        self.handle_utf16_codepoint(code_point, delta_out);
                    }
                    self.escape_buffer.clear();
                }
            } else {
                let resolved = match self.escape_buffer.as_str() {
                    "\\\"" => Some('"'),
                    "\\\\" => Some('\\'),
                    "\\/" => Some('/'),
                    "\\b" => Some('\x08'),
                    "\\f" => Some('\x0C'),
                    "\\n" => Some('\n'),
                    "\\r" => Some('\r'),
                    "\\t" => Some('\t'),
                    _ => None,
                };
                if let Some(r) = resolved {
                    if self.target_active {
                        delta_out.push(r);
                    }
                }
                self.escape_buffer.clear();
            }
            return;
        }

        // Inside a quoted string
        if let Some(ctx) = self.string_context {
            if ch == '\\' {
                self.escape_buffer.push('\\');
                return;
            }
            if ch == '"' {
                // String closing
                self.string_context = None;
                if ctx == StringContext::Value && self.target_active {
                    self.target_active = false;
                    self.fields_completed.insert(self.current_key.clone());
                    if self.fields_completed.len() >= self.spec.target_fields.len() {
                        self.mode = DecoderMode::Done;
                    }
                }
                return;
            }

            match ctx {
                StringContext::Key => {
                    self.current_key.push(ch);
                }
                StringContext::Value => {
                    if self.target_active {
                        delta_out.push(ch);
                    }
                }
            }
            return;
        }

        // Structural characters outside strings
        match ch {
            '"' => {
                // Determine whether opening a Key or a Value
                if self.current_key.is_empty() {
                    self.string_context = Some(StringContext::Key);
                } else {
                    self.string_context = Some(StringContext::Value);
                    if self.spec.target_fields.contains(&self.current_key) {
                        self.target_active = true;
                    }
                }
            }
            ':' => {
                // Key is completed, preparing for value
            }
            ',' => {
                // Reset current key for next entry
                self.current_key.clear();
                self.target_active = false;
            }
            '}' => {
                self.mode = DecoderMode::Done;
            }
            _ => {}
        }
    }

    fn handle_utf16_codepoint(&mut self, cp: u16, delta_out: &mut String) {
        // High surrogate (0xD800..=0xDBFF)
        if (0xD800..=0xDBFF).contains(&cp) {
            self.unicode_surrogate_high = Some(cp);
            return;
        }

        // Low surrogate (0xDC00..=0xDFFF)
        if (0xDC00..=0xDFFF).contains(&cp) {
            if let Some(high) = self.unicode_surrogate_high.take() {
                let code_point = 0x10000 + (((high as u32 - 0xD800) << 10) | (cp as u32 - 0xDC00));
                if let Some(c) = char::from_u32(code_point) {
                    if self.target_active {
                        delta_out.push(c);
                    }
                }
            }
            return;
        }

        // Standard BMP codepoint
        if let Some(c) = char::from_u32(cp as u32) {
            if self.target_active {
                delta_out.push(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_decode_extracts_dialogue_across_chunks() {
        let spec = StreamDecodeSpec::new(&["dialogue"]);
        let mut decoder = JsonFieldStreamDecoder::new(spec);

        let chunks = vec![
            "{\"mood\": \"excited\", ",
            "\"dialogue\": \"Hello ",
            "from Kuroshio ",
            "Gyre!\\nWelcome to the ",
            "station.\", \"priority\": 1}",
        ];

        let mut reconstructed = String::new();
        for chunk in chunks {
            let delta = decoder.push_chunk(chunk);
            reconstructed.push_str(&delta);
        }

        assert_eq!(
            reconstructed,
            "Hello from Kuroshio Gyre!\nWelcome to the station."
        );
    }

    #[test]
    fn test_stream_decode_surrogate_pair_and_escapes() {
        let spec = StreamDecodeSpec::new(&["message"]);
        let mut decoder = JsonFieldStreamDecoder::new(spec);

        // Musical note \uD83C\uDFB5 and escaped quotes
        let chunks = vec![
            "{\"message\": \"Siren 432Hz \\\"",
            "Electra\\\" \\uD83C",
            "\\uDFB5 live now\"}",
        ];

        let mut result = String::new();
        for chunk in chunks {
            result.push_str(&decoder.push_chunk(chunk));
        }

        assert_eq!(result, "Siren 432Hz \"Electra\" 🎵 live now");
    }

    #[test]
    fn test_fallback_to_pass_through_when_raw_text() {
        let spec = StreamDecodeSpec::new(&["speech"]);
        let mut decoder = JsonFieldStreamDecoder::new(spec);

        let raw = "Plain unformatted model response without JSON wrapper.";
        let res = decoder.push_chunk(raw);
        assert_eq!(res, raw);
    }
}
