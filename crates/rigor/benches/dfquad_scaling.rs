//! D3 — DF-QuAD scaling benchmark (PR-2.6 Tier 1).
//!
//! Measures the wall-clock cost of `ArgumentationGraph::compute_strengths()`
//! against three constraint-set sizes (10 / 100 / 1000) with a mixed
//! belief/justification/defeater population and a uniform support/attack edge
//! density. The goal isn't absolute numbers but a shape check — compute
//! time should scale roughly linearly in node count, and definitely stay
//! well below the `EPSILON=0.001` fixed-point convergence budget.
//!
//! Baseline numbers get recorded under `.planning/perf/baseline.json` the
//! first time this bench is run against a released version; Tier 2 will add
//! regression guardrails in CI.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rigor::constraint::graph::ArgumentationGraph;
use rigor::constraint::types::{
    Constraint, ConstraintsSection, EpistemicType, Relation, RelationType, RigorConfig,
};

/// Build a synthetic config with `n` total constraints split 60/20/20 across
/// Belief/Justification/Defeater, plus ~2n relations (mix of Supports,
/// Attacks, Undercuts). Chosen to stress the fixed-point loop without
/// creating pathological cycles that refuse to converge.
fn synthetic_config(n: usize) -> RigorConfig {
    let beliefs_n = (n * 60) / 100;
    let justifications_n = (n * 20) / 100;
    let defeaters_n = n - beliefs_n - justifications_n;

    let mk = |prefix: &str, i: usize, et: EpistemicType| Constraint {
        id: format!("{}{}", prefix, i),
        epistemic_type: et,
        name: format!("{}{}", prefix, i),
        description: "bench".into(),
        rego: "package bench".into(),
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

    let beliefs: Vec<Constraint> = (0..beliefs_n)
        .map(|i| mk("b", i, EpistemicType::Belief))
        .collect();
    let justifications: Vec<Constraint> = (0..justifications_n)
        .map(|i| mk("j", i, EpistemicType::Justification))
        .collect();
    let defeaters: Vec<Constraint> = (0..defeaters_n)
        .map(|i| mk("d", i, EpistemicType::Defeater))
        .collect();

    let mk_rel = |from: String, to: String, rt: RelationType| Relation {
        from,
        to,
        relation_type: rt,
        confidence: 1.0,
        extraction_method: None,
    };

    // ~2n relations. Each justification supports two beliefs; each defeater
    // attacks one belief; every fourth belief undercuts the next one.
    let mut relations = Vec::with_capacity(n * 2);

    for (i, j) in justifications.iter().enumerate() {
        let target_a = i % beliefs_n.max(1);
        let target_b = (i + 3) % beliefs_n.max(1);
        relations.push(mk_rel(
            j.id.clone(),
            format!("b{target_a}"),
            RelationType::Supports,
        ));
        relations.push(mk_rel(
            j.id.clone(),
            format!("b{target_b}"),
            RelationType::Supports,
        ));
    }
    for (i, d) in defeaters.iter().enumerate() {
        let target = i % beliefs_n.max(1);
        relations.push(mk_rel(
            d.id.clone(),
            format!("b{target}"),
            RelationType::Attacks,
        ));
    }
    for i in (0..beliefs_n).step_by(4) {
        let next = (i + 1) % beliefs_n.max(1);
        if i != next {
            relations.push(mk_rel(
                format!("b{i}"),
                format!("b{next}"),
                RelationType::Undercuts,
            ));
        }
    }

    RigorConfig {
        constraints: ConstraintsSection {
            beliefs,
            justifications,
            defeaters,
        },
        relations,
    }
}

fn bench_compute_strengths(c: &mut Criterion) {
    let mut group = c.benchmark_group("dfquad_scaling");
    for size in [10usize, 100, 1000] {
        let config = synthetic_config(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::new("compute_strengths", size),
            &config,
            |b, cfg| {
                b.iter(|| {
                    let mut graph = ArgumentationGraph::from_config(cfg);
                    graph.compute_strengths().expect("converges");
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_compute_strengths);
criterion_main!(benches);
