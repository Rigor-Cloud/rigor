use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::claim::KnowledgeType;

/// Top-level rigor.yaml configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RigorConfig {
    #[serde(default)]
    pub constraints: ConstraintsSection,
    #[serde(default)]
    pub relations: Vec<Relation>,
}

impl RigorConfig {
    /// Collect all constraints across epistemic categories.
    pub fn all_constraints(&self) -> Vec<&Constraint> {
        let mut all = Vec::new();
        all.extend(self.constraints.beliefs.iter());
        all.extend(self.constraints.justifications.iter());
        all.extend(self.constraints.defeaters.iter());
        all
    }
}

/// Constraints grouped by epistemic category.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConstraintsSection {
    #[serde(default)]
    pub beliefs: Vec<Constraint>,
    #[serde(default)]
    pub justifications: Vec<Constraint>,
    #[serde(default)]
    pub defeaters: Vec<Constraint>,
}

/// A single epistemic constraint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    pub id: String,
    pub epistemic_type: EpistemicType,
    pub name: String,
    pub description: String,
    pub rego: String,
    pub message: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
    /// Code locations that ground this constraint — the source of truth.
    /// If these lines change, the constraint should be re-evaluated.
    #[serde(default)]
    pub source: Vec<SourceAnchor>,

    // -------------------------------------------------------------------
    // Epistemic classification (Phase 0A).
    // -------------------------------------------------------------------
    /// Knowledge-type axis for this constraint (empirical/rational/
    /// testimonial/memory). Orthogonal to `epistemic_type`, which is the
    /// argumentation axis (belief/justification/defeater). Optional so
    /// user-authored rigor.yaml without the field deserializes unchanged.
    #[serde(default)]
    pub knowledge_type: Option<KnowledgeType>,

    // -------------------------------------------------------------------
    // Dynamic strength fields (Phase 0B).
    //
    // Consumed by Phase 4B's `compute_base_strength`. Today `graph.rs:50-54`
    // uses hardcoded defaults per EpistemicType; these fields let the
    // formula mix in per-constraint overrides, induction, and decay. All
    // optional so absence preserves current behavior.
    // -------------------------------------------------------------------
    /// If present, overrides the hardcoded per-type base strength.
    #[serde(default)]
    pub base_strength_override: Option<f64>,
    /// When this constraint was last verified (by LSP anchor check,
    /// judge verdict, or human annotation). Drives decay in Phase 4B.
    #[serde(default)]
    pub last_verified: Option<DateTime<Utc>>,
    /// Number of times this constraint has been successfully verified.
    /// Drives induction bonus in Phase 4B.
    #[serde(default)]
    pub verification_count: u64,
    /// Git commit at which the most recent verification happened.
    #[serde(default)]
    pub verified_at_commit: Option<String>,
    /// For testimonial constraints, weight applied based on the source's
    /// credibility (model tier, human, etc.). Defaults to 1.0 when absent.
    #[serde(default)]
    pub credibility_weight: Option<f64>,
    /// Leiden-cluster membership (Phase 2C). Enables cluster-aware
    /// context injection in Phase 2B.
    #[serde(default)]
    pub cluster_id: Option<String>,
}

/// A code location that grounds a constraint's truth.
/// The `anchor` pattern is more durable than line numbers — lines shift
/// with edits, but the text pattern can be re-found via grep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceAnchor {
    /// File path relative to project root
    pub path: String,
    /// Line number(s) where the truth is defined
    #[serde(default)]
    pub lines: Vec<u32>,
    /// Text pattern that grounds the truth (greppable, survives line shifts)
    #[serde(default)]
    pub anchor: Option<String>,
    /// Function or struct name for context
    #[serde(default)]
    pub context: Option<String>,
    /// SHA256 of the file content at last verification (Phase 0D).
    /// Used by Phase 2D's Gettier guard to invalidate cached verdicts
    /// when the file changes.
    #[serde(default)]
    pub file_sha256: Option<String>,
    /// SHA256 of the `anchor` substring at last verification (Phase 0D).
    /// Protects against semantically-different text that happens to match
    /// the same grep pattern.
    #[serde(default)]
    pub anchor_sha256: Option<String>,
}

/// Epistemic category of a constraint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EpistemicType {
    Belief,
    Justification,
    Defeater,
}

/// A relation between two constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub from: String,
    pub to: String,
    pub relation_type: RelationType,
    /// Per-edge confidence (Phase 0C). Multiplied into DF-QuAD attacker/
    /// supporter products in Phase 2G. Defaults to 1.0 so existing
    /// rigor.yaml files keep their current behavior.
    #[serde(default = "default_relation_confidence")]
    pub confidence: f64,
    /// How this relation was derived (Phase 0C).
    #[serde(default)]
    pub extraction_method: Option<ExtractionMethod>,
}

fn default_relation_confidence() -> f64 {
    1.0
}

/// Type of argumentation relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationType {
    Supports,
    Attacks,
    Undercuts,
}

/// How a `Relation` was extracted (Phase 0C). Enables Phase 1.5 Rigor
/// Learn to down-weight speculative edges vs. manually-authored ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionMethod {
    /// Derived from AST walk during `rigor map`.
    Ast,
    /// Derived by an LLM pass (judge or extractor).
    Llm,
    /// Inferred by DF-QuAD propagation or other rule-based heuristic.
    Inferred,
    /// Authored by a human in rigor.yaml.
    Manual,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // Phase 0A — KnowledgeType on Constraint
    // -------------------------------------------------------------------

    #[test]
    fn constraint_deserializes_without_knowledge_type() {
        // Historical rigor.yaml doesn't have the field; deserialization
        // must not fail and the field defaults to None.
        let yaml = r#"
id: c1
epistemic_type: belief
name: test
description: d
rego: "x"
message: m
"#;
        let c: Constraint = serde_yml::from_str(yaml).unwrap();
        assert!(c.knowledge_type.is_none());
    }

    #[test]
    fn constraint_round_trip_preserves_knowledge_type() {
        let c = Constraint {
            id: "c1".into(),
            epistemic_type: EpistemicType::Belief,
            name: "n".into(),
            description: "d".into(),
            rego: "r".into(),
            message: "m".into(),
            tags: vec![],
            domain: None,
            references: vec![],
            source: vec![],
            knowledge_type: Some(KnowledgeType::Empirical),
            base_strength_override: None,
            last_verified: None,
            verification_count: 0,
            verified_at_commit: None,
            credibility_weight: None,
            cluster_id: None,
        };
        let yaml = serde_yml::to_string(&c).unwrap();
        let back: Constraint = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(back.knowledge_type, Some(KnowledgeType::Empirical));
    }

    // -------------------------------------------------------------------
    // Phase 0B — Dynamic strength fields
    // -------------------------------------------------------------------

    #[test]
    fn constraint_dynamic_strength_fields_default_to_none_or_zero() {
        let yaml = r#"
id: c1
epistemic_type: belief
name: test
description: d
rego: "x"
message: m
"#;
        let c: Constraint = serde_yml::from_str(yaml).unwrap();
        assert!(c.base_strength_override.is_none());
        assert!(c.last_verified.is_none());
        assert_eq!(c.verification_count, 0);
        assert!(c.verified_at_commit.is_none());
        assert!(c.credibility_weight.is_none());
        assert!(c.cluster_id.is_none());
    }

    #[test]
    fn constraint_dynamic_strength_fields_round_trip() {
        let now = Utc::now();
        let c = Constraint {
            id: "c1".into(),
            epistemic_type: EpistemicType::Belief,
            name: "n".into(),
            description: "d".into(),
            rego: "r".into(),
            message: "m".into(),
            tags: vec![],
            domain: None,
            references: vec![],
            source: vec![],
            knowledge_type: None,
            base_strength_override: Some(0.95),
            last_verified: Some(now),
            verification_count: 42,
            verified_at_commit: Some("abc123".into()),
            credibility_weight: Some(0.85),
            cluster_id: Some("cluster-3".into()),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: Constraint = serde_json::from_str(&json).unwrap();
        assert_eq!(back.base_strength_override, Some(0.95));
        assert_eq!(back.last_verified.unwrap().timestamp(), now.timestamp());
        assert_eq!(back.verification_count, 42);
        assert_eq!(back.verified_at_commit.as_deref(), Some("abc123"));
        assert_eq!(back.credibility_weight, Some(0.85));
        assert_eq!(back.cluster_id.as_deref(), Some("cluster-3"));
    }

    // -------------------------------------------------------------------
    // Phase 0C — Relation confidence + extraction_method
    // -------------------------------------------------------------------

    #[test]
    fn relation_deserializes_without_confidence_defaulting_to_one() {
        let yaml = r#"
from: c1
to: c2
relation_type: supports
"#;
        let r: Relation = serde_yml::from_str(yaml).unwrap();
        assert_eq!(r.confidence, 1.0);
        assert!(r.extraction_method.is_none());
    }

    #[test]
    fn relation_round_trip_preserves_confidence_and_method() {
        let r = Relation {
            from: "c1".into(),
            to: "c2".into(),
            relation_type: RelationType::Attacks,
            confidence: 0.75,
            extraction_method: Some(ExtractionMethod::Llm),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: Relation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.confidence, 0.75);
        assert_eq!(back.extraction_method, Some(ExtractionMethod::Llm));
    }

    #[test]
    fn extraction_method_serializes_as_snake_case() {
        let json = serde_json::to_string(&ExtractionMethod::Inferred).unwrap();
        assert_eq!(json, "\"inferred\"");
    }

    // -------------------------------------------------------------------
    // Phase 0D — SourceAnchor fingerprinting
    // -------------------------------------------------------------------

    #[test]
    fn source_anchor_deserializes_without_fingerprints() {
        let yaml = r#"
path: src/foo.rs
lines: [10, 11]
anchor: "fn foo"
"#;
        let a: SourceAnchor = serde_yml::from_str(yaml).unwrap();
        assert!(a.file_sha256.is_none());
        assert!(a.anchor_sha256.is_none());
    }

    #[test]
    fn source_anchor_fingerprints_round_trip() {
        let a = SourceAnchor {
            path: "src/foo.rs".into(),
            lines: vec![10],
            anchor: Some("fn foo".into()),
            context: None,
            file_sha256: Some("deadbeef".into()),
            anchor_sha256: Some("cafebabe".into()),
        };
        let json = serde_json::to_string(&a).unwrap();
        let back: SourceAnchor = serde_json::from_str(&json).unwrap();
        assert_eq!(back.file_sha256.as_deref(), Some("deadbeef"));
        assert_eq!(back.anchor_sha256.as_deref(), Some("cafebabe"));
    }
}
