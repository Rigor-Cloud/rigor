use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::warn;

use crate::constraint::RigorConfig;
use crate::policy::input::EvaluationInput;

/// Raw violation output from Rego evaluation, before severity assignment.
#[derive(Debug, Clone, Deserialize)]
pub struct RawViolation {
    pub constraint_id: String,
    pub violated: bool,
    pub claims: Vec<String>,
    pub reason: String,
}

/// Policy engine wrapping regorus with prepared Rego queries.
#[derive(Clone)]
pub struct PolicyEngine {
    engine: regorus::Engine,
    /// Constraint IDs that were successfully loaded.
    loaded_constraints: Vec<String>,
}

impl PolicyEngine {
    /// Create a new PolicyEngine from a RigorConfig.
    ///
    /// Loads helpers.rego and wraps each constraint's Rego snippet in a module.
    /// Invalid Rego is logged and skipped (fail-open).
    pub fn new(config: &RigorConfig) -> Result<Self> {
        let mut engine = regorus::Engine::new();

        // Add helpers.rego
        engine
            .add_policy(
                "helpers.rego".to_string(),
                include_str!("../../../../policies/helpers.rego").to_string(),
            )
            .context("Failed to load helpers.rego")?;

        let mut loaded_constraints = Vec::new();

        for constraint in config.all_constraints() {
            let id = &constraint.id;
            // Sanitize id for use as Rego package name (replace - with _)
            let safe_id = id.replace('-', "_");
            let policy_name = format!("constraint_{safe_id}.rego");

            // Wrap user's rego snippet in a proper module
            let full_rego = format!(
                r#"package rigor.constraint_{safe_id}

import rego.v1

import data.rigor.helpers

{rego}
"#,
                safe_id = safe_id,
                rego = constraint.rego,
            );

            match engine.add_policy(policy_name.clone(), full_rego) {
                Ok(_) => {
                    loaded_constraints.push(id.clone());
                }
                Err(e) => {
                    warn!(
                        constraint_id = %id,
                        error = %e,
                        "Skipping constraint with invalid Rego (fail-open)"
                    );
                }
            }
        }

        Ok(Self {
            engine,
            loaded_constraints,
        })
    }

    /// Evaluate all loaded constraints against the given input claims.
    ///
    /// Returns raw violations from Rego evaluation.
    pub fn evaluate(&mut self, input: &EvaluationInput) -> Result<Vec<RawViolation>> {
        let input_json = serde_json::to_string(input)?;
        self.engine
            .set_input_json(&input_json)
            .context("Failed to set Rego input")?;

        let mut violations = Vec::new();

        for constraint_id in &self.loaded_constraints {
            let safe_id = constraint_id.replace('-', "_");
            let rule_path = format!("data.rigor.constraint_{safe_id}.violation");

            match self.engine.eval_rule(rule_path.clone()) {
                Ok(value) => {
                    // The violation rule should produce a set/array of violation objects
                    let json_str = value.to_json_str().unwrap_or_else(|_| "null".to_string());

                    // Try to parse as array of violations
                    if let Ok(raw_violations) = serde_json::from_str::<Vec<RawViolation>>(&json_str)
                    {
                        violations.extend(raw_violations);
                    } else if let Ok(single) = serde_json::from_str::<RawViolation>(&json_str) {
                        violations.push(single);
                    }
                    // If it's undefined/null/empty, no violations — that's fine
                }
                Err(e) => {
                    // Rule doesn't exist or eval error — skip (fail-open)
                    warn!(
                        constraint_id = %constraint_id,
                        rule = %rule_path,
                        error = %e,
                        "Failed to evaluate constraint rule (fail-open)"
                    );
                }
            }
        }

        Ok(violations)
    }

    /// Returns the list of successfully loaded constraint IDs.
    pub fn loaded_constraints(&self) -> &[String] {
        &self.loaded_constraints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::{Claim, ClaimType};
    use crate::constraint::{Constraint, ConstraintsSection, EpistemicType};

    fn make_config(constraints: Vec<Constraint>) -> RigorConfig {
        RigorConfig {
            constraints: ConstraintsSection {
                beliefs: constraints,
                justifications: vec![],
                defeaters: vec![],
            },
            relations: vec![],
        }
    }

    fn make_claim(id: &str, text: &str) -> Claim {
        Claim {
            id: id.to_string(),
            text: text.to_string(),
            domain: None,
            confidence: 0.9,
            claim_type: ClaimType::Assertion,
            source_line: None,
            source: None,
            knowledge_type: None,
        }
    }

    #[test]
    fn test_engine_creation_with_valid_constraints() {
        let config = make_config(vec![Constraint {
            id: "test-valid".to_string(),
            epistemic_type: EpistemicType::Belief,
            name: "Test valid".to_string(),
            description: "A valid constraint".to_string(),
            rego: r#"
violation contains v if {
    some c in input.claims
    contains(c.text, "unsafe")
    v := {"constraint_id": "test-valid", "violated": true, "claims": [c.id], "reason": "Found unsafe claim"}
}
"#
            .to_string(),
            message: "Found unsafe claim".to_string(),
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
        }]);

        let engine = PolicyEngine::new(&config).unwrap();
        assert_eq!(engine.loaded_constraints().len(), 1);
        assert_eq!(engine.loaded_constraints()[0], "test-valid");
    }

    #[test]
    fn test_engine_skips_invalid_rego() {
        let config = make_config(vec![Constraint {
            id: "bad-rego".to_string(),
            epistemic_type: EpistemicType::Belief,
            name: "Bad rego".to_string(),
            description: "Invalid rego syntax".to_string(),
            rego: "this is not valid rego {{{{".to_string(),
            message: "Bad".to_string(),
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
        }]);

        let engine = PolicyEngine::new(&config).unwrap();
        assert_eq!(engine.loaded_constraints().len(), 0);
    }

    #[test]
    fn test_evaluate_with_matching_claims() {
        let config = make_config(vec![Constraint {
            id: "no-unsafe".to_string(),
            epistemic_type: EpistemicType::Belief,
            name: "No unsafe claims".to_string(),
            description: "Flags unsafe claims".to_string(),
            rego: r#"
violation contains v if {
    some c in input.claims
    contains(c.text, "unsafe")
    v := {"constraint_id": "no-unsafe", "violated": true, "claims": [c.id], "reason": "Found unsafe claim"}
}
"#
            .to_string(),
            message: "Found unsafe claim".to_string(),
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
        }]);

        let mut engine = PolicyEngine::new(&config).unwrap();
        let input = EvaluationInput {
            claims: vec![
                make_claim("c1", "This code is unsafe to use"),
                make_claim("c2", "This code is safe"),
            ],
        };

        let violations = engine.evaluate(&input).unwrap();
        assert!(!violations.is_empty());
        assert!(violations.iter().any(|v| v.constraint_id == "no-unsafe"));
        assert!(violations.iter().all(|v| v.violated));
    }

    #[test]
    fn test_evaluate_with_no_matching_claims() {
        let config = make_config(vec![Constraint {
            id: "no-unsafe".to_string(),
            epistemic_type: EpistemicType::Belief,
            name: "No unsafe claims".to_string(),
            description: "Flags unsafe claims".to_string(),
            rego: r#"
violation contains v if {
    some c in input.claims
    contains(c.text, "unsafe")
    v := {"constraint_id": "no-unsafe", "violated": true, "claims": [c.id], "reason": "Found unsafe claim"}
}
"#
            .to_string(),
            message: "Found unsafe claim".to_string(),
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
        }]);

        let mut engine = PolicyEngine::new(&config).unwrap();
        let input = EvaluationInput {
            claims: vec![make_claim("c1", "This code is perfectly fine")],
        };

        let violations = engine.evaluate(&input).unwrap();
        assert!(violations.is_empty());
    }

    #[test]
    fn test_cloned_engine_produces_same_results() {
        let config = make_config(vec![Constraint {
            id: "no-unsafe".to_string(),
            epistemic_type: EpistemicType::Belief,
            name: "No unsafe claims".to_string(),
            description: "Flags unsafe claims".to_string(),
            rego: r#"
violation contains v if {
    some c in input.claims
    contains(c.text, "unsafe")
    v := {"constraint_id": "no-unsafe", "violated": true, "claims": [c.id], "reason": "Found unsafe claim"}
}
"#
            .to_string(),
            message: "Found unsafe claim".to_string(),
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
        }]);

        let engine = PolicyEngine::new(&config).unwrap();
        let mut cloned = engine.clone();

        let input = EvaluationInput {
            claims: vec![
                make_claim("c1", "This code is unsafe to use"),
                make_claim("c2", "This code is safe"),
            ],
        };

        let violations = cloned.evaluate(&input).unwrap();

        // Compare with fresh engine
        let mut fresh = PolicyEngine::new(&config).unwrap();
        let fresh_violations = fresh.evaluate(&input).unwrap();

        assert_eq!(violations.len(), fresh_violations.len());
        assert!(!violations.is_empty());
    }
}
