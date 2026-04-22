---
phase: 01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon
plan: 03
subsystem: daemon-proxy
tags: [egress, filter-chain, sse, streaming, otel, tracing, proxy, tokio-spawn]

# Dependency graph
requires:
  - phase: 01-01
    provides: "FrozenPrefix + set_frozen_prefix + verify_frozen_prefix (already green at a22ca2e, f611f83, 535fb91)"
  - phase: 01-02
    provides: "FilterChain::apply_request post-chain frozen-prefix verifier (already green at 7fe0227, 496a62d, b00e479)"
  - phase: pre-existing
    provides: "FilterChain::apply_response_chunk + finalize_response on the trait and chain type (chain.rs:158-196), ConversationCtx::new_anonymous() (ctx.rs:67), egress::SseChunk (chain.rs:15)"
provides:
  - "Per-SSE-chunk invocation of FilterChain::apply_response_chunk inside proxy.rs streaming tokio::spawn"
  - "Post-stream invocation of FilterChain::finalize_response after upstream closes"
  - "`rigor.daemon.proxy.response_chain` OTel info_span wrapping both call sites (request_id + filter_count fields)"
  - "Fast-path byte-equality check to avoid unnecessary Bytes reallocation when no filter mutates the chunk"
  - "Forward-verbatim-on-error semantics: chain error => original `bytes` forwarded (chunks never dropped)"
affects:
  - "phase 1B CCR retrieval — can now register a response-side filter and see it invoked per chunk"
  - "phase 3A annotation emission — can now emit extra synthetic SSE chunks via finalize_response"
  - "phase 01 plan 04 (integration tests) — counter-filter test can assert N chunks invoked"

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Response-side FilterChain invocation: clone Arc<FilterChain> into tokio::spawn, build one ConversationCtx per stream, enter a per-chunk OTel span, forward verbatim on error"
    - "Fast-path byte-equality check: `chunk_wrap.data.as_bytes() == bytes.as_ref()` avoids reallocation when no filter mutated the SSE chunk"
    - "Inner-scope span entry/exit (`let _entered = response_chain_span.enter(); ...; drop(_entered);`) per chunk so the span is re-entered N times (once per chunk + once for finalize) from a single info_span! declaration"

key-files:
  created: []
  modified:
    - "crates/rigor/src/daemon/proxy.rs (+82 lines net: doc comment, response_chain_bg clone, response_ctx + span declaration, per-chunk invocation block replacing the plain forward, finalize_response block inserted before cleanup)"

key-decisions:
  - "Fast-path clone preserves non-UTF-8 binary payloads. The code calls String::from_utf8_lossy + re-encode only when a filter actually mutates the chunk — byte-equal chunks use `bytes.clone()` so a rare non-UTF-8 SSE payload round-trips exactly. Simpler unconditional re-encode was rejected per plan action note."
  - "Single info_span! declaration, re-entered twice. Per-chunk enter/drop keeps the span scoped tightly to the chain invocation; finalize_response re-enters the same span. This matches the 'one logical operation per stream' OTel model — a parent span is observable end-to-end, with child work naturally nested under it."
  - "response_chain_bg is declared last in the `_bg` clone block. Placement matches plan spec exactly (after session_id_bg) to keep the clone block easy to scan."
  - "response_ctx is a fresh ConversationCtx::new_anonymous() per stream (not shared with the request-side ctx at proxy.rs:1362). Rationale: the request-side ctx is scoped inside the fallback::execute future and is dropped before the spawn is reached. Sharing would require threading it through a clonable wrapper; a fresh anonymous ctx is sufficient for response-side filter state (filters that need to correlate with request-side state will key off `request_id_bg` or `session_id_bg`, which are already captured)."
  - "Edit A (optional doc comment) was applied. It adds one line of context for future readers who see `request_chain.clone()` on the response path and wonder about cost; zero runtime impact."
  - "No new tests added in this plan. Per CONTEXT.md and the plan's <output>, new filter-invocation tests belong to Plan 04 (tests/egress_integration.rs extensions). This plan's regression guard is the existing 306 lib + 25 integration tests."

patterns-established:
  - "Response-side FilterChain wiring pattern: (1) clone Arc<FilterChain> into tokio::spawn as `response_chain_bg`, (2) build `response_ctx` + `response_chain_span` once at the top of the spawn body, (3) enter the span per-chunk and re-enter for finalize, (4) forward verbatim on error, fast-path on no-op, re-encode only on mutation."

requirements-completed: [REQ-002, REQ-003, REQ-004]

# Metrics
duration: ~5 min
completed: 2026-04-22
---

# Phase 01 Plan 03: Wire FilterChain response path into proxy.rs Summary

**Wired `FilterChain::apply_response_chunk` per SSE chunk and `FilterChain::finalize_response` post-stream into `crates/rigor/src/daemon/proxy.rs`, both under a per-stream `rigor.daemon.proxy.response_chain` OTel info_span; chunks are forwarded verbatim on error and the BLOCK kill-switch + auto-retry paths are untouched.**

## Performance

- **Duration:** ~5 min (2026-04-22T18:30:47Z → 2026-04-22T18:33:57Z)
- **Tasks:** 1 (plan has 1 `<task>` block with 4 edit sites A/B/C/D; all 4 applied)
- **Files modified:** 1 (`crates/rigor/src/daemon/proxy.rs` only)

## Accomplishments

- **Edit A applied** (optional doc comment at proxy.rs:1352 explaining why `FilterChain` clone is cheap) — future-reader trail
- **Edit B applied** — `let response_chain_bg = request_chain.clone();` at proxy.rs:1554, right after `session_id_bg` in the pre-spawn clone block
- **Edit C applied** — at proxy.rs:1632-1642 a fresh `ConversationCtx::new_anonymous()` + `info_span!("rigor.daemon.proxy.response_chain", ...)` declared once per stream. The plain 4-line `client_tx.send(Ok(bytes))` forward at line 1684 was replaced with a 37-line block that invokes `response_chain_bg.apply_response_chunk(&mut chunk_wrap, &mut response_ctx)` inside a per-chunk span entry, picks `forwarded_bytes` via a fast-path byte-equality check, and forwards `forwarded_bytes` through `client_tx` (line 1732)
- **Edit D applied** — a new block at proxy.rs:2586-2616 between the end of the `while let Some(chunk_result)` loop and the `active_streams.remove(...)` cleanup: re-enters the same `response_chain_span`, calls `response_chain_bg.finalize_response(&mut response_ctx).await`, forwards any returned `SseChunk`s verbatim via `Bytes::from(chunk.data.into_bytes())`, and logs surfaced errors via `tracing::warn!`
- **Zero regression:** all 306 lib tests pass (includes the 11 frozen-prefix tests from 01-01 and the 8 chain tests augmented by 01-02), `egress_integration` 2/2, `dogfooding` 10/10, `true_e2e` 7/7, `integration_hook` 6/6
- **Zero clippy warnings** across `cargo clippy --all-targets --all-features -- -D warnings`
- **Zero fmt diffs** — `cargo fmt -- --check` clean
- **Preserved invariants:** `drop(upstream)` kill-switch count unchanged (still 1), `auto_refine_if_needed` count unchanged (still 2), non-streaming path (line 2617+) untouched, `extract_sse_assistant_text` untouched, `extract_and_evaluate` untouched

## Task Commits

Single atomic commit for the whole wiring (plan has one `<task>` block):

1. **Task 1 (A + B + C + D): wire FilterChain response chain into proxy.rs SSE loop** — `865f023` (feat)

No separate REFACTOR commit was needed — the GREEN implementation compiled clippy-clean and fmt-clean on the first pass.

## Files Created/Modified

- `crates/rigor/src/daemon/proxy.rs` *(+82 lines, -1 line net)*

No other files touched. Per plan spec, the executor did NOT modify:
- `crates/rigor/src/daemon/egress/chain.rs` (frozen — 01-01/01-02 territory)
- `crates/rigor/src/daemon/egress/frozen.rs` (frozen — 01-01 territory)
- `crates/rigor/tests/*.rs` (reserved for plan 01-04)
- `.planning/STATE.md`, `.planning/ROADMAP.md`, `.planning/config.json` (orchestrator-managed)

## Decisions Made

1. **Applied Edit A (documentation-only).** The plan marks Edit A as optional. I applied it because it adds two lines of context that future readers will want when they see `request_chain.clone()` appearing for the response side; zero runtime cost.

2. **Fresh `ConversationCtx::new_anonymous()` per stream, not reused from the request side.** The request-side ctx at proxy.rs:1362 is scoped inside the `fallback::execute` future and is dropped before the `tokio::spawn` is reached. Sharing would require threading it through a clonable wrapper or an `Arc<Mutex<...>>`; a fresh anonymous ctx is sufficient because downstream response filters that need request correlation will key off `request_id_bg` (already captured and logged in the span).

3. **Byte-equality fast path.** The plan explicitly directs `String::from_utf8_lossy` + mutation detection + conditional re-encode, rather than unconditional `Bytes::from(chunk_wrap.data.into_bytes())`. Reason: `from_utf8_lossy` replaces invalid UTF-8 with U+FFFD, which would silently alter binary SSE payloads (rare but possible). The `chunk_wrap.data.as_bytes() == bytes.as_ref()` equality check preserves exact round-trip for the zero-response-filter common case today.

4. **Single `info_span!` declaration, re-entered via `enter()`.** Two options were available: (a) `Instrument` the per-chunk future, (b) manually `enter()` the span at each call site. I chose (b) because the chunk invocation is a short, synchronous-on-its-face `.await` inside a hot loop; manual enter/drop keeps the span scope minimal and avoids the `Instrument` trait's additional future-wrapping cost. The finalize block re-enters the same span so both appear under one logical "response_chain" span per stream.

5. **No new tests in this plan.** The plan's `<output>` and `<success_criteria>` explicitly defer "counter-filter" integration testing to Plan 04. This plan's regression guard is the existing 306 lib + 25 integration tests, all green post-change.

## Deviations from Plan

**None — plan executed exactly as written.**

No Rule 1 (bug) fixes, no Rule 2 (missing critical functionality) additions, no Rule 3 (blocking issue) fixes, and no Rule 4 (architectural) escalations were triggered. All four edits landed in the exact order and exact form prescribed by the plan's action block.

The only writer-level choice was applying optional Edit A (doc comment); this is documented in Decisions Made §1.

## Issues Encountered

- **`PreToolUse:Edit` read-before-edit reminder fired on each Edit call** despite having read proxy.rs at multiple offsets in this session. Re-read was performed to satisfy the hook. This is a tooling/hook ergonomics observation, not a rigor correctness issue. No state lost.

## Acceptance Criteria — All Passing

| Check | Result |
|-------|--------|
| `grep 'response_chain_bg\.apply_response_chunk'` returns 1 (plan expects 1) | 1 (on two lines due to rustfmt wrap; `rg -U 'response_chain_bg[\s\n]*\.apply_response_chunk'` = 1) |
| `grep 'response_chain_bg\.finalize_response'` returns 1 | 1 |
| `grep 'rigor.daemon.proxy.response_chain'` returns >= 1 | 2 (doc comment + actual span name — expected) |
| `grep 'let response_chain_bg = request_chain.clone'` returns 1 | 1 |
| `grep 'egress::ConversationCtx::new_anonymous'` returns >= 1 | 2 (request-side fallback at 1362 + response-side spawn at 1636) |
| `grep 'egress::SseChunk'` returns >= 1 | 1 |
| `grep 'drop(upstream)'` returns >= 1 (BLOCK kill-switch NOT removed) | 1 (unchanged) |
| `grep 'auto_refine_if_needed'` returns >= 1 (auto-retry NOT removed) | 2 (unchanged) |
| `cargo check -p rigor` | exit 0 |
| `cargo build -p rigor` | exit 0 |
| `cargo build --all-features` | exit 0 |
| `cargo clippy --lib -p rigor --all-features -- -D warnings` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo fmt -- --check` | clean |
| `cargo test --lib -p rigor daemon::` | 62 passed |
| `cargo test -p rigor --lib` | 306 passed |
| `cargo test --test egress_integration -p rigor` | 2 passed |
| `cargo test --test dogfooding -p rigor` | 10 passed |
| `cargo test --test true_e2e -p rigor` | 7 passed |
| `cargo test --test integration_hook -p rigor` | 6 passed |

## What This Unblocks

- **Phase 01 Plan 04 (integration tests):** `tests/egress_integration.rs` can now add a mock filter whose `apply_response_chunk` / `finalize_response` implementations are non-trivial (counter, chunk-mutator, synthetic-chunk-appender) and assert they fire N times for an N-chunk stream and that appended chunks reach the client.
- **Phase 1B (CCR retrieval loop):** response-side filter that caches/compresses the assistant text can register in this chain and be invoked per chunk with zero proxy.rs changes.
- **Phase 3A (annotation emission from proxy):** a filter whose `finalize_response` returns annotation-SSE chunks (e.g., `event: rigor-annotation\ndata: {...}\n\n`) will have those chunks forwarded to the dashboard verbatim by the block added in Edit D.
- **Any future response-side `EgressFilter`:** the plumbing is now in place; implementers register via `FilterChain::new(...)` construction at the proxy entry (currently proxy.rs:1352) and both request + response sides are invoked automatically.

## User Setup Required

None — no external service configuration needed. The wiring is in-process.

## Next Phase Readiness

- **Plan 01-04 (integration tests):** Ready. The `egress_integration.rs` suite can import `egress::{FilterChain, SseChunk, ConversationCtx}` and register a counter-filter to assert invocation semantics. The response-chain span name `rigor.daemon.proxy.response_chain` is stable and testable via `tracing-test` if desired.
- **Phase 1B (CCR):** Ready. Drop a new `EgressFilter` impl into the chain construction at proxy.rs:1352 and it will be invoked on both sides.
- **Phase 3A (annotation emission):** Ready. `finalize_response` extra chunks are forwarded verbatim — the annotation filter owns SSE framing per the locked decision in CONTEXT.md §decisions.

## Self-Check: PASSED

Verified before final commit:

- `cargo build --all-features` → exit 0 (clean finish)
- `cargo test -p rigor --lib` → 306 passed, 0 failed
- `cargo clippy --all-targets --all-features -- -D warnings` → clean
- `cargo fmt -- --check` → clean
- `cargo test --test egress_integration -p rigor` → 2 passed
- `cargo test --test dogfooding -p rigor` → 10 passed
- `cargo test --test true_e2e -p rigor` → 7 passed
- `cargo test --test integration_hook -p rigor` → 6 passed
- `grep -c 'apply_response_chunk(&mut chunk_wrap' proxy.rs` → 1 (wrapped-line verified via `rg -U`)
- `grep -c 'finalize_response' proxy.rs` → 1 (excluding comments; actual call site = 1)
- `grep -c 'rigor.daemon.proxy.response_chain' proxy.rs` → 2 (1 doc + 1 span literal)
- `grep -c 'drop(upstream)' proxy.rs` → 1 (BLOCK kill-switch intact — unchanged from baseline)
- `grep -c 'auto_refine_if_needed' proxy.rs` → 2 (auto-retry intact — unchanged from baseline)
- `test -f .planning/phases/01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon/01-03-SUMMARY.md` → EXISTS
- `git log --oneline | grep '^[a-f0-9]\+ feat(01-03)'` → `865f023 feat(01-03): wire FilterChain response chain into proxy.rs SSE loop`

---
*Phase: 01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon*
*Plan: 03*
*Completed: 2026-04-22*
