---
phase: 14-rigor-test-e2e-harness-flesh-out
plan: 01
subsystem: testing
tags: [rigor-test, rigor-harness, e2e, benchmark, criterion, html-report, jsonl, tokio, clap]

# Dependency graph
requires:
  - phase: 07-crates-rigor-tests-integration-test-infrastructure
    provides: rigor-harness primitives (MockLlmServer, TestProxy, IsolatedHome, SSE helpers)
provides:
  - Working e2e subcommand with clean-passthrough and violation-detection scenarios
  - Working bench subcommand shelling out to cargo bench -p rigor
  - Working report subcommand reading JSONL and writing HTML summary
  - 3 smoke tests covering all subcommands
affects: [rigor-test, ci-hardening, test-infra]

# Tech tracking
tech-stack:
  added: [tokio (rigor-test), reqwest (rigor-test), serde/serde_json (rigor-test)]
  patterns: [binary-invocation smoke tests via env!("CARGO_BIN_EXE_*"), shell-out to cargo bench]

key-files:
  created:
    - crates/rigor-test/src/e2e.rs
    - crates/rigor-test/src/bench.rs
    - crates/rigor-test/src/report.rs
    - crates/rigor-test/tests/smoke.rs
  modified:
    - crates/rigor-test/Cargo.toml
    - crates/rigor-test/src/main.rs

key-decisions:
  - "bench smoke test uses --help instead of full criterion run to keep tests fast"
  - "YAML suite loading deferred; --suite prints message and runs built-in scenarios"
  - "report skipped count derived from total minus pass minus fail (forward-compatible)"

patterns-established:
  - "Binary smoke tests: use env!(CARGO_BIN_EXE_*) + std::process::Command for integration testing of CLI binaries"
  - "E2E scenario pattern: MockLlmServerBuilder -> TestProxy::start_with_mock -> reqwest POST -> SSE parse -> assert"

requirements-completed: [REQ-026]

# Metrics
duration: 4min
completed: 2026-04-24
---

# Phase 14 Plan 01: rigor-test Subcommand Implementation Summary

**Replaced all three rigor-test stubs (e2e, bench, report) with real implementations backed by rigor-harness primitives, with 3 passing smoke tests**

## Performance

- **Duration:** 4 min
- **Started:** 2026-04-24T06:55:09Z
- **Completed:** 2026-04-24T06:59:11Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- E2E subcommand runs 2 built-in scenarios (clean-passthrough + violation-detection) using MockLlmServer + TestProxy
- Bench subcommand shells out to `cargo bench -p rigor` with --bench and --quick flag support
- Report subcommand reads JSONL harness event logs and writes styled HTML summary with pass/fail/skip stats
- All 3 smoke tests pass: `cargo test -p rigor-test --test smoke` runs 3 tests in 0.28s

## Task Commits

Each task was committed atomically:

1. **Task 1: Add dependencies and implement e2e, bench, report modules** - `bb0b48c` (feat)
2. **Task 2: Smoke tests for all three subcommands** - `ac5f933` (test)

## Files Created/Modified
- `crates/rigor-test/src/e2e.rs` - E2E scenario runner with MockLlmServer + TestProxy orchestration (143 lines)
- `crates/rigor-test/src/bench.rs` - Benchmark dispatcher shelling out to cargo bench (31 lines)
- `crates/rigor-test/src/report.rs` - JSONL reader + HTML report writer with graceful degradation (91 lines)
- `crates/rigor-test/tests/smoke.rs` - Smoke tests for all 3 subcommands (138 lines)
- `crates/rigor-test/Cargo.toml` - Added tokio, serde, serde_json, reqwest, rigor-harness deps + tempfile dev-dep
- `crates/rigor-test/src/main.rs` - Wired modules, replaced stubs, switched to async main

## Decisions Made
- Bench smoke test uses `--help` instead of full criterion run to keep CI fast (full run tested manually)
- YAML suite loading intentionally deferred; `--suite` flag prints informational message and runs built-in scenarios
- Report `skipped` count derived as `total - passed - failed` rather than matching specific outcome strings (forward-compatible with new outcome types)
- E2E violation scenario sets RIGOR_NO_RETRY via unsafe env::set_var (matches proven b1_kill_switch.rs pattern; scenarios run sequentially)

## Deviations from Plan

None - plan executed exactly as written.

## Known Stubs

- `crates/rigor-test/src/e2e.rs:56` - YAML suite loading prints "not yet available" message. Intentional per plan: built-in scenarios satisfy REQ-026; suite format can be added later.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- rigor-test is fully functional with all 3 subcommands
- E2E infrastructure can be extended with additional scenarios
- Report command ready for CI integration (reads harness-runs.jsonl)

## Self-Check: PASSED

- All 6 created/modified files exist on disk
- Both task commits (bb0b48c, ac5f933) found in git log
- 14-01-SUMMARY.md exists

---
*Phase: 14-rigor-test-e2e-harness-flesh-out*
*Completed: 2026-04-24*
