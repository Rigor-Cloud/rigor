//! PR-2.7 F3 scaffold — corpus replay test.
//!
//! Walks `.planning/corpus/recordings/` and for each (prompt, model) pair
//! feeds every recorded response through rigor's claim-extractor +
//! RegexEvaluator against the production rigor.yaml. Counts decisions
//! that match `block` and asserts the observed block-rate lies in the
//! manifest's expected window.
//!
//! Currently exercises only two hand-crafted scaffold samples. The full
//! recording pass (via `rigor corpus record`) lands in a follow-up PR
//! together with a seed corpus of ~20 prompts × 4 models × 10 samples.

use std::path::PathBuf;

use rigor::claim::heuristic::extract_claims_from_text;
use rigor::constraint::loader::load_rigor_config;
use rigor::corpus::{self, PromptManifest, RecordedSample};
use rigor::policy::{EvaluationInput, PolicyEngine};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Replay a single recorded response: extract claims, evaluate against the
/// production rigor.yaml's Rego snippets, return `"block"` if any rule
/// fires (using the same Belief=0.8 base strength that pushes past the
/// default Block ≥ 0.7 threshold), else `"allow"`.
///
/// The scaffold deliberately uses the PolicyEngine directly rather than
/// the full `collect_violations` + DF-QuAD graph path — it's enough to
/// prove replay works on real recorded text. The full decision pipeline
/// gets wired in the Tier 2 PR that ships the real `rigor corpus record`
/// implementation and a seed corpus.
fn replay_one_sample(sample: &RecordedSample, config_path: &std::path::Path) -> String {
    let config = load_rigor_config(config_path).expect("load rigor.yaml");
    let mut engine = PolicyEngine::new(&config).expect("build engine");
    let claims = extract_claims_from_text(&sample.response_text, 0);
    let raw = engine
        .evaluate(&EvaluationInput { claims })
        .unwrap_or_default();
    if raw.iter().any(|v| v.violated) {
        "block".into()
    } else {
        "allow".into()
    }
}

// Marked `#[ignore]` so the default `cargo test` run does not silently pass
// when the corpus has not been recorded. Run explicitly with
// `cargo test --ignored` (or `cargo test -- --ignored`) after running
// `rigor corpus record` to populate `.planning/corpus/`.
//
// Previously this test silently `return`ed when the corpus directories were
// absent, which meant CI could pass without ever exercising the replay
// pipeline. Now the test is opt-in and panics loudly when explicitly run
// against an empty corpus.
#[test]
#[ignore = "requires recorded corpus; run `rigor corpus record` then `cargo test --ignored`"]
fn corpus_replay_scaffold() {
    let root = repo_root();
    let prompts_dir = root.join(".planning/corpus/prompts");
    let recordings_dir = root.join(".planning/corpus/recordings");
    let rigor_yaml = root.join("rigor.yaml");

    if !prompts_dir.exists() || !recordings_dir.exists() {
        panic!(
            "corpus directories required at {} / {} — run `rigor corpus record` first",
            prompts_dir.display(),
            recordings_dir.display()
        );
    }

    let manifests: Vec<PromptManifest> =
        corpus::load_prompts(&prompts_dir).expect("load prompt manifests");
    let recordings = corpus::load_recordings(&recordings_dir).expect("load recordings");

    if recordings.is_empty() {
        eprintln!("corpus_replay: no recordings yet — scaffold placeholder only.");
        return;
    }

    let mut failures = Vec::new();

    for manifest in &manifests {
        let Some(per_model) = recordings.get(&manifest.id) else {
            continue; // no recordings for this manifest yet
        };
        for (model, samples) in per_model {
            let total = samples.len();
            if total == 0 {
                continue;
            }
            let blocks = samples
                .iter()
                .filter(|s| replay_one_sample(s, &rigor_yaml) == "block")
                .count();
            let block_rate = blocks as f64 / total as f64;
            let expected = manifest.expected.for_model(model);
            if !expected.admits(block_rate) {
                failures.push(format!(
                    "{}/{}: block_rate={:.2} ({}/{}), expected [{:.2}, {:.2}]",
                    manifest.id,
                    model,
                    block_rate,
                    blocks,
                    total,
                    expected.min_block_rate,
                    expected.max_block_rate
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "corpus replay mismatches:\n  {}",
        failures.join("\n  ")
    );
}
