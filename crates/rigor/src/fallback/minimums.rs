use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use anyhow::{bail, Result};
use super::types::{FailureCategory, Policy};

/// Per-category policy floor for a named minimum group.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CategoryMinimum {
    #[serde(default)]
    pub on_config_error: Option<Policy>,
    #[serde(default)]
    pub on_dependency_missing: Option<Policy>,
    #[serde(default)]
    pub on_transient_error: Option<Policy>,
    #[serde(default)]
    pub on_persistent_error: Option<Policy>,
}

/// Safety floors that cannot be overridden looser.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Minimums {
    /// Global minimum for config errors — applies to ALL components.
    #[serde(default)]
    pub on_config_error: Option<Policy>,
    /// Named category minimums (e.g. "privacy_sensitive").
    #[serde(flatten)]
    pub categories: HashMap<String, CategoryMinimum>,
}

impl CategoryMinimum {
    /// Return the minimum policy for a given failure category, if set.
    fn floor_for(&self, cat: FailureCategory) -> Option<&Policy> {
        match cat {
            FailureCategory::ConfigError => self.on_config_error.as_ref(),
            FailureCategory::DependencyMissing => self.on_dependency_missing.as_ref(),
            FailureCategory::TransientError => self.on_transient_error.as_ref(),
            FailureCategory::PersistentError => self.on_persistent_error.as_ref(),
        }
    }
}

impl Minimums {
    /// Check that a component's policy meets all applicable minimums.
    ///
    /// - Checks global minimums first (e.g. `on_config_error` applies to ALL components).
    /// - Then checks category-specific minimums if `component_category` matches a named group.
    /// - Returns `Ok(())` if compliant, `Err` with descriptive message if not.
    pub fn enforce(
        &self,
        component_name: &str,
        component_category: Option<&str>,
        failure_cat: FailureCategory,
        policy: &Policy,
    ) -> Result<()> {
        // Check global minimums first.
        let global_floor = match failure_cat {
            FailureCategory::ConfigError => self.on_config_error.as_ref(),
            // Only on_config_error is a global minimum field on Minimums.
            _ => None,
        };

        if let Some(floor) = global_floor {
            if !policy.is_at_least_as_strict_as(floor) {
                bail!(
                    "Component '{}' policy {:?} for {:?} is looser than the global minimum {:?}. \
                     Refusing to start.",
                    component_name,
                    policy,
                    failure_cat,
                    floor,
                );
            }
        }

        // Check category-specific minimums.
        if let Some(cat_name) = component_category {
            if let Some(cat_min) = self.categories.get(cat_name) {
                if let Some(floor) = cat_min.floor_for(failure_cat) {
                    if !policy.is_at_least_as_strict_as(floor) {
                        bail!(
                            "Component '{}' (category '{}') policy {:?} for {:?} is looser than \
                             the category minimum {:?}. Refusing to start.",
                            component_name,
                            cat_name,
                            policy,
                            failure_cat,
                            floor,
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn privacy_minimums() -> Minimums {
        let mut categories = HashMap::new();
        categories.insert(
            "privacy_sensitive".to_string(),
            CategoryMinimum {
                on_config_error: None,
                on_dependency_missing: None,
                on_transient_error: None,
                on_persistent_error: Some(Policy::FailClosed),
            },
        );
        Minimums {
            on_config_error: None,
            categories,
        }
    }

    #[test]
    fn compliant_policy_passes() {
        let mins = privacy_minimums();
        let result = mins.enforce(
            "pii_scrubber",
            Some("privacy_sensitive"),
            FailureCategory::PersistentError,
            &Policy::FailClosed,
        );
        assert!(result.is_ok(), "FailClosed should meet FailClosed minimum");
    }

    #[test]
    fn stricter_than_minimum_passes() {
        let mins = privacy_minimums();
        let result = mins.enforce(
            "pii_scrubber",
            Some("privacy_sensitive"),
            FailureCategory::PersistentError,
            &Policy::FailStartup,
        );
        assert!(
            result.is_ok(),
            "FailStartup (stricter) should meet FailClosed minimum"
        );
    }

    #[test]
    fn looser_than_minimum_fails() {
        let mins = privacy_minimums();
        let result = mins.enforce(
            "pii_scrubber",
            Some("privacy_sensitive"),
            FailureCategory::PersistentError,
            &Policy::FailOpen,
        );
        assert!(result.is_err(), "FailOpen should fail FailClosed minimum");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("pii_scrubber"),
            "Error should contain component name, got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("privacy_sensitive"),
            "Error should contain category, got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("Refusing to start"),
            "Error should contain 'Refusing to start.', got: {}",
            err_msg
        );
    }

    #[test]
    fn global_config_error_minimum_enforced() {
        let mins = Minimums {
            on_config_error: Some(Policy::FailStartup),
            categories: HashMap::new(),
        };
        let result = mins.enforce(
            "some_component",
            None,
            FailureCategory::ConfigError,
            &Policy::DegradeWithWarn,
        );
        assert!(
            result.is_err(),
            "DegradeWithWarn should fail FailStartup global minimum on config_error"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Refusing to start"),
            "Error should contain 'Refusing to start.', got: {}",
            err_msg
        );
    }

    #[test]
    fn uncategorized_component_only_checks_global() {
        let mins = Minimums {
            on_config_error: None,
            categories: HashMap::new(),
        };
        // No global minimum for persistent_error, no category — should pass even with FailOpen.
        let result = mins.enforce(
            "logger",
            None,
            FailureCategory::PersistentError,
            &Policy::FailOpen,
        );
        assert!(
            result.is_ok(),
            "Uncategorized component with no global minimum should pass with FailOpen"
        );
    }

    #[test]
    fn yaml_parsing() {
        let yaml = r#"
on_config_error: fail_startup
privacy_sensitive:
  on_dependency_missing: fail_startup
  on_persistent_error: fail_closed
"#;
        let mins: Minimums = serde_yml::from_str(yaml).expect("should parse Minimums YAML");
        assert_eq!(
            mins.on_config_error,
            Some(Policy::FailStartup),
            "global on_config_error should be FailStartup"
        );
        let ps = mins
            .categories
            .get("privacy_sensitive")
            .expect("should have privacy_sensitive category");
        assert_eq!(
            ps.on_dependency_missing,
            Some(Policy::FailStartup),
            "privacy_sensitive.on_dependency_missing should be FailStartup"
        );
        assert_eq!(
            ps.on_persistent_error,
            Some(Policy::FailClosed),
            "privacy_sensitive.on_persistent_error should be FailClosed"
        );
        assert_eq!(
            ps.on_config_error, None,
            "privacy_sensitive.on_config_error should be None"
        );
        assert_eq!(
            ps.on_transient_error, None,
            "privacy_sensitive.on_transient_error should be None"
        );
    }
}
