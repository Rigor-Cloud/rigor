//! Shared test harness for PR-2.6 coverage tests.
//!
//! - Fixture schema for `firing_matrix` and `false_positive` runs.
//! - `run_rigor_with_fixture` — spawn the rigor binary with a synthetic claim
//!   via `RIGOR_TEST_CLAIMS`, capture the JSON hook response.
//! - `require_openrouter!` — macro that auto-skips real-LLM tests when the
//!   `OPENROUTER_API_KEY` env var is absent.
//!
//! Included by test files via `mod support;`. Unused items are `#[allow(dead_code)]`
//! because different test files consume different subsets of the API.

#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{json, Value};

// =============================================================================
// Fixture schema
// =============================================================================

/// One test fixture. Stored on disk as JSON.
///
/// Schema (permissive — missing fields take defaults):
///
/// ```json
/// {
///   "text": "Rust uses a mark-and-sweep garbage collector",
///   "confidence": 0.9,
///   "claim_type": "assertion",
///   "expected_decision": "block",
///   "notes": "optional — human-readable description"
/// }
/// ```
///
/// `expected_decision` values: `"block"`, `"warn"`, `"allow"`, or `"none"` when
/// the hook returns no decision field (i.e. no violations triggered).
#[derive(Debug, Deserialize)]
pub struct Fixture {
    pub text: String,
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    #[serde(default = "default_claim_type")]
    pub claim_type: String,
    pub expected_decision: String,
    #[serde(default)]
    pub notes: Option<String>,
}

fn default_confidence() -> f64 {
    0.9
}

fn default_claim_type() -> String {
    "assertion".to_string()
}

/// Load a fixture from disk. Panics with the file path on parse error so
/// the failing fixture is obvious in test output.
pub fn load_fixture(path: &Path) -> Fixture {
    let bytes = fs::read(path).unwrap_or_else(|e| {
        panic!("failed to read fixture {}: {}", path.display(), e);
    });
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!("failed to parse fixture {}: {}", path.display(), e);
    })
}

// =============================================================================
// Running rigor
// =============================================================================

/// Spawn the rigor binary with a single-claim `RIGOR_TEST_CLAIMS` payload
/// derived from the fixture, using the production rigor.yaml. Returns
/// `(stdout, stderr, exit_code)`.
pub fn run_rigor_with_fixture(fixture: &Fixture) -> (String, String, i32) {
    let temp = tempfile::TempDir::new().expect("tempdir");
    // Copy the production rigor.yaml into the temp dir so find_rigor_yaml
    // locates it from `cwd = temp`.
    let rigor_yaml = production_rigor_yaml();
    fs::write(temp.path().join("rigor.yaml"), rigor_yaml).expect("write rigor.yaml");

    let claims = json!([{
        "id": "c1",
        "text": fixture.text,
        "confidence": fixture.confidence,
        "claim_type": fixture.claim_type,
    }]);

    let input = json!({
        "session_id": "pr-2.6-fixture",
        "transcript_path": temp.path().join("transcript.jsonl").to_string_lossy(),
        "cwd": temp.path().to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": false,
    });

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rigor"));
    cmd.current_dir(temp.path())
        .env("RIGOR_TEST_CLAIMS", claims.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn rigor");
    child
        .stdin
        .as_mut()
        .expect("open stdin")
        .write_all(input.to_string().as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait rigor");

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

/// The production rigor.yaml content, embedded at compile time so fixtures
/// exercise the same constraint set that ships with the binary.
pub fn production_rigor_yaml() -> &'static str {
    include_str!("../../../../rigor.yaml")
}

/// Extract the `decision` field from the hook JSON response.
///
/// Returns `None` when the response has no decision (i.e. no violations
/// fired — rigor's "allow" can manifest as either `decision: "allow"` or
/// an absent `decision` field, depending on which code path emitted it).
pub fn extract_decision(stdout: &str) -> Option<String> {
    let value: Value = serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("failed to parse hook response: {}\nstdout: {}", e, stdout));
    value
        .get("decision")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
}

/// Normalize "no decision field" to the sentinel `"none"` so fixtures can
/// express "should not block or warn" declaratively.
pub fn decision_or_none(stdout: &str) -> String {
    extract_decision(stdout).unwrap_or_else(|| "none".to_string())
}

// =============================================================================
// Fixture directory walker
// =============================================================================

/// Discover every fixture file under `base_dir`, yielding
/// `(relative_path, loaded_fixture)` pairs.
///
/// Layout:
///     base_dir/
///       <constraint_id>/
///         should_fire.json
///         should_not_fire.json
///         <any-other-fixture>.json
pub fn walk_fixtures(base_dir: &Path) -> Vec<(PathBuf, Fixture)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(base_dir) else {
        panic!("fixture dir {} does not exist", base_dir.display());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Recurse one level — fixtures live under <base>/<constraint_id>/*.json
            if let Ok(sub_entries) = fs::read_dir(&path) {
                for sub in sub_entries.flatten() {
                    let sub_path = sub.path();
                    if sub_path.extension().map(|e| e == "json").unwrap_or(false) {
                        let fixture = load_fixture(&sub_path);
                        out.push((sub_path, fixture));
                    }
                }
            }
        }
    }
    // Deterministic order so test output is stable across runs.
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

// =============================================================================
// Real-LLM gate
// =============================================================================

/// Macro to auto-skip a test when `OPENROUTER_API_KEY` is unset.
///
/// Usage:
/// ```ignore
/// #[test]
/// fn e1_real_llm_roundtrip() {
///     require_openrouter!();
///     // ... test body ...
/// }
/// ```
#[macro_export]
macro_rules! require_openrouter {
    () => {
        if std::env::var("OPENROUTER_API_KEY").is_err() {
            eprintln!("skip: OPENROUTER_API_KEY not set");
            return;
        }
    };
}

// =============================================================================
// Report formatting
// =============================================================================

/// Format a list of failures for a parametrized test, including the fixture
/// path so the operator knows exactly which fixture broke.
pub fn format_failures(failures: &[String]) -> String {
    if failures.is_empty() {
        return String::new();
    }
    let mut out = format!("\n{} fixture failure(s):\n", failures.len());
    for f in failures {
        out.push_str("  - ");
        out.push_str(f);
        out.push('\n');
    }
    out
}
