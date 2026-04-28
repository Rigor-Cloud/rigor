# Phase 10: Unit coverage gaps - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Close listed unit-level gaps to lift coverage floor. Pure test additions — no production code changes except where a test seam is structurally necessary.

Requirements: REQ-020

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All implementation at Claude's discretion. Key gaps from GitHub issue #16:

1. `should_mitm_target` — MITM allowlist tests
2. `daemon_alive` / `write_pid_file` / `remove_pid_file` — daemon lifecycle
3. `RigorCA::load_or_generate` / `server_config_for_host` / `install_ca_trust` — TLS CA
4. `peek_client_hello` — SNI edge cases
5. Evaluator fail-open on error
6. `compute_strengths` DF-QuAD boundaries (MAX_ITERATIONS, BTreeMap guard)
7. `SeverityThresholds` boundary arithmetic (0.7/0.4 boundaries)
8. `claim/heuristic.rs` pipeline ordering test
9. `memory::content_store` TTL + concurrency
10. `daemon/gate_api.rs` action gate tests

Over-editing guard: ONLY add tests. No production code changes unless absolutely needed for testability. No refactoring.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- rigor-harness IsolatedHome (for daemon lifecycle tests)
- rigor_home() with RIGOR_HOME env var (Phase 8)
- Existing `#[cfg(test)] mod tests` in most target modules

### Integration Points
- Tests should be co-located `#[cfg(test)]` unit tests in each module
- No new integration test files needed — these are unit-level gaps

</code_context>

<specifics>
## Specific Ideas

Per issue #16: Each gap has a specific risk description. Tests should directly address the named risk.

</specifics>

<deferred>
## Deferred Ideas

None.

</deferred>
