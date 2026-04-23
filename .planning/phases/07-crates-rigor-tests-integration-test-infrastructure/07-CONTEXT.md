# Phase 7: crates/rigor/tests/ integration test infrastructure - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (infrastructure phase — discuss skipped)

<domain>
## Phase Boundary

Stand up real-TCP, rustls, SSE, `$HOME` isolation harness as a shared library in `crates/rigor/tests/`. Precondition for Phases 9–12 (proxy hot-path, unit gaps, E2E gaps, mock-LLM integration tests).

Requirements: REQ-015 (shared test-support library), REQ-016 (isolated test execution), REQ-017 (reuse production types, stub network only).

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All implementation choices are at Claude's discretion — pure infrastructure phase. Use ROADMAP phase goal, success criteria, and codebase conventions to guide decisions.

Key codebase context informing decisions:
- `crates/rigor-harness/` exists as empty placeholder — intended for MockAgent, MockLLM, TestDaemon, etc.
- Existing `tests/support/mod.rs` provides Fixture, run_rigor_with_fixture, walk_fixtures — subprocess-based helpers
- `RigorCA::load_or_generate()` and `daemon_pid_file()` both use `$HOME/.rigor/`
- Production code already depends on tokio, rustls, tokio-rustls, rcgen, axum, hyper, reqwest
- Only `invariants.rs:B10` manually isolates `$HOME`; all other tests risk touching real `~/.rigor/`
- `run_rigor_*()` and `parse_response()` duplicated across 5+ test files

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `tests/support/mod.rs` — Fixture struct, walk_fixtures(), extract_decision(), require_openrouter! macro
- `src/daemon/tls.rs` — RigorCA::load_or_generate(), server_config_for_host(), generate_tls_config()
- `src/daemon/sni.rs` — peek_client_hello(), PrependedStream
- `src/daemon/egress/chain.rs` — SseChunk, EgressFilter trait, FilterChain
- `src/daemon/mod.rs` — start_daemon(), daemon_pid_file(), MITM_HOSTS
- `src/daemon/proxy.rs` — 4217-line proxy with SSE streaming, CONNECT tunnel, TLS MITM

### Established Patterns
- Subprocess tests: spawn `CARGO_BIN_EXE_rigor`, pipe JSON, capture stdout
- In-process tests: import rigor types directly, call library functions
- TempDir for filesystem isolation (tempfile crate)
- `RIGOR_TEST_CLAIMS` env var to short-circuit claim extraction
- `#[cfg(test)] mod tests` co-located unit tests in src/

### Integration Points
- `crates/rigor-harness/` — empty workspace member, intended for shared test primitives
- `crates/rigor/Cargo.toml` dev-dependencies — currently only tempfile + criterion
- `crates/rigor/tests/` — 12 existing test files all potentially consuming new harness

</code_context>

<specifics>
## Specific Ideas

No specific requirements — infrastructure phase. Refer to ROADMAP phase description and success criteria.

</specifics>

<deferred>
## Deferred Ideas

None — discuss phase skipped.

</deferred>
