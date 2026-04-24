use std::collections::{BTreeMap, HashMap};

use anyhow::Result;

use super::types::{EpistemicType, Relation, RelationType, RigorConfig};

const MAX_ITERATIONS: usize = 100;
const EPSILON: f64 = 0.001;

/// A node in the argumentation graph representing a constraint with computed strength.
#[derive(Debug, Clone)]
pub struct ConstraintNode {
    pub constraint_id: String,
    pub base_strength: f64,
    pub strength: f64,
    pub epistemic_type: EpistemicType,
}

/// Argumentation graph that computes constraint strengths via DF-QuAD fixed-point iteration.
///
/// Uses the correct DF-QuAD formula from Rago et al. 2016:
/// - Product aggregation: agg(M) = ∏(1 - sᵢ) for each sᵢ in M
/// - Combined effect: s = agg(attackers) - agg(supporters)
/// - Two-case influence:
///   - If s < 0 (attackers dominate): σ = τ - τ·|s| = τ·(1 - |s|)
///   - If s ≥ 0 (supporters dominate): σ = τ + (1 - τ)·s
///
/// Uses BTreeMap for deterministic iteration order.
pub struct ArgumentationGraph {
    nodes: BTreeMap<String, ConstraintNode>,
    relations: Vec<Relation>,
}

impl Default for ArgumentationGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ArgumentationGraph {
    pub fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            relations: Vec::new(),
        }
    }

    /// Add a constraint with default base_strength derived from epistemic type.
    pub fn add_constraint(&mut self, id: &str, epistemic_type: EpistemicType) {
        let base_strength = match &epistemic_type {
            EpistemicType::Belief => 0.8,
            EpistemicType::Justification => 0.9,
            EpistemicType::Defeater => 0.7,
        };
        self.nodes.insert(
            id.to_string(),
            ConstraintNode {
                constraint_id: id.to_string(),
                base_strength,
                strength: base_strength,
                epistemic_type,
            },
        );
    }

    pub fn add_relation(&mut self, relation: Relation) {
        self.relations.push(relation);
    }

    /// Build graph from a RigorConfig.
    pub fn from_config(config: &RigorConfig) -> Self {
        let mut graph = Self::new();
        for c in config.all_constraints() {
            graph.add_constraint(&c.id, c.epistemic_type.clone());
        }
        for r in &config.relations {
            graph.add_relation(r.clone());
        }
        graph
    }

    /// Compute strengths via DF-QuAD fixed-point iteration.
    ///
    /// Uses the correct DF-QuAD formula (Rago et al. 2016) with product aggregation
    /// and two-case influence function. Iterates over nodes in deterministic (sorted)
    /// order via BTreeMap.
    pub fn compute_strengths(&mut self) -> Result<()> {
        if self.nodes.is_empty() {
            return Ok(());
        }

        for _iteration in 0..MAX_ITERATIONS {
            let mut max_change: f64 = 0.0;
            let mut new_strengths: BTreeMap<String, f64> = BTreeMap::new();

            for (id, node) in &self.nodes {
                let supporters = self.get_supporters(id);
                let attackers = self.get_attackers(id);

                // Product aggregation: agg(M) = ∏(1 - sᵢ)
                let attack_prod: f64 = attackers
                    .iter()
                    .map(|aid| 1.0 - self.nodes[aid].strength)
                    .product();

                let support_prod: f64 = supporters
                    .iter()
                    .map(|sid| 1.0 - self.nodes[sid].strength)
                    .product();

                // Combined effect: s = agg(attackers) - agg(supporters)
                // s < 0 means attackers dominate (their product is smaller because they're stronger)
                let combined = attack_prod - support_prod;

                // Two-case influence function
                let tau = node.base_strength;
                let new_strength = if combined < 0.0 {
                    // Attackers dominate: scale down from base
                    tau * (1.0 - combined.abs())
                } else {
                    // Supporters dominate (or equal): scale up toward 1.0
                    tau + (1.0 - tau) * combined
                };

                // Clamp to [0, 1] for safety (formula should be bounded but floating point)
                let new_strength = new_strength.clamp(0.0, 1.0);

                let change = (new_strength - node.strength).abs();
                if change > max_change {
                    max_change = change;
                }
                new_strengths.insert(id.clone(), new_strength);
            }

            // Apply new strengths
            for (id, s) in &new_strengths {
                self.nodes.get_mut(id).unwrap().strength = *s;
            }

            if max_change < EPSILON {
                return Ok(());
            }
        }

        anyhow::bail!("DF-QuAD did not converge within {MAX_ITERATIONS} iterations")
    }

    pub fn get_strength(&self, id: &str) -> Option<f64> {
        self.nodes.get(id).map(|n| n.strength)
    }

    /// Get all nodes in the graph.
    pub fn nodes(&self) -> &BTreeMap<String, ConstraintNode> {
        &self.nodes
    }

    /// Get all relations in the graph.
    pub fn relations(&self) -> &[Relation] {
        &self.relations
    }

    pub fn get_all_strengths(&self) -> HashMap<String, f64> {
        self.nodes
            .iter()
            .map(|(id, n)| (id.clone(), n.strength))
            .collect()
    }

    fn get_supporters(&self, id: &str) -> Vec<String> {
        self.relations
            .iter()
            .filter(|r| r.to == id && r.relation_type == RelationType::Supports)
            .map(|r| r.from.clone())
            .collect()
    }

    fn get_attackers(&self, id: &str) -> Vec<String> {
        self.relations
            .iter()
            .filter(|r| {
                r.to == id
                    && (r.relation_type == RelationType::Attacks
                        || r.relation_type == RelationType::Undercuts)
            })
            .map(|r| r.from.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_relations_retains_base_strength() {
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("a", EpistemicType::Belief);
        graph.add_constraint("b", EpistemicType::Belief);
        graph.add_constraint("c", EpistemicType::Belief);
        graph.compute_strengths().unwrap();

        assert!((graph.get_strength("a").unwrap() - 0.8).abs() < EPSILON);
        assert!((graph.get_strength("b").unwrap() - 0.8).abs() < EPSILON);
        assert!((graph.get_strength("c").unwrap() - 0.8).abs() < EPSILON);
    }

    #[test]
    fn test_support_increases_strength() {
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("a", EpistemicType::Belief);
        graph.add_constraint("b", EpistemicType::Belief);
        graph.add_relation(Relation {
            from: "a".to_string(),
            to: "b".to_string(),
            relation_type: RelationType::Supports,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.compute_strengths().unwrap();

        assert!(graph.get_strength("b").unwrap() > 0.8);
    }

    #[test]
    fn test_attack_decreases_strength() {
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("a", EpistemicType::Belief);
        graph.add_constraint("b", EpistemicType::Belief);
        graph.add_relation(Relation {
            from: "a".to_string(),
            to: "b".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.compute_strengths().unwrap();

        assert!(graph.get_strength("b").unwrap() < 0.8);
    }

    #[test]
    fn test_undercut_weakens_relation() {
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("a", EpistemicType::Belief);
        graph.add_constraint("b", EpistemicType::Belief);
        graph.add_constraint("c", EpistemicType::Belief);
        // B supports C
        graph.add_relation(Relation {
            from: "b".to_string(),
            to: "c".to_string(),
            relation_type: RelationType::Supports,
            confidence: 1.0,
            extraction_method: None,
        });
        // A undercuts B->C (treated as attack on C for v0.1)
        graph.add_relation(Relation {
            from: "a".to_string(),
            to: "c".to_string(),
            relation_type: RelationType::Undercuts,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.compute_strengths().unwrap();

        // support_agg of C = 0.8 (from B), attack_agg of C = 0.8 (from A undercut)
        // C = 0.8 * (1.0 - 0.8 + 0.8) = 0.8 * 1.0 = 0.8 (support and undercut cancel out)
        let c_strength = graph.get_strength("c").unwrap();
        assert!(
            (c_strength - 0.8).abs() < 0.01,
            "expected 0.8, got {c_strength}"
        );
    }

    #[test]
    fn test_cycle_converges() {
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("a", EpistemicType::Belief);
        graph.add_constraint("b", EpistemicType::Belief);
        graph.add_relation(Relation {
            from: "a".to_string(),
            to: "b".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.add_relation(Relation {
            from: "b".to_string(),
            to: "a".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });
        // Must not panic or error
        graph.compute_strengths().unwrap();

        // Both should be in valid range
        let a = graph.get_strength("a").unwrap();
        let b = graph.get_strength("b").unwrap();
        assert!((0.0..=1.0).contains(&a));
        assert!((0.0..=1.0).contains(&b));
    }

    #[test]
    fn test_empty_graph() {
        let mut graph = ArgumentationGraph::new();
        assert!(graph.compute_strengths().is_ok());
    }

    #[test]
    fn test_strength_bounds() {
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("a", EpistemicType::Justification);
        graph.add_constraint("b", EpistemicType::Defeater);
        graph.add_constraint("c", EpistemicType::Belief);
        // Multiple supports to push high
        graph.add_relation(Relation {
            from: "a".to_string(),
            to: "c".to_string(),
            relation_type: RelationType::Supports,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.add_relation(Relation {
            from: "b".to_string(),
            to: "c".to_string(),
            relation_type: RelationType::Supports,
            confidence: 1.0,
            extraction_method: None,
        });
        // Multiple attacks to push low
        graph.add_relation(Relation {
            from: "a".to_string(),
            to: "b".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.add_relation(Relation {
            from: "c".to_string(),
            to: "b".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });

        graph.compute_strengths().unwrap();

        for (_, s) in graph.get_all_strengths() {
            assert!((0.0..=1.0).contains(&s), "Strength {s} out of bounds");
        }
    }

    #[test]
    fn test_base_strength_by_epistemic_type() {
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("belief", EpistemicType::Belief);
        graph.add_constraint("justification", EpistemicType::Justification);
        graph.add_constraint("defeater", EpistemicType::Defeater);

        // Before compute, check base strengths
        assert!((graph.nodes["belief"].base_strength - 0.8).abs() < f64::EPSILON);
        assert!((graph.nodes["justification"].base_strength - 0.9).abs() < f64::EPSILON);
        assert!((graph.nodes["defeater"].base_strength - 0.7).abs() < f64::EPSILON);

        // After compute with no relations, strengths should equal base
        graph.compute_strengths().unwrap();
        assert!((graph.get_strength("belief").unwrap() - 0.8).abs() < EPSILON);
        assert!((graph.get_strength("justification").unwrap() - 0.9).abs() < EPSILON);
        assert!((graph.get_strength("defeater").unwrap() - 0.7).abs() < EPSILON);
    }

    #[test]
    fn test_dfquad_golden_single_attack() {
        // Belief (base 0.8) attacked by Defeater (base 0.7)
        // belief = 0.8 * (1.0 - 0.7) = 0.24, defeater stays 0.7
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("belief", EpistemicType::Belief);
        graph.add_constraint("defeater", EpistemicType::Defeater);
        graph.add_relation(Relation {
            from: "defeater".to_string(),
            to: "belief".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.compute_strengths().unwrap();

        let belief = graph.get_strength("belief").unwrap();
        let defeater = graph.get_strength("defeater").unwrap();
        assert!(
            (belief - 0.24).abs() < 0.01,
            "expected belief ≈ 0.24, got {belief}"
        );
        assert!(
            (defeater - 0.7).abs() < 0.01,
            "expected defeater ≈ 0.7, got {defeater}"
        );
    }

    #[test]
    fn test_dfquad_golden_single_support() {
        // Belief (base 0.8) supported by Justification (base 0.9)
        // Product aggregation: attack_prod = 1.0, support_prod = 1-0.9 = 0.1
        // combined = 1.0 - 0.1 = 0.9 (supporters dominate)
        // σ = 0.8 + (1-0.8) * 0.9 = 0.8 + 0.18 = 0.98
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("belief", EpistemicType::Belief);
        graph.add_constraint("justification", EpistemicType::Justification);
        graph.add_relation(Relation {
            from: "justification".to_string(),
            to: "belief".to_string(),
            relation_type: RelationType::Supports,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.compute_strengths().unwrap();

        let belief = graph.get_strength("belief").unwrap();
        let justification = graph.get_strength("justification").unwrap();
        assert!(
            (belief - 0.98).abs() < 0.01,
            "expected belief ≈ 0.98, got {belief}"
        );
        assert!(
            (justification - 0.9).abs() < 0.01,
            "expected justification ≈ 0.9, got {justification}"
        );
    }

    #[test]
    fn test_dfquad_golden_mixed() {
        // Belief (base 0.8) with supporter Justification (0.9) and attacker Defeater (0.7)
        // Product aggregation:
        //   attack_prod = 1-0.7 = 0.3, support_prod = 1-0.9 = 0.1
        //   combined = 0.3 - 0.1 = 0.2 (supporters dominate)
        //   σ = 0.8 + (1-0.8) * 0.2 = 0.8 + 0.04 = 0.84
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("belief", EpistemicType::Belief);
        graph.add_constraint("justification", EpistemicType::Justification);
        graph.add_constraint("defeater", EpistemicType::Defeater);
        graph.add_relation(Relation {
            from: "justification".to_string(),
            to: "belief".to_string(),
            relation_type: RelationType::Supports,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.add_relation(Relation {
            from: "defeater".to_string(),
            to: "belief".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.compute_strengths().unwrap();

        let belief = graph.get_strength("belief").unwrap();
        assert!(
            (belief - 0.84).abs() < 0.01,
            "expected belief ≈ 0.84, got {belief}"
        );
        let justification = graph.get_strength("justification").unwrap();
        assert!(
            (justification - 0.9).abs() < 0.01,
            "expected justification ≈ 0.9, got {justification}"
        );
        let defeater = graph.get_strength("defeater").unwrap();
        assert!(
            (defeater - 0.7).abs() < 0.01,
            "expected defeater ≈ 0.7, got {defeater}"
        );
    }

    #[test]
    fn test_dfquad_multi_attacker_product_vs_mean() {
        // Two defeaters (both 0.7) attacking a belief (0.8)
        // Product aggregation: attack_prod = (1-0.7)*(1-0.7) = 0.09
        //   combined = 0.09 - 1.0 = -0.91 (attackers dominate)
        //   σ = 0.8 * (1 - 0.91) = 0.8 * 0.09 = 0.072
        // Mean aggregation (old wrong formula) would give:
        //   mean = 0.7, σ = 0.8 * (1-0.7) = 0.24
        // The product correctly models diminishing returns
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("belief", EpistemicType::Belief);
        graph.add_constraint("d1", EpistemicType::Defeater);
        graph.add_constraint("d2", EpistemicType::Defeater);
        graph.add_relation(Relation {
            from: "d1".to_string(),
            to: "belief".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.add_relation(Relation {
            from: "d2".to_string(),
            to: "belief".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.compute_strengths().unwrap();

        let belief = graph.get_strength("belief").unwrap();
        assert!(
            (belief - 0.072).abs() < 0.01,
            "expected belief ≈ 0.072 (product agg), got {belief}"
        );
        // Verify this is NOT the mean aggregation result (0.24)
        assert!(
            (belief - 0.24).abs() > 0.1,
            "belief should NOT be 0.24 (mean agg), got {belief}"
        );
    }

    #[test]
    fn test_dfquad_golden_symmetric_attack() {
        // Two beliefs (both base 0.8) attacking each other.
        // Fixed point: x = 0.8 * (1 - x) => x = 0.8 / 1.8 = 4/9 ≈ 0.4444
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("a", EpistemicType::Belief);
        graph.add_constraint("b", EpistemicType::Belief);
        graph.add_relation(Relation {
            from: "a".to_string(),
            to: "b".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.add_relation(Relation {
            from: "b".to_string(),
            to: "a".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.compute_strengths().unwrap();

        let a = graph.get_strength("a").unwrap();
        let b = graph.get_strength("b").unwrap();

        // Both should converge to the same value (symmetric)
        assert!(
            (a - b).abs() < 0.01,
            "expected symmetric convergence, got a={a}, b={b}"
        );

        // Both in valid range
        assert!((0.0..=1.0).contains(&a), "a out of bounds: {a}");
        assert!((0.0..=1.0).contains(&b), "b out of bounds: {b}");

        // Specific converged value: 4/9 ≈ 0.4444
        let expected = 4.0 / 9.0;
        assert!(
            (a - expected).abs() < 0.01,
            "expected a ≈ {expected}, got {a}"
        );
        assert!(
            (b - expected).abs() < 0.01,
            "expected b ≈ {expected}, got {b}"
        );
    }

    // --- DF-QuAD boundary tests (gap 6) ---

    #[test]
    fn test_btreemap_determinism() {
        // Two graphs with the same nodes and relations added in different
        // orders must produce identical strengths. BTreeMap guarantees
        // sorted iteration order, so insertion order is irrelevant.
        let mut graph_a = ArgumentationGraph::new();
        graph_a.add_constraint("alpha", EpistemicType::Belief);
        graph_a.add_constraint("beta", EpistemicType::Belief);
        graph_a.add_constraint("gamma", EpistemicType::Justification);
        graph_a.add_relation(Relation {
            from: "alpha".to_string(),
            to: "beta".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });

        let mut graph_b = ArgumentationGraph::new();
        // Deliberately different insertion order
        graph_b.add_constraint("gamma", EpistemicType::Justification);
        graph_b.add_constraint("alpha", EpistemicType::Belief);
        graph_b.add_constraint("beta", EpistemicType::Belief);
        graph_b.add_relation(Relation {
            from: "alpha".to_string(),
            to: "beta".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });

        graph_a.compute_strengths().unwrap();
        graph_b.compute_strengths().unwrap();

        let strengths_a = graph_a.get_all_strengths();
        let strengths_b = graph_b.get_all_strengths();

        assert_eq!(
            strengths_a, strengths_b,
            "BTreeMap determinism: insertion order must not affect strengths.\n  A: {strengths_a:?}\n  B: {strengths_b:?}"
        );
    }

    #[test]
    fn test_single_strong_attacker_dominance() {
        // Justification (base 0.9) attacks Defeater (base 0.7).
        // Defeater strength = 0.7 * (1 - 0.9) = 0.07 (driven very low).
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("justification", EpistemicType::Justification);
        graph.add_constraint("defeater", EpistemicType::Defeater);
        graph.add_relation(Relation {
            from: "justification".to_string(),
            to: "defeater".to_string(),
            relation_type: RelationType::Attacks,
            confidence: 1.0,
            extraction_method: None,
        });
        graph.compute_strengths().unwrap();

        let defeater = graph.get_strength("defeater").unwrap();
        assert!(
            defeater < 0.1,
            "single strong attacker (base 0.9) should drive defeater (base 0.7) below 0.1, got {defeater}"
        );
        // Exact value: 0.7 * (1 - 0.9) = 0.07
        assert!(
            (defeater - 0.07).abs() < 0.01,
            "expected defeater ~ 0.07, got {defeater}"
        );
    }

    #[test]
    fn test_constants_are_documented_values() {
        // Regression guard: if MAX_ITERATIONS or EPSILON are changed,
        // this test forces acknowledgment.
        assert_eq!(
            MAX_ITERATIONS, 100,
            "MAX_ITERATIONS changed from documented value of 100"
        );
        assert!(
            (EPSILON - 0.001).abs() < f64::EPSILON,
            "EPSILON changed from documented value of 0.001"
        );
    }

    #[test]
    fn test_zero_attacker_zero_supporter_retains_base() {
        // A single isolated node with no relations retains its base
        // strength. Explicit zero-attacker test from issue #16.
        let mut graph = ArgumentationGraph::new();
        graph.add_constraint("solo", EpistemicType::Belief);
        graph.compute_strengths().unwrap();

        let strength = graph.get_strength("solo").unwrap();
        assert!(
            (strength - 0.8).abs() < EPSILON,
            "single node with no relations should retain base strength 0.8, got {strength}"
        );
    }
}
