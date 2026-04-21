//! True end-to-end tests: transcript file -> claim extraction -> policy evaluation -> decision.
//!
//! These tests exercise the FULL pipeline without the RIGOR_TEST_CLAIMS shortcut.
//! A real transcript JSONL file is written, the rigor binary reads it, extracts claims
//! using the heuristic extractor, evaluates constraints, and produces a decision.

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

/// Run the rigor binary pointing at a real transcript file (no RIGOR_TEST_CLAIMS).
fn run_rigor_e2e(dir: &std::path::Path) -> (String, String, i32) {
    let input = json!({
        "session_id": "e2e-test",
        "transcript_path": dir.join("transcript.jsonl").to_string_lossy(),
        "cwd": dir.to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": false
    });

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rigor"));
    cmd.current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Ensure RIGOR_TEST_CLAIMS is NOT set
    cmd.env_remove("RIGOR_TEST_CLAIMS");

    let mut child = cmd.spawn().expect("Failed to spawn rigor process");
    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(input.to_string().as_bytes())
            .expect("Failed to write to stdin");
    }
    let output = child.wait_with_output().expect("Failed to read stdout");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);
    (stdout, stderr, exit_code)
}

fn parse_response(stdout: &str) -> Value {
    serde_json::from_str(stdout)
        .unwrap_or_else(|_| panic!("Failed to parse JSON response: {}", stdout))
}

/// Write a JSONL transcript file with the given assistant messages.
/// Each message is written in the simple format: {"role":"assistant","content":"..."}
/// User messages are interspersed for realism.
fn write_transcript(dir: &std::path::Path, assistant_messages: &[&str]) {
    let path = dir.join("transcript.jsonl");
    let mut file = fs::File::create(&path).expect("Failed to create transcript file");

    // Start with a user message for context
    writeln!(
        file,
        r#"{{"role":"user","content":"Tell me about this project."}}"#
    )
    .unwrap();

    for (i, msg) in assistant_messages.iter().enumerate() {
        // Escape the message for JSON embedding
        let escaped = msg
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        writeln!(file, r#"{{"role":"assistant","content":"{}"}}"#, escaped).unwrap();

        // Add a user follow-up between assistant messages (except the last)
        if i < assistant_messages.len() - 1 {
            writeln!(file, r#"{{"role":"user","content":"Tell me more."}}"#).unwrap();
        }
    }

    file.flush().unwrap();
}

// ============================================================================
// M1: True end-to-end — transcript -> extraction -> evaluation -> decision
// ============================================================================

#[test]
fn test_e2e_violation_detected_from_transcript() {
    let temp = TempDir::new().unwrap();

    // Write a simple constraint that catches claims containing "unsafe"
    let yaml = r#"
constraints:
  beliefs:
    - id: no-unsafe-claims
      epistemic_type: belief
      name: "No unsafe claims"
      description: "Detects claims about unsafe operations"
      rego: |
        violation contains v if {
          some c in input.claims
          contains(c.text, "unsafe")
          v := {
            "constraint_id": "no-unsafe-claims",
            "violated": true,
            "claims": [c.id],
            "reason": "Claim mentions unsafe operation"
          }
        }
      message: "Unsafe operation claim detected"
relations: []
"#;
    fs::write(temp.path().join("rigor.yaml"), yaml).unwrap();

    // Write a transcript where the assistant makes a claim containing "unsafe"
    write_transcript(
        temp.path(),
        &["This code uses unsafe memory operations and raw pointers."],
    );

    let (stdout, stderr, exit_code) = run_rigor_e2e(temp.path());
    assert_eq!(exit_code, 0, "stderr: {}", stderr);

    let response = parse_response(&stdout);
    assert_eq!(
        response["decision"].as_str(),
        Some("block"),
        "Should block when transcript contains claim matching constraint. Response: {}",
        stdout
    );
    assert!(
        response["reason"].is_string(),
        "Block response should include a reason. Response: {}",
        stdout
    );
}

#[test]
fn test_e2e_no_violation_from_clean_transcript() {
    let temp = TempDir::new().unwrap();

    // Write a constraint that catches "fabricated" keyword
    let yaml = r#"
constraints:
  beliefs:
    - id: no-fabricated
      epistemic_type: belief
      name: "No fabricated claims"
      description: "Detects fabricated claims"
      rego: |
        violation contains v if {
          some c in input.claims
          contains(c.text, "fabricated")
          v := {
            "constraint_id": "no-fabricated",
            "violated": true,
            "claims": [c.id],
            "reason": "Fabricated claim detected"
          }
        }
      message: "Fabricated claim detected"
relations: []
"#;
    fs::write(temp.path().join("rigor.yaml"), yaml).unwrap();

    // Write a clean transcript with no violating content
    write_transcript(
        temp.path(),
        &["The Rust compiler ensures memory safety through its ownership system."],
    );

    let (stdout, stderr, exit_code) = run_rigor_e2e(temp.path());
    assert_eq!(exit_code, 0, "stderr: {}", stderr);

    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Clean transcript should not trigger any violations. Response: {}",
        stdout
    );
}

#[test]
fn test_e2e_multiple_claims_in_transcript() {
    let temp = TempDir::new().unwrap();

    // Constraint that detects claims about streaming support
    let yaml = r#"
constraints:
  beliefs:
    - id: no-streaming
      epistemic_type: belief
      name: "No streaming claims"
      description: "Detects false streaming claims"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match("(?i)supports streaming", c.text)
          v := {
            "constraint_id": "no-streaming",
            "violated": true,
            "claims": [c.id],
            "reason": "False streaming claim"
          }
        }
      message: "False streaming claim"
relations: []
"#;
    fs::write(temp.path().join("rigor.yaml"), yaml).unwrap();

    // Write a transcript with multiple assistant messages; only the latest is extracted
    // by the heuristic extractor
    write_transcript(
        temp.path(),
        &[
            "The library is well tested and documented.",
            "The library supports streaming evaluation and real-time processing.",
        ],
    );

    let (stdout, stderr, exit_code) = run_rigor_e2e(temp.path());
    assert_eq!(exit_code, 0, "stderr: {}", stderr);

    let response = parse_response(&stdout);
    assert_eq!(
        response["decision"].as_str(),
        Some("block"),
        "Should block when latest assistant message contains violating claim. Response: {}",
        stdout
    );
}

#[test]
fn test_e2e_empty_transcript_allows() {
    let temp = TempDir::new().unwrap();

    let yaml = r#"
constraints:
  beliefs:
    - id: always-fire
      epistemic_type: belief
      name: "Always fire"
      description: "Fires on any claim"
      rego: |
        violation contains v if {
          some c in input.claims
          v := {
            "constraint_id": "always-fire",
            "violated": true,
            "claims": [c.id],
            "reason": "Fires on everything"
          }
        }
      message: "Always fires"
relations: []
"#;
    fs::write(temp.path().join("rigor.yaml"), yaml).unwrap();

    // Empty transcript file
    fs::write(temp.path().join("transcript.jsonl"), "").unwrap();

    let (stdout, stderr, exit_code) = run_rigor_e2e(temp.path());
    assert_eq!(exit_code, 0, "stderr: {}", stderr);

    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Empty transcript should produce no claims and no violations. Response: {}",
        stdout
    );
}

#[test]
fn test_e2e_user_messages_ignored() {
    let temp = TempDir::new().unwrap();

    let yaml = r#"
constraints:
  beliefs:
    - id: detect-keyword
      epistemic_type: belief
      name: "Detect keyword"
      description: "Detects a specific keyword"
      rego: |
        violation contains v if {
          some c in input.claims
          contains(c.text, "dangerous")
          v := {
            "constraint_id": "detect-keyword",
            "violated": true,
            "claims": [c.id],
            "reason": "Dangerous keyword found"
          }
        }
      message: "Keyword detected"
relations: []
"#;
    fs::write(temp.path().join("rigor.yaml"), yaml).unwrap();

    // Transcript where only the user says "dangerous", assistant says something safe
    let path = temp.path().join("transcript.jsonl");
    let mut file = fs::File::create(&path).unwrap();
    writeln!(
        file,
        r#"{{"role":"user","content":"This is a dangerous operation."}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"role":"assistant","content":"The operation is safe and well-tested."}}"#
    )
    .unwrap();
    file.flush().unwrap();

    let (stdout, stderr, exit_code) = run_rigor_e2e(temp.path());
    assert_eq!(exit_code, 0, "stderr: {}", stderr);

    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "User messages should be ignored during claim extraction. Response: {}",
        stdout
    );
}

#[test]
fn test_e2e_metadata_includes_counts() {
    let temp = TempDir::new().unwrap();

    let yaml = r#"
constraints:
  beliefs:
    - id: count-check
      epistemic_type: belief
      name: "Count check"
      description: "Simple constraint for metadata verification"
      rego: |
        violation contains v if {
          some c in input.claims
          contains(c.text, "TRIGGER_WORD_UNLIKELY_IN_NORMAL_TEXT_XYZ")
          v := {
            "constraint_id": "count-check",
            "violated": true,
            "claims": [c.id],
            "reason": "Triggered"
          }
        }
      message: "Count check"
relations: []
"#;
    fs::write(temp.path().join("rigor.yaml"), yaml).unwrap();

    write_transcript(
        temp.path(),
        &["The library provides basic functionality for evaluation."],
    );

    let (stdout, stderr, exit_code) = run_rigor_e2e(temp.path());
    assert_eq!(exit_code, 0, "stderr: {}", stderr);

    let response = parse_response(&stdout);
    // Should have constraint_count of 1
    assert_eq!(
        response["metadata"]["constraint_count"].as_u64(),
        Some(1),
        "Metadata should report 1 constraint. Response: {}",
        stdout
    );
    // Should have claim_count > 0 (extracted from transcript)
    let claim_count = response["metadata"]["claim_count"].as_u64().unwrap_or(0);
    assert!(
        claim_count > 0,
        "Metadata should report extracted claims. Response: {}",
        stdout
    );
}

#[test]
fn test_e2e_production_config_with_realistic_transcript() {
    let temp = TempDir::new().unwrap();

    // Use the production rigor.yaml
    let production_yaml = include_str!("../../../rigor.yaml");
    fs::write(temp.path().join("rigor.yaml"), production_yaml).unwrap();

    // Write a transcript with a realistic violating assistant message.
    // This claim should trigger no-fabricated-apis (mentions regorus streaming/async).
    // Note: The heuristic extractor assigns confidence based on definitive markers.
    // Words like "is", "does", "has" (word-boundary) give 0.9 confidence.
    // The Rego rule requires confidence > 0.8, so we must include a definitive marker.
    write_transcript(
        temp.path(),
        &["regorus is capable of streaming evaluation and async processing for large policy sets."],
    );

    let (stdout, stderr, exit_code) = run_rigor_e2e(temp.path());
    assert_eq!(exit_code, 0, "stderr: {}", stderr);

    let response = parse_response(&stdout);
    // The heuristic extractor should pick up this claim, and no-fabricated-apis should fire
    let decision = response["decision"].as_str();
    let reason = response
        .get("reason")
        .and_then(|r| r.as_str())
        .unwrap_or("");
    assert!(
        decision == Some("block") || reason.contains("rigor warning"),
        "Production config should detect fabricated regorus capability in real transcript. Response: {}",
        stdout
    );
}
