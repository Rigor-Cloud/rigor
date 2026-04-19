//! Evaluation-Only Benchmark
//!
//! Measures ONLY the Rego policy evaluation step, isolating regorus performance
//! from system overhead (I/O, config loading, claim extraction, etc.).
//!
//! This benchmark focuses on:
//! - PolicyEngine::evaluate() call
//! - Regorus engine execution
//! - Rego policy matching against claims
//!
//! Excludes:
//! - Config file parsing
//! - Argumentation graph computation
//! - Violation collection/formatting
//! - Decision logic
//!
//! Target: <50ms for pure evaluation (subset of full <100ms target)

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rigor::{
    claim::{Claim, ClaimType, SourceLocation},
    config::find_rigor_yaml,
    constraint::loader::load_rigor_config,
    policy::{EvaluationInput, PolicyEngine},
};

/// Create a diverse set of test claims to exercise different constraint patterns.
/// Mix of high/low confidence, different types, various content patterns.
fn create_diverse_claims() -> Vec<Claim> {
    vec![
        // High confidence assertion - may trigger fabricated API check
        Claim::new(
            "The regorus.Engine supports streaming async evaluation".to_string(),
            0.92,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 0,
                sentence_index: 0,
            }),
        ),
        // Test claim without evidence - may trigger test-evidence-supports
        Claim::new(
            "The constraint system works correctly and handles all cases".to_string(),
            0.88,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 0,
                sentence_index: 1,
            }),
        ),
        // DF-QuAD claim - may trigger dfquad-semantics check
        Claim::new(
            "DF-QuAD uses binary true/false acceptance values".to_string(),
            0.85,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 0,
                sentence_index: 2,
            }),
        ),
        // Hedged low-confidence claim - should be allowed
        Claim::new(
            "The architecture might benefit from additional caching".to_string(),
            0.65,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 0,
                sentence_index: 3,
            }),
        ),
        // Production-ready claim - may trigger prototype-defeats-strict
        Claim::new(
            "The system is production ready and fully implemented".to_string(),
            0.9,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 1,
                sentence_index: 0,
            }),
        ),
        // Definitive low-confidence - may trigger hedged-supports-uncertain
        Claim::new(
            "The API supports all OPA features".to_string(),
            0.55,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 1,
                sentence_index: 1,
            }),
        ),
        // Old Rego syntax claim - may trigger rego-syntax-accuracy
        Claim::new(
            "Rego syntax uses violation[v] for defining rules".to_string(),
            0.8,
            ClaimType::Assertion,
            Some(SourceLocation {
                message_index: 1,
                sentence_index: 2,
            }),
        ),
    ]
}

fn benchmark_evaluation_only(c: &mut Criterion) {
    // Setup: Load config and create engine (outside measurement)
    let yaml_path = find_rigor_yaml().expect("rigor.yaml not found in repo");
    let config = load_rigor_config(&yaml_path).expect("Failed to load rigor.yaml");
    let mut engine = PolicyEngine::new(&config).expect("Failed to create policy engine");

    // Setup: Create test claims (outside measurement)
    let claims = create_diverse_claims();
    let eval_input = EvaluationInput {
        claims: claims.clone(),
    };

    // Benchmark ONLY the evaluate() call - pure Rego evaluation
    c.bench_function("evaluation_only", |b| {
        b.iter(|| {
            // This is the only measured operation: Rego policy evaluation
            let raw_violations = engine
                .evaluate(black_box(&eval_input))
                .expect("Evaluation failed");

            // Ensure compiler doesn't optimize away
            black_box(raw_violations);
        });
    });
}

criterion_group!(benches, benchmark_evaluation_only);
criterion_main!(benches);
