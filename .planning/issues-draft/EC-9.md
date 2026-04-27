# EC-9: `VerificationLoop` — LSP anchor re-grounding + commit-distance staleness + credibility decay

> Part of umbrella: #34 [UMBRELLA] Epistemic Cortex
> Depends on: **EC-1**, **EC-2**, **EC-4**, **EC-7**
> Parallelizable with: **EC-7**, **EC-8**
> Lands in: `crates/rigor/src/memory/epistemic/verification.rs`

## Scope

The self-maintaining loop. After this lands:

- Empirical beliefs get periodically re-grounded by LSP: is the anchor text still at the same location with the same hash?
- Staleness is measured by **commit distance** since last verification, not wall-clock time. A belief verified in commit `abc123` is fresh until N commits have touched its anchor path.
- Testimonial sources' credibility decays by **events without validation** — logical time, not wall-clock.
- Rational beliefs (DF-QuAD-derived) get re-propagated when their subgraph changes.
- `VerificationLoop::run_pass` is the single entry point, invoked every N proxy requests (turn-gated; default 100).
- Emits `BeliefVerified` / `BeliefDrifted` / `BeliefMissing` / `StrengthUpdated` / `SourceCredibilityAdjusted` events.
- `BeliefDrifted` and `BeliefMissing` auto-inhibit via EC-7's `InhibitionLedger`.
- Respects a per-pass budget: `max_lsp_calls_per_pass` (default 50) + `max_wall_time_ms_per_pass` (default 30000).

## Design constraints pinned from the design thread

- **Commit-distance staleness, not wall-clock.** Using `git2` (already a rigor dep), count commits in `last_verified_commit..HEAD` that touched the anchor path. Stale ≥ configurable threshold.
- **Turn-gated pass interval.** `pass_interval_requests: 100` in rigor.yaml — a verification pass runs every 100 proxy requests. No wall-clock timers.
- **LSP for empirical re-grounding**, not tree-sitter or regex. Per `project_epistemology_expansion.md` memory: LSP gives semantic references, cross-file type resolution, import/re-export chains. Uses existing LSP scaffolding in `src/lsp/`.
- **Testimonial credibility decays by `events_since_last_validation`.** Not time. Implemented in EC-4's `SourceRegistry::on_assertion` / `on_validation`.
- **Rational beliefs re-propagated via DF-QuAD.** When affected subgraph changes, re-run the fixed-point iteration for just that subgraph (not full recompute).
- **Fail open on LSP errors.** If LSP can't resolve a belief's anchor (process crash, LSP timeout), treat as "unable to verify this pass" — don't emit Drifted, don't inhibit. Log and retry next pass.
- **Prioritize oldest empirical beliefs.** The "due-for-verification" query sorts by `last_verified_at ASC NULLS FIRST` to catch unverified-ever beliefs first.

## What lands

```
crates/rigor/src/memory/epistemic/
  └── verification.rs                           (VerificationLoop trait + Default impl)

crates/rigor/src/memory/epistemic/store/migrations/
  └── V8__verification_events.sql

tests/
  ├── epistemic_verification.rs
  └── epistemic_commit_distance.rs

benches/
  └── verification_pass.rs
```

## Schema contributions

**`V8__verification_events.sql`:**

```sql
-- Verification audit. Each pass emits one row per belief checked.
CREATE TABLE verification_events (
  event_id             BLOB PRIMARY KEY REFERENCES belief_events(event_id),
  belief_id            TEXT NOT NULL REFERENCES belief_state_current(belief_id) ON DELETE CASCADE,
  method               TEXT NOT NULL,             -- 'lsp_reference'|'lsp_definition'|'grep'|'file_sha256'|'anchor_sha256'|'human'|'dfquad_repropagate'
  outcome              TEXT NOT NULL,             -- 'verified'|'drifted'|'missing'|'ambiguous'|'error'
  anchor_path          TEXT,
  anchor_sha256_prior  BLOB,
  anchor_sha256_now    BLOB,
  file_sha256_prior    BLOB,
  file_sha256_now      BLOB,
  performed_at         INTEGER NOT NULL,
  performed_at_commit  TEXT,
  lsp_server_used      TEXT,
  error_detail         TEXT
) STRICT;
CREATE INDEX idx_ver_belief ON verification_events(belief_id, performed_at);
CREATE INDEX idx_ver_outcome ON verification_events(outcome, performed_at);
```

## Trait surfaces

```rust
#[async_trait]
pub trait VerificationLoop: Send + Sync {
    /// Run a single pass. Budget-capped. Called every N proxy requests.
    async fn run_pass(&self, budget: VerificationBudget) -> Result<PassReport>;

    /// Force-verify a specific belief (e.g., triggered by file watch).
    async fn verify_now(&self, belief: &BeliefId) -> Result<VerificationOutcome>;

    /// Which beliefs are due for re-verification based on freshness policy?
    async fn due_for_verification(&self, limit: usize) -> Result<Vec<BeliefId>>;

    /// Count commits in (from_commit, HEAD] that touched a given anchor path.
    /// Separated as a trait method so it's mockable in tests.
    async fn commit_distance(&self, from_commit: &str, anchor_path: &str) -> Result<usize>;
}

pub struct VerificationBudget {
    pub max_lsp_calls_per_pass: usize,       // default 50
    pub max_wall_time_ms_per_pass: u64,      // default 30_000
    pub prefer_knowledge_types: Vec<KnowledgeType>, // default [Empirical]
}

pub struct PassReport {
    pub beliefs_considered: usize,
    pub beliefs_verified: usize,
    pub beliefs_drifted: usize,
    pub beliefs_missing: usize,
    pub beliefs_errored: usize,
    pub lsp_calls_made: usize,
    pub wall_time_ms: u64,
    pub budget_exhausted: bool,
}

pub struct VerificationOutcome {
    pub belief_id: BeliefId,
    pub method: VerificationMethod,
    pub result: VerificationResult,
    pub performed_at_commit: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationMethod {
    LspReference,
    LspDefinition,
    Grep,
    FileSha256,
    AnchorSha256,
    Human,
    DfQuadRepropagate,
}

pub enum VerificationResult {
    Verified,
    Drifted { prior_anchor_sha256: [u8; 32], new_anchor_sha256: [u8; 32] },
    Missing,
    Ambiguous { reason: String },
    Error { detail: String },
}

pub struct DefaultVerificationLoop {
    store: Arc<dyn EpistemicStore>,
    inhibitions: Arc<dyn InhibitionLedger>,
    sources: Arc<dyn SourceRegistry>,
    lsp: Arc<dyn LspClient>,                 // existing client from src/lsp/
    git_repo: Arc<git2::Repository>,
    config: VerificationConfig,
}

pub struct VerificationConfig {
    pub max_lsp_calls_per_pass: usize,
    pub max_wall_time_ms_per_pass: u64,
    pub pass_interval_requests: u64,         // default 100
    pub staleness_commit_threshold: u32,     // default 20
    pub credibility_half_life_events: u32,   // default 50
    pub prefer_knowledge_types: Vec<KnowledgeType>,
    pub lsp_call_timeout_ms: u64,            // default 10_000 (per-call)
}
```

## Pass execution flow (authoritative)

```
run_pass(budget):
    t0 = monotonic_now()
    calls_made = 0
    report = PassReport::default()

    # 1. Collect candidates — oldest empirical first, then testimonial decay check, then DF-QuAD repropagation
    candidates = due_for_verification(limit = budget.max_lsp_calls_per_pass * 2)

    # 2. For each candidate, respect budget and verify
    for belief in candidates:
        if calls_made >= budget.max_lsp_calls_per_pass:
            report.budget_exhausted = true
            break
        if elapsed_ms(t0) > budget.max_wall_time_ms_per_pass:
            report.budget_exhausted = true
            break

        match belief.knowledge_type:
            Empirical => {
                # 2a. Commit-distance check
                distance = commit_distance(belief.last_verified_commit, belief.anchor_path)
                if distance < config.staleness_commit_threshold {
                    continue  // still fresh; skip re-verification
                }

                # 2b. LSP verification
                outcome = lsp_verify(belief)
                calls_made += 1
                emit event(BeliefVerified | BeliefDrifted | BeliefMissing | error)
                if drifted or missing: inhibitions.inhibit(belief_id, reason, cause=event_id)
                report.count_by_outcome[outcome] += 1
            }

            Rational => {
                # 2c. DF-QuAD re-propagation if affected subgraph has new events since last_verified
                if has_new_events_since(belief, since=belief.last_verified_at):
                    new_strength = repropagate_subgraph(belief)
                    emit event(StrengthUpdated{ prior=belief.current_strength, new=new_strength })
                    report.beliefs_verified += 1
                // no LSP call; no budget tick
            }

            Testimonial => {
                # 2d. Credibility decay check (really on Source, indirect for belief)
                // Handled by SourceRegistry::on_assertion flow; no LSP call; this branch is a no-op.
                continue
            }

            Memory => {
                # 2e. Memory beliefs are testimonial-from-previous-session; same treatment.
                continue
            }

    # 3. Sweep expired time-limited inhibitions (passive cleanup)
    inhibitions.sweep_expired(now())

    report.wall_time_ms = elapsed_ms(t0)
    return report
```

## Implementation notes & invariants

**Invariant 1: LSP calls time-out.** Each LSP call has a hard timeout (`lsp_call_timeout_ms`, default 10s). Timeout → emit `Error` outcome; don't inhibit; try again next pass.

**Invariant 2: commit_distance uses git2 revwalk with path filter.**

```rust
async fn commit_distance(&self, from: &str, anchor_path: &str) -> Result<usize> {
    let repo = self.git_repo.clone();
    tokio::task::spawn_blocking(move || {
        let from_oid = repo.revparse_single(from)?.id();
        let head_oid = repo.head()?.peel_to_commit()?.id();
        let mut revwalk = repo.revwalk()?;
        revwalk.push(head_oid)?;
        revwalk.hide(from_oid)?;
        let mut count = 0;
        for oid_res in revwalk {
            let oid = oid_res?;
            let commit = repo.find_commit(oid)?;
            if commit_touches_path(&repo, &commit, anchor_path)? {
                count += 1;
            }
        }
        Ok(count)
    }).await?
}
```

**Invariant 3: `commit_touches_path` tolerates renames.** Uses `git2::DiffOptions::include_untracked(false)` + follows renames at the file level. If a belief's anchor_path was renamed, treat as touch.

**Invariant 4: LSP verification method selection.** For each belief, try methods in order:
1. `LspReference` if the anchor has a symbol name → `textDocument/references`.
2. `LspDefinition` if the anchor is a definition site.
3. `AnchorSha256` fallback if LSP can't resolve — read file, grep for anchor text, hash the match.

**Invariant 5: `prefer_knowledge_types` filter is a prioritization, not exclusion.** Empirical beliefs go first; if budget remains, testimonial (cheap, no LSP call) follows; DF-QuAD re-propagation fills whatever time is left.

**Invariant 6: Source credibility decay is passive, not loop-driven.** EC-4's `on_assertion` increments `events_since_last_validation` on every event. `on_validation` resets it. The loop doesn't need to sweep credibility — it's always up-to-date.

**Invariant 7: pass is idempotent at a commit.** If the pass runs twice at the same HEAD with no new belief events between, the second pass does almost no work — all beliefs verified at current commit have `commit_distance = 0`.

**Operational detail: `run_pass` is called from the daemon's tick system.** EC-10's integration schedules pass invocations on turn-count modulo `pass_interval_requests`. Details of scheduling live in EC-10.

**Operational detail: `verify_now` enables file-watch-triggered verification.** Future work (not in EC-9) could hook into `rigor`'s file-watch to call `verify_now` immediately when an anchor's file changes. EC-9 provides the entry point.

**Operational detail: LSP client is shared with other rigor features.** `src/lsp/` is already used elsewhere. Rigor already spawns language-server subprocesses (rust-analyzer, typescript-language-server, etc.) on demand; this loop is another consumer of that pool.

## Unit testing plan

### `verification.rs` tests

- `test_due_for_verification_sorts_by_last_verified_asc`.
- `test_due_for_verification_limit_respected`.
- `test_due_prioritizes_empirical`.
- `test_run_pass_respects_max_lsp_calls`.
- `test_run_pass_respects_max_wall_time` — synthetic slow LSP; budget cuts off.
- `test_run_pass_empty_db_no_work`.
- `test_run_pass_budget_exhausted_flag` — exceed max_calls and flag is true.
- `test_run_pass_calls_sweep_expired`.
- `test_verify_now_single_belief`.
- `test_verify_now_on_missing_file_emits_missing`.
- `test_verify_now_on_drifted_hash_emits_drifted`.
- `test_verify_now_on_matching_hash_emits_verified`.
- `test_verify_now_lsp_timeout_emits_error_not_inhibit`.
- `test_drifted_emits_inhibit` — BeliefDrifted event → InhibitionLedger.inhibit called with reason=AnchorStale and cause_event_id.
- `test_missing_emits_inhibit` — BeliefMissing → reason=AnchorMissing.
- `test_error_does_not_emit_inhibit`.
- `test_verified_event_resets_confidence_grade_to_fresh`.

### Commit distance tests (`tests/epistemic_commit_distance.rs`)

- `test_commit_distance_zero_when_no_intervening_commits`.
- `test_commit_distance_counts_commits_touching_path`.
- `test_commit_distance_ignores_unrelated_commits`.
- `test_commit_distance_follows_renames` — file renamed in an intervening commit; still counted.
- `test_commit_distance_in_detached_head`.
- `test_commit_distance_handles_missing_from_commit` — from_commit doesn't exist; return Err.

## E2E testing plan

`tests/epistemic_verification.rs`:

**`e2e_verification_flow_verified`:**
- Create belief B1 with anchor_path = src/lib.rs, last_verified_commit = HEAD.
- Run pass. Assert: B1's commit_distance = 0 → skipped; NO event emitted.
- Advance DB: 25 commits to src/lib.rs (past threshold 20).
- Run pass. LSP resolves anchor; hash matches. BeliefVerified event emitted; last_verified_commit updated to new HEAD; confidence_grade='fresh'.

**`e2e_verification_flow_drifted`:**
- Belief B1 with anchor_path = src/lib.rs and anchor_sha256 = H1.
- 25 commits pass; file content changed; new hash = H2.
- Run pass. LSP resolves; hash differs. BeliefDrifted event emitted with prior=H1, new=H2. Inhibition with reason=AnchorStale inserted; cause_event_id = drift event.
- Retrieval now filters B1.

**`e2e_verification_flow_missing`:**
- Belief B1 with anchor_path = src/deleted.rs.
- Delete file; commit.
- Run pass. LSP can't resolve; anchor missing. BeliefMissing event + inhibition with reason=AnchorMissing.

**`e2e_verification_flow_lsp_timeout`:**
- Mock LSP with 20s delay (timeout 10s).
- Run pass. Error outcome; NO inhibition; belief stays in due_for_verification for next pass.

**`e2e_verification_budget_respected`:**
- 200 stale beliefs.
- Run pass with budget.max_lsp_calls=50.
- Exactly 50 LSP calls made; budget_exhausted flag true; 150 beliefs remain due for next pass.

**`e2e_verification_rational_repropagation`:**
- DF-QuAD graph with 5 rational beliefs.
- Add a new event that changes an attacker's strength.
- Run pass. StrengthUpdated event emitted for affected rational beliefs; current_strength column updated.

**`e2e_verification_testimonial_no_lsp`:**
- Testimonial belief B1 from source 'claude-haiku' with decaying credibility.
- Run pass. No LSP call; no event (decay is passive via SourceRegistry).

**`e2e_verification_sweep_expired_called`:**
- Insert a time-limited inhibition with until = now() - 1.
- Run pass. Inhibition is lifted (UnInhibited event); confidence_grade restored.

**`e2e_verification_idempotent_at_same_head`:**
- Run pass; record all events emitted.
- Run pass again with no belief changes.
- Second pass emits zero new events (all freshly-verified beliefs have commit_distance=0).

## Performance testing plan

`benches/verification_pass.rs`:

**Benchmark 1: single belief verification.**
- `bench_verify_single_belief_via_lsp` — mocked LSP with fixed 50ms response.
- **Threshold:** end-to-end p99 ≤ **100ms** per belief (includes LSP call + DB writes).

**Benchmark 2: commit_distance scaling.**
- `bench_commit_distance_100_commits` — HEAD has 100 commits since from_commit, 30 touch the path.
- `bench_commit_distance_10000_commits` — HEAD has 10,000 commits since from_commit.
- **Thresholds:** 100-commit ≤ **50ms**; 10,000-commit ≤ **2s**.

**Benchmark 3: full pass with budget.**
- `bench_run_pass_50_beliefs` — 50 candidates, mocked LSP with 50ms response.
- **Threshold:** total ≤ **5s** (dominated by LSP calls, serialized by budget).

**Benchmark 4: due_for_verification query.**
- `bench_due_query_10k_beliefs` — 10,000 beliefs in DB, 1000 stale.
- **Threshold:** p99 ≤ **20ms**. Index hit on `(confidence_grade, last_verified_at)`.

**Benchmark 5: DF-QuAD repropagation.**
- `bench_repropagate_subgraph_100_nodes`.
- **Threshold:** ≤ **100ms** (EPSILON=0.001, MAX_ITERATIONS=100).

**Benchmark 6: sweep_expired at scale.**
- `bench_sweep_expired_10k` — 10,000 inhibitions, 5% expired.
- **Threshold:** ≤ **100ms**.

## Acceptance criteria

- [ ] `V8__verification_events.sql` applied.
- [ ] `VerificationLoop` trait + `DefaultVerificationLoop`.
- [ ] `run_pass` respects `max_lsp_calls_per_pass` and `max_wall_time_ms_per_pass`.
- [ ] `verify_now` entry point for explicit re-verification.
- [ ] `due_for_verification` prioritizes empirical + oldest.
- [ ] `commit_distance` uses git2 revwalk with path + rename tracking.
- [ ] LSP timeout handled; Error outcome, no inhibit.
- [ ] Drifted/Missing outcomes auto-inhibit via EC-7.
- [ ] Verified outcome resets `confidence_grade='fresh'`.
- [ ] Rational beliefs re-propagated via DF-QuAD.
- [ ] Testimonial beliefs skipped (credibility decay is passive).
- [ ] `sweep_expired` called as part of pass.
- [ ] All 23 unit tests pass.
- [ ] All 9 e2e tests pass.
- [ ] All 6 perf benchmarks meet thresholds.
- [ ] `cargo clippy -- -D warnings` clean.

## Additional items surfaced in review

- **Behavior when `last_verified_commit` is unreachable.** git2 revparse may fail if the commit was amended, squashed, or garbage-collected. Spec: treat as `commit_distance = infinity` → belief is stale → trigger re-verification (which may then succeed against the current anchor). Test: `test_commit_distance_handles_missing_from_commit_triggers_reverification`.
- **DF-QuAD repropagation trigger conditions — explicit.** A rational belief's subgraph is "dirty" when any of its attackers/supporters (transitive closure up to depth N, default 3) has a newer `StrengthUpdated` / `BeliefVerified` / `BeliefDrifted` event since the belief's `last_verified_at`. Document; test `test_repropagation_triggered_by_transitive_upstream_change`, `test_repropagation_not_triggered_by_unrelated_subgraph_change`.
- **LSP timeout is per-call, not per-pass.** Each individual LSP request has `lsp_call_timeout_ms` (default 10_000). The overall pass has `max_wall_time_ms_per_pass` (default 30_000). Both enforced; budget stops after first to trigger. Test: `test_lsp_timeout_does_not_exceed_call_ceiling_even_if_pass_has_budget`.
- **LSP server startup cost first pass.** First verification pass after daemon start may spend 5-15 seconds spawning language-server subprocesses. Budget should account; default `max_wall_time_ms_per_pass` of 30s gives headroom. Document.
- **Commit-distance caching.** Computing revwalk on every belief is expensive. Cache `commit_distance(from_commit, anchor_path)` for the duration of a pass (hashmap keyed by `(from_commit, path)`). Cache dropped at pass end. Test: `test_commit_distance_cached_within_pass`.
- **DF-QuAD iteration bounded by EPSILON=0.001 + MAX_ITERATIONS=100.** Same constants as existing `src/constraint/graph.rs`. Re-use, don't duplicate.
- **Observability (X-1).** `cortex.verify.pass` span with full `PassReport` as attributes. `cortex.verify.belief` sub-span per belief verified with `method`, `outcome`, `ms`. `cortex.repropagate` span for DF-QuAD re-propagation with `subgraph_size`, `iterations`, `converged`, `ms`.
- **No-recursion note.** VerificationLoop only makes LSP calls (not LLM). No X-2 concern directly, but if a future tier-3-judge-style verification is added, it will need the X-2 header.

## Dependencies

**Blocks:** EC-10.
**Blocked by:** EC-1, EC-2, EC-4, EC-7.
**Parallelizable with:** EC-7, EC-8 (different trait surfaces).

## References

- Umbrella: [UMBRELLA] Epistemic Cortex
- EC-1, EC-2, EC-4, EC-7
- `src/lsp/` — existing LSP client
- `src/constraint/graph.rs` — existing DF-QuAD implementation
- `git2` crate docs
- Project memory: `project_epistemology_expansion.md` (LSP-over-tree-sitter decision)
