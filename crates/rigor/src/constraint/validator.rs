use super::types::RigorConfig;
use anyhow::{bail, Result};
use std::collections::HashSet;

/// Validates a parsed RigorConfig for schema-level correctness.
pub struct ConstraintValidator;

impl ConstraintValidator {
    pub fn validate(config: &RigorConfig) -> Result<()> {
        let all = config.all_constraints();
        let mut ids = HashSet::new();

        for c in &all {
            if c.name.is_empty() {
                bail!("Constraint '{}' has an empty name", c.id);
            }
            if c.rego.is_empty() {
                bail!("Constraint '{}' has an empty rego snippet", c.id);
            }
            if !ids.insert(&c.id) {
                bail!("Duplicate constraint ID: '{}'", c.id);
            }
        }

        for rel in &config.relations {
            if rel.from == rel.to {
                bail!("Self-referencing relation: '{}' -> '{}'", rel.from, rel.to);
            }
            if !ids.contains(&rel.from) {
                bail!("Relation references unknown constraint: '{}'", rel.from);
            }
            if !ids.contains(&rel.to) {
                bail!("Relation references unknown constraint: '{}'", rel.to);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::types::*;

    fn make_constraint(id: &str, etype: EpistemicType) -> Constraint {
        Constraint {
            id: id.to_string(),
            epistemic_type: etype,
            name: format!("Test {}", id),
            description: "test".to_string(),
            rego: "package test\nviolation[msg] { false }".to_string(),
            message: "test msg".to_string(),
            tags: vec![],
            domain: None,
            references: vec![],
            source: vec![],
        }
    }

    fn make_config(beliefs: Vec<Constraint>, relations: Vec<Relation>) -> RigorConfig {
        RigorConfig {
            constraints: ConstraintsSection {
                beliefs,
                justifications: vec![],
                defeaters: vec![],
            },
            relations,
        }
    }

    #[test]
    fn test_valid_config() {
        let config = make_config(
            vec![
                make_constraint("b1", EpistemicType::Belief),
                make_constraint("b2", EpistemicType::Belief),
            ],
            vec![Relation {
                from: "b1".to_string(),
                to: "b2".to_string(),
                relation_type: RelationType::Supports,
            }],
        );
        assert!(ConstraintValidator::validate(&config).is_ok());
    }

    #[test]
    fn test_duplicate_id() {
        let config = make_config(
            vec![
                make_constraint("b1", EpistemicType::Belief),
                make_constraint("b1", EpistemicType::Belief),
            ],
            vec![],
        );
        let err = ConstraintValidator::validate(&config).unwrap_err();
        assert!(err.to_string().contains("Duplicate constraint ID: 'b1'"));
    }

    #[test]
    fn test_unknown_relation_reference() {
        let config = make_config(
            vec![make_constraint("b1", EpistemicType::Belief)],
            vec![Relation {
                from: "b1".to_string(),
                to: "nonexistent".to_string(),
                relation_type: RelationType::Attacks,
            }],
        );
        let err = ConstraintValidator::validate(&config).unwrap_err();
        assert!(err
            .to_string()
            .contains("Relation references unknown constraint: 'nonexistent'"));
    }

    #[test]
    fn test_self_referencing_relation() {
        let config = make_config(
            vec![make_constraint("b1", EpistemicType::Belief)],
            vec![Relation {
                from: "b1".to_string(),
                to: "b1".to_string(),
                relation_type: RelationType::Supports,
            }],
        );
        let err = ConstraintValidator::validate(&config).unwrap_err();
        assert!(err.to_string().contains("Self-referencing relation"));
    }

    #[test]
    fn test_empty_name() {
        let mut c = make_constraint("b1", EpistemicType::Belief);
        c.name = String::new();
        let config = make_config(vec![c], vec![]);
        let err = ConstraintValidator::validate(&config).unwrap_err();
        assert!(err.to_string().contains("empty name"));
    }

    #[test]
    fn test_empty_rego() {
        let mut c = make_constraint("b1", EpistemicType::Belief);
        c.rego = String::new();
        let config = make_config(vec![c], vec![]);
        let err = ConstraintValidator::validate(&config).unwrap_err();
        assert!(err.to_string().contains("empty rego"));
    }
}
