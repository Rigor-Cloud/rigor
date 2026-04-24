---
phase: 10-unit-coverage-gaps
plan: 02
subsystem: testing
tags: [rust, unit-tests, evaluator, dfquad, severity-thresholds, claim-pipeline, fail-open]

# Dependency graph
requires:
  - phase: 10-unit-coverage-gaps
    plan: 01
    provides: Unified RIGOR_HOME_TEST_LOCK pattern, 349 tests baseline
provides:
  - 3 evaluator fail-open tests in evaluator/pipeline.rs
  - 4 DF-QuAD boundary tests in constraint/graph.rs
  - 2 SeverityThresholds boundary tests in violation/types.rs
  - 3 claim pipeline ordering tests in claim/heuristic.rs
affects: [10-03, evaluator-safety, dfquad-correctness]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "FailingEvaluator test-only struct for verifying fail-open contract"
    - "BTreeMap determinism test with different insertion orders"
    - "Pipeline ordering test using code-block + hedge interaction"

key-files:
  created: []
  modified:
    - crates/rigor/src/evaluator/pipeline.rs
    - crates/rigor/src/constraint/graph.rs
    - crates/rigor/src/violation/types.rs
    - crates/rigor/src/claim/heuristic.rs

key-decisions:
  - "FailingEvaluator defined inside test module to verify fail-open contract without modifying production code"
  - "BTreeMap determinism verified via HashMap equality (insertion-order-independent comparison)"
  - "Action intent test renamed to reflect actual filtering mechanism (is_assertion conversational prefix, not is_action_intent)"

patterns-established:
  - "Test-only ClaimEvaluator impls defined inside #[cfg(test)] mod tests blocks"
  - "DF-QuAD constants regression guard pattern (assert MAX_ITERATIONS == 100, EPSILON == 0.001)"

requirements-completed: [REQ-020]

# Metrics
duration: 4min
completed: 2026-04-24
---

# Phase 10 Plan 02: Evaluator/Constraint/Violation/Claim Unit Coverage Summary

**12 unit tests covering evaluator fail-open safety invariant, DF-QuAD determinism and boundary cases, SeverityThresholds custom values, and claim pipeline ordering interactions**

## Performance

- **Duration:** 4 min
- **Started:** 2026-04-24T02:08:42Z
- **Completed:** 2026-04-24T02:12:44Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- 3 evaluator fail-open tests proving errors never produce BLOCK verdicts (FailingEvaluator, regex error path, all-miss path)
- 4 DF-QuAD boundary tests proving BTreeMap insertion-order determinism, single strong attacker dominance, constants regression guard, and zero-attacker base retention
- 2 SeverityThresholds tests proving custom (non-default) thresholds and midpoint behavior
- 3 claim pipeline ordering tests proving strip_code_blocks runs before is_hedged, assertion+hedge filtering, and action-intent conversational prefix filtering

## Task Commits

Each task was committed atomically:

1. **Task 1: Evaluator fail-open + DF-QuAD boundary tests** - `25c3bf0` (test)
2. **Task 2: SeverityThresholds + claim pipeline ordering tests** - `7d6f8e2` (test)

## Files Created/Modified

- `crates/rigor/src/evaluator/pipeline.rs` - Added 3 fail-open tests + FailingEvaluator test struct (13 total tests)
- `crates/rigor/src/constraint/graph.rs` - Added 4 DF-QuAD boundary tests (17 total tests)
- `crates/rigor/src/violation/types.rs` - Added 2 custom threshold + midpoint tests (8 total tests)
- `crates/rigor/src/claim/heuristic.rs` - Added 3 pipeline ordering tests (21 total tests)

## Decisions Made

- FailingEvaluator is a test-only struct defined inside the `#[cfg(test)] mod tests` block, not production code. It implements the fail-open contract documented in the ClaimEvaluator trait docstring.
- BTreeMap determinism test compares `HashMap<String, f64>` outputs from `get_all_strengths()` (which returns HashMap, not Vec), proving that insertion order does not affect computed strengths.
- Renamed the action intent pipeline test from `test_pipeline_action_intent_priority` to `test_pipeline_action_intent_filtered_by_assertion` because the actual filtering mechanism is `is_assertion` (conversational prefix "let me"), not `is_action_intent`.

## Deviations from Plan

None - plan executed exactly as written. One test was renamed to accurately reflect the filtering mechanism (is_assertion conversational prefix, not is_action_intent), but the test logic and assertions are identical to what the plan specified.

## Issues Encountered

- Pre-existing flaky test `daemon::proxy::tests::score_claim_relevance_concurrent_single_flight` occasionally fails when run in parallel (documented in 10-01-SUMMARY.md). Passes in isolation. Not caused by this plan's changes.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Evaluator fail-open (gap 5), DF-QuAD boundaries (gap 6), SeverityThresholds (gap 7), and claim pipeline ordering (gap 8) coverage gaps closed
- 361 total tests (349 baseline + 12 new) all pass
- Ready for 10-03 (content store TTL/concurrency + action gate tests)

## Self-Check: PASSED

All 4 modified files exist. Both task commits (25c3bf0, 7d6f8e2) verified in git log. SUMMARY.md created.

---
*Phase: 10-unit-coverage-gaps*
*Completed: 2026-04-24*
