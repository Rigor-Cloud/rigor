use serde::{Deserialize, Serialize};

/// A constraint violation detected during evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Violation {
    pub constraint_id: String,
    pub constraint_name: String,
    pub epistemic_type: String,
    pub rego_path: String,
    pub claim_ids: Vec<String>,
    pub claim_text: Vec<String>,
    pub message: String,
    pub strength: f64,
    pub severity: Severity,
}

/// Severity level derived from violation strength.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Block,
    Warn,
    Allow,
}

/// Thresholds for mapping strength to severity.
#[derive(Debug, Clone)]
pub struct SeverityThresholds {
    pub block_threshold: f64,
    pub warn_threshold: f64,
}

impl Default for SeverityThresholds {
    fn default() -> Self {
        Self {
            block_threshold: 0.7,
            warn_threshold: 0.4,
        }
    }
}

impl SeverityThresholds {
    /// Determine severity from a violation strength value.
    pub fn determine(&self, strength: f64) -> Severity {
        if strength >= self.block_threshold {
            Severity::Block
        } else if strength >= self.warn_threshold {
            Severity::Warn
        } else {
            Severity::Allow
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threshold_exactly_at_block() {
        let t = SeverityThresholds::default();
        assert_eq!(t.determine(0.7), Severity::Block); // exactly at boundary
    }

    #[test]
    fn test_threshold_just_below_block() {
        let t = SeverityThresholds::default();
        assert_eq!(t.determine(0.6999999), Severity::Warn);
    }

    #[test]
    fn test_threshold_exactly_at_warn() {
        let t = SeverityThresholds::default();
        assert_eq!(t.determine(0.4), Severity::Warn);
    }

    #[test]
    fn test_threshold_just_below_warn() {
        let t = SeverityThresholds::default();
        assert_eq!(t.determine(0.3999999), Severity::Allow);
    }

    #[test]
    fn test_threshold_zero() {
        let t = SeverityThresholds::default();
        assert_eq!(t.determine(0.0), Severity::Allow);
    }

    #[test]
    fn test_threshold_one() {
        let t = SeverityThresholds::default();
        assert_eq!(t.determine(1.0), Severity::Block);
    }
}
