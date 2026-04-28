---
phase: 02-pr-4-corpus-exporter
plan: 01
subsystem: cli
tags: [jsonl, corpus, export, streaming, violation-log, training-data]

# Dependency graph
requires:
  - phase: 01-constraint-regex-tightening
    provides: violation log infrastructure and ViolationLogEntry types
provides:
  - CorpusRow struct for training-ready JSONL output
  - export_corpus() streaming line-by-line exporter
  - RefineCommands subcommand enum (Suggest + Export)
  - rigor refine export CLI command with --constraint, --since, --out filters
affects: [phase-3E-gepa-optimization, phase-4E-modal-discriminator]

# Tech tracking
tech-stack:
  added: []
  patterns: [streaming-bufreader-export, subcommand-enum-conversion, multi-claim-fan-out]

key-files:
  created: []
  modified:
    - crates/rigor/src/cli/refine.rs
    - crates/rigor/src/cli/mod.rs

key-decisions:
  - "Extended cli/refine.rs in-place rather than creating refine/ module directory (Option A from research)"
  - "CLI grammar change: rigor refine --apply becomes rigor refine suggest --apply (acceptable pre-1.0)"
  - "Copied parse_since logic into refine.rs rather than extracting shared util (13 lines, dedup in Phase 3E)"
  - "export_corpus count includes per-claim rows (multi-claim entries fan out)"

patterns-established:
  - "Subcommand conversion: flat-flag command to enum with Subcommand derive"
  - "Streaming export: BufReader line iteration with skip-malformed, no Vec collection"
  - "Multi-claim fan-out: one CorpusRow per claim_text element"

requirements-completed: [REQ-006, REQ-007]

# Metrics
duration: 6min
completed: 2026-04-24
---

# Phase 2 Plan 01: Corpus Exporter Summary

**Streaming JSONL corpus exporter (`rigor refine export`) with CorpusRow struct, --constraint/--since/--out filters, and 9 unit tests**

## Performance

- **Duration:** 6 min
- **Started:** 2026-04-24T07:48:40Z
- **Completed:** 2026-04-24T07:55:00Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- CorpusRow struct with 11 fields mapping ViolationLogEntry to training-ready shape (REQ-006)
- Streaming export_corpus() reads line-by-line via BufReader, never collects full Vec (REQ-007)
- Multi-claim ViolationLogEntry entries fan out to one CorpusRow per claim
- --constraint and --since filters with malformed-line skip (T-02-01 threat mitigation)
- RefineCommands subcommand enum wired into CLI dispatch
- 380 library tests pass (9 new + 371 existing), 0 warnings

## Task Commits

Each task was committed atomically:

1. **Task 1: CorpusRow + export_corpus (TDD RED)** - `7c60d33` (test)
2. **Task 1: CorpusRow + export_corpus (TDD GREEN)** - `044a759` (feat)
3. **Task 2: Wire RefineCommands + CLI dispatch** - `87707a3` (feat)

_Note: Task 1 followed TDD RED/GREEN cycle with separate commits._

## Files Created/Modified
- `crates/rigor/src/cli/refine.rs` - Added CorpusRow struct, from_violation(), parse_since_date(), export_corpus(), RefineCommands enum, run_refine_command(), run_export(), 9 unit tests
- `crates/rigor/src/cli/mod.rs` - Changed Refine variant to subcommand, updated dispatch to run_refine_command()

## Decisions Made
- Extended cli/refine.rs in-place (Option A from research) to minimize file churn and honor over-editing guard
- CLI grammar changes from `rigor refine --apply` to `rigor refine suggest --apply` -- acceptable pre-1.0 breaking change
- Copied parse_since logic (13 lines) rather than extracting shared utility; Phase 3E can deduplicate
- Summary output goes to stderr (eprintln) to keep stdout clean for JSONL piping

## Deviations from Plan

None - plan executed exactly as written.

## TDD Gate Compliance

1. RED gate: `7c60d33` (test commit with 6 failing tests, 3 passing on stubs)
2. GREEN gate: `044a759` (feat commit, all 9 tests passing)
3. REFACTOR: Not needed -- implementation was clean on first pass

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- `rigor refine export` is fully functional for Phase 3E GEPA prompt optimization
- CorpusRow JSONL format ready for Phase 4E Modal discriminator training pipeline
- Phase 3E may restructure refine.rs into a module directory when adding optimizer/evaluator/mutator

---
*Phase: 02-pr-4-corpus-exporter*
*Completed: 2026-04-24*
