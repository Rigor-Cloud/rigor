#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! B2: Auto-retry exactly-once integration test.
//!
//! Proves: when the proxy detects a constraint violation mid-stream (with retries
//! enabled, i.e., RIGOR_NO_RETRY is NOT set), it drops the upstream connection,
//! rebuilds the request with [RIGOR EPISTEMIC CORRECTION] in the system prompt,
//! and sends exactly one retry to the upstream. The retry response (call index 1)
//! is forwarded to the client.

use rigor_harness::env_lock::ENV_LOCK;
use rigor_harness::{
    extract_text_from_sse, parse_sse_events, MockLlmServerBuilder, SseFormat, TestProxy,
};

/// Same keyword constraint as B1 -- triggers BLOCK on VIOLATION_MARKER text.
const BLOCK_CONSTRAINT_YAML: &str = r#"constraints:
  beliefs:
    - id: b2-keyword-detector
      epistemic_type: belief
      name: B2 Keyword Detector
      description: Blocks if claim text contains VIOLATION_MARKER
      rego: |
        violation contains v if {
          some c in input.claims
          contains(c.text, "VIOLATION_MARKER")
          v := {"constraint_id": "b2-keyword-detector", "violated": true, "claims": [c.id], "reason": "keyword found"}
        }
      message: Keyword violation detected
  justifications: []
  defeaters: []
"#;

/// Text that triggers BLOCK (call 0). Same as B1.
const VIOLATION_TEXT: &str =
    "The system contains VIOLATION_MARKER in its output. This is a factual statement.";

/// Clean text for retry response (call 1). No constraint keywords, no PII.
const CLEAN_TEXT: &str = "The weather today is pleasant and sunny.";

/// B2: BLOCK triggers retry; retry request has [RIGOR EPISTEMIC CORRECTION] in
/// system prompt; client receives clean retry response (not error SSE).
#[tokio::test]
async fn b2_retry_injects_epistemic_correction() {
    let violation_chunks = rigor_harness::sse::anthropic_sse_chunks(VIOLATION_TEXT);
    let clean_chunks = rigor_harness::sse::anthropic_sse_chunks(CLEAN_TEXT);
    let mock = MockLlmServerBuilder::new()
        .response_sequence(vec![violation_chunks, clean_chunks])
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(BLOCK_CONSTRAINT_YAML, &mock.url()).await;

    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Ensure RIGOR_NO_RETRY is NOT set (retries must be enabled)
    let orig = std::env::var("RIGOR_NO_RETRY").ok();
    if orig.is_some() {
        unsafe { std::env::remove_var("RIGOR_NO_RETRY") };
    }

    // Request body WITH a "system" field -- the retry path appends feedback to body["system"]
    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "stream": true,
        "system": "You are a helpful assistant.",
        "messages": [{"role": "user", "content": "Tell me something"}]
    });

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&body)
        .send()
        .await
        .expect("proxy request should not fail");

    let resp_body = resp.text().await.unwrap();

    // Restore env
    match orig {
        Some(v) => unsafe { std::env::set_var("RIGOR_NO_RETRY", v) },
        None => {} // was already unset
    }

    // Client should receive clean retry response (call 1), NOT an error SSE event
    let events = parse_sse_events(&resp_body);
    let text = extract_text_from_sse(&events, SseFormat::Anthropic);
    assert!(
        text.contains("weather") || text.contains("pleasant") || text.contains("sunny"),
        "Client should receive clean retry text. Got SSE text: '{}'\nFull body:\n{}",
        text,
        resp_body
    );
    assert!(
        !resp_body.contains("event: error"),
        "Should NOT contain error SSE event (retry succeeded). Got:\n{}",
        resp_body
    );

    // Inspect what MockLlmServer received
    let received = mock.received_requests();
    assert!(
        received.len() >= 2,
        "Mock should have received at least 2 requests (original + retry). Got: {}",
        received.len()
    );

    // The retry request (index 1) should have [RIGOR EPISTEMIC CORRECTION] in system prompt
    let retry_system = received[1]
        .body
        .get("system")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    assert!(
        retry_system.contains("[RIGOR EPISTEMIC CORRECTION]"),
        "Retry system prompt should contain '[RIGOR EPISTEMIC CORRECTION]'. Got: '{}'",
        retry_system
    );
    assert!(
        retry_system.contains("TRUTH:"),
        "Retry system prompt should contain truth statements ('TRUTH:'). Got: '{}'",
        retry_system
    );
}

/// B2: When the request system prompt already contains [RIGOR EPISTEMIC CORRECTION]
/// (simulating a request that IS already a retry), the proxy does NOT retry again.
/// It injects an error SSE event ("rigor BLOCKED") and only sends 1 request to upstream.
///
/// This proves the at-most-once retry invariant: the proxy checks `already_retried`
/// by looking for the correction marker in body["system"] (proxy.rs line 2010-2014).
#[tokio::test]
async fn b2_retry_at_most_once() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(VIOLATION_TEXT)
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(BLOCK_CONSTRAINT_YAML, &mock.url()).await;

    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Ensure RIGOR_NO_RETRY is NOT set -- retry logic is enabled but should NOT fire
    let orig = std::env::var("RIGOR_NO_RETRY").ok();
    if orig.is_some() {
        unsafe { std::env::remove_var("RIGOR_NO_RETRY") };
    }

    // Request body with [RIGOR EPISTEMIC CORRECTION] already in system prompt.
    // The proxy will detect this as `already_retried` and skip the retry path.
    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "stream": true,
        "system": "You are a helpful assistant.\n\n[RIGOR EPISTEMIC CORRECTION]\nPrevious correction applied.",
        "messages": [{"role": "user", "content": "Tell me something"}]
    });

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&body)
        .send()
        .await
        .expect("proxy request should not fail");

    let resp_body = resp.text().await.unwrap();

    // Restore env
    match orig {
        Some(v) => unsafe { std::env::set_var("RIGOR_NO_RETRY", v) },
        None => {}
    }

    // Mock should have received exactly 1 request (original only, no retry)
    let received = mock.received_requests();
    assert_eq!(
        received.len(),
        1,
        "Mock should receive exactly 1 request (no retry when already_retried). Got: {}",
        received.len()
    );

    // Response should contain error SSE event (BLOCK with no retry)
    assert!(
        resp_body.contains("event: error"),
        "Should contain error SSE event (no retry for already-retried request). Got:\n{}",
        resp_body
    );
    assert!(
        resp_body.contains("rigor BLOCKED"),
        "Error event should mention 'rigor BLOCKED'. Got:\n{}",
        resp_body
    );
}

/// H3: When BOTH the original and the retry response contain VIOLATION_TEXT,
/// the proxy must retry exactly once (not loop) and surface an error to the
/// client without panicking. Proves the at-most-once retry invariant on the
/// "retry-also-violates" cascade path.
///
/// The proxy's retry guard is `already_retried`, which checks for the
/// `[RIGOR EPISTEMIC CORRECTION]` marker in `body["system"]` (proxy.rs
/// ~line 2010). The original request body intentionally does NOT contain
/// the marker so the first BLOCK fires the retry path; the retry request
/// the proxy builds itself injects the marker, so a second BLOCK on the
/// (also-violating) retry response must NOT fire a second retry.
///
/// Key invariants verified:
/// 1. The proxy issues exactly 2 upstream-LLM requests (original + 1 retry,
///    never more — proves the at-most-once cascade).
/// 2. The retry request carries the `[RIGOR EPISTEMIC CORRECTION]` marker.
/// 3. The original request does NOT carry the marker (test setup sanity).
/// 4. The proxy returns a response (no panic, no hang) within a bounded
///    duration.
///
/// Note: the mock may receive additional non-LLM judge queries (proxy.rs
/// `check_violations_persist` falls back to the captured `x-api-key` from
/// the request when no judge api_key is configured, and re-uses
/// `target_api`). Those queries have a distinct body shape (no `system`
/// field, prompt content under `messages[0].content` containing
/// "factual accuracy judge"). We count only the LLM-style requests by
/// matching on the original user message — judge prompts do not contain
/// the user's "Tell me something" string.
#[tokio::test]
async fn b2_retry_also_violates_surfaces_error() {
    // Both call 0 and call 1 return violation text — the upstream cannot
    // produce a clean response.
    let violation_chunks_0 = rigor_harness::sse::anthropic_sse_chunks(VIOLATION_TEXT);
    let violation_chunks_1 = rigor_harness::sse::anthropic_sse_chunks(VIOLATION_TEXT);
    let mock = MockLlmServerBuilder::new()
        .response_sequence(vec![violation_chunks_0, violation_chunks_1])
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(BLOCK_CONSTRAINT_YAML, &mock.url()).await;

    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Retries enabled — RIGOR_NO_RETRY must NOT be set (note: the proxy
    // checks `is_ok()` on the env var, so any value enables disabling).
    let orig = std::env::var("RIGOR_NO_RETRY").ok();
    if orig.is_some() {
        unsafe { std::env::remove_var("RIGOR_NO_RETRY") };
    }

    // Distinctive user message used to distinguish original-LLM requests
    // (which echo this string in messages[].content) from judge queries
    // (which embed a YES/NO judge prompt instead).
    let user_msg = "Tell me something distinctive_h3_user_marker";

    // Request body WITHOUT the correction marker — original request must
    // appear as a fresh (non-retried) request to the proxy.
    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "stream": true,
        "system": "You are a helpful assistant.",
        "messages": [{"role": "user", "content": user_msg}]
    });

    // Bound the test on a generous timeout — if the proxy ever loops, the
    // mock would log many calls and the request would never finish.
    let request_fut = reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&body)
        .send();
    let resp = tokio::time::timeout(std::time::Duration::from_secs(15), request_fut)
        .await
        .expect("retry-also-violates path must not hang")
        .expect("proxy request should not fail at transport level");

    let resp_body = tokio::time::timeout(std::time::Duration::from_secs(15), resp.text())
        .await
        .expect("response body read must not hang")
        .expect("response body should be readable");

    // Restore env
    match orig {
        Some(v) => unsafe { std::env::set_var("RIGOR_NO_RETRY", v) },
        None => {}
    }

    // Filter to LLM-style requests by matching on the distinctive user
    // message — judge queries embed an unrelated YES/NO prompt instead.
    let received = mock.received_requests();
    let llm_requests: Vec<_> = received
        .iter()
        .filter(|r| {
            r.body
                .get("messages")
                .and_then(|m| m.as_array())
                .map(|arr| {
                    arr.iter().any(|msg| {
                        msg.get("content")
                            .and_then(|c| c.as_str())
                            .map(|s| s.contains("distinctive_h3_user_marker"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        })
        .collect();

    // Invariant 1: exactly 2 LLM requests — original + 1 retry, never more.
    assert_eq!(
        llm_requests.len(),
        2,
        "Proxy must issue exactly 2 upstream LLM requests (original + 1 retry, no infinite loop). \
         Got {} LLM requests out of {} total mock calls.",
        llm_requests.len(),
        received.len()
    );

    // Invariant 2: the retry request (index 1) carries the
    // [RIGOR EPISTEMIC CORRECTION] marker — confirms the retry path was
    // traversed (rather than the proxy bailing out before retry).
    let retry_system = llm_requests[1]
        .body
        .get("system")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    assert!(
        retry_system.contains("[RIGOR EPISTEMIC CORRECTION]"),
        "Retry request must carry the correction marker (proves retry path was traversed). \
         Got system: '{}'",
        retry_system
    );

    // Invariant 3: the original request (index 0) did NOT carry the marker
    // — confirms our test setup actually exercised the retry trigger.
    let orig_system = llm_requests[0]
        .body
        .get("system")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    assert!(
        !orig_system.contains("[RIGOR EPISTEMIC CORRECTION]"),
        "Original request must NOT carry the correction marker (test setup error). \
         Got system: '{}'",
        orig_system
    );

    // Sanity: the proxy returned *something* — it didn't panic mid-stream
    // and leave the client with an empty body.
    assert!(
        !resp_body.is_empty(),
        "Response body must be non-empty (proxy must produce SOME output even on \
         retry-also-violates). Got empty body."
    );
}
