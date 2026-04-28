# Phase 3: PR-5 — ONNX host (feature-flagged) - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Add optional ONNX runtime behind a feature flag. Shared infra for Phase 1D Kompress and Phase 4F ModernBERT. Zero impact on default build.

Requirements: REQ-008, REQ-009

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All at Claude's discretion. Per issue #20:
- New memory/onnx_host.rs behind #[cfg(feature = "onnx")]
- ort crate as optional dep with cuda/coreml sub-features
- OnnxModelHost with load_from_hf(model_id, sha256) + local cache
- Cache at rigor_home()/models/<sha256>/
- Over-editing guard: new file + Cargo.toml feature additions only

</decisions>

<code_context>
## Existing Code Insights

- memory/ module exists with episodic.rs, content_store.rs
- rigor_home() for path resolution (Phase 8)
- Feature flags pattern: not yet used in this crate

</code_context>

<specifics>
## Specific Ideas

CPU-only first; GPU features as sub-features.

</specifics>

<deferred>
## Deferred Ideas

- Specific model loading (Kompress, ModernBERT) — future phases
- GPU tuning — defer

</deferred>
