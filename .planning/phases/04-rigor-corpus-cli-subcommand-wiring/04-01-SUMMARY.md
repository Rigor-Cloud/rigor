---
phase: 04-rigor-corpus-cli-subcommand-wiring
plan: 01
subsystem: cli
tags: [clap, corpus, openrouter, sha256, tokio, serde-json]

# Dependency graph
requires:
  - phase: PR-2.7 (c6f885c)
    provides: corpus library (record, stats, validate, client, manifest, recording modules)
provides:
  - "rigor corpus record CLI subcommand dispatching to corpus::record_prompt"
  - "rigor corpus stats CLI subcommand with JSON output via compute_stats + aggregate_by_model"
  - "rigor corpus validate CLI subcommand with SHA-256 hash and schema verification"
affects: [05-seed-corpus-recording, 06-pretty-print-stats]

# Tech tracking
tech-stack:
  added: []
  patterns: [nested-subcommand-enum, tokio-runtime-bridge, json-macro-output]

key-files:
  created:
    - crates/rigor/src/cli/corpus.rs
  modified:
    - crates/rigor/src/cli/mod.rs

key-decisions:
  - "Used serde_json::json! for stats output since ModelStats/PerModelAggregate lack Serialize derive"
  - "Stats replay uses PolicyEngine + extract_claims_from_text matching tests/corpus_replay.rs pattern"
  - "Validate uses sample.model (original unslugged name) for hash recomputation instead of reversing slug"

patterns-established:
  - "Corpus CLI pattern: CorpusCommands enum with run_corpus_command dispatcher following RefineCommands model"

requirements-completed: [REQ-010, REQ-011, REQ-012]

# Metrics
duration: 5min
completed: 2026-04-24
---

# Phase 4 Plan 1: Corpus CLI Subcommand Wiring Summary

**Wired `rigor corpus record/stats/validate` CLI over merged corpus library with tokio runtime bridge for async record and JSON stats output**

## Performance

- **Duration:** 5 min
- **Started:** 2026-04-24T08:09:27Z
- **Completed:** 2026-04-24T08:14:45Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Created cli/corpus.rs with CorpusCommands enum (Record, Stats, Validate) and three handler functions
- Wired Commands::Corpus into cli/mod.rs with exactly 3 insertions (pub mod, enum variant, dispatch arm)
- All 380 existing tests pass, zero warnings, cargo build clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Create cli/corpus.rs with CorpusCommands enum and handlers** - `cdff919` (feat)
2. **Task 2: Wire Commands::Corpus into cli/mod.rs** - `bbc74aa` (feat)

## Files Created/Modified
- `crates/rigor/src/cli/corpus.rs` - New file: CorpusCommands enum + run_corpus_command dispatcher + record/stats/validate handlers (281 lines)
- `crates/rigor/src/cli/mod.rs` - 3 insertions: pub mod corpus, Commands::Corpus variant, dispatch arm (7 lines added)

## Decisions Made
- Used `serde_json::json!` macro for stats JSON output because `ModelStats` and `PerModelAggregate` do not derive `Serialize` -- avoids modifying library code
- Stats replay function resolves rigor.yaml via explicit `--rigor-yaml` flag or `find_rigor_yaml(None)` auto-detect, falling back to pass-through `|_| false` with stderr warning if not found
- Validate handler uses `sample.model` (the original unslugged model name stored in RecordedSample) for hash recomputation rather than reversing the slug, eliminating the risk identified in Research assumption A2

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 5 (seed corpus recording) can now use `rigor corpus record --models <slugs> --prompts <dir>` to record real LLM responses
- Phase 6 (pretty-print stats) can build on `rigor corpus stats` JSON output to add table formatting
- `rigor corpus validate` is ready for CI integration to verify recording integrity

## Self-Check: PASSED

- FOUND: crates/rigor/src/cli/corpus.rs
- FOUND: crates/rigor/src/cli/mod.rs (with pub mod corpus, Commands::Corpus, dispatch arm)
- FOUND: commit cdff919 (Task 1)
- FOUND: commit bbc74aa (Task 2)
- cargo build: zero errors, zero warnings
- cargo test -p rigor --lib: 380 passed, 0 failed

---
*Phase: 04-rigor-corpus-cli-subcommand-wiring*
*Completed: 2026-04-24*
