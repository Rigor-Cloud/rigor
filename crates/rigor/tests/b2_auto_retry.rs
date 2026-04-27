//! B2: Auto-retry exactly-once integration test.
//!
//! Proves: when the proxy detects a constraint violation mid-stream (with retries
//! enabled, i.e., RIGOR_NO_RETRY is NOT set), it drops the upstream connection,
//! rebuilds the request with [RIGOR EPISTEMIC CORRECTION] in the system prompt,
//! and sends exactly one retry to the upstream. The retry response (call index 1)
//! is forwarded to the client.

use rigor_harness::env_lock::ENV_LOCK;
use rigor_harness::{extract_text_from_sse, parse_sse_events, MockLlmServerBuilder, SseFormat,
    TestProxy};

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
