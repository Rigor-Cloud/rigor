# Phase 17: CI hardening - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Supply-chain + quality gates on every PR: cargo-audit, cargo-deny, llvm-cov floor, bench-regression gate, release artifact signing.

Requirements: REQ-029, REQ-030, REQ-031, REQ-032

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All at Claude's discretion. Per issue #13:
1. cargo-audit job (deny warnings)
2. cargo-deny job + deny.toml (licenses, bans, advisories, sources)
3. Coverage floor via cargo-llvm-cov (start at 50%)
4. Bench regression gate (±15% tolerance vs baseline)
5. Release artifact signing (cosign keyless OIDC preferred)

Over-editing guard: Only add CI workflow jobs and config files. Don't modify existing workflow steps.

</decisions>

<code_context>
## Existing Code Insights

- .github/workflows/ci.yml — test, clippy, rustfmt, rigor-validate jobs
- .github/workflows/release.yml — macOS binary builds + SHA256SUMS (no signing)
- crates/rigor/benches/ — 4 criterion benchmarks exist
- No deny.toml, no coverage config, no signing config

</code_context>

<specifics>
## Specific Ideas

None beyond issue #13.

</specifics>

<deferred>
## Deferred Ideas

- Reproducible builds (bonus) — defer

</deferred>
