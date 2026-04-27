use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::oneshot;
use axum::{Router, routing::post, response::sse::{Event, Sse}, response::IntoResponse, http::header};
use futures_util::stream;

use crate::sse::{anthropic_sse_chunks, openai_sse_chunks};

/// A request received by the MockLlmServer, capturing the parsed JSON body.
#[derive(Debug, Clone)]
pub struct ReceivedRequest {
    pub body: serde_json::Value,
}

/// Builder for configuring a MockLlmServer before starting it.
pub struct MockLlmServerBuilder {
    chunks: Vec<String>,
    response_sequence: Option<Vec<Vec<String>>>,
    route_path: String,
    /// When set, the server returns this raw JSON body (not SSE).
    /// Used by non-streaming-path tests where the proxy buffers the response
    /// and runs `serde_json::from_slice` on it.
    json_body: Option<String>,
}

/// A mock LLM server that serves deterministic SSE responses on an ephemeral port.
///
/// Binds to `127.0.0.1:0` and shuts down cleanly on Drop via a oneshot channel.
pub struct MockLlmServer {
    addr: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
    received: Arc<Mutex<Vec<ReceivedRequest>>>,
}

impl MockLlmServerBuilder {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            response_sequence: None,
            route_path: "/v1/messages".to_string(),
            json_body: None,
        }
    }

    /// Set chunks to Anthropic SSE format for the given text.
    pub fn anthropic_chunks(mut self, text: &str) -> Self {
        self.chunks = anthropic_sse_chunks(text);
        self
    }

    /// Set chunks to OpenAI SSE format for the given text.
    pub fn openai_chunks(mut self, text: &str) -> Self {
        self.chunks = openai_sse_chunks(text);
        self
    }

    /// Provide raw SSE data-line payloads directly.
    pub fn raw_chunks(mut self, chunks: Vec<String>) -> Self {
        self.chunks = chunks;
        self
    }

    /// Provide a sequence of response chunk sets for per-call-index selection.
    ///
    /// When set, each call to the mock server uses the response at the matching
    /// index. If the call index exceeds the sequence length, the last entry is
    /// repeated. This is useful for B2 auto-retry tests where call 0 triggers a
    /// violation and call 1 returns a clean response.
    pub fn response_sequence(mut self, responses: Vec<Vec<String>>) -> Self {
        self.response_sequence = Some(responses);
        self
    }

    /// Set the route path (default: "/v1/messages").
    pub fn route(mut self, path: &str) -> Self {
        self.route_path = path.to_string();
        self
    }

    /// Serve a non-streaming Anthropic-format JSON response with the given
    /// assistant text. The proxy's non-streaming path runs
    /// `serde_json::from_slice` on the buffered body, so the body MUST parse
    /// as JSON for the post-response evaluation pipeline to fire.
    pub fn anthropic_json(mut self, text: &str) -> Self {
        let body = serde_json::json!({
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [{"type": "text", "text": text}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });
        self.json_body = Some(body.to_string());
        self
    }

    /// Start the server and return a running MockLlmServer.
    pub async fn build(self) -> MockLlmServer {
        let route_path = self.route_path;
        let received: Arc<Mutex<Vec<ReceivedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let received_for_server = received.clone();

        // Build the list of response sets. When response_sequence is provided,
        // each call index selects its own chunk set. Otherwise wrap the single
        // chunks vec so the handler can use the same code path.
        let all_responses: Arc<Vec<Vec<String>>> = Arc::new(
            self.response_sequence.unwrap_or_else(|| vec![self.chunks])
        );

        let call_count = Arc::new(AtomicUsize::new(0));

        let app = if let Some(json_str) = self.json_body {
            // Non-streaming JSON mode: serve raw JSON body with content-type:
            // application/json so the proxy's `serde_json::from_slice` succeeds.
            let json_arc = Arc::new(json_str);
            let received_clone = received.clone();
            let json_handler = move |body: axum::body::Bytes| {
                let received = received_clone.clone();
                let json = json_arc.clone();
                async move {
                    let parsed = serde_json::from_slice::<serde_json::Value>(&body)
                        .unwrap_or(serde_json::Value::Null);
                    received.lock().unwrap().push(ReceivedRequest { body: parsed });
                    (
                        [(header::CONTENT_TYPE, "application/json")],
                        (*json).clone(),
                    )
                        .into_response()
                }
            };
            Router::new().route(&route_path, post(json_handler))
        } else {
            let received_clone = received.clone();
            let handler = move |body: axum::body::Bytes| {
                let received = received_clone.clone();
                let responses = all_responses.clone();
                let counter = call_count.clone();
                async move {
                    // Track received request body
                    let json_body = serde_json::from_slice::<serde_json::Value>(&body)
                        .unwrap_or(serde_json::Value::Null);
                    received.lock().unwrap().push(ReceivedRequest { body: json_body });

                    // Select response by call index; repeat last if index exceeds length
                    let call_idx = counter.fetch_add(1, Ordering::SeqCst);
                    let chunks = if call_idx < responses.len() {
                        &responses[call_idx]
                    } else {
                        responses.last().unwrap()
                    };

                    let events: Vec<Result<Event, std::convert::Infallible>> = chunks
                        .iter()
                        .map(|data| Ok(Event::default().data(data)))
                        .collect();
                    Sse::new(stream::iter(events))
                }
            };
            Router::new().route(&route_path, post(handler))
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock LLM server");
        let addr = listener.local_addr().expect("get local addr");

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        MockLlmServer {
            addr,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
            received: received_for_server,
        }
    }
}

impl Default for MockLlmServerBuilder {
    fn default() -> Self { Self::new() }
}

impl MockLlmServer {
    /// Convenience constructor: start with raw SSE data-line chunks on the default route.
    pub async fn start(chunks: Vec<String>) -> Self {
        MockLlmServerBuilder::new()
            .raw_chunks(chunks)
            .build()
            .await
    }

    /// The socket address the server is listening on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The base URL of the server (e.g. `http://127.0.0.1:12345`).
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    /// Returns a snapshot of all requests received by this server.
    ///
    /// Each entry contains the parsed JSON body (or `Value::Null` if parsing
    /// failed). The order matches the order requests were received.
    pub fn received_requests(&self) -> Vec<ReceivedRequest> {
        self.received.lock().unwrap().clone()
    }
}

impl Drop for MockLlmServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_llm_starts_and_responds() {
        let server = MockLlmServerBuilder::new()
            .anthropic_chunks("hello")
            .build()
            .await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/v1/messages", server.url()))
            .body("{}")
            .send()
            .await
            .expect("send request");

        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn test_mock_llm_anthropic_format() {
        let server = MockLlmServerBuilder::new()
            .anthropic_chunks("hello world")
            .build()
            .await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/v1/messages", server.url()))
            .body("{}")
            .send()
            .await
            .unwrap();

        let body = resp.text().await.unwrap();
        // SSE events should contain content_block_delta
        assert!(body.contains("content_block_delta"), "body should contain anthropic delta events: {}", body);
        assert!(body.contains("message_stop"), "body should contain message_stop: {}", body);
    }

    #[tokio::test]
    async fn test_mock_llm_openai_format() {
        let server = MockLlmServerBuilder::new()
            .openai_chunks("hello world")
            .route("/v1/chat/completions")
            .build()
            .await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/v1/chat/completions", server.url()))
            .body("{}")
            .send()
            .await
            .unwrap();

        let body = resp.text().await.unwrap();
        assert!(body.contains("\"content\":"), "body should contain openai content deltas: {}", body);
        assert!(body.contains("[DONE]"), "body should contain [DONE]: {}", body);
    }

    #[tokio::test]
    async fn test_mock_llm_shutdown_on_drop() {
        let addr;
        {
            let server = MockLlmServerBuilder::new()
                .raw_chunks(vec!["test".to_string()])
                .build()
                .await;
            addr = server.addr();
            // server drops here
        }

        // Give a moment for shutdown to propagate
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Connection should be refused after shutdown
        let result = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{}/v1/messages", addr.port()))
            .body("{}")
            .send()
            .await;
        assert!(result.is_err(), "connection should fail after server drop");
    }

    #[tokio::test]
    async fn test_mock_llm_tracks_received_requests() {
        let server = MockLlmServerBuilder::new()
            .anthropic_chunks("tracked")
            .build()
            .await;

        let client = reqwest::Client::new();

        // Send two POST requests with different JSON bodies
        let body_a = serde_json::json!({"model": "test-a", "messages": [{"role": "user", "content": "hello"}]});
        let body_b = serde_json::json!({"model": "test-b", "messages": [{"role": "user", "content": "world"}]});

        client
            .post(format!("{}/v1/messages", server.url()))
            .json(&body_a)
            .send()
            .await
            .unwrap();

        client
            .post(format!("{}/v1/messages", server.url()))
            .json(&body_b)
            .send()
            .await
            .unwrap();

        let received = server.received_requests();
        assert_eq!(received.len(), 2, "should have received 2 requests");
        assert_eq!(received[0].body["model"], "test-a");
        assert_eq!(received[1].body["model"], "test-b");
        assert_eq!(received[0].body["messages"][0]["content"], "hello");
        assert_eq!(received[1].body["messages"][0]["content"], "world");
    }

    #[tokio::test]
    async fn test_mock_llm_response_sequence() {
        use crate::sse::{anthropic_sse_chunks, extract_text_from_sse, SseFormat};

        let chunks_a = anthropic_sse_chunks("response alpha");
        let chunks_b = anthropic_sse_chunks("response beta");

        let server = MockLlmServerBuilder::new()
            .response_sequence(vec![chunks_a, chunks_b])
            .build()
            .await;

        let client = reqwest::Client::new();

        // First request gets response A
        let resp_a = client
            .post(format!("{}/v1/messages", server.url()))
            .body("{}")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();

        // Second request gets response B
        let resp_b = client
            .post(format!("{}/v1/messages", server.url()))
            .body("{}")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();

        let events_a = crate::sse::parse_sse_events(&resp_a);
        let events_b = crate::sse::parse_sse_events(&resp_b);
        let text_a = extract_text_from_sse(&events_a, SseFormat::Anthropic);
        let text_b = extract_text_from_sse(&events_b, SseFormat::Anthropic);

        assert_eq!(text_a, "response alpha", "first call should get response A");
        assert_eq!(text_b, "response beta", "second call should get response B");
    }

    #[tokio::test]
    async fn test_mock_llm_response_sequence_repeats_last() {
        use crate::sse::{anthropic_sse_chunks, extract_text_from_sse, SseFormat};

        let chunks_only = anthropic_sse_chunks("the-only-response");

        let server = MockLlmServerBuilder::new()
            .response_sequence(vec![chunks_only])
            .build()
            .await;

        let client = reqwest::Client::new();

        // Both calls should get the same (only) response
        let resp_1 = client
            .post(format!("{}/v1/messages", server.url()))
            .body("{}")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();

        let resp_2 = client
            .post(format!("{}/v1/messages", server.url()))
            .body("{}")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();

        let events_1 = crate::sse::parse_sse_events(&resp_1);
        let events_2 = crate::sse::parse_sse_events(&resp_2);
        let text_1 = extract_text_from_sse(&events_1, SseFormat::Anthropic);
        let text_2 = extract_text_from_sse(&events_2, SseFormat::Anthropic);

        assert_eq!(text_1, "the-only-response", "first call should get the response");
        assert_eq!(text_2, "the-only-response", "second call should also get the response");
    }
}
