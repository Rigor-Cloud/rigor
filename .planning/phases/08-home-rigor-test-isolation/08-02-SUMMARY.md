---
phase: 08-home-rigor-test-isolation
plan: 02
subsystem: testing
tags: [rust, env-var, rigor-home, test-isolation, ci-guard, grep]

# Dependency graph
requires:
  - phase: 08-home-rigor-test-isolation
    plan: 01
    provides: "rigor_home() function with RIGOR_HOME env var override in paths.rs"
provides:
  - "TestProxy uses RIGOR_HOME (not HOME) for DaemonState::load isolation"
  - "CI grep guard preventing raw HOME usage regression in crates/rigor/src/"
affects: [future rigor crate contributors (CI guard enforces rigor_home() pattern)]

# Tech tracking
tech-stack:
  added: []
  patterns: ["RIGOR_HOME env var in TestProxy for narrower env mutation blast radius", "CI grep guard with rigor-home-ok allowlist"]

key-files:
  created: []
  modified:
    - "crates/rigor-harness/src/proxy.rs"
    - ".github/workflows/ci.yml"

key-decisions:
  - "RIGOR_HOME set to rigor_dir_str() (the .rigor/ subdir) not home_str() (the parent temp dir) -- matches rigor_home() semantics"
  - "CI guard added as step in existing clippy job, not a separate job -- zero-cost grep in milliseconds"

patterns-established:
  - "TestProxy env isolation: set RIGOR_HOME to IsolatedHome::rigor_dir_str() instead of mutating HOME"
  - "CI regression guard: grep for raw HOME patterns, exclude paths.rs and rigor-home-ok annotations"

requirements-completed: [REQ-018]

# Metrics
duration: 15min
completed: 2026-04-24
---

# Phase 8 Plan 2: TestProxy RIGOR_HOME Switch + CI Guard Summary

**TestProxy switched from HOME to RIGOR_HOME env var for .rigor/ path isolation, plus CI grep guard preventing raw HOME usage regression**

## Performance

- **Duration:** 15 min
- **Started:** 2026-04-23T23:56:33Z
- **Completed:** 2026-04-24T00:12:19Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Switched TestProxy::start() and start_with_mock() from mutating HOME to mutating RIGOR_HOME, reducing env mutation blast radius to only .rigor/ paths
- RIGOR_HOME is set to IsolatedHome::rigor_dir_str() (the .rigor/ subdirectory), matching rigor_home() resolution semantics
- Added CI grep guard step in clippy job that catches dirs::home_dir, env::var("HOME"), and env::var_os("HOME") outside paths.rs
- All 390 tests pass (310 lib + 19 harness + 6 harness_smoke + 55 integration)

## Task Commits

Each task was committed atomically:

1. **Task 1: Update TestProxy to use RIGOR_HOME instead of HOME** - `c7e2c56` (feat)
2. **Task 2: Add CI grep guard preventing raw HOME usage** - `ef8deae` (chore)

## Files Created/Modified
- `crates/rigor-harness/src/proxy.rs` - TestProxy::start() and start_with_mock() use RIGOR_HOME env var with rigor_dir_str() value
- `.github/workflows/ci.yml` - New "Guard against raw HOME for .rigor paths" step in clippy job

## Decisions Made
- RIGOR_HOME is set to `home.rigor_dir_str()` (the `.rigor/` subdir path) rather than `home.home_str()` (the parent temp dir), because `rigor_home()` returns the `.rigor/` directory directly when RIGOR_HOME is set
- CI guard placed as a step in the existing clippy job rather than a separate job, since it is a zero-cost grep that runs in milliseconds

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## Known Stubs
None -- all paths are fully wired.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 8 is fully complete: rigor_home() indirection (Plan 01) + TestProxy RIGOR_HOME switch + CI guard (Plan 02)
- REQ-018 is satisfied: no test writes to real ~/.rigor/, CI prevents regression
- All contributors must use crate::paths::rigor_home() for .rigor/ paths; CI will reject raw HOME access

## Self-Check: PASSED

- All files exist (proxy.rs, ci.yml, 08-02-SUMMARY.md)
- All commits found (c7e2c56, ef8deae)
- RIGOR_HOME present in proxy.rs
- CI guard step present in ci.yml
- rigor-home-ok allowlist present in ci.yml
- Zero local grep violations

---
*Phase: 08-home-rigor-test-isolation*
*Completed: 2026-04-24*
