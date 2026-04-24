# Phase 11: E2E coverage gaps - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Close listed end-to-end gaps. Tests use rigor-harness primitives (TestProxy, MockLlmServer, IsolatedHome, TestCA) to exercise full-stack scenarios through real TCP.

IMPORTANT scope boundary: Phase 12 explicitly covers "B1/B2/B3 integration tests" (streaming kill-switch, auto-retry, PII redact-before-forward) with mock-LLM. Phase 11 should focus on gaps NOT covered by Phase 12:
- CONNECT blind-tunnel for non-LLM hosts
- TLS MITM handshake with generated leaf cert
- Stop-hook integration (run_hook)
- Corpus drift validation (replay violation log fixtures)

B1/B2/B3 (BLOCK kill-switch, auto-retry, PII-before-upstream) are Phase 12's scope.

Requirements: REQ-021

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All implementation at Claude's discretion. Over-editing guard: only add new test files in crates/rigor/tests/. No production code changes.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- rigor-harness: TestProxy, MockLlmServer, IsolatedHome, TestCA, SSE helpers
- rigor_home() with RIGOR_HOME override (Phase 8)
- proxy_hotpath.rs pattern (Phase 9)

</code_context>

<specifics>
## Specific Ideas

No specific requirements beyond issue #17.

</specifics>

<deferred>
## Deferred Ideas

- B1/B2/B3 (BLOCK kill-switch, auto-retry, PII-before-upstream) — deferred to Phase 12
- Corpus replay block-rate — deferred to Phase 13/20
- getpeername transparent-mode — macOS-specific, defer if complex

</deferred>
