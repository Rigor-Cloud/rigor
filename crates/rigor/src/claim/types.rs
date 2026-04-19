use serde::{Deserialize, Serialize};

/// Location in the source transcript where a claim originated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub message_index: usize,
    pub sentence_index: usize,
}

/// A claim extracted from LLM output, used as Rego input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub domain: Option<String>,
    pub confidence: f64,
    pub claim_type: ClaimType,
    #[serde(default)]
    pub source_line: Option<usize>,
    #[serde(default)]
    pub source: Option<SourceLocation>,
}

impl Claim {
    /// Create a new claim with all required fields.
    pub fn new(
        text: String,
        confidence: f64,
        claim_type: ClaimType,
        source: Option<SourceLocation>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            text,
            domain: None,
            confidence,
            claim_type,
            source_line: None,
            source,
        }
    }
}

/// Category of claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimType {
    Assertion,
    Negation,
    CodeReference,
    ArchitecturalDecision,
    DependencyClaim,
    ActionIntent,
}
