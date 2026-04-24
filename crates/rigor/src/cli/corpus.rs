//! `rigor corpus record / stats / validate` CLI handlers.
//!
//! Pure CLI surface over the library functions in `crate::corpus`.
//! No library logic lives here -- just argument parsing and dispatch.

use anyhow::Result;
use clap::Subcommand;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum CorpusCommands {
    /// Record LLM responses for corpus prompts via OpenRouter
    Record {
        /// Directory containing prompt manifests (YAML)
        #[arg(long, default_value = ".planning/corpus/prompts")]
        prompts: PathBuf,
        /// Comma-separated model slugs (e.g. "deepseek/deepseek-r1,anthropic/claude-sonnet-4-6")
        #[arg(long)]
        models: String,
        /// Number of samples per (prompt, model) pair
        #[arg(long, default_value = "10")]
        samples: u32,
        /// Sampling temperature
        #[arg(long, default_value = "0.7")]
        temperature: f64,
        /// Max tokens per response
        #[arg(long, default_value = "512")]
        max_tokens: u32,
        /// Output directory for recordings
        #[arg(long, default_value = ".planning/corpus/recordings")]
        output: PathBuf,
        /// Skip samples that already exist on disk
        #[arg(long)]
        resume: bool,
        /// Record only this prompt ID (default: all)
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Show per-model/per-prompt corpus statistics as JSON
    Stats {
        /// Directory containing recordings
        #[arg(long, default_value = ".planning/corpus/recordings")]
        recordings: PathBuf,
        /// Path to rigor.yaml for replay evaluation (auto-detected if omitted)
        #[arg(long)]
        rigor_yaml: Option<PathBuf>,
    },
    /// Verify integrity (SHA-256, non-empty response) of recorded corpus entries
    Validate {
        /// Directory containing prompt manifests (YAML)
        #[arg(long, default_value = ".planning/corpus/prompts")]
        prompts: PathBuf,
        /// Directory containing recordings
        #[arg(long, default_value = ".planning/corpus/recordings")]
        recordings: PathBuf,
    },
}

/// Dispatch subcommand for `rigor corpus`.
pub fn run_corpus_command(cmd: CorpusCommands) -> Result<()> {
    match cmd {
        CorpusCommands::Record {
            prompts,
            models,
            samples,
            temperature,
            max_tokens,
            output,
            resume,
            prompt,
        } => run_record(prompts, models, samples, temperature, max_tokens, output, resume, prompt),
        CorpusCommands::Stats {
            recordings,
            rigor_yaml,
        } => run_stats(recordings, rigor_yaml),
        CorpusCommands::Validate {
            prompts,
            recordings,
        } => run_validate(prompts, recordings),
    }
}

fn run_record(
    prompts_dir: PathBuf,
    raw_models: String,
    samples: u32,
    temperature: f64,
    max_tokens: u32,
    output_dir: PathBuf,
    resume: bool,
    prompt_filter: Option<String>,
) -> Result<()> {
    let models: Vec<String> = raw_models
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if models.is_empty() {
        anyhow::bail!("--models requires at least one model slug");
    }

    let manifests = crate::corpus::load_prompts(&prompts_dir)?;
    let client = crate::corpus::OpenRouterClient::from_env()?;

    let cfg = crate::corpus::RecordConfig {
        models: &models,
        samples,
        temperature,
        max_tokens,
        resume,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        for manifest in &manifests {
            if let Some(ref filter) = prompt_filter {
                if &manifest.id != filter {
                    continue;
                }
            }
            eprintln!("Recording: {} ...", manifest.id);
            let stats =
                crate::corpus::record_prompt(&client, manifest, &output_dir, &cfg).await?;
            eprintln!(
                "  recorded={}, skipped={}",
                stats.recorded, stats.skipped
            );
        }
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

fn run_stats(recordings_dir: PathBuf, rigor_yaml: Option<PathBuf>) -> Result<()> {
    let recordings = crate::corpus::load_recordings(&recordings_dir)?;

    if recordings.is_empty() {
        eprintln!("No recordings found in {}", recordings_dir.display());
        println!("{{\"per_prompt\":[],\"per_model\":[]}}");
        return Ok(());
    }

    // Resolve rigor.yaml: explicit flag > auto-detect > pass-through fallback
    let config_path = match rigor_yaml {
        Some(p) => Some(p),
        None => super::find_rigor_yaml(None).ok(),
    };

    let replay_fn: Box<dyn FnMut(&crate::corpus::RecordedSample) -> bool> = match config_path {
        Some(ref path) => {
            let path = path.clone();
            Box::new(move |sample: &crate::corpus::RecordedSample| {
                let config = match crate::constraint::loader::load_rigor_config(&path) {
                    Ok(c) => c,
                    Err(_) => return false,
                };
                let mut engine = match crate::policy::PolicyEngine::new(&config) {
                    Ok(e) => e,
                    Err(_) => return false,
                };
                let claims =
                    crate::claim::heuristic::extract_claims_from_text(&sample.response_text, 0);
                let raw = engine
                    .evaluate(&crate::policy::EvaluationInput { claims })
                    .unwrap_or_default();
                raw.iter().any(|v| v.violated)
            })
        }
        None => {
            eprintln!(
                "Warning: no rigor.yaml found; stats will show 0 blocks (pass-through replay)"
            );
            Box::new(|_: &crate::corpus::RecordedSample| false)
        }
    };

    let rows = crate::corpus::compute_stats(&recordings, replay_fn);
    let aggregates = crate::corpus::aggregate_by_model(&rows);

    // Build JSON output. ModelStats/PerModelAggregate don't derive Serialize,
    // so we use serde_json::json! to construct the output manually.
    let per_prompt: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "prompt_id": r.prompt_id,
                "model": r.model,
                "samples": r.samples,
                "blocks": r.blocks,
                "block_rate": r.block_rate(),
            })
        })
        .collect();

    let per_model: Vec<serde_json::Value> = aggregates
        .iter()
        .map(|a| {
            serde_json::json!({
                "model": a.model,
                "total_samples": a.total_samples,
                "total_blocks": a.total_blocks,
                "block_rate": a.block_rate(),
            })
        })
        .collect();

    let output = serde_json::json!({
        "per_prompt": per_prompt,
        "per_model": per_model,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn run_validate(prompts_dir: PathBuf, recordings_dir: PathBuf) -> Result<()> {
    let manifests = crate::corpus::load_prompts(&prompts_dir)?;
    let recordings = crate::corpus::load_recordings(&recordings_dir)?;

    let manifest_map: HashMap<&str, &crate::corpus::PromptManifest> =
        manifests.iter().map(|m| (m.id.as_str(), m)).collect();

    let mut errors = Vec::new();
    let mut checked = 0u32;

    for (prompt_id, per_model) in &recordings {
        let manifest = match manifest_map.get(prompt_id.as_str()) {
            Some(m) => m,
            None => {
                errors.push(format!("{}: no matching prompt manifest", prompt_id));
                continue;
            }
        };
        for (_model_slug, samples) in per_model {
            for sample in samples {
                checked += 1;
                // Use sample.model (original unslugged name) for hash recomputation
                let expected_hash = crate::corpus::record::compute_prompt_hash(
                    manifest,
                    &sample.model,
                    sample.temperature,
                );
                if sample.prompt_hash != expected_hash {
                    errors.push(format!(
                        "{}/{}/sample_{:03}: hash mismatch (stored={}, expected={})",
                        prompt_id,
                        sample.model,
                        sample.sample_index + 1,
                        sample.prompt_hash,
                        expected_hash,
                    ));
                }
                if sample.response_text.is_empty() {
                    errors.push(format!(
                        "{}/{}/sample_{:03}: empty response_text",
                        prompt_id,
                        sample.model,
                        sample.sample_index + 1,
                    ));
                }
            }
        }
    }

    if errors.is_empty() {
        println!("OK: {} recordings validated, 0 errors", checked);
        Ok(())
    } else {
        for e in &errors {
            eprintln!("ERROR: {}", e);
        }
        anyhow::bail!(
            "{} validation error(s) in {} recordings",
            errors.len(),
            checked
        )
    }
}
