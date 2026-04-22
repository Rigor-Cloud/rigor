use async_trait::async_trait;
use serde_json::Value as Json;
use tracing::info;

use super::chain::{EgressFilter, FilterError};
use super::ctx::ConversationCtx;

// ---------------------------------------------------------------------------
// ClaimInjectionFilter
// ---------------------------------------------------------------------------

/// Egress filter that injects epistemic context (rigor constraints) into
/// outgoing LLM requests. Wraps the inject logic previously inlined in
/// `proxy.rs`.
pub struct ClaimInjectionFilter {
    pub epistemic_context: String,
    pub api_path: String,
}

impl ClaimInjectionFilter {
    pub fn new(epistemic_context: String, api_path: String) -> Self {
        Self {
            epistemic_context,
            api_path,
        }
    }
}

#[async_trait]
impl EgressFilter for ClaimInjectionFilter {
    fn name(&self) -> &'static str {
        "claim_injection"
    }

    async fn apply_request(
        &self,
        body: &mut Json,
        _ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> {
        if self.epistemic_context.is_empty() {
            return Ok(());
        }

        if self.api_path.contains("messages") {
            inject_anthropic_context(body, &self.epistemic_context);
            info!(
                filter = "claim_injection",
                provider = "anthropic",
                "injected epistemic context into Anthropic request"
            );
        } else if self.api_path.contains("chat/completions") {
            inject_openai_context(body, &self.epistemic_context);
            info!(
                filter = "claim_injection",
                provider = "openai",
                "injected epistemic context into OpenAI request"
            );
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Inject helpers (ported from proxy.rs)
// ---------------------------------------------------------------------------

fn inject_anthropic_context(body: &mut Json, context: &str) {
    match body.get("system") {
        Some(Json::String(existing)) => {
            body["system"] = Json::String(format!("{}\n{}", existing, context));
        }
        Some(Json::Array(blocks)) => {
            // System is an array of content blocks
            let mut new_blocks = blocks.clone();
            new_blocks.push(serde_json::json!({
                "type": "text",
                "text": context
            }));
            body["system"] = Json::Array(new_blocks);
        }
        _ => {
            // No system prompt -- add one
            body["system"] = Json::String(context.to_string());
        }
    }
}

fn inject_openai_context(body: &mut Json, context: &str) {
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        let has_system = messages
            .iter()
            .any(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"));

        if has_system {
            // Append to existing system message
            for msg in messages.iter_mut() {
                if msg.get("role").and_then(|r| r.as_str()) == Some("system") {
                    if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                        msg["content"] = Json::String(format!("{}\n{}", content, context));
                    }
                    break;
                }
            }
        } else {
            // Prepend system message
            messages.insert(
                0,
                serde_json::json!({
                    "role": "system",
                    "content": context
                }),
            );
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const RIGOR_CTX: &str = "[RIGOR] You must not hallucinate.";

    fn anthropic_filter(ctx: &str) -> ClaimInjectionFilter {
        ClaimInjectionFilter::new(ctx.to_string(), "/v1/messages".to_string())
    }

    fn openai_filter(ctx: &str) -> ClaimInjectionFilter {
        ClaimInjectionFilter::new(ctx.to_string(), "/v1/chat/completions".to_string())
    }

    // -----------------------------------------------------------------------
    // Anthropic
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn anthropic_injects_into_string_system() {
        let filter = anthropic_filter(RIGOR_CTX);
        let mut body = json!({
            "model": "claude-sonnet-4-20250514",
            "system": "You are helpful.",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let mut ctx = ConversationCtx::new_anonymous();

        filter.apply_request(&mut body, &mut ctx).await.unwrap();

        let system = body["system"].as_str().unwrap();
        assert!(
            system.contains("You are helpful."),
            "original system prompt should be preserved"
        );
        assert!(
            system.contains(RIGOR_CTX),
            "rigor context should be appended"
        );
    }

    #[tokio::test]
    async fn anthropic_injects_into_array_system() {
        let filter = anthropic_filter(RIGOR_CTX);
        let mut body = json!({
            "model": "claude-sonnet-4-20250514",
            "system": [{"type": "text", "text": "existing"}],
            "messages": [{"role": "user", "content": "hi"}]
        });
        let mut ctx = ConversationCtx::new_anonymous();

        filter.apply_request(&mut body, &mut ctx).await.unwrap();

        let system = body["system"].as_array().unwrap();
        assert_eq!(
            system.len(),
            2,
            "array should have 2 elements after injection"
        );
        assert_eq!(system[0]["text"], "existing");
        assert_eq!(system[1]["text"], RIGOR_CTX);
        assert_eq!(system[1]["type"], "text");
    }

    #[tokio::test]
    async fn anthropic_creates_system_when_absent() {
        let filter = anthropic_filter(RIGOR_CTX);
        let mut body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let mut ctx = ConversationCtx::new_anonymous();

        filter.apply_request(&mut body, &mut ctx).await.unwrap();

        let system = body["system"].as_str().unwrap();
        assert_eq!(system, RIGOR_CTX);
    }

    // -----------------------------------------------------------------------
    // OpenAI
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn openai_appends_to_existing_system_message() {
        let filter = openai_filter(RIGOR_CTX);
        let mut body = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "hi"}
            ]
        });
        let mut ctx = ConversationCtx::new_anonymous();

        filter.apply_request(&mut body, &mut ctx).await.unwrap();

        let messages = body["messages"].as_array().unwrap();
        let system_content = messages[0]["content"].as_str().unwrap();
        assert!(
            system_content.contains("You are helpful."),
            "original content should be preserved"
        );
        assert!(
            system_content.contains(RIGOR_CTX),
            "rigor context should be appended"
        );
    }

    #[tokio::test]
    async fn openai_creates_system_message_when_absent() {
        let filter = openai_filter(RIGOR_CTX);
        let mut body = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "hi"}
            ]
        });
        let mut ctx = ConversationCtx::new_anonymous();

        filter.apply_request(&mut body, &mut ctx).await.unwrap();

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2, "should have 2 messages after injection");
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], RIGOR_CTX);
        assert_eq!(messages[1]["role"], "user");
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn empty_context_is_noop() {
        let filter = ClaimInjectionFilter::new(String::new(), "/v1/messages".to_string());
        let mut body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let original = body.clone();
        let mut ctx = ConversationCtx::new_anonymous();

        filter.apply_request(&mut body, &mut ctx).await.unwrap();

        assert_eq!(
            body, original,
            "body should be unchanged when context is empty"
        );
    }

    #[tokio::test]
    async fn name_returns_claim_injection() {
        let filter = anthropic_filter(RIGOR_CTX);
        assert_eq!(filter.name(), "claim_injection");
    }
}
