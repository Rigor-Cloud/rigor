use crate::violation::types::Violation;
use owo_colors::OwoColorize;
use std::collections::HashMap;
use std::io::IsTerminal;

/// A group of violations for the same constraint.
#[derive(Debug, Clone)]
pub struct ViolationGroup {
    pub constraint_id: String,
    pub constraint_name: String,
    pub epistemic_type: String,
    pub rego_path: String,
    pub strength: f64,
    pub count: usize,
    pub first_violation: Violation,
}

/// Violations grouped by severity.
#[derive(Debug)]
pub struct GroupedViolations {
    pub blocks: Vec<ViolationGroup>,
    pub warns: Vec<ViolationGroup>,
}

impl GroupedViolations {
    /// Group violations by constraint_id within each severity level.
    /// Skips Allow-level violations.
    pub fn from_violations(violations: &[Violation]) -> Self {
        let mut block_map: HashMap<String, ViolationGroup> = HashMap::new();
        let mut warn_map: HashMap<String, ViolationGroup> = HashMap::new();

        for violation in violations {
            match violation.severity {
                crate::violation::types::Severity::Block => {
                    block_map
                        .entry(violation.constraint_id.clone())
                        .and_modify(|group| group.count += 1)
                        .or_insert_with(|| ViolationGroup {
                            constraint_id: violation.constraint_id.clone(),
                            constraint_name: violation.constraint_name.clone(),
                            epistemic_type: violation.epistemic_type.clone(),
                            rego_path: violation.rego_path.clone(),
                            strength: violation.strength,
                            count: 1,
                            first_violation: violation.clone(),
                        });
                }
                crate::violation::types::Severity::Warn => {
                    warn_map
                        .entry(violation.constraint_id.clone())
                        .and_modify(|group| group.count += 1)
                        .or_insert_with(|| ViolationGroup {
                            constraint_id: violation.constraint_id.clone(),
                            constraint_name: violation.constraint_name.clone(),
                            epistemic_type: violation.epistemic_type.clone(),
                            rego_path: violation.rego_path.clone(),
                            strength: violation.strength,
                            count: 1,
                            first_violation: violation.clone(),
                        });
                }
                crate::violation::types::Severity::Allow => {
                    // Skip allow-level violations
                }
            }
        }

        Self {
            blocks: block_map.into_values().collect(),
            warns: warn_map.into_values().collect(),
        }
    }

    /// Generate a summary line like "2 blocks, 3 warnings across 4 constraints".
    pub fn summary_line(&self) -> String {
        let block_count = self.blocks.len();
        let warn_count = self.warns.len();
        let total_constraints = block_count + warn_count;

        if total_constraints == 0 {
            return String::new();
        }

        let mut parts = Vec::new();

        if block_count > 0 {
            let block_str = if block_count == 1 {
                "1 block".to_string()
            } else {
                format!("{} blocks", block_count)
            };
            parts.push(block_str);
        }

        if warn_count > 0 {
            let warn_str = if warn_count == 1 {
                "1 warning".to_string()
            } else {
                format!("{} warnings", warn_count)
            };
            parts.push(warn_str);
        }

        let constraint_str = if total_constraints == 1 {
            "1 constraint".to_string()
        } else {
            format!("{} constraints", total_constraints)
        };

        format!("{} across {}", parts.join(", "), constraint_str)
    }
}

/// Formatter for violations with grouping, deduplication, and colored output.
pub struct ViolationFormatter {
    use_color: bool,
}

impl ViolationFormatter {
    /// Create a new formatter with automatic terminal detection.
    pub fn new() -> Self {
        // Detect if stderr is a terminal
        let is_terminal = std::io::stderr().is_terminal();

        // Check NO_COLOR env var (standard convention)
        let no_color = std::env::var("NO_COLOR").is_ok();

        // Check if TERM is "dumb"
        let term_dumb = std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false);

        let use_color = is_terminal && !no_color && !term_dumb;

        Self { use_color }
    }

    /// Create a formatter with explicit color setting (for testing).
    pub fn with_color(use_color: bool) -> Self {
        Self { use_color }
    }

    /// Format violations with grouping, severity prefixes, and coloring.
    /// Returns empty string if no block or warn violations exist.
    pub fn format_violations(&self, violations: &[Violation]) -> String {
        let grouped = GroupedViolations::from_violations(violations);

        // Return empty string if nothing to show
        if grouped.blocks.is_empty() && grouped.warns.is_empty() {
            return String::new();
        }

        let mut output = Vec::new();

        // Add summary line
        output.push(grouped.summary_line());
        output.push(String::new()); // Blank line after summary

        // Format blocks (detailed)
        for group in &grouped.blocks {
            let prefix = if self.use_color {
                format!("BLOCK ({:.2}):", group.strength)
                    .red()
                    .bold()
                    .to_string()
            } else {
                format!("BLOCK ({:.2}):", group.strength)
            };

            let header = format!(
                "{} [{}] (category: {}, rule: {})",
                prefix, group.constraint_name, group.epistemic_type, group.rego_path
            );
            output.push(header);

            // Claim text (with count if duplicated)
            if !group.first_violation.claim_text.is_empty() {
                if group.count > 1 {
                    output.push(format!(
                        "  Constraint violated {} times (showing first): {}",
                        group.count,
                        group.first_violation.claim_text.join(", ")
                    ));
                } else {
                    output.push(format!(
                        "  Claim: {}",
                        group.first_violation.claim_text.join(", ")
                    ));
                }
            }

            // Reason
            output.push(format!("  Reason: {}", group.first_violation.message));
            output.push(String::new()); // Blank line between violations
        }

        // Format warnings (compact)
        for group in &grouped.warns {
            let prefix = if self.use_color {
                format!("WARN ({:.2}):", group.strength)
                    .yellow()
                    .to_string()
            } else {
                format!("WARN ({:.2}):", group.strength)
            };

            let count_suffix = if group.count > 1 {
                format!(" (×{})", group.count)
            } else {
                String::new()
            };

            let line = format!(
                "{} [{}]{} - {}",
                prefix, group.constraint_name, count_suffix, group.first_violation.message
            );
            output.push(line);
        }

        // Remove trailing blank line if present
        while output.last().map(|s| s.is_empty()).unwrap_or(false) {
            output.pop();
        }

        output.join("\n")
    }
}

impl Default for ViolationFormatter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::violation::types::{Severity, Violation};

    fn make_violation(
        constraint_id: &str,
        constraint_name: &str,
        epistemic_type: &str,
        message: &str,
        strength: f64,
        severity: Severity,
        claim_text: Vec<String>,
    ) -> Violation {
        Violation {
            constraint_id: constraint_id.to_string(),
            constraint_name: constraint_name.to_string(),
            epistemic_type: epistemic_type.to_string(),
            rego_path: format!("data.rigor.{}", constraint_id),
            claim_ids: vec![],
            claim_text,
            message: message.to_string(),
            strength,
            severity,
        }
    }

    #[test]
    fn test_empty_violations() {
        let formatter = ViolationFormatter::with_color(false);
        let violations = vec![];
        let output = formatter.format_violations(&violations);
        assert_eq!(output, "");
    }

    #[test]
    fn test_single_block_violation() {
        let formatter = ViolationFormatter::with_color(false);
        let violations = vec![make_violation(
            "no_api_fabrication",
            "No API Fabrication",
            "belief",
            "Claimed API that doesn't exist",
            0.85,
            Severity::Block,
            vec!["The API supports feature X".to_string()],
        )];

        let output = formatter.format_violations(&violations);

        assert!(output.contains("1 block across 1 constraint"));
        assert!(output.contains("BLOCK (0.85): [No API Fabrication]"));
        assert!(output.contains("category: belief"));
        assert!(output.contains("rule: data.rigor.no_api_fabrication"));
        assert!(output.contains("Claim: The API supports feature X"));
        assert!(output.contains("Reason: Claimed API that doesn't exist"));
    }

    #[test]
    fn test_single_warn_violation() {
        let formatter = ViolationFormatter::with_color(false);
        let violations = vec![make_violation(
            "hedge_ratio",
            "Hedge Ratio",
            "justification",
            "Too many hedge words",
            0.45,
            Severity::Warn,
            vec![],
        )];

        let output = formatter.format_violations(&violations);

        assert!(output.contains("1 warning across 1 constraint"));
        assert!(output.contains("WARN (0.45): [Hedge Ratio]"));
        assert!(output.contains("Too many hedge words"));
        // Warnings are compact - no claim text shown even if present
        assert!(!output.contains("Claim:"));
    }

    #[test]
    fn test_mixed_block_and_warn() {
        let formatter = ViolationFormatter::with_color(false);
        let violations = vec![
            make_violation(
                "no_api_fabrication",
                "No API Fabrication",
                "belief",
                "Claimed API that doesn't exist",
                0.85,
                Severity::Block,
                vec!["The API supports feature X".to_string()],
            ),
            make_violation(
                "hedge_ratio",
                "Hedge Ratio",
                "justification",
                "Too many hedge words",
                0.45,
                Severity::Warn,
                vec![],
            ),
        ];

        let output = formatter.format_violations(&violations);

        // Summary shows both
        assert!(output.contains("1 block, 1 warning across 2 constraints"));

        // Blocks appear first
        let block_pos = output.find("BLOCK").unwrap();
        let warn_pos = output.find("WARN").unwrap();
        assert!(block_pos < warn_pos);
    }

    #[test]
    fn test_duplicate_constraint_collapsing() {
        let formatter = ViolationFormatter::with_color(false);
        let violations = vec![
            make_violation(
                "no_api_fabrication",
                "No API Fabrication",
                "belief",
                "Claimed API that doesn't exist",
                0.85,
                Severity::Block,
                vec!["The API supports feature X".to_string()],
            ),
            make_violation(
                "no_api_fabrication",
                "No API Fabrication",
                "belief",
                "Claimed API that doesn't exist",
                0.85,
                Severity::Block,
                vec!["The API supports feature Y".to_string()],
            ),
            make_violation(
                "no_api_fabrication",
                "No API Fabrication",
                "belief",
                "Claimed API that doesn't exist",
                0.85,
                Severity::Block,
                vec!["The API supports feature Z".to_string()],
            ),
        ];

        let output = formatter.format_violations(&violations);

        // Should show "1 block" not "3 blocks"
        assert!(output.contains("1 block across 1 constraint"));

        // Should show count and first occurrence
        assert!(output.contains("Constraint violated 3 times (showing first)"));
        assert!(output.contains("The API supports feature X"));

        // Should NOT show other claims
        assert!(!output.contains("The API supports feature Y"));
        assert!(!output.contains("The API supports feature Z"));
    }

    #[test]
    fn test_allow_violations_excluded() {
        let formatter = ViolationFormatter::with_color(false);
        let violations = vec![
            make_violation(
                "no_api_fabrication",
                "No API Fabrication",
                "belief",
                "Claimed API that doesn't exist",
                0.85,
                Severity::Block,
                vec!["The API supports feature X".to_string()],
            ),
            make_violation(
                "low_severity",
                "Low Severity",
                "defeater",
                "Minor issue",
                0.2,
                Severity::Allow,
                vec![],
            ),
        ];

        let output = formatter.format_violations(&violations);

        // Only 1 constraint (block), allow excluded
        assert!(output.contains("1 block across 1 constraint"));
        assert!(!output.contains("Low Severity"));
        assert!(!output.contains("Minor issue"));
    }

    #[test]
    fn test_no_color_produces_no_ansi() {
        let formatter = ViolationFormatter::with_color(false);
        let violations = vec![make_violation(
            "no_api_fabrication",
            "No API Fabrication",
            "belief",
            "Claimed API that doesn't exist",
            0.85,
            Severity::Block,
            vec!["The API supports feature X".to_string()],
        )];

        let output = formatter.format_violations(&violations);

        // Should not contain ANSI escape codes
        assert!(!output.contains("\x1b["));
        assert!(output.contains("BLOCK (0.85):"));
    }

    #[test]
    fn test_with_color_produces_ansi() {
        let formatter = ViolationFormatter::with_color(true);
        let violations = vec![make_violation(
            "no_api_fabrication",
            "No API Fabrication",
            "belief",
            "Claimed API that doesn't exist",
            0.85,
            Severity::Block,
            vec!["The API supports feature X".to_string()],
        )];

        let output = formatter.format_violations(&violations);

        // Should contain ANSI escape codes for red color
        assert!(output.contains("\x1b["));
    }

    #[test]
    fn test_summary_edge_cases() {
        let grouped_empty = GroupedViolations {
            blocks: vec![],
            warns: vec![],
        };
        assert_eq!(grouped_empty.summary_line(), "");

        let grouped_blocks_only = GroupedViolations {
            blocks: vec![ViolationGroup {
                constraint_id: "test".to_string(),
                constraint_name: "Test".to_string(),
                epistemic_type: "belief".to_string(),
                rego_path: "data.rigor.test".to_string(),
                strength: 0.85,
                count: 1,
                first_violation: make_violation(
                    "test",
                    "Test",
                    "belief",
                    "msg",
                    0.85,
                    Severity::Block,
                    vec![],
                ),
            }],
            warns: vec![],
        };
        assert_eq!(
            grouped_blocks_only.summary_line(),
            "1 block across 1 constraint"
        );

        let grouped_warns_only = GroupedViolations {
            blocks: vec![],
            warns: vec![ViolationGroup {
                constraint_id: "test".to_string(),
                constraint_name: "Test".to_string(),
                epistemic_type: "belief".to_string(),
                rego_path: "data.rigor.test".to_string(),
                strength: 0.45,
                count: 1,
                first_violation: make_violation(
                    "test",
                    "Test",
                    "belief",
                    "msg",
                    0.45,
                    Severity::Warn,
                    vec![],
                ),
            }],
        };
        assert_eq!(
            grouped_warns_only.summary_line(),
            "1 warning across 1 constraint"
        );
    }
}
