//! Phase 1 (PR-3) — FilterChain + frozen-prefix criterion baselines.
//!
//! Captures "before" numbers for three primitives shipped in Phase 1:
//!
//! 1. `compute_checksum` (`crates/rigor/src/daemon/egress/frozen.rs`,
//!    added by Plan 01-01) across three message-array sizes — 10 / 100 /
//!    1000. This is the xxhash64-over-canonical-JSON cost that runs once
//!    per request at `set_frozen_prefix` and once per request at
//!    `verify_frozen_prefix` (wired into `FilterChain::apply_request` by
//!    Plan 01-02).
//!
//! 2. `FilterChain::apply_response_chunk` in two configurations — a
//!    zero-filter chain (pure infrastructure cost: one reverse-iterator
//!    pass over an empty `Vec<Arc<dyn EgressFilter>>`) and a one-filter
//!    chain carrying a default-method no-op filter. This is the
//!    per-SSE-chunk cost introduced by Plan 01-03's wiring into
//!    `proxy.rs`; every streaming response pays it.
//!
//! 3. `FilterChain::finalize_response` in the same two configurations so
//!    the post-stream hook cost is captured too.
//!
//! Out of scope — intentional: no regression thresholds, no hash-algo
//! comparison (twox-hash is locked by CONTEXT.md §decisions), no
//! full-proxy E2E bench (that belongs to Phase 9/11/12/13 coverage work),
//! and no `verify_frozen_prefix` bench — it is `compute_checksum` plus an
//! equality check, so `compute_checksum` already represents its cost.
//!
//! Regression enforcement lands in Phase 17 (issue #13 / REQ-032) on top
//! of whatever baselines this bench writes to `target/criterion/`.

use std::sync::Arc;

use async_trait::async_trait;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use serde_json::{json, Value as Json};
use tokio::runtime::Runtime;

use rigor::daemon::egress::{
    compute_checksum, ConversationCtx, EgressFilter, FilterChain, FilterError, SseChunk,
};

// ---------------------------------------------------------------------------
// Harness helpers
// ---------------------------------------------------------------------------

/// A filter that uses every `EgressFilter` default body — i.e. every method
/// is a trivial `Ok(())` / `Ok(vec![])`. Represents the cheapest possible
/// per-filter overhead the chain can incur.
struct NoOpFilter;

#[async_trait]
impl EgressFilter for NoOpFilter {
    fn name(&self) -> &'static str {
        "noop"
    }

    // Default-body `apply_request` is NOT supplied by the trait — the trait
    // only defaults `apply_response_chunk` and `finalize_response`. We
    // supply a trivial `Ok(())` for `apply_request` since this bench never
    // exercises the request path. Keeping it present means the chain is a
    // valid production artifact that Plan 01-02's wiring could accept
    // verbatim.
    async fn apply_request(
        &self,
        _body: &mut Json,
        _ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> {
        Ok(())
    }
}

/// Build a realistic `Vec<Value>` of `n` chat messages. Each message is
/// ~200 bytes of JSON with a fixed-content string so the hash is stable
/// across runs (criterion reruns the workload many times per sample). We
/// alternate `role` between user/assistant so `serde_json::to_vec` spends
/// representative time on both variants.
fn build_messages(n: usize) -> Vec<Json> {
    // Fixed ~200-char body. Length stays constant; only the role flips.
    const SAMPLE: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
        Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
        Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.";
    (0..n)
        .map(|i| {
            let role = if i % 2 == 0 { "user" } else { "assistant" };
            json!({ "role": role, "content": SAMPLE })
        })
        .collect()
}

/// Single-threaded tokio runtime reused across a bench group. `block_on`
/// overhead is constant across samples so it is acceptable inside
/// `b.iter(...)` — it cancels out relative to the measured work.
fn new_runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build current-thread tokio runtime")
}

/// Build an SSE chunk with a realistic `data: {...}` payload.
fn sample_chunk() -> SseChunk {
    SseChunk {
        data: String::from("data: {\"type\":\"chunk\",\"text\":\"hello\"}"),
    }
}

// ---------------------------------------------------------------------------
// 1. compute_checksum — parameterized by message count
// ---------------------------------------------------------------------------

fn bench_compute_checksum(c: &mut Criterion) {
    let mut group = c.benchmark_group("compute_checksum");
    for size in [10usize, 100, 1000] {
        let messages = build_messages(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &messages, |b, msgs| {
            b.iter(|| {
                let h = compute_checksum(black_box(msgs));
                black_box(h);
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// 2. FilterChain::apply_response_chunk — zero_filters + one_filter
// ---------------------------------------------------------------------------

fn bench_apply_response_chunk(c: &mut Criterion) {
    let mut group = c.benchmark_group("apply_response_chunk");
    let rt = new_runtime();

    // zero_filters — empty chain, pure infrastructure cost
    {
        let chain = FilterChain::new(Vec::new());
        group.bench_function(BenchmarkId::from_parameter("zero_filters"), |b| {
            b.iter(|| {
                rt.block_on(async {
                    let mut chunk = sample_chunk();
                    let mut ctx = ConversationCtx::new_anonymous();
                    chain
                        .apply_response_chunk(black_box(&mut chunk), black_box(&mut ctx))
                        .await
                        .expect("chain is best-effort Ok");
                    black_box(chunk);
                });
            });
        });
    }

    // one_filter — chain carrying a single NoOpFilter
    {
        let filter: Arc<dyn EgressFilter> = Arc::new(NoOpFilter);
        let chain = FilterChain::new(vec![filter]);
        group.bench_function(BenchmarkId::from_parameter("one_filter"), |b| {
            b.iter(|| {
                rt.block_on(async {
                    let mut chunk = sample_chunk();
                    let mut ctx = ConversationCtx::new_anonymous();
                    chain
                        .apply_response_chunk(black_box(&mut chunk), black_box(&mut ctx))
                        .await
                        .expect("chain is best-effort Ok");
                    black_box(chunk);
                });
            });
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// 3. FilterChain::finalize_response — zero_filters + one_filter
// ---------------------------------------------------------------------------

fn bench_finalize_response(c: &mut Criterion) {
    let mut group = c.benchmark_group("finalize_response");
    let rt = new_runtime();

    // zero_filters — empty chain
    {
        let chain = FilterChain::new(Vec::new());
        group.bench_function(BenchmarkId::from_parameter("zero_filters"), |b| {
            b.iter(|| {
                rt.block_on(async {
                    let mut ctx = ConversationCtx::new_anonymous();
                    let extras = chain
                        .finalize_response(black_box(&mut ctx))
                        .await
                        .expect("chain is best-effort Ok");
                    black_box(extras);
                });
            });
        });
    }

    // one_filter — chain carrying a single NoOpFilter
    {
        let filter: Arc<dyn EgressFilter> = Arc::new(NoOpFilter);
        let chain = FilterChain::new(vec![filter]);
        group.bench_function(BenchmarkId::from_parameter("one_filter"), |b| {
            b.iter(|| {
                rt.block_on(async {
                    let mut ctx = ConversationCtx::new_anonymous();
                    let extras = chain
                        .finalize_response(black_box(&mut ctx))
                        .await
                        .expect("chain is best-effort Ok");
                    black_box(extras);
                });
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_compute_checksum,
    bench_apply_response_chunk,
    bench_finalize_response
);
criterion_main!(benches);
