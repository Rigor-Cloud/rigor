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

    // --- Custom threshold + midpoint tests (gap 7) ---

    #[test]
    fn test_custom_thresholds() {
        // Non-default thresholds: block at 0.9, warn at 0.5.
        let t = SeverityThresholds {
            block_threshold: 0.9,
            warn_threshold: 0.5,
        };
        assert_eq!(t.determine(0.9), Severity::Block, "exactly at custom block");
        assert_eq!(t.determine(0.89), Severity::Warn, "just below custom block");
        assert_eq!(t.determine(0.5), Severity::Warn, "exactly at custom warn");
        assert_eq!(t.determine(0.49), Severity::Allow, "just below custom warn");
    }

    #[test]
    fn test_threshold_midpoint() {
        let t = SeverityThresholds::default();
        // 0.55 is the midpoint between warn (0.4) and block (0.7) -> Warn
        assert_eq!(
            t.determine(0.55),
            Severity::Warn,
            "midpoint between warn and block thresholds"
        );
        // 0.85 is the midpoint between block (0.7) and 1.0 -> Block
        assert_eq!(
            t.determine(0.85),
            Severity::Block,
            "midpoint between block threshold and 1.0"
        );
    }
}
