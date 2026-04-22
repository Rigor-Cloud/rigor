//! Chat-completion client abstraction for `rigor corpus record`.
//!
//! Small trait so recording can be unit-tested with a canned mock instead
//! of real OpenRouter calls. Production code uses [`OpenRouterClient`];
//! tests use [`MockChatClient`] under `#[cfg(test)]` or test modules.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Request payload for a single chat completion.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub temperature: f64,
    pub max_tokens: u32,
}

/// Response tuple: raw text + token counts + optional cost + optional
/// provider request id. Matches [`crate::corpus::RecordedSample`] fields.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub text: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cost_usd: Option<f64>,
    pub provider_id: Option<String>,
}

/// Trait implemented by OpenRouter in production and by a test double.
#[async_trait]
pub trait ChatClient: Send + Sync {
    async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse>;
}

// =============================================================================
// OpenRouter implementation
// =============================================================================

/// OpenRouter OpenAI-compatible client. Reads the key from
/// `OPENROUTER_API_KEY`. Idempotent — safe to construct once and share.
pub struct OpenRouterClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl OpenRouterClient {
    /// Construct from the `OPENROUTER_API_KEY` env var.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .context("OPENROUTER_API_KEY must be set for `rigor corpus record`")?;
        Ok(Self::new(api_key, "https://openrouter.ai/api"))
    }

    pub fn new(api_key: String, base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("build reqwest client"),
            api_key,
            base_url: base_url.into(),
        }
    }
}

// JSON shapes for OpenAI-compatible /chat/completions.
#[derive(Serialize)]
struct OpenAiRequestMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    temperature: f64,
    max_tokens: u32,
    messages: Vec<OpenAiRequestMessage<'a>>,
}

#[derive(Deserialize)]
struct OpenAiResponseMessage {
    content: String,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    id: Option<String>,
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[async_trait]
impl ChatClient for OpenRouterClient {
    async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse> {
        let mut messages = Vec::new();
        if let Some(sys) = &req.system_prompt {
            messages.push(OpenAiRequestMessage {
                role: "system",
                content: sys,
            });
        }
        messages.push(OpenAiRequestMessage {
            role: "user",
            content: &req.prompt,
        });

        let body = OpenAiRequest {
            model: &req.model,
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            messages,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("POST /v1/chat/completions")?;

        let status = resp.status();
        let text = resp.text().await.context("read response body")?;
        if !status.is_success() {
            anyhow::bail!("openrouter returned {}: {}", status, text);
        }

        let parsed: OpenAiResponse = serde_json::from_str(&text)
            .with_context(|| format!("parse openrouter response: {}", text))?;

        let choice = parsed
            .choices
            .into_iter()
            .next()
            .context("openrouter response has no choices")?;

        let (prompt_tokens, completion_tokens) = parsed
            .usage
            .map(|u| (u.prompt_tokens, u.completion_tokens))
            .unwrap_or((0, 0));

        Ok(ChatResponse {
            text: choice.message.content,
            prompt_tokens,
            completion_tokens,
            cost_usd: None, // OpenRouter reports in headers — not wired yet
            provider_id: parsed.id,
        })
    }
}

// =============================================================================
// Test double
// =============================================================================

/// A `ChatClient` that returns pre-recorded responses. Used by tests.
#[cfg(test)]
pub struct MockChatClient {
    pub responses: std::sync::Mutex<Vec<ChatResponse>>,
    pub calls: std::sync::Mutex<Vec<ChatRequest>>,
}

#[cfg(test)]
impl MockChatClient {
    pub fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl ChatClient for MockChatClient {
    async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse> {
        self.calls.lock().unwrap().push(req.clone());
        let mut queue = self.responses.lock().unwrap();
        if queue.is_empty() {
            anyhow::bail!("MockChatClient exhausted — add more canned responses");
        }
        Ok(queue.remove(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_returns_canned_in_order_and_records_calls() {
        let mock = MockChatClient::new(vec![
            ChatResponse {
                text: "first".into(),
                prompt_tokens: 1,
                completion_tokens: 2,
                cost_usd: None,
                provider_id: None,
            },
            ChatResponse {
                text: "second".into(),
                prompt_tokens: 3,
                completion_tokens: 4,
                cost_usd: None,
                provider_id: None,
            },
        ]);

        let req = ChatRequest {
            model: "anthropic/claude-haiku-4-5".into(),
            prompt: "hi".into(),
            system_prompt: None,
            temperature: 0.7,
            max_tokens: 128,
        };

        let r1 = mock.chat(&req).await.unwrap();
        assert_eq!(r1.text, "first");
        let r2 = mock.chat(&req).await.unwrap();
        assert_eq!(r2.text, "second");

        // Exhausted → error.
        assert!(mock.chat(&req).await.is_err());

        assert_eq!(mock.calls.lock().unwrap().len(), 3);
    }
}
