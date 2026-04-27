# EC-6: `Embedder` trait + BGE-small default + sqlite-vec wiring + `RetrievalEngine` with confidence-gated modes

> Part of umbrella: #34 [UMBRELLA] Epistemic Cortex
> Depends on: **EC-1**, **EC-2**, **EC-4**, **EC-5**
> Lands in: `crates/rigor/src/memory/epistemic/embed.rs`, `retrieval.rs`

## Scope

The largest single slice in the umbrella. After this lands:

- Every belief written via EC-2 also has an associated embedding stored in `belief_embeddings` (sqlite-vec `vec0` table).
- The `Embedder` trait is pluggable per-project via rigor.yaml. `BgeSmallEmbedder` is the default local implementation using `candle-transformers` (384-dim).
- `RetrievalEngine` provides semantic top-k over beliefs, blended with the session goal embedding from EC-4.
- Four confidence-gated retrieval modes (High / Medium / Low / Empty) shape elaboration depth downstream.
- Inhibited beliefs (from EC-7) are filtered at retrieval time — never surfaced.
- Every retrieval emits a `retrieval_events` row — the attention log.

After this lands the full retrieval stack is functional but still not wired into the proxy (that's EC-10). Standalone tests demonstrate the complete flow.

## Design constraints pinned from the design thread

- **BGE-small is the default.** Local, 384-dim, CPU inference via `candle-transformers` + `tokenizers`. Zero network cost, 20–50ms per embed on modern CPU.
- **Per-project embedder choice.** `rigor.yaml` `epistemic.embedder.kind` selects `bge-small` | `openai` | `voyage` | `custom`. Dimension locked to DDL; changing embedder requires re-embed.
- **Dimension = 384 by default.** DDL specifies `FLOAT[384]`. Alternative embedders set their own dimension via config; daemon refuses to start on mismatch.
- **`sqlite-vec` is the vector store.** Single-extension, single-file, same DB as everything else. Loaded at connection-open time via `sqlite3_auto_extension` or per-connection `sqlite_vec::load`.
- **Goal-conditioned retrieval.** `blended = (1 - goal_weight) * query_embedding + goal_weight * goal_embedding`. Default `goal_weight = 0.3`.
- **Confidence-gated retrieval modes** with explicit thresholds:
    - **High** if top_score ≥ 0.9 → surface top 1–2 with FULL elaboration (~600 tokens)
    - **Medium** if any of top 3 ≥ 0.7 → surface top 3 with MEDIUM elaboration (~1500 tokens)
    - **Low** if any of top 5 ≥ 0.5 (= `confidence_floor`) → surface top 5 with MINIMAL elaboration (~2000 tokens)
    - **Empty** if best score < confidence_floor → inject "novel territory" note; response extraction escalates
- **Overfetch + filter pipeline.** Fetch `k * 3` from vec0; filter by kind/knowledge_type/min_confidence; filter inhibited; dedup against working memory; take top k; enforce token budget.
- **Inhibition-aware.** Retrieval always filters inhibited beliefs. Implementation detail from EC-7 but the contract is fixed here.
- **Empty retrieval fails open.** Cortex still injects the static preamble (constraint catalogue) — just with an "unrelated territory" banner. No preloading of "just in case" content.
- **Every retrieval logs.** `retrieval_events` captures the full signature: query, embedding, goal, retrieved ids, used ids, inhibited ids, mode, token budget, actual tokens used.

## What lands

```
crates/rigor/src/memory/epistemic/
  ├── embed.rs                                  (Embedder trait + BgeSmallEmbedder + OpenAI stub + config loader)
  └── retrieval.rs                              (RetrievalEngine + RetrievalMode + RetrievalQuery/Result)

crates/rigor/src/memory/epistemic/store/migrations/
  └── V6__belief_embeddings_and_retrieval.sql

crates/rigor/Cargo.toml                         (wire candle feature-gated deps)

tests/
  ├── epistemic_embedder.rs
  └── epistemic_retrieval.rs

benches/
  ├── embedder_latency.rs
  └── retrieval_latency.rs

models/                                         (BGE-small ONNX/safetensors + tokenizer.json)
  ├── bge-small-en-v1.5/
  │   ├── model.safetensors
  │   └── tokenizer.json
  └── README.md
```

Note on `models/` directory: the BGE model weights are ~130MB. Not committed to git (add to `.gitignore`). Downloaded automatically on first embedder use via `hf-hub` (or a rigor-specific download helper). This is the "local but-first-run-fetches" pattern.

## Schema contributions

**`V6__belief_embeddings_and_retrieval.sql`:**

```sql
-- Vector index for belief embeddings. Dimension MUST match rigor.yaml epistemic.embedder.dimension.
CREATE VIRTUAL TABLE belief_embeddings USING vec0(
  belief_id TEXT PRIMARY KEY,
  embedding FLOAT[384]
);

-- Attention log. Grows unbounded; retention policy deferred to a later issue.
CREATE TABLE retrieval_events (
  retrieval_id         BLOB PRIMARY KEY,
  session_id           TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
  timestamp            INTEGER NOT NULL,
  turn_at_time         INTEGER NOT NULL,
  query_kind           TEXT NOT NULL,                 -- 'pre_request'|'post_response'|'constraint_match'|'anchor_lookup'
  query_text           TEXT,
  query_embedding_hash BLOB,                          -- SHA-256 of query embedding for dedup
  goal_id              TEXT,
  goal_weight_applied  REAL NOT NULL,                 -- the blend factor actually used
  retrieved_ids_json   TEXT NOT NULL,                 -- JSON array of (belief_id, score)
  used_ids_json        TEXT NOT NULL,                 -- JSON array after filter + inhibition + dedup
  inhibited_ids_json   TEXT NOT NULL,
  mode                 TEXT NOT NULL,                 -- 'high'|'medium'|'low'|'empty'
  token_budget         INTEGER,
  tokens_used          INTEGER
) STRICT;
CREATE INDEX idx_ret_session  ON retrieval_events(session_id, timestamp);
CREATE INDEX idx_ret_empty    ON retrieval_events(session_id, timestamp) WHERE used_ids_json = '[]';
CREATE INDEX idx_ret_mode     ON retrieval_events(mode, timestamp);
```

## Trait surfaces

### `embed.rs`

```rust
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Embed a single text; returns a vector of the configured dimension.
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Embed a batch; may be more efficient for large corpora.
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;

    fn dimension(&self) -> usize;

    fn kind(&self) -> &str;
}

pub struct BgeSmallEmbedder {
    model: candle_transformers::models::bert::BertModel,
    tokenizer: tokenizers::Tokenizer,
    device: candle_core::Device,
}

impl BgeSmallEmbedder {
    /// Load the BGE-small model + tokenizer. If the model isn't present locally,
    /// downloads from HuggingFace hub into `<rigor_data>/models/bge-small-en-v1.5/`.
    pub async fn load() -> Result<Self>;
}

pub struct OpenAiEmbedder {
    api_key: String,
    model: String,     // "text-embedding-3-small" etc.
    client: reqwest::Client,
}

/// Factory — reads rigor.yaml epistemic.embedder section and returns the configured impl.
pub async fn load_from_config(cfg: &EmbedderConfig) -> Result<Arc<dyn Embedder>>;
```

### `retrieval.rs`

```rust
#[async_trait]
pub trait RetrievalEngine: Send + Sync {
    async fn retrieve(&self, session: &SessionId, query: RetrievalQuery) -> Result<RetrievalResult>;
}

pub struct SqliteRetrievalEngine {
    store: Arc<dyn EpistemicStore>,
    wm: Arc<dyn WorkingMemory>,
    goals: Arc<dyn GoalTracker>,
    inhibitions: Arc<dyn InhibitionLedger>,    // land together with EC-7; impl for now is a no-op stub
    embedder: Arc<dyn Embedder>,
    config: RetrievalConfig,
}

pub struct RetrievalQuery {
    pub text: String,
    pub kind: QueryKind,
    pub k: usize,                           // default 5
    pub min_confidence: f64,                // default 0.5; filter below
    pub belief_kinds: Vec<BeliefKind>,      // empty = all kinds
    pub knowledge_types: Vec<KnowledgeType>,// empty = all
    pub token_budget: Option<usize>,
    pub goal_weight: f64,                   // default config.goal_weight (0.3)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryKind { PreRequest, PostResponse, ConstraintMatch, AnchorLookup }

pub struct RetrievalResult {
    pub mode: RetrievalMode,
    pub retrieved: Vec<ScoredBelief>,       // everything from vector search (post-filter pre-inhibition)
    pub used: Vec<ScoredBelief>,            // after inhibition + dedup + budget
    pub inhibited: Vec<InhibitionMiss>,
    pub empty: bool,
    pub retrieval_id: EventId,              // retrieval_events row id
    pub goal_weight_applied: f64,
    pub token_budget: usize,
    pub tokens_used: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode { High, Medium, Low, Empty }

pub struct ScoredBelief {
    pub belief: BeliefState,
    pub score: f64,
    pub scored_by_goal_blend: bool,         // true if goal embedding contributed
}

pub struct InhibitionMiss {
    pub belief_id: BeliefId,
    pub score: f64,
    pub inhibition_reason: InhibitionReason,
}
```

### Retrieval algorithm (authoritative)

```
retrieve(session, query):
    # 0. Compute query embedding (cached for identical text in recent requests)
    query_vec = embedder.embed(query.text)

    # 1. Blend with active goal
    goal_vec = goals.active_goal_embedding(session)
    if goal_vec != None AND query.goal_weight > 0:
        blended = (1 - query.goal_weight) * query_vec + query.goal_weight * goal_vec
        goal_weight_applied = query.goal_weight
    else:
        blended = query_vec
        goal_weight_applied = 0.0

    # 2. Vector search: overfetch
    overfetch_k = query.k * 3
    scored_raw = store.nearest(blended, overfetch_k, BeliefFilter {
        kinds: query.belief_kinds,
        knowledge_types: query.knowledge_types,
        min_strength: query.min_confidence,
    })

    # 3. Inhibition filter
    inhibited = []
    filtered = []
    for (belief, score) in scored_raw:
        if inhibitions.is_inhibited(belief.id, now)?:
            inhibited.push(InhibitionMiss{ belief_id, score, reason })
        else:
            filtered.push((belief, score))

    # 4. Dedup against currently-active working memory
    active_in_wm = wm.top_active(session, n=20, min_activation=0.3).map(belief_id).to_set()
    filtered = [f for f in filtered if f.belief.id not in active_in_wm]

    # 5. Take top k
    used = filtered[:query.k]

    # 6. Token budget
    tokens_used = sum(elaboration_token_cost(mode, belief) for belief in used)
    budget = query.token_budget or config.max_dynamic_tokens
    while tokens_used > budget and used:
        used.pop()   # drop lowest-scored
        tokens_used = recompute

    # 7. Mode selection
    top_scores = [s.score for s in used]
    if used and top_scores[0] >= 0.9:
        mode = High
    elif len(top_scores) >= 3 and top_scores[:3].max >= 0.7:
        mode = Medium
    elif top_scores and top_scores[:5].max >= config.confidence_floor:
        mode = Low
    else:
        mode = Empty
        used = []  # empty mode surfaces nothing; escalates scrutiny instead

    # 8. Write retrieval_events
    retrieval_id = insert(retrieval_events ...)

    return RetrievalResult {
        mode, retrieved=filtered, used, inhibited,
        empty = used.is_empty(), retrieval_id, goal_weight_applied,
        token_budget=budget, tokens_used
    }
```

## Implementation notes & invariants

**Invariant 1: every belief write pairs with an embed write.** When EC-2's `append_event` processes a `BeliefAsserted`, it (after EC-6 lands) also calls `embedder.embed(payload.text)` and inserts into `belief_embeddings`. If embedding fails, the event still commits (embedding is lazy — next retrieval triggers a late embed). Don't block the hot write path on embedder latency.

**Invariant 2: embedding dimension must match DDL.** Checked at daemon startup (EC-4 already does this for goals; extended here for beliefs).

**Invariant 3: no embedder call inside transactions.** Embedder calls can take 50ms+ (local) or 500ms+ (remote). Calling them inside a SQL transaction would hold the writer lock way too long. Async boundary: compute embedding outside the tx, insert inside a brief tx.

**Invariant 4: OpenAiEmbedder must use rigor's own proxy.** Rigor eating its own dog food: all LLM calls go through `127.0.0.1:8787`. This includes the embedder when kind=openai. But beware: calling `embed` during a request is recursive — need a feature flag to skip embedder-triggered traffic from being re-evaluated by rigor itself.

**Invariant 5: retrieval is read-only on `belief_state_current` + `belief_embeddings`.** Only `retrieval_events` is written. No mutation of the belief state during retrieval.

**Invariant 6: query_embedding_hash dedup.** If the same canonical query text is submitted in the same session within a short window (configurable; default 10 turns), reuse the previous embedding. Saves 50ms per duplicate.

**Invariant 7: Empty mode's used[] is empty.** Even if there are low-scored retrieved items, Empty mode surfaces nothing in used[]. The intent is "novel territory" → escalate extraction; dumping weak retrievals into context would pollute.

**Operational detail: embedder cache.** A process-local LRU cache of `(canonical_text) → Vec<f32>`, capped at 10k entries. Survives across requests within a daemon run. Lost on restart (which is fine — recomputable from text).

**Operational detail: model download.** First BGE-small load triggers HuggingFace download to `<rigor_data_dir>/models/`. Retries on network failure; fails loud if not available.

**Operational detail: WAL mode + vec0.** sqlite-vec supports WAL. No special handling needed.

## Unit testing plan

### `embed.rs` tests

- `test_bge_small_dimension_is_384`.
- `test_bge_small_deterministic` — same input text → same vector across 10 invocations.
- `test_bge_small_different_inputs_different_outputs` — trivial sanity.
- `test_bge_small_cosine_similarity_semantic` — "Rust has no garbage collector" vs. "Rust uses ownership instead of GC" → similarity > 0.7. Compared to "The weather is nice" → similarity < 0.4.
- `test_bge_small_batch_matches_singles` — `embed_batch([a, b, c])` produces the same vectors as three separate `embed()` calls.
- `test_bge_small_handles_empty_string`.
- `test_bge_small_handles_long_input` — 10,000-char input doesn't crash; truncated to model's context window.
- `test_openai_embedder_uses_rigor_proxy` — mocked HTTP; verify request goes to `127.0.0.1:8787`.
- `test_load_from_config_bge_small`.
- `test_load_from_config_openai`.
- `test_load_from_config_invalid_kind_errors`.
- `test_embedder_cache_hit_avoids_recompute`.

### `retrieval.rs` tests

- `test_query_embedding_computed_once_per_retrieve`.
- `test_goal_blending_applied_when_active_goal`.
- `test_goal_blending_skipped_when_no_goal`.
- `test_goal_blending_skipped_when_weight_zero`.
- `test_overfetch_k_is_3x`.
- `test_filter_by_belief_kind`.
- `test_filter_by_knowledge_type`.
- `test_filter_by_min_confidence`.
- `test_inhibited_beliefs_surface_in_inhibited_list_not_used`.
- `test_wm_dedup_excludes_already_active`.
- `test_token_budget_enforced_by_dropping_lowest_scored`.
- `test_mode_high_at_top_score_0_9`.
- `test_mode_high_at_top_score_0_91` — just above threshold.
- `test_mode_medium_at_top_score_0_89_second_0_7` — boundary case.
- `test_mode_low_at_top_score_0_6`.
- `test_mode_empty_at_best_below_0_5` — confirms `used` is empty even if there are sub-floor retrieved items.
- `test_retrieval_event_persisted` — SELECT from retrieval_events after retrieve.
- `test_retrieval_event_records_correct_mode`.
- `test_retrieval_event_captures_inhibited_ids`.
- `test_empty_retrieval_event_has_used_empty_array`.

## E2E testing plan

`tests/epistemic_retrieval.rs`:

**`e2e_full_retrieval_stack_high_mode`:**
- Populate DB with 100 beliefs; embed each.
- Set session goal; embed goal.
- Query text semantically close to 1–2 beliefs.
- Retrieve; assert mode = High; used contains 1–2 beliefs; scores ≥ 0.9.
- Assert retrieval_events row with mode='high'.

**`e2e_full_retrieval_stack_empty_mode`:**
- Populate DB with 100 beliefs in domain A.
- Set session goal in domain A.
- Query text from domain B (unrelated).
- Retrieve; assert mode = Empty; used = []; inhibited = []; retrieved may have sub-floor entries.
- Retrieval_events row with mode='empty' and used_ids_json = '[]'.

**`e2e_inhibited_beliefs_filtered`:**
- Populate DB with 100 beliefs; inhibit 10 (via EC-7 stub).
- Retrieve; assert the 10 inhibited beliefs never appear in `used`; they may appear in `inhibited`.

**`e2e_working_memory_dedup`:**
- Populate DB with belief B.
- Activate B in session S's working memory at activation 0.8.
- Retrieve a query semantically close to B.
- Assert B is in `retrieved` but NOT in `used`.

**`e2e_goal_blending_alters_ranking`:**
- Populate DB with belief G (matches goal) and belief Q (matches query).
- Set session goal G.
- Query text matches Q but not G.
- With `goal_weight=0` → Q ranks higher.
- With `goal_weight=0.5` → G ranks higher.

**`e2e_dimension_mismatch_refuses_start`:**
- DDL has FLOAT[384]; rigor.yaml has dimension=1024.
- Daemon startup fails with clear message.

**`e2e_embedder_swap_requires_reembed`:**
- DDL + seed beliefs with BGE-small.
- Change embedder to OpenAI (different dim).
- Daemon refuses to start; instructions to reembed.
- `rigor epistemic reembed --force` runs; all beliefs re-embedded with new dim; DDL updated via migration.

**`e2e_token_budget_enforced`:**
- Populate 10 high-scoring beliefs; token cost per belief 400.
- Query with budget=1000; retrieve k=5.
- `used` contains 2 beliefs (total 800 tokens); 3 dropped.

**`e2e_mode_transitions_under_threshold_adjustments`:**
- Populate; run retrieval with default confidence_floor=0.5 → mode Low.
- Bump confidence_floor to 0.8 in config; reload.
- Same query → mode Empty.
- No DB state change; pure config-driven.

## Performance testing plan

`benches/embedder_latency.rs`:

**Benchmark 1: BGE-small single embed.**
- `bench_bge_embed_short_text` — 20-word input.
- **Threshold:** p99 ≤ **50ms** on a modern CPU (Apple M-series, x86_64 AVX2).

**Benchmark 2: BGE-small batch throughput.**
- `bench_bge_embed_batch_32` — batch of 32.
- **Threshold:** total ≤ **500ms** (≈ 16ms per item amortized).

**Benchmark 3: cache hit latency.**
- `bench_embedder_cache_hit` — 10,000 repeated same-text embeds.
- **Threshold:** p99 ≤ **1μs** (pure HashMap lookup).

`benches/retrieval_latency.rs`:

**Benchmark 4: retrieve pipeline end-to-end.**
- `bench_retrieve_10k_corpus` — 10,000 beliefs in DB, random query, k=5.
- **Threshold:** p99 ≤ **15ms** including embed + vec search + filters + write retrieval_events. With cache hit on query: p99 ≤ **5ms**.

**Benchmark 5: vec0 nearest query.**
- `bench_vec0_nearest_k20_10k_corpus` — raw sqlite-vec search without downstream filters.
- **Threshold:** p99 ≤ **10ms** for top-20 on 10k corpus.

**Benchmark 6: vec0 nearest at 100k corpus.**
- `bench_vec0_nearest_k20_100k_corpus`.
- **Threshold:** p99 ≤ **30ms**.

**Benchmark 7: goal blending overhead.**
- `bench_retrieve_with_goal_blend` vs. `bench_retrieve_no_goal_blend`.
- **Threshold:** blend overhead ≤ **1ms**.

**Benchmark 8: retrieval_events write throughput.**
- `bench_write_retrieval_event`.
- **Threshold:** p99 ≤ **2ms** per insert.

**Recall quality benchmark (offline, not in CI):**

`benches/recall_quality.rs` (manual, run by developer):
- Fixed eval set of 100 query-to-expected-belief pairs.
- Run retrieval; measure recall@5 (how often the expected belief appears in top 5).
- **Threshold:** recall@5 ≥ **0.85** with BGE-small on English text. Recorded but not enforced in CI.

## Acceptance criteria

- [ ] `V6__belief_embeddings_and_retrieval.sql` applied; tables + indexes present.
- [ ] `belief_embeddings vec0` at configured dimension.
- [ ] `retrieval_events` table with all columns.
- [ ] `Embedder` trait defined with `embed`, `embed_batch`, `dimension`, `kind`.
- [ ] `BgeSmallEmbedder` loads local weights on first use.
- [ ] `OpenAiEmbedder` routes through `127.0.0.1:8787` proxy.
- [ ] `load_from_config` factory respects rigor.yaml kind selection.
- [ ] Embedder cache implemented (LRU, 10k capped).
- [ ] `RetrievalEngine` trait + `SqliteRetrievalEngine` impl.
- [ ] Retrieval algorithm matches the 8-step pipeline above.
- [ ] Mode thresholds match design: High ≥0.9, Medium ≥0.7, Low ≥0.5, Empty <0.5.
- [ ] Empty mode returns `used = []` even when sub-floor items exist.
- [ ] Working-memory dedup filters active beliefs from results.
- [ ] Inhibition filter splits retrieved into used vs. inhibited.
- [ ] Token budget drops lowest-scored first.
- [ ] Every retrieve writes a `retrieval_events` row.
- [ ] Dimension mismatch rejects startup.
- [ ] Embedder swap requires `rigor epistemic reembed --force` and schema migration.
- [ ] All 32 unit tests pass.
- [ ] All 9 e2e tests pass.
- [ ] All 8 perf benchmarks meet thresholds.
- [ ] Recall quality eval recorded (not CI-enforced).
- [ ] `cargo clippy -- -D warnings` clean.

## Additional items surfaced in review

- **No-recursion for OpenAI embedder (X-2).** When kind=openai, the HTTP request to OpenAI/Anthropic MUST carry `X-Rigor-Internal: embedder`. Test: `test_openai_embedder_sets_rigor_internal_header`. Integration: `test_openai_embedder_not_re_evaluated_by_cortex` (cortex active, embed request goes through, proxy short-circuits).
- **Model download retry strategy.** First-boot `BgeSmallEmbedder::load` downloads ~130MB from HuggingFace. Network failures shouldn't be terminal. Spec: 3 retries with exponential backoff (1s, 4s, 16s); on exhaustion, return clear error naming the hub URL and local cache path for manual download. Test: `test_bge_download_retries_on_transient_failure`.
- **Model integrity check.** After download, verify SHA-256 of `model.safetensors` against a committed expected hash. Prevents corruption from interrupted downloads. `test_bge_load_rejects_corrupted_weights`.
- **HNSW recall vs. exact-search tradeoff documentation.** At what corpus size does sqlite-vec's HNSW index kick in vs. brute-force? Document in the issue: up to 10k vectors → brute force is fast enough; 10k–1M → HNSW is the default; 1M+ → recall degradation needs measurement. Record measured recall@k vs. corpus-size table in `benches/baselines/`.
- **Cold-start behavior (empty DB).** Retrieve on fresh DB → empty vector space → mode=Empty for first N requests. Test: `test_retrieve_on_empty_db_returns_empty_mode`. Expected; not a bug; the fallback path in EC-8 handles this gracefully.
- **Embedding cache invalidation on embedder swap.** The in-process LRU cache is by text; when embedder changes, the cached vectors are wrong. On embedder-kind-change detection at startup: clear cache. Test: `test_cache_cleared_on_embedder_kind_change`.
- **Retrieval tokens_used accurate estimation.** The issue says to estimate per-belief tokens; specify the formula: `tokens ≈ chars / 3.7` for belief payload including provenance line. Add `test_tokens_used_estimate_within_20_percent_of_actual` using a sample mode output.
- **Observability (X-1).** `cortex.retrieve` span with full attribute set: `session_id`, `mode`, `k`, `retrieved_count`, `used_count`, `inhibited_count`, `embed_ms`, `vec_search_ms`, `total_ms`, `goal_weight_applied`. `cortex.embed` span per embedder call with `kind`, `text_len`, `dimension`, `ms`.
- **Token budget truncation telemetry.** When tokens would exceed budget, emit a `cortex.retrieve.truncated` counter with how many beliefs were dropped. Helps tune budget defaults.

## Dependencies

**Blocks:** EC-7, EC-8, EC-10, EC-11.
**Blocked by:** EC-1, EC-2, EC-4, EC-5.

## References

- Umbrella: [UMBRELLA] Epistemic Cortex
- EC-1, EC-2, EC-4, EC-5
- `sqlite-vec` docs: https://github.com/asg017/sqlite-vec
- Candle-transformers BGE example
- Project memory: `project_token_economy.md`, `project_epistemology_expansion.md`
