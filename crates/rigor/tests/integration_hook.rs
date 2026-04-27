#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! Integration tests for the Rigor stop hook.
//!
//! These tests verify the complete flow from stdin JSON to stdout JSON response.

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

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
        response.get("decision").is_none() || response["decision"].is_null(),
        "Should allow (decision null or absent)"
    );
    assert!(
        response["metadata"]["version"].is_string(),
        "Should include version in metadata"
    );
}

#[test]
fn test_stop_hook_active_allows_immediately() {
    // When stop_hook_active=true, must allow to prevent infinite loops
    let input = json!({
        "session_id": "test-session",
        "transcript_path": "/tmp/transcript.jsonl",
        "cwd": "/tmp",
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": true
    });

    let (stdout, stderr, exit_code) = run_rigor_with_input(&input);

    assert_eq!(exit_code, 0, "Should exit with code 0");

    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Must allow when stop_hook_active=true"
    );

    // Should log warning about stop_hook_active
    assert!(
        stderr.contains("stop_hook_active") || stderr.contains("allowing"),
        "Should log about stop_hook_active handling"
    );
}

#[test]
fn test_allow_with_rigor_lock() {
    // With rigor.lock present, Phase 1 still allows (no constraint evaluation yet)
    let temp = TempDir::new().unwrap();
    let lock_path = temp.path().join("rigor.lock");
    fs::write(&lock_path, "# Phase 1 placeholder\nconstraints: []").unwrap();

    let input = json!({
        "session_id": "test-session",
        "transcript_path": temp.path().join("transcript.jsonl").to_string_lossy(),
        "cwd": temp.path().to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": false
    });

    let (stdout, stderr, exit_code) = run_rigor_with_input(&input);

    assert_eq!(exit_code, 0, "Should exit with code 0");

    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Phase 1 always allows (constraint eval in Phase 2)"
    );

    // Should log that config was found
    assert!(
        stderr.contains("rigor.lock") || stderr.contains("Found"),
        "Should log that config was found"
    );
}

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
    assert!(
        response["metadata"]["error_message"].is_string(),
        "Should include error message"
    );
}

#[test]
fn test_missing_fields_fails_open() {
    // Partial JSON (missing required fields) should fail open
    let input = json!({
        "session_id": "test"
        // Missing other required fields
    });

    let (stdout, _stderr, exit_code) = run_rigor_with_input(&input);

    assert_eq!(exit_code, 0, "Should exit 0 (fail open)");

    let response = parse_response(&stdout);
    assert_eq!(
        response["metadata"]["error"], true,
        "Should indicate error in metadata"
    );
}

#[test]
fn test_metadata_includes_version() {
    let input = json!({
        "session_id": "test-session",
        "transcript_path": "/tmp/transcript.jsonl",
        "cwd": "/tmp",
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": false
    });

    let (stdout, _stderr, _exit_code) = run_rigor_with_input(&input);
    let response = parse_response(&stdout);

    let version = response["metadata"]["version"].as_str().unwrap();
    assert!(!version.is_empty(), "Version should not be empty");
    assert!(version.starts_with("0."), "Version should be 0.x.x");
}
