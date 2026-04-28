# [UMBRELLA] Epistemic Cortex: session-portable SQLite+vec belief store, event-sourced state, dynamic context assembly

Tracks the full Epistemic Cortex landing — rigor's cognitive substrate that replaces the static `build_epistemic_context` preamble with an event-sourced, logical-time-scoped, goal-conditioned retrieval system backed by SQLite + sqlite-vec, portable across sessions for a single user.

This is not a cache. It is the epistemic executive layer that any LLM routed through rigor's proxy gains, without the LLM or agent having to cooperate. The design target is a prefrontal-cortex-like layer that holds working memory, does source monitoring, tracks metacognitive confidence, performs goal-directed retrieval, monitors reality via LSP-anchor revalidation, and actively inhibits contradicted or stale beliefs.

## Motivation

Today rigor's `build_epistemic_context` (at `src/daemon/proxy.rs:1256`) packs a static snapshot of the constraint catalogue + current DF-QuAD strengths into a single system-prompt preamble. Same context every call, independent of what the current claim is about. That is a fixed-photograph grounding. For the vision documented in `.planning/roadmap/epistemic-expansion-plan.md` — knowledge types, justification tracking, Gettier guards, induction, credibility scoring, dynamic base strength, LSP-driven anchor re-grounding — a fixed photograph is insufficient.

The Epistemic Cortex replaces the fixed photograph with a live cognitive layer:

- **Working memory** — session-scoped active belief set with turn-based activation decay (not wall-clock)
- **Source monitoring** — every belief carries `KnowledgeType` + `ExtractionMethod` + credibility-weighted source
- **Metacognition** — DF-QuAD strengths, verification counts, contradiction counts, freshness state
- **Executive retrieval** — goal-conditioned top-k with confidence-gated elaboration modes (High / Medium / Low / Empty)
- **Reality monitoring** — periodic LSP-driven anchor revalidation; stale justifications get auto-inhibited
- **Inhibition** — first-class suppression ledger for contradicted / stale / low-credibility beliefs

All surfaced into the LLM context via a typed, cache-disciplined prompt structure that respects Claude's `cache_control: ephemeral` boundary so the amortizable preamble stays cached and only the per-turn delta pays full token price.

## Pinned design decisions

The following decisions are load-bearing for every sub-issue. They are the output of an extensive design thread; each is non-negotiable for this landing.

### Integration surface
- **Proxy-only.** No MCP, no per-tool plugins, no agent cooperation required. Rigor's transport-layer interception (TLS MITM) is the universal integration point. This aligns with rigor's existing "layer is universal" principle documented in the wider roadmap.
- **Model-agnostic.** Rigor never reads or writes the agent's conversation history. Claude Code keeps its history; opencode keeps its history; rigor maintains its own epistemic state and injects it at the system-prompt boundary. The model is the integration layer.

### Storage
- **SQLite as primary store.** Not a dedicated vector DB, not a dedicated graph DB, not a KV store. SQLite with `WITH RECURSIVE` for BFS/DFS, `sqlite-vec` for semantic similarity, and relational primitives for everything else.
- **Single shared DB** at `~/.rigor/rigor.db` — all sessions, all graphs, all events. Session is a scoping dimension at read time, not a separate file.
- **WAL mode, single-writer (daemon), many-readers (CLI).** `PRAGMA journal_mode = WAL`; `PRAGMA synchronous = NORMAL`; `PRAGMA foreign_keys = ON`; `PRAGMA busy_timeout = 5000`. Daemon holds the writer lock via extension of the existing `~/.rigor/daemon.pid`; CLI processes (`rigor log`, `rigor refine`) use read-only connections.
- **Portable across sessions, not users.** No cross-user graph sharing for this landing. No signing, no registry, no trust envelope. `rigor graph export <hash>` via `VACUUM INTO` can produce a single-file `.db` for session archival.
- **Postgres + pgvector as the eventual cloud backend** (Phase 4D, out of scope for this umbrella). Schema is written in dialect-portable form: `BLOB` → `bytea` trivial swap; unix-epoch-ms `INTEGER` → `timestamptz` trivial swap; JSON ops via a portable helper; vec0 → pgvector HNSW; same `WITH RECURSIVE` syntax.

### Driver & deps
- **`rusqlite` with `bundled` feature** — no system libsqlite dependency; reproducible builds; deterministic behavior across user machines. Not `sqlx` (macro overhead not needed here).
- **`sqlite-vec` extension** — single-extension vector search. Not a separate vector DB.
- **`refinery`** — schema migrations managed via `user_version` + versioned SQL files.
- **`r2d2` + `r2d2_sqlite`** — connection pooling for concurrent reads from the daemon's async context.
- **`candle-core` + `candle-transformers` + `tokenizers`** (feature-gated: `local-embeddings`) — BGE-small embedder, local CPU inference.

### Event sourcing
- **Event log is the source of truth.** `belief_events` table is append-only; never updated, never deleted outside explicit retention policy. Every state change passes through it.
- **Projections are caches, rebuildable.** `belief_state_current`, `working_memory`, `belief_edges` are derived from events. Blow them away and replay should reproduce identical state.
- **One transaction per event.** Event insert and projection update commit together or neither commits. SQLite's transactional guarantees ensure no drift.
- **Direct projection UPDATEs are forbidden outside `projection::apply_in_tx`.** Enforced by making projection tables private to the `store` module.

### Identity & canonicalization
- **Custom `CanonicalHash` format, not JCS.** Streaming SHA-256 over length-prefixed typed fields with locked enum tag bytes. ~10× faster than `serde_jcs` (target: <1μs vs. ~5–10μs per event), no intermediate String allocation, no JSON escaping.
- **Tag bytes are permanent.** Once an `EventPayload` variant is assigned a tag byte, that byte is locked for all time. New variants get new tags. Removed variants leave gaps.
- **`CANONICAL_FORMAT_VERSION` is the first byte hashed.** If the format is ever revved, both versions coexist via explicit version byte.
- **Content-addressed event IDs** — event_id = SHA-256 of canonical bytes. Same logical event → same event_id regardless of machine / writer.

### Logical time (this is the big one)
Wall-clock time is the wrong clock for epistemic decay. Different users and different projects have wildly divergent session cadences; a belief shouldn't decay during weekends and shouldn't survive 500 commits in a 2-hour burst.

- **Working memory activation decays per-turn, not per-second.** `activation = initial * (0.5 ** (elapsed_turns / half_life_turns))`. `sessions.turn_count` is first-class; every proxy request increments it. Background events (verification, decay sweeps, hook callbacks) do not tick the counter — only agent-initiated interaction does.
- **Empirical staleness is commit-distance, not wall-clock.** Using `git2` (already a rigor dep), count commits between `belief.last_verified_commit` and `HEAD` that touched the anchor path. If count ≥ `staleness_commit_threshold` (default 20), mark stale and trigger re-verification.
- **Testimonial credibility decays per events-without-validation.** `credibility = base * (0.5 ** (events_since_last_validation / half_life_events))`. `sources.events_since_last_validation` increments on assertion from that source; resets on any subsequent `BeliefVerified` where the source contributed.
- **Verification-loop pass interval is turn-gated** — `pass_interval_requests: 100` means a verification pass runs every 100 proxy requests. No wall-clock timers anywhere in the system.

The only wall-clock surfaces that remain: event `timestamp` (for audit ordering within a commit), optional `inhibited_until` (for configurable time-limited inhibitions; default is indefinite), and `sources.last_seen_at` (informational only).

### Session model
- **Session = user-facing Claude conversation.** Multiple daemon restarts can occur within one session; multiple sessions can run through one daemon.
- **Session detection primary: Claude Code hooks.** `/api/hooks/session/{start,end}` on the daemon's existing axum router. Hook fires SessionStart; the next proxy request from the matching client fingerprint within 5 seconds is correlated.
- **Session detection fallback: prefix-hash.** SHA-256 over the first N user messages of the request body (canonical-serialized). Same hash = same session; different hash = new session. No wall-clock extension — if prefix drifts, new session.
- **No wall-clock session timeout.**
- **Cross-session learning is expected.** `belief_events`, `sources`, `inhibitions` are global; `working_memory`, `session_goals`, `retrieval_events` are session-scoped but stored in the same DB.

### Retrieval contract
- **Goal-conditioned.** Every session has an extracted goal (one-shot LLM call on first user message); retrieval blends query embedding with goal embedding at configurable weight (default 0.3).
- **Confidence-gated modes** — High (`top_score >= 0.9`) / Medium (`any >= 0.7` in top 3) / Low (`any >= 0.5` in top 5) / Empty (`< confidence_floor`). Elaboration depth scales with confidence.
- **Empty retrieval fails open** — full static preamble still injected; empty-retrieval flagged in logs; response extraction escalated for novel-topic learning. No preloading.
- **Inhibited beliefs are filtered at retrieval time.** Always. Stale justifications never leak into prompts.

### Context assembly
- **Stable / dynamic split with cache boundary.** Constraint catalogue + rubric live in cached block (stable across session). Session state + working memory + retrieved + inhibited + metacognitive flags live below `cache_control: ephemeral` marker.
- **Typed sections.** Each region is labeled so the model can interpret them correctly (inhibition list vs. retrieved list vs. working memory).
- **Budget per confidence mode, capped per project** via `max_dynamic_tokens`.

### Epistemology expansion hooks
Every item in `.planning/roadmap/epistemic-expansion-plan.md` maps to concrete schema:

| Plan item | Schema location |
|---|---|
| Knowledge types (empirical/rational/testimonial/memory) | `belief_state_current.knowledge_type` + index |
| Justification tracking | `belief_state_current.kind='justification'` + `belief_edges.relation_type='justified_by'` |
| Gettier guards | `verification_events` with `anchor_sha256` + `file_sha256` comparison → auto-inhibit on drift |
| Induction tracking | `belief_state_current.verification_count` + `last_verified_at` + `last_verified_commit` |
| Credibility scoring | `sources` table with `credibility_weight` + `events_since_last_validation` |
| Dynamic DF-QuAD base strength | Phase 4B `compute_base_strength` reads these columns; caches into `belief_state_current.current_strength` via `StrengthUpdated` events |

### TDD discipline
Per project memory (`feedback_tdd.md`): all rigor development uses TDD with rigorous e2e tests from the start. Every sub-issue below specifies a three-layer test plan (unit, e2e, performance) with concrete function-level contracts and measurable thresholds.

### Model discipline for subagents
Per project memory (`feedback_subagent_model.md`): every Agent dispatch uses `model: "opus"` — never Haiku or Sonnet, even for "mechanical" tasks. This applies to any delegation during implementation.

## Open sub-issues

Twelve sub-issues. Each is independently shippable behind the prior gate; the dependency graph below makes ordering explicit.

### Foundation (strict sequence)

- [ ] **EC-1** (#35) — SQLite substrate: deps, pragmas, writer-lock, first DDL, CanonicalHash trait, locked tag-byte registry. Zero functional change; all infrastructure.
- [ ] **EC-2** (#36) — Event log + projections + `EpistemicStore` trait + SqliteEpistemicStore. Daemon can write events and read projections; no other layer wired.

### Session & source layer (parallelizable with EC-3 and EC-4)

- [ ] **EC-3** (#37) — `SessionResolver` with hook endpoints (primary) + prefix-hash fallback. `sessions.turn_count` increments per request. No wall-clock.
- [ ] **EC-4** (#38) — `SourceRegistry` with seeded defaults + `GoalTracker` (one-shot LLM goal extraction per session) + goal embeddings.

### Cognitive substrate (sequential; each layer depends on the prior)

- [ ] **EC-5** (#39) — `WorkingMemory` with turn-based activation decay. Per-session; half-life in turns; evict below threshold.
- [ ] **EC-6** (#40) — `Embedder` trait + `BgeSmallEmbedder` + sqlite-vec + `RetrievalEngine` with confidence-gated modes. Per-project embedder config in rigor.yaml.
- [ ] **EC-7** (#41) — `InhibitionLedger` + contradiction detection (constraint co-violation → polarity → LLM-judge). Automatic inhibit on Drifted / Missing / Contradicted.

### Presentation & rollout

- [ ] **EC-8** (#42) — `ContextAssembler` with stable/dynamic split, `cache_control: ephemeral` boundary, empty-retrieval mode, typed sections.
- [ ] **EC-9** (#43) — `VerificationLoop` with LSP anchor re-grounding + commit-distance staleness + turn-gated pass interval + testimonial credibility decay.
- [ ] **EC-10** (#44) — Proxy cutover: `build_epistemic_context` → `EpistemicCortex::assemble_for_request`, behind `epistemic_cortex` config flag. First user-visible change.

### Downstream (post-cutover; parallelizable)

- [ ] **EC-11** (#45) — `rigor refine --gaps` CLI surfacing persistent empty-retrieval patterns with remediation suggestions.
- [ ] **EC-12** (#46) — One-shot migration: `~/.rigor/memory.json` → belief_events; deprecate the JSON file.

## Dependency graph

```
EC-1 ──► EC-2 ──┬─► EC-3 ──┐
                │          ├─► EC-5 ──► EC-6 ──► EC-7 ──► EC-8 ──┐
                └─► EC-4 ──┘                                      ├─► EC-10 ──┬─► EC-11
                                                  EC-9 ──────────┘            └─► EC-12
```

- EC-3 and EC-4 can ship in parallel once EC-2 lands.
- EC-9 (`VerificationLoop`) can land in parallel with EC-7 and EC-8 — it uses EC-2's store but doesn't depend on retrieval.
- EC-11 and EC-12 parallelize post-cutover.

## Recommended order

1. **EC-1** — deps, pragmas, CanonicalHash. No daemon changes yet.
2. **EC-2** — event log + projections. Daemon can observe events (still logs violations.jsonl in parallel).
3. **EC-3** + **EC-4** in parallel.
4. **EC-5** — working memory activation.
5. **EC-6** — largest single stage; embedder + vec + retrieval.
6. **EC-7** — inhibition + contradiction.
7. **EC-8** — assembler (still not wired to proxy; standalone harness).
8. **EC-9** — verification loop in parallel with EC-7/EC-8.
9. **EC-10** — proxy cutover. Flag-gated. This is the visible one.
10. **EC-11**, **EC-12** — downstream polish / cleanup.

## Per-project config surface

All knobs live in `rigor.yaml` under a new top-level `epistemic:` section:

```yaml
epistemic:
  embedder:
    kind: bge-small            # | "openai" | "voyage" | "custom"
    dimension: 384             # MUST match the DDL; changing requires full re-embed
  decay:
    working_memory_half_life_turns: 10
    staleness_commit_threshold: 20
    credibility_half_life_events: 50
  retrieval:
    confidence_floor: 0.5
    max_dynamic_tokens: 1500
    goal_weight: 0.3
  verification:
    max_lsp_calls_per_pass: 50
    pass_interval_requests: 100
    prefer_knowledge_types: ["empirical"]
```

## Module tree (final shape)

```
crates/rigor/src/memory/
├── content_store.rs                      (existing)
├── episodic.rs                           (existing; retired in EC-12)
├── mod.rs                                (updated to re-export epistemic)
└── epistemic/
    ├── mod.rs                            pub EpistemicCortex facade
    ├── canonical.rs                      CanonicalHash trait + locked tag registry
    ├── event.rs                          BeliefEvent + EventPayload variants
    ├── projection.rs                     apply_in_tx + rebuild_from_events
    ├── cortex.rs                         EpistemicCortex struct
    │
    ├── store/
    │   ├── mod.rs                        EpistemicStore trait
    │   ├── sqlite.rs                     SqliteEpistemicStore
    │   ├── in_memory.rs                  InMemoryEpistemicStore (tests)
    │   ├── schema.rs                     DDL strings
    │   └── migrations/
    │       ├── V1__init.sql
    │       ├── V2__vec_tables.sql
    │       └── ...
    │
    ├── session.rs                        SessionResolver + hook endpoints
    ├── sources.rs                        SourceRegistry
    ├── goals.rs                          GoalTracker
    ├── working_memory.rs                 WorkingMemory trait + SqliteWorkingMemory
    ├── embed.rs                          Embedder trait + BgeSmallEmbedder
    ├── retrieval.rs                      RetrievalEngine + RetrievalMode + RetrievalQuery
    ├── inhibition.rs                     InhibitionLedger + contradiction detection
    ├── verification.rs                   VerificationLoop + commit-distance
    └── context.rs                        ContextAssembler
```

## Success criteria for closing this umbrella

- All 12 sub-issues closed with tests passing (unit + e2e + perf thresholds met).
- `proxy_request` uses `EpistemicCortex::assemble_for_request` with `epistemic_cortex` flag on by default on main.
- `~/.rigor/rigor.db` exists after first daemon start; schema version matches DDL; WAL mode confirmed.
- `rigor refine --gaps` surfaces real empty-retrieval patterns on a populated DB.
- `memory.json` is migrated and deprecated; `rigor refine` uses unified store.
- All canonical event IDs are stable across runs (golden-file tests green).
- Daemon restart does not lose relevance cache — cold-start retrieval returns prior verdicts.
- Performance budgets met: per-request proxy overhead <50ms p99; retrieval top-k <10ms p99; canonical hash <1μs per event; replay 100k events <10s.

## Out of scope (future work seeded here for continuity)

The following items are DELIBERATELY excluded from this umbrella but are close enough in design space that pointers here prevent re-discovery cost later. Each will get its own umbrella when unblocked.

### Edit-minimality constraint family
Empirical basis: the 2026 Rehir article "Coding Models Are Doing Too Much" (nrehiew.github.io/blog/minimal_editing/) shows that frontier coding models systematically over-edit — rewriting more than the minimal fix requires. Measurable via:
- Normalized token-Levenshtein distance on Python-style tokenized code (relative to ground-truth minimal fix).
- Added cognitive-complexity score (nesting penalties, recursion, control-flow depth).

Both metrics become computable constraints under rigor's existing Belief/Justification/Defeater model. Specifically: a Defeater-kind constraint `edit-minimality` lives in `src/defaults/<language>.rs` and fires when a proposed code change exceeds a minimality threshold for its stated task.

This umbrella contributes one small piece of this story: **EC-8's preservation-minimality instruction** in the ContextAssembler injects the Rehir article's universally-validated instruction ("preserve original code as much as possible") when `AssemblerHint.active_tools` includes file-mutation tools. That alone is empirically shown to improve both correctness AND minimality on frontier models. The full constraint family + `rigor refine --over-editing` detection (EC-11's future extension) is a separate umbrella.

### RL-trained Judge Agent — training recipe
This umbrella builds the **substrate** an RL-trained Judge Agent would use (belief_events are the preference dataset; retrieval_events are the attention log; commit-distance + LSP give verifiable rewards). The training loop itself is a separate initiative.

Training recipe pinned from the same Rehir article (so the next session doesn't re-litigate):
- **SFT memorizes** — in-domain 0.932 Pass@1, out-of-domain 0.458 (catastrophic collapse on held-out mutation types). Do NOT use SFT alone.
- **RL with KL-minimality bias generalizes** — maintains LiveCodeBench with no forgetting. Recipe: Qwen3-scale base + rejection-sampled SFT as warm-start + RL on composite reward.
- **Reward shape**: `r = r_edit + 0.1` (pass), `r = -0.2` (fail). Failed rollouts receiving `0` caused reward hacking in LoRA — the negative-reward-for-failure is load-bearing.
- **LoRA rank 64 ≈ full RL for behavioral tuning** — cheaper training path once the pipeline works.

Prerequisite before starting the RL-judge umbrella: (a) >10k human annotations in the violation log (i.e., `rigor annotate` adoption), (b) GEPA prompt-optimization showing diminishing returns, (c) a specific failure mode prompt-space can't fix. Until those land, RL is expensive theatre — prompt-space and GEPA are cheaper first moves.

### Already-listed out-of-scope (retained)
- Cross-user graph sharing (Phase 4D / Rigor Cloud)
- Postgres backend (Phase 4D; trait-swap pattern already designed)
- Vector clustering / Leiden on the belief graph (`cluster_id` column exists; population is future)
- Cross-graph queries (codebase × argumentation) — schema supports via cross-kind edges; query patterns are future
- Graphify replacement (external tool stays until native replacement)
- Retention / GC for `belief_events` and `retrieval_events`
- `rigor reload` live-config CLI (hinted in EC-10; may be deferred)

## Additional items surfaced in review

Five cross-cutting concerns discovered during the pre-implementation audit. Each is load-bearing across multiple sub-issues and must be resolved at the umbrella level rather than in any single child.

### X-1: Observability contract for every layer

Every layer emits OTel spans/counters into the existing pipeline in `src/observability/`. Required attributes per layer:

| Layer | Span name | Attributes |
|---|---|---|
| Event append | `cortex.append_event` | `event_type`, `belief_id`, `session_id`, `canonical_ms`, `tx_ms` |
| Projection apply | `cortex.projection.apply` | `event_type`, `projection_ms` |
| Session resolve | `cortex.session.resolve` | `detection_method`, `is_new`, `turn_count_after` |
| Goal extract | `cortex.goal.extract` | `session_id`, `llm_ms`, `model`, `fell_back` |
| WM activate/touch | `cortex.wm.activate` / `cortex.wm.touch` | `session_id`, `belief_id`, `role` |
| Retrieve | `cortex.retrieve` | `session_id`, `mode`, `k`, `retrieved_count`, `used_count`, `inhibited_count`, `embed_ms`, `vec_search_ms`, `total_ms`, `goal_weight_applied` |
| Inhibit | `cortex.inhibit` / `cortex.lift` | `belief_id`, `reason` |
| Verification pass | `cortex.verify.pass` | `beliefs_considered`, `beliefs_verified`, `beliefs_drifted`, `beliefs_missing`, `lsp_calls_made`, `wall_time_ms`, `budget_exhausted` |
| Context assemble | `cortex.assemble` | `session_id`, `mode`, `stable_tokens`, `dynamic_tokens`, `preamble_cache_hit` |
| Cortex tick | `cortex.tick` | `passes_completed`, full PassReport attributes |

Counters (integer) and histograms (latency) derivable from spans. EC-10 wires span emission at the facade; each sub-issue adds emission at its layer's entry points.

### X-2: No-recursion discipline for rigor's own LLM calls

Three rigor-internal LLM call sites exist: goal extraction (EC-4), tier-3 contradiction judge (EC-7), and OpenAI embedder when configured (EC-6). All three are outbound HTTP requests that would, without guards, be re-intercepted by rigor's MITM proxy and re-evaluated as if they were user agent traffic.

Mitigation — a **`X-Rigor-Internal` request header** marks all rigor-internal traffic. The proxy's request handler short-circuits when this header is present: no claim extraction, no evaluator pipeline, no cortex injection, no `record_response`. The upstream call still goes through normally.

Header value: `X-Rigor-Internal: <purpose>` where purpose is one of `goal-extraction` | `contradiction-judge` | `embedder`.

Every rigor-internal call site MUST set this header. Missing-header → recursion. Tests in EC-4, EC-6, EC-7 verify the header is set; an integration test in EC-10 verifies the proxy short-circuit.

### X-3: Rollback procedure for EC-10 cutover

Flag-off reverts to pre-EC-10 path. But the DB still contains belief_events from flag-on periods. Two decisions:

1. **Events stay.** A flag-off daemon simply stops writing new events. Existing events remain for future flag-on re-enablement. No retention cleanup on rollback.
2. **Projections may go stale.** With flag-off, projections don't advance. On flag-on re-enablement, call `rebuild_projections()` to catch up. Document this in the config description for `epistemic_cortex`.
3. **No automatic data migration back to `violations.jsonl`.** If user wants historical violation data in the old format, `rigor epistemic export --format jsonl` (out of scope for this umbrella) is the future tool.

Rollback is thus: flip flag to false → restart daemon → done. Data is preserved, available for re-enable.

### X-4: Cross-platform support

Target platforms:
- **Linux x86_64** — primary CI target. All features supported.
- **macOS arm64 + x86_64** — primary user target. All features supported. BGE-small via candle Metal acceleration if available.
- **Windows x86_64** — best-effort. `rusqlite bundled` works; `git2` works; LSP client has fewer server options available. Not a CI target in this umbrella.

Per-feature platform notes:
- `fs2` file locking: POSIX + Windows flock. Tested on both.
- BGE-small CPU: all platforms. GPU acceleration is opt-in via candle-cuda feature, not included in default build.
- LSP client: depends on user having language servers installed. No change from current rigor behavior.

### X-5: Schema forward-migration and version safety

- **Forward migrations** via refinery: older binary + newer DB is detected at startup. `PRAGMA user_version` > binary's max version → daemon refuses to start with: "Database schema is at version {N}; this binary supports up to {M}. Upgrade rigor."
- **Backward migrations** are unsupported. If a user downgrades the binary, they must restore from backup (or the safely-renamed `memory.json.migrated-TIMESTAMP`). Document in release notes.
- **Mid-migration failures** leave the DB in a recoverable state: refinery records each migration as atomic; a failed migration rolls back and the daemon refuses to start pointing to the specific migration file.
- **Idempotent seeds.** V4's `INSERT INTO sources` uses `ON CONFLICT DO NOTHING` implicitly via primary-key constraint so re-running the migration (e.g., if user deletes the refinery `__diesel_schema_migrations`-equivalent tracking table) is safe.

## Reference

- Design thread (this umbrella body distills the full design decisions)
- `.planning/roadmap/epistemic-expansion-plan.md` — prior epistemic roadmap
- `src/daemon/proxy.rs:1256` — current `build_epistemic_context` call site (replaced in EC-10)
- `src/memory/content_store.rs` — existing pluggable-backend pattern the epistemic store follows
- Project memory: `project_epistemology_expansion.md`, `project_token_economy.md`, `project_layer_is_universal.md`, `feedback_tdd.md`, `feedback_subagent_model.md`
