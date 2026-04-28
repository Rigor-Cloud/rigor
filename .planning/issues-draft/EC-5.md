# EC-5: `WorkingMemory` — session-scoped activation with turn-based decay

> Part of umbrella: #34 [UMBRELLA] Epistemic Cortex
> Depends on: **EC-1**, **EC-2**, **EC-3**
> Lands in: `crates/rigor/src/memory/epistemic/working_memory.rs`

## Scope

Session-scoped active belief set with logical-time activation decay. After this lands:

- Every session has a working memory projection tracking which beliefs are currently "on the model's desk" with activation scores.
- Activation decays **per-turn within the session**, not per wall-clock second. Long pauses don't decay anything; intensive coding bursts decay quickly.
- Beliefs are activated on assertion, touched on reference, decayed on retrieval, and evicted below threshold.
- `WorkingMemoryActivated` and `WorkingMemoryTouched` events feed the projection.
- The projection is session-scoped but stored in the shared DB; cross-session leakage is prevented by the compound `(session_id, belief_id)` PK.

This is the cognitive kernel analog: the PFC's working-memory trace. It's what the context assembler (EC-8) will surface as "Active working memory" in the injected prompt.

## Design constraints pinned from the design thread

- **Logical time, not wall-clock.** The core rule from the design thread: *"session time for a project is very different, can't use a wall clock."* The clock is `sessions.turn_count` (from EC-3). Wall-clock timestamps are kept for audit ordering only, never for decay.
- **Decay formula:** `activation = initial_activation * (0.5 ** (elapsed_turns / half_life_turns))`, where `elapsed_turns = sessions.turn_count - wm.last_touched_at_turn`.
- **Default half-life: 10 turns.** Configurable per-project via `epistemic.decay.working_memory_half_life_turns`.
- **Eviction threshold: 0.1.** Below this, a belief falls out of working memory. Configurable but generally unchanged.
- **Only proxy-request turns tick the clock.** Verification-loop passes, background decay sweeps, hook callbacks — none of these increment `turn_count`. Only agent-initiated requests.
- **Per-session isolation.** A belief can have high activation in session A and be absent from session B, simultaneously. Achieved via compound PK.
- **Decay is computed on read, not on a timer.** No background "decay sweep thread." When `top_active` is called, activation is computed live from `last_touched_at_turn` vs. current `turn_count`. Eviction is a write path triggered by explicit `evict_below` calls (typically at the start of every `top_active` invocation).

## What lands

```
crates/rigor/src/memory/epistemic/
  └── working_memory.rs                         (WorkingMemory trait + SqliteWorkingMemory + InMemory)

crates/rigor/src/memory/epistemic/store/migrations/
  └── V5__working_memory.sql

tests/
  └── epistemic_working_memory.rs

benches/
  └── working_memory.rs
```

## Schema contributions

**`V5__working_memory.sql`:**

```sql
CREATE TABLE working_memory (
  session_id              TEXT NOT NULL,
  belief_id               TEXT NOT NULL,
  initial_activation      REAL NOT NULL,                  -- activation at the time of last touch
  first_activated_at_turn INTEGER NOT NULL,               -- session.turn_count when first activated
  last_touched_at_turn    INTEGER NOT NULL,               -- session.turn_count when last touched
  touch_count             INTEGER NOT NULL DEFAULT 1,
  role                    TEXT NOT NULL,                  -- 'under_evaluation'|'supporting'|'contradicting'|'goal_relevant'|'surfaced'
  last_event_id           BLOB NOT NULL,
  PRIMARY KEY (session_id, belief_id),
  FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE,
  FOREIGN KEY (belief_id)  REFERENCES belief_state_current(belief_id) ON DELETE CASCADE,
  FOREIGN KEY (last_event_id) REFERENCES belief_events(event_id)
) STRICT;

-- Index supports top-N-active queries. Activation is computed live, but approximation via
-- (initial_activation, last_touched_at_turn) lets the index narrow candidates cheaply.
CREATE INDEX idx_wm_session_recent ON working_memory(session_id, last_touched_at_turn DESC);
CREATE INDEX idx_wm_session_initial ON working_memory(session_id, initial_activation DESC);
```

## Trait surfaces

```rust
// crates/rigor/src/memory/epistemic/working_memory.rs

#[async_trait]
pub trait WorkingMemory: Send + Sync {
    /// Activate a belief into the session's working memory at initial_activation.
    /// If already present, updates role and resets activation to the new initial value.
    /// Emits WorkingMemoryActivated.
    async fn activate(
        &self,
        session: &SessionId,
        belief: &BeliefId,
        role: ActivationRole,
        initial_activation: f64,
    ) -> Result<()>;

    /// Boost activation for an already-active belief.
    /// Adds `delta` to the computed-live activation, capped at 1.0.
    /// Resets last_touched_at_turn to the current session turn.
    /// Emits WorkingMemoryTouched.
    /// Returns Err if belief isn't active in this session.
    async fn touch(
        &self,
        session: &SessionId,
        belief: &BeliefId,
        delta: f64,
    ) -> Result<()>;

    /// Top-N active beliefs, sorted by computed-live activation score descending.
    /// Filters below min_activation threshold.
    /// Does NOT mutate state (read-only).
    async fn top_active(
        &self,
        session: &SessionId,
        n: usize,
        min_activation: f64,
    ) -> Result<Vec<ActiveBelief>>;

    /// Live activation for a specific belief in a session. None if not active.
    async fn activation(&self, session: &SessionId, belief: &BeliefId) -> Result<Option<f64>>;

    /// Evict beliefs whose computed-live activation falls below threshold.
    /// Returns number evicted.
    /// Typically called at the start of top_active, but also standalone for cleanup.
    async fn evict_below(&self, session: &SessionId, threshold: f64) -> Result<usize>;

    /// Per-session diagnostic: size of working memory, mean activation, oldest touch.
    async fn stats(&self, session: &SessionId) -> Result<WorkingMemoryStats>;
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationRole {
    UnderEvaluation,        // Just surfaced by extract_and_evaluate_text
    Supporting,             // Supports a currently-held belief
    Contradicting,          // Contradicts a currently-held belief
    GoalRelevant,           // Surfaced by goal-conditioned retrieval
    Surfaced,               // Injected into context
}

pub struct ActiveBelief {
    pub belief_id: BeliefId,
    pub activation: f64,       // computed live
    pub initial_activation: f64,
    pub first_activated_at_turn: i64,
    pub last_touched_at_turn: i64,
    pub touch_count: i64,
    pub role: ActivationRole,
}

pub struct WorkingMemoryStats {
    pub active_count: usize,
    pub mean_activation: f64,
    pub oldest_last_touched_at_turn: Option<i64>,
    pub current_turn: i64,
}
```

## Event types wired

- `WorkingMemoryActivated { role, initial_activation, activated_at_turn }` — populates `working_memory` via the projection.
- `WorkingMemoryTouched { activation_delta, touched_at_turn }` — bumps `last_touched_at_turn` and `touch_count`.

## Decay math (authoritative)

```rust
fn live_activation(
    initial_activation: f64,
    last_touched_at_turn: i64,
    current_turn: i64,
    half_life_turns: u32,
) -> f64 {
    if current_turn <= last_touched_at_turn {
        return initial_activation;
    }
    let elapsed = (current_turn - last_touched_at_turn) as f64;
    let decay = 0.5f64.powf(elapsed / half_life_turns as f64);
    initial_activation * decay
}
```

Properties:
- At `elapsed = 0` → activation = initial_activation.
- At `elapsed = half_life` → activation = 0.5 * initial.
- At `elapsed = 2 * half_life` → activation = 0.25 * initial.
- At `elapsed = 4 * half_life` → activation = 0.0625 * initial (below 0.1 threshold for most initial values; evicted).

## Implementation notes & invariants

**Invariant 1: turn_count reads are snapshot.** When `top_active` is called, it SELECTs `sessions.turn_count` once at the start. All subsequent decay computations in the same call use that snapshot. Prevents races where concurrent requests bump turn_count mid-query.

**Invariant 2: activation never exceeds 1.0.** `touch`'s `activation + delta` is clamped at 1.0.

**Invariant 3: activation is purely a read-side computation.** The DB stores `initial_activation` and `last_touched_at_turn`. Live activation is computed on every read. This lets half-life changes (config tweak) immediately affect behavior without rewriting any rows.

**Invariant 4: eviction is monotonic.** Once a belief is evicted from working memory (row deleted), it's out until re-activated. Cannot silently "undelete" due to clock changes.

**Invariant 5: `touch` on non-active belief is an error.** The caller must `activate` first. Returning Ok silently would mask missing state.

**Invariant 6: `role` is updated on re-activation.** If a belief goes from `UnderEvaluation` to `GoalRelevant`, calling `activate` again updates the role in-place. This is the only mutation path for `role` (no separate update method).

**Operational detail: evict_below is called inline.** `top_active` starts with `evict_below(session, threshold=0.01)` to prune truly dead entries before the SELECT. The SELECT then filters by computed activation above `min_activation`. Two-layer filter: cheap eviction at the bottom, precise filter at the top.

**Operational detail: no background decay worker.** Decay is lazy. A session that produces one request per week will have its working memory evicted on the next `top_active` call, naturally. No threads, no timers, no cron.

## Unit testing plan

`working_memory.rs` tests + `tests/epistemic_working_memory.rs` for cross-impl property tests.

### Pure math tests (no DB)

- `test_decay_at_zero_elapsed_no_decay` — `live_activation(0.8, 10, 10, 10) == 0.8`.
- `test_decay_at_half_life` — `live_activation(0.8, 10, 20, 10) == 0.4`.
- `test_decay_at_two_half_lives` — `live_activation(0.8, 10, 30, 10) == 0.2`.
- `test_decay_at_four_half_lives_below_eviction` — `live_activation(0.8, 10, 50, 10) == 0.05 < 0.1`.
- `test_decay_never_negative` — pathological large elapsed → activation is in (0, initial], never negative.
- `test_decay_current_turn_equals_last_touched` — elapsed = 0, returns initial.

### Trait contract tests (run against both SqliteWorkingMemory and InMemory)

- `contract_activate_inserts_row`.
- `contract_activate_emits_event`.
- `contract_activate_on_existing_replaces_role`.
- `contract_activate_resets_activation_on_repeat`.
- `contract_touch_bumps_last_touched`.
- `contract_touch_increments_touch_count`.
- `contract_touch_clamps_at_1_0`.
- `contract_touch_on_inactive_errors`.
- `contract_top_active_sorts_by_activation_desc`.
- `contract_top_active_filters_below_min_activation`.
- `contract_top_active_snapshots_turn_count` — increment turn while top_active runs; result uses the snapshot.
- `contract_top_active_is_session_scoped` — belief in session A with high activation doesn't appear in session B's top_active.
- `contract_evict_below_removes_stale` — 100 beliefs activated, many turns pass, evict_below(0.1) removes ~half.
- `contract_evict_below_returns_count`.
- `contract_activation_none_for_unactivated`.
- `contract_stats_accurate`.
- `contract_delete_session_cascades_working_memory` — ON DELETE CASCADE from `sessions`.

## E2E testing plan

`tests/epistemic_working_memory.rs`:

**`e2e_activation_lifecycle_across_turns`:**
- Create session S with turn_count=0.
- `activate(S, B1, Role::Surfaced, 0.8)` at turn=1.
- Advance turn to 11 (one half-life).
- `activation(S, B1)` returns ≈ 0.4.
- Advance to turn 21 (two half-lives).
- `activation(S, B1)` returns ≈ 0.2.
- Advance to turn 41 → activation 0.05 → below threshold.
- `top_active(S, 10, 0.1)` omits B1.
- Verify `evict_below(S, 0.1)` now returns 1 (B1).

**`e2e_touch_resets_decay`:**
- Activate B1 at turn 1 with 0.8.
- Advance to turn 10 (just before half-life).
- `touch(S, B1, +0.1)` → new initial_activation = clamped(0.8 * 0.5^0.9 + 0.1, 1.0) ≈ 0.53; last_touched=10.
- Advance to turn 20.
- `activation` returns ≈ 0.26 (half-life from the new touch).

**`e2e_per_session_isolation`:**
- Activate B1 in S1 at turn 1, activation 0.9.
- Activate B1 in S2 at turn 1, activation 0.3.
- `top_active(S1, 10, 0.1)` → B1 at 0.9.
- `top_active(S2, 10, 0.1)` → B1 at 0.3.
- Delete S1 (cascade). `activation(S2, B1)` still returns ≈ 0.3.

**`e2e_background_events_do_not_bump_turn`:**
- Session at turn 5.
- Run verification loop pass (EC-9 stub for this test); many events emitted.
- `sessions.turn_count` still 5.
- Decay computations unaffected.

**`e2e_config_change_takes_effect_live`:**
- Activate B1 with default half_life_turns=10.
- Verify decay at turn 11 gives activation 0.4.
- Update rigor.yaml to half_life_turns=20; reload config.
- Verify decay at turn 11 gives activation 0.8 * 0.5^0.5 ≈ 0.57. Row unchanged; math changed.

**`e2e_evict_below_is_monotonic`:**
- Activate and let decay to below threshold.
- Evict removes it.
- A subsequent retrieval that would have "surfaced" the same belief must go through `activate` again — not a ghost re-appearance.

## Performance testing plan

`benches/working_memory.rs`:

**Benchmark 1: activate throughput.**
- `bench_activate_fresh` — 10,000 activations for distinct beliefs in one session.
- **Threshold:** p99 ≤ **1ms** per call.

**Benchmark 2: touch throughput.**
- `bench_touch_hot` — 10,000 touches of the same belief.
- **Threshold:** p99 ≤ **0.5ms**.

**Benchmark 3: top_active with decay computation.**
- `bench_top_active_1k_beliefs` — session with 1000 active beliefs, various ages.
- **Threshold:** p99 ≤ **2ms**.

**Benchmark 4: decay-only math benchmark.**
- `bench_live_activation_math` — pure function.
- **Threshold:** ≤ **50ns** per invocation.

**Benchmark 5: evict_below at scale.**
- `bench_evict_below_10k` — session with 10,000 beliefs, half below threshold.
- **Threshold:** p99 ≤ **10ms**. Eviction is batched DELETE.

**Benchmark 6: concurrent session top_active.**
- `bench_top_active_concurrent_8_sessions` — 8 tokio tasks, each querying own session (1000 beliefs), in parallel.
- **Threshold:** per-task p99 ≤ **5ms** under contention.

## Acceptance criteria

- [ ] `V5__working_memory.sql` applied; table + indexes present.
- [ ] `WorkingMemory` trait defined with 6 methods.
- [ ] `SqliteWorkingMemory` and `InMemoryWorkingMemory` both implement the trait and pass the same contract suite.
- [ ] Decay formula matches `0.5 ** (elapsed_turns / half_life_turns)`.
- [ ] Half-life configurable via `epistemic.decay.working_memory_half_life_turns`.
- [ ] Activation capped at 1.0 on touch.
- [ ] `touch` on non-active belief returns error.
- [ ] `top_active` snapshot-reads turn_count.
- [ ] `evict_below` is monotonic and returns eviction count.
- [ ] `WorkingMemoryActivated` and `WorkingMemoryTouched` events land in the log.
- [ ] Projection updates atomically with event insert.
- [ ] Per-session isolation verified by compound PK.
- [ ] All 23 unit + contract tests pass.
- [ ] All 5 e2e tests pass.
- [ ] All 6 perf benchmarks meet thresholds.
- [ ] `cargo clippy -- -D warnings` clean.

## Additional items surfaced in review

- **Expose eviction threshold via config.** Currently `0.1` is hardcoded in the Decay section. Add `epistemic.decay.working_memory_eviction_threshold: 0.1` to `DecayConfig` so projects can tune. Test: `test_eviction_threshold_config_honored`.
- **`stats()` coverage.** Add `test_stats_active_count_accurate`, `test_stats_mean_activation`, `test_stats_oldest_touch` — the `stats()` method is documented but the unit test list didn't enumerate them explicitly. Fill in.
- **Per-session initial activation by role.** Currently callers pass `initial_activation` explicitly. Add default-by-role helpers: `UnderEvaluation=0.7`, `Supporting=0.8`, `Contradicting=0.9`, `GoalRelevant=0.6`, `Surfaced=0.8`. Expose as `ActivationRole::default_initial() -> f64`. Cortex callers use these defaults unless overriding.
- **Cross-session activation isolation test — explicit.** Already named `contract_top_active_is_session_scoped` but extend to `test_activation_in_session_a_does_not_leak_via_touch_in_session_b` — touching a belief in session B should not affect session A's activation score for that belief.
- **Cascading delete correctness.** `test_session_delete_cascades_working_memory` is in plan. Also add `test_belief_delete_cascades_working_memory` — if a belief is deleted from `belief_state_current` (retention policy, future feature), its WM rows across all sessions are removed.
- **Observability (X-1).** `cortex.wm.activate` and `cortex.wm.touch` spans with `session_id`, `belief_id`, `role`, `new_activation`. `cortex.wm.decay_applied` counter on each `top_active` call reporting how many beliefs dropped below threshold.
- **No-recursion note.** Working memory never makes LLM calls; no X-2 concern. Noted for completeness.

## Dependencies

**Blocks:** EC-6, EC-7, EC-8, EC-10.
**Blocked by:** EC-1, EC-2, EC-3.

## References

- Umbrella: [UMBRELLA] Epistemic Cortex
- EC-1, EC-2, EC-3
- Project memory: `project_epistemology_expansion.md`
