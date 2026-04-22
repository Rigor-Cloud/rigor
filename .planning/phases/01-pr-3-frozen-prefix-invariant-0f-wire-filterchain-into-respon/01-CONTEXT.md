# Phase 1: PR-3 — frozen-prefix invariant (0F) + wire FilterChain into response path (0G) — Context

**Gathered:** 2026-04-22
**Status:** Ready for planning
**Source:** GitHub issue #18 body + `.planning/roadmap/epistemic-expansion-plan.md` §5.6 (0F), §5.7 (0G)
**Workstream:** phase-0-close (active)

<domain>
## Phase Boundary

Deliver two tightly-coupled Phase 0 pieces in one PR:

1. **0F — Frozen-prefix invariant.** Introduce a typed invariant (`FrozenPrefix { message_count, byte_checksum }`) stored in `ConversationCtx::scratch`. A post-chain verifier in `FilterChain::apply_request` recomputes the checksum over `messages[0..message_count]`. Divergence → `panic!` in debug builds, `warn!` + reject in release.

2. **0G — Wire FilterChain into response path.** The `EgressFilter` trait already has `apply_response_chunk` and `finalize_response` methods, but nobody calls them today. Wire them into the SSE loop at `crates/rigor/src/daemon/proxy.rs:1517-1644` and into post-stream cleanup. Existing inline code (claim extraction, violation persistence, auto-retry) MUST stay and run alongside.

Shipping 0F together with 0G guarantees the first response-side chain consumer is born with the invariant already enforced — preventing a future CCR/audit/annotation filter from silently busting prompt-cache invariants.

**What this unblocks (downstream):**
- Phase 1B (CCR retrieval loop)
- Phase 3A (annotation emission from proxy)
- Any future response-side `EgressFilter` impl

</domain>

<decisions>
## Implementation Decisions (LOCKED — from issue #18)

### Files to create
- `crates/rigor/src/daemon/egress/frozen.rs` — home for `FrozenPrefix` struct, `compute_checksum`, `set_frozen_prefix`, `verify_frozen_prefix`.

### Files to modify
- `crates/rigor/src/daemon/egress/mod.rs` — export `frozen::*`
- `crates/rigor/src/daemon/egress/chain.rs` — add `verify_frozen_prefix` helper; call it after `apply_request` succeeds (around lines 112-121)
- `crates/rigor/src/daemon/proxy.rs` — around lines 1517-1644 (SSE chunk parsing). Construct the response chain once per request (same chain as the request side). Call `chain.apply_response_chunk(&mut chunk, &mut ctx).await` per SSE chunk. Call `chain.finalize_response(&mut ctx).await` after stream close.

### API sketch (from issue)

```rust
// egress/frozen.rs
pub struct FrozenPrefix {
    pub message_count: usize,
    pub byte_checksum: u64,
}

pub fn compute_checksum(messages: &[serde_json::Value]) -> u64 {
    // xxhash or fnv over canonical bytes
}

pub fn set_frozen_prefix(ctx: &mut ConversationCtx, messages: &[serde_json::Value], freeze_count: usize) {
    ctx.scratch_set(FrozenPrefix {
        message_count: freeze_count,
        byte_checksum: compute_checksum(&messages[..freeze_count]),
    });
}

pub fn verify_frozen_prefix(ctx: &ConversationCtx, messages: &[serde_json::Value]) -> Result<(), FilterError> {
    // if scratch has FrozenPrefix, recompute over [0..message_count] and compare
    // absence of FrozenPrefix in scratch → Ok (backward compat — no-op)
}
```

### Invariant protocol (LOCKED)
- Request filters MAY mutate messages at index `>= frozen_message_count`.
- Request filters that NEED to change the frozen prefix MUST explicitly call `set_frozen_prefix` with the new baseline.
- Verifier runs ONCE, after all request filters, before upstream send.
- Debug build on violation: `panic!`. Release build on violation: `warn!` + reject request.

### Out of scope (LOCKED)
- Any actual response-side filter implementation (CCR, audit, annotation). Those land in Phase 1 / Phase 3.
- Canonicalizer (`0F` companion piece from §5.6) — tracked separately if needed; NOT in this PR.
- Compression stages (Phase 1).

### Claude's Discretion
- Choice of hash function (xxhash vs fnv vs siphash). Must be deterministic across runs; not cryptographic. The existing `sha2` dependency covers content-addressing; this is different — we want speed. **Default pick: `twox-hash` (xxhash) because rigor already uses `fnv = "1.0"` in Cargo.toml for some internal maps, so we avoid adding a new dep if fnv is adequate — must verify perf is acceptable for large prompts.**
- SSE chunk-boundary handling — whether the filter sees per-SSE-event or per-accumulated-string. **Default pick: per-SSE-event (raw `&mut Vec<u8>` or `&mut String` per chunk) to let filters buffer if they want, matching existing `extract_sse_assistant_text` style at proxy.rs:3074+.**
- Where exactly to construct the response chain — once at request entry (reusing the request chain) vs. separately. **Default pick: reuse — same `Arc<FilterChain>` both sides, consistent with existing codebase pattern.**
- Error-handling posture when `finalize_response` emits extra SSE chunks — forward verbatim, or require chunks to be pre-framed. **Default pick: forward verbatim; filter owns framing.**

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Spec sources
- `.planning/roadmap/epistemic-expansion-plan.md` §5.6 (0F frozen prefix + canonicalizer) — lines 220-253 approx
- `.planning/roadmap/epistemic-expansion-plan.md` §5.7 (0G response-side chain) — lines 255-259

### Code sources (read-before-modify)
- `crates/rigor/src/daemon/egress/chain.rs` — `EgressFilter` trait (:42), `FilterChain::apply_request` (:112-121), `apply_response_chunk` (:56-71 trait methods)
- `crates/rigor/src/daemon/egress/ctx.rs` — `ConversationCtx::scratch` (:76)
- `crates/rigor/src/daemon/egress/claim_injection.rs` — sole existing filter impl (:15-62) — shows the pattern
- `crates/rigor/src/daemon/proxy.rs:1135` — request-side chain construction point
- `crates/rigor/src/daemon/proxy.rs:1517-1644` — SSE streaming handler (insert response-chain calls here)
- `crates/rigor/src/daemon/proxy.rs:3074+` — `extract_sse_assistant_text` (existing SSE parser style)
- `crates/rigor/src/constraint/graph.rs:447` — DF-QuAD regression guard (must remain green)

### Tests (read-before-extending)
- `crates/rigor/tests/egress_integration.rs` — existing mock-filter pattern; extend for new response-hook tests
- `crates/rigor/tests/dogfooding.rs`, `firing_matrix.rs`, `false_positive.rs`, `invariants.rs` — MUST still pass

### Project instructions
- `./CLAUDE.md` — project-level guidelines (TDD required per memory feedback)
- `.github/pull_request_template.md` — PR body format

### Memory files relevant to this phase
- `feedback_tdd.md` — TDD required
- `project_chunk_eval.md` — existing streaming evaluation, not to be regressed
- Verified truth: "FilterChain: Outer-to-Inner Request, Inner-to-Outer Response" — apply_response_chunk runs inner→outer, best-effort (errors logged, chain continues)

</canonical_refs>

<specifics>
## Specific Tests Required (from issue)

1. `frozen.rs` unit tests:
   - `compute_checksum` deterministic across runs
   - round-trip `set_frozen_prefix` + `verify_frozen_prefix` = `Ok`
   - mutation of frozen range → `Err(FilterError)`
   - absence of `FrozenPrefix` in scratch = `Ok` (backward compat — no-op)

2. `tests/egress_integration.rs`:
   - New test filter that mutates `messages[0]` after `set_frozen_prefix(count=1)` → expect `apply_request` to fail
   - New test filter with `apply_response_chunk` that counts chunks → assert chain invoked N times
   - `finalize_response` returns extra SSE chunks → assert they're forwarded

3. Existing 7+ integration tests must still pass (`dogfooding`, `firing_matrix`, `false_positive`, `invariants`, etc.)

4. DF-QuAD regression guard at `constraint/graph.rs:447` must remain green.

## Acceptance criteria (from issue)

- `cargo test --all-features` green
- `cargo clippy --all-targets --all-features -- -D warnings` clean
- `cargo fmt -- --check` clean
- `./target/release/rigor validate --path rigor.yaml` reports 53 constraints
- PR description follows `.github/pull_request_template.md` including the Release Notes block
- All 5 CI jobs pass including `Prompt-Injection Scan` and the approval gate

</specifics>

<deferred>
## Deferred Ideas

- **Canonicalizer filters** (`CanonicalizeToolsFilter`, `DynamicContentFilter` from §5.6) — tracked as a follow-up issue if prompt-cache invariance becomes a real production concern. Not required to unblock Phase 1B / 3A.
- **Actual response-side filters** (CCR, audit, annotation) — land in Phase 1 / Phase 3. This PR only builds the plumbing.
- **Hash algorithm benchmarks** — pick one default (see Claude's Discretion), revisit only if perf shows up in flamegraph.

</deferred>

---

*Phase: 01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon*
*Context gathered: 2026-04-22 from issue #18 + epistemic-expansion-plan.md*
*Issue tracker: #18 (umbrella: #28)*
