---
phase: 7
slug: crates-rigor-tests-integration-test-infrastructure
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-24
---

# Phase 7 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust) |
| **Config file** | `crates/rigor-harness/Cargo.toml` |
| **Quick run command** | `cargo check -p rigor-harness && cargo test -p rigor-harness` |
| **Full suite command** | `cargo test -p rigor-harness && cargo test -p rigor --test '*'` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo check -p rigor-harness && cargo test -p rigor-harness`
- **After every plan wave:** Run `cargo test -p rigor-harness && cargo test -p rigor --test '*'`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | Status |
|---------|------|------|-------------|-----------|-------------------|--------|
| 07-01-01 | 01 | 1 | REQ-015 | unit | `cargo test -p rigor-harness --lib` | ⬜ pending |
| 07-02-01 | 02 | 1 | REQ-016 | integration | `cargo test -p rigor --test invariants` | ⬜ pending |
| 07-03-01 | 03 | 2 | REQ-017 | integration | `cargo test -p rigor-harness --test '*'` | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/rigor-harness/src/lib.rs` — module structure with IsolatedHome, TestCA, MockLlmServer, TestProxy
- [ ] `crates/rigor-harness/Cargo.toml` — dependencies wired from workspace
