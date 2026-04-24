# Phase 16: macOS CI matrix - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Add macOS to CI matrix. Currently all CI jobs run ubuntu-latest only, but release ships macOS binaries with macOS-specific code paths (keychain trust, getpeername, Bun/connect bypass).

Requirements: REQ-028

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All at Claude's discretion. Key guidance from issue #10:
- CI matrix: [ubuntu-latest, macos-14] minimum
- Run cargo test --all-features on both
- Consider macOS only on main + release branches if cost is a concern
- Over-editing guard: only modify .github/workflows/ci.yml matrix, don't restructure the workflow

</decisions>

<code_context>
## Existing Code Insights

- .github/workflows/ci.yml currently uses `runs-on: ubuntu-latest`
- .github/workflows/release.yml uses `macos-14` for release builds
- macOS-specific code: daemon/tls.rs install_ca_trust, daemon/sni.rs, getpeername hook

</code_context>

<specifics>
## Specific Ideas

None beyond issue #10.

</specifics>

<deferred>
## Deferred Ideas

None.

</deferred>
