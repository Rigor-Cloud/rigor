use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::minimums::Minimums;
use super::types::{FailureCategory, Policy, PolicySet, RetryConfig};

/// Per-component policy override.
///
/// Any field left as `None` falls through to the default `PolicySet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentPolicy {
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub on_config_error: Option<Policy>,
    #[serde(default)]
    pub on_dependency_missing: Option<Policy>,
    #[serde(default)]
    pub on_transient_error: Option<Policy>,
    #[serde(default)]
    pub on_persistent_error: Option<Policy>,
    #[serde(default)]
    pub retry: Option<RetryConfig>,
}

/// Top-level fallback configuration parsed from rigor.yaml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    #[serde(default)]
    pub minimums: Minimums,
    pub default: PolicySet,
    #[serde(default)]
    pub components: HashMap<String, ComponentPolicy>,
}

/// Raw YAML shape: rigor.yaml has `fallback:` as a top-level key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigorYamlFallback {
    #[serde(default)]
    pub fallback: Option<FallbackConfig>,
}

impl FallbackConfig {
    /// Create a sensible default configuration.
    ///
    /// - `on_config_error`: FailStartup
    /// - `on_dependency_missing`: FailStartup
    /// - `on_transient_error`: RetryThenFailClosed { attempts: 3, backoff_ms: [500, 2000, 8000] }
    /// - `on_persistent_error`: FailClosed
    /// - No components, no minimums.
    pub fn default_config() -> Self {
        FallbackConfig {
            minimums: Minimums::default(),
            default: PolicySet {
                on_config_error: Policy::FailStartup,
                on_dependency_missing: Policy::FailStartup,
                on_transient_error: Policy::RetryThenFailClosed {
                    attempts: 3,
                    backoff_ms: vec![500, 2000, 8000],
                },
                on_persistent_error: Policy::FailClosed,
                retry: None,
            },
            components: HashMap::new(),
        }
    }

    /// Read a YAML file, extract the `fallback:` section, and return
    /// `FallbackConfig`. Returns `default_config()` if the file has no
    /// `fallback:` key.
    pub fn from_yaml(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read fallback config from {}", path.display()))?;
        let wrapper: RigorYamlFallback = serde_yml::from_str(&contents)
            .with_context(|| format!("failed to parse YAML from {}", path.display()))?;
        Ok(wrapper.fallback.unwrap_or_else(Self::default_config))
    }

    /// Resolve the effective policy for a component + failure category.
    ///
    /// Resolution order: component override -> default fallthrough.
    /// Returns an owned `Policy` (clone).
    pub fn policy_for(&self, component: &str, cat: FailureCategory) -> Policy {
        if let Some(comp) = self.components.get(component) {
            let override_policy = match cat {
                FailureCategory::ConfigError => comp.on_config_error.as_ref(),
                FailureCategory::DependencyMissing => comp.on_dependency_missing.as_ref(),
                FailureCategory::TransientError => comp.on_transient_error.as_ref(),
                FailureCategory::PersistentError => comp.on_persistent_error.as_ref(),
            };
            if let Some(p) = override_policy {
                return p.clone();
            }
        }
        self.default.policy_for(cat).clone()
    }

    /// Validate the entire configuration at startup.
    ///
    /// - Validates default policies against global minimums.
    /// - Validates each component's resolved policy against its category minimums.
    /// - Validates retry configs (non-empty backoff arrays, attempts > 0).
    /// - Returns `Err` with a precise message on the first violation.
    pub fn validate(&self) -> Result<()> {
        // Validate retry configs in the default PolicySet.
        Self::validate_policy_retry(&self.default.on_config_error, "default.on_config_error")?;
        Self::validate_policy_retry(
            &self.default.on_dependency_missing,
            "default.on_dependency_missing",
        )?;
        Self::validate_policy_retry(
            &self.default.on_transient_error,
            "default.on_transient_error",
        )?;
        Self::validate_policy_retry(
            &self.default.on_persistent_error,
            "default.on_persistent_error",
        )?;
        if let Some(ref retry) = self.default.retry {
            Self::validate_retry_config(retry, "default.retry")?;
        }

        // Validate default policies against global minimums.
        for cat in ALL_FAILURE_CATEGORIES {
            let policy = self.default.policy_for(*cat);
            self.minimums
                .enforce("default", None, *cat, policy)
                .with_context(|| {
                    format!(
                        "default policy for {:?} violates global minimums",
                        cat
                    )
                })?;
        }

        // Validate each component's resolved policy.
        for (name, comp) in &self.components {
            let comp_category = comp.category.as_deref();

            // Validate component-level retry configs.
            if let Some(ref p) = comp.on_config_error {
                Self::validate_policy_retry(p, &format!("{}.on_config_error", name))?;
            }
            if let Some(ref p) = comp.on_dependency_missing {
                Self::validate_policy_retry(p, &format!("{}.on_dependency_missing", name))?;
            }
            if let Some(ref p) = comp.on_transient_error {
                Self::validate_policy_retry(p, &format!("{}.on_transient_error", name))?;
            }
            if let Some(ref p) = comp.on_persistent_error {
                Self::validate_policy_retry(p, &format!("{}.on_persistent_error", name))?;
            }
            if let Some(ref retry) = comp.retry {
                Self::validate_retry_config(retry, &format!("{}.retry", name))?;
            }

            // Validate each failure category against minimums.
            for cat in ALL_FAILURE_CATEGORIES {
                let resolved = self.policy_for(name, *cat);
                self.minimums
                    .enforce(name, comp_category, *cat, &resolved)
                    .with_context(|| {
                        format!(
                            "component '{}' resolved policy for {:?} violates minimums",
                            name, cat
                        )
                    })?;
            }
        }

        Ok(())
    }

    /// Validate that a retry-bearing Policy has valid retry parameters.
    fn validate_policy_retry(policy: &Policy, label: &str) -> Result<()> {
        match policy {
            Policy::RetryThenFailClosed { attempts, backoff_ms }
            | Policy::RetryThenFailOpen { attempts, backoff_ms }
            | Policy::RetryThenDegrade { attempts, backoff_ms } => {
                if *attempts == 0 {
                    bail!(
                        "{}: retry policy has attempts=0, must be > 0",
                        label
                    );
                }
                if backoff_ms.is_empty() {
                    bail!(
                        "{}: retry policy has empty backoff_ms, must be non-empty",
                        label
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Validate a standalone RetryConfig.
    fn validate_retry_config(retry: &RetryConfig, label: &str) -> Result<()> {
        if retry.attempts == 0 {
            bail!("{}: retry config has attempts=0, must be > 0", label);
        }
        if retry.backoff_ms.is_empty() {
            bail!(
                "{}: retry config has empty backoff_ms, must be non-empty",
                label
            );
        }
        Ok(())
    }
}

const ALL_FAILURE_CATEGORIES: &[FailureCategory] = &[
    FailureCategory::ConfigError,
    FailureCategory::DependencyMissing,
    FailureCategory::TransientError,
    FailureCategory::PersistentError,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Full YAML config used by tests 1-4.
    fn full_config_yaml() -> &'static str {
        r#"fallback:
  minimums:
    on_config_error: fail_startup
    privacy_sensitive:
      on_dependency_missing: fail_startup
      on_persistent_error: fail_closed
  default:
    on_config_error: fail_startup
    on_dependency_missing: fail_startup
    on_transient_error:
      retry_then_fail_closed:
        attempts: 3
        backoff_ms: [500, 2000, 8000]
    on_persistent_error: fail_closed
  components:
    pseudonymize:
      category: privacy_sensitive
    claim_injection:
      on_persistent_error: degrade_with_warn
    judge_api:
      category: best_effort
      on_transient_error:
        retry_then_degrade:
          attempts: 2
          backoff_ms: [1000, 5000]
      on_persistent_error: degrade_with_warn
"#
    }

    fn write_yaml_to_tempfile(yaml: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("create temp file");
        f.write_all(yaml.as_bytes()).expect("write yaml");
        f.flush().expect("flush");
        f
    }

    fn load_full_config() -> FallbackConfig {
        let f = write_yaml_to_tempfile(full_config_yaml());
        FallbackConfig::from_yaml(f.path()).expect("should parse full config")
    }

    // ---- Test 1 ----
    #[test]
    fn parse_full_config() {
        let cfg = load_full_config();

        // Default policies.
        assert_eq!(cfg.default.on_config_error, Policy::FailStartup);
        assert_eq!(cfg.default.on_dependency_missing, Policy::FailStartup);
        assert_eq!(cfg.default.on_persistent_error, Policy::FailClosed);
        match &cfg.default.on_transient_error {
            Policy::RetryThenFailClosed { attempts, backoff_ms } => {
                assert_eq!(*attempts, 3);
                assert_eq!(*backoff_ms, vec![500, 2000, 8000]);
            }
            other => panic!("expected RetryThenFailClosed, got {:?}", other),
        }

        // Components exist.
        assert!(
            cfg.components.contains_key("pseudonymize"),
            "should have pseudonymize component"
        );
        assert!(
            cfg.components.contains_key("claim_injection"),
            "should have claim_injection component"
        );
        assert!(
            cfg.components.contains_key("judge_api"),
            "should have judge_api component"
        );
        assert_eq!(cfg.components.len(), 3);

        // Verify component details.
        let pseudo = &cfg.components["pseudonymize"];
        assert_eq!(pseudo.category.as_deref(), Some("privacy_sensitive"));
        assert!(pseudo.on_persistent_error.is_none());

        let judge = &cfg.components["judge_api"];
        assert_eq!(judge.category.as_deref(), Some("best_effort"));
        match judge.on_transient_error.as_ref().unwrap() {
            Policy::RetryThenDegrade { attempts, backoff_ms } => {
                assert_eq!(*attempts, 2);
                assert_eq!(*backoff_ms, vec![1000, 5000]);
            }
            other => panic!("expected RetryThenDegrade, got {:?}", other),
        }
    }

    // ---- Test 2 ----
    #[test]
    fn policy_for_with_component_override() {
        let cfg = load_full_config();

        // claim_injection overrides on_persistent_error to DegradeWithWarn.
        assert_eq!(
            cfg.policy_for("claim_injection", FailureCategory::PersistentError),
            Policy::DegradeWithWarn,
        );

        // claim_injection inherits default for on_config_error.
        assert_eq!(
            cfg.policy_for("claim_injection", FailureCategory::ConfigError),
            Policy::FailStartup,
        );

        // judge_api overrides on_transient_error.
        match cfg.policy_for("judge_api", FailureCategory::TransientError) {
            Policy::RetryThenDegrade { attempts, backoff_ms } => {
                assert_eq!(attempts, 2);
                assert_eq!(backoff_ms, vec![1000, 5000]);
            }
            other => panic!("expected RetryThenDegrade, got {:?}", other),
        }
    }

    // ---- Test 3 ----
    #[test]
    fn policy_for_unknown_component_uses_default() {
        let cfg = load_full_config();

        assert_eq!(
            cfg.policy_for("nonexistent_component", FailureCategory::ConfigError),
            Policy::FailStartup,
        );
        assert_eq!(
            cfg.policy_for("nonexistent_component", FailureCategory::PersistentError),
            Policy::FailClosed,
        );
        match cfg.policy_for("nonexistent_component", FailureCategory::TransientError) {
            Policy::RetryThenFailClosed { attempts, backoff_ms } => {
                assert_eq!(attempts, 3);
                assert_eq!(backoff_ms, vec![500, 2000, 8000]);
            }
            other => panic!("expected RetryThenFailClosed, got {:?}", other),
        }
    }

    // ---- Test 4 ----
    #[test]
    fn validate_compliant_config_passes() {
        let cfg = load_full_config();
        cfg.validate().expect("full config should pass validation");
    }

    // ---- Test 5 ----
    #[test]
    fn validate_rejects_looser_than_minimum() {
        let yaml = r#"fallback:
  minimums:
    privacy_sensitive:
      on_persistent_error: fail_closed
  default:
    on_config_error: fail_startup
    on_dependency_missing: fail_startup
    on_transient_error:
      retry_then_fail_closed:
        attempts: 3
        backoff_ms: [500, 2000, 8000]
    on_persistent_error: fail_closed
  components:
    pseudonymize:
      category: privacy_sensitive
      on_persistent_error: fail_open
"#;
        let f = write_yaml_to_tempfile(yaml);
        let cfg = FallbackConfig::from_yaml(f.path()).expect("should parse");
        let result = cfg.validate();
        assert!(result.is_err(), "should reject FailOpen for privacy_sensitive persistent_error");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("pseudonymize"),
            "error should mention component name, got: {}",
            err_msg
        );
    }

    // ---- Test 6 ----
    #[test]
    fn validate_rejects_empty_backoff() {
        let yaml = r#"fallback:
  default:
    on_config_error: fail_startup
    on_dependency_missing: fail_startup
    on_transient_error:
      retry_then_fail_closed:
        attempts: 3
        backoff_ms: []
    on_persistent_error: fail_closed
"#;
        let f = write_yaml_to_tempfile(yaml);
        let cfg = FallbackConfig::from_yaml(f.path()).expect("should parse");
        let result = cfg.validate();
        assert!(result.is_err(), "should reject empty backoff_ms");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("empty backoff_ms"),
            "error should mention empty backoff_ms, got: {}",
            err_msg
        );
    }

    // ---- Test 7 ----
    #[test]
    fn validate_rejects_zero_attempts() {
        let yaml = r#"fallback:
  default:
    on_config_error: fail_startup
    on_dependency_missing: fail_startup
    on_transient_error:
      retry_then_fail_closed:
        attempts: 0
        backoff_ms: [500]
    on_persistent_error: fail_closed
"#;
        let f = write_yaml_to_tempfile(yaml);
        let cfg = FallbackConfig::from_yaml(f.path()).expect("should parse");
        let result = cfg.validate();
        assert!(result.is_err(), "should reject attempts=0");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("attempts=0"),
            "error should mention attempts=0, got: {}",
            err_msg
        );
    }

    // ---- Test 8 ----
    #[test]
    fn default_config_validates() {
        let cfg = FallbackConfig::default_config();
        cfg.validate()
            .expect("default_config() should always pass validate()");
    }

    // ---- Test 9 ----
    #[test]
    fn no_fallback_section_returns_default() {
        let yaml = r#"
some_other_key: true
version: 1
"#;
        let f = write_yaml_to_tempfile(yaml);
        let contents = std::fs::read_to_string(f.path()).unwrap();
        let wrapper: RigorYamlFallback = serde_yml::from_str(&contents).expect("should parse");
        assert!(
            wrapper.fallback.is_none(),
            "fallback field should be None when YAML has no fallback: key"
        );
    }
}
