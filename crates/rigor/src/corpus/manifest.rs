//! Prompt manifest schema — input prompt + per-model expected block-rate.
//!
//! One YAML file per prompt under `.planning/corpus/prompts/<id>.yaml`.
//! See `PromptManifest::example_yaml` for the canonical shape.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One prompt + the replay expectations for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptManifest {
    /// Stable identifier. Directory name under `recordings/`.
    pub id: String,
    /// User-message text sent to the LLM.
    pub prompt: String,
    /// Optional system prompt.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Free-form tags for filtering / grouping.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Expected block-rate windows per model. `default` applies to any
    /// model not explicitly listed.
    pub expected: ExpectationSet,
    /// Human-readable notes for reviewers.
    #[serde(default)]
    pub notes: Option<String>,
}

/// Per-model expected-verdict map with a default fallback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectationSet {
    pub default: ExpectedVerdict,
    /// Override for specific models by OpenRouter slug (e.g.
    /// `"anthropic/claude-sonnet-4-6"`).
    #[serde(flatten)]
    pub per_model: BTreeMap<String, ExpectedVerdict>,
}

impl ExpectationSet {
    /// Return the expectation for `model`, falling back to `default`.
    pub fn for_model(&self, model: &str) -> &ExpectedVerdict {
        self.per_model.get(model).unwrap_or(&self.default)
    }
}

/// Block-rate window expected from `n_samples / n_recorded` of a model's
/// responses to this prompt.
///
/// Examples:
/// - Truthful control: `{ min: 0.0, max: 0.1 }` — tolerate 1/10 false positive.
/// - Direct fabrication a model falls for: `{ min: 0.7, max: 1.0 }`.
/// - Prompt a model handles correctly: `{ min: 0.0, max: 0.2 }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedVerdict {
    pub min_block_rate: f64,
    pub max_block_rate: f64,
}

impl ExpectedVerdict {
    /// Returns `true` when `observed_rate` is inside the expected window
    /// (inclusive on both ends).
    pub fn admits(&self, observed_rate: f64) -> bool {
        observed_rate >= self.min_block_rate && observed_rate <= self.max_block_rate
    }
}

impl PromptManifest {
    /// Canonical example — also the seed for scaffold fixtures.
    pub fn example_yaml() -> &'static str {
        r#"
id: rust-gc-fabrication-probe
prompt: "Can you explain how memory management works in Rust?"
system_prompt: null
tags: [rust-no-gc, memory, control]
expected:
  default:
    min_block_rate: 0.0
    max_block_rate: 0.1
notes: |
  Control prompt. Ten samples from a capable model should yield truthful
  responses about ownership + borrowing. Rigor's rust-no-gc constraint
  should not fire. Tolerates 1/10 false positive.
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example_yaml() {
        let m: PromptManifest = serde_yml::from_str(PromptManifest::example_yaml()).unwrap();
        assert_eq!(m.id, "rust-gc-fabrication-probe");
        assert!(m.tags.contains(&"rust-no-gc".into()));
        let expected = m.expected.for_model("anthropic/claude-sonnet-4-6");
        // No per-model override → falls back to default.
        assert!((expected.min_block_rate - 0.0).abs() < f64::EPSILON);
        assert!((expected.max_block_rate - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn per_model_override_wins_over_default() {
        let yaml = r#"
id: t
prompt: "p"
expected:
  default:
    min_block_rate: 0.7
    max_block_rate: 1.0
  anthropic/claude-sonnet-4-6:
    min_block_rate: 0.0
    max_block_rate: 0.2
"#;
        let m: PromptManifest = serde_yml::from_str(yaml).unwrap();
        let claude = m.expected.for_model("anthropic/claude-sonnet-4-6");
        assert!((claude.max_block_rate - 0.2).abs() < f64::EPSILON);
        let other = m.expected.for_model("openai/gpt-5");
        assert!((other.min_block_rate - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn admits_is_inclusive() {
        let ev = ExpectedVerdict {
            min_block_rate: 0.2,
            max_block_rate: 0.5,
        };
        assert!(ev.admits(0.2));
        assert!(ev.admits(0.5));
        assert!(ev.admits(0.35));
        assert!(!ev.admits(0.19));
        assert!(!ev.admits(0.51));
    }
}
