---
phase: 01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon
plan: 02
completed: 2026-04-22
status: complete
executor_note: "Executor killed after GREEN commit before SUMMARY.md; orchestrator wrote SUMMARY manually based on commits + spot-checks. All acceptance criteria verified post-kill."
---

# Plan 01-02 Summary: wire `verify_frozen_prefix` into `FilterChain::apply_request`

## What was built

`FilterChain::apply_request` now invokes `verify_frozen_prefix(ctx, messages)` once after the request-filter loop completes. This makes the frozen-prefix invariant (delivered in 01-01) **enforced automatically** for every request that flows through the chain — downstream filters (CCR, annotation, audit, etc.) inherit the guarantee without per-filter boilerplate.

### Posture

| Build | Behavior on violation |
|-------|----------------------|
| Debug (`cfg(debug_assertions)`) | `panic!("frozen-prefix invariant violated: {e}")` — loud failure in dev/CI |
| Release | `tracing::warn!` + return `FilterError::Internal { filter: "frozen_prefix", reason: ... }` — fail-closed reject |

### Backward compatibility

Chains that never seal a `FrozenPrefix` see `ctx.scratch_get::<FrozenPrefix>()` return `None`. The verifier short-circuits to `Ok(())` — no behavior change for existing callers (the one today being `ClaimInjectionFilter`).

## Commits (TDD cycle)

| Commit | Phase | Description |
|--------|-------|-------------|
| `7fe0227` | **RED** | `test(01-02)`: add 4 failing tokio tests + `SealerFilter` / `EvilFilter` / `TailMutator` fixtures |
| `496a62d` | **GREEN** | `feat(01-02)`: wire verify_frozen_prefix call at end of apply_request |

## Files modified

| File | Change |
|------|--------|
| `crates/rigor/src/daemon/egress/chain.rs` | Added `use super::frozen` (line 7); `frozen::verify_frozen_prefix(...)` invocation at line 137; 4 new tokio tests + 3 fixtures in `#[cfg(test)]` module |

## Tests added (4)

- `apply_request_passes_when_no_frozen_prefix_sealed` — backward-compat no-op
- `apply_request_passes_when_frozen_range_unchanged` — legal tail mutation after seal
- `apply_request_debug_panics_on_frozen_violation` — `#[cfg(debug_assertions)]` + `#[should_panic(expected = "frozen-prefix invariant violated")]`
- `apply_request_release_returns_err_on_frozen_violation` — `#[cfg(not(debug_assertions))]` — asserts `FilterError::Internal` with reason "checksum mismatch"

## Acceptance verification (spot-checked post-kill)

| Check | Result |
|-------|--------|
| `cargo check -p rigor` | ✓ 2.92s clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✓ clean (no warnings) |
| `cargo test -p rigor --lib frozen` | ✓ 11 passed |
| `cargo test -p rigor --lib chain::tests` | ✓ 8 passed |
| TDD gate — RED before GREEN | ✓ `7fe0227` before `496a62d` |
| No STATE.md / ROADMAP.md / proxy.rs changes | ✓ |
| Debug-build `#[should_panic]` test runs | ✓ `apply_request_debug_panics_on_frozen_violation - should panic ... ok` |

Release-build twin test (`apply_request_release_returns_err_on_frozen_violation`) is gated behind `#[cfg(not(debug_assertions))]` — compiles and runs only in release mode. Not exercised during `cargo test` (debug) but will fire in `cargo test --release`.

## What this unblocks

- Plan 01-03 can wire `FilterChain::apply_response_chunk` / `finalize_response` into `proxy.rs` knowing the request-side invariant is already enforced.
- Future egress filters (CCR in Phase 1B, annotation in Phase 3A, audit in later phases) inherit the frozen-prefix guarantee without per-filter work.

## Issues encountered

- Rust-analyzer reported spurious `unused_imports` on `use super::frozen` (line 7) and `second test attribute is supplied` (line 579). Both are false positives — `cargo clippy -- -D warnings` and `cargo build` both exit clean. No action needed.

## Self-Check: PASSED (verified by orchestrator spot-checks after executor was killed)
