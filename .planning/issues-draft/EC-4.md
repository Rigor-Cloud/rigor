# EC-4: `SourceRegistry` + `GoalTracker` — credibility & goal-conditioned retrieval substrate

> Part of umbrella: #34 [UMBRELLA] Epistemic Cortex
> Depends on: **EC-1**, **EC-2**
> Parallelizable with: **EC-3**
> Lands in: `crates/rigor/src/memory/epistemic/sources.rs`, `goals.rs`, plus migration

## Scope

Two tightly-coupled data substrates that feed retrieval and credibility reasoning. After this lands:

- Every belief in the system knows where it came from via a `source_id` FK into the `sources` table.
- Sources have a live `credibility_weight` that decays by **events-without-validation**, not wall-clock.
- Default sources are seeded at DB init: Claude models, GPT models, human, LSP, AST, rigor-map.
- Every session has (at most) one active `Goal`, extracted by a one-shot LLM call on the first user message.
- Goal text and goal embedding are persisted so EC-6's retrieval can blend goal similarity into its query.
- Two new events — `SourceCredibilityAdjusted` and `GoalExtracted` / `GoalCompleted` — are wired into the projections from EC-2.

No retrieval yet; no embeddings used yet (goal embeddings are stored but unused until EC-6). This issue delivers only the static substrate that retrieval depends on.

## Design constraints pinned from the design thread

- **Source credibility decays by events, not time.** `credibility = base * (0.5 ** (events_since_last_validation / half_life_events))`. A source producing many claims without validation halves its credibility per `half_life_events` claims. A source producing fewer but consistently verified claims holds credibility indefinitely.
- **`events_since_last_validation` is a first-class counter.** Increments on every `BeliefAsserted` attributed to that source. Resets to 0 on every `BeliefVerified` that names the source.
- **Default seed sources have defensible starting credibility.** Opus 0.95, Sonnet 0.85, Haiku 0.75, human 1.0, LSP 0.99, AST 0.95, rigor-map 0.9.
- **Goal extraction is one-shot per session.** Called on first user message; result cached; subsequent requests reuse.
- **Goal embedding is persisted.** Stored separately from belief embeddings (different `vec0` virtual table) so EC-6 can blend.
- **Per-project config drives embedder dimension.** rigor.yaml's `epistemic.embedder.dimension` must match the DDL `FLOAT[N]` for goal_embeddings. Dimension mismatch on startup → daemon refuses to start and prints re-embed instructions.
- **LLM goal extraction runs through rigor's own proxy path.** Rigor eating its own dog food. Uses the user's configured model, default Opus.

## What lands

```
crates/rigor/src/memory/epistemic/
  ├── sources.rs                                (SourceRegistry trait + SqliteSourceRegistry)
  └── goals.rs                                  (GoalTracker trait + SqliteGoalTracker)

crates/rigor/src/memory/epistemic/store/migrations/
  └── V4__sources_and_goals.sql

crates/rigor/src/config/
  └── epistemic.rs                              (NEW: EpistemicConfig loaded from rigor.yaml)

tests/
  ├── epistemic_sources.rs
  └── epistemic_goals.rs

benches/
  └── source_credibility.rs
```

## Schema contributions

**`V4__sources_and_goals.sql`:**

```sql
CREATE TABLE sources (
  source_id                     TEXT PRIMARY KEY,
  source_kind                   TEXT NOT NULL,            -- 'claude-opus'|'claude-sonnet'|'claude-haiku'|'gpt-5'|'gpt-4o'|'human'|'lsp-verified'|'ast-extracted'|'rigor-map'|'external'
  display_name                  TEXT NOT NULL,
  base_credibility              REAL NOT NULL,
  credibility_weight            REAL NOT NULL,            -- current; derived from base + events_since_last_validation
  events_since_last_validation  INTEGER NOT NULL DEFAULT 0,
  total_contributions           INTEGER NOT NULL DEFAULT 0,
  accurate_count                INTEGER NOT NULL DEFAULT 0,
  contradicted_count            INTEGER NOT NULL DEFAULT 0,
  first_seen_at                 INTEGER NOT NULL,
  last_seen_at                  INTEGER NOT NULL,
  last_event_id                 BLOB,                     -- last event that updated credibility
  FOREIGN KEY (last_event_id) REFERENCES belief_events(event_id)
) STRICT;

-- Seed rows inserted via migration body (rather than an imperative seed function)
-- so DBs created from scratch have consistent defaults.
INSERT INTO sources (source_id, source_kind, display_name, base_credibility, credibility_weight, first_seen_at, last_seen_at)
VALUES
  ('claude-opus-4-7',      'claude-opus',    'Claude Opus 4.7',     0.95, 0.95, 0, 0),
  ('claude-sonnet-4-6',    'claude-sonnet',  'Claude Sonnet 4.6',   0.85, 0.85, 0, 0),
  ('claude-haiku-4-5',     'claude-haiku',   'Claude Haiku 4.5',    0.75, 0.75, 0, 0),
  ('gpt-5',                'gpt-5',          'GPT-5',               0.85, 0.85, 0, 0),
  ('gpt-4o',               'gpt-4o',         'GPT-4o',              0.80, 0.80, 0, 0),
  ('human',                'human',          'Human',               1.00, 1.00, 0, 0),
  ('lsp-verified',         'lsp-verified',   'LSP-verified anchor', 0.99, 0.99, 0, 0),
  ('ast-extracted',        'ast-extracted',  'AST-extracted',       0.95, 0.95, 0, 0),
  ('rigor-map',            'rigor-map',      'rigor map',           0.90, 0.90, 0, 0);

CREATE TABLE session_goals (
  session_id    TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
  goal_id       TEXT NOT NULL,
  goal_text     TEXT NOT NULL,
  extracted_by  TEXT NOT NULL REFERENCES sources(source_id),
  extracted_at  INTEGER NOT NULL,
  completed_at  INTEGER,
  is_active     INTEGER NOT NULL DEFAULT 1,
  PRIMARY KEY (session_id, goal_id)
) STRICT;
CREATE INDEX idx_goals_active ON session_goals(session_id, is_active) WHERE is_active = 1;

-- Vector index for goal embeddings. Dimension must match rigor.yaml epistemic.embedder.dimension.
-- EC-6 is the first consumer; EC-4 writes entries but doesn't query them.
CREATE VIRTUAL TABLE goal_embeddings USING vec0(
  goal_pk   TEXT PRIMARY KEY,      -- {session_id}::{goal_id}
  embedding FLOAT[384]
);
```

## Trait surfaces

### `sources.rs`

```rust
#[async_trait]
pub trait SourceRegistry: Send + Sync {
    /// Upsert a source definition.
    async fn register(&self, spec: &SourceSpec) -> Result<()>;

    /// Lookup by id.
    async fn get(&self, id: &str) -> Result<Option<Source>>;

    /// All registered sources.
    async fn list(&self) -> Result<Vec<Source>>;

    /// Current credibility for a source, computed as
    ///     base * (0.5 ** (events_since_last_validation / half_life_events))
    /// with half_life_events from the per-project rigor.yaml config.
    async fn credibility(&self, id: &str) -> Result<f64>;

    /// Increment events_since_last_validation + total_contributions.
    /// Called when a BeliefAsserted event attributes to this source.
    /// Emits SourceCredibilityAdjusted if the new computed weight drifts ≥ 0.01.
    async fn on_assertion(&self, id: &str, event: &BeliefEvent) -> Result<()>;

    /// Reset events_since_last_validation to 0. Bumps accurate_count.
    /// Called when a BeliefVerified event attributes (directly or transitively) to this source.
    async fn on_validation(&self, id: &str, event: &BeliefEvent) -> Result<()>;

    /// Bump contradicted_count. Decays credibility proportionally to severity (configurable).
    async fn on_contradiction(&self, id: &str, event: &BeliefEvent) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct SourceSpec {
    pub source_id: String,
    pub source_kind: String,
    pub display_name: String,
    pub base_credibility: f64,
}

#[derive(Clone, Debug)]
pub struct Source {
    pub source_id: String,
    pub source_kind: String,
    pub display_name: String,
    pub base_credibility: f64,
    pub credibility_weight: f64,
    pub events_since_last_validation: i64,
    pub total_contributions: i64,
    pub accurate_count: i64,
    pub contradicted_count: i64,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
}
```

### `goals.rs`

```rust
#[async_trait]
pub trait GoalTracker: Send + Sync {
    /// Extract a session goal via a one-shot LLM call on the first user message.
    /// Idempotent: if a goal already exists for this session, return it without re-calling.
    async fn extract_from_first_message(&self, session: &SessionId, first_user_message: &str) -> Result<Goal>;

    /// Currently-active goal for a session, if any.
    async fn active_goal(&self, session: &SessionId) -> Result<Option<Goal>>;

    /// Append an additional goal (e.g., mid-session pivot).
    async fn append_goal(&self, session: &SessionId, goal_text: &str, extracted_by: &str) -> Result<Goal>;

    /// Mark the currently-active goal complete.
    async fn complete_active(&self, session: &SessionId) -> Result<()>;

    /// Embedding for the currently-active goal. Loaded lazily; cached per session.
    /// Returns None if no active goal OR if embeddings aren't yet available (EC-6 not landed).
    async fn active_goal_embedding(&self, session: &SessionId) -> Result<Option<Vec<f32>>>;
}

pub struct Goal {
    pub session_id: SessionId,
    pub goal_id: String,
    pub goal_text: String,
    pub extracted_by: String,
    pub extracted_at: i64,
    pub completed_at: Option<i64>,
    pub is_active: bool,
}
```

### `config/epistemic.rs`

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct EpistemicConfig {
    pub embedder: EmbedderConfig,
    pub decay: DecayConfig,
    pub retrieval: RetrievalConfig,
    pub verification: VerificationConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct DecayConfig {
    pub working_memory_half_life_turns: u32,    // default 10
    pub staleness_commit_threshold: u32,        // default 20
    pub credibility_half_life_events: u32,      // default 50
    pub contradiction_decay_factor: f64,        // default 0.2 (multiplies credibility on contradiction)
}

// Other nested structs stubbed here; fully populated by their owning issues.

/// Load from rigor.yaml. Called during daemon startup.
pub fn load_from_rigor_yaml(path: &Path) -> Result<EpistemicConfig>;
```

## Event types wired

- `SourceCredibilityAdjusted { source_id, prior, new, reason }` — emitted from `on_assertion` / `on_validation` / `on_contradiction` when credibility drift exceeds the 0.01 epsilon.
- `GoalExtracted { goal_id, goal_text }` — emitted from `extract_from_first_message` and `append_goal`.
- `GoalCompleted { goal_id }` — emitted from `complete_active`.

All three exist in the EC-2 `EventPayload` enum; EC-4 is where they first get populated.

## Implementation notes & invariants

**Invariant 1: credibility weight drift epsilon = 0.01.** `SourceCredibilityAdjusted` is only emitted if `|new - prior| >= 0.01`. Prevents an event-log flood from repeated small decays.

**Invariant 2: seed sources are append-only.** The V4 migration inserts seeds; later migrations may add more seed rows but must not update existing ones (the live `credibility_weight` is mutable via events).

**Invariant 3: `on_assertion` and `on_validation` are atomic.** Both call paths must operate inside the same transaction as the event that triggered them. Otherwise credibility drift can race with belief counting.

**Invariant 4: goal extraction failures fail open.** If the LLM call fails or times out, `extract_from_first_message` returns a placeholder `Goal{ goal_text: first_user_message.first_100_chars(), extracted_by: "fallback" }` so retrieval can still blend something. A warning event is logged.

**Invariant 5: one active goal per session at a time.** `append_goal` auto-completes the previous active goal (emits `GoalCompleted` first, then `GoalExtracted`).

**Invariant 6: goal_embedding_dimension_matches_ddl.** On daemon startup, rigor reads `epistemic.embedder.dimension` from rigor.yaml and compares against `PRAGMA table_info(goal_embeddings)`. Mismatch → refuse to start with a message:

```
error: goal_embeddings table has dimension 384 but rigor.yaml specifies 1536.
       Changing embedder dimension requires re-embedding all goals and beliefs.
       Run `rigor epistemic reembed --force` to perform the migration.
       Refusing to start to avoid silent corruption.
```

**Operational detail: goal extraction prompt.** Stored in `src/memory/epistemic/goals/prompts/goal_extraction.txt`, embedded at compile time via `include_str!`. Format:

```
You are extracting a single, concrete, actionable goal from the user's first message in a coding session.

Return ONLY the goal as a single imperative sentence of at most 20 words. No preamble.

If the message is a question (not a task), return it as a question.
If the message is ambiguous, return the most specific interpretation you can infer.

Message:
---
{FIRST_USER_MESSAGE}
---

Goal:
```

**Operational detail: credibility formula.**

```rust
fn compute_credibility(source: &Source, half_life_events: u32) -> f64 {
    let decay_factor = 0.5f64.powf(source.events_since_last_validation as f64 / half_life_events as f64);
    source.base_credibility * decay_factor
}
```

## Unit testing plan

### `sources.rs` tests

- `test_default_sources_seeded` — fresh DB has all 9 seed rows.
- `test_register_upserts` — calling `register` on an existing id updates display_name and base_credibility.
- `test_get_returns_existing_source` — lookup by id works.
- `test_get_returns_none_on_unknown_id`.
- `test_on_assertion_increments_counter` — `events_since_last_validation` goes from 0 to 1; `total_contributions` goes from 0 to 1.
- `test_on_validation_resets_counter` — after N assertions (counter = N), one validation sets counter back to 0.
- `test_on_contradiction_bumps_contradicted_count`.
- `test_credibility_formula_at_zero_events` — `events_since_last_validation = 0` → credibility == base.
- `test_credibility_formula_at_half_life_events` — `events_since_last_validation = half_life_events` → credibility == 0.5 * base.
- `test_credibility_formula_at_two_half_lives` — counter = 2 * half_life → credibility == 0.25 * base.
- `test_credibility_adjusted_event_emitted_above_epsilon` — after enough assertions to drop credibility by 0.02, event is emitted.
- `test_credibility_adjusted_event_suppressed_below_epsilon` — single assertion (drift 0.001) does NOT emit.
- `test_validation_emits_credibility_adjusted_when_drift_exceeds_epsilon`.
- `test_contradiction_applies_decay_factor` — `on_contradiction` multiplies credibility by `contradiction_decay_factor` (default 0.2).

### `goals.rs` tests

- `test_extract_from_first_message_calls_llm_once` — mocked LLM client; verify call count = 1 for first message, 0 for subsequent.
- `test_extract_is_idempotent` — calling twice with same session returns same Goal.
- `test_extract_persists_goal_row` — `session_goals` table has one row after call.
- `test_extract_persists_goal_embedding_if_embedder_ready` — skipped if EC-6 embedder not yet present; stub test marked `#[ignore]` with feature gate.
- `test_extract_emits_goal_extracted_event`.
- `test_extract_llm_failure_returns_fallback` — mock LLM fails; goal_text is first 100 chars; extracted_by = "fallback".
- `test_append_goal_auto_completes_previous` — first goal active; append new goal; first has completed_at set, is_active=0; new is active.
- `test_complete_active_marks_inactive` — completes; is_active=0; emits GoalCompleted event.
- `test_active_goal_returns_sole_active_row` — multiple historical goals, only one active.
- `test_active_goal_returns_none_for_new_session`.

### `config/epistemic.rs` tests

- `test_load_defaults_on_missing_section` — rigor.yaml without `epistemic:` section loads all defaults.
- `test_load_partial_overrides` — only `epistemic.decay.working_memory_half_life_turns: 20` overrides that single field; others keep defaults.
- `test_load_invalid_values_reject` — negative half_life rejected at parse time.
- `test_dimension_mismatch_daemon_startup_fails` — writes a 1536 dim to yaml with DDL at 384; SqliteGoalTracker::new returns an error naming both values.

## E2E testing plan

`tests/epistemic_sources.rs`:

**`e2e_source_lifecycle`:**
- Fresh DB; register custom source 'test-model'.
- Apply 100 `BeliefAsserted` events attributed to 'test-model'.
- Check credibility: with half_life_events=50 and base=0.80 → credibility ≈ 0.80 * 0.5^2 = 0.20.
- Apply one `BeliefVerified` event attributed; counter resets to 0; credibility returns to 0.80.
- `SourceCredibilityAdjusted` events present for both drift windows.

**`e2e_source_credibility_recovery`:**
- Source sits at credibility 0.20 (100 assertions, 0 validations, base 0.80).
- 5 validations in a row; after each, counter = 0 and credibility = 0.80. No drift event after the first (already at max).

**`e2e_source_contradiction_penalty`:**
- Source base 0.95.
- Apply `on_contradiction`; credibility drops to 0.95 * 0.2 = 0.19.
- Subsequent validations reset counter; credibility climbs back toward 0.95 over successive validations.

`tests/epistemic_goals.rs`:

**`e2e_goal_extraction_real_llm_mocked`:**
- Mocked LLM returns "build SQLite graph storage for rigor's epistemic layer".
- `extract_from_first_message` for session S.
- `session_goals` row exists; `is_active=1`; `extracted_by='claude-opus-4-7'` (default model).
- `GoalExtracted` event in log.
- Second call for same session returns same goal without invoking LLM (verified via mock call counter).

**`e2e_goal_llm_failure_fallback`:**
- Mocked LLM returns error.
- Extract falls back; goal_text = first 100 chars of input; extracted_by = 'fallback'.
- `GoalExtracted` event STILL emitted (with source='fallback').

**`e2e_goal_append_sequence`:**
- Extract initial goal G1.
- Append G2.
- Active goal is G2; G1 is `completed_at` set.
- `GoalCompleted` for G1 emitted, then `GoalExtracted` for G2.

**`e2e_goal_dimension_mismatch_rejected`:**
- Write rigor.yaml with `epistemic.embedder.dimension: 1024`.
- DB has `goal_embeddings FLOAT[384]`.
- SqliteGoalTracker::new returns error naming both values.

## Performance testing plan

`benches/source_credibility.rs`:

**Benchmark 1: on_assertion hot path.**
- `bench_on_assertion` — invoke 10,000 times for a single source.
- **Threshold:** p99 ≤ **0.5ms** per call (UPDATE + conditional event emit).

**Benchmark 2: credibility computation.**
- `bench_compute_credibility_math` — pure function benchmark.
- **Threshold:** ≤ **50ns** per call.

**Benchmark 3: goal extraction latency.**
- `bench_extract_goal_end_to_end` — includes mocked LLM call (mocked with 100ms fixed delay to simulate network).
- **Threshold:** p99 ≤ **2 seconds** end-to-end (mocked delay + DB writes).

**Benchmark 4: goal lookup on hot path.**
- `bench_active_goal_cache_hit` — active_goal called 10,000 times for the same session.
- **Threshold:** p99 ≤ **0.1ms** (assumes in-memory cache after first DB hit).

**Benchmark 5: source registration.**
- `bench_register_source` — register 1000 sources.
- **Threshold:** ≤ **2 seconds** total.

## Acceptance criteria

- [ ] `V4__sources_and_goals.sql` migration applied; `sources` table has 9 seed rows.
- [ ] `goal_embeddings vec0` virtual table created at the configured dimension.
- [ ] `SourceRegistry` trait + `SqliteSourceRegistry` impl land.
- [ ] `GoalTracker` trait + `SqliteGoalTracker` impl land.
- [ ] `EpistemicConfig` loads from rigor.yaml with defaults.
- [ ] Dimension mismatch on startup refuses to start with actionable message.
- [ ] Credibility formula: `base * 0.5^(events/half_life)`; unit tests exercise 0, 1x, 2x half-lives.
- [ ] `SourceCredibilityAdjusted` events suppressed below 0.01 drift.
- [ ] Goal extraction is one-shot per session (idempotent on duplicate call).
- [ ] Goal extraction failure falls back to first 100 chars with source='fallback'.
- [ ] `append_goal` auto-completes previous active goal.
- [ ] All 24 unit tests pass.
- [ ] All 7 e2e tests pass.
- [ ] All 5 perf benchmarks meet thresholds.
- [ ] `cargo clippy -- -D warnings` clean.

## Additional items surfaced in review

- **No-recursion for goal extraction LLM call (X-2).** The goal-extraction HTTP request MUST carry `X-Rigor-Internal: goal-extraction`. Add `test_goal_extraction_sets_rigor_internal_header`. Also add an integration test that fires goal extraction while a cortex is active and verifies the proxy does NOT claim-extract the goal-extraction traffic.
- **Default goal extraction model = Opus.** Per project memory `feedback_subagent_model.md`: every rigor-dispatched LLM call uses Opus. Hardcode `claude-opus-4-7` as the default model for goal extraction; allow override via `epistemic.goals.extractor_model` config key.
- **LLM call failure modes.** Add `test_goal_extraction_handles_timeout` (30s timeout → fallback), `test_goal_extraction_handles_quota_error` (429 response → fallback), `test_goal_extraction_handles_malformed_response` (no identifiable goal text → fallback). Each emits a diagnostic event with `source_id='fallback'`.
- **Source credibility drift floor.** A source shouldn't decay below a floor (e.g., 0.05) — prevents credibility reaching zero and creating division-by-zero or strange weighting. Add `epistemic.decay.credibility_floor: 0.05` config; clamp `compute_credibility` output at this floor. Test: `test_credibility_never_below_floor`.
- **Credibility recovery on validation.** Currently on_validation resets `events_since_last_validation` to 0, returning credibility to base. But a historically contradicted source shouldn't fully recover from one validation. Add: `credibility_weight = max(floor, base * decay) - penalty_per_contradiction * contradicted_count`. Document as a follow-up decision; default impl uses the simple formula; note for Phase 4B refinement.
- **Seed sources use fresh-timestamp on first-seen, not epoch.** Current DDL `first_seen_at: 0, last_seen_at: 0` — update on first actual use via `on_assertion`. Add test that confirms seed sources have `first_seen_at > 0` after first real use.
- **Observability (X-1).** `cortex.goal.extract` span with `session_id`, `llm_ms`, `model`, `fell_back: bool`. `cortex.source.credibility_changed` span on emit.
- **Dimension migration tooling.** If user changes `epistemic.embedder.dimension` in rigor.yaml, daemon refuses to start (per design). Add companion `rigor epistemic reembed --force` CLI that (a) blows away all embeddings, (b) alters the vec0 table dimension, (c) re-embeds all beliefs. Out-of-scope code-wise for EC-4 but mentioned here because the `goal_embeddings` dimension is set by EC-4's migration.

## Dependencies

**Blocks:** EC-5, EC-6, EC-7, EC-9, EC-10.
**Blocked by:** EC-1, EC-2.
**Parallelizable with:** EC-3.

## References

- Umbrella: [UMBRELLA] Epistemic Cortex
- EC-1, EC-2
- `src/claim/types.rs` — existing `KnowledgeType` enum
- `src/constraint/types.rs` — existing `credibility_weight` field on Constraint (now backed by sources table)
- Project memory: `project_epistemology_expansion.md`
