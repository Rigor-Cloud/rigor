# Phase 12: Mock-LLM server + B1/B2/B3 integration tests - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Build B1/B2/B3 integration tests using MockLlmServer from rigor-harness (already built in Phase 7). May need to enhance MockLlmServer with request tracking for PII inspection.

- B1: Streaming kill-switch — BLOCK mid-stream drops upstream, client gets error SSE
- B2: Auto-retry exactly-once — BLOCK triggers retry with violation marker, max 1 retry
- B3: PII redact-before-forward — upstream never receives raw PII

Requirements: REQ-022, REQ-023, REQ-024

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All implementation at Claude's discretion. Key constraints:
- MockLlmServer already exists in rigor-harness (Phase 7) — enhance, don't rebuild
- TestProxy with CONNECT support already exists (Phase 11)
- Tests use TestProxy::start_with_mock() to route proxy to MockLlmServer
- May need request tracking in MockLlmServer (Arc<Mutex<Vec<ReceivedRequest>>>)
- Over-editing guard: rigor-harness changes OK (test crate), no crates/rigor/src/ changes

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- MockLlmServer with Anthropic/OpenAI SSE formats
- TestProxy with CONNECT + upgrade support
- IsolatedHome + RIGOR_HOME isolation
- proxy_hotpath.rs patterns from Phase 9
- connect_tunnel.rs patterns from Phase 11
- JudgeClient trait seam from Phase 9

</code_context>

<specifics>
## Specific Ideas

Per issue #24: MockLlmServer should track received requests for PII inspection. B1/B2/B3 are separate test files.

</specifics>

<deferred>
## Deferred Ideas

- F6 full-proxy corpus replay — Phase 13
- Performance benchmarks — out of scope

</deferred>
