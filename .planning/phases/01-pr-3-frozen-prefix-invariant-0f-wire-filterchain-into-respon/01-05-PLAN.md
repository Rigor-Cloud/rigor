---
phase: 01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon
plan: 05
type: execute
wave: 3
depends_on: [01, 02, 03]
files_modified:
  - crates/rigor/Cargo.toml
  - crates/rigor/benches/filter_chain_overhead.rs
autonomous: true
requirements: [REQ-001, REQ-002]

must_haves:
  truths:
    - "cargo bench --bench filter_chain_overhead --no-run exits 0 (bench compiles against the shipped public API)"
    - "cargo bench --bench filter_chain_overhead -- --output-format bencher exits 0 and produces exactly 7 measurement rows"
    - "Baseline data is written under target/criterion/ for compute_checksum/{10,100,1000}, apply_response_chunk/{zero_filters,one_filter}, and finalize_response/{zero_filters,one_filter}"
    - "cargo fmt -- --check and cargo clippy --benches --all-features -- -D warnings pass with the new bench in place"
    - "No existing bench file (hook_latency.rs, evaluation_only.rs, dfquad_scaling.rs) is modified; no regression in their behavior"
    - "No regression thresholds are introduced — this plan only establishes baselines; enforcement lands in Phase 17 (issue #13, REQ-032)"
  artifacts:
    - path: "crates/rigor/benches/filter_chain_overhead.rs"
      provides: "Criterion benchmarks for compute_checksum (sizes 10/100/1000), FilterChain::apply_response_chunk (zero/one filter), FilterChain::finalize_response (zero/one filter)"
      contains: "criterion_group!"
      min_lines: 110
    - path: "crates/rigor/Cargo.toml"
      provides: "[[bench]] entry registering filter_chain_overhead with harness = false"
      contains: "filter_chain_overhead"
  key_links:
    - from: "crates/rigor/benches/filter_chain_overhead.rs"
      to: "crates/rigor/src/daemon/egress/frozen.rs"
      via: "rigor::daemon::egress::compute_checksum re-exported through egress/mod.rs `pub use frozen::*;`"
      pattern: "compute_checksum"
    - from: "crates/rigor/benches/filter_chain_overhead.rs"
      to: "crates/rigor/src/daemon/egress/chain.rs"
      via: "FilterChain::new / apply_response_chunk / finalize_response + SseChunk, EgressFilter trait"
      pattern: "FilterChain::new"
    - from: "crates/rigor/benches/filter_chain_overhead.rs"
      to: "crates/rigor/src/daemon/egress/ctx.rs"
      via: "ConversationCtx::new_anonymous harness factory"
      pattern: "ConversationCtx::new_anonymous"
    - from: "crates/rigor/Cargo.toml"
      to: "crates/rigor/benches/filter_chain_overhead.rs"
      via: "[[bench]] harness = false entry"
      pattern: "name = \"filter_chain_overhead\""
---

<objective>
Add criterion baseline benchmarks for the Phase 1 (PR-3) egress primitives so
Phase 17 (issue #13 / REQ-032 "bench-regression gate, fail on >20% regression
for the evaluator hot path") has meaningful "before" data when it lands.

Purpose:
- Measure `compute_checksum` (frozen.rs, added by Plan 01-01) across three
  realistic message-array sizes so the xxhash64-over-canonical-JSON cost is
  captured as a function of prompt size. This is the hot path that runs once
  per request (at `set_frozen_prefix`) and once more per request (at
  `verify_frozen_prefix` inside `FilterChain::apply_request`, wired by Plan
  01-02). If hash cost ever grows super-linearly in messages, this bench will
  catch it.
- Measure `FilterChain::apply_response_chunk` overhead in the two shipped
  configurations — `FilterChain::new(vec![])` (zero filters — pure chain
  infrastructure cost: an empty rev iterator) and a one-filter chain with a
  default-method `NoOpFilter`. This is the per-SSE-chunk cost introduced by
  Plan 01-03's wiring into proxy.rs; today every in-flight streaming response
  will pay it.
- Measure `FilterChain::finalize_response` in the same two configurations so
  the post-stream hook cost is also captured.

Output:
- A new `crates/rigor/benches/filter_chain_overhead.rs` file containing three
  criterion groups (`compute_checksum`, `apply_response_chunk`,
  `finalize_response`) wired via `criterion_group!` + `criterion_main!`.
- A new `[[bench]]` entry in `crates/rigor/Cargo.toml` immediately after the
  existing `dfquad_scaling` entry at lines 86-88. No new dev-dependency
  (criterion 0.5 already present at Cargo.toml:76).
- Baseline data written to `target/criterion/` on first run. No regression
  threshold is introduced in this plan — enforcement is Phase 17's job.

This plan is additive. It MUST NOT modify `frozen.rs`, `chain.rs`, `ctx.rs`,
`proxy.rs`, or any other production source. It MUST NOT modify
`benches/hook_latency.rs`, `benches/evaluation_only.rs`, or
`benches/dfquad_scaling.rs`.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/STATE.md
@.planning/ROADMAP.md
@.planning/REQUIREMENTS.md
@.planning/phases/01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon/01-CONTEXT.md
@.planning/phases/01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon/01-01-PLAN.md
@.planning/phases/01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon/01-03-PLAN.md
@crates/rigor/Cargo.toml
@crates/rigor/benches/hook_latency.rs
@crates/rigor/benches/dfquad_scaling.rs
@crates/rigor/src/daemon/egress/chain.rs
@crates/rigor/src/daemon/egress/ctx.rs

<interfaces>
<!--
Types / signatures this bench MUST use verbatim. All of these are public today
(via `pub use` in crates/rigor/src/daemon/egress/mod.rs) OR will be public after
Plan 01-01 lands (frozen::* added to the same glob re-export).

Executor DOES NOT need to explore the codebase — these are the final contracts.
-->

From `rigor::daemon::egress` (chain.rs — already shipped on main):
```rust
#[derive(Debug, Clone)]
pub struct SseChunk {
    pub data: String,
}

#[async_trait::async_trait]
pub trait EgressFilter: Send + Sync {
    fn name(&self) -> &'static str;

    async fn apply_request(
        &self,
        body: &mut serde_json::Value,
        ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError>;

    // Default impl = Ok(()) pass-through — NoOpFilter needs nothing else.
    async fn apply_response_chunk(
        &self,
        _chunk: &mut SseChunk,
        _ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError> { Ok(()) }

    // Default impl = Ok(vec![]) — NoOpFilter needs nothing else.
    async fn finalize_response(
        &self,
        _ctx: &mut ConversationCtx,
    ) -> Result<Vec<SseChunk>, FilterError> { Ok(vec![]) }
}

#[derive(Clone)]
pub struct FilterChain { /* Vec<Arc<dyn EgressFilter>> */ }

impl FilterChain {
    pub fn new(filters: Vec<std::sync::Arc<dyn EgressFilter>>) -> Self;
    pub fn empty() -> Self;

    pub async fn apply_response_chunk(
        &self,
        chunk: &mut SseChunk,
        ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError>;

    pub async fn finalize_response(
        &self,
        ctx: &mut ConversationCtx,
    ) -> Result<Vec<SseChunk>, FilterError>;
}
```

From `rigor::daemon::egress` (ctx.rs — already shipped on main):
```rust
impl ConversationCtx {
    pub fn new_anonymous() -> Self;  // used as bench harness factory
}
```

From `rigor::daemon::egress` (frozen.rs — added by Plan 01-01, re-exported via
`pub use frozen::*;` in egress/mod.rs per Plan 01-01 Step 2):
```rust
pub fn compute_checksum(messages: &[serde_json::Value]) -> u64;
```

Existing bench pattern in repo (hook_latency.rs + dfquad_scaling.rs):
- Import from `rigor::...` paths (criterion `harness = false`).
- Use `criterion_group!` + `criterion_main!` at bottom.
- Parameterized-size benches use `BenchmarkId::new(name, size)` +
  `group.bench_with_input(...)`; `group.throughput(Throughput::Elements(n))`
  is added where size is meaningful.
- Async criterion is NOT in dev-deps. Any `async fn` call in a bench uses a
  locally-constructed tokio current-thread runtime:
  `tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()`.
  We call `rt.block_on(async { ... })` inside `b.iter(...)`.

Cargo.toml bench entries already follow a uniform shape (lines 78-88):
```toml
[[bench]]
name = "hook_latency"
harness = false

[[bench]]
name = "evaluation_only"
harness = false

[[bench]]
name = "dfquad_scaling"
harness = false
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Register the new bench in Cargo.toml</name>
  <files>crates/rigor/Cargo.toml</files>
  <read_first>
    - crates/rigor/Cargo.toml (lines 74-88 — existing `[dev-dependencies]` and the three `[[bench]]` entries; criterion 0.5 with html_reports is already present at line 76 so NO dep change is needed).
    - crates/rigor/benches/dfquad_scaling.rs (confirm the `harness = false` + `criterion_group! / criterion_main!` shape that this plan copies).
    - .planning/phases/01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon/01-01-PLAN.md §files_modified (confirms frozen.rs + egress/mod.rs `pub use frozen::*;` are in place — this bench depends on that re-export).
    - .planning/phases/01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon/01-03-PLAN.md §interfaces (confirms `SseChunk { data: String }` shape is final).
  </read_first>
  <action>
    Edit `crates/rigor/Cargo.toml`. The file currently ends with three
    `[[bench]]` entries at lines 78-88. APPEND a fourth `[[bench]]` entry
    immediately after the existing `dfquad_scaling` block (i.e. after line 88)
    so the file now ends with FOUR bench entries. The new entry mirrors the
    existing shape exactly — no extra keys.

    Final lines 86-92 of Cargo.toml MUST read:

    ```toml
    [[bench]]
    name = "dfquad_scaling"
    harness = false

    [[bench]]
    name = "filter_chain_overhead"
    harness = false
    ```

    Rationale:
    - `harness = false` matches the established pattern — criterion supplies
      its own main function via `criterion_main!`.
    - No `path = ...` key needed; cargo resolves `benches/filter_chain_overhead.rs`
      by convention, matching how the three existing benches are registered.
    - NO dev-dependency change. criterion = 0.5 already present at line 76.
    - NO `[dependencies]` change. The bench uses only public `rigor::*` paths
      (exercised by existing `crates/rigor/tests/egress_integration.rs:7`
      `use rigor::daemon::egress::*;`) plus criterion from `[dev-dependencies]`.

    Do NOT reorder, rename, or remove any existing `[[bench]]` entry.
    Do NOT add `[features]` or any other new table.
  </action>
  <verify>
    <automated>cargo check -p rigor --benches 2>&amp;1 | tail -10</automated>
  </verify>
  <acceptance_criteria>
    - `grep -c 'name = "filter_chain_overhead"' crates/rigor/Cargo.toml` returns 1
    - `grep -A1 'name = "filter_chain_overhead"' crates/rigor/Cargo.toml | grep -c 'harness = false'` returns 1
    - `grep -c '^\[\[bench\]\]' crates/rigor/Cargo.toml` returns 4 (was 3 before this plan)
    - `grep -c 'name = "hook_latency"' crates/rigor/Cargo.toml` returns 1 (existing entry untouched)
    - `grep -c 'name = "evaluation_only"' crates/rigor/Cargo.toml` returns 1 (existing entry untouched)
    - `grep -c 'name = "dfquad_scaling"' crates/rigor/Cargo.toml` returns 1 (existing entry untouched)
    - `cargo metadata --manifest-path crates/rigor/Cargo.toml --format-version 1 2>/dev/null | grep -c '"filter_chain_overhead"'` returns >= 1
    - At this task-complete checkpoint, `cargo check -p rigor --benches` is ALLOWED to fail with "file not found: benches/filter_chain_overhead.rs" — the file is created in Task 2. Do NOT treat that as a blocker here.
  </acceptance_criteria>
  <done>
    Cargo.toml declares a fourth `[[bench]]` named `filter_chain_overhead` with
    `harness = false`; the three existing bench entries are unchanged; no dep
    additions; `cargo metadata` sees the new bench target.
  </done>
</task>

<task type="auto">
  <name>Task 2: Create the filter_chain_overhead.rs bench with 7 measurements</name>
  <files>crates/rigor/benches/filter_chain_overhead.rs</files>
  <read_first>
    - crates/rigor/benches/hook_latency.rs (full file — shortest existing bench; the shape for `criterion_group! / criterion_main!` + `c.bench_function(...)`).
    - crates/rigor/benches/dfquad_scaling.rs (full file — the shape for parameterized-size benches using `BenchmarkId::new(name, size)` + `group.bench_with_input(...)`).
    - crates/rigor/src/daemon/egress/chain.rs (lines 1-165 — SseChunk, EgressFilter trait with DEFAULT method bodies for `apply_response_chunk` and `finalize_response`, `FilterChain::new`, `apply_response_chunk`, `finalize_response`).
    - crates/rigor/src/daemon/egress/ctx.rs (lines 65-72 — `ConversationCtx::new_anonymous` signature).
    - .planning/phases/01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon/01-01-PLAN.md §tasks (confirms `pub use frozen::*;` is added to egress/mod.rs so `rigor::daemon::egress::compute_checksum` is the reachable path).
    - crates/rigor/tests/egress_integration.rs line 7 (confirms `use rigor::daemon::egress::*;` is the working public import path).
  </read_first>
  <action>
    Create `crates/rigor/benches/filter_chain_overhead.rs` with the EXACT
    content below. This file is the whole deliverable for this task — do not
    abbreviate, do not invent variants, do not add extra measurements.

    ```rust
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
    use criterion::{
        black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput,
    };
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
            group.bench_with_input(
                BenchmarkId::from_parameter(size),
                &messages,
                |b, msgs| {
                    b.iter(|| {
                        let h = compute_checksum(black_box(msgs));
                        black_box(h);
                    });
                },
            );
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
    ```

    Notes for the executor (do NOT need to re-derive these — they are the
    reasoning behind the skeleton above, recorded so you can sanity-check):

    - `async_trait` is a direct dep of `rigor` (Cargo.toml:63) and is therefore
      available inside a bench target via the normal `[dev-dependencies]` +
      `[dependencies]` merge rules. No Cargo.toml addition is needed for it.
    - `tokio` is a direct dep (Cargo.toml:27) with `rt-multi-thread, macros,
      signal`; `new_current_thread()` does NOT require the `rt-multi-thread`
      feature — it is provided by `rt` which is implied by `rt-multi-thread`.
      No Cargo.toml change is needed. If `cargo check --benches` ever
      complains about `enable_all`, it means a future tokio change removed it;
      fall back to `enable_io().enable_time()`.
    - `build_messages(1000)` produces ~200 KB of message bytes and takes a
      non-trivial time per iteration. criterion auto-adjusts sample size for
      this — do NOT hard-cap `sample_size` or `measurement_time`. Let the
      baseline be whatever criterion decides by default.
    - `NoOpFilter::apply_request` is intentionally a trivial `Ok(())` rather
      than `unimplemented!()`. This lets the chain double as a production-valid
      artifact (Plan 01-02 could pass it through `FilterChain::apply_request`
      without panic). The bench never exercises `apply_request`, but keeping
      it sound makes the bench file safer to grep against as an API sample.
    - `black_box` is applied to the mutable references AND to the returned
      hash/chunk/extras so the compiler cannot DCE the calls.
    - Do NOT add `#[cfg(test)] mod tests { ... }` — bench files are not test
      files and criterion provides no test harness here.

    What NOT to do:

    - Do NOT add a fourth bench function (e.g. a two-filter chain). CONTEXT.md
      §decisions + the objective above lock the measurement set to exactly 7.
    - Do NOT bench `compute_checksum` against alternate hash algorithms.
      CONTEXT.md §decisions locked `twox-hash`.
    - Do NOT bench `set_frozen_prefix` or `verify_frozen_prefix` directly.
      Both are `compute_checksum` + an equality/insert; measuring
      `compute_checksum` captures the work.
    - Do NOT touch `hook_latency.rs`, `evaluation_only.rs`, `dfquad_scaling.rs`.
    - Do NOT modify `Cargo.toml` beyond Task 1's additive `[[bench]]` entry.
  </action>
  <verify>
    <automated>cargo bench --bench filter_chain_overhead --no-run 2>&amp;1 | tail -10</automated>
  </verify>
  <acceptance_criteria>
    - `test -f crates/rigor/benches/filter_chain_overhead.rs` succeeds
    - `wc -l crates/rigor/benches/filter_chain_overhead.rs | awk '{print $1}'` is >= 110
    - `grep -c 'criterion_group!' crates/rigor/benches/filter_chain_overhead.rs` returns 1
    - `grep -c 'criterion_main!' crates/rigor/benches/filter_chain_overhead.rs` returns 1
    - `grep -c 'fn bench_compute_checksum' crates/rigor/benches/filter_chain_overhead.rs` returns 1
    - `grep -c 'fn bench_apply_response_chunk' crates/rigor/benches/filter_chain_overhead.rs` returns 1
    - `grep -c 'fn bench_finalize_response' crates/rigor/benches/filter_chain_overhead.rs` returns 1
    - `grep -c 'compute_checksum' crates/rigor/benches/filter_chain_overhead.rs` returns >= 3 (import + call + bench id usage)
    - `grep -c 'FilterChain::new' crates/rigor/benches/filter_chain_overhead.rs` returns >= 4 (zero_filters + one_filter for each of apply_response_chunk and finalize_response)
    - `grep -c 'BenchmarkId::from_parameter' crates/rigor/benches/filter_chain_overhead.rs` returns >= 5 (3 size rows + 2 per-group config rows * 2 groups = 7, but size rows share the constructor call pattern; at minimum 5 occurrences)
    - `grep -c '"zero_filters"' crates/rigor/benches/filter_chain_overhead.rs` returns 2
    - `grep -c '"one_filter"' crates/rigor/benches/filter_chain_overhead.rs` returns 2
    - `grep -c 'struct NoOpFilter' crates/rigor/benches/filter_chain_overhead.rs` returns 1
    - `grep -c 'impl EgressFilter for NoOpFilter' crates/rigor/benches/filter_chain_overhead.rs` returns 1
    - `cargo bench --bench filter_chain_overhead --no-run` exits 0 (compiles cleanly against the real `rigor::daemon::egress::*` surface)
    - `cargo fmt -- --check` exits 0 (includes the new bench file)
    - `cargo clippy --benches --all-features -- -D warnings` exits 0
    - The three existing bench files are unchanged: `git diff --name-only crates/rigor/benches/ | grep -Ev '^crates/rigor/benches/filter_chain_overhead\.rs$' | wc -l` returns 0
  </acceptance_criteria>
  <done>
    `filter_chain_overhead.rs` exists with the three bench functions + NoOpFilter
    helper, compiles under `cargo bench --no-run`, passes fmt + clippy, and
    the three pre-existing bench files were not modified.
  </done>
</task>

<task type="auto">
  <name>Task 3: Execute the bench once to produce baselines and verify 7 rows</name>
  <files>crates/rigor/benches/filter_chain_overhead.rs</files>
  <read_first>
    - crates/rigor/benches/filter_chain_overhead.rs (the file written in Task 2 — this task doesn't modify it, only runs it).
    - crates/rigor/Cargo.toml lines 86-92 (the `[[bench]] filter_chain_overhead` entry registered in Task 1).
  </read_first>
  <action>
    Run the bench binary end-to-end so that `target/criterion/` gets populated
    with baseline data AND so we confirm the 7 expected measurement rows are
    produced.

    Run the bench via criterion's bencher output format so the row names are
    machine-greppable:

    ```bash
    cargo bench --bench filter_chain_overhead -- --output-format bencher 2>&1 | tee /tmp/filter_chain_overhead_bench.out
    ```

    The `bencher` output format (criterion feature shipped with 0.5) emits one
    line per measurement with the `test <group>/<id> ... bench: ...` shape.
    That makes the row presence checks below robust to criterion version skew
    in HTML/JSON layouts.

    Expected output lines (order not guaranteed):

    ```
    test compute_checksum/10         ... bench: ...
    test compute_checksum/100        ... bench: ...
    test compute_checksum/1000       ... bench: ...
    test apply_response_chunk/zero_filters ... bench: ...
    test apply_response_chunk/one_filter   ... bench: ...
    test finalize_response/zero_filters    ... bench: ...
    test finalize_response/one_filter      ... bench: ...
    ```

    Total bench runtime will be a few minutes (criterion default 100 samples
    per measurement, mostly in `compute_checksum/1000`). Do NOT short-circuit
    with `--sample-size 10` or similar flags — the baseline MUST be a default
    criterion run so Phase 17's comparison is apples-to-apples.

    If the run exits non-zero, do NOT continue — diagnose. Most likely causes:
    - Missing `pub use frozen::*;` in egress/mod.rs → Plan 01-01 did not land
      correctly; this is a dependency violation, not a bench bug.
    - `SseChunk { data: String }` shape changed → chain.rs was modified
      outside Plan 01-03; that is a regression, not a bench bug.
    - Tokio feature flag mismatch (`enable_all` gone) → fall back to
      `enable_io().enable_time()` in `new_runtime()` and re-run.

    If the run succeeds, commit the file and Cargo.toml change from Tasks 1+2.
    Criterion writes to `target/criterion/` which is already git-ignored by
    the repo — do NOT commit those artifacts. Do NOT commit `/tmp/filter_chain_overhead_bench.out`.
  </action>
  <verify>
    <automated>cargo bench --bench filter_chain_overhead -- --output-format bencher 2>&amp;1 | tee /tmp/filter_chain_overhead_bench.out &amp;&amp; grep -Ec '^test (compute_checksum/(10|100|1000)|apply_response_chunk/(zero_filters|one_filter)|finalize_response/(zero_filters|one_filter)) .* bench:' /tmp/filter_chain_overhead_bench.out</automated>
  </verify>
  <acceptance_criteria>
    - `cargo bench --bench filter_chain_overhead -- --output-format bencher` exits 0
    - `grep -c '^test compute_checksum/10 ' /tmp/filter_chain_overhead_bench.out` returns 1
    - `grep -c '^test compute_checksum/100 ' /tmp/filter_chain_overhead_bench.out` returns 1
    - `grep -c '^test compute_checksum/1000 ' /tmp/filter_chain_overhead_bench.out` returns 1
    - `grep -c '^test apply_response_chunk/zero_filters ' /tmp/filter_chain_overhead_bench.out` returns 1
    - `grep -c '^test apply_response_chunk/one_filter ' /tmp/filter_chain_overhead_bench.out` returns 1
    - `grep -c '^test finalize_response/zero_filters ' /tmp/filter_chain_overhead_bench.out` returns 1
    - `grep -c '^test finalize_response/one_filter ' /tmp/filter_chain_overhead_bench.out` returns 1
    - (Total 7 measurement lines — matches REQ-032's future hot-path coverage.)
    - `test -d target/criterion/compute_checksum` succeeds
    - `test -d target/criterion/apply_response_chunk` succeeds
    - `test -d target/criterion/finalize_response` succeeds
    - Pre-existing benches still work: `cargo bench --bench hook_latency --no-run` exits 0 AND `cargo bench --bench dfquad_scaling --no-run` exits 0 AND `cargo bench --bench evaluation_only --no-run` exits 0 (regression guard — we did not break the bench build).
  </acceptance_criteria>
  <done>
    A full criterion run of `filter_chain_overhead` succeeded, `target/criterion/`
    contains baseline data for all 7 measurement rows, the pre-existing three
    benches still compile, and the committed state is `Cargo.toml` +
    `crates/rigor/benches/filter_chain_overhead.rs` (no other changes).
  </done>
</task>

</tasks>

<verification>
  Plan-wide gates (run after Task 3 completes):

  - `cargo fmt -- --check` exits 0
  - `cargo clippy --benches --all-features -- -D warnings` exits 0
  - `cargo check -p rigor --benches` exits 0
  - `cargo bench --bench filter_chain_overhead --no-run` exits 0
  - `cargo bench --bench filter_chain_overhead -- --output-format bencher` exits 0 and emits exactly 7 `^test .* bench:` rows matching the expected names
  - `cargo test -p rigor --lib daemon::egress` exits 0 (frozen.rs tests from Plan 01-01 + chain.rs tests from main still green — regression guard)
  - `cargo test --test egress_integration -p rigor` exits 0 (integration tests from Plan 01-04 or pre-existing egress tests still green — regression guard)
  - `git diff --name-only` shows exactly two modified/added paths: `crates/rigor/Cargo.toml` and `crates/rigor/benches/filter_chain_overhead.rs`; the three pre-existing bench files and all `src/` files are unchanged
  - `target/criterion/` is NOT staged (already gitignored — verify with `git check-ignore target/criterion/compute_checksum/10/` returning 0 exit + a non-empty path)
</verification>

<success_criteria>
- REQ-001 (baseline side): `compute_checksum` has criterion baselines at 10 / 100 / 1000 messages. Phase 17's bench-regression gate (REQ-032) has the data it needs to fail CI on >20% regression of the xxhash64-over-messages hot path.
- REQ-002 (baseline side): `FilterChain::apply_response_chunk` and `FilterChain::finalize_response` have criterion baselines in both zero-filter and one-filter configurations. Phase 17's gate has the data it needs to fail CI on >20% regression of the per-chunk and per-stream-end hooks introduced by Plan 01-03's wiring.
- Bench build does not regress: the three pre-existing bench files (`hook_latency`, `evaluation_only`, `dfquad_scaling`) still pass `cargo bench --no-run`.
- Production code is untouched: `frozen.rs`, `chain.rs`, `ctx.rs`, `proxy.rs` diff is empty. This plan is purely additive over the outputs of Plans 01-01, 01-02, 01-03.
- No new dependency is added. criterion 0.5 (already in `[dev-dependencies]`) and the existing direct deps (async_trait, tokio, serde_json) cover the bench.
- Plan 01-04 (integration tests, same wave) is unaffected — files_modified is disjoint (`benches/*.rs` here vs `tests/*.rs` there).
</success_criteria>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| bench harness → crate public API | The bench runs inside the `rigor` crate's own compilation unit as a `[[bench]]` target. It only consumes `pub` items from `rigor::daemon::egress::*`. No network, no filesystem, no untrusted input. Runs only under developer `cargo bench` invocation and in CI — never in a deployed rigor daemon. |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-01-05-01 | Tampering | benches/filter_chain_overhead.rs | accept | Bench code is committed alongside production code and covered by the same `cargo fmt` / `cargo clippy --benches -- -D warnings` + PR review. No runtime-loaded bench payload. |
| T-01-05-02 | Information Disclosure | target/criterion/ HTML reports | accept | Report artifacts contain only timing numbers from synthetic fixtures (`build_messages` — deterministic public string). No PII, secrets, or user data. `target/` is gitignored; CI uploads are scoped to the existing artifact-retention policy. |
| T-01-05-03 | Denial of Service | CI runtime budget | mitigate | Bench runs only when `cargo bench` is explicitly invoked (i.e. Phase 17's bench-regression gate job — not every PR). Default criterion sample size is acceptable at ~1–3 min wall-clock for this bench set; no `sample_size` override bloats it. |
| T-01-05-04 | Repudiation / Elevation / Spoofing | n/a | accept | Bench does not cross a privilege boundary, does not log, does not authenticate. No applicable S/R/E threats. |
</threat_model>

<output>
After completion, create `.planning/phases/01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon/01-05-SUMMARY.md`.

The SUMMARY must record:
- Concrete baseline numbers produced by Task 3 (copy the 7 `test <name> ... bench: X ns/iter (+/- Y)` lines verbatim from `/tmp/filter_chain_overhead_bench.out`) so Phase 17 has a git-tracked starting point.
- Confirmation that no file outside `Cargo.toml` + `benches/filter_chain_overhead.rs` was touched.
- Cross-reference: "Consumed by Phase 17 (issue #13 / REQ-032 bench-regression gate)."
</output>
