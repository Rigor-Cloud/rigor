# Phase 19: rigor-validate CI expansion - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Expand rigor-validate CI job beyond YAML parse to actual evaluation: eval against constraints, corpus replay, violation-log consistency check.

Requirements: REQ-034

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All at Claude's discretion. Per issue #12:
- Expand CI job to run corpus replay test
- Add violation-log consistency check
- Over-editing guard: only modify .github/workflows/ci.yml rigor-validate job

</decisions>

<code_context>
## Existing Code Insights

- rigor-validate CI job exists (ci.yml) — runs `rigor validate --path rigor.yaml`
- corpus_proxy_replay.rs exists (Phase 13)
- corpus_replay.rs exists (claim-extractor replay)

</code_context>

<specifics>
## Specific Ideas

None beyond issue #12.

</specifics>

<deferred>
## Deferred Ideas

None.

</deferred>
