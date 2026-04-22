//! Recorded-LLM corpus — statistical replay testing against real model outputs.
//!
//! See `.planning/roadmap/pr-2.7-test-coverage-plan.md` for the full design.
//!
//! Two data types live here:
//! - [`PromptManifest`] — YAML-defined input prompt + per-model expected
//!   block-rate windows. Committed under `.planning/corpus/prompts/`.
//! - [`RecordedSample`] — one LLM response recorded via `rigor corpus record`.
//!   Committed under `.planning/corpus/recordings/<prompt-id>/<model>/NNN.json`.
//!
//! Replay tests load manifests + recordings, feed each response through
//! rigor's claim extractor + evaluator, aggregate block counts per
//! (prompt, model), and assert the block rate lies in the manifest's
//! expected window. Zero network, deterministic.

pub mod client;
pub mod manifest;
pub mod record;
pub mod recording;
pub mod stats;

pub use client::{ChatClient, ChatRequest, ChatResponse, OpenRouterClient};
pub use manifest::{ExpectedVerdict, PromptManifest};
pub use record::{record_prompt, RecordConfig, RecordStats};
pub use recording::{RecordedSample, TokenCounts};
pub use stats::{aggregate_by_model, compute_stats, ModelStats, PerModelAggregate};

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

/// Load every manifest under `prompts_dir`.
pub fn load_prompts(prompts_dir: &Path) -> Result<Vec<PromptManifest>> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(prompts_dir)
        .with_context(|| format!("read prompts dir {}", prompts_dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let bytes =
            std::fs::read(&path).with_context(|| format!("read manifest {}", path.display()))?;
        let manifest: PromptManifest = serde_yml::from_slice(&bytes)
            .with_context(|| format!("parse manifest {}", path.display()))?;
        out.push(manifest);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Walk `recordings_dir/<prompt-id>/<model-slug>/NNN.json` and group into
/// `{ prompt_id: { model: [samples ordered by sample_index] } }`.
pub fn load_recordings(
    recordings_dir: &Path,
) -> Result<BTreeMap<String, BTreeMap<String, Vec<RecordedSample>>>> {
    let mut out: BTreeMap<String, BTreeMap<String, Vec<RecordedSample>>> = BTreeMap::new();

    let Ok(prompt_dirs) = std::fs::read_dir(recordings_dir) else {
        return Ok(out);
    };

    for prompt_entry in prompt_dirs.flatten() {
        let prompt_path = prompt_entry.path();
        if !prompt_path.is_dir() {
            continue;
        }

        let Ok(model_dirs) = std::fs::read_dir(&prompt_path) else {
            continue;
        };
        for model_entry in model_dirs.flatten() {
            let model_path = model_entry.path();
            if !model_path.is_dir() {
                continue;
            }

            let Ok(sample_files) = std::fs::read_dir(&model_path) else {
                continue;
            };
            for sample_entry in sample_files.flatten() {
                let sample_path = sample_entry.path();
                if sample_path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let bytes = std::fs::read(&sample_path)
                    .with_context(|| format!("read sample {}", sample_path.display()))?;
                let sample: RecordedSample = serde_json::from_slice(&bytes)
                    .with_context(|| format!("parse sample {}", sample_path.display()))?;

                out.entry(sample.prompt_id.clone())
                    .or_default()
                    .entry(sample.model.clone())
                    .or_default()
                    .push(sample);
            }
        }
    }

    // Deterministic ordering for stable test output.
    for per_prompt in out.values_mut() {
        for samples in per_prompt.values_mut() {
            samples.sort_by_key(|s| s.sample_index);
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_prompts_empty_dir_is_ok() {
        let tmp = TempDir::new().unwrap();
        let prompts = load_prompts(tmp.path()).unwrap();
        assert!(prompts.is_empty());
    }

    #[test]
    fn load_recordings_missing_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let out = load_recordings(&tmp.path().join("does-not-exist")).unwrap();
        assert!(out.is_empty());
    }
}
