#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! A1 — Constraint firing matrix.
//!
//! For every constraint in `rigor.yaml` that ships a fixture, verify it fires
//! on a `should_fire.json` claim and does NOT fire on a `should_not_fire.json`
//! control. Adding a new constraint should be accompanied by adding its
//! fixture pair under `tests/fixtures/firing_matrix/<constraint_id>/`.
//!
//! This is the Tier 1 false-negative / precision smoke test for rigor's
//! 53 self-grounding constraints.

use std::path::Path;

mod support;

#[test]
fn firing_matrix_covers_every_fixture() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/firing_matrix");
    let fixtures = support::walk_fixtures(&base);

    if fixtures.is_empty() {
        // Not an error yet — fixtures land in the next PR step.
        eprintln!(
            "firing_matrix: no fixtures found under {}. Fixtures are added in PR-2.6 step 2.",
            base.display()
        );
        return;
    }

    let mut failures = Vec::new();

    for (path, fixture) in &fixtures {
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
        if actual != fixture.expected_decision {
            failures.push(format!(
                "{}: expected decision={:?} but got {:?} (notes: {})",
                path.display(),
                fixture.expected_decision,
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
