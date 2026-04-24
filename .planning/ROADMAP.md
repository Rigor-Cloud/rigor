---
project: rigor
milestone: phase-0-close-plus-coverage
created: 2026-04-22
owner: vibhav
status: active
---

# Rigor Roadmap — Phase 0 close-out + coverage/CI hardening

**Scope:** Close all 23 open GitHub issues (#6–#28). Phases below map 1:1 to issues. GitHub is the source-of-truth tracker; this roadmap is the GSD implementation scaffold.

**Existing roadmap docs (not superseded):**
- `.planning/roadmap/ROADMAP.md` — zappa-inspired hackathon/architectural vision (orthogonal)
- `.planning/roadmap/epistemic-expansion-plan.md` — full 0A–0J Phase 0 spec (authoritative source for Phase 1–3 scope below)
- `.planning/roadmap/pr-2.7-test-coverage-plan.md` — test-coverage authoritative source for Phase 8 scope

**Workstream mapping:**
- `phase-0-close` — Phases 1–3 (unblocks Phase 1 CCR + GEPA + Modal training)
- `corpus-cli` — Phases 4–6 (PR-2.7 Tier 2)
- `test-infra` — Phases 7–8 (foundation for coverage work)
- `coverage` — Phases 9–14 (PR-2.7 Tier 3 + unit/E2E gaps)
- `ci-hardening` — Phases 15–21 (CI + release)

---

## Milestone 0: Close all open issues

### Workstream: phase-0-close

#### Phase 1: PR-3 — frozen-prefix invariant (0F) + wire FilterChain into response path (0G) ✓ COMPLETE 2026-04-23
**Goal:** Land frozen-prefix invariant over the egress request-body messages array; wire the FilterChain into the proxy response path so egress filters run on streamed chunks. Unblocks Phase 1B CCR annotation emission and Phase 3A retroactive annotation.
**Issue:** #18
**Depends on:** none
**Requirements:** REQ-001, REQ-002, REQ-003, REQ-004, REQ-005 (all verified, 14/14 must-haves)
**Workstream:** phase-0-close
**Canonical spec:** `.planning/roadmap/epistemic-expansion-plan.md` sections 0F, 0G
**Verification:** `.planning/phases/01-pr-3-frozen-prefix-invariant-0f-wire-filterchain-into-respon/01-VERIFICATION.md`
**Plans:** 5 plans (all complete)
- [x] 01-01-PLAN.md — frozen.rs module + twox-hash dep + 8 unit tests (TDD, wave 1)
- [x] 01-02-PLAN.md — wire verify_frozen_prefix into FilterChain::apply_request (wave 2)
- [x] 01-03-PLAN.md — wire FilterChain response methods into proxy.rs SSE loop + OTel span (wave 2)
- [x] 01-04-PLAN.md — integration tests + full regression + 53-constraint acceptance check (wave 3)
- [x] 01-05-PLAN.md — criterion baselines for compute_checksum + response-chain overhead (wave 3, parallel with 04)

#### Phase 2: PR-4 — corpus exporter (`rigor refine export`)
**Goal:** `rigor refine export` emits training-ready JSONL from the violation log. Unblocks Phase 3E GEPA prompt optimization and Phase 4E Modal discriminator training.
**Issue:** #19
**Depends on:** none
**Requirements:** REQ-006, REQ-007
**Workstream:** phase-0-close
**Canonical spec:** `.planning/roadmap/epistemic-expansion-plan.md` section 0J

#### Phase 3: PR-5 — ONNX host (feature-flagged)
**Goal:** Add optional ONNX runtime behind a feature flag. Shared infra for Phase 1D Kompress (context compression) and Phase 4F ModernBERT safety discriminator.
**Issue:** #20
**Depends on:** none
**Requirements:** REQ-008, REQ-009
**Workstream:** phase-0-close
**Canonical spec:** `.planning/roadmap/epistemic-expansion-plan.md` section 0H

### Workstream: corpus-cli

#### Phase 4: `rigor corpus` CLI subcommand wiring
**Goal:** Wire `rigor corpus record / stats / validate` dispatchers over already-merged library functions. Pure CLI surface; logic exists in lib.
**Issue:** #21
**Depends on:** none
**Requirements:** REQ-010, REQ-011, REQ-012
**Workstream:** corpus-cli

#### Phase 5: Seed corpus recording (20 prompts × 4 models × 10 samples)
**Goal:** Record the baseline PR-2.7 corpus on OpenRouter for reproducible replay. ~$2–5 of inference.
**Issue:** #22
**Depends on:** Phase 4
**Requirements:** REQ-013
**Workstream:** corpus-cli

#### Phase 6: Pretty-printed stats table for `rigor corpus stats`
**Goal:** Replace JSON-only output with a TTY-friendly aligned table.
**Issue:** #23
**Depends on:** Phase 4
**Requirements:** REQ-014
**Workstream:** corpus-cli

### Workstream: test-infra

#### Phase 7: crates/rigor/tests/ integration test infrastructure ✓ COMPLETE 2026-04-24
**Goal:** Stand up real-TCP, rustls, SSE, `$HOME` isolation harness as a shared library. Precondition for Phases 9–12.
**Issue:** #8
**Depends on:** none
**Requirements:** REQ-015, REQ-016, REQ-017
**Workstream:** test-infra
**Plans:** 2 plans (all complete)
- [x] 07-01-PLAN.md — IsolatedHome, TestCA, MockLlmServer, SSE helpers (wave 1)
- [x] 07-02-PLAN.md — TestProxy, subprocess helpers, smoke integration test (wave 2)

#### Phase 8: `$HOME/.rigor` test isolation
**Goal:** Tests must not touch the real `$HOME/.rigor` (PID file, CA cert, violations log). Use `TempDir` fixtures.
**Issue:** #15
**Depends on:** Phase 7
**Requirements:** REQ-018
**Workstream:** test-infra
**Plans:** 2 plans
- [x] 08-01-PLAN.md — Create paths.rs with rigor_home() + replace all 17 call sites (wave 1)
- [x] 08-02-PLAN.md — Update TestProxy to RIGOR_HOME + CI grep guard (wave 2)

### Workstream: coverage

#### Phase 9: daemon/proxy.rs hot-path tests
**Goal:** Cover `proxy_request`, `extract_and_evaluate`, `scope_judge_check`, `score_claim_relevance` — currently zero test coverage.
**Issue:** #7
**Depends on:** Phase 7, Phase 8
**Requirements:** REQ-019
**Workstream:** coverage
**Plans:** 2 plans
- [x] 09-01-PLAN.md — JudgeClient trait seam + unit tests for judge functions (wave 1)
- [x] 09-02-PLAN.md — Integration tests for extract_and_evaluate, evaluate_text_inline, proxy_request (wave 2)

#### Phase 10: Unit coverage gaps (MITM allowlist, daemon lifecycle, TLS CA, SNI, DF-QuAD, content_store TTL, action gates)
**Goal:** Close listed unit-level gaps to lift coverage floor.
**Issue:** #16
**Depends on:** Phase 7
**Requirements:** REQ-020
**Workstream:** coverage
**Plans:** 3/3 plans complete
- [x] 10-01-PLAN.md — Daemon module tests: MITM allowlist, PID lifecycle, TLS CA, SNI edge cases (wave 1)
- [x] 10-02-PLAN.md — Evaluator fail-open, DF-QuAD boundaries, SeverityThresholds, claim pipeline ordering (wave 1)
- [x] 10-03-PLAN.md — Content store TTL/concurrency, action gate timeout/lifecycle (wave 1)

#### Phase 11: E2E coverage gaps (BLOCK kill-switch, auto-retry, PII-before-upstream, blind-tunnel, TLS MITM handshake, stop-hook, corpus drift)
**Goal:** Close listed end-to-end gaps (Phase 11 scope: blind-tunnel, TLS MITM, stop-hook, PID lifecycle; Phase 12 covers B1/B2/B3).
**Issue:** #17
**Depends on:** Phase 7
**Requirements:** REQ-021
**Workstream:** coverage
**Plans:** 2 plans
- [x] 11-01-PLAN.md — TestProxy CONNECT upgrade support + blind-tunnel/MITM handshake E2E tests (wave 1)
- [x] 11-02-PLAN.md — Stop-hook harness E2E + PID crash recovery lifecycle tests (wave 1)

#### Phase 12: Mock-LLM server harness + B1/B2/B3 integration tests
**Goal:** Build mock-LLM server + streaming kill-switch / auto-retry / PII redact-before-forward integration tests. Largest chunk; unblocks Phase 13.
**Issue:** #24
**Depends on:** Phase 7
**Requirements:** REQ-022, REQ-023, REQ-024
**Workstream:** coverage

#### Phase 13: F6 full-proxy corpus replay via mock-LLM
**Goal:** Exercise full MITM → streaming → decision path against recorded corpus bytes.
**Issue:** #25
**Depends on:** Phase 5, Phase 12
**Requirements:** REQ-025
**Workstream:** coverage

#### Phase 14: rigor-test e2e harness flesh-out
**Goal:** Replace "not yet implemented" stubs in rigor-test subcommands with real flows.
**Issue:** #6
**Depends on:** Phase 7
**Requirements:** REQ-026
**Workstream:** coverage

### Workstream: ci-hardening

#### Phase 15: Split `ci-approval` environment into fast/gated
**Goal:** Let clippy/rustfmt/tests fail-close autonomously. Move gated approval to jobs that actually need secrets.
**Issues:** #11, #27
**Depends on:** none
**Requirements:** REQ-027
**Workstream:** ci-hardening

#### Phase 16: macOS CI matrix
**Goal:** Release ships macOS binaries but CI only runs ubuntu-latest. Add darwin to the matrix.
**Issue:** #10
**Depends on:** none
**Requirements:** REQ-028
**Workstream:** ci-hardening

#### Phase 17: CI hardening — cargo-audit, cargo-deny, llvm-cov floor, bench-regression gate, release artifact signing
**Goal:** Supply-chain + quality gates on every PR.
**Issue:** #13
**Depends on:** Phase 15
**Requirements:** REQ-029, REQ-030, REQ-031, REQ-032
**Workstream:** ci-hardening

#### Phase 18: pr-injection-scan.yml self-regression corpus
**Goal:** Add fixture corpus that exercises all 9 regex patterns + 30KB-capped judge path.
**Issue:** #14
**Depends on:** none
**Requirements:** REQ-033
**Workstream:** ci-hardening

#### Phase 19: rigor-validate CI expansion (rigor eval + corpus replay + violation-log cross-check)
**Goal:** Current rigor-validate only parses rigor.yaml. Extend to actual evaluation flow.
**Issue:** #12
**Depends on:** Phase 4, Phase 13
**Requirements:** REQ-034
**Workstream:** ci-hardening

#### Phase 20: Wire corpus replay into EvaluatorPipeline + CI drift check
**Goal:** PR-2.7 corpus isn't wired to EvaluatorPipeline. Close the loop + gate CI on drift.
**Issue:** #9
**Depends on:** Phase 4, Phase 5
**Requirements:** REQ-035
**Workstream:** ci-hardening

#### Phase 21: Stale `rigor` binary detection
**Goal:** Catch the `~/.cargo/bin/rigor` drift incident (e68c9cf) via test fixture comparing PATH binary to cargo-built.
**Issue:** #26
**Depends on:** none
**Requirements:** REQ-036
**Workstream:** ci-hardening

---

## Tracking

- Umbrella issue: #28
- Recommended execution order: see "Critical-path ordering" in conversation with Claude 2026-04-22
- Wave 1 (parallel): Phases 1, 7+8, 15, 16, 18, 21
- Wave 2: Phases 2, 3, 4, 10, 11, 14, 17, 19
- Wave 3: Phases 5, 6, 9, 12
- Wave 4: Phases 13, 20

## Cross-references

- Codebase map: `.planning/codebase/{STACK,ARCHITECTURE,STRUCTURE,CONVENTIONS,INTEGRATIONS,TESTING,CONCERNS}.md`
- Knowledge graph: `.planning/graphs/graph.html` (2927 nodes / 7986 edges, built 2026-04-22)
- Constraint DSL: `rigor.yaml`
- GSD workflow commands: `/gsd-plan-phase`, `/gsd-execute-phase`, `/gsd-autonomous`
