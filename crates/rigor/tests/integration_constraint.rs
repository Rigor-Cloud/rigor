//! Integration tests for the constraint evaluation pipeline.
//!
//! Tests the full flow: rigor.yaml -> loader -> graph -> engine -> collector -> decision.

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

/// Helper to run rigor binary with JSON input in a specific working directory.
fn run_rigor_in_dir(dir: &std::path::Path, input_json: &Value) -> (String, String, i32) {
    run_rigor_in_dir_with_env(dir, input_json, &[])
}

/// Helper to run rigor binary with JSON input, working directory, and extra env vars.
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

fn parse_response(stdout: &str) -> Value {
    serde_json::from_str(stdout)
        .unwrap_or_else(|_| panic!("Failed to parse JSON response: {}", stdout))
}

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
        response.get("decision").is_none() || response["decision"].is_null(),
        "Valid config with no claims should allow. Got: {}",
        stdout
    );
    // Should report constraint count
    assert!(
        response["metadata"]["constraint_count"].as_u64().unwrap() > 0,
        "Should report constraints loaded"
    );
}

#[test]
fn test_block_on_violation() {
    let temp = TempDir::new().unwrap();

    // Create a rigor.yaml with a constraint that always violates
    let yaml = r#"
constraints:
  beliefs:
    - id: always-fail
      epistemic_type: belief
      name: "Always fails"
      description: "This constraint always produces a violation"
      rego: |
        violation contains v if {
          some c in input.claims
          v := {"constraint_id": "always-fail", "violated": true, "claims": [c.id], "reason": "Always violates"}
        }
      message: "Always fails"
relations: []
"#;
    fs::write(temp.path().join("rigor.yaml"), yaml).unwrap();

    // Provide test claims via env var
    let claims = json!([{
        "id": "c1",
        "text": "test claim",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let input = default_input(temp.path());
    let (stdout, stderr, exit_code) = run_rigor_in_dir_with_env(
        temp.path(),
        &input,
        &[("RIGOR_TEST_CLAIMS", &claims.to_string())],
    );

    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);
    assert_eq!(
        response["decision"].as_str(),
        Some("block"),
        "High-strength violation should block. Got: {}",
        stdout
    );
    assert!(
        response["reason"].is_string(),
        "Block response should include reason"
    );
}

#[test]
fn test_warn_on_medium_violation() {
    let temp = TempDir::new().unwrap();

    // DF-QuAD product aggregation: need strength in warn range [0.4, 0.7).
    // Strategy: belief (0.8) attacked by defeater d1, but d1 is itself
    // attacked by another defeater d2, weakening d1's effective strength.
    //
    // d2 (0.7) attacks d1 (0.7):
    //   d1: attack_prod = 1-0.7 = 0.3, combined = 0.3-1.0 = -0.7
    //   d1 strength = 0.7 * (1-0.7) = 0.21
    //
    // warn-belief (0.8) attacked by d1 (0.21):
    //   attack_prod = 1-0.21 = 0.79, combined = 0.79-1.0 = -0.21
    //   belief strength = 0.8 * (1-0.21) = 0.632 → warn range!
    let yaml = r#"
constraints:
  beliefs:
    - id: warn-belief
      epistemic_type: belief
      name: "Warn level belief"
      description: "This belief has medium strength due to weakened attacker"
      rego: |
        violation contains v if {
          some c in input.claims
          v := {"constraint_id": "warn-belief", "violated": true, "claims": [c.id], "reason": "Medium severity violation"}
        }
      message: "Medium severity"
  defeaters:
    - id: d1
      epistemic_type: defeater
      name: "Weakened defeater"
      description: "Attacked by d2, reducing its effective strength"
      rego: |
        violation contains v if { false }
      message: "N/A"
    - id: d2
      epistemic_type: defeater
      name: "Counter defeater"
      description: "Attacks d1 to weaken it"
      rego: |
        violation contains v if { false }
      message: "N/A"
relations:
  - from: d1
    to: warn-belief
    relation_type: attacks
  - from: d2
    to: d1
    relation_type: attacks
"#;
    fs::write(temp.path().join("rigor.yaml"), yaml).unwrap();

    let claims = json!([{
        "id": "c1",
        "text": "test claim",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let input = default_input(temp.path());
    let (stdout, stderr, exit_code) = run_rigor_in_dir_with_env(
        temp.path(),
        &input,
        &[("RIGOR_TEST_CLAIMS", &claims.to_string())],
    );

    assert_eq!(exit_code, 0, "stderr: {}", stderr);
    let response = parse_response(&stdout);

    // Should NOT be block (strength should be in warn range)
    assert_ne!(
        response["decision"].as_str(),
        Some("block"),
        "Medium-strength violation should not block. Response: {}",
        stdout
    );
    // Should allow with a warning reason
    assert!(
        response.get("reason").is_some() && response["reason"].is_string(),
        "Should include warning reason. Response: {}",
        stdout
    );
    let reason = response["reason"].as_str().unwrap();
    assert!(
        reason.contains("rigor warning"),
        "Reason should be a rigor warning. Got: {}",
        reason
    );
}

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

#[test]
fn test_invalid_rego_fails_open() {
    let temp = TempDir::new().unwrap();

    let yaml = r#"
constraints:
  beliefs:
    - id: bad-rego
      epistemic_type: belief
      name: "Bad Rego"
      description: "Invalid Rego syntax"
      rego: "this is not valid rego {{{{"
      message: "Bad"
relations: []
"#;
    fs::write(temp.path().join("rigor.yaml"), yaml).unwrap();

    let claims = json!([{
        "id": "c1",
        "text": "test claim",
        "confidence": 0.9,
        "claim_type": "assertion"
    }]);

    let input = default_input(temp.path());
    let (stdout, _stderr, exit_code) = run_rigor_in_dir_with_env(
        temp.path(),
        &input,
        &[("RIGOR_TEST_CLAIMS", &claims.to_string())],
    );

    assert_eq!(exit_code, 0, "Should exit 0 (fail open)");
    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Invalid Rego should fail open (allow). Got: {}",
        stdout
    );
}

#[test]
fn test_rigor_test_claims_empty_array() {
    let temp = TempDir::new().unwrap();
    let yaml = r#"
constraints:
  beliefs:
    - id: always-fail
      epistemic_type: belief
      name: "Always fails"
      description: "This constraint always produces a violation"
      rego: |
        violation contains v if {
          some c in input.claims
          v := {"constraint_id": "always-fail", "violated": true, "claims": [c.id], "reason": "Always violates"}
        }
      message: "Always fails"
relations: []
"#;
    fs::write(temp.path().join("rigor.yaml"), yaml).unwrap();

    let input = default_input(temp.path());
    let (stdout, _stderr, exit_code) = run_rigor_in_dir_with_env(
        temp.path(),
        &input,
        &[("RIGOR_TEST_CLAIMS", "[]")],
    );

    assert_eq!(exit_code, 0);
    let response = parse_response(&stdout);
    // Empty claims = no violations = allow
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Empty claims should produce no violations. Got: {}",
        stdout
    );
}

#[test]
fn test_rigor_test_claims_malformed_falls_back() {
    let temp = TempDir::new().unwrap();
    // No rigor.yaml, so even if fallback extraction happens, no constraints to evaluate
    let input = default_input(temp.path());
    let (_stdout, _stderr, exit_code) = run_rigor_in_dir_with_env(
        temp.path(),
        &input,
        &[("RIGOR_TEST_CLAIMS", "not valid json at all")],
    );

    // Should not crash -- malformed RIGOR_TEST_CLAIMS falls back to transcript
    assert_eq!(exit_code, 0, "Malformed RIGOR_TEST_CLAIMS should not crash");
}
