---
project: rigor
milestone: phase-0-close-plus-coverage
created: 2026-04-22
---

# Rigor Requirements — Phase 0 close-out + coverage/CI hardening

Requirements are grouped by workstream. Every REQ-ID is referenced by at least one phase in `.planning/ROADMAP.md`.

---

## Workstream: phase-0-close

### PR-3 (Phase 1 — issue #18)

- **REQ-001** — Frozen-prefix invariant MUST be enforced over the egress request-body messages array. Once `set_frozen_prefix(ctx, messages, N)` has been called in a request, no subsequent request filter may mutate `messages[0..N]`. The verifier runs ONCE after all request filters but before upstream send. Divergence MUST `panic!` in debug builds and MUST `tracing::warn!` + reject in release builds. Absence of `FrozenPrefix` in scratch MUST be a no-op (backward compatibility). Verified by property tests: round-trip set + verify = Ok; mutation of frozen range → Err; absence of FrozenPrefix = Ok.
- **REQ-002** — `FilterChain::apply_response_chunk` and `FilterChain::finalize_response` MUST be wired into `daemon/proxy.rs` response path, running inner→outer, best-effort (errors logged, chain continues) — matching the established FilterChain contract.
- **REQ-003** — Response-path filter execution MUST NOT drop SSE chunks on error. Filter panics/errors logged via `tracing::warn!` and chunk forwarded verbatim.
- **REQ-004** — All response-path filter invocations emit an OpenTelemetry span under the existing `rigor.daemon.proxy` tracer.
- **REQ-005** — At least one integration test in `crates/rigor/tests/` demonstrates end-to-end frozen-prefix enforcement through the FilterChain: a test request filter that mutates the frozen message prefix causes `FilterChain::apply_request` to return `Err` in release builds and `panic!` in debug builds.

### PR-4 (Phase 2 — issue #19)

- **REQ-006** — `rigor refine export` MUST emit JSONL where each line is one training record (violation + context + ground-truth decision + metadata).
- **REQ-007** — Exporter MUST be streaming (does not load the full violations log into memory); output path MUST be `--out <path>` with stdout fallback.

### PR-5 (Phase 3 — issue #20)

- **REQ-008** — ONNX runtime integration MUST be behind a Cargo feature flag `onnx` (default off). Default build remains pure-Rust with no ONNX dependency.
- **REQ-009** — A trait abstraction (`InferenceHost` or similar) MUST separate ONNX from the rest of the codebase, so Kompress and ModernBERT can depend on the trait, not the concrete runtime.

---

## Workstream: corpus-cli

### Phase 4 — issue #21

- **REQ-010** — `rigor corpus record` CLI subcommand dispatches to `ChatClient::record` and writes to `~/.rigor/corpus/`.
- **REQ-011** — `rigor corpus stats` reads `~/.rigor/corpus/` and emits per-model/per-prompt summary.
- **REQ-012** — `rigor corpus validate` verifies integrity (SHA-256, schema) of recorded corpus entries.

### Phase 5 — issue #22

- **REQ-013** — Seed corpus includes 20 prompts × 4 models × 10 samples (800 records minimum). Models: claude-sonnet-4-6, claude-haiku-4-5, gpt-4o-mini, gemini-2.0-flash (or current OpenRouter equivalents). Seed committed to `.planning/corpus/recordings/seed/`.

### Phase 6 — issue #23

- **REQ-014** — `rigor corpus stats` produces aligned TTY output with columns: model, prompt, samples, drift_score, last_recorded.

---

## Workstream: test-infra

### Phase 7 — issue #8

- **REQ-015** — `crates/rigor/tests/` contains a shared test-support library exposing: real TCP proxy bring-up, rustls CA generation, SSE client, isolated HOME fixture.
- **REQ-016** — Each integration test can be run alone (`cargo test --test <name>`) without leaking state into the real `$HOME`.
- **REQ-017** — Test support library reuses production types where possible; fixtures stub network (mock-LLM) but not internal logic.

### Phase 8 — issue #15

- **REQ-018** — No test writes to real `$HOME/.rigor/`. All tests touching daemon lifecycle, CA cert, or violations log use a `TempDir`-based fixture from the test-support library (REQ-015). Verified by a CI check that greps test output for the real `$HOME` path.

---

## Workstream: coverage

### Phase 9 — issue #7

- **REQ-019** — Unit or integration tests exist for each of: `proxy_request`, `extract_and_evaluate`, `scope_judge_check`, `score_claim_relevance`. Coverage measured by `cargo llvm-cov` MUST be non-zero for each.

### Phase 10 — issue #16

- **REQ-020** — Unit tests exist for: MITM allowlist matching, daemon lifecycle (start/stop/PID file), TLS CA generation + leaf cert signing, SNI extraction, DF-QuAD boundary cases (single-attacker dominance, zero-attacker), SeverityThresholds comparison at exact thresholds (0.7, 0.4), content_store TTL eviction behavior, action gate timeout (60s).

### Phase 11 — issue #17

- **REQ-021** — E2E tests exist for: BLOCK kill-switch (upstream connection drops mid-stream), auto-retry (exactly-once injection of violation feedback), PII-before-upstream (sanitizer runs before forwarding), blind-tunnel (non-LLM hosts preserve E2E TLS), TLS MITM handshake (leaf cert validates against generated CA), stop-hook (post-response evaluation path), corpus drift detection.

### Phase 12 — issue #24

- **REQ-022** — Mock-LLM server harness in `crates/rigor/tests/support/` serves deterministic SSE responses configurable per-test.
- **REQ-023** — B1: streaming kill-switch test — daemon BLOCK drops upstream within N ms of decision.
- **REQ-024** — B2: auto-retry exactly-once test — on BLOCK, one retry with violation-feedback-injected prompt, not two.
- **REQ-025a** — B3: PII redact-before-forward — sanitizer modifies request body before upstream send (not after, not in parallel).

### Phase 13 — issue #25

- **REQ-025** — F6 full-proxy replay drives recorded corpus bytes through the complete MITM→streaming→decision pipeline with no network calls. Asserts identical verdicts between live and replay.

### Phase 14 — issue #6

- **REQ-026** — `rigor-test` subcommands (currently stubbed with "not yet implemented") have real implementations with passing smoke tests.

---

## Workstream: ci-hardening

### Phase 15 — issues #11, #27

- **REQ-027** — CI workflow split: `ci-fast` (clippy, rustfmt, unit tests, no secrets, no approval) runs on every push. `ci-approval` (LLM-key-requiring jobs) runs only when reviewer adds `run-ci-approval` label or on merged-to-main. Clippy/rustfmt/tests fail-close without manual gate.

### Phase 16 — issue #10

- **REQ-028** — GitHub Actions matrix includes `macos-latest` in addition to `ubuntu-latest` for at least test + build jobs. Release job matches the macOS target.

### Phase 17 — issue #13

- **REQ-029** — `cargo audit` runs on every PR and fails on critical/high advisories.
- **REQ-030** — `cargo deny` runs on every PR with a project `deny.toml` gating licenses + banned crates.
- **REQ-031** — `cargo llvm-cov` enforces a coverage floor (configurable; initial value ≥ 60% for `crates/rigor/src/`).
- **REQ-032** — Bench-regression gate: criterion benchmarks vs. baseline, fail on >20% regression for the evaluator hot path.

### Phase 18 — issue #14

- **REQ-033** — `pr-injection-scan.yml` has a committed corpus of known-injection fixtures in `.github/corpus/injection/`. CI asserts each fixture triggers at least one of the 9 regex patterns OR the judge path. Run on every PR to detect regex decay.

### Phase 19 — issue #12

- **REQ-034** — `rigor-validate` CI job runs `rigor eval` against `rigor.yaml` AND replays the committed seed corpus (from REQ-013) AND cross-checks `~/.rigor/violations.jsonl` for any new violation types not declared in rigor.yaml. Fails CI if any of the three disagrees.

### Phase 20 — issue #9

- **REQ-035** — `EvaluatorPipeline` accepts a `CorpusReplay` backend alongside live evaluation. CI job runs full replay on every PR; fails on behavioral drift (verdict disagrees with recorded ground truth).

### Phase 21 — issue #26

- **REQ-036** — Test fixture compares `which rigor` vs. `cargo run --bin rigor -- --version` in CI. Fails if the PATH binary is older than the built one. Prevents the `e68c9cf` stale-binary incident.

---

## Traceability

| REQ-ID | Issue | Phase | Workstream |
|--------|-------|-------|------------|
| REQ-001..005 | #18 | 1 | phase-0-close |
| REQ-006..007 | #19 | 2 | phase-0-close |
| REQ-008..009 | #20 | 3 | phase-0-close |
| REQ-010..012 | #21 | 4 | corpus-cli |
| REQ-013 | #22 | 5 | corpus-cli |
| REQ-014 | #23 | 6 | corpus-cli |
| REQ-015..017 | #8 | 7 | test-infra |
| REQ-018 | #15 | 8 | test-infra |
| REQ-019 | #7 | 9 | coverage |
| REQ-020 | #16 | 10 | coverage |
| REQ-021 | #17 | 11 | coverage |
| REQ-022..024, REQ-025a | #24 | 12 | coverage |
| REQ-025 | #25 | 13 | coverage |
| REQ-026 | #6 | 14 | coverage |
| REQ-027 | #11, #27 | 15 | ci-hardening |
| REQ-028 | #10 | 16 | ci-hardening |
| REQ-029..032 | #13 | 17 | ci-hardening |
| REQ-033 | #14 | 18 | ci-hardening |
| REQ-034 | #12 | 19 | ci-hardening |
| REQ-035 | #9 | 20 | ci-hardening |
| REQ-036 | #26 | 21 | ci-hardening |
