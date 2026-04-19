//! Full Hook Latency Benchmark
//!
//! Measures the complete Rigor hook pipeline end-to-end:
//! - Config loading (rigor.yaml)
//! - Claim extraction from test data
//! - Policy evaluation (regorus)
//! - Violation collection
//! - Decision formatting
//!
//! Target: <100ms mean latency for full pipeline

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rigor::{
    claim::{Claim, ClaimType, SourceLocation},
    config::find_rigor_yaml,
    constraint::{graph::ArgumentationGraph, loader::load_rigor_config},
    policy::{EvaluationInput, PolicyEngine},
    violation::{collect_violations, determine_decision, ConstraintMeta, SeverityThresholds},
};
use std::collections::HashMap;

/// Create realistic test claims for benchmarking.
/// These claims are designed to trigger some constraints in rigor.yaml.
fn create_test_claims() -> Vec<Claim> {
    vec![
        Claim::new(
            "The regorus library provides streaming evaluation support".to_string(),
            0.85,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 0,
                sentence_index: 0,
            }),
        ),
        Claim::new(
            "Tests verify the constraint evaluation works correctly".to_string(),
            0.75,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 0,
                sentence_index: 1,
            }),
        ),
        Claim::new(
            "The implementation uses DF-QuAD semantics for aggregation".to_string(),
            0.9,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 0,
                sentence_index: 2,
            }),
        ),
        Claim::new(
            "Rego syntax uses contains and if keywords from rego.v1".to_string(),
            0.95,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 0,
                sentence_index: 3,
            }),
        ),
        Claim::new(
            "The system might need additional performance optimization".to_string(),
            0.6,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 0,
                sentence_index: 4,
            }),
        ),
    ]
}

/// Build constraint metadata map from config.
/// This mimics what the main pipeline does in lib.rs.
fn build_constraint_meta(
    config: &rigor::constraint::types::RigorConfig,
) -> HashMap<String, ConstraintMeta> {
    config
        .all_constraints()
        .iter()
        .map(|c| {
            let epistemic_type = match c.epistemic_type {
                rigor::constraint::types::EpistemicType::Belief => "belief",
                rigor::constraint::types::EpistemicType::Justification => "justification",
                rigor::constraint::types::EpistemicType::Defeater => "defeater",
            };
            (
                c.id.clone(),
                ConstraintMeta {
                    name: c.name.clone(),
                    epistemic_type: epistemic_type.to_string(),
                    rego_path: format!("data.rigor.{}", c.id),
                },
            )
        })
        .collect()
}

fn benchmark_full_hook_pipeline(c: &mut Criterion) {
    // Find rigor.yaml (this repo dogfoods itself)
    let yaml_path = find_rigor_yaml().expect("rigor.yaml not found in repo");

    // Pre-load config outside measurement (setup cost)
    let config = load_rigor_config(&yaml_path).expect("Failed to load rigor.yaml");
    let constraint_meta = build_constraint_meta(&config);

    // Pre-compute argumentation graph strengths (setup cost)
    let mut graph = ArgumentationGraph::from_config(&config);
    graph
        .compute_strengths()
        .expect("Failed to compute strengths");
    let strengths = graph.get_all_strengths();

    // Pre-create policy engine (setup cost)
    let mut engine = PolicyEngine::new(&config).expect("Failed to create policy engine");

    // Pre-create test claims (setup cost)
    let claims = create_test_claims();
    let eval_input = EvaluationInput {
        claims: claims.clone(),
    };

    // Pre-create thresholds (setup cost)
    let thresholds = SeverityThresholds::default();

    // Now benchmark only the hot path: evaluate + collect + decide
    c.bench_function("full_hook_latency", |b| {
        b.iter(|| {
            // Measure only the evaluation pipeline (not config loading)
            let raw_violations = engine
                .evaluate(black_box(&eval_input))
                .expect("Evaluation failed");

            let violations = collect_violations(
                black_box(raw_violations),
                black_box(&strengths),
                black_box(&thresholds),
                black_box(&constraint_meta),
                black_box(&claims),
            );

            let decision = determine_decision(black_box(&violations));

            // Ensure compiler doesn't optimize away
            black_box(decision);
        });
    });
}

criterion_group!(benches, benchmark_full_hook_pipeline);
criterion_main!(benches);
