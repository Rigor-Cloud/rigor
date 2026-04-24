---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: ready_to_plan
stopped_at: Completed 06-01 (pretty-printed stats table)
last_updated: "2026-04-24T15:31:20Z"
last_activity: 2026-04-24
progress:
  total_phases: 21
  completed_phases: 17
  total_plans: 25
  completed_plans: 26
  percent: 81
---

# Project State

## Current Position

Phase: 21
Plan: Not started
**Status:** Ready to plan
**Last Completed Phase:** 13 — F6 full-proxy corpus replay
**Last Activity:** 2026-04-24
**Last Activity Description:** Completed 13-01: F6 full-proxy corpus replay via MockLlmServer + TestProxy

## Milestone Overview

**Milestone:** phase-0-close-plus-coverage
**Total phases:** 21
**Workstreams:** 5 (phase-0-close, corpus-cli, test-infra, coverage, ci-hardening)
**Umbrella issue:** #28

## Progress

**Phases Complete:** 1 / 21 (Phase 1)
**Plans Complete:** 5 / 5 for Phase 1
**Phase 7 Progress:** 2 / 2 plans complete

## Active Workstream

**Name:** phase-0-close
**Path:** `.planning/workstreams/phase-0-close/`
**Phases covered:** 1 ✓, 2 (pending), 3 ✓

## Context loaded

- Codebase map: `.planning/codebase/` (built 2026-04-19)
- Knowledge graph: `.planning/graphs/graph.html` (2927 nodes / 7986 edges, built 2026-04-22)
- Original plan: `.planning/roadmap/epistemic-expansion-plan.md` (canonical source for Phases 1–3)
- PR-2.7 plan: `.planning/roadmap/pr-2.7-test-coverage-plan.md` (canonical source for Phases 4–14)

## Decisions

- No global env mutation: IsolatedHome uses Command::env() only, never std::env::set_var
- TestCA is purely in-memory, follows production rcgen pattern but skips persistence
- SSE chunk generation lives in sse.rs, shared by MockLlmServer and test assertions
- TestProxy uses spawn_blocking + env save/restore for HOME isolation during DaemonState::load
- Subprocess helpers use runtime binary discovery (not compile-time env! macro) since rigor-harness is a library
- rigor_home() panics on failure rather than returning Result to avoid cascading signature changes
- Option<PathBuf> return types preserved with Some(rigor_home()...) wrapping to minimize caller changes
- Category B HOME usages annotated with // rigor-home-ok for CI grep guard allowlisting
- rigor_home() panics on failure rather than returning Result to avoid cascading signature changes
- RIGOR_HOME set to rigor_dir_str() (the .rigor/ subdir) matching rigor_home() semantics
- CI grep guard placed as step in clippy job (not separate job) -- zero-cost grep
- JudgeClient trait tests placed in proxy.rs mod tests (not separate file) due to #[cfg(test)] visibility
- JudgeClient/JudgeError/ReqwestJudgeClient made pub (not pub(crate)) because DaemonState.judge_client field requires pub trait
- Concurrency test uses tokio::sync::Barrier + try_acquire (not proptest async wrapper)
- extract_and_evaluate and evaluate_text_inline tested indirectly through proxy_request via TestProxy because functions are private and proxy.rs modification was prohibited
- Unified RIGOR_HOME_TEST_LOCK across all test modules to prevent parallel env var races
- Arc pointer equality used to verify server_config_for_host caching behavior
- PID 2000000 as dead-PID sentinel (exceeds typical OS PID ranges)
- FailingEvaluator test-only struct verifies fail-open contract inside #[cfg(test)] module
- Instant subtraction for expired gate simulation (macOS-safe)
- TestProxy upgraded from axum::serve to hyper_util accept loop for CONNECT upgrade support
- rcgen used for PEM parsing in tests (avoids new rustls-pemfile dependency)
- MITM test uses raw TCP + TLS handshake for full pipeline validation
- Local PID_TEST_LOCK mutex in integration tests since RIGOR_HOME_TEST_LOCK is pub(crate)
- RIGOR_HOME set to tempdir root (not .rigor subdir) since rigor_home() returns env var as-is
- Body-only request tracking in ReceivedRequest (no headers) -- sufficient for B3 PII inspection
- response_sequence wraps single-chunks fallback into vec![chunks] for unified handler code path
- JSON parse failure in MockLlmServer stores Value::Null rather than panicking
- B2 retry_at_most_once uses pre-injected [RIGOR EPISTEMIC CORRECTION] marker to test already_retried guard directly
- B3 tests use stream:true (realistic SSE path) since PII-IN runs on request path regardless of streaming mode
- Per-file ENV_LOCK mutex for RIGOR_NO_RETRY rather than cross-file shared mutex
- Bench smoke test uses --help instead of full criterion run to keep tests fast
- YAML suite loading deferred; --suite prints message and runs built-in scenarios
- Report skipped count derived from total minus pass minus fail (forward-compatible)
- cargo-deny 0.19.x format used; 11 known advisories temporarily ignored with tracking reasons
- License allow-list extended to 13 entries based on actual dep tree (MIT-0, Unicode-3.0, MPL-2.0, CDLA-Permissive-2.0 added)
- Keyless OIDC signing (no private keys) via GitHub Actions id-token: write permission
- Extended cli/refine.rs in-place for corpus exporter rather than creating module directory
- CLI grammar: rigor refine --apply becomes rigor refine suggest --apply (pre-1.0 acceptable)
- Used serde_json::json! for stats output since ModelStats/PerModelAggregate lack Serialize derive
- Validate uses sample.model (original unslugged) for hash recomputation, not reversed slug
- ureq (sync) backend for hf-hub: InferenceHost::load is synchronous, no async complexity
- tls-native for ort build-script downloads: separate from runtime rustls
- ndarray 0.17 (not 0.16): ort 2.0.0-rc.12 requires ^0.17
- Content-addressed model cache: <rigor_home>/models/<sha256>/<filename>
- No external table crate for stats output -- format! with dynamic widths keeps deps minimal
- Default stats output changed from JSON to table (TTY-friendly); --format json preserves backward compatibility
- F6 proxy replay uses focused constraint set (rust-no-gc only) for debug-mode performance; full 53-constraint set tested via corpus_replay.rs PolicyEngine path
- F6 proxy replay omits x-api-key to prevent async LLM-as-judge from consuming MockLlmServer response_sequence entries
- Default 80-recording smoke mode (1 per prompt/model pair); RIGOR_FULL_CORPUS=1 for all 800

## Session Continuity

**Stopped At:** Completed 13-01 (F6 full-proxy corpus replay)
**Resume File:** None

**Planned Phase:** 13 complete (F6-full-proxy-corpus-replay) -- 1/1 plans -- 2026-04-24
