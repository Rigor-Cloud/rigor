---
phase: 03-pr-5-onnx-host
plan: 01
subsystem: infra
tags: [onnx, ort, hf-hub, ndarray, feature-flag, inference, ml]

# Dependency graph
requires:
  - phase: none
    provides: none (standalone infra addition)
provides:
  - OnnxModelHost struct for local ONNX model caching
  - InferenceHost trait for backend-agnostic inference
  - onnx / onnx-cuda / onnx-coreml Cargo feature flags
affects: [1D-kompress, 4F-safety-discriminator]

# Tech tracking
tech-stack:
  added: [ort 2.0.0-rc.12, hf-hub 0.5, ndarray 0.17]
  patterns: [optional-dep feature gating, content-addressed model cache, trait-based inference abstraction]

key-files:
  created:
    - crates/rigor/src/memory/onnx_host.rs
  modified:
    - crates/rigor/Cargo.toml
    - crates/rigor/src/memory/mod.rs

key-decisions:
  - "ureq (sync) backend for hf-hub instead of tokio — InferenceHost::load is synchronous"
  - "tls-native for ort build-script downloads — separate from project's runtime rustls usage"
  - "ndarray 0.17 (not 0.16) to match ort 2.x compatibility requirement"
  - "Content-addressed cache keyed by SHA-256 — <cache_dir>/<sha256>/<filename>"

patterns-established:
  - "Feature-gated modules: #[cfg(feature = \"onnx\")] pub mod onnx_host in memory/mod.rs"
  - "InferenceHost trait: sync load() returning PathBuf — consumers depend on trait not runtime"

requirements-completed: [REQ-008, REQ-009]

# Metrics
duration: 6min
completed: 2026-04-24
---

# Phase 3: ONNX Host (feature-flagged) Summary

**InferenceHost trait + OnnxModelHost with HF Hub download and SHA-256 verified content-addressed cache behind `onnx` feature flag**

## Performance

- **Duration:** 6 min
- **Started:** 2026-04-24T08:18:56Z
- **Completed:** 2026-04-24T08:25:35Z
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments
- Added `onnx`, `onnx-cuda`, `onnx-coreml` feature flags to Cargo.toml with ort 2.0.0-rc.12, hf-hub 0.5, ndarray 0.17 as optional deps
- Created `InferenceHost` trait (REQ-009) separating ONNX runtime from consumers
- Implemented `OnnxModelHost` with HuggingFace Hub sync download, SHA-256 verification, and content-addressed local cache
- 9 unit tests covering cache hit, SHA mismatch, missing file, corrupt cache detection, trait object safety, and cache directory creation
- Zero impact on default build: `cargo check -p rigor` and all 380 default tests pass unchanged

## Task Commits

Each task was committed atomically:

1. **Task 1: Cargo.toml feature flags** - `43ce633` (chore)
2. **Task 2+3: OnnxModelHost + mod.rs wiring** - `adf6dba` (feat)

## Files Created/Modified
- `crates/rigor/Cargo.toml` - Added ort, hf-hub, ndarray optional deps + [features] section
- `crates/rigor/src/memory/onnx_host.rs` - OnnxModelHost struct, InferenceHost trait, 9 unit tests
- `crates/rigor/src/memory/mod.rs` - Feature-gated module declaration

## Decisions Made
- Used ureq (sync) backend for hf-hub: InferenceHost::load is a synchronous function, avoiding unnecessary async complexity for a blocking I/O operation
- Used tls-native for ort build-script downloads: the build script downloads ONNX Runtime binaries at compile time, separate concern from the project's runtime rustls TLS
- Bumped ndarray to 0.17 (not 0.16 as initially planned): ort 2.0.0-rc.12 requires ndarray ^0.17
- Content-addressed cache layout: `<rigor_home>/models/<sha256>/<filename>` -- immutable by design, corrupt files auto-detected and re-downloaded

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed ndarray version for ort 2.x compatibility**
- **Found during:** Task 2 (cargo check --features onnx)
- **Issue:** Plan specified ndarray 0.16, but ort 2.0.0-rc.12 requires ndarray ^0.17
- **Fix:** Changed ndarray dep to version 0.17
- **Files modified:** crates/rigor/Cargo.toml
- **Verification:** cargo check -p rigor --features onnx passes
- **Committed in:** adf6dba

**2. [Rule 3 - Blocking] Added tls-native feature for ort download-binaries**
- **Found during:** Task 2 (cargo check --features onnx)
- **Issue:** ort's download-binaries feature requires a TLS provider; disabling default-features removed tls-native
- **Fix:** Explicitly enabled tls-native feature on ort dep
- **Files modified:** crates/rigor/Cargo.toml
- **Verification:** cargo check -p rigor --features onnx passes
- **Committed in:** adf6dba

**3. [Rule 3 - Blocking] Switched hf-hub from tokio to ureq feature**
- **Found during:** Task 2 (cargo check --features onnx)
- **Issue:** hf_hub::api::sync::Api requires ureq feature, not tokio
- **Fix:** Changed hf-hub features from ["tokio"] to ["ureq"]
- **Files modified:** crates/rigor/Cargo.toml
- **Verification:** cargo check -p rigor --features onnx passes
- **Committed in:** adf6dba

---

**Total deviations:** 3 auto-fixed (3 blocking issues)
**Impact on plan:** All auto-fixes necessary for compilation. No scope creep.

## Issues Encountered
None beyond the auto-fixed blocking issues above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- ONNX host infrastructure ready for Phase 1D (Kompress) and Phase 4F (Safety Discriminator)
- Consumers implement `InferenceHost` trait or use `OnnxModelHost` directly
- GPU acceleration available via `--features onnx-cuda` or `--features onnx-coreml`

---
*Phase: 03-pr-5-onnx-host*
*Completed: 2026-04-24*
