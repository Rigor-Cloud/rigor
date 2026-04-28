---
phase: 11-e2e-coverage-gaps
plan: 02
subsystem: testing
tags: [stop-hook, pid, daemon, e2e, subprocess, rigor-harness, lifecycle, crash-recovery]

# Dependency graph
requires:
  - phase: 07-crates-rigor-tests-integration-test-infrastructure
    provides: IsolatedHome, subprocess helpers (run_rigor, run_rigor_with_claims, parse_response, default_hook_input)
provides:
  - E2E stop-hook tests proving harness subprocess helpers compose with real constraint pipeline
  - E2E PID file crash recovery lifecycle tests covering write-crash-detect_stale-rewrite-cleanup
affects: [12-b1-b2-b3-integration, daemon-lifecycle, stop-hook-coverage]

# Tech tracking
tech-stack:
  added: []
  patterns: [RIGOR_HOME env var isolation via local mutex + tempdir for integration tests, RIGOR_TEST_CLAIMS env var override for deterministic claim injection in subprocess tests]

key-files:
  created:
    - crates/rigor/tests/stop_hook_e2e.rs
    - crates/rigor/tests/pid_lifecycle_e2e.rs
  modified: []

key-decisions:
  - "Used local PID_TEST_LOCK mutex in integration test since RIGOR_HOME_TEST_LOCK is pub(crate) and inaccessible from tests/ directory"
  - "PID 2000000 as dead-PID sentinel (consistent with existing daemon/mod.rs unit tests)"
  - "RIGOR_HOME set to tempdir root (not a .rigor subdir) since rigor_home() returns RIGOR_HOME value as-is"

patterns-established:
  - "Stop-hook E2E test pattern: IsolatedHome + write_rigor_yaml + run_rigor_with_claims for deterministic constraint evaluation"
  - "PID lifecycle E2E test pattern: local mutex + RIGOR_HOME env var + tempdir for serialized daemon function testing"

requirements-completed: [REQ-021]

# Metrics
duration: 4min
completed: 2026-04-24
---

# Phase 11 Plan 02: Stop-Hook & PID Lifecycle E2E Tests Summary

**7 E2E tests covering stop-hook constraint evaluation via rigor-harness subprocess helpers and PID file crash recovery lifecycle with RIGOR_HOME isolation**

## Performance

- **Duration:** 4 min
- **Started:** 2026-04-24T02:54:39Z
- **Completed:** 2026-04-24T02:59:20Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- 4 stop-hook E2E tests proving harness subprocess helpers (run_rigor, run_rigor_with_claims, parse_response, extract_decision) compose correctly with real constraint pipelines: block on keyword match, allow on clean claim, allow with no constraints, metadata version present
- 3 PID lifecycle E2E tests covering the full write-crash-detect_stale-rewrite-cleanup lifecycle, directory auto-creation, and atomic stale PID overwrite -- gaps not covered by existing daemon/mod.rs unit tests
- All 371 existing lib tests continue to pass (zero regression)

## Task Commits

Each task was committed atomically:

1. **Task 1: Create stop_hook_e2e.rs with harness-based stop-hook tests** - `aaf5b58` (test)
2. **Task 2: Create pid_lifecycle_e2e.rs with PID crash recovery lifecycle tests** - `35fa7c1` (test)

## Files Created/Modified
- `crates/rigor/tests/stop_hook_e2e.rs` - 144 lines: 4 E2E tests for stop-hook evaluation path via rigor-harness subprocess helpers
- `crates/rigor/tests/pid_lifecycle_e2e.rs` - 156 lines: 3 E2E tests for PID file crash recovery lifecycle with RIGOR_HOME isolation

## Decisions Made
- **Local PID_TEST_LOCK mutex:** Integration tests in `tests/` cannot access `pub(crate) RIGOR_HOME_TEST_LOCK` from `paths.rs`, so a local static mutex is defined in `pid_lifecycle_e2e.rs` for serialization.
- **PID 2000000 sentinel:** Consistent with existing `daemon/mod.rs` unit tests -- exceeds typical OS PID ranges and is extremely unlikely to be a real running process.
- **RIGOR_HOME = tempdir root:** `rigor_home()` returns the `RIGOR_HOME` env var value as-is, so the env var points directly to the temp directory where `daemon.pid` will be written (not to a `.rigor` subdir).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Removed unused import `daemon_pid_file`**
- **Found during:** Task 2
- **Issue:** `daemon_pid_file` was imported but not used in any test, causing a compiler warning
- **Fix:** Removed the unused import from the `use` statement
- **Files modified:** `crates/rigor/tests/pid_lifecycle_e2e.rs`
- **Committed in:** `35fa7c1` (part of Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Trivial unused-import cleanup. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 11 (e2e-coverage-gaps) is now complete: both plans (01: CONNECT tunnel, 02: stop-hook & PID lifecycle) are done
- All E2E coverage gaps identified in the research phase are now addressed
- Ready for Phase 12 (B1/B2/B3 integration tests) which can build on the harness primitives exercised here

## Self-Check: PASSED

- FOUND: crates/rigor/tests/stop_hook_e2e.rs
- FOUND: crates/rigor/tests/pid_lifecycle_e2e.rs
- FOUND: commit aaf5b58
- FOUND: commit 35fa7c1

---
*Phase: 11-e2e-coverage-gaps*
*Completed: 2026-04-24*
