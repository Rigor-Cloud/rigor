use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value as Json;

use super::ctx::ConversationCtx;

// ---------------------------------------------------------------------------
// SseChunk
// ---------------------------------------------------------------------------

/// Wraps a raw SSE `data:` line.
#[derive(Debug, Clone)]
pub struct SseChunk {
    pub data: String,
}

// ---------------------------------------------------------------------------
// FilterError
// ---------------------------------------------------------------------------

/// Errors produced by filter operations.
#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("filter `{filter}` blocked the request: {reason}")]
    Blocked { filter: String, reason: String },

    #[error("filter `{filter}` encountered an error: {reason}")]
    Internal { filter: String, reason: String },
}

// ---------------------------------------------------------------------------
// EgressFilter trait
// ---------------------------------------------------------------------------

/// A single filter in the egress pipeline.
///
/// Filters are applied in *onion* order:
/// - **request**:  outer -> inner  (index 0 first)
/// - **response**: inner -> outer  (last index first)
#[async_trait]
pub trait EgressFilter: Send + Sync {
    /// Human-readable name used in error messages and tracing.
    fn name(&self) -> &'static str;

    /// Mutate / inspect the outgoing request body.
    /// Return `Err(FilterError::Blocked { .. })` to reject the request.
    async fn apply_request(
        &self,
        body: &mut Json,
        ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError>;

    /// Mutate / inspect a single SSE response chunk.
    /// Default implementation is a no-op pass-through.
    async fn apply_response_chunk(
        &self,
        _chunk: &mut SseChunk,
        _ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> {
        Ok(())
    }

    /// Called once after the response stream ends.
    /// May return extra synthetic chunks to append.
    async fn finalize_response(
        &self,
        _ctx: &mut ConversationCtx,
    ) -> Result<Vec<SseChunk>, FilterError> {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------
// FilterChain
// ---------------------------------------------------------------------------

/// An ordered collection of [`EgressFilter`]s applied in onion order.
///
/// Cloned per-request so filters behind `Arc` are shared cheaply.
#[derive(Clone)]
pub struct FilterChain {
    filters: Vec<Arc<dyn EgressFilter>>,
}

impl FilterChain {
    /// Create a chain from an ordered list of filters (outer-first).
    pub fn new(filters: Vec<Arc<dyn EgressFilter>>) -> Self {
        Self { filters }
    }

    /// Create an empty (pass-through) chain.
    pub fn empty() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// Returns `true` when the chain contains no filters.
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// Number of filters in the chain.
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    /// Apply all filters to the outgoing request body (outer -> inner).
    ///
    /// Fail-closed: the first error aborts the chain and the request is rejected.
    pub async fn apply_request(
        &self,
        body: &mut Json,
        ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> {
        for f in &self.filters {
            f.apply_request(body, ctx).await?;
        }
        Ok(())
    }

    /// Apply all filters to a response chunk (inner -> outer, i.e. reverse).
    ///
    /// Best-effort: errors are logged but the chunk continues through the chain.
    pub async fn apply_response_chunk(
        &self,
        chunk: &mut SseChunk,
        ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> {
        for f in self.filters.iter().rev() {
            if let Err(e) = f.apply_response_chunk(chunk, ctx).await {
                tracing::warn!(
                    filter = f.name(),
                    error = %e,
                    "response chunk filter error (continuing)"
                );
            }
        }
        Ok(())
    }

    /// Finalize the response by calling each filter in reverse order.
    ///
    /// Collects any extra chunks the filters want to append.
    pub async fn finalize_response(
        &self,
        ctx: &mut ConversationCtx,
    ) -> Result<Vec<SseChunk>, FilterError> {
        let mut extra = Vec::new();
        for f in self.filters.iter().rev() {
            match f.finalize_response(ctx).await {
                Ok(chunks) => extra.extend(chunks),
                Err(e) => {
                    tracing::warn!(
                        filter = f.name(),
                        error = %e,
                        "finalize_response filter error (continuing)"
                    );
                }
            }
        }
        Ok(extra)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    /// Records the global call order via an AtomicU32 counter.
    struct OrderTracker {
        label: &'static str,
        request_order: AtomicU32,
        response_order: AtomicU32,
        counter: Arc<AtomicU32>,
    }

    impl OrderTracker {
        fn new(label: &'static str, counter: Arc<AtomicU32>) -> Self {
            Self {
                label,
                request_order: AtomicU32::new(0),
                response_order: AtomicU32::new(0),
                counter,
            }
        }

        fn request_seq(&self) -> u32 {
            self.request_order.load(Ordering::SeqCst)
        }

        fn response_seq(&self) -> u32 {
            self.response_order.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl EgressFilter for OrderTracker {
        fn name(&self) -> &'static str {
            self.label
        }

        async fn apply_request(
            &self,
            _body: &mut Json,
            _ctx: &mut ConversationCtx,
        ) -> Result<(), FilterError> {
            let seq = self.counter.fetch_add(1, Ordering::SeqCst);
            self.request_order.store(seq, Ordering::SeqCst);
            Ok(())
        }

        async fn apply_response_chunk(
            &self,
            _chunk: &mut SseChunk,
            _ctx: &mut ConversationCtx,
        ) -> Result<(), FilterError> {
            let seq = self.counter.fetch_add(1, Ordering::SeqCst);
            self.response_order.store(seq, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Appends a label string to a JSON array at `$.tags`.
    struct BodyMutator {
        label: &'static str,
    }

    #[async_trait]
    impl EgressFilter for BodyMutator {
        fn name(&self) -> &'static str {
            self.label
        }

        async fn apply_request(
            &self,
            body: &mut Json,
            _ctx: &mut ConversationCtx,
        ) -> Result<(), FilterError> {
            if let Some(arr) = body.get_mut("tags").and_then(|v| v.as_array_mut()) {
                arr.push(json!(self.label));
            }
            Ok(())
        }
    }

    /// Always blocks with `FilterError::Blocked`.
    struct BlockingFilter;

    #[async_trait]
    impl EgressFilter for BlockingFilter {
        fn name(&self) -> &'static str {
            "blocker"
        }

        async fn apply_request(
            &self,
            _body: &mut Json,
            _ctx: &mut ConversationCtx,
        ) -> Result<(), FilterError> {
            Err(FilterError::Blocked {
                filter: "blocker".into(),
                reason: "always blocked".into(),
            })
        }
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn request_runs_outer_to_inner() {
        let counter = Arc::new(AtomicU32::new(0));
        let f1 = Arc::new(OrderTracker::new("f1", Arc::clone(&counter)));
        let f2 = Arc::new(OrderTracker::new("f2", Arc::clone(&counter)));

        let f1_ref = Arc::clone(&f1);
        let f2_ref = Arc::clone(&f2);

        let chain = FilterChain::new(vec![
            f1 as Arc<dyn EgressFilter>,
            f2 as Arc<dyn EgressFilter>,
        ]);

        let mut body = json!({});
        let mut ctx = ConversationCtx::new_anonymous();
        chain.apply_request(&mut body, &mut ctx).await.unwrap();

        assert!(
            f1_ref.request_seq() < f2_ref.request_seq(),
            "f1 (seq={}) should run before f2 (seq={})",
            f1_ref.request_seq(),
            f2_ref.request_seq(),
        );
    }

    #[tokio::test]
    async fn response_runs_inner_to_outer() {
        let counter = Arc::new(AtomicU32::new(0));
        let f1 = Arc::new(OrderTracker::new("f1", Arc::clone(&counter)));
        let f2 = Arc::new(OrderTracker::new("f2", Arc::clone(&counter)));

        let f1_ref = Arc::clone(&f1);
        let f2_ref = Arc::clone(&f2);

        let chain = FilterChain::new(vec![
            f1 as Arc<dyn EgressFilter>,
            f2 as Arc<dyn EgressFilter>,
        ]);

        let mut chunk = SseChunk {
            data: "hello".into(),
        };
        let mut ctx = ConversationCtx::new_anonymous();
        chain
            .apply_response_chunk(&mut chunk, &mut ctx)
            .await
            .unwrap();

        // Reverse order: f2 runs first (inner), then f1 (outer).
        assert!(
            f2_ref.response_seq() < f1_ref.response_seq(),
            "f2 (seq={}) should run before f1 (seq={}) on response",
            f2_ref.response_seq(),
            f1_ref.response_seq(),
        );
    }

    #[tokio::test]
    async fn request_error_stops_chain() {
        let mutator = Arc::new(BodyMutator {
            label: "should_not_run",
        });

        let chain = FilterChain::new(vec![
            Arc::new(BlockingFilter) as Arc<dyn EgressFilter>,
            mutator as Arc<dyn EgressFilter>,
        ]);

        let mut body = json!({"tags": []});
        let mut ctx = ConversationCtx::new_anonymous();

        let result = chain.apply_request(&mut body, &mut ctx).await;
        assert!(result.is_err(), "chain should return an error");

        let tags = body["tags"].as_array().unwrap();
        assert!(
            tags.is_empty(),
            "body should be unchanged because the chain was short-circuited"
        );
    }

    #[tokio::test]
    async fn body_mutation_accumulates_across_filters() {
        let chain = FilterChain::new(vec![
            Arc::new(BodyMutator { label: "first" }) as Arc<dyn EgressFilter>,
            Arc::new(BodyMutator { label: "second" }) as Arc<dyn EgressFilter>,
        ]);

        let mut body = json!({"tags": []});
        let mut ctx = ConversationCtx::new_anonymous();
        chain.apply_request(&mut body, &mut ctx).await.unwrap();

        let tags = body["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], json!("first"));
        assert_eq!(tags[1], json!("second"));
    }

    #[tokio::test]
    async fn empty_chain_is_passthrough() {
        let chain = FilterChain::empty();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);

        let mut body = json!({"model": "test", "messages": []});
        let original = body.clone();
        let mut ctx = ConversationCtx::new_anonymous();

        chain.apply_request(&mut body, &mut ctx).await.unwrap();
        assert_eq!(
            body, original,
            "body should be unchanged through empty chain"
        );
    }
}
