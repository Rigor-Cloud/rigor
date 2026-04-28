# Phase 8: `$HOME/.rigor` test isolation - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Tests must not touch the real `$HOME/.rigor` (PID file, CA cert, violations log). Introduce a `rigor_home()` indirection and update all call sites. Every test that touches daemon lifecycle, CA cert, or violations log uses TempDir fixtures from the rigor-harness IsolatedHome (Phase 7).

Requirements: REQ-018

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All implementation choices are at Claude's discretion. Key guidance from GitHub issue #15:

- Introduce `rigor_home()` function with `RIGOR_HOME` env var override (or `RigorPaths` struct)
- Update call sites: `daemon/mod.rs` (daemon_pid_file, daemon_alive), `daemon/tls.rs` (ca_cert_path, ca_key_path), logging.rs, session log writers
- Add CI grep that fails on new `dirs::home_dir()` / raw `$HOME` reads in `crates/rigor/src/`
- Use Phase 7's IsolatedHome from rigor-harness for test fixtures

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `crates/rigor-harness/src/home.rs` — IsolatedHome (TempDir + HOME env isolation, built in Phase 7)
- `crates/rigor/tests/invariants.rs:B10` — only existing test that isolates HOME manually

### Established Patterns
- `dirs::home_dir()` used throughout production code for `~/.rigor/` path resolution
- `daemon_pid_file()`, `ca_cert_path()`, `ca_key_path()` are standalone functions returning PathBuf
- Production code does NOT thread a paths struct — each function resolves HOME independently

### Integration Points
- All 17 production `dirs::home_dir()` call sites need indirection
- 4 `env::var("HOME")` usages need review
- rigor-harness IsolatedHome provides TempDir + HOME env override for tests

</code_context>

<specifics>
## Specific Ideas

Per issue #15: Add a clippy lint or CI grep that fails on new `dirs::home_dir()` / raw `$HOME` reads inside `crates/rigor/src/` outside the `rigor_home()` definition.

</specifics>

<deferred>
## Deferred Ideas

None — discuss phase skipped.

</deferred>
