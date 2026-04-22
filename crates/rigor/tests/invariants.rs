//! B-series invariant tests — PR-2.6 Tier 1.
//!
//! - B4: DF-QuAD determinism — 100 identical inputs must produce 100 identical
//!   strength maps. Guards against accidental `HashMap` swap in `graph.rs`.
//! - B10: enforcement-requires-traffic-routing — a rigor stop-hook invocation
//!   with no rigor.yaml and no live daemon must exit silently with allow and
//!   not write to the violation log.
//!
//! B1 (streaming kill-switch), B2 (auto-retry exactly once), and B3 (PII
//! redact before forward) need a mock LLM server; they're deferred to PR-2.7.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use rigor::constraint::graph::ArgumentationGraph;
use rigor::constraint::types::{
    Constraint, ConstraintsSection, EpistemicType, Relation, RelationType, RigorConfig,
};
use serde_json::{json, Value};

mod support;

// =============================================================================
// B4 — DF-QuAD determinism
// =============================================================================

/// Build a non-trivial config: 3 beliefs + 1 defeater + mixed relations.
/// Same shape as the regression guard at `graph.rs:test_strength_bounds`
/// but intentionally chosen so every relation-type is exercised.
fn deterministic_test_config() -> RigorConfig {
    let mk = |id: &str, et: EpistemicType| Constraint {
        id: id.into(),
        epistemic_type: et,
        name: id.into(),
        description: "test".into(),
        rego: "package test".into(),
        message: "m".into(),
        tags: vec![],
        domain: None,
        references: vec![],
        source: vec![],
        knowledge_type: None,
        base_strength_override: None,
        last_verified: None,
        verification_count: 0,
        verified_at_commit: None,
        credibility_weight: None,
        cluster_id: None,
    };

    let mk_rel = |from: &str, to: &str, rt: RelationType| Relation {
        from: from.into(),
        to: to.into(),
        relation_type: rt,
        confidence: 1.0,
        extraction_method: None,
    };

    RigorConfig {
        constraints: ConstraintsSection {
            beliefs: vec![
                mk("b1", EpistemicType::Belief),
                mk("b2", EpistemicType::Belief),
                mk("b3", EpistemicType::Belief),
            ],
            justifications: vec![mk("j1", EpistemicType::Justification)],
            defeaters: vec![mk("d1", EpistemicType::Defeater)],
        },
        relations: vec![
            mk_rel("j1", "b1", RelationType::Supports),
            mk_rel("d1", "b1", RelationType::Attacks),
            mk_rel("b2", "b1", RelationType::Supports),
            mk_rel("b3", "b2", RelationType::Undercuts),
            mk_rel("j1", "b3", RelationType::Attacks),
        ],
    }
}

#[test]
fn b4_dfquad_determinism_100_runs() {
    let config = deterministic_test_config();

    // Compute once to establish the baseline. Collect into a sorted Vec so
    // equality checks don't depend on the inner HashMap's iteration order.
    let mut baseline_graph = ArgumentationGraph::from_config(&config);
    baseline_graph
        .compute_strengths()
        .expect("baseline compute_strengths must succeed");
    let mut baseline: Vec<(String, f64)> = baseline_graph.get_all_strengths().into_iter().collect();
    baseline.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(
        baseline.len(),
        5,
        "baseline should report strengths for all 5 constraints"
    );

    // 100 further runs: every output must match the baseline bit-for-bit.
    for run in 0..100 {
        let mut graph = ArgumentationGraph::from_config(&config);
        graph
            .compute_strengths()
            .unwrap_or_else(|e| panic!("run {} compute_strengths failed: {}", run, e));
        let mut result: Vec<(String, f64)> = graph.get_all_strengths().into_iter().collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(
            result.len(),
            baseline.len(),
            "run {} produced a different constraint count",
            run
        );

        for ((b_id, b_s), (r_id, r_s)) in baseline.iter().zip(result.iter()) {
            assert_eq!(
                b_id, r_id,
                "run {} — constraint set diverged: baseline {:?} vs run {:?}",
                run, b_id, r_id
            );
            // Bit-exact equality: DF-QuAD uses deterministic floating-point math.
            assert_eq!(
                b_s.to_bits(),
                r_s.to_bits(),
                "run {} — strength for {} diverged: baseline {} vs run {}",
                run,
                b_id,
                b_s,
                r_s
            );
        }
    }
}

// =============================================================================
// B10 — enforcement-requires-traffic-routing
// =============================================================================

/// A stop-hook invocation with no rigor.yaml in the tree AND no live daemon
/// must emit `{"decision":"allow"}` (or omit the field) and write nothing to
/// the violation log. This is the primary contract documented in the
/// `enforcement-requires-traffic-routing` constraint — rigor never acts as
/// a background enforcer without explicit opt-in via routing or a config file.
#[test]
fn b10_stop_hook_without_rigor_yaml_or_daemon_is_inert() {
    let temp = tempfile::TempDir::new().unwrap();
    // Explicitly avoid copying rigor.yaml — we want the no-config path.
    assert!(
        !temp.path().join("rigor.yaml").exists(),
        "sanity: temp dir should have no rigor.yaml"
    );

    // Fake HOME so ~/.rigor/daemon.pid certainly doesn't exist.
    let fake_home = temp.path().join("home");
    fs::create_dir_all(&fake_home).unwrap();

    let input = json!({
        "session_id": "pr-2.6-b10",
        "transcript_path": temp.path().join("transcript.jsonl").to_string_lossy(),
        "cwd": temp.path().to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": false,
    });

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rigor"));
    cmd.current_dir(temp.path())
        .env("HOME", fake_home.to_string_lossy().to_string())
        // RIGOR_TEST_CLAIMS is intentionally unset here — we're testing the
        // bypass path, not the test-override.
        .env_remove("RIGOR_TEST_CLAIMS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn rigor");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();

    assert_eq!(
        out.status.code().unwrap_or(-1),
        0,
        "rigor exit code must be 0 (allow); stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let response: Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("bad JSON: {} — {}", e, stdout));

    // Decision should be absent or "allow" — never "block" or "warn".
    let decision = response.get("decision").and_then(|d| d.as_str());
    assert!(
        decision.is_none() || decision == Some("allow"),
        "inert path must not emit block/warn; got {:?}",
        decision
    );

    // Violation log under the fake HOME must not exist — the inert path
    // never writes anything.
    let violations_path = fake_home.join(".rigor").join("violations.jsonl");
    assert!(
        !violations_path.exists(),
        "inert path must not create {} — it did",
        violations_path.display()
    );
}
