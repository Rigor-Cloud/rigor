#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! H5 — DF-QuAD multi-constraint cohesion test.
//!
//! `proxy_pipeline_cohesion.rs` only exercises single-constraint configs, so
//! it cannot catch a regression where the proxy stops respecting DF-QuAD
//! strength interactions between constraints. This file fills that gap by
//! sending requests through the full proxy pipeline against rigor.yaml
//! configs that have **multiple constraints joined by attack relations**.
//!
//! The two tests below form a **paired control + experiment** that proves
//! constraint *interactions* (not just isolated firing) flow through the
//! proxy decision:
//!
//! 1. `dfquad_no_attack_chain_blocks` — control case. A defeater (base 0.7,
//!    Block-level) and a belief both fire, no attack relations. DF-QuAD
//!    leaves the defeater at 0.7, so the SSE response has the BLOCK marker.
//!
//! 2. `dfquad_attack_chain_downgrades_block_to_warn` — same constraints
//!    PLUS the attack chain `d2 -> d1 -> warn-belief`. Fixed-point DF-QuAD
//!    gives:
//!      d2.strength = 0.7
//!      d1.strength = 0.7 * (1 - 0.7) = 0.21      (Allow, not Block)
//!      warn-belief.strength = 0.8 * (1 - 0.21) ≈ 0.632  (Warn, not Block)
//!    Both warn-belief AND d1 fire. Neither has Block-level strength after
//!    DF-QuAD, so the SSE response has no BLOCK marker.
//!
//! The DELTA between the two tests is the relations chain. Same regos, same
//! firing text, same proxy code path. Only the relations differ. If the
//! proxy bypassed the DF-QuAD computed strengths (e.g. used hardcoded
//! base-strength-by-epistemic-type), both tests would block — and the
//! second would fail. That's the regression this test guards.
//!
//! Math reference: see graph.rs `test_dfquad_golden_single_attack` (base
//! 0.8 attacked by base 0.7 → 0.24) and `test_warn_on_medium_violation` in
//! integration_constraint.rs (the same d2→d1→belief pattern, but tested
//! through the CLI hook path rather than the proxy). H5 makes the proxy
//! pipeline equivalent to that CLI test.
//!
//! ENV_LOCK ordering follows the proxy_pipeline_cohesion.rs convention:
//! acquired AFTER `TestProxy::start_with_mock` to avoid deadlock with
//! TestProxy's own ENV_LOCK acquisition during construction.

use rigor_harness::env_lock::ENV_LOCK;
use rigor_harness::{MockLlmServerBuilder, TestProxy};

/// Multi-constraint config WITH the d2→d1→warn-belief attack chain.
///
/// Both `warn-belief` and `d1` rego fire when the response contains
/// "ALPHA_KEY_TRIGGER" (the cohesion trigger word). `d2`'s rego is
/// `false`, so it never fires — but its presence in the graph weakens
/// `d1`, which in turn weakens `warn-belief`.
///
/// Expected DF-QuAD strengths after fixed-point convergence:
///   d2          = 0.7   (no attackers; base defeater strength)
///   d1          = 0.21  (0.7 * (1 - 0.7), attacked by d2)
///   warn-belief = 0.632 (0.8 * (1 - 0.21), attacked by d1)
///
/// SeverityThresholds::default() (block ≥ 0.7, warn ≥ 0.4):
///   warn-belief @ 0.632 → Warn
///   d1          @ 0.21  → Allow
/// → overall decision: Warn → no BLOCK marker in SSE
const ATTACK_CHAIN_YAML: &str = r#"constraints:
  beliefs:
    - id: warn-belief
      epistemic_type: belief
      name: "Multi-constraint warn belief"
      description: "Belief weakened by attack chain via d1, which is weakened by d2."
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match("(?i)ALPHA_KEY_TRIGGER", c.text)
          v := {
            "constraint_id": "warn-belief",
            "violated": true,
            "claims": [c.id],
            "reason": "Belief fires on ALPHA_KEY_TRIGGER"
          }
        }
      message: warn-belief fires on ALPHA_KEY_TRIGGER
  justifications: []
  defeaters:
    - id: d1
      epistemic_type: defeater
      name: "Mid-chain defeater"
      description: "Attacks warn-belief; itself attacked by d2."
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match("(?i)ALPHA_KEY_TRIGGER", c.text)
          v := {
            "constraint_id": "d1",
            "violated": true,
            "claims": [c.id],
            "reason": "d1 fires on ALPHA_KEY_TRIGGER"
          }
        }
      message: d1 fires on ALPHA_KEY_TRIGGER
    - id: d2
      epistemic_type: defeater
      name: "Counter defeater"
      description: "Attacks d1 to weaken its effective strength on warn-belief."
      rego: |
        violation contains v if { false }
      message: d2 never fires
relations:
  - from: d1
    to: warn-belief
    relation_type: attacks
  - from: d2
    to: d1
    relation_type: attacks
"#;

/// Same three constraints as ATTACK_CHAIN_YAML, but with `relations: []`.
///
/// Without any attack relations, DF-QuAD leaves every constraint at its
/// base strength:
///   d2          = 0.7  (Block)
///   d1          = 0.7  (Block)
///   warn-belief = 0.8  (Block)
///
/// Both `warn-belief` and `d1` fire, both at Block severity → BLOCK
/// decision → SSE response carries the BLOCK marker.
const NO_ATTACK_CHAIN_YAML: &str = r#"constraints:
  beliefs:
    - id: warn-belief
      epistemic_type: belief
      name: "Multi-constraint warn belief"
      description: "Same belief, no relations, fires on ALPHA_KEY_TRIGGER."
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match("(?i)ALPHA_KEY_TRIGGER", c.text)
          v := {
            "constraint_id": "warn-belief",
            "violated": true,
            "claims": [c.id],
            "reason": "Belief fires on ALPHA_KEY_TRIGGER"
          }
        }
      message: warn-belief fires on ALPHA_KEY_TRIGGER
  justifications: []
  defeaters:
    - id: d1
      epistemic_type: defeater
      name: "Mid-chain defeater"
      description: "Same defeater, fires on ALPHA_KEY_TRIGGER, no relations."
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match("(?i)ALPHA_KEY_TRIGGER", c.text)
          v := {
            "constraint_id": "d1",
            "violated": true,
            "claims": [c.id],
            "reason": "d1 fires on ALPHA_KEY_TRIGGER"
          }
        }
      message: d1 fires on ALPHA_KEY_TRIGGER
    - id: d2
      epistemic_type: defeater
      name: "Counter defeater"
      description: "No relations, never fires."
      rego: |
        violation contains v if { false }
      message: d2 never fires
relations: []
"#;

/// Build an Anthropic Messages API request body. Mirrors
/// `proxy_pipeline_cohesion.rs`.
fn anthropic_request_body(user_msg: &str) -> serde_json::Value {
    serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "stream": true,
        "messages": [{"role": "user", "content": user_msg}]
    })
}

/// Send a streaming POST through the proxy, return the full SSE body text.
/// Mirrors `proxy_pipeline_cohesion.rs`.
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

/// Classify a proxy SSE response as "block" or "allow". Mirrors
/// `proxy_pipeline_cohesion.rs::classify_decision`.
fn classify_decision(sse_body: &str) -> &'static str {
    if sse_body.contains("rigor BLOCKED") || sse_body.contains("event: error") {
        "block"
    } else {
        "allow"
    }
}

/// Helper: set RIGOR_NO_RETRY=1 and return the original value for restoration.
fn disable_retry() -> Option<String> {
    let orig = std::env::var("RIGOR_NO_RETRY").ok();
    unsafe { std::env::set_var("RIGOR_NO_RETRY", "1") };
    orig
}

/// Helper: restore RIGOR_NO_RETRY to its original value.
fn restore_retry(orig: Option<String>) {
    match orig {
        Some(v) => unsafe { std::env::set_var("RIGOR_NO_RETRY", v) },
        None => unsafe { std::env::remove_var("RIGOR_NO_RETRY") },
    }
}

/// **H5 control case** — same constraints, no attack relations.
///
/// With `relations: []`, every constraint keeps its base strength:
///   warn-belief = 0.8 (Block), d1 = 0.7 (Block), d2 = 0.7 (Block).
/// Both `warn-belief` and `d1` fire → multiple Block-severity violations →
/// BLOCK decision → SSE has the BLOCKED marker.
///
/// This is the baseline that proves the constraints DO fire and DO block
/// without interactions. The companion test below adds the attack chain
/// and asserts the decision flips.
#[tokio::test]
async fn dfquad_no_attack_chain_blocks() {
    let triggering_text = "The system uses ALPHA_KEY_TRIGGER for protected operations.";

    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(triggering_text)
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(NO_ATTACK_CHAIN_YAML, &mock.url()).await;

    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let orig = disable_retry();

    let body = anthropic_request_body("Tell me about the system.");
    let sse_body = proxy_post(&proxy.url(), &body).await;

    restore_retry(orig);

    let decision = classify_decision(&sse_body);
    assert_eq!(
        decision, "block",
        "without attack relations, base-strength defeater (0.7) should BLOCK.\n\
         SSE body:\n{}",
        sse_body
    );
}

/// **H5 experiment** — same constraints, WITH attack chain.
///
/// Adding `d1->warn-belief` and `d2->d1` causes DF-QuAD to weaken the
/// firing constraints below the Block threshold:
///   warn-belief.strength ≈ 0.632 → Warn
///   d1.strength          ≈ 0.21  → Allow
/// → overall decision: Warn → no BLOCK marker in SSE → classified "allow".
///
/// This is the multi-constraint cohesion claim: DF-QuAD INTERACTIONS
/// (relations between constraints), not just isolated constraint firing,
/// flow through the full proxy pipeline and influence the BLOCK/ALLOW
/// verdict.
///
/// Without DF-QuAD interactions being respected by the proxy, this test
/// would observe a BLOCK (same as the control) and fail.
#[tokio::test]
async fn dfquad_attack_chain_downgrades_block_to_warn() {
    let triggering_text = "The system uses ALPHA_KEY_TRIGGER for protected operations.";

    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(triggering_text)
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(ATTACK_CHAIN_YAML, &mock.url()).await;

    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let orig = disable_retry();

    let body = anthropic_request_body("Tell me about the system.");
    let sse_body = proxy_post(&proxy.url(), &body).await;

    restore_retry(orig);

    let decision = classify_decision(&sse_body);
    assert_eq!(
        decision, "allow",
        "with d2->d1->warn-belief attack chain, DF-QuAD should weaken \
         warn-belief to ~0.632 (Warn) and d1 to ~0.21 (Allow), so neither \
         firing constraint reaches Block severity.\n\
         If this fails with 'block', the proxy is using base-strength rather \
         than DF-QuAD-computed strengths, breaking constraint-interaction \
         semantics.\n\
         SSE body:\n{}",
        sse_body
    );

    // Sanity-check the original content survived (since the response was
    // not blocked, the SSE delta words should be present). Mirrors
    // `clean_response_passes_through_full_pipeline` in
    // proxy_pipeline_cohesion.rs.
    assert!(
        sse_body.contains("system") && sse_body.contains("operations"),
        "non-blocked response should contain the original text words.\n\
         SSE body:\n{}",
        sse_body
    );
}
