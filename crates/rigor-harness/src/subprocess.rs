use crate::home::IsolatedHome;
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

/// Locate the rigor binary.
///
/// Prefers `CARGO_BIN_EXE_rigor` (set by cargo in integration test contexts),
/// then falls back to `RIGOR_BIN` env var, then searches PATH for `rigor`.
fn rigor_bin() -> String {
    std::env::var("CARGO_BIN_EXE_rigor")
        .or_else(|_| std::env::var("RIGOR_BIN"))
        .unwrap_or_else(|_| "rigor".to_string())
}

/// Run the rigor binary with `input` JSON on stdin, using the given `IsolatedHome`.
///
/// Returns `(stdout, stderr, exit_code)`. HOME is set to the isolated path,
/// and `RIGOR_TEST_CLAIMS` is removed to ensure a clean slate.
pub fn run_rigor(home: &IsolatedHome, input: &Value) -> (String, String, i32) {
    run_rigor_inner(home, input, &[("RIGOR_TEST_CLAIMS", None)])
}

/// Run the rigor binary with a `RIGOR_TEST_CLAIMS` override.
pub fn run_rigor_with_claims(home: &IsolatedHome, input: &Value, claims_json: &str) -> (String, String, i32) {
    run_rigor_inner(home, input, &[("RIGOR_TEST_CLAIMS", Some(claims_json))])
}

/// Run the rigor binary with additional environment variables.
pub fn run_rigor_with_env(home: &IsolatedHome, input: &Value, env_vars: &[(&str, &str)]) -> (String, String, i32) {
    let env_actions: Vec<(&str, Option<&str>)> = env_vars.iter().map(|(k, v)| (*k, Some(*v))).collect();
    run_rigor_inner(home, input, &env_actions)
}

/// Internal: spawn rigor with HOME isolation and env overrides.
///
/// `env_actions` is a list of `(key, value)` pairs:
/// - `Some(val)` sets the env var
/// - `None` removes the env var
fn run_rigor_inner(home: &IsolatedHome, input: &Value, env_actions: &[(&str, Option<&str>)]) -> (String, String, i32) {
    let mut cmd = Command::new(rigor_bin());
    cmd.current_dir(&home.path)
        .env("HOME", home.home_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for &(key, value) in env_actions {
        match value {
            Some(v) => { cmd.env(key, v); }
            None => { cmd.env_remove(key); }
        }
    }

    let mut child = cmd.spawn().expect("spawn rigor binary");
    child
        .stdin
        .as_mut()
        .expect("open stdin")
        .write_all(input.to_string().as_bytes())
        .expect("write input to stdin");

    let output = child.wait_with_output().expect("wait for rigor");

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

/// Parse a JSON response from rigor stdout.
pub fn parse_response(stdout: &str) -> Value {
    serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("Failed to parse JSON response: {}\nstdout: {}", e, stdout))
}

/// Extract the `decision` field from rigor's JSON response.
pub fn extract_decision(stdout: &str) -> Option<String> {
    let value: Value = serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("Failed to parse hook response: {}\nstdout: {}", e, stdout));
    value
        .get("decision")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
}

/// Build the standard hook input JSON for a given IsolatedHome.
pub fn default_hook_input(home: &IsolatedHome) -> Value {
    serde_json::json!({
        "session_id": "harness-test",
        "transcript_path": home.path.join("transcript.jsonl").to_string_lossy().to_string(),
        "cwd": home.path.to_string_lossy().to_string(),
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": false
    })
}
