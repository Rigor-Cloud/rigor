# Phase 6: Pretty-printed stats table for `rigor corpus stats` - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Replace JSON-only output with a TTY-friendly aligned table. Add --format json|csv flags.

Requirements: REQ-014

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All at Claude's discretion. Per issue #23:
- Pretty-printed table with aligned columns for per-prompt stats
- --format json flag (structured)
- --format csv flag (spreadsheet)
- Over-editing guard: only modify cli/corpus.rs stats handler

</decisions>

<code_context>
## Existing Code Insights

- cli/corpus.rs just created in Phase 4 — has stats handler that outputs JSON
- corpus::stats module has compute_stats, aggregate_by_model

</code_context>

<specifics>
## Specific Ideas

None beyond issue #23.

</specifics>

<deferred>
## Deferred Ideas

- Precision/recall/F1 — requires GEPA ground-truth labels

</deferred>
