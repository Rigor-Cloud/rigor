---
phase: 01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon
plan: 01
subsystem: infra
tags: [egress, filter-chain, xxhash, twox-hash, invariant, serde_json, tdd]

# Dependency graph
requires:
  - phase: pre-existing
    provides: "EgressFilter trait, FilterChain, FilterError enum, ConversationCtx with scratch_set/scratch_get (crates/rigor/src/daemon/egress/{chain,ctx}.rs)"
provides:
  - "FrozenPrefix struct stored in ConversationCtx::scratch"
  - "compute_checksum(&[Json]) -> u64 — xxhash64 over canonical serde_json bytes, separator-safe"
  - "set_frozen_prefix(&mut ctx, &msgs, freeze_count) — seals first N messages with freeze_count clamped to msgs.len()"
  - "verify_frozen_prefix(&ctx, &msgs) -> Result<(), FilterError> — backward-compat no-op if unsealed, otherwise FilterError::Internal on mismatch or truncation"
  - "twox-hash 2.x dependency (xxhash64 only, no rand)"
affects:
  - "phase 01 plan 02 (wire verify_frozen_prefix into FilterChain::apply_request post-chain verifier)"
  - "phase 01 plan 03 (proxy.rs — seal frozen prefix before request filters run)"
  - "phase 01 plan 04 (egress_integration.rs — new tests asserting filters cannot mutate frozen range)"
  - "phase 1B CCR retrieval loop"
  - "phase 3A annotation emission from proxy"

# Tech tracking
tech-stack:
  added:
    - "twox-hash = \"2.0\" (xxhash64 feature only, default-features = false to exclude rand)"
  patterns:
    - "Scratch-map invariant pattern: typed data stored in ConversationCtx::scratch, verified post-mutation"
    - "TDD with todo!() stubs in RED phase so tests panic deterministically (not compile-fail)"
    - "Fail-closed verifier returns FilterError::Internal for caller to decide debug-panic vs warn+reject"

key-files:
  created:
    - "crates/rigor/src/daemon/egress/frozen.rs (203 lines, 8 unit tests)"
  modified:
    - "crates/rigor/Cargo.toml (+1 line: twox-hash dep)"
    - "crates/rigor/src/daemon/egress/mod.rs (+2 lines: pub mod frozen; pub use frozen::*;)"

key-decisions:
  - "Hash function: twox-hash::XxHash64 (non-cryptographic, fast). Not sha2 (sha2 remains reserved for content-addressing)."
  - "twox-hash configured with default-features = false + features = [\"xxhash64\"] to exclude the rand dependency that twox-hash 2.x ships by default."
  - "serde_json::to_vec over the whole message Value (no manual canonicalization). Deterministic for a given Value because serde_json preserves the internal map order. A 0u8 separator is written between per-message byte slices to eliminate boundary-collision ambiguity (\"ab\"||\"c\" vs \"a\"||\"bc\")."
  - "freeze_count clamped to messages.len() in set_frozen_prefix — callers cannot seal a range larger than what they passed."
  - "Backward compat: verify_frozen_prefix is a no-op (returns Ok(())) when no FrozenPrefix exists in scratch. Existing egress_integration tests continue passing with zero changes."
  - "Added 1 test beyond the 7 specified in the plan: set_frozen_prefix_clamps_to_messages_len — documents and locks the clamping semantics chosen above."

patterns-established:
  - "TDD stub pattern: function-body todo!() macros in RED phase produce panics with clear messages; tests then pass in GREEN when real impl replaces the todo!()s."
  - "xxhash64 as rigor's standard non-cryptographic checksum (distinct from sha2 for content-addressing)."

requirements-completed: [REQ-001]

# Metrics
duration: ~34 min
completed: 2026-04-22
---

# Phase 01 Plan 01: Frozen-prefix invariant module Summary

**FrozenPrefix invariant module: xxhash64-based checksum over the first N messages, sealed in ConversationCtx::scratch, with a post-chain verifier that returns FilterError::Internal on mismatch — TDD-authored with 8 passing unit tests.**

## Performance

- **Duration:** ~34 min
- **Started:** 2026-04-22T17:45:00Z (approx — task 1 first bash)
- **Completed:** 2026-04-22T18:19:00Z
- **Tasks:** 2 (plan has 2 `<task>` blocks; task 2 has a RED+GREEN split per TDD)
- **Files modified:** 3 (Cargo.toml, egress/mod.rs, egress/frozen.rs [created])

## Accomplishments

- New `crates/rigor/src/daemon/egress/frozen.rs` module exposing `FrozenPrefix`, `compute_checksum`, `set_frozen_prefix`, `verify_frozen_prefix`
- 8 unit tests covering determinism, content-sensitivity, round-trip, tamper detection, tail-mutation safety, backward compat (no-op when unsealed), bounds checking, and clamp semantics — all pass
- twox-hash 2.x wired in with the minimal feature set (`xxhash64` only) to avoid pulling in `rand`
- Module exported via `egress/mod.rs` so downstream plans can `use rigor::daemon::egress::{FrozenPrefix, set_frozen_prefix, verify_frozen_prefix}`
- Strict TDD cycle (RED commit precedes GREEN commit in git log) — plan-checker TDD gate satisfied
- Zero regression: existing `egress_integration` tests still pass (2/2)
- Zero clippy warnings across `--all-targets --all-features`
- `cargo fmt -- --check` clean

## Task Commits

Each task was committed atomically, with TDD splitting Task 2 into RED + GREEN:

1. **Task 1: Add twox-hash dependency** — `a22ca2e` (chore)
2. **Task 2 (RED): Add failing tests for frozen-prefix invariant** — `f611f83` (test)
3. **Task 2 (GREEN): Implement frozen-prefix invariant module** — `535fb91` (feat)

No separate REFACTOR commit was needed — the GREEN implementation was already clean enough to pass clippy + fmt without additional cleanup. The `canonical_bytes` helper mentioned in the plan was deliberately not extracted because `compute_checksum` is the only call site.

## Files Created/Modified

- `crates/rigor/src/daemon/egress/frozen.rs` *(created, 203 lines)* — FrozenPrefix struct + 3 public functions + 8 `#[cfg(test)] mod tests` cases
- `crates/rigor/src/daemon/egress/mod.rs` *(+2 lines)* — added `pub mod frozen;` and `pub use frozen::*;`
- `crates/rigor/Cargo.toml` *(+1 line)* — `twox-hash = { version = "2.0", default-features = false, features = ["xxhash64"] }`
- `Cargo.lock` *(auto-updated)* — twox-hash v2.1.2 locked

## Decisions Made

1. **Hash function — twox-hash (xxhash64).** The plan's CONTEXT.md explicitly locks this as the default pick. `sha2` remains reserved for content-addressing (per project verified-truth: "Content Addressing Uses SHA-256"). xxhash64 is non-cryptographic, fast, and deterministic across runs.

2. **`default-features = false, features = ["xxhash64"]`.** twox-hash 2.x's default feature set pulls in `rand`. We only need xxhash64, so we exclude the rest. Rationale echoed in the Cargo.toml and in the Task 1 commit message.

3. **`serde_json::to_vec` + `0u8` separator.** No manual canonicalization. The plan accepts that serde_json's own iteration order is stable for a given `Value`. A `0u8` byte is written between per-message bytes to avoid the boundary-ambiguity attack (`"ab"||"c"` vs `"a"||"bc"`).

4. **`freeze_count` clamped to `messages.len()`.** Defensive choice — if a caller passes `99` but only provides 3 messages, we seal the 3 they have instead of panicking. This is verified by the `set_frozen_prefix_clamps_to_messages_len` test (the +1 test beyond the plan's 7).

5. **Backward-compat no-op.** `verify_frozen_prefix` returns `Ok(())` when no `FrozenPrefix` is in scratch — existing flows that never call `set_frozen_prefix` continue to work unchanged. The existing `egress_integration` tests confirm this.

6. **Error variant — `FilterError::Internal` (not `::Blocked`).** A checksum mismatch is a rigor-internal invariant failure, not the upstream LLM provider blocking. `Internal` makes the caller (FilterChain in plan 02) choose the right posture: `panic!` in debug, `warn!` + reject in release.

## Deviations from Plan

**None auto-fixed.** The plan was executed as specified, with one small additive enhancement:

### Additive: 8th test case beyond plan's 7

- **Added during:** Task 2 (RED phase)
- **Addition:** `set_frozen_prefix_clamps_to_messages_len` test
- **Why:** The plan's `<action>` block explicitly includes this test in its code sketch (see plan line 372-384), even though only 7 bulleted behaviors are listed in `<behavior>`. The plan-file action-block is the authoritative source, so the 8th test was included.
- **Impact:** None negative — strictly more coverage.

No Rule 1 / Rule 2 / Rule 3 / Rule 4 fixes were needed. No architectural changes.

---

**Total deviations:** 0 auto-fixed. 1 additive test beyond the 7 listed in the plan's behavior bullets (but included in the plan's action code sketch).
**Impact on plan:** Plan executed exactly as written. Downstream plans (02, 03, 04) can now import the three functions verbatim.

## Issues Encountered

- **`cargo fmt -- --check` failed once** after the initial GREEN implementation: rustfmt preferred one-line signatures for functions with two parameters. Resolved by running `cargo fmt -p rigor` before commit. No logic change. Included in the GREEN commit.

## TDD Gate Compliance

- **RED commit:** `f611f83` — `test(01-01): add failing tests for frozen-prefix invariant` — 8/8 tests panic with `todo!()`
- **GREEN commit:** `535fb91` — `feat(01-01): implement frozen-prefix invariant module` — 8/8 tests pass
- **REFACTOR commit:** not needed (GREEN passed clippy + fmt without further cleanup)
- **Gate order:** RED precedes GREEN in `git log` — verified via `git log --oneline | head -5`

## User Setup Required

None — no external service configuration needed.

## Next Phase Readiness

- **Plan 02 (wire into FilterChain::apply_request):** Ready. `verify_frozen_prefix` is the single call it needs to insert after the filter loop in `FilterChain::apply_request`.
- **Plan 03 (wire FilterChain into response path in proxy.rs):** Ready. It will additionally need to call `set_frozen_prefix(&mut ctx, &msgs, count)` before the request chain runs; the API exists and is tested.
- **Plan 04 (integration tests):** Ready. `tests/egress_integration.rs` can add a mock filter that mutates `messages[0]` after `set_frozen_prefix(count=1)` and assert `apply_request` → `Err(FilterError::Internal { filter: "frozen_prefix", .. })`.

## Self-Check: PASSED

Verified before final commit:

- `test -f crates/rigor/src/daemon/egress/frozen.rs` → EXISTS
- `grep -c 'pub struct FrozenPrefix' frozen.rs` → 1
- `grep -c 'pub fn compute_checksum' frozen.rs` → 1
- `grep -c 'pub fn set_frozen_prefix' frozen.rs` → 1
- `grep -c 'pub fn verify_frozen_prefix' frozen.rs` → 1
- `grep -c 'pub mod frozen' egress/mod.rs` → 1
- `grep -c 'pub use frozen::\*' egress/mod.rs` → 1
- `cargo test --lib -p rigor daemon::egress::frozen` → 8 passed, 0 failed
- `cargo clippy --all-targets --all-features -- -D warnings` → clean
- `cargo fmt -- --check` → clean
- `cargo test --test egress_integration -p rigor` → 2 passed (no regression)
- TDD gate: `git log | grep -E '^[a-f0-9]+ (test|feat)\(01-01\)'` shows `test(` precedes `feat(`

---
*Phase: 01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon*
*Plan: 01*
*Completed: 2026-04-22*
