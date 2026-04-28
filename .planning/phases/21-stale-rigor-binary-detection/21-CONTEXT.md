# Phase 21: Stale rigor binary detection - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Add an ignored test that detects when ~/.cargo/bin/rigor is from a different repo or version than the current build. Protects against the drift incident (e68c9cf).

Requirements: REQ-036

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All at Claude's discretion. Per issue #26:
- Create `tests/installed_binary_drift.rs` with `#[ignore]` (opt-in via `cargo test --ignored`)
- Compare `which rigor` output against fresh `cargo build` output
- Emit clear message: "run cargo install --path crates/rigor --force"
- Over-editing guard: one new test file only

</decisions>

<code_context>
## Existing Code Insights

- rigor binary: crates/rigor/src/main.rs
- rigor validate subcommand exists
- No existing version comparison mechanism

</code_context>

<specifics>
## Specific Ideas

None beyond issue #26.

</specifics>

<deferred>
## Deferred Ideas

None.

</deferred>
