# Phase 4: `rigor corpus` CLI subcommand wiring - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Wire `rigor corpus record / stats / validate` dispatchers over already-merged library functions. Pure CLI surface; logic exists in lib.

Requirements: REQ-010, REQ-011, REQ-012

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All at Claude's discretion. Per issue #21:
- Add Commands::Corpus with CorpusCommands enum (Record, Stats, Validate)
- Create cli/corpus.rs with clap-derived subcommands
- Dispatch to existing library API (corpus::record_prompt, corpus::compute_stats, etc.)
- Stats can start with JSON output (pretty-print is Phase 6)
- Over-editing guard: only add cli/corpus.rs and wire into mod.rs

</decisions>

<code_context>
## Existing Code Insights

- corpus/ module exists with client.rs, record.rs, stats.rs, mod.rs
- Library functions already tested
- cli/mod.rs has Commands enum with existing variants
- Pattern from cli/refine.rs (just extended in Phase 2) is the model

</code_context>

<specifics>
## Specific Ideas

None beyond issue #21.

</specifics>

<deferred>
## Deferred Ideas

- Pretty-print stats table — Phase 6
- Seed corpus recording — Phase 5

</deferred>
