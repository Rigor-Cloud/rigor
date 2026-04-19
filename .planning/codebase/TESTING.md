# Testing Patterns

**Analysis Date:** 2026-04-19

## Test Framework

**Runner:**
- Built-in `cargo test` (Rust's standard test harness)
- `tokio::test` for async tests — `tokio = { version = "1", features = ["rt-multi-thread", "macros"] }` (`crates/rigor/Cargo.toml:27`)
- Config: no custom config files; controlled by the `[dev-dependencies]` and `[[bench]]` sections in `crates/rigor/Cargo.toml:70-80`

**Benchmark Framework:**
- `criterion = { version = "0.5", features = ["html_reports"] }` — declared as a dev-dependency (`crates/rigor/Cargo.toml:72`)
- Two benchmark binaries registered with `harness = false`:
  - `crates/rigor/benches/hook_latency.rs` — full-pipeline latency target <100 ms
  - `crates/rigor/benches/evaluation_only.rs` — pure Rego evaluation target <50 ms

**Assertion Library:** standard library `assert!`, `assert_eq!`, `assert_ne!`, plus `panic!` in unreachable match arms. No external assertion crate.

**Test scale:** ~265 `#[test]` / `#[tokio::test]` functions across `crates/rigor/src/` (inline) and `crates/rigor/tests/` (integration).

**Run Commands:**
```bash
cargo test --all-features          # Run every test (what CI runs)
cargo test --lib                   # Only unit tests in crates/rigor/src/
cargo test --test integration_hook # Run one integration test file
cargo test -- --nocapture          # Show println!/eprintln! output
cargo bench                        # Run criterion benchmarks
cargo clippy --all-targets --all-features -- -D warnings  # Lint test code too
```

## Test File Organization

**Location (two-tier):**
- **Unit tests** live inline in the source file they test, inside `#[cfg(test)] mod tests { ... }`. This is the default pattern for every pure-logic module — confidence scoring, hedge detection, heuristic claim extraction, violation collection, graph math.
- **Integration tests** live in `crates/rigor/tests/*.rs` and drive the compiled `rigor` binary via `Command::new(env!("CARGO_BIN_EXE_rigor"))`.
- **Benchmarks** live in `crates/rigor/benches/*.rs`.

**Naming:**
- Inline test modules are always named `mod tests`
- Test function names describe the scenario in `test_<condition>_<expected_outcome>` form, e.g. `test_block_on_violation`, `test_invalid_yaml_fails_open`, `test_threshold_exactly_at_block`, `test_rigor_test_claims_malformed_falls_back`
- Some integration tests use a flatter verb-phrase style: `claim_injection_plus_custom_filter_compose`, `execute_retry_succeeds_on_second_attempt`, `filter_chain_with_ctx_scratch_passes_state`

**Structure:**
```
crates/rigor/
├── src/
│   ├── claim/
│   │   ├── heuristic.rs       # module code + `#[cfg(test)] mod tests` at bottom
│   │   └── confidence.rs      # same pattern
│   ├── violation/
│   │   └── collector.rs       # inline tests for collect_violations / determine_decision
│   └── ...
├── tests/                      # integration tests (spawn the binary)
│   ├── integration_hook.rs     # stdin/stdout JSON contract
│   ├── integration_constraint.rs # full constraint pipeline, uses RIGOR_TEST_CLAIMS
│   ├── true_e2e.rs             # writes real transcript JSONL, no test hooks
│   ├── dogfooding.rs           # loads production rigor.yaml, tests self-constraint
│   ├── claim_extraction_e2e.rs # end-to-end claim extraction
│   ├── egress_integration.rs   # async filter chain tests (#[tokio::test])
│   └── fallback_integration.rs
└── benches/
    ├── hook_latency.rs
    └── evaluation_only.rs
```

## Test Structure

**Inline unit test module template:**
```rust
// crates/rigor/src/claim/confidence.rs:39-78
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definitive_is() {
        assert_eq!(assign_confidence("X is Y"), 0.9);
    }

    #[test]
    fn test_negation_priority() {
        // Negation should take priority over definitive
        assert_eq!(assign_confidence("This is not supported"), 0.8);
    }
}
```

**Async test template:**
```rust
// crates/rigor/tests/egress_integration.rs:31
#[tokio::test]
async fn claim_injection_plus_custom_filter_compose() {
    let chain = FilterChain::new(vec![
        Arc::new(ClaimInjectionFilter::new(...)),
        Arc::new(UppercaseFilter),
    ]);
    let mut body = serde_json::json!({ "messages": [...] });
    let mut ctx = ConversationCtx::new_anonymous();

    chain.apply_request(&mut body, &mut ctx).await.unwrap();

    assert!(body["system"].as_str().unwrap().contains("rigor says"));
}
```

**Integration test template (binary spawn):**
```rust
// crates/rigor/tests/integration_hook.rs:12-34
fn run_rigor_with_input(input_json: &Value) -> (String, String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rigor"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn rigor process");

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin.write_all(input_json.to_string().as_bytes())
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to read stdout");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);
    (stdout, stderr, exit_code)
}
```

**Patterns observed:**
- **AAA (Arrange/Act/Assert) implicit**: tests are typically 5–30 lines, no setup/teardown helpers beyond per-file `fn` factories (`raw(...)`, `default_thresholds()`, `setup_production_config()`)
- **No `#[before_each]` / `#[after_each]`** — `TempDir::new().unwrap()` at the top of each test gives automatic cleanup via `Drop`
- **Assertion messages are diagnostic**: many `assert!` / `assert_eq!` include a formatted message with the actual value, e.g. `assert_eq!(exit_code, 0, "stderr: {}", stderr)` and `assert_ne!(response["decision"], ..., "Medium-strength violation should not block. Response: {}", stdout)`
- **`match` on enum outcomes** rather than unwrap + eq: see `crates/rigor/src/violation/collector.rs:188-194` and `crates/rigor/src/fallback/mod.rs:234-241`:
  ```rust
  match determine_decision(&violations) {
      Decision::Warn { violations } => { assert_eq!(violations.len(), 1); }
      other => panic!("Expected Warn, got {:?}", other),
  }
  ```

## Mocking

**Framework:** no dedicated mocking crate (no `mockall`, `mockito`, `wiremock` in `Cargo.toml`). Mocking is done via hand-rolled trait implementations.

**Hand-rolled mock pattern** (from `crates/rigor/tests/egress_integration.rs:13-29`):
```rust
struct UppercaseFilter;

#[async_trait]
impl EgressFilter for UppercaseFilter {
    fn name(&self) -> &'static str { "uppercase" }

    async fn apply_request(&self, body: &mut Json, _ctx: &mut ConversationCtx) -> Result<(), FilterError> {
        // test-specific behavior
        Ok(())
    }
}
```

**Test-dedicated harness crate:** `crates/rigor-harness/` is reserved for shared primitives (`MockAgent`, `MockLLM`, `TestDaemon`, `TestGitRepo`, `MockLSP`, `EventCapture`) — currently a stub (`crates/rigor-harness/src/lib.rs`, 8 lines) with a comment pointing at `docs/superpowers/specs/2026-04-15-test-harness-architecture-design.md` for the intended API.

**Environment-based test injection** replaces most mocking needs:
- `RIGOR_TEST_CLAIMS` — env var accepted by `crates/rigor/src/lib.rs:145` that overrides transcript extraction with inline JSON claims. Every integration test that evaluates constraints uses this to supply deterministic claims without touching the extractor.
- `RIGOR_FAIL_CLOSED` — tested by construction in `crates/rigor/src/main.rs:8`
- `RIGOR_DEBUG` — toggles debug-level tracing, including raw input JSON logging in `crates/rigor/src/hook/input.rs:22`

**What to Mock:**
- External process boundaries (LSP servers, Claude CLI) — mock the entire subprocess via fixture data
- Async egress filters — implement the `EgressFilter` trait directly in the test file
- LLM API calls — bypass with `RIGOR_TEST_CLAIMS`

**What NOT to Mock:**
- The `rigor` binary itself — integration tests always spawn the real `CARGO_BIN_EXE_rigor` process
- `regorus` Rego evaluation — run real Rego policies end-to-end
- `TempDir` / real filesystem — every test writes real `rigor.yaml` + `transcript.jsonl` files and reads them back
- `tempfile` + real process spawning are preferred over in-memory shims

## Fixtures and Factories

**Test-data builders** are plain functions inside the test file. Example from `crates/rigor/src/violation/collector.rs:134-145`:
```rust
fn raw(id: &str, violated: bool, reason: &str) -> RawViolation {
    RawViolation {
        constraint_id: id.to_string(),
        violated,
        claims: vec!["c1".to_string()],
        reason: reason.to_string(),
    }
}

fn default_thresholds() -> SeverityThresholds {
    SeverityThresholds::default()
}
```

**Integration test factories** for hook input:
```rust
// crates/rigor/tests/integration_constraint.rs:50-59
fn default_input(dir: &std::path::Path) -> Value {
    json!({
        "session_id": "test-constraint",
        "transcript_path": dir.join("transcript.jsonl").to_string_lossy(),
        "cwd": dir.to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": false
    })
}
```

**Embedded fixtures via `include_str!`:**
- `crates/rigor/tests/integration_constraint.rs:85` loads the example YAML: `let example = include_str!("../../../examples/rigor.yaml");`
- `crates/rigor/tests/dogfooding.rs:16` embeds the production `rigor.yaml`: `const PRODUCTION_RIGOR_YAML: &str = include_str!("../../../rigor.yaml");`
- `crates/rigor/src/policy/engine.rs:37` embeds the Rego helpers: `include_str!("../../../../policies/helpers.rego")`

**Inline YAML fixtures:** most integration tests hand-write small `rigor.yaml` strings using raw string literals (`r#"..."#`) for readability — see `crates/rigor/tests/integration_constraint.rs:111-125,172-207`.

**Location:** no `fixtures/` directory. Test data lives alongside the tests either as inline strings or as files written to `TempDir` at runtime. Shared project-level fixtures live at `examples/rigor.yaml` and `policies/helpers.rego`.

## Coverage

**Requirements:** no coverage tool configured, no minimum threshold enforced in CI.

**View Coverage:**
```bash
# Not part of the toolchain; if needed, use cargo-llvm-cov or tarpaulin:
cargo install cargo-llvm-cov
cargo llvm-cov --all-features --html
```

**Effective coverage today** (based on `#[cfg(test)]` presence per module — `grep -rc '#\[cfg(test)\]' crates/rigor/src/`):
- Pure-logic modules have inline unit tests (`claim/*`, `constraint/*`, `violation/*`, `policy/*`, `fallback/*`, `hook/*` indirectly via integration)
- Daemon / TLS / LSP / CLI glue code is exercised through integration tests rather than unit tests (most `cli/*.rs`, `daemon/*.rs`, `lsp/*.rs` lack inline test modules)

## Test Types

**Unit Tests (inline):**
- Scope: single function or struct in one file
- Location: bottom of the source file under `#[cfg(test)] mod tests { use super::*; ... }`
- Examples: `crates/rigor/src/claim/heuristic.rs:187-353` (21 tests), `crates/rigor/src/violation/types.rs:55-93` (6 tests), `crates/rigor/src/violation/collector.rs:130-292` (5 tests)

**Integration Tests:**
- Scope: full `rigor` binary invocation — stdin JSON in, stdout JSON out
- Location: `crates/rigor/tests/*.rs`
- Seven files, each focusing on a subsystem:
  - `integration_hook.rs` — hook JSON contract (allow/error/metadata)
  - `integration_constraint.rs` — full constraint pipeline with `RIGOR_TEST_CLAIMS` injection
  - `true_e2e.rs` — writes real transcript JSONL, tests heuristic extractor end-to-end
  - `dogfooding.rs` — runs against the production `rigor.yaml`
  - `claim_extraction_e2e.rs` — claim extraction in isolation
  - `egress_integration.rs` — async filter chain (`#[tokio::test]`)
  - `fallback_integration.rs` — retry + fallback policy orchestration
- All use `tempfile::TempDir` for isolation and `std::process::Command` to spawn the binary

**E2E Tests:**
- `true_e2e.rs` and `dogfooding.rs` are the closest thing — no Playwright/Cypress-style browser testing
- `rigor-test` binary (`crates/rigor-test/src/main.rs`) is intended to host Layer 3 (real-agent E2E) and Layer 4 (token-economy benchmarks), currently a stub shipping with `--help` only

**CI Self-Validation:**
`.github/workflows/ci.yml:57-74` adds a `rigor-validate` job that builds the release binary then runs `./target/release/rigor validate rigor.yaml` against the project's own config — verifying Rigor can parse and validate its own constraints.

## Common Patterns

**Async Testing:**
```rust
// crates/rigor/src/fallback/mod.rs:225-241
#[tokio::test]
async fn execute_success_returns_ok() {
    let cfg = FallbackConfig::default_config();
    let result = cfg
        .execute("test_comp", || async {
            Ok::<i32, (FailureCategory, String)>(42)
        })
        .await;

    match result {
        FallbackOutcome::Ok(v) => assert_eq!(v, 42),
        other => panic!("expected Ok(42), got {:?}", std::mem::discriminant(&other)),
    }
}
```

**Error Testing (fail-open contract):**
```rust
// crates/rigor/tests/integration_constraint.rs:249-263
#[test]
fn test_invalid_yaml_fails_open() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("rigor.yaml"), "{{{{not valid yaml").unwrap();

    let input = default_input(temp.path());
    let (stdout, _stderr, exit_code) = run_rigor_in_dir(temp.path(), &input);

    assert_eq!(exit_code, 0, "Should exit 0 (fail open)");
    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Invalid YAML should fail open (allow). Got: {}",
        stdout
    );
}
```

**Retry / Counter Testing (shared state across closures):**
```rust
// crates/rigor/src/fallback/mod.rs:273-313
let call_count = Arc::new(AtomicU32::new(0));
let cc = call_count.clone();
let result = cfg.execute("retry_comp", move || {
    let cc = cc.clone();
    async move {
        let n = cc.fetch_add(1, Ordering::SeqCst) + 1;
        if n == 1 { Err(...) } else { Ok(99) }
    }
}).await;

assert_eq!(call_count.load(Ordering::SeqCst), 2);
```

**Boundary / Threshold Testing:** exhaustively cover boundary values, naming each explicitly — see `crates/rigor/src/violation/types.rs:55-93` which tests `0.0`, `0.3999999`, `0.4`, `0.6999999`, `0.7`, and `1.0` as separate functions named by boundary.

**JSON Response Assertions:** always `parse_response(&stdout)` into `serde_json::Value` then drill with `response["metadata"]["version"]`. Check both structure (`.is_string()`, `.is_none()`, `.as_u64()`) and value.

**Benchmark Pattern (Criterion):**
```rust
// crates/rigor/benches/hook_latency.rs:128-148
c.bench_function("full_hook_latency", |b| {
    b.iter(|| {
        let raw_violations = engine.evaluate(black_box(&eval_input)).unwrap();
        let violations = collect_violations(
            black_box(raw_violations),
            black_box(&strengths),
            // ...
        );
        black_box(determine_decision(&violations));
    });
});

criterion_group!(benches, benchmark_full_hook_pipeline);
criterion_main!(benches);
```
Setup (config load, engine init, claim creation) happens **outside** `b.iter(|| ...)` so only the measured hot path is timed. Every input is wrapped in `black_box(...)` to prevent dead-code elimination.

---

*Testing analysis: 2026-04-19*
