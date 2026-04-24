# Phase 2: PR-4 — corpus exporter (`rigor refine export`) - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

`rigor refine export` emits training-ready JSONL from the violation log. Unblocks Phase 3E GEPA prompt optimization and Phase 4E Modal discriminator training.

Requirements: REQ-006, REQ-007
Canonical spec: `.planning/roadmap/epistemic-expansion-plan.md` section 0J

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All at Claude's discretion. Per issue #19:
- New modules: refine/mod.rs, refine/corpus.rs (or extend existing cli/refine.rs)
- CLI: `rigor refine export --constraint <id> --since <date> --format jsonl --output <path>`
- Read through ViolationLogBackend trait
- Start with JSONL only (Parquet later)
- Over-editing guard: don't refactor existing violation log code

</decisions>

<code_context>
## Existing Code Insights

- cli/refine.rs already exists — check what's there
- ViolationLogBackend trait exists for reading violations
- violations.jsonl at ~/.rigor/violations.jsonl
- rigor_home() for path resolution (Phase 8)

</code_context>

<specifics>
## Specific Ideas

None beyond issue #19.

</specifics>

<deferred>
## Deferred Ideas

- Parquet format — defer
- rigor refine optimize (GEPA) — Phase 3E

</deferred>
