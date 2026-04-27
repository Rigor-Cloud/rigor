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

// =============================================================================
// H6: Subprocess failure-mode tests.
//
// These tests assert that the rigor binary handles three categories of bad
// input gracefully — without panicking, hanging, or silently producing
// malformed output. They lock in the binary's CURRENT fail-open behaviour:
//
//   1. Missing rigor.yaml      -> exit 0, allow (no decision field).
//   2. Malformed YAML          -> exit 0, allow (load_rigor_config fails,
//                                 evaluate_constraints fails open).
//   3. Invalid Rego in YAML    -> exit 0, allow (PolicyEngine::new logs &
//                                 skips invalid Rego per-constraint, no
//                                 violations produced).
//
// If any of these regress (e.g. the binary starts panicking, exiting non-zero,
// or emitting a Block decision on a config error), the corresponding test
// will fail loudly. Update the assertion deliberately when changing policy.
// =============================================================================

/// rigor.yaml that is syntactically broken YAML (unbalanced braces / colons).
/// `load_rigor_config` should fail to parse this; the binary must fail open.
const MALFORMED_YAML: &str = "constraints: { invalid: yaml: here\n  beliefs: [\n";

/// rigor.yaml that is structurally valid YAML and passes the schema validator
/// (non-empty rego field) but contains Rego source that regorus cannot parse.
/// `PolicyEngine::new` must skip the constraint with a warning, leaving zero
/// loaded constraints — so evaluation produces zero violations.
const INVALID_REGO_YAML: &str = r#"constraints:
  beliefs:
    - id: e2e-broken-rego
      epistemic_type: belief
      name: E2E Broken Rego
      description: Constraint whose Rego body is intentionally unparseable
      rego: |
        this is not valid rego at all $$$ ###
      message: This should never fire because the Rego is broken
  justifications: []
  defeaters: []
"#;

/// Any-claims JSON used by the invalid-Rego test to force the evaluation
/// path. Without `RIGOR_TEST_CLAIMS` the binary may short-circuit before
/// PolicyEngine::new runs.
const ANY_CLAIMS: &str = r#"[{"id":"c1","text":"some text that should not match anything","confidence":0.9,"claim_type":"assertion"}]"#;

#[test]
fn stop_hook_handles_missing_rigor_yaml() {
    // IsolatedHome creates a fresh tempdir but does NOT write a rigor.yaml.
    // The binary should detect the absent config, drain stdin, and emit a
    // fail-open allow response. It must not panic or exit non-zero.
    let home = IsolatedHome::new();

    let input = default_hook_input(&home);
    let (stdout, stderr, exit_code) = run_rigor(&home, &input);

    assert_eq!(
        exit_code, 0,
        "rigor should exit 0 when rigor.yaml is missing (fail-open). stderr: {}",
        stderr,
    );

    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Missing rigor.yaml must produce no decision (allow). Got: {}",
        stdout,
    );
}

#[test]
fn stop_hook_handles_malformed_yaml() {
    // The YAML parser inside load_rigor_config will reject this content.
    // evaluate_constraints catches the error, logs it, and falls open.
    let home = IsolatedHome::new();
    home.write_rigor_yaml(MALFORMED_YAML);

    let input = default_hook_input(&home);
    let (stdout, stderr, exit_code) = run_rigor(&home, &input);

    assert_eq!(
        exit_code, 0,
        "rigor should exit 0 on malformed YAML (fail-open). stderr: {}",
        stderr,
    );

    // Output must still be valid JSON the hook can parse — no panic, no
    // half-written output. parse_response panics on bad JSON so this also
    // serves as a structural assertion.
    let response = parse_response(&stdout);
    assert!(
        response.get("decision").is_none() || response["decision"].is_null(),
        "Malformed YAML must fail open (no decision). Got: {}",
        stdout,
    );

    // The binary should have logged something explanatory to stderr —
    // either the YAML parse error or the fail-open notice. We don't pin
    // the exact wording but require it to mention rigor.yaml or YAML/parse.
    let stderr_lower = stderr.to_lowercase();
    assert!(
        stderr_lower.contains("rigor.yaml")
            || stderr_lower.contains("yaml")
            || stderr_lower.contains("parse")
            || stderr_lower.contains("fail"),
        "Expected diagnostic message about the YAML parse failure. stderr: {}",
        stderr,
    );
}

#[test]
fn stop_hook_handles_invalid_rego() {
    // YAML schema is valid (non-empty rego field), so ConstraintValidator
    // accepts it. PolicyEngine::new then tries to compile each constraint's
    // Rego via regorus — invalid Rego is logged and skipped (fail-open),
    // leaving zero loaded constraints. The hook must complete normally and
    // report zero violations / no block decision.
    let home = IsolatedHome::new();
    home.write_rigor_yaml(INVALID_REGO_YAML);

    let input = default_hook_input(&home);
    // Use run_rigor_with_claims so RIGOR_TEST_CLAIMS forces the evaluation
    // path (otherwise transcript-extraction would just return zero claims
    // and we'd never exercise the broken-Rego compilation branch).
    let (stdout, stderr, exit_code) = run_rigor_with_claims(&home, &input, ANY_CLAIMS);

    assert_eq!(
        exit_code, 0,
        "rigor should exit 0 when a constraint has invalid Rego (fail-open). stderr: {}",
        stderr,
    );

    let response = parse_response(&stdout);

    // Either fail-open allow (no decision) OR an explicit error decision —
    // both are acceptable per the H6 spec. Block is NOT acceptable: a
    // broken constraint must never produce a violation.
    let decision = response
        .get("decision")
        .and_then(|d| d.as_str());
    assert!(
        decision != Some("block"),
        "Invalid Rego must not produce a block decision. Got: {}",
        stdout,
    );

    // If the binary fails open, violation count is 0. The metadata
    // constraint_count for an all-broken-Rego config is 0 (the constraint
    // was skipped). We assert one of the two acceptable shapes.
    let constraint_count = response
        .get("metadata")
        .and_then(|m| m.get("constraint_count"))
        .and_then(|c| c.as_u64());
    if let Some(cc) = constraint_count {
        // Allow either:
        //   - cc == 0 (constraint was dropped during PolicyEngine::new), or
        //   - cc > 0 with no decision (constraint counted but no violations).
        // Reject only the impossible state of "block with broken Rego",
        // already asserted above. Nothing further to enforce on cc here.
        let _ = cc;
    }
}
