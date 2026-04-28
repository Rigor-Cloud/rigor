# EC-8: `ContextAssembler` — cache-disciplined dynamic prompt injection

> Part of umbrella: #34 [UMBRELLA] Epistemic Cortex
> Depends on: **EC-1**, **EC-2**, **EC-3**, **EC-4**, **EC-5**, **EC-6**, **EC-7**
> Lands in: `crates/rigor/src/memory/epistemic/context.rs`

## Scope

The presentation layer. After this lands:

- `ContextAssembler` takes epistemic state and produces the structured system-prompt injection that replaces `build_epistemic_context` at the proxy cutover (EC-10).
- Injection is split into a **stable preamble** (constraint catalogue + rubric) and a **dynamic body** (session state, working memory, retrieved grounding, inhibitions, metacognitive flags), separated by a `cache_control: ephemeral` marker so Claude's prompt cache hits on the preamble.
- Four output shapes correspond to the four retrieval modes (High / Medium / Low / Empty); elaboration depth scales with confidence.
- Empty-retrieval mode outputs a "novel territory" note and flags the turn for escalated response extraction.
- Typed sections (labeled blocks) let the model parse working memory vs. retrieved vs. inhibited separately.
- Token budgets are enforced per mode; the overall `max_dynamic_tokens` config is a hard ceiling.

After this lands, the cortex is complete end-to-end. EC-10 is a one-line swap at `proxy.rs:1256`; this issue delivers everything downstream of that swap.

## Design constraints pinned from the design thread

- **Stable / dynamic split with a cache boundary.** Constraint catalogue + scoring rubric live in cached block. Session state + working memory + retrieved + inhibited + metacognitive flags live below `cache_control: ephemeral`. This is non-negotiable — without it the dynamic retrieval feature is too expensive to run.
- **Typed sections.** Each region is labeled (`# Active working memory`, `# Retrieved grounding`, `# Inhibited beliefs`, `# Metacognitive flags`, `# Note`) so the model can interpret them correctly.
- **Confidence-gated elaboration depth:**
    - **High** (~600 tokens) — top 1–2 with FULL elaboration: anchor path, verification history, credibility, strength, role.
    - **Medium** (~1500 tokens) — top 3 with MEDIUM elaboration: one-line provenance, no history.
    - **Low** (~2000 tokens) — top 5 with MINIMAL elaboration: id + strength + one-line.
    - **Empty** (~200 tokens) — novel-territory note only.
- **Token budget is per-project configurable.** `epistemic.retrieval.max_dynamic_tokens` (default 1500) caps the full dynamic block regardless of mode. High/Medium/Low budgets are targets within this ceiling.
- **Working memory top-5 always surfaced.** Even in Empty mode, if the session has active WM beliefs, surface them — they're the ongoing context. Separate from retrieval.
- **Inhibitions surfaced explicitly.** If any belief was suppressed during this turn's retrieval, list it under `# Inhibited beliefs` so the model knows NOT to resurrect those claims.
- **Metacognitive flags surface repeat violations.** Constraints violated >N times historically get a pointed callout: "You've previously violated `X` 109 times. Counter-evidence pinned above." N is configurable (default 50).
- **Empty-mode note signals escalation.** The note tells the model "this is novel territory; your response will be extracted for future verification." This is read-facing documentation of rigor's learning loop.

## What lands

```
crates/rigor/src/memory/epistemic/
  └── context.rs                                (ContextAssembler trait + DefaultContextAssembler + mode renderers)

tests/
  └── epistemic_context.rs

benches/
  └── context_assembly.rs

crates/rigor/src/memory/epistemic/templates/
  ├── preamble.md                               (stable section — loaded once at startup)
  ├── mode_high.md                              (handlebars-style template)
  ├── mode_medium.md
  ├── mode_low.md
  └── mode_empty.md
```

Templates are not handlebars literally — they're `include_str!`'d into Rust and filled via `format!` or a small custom templater. No external template engine dependency.

## Schema contributions

**None.** ContextAssembler is read-only against the substrate laid down by EC-2 through EC-7.

## Trait surfaces

```rust
#[async_trait]
pub trait ContextAssembler: Send + Sync {
    /// Produce the structured system-prompt injection for a request.
    async fn assemble(
        &self,
        session: &SessionId,
        request_hint: &AssemblerHint,
    ) -> Result<AssembledContext>;
}

pub struct AssemblerHint {
    pub user_message_text: Option<String>,       // last user message — drives retrieval query
    pub active_tools: Vec<String>,               // e.g. ['Read', 'Edit', 'Bash']
    pub active_files: Vec<String>,               // filenames referenced in tool-use context
    pub query_kind: QueryKind,                   // default PreRequest
}

pub struct AssembledContext {
    /// Stable block — same across turns of the session. Passed to upstream with cache_control.
    pub stable_preamble: String,

    /// Marker for the API's cache_control: ephemeral insertion point.
    /// Consumers of AssembledContext insert the cache_control metadata at this boundary.
    pub cache_boundary: CacheBoundary,

    /// Dynamic block — per-turn.
    pub dynamic_body: String,

    /// Total token count (estimated via tiktoken or a rigor heuristic). Used for logging/benches.
    pub total_tokens_est: usize,
    pub stable_tokens_est: usize,
    pub dynamic_tokens_est: usize,

    /// The retrieval event(s) emitted during assembly (for traceability in retrieval_events).
    pub retrieval_ids: Vec<EventId>,

    /// The mode rendered (High / Medium / Low / Empty).
    pub rendered_mode: RetrievalMode,
}

pub struct CacheBoundary;  // unit marker; real type carries no data, just a position.

pub struct DefaultContextAssembler {
    store: Arc<dyn EpistemicStore>,
    wm: Arc<dyn WorkingMemory>,
    retrieval: Arc<dyn RetrievalEngine>,
    inhibitions: Arc<dyn InhibitionLedger>,
    sources: Arc<dyn SourceRegistry>,
    goals: Arc<dyn GoalTracker>,
    sessions: Arc<dyn SessionResolver>,
    config: AssemblerConfig,
    rigor_config: Arc<RigorConfig>,   // for the stable-preamble constraint catalogue
}

pub struct AssemblerConfig {
    pub max_dynamic_tokens: usize,                 // hard ceiling — default 1500
    pub mode_high_target_tokens: usize,            // default 600
    pub mode_medium_target_tokens: usize,          // default 1500
    pub mode_low_target_tokens: usize,             // default 2000 (capped by max_dynamic_tokens)
    pub mode_empty_target_tokens: usize,           // default 200
    pub wm_top_k: usize,                           // default 5
    pub metacognitive_flag_violation_threshold: u64, // default 50
    pub repeat_violation_lookback_turns: i64,      // default 1000 — how far back to count
}
```

## Output format (authoritative)

The rendered system-prompt structure emitted by assemble:

```
<stable_preamble>
# Epistemic grounding

You are operating inside rigor's constraint-enforcement layer. Every claim you assert is
evaluated against the catalogue below. Violations are blocked, warned, or allowed by the
scoring rubric. Align your responses with the catalogue.

## Constraint catalogue ({N} constraints)

### Beliefs (base strength 0.8)
- {id}: {name} — {one-line description}
- ...

### Justifications (base strength 0.9)
- ...

### Defeaters (base strength 0.7)
- ...

## Scoring rubric
- Block threshold:    ≥ 0.7 (request will be blocked; retry with correction)
- Warn threshold:     ≥ 0.4 (logged; model should self-correct)
- Allow:              < 0.4 (no action)

## How claims are evaluated
- Text from your response is parsed into sentence-level claims.
- Each claim is evaluated against relevant constraints via regex + Rego + semantic judge.
- Claims that assert known-false patterns (see high-risk categories below) are especially watched.

## High-risk constraint categories
- {category}: fired in {N} prior sessions
- ...

---------------{cache_control: ephemeral marker inserted here}---------------
</stable_preamble>

<dynamic_body>
# Session state
Session: {session_id}
Turn: {turn_count}
Goal: "{goal_text}"
Git commit: {git_commit}{dirty_suffix}

# Active working memory ({active_count} beliefs; top {N} shown by activation)
- [activation={activation:.2} role={role} strength={strength:.2}] {belief_summary}
  ↳ source: {source_display_name} · verified {last_verified_at} at {last_verified_commit}
- ...

# Retrieved grounding ({mode}; {N} beliefs; overfetched {overfetch_N}, filtered {filtered_N}, inhibited {inhibited_N})
{mode-specific elaboration — see below}

# Inhibited beliefs the model must respect (not surfaced above but suppressed for a reason)
- [{inhibition_reason}] {belief_summary}
  ↳ reason detail: {cause_event_summary}
- ...

# Metacognitive flags
- You've previously violated `{constraint_id}` {count} times in the last {lookback} turns.
  Counter-evidence pinned in the retrieved section above.
- ...

{optional mode-empty note}
</dynamic_body>
```

### Mode-specific elaboration renderings

**High mode** (top 1–2 with full elaboration):
```
1. [{id}] {summary}
   Text: "{full claim text, up to 200 chars}"
   Strength: {current} (base {base}) · {confidence_grade}
   Source: {source_display_name} credibility {credibility:.2}
   Anchor: {anchor_path}:{anchor_lines} · file_sha256 {...:8}
   Verified: {count} times, last at commit {commit:8}
   Retrieval score: {score:.3}
```

**Medium mode** (top 3 with medium elaboration):
```
1. [{id}] {summary} · strength {current:.2} · {source_kind} · {anchor_path}:{lines} · retrieval {score:.2}
2. ...
3. ...
```

**Low mode** (top 5 with minimal elaboration):
```
1. [{id:16}] {summary_truncated_80}
   strength {current:.2} · retrieval {score:.2}
2. ...
5. ...
```

**Empty mode**:
```
⚠ No past grounding retrieved for this turn's topic.

This appears to be novel territory for the rigor epistemic layer. Your response will be
extracted and evaluated against the full constraint catalogue above. Any claims that pass
evaluation will be added to memory and become retrievable in future turns.

Be especially careful about:
- Asserting facts without grounding in the catalogue above.
- Claiming knowledge the catalogue does not support.
```

## Implementation notes & invariants

**Invariant 1: stable preamble is cacheable across the session.** The constraint catalogue changes only when rigor.yaml changes; rigor.yaml changes reload the daemon. Within a running daemon, the stable preamble is a cached `Arc<String>` — computed once per daemon start (or per rigor.yaml reload), shared across all requests.

**Invariant 2: dynamic body MUST NOT reference the preamble by content copy.** It references by constraint ID only. Otherwise cache hits don't save tokens — the model has to reconcile duplication.

**Invariant 3: total_tokens_est is a soft estimate.** Uses a fast heuristic (tokens ≈ chars / 3.7 for English + code). tiktoken-style precise counting is opt-in via a feature flag for benchmarks only; adding it to the hot path is too expensive.

**Invariant 4: dynamic body token budget is a hard ceiling.** The mode-specific target is a goal; `max_dynamic_tokens` is never exceeded. Truncation happens by dropping lowest-priority sections first, in order: metacognitive flags → inhibitions → retrieved → working memory. The session state header is never truncated.

**Invariant 5: Empty mode still renders working memory and session state.** The "Empty" designation is about retrieval, not the entire dynamic body. WM remains present because it's about what the session has already talked about — orthogonal to retrieval.

**Invariant 6: metacognitive flag queries are cheap.** The violation-count query uses `belief_events` with `event_type='contradicted'` and an index-only scan. Threshold `metacognitive_flag_violation_threshold` (default 50) prevents the flag section from growing unbounded.

**Invariant 7: assemble is pure.** Given the same epistemic state, same goal, same query, assemble returns the same bytes. No wall-clock in the output (timestamps are from events, not from now()).

**Operational detail: rigor.yaml reload.** The stable preamble is invalidated on SIGHUP or CLI `rigor reload`. Implementing reload is out of scope for EC-8; for now the preamble is computed at daemon start.

**Operational detail: `cache_boundary` consumer.** EC-10's proxy integration takes `AssembledContext` and constructs the Anthropic API request with `cache_control: ephemeral` inserted at the boundary position. The assembler itself does not emit raw API-specific markup.

## Unit testing plan

### `context.rs` tests

- `test_stable_preamble_includes_all_constraints` — 100 constraints → all IDs present in preamble.
- `test_stable_preamble_sorted_by_epistemic_type` — beliefs section, then justifications, then defeaters.
- `test_stable_preamble_independent_of_session` — same preamble for sessions S1 and S2.
- `test_dynamic_body_includes_session_header`.
- `test_dynamic_body_includes_turn_count`.
- `test_dynamic_body_includes_goal_text`.
- `test_dynamic_body_active_wm_section` — top-5 rendered; activation values correct.
- `test_dynamic_body_wm_empty_omits_section` — no active WM → no section, not an empty header.
- `test_dynamic_body_retrieved_section_high_mode` — 1 belief with full elaboration.
- `test_dynamic_body_retrieved_section_medium_mode` — 3 beliefs medium elaboration.
- `test_dynamic_body_retrieved_section_low_mode` — 5 beliefs minimal elaboration.
- `test_dynamic_body_retrieved_section_empty_mode` — novel-territory note rendered.
- `test_dynamic_body_inhibited_section_rendered_when_present`.
- `test_dynamic_body_inhibited_section_omitted_when_empty`.
- `test_dynamic_body_metacognitive_flags_rendered_above_threshold` — constraint violated 55 times → flagged.
- `test_dynamic_body_metacognitive_flags_omitted_below_threshold` — constraint violated 20 times → no flag.
- `test_dynamic_body_token_budget_enforced`.
- `test_dynamic_body_truncation_priority_order` — over-budget → first metacognitive flags dropped, then inhibitions, then retrieved.
- `test_assemble_deterministic_on_same_inputs`.
- `test_cache_boundary_in_output`.
- `test_token_count_estimation_within_10_percent_of_tiktoken` — statistical bench, not strict test.
- `test_empty_mode_still_renders_wm`.
- `test_empty_mode_still_renders_session_state`.

## E2E testing plan

`tests/epistemic_context.rs`:

**`e2e_assemble_end_to_end_high_mode`:**
- Populate DB: 3 beliefs, one scores 0.92 against test query.
- Activate 2 WM beliefs in test session.
- Extract goal.
- Call `assemble`.
- Assert: stable_preamble non-empty; dynamic_body includes session header, WM section with 2 items, retrieved section in High format with 1 item at 0.92; no inhibitions section; no metacognitive flags.

**`e2e_assemble_end_to_end_empty_mode`:**
- Populate DB with 100 beliefs on topic X.
- Set goal on topic X.
- Query on topic Y (unrelated).
- Assert: retrieved section is the novel-territory note; WM and session state still present.

**`e2e_assemble_with_inhibitions`:**
- Inhibit 2 beliefs for reason=AnchorStale.
- Query that would otherwise retrieve those 2.
- Assert dynamic_body includes inhibitions section naming both with reasons.

**`e2e_assemble_with_metacognitive_flags`:**
- Pre-populate `belief_events` with 55 `Contradicted` events for constraint `rust-no-gc`.
- Assemble.
- Assert metacognitive flags section mentions `rust-no-gc` with count=55.

**`e2e_assemble_token_budget_enforced`:**
- Configure `max_dynamic_tokens=500`.
- Populate enough state for 3000 tokens if unbounded.
- Assert dynamic_tokens_est ≤ 500.
- Assert priority: WM + session state retained; metacognitive + inhibitions dropped first.

**`e2e_assemble_preamble_stable_across_session`:**
- Assemble twice in same session with different queries.
- Assert `stable_preamble` bytes are identical.
- Assert `dynamic_body` differs.

**`e2e_assemble_preamble_changes_on_rigor_yaml_reload`:**
- Assemble → capture preamble hash.
- Modify rigor.yaml (add a constraint); reload (test helper triggers invalidation).
- Assemble → assert preamble hash changed.

**`e2e_assemble_goal_context_appears_when_set`:**
- Session has active goal.
- Assemble.
- Assert goal text appears in session state section.

**`e2e_assemble_goal_context_omitted_when_none`:**
- Session has no goal.
- Assemble.
- Assert session state does not include goal line (or includes a placeholder like "Goal: (none)").

## Performance testing plan

`benches/context_assembly.rs`:

**Benchmark 1: assemble end-to-end (High mode).**
- `bench_assemble_high` — session with 100 WM beliefs, 1 retrieved at 0.92; preamble ~3000 tokens.
- **Threshold:** p99 ≤ **30ms** including retrieval.

**Benchmark 2: assemble end-to-end (Low mode).**
- `bench_assemble_low` — 5 retrieved at score 0.5–0.7; dynamic body at budget ceiling.
- **Threshold:** p99 ≤ **35ms**.

**Benchmark 3: assemble Empty mode.**
- `bench_assemble_empty` — retrieval returns nothing.
- **Threshold:** p99 ≤ **10ms** (no retrieval elaboration cost).

**Benchmark 4: stable preamble rendering (cold start).**
- `bench_stable_preamble_render_100_constraints` — first render after daemon start.
- **Threshold:** ≤ **20ms** one-time. Subsequent calls hit the Arc cache and are free.

**Benchmark 5: token estimation.**
- `bench_token_estimate` — estimate tokens for a 10KB dynamic body.
- **Threshold:** ≤ **1ms**.

**Benchmark 6: truncation to budget.**
- `bench_truncation_3000_to_500_tokens` — body at 3000 tokens, truncate to 500 enforcing priority order.
- **Threshold:** ≤ **5ms**.

**Benchmark 7: concurrent assemble (8 sessions).**
- `bench_assemble_concurrent_8` — 8 tokio tasks assembling for distinct sessions.
- **Threshold:** per-task p99 ≤ **50ms** under contention.

## Acceptance criteria

- [ ] `ContextAssembler` trait + `DefaultContextAssembler` impl.
- [ ] Output split into stable_preamble + cache_boundary + dynamic_body.
- [ ] Stable preamble cached as `Arc<String>` after first render; shared across sessions.
- [ ] Four mode renderers (High / Medium / Low / Empty) produce documented formats.
- [ ] Dynamic body section priority: session state (never dropped) → WM → retrieved → inhibitions → metacognitive flags.
- [ ] Truncation applies reverse priority.
- [ ] `metacognitive_flag_violation_threshold` honored.
- [ ] Empty mode renders WM and session state; retrieval is the novel-territory note.
- [ ] Token estimation within 10% of tiktoken (sampled; not enforced hard).
- [ ] All 23 unit tests pass.
- [ ] All 9 e2e tests pass.
- [ ] All 7 perf benchmarks meet thresholds.
- [ ] `cargo clippy -- -D warnings` clean.

## Additional items surfaced in review

- **Behavior when preamble exceeds Anthropic prompt ceiling.** Anthropic caps system prompts at some size (currently ~200k tokens for Opus 4.7 with 1M context). If constraint catalogue grows huge (10k+ constraints), preamble could exceed this. Spec: if preamble token estimate > `preamble_hard_ceiling` (default 150_000 tokens), trim constraint catalogue to top-K constraints by historical firing rate. Add `epistemic.assembler.preamble_hard_ceiling` config; test `test_preamble_trimmed_when_over_ceiling`.
- **Token estimation drift telemetry.** The heuristic (`chars / 3.7`) drifts from actual tokenizer count on code-heavy text. When drift exceeds 20% (measured against tiktoken in the bench), emit a `cortex.assemble.estimation_drift` warning. The feature is opt-in via `--features precise-token-count`.
- **Goal-line rendering when no goal.** Authoritative: if `active_goal` returns None, omit the `Goal:` line entirely (rather than rendering "Goal: (none)"). Cleaner, no confusing empty placeholder. Test: `test_goal_line_omitted_when_no_active_goal`.
- **Cache boundary semantic.** Consumers (EC-10's `assembled_to_body_with_cache_markers`) insert Anthropic's `{ type: "ephemeral" }` cache_control marker on the LAST content block of the stable preamble. The marker applies to "everything up to this point." Clarify in `CacheBoundary` docs.
- **Section ordering is fixed.** Dynamic body section order is deterministic and documented: session state → goal → working memory → retrieved → inhibitions → metacognitive flags → optional empty note. Tests assert order: `test_dynamic_body_section_order_deterministic`.
- **Empty mode wording is the agent's signal for escalation.** Exact text matters — future tooling might pattern-match. Lock wording in a const `EMPTY_MODE_NOTE: &str = "⚠ No past grounding retrieved..."` with a golden test `test_empty_mode_exact_text_matches_golden`.
- **Thread safety of `stable_preamble_cache`.** `tokio::sync::RwLock<Option<Arc<String>>>` — many readers, rare writer (on invalidate). Test: `test_concurrent_assemble_shares_cached_preamble` (no duplicate renders).
- **Observability (X-1).** `cortex.assemble` span with `session_id`, `mode`, `stable_tokens_est`, `dynamic_tokens_est`, `preamble_cache_hit: bool`, `assemble_ms`.
- **No-recursion note.** Assembler never makes LLM calls; no X-2 concern.
- **Code-editing preservation-minimality instruction.** When `AssemblerHint.active_tools` includes any of `"Edit"`, `"Write"`, `"NotebookEdit"` (or any future file-mutation tool), the stable preamble injects an additional subsection titled `## Code-editing minimality` with canonical wording: *"When making code changes, preserve the original code and logic as much as possible. Make the minimal edit that addresses the requirement. Over-editing — rewriting more than necessary, renaming unrelated variables, adding unrequested validation, or restructuring existing logic — is explicitly discouraged and will be flagged."* Empirical basis: the 2026 Rehir article "Coding Models Are Doing Too Much" (nrehiew.github.io/blog/minimal_editing/) shows that this single explicit instruction universally improves both Pass@1 correctness AND token-Levenshtein minimality across every frontier model tested (Opus, Sonnet, GPT-5, Qwen, others), with reasoning models showing the largest gains. Over-editing is empirically a default behavior — not a capability limit — so this is the cheapest + highest-leverage cortex intervention for code-edit sessions. Add config knob `epistemic.assembler.inject_code_minimality_when_editing: bool` (default `true`). Because the catalogue includes this subsection only when the relevant tools are active, the stable preamble cache keys on the presence/absence of that section — either one cached version per (tool-set shape), or the subsection goes below the cache boundary into the dynamic body. Pick the latter to keep cache simple: render the preservation instruction in the DYNAMIC body as its own typed section `# Code-editing discipline` between `# Session state` and `# Active working memory`. Tests:
    - `test_assemble_with_edit_tool_includes_preservation_instruction`
    - `test_assemble_without_edit_tool_omits_preservation_instruction`
    - `test_assemble_with_write_tool_includes_preservation_instruction`
    - `test_preservation_instruction_is_in_dynamic_not_stable_preamble` (keeps preamble cache stable across turns with different tool presence)
    - `test_config_knob_disables_preservation_instruction` (flag off → section omitted even with edit tools)
    - Golden-text test: `test_preservation_instruction_exact_wording_matches_golden` (locks the canonical string so future tooling can pattern-match).

## Dependencies

**Blocks:** EC-10.
**Blocked by:** EC-1, EC-2, EC-3, EC-4, EC-5, EC-6, EC-7.

## References

- Umbrella: [UMBRELLA] Epistemic Cortex
- EC-1 through EC-7
- `src/daemon/proxy.rs:1256` — existing `build_epistemic_context` call site
- Claude prompt caching docs (cache_control: ephemeral)
- Project memory: `project_token_economy.md`, `project_epistemology_expansion.md`
