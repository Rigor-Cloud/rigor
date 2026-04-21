use anyhow::Result;
use serde::Serialize;

use crate::claim::Claim;

/// Input data model for Rego evaluation.
/// Claims are serialized as `input.claims` for Rego rules.
#[derive(Debug, Clone, Serialize)]
pub struct EvaluationInput {
    pub claims: Vec<Claim>,
}

impl EvaluationInput {
    /// Serialize the evaluation input to a serde_json::Value for regorus.
    pub fn to_json_value(&self) -> Result<serde_json::Value> {
        let value = serde_json::to_value(self)?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::ClaimType;

    #[test]
    fn test_evaluation_input_serializes() {
        let input = EvaluationInput {
            claims: vec![Claim {
                id: "c1".to_string(),
                text: "Rust is memory safe".to_string(),
                domain: Some("safety".to_string()),
                confidence: 0.9,
                claim_type: ClaimType::Assertion,
                source_line: None,
                source: None,
                knowledge_type: None,
            }],
        };
        let json = input.to_json_value().unwrap();
        assert!(json["claims"].is_array());
        assert_eq!(json["claims"][0]["text"], "Rust is memory safe");
    }
}
