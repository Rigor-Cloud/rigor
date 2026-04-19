use std::collections::HashMap;

use crate::policy::RawViolation;
use crate::violation::types::{Severity, SeverityThresholds, Violation};

/// Constraint metadata for formatting violations.
#[derive(Debug, Clone)]
pub struct ConstraintMeta {
    pub name: String,
    pub epistemic_type: String,
    pub rego_path: String,
}

/// Decision derived from violation analysis.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// No violations or only allow-level violations.
    Allow,
    /// Warn-level violations present but no blockers.
    Warn { violations: Vec<Violation> },
    /// Block-level violation present — reject.
    Block { violations: Vec<Violation> },
}

/// Collect raw Rego violations into typed Violation structs with severity.
///
/// - Filters to only `violated: true` entries
/// - Looks up strength for each constraint_id (default 0.8)
/// - Assigns severity via thresholds
/// - Populates constraint metadata from constraint_meta
/// - Resolves claim IDs to claim text from claims
/// - Sorts by strength descending (strongest violations first)
pub fn collect_violations(
    raw: Vec<RawViolation>,
    strengths: &HashMap<String, f64>,
    thresholds: &SeverityThresholds,
    constraint_meta: &HashMap<String, ConstraintMeta>,
    claims: &[crate::claim::types::Claim],
) -> Vec<Violation> {
    let mut violations: Vec<Violation> = raw
        .into_iter()
        .filter(|r| r.violated)
        .map(|r| {
            let strength = strengths.get(&r.constraint_id).copied().unwrap_or(0.8);
            let severity = thresholds.determine(strength);

            // Look up constraint metadata
            let meta = constraint_meta.get(&r.constraint_id);
            let constraint_name = meta
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| r.constraint_id.clone());
            let epistemic_type = meta
                .as_ref()
                .map(|m| m.epistemic_type.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let rego_path = meta
                .as_ref()
                .map(|m| m.rego_path.clone())
                .unwrap_or_default();

            // Resolve claim IDs to claim text
            let claim_text: Vec<String> = r
                .claims
                .iter()
                .filter_map(|claim_id| {
                    claims
                        .iter()
                        .find(|c| &c.id == claim_id)
                        .map(|c| c.text.clone())
                })
                .collect();

            Violation {
                constraint_id: r.constraint_id,
                constraint_name,
                epistemic_type,
                rego_path,
                claim_ids: r.claims,
                claim_text,
                message: r.reason,
                strength,
                severity,
            }
        })
        .collect();

    // Sort by strength descending (strongest violations first)
    violations.sort_by(|a, b| {
        b.strength
            .partial_cmp(&a.strength)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    violations
}

/// Determine the overall decision from a set of violations.
///
/// - Any Block severity -> Block decision with violations
/// - Any Warn severity -> Warn decision with violations
/// - Otherwise -> Allow
pub fn determine_decision(violations: &[Violation]) -> Decision {
    let block_violations: Vec<Violation> = violations
        .iter()
        .filter(|v| v.severity == Severity::Block)
        .cloned()
        .collect();

    if !block_violations.is_empty() {
        return Decision::Block {
            violations: block_violations,
        };
    }

    let warn_violations: Vec<Violation> = violations
        .iter()
        .filter(|v| v.severity == Severity::Warn)
        .cloned()
        .collect();

    if !warn_violations.is_empty() {
        return Decision::Warn {
            violations: warn_violations,
        };
    }

    Decision::Allow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(id: &str, violated: bool, reason: &str) -> RawViolation {
        RawViolation {
            constraint_id: id.to_string(),
            violated,
            claims: vec!["c1".to_string()],
            reason: reason.to_string(),
        }
    }

    fn default_thresholds() -> SeverityThresholds {
        SeverityThresholds::default()
    }

    #[test]
    fn test_empty_violations_allow() {
        let violations = collect_violations(
            vec![],
            &HashMap::new(),
            &default_thresholds(),
            &HashMap::new(),
            &[],
        );
        assert!(violations.is_empty());
        assert_eq!(determine_decision(&violations), Decision::Allow);
    }

    #[test]
    fn test_warn_violations() {
        let mut strengths = HashMap::new();
        strengths.insert("warn-constraint".to_string(), 0.5); // Between 0.4 and 0.7 -> Warn

        let mut meta = HashMap::new();
        meta.insert(
            "warn-constraint".to_string(),
            ConstraintMeta {
                name: "Warning Constraint".to_string(),
                epistemic_type: "belief".to_string(),
                rego_path: "data.rigor.warn_constraint".to_string(),
            },
        );

        let violations = collect_violations(
            vec![raw("warn-constraint", true, "some warning")],
            &strengths,
            &default_thresholds(),
            &meta,
            &[],
        );

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].severity, Severity::Warn);
        assert_eq!(violations[0].constraint_name, "Warning Constraint");
        assert_eq!(violations[0].epistemic_type, "belief");

        match determine_decision(&violations) {
            Decision::Warn { violations } => {
                assert_eq!(violations.len(), 1);
                assert_eq!(violations[0].constraint_id, "warn-constraint");
            }
            other => panic!("Expected Warn, got {:?}", other),
        }
    }

    #[test]
    fn test_block_violation_present() {
        let mut strengths = HashMap::new();
        strengths.insert("block-constraint".to_string(), 0.9); // >= 0.7 -> Block

        let mut meta = HashMap::new();
        meta.insert(
            "block-constraint".to_string(),
            ConstraintMeta {
                name: "Block Constraint".to_string(),
                epistemic_type: "defeater".to_string(),
                rego_path: "data.rigor.block_constraint".to_string(),
            },
        );

        let violations = collect_violations(
            vec![raw("block-constraint", true, "critical issue")],
            &strengths,
            &default_thresholds(),
            &meta,
            &[],
        );

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].severity, Severity::Block);
        assert_eq!(violations[0].constraint_name, "Block Constraint");
        assert_eq!(violations[0].epistemic_type, "defeater");

        match determine_decision(&violations) {
            Decision::Block { violations } => {
                assert_eq!(violations.len(), 1);
                assert_eq!(violations[0].constraint_id, "block-constraint");
            }
            other => panic!("Expected Block, got {:?}", other),
        }
    }

    #[test]
    fn test_violations_sorted_by_strength_descending() {
        let mut strengths = HashMap::new();
        strengths.insert("low".to_string(), 0.3);
        strengths.insert("high".to_string(), 0.9);
        strengths.insert("mid".to_string(), 0.5);

        let violations = collect_violations(
            vec![
                raw("low", true, "low"),
                raw("high", true, "high"),
                raw("mid", true, "mid"),
            ],
            &strengths,
            &default_thresholds(),
            &HashMap::new(),
            &[],
        );

        assert_eq!(violations.len(), 3);
        assert_eq!(violations[0].constraint_id, "high");
        assert_eq!(violations[1].constraint_id, "mid");
        assert_eq!(violations[2].constraint_id, "low");
    }

    #[test]
    fn test_unknown_constraint_gets_default_strength() {
        let violations = collect_violations(
            vec![raw("unknown", true, "unknown constraint")],
            &HashMap::new(), // No strengths configured
            &default_thresholds(),
            &HashMap::new(),
            &[],
        );

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].strength, 0.8);
        // 0.8 >= 0.7 -> Block
        assert_eq!(violations[0].severity, Severity::Block);
        // Unknown constraint should use constraint_id as name
        assert_eq!(violations[0].constraint_name, "unknown");
        assert_eq!(violations[0].epistemic_type, "unknown");
        assert_eq!(violations[0].rego_path, "");
    }

    #[test]
    fn test_non_violated_filtered_out() {
        let violations = collect_violations(
            vec![raw("a", false, "not violated"), raw("b", true, "violated")],
            &HashMap::new(),
            &default_thresholds(),
            &HashMap::new(),
            &[],
        );

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].constraint_id, "b");
    }
}
