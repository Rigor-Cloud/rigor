//! E1 — Real-LLM proof-of-life (PR-2.6 Tier 1).
//!
//! Auto-skips when `OPENROUTER_API_KEY` is unset. When the key is present,
//! issues a real completion request to OpenRouter using the same HTTP shape
//! rigor's LLM-as-judge uses internally and verifies the response parses to
//! a non-empty string. This is a narrow but honest proof that:
//!   - the API key in `Rigor-Cloud/rigor` repo secrets is valid
//!   - openrouter.ai is reachable over HTTPS from the test environment
//!   - the OpenAI-compatible response shape rigor expects still lands
//!
//! Deeper integration (semantic constraint firing end-to-end via the daemon,
//! auto-retry dynamics, multi-provider parity) is deferred to PR-2.7.

use serde_json::{json, Value};

mod support;

#[tokio::test]
async fn e1_openrouter_proof_of_life() {
    require_openrouter!();

    let api_key =
        std::env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY must be set (guarded)");
    // DeepSeek R1 is the rigor-wide default for judge-style tests — a
    // reasoning model gives stronger signal on adversarial fixtures.
    // Override via RIGOR_E1_MODEL when a cheaper/faster model suffices.
    let model =
        std::env::var("RIGOR_E1_MODEL").unwrap_or_else(|_| "deepseek/deepseek-r1".to_string());

    let body = json!({
        "model": model,
        "max_tokens": 64,
        "temperature": 0.0,
        "messages": [
            {
                "role": "user",
                "content": "Reply with exactly the word OK and nothing else."
            }
        ],
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("build client");

    let response = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(&api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("openrouter POST");

    let status = response.status();
    let body: Value = response
        .json()
        .await
        .expect("openrouter response must be JSON");

    assert!(
        status.is_success(),
        "openrouter returned non-2xx (status={}, body={})",
        status,
        body
    );

    // OpenAI-compatible response shape: { choices: [{ message: { content } }] }.
    // rigor's judge parser walks the same path, so this doubles as a shape check.
    let content = body
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("response missing choices[0].message.content: {}", body));

    assert!(!content.trim().is_empty(), "model returned empty content");
    eprintln!("e1: model='{}' got content='{}'", model, content.trim());
}
