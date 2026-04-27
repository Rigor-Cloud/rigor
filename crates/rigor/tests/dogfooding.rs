#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! Dogfooding tests: Rigor constrains itself.
//!
//! Loads the PRODUCTION rigor.yaml from the project root and verifies
//! constraints fire correctly on crafted claims.
//!
//! These tests adapt to whatever constraints exist in rigor.yaml —
//! after `rigor init` (tier 1+2) or after `/rigor:map` (tier 3).

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

/// The production rigor.yaml, embedded at compile time.
const PRODUCTION_RIGOR_YAML: &str = include_str!("../../../rigor.yaml");

fn run_rigor_with_claims(dir: &std::path::Path, claims: &Value) -> (String, String, i32) {
    let input = json!({
        "session_id": "dogfood-test",
        "transcript_path": dir.join("transcript.jsonl").to_string_lossy(),
        "cwd": dir.to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": false
    });

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rigor"));
    cmd.current_dir(dir)
        .env("RIGOR_TEST_CLAIMS", claims.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("Failed to spawn rigor process");
    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(input.to_string().as_bytes())
            .expect("Failed to write to stdin");
    }
    let output = child.wait_with_output().expect("Failed to read stdout");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

fn parse_response(stdout: &str) -> Value {
    serde_json::from_str(stdout)
        .unwrap_or_else(|_| panic!("Failed to parse JSON response: {}", stdout))
}

fn setup_production_config() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("rigor.yaml"), PRODUCTION_RIGOR_YAML).unwrap();
    temp
}

// ============================================================================
// Tier 1: Rust language defaults
// ============================================================================

#[test]
fn test_dogfood_rust_no_gc_fires() {
    let temp = setup_production_config();
    let claims = json!([{
        "id": "c1",
        "text": "Rust uses a garbage collector for memory management with mark and sweep",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let (stdout, stderr, exit_code) = run_rigor_with_claims(temp.path(), &claims);
    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    assert_eq!(
        response["decision"].as_str(),
        Some("block"),
        "rust-no-gc should block on GC claim. Response: {}",
        stdout
    );
}

#[test]
fn test_dogfood_rust_no_null_fires() {
    let temp = setup_production_config();
    let claims = json!([{
        "id": "c1",
        "text": "Rust has null pointers like C and you can get a NullPointerException",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let (stdout, stderr, exit_code) = run_rigor_with_claims(temp.path(), &claims);
    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    assert_eq!(
        response["decision"].as_str(),
        Some("block"),
        "rust-no-null should block on null pointer claim. Response: {}",
        stdout
    );
}

#[test]
fn test_dogfood_rust_no_exceptions_fires() {
    let temp = setup_production_config();
    let claims = json!([{
        "id": "c1",
        "text": "Rust uses try catch blocks for exception handling",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let (stdout, stderr, exit_code) = run_rigor_with_claims(temp.path(), &claims);
    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    assert_eq!(
        response["decision"].as_str(),
        Some("block"),
        "rust-no-exceptions should block on try/catch claim. Response: {}",
        stdout
    );
}

#[test]
fn test_dogfood_rust_no_inheritance_fires() {
    let temp = setup_production_config();
    let claims = json!([{
        "id": "c1",
        "text": "Rust uses class inheritance where subclass extends the superclass",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let (stdout, stderr, exit_code) = run_rigor_with_claims(temp.path(), &claims);
    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    assert_eq!(
        response["decision"].as_str(),
        Some("block"),
        "rust-no-inheritance should block on class claim. Response: {}",
        stdout
    );
}

// ============================================================================
// Tier 2: Dependency constraints
// ============================================================================

#[test]
fn test_dogfood_regorus_subset() {
    let temp = setup_production_config();
    let claims = json!([{
        "id": "c1",
        "text": "regorus supports http.send for outbound HTTP calls from Rego policies",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let (stdout, stderr, exit_code) = run_rigor_with_claims(temp.path(), &claims);
    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    assert_eq!(
        response["decision"].as_str(),
        Some("block"),
        "regorus-capabilities should block. Response: {}",
        stdout
    );
}

#[test]
fn test_dogfood_axum_not_actix() {
    let temp = setup_production_config();
    let claims = json!([{
        "id": "c1",
        "text": "axum is built on actix-web and uses actix handlers",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let (stdout, stderr, exit_code) = run_rigor_with_claims(temp.path(), &claims);
    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    assert_eq!(
        response["decision"].as_str(),
        Some("block"),
        "axum-is-tower-based should block. Response: {}",
        stdout
    );
}

#[test]
fn test_dogfood_tokio_not_green_threads() {
    let temp = setup_production_config();
    let claims = json!([{
        "id": "c1",
        "text": "tokio uses green threads like goroutines for preemptive scheduling",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let (stdout, stderr, exit_code) = run_rigor_with_claims(temp.path(), &claims);
    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    assert_eq!(
        response["decision"].as_str(),
        Some("block"),
        "tokio-is-async-runtime should block. Response: {}",
        stdout
    );
}

// ============================================================================
// Negative: truthful claims must NOT trigger
// ============================================================================

#[test]
fn test_dogfood_truthful_rust_claim_allowed() {
    let temp = setup_production_config();
    let claims = json!([{
        "id": "c1",
        "text": "Rust uses ownership and borrowing for memory management without a garbage collector",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let (stdout, stderr, exit_code) = run_rigor_with_claims(temp.path(), &claims);
    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Truthful Rust claim should NOT trigger. Response: {}",
        stdout
    );
}

#[test]
fn test_dogfood_truthful_regorus_claim_allowed() {
    let temp = setup_production_config();
    let claims = json!([{
        "id": "c1",
        "text": "Regorus is a Rego evaluator implemented in Rust",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let (stdout, stderr, exit_code) = run_rigor_with_claims(temp.path(), &claims);
    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Truthful regorus claim should NOT trigger. Response: {}",
        stdout
    );
}

// ============================================================================
// Meta: constraint count
// ============================================================================

#[test]
fn test_dogfood_production_config_loads() {
    let temp = setup_production_config();
    let claims = json!([]);

    let (stdout, stderr, exit_code) = run_rigor_with_claims(temp.path(), &claims);
    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    let count = response["metadata"]["constraint_count"]
        .as_u64()
        .unwrap_or(0);
    assert!(
        count >= 9,
        "Should have at least 9 constraints (5 lang + 4 dep). Got: {}",
        count
    );
}
