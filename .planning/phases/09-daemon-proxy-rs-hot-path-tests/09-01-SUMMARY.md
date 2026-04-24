---
phase: 09-daemon-proxy-rs-hot-path-tests
plan: 01
subsystem: testing
tags: [async-trait, judge-client, trait-seam, mock, tokio, proptest]

# Dependency graph
requires:
  - phase: 07-crates-rigor-tests-integration-test-infrastructure
    provides: rigor-harness crate with IsolatedHome, MockLlmServer, TestProxy
  - phase: 08-home-rigor-test-isolation
    provides: rigor_home() RIGOR_HOME indirection for test isolation
provides:
  - JudgeClient trait seam abstracting LLM judge HTTP calls in proxy.rs
  - ReqwestJudgeClient production implementation wrapping reqwest::Client
  - judge_client field in DaemonState (Arc<dyn JudgeClient>)
  - 13 unit tests covering scope_judge_check, check_violations_persist, score_claim_relevance
  - MockJudgeClient test utility with call counting
  - clear_relevance_cache() and reset_relevance_semaphore() test helpers
  - proptest dev-dependency available for future property tests
affects: [09-02-PLAN, phase-10, phase-11, phase-12]

# Tech tracking
tech-stack:
  added: [proptest 1.x (dev-dep)]
  patterns: [JudgeClient trait seam for LLM call abstraction, MockJudgeClient with call counting]

key-files:
  created: []
  modified:
    - crates/rigor/src/daemon/proxy.rs
    - crates/rigor/src/daemon/mod.rs
    - crates/rigor/Cargo.toml

key-decisions:
  - "Tests placed in proxy.rs #[cfg(test)] mod tests (not separate integration test file) because #[cfg(test)] helpers and private functions are not visible to integration tests compiled as separate crates"
  - "JudgeClient, JudgeError, ReqwestJudgeClient made pub (not pub(crate)) because DaemonState is pub and its judge_client field must reference a pub trait"
  - "Three functions (scope_judge_check, check_violations_persist, score_claim_relevance) made pub for testability"
  - "Concurrency test uses tokio::sync::Barrier + try_acquire pattern matching production code, not proptest wrapper (simpler, deterministic)"

patterns-established:
  - "JudgeClient trait: &dyn JudgeClient replaces &reqwest::Client for all LLM judge calls"
  - "MockJudgeClient: returns canned serde_json::Value, counts calls via Arc<AtomicUsize>"
  - "Test isolation: clear_relevance_cache() + reset_relevance_semaphore() before each relevance test"

requirements-completed: []

# Metrics
duration: 13min
completed: 2026-04-24
---

# Phase 9 Plan 01: JudgeClient Trait Seam + Judge Function Unit Tests Summary

**JudgeClient trait seam abstracting 3 LLM judge functions behind async trait with 13 deterministic unit tests via MockJudgeClient**

## Performance

- **Duration:** 13 min
- **Started:** 2026-04-24T00:42:42Z
- **Completed:** 2026-04-24T00:55:59Z
- **Tasks:** 2
- **Files modified:** 4 (proxy.rs, mod.rs, Cargo.toml, Cargo.lock)

## Accomplishments
- Introduced JudgeClient trait seam: all 3 LLM judge functions now take &dyn JudgeClient instead of &reqwest::Client
- Updated all 5 call sites in proxy.rs to use DaemonState.judge_client
- Added 13 unit tests covering all decision branches (YES/NO/timeout/error/no-key/empty/caching/concurrency)
- All 323 tests pass (310 existing + 13 new), zero warnings, no flakiness

## Task Commits

Each task was committed atomically:

1. **Task 1: Add JudgeClient trait seam + proptest dep + cfg(test) helpers** - `2bc20cd` (feat)
2. **Task 2: Unit tests for scope_judge_check, check_violations_persist, score_claim_relevance** - `4e07ac4` (test)

## Files Created/Modified
- `crates/rigor/src/daemon/proxy.rs` - JudgeClient trait, JudgeError enum, ReqwestJudgeClient impl, function signature changes, call site updates, 13 unit tests, test helpers
- `crates/rigor/src/daemon/mod.rs` - judge_client: Arc<dyn JudgeClient> field in DaemonState, initialization in load() and empty()
- `crates/rigor/Cargo.toml` - Added proptest to dev-dependencies
- `Cargo.lock` - Updated with proptest dependency tree

## Decisions Made
- Tests placed in proxy.rs `#[cfg(test)] mod tests` block rather than separate `proxy_judge_tests.rs` file. Reason: integration tests in `crates/rigor/tests/` are compiled as separate crates and cannot see `#[cfg(test)]` helper functions (`clear_relevance_cache`, `reset_relevance_semaphore`) or private items. The plan explicitly anticipated this fallback.
- Used `tokio::sync::Barrier` for concurrent single-flight test instead of proptest async wrapper. The core invariant (AtomicBool CAS) is deterministic; proptest adds complexity with async without meaningful benefit for this specific test.
- Made `JudgeClient`, `JudgeError`, `ReqwestJudgeClient` fully `pub` rather than `pub(crate)` because `DaemonState` is `pub` and its `judge_client` field must reference a `pub` trait to compile.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Tests placed in mod tests instead of separate file**
- **Found during:** Task 2
- **Issue:** Integration tests in `crates/rigor/tests/` cannot see `#[cfg(test)]` items from the library crate (compiled as separate crate)
- **Fix:** Added tests to existing `#[cfg(test)] mod tests` block in proxy.rs as planned fallback (Option B)
- **Files modified:** crates/rigor/src/daemon/proxy.rs
- **Verification:** All 13 tests pass with `--test-threads=1`, no flakiness across 3 runs

---

**Total deviations:** 1 auto-fixed (Rule 3 - blocking)
**Impact on plan:** Minimal -- plan explicitly anticipated this fallback. Tests provide identical coverage regardless of file location.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- JudgeClient trait seam is in place for Plan 09-02 (proxy_request + extract_and_evaluate tests)
- MockJudgeClient pattern established for future test plans
- proptest available for property-based tests in future plans
- All 323 tests green

## Self-Check: PASSED

- All key files exist (proxy.rs, mod.rs, Cargo.toml, SUMMARY.md)
- Both commits found (2bc20cd, 4e07ac4)
- JudgeClient trait exists in proxy.rs
- 323 tests pass (310 existing + 13 new)

---
*Phase: 09-daemon-proxy-rs-hot-path-tests*
*Completed: 2026-04-24*
