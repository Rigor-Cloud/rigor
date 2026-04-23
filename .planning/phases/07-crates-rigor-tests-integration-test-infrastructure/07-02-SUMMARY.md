---
phase: 07-crates-rigor-tests-integration-test-infrastructure
plan: 02
subsystem: testing
tags: [rust, axum, tokio, integration-test, test-proxy, subprocess-harness]

requires:
  - phase: 07-01
    provides: "IsolatedHome, TestCA, MockLlmServer, SSE helpers"
provides:
  - "TestProxy: production daemon on ephemeral port with IsolatedHome for HOME isolation"
  - "Subprocess helpers: run_rigor, parse_response, extract_decision with HOME isolation"
  - "harness_smoke.rs: 6 integration tests proving all harness primitives compose correctly"
  - "rigor-harness as dev-dependency of rigor crate"
affects: [09-proxy-integration, 10-e2e-tests, 11-coverage, 12-regression]

tech-stack:
  added: []
  patterns: [spawn-blocking-env-isolation, runtime-binary-discovery, graceful-proxy-shutdown]

key-files:
  created:
    - crates/rigor-harness/src/proxy.rs
    - crates/rigor-harness/src/subprocess.rs
    - crates/rigor/tests/harness_smoke.rs
  modified:
    - crates/rigor-harness/Cargo.toml
    - crates/rigor-harness/src/lib.rs
    - crates/rigor/Cargo.toml

key-decisions:
  - "TestProxy uses spawn_blocking + env save/restore for HOME isolation during DaemonState::load"
  - "subprocess helpers use runtime binary discovery (CARGO_BIN_EXE_rigor -> RIGOR_BIN -> PATH) instead of compile-time env! macro"
  - "Minimal valid rigor.yaml uses ConstraintsSection struct format, not plain array"

patterns-established:
  - "spawn_blocking env isolation: save original env, set test values, call production code, restore originals"
  - "Runtime binary discovery: CARGO_BIN_EXE_rigor (test context) -> RIGOR_BIN (override) -> rigor (PATH)"
  - "TestProxy start_with_mock: combines RIGOR_TARGET_API + HOME isolation for mock-backed proxy tests"

requirements-completed: [REQ-015, REQ-016, REQ-017]

duration: 5min
completed: 2026-04-24
---

# Phase 7 Plan 2: TestProxy, Subprocess Helpers, and Smoke Tests Summary

**TestProxy wrapping production DaemonState+build_router on ephemeral port with HOME isolation, subprocess helpers, and 6-test smoke suite proving all harness primitives compose end-to-end**

## Performance

- **Duration:** 5 min
- **Started:** 2026-04-23T22:47:17Z
- **Completed:** 2026-04-23T22:52:42Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Built TestProxy that brings up the real production proxy (DaemonState + build_router) on an ephemeral port with IsolatedHome
- Built subprocess helpers (run_rigor, parse_response, extract_decision, default_hook_input) with safe HOME isolation
- Created 6 integration smoke tests exercising IsolatedHome, TestCA, MockLlmServer (Anthropic + OpenAI), subprocess helpers, and TestProxy
- Established rigor-harness as dev-dependency of rigor crate (no circular dep: dev-deps are not transitive)

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement TestProxy and subprocess helpers, wire rigor dependency** - `a6a968c` (feat)
2. **Task 2: Add rigor-harness dev-dep to rigor and create smoke integration test** - `3121f2d` (feat)

## Files Created/Modified
- `crates/rigor-harness/Cargo.toml` - Added rigor as regular dependency
- `crates/rigor-harness/src/lib.rs` - Added proxy and subprocess module declarations + re-exports
- `crates/rigor-harness/src/proxy.rs` - TestProxy with start(), start_with_mock(), Drop shutdown
- `crates/rigor-harness/src/subprocess.rs` - run_rigor, run_rigor_with_claims, run_rigor_with_env, parse_response, extract_decision, default_hook_input
- `crates/rigor/Cargo.toml` - Added rigor-harness as dev-dependency
- `crates/rigor/tests/harness_smoke.rs` - 6 integration tests for all harness primitives

## Decisions Made
- TestProxy uses `tokio::task::spawn_blocking` with env save/restore pattern for HOME isolation, avoiding unsafe global env mutation in async context
- Subprocess helpers use runtime binary discovery (`CARGO_BIN_EXE_rigor` at runtime, not `env!()` at compile time) because rigor-harness is a library, not an integration test binary
- Minimal valid rigor.yaml must use `constraints:` with `beliefs/justifications/defeaters` struct fields, not `constraints: []`

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Changed CARGO_BIN_EXE_rigor from compile-time to runtime lookup**
- **Found during:** Task 1 (subprocess.rs implementation)
- **Issue:** `env!("CARGO_BIN_EXE_rigor")` is only available in integration test binary context; rigor-harness is a library crate so the macro fails at compile time
- **Fix:** Changed to `std::env::var("CARGO_BIN_EXE_rigor")` at runtime with fallback chain: CARGO_BIN_EXE_rigor -> RIGOR_BIN -> "rigor" (PATH)
- **Files modified:** crates/rigor-harness/src/subprocess.rs
- **Verification:** cargo check -p rigor-harness succeeds
- **Committed in:** a6a968c (Task 1 commit)

**2. [Rule 1 - Bug] Fixed invalid rigor.yaml format in proxy tests**
- **Found during:** Task 1 (proxy.rs unit tests)
- **Issue:** `constraints: []` is invalid -- ConstraintsSection is a struct with beliefs/justifications/defeaters fields, not a plain array
- **Fix:** Changed to `constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n`
- **Files modified:** crates/rigor-harness/src/proxy.rs (test constants)
- **Verification:** cargo test -p rigor-harness passes (19 tests)
- **Committed in:** a6a968c (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Both fixes necessary for compilation and test correctness. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All harness primitives (IsolatedHome, TestCA, MockLlmServer, TestProxy, subprocess helpers) are exported from rigor-harness and ready for Phases 9-12
- TestProxy::start_with_mock enables mock-backed proxy tests for proxy integration (Phase 9)
- Subprocess helpers enable isolated binary invocation tests for regression suites (Phase 12)
- 25 total tests (19 unit + 6 integration) provide confidence in harness correctness

## Self-Check: PASSED

All 6 files verified present. Both commit hashes (a6a968c, 3121f2d) verified in git log.

---
*Phase: 07-crates-rigor-tests-integration-test-infrastructure*
*Completed: 2026-04-24*
