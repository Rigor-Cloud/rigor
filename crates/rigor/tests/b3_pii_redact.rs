#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! B3: PII redact-before-forward integration test.
//!
//! Proves: when a user message contains PII (email addresses, SSNs), the proxy's
//! PII-IN scan detects and redacts them BEFORE forwarding the request to upstream.
//! MockLlmServer's request tracking verifies the upstream received redacted text
//! with [REDACTED:*] tags instead of raw PII.

use rigor_harness::{MockLlmServerBuilder, TestProxy};

/// Minimal constraint YAML -- no violations expected. B3 tests PII redaction,
/// not the BLOCK path.
const MINIMAL_YAML: &str = "constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n";

/// Extract the last user message content from a received request body.
fn extract_last_user_content(body: &serde_json::Value) -> Option<String> {
    body["messages"]
        .as_array()?
        .iter()
        .rev()
        .find(|m| m["role"] == "user")?["content"]
        .as_str()
        .map(|s| s.to_string())
}

/// Helper: build a valid Anthropic request body with given user message.
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

/// B3: PII (SSN + email) in user message is redacted before upstream send.
/// MockLlmServer receives [REDACTED:*] tags instead of raw PII.
#[tokio::test]
async fn b3_pii_redacted_before_upstream_send() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("I understand your request.")
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;

    let body = anthropic_request_body(
        true,
        "My SSN is 123-45-6789 and my email is secret@example.com",
    );
    let resp = proxy_post(&proxy.url(), &body).await;

    // Consume response to ensure the proxy finishes processing
    let _resp_body = resp.text().await.unwrap();

    // Inspect what MockLlmServer received
    let received = mock.received_requests();
    assert!(
        !received.is_empty(),
        "Mock should have received at least one request"
    );

    let user_content = extract_last_user_content(&received[0].body)
        .expect("received request should have user message content");

    // PII should be redacted
    assert!(
        !user_content.contains("123-45-6789"),
        "SSN should be redacted in upstream request. Got: '{}'",
        user_content
    );
    assert!(
        !user_content.contains("secret@example.com"),
        "Email should be redacted in upstream request. Got: '{}'",
        user_content
    );
    assert!(
        user_content.contains("[REDACTED:"),
        "Upstream request should contain [REDACTED:*] tags. Got: '{}'",
        user_content
    );
}

/// B3: PII redaction is transparent to the client -- client receives 200 with
/// a normal response body.
#[tokio::test]
async fn b3_pii_redaction_transparent_to_client() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("I understand your request.")
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;

    let body = anthropic_request_body(
        true,
        "My SSN is 123-45-6789 and my email is secret@example.com",
    );
    let resp = proxy_post(&proxy.url(), &body).await;
    let status = resp.status();
    let resp_body = resp.text().await.unwrap();

    assert_eq!(
        status, 200,
        "PII redaction should be transparent -- client gets 200"
    );
    assert!(!resp_body.is_empty(), "Response body should not be empty");
}

/// B3: Negative test -- a clean message (no PII) is forwarded verbatim.
/// No [REDACTED:] tags present in upstream request.
#[tokio::test]
async fn b3_no_redaction_for_clean_message() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("Paris is the capital of France.")
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;

    let clean_msg = "What is the capital of France?";
    let body = anthropic_request_body(true, clean_msg);
    let resp = proxy_post(&proxy.url(), &body).await;

    // Consume response
    let _resp_body = resp.text().await.unwrap();

    let received = mock.received_requests();
    assert!(
        !received.is_empty(),
        "Mock should have received at least one request"
    );

    let user_content = extract_last_user_content(&received[0].body)
        .expect("received request should have user message content");

    // Clean message should be forwarded verbatim (no redaction)
    assert_eq!(
        user_content, clean_msg,
        "Clean message should be forwarded verbatim. Got: '{}'",
        user_content
    );
    assert!(
        !user_content.contains("[REDACTED:"),
        "Clean message should NOT contain redaction tags. Got: '{}'",
        user_content
    );
}
