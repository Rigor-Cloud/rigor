---
phase: 10-unit-coverage-gaps
plan: 01
subsystem: testing
tags: [rust, unit-tests, tls, mitm, sni, pid-lifecycle, daemon, security]

# Dependency graph
requires:
  - phase: 08-rigor-home-test-isolation
    provides: RIGOR_HOME env var isolation pattern for test fixtures
provides:
  - 13 MITM allowlist + PID lifecycle tests in daemon/mod.rs
  - 6 TLS CA generation + leaf cert signing tests in daemon/tls.rs
  - 7 SNI extraction edge case tests in daemon/sni.rs
  - Crate-wide RIGOR_HOME_TEST_LOCK for env var serialization
affects: [10-02, 10-03, daemon-security, test-infrastructure]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Shared crate-wide RIGOR_HOME_TEST_LOCK in paths.rs for cross-module env var serialization"
    - "with_temp_rigor_home helper pattern for PID/CA filesystem tests"
    - "build_client_hello_with_extensions helper for SNI byte construction tests"
    - "wrap_in_tls_record helper for async peek_client_hello tests"

key-files:
  created: []
  modified:
    - crates/rigor/src/daemon/mod.rs
    - crates/rigor/src/daemon/tls.rs
    - crates/rigor/src/daemon/sni.rs
    - crates/rigor/src/paths.rs

key-decisions:
  - "Unified RIGOR_HOME env var lock (RIGOR_HOME_TEST_LOCK) across all test modules to prevent parallel test races"
  - "Used Arc pointer equality to verify server_config_for_host caching behavior"
  - "PID 2000000 used as dead-PID sentinel (exceeds typical OS PID ranges)"

patterns-established:
  - "Crate-wide env lock: all tests mutating RIGOR_HOME use crate::paths::RIGOR_HOME_TEST_LOCK"
  - "with_temp_rigor_home helper: save/set/restore env var with poison-recovery"
  - "build_client_hello_with_extensions: composable TLS ClientHello byte builder for SNI tests"

requirements-completed: [REQ-020]

# Metrics
duration: 9min
completed: 2026-04-24
---

# Phase 10 Plan 01: Daemon Module Unit Coverage Summary

**26 unit tests covering MITM allowlist routing, PID lifecycle, TLS CA generation/caching, and SNI extraction edge cases with unified env var serialization**

## Performance

- **Duration:** 9 min
- **Started:** 2026-04-24T01:55:32Z
- **Completed:** 2026-04-24T02:05:01Z
- **Tasks:** 3
- **Files modified:** 4

## Accomplishments

- 13 MITM allowlist + PID lifecycle tests in daemon/mod.rs covering exact/suffix match, disabled flag, empty target, no-port, subdomain suffix, write/remove PID, alive/dead/garbage detection
- 6 TLS CA tests in daemon/tls.rs covering generate-new, load-existing roundtrip, server_config_for_host generation + caching, install_ca_trust error path, legacy self-signed config
- 7 SNI edge case tests in daemon/sni.rs covering truncated records, ALPN alongside SNI, missing SNI extension, two truncation points, async peek_client_hello, non-TLS record
- Unified RIGOR_HOME_TEST_LOCK prevents env var races across test modules running in parallel

## Task Commits

Each task was committed atomically:

1. **Task 1: MITM allowlist + PID lifecycle tests in daemon/mod.rs** - `648b56f` (test)
2. **Task 2: TLS CA generation + leaf cert tests in daemon/tls.rs** - `0d14812` (test)
3. **Task 3: SNI extraction edge case tests in daemon/sni.rs** - `0d87ba6` (test)
4. **Fix: Unified RIGOR_HOME env var lock** - `ef7e5b2` (fix)

## Files Created/Modified

- `crates/rigor/src/daemon/mod.rs` - Added #[cfg(test)] mod tests with 13 tests (7 MITM + 6 PID)
- `crates/rigor/src/daemon/tls.rs` - Added #[cfg(test)] mod tests with 6 TLS CA tests
- `crates/rigor/src/daemon/sni.rs` - Expanded existing mod tests from 2 to 9 tests (7 new)
- `crates/rigor/src/paths.rs` - Added crate-wide RIGOR_HOME_TEST_LOCK, migrated existing tests to shared lock

## Decisions Made

- Unified RIGOR_HOME env var lock: Each module previously had its own ENV_LOCK mutex, but they all protected the same global resource. Created a single crate-wide lock to prevent races.
- Used Arc pointer equality for cache verification: server_config_for_host caching tested by asserting `Arc::ptr_eq` on two calls with the same hostname.
- PID 2000000 as dead-PID sentinel: Exceeds typical OS PID ranges, reliable across macOS and Linux.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed env var race causing parallel test failures**
- **Found during:** Post-Task 3 full suite verification
- **Issue:** daemon/mod.rs, daemon/tls.rs, and paths.rs each had independent ENV_LOCK mutexes protecting the same global RIGOR_HOME env var. When tests from different modules ran in parallel, they raced on the env var, causing assertion failures and mutex poisoning.
- **Fix:** Added `pub(crate) RIGOR_HOME_TEST_LOCK` in paths.rs (#[cfg(test)]), migrated all test modules to use the shared lock with poison recovery.
- **Files modified:** crates/rigor/src/paths.rs, crates/rigor/src/daemon/mod.rs, crates/rigor/src/daemon/tls.rs
- **Verification:** Full test suite (349 tests) passes consistently with default parallel execution.
- **Committed in:** `ef7e5b2`

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Essential fix for test reliability. The pre-existing paths.rs tests also benefited from the shared lock. No scope creep.

## Issues Encountered

- Pre-existing flaky test `daemon::proxy::tests::score_claim_relevance_caching` occasionally fails when run in parallel (not caused by this plan's changes). Passes when run in isolation. Logged as out-of-scope.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Daemon module test coverage gaps (1-4) closed
- 349 total tests pass (323 pre-existing + 26 new)
- Shared RIGOR_HOME_TEST_LOCK pattern established for future test modules
- Ready for 10-02 (evaluator/constraint/violation tests) and 10-03 (content store/gate tests)

---
*Phase: 10-unit-coverage-gaps*
*Completed: 2026-04-24*
