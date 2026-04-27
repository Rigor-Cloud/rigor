//! E2E scenario runner using MockLlmServer + TestProxy from rigor-harness.
//!
//! Launches an in-process mock LLM and proxy, sends requests through the proxy,
//! and verifies the rigor pipeline produces expected decisions.

use rigor_harness::{
    extract_text_from_sse, parse_sse_events, MockLlmServerBuilder, SseFormat, TestProxy,
};

/// Rego keyword constraint that fires `violated: true` when claim text
/// contains "VIOLATION_MARKER". Copied from b1_kill_switch.rs.
const BLOCK_CONSTRAINT_YAML: &str = r#"constraints:
  beliefs:
    - id: e2e-keyword-detector
      epistemic_type: belief
      name: E2E Keyword Detector
      description: Blocks if claim text contains VIOLATION_MARKER
      rego: |
        violation contains v if {
          some c in input.claims
          contains(c.text, "VIOLATION_MARKER")
          v := {"constraint_id": "e2e-keyword-detector", "violated": true, "claims": [c.id], "reason": "keyword found"}
        }
      message: Keyword violation detected
  justifications: []
  defeaters: []
"#;

const MINIMAL_YAML: &str = "constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n";

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

/// Run built-in E2E scenarios. If `suite` is provided, prints a message and
/// runs built-in scenarios anyway (YAML suite loading not yet available).
pub async fn run_e2e(suite: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    if let Some(ref path) = suite {
        eprintln!(
            "YAML suite loading not yet available (got: {}); running built-in scenarios.",
            path.display()
        );
    }

    let mut passed = 0u32;
    let total = 2u32;

    // Scenario 1: clean passthrough
    {
        let mock = MockLlmServerBuilder::new()
            .anthropic_chunks(
                "The Rust compiler ensures memory safety through ownership and borrowing. \
                 This is a factual statement about Rust.",
            )
            .build()
            .await;

        let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;

        let body = anthropic_request_body(true, "Tell me about Rust");
        let resp = proxy_post(&proxy.url(), &body).await;

        anyhow::ensure!(
            resp.status().is_success(),
            "clean-passthrough: proxy returned {}",
            resp.status()
        );

        let resp_body = resp.text().await?;
        let events = parse_sse_events(&resp_body);
        let text = extract_text_from_sse(&events, SseFormat::Anthropic);

        anyhow::ensure!(
            !text.is_empty(),
            "clean-passthrough: expected non-empty response text"
        );

        println!("PASS: clean-passthrough");
        passed += 1;
    }

    // Scenario 2: violation detection
    {
        // Set RIGOR_NO_RETRY so the proxy does not retry on BLOCK
        let orig_no_retry = std::env::var("RIGOR_NO_RETRY").ok();
        // Safety: e2e scenarios run sequentially in a single-threaded context
        unsafe { std::env::set_var("RIGOR_NO_RETRY", "1") };

        let violation_text =
            "The system contains VIOLATION_MARKER in its output. This is a factual statement.";

        let mock = MockLlmServerBuilder::new()
            .anthropic_chunks(violation_text)
            .build()
            .await;

        let proxy = TestProxy::start_with_mock(BLOCK_CONSTRAINT_YAML, &mock.url()).await;

        let body = anthropic_request_body(true, "Tell me something");
        let resp = proxy_post(&proxy.url(), &body).await;
        let resp_body = resp.text().await?;

        // Restore env before assertions
        match orig_no_retry {
            Some(v) => unsafe { std::env::set_var("RIGOR_NO_RETRY", v) },
            None => unsafe { std::env::remove_var("RIGOR_NO_RETRY") },
        }

        let has_block = resp_body.contains("rigor BLOCKED") || resp_body.contains("event: error");
        anyhow::ensure!(
            has_block,
            "violation-detection: expected 'rigor BLOCKED' or 'event: error' in response. Got:\n{}",
            resp_body
        );

        println!("PASS: violation-detection");
        passed += 1;
    }

    println!("e2e: {passed}/{total} scenarios passed");

    if passed < total {
        anyhow::bail!("e2e: {}/{} scenarios failed", total - passed, total);
    }

    Ok(())
}
