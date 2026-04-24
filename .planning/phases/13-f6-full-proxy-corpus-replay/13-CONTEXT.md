# Phase 13: F6 full-proxy corpus replay via mock-LLM - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Exercise full MITM -> streaming -> decision path against recorded corpus bytes. Replay recorded responses through MockLlmServer + TestProxy to exercise the complete proxy pipeline (PII scan, claim extraction, evaluation, kill-switch).

Requirements: REQ-025

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All at Claude's discretion. Per issue #25:
- Extend MockLlmServer with replay_recording() capability
- Create corpus_proxy_replay.rs integration test
- Assert observed decisions match manifest windows
- Over-editing guard: extend MockLlmServer, don't rebuild; create one new test file

</decisions>

<code_context>
## Existing Code Insights

- MockLlmServer already has response_sequence + request tracking (Phase 12)
- TestProxy with CONNECT + upgrade support (Phase 11)
- 800 corpus recordings in .planning/corpus/recordings/ (Phase 5)
- corpus_replay.rs exists (claim-extractor-only, not proxy-path)

</code_context>

<specifics>
## Specific Ideas

None beyond issue #25.

</specifics>

<deferred>
## Deferred Ideas

None.

</deferred>
