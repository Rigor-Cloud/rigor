#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! Smoke tests for rigor-harness primitives.
//!
//! Verifies that IsolatedHome, TestCA, MockLlmServer, TestProxy, and SSE helpers
//! compose correctly. This is the acceptance gate for Phase 7 (issue #8).

use rigor_harness::{
    extract_text_from_sse, parse_sse_events, IsolatedHome, MockLlmServerBuilder, SseFormat,
};

/// Minimal valid rigor.yaml for tests (ConstraintsSection is a struct, not a list).
const MINIMAL_YAML: &str = "constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n";

#[test]
fn test_isolated_home_does_not_touch_real_home() {
    let real_home = std::env::var("HOME").unwrap();
    let home = IsolatedHome::new();

    assert_ne!(home.home_str(), real_home);
    assert!(home.rigor_dir.exists());

    let yaml_path = home.write_rigor_yaml(MINIMAL_YAML);
    assert!(yaml_path.exists());

    // IsolatedHome must live under a temp directory, not under real HOME
    assert!(
        !home.path.starts_with(&real_home) || real_home.contains("tmp"),
        "IsolatedHome path {} must not be under real HOME {}",
        home.path.display(),
        real_home,
    );
}

#[test]
fn test_test_ca_generates_valid_certs() {
    let ca = rigor_harness::TestCA::new().expect("TestCA::new should succeed");

    let server_config = ca
        .server_config_for_host("api.anthropic.com")
        .expect("server config should succeed");
    assert!(std::sync::Arc::strong_count(&server_config) >= 1);

    let _client = ca.reqwest_client();

    assert!(
        ca.ca_cert_pem().contains("BEGIN CERTIFICATE"),
        "PEM should contain BEGIN CERTIFICATE header",
    );
}

#[tokio::test]
async fn test_mock_llm_serves_anthropic_sse() {
    let expected_text = "Rust uses ownership for memory safety.";
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(expected_text)
        .build()
        .await;

    assert_ne!(mock.addr().port(), 0);

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/messages", mock.url()))
        .header("content-type", "application/json")
        .body(r#"{"model":"test","messages":[{"role":"user","content":"test"}],"stream":true}"#)
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 200);

    let body = resp.text().await.expect("read body");
    let events = parse_sse_events(&body);
    let text = extract_text_from_sse(&events, SseFormat::Anthropic);

    let normalized = text.trim();
    assert!(
        normalized.contains("Rust") && normalized.contains("memory safety"),
        "Expected text about Rust ownership, got: {}",
        normalized,
    );
}

#[tokio::test]
async fn test_mock_llm_openai_format() {
    let expected_text = "Hello from OpenAI format.";
    let mock = MockLlmServerBuilder::new()
        .openai_chunks(expected_text)
        .route("/v1/chat/completions")
        .build()
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", mock.url()))
        .header("content-type", "application/json")
        .body(r#"{"model":"test","messages":[{"role":"user","content":"test"}],"stream":true}"#)
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 200);

    let body = resp.text().await.expect("read body");
    let events = parse_sse_events(&body);
    let text = extract_text_from_sse(&events, SseFormat::OpenAI);
    let normalized = text.trim();
    assert!(
        normalized.contains("Hello") && normalized.contains("OpenAI"),
        "Expected OpenAI text, got: {}",
        normalized,
    );
}

#[test]
fn test_subprocess_with_isolated_home() {
    use rigor_harness::{default_hook_input, parse_response, run_rigor};

    let home = IsolatedHome::new();
    home.write_rigor_yaml(MINIMAL_YAML);

    let input = default_hook_input(&home);
    let (stdout, stderr, exit_code) = run_rigor(&home, &input);

    assert_eq!(
        exit_code, 0,
        "rigor should exit 0 with empty constraints. stderr: {}",
        stderr,
    );

    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "No constraints should produce no decision. Got: {}",
        stdout,
    );
}

#[tokio::test]
async fn test_test_proxy_starts_and_accepts_connections() {
    use rigor_harness::TestProxy;

    let proxy = TestProxy::start(MINIMAL_YAML).await;

    assert_ne!(proxy.addr().port(), 0);

    let client = reqwest::Client::new();
    let resp = client
        .get(proxy.url())
        .send()
        .await
        .expect("GET / should succeed");

    // The production router has routes; any response proves the proxy is up
    assert!(
        resp.status().is_success() || resp.status().is_redirection() || resp.status() == 404,
        "Proxy should respond. Got status: {}",
        resp.status(),
    );
}
