# Phase 10: Unit Coverage Gaps - Research

**Researched:** 2026-04-24
**Domain:** Rust unit testing, coverage gap closure (daemon, TLS, DF-QuAD, claim pipeline, content store, action gates)
**Confidence:** HIGH

## Summary

Phase 10 closes 10 specific unit-test gaps listed in GitHub issue #16. All targets are existing Rust modules within `crates/rigor/src/`. The phase is purely additive: only `#[cfg(test)]` test functions are added, with zero production code changes unless a test seam is structurally necessary (and the CONTEXT.md over-editing guard discourages even that).

Every target module was read in full. Existing test counts, function signatures, visibility, and testability constraints were analyzed for each gap. The codebase currently has 323 passing unit tests. All modules compile and pass. Key risk areas include the MITM allowlist test depending on a global `AtomicBool`, the daemon lifecycle tests needing filesystem isolation (RIGOR_HOME), and the TLS CA tests needing rcgen without disk persistence.

**Primary recommendation:** Organize work into 3-4 waves by module cluster. Most tests are straightforward `#[test]` additions inside existing `mod tests` blocks; the daemon/mod.rs (MITM + PID lifecycle) is the only module without an existing test block. Tests for TLS CA generation should use `tempfile::TempDir` + `RIGOR_HOME` env var for isolation.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions
None explicitly locked. All implementation at Claude's discretion.

### Claude's Discretion
All implementation at Claude's discretion. Key gaps from GitHub issue #16:

1. `should_mitm_target` -- MITM allowlist tests
2. `daemon_alive` / `write_pid_file` / `remove_pid_file` -- daemon lifecycle
3. `RigorCA::load_or_generate` / `server_config_for_host` / `install_ca_trust` -- TLS CA
4. `peek_client_hello` -- SNI edge cases
5. Evaluator fail-open on error
6. `compute_strengths` DF-QuAD boundaries (MAX_ITERATIONS, BTreeMap guard)
7. `SeverityThresholds` boundary arithmetic (0.7/0.4 boundaries)
8. `claim/heuristic.rs` pipeline ordering test
9. `memory::content_store` TTL + concurrency
10. `daemon/gate_api.rs` action gate tests

Over-editing guard: ONLY add tests. No production code changes unless absolutely needed for testability. No refactoring.

### Deferred Ideas (OUT OF SCOPE)
None.

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REQ-020 | Unit tests exist for: MITM allowlist matching, daemon lifecycle (start/stop/PID file), TLS CA generation + leaf cert signing, SNI extraction, DF-QuAD boundary cases (single-attacker dominance, zero-attacker), SeverityThresholds comparison at exact thresholds (0.7, 0.4), content_store TTL eviction behavior, action gate timeout (60s) | All 10 target modules analyzed; existing test counts, function signatures, and missing coverage documented in Architecture Patterns section |

</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| MITM allowlist matching | Daemon (proxy routing) | -- | `should_mitm_target` decides CONNECT tunnel routing |
| Daemon lifecycle (PID) | Daemon (process mgmt) | Filesystem | PID file write/read/remove on disk |
| TLS CA generation | Daemon (TLS) | Filesystem | rcgen CA cert persisted to `~/.rigor/` |
| SNI extraction | Daemon (TLS) | -- | In-memory byte parsing, no disk |
| Evaluator fail-open | Evaluator pipeline | -- | Error handling in ClaimEvaluator trait impls |
| DF-QuAD strengths | Constraint graph | -- | Pure math: fixed-point iteration |
| SeverityThresholds | Violation types | -- | Pure function: f64 comparison |
| Claim heuristic pipeline | Claim extraction | -- | Text processing pipeline ordering |
| Content store TTL | Memory/cache | -- | moka::sync::Cache TTL behavior |
| Action gate timeout | Daemon (gate API) | -- | Gate state management + cleanup |

## Standard Stack

### Core (already in project -- no new dependencies)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `tempfile` | 3.x | Isolated directories for PID/CA tests | Already in dev-dependencies [VERIFIED: Cargo.toml] |
| `tokio` | 1.x | Async runtime for content_store + SNI tests | Already in dependencies [VERIFIED: Cargo.toml] |
| `rcgen` | 0.13 | CA cert generation (production code uses it) | Already in dependencies [VERIFIED: Cargo.toml] |
| `moka` | 0.12 | sync::Cache TTL eviction | Already in dependencies [VERIFIED: Cargo.toml] |
| `rigor-harness` | path dep | IsolatedHome, TestCA fixtures | Already in dev-dependencies [VERIFIED: Cargo.toml] |

### No New Dependencies Needed
Every test in this phase uses libraries already declared in `Cargo.toml`. No additions required. [VERIFIED: Cargo.toml lines 75-79]

## Architecture Patterns

### Gap-by-Gap Analysis

Each gap is analyzed with: existing test count, function signature, what is missing, and the exact tests to add.

---

#### Gap 1: `should_mitm_target` (daemon/mod.rs:122)

**File:** `crates/rigor/src/daemon/mod.rs`
**Existing test module:** NONE -- daemon/mod.rs has no `#[cfg(test)] mod tests` block [VERIFIED: grep]
**Existing tests:** 0

**Function signature:**
```rust
pub fn should_mitm_target(target: &str) -> bool
```

**Dependency:** Reads `ws::is_mitm_enabled()` which checks a global `AtomicBool`. Also iterates `MITM_HOSTS` array with exact match and `.` suffix match.

**What is missing:**
- Exact host match (`api.anthropic.com:443` -> true when MITM enabled)
- Suffix match (`us-east5-aiplatform.googleapis.com:443` -> true)
- Non-LLM host (`github.com:443` -> false)
- Partial suffix mismatch (e.g., `notapi.anthropic.com:443` should match because it ends_with `.api.anthropic.com`)
- Port-only target (just `host` without `:port`)
- Empty target
- MITM disabled returns false regardless of host

**Testability concern:** `is_mitm_enabled()` reads a global `AtomicBool` (`ws::MITM_ENABLED`). Tests must call `ws::set_mitm_enabled(true/false)` and are NOT safe to run in parallel with other MITM-dependent tests. Use `serial_test` or a test-local mutex. However, since `serial_test` is not in dev-dependencies and this is the only module needing it, use a module-local `Mutex` (same pattern as `paths.rs` tests). [VERIFIED: paths.rs lines 28-29]

---

#### Gap 2: Daemon PID lifecycle (daemon/mod.rs:32,45,58)

**File:** `crates/rigor/src/daemon/mod.rs`
**Existing test module:** NONE
**Existing tests:** 0

**Function signatures:**
```rust
pub fn daemon_pid_file() -> Option<PathBuf>        // line 24
pub fn write_pid_file() -> std::io::Result<()>      // line 32
pub fn remove_pid_file()                             // line 45
pub fn daemon_alive() -> bool                        // line 58
```

**Dependency:** All call `daemon_pid_file()` which calls `crate::paths::rigor_home()`. This reads `RIGOR_HOME` env var. Tests MUST set `RIGOR_HOME` to a temp dir. Must serialize with a Mutex since `std::env::set_var` is process-global.

**What is missing:**
- `write_pid_file` creates file with current PID content
- `remove_pid_file` deletes the file
- `daemon_alive` returns false when no PID file exists
- `daemon_alive` returns true for current process PID (write self, check)
- `daemon_alive` returns false for stale/non-existent PID
- `daemon_alive` returns false for garbage content in PID file

**Testability concern:** `daemon_alive` calls `libc::kill(pid, 0)` in an unsafe block. For "alive" test, use current process PID. For "dead" test, use a PID that is guaranteed dead (e.g., PID 2000000 which exceeds typical PID ranges, or write a non-numeric string).

---

#### Gap 3: TLS CA (daemon/tls.rs:42, :118, :176)

**File:** `crates/rigor/src/daemon/tls.rs`
**Existing test module:** NONE [VERIFIED: grep]
**Existing tests:** 0

**Function signatures:**
```rust
// RigorCA methods
pub fn load_or_generate() -> Result<Self>                           // line 38
pub fn server_config_for_host(&self, hostname: &str) -> Result<Arc<ServerConfig>>  // line 114
pub fn ca_cert_path(&self) -> PathBuf                               // line 165

// Standalone functions
pub fn install_ca_trust() -> Result<()>                             // line 172
pub fn generate_tls_config(hosts: &[&str]) -> Result<ServerConfig>  // line 234
```

**What is missing:**
- `load_or_generate`: generates new CA when files don't exist
- `load_or_generate`: loads existing CA from disk when files exist (roundtrip)
- `server_config_for_host`: generates valid ServerConfig for a hostname
- `server_config_for_host`: caches configs (second call returns cached)
- `generate_tls_config`: creates self-signed cert for given hosts
- `install_ca_trust`: fails with clear error when CA cert doesn't exist

**Testability concern:** `load_or_generate` uses `crate::paths::rigor_home()` for CA file paths. Tests must set `RIGOR_HOME` to a temp dir. `install_ca_trust` calls `security` CLI (macOS keychain) -- test only the "cert missing" error path; do NOT actually install anything. Must serialize tests that set RIGOR_HOME with a Mutex (share the same mutex as Gap 2 tests since they are in the same module's test block or a sibling module).

**Recommendation:** Add `#[cfg(test)] mod tests` to `daemon/tls.rs`. Tests for `load_or_generate` and `server_config_for_host` use `tempfile::TempDir` + `RIGOR_HOME` env var. The `install_ca_trust` test only exercises the "cert missing" error path.

---

#### Gap 4: `peek_client_hello` (daemon/sni.rs:15)

**File:** `crates/rigor/src/daemon/sni.rs`
**Existing test module:** YES (lines 169-214) [VERIFIED: source]
**Existing tests:** 2 (`test_parse_sni_minimal`, `test_parse_sni_returns_none_for_non_clienthello`)

**Function signatures:**
```rust
pub async fn peek_client_hello<R: AsyncRead + Unpin>(stream: &mut R) -> io::Result<(Vec<u8>, Option<String>)>
fn parse_sni_from_client_hello(data: &[u8]) -> Option<String>  // private
```

**What is missing per issue #16:**
- Fragmented TLS record (record_len > 16KB -> rejection)
- ALPN extension present alongside SNI (verify SNI still extracted)
- Missing SNI extension (extensions present but no type 0x0000)
- Truncated data (various truncation points)
- `peek_client_hello` async test: feed bytes through `tokio::io::duplex` or `std::io::Cursor` adapted to AsyncRead

**Testability:** `parse_sni_from_client_hello` is private but accessible from within the `mod tests` block. `peek_client_hello` is async and generic over `AsyncRead + Unpin`, so `tokio::io::Cursor` works directly.

---

#### Gap 5: Evaluator fail-open on error

**File:** `crates/rigor/src/evaluator/pipeline.rs`
**Existing test module:** YES (lines 371-611) [VERIFIED: source]
**Existing tests:** 10

**What is missing per issue #16:**
- Force `RegexEvaluator` to error, assert it returns `EvalResult::allow` (fail-open)
- Force `SemanticEvaluator` to error (e.g., lookup panics), assert allow
- Pipeline-level: all evaluators error -> still returns allow

**How to force errors:**
- `RegexEvaluator`: construct with a `PolicyEngine` whose `evaluate` returns `Err`. The simplest approach: build a `RegexEvaluator` with a valid config, then call `evaluate` with a claim + a constraint whose `id` does not match any loaded rule. But that produces `Ok([])` not `Err`. A more reliable approach: implement a custom `ClaimEvaluator` that always returns error. Actually, looking at the code more carefully: `RegexEvaluator::evaluate` line 131 calls `engine.evaluate(&input)` which can return `Err` if Rego parsing fails. The existing `make_config`+`RegexEvaluator::new(&config)` pattern creates a valid engine. To make it error, we need to construct a `PolicyEngine` with invalid Rego. But `PolicyEngine::new` skips invalid constraints at init time, so the engine itself is valid; `evaluate` only errors on regorus internal failures.
- Best approach: create a custom `ClaimEvaluator` impl that returns `EvalResult::allow` with an error message, verifying the pipeline contract. The fail-open pattern is documented in the trait doc comment (line 76: "On internal error, prefer returning an `allow` result").

**Recommendation:** The fail-open tests should verify:
1. `RegexEvaluator` returns allow when engine.evaluate returns Err (need a way to trigger this -- possibly with an empty/invalid Rego that passes `new` but fails `evaluate`)
2. Pipeline `evaluate_claim` with no matching evaluators returns allow (already tested by `empty_pipeline_without_fallback_is_permissive`)
3. Add a test with a `struct FailingEvaluator` that always returns allow-with-error-reason, confirming the fail-open contract

---

#### Gap 6: `compute_strengths` DF-QuAD boundaries (constraint/graph.rs:82)

**File:** `crates/rigor/src/constraint/graph.rs`
**Existing test module:** YES (lines 191-562) [VERIFIED: source]
**Existing tests:** 13

**What is missing per issue #16:**
- `MAX_ITERATIONS` exhaustion: create a graph that does NOT converge within 100 iterations, assert `compute_strengths()` returns `Err`
- Zero-attacker case: node with no attackers and no supporters retains base_strength (covered by `test_no_relations_retains_base_strength` but issue wants explicit zero-attacker naming)
- Single-attacker dominance: already covered by `test_dfquad_golden_single_attack` but issue wants verification that a single strong attacker (base 0.9) against a weak node (base 0.7) drives strength near zero
- BTreeMap determinism guard: two graphs with same nodes added in different order produce identical strengths

**Testability:** All pure functions, no side effects. Easy to test. The MAX_ITERATIONS non-convergence test is the hardest -- need to construct a graph that oscillates. However, the DF-QuAD formula with product aggregation is mathematically convergent for finite graphs. A near-non-convergent case: large cycle with near-unity strengths. May need to temporarily reduce EPSILON or set up a pathological case. Actually, examining the code: `MAX_ITERATIONS` is 100, `EPSILON` is 0.001. For a mutual-attack cycle with equal strengths, convergence is fast (4/9 fixed point). A 3-node cycle might take more iterations. Actually, it is difficult to construct a genuinely non-convergent graph. The test may need to verify that the error message is correct by using a mock, but since this is a unit test of the actual code and the formula IS convergent... We should test the error path structurally by confirming the error message string is correct when bailout occurs. One approach: test that after 100 iterations with EPSILON=0.001, the specific converged-or-not result is correct.

**Recommendation:** Add tests for:
1. BTreeMap ordering determinism (add nodes in different orders, verify identical outputs)
2. Single strong attacker (Justification base=0.9 attacking Defeater base=0.7)
3. Verify `MAX_ITERATIONS` constant is 100 and `EPSILON` is 0.001 (assertion tests documenting the constants)

---

#### Gap 7: `SeverityThresholds` boundary arithmetic (violation/types.rs)

**File:** `crates/rigor/src/violation/types.rs`
**Existing test module:** YES (lines 55-94) [VERIFIED: source]
**Existing tests:** 6

**What is already covered:**
- Exactly at block (0.7) -> Block
- Just below block (0.6999999) -> Warn
- Exactly at warn (0.4) -> Warn
- Just below warn (0.3999999) -> Allow
- Zero -> Allow
- One -> Block

**What is missing per issue #16:**
- The issue description says "0.7/0.4 boundaries with >= operator" -- the existing tests already cover this comprehensively. REQ-020 says "SeverityThresholds comparison at exact thresholds (0.7, 0.4)" -- also covered.
- Possible additions: custom thresholds (non-default), negative strength, NaN behavior, very large strength (>1.0)

**Recommendation:** This gap is ALREADY WELL-COVERED. Add 1-2 marginal tests for completeness: custom thresholds test and a midpoint test (0.55 -> Warn).

---

#### Gap 8: `claim/heuristic.rs` pipeline ordering

**File:** `crates/rigor/src/claim/heuristic.rs`
**Existing test module:** YES (lines 214-387) [VERIFIED: source]
**Existing tests:** 18

**What is missing per issue #16:**
The issue asks for a test verifying the pipeline ordering: `strip_code_blocks -> unicode_sentences -> is_assertion -> !is_hedged -> classify`

**What is already covered:**
- `test_extract_full_pipeline`: verifies end-to-end (assertion + hedge filtering)
- `test_extract_filters_hedged`: verifies hedge filtering in pipeline
- `test_extract_strips_code`: verifies code block stripping
- `test_extract_source_location`: verifies source location tracking
- Individual function tests for `is_assertion`, `classify_claim_type`, etc.

**What is missing:** An explicit ordering test that verifies a hedged sentence inside a code block is properly handled (strip_code_blocks removes it before is_assertion sees it). Also: a sentence that would fail assertion check but pass hedge check -- order matters.

**Recommendation:** Add 1-2 tests verifying ordering interaction:
1. Code block containing a hedged claim -- should produce zero claims (code stripping happens first)
2. A sentence that passes assertion but fails hedge -- should not appear in output
3. Verify `is_action_intent` takes priority over `classify_claim_type` in the pipeline

---

#### Gap 9: `memory::content_store` TTL + concurrency

**File:** `crates/rigor/src/memory/content_store.rs`
**Existing test module:** YES (lines 311-529) [VERIFIED: source]
**Existing tests:** 4 sync + 10 async = 14 total (via `#[tokio::test]`)

**What is already covered:**
- `compression_ttl_evicts_after_deadline`: proves TTL eviction works
- `audit_does_not_evict`: proves DashMap-backed categories are permanent
- Store/retrieve roundtrip, missing retrieval, category isolation
- Search with substring matching

**What is missing per issue #16:**
- Concurrency test: multiple concurrent stores + retrieves don't corrupt
- Verdict TTL eviction (only compression TTL is tested, not verdict)
- Concurrent store + TTL eviction race
- `with_ttls` test helper exists but only used in 2 tests

**Recommendation:** Add tests for:
1. Verdict TTL eviction (parallel to `compression_ttl_evicts_after_deadline`)
2. Concurrent store from multiple tasks (use `tokio::spawn` + `tokio::sync::Barrier`)
3. Concurrent retrieve during TTL window (store, spawn readers, wait past TTL, assert None)

---

#### Gap 10: `daemon/gate_api.rs` action gate tests

**File:** `crates/rigor/src/daemon/gate_api.rs`
**Existing test module:** YES (lines 145-199) [VERIFIED: source]
**Existing tests:** 5

**Also related:** `crates/rigor/src/daemon/gate.rs` -- NO test module [VERIFIED: grep]

**What is already covered in gate_api.rs:**
- `compute_decision_response` for all 4 (decision, snapshot) combinations
- no_session, pending, approved, rejected status strings
- snapshot_id and affected_paths propagation

**What is missing per issue #16:**
- Action gate timeout (60s): `cleanup_expired_gates` auto-rejects after `GATE_TIMEOUT_SECS`
- `apply_decision` sends oneshot channel signal
- `create_realtime_gate` returns a oneshot receiver
- Gate state lifecycle: create -> approve/reject -> decision stored

**Where to put tests:** `gate.rs` has no test module. Since `cleanup_expired_gates`, `apply_decision`, and `create_realtime_gate` are all in `gate.rs`, add `#[cfg(test)] mod tests` to `gate.rs`. These functions take `SharedState` as input, which requires constructing a `DaemonState`. DaemonState::load needs a real rigor.yaml and calls `rigor_home()`. Use `DaemonState::empty(event_tx)` which is simpler (no yaml needed) but still calls `rigor_home()` for CA generation (which can fail gracefully). Alternative: construct `SharedState` directly with `Arc::new(Mutex::new(...))` using a manually-constructed DaemonState. But DaemonState has many fields.

**Testability concern:** Constructing a `SharedState` for gate tests. The simplest approach: `DaemonState::empty(event_tx)` requires a broadcast channel and calls TLS/CA code that touches `rigor_home()`. Set `RIGOR_HOME` to tempdir. Or: test `cleanup_expired_gates` indirectly by building the shared state manually. Actually, examining the code more carefully:

- `cleanup_expired_gates` takes `&SharedState` and only accesses `action_gates` (a HashMap)
- `apply_decision` takes `&SharedState` and accesses `action_gates` + `gate_decisions`
- `create_realtime_gate` takes `&SharedState` and inserts into `action_gates`

So the test needs a `SharedState` with just those fields populated. The cleanest approach is to use `DaemonState::empty(event_tx)` with RIGOR_HOME set to a tempdir, accepting that TLS/CA init happens as a side effect. This is what the rest of the test suite does.

**Recommendation:** Add `#[cfg(test)] mod tests` to `daemon/gate.rs` with tests for:
1. `cleanup_expired_gates` removes gates older than 60s and auto-rejects via oneshot
2. `apply_decision` with approved=true sends true on oneshot
3. `apply_decision` with approved=false sends false on oneshot
4. `apply_decision` on non-existent gate returns Err
5. `create_realtime_gate` returns a valid oneshot Receiver

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Temp directories for PID/CA tests | Manual mkdtemp + cleanup | `tempfile::TempDir` (auto-drops) | Already in dev-deps, RAII cleanup |
| TLS cert generation for tests | Manual openssl calls | `rigor-harness::TestCA` or direct rcgen | Already in test harness |
| Async test runtime | Manual Runtime::new() | `#[tokio::test]` macro | Standard, already used throughout |
| ENV var isolation | Manual save/restore | Mutex + save/restore pattern from `paths.rs` | Proven pattern in codebase |

## Common Pitfalls

### Pitfall 1: Global MITM_ENABLED AtomicBool Race
**What goes wrong:** Tests that call `set_mitm_enabled(true)` race with other tests reading the global.
**Why it happens:** `MITM_ENABLED` is a process-global `AtomicBool`. Rust runs unit tests in parallel by default.
**How to avoid:** Use a `static Mutex` guard in the test module. Group all MITM-dependent tests behind the same mutex.
**Warning signs:** Flaky test results that differ between `cargo test` and `cargo test -- --test-threads=1`.

### Pitfall 2: RIGOR_HOME env var Pollution
**What goes wrong:** Tests that set `RIGOR_HOME` to a tempdir interfere with other tests reading `rigor_home()`.
**Why it happens:** `std::env::set_var` is process-global and unsafe in Rust 2024+.
**How to avoid:** Use a module-local `Mutex` (same pattern as `paths.rs` lines 28-29). Always save+restore the original value.
**Warning signs:** PID file tests finding unexpected files, CA tests finding pre-existing certs.

### Pitfall 3: moka TTL Eviction Timing
**What goes wrong:** TTL eviction test passes locally but fails in CI due to timing.
**Why it happens:** moka evicts lazily on access. The test sleeps for 120ms after a 50ms TTL, but CI machines may be slower.
**How to avoid:** Use generous sleep margins (2-3x TTL). The existing `compression_ttl_evicts_after_deadline` test uses 50ms TTL + 120ms sleep, which works. Follow this pattern.
**Warning signs:** Test timeouts or flaky eviction assertions.

### Pitfall 4: Constructing DaemonState for Gate Tests
**What goes wrong:** Tests construct `DaemonState` directly, hitting many required fields.
**Why it happens:** `DaemonState` has 25+ fields including policy_engine, TLS configs, etc.
**How to avoid:** Use `DaemonState::empty(event_tx)` which has reasonable defaults. Set `RIGOR_HOME` to tempdir first so CA/TLS init doesn't pollute real HOME.
**Warning signs:** Test constructors becoming longer than the test logic.

### Pitfall 5: PID File Test on CI (Linux vs macOS)
**What goes wrong:** `daemon_alive` uses `libc::kill(pid, 0)` which has platform differences in error codes.
**Why it happens:** On Linux, `kill(pid, 0)` returns ESRCH for non-existent PIDs and EPERM for other-user PIDs. On macOS, behavior is similar but edge cases differ.
**How to avoid:** Test with the current process PID (known alive) and with PID `i32::MAX` (known dead). Don't rely on specific errno values -- just check the boolean return.
**Warning signs:** Tests pass on macOS but fail on Linux or vice versa.

## Code Examples

### Pattern: ENV Var Isolation with Mutex
```rust
// Source: crates/rigor/src/paths.rs lines 28-45 [VERIFIED: source]
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_needing_env_var() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("RIGOR_HOME").ok();
        let tmp = tempfile::TempDir::new().unwrap();
        unsafe { std::env::set_var("RIGOR_HOME", tmp.path().join(".rigor")) };

        // ... test logic ...

        // Restore
        match original {
            Some(v) => unsafe { std::env::set_var("RIGOR_HOME", v) },
            None => unsafe { std::env::remove_var("RIGOR_HOME") },
        }
    }
}
```

### Pattern: Constructing SharedState for Gate Tests
```rust
// Source: daemon/mod.rs DaemonState::empty [VERIFIED: source lines 322-375]
use tokio::sync::broadcast;
use std::sync::{Arc, Mutex};

fn test_shared_state() -> (SharedState, ws::EventSender) {
    let (event_tx, _rx) = ws::create_event_channel();
    let state = DaemonState::empty(event_tx.clone()).unwrap();
    (Arc::new(Mutex::new(state)), event_tx)
}
```

### Pattern: Async TTL Eviction Test
```rust
// Source: content_store.rs lines 433-448 [VERIFIED: source]
#[tokio::test]
async fn compression_ttl_evicts_after_deadline() {
    let store = InMemoryBackend::with_ttls(
        std::time::Duration::from_millis(50),
        std::time::Duration::from_secs(60),
    );
    let hash = store.store(b"short-lived".to_vec(), Category::Compression, None, None)
        .await.unwrap();
    assert!(store.retrieve(&hash).await.unwrap().is_some());

    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    assert!(store.retrieve(&hash).await.unwrap().is_none());
}
```

### Pattern: ClientHello byte construction for SNI tests
```rust
// Source: daemon/sni.rs lines 173-207 [VERIFIED: source]
// Build a minimal TLS ClientHello with SNI
fn build_client_hello_with_sni(hostname: &str) -> Vec<u8> {
    let mut data = Vec::new();
    data.push(0x01); // ClientHello type
    data.extend_from_slice(&[0, 0, 0]); // length placeholder
    data.extend_from_slice(&[0x03, 0x03]); // TLS 1.2 version
    data.extend_from_slice(&[0u8; 32]); // random
    data.push(0); // session_id length = 0
    data.extend_from_slice(&[0, 2]); // cipher suites length
    data.extend_from_slice(&[0xc0, 0x2f]); // one cipher
    data.push(1); // compression methods length
    data.push(0); // null compression

    // Build SNI extension
    let host = hostname.as_bytes();
    let mut sni_ext = Vec::new();
    sni_ext.extend_from_slice(&[0, 0]); // ext type = SNI
    let sni_data_len = 2 + 1 + 2 + host.len();
    sni_ext.extend_from_slice(&(sni_data_len as u16).to_be_bytes());
    sni_ext.extend_from_slice(&((1 + 2 + host.len()) as u16).to_be_bytes());
    sni_ext.push(0); // name_type = host_name
    sni_ext.extend_from_slice(&(host.len() as u16).to_be_bytes());
    sni_ext.extend_from_slice(host);

    data.extend_from_slice(&(sni_ext.len() as u16).to_be_bytes());
    data.extend_from_slice(&sni_ext);
    data
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `std::env::set_var` (safe) | `unsafe { std::env::set_var }` | Rust 1.83 (2024-11) | All env mutation must be in unsafe blocks [VERIFIED: paths.rs uses unsafe] |
| `moka` lazy eviction | Same | Current | TTL tests must trigger access after sleep to force eviction [VERIFIED: content_store.rs tests] |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | PID `i32::MAX` is not a real running process on test machines | Gap 2 analysis | daemon_alive test would produce false positive; use a more defensive approach like checking a recently-dead PID |
| A2 | DF-QuAD with product aggregation is mathematically convergent for all finite graphs | Gap 6 analysis | Cannot construct a non-convergent test case; may need to verify the error path differently |

## Open Questions

1. **MAX_ITERATIONS exhaustion test**
   - What we know: DF-QuAD with product aggregation converges for finite graphs. EPSILON=0.001 and MAX_ITERATIONS=100.
   - What's unclear: Whether it is possible to construct a graph that genuinely does not converge in 100 iterations with this formula.
   - Recommendation: Test the constant values and the error message format. If a non-convergent case cannot be constructed, document this as a property of the algorithm and add a comment explaining why the error path exists (defense against floating-point edge cases or future formula changes).

2. **Gate timeout test approach**
   - What we know: `cleanup_expired_gates` checks `Instant::now().duration_since(entry.created_at) > cutoff`. The cutoff is 60 seconds.
   - What's unclear: How to simulate passage of 60+ seconds without sleeping in tests.
   - Recommendation: The `ActionGateEntry.created_at` is `std::time::Instant`, which cannot be mocked. However, we can construct an entry with `created_at` set to `Instant::now() - Duration::from_secs(61)` via `Instant::now().checked_sub(Duration::from_secs(61))` (available since Rust 1.79). If `checked_sub` is not available, use a short timeout approach: modify the `created_at` field directly since it is `pub`. Actually, `Instant` does not support subtraction-to-past on all platforms. The simplest approach: `Instant::now() - Duration::from_secs(61)` (the `Sub<Duration>` impl panics if result would be before the epoch on some platforms). Alternative: directly set `created_at` to an instant from 61 seconds ago using the platform clock. The safest approach is to use `Instant::now()` and then `std::thread::sleep(Duration::from_millis(10))` with a modified GATE_TIMEOUT_SECS -- but the constant is not configurable. Best approach: set `created_at` = `Instant::now() - Duration::from_secs(61)`. On macOS (the target platform), `Instant` uses `mach_absolute_time` and subtraction works correctly.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `#[tokio::test]` |
| Config file | Cargo.toml `[dev-dependencies]` |
| Quick run command | `cargo test --lib -p rigor` |
| Full suite command | `cargo test -p rigor` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REQ-020.1 | MITM allowlist matching | unit | `cargo test --lib -p rigor daemon::tests::` | No -- Wave 0 |
| REQ-020.2 | Daemon lifecycle PID | unit | `cargo test --lib -p rigor daemon::tests::` | No -- Wave 0 |
| REQ-020.3 | TLS CA gen + leaf signing | unit | `cargo test --lib -p rigor daemon::tls::tests::` | No -- Wave 0 |
| REQ-020.4 | SNI extraction edges | unit | `cargo test --lib -p rigor daemon::sni::tests::` | Yes (2 tests, needs expansion) |
| REQ-020.5 | Evaluator fail-open | unit | `cargo test --lib -p rigor evaluator::pipeline::tests::` | Yes (10 tests, needs fail-open case) |
| REQ-020.6 | DF-QuAD boundaries | unit | `cargo test --lib -p rigor constraint::graph::tests::` | Yes (13 tests, needs boundary cases) |
| REQ-020.7 | SeverityThresholds exact | unit | `cargo test --lib -p rigor violation::types::tests::` | Yes (6 tests, mostly covered) |
| REQ-020.8 | Claim pipeline ordering | unit | `cargo test --lib -p rigor claim::heuristic::tests::` | Yes (18 tests, needs ordering test) |
| REQ-020.9 | Content store TTL + concurrency | unit | `cargo test --lib -p rigor memory::content_store::tests::` | Yes (14 tests, needs concurrency) |
| REQ-020.10 | Action gate timeout | unit | `cargo test --lib -p rigor daemon::gate::tests::` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test --lib -p rigor`
- **Per wave merge:** `cargo test -p rigor`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `daemon/mod.rs` needs `#[cfg(test)] mod tests` block (for gaps 1, 2)
- [ ] `daemon/tls.rs` needs `#[cfg(test)] mod tests` block (for gap 3)
- [ ] `daemon/gate.rs` needs `#[cfg(test)] mod tests` block (for gap 10)

## Sources

### Primary (HIGH confidence)
- `crates/rigor/src/daemon/mod.rs` -- full source read, function signatures verified
- `crates/rigor/src/daemon/tls.rs` -- full source read, RigorCA API verified
- `crates/rigor/src/daemon/sni.rs` -- full source read, existing tests verified
- `crates/rigor/src/daemon/gate_api.rs` -- full source read, existing tests verified
- `crates/rigor/src/daemon/gate.rs` -- full source read, no tests confirmed
- `crates/rigor/src/constraint/graph.rs` -- full source read, 13 existing tests verified
- `crates/rigor/src/violation/types.rs` -- full source read, 6 existing tests verified
- `crates/rigor/src/claim/heuristic.rs` -- full source read, 18 existing tests verified
- `crates/rigor/src/memory/content_store.rs` -- full source read, 14 existing tests verified
- `crates/rigor/src/evaluator/pipeline.rs` -- full source read, 10 existing tests verified
- `crates/rigor/src/paths.rs` -- env var isolation pattern verified
- `crates/rigor-harness/src/home.rs` -- IsolatedHome API verified
- `crates/rigor-harness/src/ca.rs` -- TestCA API verified
- `crates/rigor/Cargo.toml` -- all dependencies verified
- `cargo test --lib -p rigor` -- 323 tests passing verified

### Secondary (MEDIUM confidence)
- Rust `std::env::set_var` unsafe requirement (Rust 1.83+) -- verified by `paths.rs` using unsafe blocks

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all deps already in Cargo.toml, verified by reading it
- Architecture: HIGH -- every target file read in full, function signatures and test modules documented
- Pitfalls: HIGH -- identified from actual codebase patterns (env var isolation, AtomicBool globals, moka TTL)

**Research date:** 2026-04-24
**Valid until:** 2026-05-24 (stable Rust codebase, no external dependency changes expected)
