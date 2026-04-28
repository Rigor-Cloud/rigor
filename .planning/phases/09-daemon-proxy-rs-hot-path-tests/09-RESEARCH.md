# Phase 9: daemon/proxy.rs hot-path tests - Research

**Researched:** 2026-04-24
**Domain:** Rust unit/integration testing for LLM proxy hot-path functions
**Confidence:** HIGH

## Summary

Phase 9 covers writing tests for four currently-untested functions in `crates/rigor/src/daemon/proxy.rs`: `proxy_request`, `extract_and_evaluate`, `scope_judge_check`, and `score_claim_relevance`. These are the security-critical decision functions in rigor's MITM proxy pipeline. The only production code change is introducing a `ChatClient` trait seam to abstract the `reqwest::Client` LLM calls made by `scope_judge_check` and `check_violations_persist`, enabling deterministic testing without real API calls.

The existing `rigor-harness` crate (Phase 7/8) provides `IsolatedHome`, `TestCA`, `MockLlmServer`, `TestProxy`, and SSE helpers. `TestProxy::start_with_mock()` composes a production proxy pointed at a `MockLlmServer` on an ephemeral port -- this is the primary integration test vehicle for `proxy_request`. For `scope_judge_check`, `check_violations_persist`, and `score_claim_relevance`, the trait seam is more appropriate because these functions make non-streaming Anthropic API calls that MockLlmServer does not currently support. The `RELEVANCE_SEMAPHORE` (an `AtomicBool`-based `SimpleSemaphore`) controls single-flight for `score_claim_relevance` and is a global static, requiring careful isolation in concurrent tests.

**Primary recommendation:** Introduce a narrow `JudgeClient` async trait (not reusing `corpus::ChatClient` which has a different shape) with a single `call_judge` method. The two LLM-calling functions (`scope_judge_check`, `check_violations_persist`) take `&dyn JudgeClient` instead of `&reqwest::Client`. A `ReqwestJudgeClient` wraps the current logic; tests use a `MockJudgeClient` returning canned responses.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
None explicitly locked -- all implementation choices are at Claude's discretion per CONTEXT.md.

### Claude's Discretion
All implementation choices are at Claude's discretion. Key guidance from GitHub issue #7:

- Inject a `ChatClient` trait seam so `scope_judge_check` / `check_violations_persist` can be driven by a fake in tests
- Unit tests for `proxy_request` decision branches against canned SSE streams (real TCP + rustls OK via rigor-harness)
- Property test that `score_claim_relevance` with N concurrent callers produces exactly one scored verdict (rest are no-ops)
- Tests for `extract_and_evaluate` and `evaluate_text_inline` claim-to-violation pipeline

CRITICAL -- Over-editing guard:
- The `ChatClient` trait seam is the ONLY production code modification
- Do NOT refactor proxy.rs beyond adding the trait and injecting it
- Do NOT restructure existing functions, rename variables, or change error handling
- Tests go in crates/rigor/tests/ using rigor-harness primitives

### Deferred Ideas (OUT OF SCOPE)
None -- discuss phase skipped.

</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REQ-019 | Unit or integration tests exist for each of: `proxy_request`, `extract_and_evaluate`, `scope_judge_check`, `score_claim_relevance`. Coverage measured by `cargo llvm-cov` MUST be non-zero for each. | Trait seam pattern enables deterministic tests without LLM; TestProxy+MockLlmServer covers proxy_request; RELEVANCE_SEMAPHORE isolation strategy covers score_claim_relevance concurrency |

</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| ChatClient trait seam | API / Backend (proxy.rs) | -- | Production code change: extracting HTTP call behind trait |
| proxy_request tests | API / Backend (test) | -- | Tests the proxy handler decision tree |
| extract_and_evaluate tests | API / Backend (test) | -- | Tests claim extraction + policy evaluation pipeline |
| scope_judge_check tests | API / Backend (test) | -- | Tests LLM judge for scope checking |
| score_claim_relevance tests | API / Backend (test) | -- | Tests concurrent single-flight relevance scoring |
| check_violations_persist tests | API / Backend (test) | -- | Tests LLM judge for retry verification |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.x (in-tree) | Async runtime for test execution | Already used; `#[tokio::test]` for async tests [VERIFIED: Cargo.toml] |
| rigor-harness | workspace (in-tree) | IsolatedHome, MockLlmServer, TestProxy, SSE helpers | Phase 7/8 deliverable, purpose-built for these tests [VERIFIED: crates/rigor-harness/] |
| async-trait | 0.1 (in-tree) | Trait object dispatch for async trait methods | Already in dependencies, needed for `JudgeClient` trait [VERIFIED: Cargo.toml line 63] |
| proptest | 1.11.0 | Property-based testing for concurrency invariants | Required by issue #7 for score_claim_relevance [VERIFIED: cargo search] |
| tempfile | 3.x (in-tree dev-dep) | Temp dirs for test isolation | Already a dev-dependency [VERIFIED: Cargo.toml line 76] |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio-test | (part of tokio) | Test utilities like `assert_ready`, time control | If needing to mock time for 30s timeouts in score_claim_relevance |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| proptest | Plain spawn-N-tasks loop | proptest generates shrinkable N values; plain loop is simpler but not property-testable |
| New JudgeClient trait | Extending corpus::ChatClient | corpus::ChatClient has wrong shape (ChatRequest/ChatResponse vs raw JSON); reusing it would force adapter code |
| MockLlmServer for judge | Trait mock for judge | MockLlmServer only serves SSE; judge calls are non-streaming JSON. Trait mock is cleaner. |

**Installation:**
```bash
# proptest needs to be added to dev-dependencies
cargo add proptest@1.11 --dev --manifest-path crates/rigor/Cargo.toml
```

**Version verification:**
- proptest 1.11.0: current latest [VERIFIED: `cargo search proptest` on 2026-04-24]
- async-trait 0.1: already in Cargo.toml [VERIFIED: line 63]
- tokio 1.x: already in Cargo.toml [VERIFIED: line 27]

## Architecture Patterns

### System Architecture Diagram

```
                    +------------------+
                    | Test binary      |
                    | (cargo test)     |
                    +--------+---------+
                             |
            +----------------+----------------+
            |                |                |
   +--------v------+  +-----v------+  +------v--------+
   | proxy_request  |  | extract_   |  | scope_judge_  |
   | integration    |  | evaluate   |  | check unit    |
   | tests          |  | unit tests |  | tests         |
   +--------+------+  +-----+------+  +------+--------+
            |                |                |
   +--------v------+  +-----v------+  +------v--------+
   | TestProxy +    |  | SharedState|  | MockJudge     |
   | MockLlmServer  |  | + event_tx |  | Client        |
   | (HTTP round-   |  | (in-memory)|  | (canned JSON) |
   | trip)           |  +------------+  +---------------+
   +---------+------+
             |
    +--------v------+
    | MockLlmServer |
    | (SSE on       |
    | ephemeral     |
    | port)         |
    +---------------+
```

Data flow for proxy_request test:
1. Test creates MockLlmServer with canned SSE response
2. Test creates TestProxy pointed at MockLlmServer
3. Test sends HTTP POST to TestProxy with crafted request body
4. TestProxy routes to proxy_request which processes, forwards to MockLlmServer
5. Response flows back through evaluation pipeline
6. Test asserts on response status, body, and events emitted

Data flow for unit tests (extract_and_evaluate, scope_judge_check):
1. Test constructs minimal SharedState / MockJudgeClient
2. Test calls function directly with canned inputs
3. Test asserts on return value or events emitted on event_tx channel

### Recommended Project Structure
```
crates/rigor/src/daemon/
  proxy.rs               # +JudgeClient trait, +JudgeClientImpl (production wrapper)
                         # Existing functions get &dyn JudgeClient parameter

crates/rigor/tests/
  proxy_hotpath.rs       # All phase 9 tests in one integration test file
                         # - proxy_request_* (via TestProxy + MockLlmServer)
                         # - extract_and_evaluate_* (direct call)
                         # - scope_judge_check_* (via MockJudgeClient)
                         # - score_claim_relevance_* (concurrency property test)
```

### Pattern 1: JudgeClient Trait Seam
**What:** Extract the HTTP call pattern from `scope_judge_check` and `check_violations_persist` into a trait.
**When to use:** When production code makes external HTTP calls that need to be replaced in tests.

The current pattern in both functions is identical:
```rust
// Current: both functions take &reqwest::Client and build requests like this
let mut req = client
    .post(format!("{}/v1/messages", api_url))
    .header("anthropic-version", "2023-06-01")
    .header("content-type", "application/json");
req = apply_provider_auth(req, api_key);
let resp = tokio::time::timeout(Duration::from_secs(10), req.json(&body).send()).await;
// Then parse Anthropic JSON response: content[0].text
```
[VERIFIED: proxy.rs lines 2935-2943, 3061-3069]

**Minimal trait surface:**
```rust
// Source: derived from proxy.rs usage analysis
use async_trait::async_trait;

/// Abstraction for LLM judge calls (scope check, violation persist check).
/// Production: wraps reqwest + apply_provider_auth.
/// Tests: returns canned serde_json::Value responses.
#[async_trait]
pub(crate) trait JudgeClient: Send + Sync {
    /// Send a non-streaming messages API call and return the parsed JSON response.
    /// Handles auth, timeout, and error mapping internally.
    async fn call_judge(
        &self,
        api_url: &str,
        api_key: &str,
        body: &serde_json::Value,
        timeout_secs: u64,
    ) -> Result<serde_json::Value, JudgeError>;
}

pub(crate) enum JudgeError {
    Timeout,
    HttpError(u16),
    NetworkError(String),
    ParseError(String),
}
```
[ASSUMED — exact trait surface is Claude's discretion]

### Pattern 2: TestProxy Integration Test for proxy_request
**What:** Use TestProxy::start_with_mock() to exercise proxy_request end-to-end.
**When to use:** Testing the full proxy_request decision tree including SSE streaming, PII detection, claim extraction, and evaluation.

```rust
// Source: crates/rigor-harness/src/proxy.rs TestProxy::start_with_mock
use rigor_harness::{MockLlmServerBuilder, TestProxy};

#[tokio::test]
async fn proxy_request_allow_on_clean_response() {
    // MockLlmServer serves benign Anthropic SSE response
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("The sky is blue.")
        .build()
        .await;

    let yaml = r#"
constraints:
  beliefs:
    - id: test-belief
      name: Test belief
      description: A test constraint
  justifications: []
  defeaters: []
"#;
    let proxy = TestProxy::start_with_mock(yaml, &mock.url()).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 100,
            "stream": true,
            "messages": [{"role": "user", "content": "What color is the sky?"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
}
```
[VERIFIED: TestProxy::start_with_mock exists at proxy.rs:87-148]

### Pattern 3: Concurrency Property Test for SimpleSemaphore
**What:** Verify that N concurrent callers of score_claim_relevance's single-flight path produce exactly one scorer.
**When to use:** Testing the RELEVANCE_SEMAPHORE global single-flight invariant.

The `SimpleSemaphore` is a static `AtomicBool`. `score_claim_relevance` is called in a `tokio::spawn` inside `extract_and_evaluate_text` (line 3434-3472). The key invariant: when N tasks call `RELEVANCE_SEMAPHORE.try_acquire()` concurrently, exactly one succeeds (returns `Some(())`), rest get `None` and skip. After the winner calls `release()`, the next call can acquire.

This is NOT about proptest generating random inputs -- it's about spawning N tasks racing to acquire the semaphore. proptest can vary N and timing, but the core test is a concurrent race.

```rust
// Source: proxy.rs lines 3476-3498 (SimpleSemaphore)
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[tokio::test]
async fn semaphore_exactly_one_acquires() {
    // SimpleSemaphore is private, so test via the public interface:
    // call score_claim_relevance from N tasks, verify exactly one LLM call
    let call_count = Arc::new(AtomicUsize::new(0));
    let n = 10;
    let mut handles = Vec::new();
    for _ in 0..n {
        let count = call_count.clone();
        handles.push(tokio::spawn(async move {
            // ... attempt to call score_claim_relevance ...
            // Mock judge client counts calls
        }));
    }
    for h in handles { h.await.unwrap(); }
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}
```
[ASSUMED — exact test shape depends on how SimpleSemaphore is exposed for testing]

### Anti-Patterns to Avoid
- **Global static leakage:** `RELEVANCE_CACHE` and `RELEVANCE_SEMAPHORE` are `static` globals. Tests that populate the cache WILL leak state into subsequent tests running in the same process. Mitigate by either (a) exposing a `#[cfg(test)] fn clear_relevance_cache()` helper, or (b) using unique claim text per test so cache collisions are impossible.
- **Refactoring proxy.rs beyond the trait seam:** The over-editing guard explicitly forbids restructuring existing functions. The trait seam is additive -- new trait definition, new struct wrapping current logic, parameter changes to `scope_judge_check` and `check_violations_persist` signatures.
- **Testing via MockLlmServer for non-streaming judge calls:** MockLlmServer only serves SSE. The judge functions (`scope_judge_check`, `check_violations_persist`) expect a JSON response, not SSE events. Use the trait mock, not MockLlmServer, for these.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Mock LLM responses | Custom HTTP server for judge | `JudgeClient` trait + `MockJudgeClient` | Trait mock is deterministic, zero network overhead, no port conflicts |
| Isolated HOME | Manual TempDir + env mgmt | `IsolatedHome` from rigor-harness | Already handles .rigor/ subdir, cleanup, path generation |
| SSE response fixtures | Hand-crafted SSE strings | `anthropic_sse_chunks()` / `openai_sse_chunks()` from rigor-harness | Correct SSE framing with message_start/stop, content_block_delta |
| Test proxy setup | Manual DaemonState + bind | `TestProxy::start_with_mock()` | Handles RIGOR_HOME isolation, ephemeral port, graceful shutdown |
| Property test framework | Manual random + loop | `proptest` crate | Shrinking, deterministic replay, failure persistence |

**Key insight:** The rigor-harness crate from Phase 7 already solves the integration test infrastructure problem. The gap is the trait seam for judge calls (new) and property testing for concurrency (proptest is new).

## Common Pitfalls

### Pitfall 1: RELEVANCE_CACHE Cross-Test Contamination
**What goes wrong:** `RELEVANCE_CACHE` is a `static LazyLock<Mutex<HashMap>>`. If test A populates the cache with claim text "X", test B running later in the same process sees the cached result, changing behavior.
**Why it happens:** Rust integration tests in the same file run in a single process. Static globals persist across tests.
**How to avoid:** Use unique claim text per test (e.g., prefix with test name). Or expose a `#[cfg(test)] pub fn clear_relevance_cache()` function. The function already exists as `lookup_relevance` (read-only); adding a clear function is minimal.
**Warning signs:** Flaky tests that pass alone but fail when run together.

### Pitfall 2: RELEVANCE_SEMAPHORE State After Panic
**What goes wrong:** If a test panics while holding the semaphore (after `try_acquire` but before `release`), subsequent tests in the same process can never acquire it.
**Why it happens:** `SimpleSemaphore` has no guard/RAII. The `release()` call in the `tokio::spawn` at line 3471 won't run if the task panics.
**How to avoid:** Always release the semaphore in tests, or add a `#[cfg(test)] fn reset_relevance_semaphore()` helper. Alternatively, test the semaphore behavior in isolation by extracting `SimpleSemaphore` into its own module with unit tests.
**Warning signs:** Tests hang or skip the LLM-as-judge path unexpectedly.

### Pitfall 3: DaemonState::load Requires Valid rigor.yaml
**What goes wrong:** `DaemonState::load()` calls `load_rigor_config()` which parses `rigor.yaml`. An invalid YAML crashes the test with an unhelpful error.
**Why it happens:** The config parser is strict about the `ConstraintsSection` struct shape.
**How to avoid:** Use the minimal valid YAML pattern from harness_smoke.rs: `constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n`. For tests needing specific constraints, add entries to the appropriate list.
**Warning signs:** `DaemonState::load failed in TestProxy` panic messages.

### Pitfall 4: ViolationLogger Touching Real Filesystem
**What goes wrong:** `extract_and_evaluate_text` calls `ViolationLogger::new()` which uses `rigor_home()`. If `RIGOR_HOME` is not set to the isolated dir, violations write to real `~/.rigor/violations.jsonl`.
**Why it happens:** Phase 8 fixed `rigor_home()` to respect `RIGOR_HOME`, but the env var must be set during test execution.
**How to avoid:** For tests calling `extract_and_evaluate_text` directly (not via TestProxy), set `RIGOR_HOME` to a temp dir. TestProxy::start_with_mock already handles this.
**Warning signs:** CI grep guard (Phase 8 REQ-018) catching real HOME path in test output.

### Pitfall 5: MockLlmServer Only Serves SSE, Not JSON
**What goes wrong:** Using MockLlmServer for scope_judge_check/check_violations_persist tests. These functions send `"stream": false` requests and expect a JSON response body, not SSE events.
**Why it happens:** MockLlmServer was built for proxy_request streaming tests in Phase 7.
**How to avoid:** Use the JudgeClient trait mock for judge calls. Only use MockLlmServer for proxy_request integration tests where the proxy itself makes the streaming call.
**Warning signs:** Judge functions timing out or returning parse errors.

## Code Examples

### Example 1: Minimal SharedState for Unit Tests
```rust
// Source: derived from DaemonState::load (daemon/mod.rs:208-300)
// and TestProxy::start (rigor-harness/src/proxy.rs:28-81)

use rigor::daemon::{DaemonState, SharedState};
use rigor::daemon::ws::create_event_channel;
use std::sync::{Arc, Mutex};

fn make_test_state(yaml_content: &str) -> (SharedState, tokio::sync::broadcast::Receiver<rigor::daemon::ws::DaemonEvent>) {
    let home = rigor_harness::IsolatedHome::new();
    let yaml_path = home.write_rigor_yaml(yaml_content);

    // Set RIGOR_HOME for rigor_home() calls during DaemonState::load
    unsafe { std::env::set_var("RIGOR_HOME", home.rigor_dir_str()) };

    let (event_tx, event_rx) = create_event_channel();
    let state = DaemonState::load(yaml_path, event_tx).expect("load test state");

    unsafe { std::env::remove_var("RIGOR_HOME") };

    (Arc::new(Mutex::new(state)), event_rx)
}
```
[VERIFIED: DaemonState::load signature at daemon/mod.rs:209; create_event_channel at ws.rs:304]

NOTE: The unsafe `set_var`/`remove_var` is only safe when tests run in isolation (one thread at a time for the env-mutating setup). TestProxy::start_with_mock uses `spawn_blocking` to scope this safely. For direct unit tests, consider using serial test execution or the `serial_test` crate.

### Example 2: MockJudgeClient for scope_judge_check
```rust
// Source: derived from scope_judge_check signature at proxy.rs:3035-3131

use async_trait::async_trait;

struct MockJudgeClient {
    response: serde_json::Value,
}

#[async_trait]
impl JudgeClient for MockJudgeClient {
    async fn call_judge(
        &self,
        _api_url: &str,
        _api_key: &str,
        _body: &serde_json::Value,
        _timeout_secs: u64,
    ) -> Result<serde_json::Value, JudgeError> {
        Ok(self.response.clone())
    }
}

// Usage: test scope_judge_check with "YES" response
let mock = MockJudgeClient {
    response: serde_json::json!({
        "content": [{"type": "text", "text": "YES The action is within scope."}]
    }),
};
```
[ASSUMED -- exact JudgeClient API shape is Claude's discretion]

### Example 3: Anthropic JSON Response for Judge Tests
```rust
// Source: proxy.rs response parsing pattern at lines 2972-2980, 3114-3122
// Both scope_judge_check and check_violations_persist parse the same structure:

fn anthropic_json_response(text: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "msg_test",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-20250514",
        "content": [{"type": "text", "text": text}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    })
}
```
[VERIFIED: response parsing at proxy.rs lines 2972-2980]

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Mocking reqwest with wiremock/mockito | Trait seam with mock impl | Standard Rust pattern | More explicit, no HTTP overhead, compile-time checked |
| Global env::set_var for HOME isolation | RIGOR_HOME env var (Phase 8) | Phase 8 (2026-04) | Safe in-process testing via rigor_home() |
| No property testing in rigor | proptest for concurrency invariants | Phase 9 (new) | Captures single-flight and cache invariants |

**Deprecated/outdated:**
- `std::env::set_var("HOME", ...)` for test isolation: replaced by RIGOR_HOME in Phase 8. The old approach was unsafe in parallel tests.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | JudgeClient trait with `call_judge(api_url, api_key, body, timeout_secs) -> Result<Value, JudgeError>` is the right abstraction | Architecture Patterns | Low -- shape can be adjusted; the key decision is "trait seam vs HTTP mock" and trait seam is locked |
| A2 | proptest 1.11.0 with `tokio::Runtime` block_on wrapper is sufficient for async concurrency property tests | Standard Stack | Low -- proptest itself is sync but wrapping async code in `Runtime::block_on` is standard practice |
| A3 | SimpleSemaphore can be tested through score_claim_relevance without exposing it publicly | Code Examples | Medium -- may need `#[cfg(test)] pub` or extracting to a testable module |
| A4 | RELEVANCE_CACHE can be isolated per test using unique claim text strings | Pitfalls | Low -- alternative is exposing a clear function |

## Open Questions

1. **Should JudgeClient be `pub(crate)` or `pub`?**
   - What we know: Only proxy.rs uses it. No external consumers.
   - What's unclear: Whether future phases (10-12) will want to mock judge calls from integration tests in `crates/rigor/tests/`.
   - Recommendation: Start with `pub(crate)`. Integration tests in `crates/rigor/tests/` are part of the same crate for `#[cfg(test)]` purposes, so they CAN see `pub(crate)` items if the module is `pub`. Since `daemon::proxy` is a `pub` module, `pub(crate)` items in it are visible to integration tests.

2. **How to handle RELEVANCE_SEMAPHORE in concurrent tests?**
   - What we know: It's a global `static`. Tests that acquire it can block other tests.
   - What's unclear: Whether `#[cfg(test)] fn reset_relevance_semaphore()` is an acceptable production code change under the over-editing guard.
   - Recommendation: A test-only reset function is minimal (2 lines) and aligns with the "only trait seam" guard. Alternatively, run semaphore tests with `#[serial]` from `serial_test` crate.

3. **Where to add non-streaming response support?**
   - What we know: MockLlmServer only serves SSE. Judge functions need JSON responses.
   - What's unclear: Whether to extend MockLlmServer with a non-streaming mode or rely entirely on the trait mock.
   - Recommendation: Trait mock for this phase. MockLlmServer extension can be deferred to Phase 12 if needed.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| rustc | Compilation | Yes | 1.93.0 | -- |
| cargo | Build/test | Yes | (bundled) | -- |
| cargo-llvm-cov | REQ-019 coverage verification | No | -- | `cargo install cargo-llvm-cov`; or CI-only |
| proptest | Concurrency property tests | No (not in Cargo.lock) | 1.11.0 on crates.io | `cargo add proptest@1.11 --dev` |

**Missing dependencies with no fallback:**
- cargo-llvm-cov: Required by REQ-019 for coverage measurement. Install via `cargo install cargo-llvm-cov` or `rustup component add llvm-tools-preview && cargo install cargo-llvm-cov`. Can be deferred to CI-only (Phase 17 REQ-031 will formalize this).

**Missing dependencies with fallback:**
- proptest: Not yet in dependencies. Add via `cargo add proptest@1.11 --dev --manifest-path crates/rigor/Cargo.toml`.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) + proptest 1.11.0 |
| Config file | None needed -- proptest uses default config |
| Quick run command | `cargo test --test proxy_hotpath -p rigor -- --test-threads=1` |
| Full suite command | `cargo test -p rigor` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REQ-019-a | proxy_request allow on clean response | integration | `cargo test --test proxy_hotpath proxy_request_allow -p rigor -x` | No -- Wave 0 |
| REQ-019-b | proxy_request block on violation | integration | `cargo test --test proxy_hotpath proxy_request_block -p rigor -x` | No -- Wave 0 |
| REQ-019-c | proxy_request paused passthrough | integration | `cargo test --test proxy_hotpath proxy_request_paused -p rigor -x` | No -- Wave 0 |
| REQ-019-d | extract_and_evaluate with claims | unit | `cargo test --test proxy_hotpath extract_and_evaluate -p rigor -x` | No -- Wave 0 |
| REQ-019-e | extract_and_evaluate no text | unit | `cargo test --test proxy_hotpath extract_and_evaluate_no_text -p rigor -x` | No -- Wave 0 |
| REQ-019-f | scope_judge_check within_scope | unit | `cargo test --test proxy_hotpath scope_judge_within -p rigor -x` | No -- Wave 0 |
| REQ-019-g | scope_judge_check out_of_scope | unit | `cargo test --test proxy_hotpath scope_judge_out -p rigor -x` | No -- Wave 0 |
| REQ-019-h | scope_judge_check timeout failopen | unit | `cargo test --test proxy_hotpath scope_judge_timeout -p rigor -x` | No -- Wave 0 |
| REQ-019-i | score_claim_relevance single-flight | property | `cargo test --test proxy_hotpath semaphore_single_flight -p rigor -x` | No -- Wave 0 |
| REQ-019-j | check_violations_persist YES | unit | `cargo test --test proxy_hotpath violations_persist_yes -p rigor -x` | No -- Wave 0 |
| REQ-019-k | check_violations_persist NO | unit | `cargo test --test proxy_hotpath violations_persist_no -p rigor -x` | No -- Wave 0 |
| REQ-019-l | evaluate_text_inline returns decision | unit | `cargo test --test proxy_hotpath evaluate_text_inline -p rigor -x` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test --test proxy_hotpath -p rigor -- --test-threads=1`
- **Per wave merge:** `cargo test -p rigor`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/rigor/tests/proxy_hotpath.rs` -- all Phase 9 tests
- [ ] `proptest` added to `[dev-dependencies]` in `crates/rigor/Cargo.toml`
- [ ] JudgeClient trait + JudgeClientImpl in `proxy.rs` (the only production change)
- [ ] `#[cfg(test)]` helpers for RELEVANCE_CACHE/RELEVANCE_SEMAPHORE reset

## Detailed Function Analysis

### proxy_request (line 1008-2790)
**Signature:** `async fn proxy_request(state: SharedState, headers: HeaderMap, body: Bytes, path: &str) -> Response` [VERIFIED: proxy.rs:1008-1013]

**Key decision branches:**
1. **Paused proxy** (line 1020): If `st.proxy_paused`, forwards raw without evaluation
2. **Retroactive gate** (line 1064): Checks pending action gates by session_id
3. **JSON parse failure** (line 1149): Returns 400 Bad Request
4. **Streaming vs non-streaming** (line 1161): `body_json.get("stream")`
5. **PII-IN redaction** (line 1321): Detects and redacts PII before forwarding
6. **Streaming path** (line 1531-2674): Chunk-by-chunk evaluation with killswitch
7. **Non-streaming path** (line 2676-2790): Buffer response, spawn extract_and_evaluate
8. **Upstream error** (line 1484): Returns 502 Bad Gateway

**External dependencies:**
- `reqwest::Client` (via `st.http_client`) for upstream forwarding
- `DaemonState` fields: config, graph, target_api, api_key, event_tx, policy_engine, etc.
- `build_epistemic_context()` for claim injection
- `egress::FilterChain` for request/response filtering

**Test strategy:** Integration via TestProxy+MockLlmServer. Test branches 1, 3, 5, 7 (non-streaming). Streaming tests (branch 6) require SSE response parsing.

### extract_and_evaluate (line 2830-2871)
**Signature:** `fn extract_and_evaluate(response_bytes: &[u8], path: &str, request_id: &str, event_tx: &EventSender, state: &SharedState)` [VERIFIED: proxy.rs:2830-2836]

**Dependencies:**
- `serde_json::from_slice` to parse response bytes
- `extract_assistant_text()` for Anthropic/OpenAI format handling
- `extract_and_evaluate_text()` for the actual evaluation pipeline
- `EventSender` for emitting Decision events (allow on parse failure / no text)

**Note:** This function is synchronous (not async). It delegates to `extract_and_evaluate_text` which does the actual claim extraction and policy evaluation. The function itself is thin -- the main testable behaviors are: (a) parse failure -> emit allow, (b) no assistant text -> emit allow, (c) valid response -> delegates to extract_and_evaluate_text.

**Test strategy:** Direct call with crafted response_bytes. Verify events emitted on event_tx.

### scope_judge_check (line 3035-3131)
**Signature:** `async fn scope_judge_check(client: &reqwest::Client, api_url: &str, api_key: &str, model: &str, user_message: &str, action_intent: &str, event_tx: &EventSender) -> (bool, String)` [VERIFIED: proxy.rs:3035-3043]

**LLM call pattern:**
- POST to `{api_url}/v1/messages`
- Headers: `anthropic-version: 2023-06-01`, `content-type: application/json`
- Auth via `apply_provider_auth()`
- 10-second timeout
- Parses Anthropic response: `content[0].text`
- YES prefix -> within_scope=true; else false
- **Fail-open on errors:** timeout, HTTP errors, parse errors all return (true, "reason")

**Test strategy:** Replace `&reqwest::Client` with `&dyn JudgeClient`. Mock returns canned Anthropic JSON. Test: YES, NO, timeout, HTTP 401, parse error.

### check_violations_persist (line 2878-3031)
**Signature:** `async fn check_violations_persist(client: &reqwest::Client, api_url: &str, api_key: Option<&str>, model: &str, original_violations: &[String], retry_text: &str, event_tx: &EventSender) -> bool` [VERIFIED: proxy.rs:2878-2886]

**Behavior:**
- Returns false (fail-open) if: no api_key, empty violations, empty retry_text
- Calls LLM with violation list + retry text
- YES -> violations persist (return true)
- Any error -> fail-open (return false)

**Test strategy:** Same JudgeClient trait mock. Test: YES, NO, no API key, empty violations, timeout.

### score_claim_relevance (line 3523-3808)
**Signature:** `async fn score_claim_relevance(client: &reqwest::Client, api_url: &str, api_key: Option<&str>, model: &str, claims: &[(String, String)], constraints: &[(String, String)], event_tx: &EventSender)` [VERIFIED: proxy.rs:3523-3531]

**Key behaviors:**
- Skips if no api_key, empty claims, or empty constraints
- Checks RELEVANCE_CACHE first, emits cached results
- Skips LLM call if all claims cached
- RELEVANCE_SEMAPHORE controls single-flight (called at line 3434, released at line 3471)
- 30-second timeout per attempt
- Retry loop with backoff for 429 (rate limit): delays [0, 3, 8, 15] seconds
- Parses pipe-delimited response lines: `claim_id|constraint_id|high/medium|reason`
- Caches results in RELEVANCE_CACHE

**Test strategy:** JudgeClient trait mock for the LLM call. Concurrency test for RELEVANCE_SEMAPHORE via proptest or multi-task spawn.

### evaluate_text_inline (line 3135-3210)
**Signature:** `fn evaluate_text_inline(assistant_text: &str, _path: &str, request_id: &str, event_tx: &EventSender, state: &SharedState) -> String` [VERIFIED: proxy.rs:3135-3141]

**Behavior:**
- Calls `extract_and_evaluate_text` first (for side effects: events)
- Then re-runs claim extraction + policy evaluation independently
- Returns "allow", "warn", or "block" as String
- Depends on SharedState for config, graph strengths, policy_engine

**Test strategy:** Direct call with constructed SharedState containing known constraints. Feed text that triggers/doesn't trigger violations.

## Trait Seam Impact Analysis

The trait seam changes function signatures for `scope_judge_check` and `check_violations_persist`. All callers must be updated:

**scope_judge_check callers:**
- proxy.rs streaming path, line 1816: `scope_judge_check(&http, &judge_url, &api_key, &judge_model, &user_msg, &action_text, &etx)` [VERIFIED: proxy.rs:1816-1825]

**check_violations_persist callers:**
- proxy.rs streaming block path, line 2190: `check_violations_persist(&http_client_bg, ...)` [VERIFIED: proxy.rs:2190-2199]
- proxy.rs post-stream evaluation, line 2376: `check_violations_persist(&http_client_bg, ...)` [VERIFIED: proxy.rs:2376-2385]
- proxy.rs post-stream retry verification, line 2541: `check_violations_persist(&http_client_bg, ...)` [VERIFIED: proxy.rs:2541-2550]

**score_claim_relevance callers:**
- proxy.rs extract_and_evaluate_text, line 3461: `score_claim_relevance(&http_client, ...)` [VERIFIED: proxy.rs:3461-3470]

**Total call sites:** 1 for scope_judge_check, 3 for check_violations_persist, 1 for score_claim_relevance = 5 call sites to update.

**Proposed injection point:** The `reqwest::Client` (`http_client`) comes from `DaemonState.http_client`. The JudgeClient can be stored in DaemonState as `pub judge_client: Arc<dyn JudgeClient>`, initialized from `http_client` during `DaemonState::load()`. Callers read it from state instead of using `http_client` directly. This is the minimal change -- one new field in DaemonState, one initialization line in `load()`, and 5 call sites updated. Alternatively, the JudgeClient can be passed as a parameter to the functions without storing in state.

## Sources

### Primary (HIGH confidence)
- `crates/rigor/src/daemon/proxy.rs` -- all function signatures, call sites, and behavior analyzed directly from source
- `crates/rigor-harness/src/` -- IsolatedHome, MockLlmServer, TestProxy, SSE helpers verified from source
- `crates/rigor/Cargo.toml` -- dependency versions verified from manifest

### Secondary (MEDIUM confidence)
- `cargo search proptest` -- version 1.11.0 confirmed from crates.io registry [VERIFIED: cargo search 2026-04-24]
- Tokio testing documentation -- standard `#[tokio::test]` patterns

### Tertiary (LOW confidence)
- proptest async support -- community patterns for wrapping async in `Runtime::block_on` within proptest; not verified against proptest 1.11.0 docs directly

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries verified in Cargo.toml or crates.io registry
- Architecture: HIGH -- all function signatures, call sites, and data flows read directly from source
- Pitfalls: HIGH -- identified from source analysis of global statics and initialization patterns
- Trait seam design: MEDIUM -- exact API shape is assumed, but the pattern is well-established

**Research date:** 2026-04-24
**Valid until:** 2026-05-24 (stable codebase, no external API changes expected)
