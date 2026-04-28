---
phase: 12-mock-llm-server-harness-b1-b2-b3-integration-tests
plan: 01
subsystem: testing
tags: [axum, sse, mock-server, request-tracking, response-sequence]

# Dependency graph
requires:
  - phase: 07-harness-primitives
    provides: MockLlmServer, MockLlmServerBuilder, SSE helpers
provides:
  - ReceivedRequest struct for inspecting forwarded request bodies
  - response_sequence() builder for per-call-index SSE response selection
  - received_requests() accessor on MockLlmServer
affects: [12-02-PLAN, b1-kill-switch, b2-auto-retry, b3-pii-redact]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Arc<Mutex<Vec<ReceivedRequest>>> for thread-safe request capture in axum handlers"
    - "AtomicUsize call counter for response sequence selection"

key-files:
  created: []
  modified:
    - crates/rigor-harness/src/mock_llm.rs
    - crates/rigor-harness/src/lib.rs

key-decisions:
  - "Body-only request tracking (no headers) -- sufficient for B3 PII inspection"
  - "response_sequence wraps single-chunks fallback into vec![chunks] for unified handler code path"
  - "JSON parse failure stores Value::Null rather than panicking"

patterns-established:
  - "response_sequence + AtomicUsize counter for multi-response mock servers"
  - "parse_sse_events + extract_text_from_sse for asserting SSE response text content"

requirements-completed: [REQ-022]

# Metrics
duration: 5min
completed: 2026-04-24
---

# Phase 12 Plan 01: MockLlmServer Enhancement Summary

**Request tracking via received_requests() and per-call-index response selection via response_sequence() builder on MockLlmServer**

## Performance

- **Duration:** 4m 48s
- **Started:** 2026-04-24T06:05:42Z
- **Completed:** 2026-04-24T06:10:30Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- MockLlmServer now captures every request body via Arc<Mutex<Vec<ReceivedRequest>>> and exposes received_requests() accessor
- New response_sequence() builder method enables per-call-index SSE response selection (call 0 = violation, call 1 = clean for B2)
- All 22 rigor-harness tests + 6 harness_smoke + 8 proxy_hotpath pass (36 total)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add ReceivedRequest struct and request tracking to MockLlmServer** - `0ded4fe` (feat)
2. **Task 2: Re-export ReceivedRequest from rigor-harness crate root** - `58c7685` (chore)

## Files Created/Modified
- `crates/rigor-harness/src/mock_llm.rs` - ReceivedRequest struct, received field, received_requests() accessor, response_sequence() builder, AtomicUsize call counter, 3 new tests
- `crates/rigor-harness/src/lib.rs` - Re-export ReceivedRequest from crate root

## Decisions Made
- Body-only request tracking (no headers) -- headers can be added later if needed, body is sufficient for B3 PII verification
- response_sequence wraps single-chunks fallback into vec![chunks] so the handler uses a single code path regardless of whether response_sequence() or anthropic_chunks()/openai_chunks()/raw_chunks() was called
- JSON parse failure stores Value::Null rather than panicking, ensuring the mock server remains robust

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed SSE text assertion in response_sequence tests**
- **Found during:** Task 1 (test_mock_llm_response_sequence)
- **Issue:** Tests used `contains("response alpha")` on raw SSE output, but Anthropic SSE format splits text into word-level chunks across separate JSON delta events, so the exact phrase never appears contiguously
- **Fix:** Used `parse_sse_events()` + `extract_text_from_sse()` to reassemble full text before assertion
- **Files modified:** crates/rigor-harness/src/mock_llm.rs
- **Verification:** All 3 new tests pass
- **Committed in:** 0ded4fe (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Test assertion approach corrected to match SSE format. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- MockLlmServer is ready for B1/B2/B3 integration tests (Plan 12-02)
- received_requests() enables B3 PII redact-before-forward verification
- response_sequence() enables B2 auto-retry two-response pattern
- ReceivedRequest exported from crate root for integration test imports

---
*Phase: 12-mock-llm-server-harness-b1-b2-b3-integration-tests*
*Completed: 2026-04-24*
