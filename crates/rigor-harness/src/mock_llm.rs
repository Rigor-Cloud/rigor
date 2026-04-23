use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use axum::{Router, routing::post, response::sse::{Event, Sse}};
use futures_util::stream;

use crate::sse::{anthropic_sse_chunks, openai_sse_chunks};

/// Builder for configuring a MockLlmServer before starting it.
pub struct MockLlmServerBuilder {
    chunks: Vec<String>,
    route_path: String,
}

/// A mock LLM server that serves deterministic SSE responses on an ephemeral port.
///
/// Binds to `127.0.0.1:0` and shuts down cleanly on Drop via a oneshot channel.
pub struct MockLlmServer {
    addr: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockLlmServerBuilder {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            route_path: "/v1/messages".to_string(),
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

    /// Set the route path (default: "/v1/messages").
    pub fn route(mut self, path: &str) -> Self {
        self.route_path = path.to_string();
        self
    }

    /// Start the server and return a running MockLlmServer.
    pub async fn build(self) -> MockLlmServer {
        let chunks = Arc::new(self.chunks);
        let route_path = self.route_path;

        let chunks_clone = chunks.clone();
        let handler = move || {
            let chunks = chunks_clone.clone();
            async move {
                let events: Vec<Result<Event, std::convert::Infallible>> = chunks
                    .iter()
                    .map(|data| Ok(Event::default().data(data)))
                    .collect();
                Sse::new(stream::iter(events))
            }
        };

        let app = Router::new().route(&route_path, post(handler));

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
}
