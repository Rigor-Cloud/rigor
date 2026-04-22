---
phase: 01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon
plan: 05
subsystem: benches
tags: [bench, criterion, baseline, filter-chain, frozen-prefix, xxhash, twox-hash, perf, phase-17-prereq, req-032]

# Dependency graph
requires:
  - phase: 01-01
    provides: "rigor::daemon::egress::compute_checksum (xxhash64-over-canonical-serde_json) re-exported via egress/mod.rs pub use frozen::*;"
  - phase: 01-02
    provides: "FilterChain::apply_request post-chain frozen-prefix verifier (not exercised by this bench but cements compute_checksum as the hot path)"
  - phase: 01-03
    provides: "SseChunk { data: String } shape + FilterChain::apply_response_chunk/finalize_response as the per-chunk / per-stream-end hooks wired into proxy.rs"
  - phase: pre-existing
    provides: "EgressFilter trait (default apply_response_chunk + finalize_response bodies), FilterChain::new, ConversationCtx::new_anonymous, criterion 0.5 in [dev-dependencies], async_trait + tokio + serde_json as direct deps"
provides:
  - "Criterion baseline measurements for compute_checksum at 10 / 100 / 1000 messages"
  - "Criterion baseline measurements for FilterChain::apply_response_chunk (zero_filters, one_filter)"
  - "Criterion baseline measurements for FilterChain::finalize_response (zero_filters, one_filter)"
  - "Seven rows of baseline data under target/criterion/ that Phase 17 (issue #13 / REQ-032 bench-regression gate) can compare against"
  - "[[bench]] entry `filter_chain_overhead` in crates/rigor/Cargo.toml (harness = false)"
affects:
  - "Phase 17 / issue #13 / REQ-032 (bench-regression gate, fail on >20% regression for the evaluator hot path) — this plan supplies the before-numbers"
  - "No production code impact — this plan is purely additive bench tooling"

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "criterion baseline capture pattern: bench_with_input + BenchmarkId::from_parameter for size-parameterized rows, bench_function + BenchmarkId::from_parameter(\"label\") for config-parameterized rows"
    - "Local tokio current-thread runtime inside b.iter — build once per group, reuse across samples; block_on overhead cancels out relative to the measured work"
    - "Default-bodied NoOpFilter as a minimal one-filter harness so per-filter overhead is isolated from any real filter work"
    - "black_box on both inputs and outputs to foil DCE in the release-profile bench build"
  third_party_apis:
    - "criterion 0.5 (html_reports feature) — already in [dev-dependencies]"

key-files:
  created:
    - "crates/rigor/benches/filter_chain_overhead.rs (226 lines, 3 bench functions, 7 measurement rows)"
  modified:
    - "crates/rigor/Cargo.toml (+4 lines: [[bench]] name = \"filter_chain_overhead\", harness = false)"

key-decisions:
  - "Baselines captured with criterion defaults (100 samples per measurement) rather than --quick, so Phase 17's comparison is apples-to-apples with whatever CI captures later. The plan explicitly forbids --sample-size overrides."
  - "Three `compute_checksum` sizes (10/100/1000) — mirrors dfquad_scaling.rs shape and brackets realistic prompt sizes from Claude Code's typical 2-50 message traffic up through Phase 1B CCR retrieval loops that may approach 1000 messages."
  - "Two `apply_response_chunk` / `finalize_response` configurations (zero_filters, one_filter) capture the two ends of the spectrum today: an empty Vec<Arc<dyn EgressFilter>> (pure reverse-iterator pass) vs one filter carrying default no-op bodies. A 'two_filter' row was intentionally omitted per CONTEXT.md §decisions — measurement set is locked to exactly 7."
  - "NoOpFilter supplies a trivial Ok(()) apply_request so the filter is a valid production artifact (Plan 01-02's post-chain verifier would accept it). The bench never exercises apply_request; keeping it sound makes the file a credible API sample."
  - "Single-threaded tokio runtime. new_current_thread + enable_all avoids the rt-multi-thread dispatch cost and matches the 'small stream-of-work' shape of a bench loop."
  - "Fast-path byte-equality check NOT modelled. This bench measures the chain methods themselves; the proxy.rs fast-path lives in proxy.rs and would be a separate bench."

patterns-established:
  - "criterion baseline capture: 226-line, 3-group bench with 7 measurement rows, wired via [[bench]] harness = false — matches the hook_latency + dfquad_scaling + evaluation_only shape already in the repo."

requirements-completed: [REQ-001, REQ-002]

# Metrics
duration: ~4.5 min
completed: 2026-04-22
---

# Phase 01 Plan 05: Criterion baselines for compute_checksum + FilterChain response chain Summary

**Seven criterion baselines captured for the Phase 1 egress primitives — compute_checksum at 10/100/1000 messages, FilterChain::apply_response_chunk and FilterChain::finalize_response in zero/one-filter configs — so Phase 17 (issue #13 / REQ-032) can compare future CI runs against today's "before" numbers without any production code change.**

## Performance

- **Duration:** ~4.5 min (2026-04-22T18:49:15Z → 2026-04-22T18:53:46Z)
- **Tasks:** 3 (Task 1 Cargo.toml, Task 2 bench source, Task 3 run-to-capture)
- **Files modified:** 2 (crates/rigor/Cargo.toml, crates/rigor/benches/filter_chain_overhead.rs)

## Baseline Numbers (captured 2026-04-22, default criterion sample size)

Raw rows from `cargo bench --bench filter_chain_overhead -- --output-format bencher`:

```
test compute_checksum/10 ... bench:        1836 ns/iter (+/- 14)
test compute_checksum/100 ... bench:       18752 ns/iter (+/- 192)
test compute_checksum/1000 ... bench:      190911 ns/iter (+/- 1835)

test apply_response_chunk/zero_filters ... bench:         247 ns/iter (+/- 3)
test apply_response_chunk/one_filter ... bench:         264 ns/iter (+/- 2)

test finalize_response/zero_filters ... bench:         235 ns/iter (+/- 2)
test finalize_response/one_filter ... bench:         253 ns/iter (+/- 1)
```

Observations (informational — no enforcement this plan):

- `compute_checksum` scales linearly (10 → 100 → 1000 messages produces ~10× → ~10× → ~10× iter-time), confirming the twox-hash64-over-serde_json path is O(n) in message count and does not hit a super-linear regression at 1000 messages. Phase 17's >20% gate will have a clean per-size baseline to compare against.
- `apply_response_chunk` and `finalize_response` overhead is ~240 ns in both configurations. The zero→one-filter delta is ~17 ns (apply_response_chunk) / ~18 ns (finalize_response) — roughly one vtable call + `Ok(())` return for the default trait body, which is the expected floor.
- The `one_filter` / `zero_filters` gap is deliberately tiny (NoOpFilter uses every default trait body); a real filter's cost on top of this floor will be the filter's own work, not framework overhead.

Raw capture saved at `/tmp/filter_chain_overhead_bench.out` at run time. HTML reports (plotters backend) written under `target/criterion/{compute_checksum,apply_response_chunk,finalize_response}/` — gitignored, not committed.

## Task Commits

Each task committed atomically on the main working tree (sequential executor, hooks enabled):

1. **Task 1: Register [[bench]] entry in Cargo.toml** — `4aa7df8` (chore)
2. **Task 2: Create filter_chain_overhead.rs bench with 7 measurements** — `d207667` (bench)
3. **Task 3: Execute the bench to produce baselines** — no new commit (run-only; `target/criterion/` is gitignored, `/tmp/filter_chain_overhead_bench.out` is ephemeral)

SUMMARY.md is added in the plan's final metadata commit per executor protocol.

## Files Created/Modified

- `crates/rigor/benches/filter_chain_overhead.rs` *(created, 226 lines after `cargo fmt` re-flowed one bench_with_input call)* — 3 criterion groups, 7 measurement rows, NoOpFilter harness, size-parameterized `compute_checksum` bench + label-parameterized chain benches
- `crates/rigor/Cargo.toml` *(+4 lines)* — fourth `[[bench]]` entry: `name = "filter_chain_overhead"`, `harness = false`

No other files touched. Per plan spec, the executor did NOT modify:
- `crates/rigor/benches/hook_latency.rs`, `evaluation_only.rs`, `dfquad_scaling.rs` (pre-existing benches — regression guard)
- `crates/rigor/src/daemon/egress/{frozen,chain,ctx,mod}.rs` (frozen — 01-01/01-02 territory)
- `crates/rigor/src/daemon/proxy.rs` (frozen — 01-03 territory)
- `crates/rigor/tests/*.rs` (frozen — 01-04 territory)
- `.planning/STATE.md`, `.planning/ROADMAP.md`, `.planning/REQUIREMENTS.md`, `.planning/config.json` (orchestrator-managed)

## Decisions Made

1. **Default sample size (100) rather than `--quick`.** The plan forbids sample-size overrides so Phase 17's comparison is apples-to-apples. Full run took ~60 s wall-clock (mostly `compute_checksum/1000`) — well inside the CI budget the plan contemplates.

2. **`rigor::daemon::egress::{compute_checksum, FilterChain, ...}` import path.** This is the glob re-export via `pub use frozen::*;` + `pub use chain::*;` + `pub use ctx::*;` in `egress/mod.rs`, matching `crates/rigor/tests/egress_integration.rs:7`'s working pattern. No `rigor::daemon::egress::frozen::compute_checksum` path games.

3. **Single-threaded tokio runtime built once per group.** `rt.block_on` inside `b.iter` adds constant overhead that cancels out across samples; using `new_current_thread` avoids rt-multi-thread's per-task dispatch cost, which would otherwise dominate the ~240 ns measurement.

4. **NoOpFilter has a trivial `Ok(())` `apply_request`.** The trait only defaults `apply_response_chunk` and `finalize_response`; `apply_request` has no default body. Supplying a trivial impl keeps the filter a production-valid artifact (Plan 01-02's verifier could accept it) even though this bench never exercises the request path.

5. **`cargo fmt` re-flowed one call site after initial write.** The Task 2 `<action>` source had a multi-line `bench_with_input` call that rustfmt compacted to a single `bench_with_input(..., ..., |b, msgs| { ... })` form. Semantics unchanged; included in the `bench(01-05)` commit.

## Deviations from Plan

**None auto-fixed.** The plan was executed exactly as written. No Rule 1 / Rule 2 / Rule 3 / Rule 4 triggers.

The only executor-level discretion was accepting rustfmt's compaction of the `bench_with_input` call site during Task 2 — a cosmetic reformatting, no logic change, recorded in the Task 2 commit. This is below the threshold for a "deviation" per the executor rules.

## Issues Encountered

- **`cargo fmt -- --check` failed once** immediately after the initial bench-source write because rustfmt preferred a more compact form for `bench_with_input`. Resolved by running `cargo fmt -p rigor` before commit. No logic change, no additional commit — included in the Task 2 commit.
- **`PreToolUse:Edit` read-before-edit reminder fired once** on the Cargo.toml edit despite having already read the file earlier in the session. The edit had already landed successfully; verified via `Read` and continued. Tooling ergonomics observation, not a correctness issue.

## Acceptance Criteria — All Passing

| Check | Result |
|-------|--------|
| `cargo bench --bench filter_chain_overhead --no-run` exits 0 | 0 (clean `Finished` line) |
| `cargo bench --bench filter_chain_overhead -- --output-format bencher` produces 7 rows | 7 rows, 3 per-group blocks |
| `grep -c '^\[\[bench\]\]' crates/rigor/Cargo.toml` returns 4 | 4 |
| `grep -c 'name = "filter_chain_overhead"' crates/rigor/Cargo.toml` returns 1 | 1 |
| `wc -l crates/rigor/benches/filter_chain_overhead.rs` >= 110 | 226 |
| `cargo fmt -- --check` | clean (exit 0) |
| `cargo clippy --benches --all-features -- -D warnings` | clean |
| `cargo check -p rigor --benches` | clean |
| `cargo bench --bench hook_latency --no-run` | Executable (regression guard) |
| `cargo bench --bench evaluation_only --no-run` | Executable (regression guard) |
| `cargo bench --bench dfquad_scaling --no-run` | Executable (regression guard) |
| `cargo test -p rigor --lib daemon::egress` | 29 passed, 0 failed |
| `cargo test --test egress_integration -p rigor` | 5 passed, 0 failed |
| `git diff --name-only HEAD~2..HEAD` lists exactly 2 paths | `crates/rigor/Cargo.toml`, `crates/rigor/benches/filter_chain_overhead.rs` |
| `target/criterion/{compute_checksum,apply_response_chunk,finalize_response}` exist | all 3 dirs present |
| `git check-ignore target/criterion/compute_checksum/10/` | exit 0 (gitignored — not committed) |

## Consumed By

- **Phase 17 (issue #13 / REQ-032 bench-regression gate)** — the 7 rows captured above are the "before" numbers Phase 17's CI job will compare against. Phase 17 owns the >20% threshold and the CI wiring; this plan only writes the baseline.

## What This Unblocks

- **Phase 17 bench-regression gate:** Ready. All 7 measurement rows exist in canonical `target/criterion/` layout; the per-stage `estimates.json` files under each group dir are the machine-readable source of truth for a future `cargo bench -- --save-baseline main && cargo bench -- --baseline main` comparison.
- **Future response-side filters (Phase 1B CCR, Phase 3A annotation emission):** `apply_response_chunk` and `finalize_response` have measured per-invocation cost now. Adding a filter whose own work is an order of magnitude above the 17 ns no-op delta will be visibly non-trivial; a filter that adds single-digit-ns of its own work per chunk will be indistinguishable from noise — informative when deciding if a given filter needs its own per-filter bench.
- **Hash-algorithm swap guard (if ever reconsidered):** `compute_checksum/1000` at 190 µs is the budget to beat. A move off twox-hash64 would be caught by Phase 17 if slower; a move *onto* a slower hash (e.g. sha2 for content-addressing) would be explicitly off-path anyway.

## User Setup Required

None — no external service configuration, no env vars, no manual verification. Bench runs only under explicit `cargo bench` invocation (CI or developer local).

## Next Phase Readiness

- **Phase 01 overall:** All 5 plans (01 through 05) complete. Phase 1 (PR-3) umbrella ready to PR against `main`.
- **Phase 17 bench-regression gate:** Ready to plan. Data exists under `target/criterion/` once CI runs a canonical baseline; the CI job will pick it up via criterion's `--save-baseline` / `--baseline` convention.

## Self-Check

Verified before writing this SUMMARY:

- `cargo bench --bench filter_chain_overhead --no-run` → exit 0
- `cargo bench --bench filter_chain_overhead -- --output-format bencher` → 7 rows, all expected names
- `cargo fmt -- --check` → exit 0
- `cargo clippy --benches --all-features -- -D warnings` → exit 0
- `cargo check -p rigor --benches` → exit 0
- `cargo test -p rigor --lib daemon::egress` → 29 passed
- `cargo test --test egress_integration -p rigor` → 5 passed
- Pre-existing benches `hook_latency`, `evaluation_only`, `dfquad_scaling` → all `--no-run` exit 0
- `test -f crates/rigor/benches/filter_chain_overhead.rs` → EXISTS
- `grep -c '^\[\[bench\]\]' crates/rigor/Cargo.toml` → 4
- `git log --oneline | grep -E '^[a-f0-9]+ (chore|bench)\(01-05\)'` → `d207667 bench(01-05): ...`, `4aa7df8 chore(01-05): ...`
- `target/criterion/{compute_checksum,apply_response_chunk,finalize_response}` → all present
- `git check-ignore target/criterion/compute_checksum/10/` → exit 0 (gitignored)
- `git diff --name-only HEAD~2..HEAD` → exactly `crates/rigor/Cargo.toml` and `crates/rigor/benches/filter_chain_overhead.rs`

## Self-Check: PASSED

---
*Phase: 01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon*
*Plan: 05*
*Completed: 2026-04-22*
