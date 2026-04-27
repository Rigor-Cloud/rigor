# EC-7: `InhibitionLedger` + contradiction detection

> Part of umbrella: #34 [UMBRELLA] Epistemic Cortex
> Depends on: **EC-1**, **EC-2**, **EC-4**, **EC-5**, **EC-6**
> Lands in: `crates/rigor/src/memory/epistemic/inhibition.rs`

## Scope

Active suppression layer — the "don't surface this" gate. After this lands:

- `InhibitionLedger` trait persists inhibition history with reasons and causal event links.
- Retrieval (EC-6) queries the ledger at every call to filter inhibited beliefs out of `used`.
- Contradiction detection runs on every `record_response` path in the cortex: for each new claim, check against currently-active working memory for conflicts. On conflict, emit `Contradicted` + `Inhibited`.
- Three tiers of contradiction detection, used in order:
    1. **Constraint co-violation (primary)** — deterministic: two claims both fire the same constraint with opposite verdicts → contradiction.
    2. **Embedding polarity (secondary)** — high embedding similarity AND opposite polarity signals in the text.
    3. **LLM-as-judge (tertiary, opt-in)** — only for high-stakes constraints tagged `contradiction_judge=true` in rigor.yaml.
- `BeliefDrifted` / `BeliefMissing` from EC-9's verification loop automatically inhibit their target beliefs.
- Inhibitions are REVERSIBLE — subsequent re-verification emits `UnInhibited` and clears the suppression.
- Nothing is deleted — inhibitions are first-class audit entries with reasons and causal event FKs.

## Design constraints pinned from the design thread

- **Contradicted / stale / low-credibility beliefs get suppressed, not deleted.** Every inhibition is a first-class, audited entry.
- **Three-tier contradiction detection**, in order:
    1. Constraint co-violation first (deterministic, cheap, covers the most important case)
    2. Embedding-polarity as secondary
    3. LLM-judge as tertiary for high-stakes constraints only
- **Auto-inhibit on Drifted / Missing / Contradicted.** EC-9's verification loop and EC-7's contradiction detection both feed the same inhibition path.
- **Default inhibitions are indefinite.** Lifted only by re-verification events. `inhibited_until` column supports optional time-limited inhibitions but the default is NULL (indefinite).
- **Retrieval filters inhibitions at query time.** Always. Stale justifications never leak into prompts. This is the contract EC-6 relies on.

## What lands

```
crates/rigor/src/memory/epistemic/
  └── inhibition.rs                             (InhibitionLedger trait + SqliteInhibitionLedger + contradiction detectors)

crates/rigor/src/memory/epistemic/store/migrations/
  └── V7__inhibitions.sql

tests/
  ├── epistemic_inhibitions.rs
  └── epistemic_contradiction.rs

benches/
  └── inhibition_lookup.rs
```

## Schema contributions

**`V7__inhibitions.sql`:**

```sql
CREATE TABLE inhibitions (
  belief_id        TEXT NOT NULL,
  inhibited_at     INTEGER NOT NULL,
  inhibited_until  INTEGER,                       -- NULL = indefinite
  reason           TEXT NOT NULL,                 -- 'anchor_stale'|'contradicted_by'|'credibility_decay'|'manual'|'gettier_guard'|'anchor_missing'|'source_contradicted'
  cause_event_id   BLOB,                          -- the event that triggered the inhibition
  lifted_at        INTEGER,                       -- NULL until lifted; UpdATE only by UnInhibited events
  lifted_event_id  BLOB,
  PRIMARY KEY (belief_id, inhibited_at),
  FOREIGN KEY (belief_id)       REFERENCES belief_state_current(belief_id) ON DELETE CASCADE,
  FOREIGN KEY (cause_event_id)  REFERENCES belief_events(event_id),
  FOREIGN KEY (lifted_event_id) REFERENCES belief_events(event_id)
) STRICT;

-- Partial index for active (unlifted) inhibitions
CREATE INDEX idx_inhibitions_active ON inhibitions(belief_id) WHERE lifted_at IS NULL;
-- For time-based expiry of time-limited inhibitions (rare path, but indexed for GC)
CREATE INDEX idx_inhibitions_expiry ON inhibitions(inhibited_until) WHERE inhibited_until IS NOT NULL AND lifted_at IS NULL;
```

## Trait surfaces

### `inhibition.rs`

```rust
#[async_trait]
pub trait InhibitionLedger: Send + Sync {
    /// Add an inhibition entry. Idempotent if an active inhibition with the same reason already exists.
    /// Emits `Inhibited` event.
    async fn inhibit(
        &self,
        belief: &BeliefId,
        reason: InhibitionReason,
        until: Option<i64>,
        cause: Option<EventId>,
    ) -> Result<()>;

    /// Lift all active inhibitions for a belief matching the given reason (or all reasons if None).
    /// Emits `UnInhibited` event.
    async fn lift(
        &self,
        belief: &BeliefId,
        reason_filter: Option<InhibitionReason>,
    ) -> Result<usize>;

    /// Whether a belief is currently inhibited. Used by retrieval (EC-6) on every call.
    async fn is_inhibited(&self, belief: &BeliefId, at_timestamp: i64) -> Result<Option<ActiveInhibition>>;

    /// Batch lookup for retrieval's inhibition-filter step.
    async fn inhibited_among(&self, beliefs: &[BeliefId], at_timestamp: i64) -> Result<HashSet<BeliefId>>;

    /// Full history of inhibitions for a belief.
    async fn history(&self, belief: &BeliefId) -> Result<Vec<InhibitionRecord>>;

    /// Active inhibition count.
    async fn active_count(&self) -> Result<usize>;

    /// Sweep time-limited inhibitions whose `inhibited_until` has passed.
    /// Returns number lifted. Called periodically by the verification loop (EC-9).
    async fn sweep_expired(&self, at_timestamp: i64) -> Result<usize>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InhibitionReason {
    AnchorStale,
    AnchorMissing,
    ContradictedBy,
    CredibilityDecay,
    GettierGuard,
    Manual,
    SourceContradicted,
}

pub struct ActiveInhibition {
    pub belief_id: BeliefId,
    pub inhibited_at: i64,
    pub inhibited_until: Option<i64>,
    pub reason: InhibitionReason,
    pub cause_event_id: Option<EventId>,
}

pub struct InhibitionRecord {
    pub inhibited_at: i64,
    pub inhibited_until: Option<i64>,
    pub reason: InhibitionReason,
    pub cause_event_id: Option<EventId>,
    pub lifted_at: Option<i64>,
    pub lifted_event_id: Option<EventId>,
}

#[async_trait]
pub trait ContradictionDetector: Send + Sync {
    /// Detect contradictions between a new claim and active working-memory beliefs.
    /// Returns ordered list of (active_belief_id, reason) pairs.
    async fn detect(
        &self,
        session: &SessionId,
        new_claim: &Claim,
        new_verdict: &Verdict,
    ) -> Result<Vec<ContradictionFinding>>;
}

pub struct ContradictionFinding {
    pub against_belief_id: BeliefId,
    pub method: ContradictionMethod,      // 'constraint_co_violation' | 'embedding_polarity' | 'llm_judge'
    pub evidence_json: String,
    pub confidence: f64,                  // how strongly we think this is a contradiction
}

pub struct TieredContradictionDetector {
    store: Arc<dyn EpistemicStore>,
    wm: Arc<dyn WorkingMemory>,
    embedder: Arc<dyn Embedder>,
    judge_llm: Option<Arc<dyn JudgeLlm>>,   // None disables tier 3
    config: ContradictionConfig,
}

pub struct ContradictionConfig {
    pub enable_tier_1_constraint_coviolation: bool,    // default true
    pub enable_tier_2_embedding_polarity: bool,        // default true
    pub tier_2_similarity_threshold: f64,              // default 0.75
    pub enable_tier_3_llm_judge: bool,                 // default false
    pub tier_3_only_for_constraint_tags: Vec<String>,  // default ['high-stakes']
}
```

## Tier-by-tier detection semantics

### Tier 1: constraint co-violation (primary)

```
For each active belief B in session working memory:
    If both `new_claim` and `B.claim` would fire the same constraint C, but with opposite verdicts
    (one says "this constraint is violated", the other says "this constraint holds") → contradiction.

Mechanically:
    - Run new_claim through the constraint evaluator; get set of (C_id, verdict) tuples.
    - For each active belief B (which has stored verdicts in its payload), compare:
      shared constraints with opposite verdicts → ContradictionFinding with method=constraint_co_violation.
```

Cheap, deterministic, doesn't require embedding calls.

### Tier 2: embedding polarity (secondary)

```
For each active belief B with embedding(B) similarity to new_claim > tier_2_similarity_threshold (0.75):
    Check for polarity markers:
    - new_claim contains negation term ("no", "not", "never", "doesn't") that B doesn't, or vice versa
    - new_claim explicitly negates a concept B asserts (e.g., "Rust has GC" vs. "Rust has no GC")
Detected polarity + high similarity → ContradictionFinding with method=embedding_polarity.
```

Cost: one embedding call + O(active_wm_size) cosine comparisons. Done in parallel with tier 1.

### Tier 3: LLM-as-judge (tertiary, opt-in)

```
Only for constraints tagged `high-stakes` (configurable).
For each (new_claim, B) pair that tier 1 and tier 2 didn't catch but that both touch a high-stakes constraint:
    Call judge LLM: "Claim A: '...'. Claim B: '...'. Do these contradict? Yes/No + brief reason."
    If yes → ContradictionFinding with method=llm_judge and confidence from judge's explicit statement.
```

Cost: one LLM call per pair. Budget-capped at `tier_3_max_calls_per_turn` (default 3).

## Implementation notes & invariants

**Invariant 1: `is_inhibited` is sub-millisecond.** Retrieval hits this on every candidate belief. The `idx_inhibitions_active` partial index makes it an index-only scan.

**Invariant 2: `inhibit` is idempotent per (belief, reason).** If an active inhibition exists with the same reason, `inhibit` is a no-op (returns Ok without emitting a new event). Prevents event spam on repeated-drift detections.

**Invariant 3: `lift` with `reason_filter=None` lifts ALL active inhibitions.** Useful for manual override (`rigor inhibit lift belief_id`).

**Invariant 4: auto-inhibit on `BeliefDrifted` / `BeliefMissing` is wired by EC-9.** EC-7 only provides the contract; the actual hook-up lives in the verification loop.

**Invariant 5: `sweep_expired` is idempotent.** Running it multiple times at the same timestamp lifts only un-lifted entries.

**Invariant 6: contradiction detection runs inline in `record_response`.** Not a background job — we want the next turn's retrieval to see the new inhibition.

**Invariant 7: LLM-judge failures fall open.** If the judge call fails or times out, no contradiction is emitted. We don't block on an unreliable tool.

**Invariant 8: `confidence_grade` update.** When a belief is inhibited, `belief_state_current.confidence_grade` is updated to `'inhibited'`. When all active inhibitions are lifted, confidence_grade is re-derived from the belief's other state (fresh/stale/unverified).

**Operational detail: constraint co-violation implementation.** Uses the existing `PolicyEngine` from `src/policy/engine.rs`. Claim is evaluated against the full constraint set; verdicts recorded. Same-constraint-opposite-verdict is a straightforward set comparison.

**Operational detail: polarity markers.** Initial set hard-coded:
- Negation prefixes: "no", "not", "never", "doesn't", "does not", "isn't", "is not", "cannot", "can't", "won't", "will not".
- Domain-specific antonym pairs (small static table): (is, is not), (has, has no), (uses, does not use), (supports, does not support), (Rust has GC, Rust has no GC) — extensible via a future `contradiction_polarity_pairs.yaml`.

## Unit testing plan

### `inhibition.rs` tests

- `test_inhibit_inserts_row`.
- `test_inhibit_emits_event`.
- `test_inhibit_idempotent_per_reason` — second call with same (belief, reason) doesn't re-insert and doesn't emit.
- `test_inhibit_different_reasons_both_active` — AnchorStale and ContradictedBy can coexist.
- `test_lift_with_filter_only_lifts_matching_reasons`.
- `test_lift_without_filter_lifts_all`.
- `test_lift_emits_un_inhibited_event`.
- `test_lift_updates_lifted_at_and_event_id`.
- `test_is_inhibited_true_for_active`.
- `test_is_inhibited_false_after_lift`.
- `test_is_inhibited_respects_inhibited_until` — time-limited inhibition: at_timestamp > inhibited_until → returns None.
- `test_is_inhibited_active_at_exact_expiry` — edge case: at_timestamp == inhibited_until → NOT inhibited (treat as exclusive upper bound).
- `test_inhibited_among_batch_lookup_correct`.
- `test_history_returns_all_entries_including_lifted`.
- `test_sweep_expired_lifts_time_limited`.
- `test_sweep_expired_leaves_indefinite_untouched`.
- `test_inhibit_updates_confidence_grade_to_inhibited`.
- `test_lift_restores_confidence_grade_when_no_active_remain`.

### Contradiction detector tests

- `test_tier_1_constraint_co_violation_detected` — two claims fire same constraint with opposite verdicts.
- `test_tier_1_no_violation_no_detection`.
- `test_tier_2_embedding_polarity_detected` — "Rust has GC" vs. "Rust has no GC" with BGE-small similarity > 0.75.
- `test_tier_2_below_similarity_threshold_not_detected`.
- `test_tier_2_high_similarity_no_polarity_not_detected` — "Rust is fast" vs. "Rust is memory-safe" (high sim, no polarity).
- `test_tier_3_only_runs_for_tagged_constraints`.
- `test_tier_3_llm_failure_fails_open` — mocked judge returns error; no finding emitted.
- `test_tier_3_llm_success_emits_finding`.
- `test_tiers_run_in_order_and_stop_early` — tier 1 detects → tier 2 and 3 don't run.
- `test_tier_2_and_3_disabled_only_tier_1_runs`.
- `test_contradiction_finding_includes_evidence`.

## E2E testing plan

`tests/epistemic_inhibitions.rs`:

**`e2e_inhibition_filters_retrieval`:**
- Store belief B1 with high-score embedding match for a test query.
- Inhibit B1 with reason=AnchorStale.
- Call retrieval for the matching query.
- Assert B1 appears in `inhibited` list; NOT in `used` list.

**`e2e_uninhibit_restores_visibility`:**
- Inhibit B1.
- Retrieve → B1 suppressed.
- Lift inhibition.
- Retrieve again → B1 appears in `used`.

**`e2e_belief_drifted_auto_inhibits`:**
- Assert BeliefDrifted event for belief B1.
- Assert inhibitions row exists with reason=AnchorStale, cause_event_id = drift event.
- confidence_grade = 'inhibited'.

**`e2e_time_limited_inhibition_auto_lifts`:**
- Inhibit with inhibited_until = now() + 60000 (60s in future).
- is_inhibited at now() → true.
- Advance clock to now + 61000. Call sweep_expired.
- is_inhibited now → false. confidence_grade restored.

`tests/epistemic_contradiction.rs`:

**`e2e_tier_1_end_to_end`:**
- Session S. Record response with claim "Rust uses GC" fires constraint `rust-no-gc` with verdict=violated.
- Record response with claim "Rust has no GC" fires same constraint with verdict=allow.
- Contradiction detected (tier 1).
- Contradicted event emitted; first belief inhibited with reason=ContradictedBy.
- confidence_grade updated.

**`e2e_tier_2_end_to_end`:**
- Two beliefs without shared constraints but high embedding similarity and clear polarity.
- Record both.
- Tier 2 detects.
- Second-inserted belief inhibited (policy: newer contradicts older by default).

**`e2e_tier_3_gated_by_tag`:**
- Constraint tagged `high-stakes`.
- Two beliefs touch this constraint, tier 1 doesn't fire (not co-violation), tier 2 doesn't fire (low embedding similarity).
- Tier 3 LLM-judge called (mocked; returns contradiction).
- Inhibition applied.
- For a NON-high-stakes constraint, tier 3 is not called; no inhibition.

**`e2e_tier_3_budget_cap`:**
- 10 high-stakes pairs in one turn.
- `tier_3_max_calls_per_turn = 3`.
- LLM-judge called at most 3 times; remaining pairs get no finding.

**`e2e_contradiction_persists_across_daemon_restart`:**
- Inhibit B1. Shut down daemon. Restart.
- Retrieve → B1 still suppressed.

## Performance testing plan

`benches/inhibition_lookup.rs`:

**Benchmark 1: is_inhibited hot path.**
- `bench_is_inhibited_positive` — belief is inhibited; return ActiveInhibition.
- `bench_is_inhibited_negative` — belief is not inhibited; return None.
- **Threshold:** p99 ≤ **0.3ms** (partial index hit).

**Benchmark 2: inhibited_among batch lookup.**
- `bench_inhibited_among_100` — batch of 100 belief IDs; mix of inhibited and not.
- **Threshold:** p99 ≤ **2ms**.

**Benchmark 3: inhibit + event emit.**
- `bench_inhibit_fresh` — 10,000 fresh inhibitions.
- **Threshold:** p99 ≤ **1.5ms** per call.

**Benchmark 4: sweep_expired at scale.**
- 10,000 inhibitions, half time-limited with expired until, half indefinite.
- `bench_sweep_expired_10k`.
- **Threshold:** ≤ **100ms** total.

**Benchmark 5: contradiction detection tier 1 throughput.**
- Evaluator + WM lookup overhead, 100 beliefs in WM, one new claim.
- **Threshold:** p99 ≤ **20ms**. Tier 1 uses the existing PolicyEngine which is already optimized.

**Benchmark 6: contradiction detection tier 2.**
- Embedding compute + similarity + polarity check.
- **Threshold:** p99 ≤ **100ms** (dominated by embed cost).

**Benchmark 7: contradiction detection tier 3.**
- Mocked LLM with fixed 500ms response.
- **Threshold:** p99 ≤ **1.5s** including DB writes.

## Acceptance criteria

- [ ] `V7__inhibitions.sql` applied; table + indexes present (partial index for active lookups).
- [ ] `InhibitionLedger` trait + `SqliteInhibitionLedger`.
- [ ] Idempotent inhibit (per belief + reason).
- [ ] Lift with and without reason filter.
- [ ] `is_inhibited` sub-millisecond on partial index.
- [ ] `inhibited_among` batch variant for retrieval.
- [ ] `Inhibited` and `UnInhibited` events flow through EC-2 projection.
- [ ] `confidence_grade` updates on inhibit and uninhibit.
- [ ] `sweep_expired` lifts time-limited inhibitions; idempotent.
- [ ] `ContradictionDetector` trait + `TieredContradictionDetector` impl.
- [ ] Tier 1: constraint co-violation using PolicyEngine.
- [ ] Tier 2: embedding polarity using BGE-small + polarity markers.
- [ ] Tier 3: LLM-as-judge gated by constraint tag.
- [ ] Tier 3 fails open on LLM error.
- [ ] Tier 3 budget-capped per turn.
- [ ] Tiers run in order; stop early on detection.
- [ ] Retrieval (EC-6) filters inhibited beliefs at query time.
- [ ] All 29 unit tests pass.
- [ ] All 9 e2e tests pass.
- [ ] All 7 perf benchmarks meet thresholds.
- [ ] `cargo clippy -- -D warnings` clean.

## Additional items surfaced in review

- **Tiers are short-circuited on first fire.** Authoritative invariant: as soon as one tier produces a `ContradictionFinding` for a `(new_claim, active_belief)` pair, lower tiers are skipped for THAT pair. But tier 2 may still run against other active beliefs in the same `detect()` call. Add `test_tier_1_fires_stops_tier_2_for_same_pair`. And `test_tier_1_fires_for_pair_A_still_runs_tier_2_for_pair_B`.
- **High-stakes tag convention.** The constraint's `Constraint.tags: Vec<String>` already exists. Convention: tag `"high-stakes"` marks constraints eligible for tier 3 LLM-judge. Configurable via `tier_3_only_for_constraint_tags: Vec<String>` (default `["high-stakes"]`). Document in rigor.yaml schema docs.
- **No-recursion for tier-3 judge (X-2).** Tier-3 LLM call MUST carry `X-Rigor-Internal: contradiction-judge`. Test: `test_tier_3_sets_rigor_internal_header`.
- **Tier 2 polarity markers configurable.** Hardcoded starter list (negation prefixes + antonym pairs) should be extensible via `src/memory/epistemic/templates/contradiction_polarity_pairs.yaml` (or similar config file, loadable at daemon start). Out-of-scope to fully implement but create the file structure with starter content.
- **Contradiction inhibits newer belief by default — document explicitly.** When pairs (new_claim, B) contradict, by default `new_claim` is the one inhibited (the assertion that just arrived). Override: if the active belief is `Contradicting`-role in WM, inhibit `B` instead. Document as policy; test both branches.
- **Idempotent inhibit guard under contention.** Concurrent contradiction detection from two tokio tasks for the same pair should not double-emit `Inhibited`. Rely on the EC-7 idempotency invariant + SQL unique constraint on `(belief_id, inhibited_at, reason)` — but emissions can collide if timestamps match. Add a retry loop: on duplicate-key error, re-read inhibition state and return Ok.
- **Observability (X-1).** `cortex.inhibit` span with `belief_id`, `reason`, `until_set`. `cortex.contradiction.detect` span per `detect()` call with `tiers_run`, `findings_count`, `ms`.

## Dependencies

**Blocks:** EC-8, EC-10.
**Blocked by:** EC-1, EC-2, EC-4, EC-5, EC-6.

## References

- Umbrella: [UMBRELLA] Epistemic Cortex
- EC-1, EC-2, EC-4, EC-5, EC-6
- `src/policy/engine.rs` — existing PolicyEngine used by tier 1
- Project memory: `project_epistemology_expansion.md`
