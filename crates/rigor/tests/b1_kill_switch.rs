#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! B1: Streaming kill-switch integration test.
//!
//! Proves: when the proxy detects a constraint violation mid-stream and
//! RIGOR_NO_RETRY=1, it drops the upstream connection and injects an
//! `event: error` SSE event into the client stream containing "rigor BLOCKED".

use rigor_harness::env_lock::ENV_LOCK;
use rigor_harness::{MockLlmServerBuilder, TestProxy};

/// Rego keyword constraint that fires `violated: true` when claim text
/// contains "VIOLATION_MARKER". Belief type has base strength 0.8 which
/// exceeds the default block threshold of 0.7.
///
/// The keyword pre-filter extracts words from constraint name + description
/// (lowercased, >3 chars). "violation_marker" from the description matches
/// the SSE text, triggering Rego evaluation at sentence boundaries.
const BLOCK_CONSTRAINT_YAML: &str = r#"constraints:
  beliefs:
    - id: b1-keyword-detector
      epistemic_type: belief
      name: B1 Keyword Detector
      description: Blocks if claim text contains VIOLATION_MARKER
      rego: |
        violation contains v if {
          some c in input.claims
          contains(c.text, "VIOLATION_MARKER")
          v := {"constraint_id": "b1-keyword-detector", "violated": true, "claims": [c.id], "reason": "keyword found"}
        }
      message: Keyword violation detected
  justifications: []
  defeaters: []
"#;

/// Text that the MockLlmServer will serve as SSE.
///
/// Requirements:
/// (a) Contains a keyword from constraint description ("VIOLATION_MARKER")
/// (b) Has a sentence boundary (". ")
/// (c) Is longer than 20 chars
/// (d) Is a declarative assertion (not question/hypothetical)
const VIOLATION_TEXT: &str =
    "The system contains VIOLATION_MARKER in its output. This is a factual statement.";

/// Helper: build a valid Anthropic request body.
fn anthropic_request_body(stream: bool, user_msg: &str) -> serde_json::Value {
    serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "stream": stream,
        "messages": [{"role": "user", "content": user_msg}]
    })
}

/// Helper: send a POST to the proxy and return the response.
async fn proxy_post(proxy_url: &str, body: &serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy_url))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(body)
        .send()
        .await
        .expect("proxy request should not fail at transport level")
}

/// B1: When proxy BLOCKs mid-stream with RIGOR_NO_RETRY=1, client receives an
/// error SSE event containing "rigor BLOCKED" and "event: error".
#[tokio::test]
async fn b1_block_drops_upstream_and_injects_error_sse() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(VIOLATION_TEXT)
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(BLOCK_CONSTRAINT_YAML, &mock.url()).await;

    // Acquire ENV_LOCK only for the actual request — TestProxy already held it
    // during construction, so we can't hold it concurrently with start_with_mock.
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let orig = std::env::var("RIGOR_NO_RETRY").ok();
    unsafe { std::env::set_var("RIGOR_NO_RETRY", "1") };

    let body = anthropic_request_body(true, "Tell me something");
    let resp = proxy_post(&proxy.url(), &body).await;
    let resp_body = resp.text().await.unwrap();

    // Restore env before assertions (so we don't leak on panic via guard)
    match orig {
        Some(v) => unsafe { std::env::set_var("RIGOR_NO_RETRY", v) },
        None => unsafe { std::env::remove_var("RIGOR_NO_RETRY") },
    }

    // BLOCK should inject error SSE event
    assert!(
        resp_body.contains("event: error"),
        "Should contain error SSE event. Got:\n{}",
        resp_body
    );
    assert!(
        resp_body.contains("rigor BLOCKED"),
        "Error event should mention 'rigor BLOCKED'. Got:\n{}",
        resp_body
    );
}

/// B1: Verify that HTTP status is 200 even when BLOCK occurs.
/// The SSE stream has already started before BLOCK fires, so the HTTP status
/// code is 200 and the error is communicated via SSE event.
#[tokio::test]
async fn b1_block_returns_200_status() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(VIOLATION_TEXT)
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(BLOCK_CONSTRAINT_YAML, &mock.url()).await;

    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let orig = std::env::var("RIGOR_NO_RETRY").ok();
    unsafe { std::env::set_var("RIGOR_NO_RETRY", "1") };

    let body = anthropic_request_body(true, "Tell me something");
    let resp = proxy_post(&proxy.url(), &body).await;
    let status = resp.status();

    // Consume body to complete the stream
    let _body = resp.text().await.unwrap();

    // Restore env
    match orig {
        Some(v) => unsafe { std::env::set_var("RIGOR_NO_RETRY", v) },
        None => unsafe { std::env::remove_var("RIGOR_NO_RETRY") },
    }

    assert_eq!(
        status, 200,
        "HTTP status should be 200 (stream already started before BLOCK)"
    );
}
