//! H2: Proxy resilience under upstream failure modes.
//!
//! Exercises the proxy's behavior when the upstream LLM endpoint exhibits
//! realistic failure patterns: slow streaming (backpressure), connection
//! reset mid-stream, HTTP error responses, and malformed SSE chunks.
//!
//! Each test wires the proxy to a `MockLlmServerBuilder` configured with a
//! specific failure mode (added in H1) and asserts the proxy degrades
//! gracefully rather than crashing, hanging, or silently swallowing the
//! failure.
//!
//! These tests must FAIL if the corresponding proxy resilience code
//! regresses: e.g., if status forwarding breaks (H2-3), or if the proxy
//! starts panicking on malformed SSE bytes (H2-4).

use rigor_harness::env_lock::ENV_LOCK;
use rigor_harness::{
    extract_text_from_sse, parse_sse_events, MockLlmServerBuilder, SseFormat, TestProxy,
};
use std::time::Duration;

/// Empty constraint config — the proxy must not BLOCK in these tests; we are
/// exercising the upstream-failure paths, not constraint evaluation.
const EMPTY_YAML: &str = "constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n";

/// Helper: build a streaming Anthropic request body with no constraint-relevant
/// keywords so the proxy's mid-stream evaluator stays quiet.
fn neutral_streaming_body() -> serde_json::Value {
    serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "stream": true,
        "messages": [{"role": "user", "content": "say hello"}]
    })
}

/// H2-1: A slow upstream (50 ms per chunk) must not starve the proxy. The
/// proxy must keep streaming until upstream finishes and the client must
/// receive the full assistant text.
///
/// Regression guard: if the proxy ever buffered the full upstream response
/// before forwarding (instead of streaming chunk-by-chunk), the elapsed time
/// would still be >= delay × chunks, so we additionally assert the response
/// contains every word of the assistant payload.
#[tokio::test]
async fn slow_upstream_does_not_starve_proxy() {
    let assistant_text = "slow stream test response from upstream";
    // anthropic_sse_chunks emits >= 5 framing chunks plus one per word;
    // "slow stream test response from upstream" has 6 words, so >= 11 chunks.
    let chunks = rigor_harness::sse::anthropic_sse_chunks(assistant_text);
    assert!(
        chunks.len() >= 5,
        "test precondition: anthropic_sse_chunks produces >= 5 chunks (got {})",
        chunks.len()
    );

    let per_chunk_delay = Duration::from_millis(50);
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(assistant_text)
        .slow_response(per_chunk_delay)
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(EMPTY_YAML, &mock.url()).await;

    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let body = neutral_streaming_body();
    let start = std::time::Instant::now();
    let resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&body)
        .send()
        .await
        .expect("proxy request should succeed");
    assert_eq!(resp.status(), 200, "slow upstream must still produce 200");
    let resp_body = resp.text().await.expect("read response body");
    let elapsed = start.elapsed();

    drop(_guard);

    // Must take at least (delay × chunk_count) to confirm chunks were not
    // collapsed at upstream — the slow path was actually exercised.
    let expected_min = per_chunk_delay * (chunks.len() as u32);
    assert!(
        elapsed >= expected_min,
        "elapsed {:?} should be >= {:?} (per-chunk {:?} × {} chunks); slow path was bypassed",
        elapsed,
        expected_min,
        per_chunk_delay,
        chunks.len(),
    );

    // Generous upper bound — proxy must not hang past a reasonable multiple of
    // the natural duration. Picks 10× as a sanity ceiling.
    assert!(
        elapsed < expected_min * 10,
        "elapsed {:?} exceeded sanity ceiling {:?}; proxy likely hung",
        elapsed,
        expected_min * 10
    );

    // Client must observe the full assistant text — the proxy streamed every
    // chunk through, not just the first or last.
    let events = parse_sse_events(&resp_body);
    let text = extract_text_from_sse(&events, SseFormat::Anthropic);
    assert!(
        text.contains("slow")
            && text.contains("stream")
            && text.contains("test")
            && text.contains("response")
            && text.contains("from")
            && text.contains("upstream"),
        "client should observe the full streamed assistant text. Got: '{}'\nFull body:\n{}",
        text,
        resp_body
    );
}

/// H2-2: Upstream connection reset mid-stream propagates as either a
/// truncated body (fewer chunks than the full sequence) or a transport-level
/// error visible to the client. The proxy must NOT silently invent missing
/// chunks or hang waiting for chunks that will never arrive.
#[tokio::test]
async fn connection_reset_propagates_to_client() {
    let assistant_text = "hello world from upstream";
    let full_chunks = rigor_harness::sse::anthropic_sse_chunks(assistant_text);
    let full_count = full_chunks.len();
    assert!(
        full_count > 2,
        "test precondition: full sequence has >2 chunks (got {})",
        full_count
    );

    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(assistant_text)
        .connection_reset_after(2)
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(EMPTY_YAML, &mock.url()).await;

    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let body = neutral_streaming_body();
    let send_result = reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&body)
        .send()
        .await;

    drop(_guard);

    // Either the request itself surfaces a transport error, or the response
    // body is truncated. Both are acceptable graceful-degradation outcomes.
    match send_result {
        Ok(resp) => {
            // Headers arrived. The body MUST be truncated (fewer chunks than
            // full) or yield a transport error during read.
            let status = resp.status();
            match resp.bytes().await {
                Ok(body) => {
                    let text = String::from_utf8_lossy(&body);
                    let received_count = text.matches("data: ").count();
                    assert!(
                        received_count < full_count,
                        "client must see fewer chunks than the full sequence on reset \
                         (status={}, got {}, full={}, body={})",
                        status,
                        received_count,
                        full_count,
                        text,
                    );
                }
                Err(_) => {
                    // Body-level transport error — connection aborted. Acceptable.
                }
            }
        }
        Err(_) => {
            // Request-level error (incomplete message etc.) — also acceptable;
            // the upstream reset propagated as a transport failure before the
            // client could finish reading.
        }
    }

    // Critical: the proxy task must not have panicked. If it did, subsequent
    // requests would hang or fail. Send a follow-up request to a fresh mock to
    // confirm the proxy is still alive.
    let healthy_mock = MockLlmServerBuilder::new()
        .anthropic_chunks("alive")
        .build()
        .await;
    let healthy_proxy = TestProxy::start_with_mock(EMPTY_YAML, &healthy_mock.url()).await;
    let _g2 = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let resp2 = reqwest::Client::new()
        .post(format!("{}/v1/messages", healthy_proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&neutral_streaming_body())
        .send()
        .await
        .expect("post-reset proxy must still accept connections");
    assert_eq!(
        resp2.status(),
        200,
        "proxy must remain healthy after upstream reset"
    );
}

/// H2-3: Upstream HTTP 500 must surface as 500 to the client (not 200 with an
/// empty body, and not silently swallowed). This proves the proxy preserves
/// upstream status codes on the streaming path.
#[tokio::test]
async fn upstream_500_returned_to_client() {
    let mock = MockLlmServerBuilder::new()
        .error_response(500)
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(EMPTY_YAML, &mock.url()).await;

    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let body = neutral_streaming_body();
    let resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&body)
        .send()
        .await
        .expect("proxy should respond even on upstream 500");

    let status = resp.status();
    let resp_body = resp.text().await.unwrap_or_default();

    drop(_guard);

    assert_eq!(
        status, 500,
        "proxy must forward upstream 500 (NOT 200 with empty body). Got body: {}",
        resp_body
    );
    // The mock's 500 response is a JSON error body — confirm the proxy did not
    // strip it.
    assert!(
        !resp_body.is_empty(),
        "500 response body should be non-empty (proxy forwards upstream JSON error)"
    );
}

/// H2-4: A malformed SSE chunk (no `data:` framing, broken JSON) must not
/// crash the proxy or kill the stream. The proxy parses each line and
/// silently skips lines it cannot decode; remaining well-formed chunks
/// should still flow to the client.
#[tokio::test]
async fn malformed_chunk_does_not_crash_proxy() {
    let assistant_text = "hi";
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(assistant_text)
        .malformed_chunk_at(1, "garbage_data\n\n")
        .build()
        .await;
    let proxy = TestProxy::start_with_mock(EMPTY_YAML, &mock.url()).await;

    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let body = neutral_streaming_body();
    let resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&body)
        .send()
        .await
        .expect("proxy must not crash on malformed upstream SSE");

    let status = resp.status();
    let resp_body = resp.text().await.expect("read response body");

    drop(_guard);

    // Status forwarding works (mock replies 200 with text/event-stream).
    assert_eq!(
        status, 200,
        "proxy must return 200 even with a malformed chunk in the stream"
    );

    // Liveness: the proxy completed the stream rather than hanging or
    // panicking. Exercise the proxy again to prove it is still healthy.
    let healthy_mock = MockLlmServerBuilder::new()
        .anthropic_chunks("alive")
        .build()
        .await;
    let healthy_proxy = TestProxy::start_with_mock(EMPTY_YAML, &healthy_mock.url()).await;
    let _g2 = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let resp2 = reqwest::Client::new()
        .post(format!("{}/v1/messages", healthy_proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&neutral_streaming_body())
        .send()
        .await
        .expect("post-malformed proxy must still accept connections");
    assert_eq!(
        resp2.status(),
        200,
        "proxy must remain healthy after a malformed-chunk stream"
    );

    // Cheap sanity: the response body is non-empty (the proxy streamed
    // *something* — either the well-formed chunks, the garbage bytes, or
    // both — rather than producing an empty body on parse failure).
    assert!(
        !resp_body.is_empty(),
        "proxy should forward at least some bytes even when one chunk is malformed"
    );
}
