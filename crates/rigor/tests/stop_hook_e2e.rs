//! E2E tests for the stop-hook evaluation path via rigor-harness subprocess helpers.
//!
//! Proves that the harness subprocess helpers (run_rigor, run_rigor_with_claims,
//! parse_response, default_hook_input) compose correctly with a real constraint
//! pipeline exercising the full claim extraction -> constraint evaluation -> decision flow.

use rigor_harness::{
    default_hook_input, extract_decision, parse_response, run_rigor, run_rigor_with_claims,
    IsolatedHome,
};

/// Minimal valid rigor.yaml with no constraints (always allows).
const MINIMAL_YAML: &str =
    "constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n";

/// rigor.yaml with a single belief constraint that detects `VIOLATION_MARKER` in claim text.
/// Beliefs have base strength 0.8 which exceeds the default block threshold of 0.7,
/// so a matching claim triggers a Block decision.
const KEYWORD_CONSTRAINT_YAML: &str = r#"constraints:
  beliefs:
    - id: e2e-keyword-detector
      epistemic_type: belief
      name: E2E Keyword Detector
      description: Blocks if claim text contains VIOLATION_MARKER
      rego: |
        violation contains v if {
          some c in input.claims
          contains(c.text, "VIOLATION_MARKER")
          v := {"constraint_id": "e2e-keyword-detector", "violated": true, "claims": [c.id], "reason": "keyword found"}
        }
      message: Keyword violation detected
  justifications: []
  defeaters: []
"#;

/// Claims JSON containing the keyword that triggers the constraint.
const VIOLATING_CLAIMS: &str = r#"[{"id":"c1","text":"This output contains VIOLATION_MARKER which should trigger","confidence":0.9,"claim_type":"assertion"}]"#;

/// Claims JSON that does NOT contain the trigger keyword.
const CLEAN_CLAIMS: &str = r#"[{"id":"c1","text":"This is a perfectly normal claim with no issues","confidence":0.9,"claim_type":"assertion"}]"#;

#[test]
fn stop_hook_blocks_on_matching_claim() {
    let home = IsolatedHome::new();
    home.write_rigor_yaml(KEYWORD_CONSTRAINT_YAML);

    let input = default_hook_input(&home);
    let (stdout, stderr, exit_code) = run_rigor_with_claims(&home, &input, VIOLATING_CLAIMS);

    assert_eq!(
        exit_code, 0,
        "rigor should exit 0 even on block. stderr: {}",
        stderr,
    );

    let decision = extract_decision(&stdout);
    assert_eq!(
        decision,
        Some("block".to_string()),
        "Claim with VIOLATION_MARKER should trigger block decision. stdout: {}",
        stdout,
    );

    // Also verify via parsed response
    let response = parse_response(&stdout);
    assert_eq!(
        response["decision"].as_str(),
        Some("block"),
        "Parsed decision field should be 'block'. Full response: {}",
        response,
    );
}

#[test]
fn stop_hook_allows_on_no_matching_claim() {
    let home = IsolatedHome::new();
    home.write_rigor_yaml(KEYWORD_CONSTRAINT_YAML);

    let input = default_hook_input(&home);
    let (stdout, stderr, exit_code) = run_rigor_with_claims(&home, &input, CLEAN_CLAIMS);

    assert_eq!(
        exit_code, 0,
        "rigor should exit 0 on allow. stderr: {}",
        stderr,
    );

    let decision = extract_decision(&stdout);
    assert!(
        decision.is_none(),
        "Clean claim should produce no decision (allow). Got: {:?}. stdout: {}",
        decision,
        stdout,
    );
}

#[test]
fn stop_hook_allows_with_no_constraints() {
    let home = IsolatedHome::new();
    home.write_rigor_yaml(MINIMAL_YAML);

    let input = default_hook_input(&home);
    let (stdout, stderr, exit_code) = run_rigor(&home, &input);

    assert_eq!(
        exit_code, 0,
        "rigor should exit 0 with no constraints. stderr: {}",
        stderr,
    );

    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "No constraints should produce no decision (allow). Got: {}",
        stdout,
    );
}

#[test]
fn stop_hook_metadata_includes_version() {
    let home = IsolatedHome::new();
    home.write_rigor_yaml(MINIMAL_YAML);

    let input = default_hook_input(&home);
    let (stdout, stderr, exit_code) = run_rigor(&home, &input);

    assert_eq!(
        exit_code, 0,
        "rigor should exit 0. stderr: {}",
        stderr,
    );

    let response = parse_response(&stdout);
    let version = response
        .get("metadata")
        .and_then(|m| m.get("version"))
        .and_then(|v| v.as_str());

    assert!(
        version.is_some() && !version.unwrap().is_empty(),
        "Response metadata should include a non-empty version string. Response: {}",
        response,
    );
}
