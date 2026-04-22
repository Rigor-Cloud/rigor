//! Integration tests for the egress filter chain.

use async_trait::async_trait;
use serde_json::Value as Json;
use std::sync::Arc;

use rigor::daemon::egress::*;

/// Test 1: ClaimInjectionFilter composes with a custom filter in a chain.
/// Verifies that filters run in order and mutations accumulate.
///
/// The filter below uppercases all string "content" fields in messages[].
struct UppercaseFilter;

#[async_trait]
impl EgressFilter for UppercaseFilter {
    fn name(&self) -> &'static str {
        "uppercase"
    }

    async fn apply_request(
        &self,
        body: &mut Json,
        _ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> {
        if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
            for msg in messages {
                if let Some(content) = msg
                    .get_mut("content")
                    .and_then(|c| c.as_str().map(|s| s.to_uppercase()))
                {
                    msg["content"] = Json::String(content);
                }
            }
        }
        Ok(())
    }
}

#[tokio::test]
async fn claim_injection_plus_custom_filter_compose() {
    let chain = FilterChain::new(vec![
        Arc::new(ClaimInjectionFilter::new(
            "rigor says: be careful".to_string(),
            "/v1/messages".to_string(),
        )),
        Arc::new(UppercaseFilter),
    ]);

    let mut body = serde_json::json!({
        "system": "You are helpful.",
        "messages": [{"role": "user", "content": "hello world"}]
    });
    let mut ctx = ConversationCtx::new_anonymous();

    chain.apply_request(&mut body, &mut ctx).await.unwrap();

    // Claim injection ran first: system prompt has rigor context
    let system = body["system"].as_str().unwrap();
    assert!(system.contains("rigor says: be careful"));

    // Uppercase filter ran second: message content is uppercased
    let content = body["messages"][0]["content"].as_str().unwrap();
    assert_eq!(content, "HELLO WORLD");
}

/// Test 2: Filters can pass state via ConversationCtx scratch.
#[tokio::test]
async fn filter_chain_with_ctx_scratch_passes_state() {
    #[derive(Debug)]
    struct SharedData(String);

    struct WriterFilter;
    struct ReaderFilter;

    #[async_trait]
    impl EgressFilter for WriterFilter {
        fn name(&self) -> &'static str {
            "writer"
        }
        async fn apply_request(
            &self,
            _body: &mut Json,
            ctx: &mut ConversationCtx,
        ) -> Result<(), FilterError> {
            ctx.scratch_set(SharedData("from_writer".to_string()));
            Ok(())
        }
    }

    #[async_trait]
    impl EgressFilter for ReaderFilter {
        fn name(&self) -> &'static str {
            "reader"
        }
        async fn apply_request(
            &self,
            body: &mut Json,
            ctx: &mut ConversationCtx,
        ) -> Result<(), FilterError> {
            if let Some(data) = ctx.scratch_get::<SharedData>() {
                body["scratch_value"] = Json::String(data.0.clone());
            }
            Ok(())
        }
    }

    let chain = FilterChain::new(vec![Arc::new(WriterFilter), Arc::new(ReaderFilter)]);

    let mut body = serde_json::json!({});
    let mut ctx = ConversationCtx::new_anonymous();
    chain.apply_request(&mut body, &mut ctx).await.unwrap();

    assert_eq!(body["scratch_value"].as_str().unwrap(), "from_writer");
}
