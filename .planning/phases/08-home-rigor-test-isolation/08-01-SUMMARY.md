---
phase: 08-home-rigor-test-isolation
plan: 01
subsystem: testing
tags: [rust, env-var, path-indirection, test-isolation, rigor-home]

# Dependency graph
requires:
  - phase: 07-crates-rigor-tests-integration-test-infrastructure
    provides: IsolatedHome fixture in rigor-harness
provides:
  - "rigor_home() function with RIGOR_HOME env var override"
  - "All 17 .rigor/ path call sites routed through single indirection"
  - "Category B (non-.rigor/) HOME usages annotated with // rigor-home-ok"
affects: [08-02 (CI grep guard), rigor-harness TestProxy (RIGOR_HOME migration)]

# Tech tracking
tech-stack:
  added: []
  patterns: ["RIGOR_HOME env var override for .rigor/ path resolution (XDG-style)"]

key-files:
  created:
    - "crates/rigor/src/paths.rs"
  modified:
    - "crates/rigor/src/lib.rs"
    - "crates/rigor/src/daemon/mod.rs"
    - "crates/rigor/src/daemon/tls.rs"
    - "crates/rigor/src/logging/violation_log.rs"
    - "crates/rigor/src/logging/session_registry.rs"
    - "crates/rigor/src/observability/tracing.rs"
    - "crates/rigor/src/alerting/mod.rs"
    - "crates/rigor/src/memory/episodic.rs"
    - "crates/rigor/src/cli/config.rs"
    - "crates/rigor/src/cli/serve.rs"
    - "crates/rigor/src/cli/refine.rs"
    - "crates/rigor/src/cli/eval.rs"
    - "crates/rigor/src/cli/trust.rs"
    - "crates/rigor/src/cli/gate.rs"
    - "crates/rigor/src/cli/scan.rs"

key-decisions:
  - "rigor_home() panics on failure rather than returning Result to avoid cascading signature changes"
  - "Option<PathBuf> return types preserved with Some(rigor_home()...) wrapping to minimize caller changes"
  - "Unused Context imports removed from trust.rs and episodic.rs as direct consequence of the path swap"

patterns-established:
  - "RIGOR_HOME env var override: all .rigor/ paths must go through crate::paths::rigor_home()"
  - "// rigor-home-ok annotation: marks legitimate non-.rigor HOME usage for CI grep guard"

requirements-completed: [REQ-018]

# Metrics
duration: 17min
completed: 2026-04-24
---

# Phase 8 Plan 1: rigor_home() Indirection Summary

**Centralized ~/.rigor/ path resolution behind rigor_home() with RIGOR_HOME env var override enabling test isolation without unsafe global HOME mutation**

## Performance

- **Duration:** 17 min
- **Started:** 2026-04-23T23:35:26Z
- **Completed:** 2026-04-23T23:52:25Z
- **Tasks:** 2
- **Files modified:** 16 (1 created, 15 modified)

## Accomplishments
- Created `crate::paths::rigor_home()` with RIGOR_HOME env var override and dirs::home_dir()/.rigor fallback
- Replaced all 17 Category A call sites across 12 files with mechanical path swaps
- Annotated all 4 Category B sites with `// rigor-home-ok` for CI grep guard compatibility
- All 310 lib tests + 7 integration tests pass, zero warnings

## Task Commits

Each task was committed atomically:

1. **Task 1: Create paths.rs with rigor_home() and unit tests (TDD)**
   - `57bf7cf` (test: add failing tests for rigor_home() -- RED phase)
   - `5d91813` (feat: implement rigor_home() with RIGOR_HOME env var override -- GREEN phase)

2. **Task 2: Replace all 17 Category A call sites, annotate Category B** - `3591afb` (feat)

## Files Created/Modified
- `crates/rigor/src/paths.rs` - New module: rigor_home() with RIGOR_HOME override + 4 unit tests
- `crates/rigor/src/lib.rs` - Added `pub mod paths;` declaration
- `crates/rigor/src/daemon/mod.rs` - daemon_pid_file() uses rigor_home()
- `crates/rigor/src/daemon/tls.rs` - ca_cert_path(), ca_key_path() use rigor_home(); install_ca_trust annotated rigor-home-ok
- `crates/rigor/src/logging/violation_log.rs` - ViolationLogger::new() uses rigor_home()
- `crates/rigor/src/logging/session_registry.rs` - registry_path(), session_log_dir() use rigor_home()
- `crates/rigor/src/observability/tracing.rs` - init_tracing() log directory uses rigor_home()
- `crates/rigor/src/alerting/mod.rs` - alerts_path() uses rigor_home()
- `crates/rigor/src/memory/episodic.rs` - MemoryStore::path() uses rigor_home()
- `crates/rigor/src/cli/config.rs` - config_path() uses rigor_home()
- `crates/rigor/src/cli/serve.rs` - serve_pid_file(), run_background() use rigor_home()
- `crates/rigor/src/cli/refine.rs` - rigor_dir() uses rigor_home()
- `crates/rigor/src/cli/eval.rs` - rigor_dir() uses rigor_home()
- `crates/rigor/src/cli/trust.rs` - rigor_bin_dir(), ensure_ca_bundle(), is_rigor_bin_in_path() use rigor_home(); shell_profile_path annotated rigor-home-ok
- `crates/rigor/src/cli/gate.rs` - claude_settings_path() annotated rigor-home-ok
- `crates/rigor/src/cli/scan.rs` - claude_settings_path() annotated rigor-home-ok

## Decisions Made
- rigor_home() panics on failure (via expect()) rather than returning Result<PathBuf>, to avoid cascading function signature changes across 17 call sites -- matches the over-editing guard constraint
- Option<PathBuf> return types (daemon_pid_file, serve_pid_file, registry_path, session_log_dir) preserved by wrapping in Some() -- avoids caller code changes
- Removed now-unused `Context` imports from trust.rs and episodic.rs since the `.context("...")` calls were eliminated with the path swap

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Removed unused Context imports**
- **Found during:** Task 2 (call site replacement)
- **Issue:** After removing `dirs::home_dir().context(...)` calls, the `Context` trait import became unused, causing compiler warnings
- **Fix:** Removed `Context` from `use anyhow::{Context, Result}` in trust.rs and episodic.rs
- **Files modified:** crates/rigor/src/cli/trust.rs, crates/rigor/src/memory/episodic.rs
- **Verification:** `cargo check -p rigor --all-features` produces zero warnings
- **Committed in:** 3591afb (part of Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 Rule 1 bug)
**Impact on plan:** Minimal -- removed 2 unused imports caused directly by the path swap. No scope creep.

## TDD Gate Compliance

- RED gate: `57bf7cf` (test commit with todo!() stub -- all 4 tests fail)
- GREEN gate: `5d91813` (feat commit implementing rigor_home() -- all 4 tests pass)
- REFACTOR gate: Not needed (implementation is minimal and clean)

## Issues Encountered
None

## Known Stubs
None -- all paths are fully wired through rigor_home().

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- rigor_home() indirection is complete -- Plan 08-02 (CI grep guard + TestProxy migration) can proceed
- RIGOR_HOME env var is ready for use by rigor-harness IsolatedHome/TestProxy
- CI grep guard pattern verified: `grep -rn 'dirs::home_dir|env::var("HOME")|env::var_os("HOME")' crates/rigor/src/ --include='*.rs' | grep -v 'src/paths.rs' | grep -v 'rigor-home-ok'` returns empty

## Self-Check: PASSED

- All files exist (paths.rs, lib.rs)
- All commits found (57bf7cf, 5d91813, 3591afb)
- pub fn rigor_home present in paths.rs
- pub mod paths present in lib.rs
- CI grep guard: zero violations
- Category B annotations: 4/4

---
*Phase: 08-home-rigor-test-isolation*
*Completed: 2026-04-24*
