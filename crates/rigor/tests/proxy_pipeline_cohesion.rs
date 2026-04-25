//! Proxy pipeline cohesion test.
//!
//! Exercises the complete proxy decision chain as a connected flow:
//!   proxy_request -> extract_and_evaluate_text -> collect_violations -> determine_decision
//!
//! Uses a constraint that fires on a specific keyword ("FORBIDDEN_KEYWORD_XYZ"),
//! sends that keyword through MockLlmServer + TestProxy, and asserts the full
//! decision chain: claim extracted -> violation collected -> decision determined
//! -> response modified with BLOCK marker.
//!
//! Also verifies the inverse: a clean response (no keyword) passes through
//! unmodified (ALLOW).
//!
//! **RIGOR_NO_RETRY**: All tests set `RIGOR_NO_RETRY=1` to disable the B2
//! auto-retry mechanism, ensuring violations produce BLOCK error SSE events
//! rather than transparent retry+resubmit. This is serialized via ENV_LOCK
//! to prevent races with parallel tests.

use rigor_harness::{MockLlmServerBuilder, TestProxy};

/// Serializes env var mutations for RIGOR_NO_RETRY across parallel tests.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Constraint YAML with a regex that fires when the response contains "FORBIDDEN_KEYWORD_XYZ".
/// This is a synthetic constraint designed for deterministic testing.
const COHESION_CONSTRAINT_YAML: &str = r#"constraints:
  beliefs:
    - id: no-forbidden-keyword
      epistemic_type: belief
      name: "No Forbidden Keyword"
      description: "Responses must not contain the phrase FORBIDDEN_KEYWORD_XYZ."
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match("(?i)FORBIDDEN_KEYWORD_XYZ", c.text)
          v := {
            "constraint_id": "no-forbidden-keyword",
            "violated": true,
            "claims": [c.id],
            "reason": "Response contains forbidden keyword"
          }
        }
      message: Response contains forbidden keyword FORBIDDEN_KEYWORD_XYZ
  justifications: []
  defeaters: []
"#;

/// Build an Anthropic Messages API request body.
fn anthropic_request_body(user_msg: &str) -> serde_json::Value {
    serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "stream": true,
        "messages": [{"role": "user", "content": user_msg}]
    })
}

/// Send a streaming POST through the proxy, return the full SSE body text.
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

/// Full pipeline cohesion test: violating response triggers BLOCK.
///
/// Chain verified:
/// 1. MockLlmServer returns SSE containing the forbidden keyword
/// 2. TestProxy receives the request and forwards to MockLlmServer
/// 3. Proxy extracts claims from the SSE stream (extract_and_evaluate_text)
/// 4. Rego policy evaluates claims and finds a violation (collect_violations)
/// 5. Proxy determines BLOCK decision (determine_decision)
/// 6. Proxy injects error SSE event with "rigor BLOCKED" marker
#[tokio::test]
async fn violating_response_triggers_block_through_full_pipeline() {
    let _guard = ENV_LOCK.lock().unwrap();
    let orig = disable_retry();

    let violating_text =
        "The system uses FORBIDDEN_KEYWORD_XYZ for internal processing.";

    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(violating_text)
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(COHESION_CONSTRAINT_YAML, &mock.url()).await;

    let body = anthropic_request_body("Tell me about the system.");
    let sse_body = proxy_post(&proxy.url(), &body).await;

    restore_retry(orig);

    let decision = classify_decision(&sse_body);
    assert_eq!(
        decision, "block",
        "violating response should be BLOCKED.\nSSE body:\n{}",
        sse_body
    );

    // Verify the block message mentions the constraint or BLOCKED marker
    assert!(
        sse_body.contains("forbidden keyword")
            || sse_body.contains("no-forbidden-keyword")
            || sse_body.contains("BLOCKED"),
        "block SSE should reference the constraint or contain BLOCKED marker.\nSSE body:\n{}",
        sse_body
    );
}

/// Full pipeline cohesion test: clean response passes through (ALLOW).
///
/// Same pipeline, but the response does NOT contain the forbidden keyword.
/// Verifies that the proxy allows clean responses through without modification.
#[tokio::test]
async fn clean_response_passes_through_full_pipeline() {
    let _guard = ENV_LOCK.lock().unwrap();
    let orig = disable_retry();

    let clean_text = "The system uses standard algorithms for processing data efficiently.";

    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(clean_text)
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(COHESION_CONSTRAINT_YAML, &mock.url()).await;

    let body = anthropic_request_body("Tell me about the system.");
    let sse_body = proxy_post(&proxy.url(), &body).await;

    restore_retry(orig);

    let decision = classify_decision(&sse_body);
    assert_eq!(
        decision, "allow",
        "clean response should be ALLOWED.\nSSE body:\n{}",
        sse_body
    );

    // Verify the original content words are present in the SSE response.
    // Words appear in individual SSE delta chunks (split at word boundaries).
    assert!(
        sse_body.contains("standard") && sse_body.contains("algorithms"),
        "clean response should contain the original text words.\nSSE body:\n{}",
        sse_body
    );
}

/// Sequential requests: block then allow through the same proxy instance.
///
/// Verifies that the proxy pipeline handles multiple requests correctly,
/// maintaining proper state isolation between requests (no cross-contamination
/// of violations from the first request into the second).
#[tokio::test]
async fn sequential_block_then_allow_same_proxy() {
    use rigor_harness::sse::anthropic_sse_chunks;

    let _guard = ENV_LOCK.lock().unwrap();
    let orig = disable_retry();

    let violating_text =
        "The system relies on FORBIDDEN_KEYWORD_XYZ for all operations.";
    let clean_text = "The system relies on well-tested algorithms for all operations.";

    let violating_chunks = anthropic_sse_chunks(violating_text);
    let clean_chunks = anthropic_sse_chunks(clean_text);

    let mock = MockLlmServerBuilder::new()
        .response_sequence(vec![violating_chunks, clean_chunks])
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(COHESION_CONSTRAINT_YAML, &mock.url()).await;

    // Request 1: violating response -> BLOCK
    let body1 = anthropic_request_body("First request");
    let sse1 = proxy_post(&proxy.url(), &body1).await;
    assert_eq!(
        classify_decision(&sse1),
        "block",
        "first request (violating) should be BLOCKED.\nSSE body:\n{}",
        sse1
    );

    // Request 2: clean response -> ALLOW
    let body2 = anthropic_request_body("Second request");
    let sse2 = proxy_post(&proxy.url(), &body2).await;

    restore_retry(orig);

    assert_eq!(
        classify_decision(&sse2),
        "allow",
        "second request (clean) should be ALLOWED.\nSSE body:\n{}",
        sse2
    );

    // Verify mock received both requests (no retry = exactly 2 requests)
    let received = mock.received_requests();
    assert_eq!(
        received.len(),
        2,
        "mock should have received exactly 2 requests"
    );
}
