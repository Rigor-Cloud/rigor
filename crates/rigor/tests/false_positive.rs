#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! A3 — False-positive precision tests.
//!
//! Claims that look like violations but are correct speech — negated,
//! quoted, historical, meta-discussion, comparative, user-question echo.
//! None should trigger a block or warn.
//!
//! Fixtures live under `tests/fixtures/false_positive/<constraint_id>/<scenario>.json`.
//! Each fixture's `expected_decision` should be `"none"` or `"allow"`.

use std::path::Path;

mod support;

#[test]
fn false_positive_probes_do_not_fire() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/false_positive");
    let fixtures = support::walk_fixtures(&base);

    if fixtures.is_empty() {
        eprintln!(
            "false_positive: no fixtures found under {}. Fixtures are added in PR-2.6 step 3.",
            base.display()
        );
        return;
    }

    let mut failures = Vec::new();

    for (path, fixture) in &fixtures {
        if !matches!(fixture.expected_decision.as_str(), "none" | "allow") {
            failures.push(format!(
                "{}: false_positive fixtures must have expected_decision ∈ {{\"none\", \"allow\"}}, got {:?}",
                path.display(),
                fixture.expected_decision
            ));
            continue;
        }

        let (stdout, stderr, exit_code) = support::run_rigor_with_fixture(fixture);

        if exit_code != 0 {
            failures.push(format!(
                "{}: rigor exited with code {} — stderr: {}",
                path.display(),
                exit_code,
                stderr.trim()
            ));
            continue;
        }

        let actual = support::decision_or_none(&stdout);
        // Both "none" and "allow" are acceptable non-block outcomes.
        let is_pass = matches!(actual.as_str(), "none" | "allow");
        if !is_pass {
            failures.push(format!(
                "{}: false-positive fired — expected none/allow, got {:?} (scenario: {})",
                path.display(),
                actual,
                fixture.notes.as_deref().unwrap_or("(none)")
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{}",
        support::format_failures(&failures)
    );
}
