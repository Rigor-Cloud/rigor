use serde::{Deserialize, Serialize};

/// Top-level rigor.yaml configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigorConfig {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// Type of argumentation relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationType {
    Supports,
    Attacks,
    Undercuts,
}
