# Testing Patterns

**Analysis Date:** 2026-04-19

## Test Framework

**Runner:**
- Rust built-in test framework (via `#[test]` and `cargo test`)
- No external test runner (Cargo default)
- Config: No explicit test config file

**Benchmarks:**
- Framework: `criterion` crate
- Config: `Cargo.toml` defines `[[bench]]` targets with `harness = false`
- Benchmarks located in: `crates/rigor/benches/`

**Assertion Library:**
- Rust standard `assert!()`, `assert_eq!()`, `assert_ne!()`
- Custom helpers for specific patterns (see below)

**Run Commands:**
```bash
cargo test                    # Run all tests
cargo test --test <name>      # Run specific test file
cargo test <filter>           # Run tests matching filter
cargo bench                   # Run criterion benchmarks
cargo test -- --nocapture    # Show println! output during tests
cargo test -- --test-threads=1  # Run tests serially
```

## Test File Organization

**Location:**
- Integration tests: `crates/rigor/tests/` directory (separate from source)
- Unit tests: Co-located with source in `crates/rigor/src/` modules (if any)
- Benchmarks: `crates/rigor/benches/` directory

**Naming Convention:**
- Test files: descriptive snake_case: `integration_hook.rs`, `integration_constraint.rs`, `claim_extraction_e2e.rs`
- Test functions: `test_<subject>_<condition>()` or `test_<feature>_<expected_behavior>()`

**Test Files Present:**
- `crates/rigor/tests/integration_hook.rs` — Hook input/output pipeline
- `crates/rigor/tests/integration_constraint.rs` — Constraint loading and evaluation
- `crates/rigor/tests/fallback_integration.rs` — Fallback behavior
- `crates/rigor/tests/egress_integration.rs` — Egress/output handling
- `crates/rigor/tests/claim_extraction_e2e.rs` — E2E claim extraction
- `crates/rigor/tests/true_e2e.rs` — Full end-to-end flow
- `crates/rigor/tests/dogfooding.rs` — Self-validation
- `crates/rigor/benches/hook_latency.rs` — Full pipeline latency
- `crates/rigor/benches/evaluation_only.rs` — Constraint evaluation performance

## Test Structure

**Module Header:**
All test files start with doc comment describing scope:
```rust
//! Integration tests for the Rigor stop hook.
//!
//! These tests verify the complete flow from stdin JSON to stdout JSON response.
```

**Helper Functions:**
Tests use shared helper functions for subprocess invocation and input/output handling.

**Example from `integration_hook.rs`:**
```rust
/// Helper to run rigor with JSON input and capture output
fn run_rigor_with_input(input_json: &Value) -> (String, String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rigor"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn rigor process");

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(input_json.to_string().as_bytes())
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to read stdout");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    (stdout, stderr, exit_code)
}

/// Parse stdout as JSON response
fn parse_response(stdout: &str) -> Value {
    serde_json::from_str(stdout).expect("Failed to parse JSON response")
}
```

**Setup Pattern:**
Tests use `tempfile::TempDir` for isolated filesystem setup:
```rust
#[test]
fn test_no_config_allows() {
    let temp = TempDir::new().unwrap();
    // No rigor.yaml or rigor.lock in temp dir
    let input = default_input(temp.path());
    let (stdout, _stderr, exit_code) = run_rigor_in_dir(temp.path(), &input);

    assert_eq!(exit_code, 0);
    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "No config should allow"
    );
}
```

**Teardown Pattern:**
- Implicit via `TempDir` drop (automatic cleanup)
- No explicit teardown code needed

## Integration Test Patterns

**Subprocess-Based Testing:**
Tests invoke the compiled binary directly via `Command::new(env!("CARGO_BIN_EXE_rigor"))`.

**Example Pattern:**
```rust
fn run_rigor_in_dir_with_env(
    dir: &std::path::Path,
    input_json: &Value,
    env_vars: &[(&str, &str)],
) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rigor"));
    cmd.current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (key, val) in env_vars {
        cmd.env(key, val);
    }

    let mut child = cmd.spawn().expect("Failed to spawn rigor process");
    // ... write input, capture output ...
    (stdout, stderr, exit_code)
}
```

**JSON Input/Output:**
- Tests construct input as `serde_json::Value` (JSON)
- Tests parse response with `serde_json::from_str()`
- Assertions on response fields: `response["decision"].is_null()`

## Error Testing

**Pattern: Fail-Open Validation**
Tests verify that errors result in allow decisions (fail-open principle):

```rust
#[test]
fn test_invalid_json_fails_open() {
    // Invalid JSON input should fail open (return allow with error)
    let mut child = Command::new(env!("CARGO_BIN_EXE_rigor"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn rigor process");

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"not valid json")
            .expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to read stdout");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    assert_eq!(exit_code, 0, "Should exit 0 (fail open)");

    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Should allow (fail open)"
    );
    assert_eq!(
        response["metadata"]["error"], true,
        "Should indicate error in metadata"
    );
}
```

## Test Data & Fixtures

**Location:**
- Inline JSON fixtures in test files (via `serde_json::json!()` macro)
- Example configs embedded via `include_str!()`: `let example = include_str!("../../../examples/rigor.yaml");`
- Test claims created with builder functions

**Example from `benches/hook_latency.rs`:**
```rust
fn create_test_claims() -> Vec<Claim> {
    vec![
        Claim::new(
            "The regorus library provides streaming evaluation support".to_string(),
            0.85,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 0,
                sentence_index: 0,
            }),
        ),
        // ... more claims
    ]
}
```

**JSON Fixtures:**
Tests build input JSON inline:
```rust
let input = json!({
    "session_id": "test-session",
    "transcript_path": temp.path().join("transcript.jsonl").to_string_lossy(),
    "cwd": temp.path().to_string_lossy(),
    "permission_mode": "default",
    "hook_event_name": "stop",
    "stop_hook_active": false
});
```

## Benchmark Patterns

**Framework:** `criterion` with HTML report generation

**Config:**
```toml
[[bench]]
name = "hook_latency"
harness = false

[[bench]]
name = "evaluation_only"
harness = false
```

**Benchmark Structure (from `hook_latency.rs`):**
```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn build_constraint_meta(
    config: &rigor::constraint::types::RigorConfig,
) -> HashMap<String, ConstraintMeta> {
    // ... builds test data ...
}

fn hook_latency_benchmark(c: &mut Criterion) {
    c.bench_function("full_hook_pipeline", |b| {
        b.iter(|| {
            // Benchmark code here
        })
    });
}

criterion_group!(benches, hook_latency_benchmark);
criterion_main!(benches);
```

**Targets:**
- `hook_latency.rs` — measures complete pipeline (goal: <100ms mean)
- `evaluation_only.rs` — isolates constraint evaluation performance

## Coverage

**Requirements:** None enforced (no coverage targets configured)

**View Coverage:**
```bash
# No built-in coverage setup; would require external tool like tarpaulin
cargo tarpaulin --out Html
```

## Test Types

**Unit Tests:**
- None explicitly visible in this codebase
- Architecture favors integration tests over unit tests
- Can be added co-located with modules if needed (Rust convention: `#[cfg(test)] mod tests { }`)

**Integration Tests:**
- Location: `crates/rigor/tests/`
- Scope: Full pipeline from input to output
- Pattern: Subprocess invocation with JSON input/output
- Focus: Behavior verification, not internal state

**E2E Tests:**
- Tests like `true_e2e.rs`, `claim_extraction_e2e.rs` verify end-to-end behavior
- May include real configuration files, real claim extraction
- Test full constraint evaluation loop

**Performance/Benchmark Tests:**
- Criterion benchmarks in `crates/rigor/benches/`
- Measure full pipeline and isolated constraint evaluation
- Generate HTML reports: `target/criterion/`

## Test Examples

**Basic Allow Test (from `integration_hook.rs`):**
```rust
#[test]
fn test_allow_response_no_config() {
    // No rigor.lock in temp directory = always allow
    let temp = TempDir::new().unwrap();

    let input = json!({
        "session_id": "test-session",
        "transcript_path": temp.path().join("transcript.jsonl").to_string_lossy(),
        "cwd": temp.path().to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": false
    });

    let (stdout, _stderr, exit_code) = run_rigor_with_input(&input);

    assert_eq!(exit_code, 0, "Should exit with code 0");

    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_null(),
        "Should allow (decision null or absent)"
    );
    assert!(
        response["metadata"]["version"].is_string(),
        "Should include version in metadata"
    );
}
```

**Config Loading Test (from `integration_constraint.rs`):**
```rust
#[test]
fn test_valid_config_loads() {
    let temp = TempDir::new().unwrap();
    // Copy example rigor.yaml to temp dir
    let example = include_str!("../../../examples/rigor.yaml");
    fs::write(temp.path().join("rigor.yaml"), example).unwrap();

    let input = default_input(temp.path());
    let (stdout, stderr, exit_code) = run_rigor_in_dir(temp.path(), &input);

    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    // No claims = no violations = allow
    assert!(
        response.get("decision").is_null(),
        "Valid config with no claims should allow. Got: {}",
        stdout
    );
}
```

**Environment Variable Injection (from `integration_constraint.rs`):**
```rust
fn run_rigor_in_dir_with_env(
    dir: &std::path::Path,
    input_json: &Value,
    env_vars: &[(&str, &str)],
) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rigor"));
    cmd.current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (key, val) in env_vars {
        cmd.env(key, val);
    }
    // ... rest of invocation ...
}
```

## Test-Only Code

**Markers:**
```rust
#[cfg(test)]
mod tests {
    // Test-only code
}
```

**Test Dependency:**
From `Cargo.toml`:
```toml
[dev-dependencies]
tempfile = "3"
criterion = { version = "0.5", features = ["html_reports"] }
```

## Helpful Testing Env Vars

**For constraint evaluation:**
- `RIGOR_TEST_CLAIMS` — Override transcript extraction with JSON claims (for testing)
- `RIGOR_DEBUG` — Enable debug-level logging

**Example from `lib.rs`:**
```rust
let claims = match std::env::var("RIGOR_TEST_CLAIMS") {
    Ok(json_str) => {
        match serde_json::from_str::<Vec<Claim>>(&json_str) {
            Ok(claims) => {
                info!(count = claims.len(), "Loaded test claims from RIGOR_TEST_CLAIMS");
                claims
            }
            Err(e) => {
                warn!(error = %e, "Failed to parse RIGOR_TEST_CLAIMS, falling back to transcript");
                extract_claims_from_transcript(Path::new(transcript_path))?
            }
        }
    }
    Err(_) => extract_claims_from_transcript(Path::new(transcript_path))?
};
```

---

*Testing analysis: 2026-04-19*
