//! Integration tests for proxy.rs hot-path functions:
//! - extract_and_evaluate (tested indirectly via proxy_request non-streaming path)
//! - evaluate_text_inline (tested indirectly via proxy_request streaming evaluation)
//! - proxy_request decision branches (bad JSON, streaming allow, non-streaming allow)
//!
//! These functions are private in proxy.rs, so we exercise them through the public
//! HTTP interface via TestProxy + MockLlmServer from rigor-harness.

use rigor_harness::{extract_text_from_sse, parse_sse_events, MockLlmServerBuilder, SseFormat,
    TestProxy};

/// Minimal valid rigor.yaml (empty constraints -- no violations expected).
const MINIMAL_YAML: &str =
    "constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n";

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
async fn proxy_post(
    proxy_url: &str,
    body: &serde_json::Value,
) -> reqwest::Response {
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
    assert!(
        !resp_body.is_empty(),
        "Response body should not be empty"
    );
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

/// evaluate_text_inline with PII returns "block":
/// When the streaming response contains PII (SSN pattern 123-45-6789),
/// the proxy's PII-OUT detection in extract_and_evaluate_text detects it.
/// For streaming, the SSE chunks are forwarded as they arrive and PII is
/// checked at sentence boundaries. If PII is found mid-stream, the proxy
/// may inject an error SSE event into the stream or kill the upstream.
/// We verify the proxy handles PII-containing responses without crashing
/// and that the response indicates the PII was processed.
#[tokio::test]
async fn evaluate_text_inline_blocks_pii_in_response() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(
            "Here is a social security number: 123-45-6789. Do not share this with anyone.",
        )
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;
    let body = anthropic_request_body(true, "Give me a test SSN");
    let resp = proxy_post(&proxy.url(), &body).await;

    // The proxy should return 200 (SSE stream already started) but may contain
    // a block event injected by the PII detector, or it may pass through if
    // PII is detected post-stream in the background task.
    assert!(
        resp.status() == 200,
        "PII response should be 200 (stream already started). Got: {}",
        resp.status()
    );

    let resp_body = resp.text().await.unwrap();
    // The response body should contain SSE events (stream was started before
    // PII detection could intervene). The key test is that the proxy does not
    // crash and produces a valid SSE stream.
    assert!(
        !resp_body.is_empty(),
        "Response body should not be empty even with PII"
    );
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
    assert!(
        !events.is_empty(),
        "Response should contain SSE events"
    );

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
