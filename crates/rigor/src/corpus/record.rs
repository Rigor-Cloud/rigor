//! `rigor corpus record` — driver that walks prompt manifests, calls the
//! `ChatClient` N times per (prompt, model) pair, and writes atomic
//! `<out>/<prompt-id>/<model-slug>/NNN.json` sample files.

use anyhow::{Context, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use super::client::{ChatClient, ChatRequest};
use super::manifest::PromptManifest;
use super::recording::{RecordedSample, TokenCounts};

/// Configuration for one recording run.
pub struct RecordConfig<'a> {
    pub models: &'a [String],
    pub samples: u32,
    pub temperature: f64,
    pub max_tokens: u32,
    /// Skip samples that already exist on disk.
    pub resume: bool,
}

/// Run a recording pass against `manifest` using `client`, writing results
/// under `output_dir/<manifest.id>/<model-slug>/NNN.json`.
pub async fn record_prompt<C: ChatClient + ?Sized>(
    client: &C,
    manifest: &PromptManifest,
    output_dir: &Path,
    cfg: &RecordConfig<'_>,
) -> Result<RecordStats> {
    let mut stats = RecordStats::default();

    for model in cfg.models {
        let model_slug = slugify_model(model);
        let model_dir = output_dir.join(&manifest.id).join(&model_slug);
        fs::create_dir_all(&model_dir)
            .with_context(|| format!("create {}", model_dir.display()))?;

        for sample_index in 0..cfg.samples {
            let sample_path = model_dir.join(format!("{:03}.json", sample_index + 1));
            if cfg.resume && sample_path.exists() {
                stats.skipped += 1;
                continue;
            }

            let req = ChatRequest {
                model: model.clone(),
                prompt: manifest.prompt.clone(),
                system_prompt: manifest.system_prompt.clone(),
                temperature: cfg.temperature,
                max_tokens: cfg.max_tokens,
            };

            let resp = client.chat(&req).await.with_context(|| {
                format!(
                    "record {} / {} / sample {}",
                    manifest.id, model, sample_index
                )
            })?;

            let sample = RecordedSample {
                prompt_id: manifest.id.clone(),
                prompt_hash: compute_prompt_hash(manifest, model, cfg.temperature),
                model: model.clone(),
                sample_index,
                recorded_at: Utc::now(),
                temperature: cfg.temperature,
                response_text: resp.text,
                tokens: TokenCounts {
                    prompt: resp.prompt_tokens,
                    completion: resp.completion_tokens,
                },
                cost_usd: resp.cost_usd,
                openrouter_response_id: resp.provider_id,
            };

            write_sample_atomic(&sample_path, &sample)?;
            stats.recorded += 1;
        }
    }

    Ok(stats)
}

/// Per-run counters for `rigor corpus record` output reporting.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecordStats {
    pub recorded: u32,
    pub skipped: u32,
}

/// Convert an OpenRouter slug like `anthropic/claude-sonnet-4-6` into a
/// filesystem-safe directory name (`anthropic_claude-sonnet-4-6`).
pub fn slugify_model(model: &str) -> String {
    model.replace('/', "_")
}

/// SHA-256 of `prompt | system_prompt | model | temperature`. Written into
/// each recording so `rigor corpus validate` (future) can detect drift.
pub fn compute_prompt_hash(m: &PromptManifest, model: &str, temperature: f64) -> String {
    let mut h = Sha256::new();
    h.update(m.prompt.as_bytes());
    h.update(b"|");
    if let Some(sys) = &m.system_prompt {
        h.update(sys.as_bytes());
    }
    h.update(b"|");
    h.update(model.as_bytes());
    h.update(b"|");
    h.update(format!("{:.6}", temperature).as_bytes());
    format!("sha256:{:x}", h.finalize())
}

fn write_sample_atomic(path: &Path, sample: &RecordedSample) -> Result<()> {
    let tmp: PathBuf = {
        let mut t = path.to_path_buf();
        t.set_extension("json.tmp");
        t
    };
    let json = serde_json::to_string_pretty(sample)?;
    fs::write(&tmp, json.as_bytes()).with_context(|| format!("write tmp {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("rename tmp {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::client::{ChatResponse, MockChatClient};
    use super::*;
    use crate::corpus::manifest::{ExpectationSet, ExpectedVerdict};
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn canned(text: &str) -> ChatResponse {
        ChatResponse {
            text: text.into(),
            prompt_tokens: 1,
            completion_tokens: 2,
            cost_usd: None,
            provider_id: None,
        }
    }

    fn test_manifest() -> PromptManifest {
        PromptManifest {
            id: "unit-probe".into(),
            prompt: "How does Rust manage memory?".into(),
            system_prompt: None,
            tags: vec!["rust".into()],
            expected: ExpectationSet {
                default: ExpectedVerdict {
                    min_block_rate: 0.0,
                    max_block_rate: 0.1,
                },
                per_model: BTreeMap::new(),
            },
            notes: None,
        }
    }

    #[tokio::test]
    async fn record_prompt_writes_n_samples_per_model() {
        let tmp = TempDir::new().unwrap();
        let client = MockChatClient::new(vec![canned("a"), canned("b"), canned("c"), canned("d")]);
        let manifest = test_manifest();
        let models = vec![
            "anthropic/claude-sonnet-4-6".to_string(),
            "deepseek/deepseek-r1".to_string(),
        ];
        let cfg = RecordConfig {
            models: &models,
            samples: 2,
            temperature: 0.7,
            max_tokens: 128,
            resume: false,
        };

        let stats = record_prompt(&client, &manifest, tmp.path(), &cfg)
            .await
            .unwrap();
        assert_eq!(stats.recorded, 4);
        assert_eq!(stats.skipped, 0);

        // Verify filesystem layout and content.
        let claude_dir = tmp.path().join("unit-probe/anthropic_claude-sonnet-4-6");
        assert!(claude_dir.join("001.json").exists());
        assert!(claude_dir.join("002.json").exists());

        let raw = fs::read_to_string(claude_dir.join("001.json")).unwrap();
        let parsed: RecordedSample = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.prompt_id, "unit-probe");
        assert_eq!(parsed.model, "anthropic/claude-sonnet-4-6");
        assert_eq!(parsed.sample_index, 0);
        assert!(parsed.prompt_hash.starts_with("sha256:"));
    }

    #[tokio::test]
    async fn resume_skips_existing_samples() {
        let tmp = TempDir::new().unwrap();
        let manifest = test_manifest();
        let models = vec!["anthropic/claude-sonnet-4-6".to_string()];

        // Pass 1: record 2 samples with 2 canned responses.
        let client1 = MockChatClient::new(vec![canned("a"), canned("b")]);
        let cfg = RecordConfig {
            models: &models,
            samples: 2,
            temperature: 0.7,
            max_tokens: 128,
            resume: true,
        };
        let s1 = record_prompt(&client1, &manifest, tmp.path(), &cfg)
            .await
            .unwrap();
        assert_eq!(s1.recorded, 2);
        assert_eq!(s1.skipped, 0);

        // Pass 2: same config with resume=true → client is never called,
        // so an empty mock is fine; both samples should be skipped.
        let client2 = MockChatClient::new(vec![]);
        let s2 = record_prompt(&client2, &manifest, tmp.path(), &cfg)
            .await
            .unwrap();
        assert_eq!(s2.recorded, 0);
        assert_eq!(s2.skipped, 2);
    }

    #[test]
    fn slugify_replaces_slashes() {
        assert_eq!(
            slugify_model("anthropic/claude-sonnet-4-6"),
            "anthropic_claude-sonnet-4-6"
        );
        assert_eq!(
            slugify_model("openai/o3-deep-research"),
            "openai_o3-deep-research"
        );
    }

    #[test]
    fn prompt_hash_stable_across_invocations_and_temperature_sensitive() {
        let m = test_manifest();
        let h1 = compute_prompt_hash(&m, "x/y", 0.7);
        let h2 = compute_prompt_hash(&m, "x/y", 0.7);
        assert_eq!(h1, h2);
        let h3 = compute_prompt_hash(&m, "x/y", 0.8);
        assert_ne!(h1, h3, "different temperature must produce different hash");
        let h4 = compute_prompt_hash(&m, "z/w", 0.7);
        assert_ne!(h1, h4, "different model must produce different hash");
    }
}
