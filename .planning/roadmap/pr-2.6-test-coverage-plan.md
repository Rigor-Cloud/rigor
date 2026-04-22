# PR-2.6 / 2.7 — Test Coverage Audit + Perf Harness

**Version:** v1
**Status:** proposed — PR-2.6 Tier 1 in progress
**Scope:** close coverage gaps exposed by the rigor.yaml audit; establish perf baselines; unlock real-LLM integration tests via the OpenRouter key (set on repo secrets 2026-04-22).

## Inventory

| Bucket | Files | Count |
|---|---|---|
| Integration tests (`tests/*.rs`) | 7 | ~41 `fn test_*` |
| Unit tests (inline `#[cfg(test)]`) | 45 modules | ~280 fns |
| Benches (criterion) | 2 | `hook_latency`, `evaluation_only` |
| Real-LLM tests | 0 | — |
| Firing-matrix coverage | — | 10 of 53 constraints exercised in `dogfooding.rs` |

## Coverage gaps (what this plan addresses)

1. **43 of 53 constraints** have no firing fixture.
2. **Zero false-positive protection** — negation / quotation / meta-discussion all untested.
3. **Streaming kill-switch timing** never asserted at the connection level.
4. **Auto-retry exactly-once** not integration-tested.
5. **PII redact-before-forward** invariant never verified by inspecting the upstream request.
6. **DF-QuAD determinism** not stressed across 100+ runs.
7. **Fail-open under injected failures** covered piecemeal; no systematic pass.
8. **Atomic log rewrite crash resilience** untested.
9. **Content store TTL** only unit-tested, not integration.
10. **Gate 60s timeout** untested.
11. **WS event emission completeness** not asserted (every claim → event?).
12. **OTel GenAI spans** never inspected in tests.
13. **Real-LLM round-trip** never exercised.
14. **Cost tracking accuracy** not validated against provider billing.

## Test plan — five categories

### A. Coverage tests

**A1. Constraint firing matrix (Tier 1):** per-constraint fixture pairs at `crates/rigor/tests/fixtures/firing_matrix/<constraint_id>/{should_fire,should_not_fire}.json`. Parametrized test walks the directory; adding a constraint auto-requires its pair. 53 × 2 = 106 assertions.

**A2. Adversarial false-negative probes (Tier 2):** hedged fabrications, code-block fabrications, cross-sentence chains, implicit tool-call fabrications, non-English, multi-constraint cascades. Each documents a known limitation or hardens the pipeline.

**A3. False-positive probes (Tier 1, top 15 constraints):** negated / quoted / historical / meta / comparative / user-echo. ~6 fixtures × 15 constraints = 90 negative assertions.

### B. Invariant / correctness tests

| # | Tier | Test | Catches |
|---|---|---|---|
| B1 | 1 | Streaming kill-switch drops upstream on BLOCK | AI "graceful finish" regression |
| B2 | 1 | Auto-retry fires exactly once; second BLOCK surfaces | Unbounded retry regression |
| B3 | 1 | PII redacted before upstream `POST` (inspect intercepted request) | Redaction-after-forward bug |
| B4 | 1 | DF-QuAD determinism — 100 identical inputs → 100 identical strengths | HashMap swap regression |
| B5 | 2 | Fail-open: inject error at Rego / judge / LSP / network; assert allow | Fail-closed regression |
| B6 | 2 | Atomic log rewrite survives SIGKILL mid-flight | Corruption on crash |
| B7 | 2 | Content store Verdict actually expires at 24h | TTL drift |
| B8 | 2 | Gate 60s timeout auto-rejects | Hung request regression |
| B9 | 2 | `anchor_sha256` mismatch invalidates cached verdict | Stale cache regression |
| B10 | 1 | `enforcement-requires-traffic-routing` — daemon without traffic is inert | Runtime-hook confusion |

### C. Observability tests (Tier 2)

- C1: Every extracted claim emits `ClaimExtracted` WS event
- C2: Every BLOCK emits `Decision` WS event with matching violations
- C3: OTel GenAI spans present for proxied LLM calls
- C4: Cost tracking matches OpenRouter billing (real-LLM)

### D. Performance harness

| # | Tier | Bench | Metric | Target |
|---|---|---|---|---|
| D1 | 1 | `proxy_roundtrip` vs. bypass | Added latency per request | ≤ 5ms median |
| D2 | 2 | `claim_extract` over 1k/10k/100k char | p99 latency | ≤ 100ms for 100k |
| D3 | 1 | `dfquad_scaling` over 10/100/1000 constraints | Compute time | O(n) confirmed |
| D4 | 2 | `content_store_concurrent` | Throughput | ≥ 10k ops/s |
| D5 | 2 | `violation_log_throughput` | Entries/s | ≥ 5k |
| D6 | 2 | `pii_redact_overhead` | Delta | ≤ 2ms |
| D7 | 2 | `e2e_block_latency` (real LLM) | Time-to-kill | ≤ 500ms |
| D8 | 1 | `startup_cost` — cold-start to ready | End-to-end | ≤ 2s |

Baseline committed to `.planning/perf/baseline.json`. CI enforcement (±10% regression budget) deferred to PR-2.7.

### E. Real-LLM integration (gated by `OPENROUTER_API_KEY`)

| # | Tier | Test |
|---|---|---|
| E1 | 1 | Real stream through rigor → claims extracted → correct decision |
| E2 | 2 | Real auto-retry — force violation, verify self-correction on round 2 |
| E3 | 2 | Multi-provider (Claude + GPT via OpenRouter) both behave identically |
| E4 | 2 | MITM cert chain integrity via generated CA |
| E5 | 2 | 429 rate-limit passes through without masking |

Gated by `#[ignore]` + env check. CI runs Tier 1 real-LLM on `workflow_dispatch` + nightly cron. Not on every PR (cost control).

## Infrastructure

1. **Mock LLM server** — `crates/rigor/tests/support/mock_llm.rs`: local hyper server, canned SSE responses.
2. **Fixture-driven test runner** — single parametrized `firing_matrix` test walks the fixtures dir.
3. **Crash-injection helper** — subprocess + SIGKILL at configurable syscall. Tier 2.
4. **Real-LLM gate** — `require_openrouter!()` macro auto-skips without the env var.
5. **Baseline bench script** — `scripts/record-baseline.sh`. Tier 2.

## Tier 1 (PR-2.6)

- A1 full (53 × 2 fixtures)
- A3 for top 15 constraints (~90 fixtures)
- B1 / B2 / B3 / B4 / B10
- D1 / D3 / D8 (baselines only)
- E1 (one real-LLM proof-of-life)
- Infrastructure 1, 2, 4

Estimated: ~1500 LOC of tests + infra + ~250 fixture files.

## Tier 2 (PR-2.7)

- A2 adversarial probes
- B5 / B6 / B7 / B8 / B9
- C1-4 observability
- D2 / D4 / D5 / D6 / D7
- E2-5
- Infrastructure 3 (crash injection), 5 (baseline script + regression CI)

## Defaults chosen

- Fixtures live under `crates/rigor/tests/fixtures/` (test code + data colocated).
- Real-LLM trigger: `workflow_dispatch` + nightly — not per-PR.
- Crash injection deferred to Tier 2.
- Baseline regression enforcement deferred to Tier 2.

## Out of scope

- Fuzzing (tarpaulin-style or cargo-fuzz campaigns) — separate initiative.
- Mutation testing — tracked under `.planning/roadmap/epistemic-expansion-plan.md` Phase 4 future items.
- Cross-OS matrix (tests run on Linux CI + macOS dev). Windows support not targeted.
