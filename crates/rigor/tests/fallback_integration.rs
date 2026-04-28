#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! Integration tests for fallback policy system.
//!
//! These tests verify that the full config → resolve → execute pipeline works
//! end-to-end with realistic YAML configurations.

use rigor::fallback::*;

#[test]
fn full_pipeline_compliant_config_validates_and_resolves() {
    let yaml = r#"
fallback:
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
"#;
    let parsed: RigorYamlFallback = serde_yml::from_str(yaml).unwrap();
    let config = parsed.fallback.unwrap();

    // Validate passes
    config.validate().expect("compliant config should validate");

    // claim_injection overrides persistent_error
    assert_eq!(
        config.policy_for("claim_injection", FailureCategory::PersistentError),
        Policy::DegradeWithWarn
    );

    // pseudonymize inherits default for persistent_error (FailClosed)
    // — which meets the privacy_sensitive minimum
    assert_eq!(
        config.policy_for("pseudonymize", FailureCategory::PersistentError),
        Policy::FailClosed
    );
}

#[test]
fn full_pipeline_rejects_config_violating_minimum() {
    let yaml = r#"
fallback:
  minimums:
    privacy_sensitive:
      on_persistent_error: fail_closed
  default:
    on_config_error: fail_startup
    on_dependency_missing: fail_startup
    on_transient_error: fail_closed
    on_persistent_error: fail_closed
  components:
    pseudonymize:
      category: privacy_sensitive
      on_persistent_error: degrade_with_warn
"#;
    let parsed: RigorYamlFallback = serde_yml::from_str(yaml).unwrap();
    let config = parsed.fallback.unwrap();

    let err = config.validate().unwrap_err();
    // anyhow's Display only shows the outermost context; use Debug to see the full chain
    // including the inner "Refusing to start" message from minimums enforcement.
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("pseudonymize"),
        "should name the offending component: {msg}"
    );
    assert!(
        msg.contains("Refusing to start"),
        "should mention refusing to start: {msg}"
    );
}

#[tokio::test]
async fn execute_with_component_override_uses_correct_policy() {
    let yaml = r#"
fallback:
  minimums: {}
  default:
    on_config_error: fail_startup
    on_dependency_missing: fail_startup
    on_transient_error: fail_closed
    on_persistent_error: fail_closed
  components:
    judge_api:
      on_persistent_error: degrade_with_warn
"#;
    let parsed: RigorYamlFallback = serde_yml::from_str(yaml).unwrap();
    let config = parsed.fallback.unwrap();

    // judge_api with degrade_with_warn should skip, not block
    let result: FallbackOutcome<i32> = config
        .execute("judge_api", || async {
            Err((FailureCategory::PersistentError, "api down".to_string()))
        })
        .await;
    assert!(matches!(result, FallbackOutcome::Skipped));

    // A different component without override should block
    let result: FallbackOutcome<i32> = config
        .execute("other", || async {
            Err((FailureCategory::PersistentError, "api down".to_string()))
        })
        .await;
    assert!(matches!(result, FallbackOutcome::Blocked(_)));
}
