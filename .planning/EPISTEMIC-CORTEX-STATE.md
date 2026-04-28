# Epistemic Cortex — Session Resumption State

> **Snapshot** of the Epistemic Cortex implementation posture. Read this first when resuming work on the cortex from a cold context.

## Current stage

**Pre-implementation.** All design, issue-drafting, review, and memory capture is done. Ready to begin EC-1 implementation.

- Umbrella and 12 sub-issues: drafted → published → reviewed → gap closures added
- Architectural decisions: saved as project memory (auto-loads next session)
- Graphify codebase map: committed under `.planning/graphs/`
- No code has been written for the cortex yet

## How to resume

```bash
# 1. Load architectural context
gh issue view 34                              # umbrella
gh issue view 35                              # EC-1 — the next work item

# 2. Or read local drafts (authoritative copy; same body as the GitHub issues)
cat .planning/issues-draft/UMBRELLA.md
cat .planning/issues-draft/EC-1.md

# 3. For module navigation of the existing codebase
cat .planning/graphs/GRAPH_REPORT.md

# 4. Confirm git state
git status
git log --oneline -10
git branch --show-current
```

## Status snapshot

| Artifact | Location | Status |
|---|---|---|
| Umbrella issue | [rigor-cloud/rigor#34](https://github.com/Rigor-Cloud/rigor/issues/34) | Published + gap closures |
| EC-1 — SQLite substrate | [#35](https://github.com/Rigor-Cloud/rigor/issues/35) | Ready to implement |
| EC-2 — Event log + projections | [#36](https://github.com/Rigor-Cloud/rigor/issues/36) | Queued |
| EC-3 — SessionResolver | [#37](https://github.com/Rigor-Cloud/rigor/issues/37) | Queued |
| EC-4 — SourceRegistry + GoalTracker | [#38](https://github.com/Rigor-Cloud/rigor/issues/38) | Queued |
| EC-5 — WorkingMemory | [#39](https://github.com/Rigor-Cloud/rigor/issues/39) | Queued |
| EC-6 — Embedder + sqlite-vec + Retrieval | [#40](https://github.com/Rigor-Cloud/rigor/issues/40) | Queued |
| EC-7 — InhibitionLedger + contradiction | [#41](https://github.com/Rigor-Cloud/rigor/issues/41) | Queued |
| EC-8 — ContextAssembler | [#42](https://github.com/Rigor-Cloud/rigor/issues/42) | Queued |
| EC-9 — VerificationLoop | [#43](https://github.com/Rigor-Cloud/rigor/issues/43) | Queued |
| EC-10 — Proxy cutover | [#44](https://github.com/Rigor-Cloud/rigor/issues/44) | Queued |
| EC-11 — rigor refine --gaps | [#45](https://github.com/Rigor-Cloud/rigor/issues/45) | Queued |
| EC-12 — memory.json migration | [#46](https://github.com/Rigor-Cloud/rigor/issues/46) | Queued |
| Local drafts | `.planning/issues-draft/*.md` | 5067 lines total |
| Codebase map | `.planning/graphs/GRAPH_REPORT.md` | Labeled communities |
| Phase 0 umbrella (parallel work) | [#28](https://github.com/Rigor-Cloud/rigor/issues/28) | Separate track |

Current branch at time of drafting: `phase-0/pr-2.7-corpus-scaffold` (Phase 0 PR-2.7 work, not cortex work). Cortex work should start on a fresh branch off `main`, e.g. `phase-1/ec-1-sqlite-substrate`.

## Dependency order

```
EC-1 (#35) ── substrate, zero behavior change
   └─► EC-2 (#36) ── event log + projections
         ├─► EC-3 (#37) ── session resolver       ─┐
         └─► EC-4 (#38) ── sources + goals        ─┤ (EC-3 + EC-4 parallelizable)
               └─► EC-5 (#39) ── working memory   ─┘
                     └─► EC-6 (#40) ── embedder + retrieval  (largest single slice)
                           └─► EC-7 (#41) ── inhibition + contradiction
                                 └─► EC-8 (#42) ── context assembler
                                                                        ┐
                                       EC-9 (#43) ── verification loop ─┤  (EC-9 parallelizable with 7/8)
                                                                        │
                                                                    EC-10 (#44) ── proxy cutover (flag-gated)
                                                                        ├─► EC-11 (#45) ── rigor refine --gaps
                                                                        └─► EC-12 (#46) ── memory.json migration
```

## Key implementation discipline

Every sub-issue assumes these disciplines; they're not repeated in each. Fresh session should treat them as global rules:

- **TDD required** (per `feedback_tdd.md` memory). Trait-contract tests before impl; e2e tests alongside impl; perf benches with committed baselines.
- **Opus for any spawned Agent** (per `feedback_subagent_model.md`).
- **Every layer emits OTel spans** per umbrella X-1. Span name and required attributes specified per layer.
- **Every rigor-internal LLM call sets `X-Rigor-Internal` header** per umbrella X-2. Three call sites: goal extraction (EC-4), tier-3 contradiction judge (EC-7), OpenAI embedder when configured (EC-6). Proxy short-circuits on this header.
- **Trait-first, impl-second.** Write `EpistemicStore`/`WorkingMemory`/`RetrievalEngine`/etc. trait + contract-test suite; then `SqliteX` and `InMemoryX` impls both satisfy it.
- **Schema migrations are append-only.** V1 → V12 over the course of this umbrella. Never edit a published migration file.
- **`CANONICAL_FORMAT_VERSION = 0x01` locked.** Event-payload tag bytes are permanent; new variants get new tags; removed variants leave gaps.
- **Logical time only, not wall-clock** (per `feedback_cortex_logical_time.md` memory).
- **SQLite + sqlite-vec for all persistence** (per `project_cortex_sqlite_primary.md` memory).
- **Proxy-only integration, no MCP** (per `feedback_cortex_mcp_rejected.md` memory).

## What's NOT in scope for this umbrella

- Cross-user graph sharing (Phase 4D / Rigor Cloud)
- Postgres backend (Phase 4D; designed as a trait-swap, not a rewrite)
- Vector clustering / Leiden (separate; `cluster_id` column already exists on `Constraint`)
- RL-trained judge agent (separate initiative; this umbrella is the substrate it needs)
- Graphify replacement (external tool continues; native replacement is future work)
- Cross-user memory import / signed portable graphs
- Retention / garbage-collection policies for `belief_events` and `retrieval_events`
- `rigor reload` live-config CLI (hinted in EC-10; may be deferred)

## Related memory entries (auto-load on session start in this repo)

Design decisions surfaced in this umbrella, saved to `~/.claude/projects/.../memory/`:

- `feedback_cortex_mcp_rejected.md` — MCP off the table; proxy-only
- `feedback_cortex_logical_time.md` — turns / commits / events; never wall-clock
- `project_cortex_sqlite_primary.md` — rigor.db + sqlite-vec for everything
- `project_cortex_session_definition.md` — session = Claude conversation; hook + prefix-hash detection
- `project_cortex_bge_small_default.md` — local 384-dim default embedder; per-project configurable
- `reference_kuzudb_archived.md` — KuzuDB wound down 2025; don't suggest it
- `reference_over_editing_article.md` — Rehir 2026: preservation instruction universally works; RL+KL generalizes; exact reward shape

Upstream plans this umbrella implements (pre-existing memory):

- `project_epistemology_expansion.md` — the six-item knowledge-types plan
- `project_token_economy.md` — cache-by-knowledge-type patterns the cortex enables
- `project_layer_is_universal.md` — "proxy is the integration, not plugins"
- `project_dfquad_formula.md` — DF-QuAD product-of-complements (used by EC-9 repropagation)
- `project_epistemic_sandbox.md` — vision this landing serves

## External evidence integrated into the design

### Rehir 2026 "Coding Models Are Doing Too Much" (nrehiew.github.io/blog/minimal_editing/)
Empirical basis for two updates to the current umbrella, pushed 2026-04-23:

- **EC-8 (#42)** got a preservation-minimality instruction section. When `AssemblerHint.active_tools` includes `Edit`/`Write`/`NotebookEdit`, the dynamic body renders a `# Code-editing discipline` block with the paper's empirically-validated instruction ("preserve original code and logic as much as possible"). Golden-text test locks the exact wording.
- **EC-11 (#45)** got over-editing as a second gap category. Future `rigor refine --over-editing` surfaces proxy requests where token-Levenshtein distance + added cognitive-complexity delta exceed historical norms. Out-of-scope core; wires in when tool-use diffs flow through claim extraction.
- **Umbrella (#34)** got an "Out of scope — future work" section pinning: (a) the `edit-minimality` Defeater constraint family for `src/defaults/<language>.rs`, and (b) the RL-Judge-Agent training recipe (SFT memorizes; RL+KL generalizes; reward `r = r_edit + 0.1` on pass, `r = -0.2` on fail; LoRA rank 64 ≈ full RL).

Full findings preserved in `reference_over_editing_article.md` memory entry (pinned numbers + metrics + reward-shape + reward-hacking warning). Future sessions load this automatically.

## Files and paths referenced by the issues

- `src/daemon/proxy.rs:1256` — where `build_epistemic_context` is called today; replaced in EC-10
- `src/memory/content_store.rs` — existing pluggable-backend pattern the cortex traits follow
- `src/memory/episodic.rs` — legacy `MemoryStore`; migrated and deprecated in EC-12
- `src/constraint/graph.rs` — existing DF-QuAD `ArgumentationGraph`; reused by EC-9
- `src/claim/types.rs` — `Claim`, `ClaimType`, `KnowledgeType` enums reused
- `src/constraint/types.rs` — `Constraint`, `SourceAnchor`, `Relation`, `ExtractionMethod` reused
- `src/lsp/` — existing LSP client used by EC-9's verification loop
- `src/daemon/governance.rs` — existing file-lock discipline pattern for writer lock
- `~/.rigor/daemon.pid` — existing liveness convention; cortex adds `~/.rigor/rigor.db.writer.lock`
- `~/.rigor/rigor.db` — new single SQLite file holding all cortex state

## Open non-blocking questions for implementation time

These came up in design but didn't need to be pinned before starting:

- Exact half-life default for `working_memory_half_life_turns` (currently 10; EC-5 ships with this and we measure in production).
- Exact `staleness_commit_threshold` default (currently 20; EC-9 ships, measure).
- Exact `credibility_half_life_events` default (currently 50; EC-4 ships, measure).
- Goal-extraction prompt wording (draft in EC-4's operational notes; may want iteration).
- Retention policy for `belief_events` — deferred; current guidance is "keep forever in local DB."

## Preferred first commit sequence after picking this up

1. Create a branch off main: `git checkout -b phase-1/ec-1-sqlite-substrate main`.
2. `cat .planning/issues-draft/EC-1.md` — re-read the issue locally.
3. Add Cargo deps from EC-1's "Cargo additions" section.
4. Write `canonical.rs` tests first (TDD): primitive impls + `CANONICAL_FORMAT_VERSION = 0x01` + golden file.
5. Implement `canonical.rs` primitive impls.
6. Write `store/sqlite.rs` tests: pragma application, writer lock, migration v1.
7. Implement `SqliteSubstrate` + `V1__init.sql`.
8. Run `cargo clippy -- -D warnings` and `cargo fmt --check`.
9. Write perf benchmark (`canonical_hash.rs`); commit baselines.
10. Mark EC-1 acceptance criteria checkboxes; open PR referencing #35 and #34.
