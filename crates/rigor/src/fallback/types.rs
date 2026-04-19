use std::fmt;
use serde::{Deserialize, Serialize, Serializer, Deserializer};
use serde::de::{self, Visitor, MapAccess};

/// What kind of failure occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    ConfigError,
    DependencyMissing,
    TransientError,
    PersistentError,
}

/// What rigor does in response to a failure.
///
/// Serde: unit variants serialize as plain strings (`fail_startup`).
/// Struct variants serialize as single-key maps (`retry_then_fail_closed: {attempts: 3, ...}`).
/// This matches the user-facing YAML format in rigor.yaml.
#[derive(Debug, Clone, PartialEq)]
pub enum Policy {
    FailStartup,
    FailClosed,
    FailOpen,
    DegradeWithWarn,
    RetryThenFailClosed {
        attempts: u32,
        backoff_ms: Vec<u64>,
    },
    RetryThenFailOpen {
        attempts: u32,
        backoff_ms: Vec<u64>,
    },
    RetryThenDegrade {
        attempts: u32,
        backoff_ms: Vec<u64>,
    },
}

const POLICY_VARIANTS: &[&str] = &[
    "fail_startup", "fail_closed", "fail_open", "degrade_with_warn",
    "retry_then_fail_closed", "retry_then_fail_open", "retry_then_degrade",
];

#[derive(Deserialize)]
struct RetryFields {
    attempts: u32,
    backoff_ms: Vec<u64>,
}

impl Serialize for Policy {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Policy::FailStartup => serializer.serialize_str("fail_startup"),
            Policy::FailClosed => serializer.serialize_str("fail_closed"),
            Policy::FailOpen => serializer.serialize_str("fail_open"),
            Policy::DegradeWithWarn => serializer.serialize_str("degrade_with_warn"),
            Policy::RetryThenFailClosed { attempts, backoff_ms } => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("retry_then_fail_closed", &RetryFieldsRef { attempts: *attempts, backoff_ms })?;
                map.end()
            }
            Policy::RetryThenFailOpen { attempts, backoff_ms } => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("retry_then_fail_open", &RetryFieldsRef { attempts: *attempts, backoff_ms })?;
                map.end()
            }
            Policy::RetryThenDegrade { attempts, backoff_ms } => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("retry_then_degrade", &RetryFieldsRef { attempts: *attempts, backoff_ms })?;
                map.end()
            }
        }
    }
}

#[derive(Serialize)]
struct RetryFieldsRef<'a> {
    attempts: u32,
    backoff_ms: &'a Vec<u64>,
}

impl<'de> Deserialize<'de> for Policy {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(PolicyVisitor)
    }
}

struct PolicyVisitor;

impl<'de> Visitor<'de> for PolicyVisitor {
    type Value = Policy;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "a policy string (e.g. \"fail_closed\") or a single-key map (e.g. {{retry_then_fail_closed: {{attempts: 3, backoff_ms: [100]}}}})")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        match v {
            "fail_startup" => Ok(Policy::FailStartup),
            "fail_closed" => Ok(Policy::FailClosed),
            "fail_open" => Ok(Policy::FailOpen),
            "degrade_with_warn" => Ok(Policy::DegradeWithWarn),
            _ => Err(E::unknown_variant(v, POLICY_VARIANTS)),
        }
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let key: String = map.next_key()?
            .ok_or_else(|| de::Error::custom("empty map for Policy"))?;

        let result = match key.as_str() {
            "retry_then_fail_closed" => {
                let fields: RetryFields = map.next_value()?;
                Ok(Policy::RetryThenFailClosed { attempts: fields.attempts, backoff_ms: fields.backoff_ms })
            }
            "retry_then_fail_open" => {
                let fields: RetryFields = map.next_value()?;
                Ok(Policy::RetryThenFailOpen { attempts: fields.attempts, backoff_ms: fields.backoff_ms })
            }
            "retry_then_degrade" => {
                let fields: RetryFields = map.next_value()?;
                Ok(Policy::RetryThenDegrade { attempts: fields.attempts, backoff_ms: fields.backoff_ms })
            }
            _ => Err(de::Error::unknown_variant(&key, POLICY_VARIANTS)),
        };

        // Ensure no extra keys
        if map.next_key::<String>()?.is_some() {
            return Err(de::Error::custom("Policy map must have exactly one key"));
        }

        result
    }
}

/// Result of running an operation under a fallback policy.
#[derive(Debug)]
pub enum FallbackOutcome<T> {
    /// Operation succeeded.
    Ok(T),
    /// Component was skipped (fail_open or degrade_with_warn).
    Skipped,
    /// Operation succeeded with caveats (degraded mode).
    Degraded(T),
    /// Request blocked (fail_closed). String contains the reason.
    Blocked(String),
}

/// A complete mapping from every FailureCategory to a Policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySet {
    pub on_config_error: Policy,
    pub on_dependency_missing: Policy,
    pub on_transient_error: Policy,
    pub on_persistent_error: Policy,
    #[serde(default)]
    pub retry: Option<RetryConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub attempts: u32,
    pub backoff_ms: Vec<u64>,
}

impl PolicySet {
    pub fn policy_for(&self, cat: FailureCategory) -> &Policy {
        match cat {
            FailureCategory::ConfigError => &self.on_config_error,
            FailureCategory::DependencyMissing => &self.on_dependency_missing,
            FailureCategory::TransientError => &self.on_transient_error,
            FailureCategory::PersistentError => &self.on_persistent_error,
        }
    }
}

impl Policy {
    /// Strictness rank: higher = stricter.
    /// fail_startup > fail_closed > retry_then_fail_closed > retry_then_degrade
    /// > degrade_with_warn > retry_then_fail_open > fail_open
    pub fn strictness(&self) -> u8 {
        match self {
            Policy::FailStartup => 7,
            Policy::FailClosed => 6,
            Policy::RetryThenFailClosed { .. } => 5,
            Policy::RetryThenDegrade { .. } => 4,
            Policy::DegradeWithWarn => 3,
            Policy::RetryThenFailOpen { .. } => 2,
            Policy::FailOpen => 1,
        }
    }

    pub fn is_at_least_as_strict_as(&self, floor: &Policy) -> bool {
        self.strictness() >= floor.strictness()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strictness_ordering() {
        assert_eq!(Policy::FailStartup.strictness(), 7);
        assert_eq!(Policy::FailClosed.strictness(), 6);
        assert_eq!(
            Policy::RetryThenFailClosed {
                attempts: 3,
                backoff_ms: vec![100, 200, 400],
            }
            .strictness(),
            5
        );
        assert_eq!(
            Policy::RetryThenDegrade {
                attempts: 3,
                backoff_ms: vec![100, 200, 400],
            }
            .strictness(),
            4
        );
        assert_eq!(Policy::DegradeWithWarn.strictness(), 3);
        assert_eq!(
            Policy::RetryThenFailOpen {
                attempts: 3,
                backoff_ms: vec![100, 200, 400],
            }
            .strictness(),
            2
        );
        assert_eq!(Policy::FailOpen.strictness(), 1);

        // Verify monotonic decrease
        assert!(Policy::FailStartup.strictness() > Policy::FailClosed.strictness());
        assert!(
            Policy::FailClosed.strictness()
                > Policy::RetryThenFailClosed {
                    attempts: 1,
                    backoff_ms: vec![100],
                }
                .strictness()
        );
        assert!(
            Policy::RetryThenFailClosed {
                attempts: 1,
                backoff_ms: vec![100],
            }
            .strictness()
                > Policy::RetryThenDegrade {
                    attempts: 1,
                    backoff_ms: vec![100],
                }
                .strictness()
        );
        assert!(
            Policy::RetryThenDegrade {
                attempts: 1,
                backoff_ms: vec![100],
            }
            .strictness()
                > Policy::DegradeWithWarn.strictness()
        );
        assert!(
            Policy::DegradeWithWarn.strictness()
                > Policy::RetryThenFailOpen {
                    attempts: 1,
                    backoff_ms: vec![100],
                }
                .strictness()
        );
        assert!(
            Policy::RetryThenFailOpen {
                attempts: 1,
                backoff_ms: vec![100],
            }
            .strictness()
                > Policy::FailOpen.strictness()
        );
    }

    #[test]
    fn fail_closed_is_at_least_as_strict_as_degrade() {
        assert!(Policy::FailClosed.is_at_least_as_strict_as(&Policy::DegradeWithWarn));
        assert!(Policy::FailClosed.is_at_least_as_strict_as(&Policy::FailOpen));
        assert!(Policy::FailClosed.is_at_least_as_strict_as(&Policy::FailClosed));
    }

    #[test]
    fn fail_open_not_as_strict_as_fail_closed() {
        assert!(!Policy::FailOpen.is_at_least_as_strict_as(&Policy::FailClosed));
        assert!(!Policy::FailOpen.is_at_least_as_strict_as(&Policy::DegradeWithWarn));
        assert!(!Policy::FailOpen.is_at_least_as_strict_as(&Policy::RetryThenFailOpen {
            attempts: 1,
            backoff_ms: vec![100],
        }));
        // FailOpen is at least as strict as itself
        assert!(Policy::FailOpen.is_at_least_as_strict_as(&Policy::FailOpen));
    }

    #[test]
    fn policy_set_lookup() {
        let ps = PolicySet {
            on_config_error: Policy::FailStartup,
            on_dependency_missing: Policy::FailClosed,
            on_transient_error: Policy::RetryThenFailOpen {
                attempts: 3,
                backoff_ms: vec![100, 200, 400],
            },
            on_persistent_error: Policy::DegradeWithWarn,
            retry: None,
        };

        assert_eq!(
            ps.policy_for(FailureCategory::ConfigError),
            &Policy::FailStartup
        );
        assert_eq!(
            ps.policy_for(FailureCategory::DependencyMissing),
            &Policy::FailClosed
        );
        assert_eq!(
            ps.policy_for(FailureCategory::TransientError),
            &Policy::RetryThenFailOpen {
                attempts: 3,
                backoff_ms: vec![100, 200, 400],
            }
        );
        assert_eq!(
            ps.policy_for(FailureCategory::PersistentError),
            &Policy::DegradeWithWarn
        );
    }

    #[test]
    fn yaml_round_trip() {
        // First, serialize a known PolicySet to discover the canonical YAML format
        let original = PolicySet {
            on_config_error: Policy::FailStartup,
            on_dependency_missing: Policy::FailClosed,
            on_transient_error: Policy::RetryThenFailOpen {
                attempts: 3,
                backoff_ms: vec![100, 200, 400],
            },
            on_persistent_error: Policy::DegradeWithWarn,
            retry: Some(RetryConfig {
                attempts: 5,
                backoff_ms: vec![50, 100, 200, 400, 800],
            }),
        };
        let yaml = serde_yml::to_string(&original).expect("should serialize to YAML");
        let ps: PolicySet = serde_yml::from_str(&yaml).expect("YAML should parse into PolicySet");

        assert_eq!(ps.on_config_error, Policy::FailStartup);
        assert_eq!(ps.on_dependency_missing, Policy::FailClosed);
        assert_eq!(
            ps.on_transient_error,
            Policy::RetryThenFailOpen {
                attempts: 3,
                backoff_ms: vec![100, 200, 400],
            }
        );
        assert_eq!(ps.on_persistent_error, Policy::DegradeWithWarn);

        let retry = ps.retry.expect("retry config should be present");
        assert_eq!(retry.attempts, 5);
        assert_eq!(retry.backoff_ms, vec![50, 100, 200, 400, 800]);
    }

    #[test]
    fn yaml_hand_authored_format() {
        // This tests the exact YAML format users will write in rigor.yaml.
        // Externally-tagged serde enums use variant name as map key.
        let yaml = r#"
on_config_error: fail_startup
on_dependency_missing: fail_startup
on_transient_error:
  retry_then_fail_closed:
    attempts: 3
    backoff_ms: [500, 2000, 8000]
on_persistent_error: fail_closed
"#;
        let ps: PolicySet = serde_yml::from_str(yaml).unwrap();
        assert_eq!(*ps.policy_for(FailureCategory::ConfigError), Policy::FailStartup);
        assert_eq!(*ps.policy_for(FailureCategory::DependencyMissing), Policy::FailStartup);
        assert_eq!(*ps.policy_for(FailureCategory::PersistentError), Policy::FailClosed);
        match ps.policy_for(FailureCategory::TransientError) {
            Policy::RetryThenFailClosed { attempts, backoff_ms } => {
                assert_eq!(*attempts, 3);
                assert_eq!(*backoff_ms, vec![500, 2000, 8000]);
            }
            other => panic!("expected RetryThenFailClosed, got {:?}", other),
        }
    }
}
