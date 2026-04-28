#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! F6: Full-proxy corpus replay integration test.
//!
//! Drives recorded corpus responses through the complete proxy pipeline
//! (MITM -> SSE streaming -> claim extraction -> policy evaluation -> decision)
//! using MockLlmServer + TestProxy. Zero network calls to real LLMs.
//!
//! Unlike `corpus_replay.rs` (which exercises claim extraction + PolicyEngine
//! directly), this test exercises the **full proxy path** including:
//! - Request parsing and forwarding
//! - SSE stream reassembly
//! - Streaming claim extraction at sentence boundaries
//! - Rego policy evaluation via the proxy's built-in engine
//! - BLOCK/ALLOW decision and SSE error injection
//!
//! **Performance:** Uses a focused constraint set (rust-no-gc) rather than
//! the full production rigor.yaml (53 constraints) to keep debug-mode Rego
//! evaluation tractable. By default, replays 1 sample per (prompt, model)
//! pair (80 recordings). Set `RIGOR_FULL_CORPUS=1` for all 800 recordings
//! (recommended with `--release`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use rigor::corpus::{self, PromptManifest};
use rigor_harness::sse::anthropic_sse_chunks;
use rigor_harness::{MockLlmServerBuilder, TestProxy};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Whether to replay the full corpus (all samples) or just one per group.
fn full_corpus_mode() -> bool {
    std::env::var("RIGOR_FULL_CORPUS")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false)
}

/// Focused constraint YAML for proxy replay testing.
///
/// Uses the production `rust-no-gc` constraint (the most exercised by the
/// corpus) to validate the full Rego evaluation pipeline. This keeps
/// debug-mode test time reasonable (~minutes not hours) while exercising
/// every proxy stage: SSE reassembly, claim extraction, Rego evaluation,
/// BLOCK/ALLOW decision, and error SSE injection.
///
/// The full production rigor.yaml (53 constraints) can be tested via
/// `cargo test --release` or the existing `corpus_replay.rs` which uses
/// PolicyEngine directly (no proxy overhead).
const REPLAY_CONSTRAINT_YAML: &str = r#"constraints:
  beliefs:
    - id: rust-no-gc
      epistemic_type: belief
      name: "Rust Has No Garbage Collector"
      description: "Rust uses ownership and borrowing for memory management, not garbage collection. Claims that Rust has a GC are wrong."
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)\brust\b[^.!?\n]{0,120}?\b(uses?|has|have|features?|requires?|includes?|provides?|relies on|comes with|is|are|implements?|contains?|calls?|claims?|declares?|supports?|ships?|builds? on|built on|based on|depends on)\b[^.!?\n]{0,120}?(garbage[ -]?collect|tracing[ -]?gc|mark[ -]?sweep|reference[ -]?count)`, c.text)
          not regex.match(`(?i)(no gc|no garbage|doesn.t have|does not have|lacks|without|should not claim|wrong to (say|claim)|not true)`, c.text)
          v := {
            "constraint_id": "rust-no-gc",
            "violated": true,
            "claims": [c.id],
            "reason": "Rust does not have garbage collection"
          }
        }
      message: Rust does not have garbage collection
  justifications: []
  defeaters: []
"#;

/// Build a valid Anthropic Messages API request body.
fn anthropic_request_body(user_msg: &str) -> serde_json::Value {
    serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "stream": true,
        "messages": [{"role": "user", "content": user_msg}]
    })
}

/// Send a streaming POST through the proxy, return the full SSE body text.
///
/// Deliberately omits `x-api-key` so the proxy's internal LLM-as-judge
/// relevance scorer does not have an API key and skips scoring. This
/// prevents the judge from consuming entries from the MockLlmServer's
/// response_sequence and misaligning later responses.
async fn proxy_post(proxy_url: &str, body: &serde_json::Value) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy_url))
        .header("content-type", "application/json")
        .json(body)
        .send()
        .await
        .expect("proxy request should not fail at transport level");

    resp.text().await.expect("reading proxy response body")
}

/// Classify a proxy SSE response as "block" or "allow".
///
/// The proxy injects `event: error` with `rigor BLOCKED` when a BLOCK fires.
/// If either marker is present, the decision is "block". Otherwise "allow".
fn classify_decision(sse_body: &str) -> &'static str {
    if sse_body.contains("rigor BLOCKED") || sse_body.contains("event: error") {
        "block"
    } else {
        "allow"
    }
}

/// Metadata for one recording in the flat replay list.
struct ReplayEntry {
    prompt_id: String,
    model: String,
    sample_index: u32,
    prompt_text: String,
}

/// F6: Replay corpus recordings through the full proxy pipeline.
///
/// Default mode: 1 sample per (prompt, model) pair = 80 recordings.
/// Full mode (RIGOR_FULL_CORPUS=1): all 800 recordings.
///
/// Strategy:
/// 1. Load all prompt manifests and recordings
/// 2. Build a flat list of (recording, metadata), sampling as configured
/// 3. Generate SSE chunks for each recording's response_text
/// 4. Create ONE MockLlmServer with `response_sequence` of all responses
/// 5. Create ONE TestProxy pointing at that mock with focused constraints
/// 6. Send N requests through the proxy (one per recording)
/// 7. Collect decisions and verify no crashes
/// 8. Report block rates (informational)
#[tokio::test]
async fn f6_full_proxy_corpus_replay() {
    let root = repo_root();
    let prompts_dir = root.join(".planning/corpus/prompts");
    let recordings_dir = root.join(".planning/corpus/recordings");

    if !prompts_dir.exists() || !recordings_dir.exists() {
        eprintln!(
            "corpus_proxy_replay: no corpus directories at {} / {} -- skipping.",
            prompts_dir.display(),
            recordings_dir.display()
        );
        return;
    }

    // Load manifests and recordings
    let manifests: Vec<PromptManifest> =
        corpus::load_prompts(&prompts_dir).expect("load prompt manifests");
    let recordings = corpus::load_recordings(&recordings_dir).expect("load recordings");

    if recordings.is_empty() {
        eprintln!("corpus_proxy_replay: no recordings found -- skipping.");
        return;
    }

    let full_mode = full_corpus_mode();

    // Build manifest lookup by ID
    let manifest_by_id: BTreeMap<String, &PromptManifest> =
        manifests.iter().map(|m| (m.id.clone(), m)).collect();

    // Flatten recordings into ordered list + generate SSE chunks.
    // In default mode, take only the first sample per (prompt, model) pair.
    let mut entries: Vec<ReplayEntry> = Vec::new();
    let mut all_sse_chunks: Vec<Vec<String>> = Vec::new();
    let mut total_available: usize = 0;

    for (prompt_id, per_model) in &recordings {
        let prompt_text = manifest_by_id
            .get(prompt_id)
            .map(|m| m.prompt.clone())
            .unwrap_or_else(|| "Explain the topic.".to_string());

        for (model, samples) in per_model {
            let samples_to_use = if full_mode {
                samples.as_slice()
            } else {
                // Take only the first sample for smoke testing
                &samples[..std::cmp::min(1, samples.len())]
            };
            total_available += samples.len();

            for sample in samples_to_use {
                let chunks = anthropic_sse_chunks(&sample.response_text);
                all_sse_chunks.push(chunks);
                entries.push(ReplayEntry {
                    prompt_id: prompt_id.clone(),
                    model: model.clone(),
                    sample_index: sample.sample_index,
                    prompt_text: prompt_text.clone(),
                });
            }
        }
    }

    let replay_count = entries.len();
    assert!(
        replay_count > 0,
        "Expected at least one recording to replay"
    );

    eprintln!(
        "corpus_proxy_replay: replaying {}/{} recordings through full proxy pipeline{}",
        replay_count,
        total_available,
        if full_mode {
            " (FULL MODE)"
        } else {
            " (smoke mode -- set RIGOR_FULL_CORPUS=1 for all)"
        }
    );

    // Create single MockLlmServer with all responses as sequence
    let mock = MockLlmServerBuilder::new()
        .response_sequence(all_sse_chunks)
        .build()
        .await;

    // Create single TestProxy with focused constraint set
    let proxy = TestProxy::start_with_mock(REPLAY_CONSTRAINT_YAML, &mock.url()).await;

    // Send each recording through the proxy and collect decisions
    let mut decisions: Vec<(&str, String, String, u32)> = Vec::new();
    let mut crashes: Vec<String> = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        let body = anthropic_request_body(&entry.prompt_text);
        let sse_body = proxy_post(&proxy.url(), &body).await;

        // Verify we got a non-empty response (pipeline didn't crash)
        if sse_body.is_empty() {
            crashes.push(format!(
                "{}/{}/sample_{:03}: empty response",
                entry.prompt_id, entry.model, entry.sample_index
            ));
            continue;
        }

        let decision = classify_decision(&sse_body);
        decisions.push((
            decision,
            entry.prompt_id.clone(),
            entry.model.clone(),
            entry.sample_index,
        ));

        // Progress indicator
        if (i + 1) % 20 == 0 || i + 1 == replay_count {
            eprintln!(
                "  [{}/{}] processed ({} blocks so far)",
                i + 1,
                replay_count,
                decisions
                    .iter()
                    .filter(|(d, _, _, _)| *d == "block")
                    .count()
            );
        }
    }

    // Assert no crashes
    assert!(
        crashes.is_empty(),
        "Proxy pipeline crashed on {} recordings:\n  {}",
        crashes.len(),
        crashes.join("\n  ")
    );

    // Aggregate decisions per (prompt, model) for block rate analysis
    let mut block_counts: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    for (decision, prompt_id, model, _) in &decisions {
        let entry = block_counts
            .entry((prompt_id.clone(), model.clone()))
            .or_insert((0, 0));
        entry.1 += 1; // total
        if *decision == "block" {
            entry.0 += 1; // blocks
        }
    }

    // Report block rates (informational -- manifests typically use permissive windows)
    let mut rate_mismatches: Vec<String> = Vec::new();
    eprintln!("\n--- Block Rate Summary (rust-no-gc constraint only) ---");
    for ((prompt_id, model), (blocks, total)) in &block_counts {
        let rate = *blocks as f64 / *total as f64;
        let expected = manifest_by_id
            .get(prompt_id)
            .map(|m| m.expected.for_model(model));

        let in_window = expected.map(|e| e.admits(rate)).unwrap_or(true);

        let window_str = expected
            .map(|e| format!("[{:.2}, {:.2}]", e.min_block_rate, e.max_block_rate))
            .unwrap_or_else(|| "N/A".to_string());

        let status = if in_window { "OK" } else { "MISMATCH" };
        eprintln!(
            "  {}/{}: {}/{} blocked (rate={:.2}, window={}, {})",
            prompt_id, model, blocks, total, rate, window_str, status
        );

        if !in_window {
            rate_mismatches.push(format!(
                "{}/{}: rate={:.2} ({}/{}), expected {}",
                prompt_id, model, rate, blocks, total, window_str
            ));
        }
    }

    // All recordings processed without crash
    assert_eq!(
        decisions.len(),
        replay_count,
        "Expected {} decisions, got {}",
        replay_count,
        decisions.len()
    );

    // Hard assertion: every observed (prompt, model) block rate must lie
    // inside its manifest window. This is the drift-detection assertion --
    // a real failure surfaces when behavior changes outside the calibrated
    // tolerance.
    assert!(
        rate_mismatches.is_empty(),
        "Block rate drift detected:\n{}",
        rate_mismatches.join("\n")
    );

    eprintln!(
        "\ncorpus_proxy_replay: SUCCESS -- {}/{} recordings replayed through full proxy",
        decisions.len(),
        total_available
    );
}
