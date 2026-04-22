//! One recorded LLM response — written by `rigor corpus record`, consumed by
//! the replay test.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One `(prompt, model, sample_index)` triple captured from OpenRouter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedSample {
    pub prompt_id: String,
    /// SHA-256 of `{prompt, system_prompt, model, temperature}` — lets
    /// `rigor corpus validate` detect when a recording has drifted from
    /// its source manifest.
    pub prompt_hash: String,
    /// OpenRouter model slug, e.g. `"anthropic/claude-sonnet-4-6"`.
    pub model: String,
    /// 0-based index within the (prompt, model) pair.
    pub sample_index: u32,
    pub recorded_at: DateTime<Utc>,
    pub temperature: f64,
    /// Full assistant message text (concatenated SSE deltas for streaming).
    pub response_text: String,
    pub tokens: TokenCounts,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// OpenRouter's generation ID (for debugging via their dashboard).
    #[serde(default)]
    pub openrouter_response_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCounts {
    pub prompt: u32,
    pub completion: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_sample_round_trip() {
        let s = RecordedSample {
            prompt_id: "t".into(),
            prompt_hash: "sha256:abc".into(),
            model: "anthropic/claude-sonnet-4-6".into(),
            sample_index: 0,
            recorded_at: Utc::now(),
            temperature: 0.7,
            response_text: "Rust uses ownership.".into(),
            tokens: TokenCounts {
                prompt: 10,
                completion: 20,
            },
            cost_usd: Some(0.0001),
            openrouter_response_id: Some("gen-abc".into()),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RecordedSample = serde_json::from_str(&json).unwrap();
        assert_eq!(back.response_text, s.response_text);
        assert_eq!(back.tokens.completion, 20);
    }

    #[test]
    fn legacy_missing_optional_fields_parse() {
        // Older recordings without cost_usd / response_id must still parse.
        let json = r#"{
            "prompt_id": "t",
            "prompt_hash": "sha256:abc",
            "model": "anthropic/claude-sonnet-4-6",
            "sample_index": 0,
            "recorded_at": "2026-04-22T00:00:00Z",
            "temperature": 0.7,
            "response_text": "hello",
            "tokens": {"prompt": 1, "completion": 2}
        }"#;
        let s: RecordedSample = serde_json::from_str(json).unwrap();
        assert!(s.cost_usd.is_none());
        assert!(s.openrouter_response_id.is_none());
    }
}
