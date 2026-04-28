---
phase: 12-mock-llm-server-harness-b1-b2-b3-integration-tests
plan: 02
subsystem: testing
tags: [integration-tests, sse, block, retry, pii-redaction, streaming-proxy]

# Dependency graph
requires:
  - phase: 12-mock-llm-server-harness-b1-b2-b3-integration-tests
    provides: MockLlmServer with received_requests() and response_sequence()
provides:
  - B1 streaming kill-switch integration test (BLOCK + RIGOR_NO_RETRY -> error SSE)
  - B2 auto-retry exactly-once integration test (BLOCK -> retry with [RIGOR EPISTEMIC CORRECTION])
  - B3 PII redact-before-forward integration test (PII -> [REDACTED:*] tags before upstream)
affects: [coverage-reports, ci-hardening]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "ENV_LOCK mutex for RIGOR_NO_RETRY env var serialization across parallel tests"
    - "response_sequence for multi-response mock server (violation call 0, clean call 1)"
    - "received_requests() inspection for verifying proxy-forwarded request content"
    - "Rego keyword-match constraint pattern for deterministic BLOCK triggering in tests"

key-files:
  created:
    - crates/rigor/tests/b1_kill_switch.rs
    - crates/rigor/tests/b2_auto_retry.rs
    - crates/rigor/tests/b3_pii_redact.rs
  modified: []

key-decisions:
  - "B2 retry_at_most_once test uses pre-injected [RIGOR EPISTEMIC CORRECTION] in system prompt rather than response_sequence with two violations, simplifying test setup and directly testing the already_retried guard"
  - "B3 tests use stream:true since MockLlmServer serves SSE -- more realistic than stream:false"
  - "Separate ENV_LOCK per test file rather than cross-file shared mutex -- simpler and test-threads=1 prevents cross-binary races"

patterns-established:
  - "Deterministic BLOCK trigger: Rego constraint with VIOLATION_MARKER keyword + SSE text with matching keyword at sentence boundary"
  - "RIGOR_NO_RETRY save/restore pattern: acquire mutex, save original, set/unset, run test, restore"
  - "PII verification via received_requests(): send PII in user message, inspect mock's received body for [REDACTED:*] tags"

requirements-completed: [REQ-023, REQ-024]

# Metrics
duration: 9min
completed: 2026-04-24
---

# Phase 12 Plan 02: B1/B2/B3 Integration Tests Summary

**Three streaming proxy integration tests proving BLOCK kill-switch (B1), auto-retry with epistemic correction injection (B2), and PII redact-before-forward (B3) via MockLlmServer + TestProxy**

## Performance

- **Duration:** 9m 25s
- **Started:** 2026-04-24T06:23:07Z
- **Completed:** 2026-04-24T06:32:32Z
- **Tasks:** 3
- **Files created:** 3

## Accomplishments
- B1: Proves streaming BLOCK with RIGOR_NO_RETRY=1 drops upstream and injects `event: error` SSE containing "rigor BLOCKED", with HTTP 200 status (stream started before BLOCK)
- B2: Proves exactly-one retry fires with [RIGOR EPISTEMIC CORRECTION] + TRUTH: statements in system prompt, and second BLOCK (already_retried) gives up without retrying again
- B3: Proves PII (SSN 123-45-6789, email secret@example.com) is redacted to [REDACTED:*] tags before upstream send, clean messages forwarded verbatim, redaction transparent to client
- Full regression: all 28 integration tests pass (7 new + 21 existing)

## Task Commits

Each task was committed atomically:

1. **Task 1: B1 streaming kill-switch integration test** - `d9fb07b` (test)
2. **Task 2: B2 auto-retry exactly-once integration test** - `c86dff0` (test)
3. **Task 3: B3 PII redact-before-forward integration test** - `f865350` (test)

## Files Created/Modified
- `crates/rigor/tests/b1_kill_switch.rs` - 2 tests: b1_block_drops_upstream_and_injects_error_sse, b1_block_returns_200_status
- `crates/rigor/tests/b2_auto_retry.rs` - 2 tests: b2_retry_injects_epistemic_correction, b2_retry_at_most_once
- `crates/rigor/tests/b3_pii_redact.rs` - 3 tests: b3_pii_redacted_before_upstream_send, b3_pii_redaction_transparent_to_client, b3_no_redaction_for_clean_message

## Decisions Made
- B2 `b2_retry_at_most_once` test uses pre-injected `[RIGOR EPISTEMIC CORRECTION]` marker in the system prompt of the original request, rather than using response_sequence with two violation responses. This directly tests the `already_retried` guard path (proxy.rs line 2010-2014) and is simpler to reason about -- only 1 mock request needed.
- B3 tests use `stream: true` since MockLlmServer always serves SSE. PII-IN redaction runs on the request path (before forwarding), so streaming mode doesn't affect PII test correctness, but it's the more realistic scenario.
- Each test file has its own `ENV_LOCK` mutex rather than a shared cross-file mutex. Integration tests run with `--test-threads=1` for cross-binary isolation.

## Deviations from Plan

None - plan executed exactly as written. All three test files created, all 7 tests pass, no production code modified.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- B1/B2/B3 integration tests are green and cover REQ-023, REQ-024, REQ-025a
- Phase 12 is complete (2/2 plans done)
- No blockers for subsequent phases

## Self-Check: PASSED

- All 3 test files exist on disk
- All 3 task commits verified in git history (d9fb07b, c86dff0, f865350)
- No production code (crates/rigor/src/) modified
- Full regression: 28/28 integration tests pass

---
*Phase: 12-mock-llm-server-harness-b1-b2-b3-integration-tests*
*Completed: 2026-04-24*
