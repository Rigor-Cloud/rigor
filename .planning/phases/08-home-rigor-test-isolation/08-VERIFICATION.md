---
phase: 08-home-rigor-test-isolation
verified: 2026-04-24T06:15:00Z
status: passed
score: 9/9
overrides_applied: 0
---

# Phase 8: $HOME/.rigor Test Isolation Verification Report

**Phase Goal:** Tests must not touch the real `$HOME/.rigor` (PID file, CA cert, violations log). Use `TempDir` fixtures.
**Verified:** 2026-04-24T06:15:00Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | rigor_home() returns RIGOR_HOME path when env var is set and non-empty | VERIFIED | paths.rs:12-16 checks `std::env::var("RIGOR_HOME")`, returns `PathBuf::from(val)` if non-empty. Test `rigor_home_returns_rigor_home_env_when_set` passes. |
| 2 | rigor_home() falls back to dirs::home_dir()/.rigor when RIGOR_HOME is unset | VERIFIED | paths.rs:17-19 falls back to `dirs::home_dir().join(".rigor")`. Tests `rigor_home_falls_back_when_unset` and `rigor_home_ignores_empty_rigor_home_env` both pass. |
| 3 | All 17 Category A call sites use crate::paths::rigor_home() | VERIFIED | `grep -rn 'crate::paths::rigor_home' crates/rigor/src/ | grep -v paths.rs | wc -l` returns 17. All 12 source files confirmed: daemon/mod.rs(1), daemon/tls.rs(2), logging/violation_log.rs(1), logging/session_registry.rs(2), observability/tracing.rs(1), alerting/mod.rs(1), memory/episodic.rs(1), cli/config.rs(1), cli/serve.rs(2), cli/refine.rs(1), cli/eval.rs(1), cli/trust.rs(3). |
| 4 | Category B call sites are unchanged with '// rigor-home-ok' comments | VERIFIED | 4 annotations found: gate.rs:516 (.claude/settings.json), scan.rs:190 (.claude/settings.json), trust.rs:48 (.zshrc/.bashrc), tls.rs:193 (Library/Keychains). All are non-.rigor HOME usages. |
| 5 | No function signatures changed beyond path resolution swap | VERIFIED | `git diff` of all 14 modified files shows zero function signature changes (no added/removed `fn` lines). |
| 6 | TestProxy sets RIGOR_HOME (not HOME) for DaemonState::load isolation | VERIFIED | proxy.rs uses `std::env::set_var("RIGOR_HOME", &rigor_home_str)` in both start() and start_with_mock(). `grep '"HOME"' proxy.rs` returns empty -- zero raw HOME mutations remain. |
| 7 | IsolatedHome exposes rigor_dir_str() suitable for RIGOR_HOME env var | VERIFIED | home.rs:40-42 exposes `pub fn rigor_dir_str() -> String`. proxy.rs calls `home.rigor_dir_str()` to get the .rigor/ subdirectory path, matching rigor_home() resolution semantics. |
| 8 | CI grep guard fails the build if new raw HOME usage appears | VERIFIED | .github/workflows/ci.yml:81-96 contains "Guard against raw HOME for .rigor paths" step in clippy job. Greps for `dirs::home_dir`, `env::var("HOME")`, `env::var_os("HOME")`, excludes `paths.rs` and `rigor-home-ok` lines, exits 1 on violation. |
| 9 | All existing tests pass after changes | VERIFIED | `cargo test -p rigor --lib` = 310 passed, 0 failed. `cargo test -p rigor-harness --all-targets` = 19 passed, 0 failed. Zero raw HOME grep violations locally. |

**Score:** 9/9 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/rigor/src/paths.rs` | rigor_home() with RIGOR_HOME env var override | VERIFIED | 93 lines. `pub fn rigor_home() -> PathBuf` with RIGOR_HOME check + dirs fallback. 4 unit tests. |
| `crates/rigor/src/lib.rs` | pub mod paths declaration | VERIFIED | Line 24: `pub mod paths;` |
| `crates/rigor-harness/src/proxy.rs` | TestProxy using RIGOR_HOME instead of HOME | VERIFIED | All env var references are RIGOR_HOME. Uses `rigor_dir_str()` for the value. |
| `.github/workflows/ci.yml` | CI grep guard step | VERIFIED | Lines 81-96: "Guard against raw HOME for .rigor paths" in clippy job. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| daemon/mod.rs | crate::paths::rigor_home | function call | WIRED | Line 25: `crate::paths::rigor_home().join("daemon.pid")` |
| daemon/tls.rs | crate::paths::rigor_home | function call | WIRED | Lines 20, 24: `.join("ca.pem")`, `.join("ca-key.pem")` |
| logging/violation_log.rs | crate::paths::rigor_home | function call | WIRED | Line 23: `crate::paths::rigor_home()` |
| proxy.rs | rigor_home (rigor crate) | RIGOR_HOME env var | WIRED | Sets RIGOR_HOME env var which rigor_home() reads at runtime |
| ci.yml | paths.rs | grep exclusion | WIRED | `grep -v 'src/paths.rs'` excludes the canonical implementation |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| paths.rs | RIGOR_HOME env var | std::env::var("RIGOR_HOME") | Yes -- reads real env var | FLOWING |
| proxy.rs | rigor_home_str | home.rigor_dir_str() | Yes -- from TempDir-backed IsolatedHome | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| rigor_home() unit tests pass | `cargo test -p rigor --lib paths -- --test-threads=1` | 4 passed, 0 failed | PASS |
| Zero raw HOME grep violations | `grep -rn ... \| grep -v paths.rs \| grep -v rigor-home-ok` | (empty output) | PASS |
| All 310 lib tests pass | `cargo test -p rigor --lib -- --test-threads=1` | 310 passed, 0 failed | PASS |
| All 19 harness tests pass | `cargo test -p rigor-harness --all-targets -- --test-threads=1` | 19 passed, 0 failed | PASS |
| Commits exist | `git log --oneline` for 5 commit hashes | All 5 found | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| REQ-018 | 08-01, 08-02 | No test writes to real $HOME/.rigor/. TempDir fixtures + CI check. | SATISFIED | rigor_home() indirection with RIGOR_HOME override (paths.rs), TestProxy sets RIGOR_HOME to TempDir .rigor/ path (proxy.rs), CI grep guard in ci.yml prevents regression. Zero raw HOME usage outside paths.rs and allowlisted lines. |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none found) | - | - | - | - |

No TODOs, FIXMEs, placeholders, empty implementations, or stub patterns found in paths.rs or proxy.rs.

### Human Verification Required

(none)

### Confirmation Bias Counter Findings

1. **Partially met?** REQ-018 specifies "Verified by a CI check that greps test output for the real `$HOME` path." The actual CI guard greps source code for raw HOME patterns rather than test output. This is a stronger implementation that catches the problem at the source rather than at runtime. Intent fully met.
2. **Test that doesn't test what it claims?** All 4 paths.rs tests genuinely exercise env var override/fallback behavior with real assertions. No false passes found.
3. **Uncovered error path?** The `rigor_home()` panic when both RIGOR_HOME and HOME are unset has no test. This is intentional per threat model (T-08-02: "accept" -- unrecoverable config error). INFO-level only.

### Gaps Summary

No gaps found. All 9 must-haves verified with codebase evidence. The phase goal -- tests must not touch the real `$HOME/.rigor/` -- is achieved through a three-layer defense:

1. **Production code** (paths.rs): All .rigor/ path resolution goes through `rigor_home()` which checks `RIGOR_HOME` env var first
2. **Test harness** (proxy.rs): TestProxy sets `RIGOR_HOME` to a TempDir-backed `.rigor/` directory, not the real HOME
3. **CI regression guard** (ci.yml): Grep step in clippy job catches any new raw HOME usage for .rigor/ paths

---

_Verified: 2026-04-24T06:15:00Z_
_Verifier: Claude (gsd-verifier)_
