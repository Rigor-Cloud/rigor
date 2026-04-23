---
phase: 07-crates-rigor-tests-integration-test-infrastructure
plan: 01
subsystem: testing
tags: [rust, rcgen, rustls, axum, sse, tempdir, integration-test]

requires:
  - phase: none
    provides: "Standalone primitives -- no prior phase dependencies"
provides:
  - "IsolatedHome: TempDir-based HOME override for subprocess test isolation"
  - "TestCA: ephemeral in-memory CA with per-host ServerConfig and reqwest client"
  - "MockLlmServer: axum SSE server with Anthropic and OpenAI format builders"
  - "SSE helpers: parse, extract, and generate SSE event sequences"
affects: [07-02, 09-proxy-integration, 10-e2e-tests, 11-coverage]

tech-stack:
  added: []
  patterns: [rcgen-ephemeral-ca, oneshot-shutdown-on-drop, builder-pattern-mock-server, sse-roundtrip-testing]

key-files:
  created:
    - crates/rigor-harness/src/home.rs
    - crates/rigor-harness/src/ca.rs
    - crates/rigor-harness/src/mock_llm.rs
    - crates/rigor-harness/src/sse.rs
  modified:
    - crates/rigor-harness/Cargo.toml
    - crates/rigor-harness/src/lib.rs

key-decisions:
  - "No global env mutation: IsolatedHome passes paths via Command::env() only, never std::env::set_var"
  - "TestCA is purely in-memory: follows production rcgen pattern but never persists to disk"
  - "SSE chunk generation and extraction live in sse.rs, shared by MockLlmServer and test assertions"

patterns-established:
  - "Oneshot shutdown pattern: MockLlmServer uses oneshot::Sender in Drop for graceful axum shutdown"
  - "Builder pattern for mock servers: MockLlmServerBuilder configures format and route before build()"
  - "SSE roundtrip testing: generate chunks -> extract text -> assert equality"

requirements-completed: [REQ-015, REQ-016, REQ-017]

duration: 6min
completed: 2026-04-24
---

# Phase 7 Plan 1: Core Harness Primitives Summary

**IsolatedHome, TestCA, MockLlmServer, and SSE helpers providing test isolation, ephemeral TLS, and deterministic LLM responses for integration testing**

## Performance

- **Duration:** 6 min
- **Started:** 2026-04-23T22:37:13Z
- **Completed:** 2026-04-23T22:43:00Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Built IsolatedHome with TempDir-based HOME isolation (no global env mutation, safe for parallel tests)
- Built TestCA following production rcgen pattern from daemon/tls.rs with reqwest client integration
- Built MockLlmServer with builder pattern supporting Anthropic and OpenAI SSE streaming formats
- Built SSE helpers with roundtrip capability: generate chunks -> parse events -> extract text
- 17 unit tests passing, covering all four primitives

## Task Commits

Each task was committed atomically:

1. **Task 1: Wire Cargo.toml dependencies and module structure** - `d7118d3` (chore)
2. **Task 2: Implement IsolatedHome, TestCA, MockLlmServer, and SSE helpers** - `8f657f8` (feat)

## Files Created/Modified
- `crates/rigor-harness/Cargo.toml` - Added 12 dependencies (tokio, axum, rcgen, rustls, reqwest, etc.)
- `crates/rigor-harness/src/lib.rs` - Module declarations and re-exports for all four primitives
- `crates/rigor-harness/src/home.rs` - IsolatedHome with TempDir, .rigor/ dir, rigor.yaml writer
- `crates/rigor-harness/src/ca.rs` - TestCA with ephemeral rcgen CA, per-host ServerConfig, reqwest client
- `crates/rigor-harness/src/mock_llm.rs` - MockLlmServer with builder, Anthropic/OpenAI SSE, graceful shutdown
- `crates/rigor-harness/src/sse.rs` - parse_sse_events, extract_text_from_sse, anthropic/openai chunk generators

## Decisions Made
- No global env mutation: IsolatedHome uses Command::env() pattern only, never std::env::set_var
- TestCA is purely in-memory, following the exact rcgen pattern from daemon/tls.rs but skipping persistence
- SSE chunk generation functions live in sse.rs (not mock_llm.rs) so they can be shared by both MockLlmServer and direct test assertions
- MockLlmServer binds to 127.0.0.1:0 only (loopback + ephemeral port per threat model T-07-03)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed split_inclusive type mismatch in sse.rs**
- **Found during:** Task 2 (SSE helpers implementation)
- **Issue:** `text.split_inclusive(' ')` returns a `SplitInclusive` iterator, not a `Vec<&str>`
- **Fix:** Added `.collect()` call to convert iterator to Vec before conditional check
- **Files modified:** crates/rigor-harness/src/sse.rs
- **Verification:** cargo check passes, all SSE roundtrip tests pass
- **Committed in:** 8f657f8 (part of Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Trivial compile fix. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All four primitives exported from rigor-harness crate and ready for Plan 02 (TestProxy, subprocess helpers)
- TestCA and MockLlmServer will be composed by TestProxy in Plan 02
- IsolatedHome provides the HOME isolation that TestProxy needs for DaemonState::load()

## Self-Check: PASSED

All 7 files verified present. Both commit hashes (d7118d3, 8f657f8) verified in git log.

---
*Phase: 07-crates-rigor-tests-integration-test-infrastructure*
*Completed: 2026-04-24*
