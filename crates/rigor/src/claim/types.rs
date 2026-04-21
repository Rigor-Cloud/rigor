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
    /// Epistemic classification — empirical / rational / testimonial / memory.
    /// Orthogonal to `claim_type` (which is the intent axis). Optional so
    /// older extraction paths and deserialized historical claims continue
    /// to work unchanged.
    #[serde(default)]
    pub knowledge_type: Option<KnowledgeType>,
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
            knowledge_type: None,
        }
    }
}

/// Category of claim — intent axis (rule-derived from text shape).
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

/// Epistemological classification — justification axis.
///
/// Separate from [`ClaimType`] because the same textual assertion can have
/// different epistemic standing depending on its grounding: a code-anchored
/// claim verified by grep/LSP is `Empirical`; the same sentence paraphrased
/// from a README is `Testimonial`. Downstream evaluators and DF-QuAD base
/// strength use this to weight claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeType {
    /// Code-anchored, verified via grep or LSP.
    Empirical,
    /// Derived from other claims via DF-QuAD propagation.
    Rational,
    /// From documentation, README, or an LLM — requires credibility weighting.
    Testimonial,
    /// Reused from a prior `rigor map` run or prior session.
    Memory,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_type_serde_round_trip_all_variants() {
        for kt in [
            KnowledgeType::Empirical,
            KnowledgeType::Rational,
            KnowledgeType::Testimonial,
            KnowledgeType::Memory,
        ] {
            let json = serde_json::to_string(&kt).unwrap();
            let back: KnowledgeType = serde_json::from_str(&json).unwrap();
            assert_eq!(kt, back, "round trip failed for {:?}", kt);
        }
    }

    #[test]
    fn knowledge_type_serializes_as_snake_case() {
        let json = serde_json::to_string(&KnowledgeType::Empirical).unwrap();
        assert_eq!(json, "\"empirical\"");
    }

    #[test]
    fn claim_new_defaults_knowledge_type_to_none() {
        let c = Claim::new("x".into(), 0.5, ClaimType::Assertion, None);
        assert!(c.knowledge_type.is_none());
    }

    #[test]
    fn claim_deserialize_without_knowledge_type_field() {
        // Historical claims don't have the field; deserialization must not fail.
        let json = r#"{
            "id": "c1",
            "text": "x",
            "confidence": 0.7,
            "claim_type": "assertion"
        }"#;
        let c: Claim = serde_json::from_str(json).unwrap();
        assert!(c.knowledge_type.is_none());
    }

    #[test]
    fn claim_round_trip_preserves_knowledge_type() {
        let mut c = Claim::new("x".into(), 0.5, ClaimType::Assertion, None);
        c.knowledge_type = Some(KnowledgeType::Empirical);
        let json = serde_json::to_string(&c).unwrap();
        let back: Claim = serde_json::from_str(&json).unwrap();
        assert_eq!(back.knowledge_type, Some(KnowledgeType::Empirical));
    }
}
