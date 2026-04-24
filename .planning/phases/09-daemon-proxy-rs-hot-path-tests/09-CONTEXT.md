# Phase 9: daemon/proxy.rs hot-path tests - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Cover `proxy_request`, `extract_and_evaluate`, `scope_judge_check`, `score_claim_relevance` — currently zero test coverage. These are the security-critical functions that enforce rigor's BLOCK, auto-retry, PII redaction, and relevance scoring behaviors.

Requirements: REQ-019

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All implementation choices are at Claude's discretion. Key guidance from GitHub issue #7:

- Inject a `ChatClient` trait seam so `scope_judge_check` / `check_violations_persist` can be driven by a fake in tests
- Unit tests for `proxy_request` decision branches against canned SSE streams (real TCP + rustls OK via rigor-harness)
- Property test that `score_claim_relevance` with N concurrent callers produces exactly one scored verdict (rest are no-ops)
- Tests for `extract_and_evaluate` and `evaluate_text_inline` claim→violation pipeline

CRITICAL — Over-editing guard:
- The `ChatClient` trait seam is the ONLY production code modification
- Do NOT refactor proxy.rs beyond adding the trait and injecting it
- Do NOT restructure existing functions, rename variables, or change error handling
- Tests go in crates/rigor/tests/ using rigor-harness primitives

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `crates/rigor-harness/` — IsolatedHome, TestCA, MockLlmServer, TestProxy, SSE helpers (Phase 7)
- `crates/rigor/src/paths.rs` — rigor_home() with RIGOR_HOME env var (Phase 8)
- Existing proxy.rs unit tests (lines 3834-4107): apply_provider_auth, replace_last_user_content, detect_pii, extract_sse_*

### Established Patterns
- proxy.rs is 4134 lines of MITM/streaming/evaluation code
- Functions call external LLMs via reqwest for scope_judge_check, check_violations_persist
- RELEVANCE_CACHE is a global Mutex<HashMap> — may leak across in-process tests
- RELEVANCE_SEMAPHORE controls single-flight for score_claim_relevance

### Integration Points
- proxy_request is called from the axum router handler
- extract_and_evaluate is called from the SSE streaming loop
- scope_judge_check calls out to LLM API — needs trait seam for testing
- TestProxy from rigor-harness wraps production build_router() on ephemeral port

</code_context>

<specifics>
## Specific Ideas

Per issue #7: The ChatClient trait seam must be minimal — extract the HTTP call pattern, inject via DaemonState or function parameter. Do not refactor the entire proxy module.

</specifics>

<deferred>
## Deferred Ideas

None — discuss phase skipped.

</deferred>
