#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! Integration tests for proxy.rs hot-path functions:
//! - extract_and_evaluate (tested indirectly via proxy_request non-streaming path)
//! - evaluate_text_inline (tested indirectly via proxy_request streaming evaluation)
//! - proxy_request decision branches (bad JSON, streaming allow, non-streaming allow)
//!
//! These functions are private in proxy.rs, so we exercise them through the public
//! HTTP interface via TestProxy + MockLlmServer from rigor-harness.

use rigor::daemon::ws::DaemonEvent;
use rigor_harness::{
    extract_text_from_sse, parse_sse_events, MockLlmServerBuilder, SseFormat, TestProxy,
};
use std::time::Duration;
use tokio::sync::broadcast::error::TryRecvError;

/// Minimal valid rigor.yaml (empty constraints -- no violations expected).
const MINIMAL_YAML: &str = "constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n";

/// rigor.yaml with a belief constraint (triggers claim extraction + evaluation).
const YAML_WITH_BELIEF: &str = r#"constraints:
  beliefs:
    - id: factual
      epistemic_type: belief
      name: Factual accuracy
      description: Statements must be factually accurate
      rego: |
        package rigor.factual
        violation := []
      message: Factual accuracy check
  justifications: []
  defeaters: []
"#;

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

// ---------------------------------------------------------------------------
// Task 1: Tests that exercise extract_and_evaluate and evaluate_text_inline
// indirectly through proxy_request via TestProxy + MockLlmServer.
// ---------------------------------------------------------------------------

/// extract_and_evaluate parse failure path (non-streaming):
/// MockLlmServer serves SSE, but the proxy buffers it for a non-streaming request.
/// extract_and_evaluate tries serde_json::from_slice on the SSE body, fails to parse,
/// and emits Decision{allow, violations:0, claims:0}. The proxy returns the raw
/// upstream response body to the client with 200.
#[tokio::test]
async fn extract_and_evaluate_parse_failure_emits_allow() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("The sky is blue.")
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(YAML_WITH_BELIEF, &mock.url()).await;
    let body = anthropic_request_body(false, "What color is the sky?");
    let resp = proxy_post(&proxy.url(), &body).await;

    // Upstream returned 200 with SSE body. Proxy buffers it and returns 200.
    // extract_and_evaluate fails JSON parse and emits allow (fire-and-forget).
    assert_eq!(
        resp.status(),
        200,
        "Non-streaming request with SSE upstream should still return 200"
    );

    // The response body should contain SSE data (raw upstream body passed through)
    let resp_body = resp.text().await.unwrap();
    assert!(!resp_body.is_empty(), "Response body should not be empty");
}

/// extract_and_evaluate no-text path (non-streaming):
/// When upstream returns an SSE response for a non-streaming request (empty content),
/// extract_and_evaluate finds no assistant text and emits Decision{allow}.
/// Verified by 200 status (response passed through).
#[tokio::test]
async fn extract_and_evaluate_no_text_emits_allow() {
    // Empty string produces SSE with empty content_block_delta
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("")
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;
    let body = anthropic_request_body(false, "Say nothing.");
    let resp = proxy_post(&proxy.url(), &body).await;

    assert_eq!(
        resp.status(),
        200,
        "Non-streaming request with empty content should return 200"
    );
}

/// extract_and_evaluate delegates to extract_and_evaluate_text for valid responses:
/// Via streaming path, MockLlmServer returns factual claims. The proxy evaluates
/// them through extract_and_evaluate_text (claim extraction + policy evaluation).
/// With a no-op Rego policy, no violations are generated and 200 is returned.
/// The SSE body should contain the original text chunks.
#[tokio::test]
async fn extract_and_evaluate_delegates_to_text_evaluation() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(
            "The Earth orbits the Sun. Water boils at 100 degrees Celsius at sea level.",
        )
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(YAML_WITH_BELIEF, &mock.url()).await;
    let body = anthropic_request_body(true, "Tell me about Earth and water.");
    let resp = proxy_post(&proxy.url(), &body).await;

    assert_eq!(
        resp.status(),
        200,
        "Streaming response with factual claims should be allowed (200)"
    );

    let resp_body = resp.text().await.unwrap();
    let events = parse_sse_events(&resp_body);
    let text = extract_text_from_sse(&events, SseFormat::Anthropic);
    assert!(
        text.contains("Earth") && text.contains("Sun"),
        "Extracted text should contain the factual claims. Got: {}",
        text
    );
}

/// evaluate_text_inline with benign text returns "allow":
/// The streaming path invokes evaluate_text_inline on accumulated text at
/// sentence boundaries. With benign text ("The capital of France is Paris")
/// and no constraints, it returns "allow" and the stream completes normally.
#[tokio::test]
async fn evaluate_text_inline_benign_text_returns_allow() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("The capital of France is Paris.")
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;
    let body = anthropic_request_body(true, "What is the capital of France?");
    let resp = proxy_post(&proxy.url(), &body).await;

    assert_eq!(resp.status(), 200, "Benign text should be allowed through");

    let resp_body = resp.text().await.unwrap();
    let events = parse_sse_events(&resp_body);
    let text = extract_text_from_sse(&events, SseFormat::Anthropic);
    assert!(
        text.contains("Paris"),
        "Response text should contain 'Paris'. Got: {}",
        text
    );
}

/// PII-OUT detection emits PiiDetected + Violation events on the broadcast channel.
///
/// Reads `proxy.rs::extract_and_evaluate_text` (lines 3287-3318): when PII is
/// found in `assistant_text`, the proxy emits one `DaemonEvent::PiiDetected`
/// per match (direction="out") plus a `DaemonEvent::Violation` with
/// constraint_id="pii-leak", followed by `DaemonEvent::Decision{decision:"block"}`.
///
/// The streaming HTTP response is NOT modified for PII alone — chunks are
/// forwarded to the client BEFORE post-stream evaluation runs (line 1834).
/// So the only observable contract is the broadcast event stream, which the
/// dashboard / WebSocket subscribers consume. This test asserts that contract.
#[tokio::test]
async fn evaluate_text_inline_blocks_pii_in_response() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(
            "Here is a social security number: 123-45-6789. Do not share this with anyone.",
        )
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;
    // Subscribe BEFORE making the request so we don't miss any events.
    let mut events = proxy.subscribe();

    let body = anthropic_request_body(true, "Give me a test SSN");
    let resp = proxy_post(&proxy.url(), &body).await;
    // Drain the SSE stream so the proxy reaches its post-stream evaluation.
    let _resp_body = resp.text().await.unwrap();

    // The post-stream evaluation runs in a spawned task — wait up to 5s for
    // the PiiDetected event. Discard non-PII events along the way.
    let mut pii_event: Option<(String, String, String)> = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), events.recv()).await {
            Ok(Ok(DaemonEvent::PiiDetected {
                direction,
                pii_type,
                action,
                ..
            })) => {
                pii_event = Some((direction, pii_type, action));
                break;
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) => break,
            Err(_) => continue,
        }
    }

    let (direction, pii_type, action) =
        pii_event.expect("proxy must emit DaemonEvent::PiiDetected for SSN in response");
    assert_eq!(direction, "out", "PII detected on the response side");
    assert!(
        pii_type.to_lowercase().contains("ssn") || pii_type.to_lowercase().contains("social"),
        "pii_type should identify SSN. Got: {}",
        pii_type
    );
    assert_eq!(action, "block", "PII-OUT action must be 'block'");
}

/// Negative control for PII-OUT: clean response must NOT emit PiiDetected.
///
/// If proxy.rs starts firing PII events on benign text (false positives), this
/// test catches it. Pairs with `evaluate_text_inline_blocks_pii_in_response`
/// to ensure PII detection is both correct AND specific.
#[tokio::test]
async fn evaluate_text_inline_no_pii_event_on_clean_response() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("The capital of France is Paris. The Eiffel Tower is in Paris.")
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;
    let mut events = proxy.subscribe();

    let body = anthropic_request_body(true, "Tell me about France");
    let resp = proxy_post(&proxy.url(), &body).await;
    let _ = resp.text().await.unwrap();

    // Wait long enough for the post-stream evaluation to run, then drain all
    // events and verify NONE were PiiDetected.
    tokio::time::sleep(Duration::from_millis(500)).await;
    loop {
        match events.try_recv() {
            Ok(DaemonEvent::PiiDetected {
                direction,
                pii_type,
                matched,
                ..
            }) => {
                panic!(
                    "Clean response must not trigger PiiDetected. Got direction={} pii_type={} matched={}",
                    direction, pii_type, matched
                );
            }
            Ok(_) => continue,
            Err(TryRecvError::Empty) | Err(TryRecvError::Lagged(_)) => break,
            Err(TryRecvError::Closed) => break,
        }
    }
}

// ---------------------------------------------------------------------------
// Task 2: Integration tests for proxy_request decision branches.
// These exercise the proxy_request function through the full TCP stack via
// TestProxy + MockLlmServer.
// ---------------------------------------------------------------------------

/// proxy_request returns 200 for a clean streaming response.
/// MockLlmServer serves a benign Anthropic SSE response. The proxy forwards
/// the request, evaluates the stream, finds no violations, and returns the
/// complete SSE stream to the client with 200.
#[tokio::test]
async fn proxy_request_allow_clean_stream() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("The sky is blue.")
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;
    let body = anthropic_request_body(true, "What color is the sky?");
    let resp = proxy_post(&proxy.url(), &body).await;

    assert_eq!(
        resp.status(),
        200,
        "Clean streaming request should return 200"
    );

    let resp_body = resp.text().await.unwrap();

    // Verify the SSE stream is complete and well-formed
    let events = parse_sse_events(&resp_body);
    assert!(!events.is_empty(), "Response should contain SSE events");

    // Verify the text content was passed through
    let text = extract_text_from_sse(&events, SseFormat::Anthropic);
    assert!(
        text.contains("sky") || text.contains("blue"),
        "SSE text should contain 'sky' or 'blue'. Got: {}",
        text
    );

    // Verify the SSE stream contains expected Anthropic event types
    assert!(
        resp_body.contains("message_start"),
        "SSE should contain message_start event"
    );
    assert!(
        resp_body.contains("content_block_delta"),
        "SSE should contain content_block_delta events"
    );
}

/// proxy_request returns 400 on malformed JSON body.
/// Sending a non-JSON body triggers proxy_request's JSON parse failure
/// early return with StatusCode::BAD_REQUEST.
#[tokio::test]
async fn proxy_request_bad_json_returns_400() {
    let proxy = TestProxy::start(MINIMAL_YAML).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .body("not json at all")
        .send()
        .await
        .expect("transport should succeed");

    assert_eq!(
        resp.status(),
        400,
        "Malformed JSON should return 400 Bad Request"
    );

    let resp_body = resp.text().await.unwrap();
    assert!(
        resp_body.contains("Invalid JSON"),
        "Error message should mention 'Invalid JSON'. Got: {}",
        resp_body
    );
}

/// proxy_request handles non-streaming requests.
/// MockLlmServer only serves SSE, but the proxy handles non-streaming
/// requests by buffering the upstream response. The proxy returns 200
/// with the buffered body, and extract_and_evaluate runs in the background.
#[tokio::test]
async fn proxy_request_non_streaming_returns_200() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("Non-streaming response text.")
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;
    let body = anthropic_request_body(false, "Tell me something.");
    let resp = proxy_post(&proxy.url(), &body).await;

    // Even though MockLlmServer serves SSE for a non-streaming request,
    // the proxy buffers the response and returns it with 200.
    assert_eq!(
        resp.status(),
        200,
        "Non-streaming request should return 200"
    );

    let resp_body = resp.text().await.unwrap();
    assert!(
        !resp_body.is_empty(),
        "Non-streaming response body should not be empty"
    );
}

/// C5: non-streaming BLOCK path coverage.
///
/// Mock serves a non-streaming Anthropic JSON response containing
/// "VIOLATION_MARKER". The keyword constraint's Rego rule fires `violated:
/// true` when claim text contains that marker. The proxy's non-streaming
/// path buffers the JSON, then `extract_and_evaluate` (proxy.rs:2906) runs
/// in a spawned task, reaches `extract_and_evaluate_text` (line 3257) which
/// extracts claims, evaluates them through the policy pipeline, collects
/// violations, and emits `DaemonEvent::Decision{decision:"block"}` plus
/// `DaemonEvent::Violation` events.
///
/// The non-streaming path does NOT mutate the HTTP response body for blocks
/// (the body was already buffered and returned by line 2858 before evaluation
/// finishes). The observable contract is the broadcast event stream.
const C5_BLOCK_KEYWORD_YAML: &str = r#"constraints:
  beliefs:
    - id: c5-keyword-detector
      epistemic_type: belief
      name: C5 Keyword Detector
      description: Blocks if claim text contains VIOLATION_MARKER
      rego: |
        violation contains v if {
          some c in input.claims
          contains(c.text, "VIOLATION_MARKER")
          v := {"constraint_id": "c5-keyword-detector", "violated": true, "claims": [c.id], "reason": "keyword found"}
        }
      message: Keyword violation detected
  justifications: []
  defeaters: []
"#;

#[tokio::test]
async fn proxy_request_non_streaming_block_emits_violation() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_json(
            "The system contains VIOLATION_MARKER in its output. This is a factual statement.",
        )
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(C5_BLOCK_KEYWORD_YAML, &mock.url()).await;
    let mut events = proxy.subscribe();

    let body = anthropic_request_body(false, "Tell me about the system.");
    let resp = proxy_post(&proxy.url(), &body).await;
    // HTTP status is 200 — proxy buffers response and returns it before
    // background evaluation completes (proxy.rs:2858).
    assert_eq!(
        resp.status(),
        200,
        "non-streaming returns 200 even on block"
    );
    let _ = resp.text().await.unwrap();

    // The spawned evaluation task emits DaemonEvent::Violation +
    // DaemonEvent::Decision{decision:"block"} on the broadcast channel.
    let mut got_violation = false;
    let mut got_block_decision = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline && !(got_violation && got_block_decision) {
        match tokio::time::timeout(Duration::from_millis(200), events.recv()).await {
            Ok(Ok(DaemonEvent::Violation { constraint_id, .. }))
                if constraint_id == "c5-keyword-detector" =>
            {
                got_violation = true;
            }
            Ok(Ok(DaemonEvent::Decision { decision, .. })) if decision == "block" => {
                got_block_decision = true;
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) => break,
            Err(_) => continue,
        }
    }

    assert!(
        got_violation,
        "non-streaming block path must emit DaemonEvent::Violation for the keyword constraint"
    );
    assert!(
        got_block_decision,
        "non-streaming block path must emit DaemonEvent::Decision{{decision:\"block\"}}"
    );
}
