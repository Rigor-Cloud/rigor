#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! Integration tests for the egress filter chain.

use async_trait::async_trait;
use serde_json::Value as Json;
use std::sync::atomic::{AtomicUsize, Ordering};
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

// ===========================================================================
// §5.6 "0F" frozen-prefix invariant tests
// ===========================================================================

/// Seals a FrozenPrefix with message_count=1 at apply_request time.
struct Sealer {
    freeze_count: usize,
}

#[async_trait]
impl EgressFilter for Sealer {
    fn name(&self) -> &'static str {
        "sealer"
    }
    async fn apply_request(
        &self,
        body: &mut Json,
        ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> {
        let msgs: Vec<Json> = body
            .get("messages")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        set_frozen_prefix(ctx, &msgs, self.freeze_count);
        Ok(())
    }
}

/// Mutates messages[0]["content"] — ILLEGAL once Sealer(1) has run.
struct FirstMessageMutator;

#[async_trait]
impl EgressFilter for FirstMessageMutator {
    fn name(&self) -> &'static str {
        "first_msg_mutator"
    }
    async fn apply_request(
        &self,
        body: &mut Json,
        _ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> {
        if let Some(arr) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
            if let Some(first) = arr.get_mut(0) {
                first["content"] = serde_json::json!("HIJACKED");
            }
        }
        Ok(())
    }
}

/// Test 1 (from 01-CONTEXT.md §specifics):
///   New test filter that mutates messages[0] after set_frozen_prefix(count=1)
///   → expect apply_request to fail.
///
/// Gated on release-build semantics (debug build panics; see the debug twin
/// test `frozen_prefix_violation_panics_in_debug` below).
#[cfg(not(debug_assertions))]
#[tokio::test]
async fn frozen_prefix_violation_rejects_request_in_release() {
    let chain = FilterChain::new(vec![
        Arc::new(Sealer { freeze_count: 1 }),
        Arc::new(FirstMessageMutator),
    ]);
    let mut body = serde_json::json!({
        "messages": [
            {"role": "user", "content": "original-system-prompt"},
            {"role": "user", "content": "turn-1"}
        ]
    });
    let mut ctx = ConversationCtx::new_anonymous();
    let err = chain
        .apply_request(&mut body, &mut ctx)
        .await
        .expect_err("frozen-prefix mutation must surface as FilterError in release");
    match err {
        FilterError::Internal { filter, reason } => {
            assert_eq!(filter, "frozen_prefix");
            assert!(
                reason.contains("checksum mismatch"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

/// Debug-build companion: panic is expected (matches chain.rs behavior).
#[cfg(debug_assertions)]
#[tokio::test]
#[should_panic(expected = "frozen-prefix invariant violated")]
async fn frozen_prefix_violation_panics_in_debug() {
    let chain = FilterChain::new(vec![
        Arc::new(Sealer { freeze_count: 1 }),
        Arc::new(FirstMessageMutator),
    ]);
    let mut body = serde_json::json!({
        "messages": [
            {"role": "user", "content": "original-system-prompt"},
            {"role": "user", "content": "turn-1"}
        ]
    });
    let mut ctx = ConversationCtx::new_anonymous();
    let _ = chain.apply_request(&mut body, &mut ctx).await;
}

// ===========================================================================
// §5.7 "0G" response-side chain tests
// ===========================================================================

/// Counts how many times apply_response_chunk is invoked.
struct CountingChunkFilter {
    chunk_count: Arc<AtomicUsize>,
}

#[async_trait]
impl EgressFilter for CountingChunkFilter {
    fn name(&self) -> &'static str {
        "counting_chunk"
    }
    async fn apply_request(
        &self,
        _body: &mut Json,
        _ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> {
        Ok(())
    }
    async fn apply_response_chunk(
        &self,
        _chunk: &mut SseChunk,
        _ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> {
        self.chunk_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// Test 2 (from 01-CONTEXT.md §specifics):
///   New test filter with apply_response_chunk that counts chunks
///   → assert chain invoked N times.
#[tokio::test]
async fn response_chunk_filter_is_invoked_per_chunk() {
    let counter = Arc::new(AtomicUsize::new(0));
    let chain = FilterChain::new(vec![Arc::new(CountingChunkFilter {
        chunk_count: Arc::clone(&counter),
    })]);
    let mut ctx = ConversationCtx::new_anonymous();

    let raw_chunks = [
        "data: {\"delta\":{\"text\":\"hello\"}}\n\n",
        "data: {\"delta\":{\"text\":\" world\"}}\n\n",
        "data: [DONE]\n\n",
    ];

    for raw in raw_chunks.iter() {
        let mut chunk = SseChunk {
            data: (*raw).to_string(),
        };
        chain
            .apply_response_chunk(&mut chunk, &mut ctx)
            .await
            .expect("apply_response_chunk must not error");
    }

    assert_eq!(
        counter.load(Ordering::SeqCst),
        raw_chunks.len(),
        "counting filter should have been invoked once per chunk"
    );
}

/// Emits two synthetic SseChunks during finalize_response.
struct FinalizeEmitter {
    extra: Vec<String>,
}

#[async_trait]
impl EgressFilter for FinalizeEmitter {
    fn name(&self) -> &'static str {
        "finalize_emitter"
    }
    async fn apply_request(
        &self,
        _body: &mut Json,
        _ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> {
        Ok(())
    }
    async fn finalize_response(
        &self,
        _ctx: &mut ConversationCtx,
    ) -> Result<Vec<SseChunk>, FilterError> {
        Ok(self
            .extra
            .iter()
            .cloned()
            .map(|data| SseChunk { data })
            .collect())
    }
}

/// Test 3 (from 01-CONTEXT.md §specifics):
///   finalize_response returns extra SSE chunks → assert they're forwarded.
/// "Forwarded" here means the FilterChain returns them to the caller
/// (the proxy.rs wiring from Plan 03 in turn sends them to the client).
#[tokio::test]
async fn finalize_response_extra_chunks_are_returned() {
    let chain = FilterChain::new(vec![Arc::new(FinalizeEmitter {
        extra: vec![
            "data: {\"type\":\"rigor.annotation\",\"note\":\"hello\"}\n\n".to_string(),
            "data: [RIGOR-END]\n\n".to_string(),
        ],
    })]);
    let mut ctx = ConversationCtx::new_anonymous();
    let extras = chain
        .finalize_response(&mut ctx)
        .await
        .expect("finalize_response must succeed");
    assert_eq!(extras.len(), 2, "both extras must be forwarded");
    assert!(extras[0].data.contains("rigor.annotation"));
    assert!(extras[1].data.contains("[RIGOR-END]"));
}
