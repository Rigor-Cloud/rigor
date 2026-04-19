pub mod types;
pub use types::*;
pub mod minimums;
pub use minimums::*;
pub mod config;
pub use config::*;

use std::future::Future;
use std::time::Duration;
use tracing::{error, info, warn};

impl FallbackConfig {
    /// Execute an operation under fallback policy governance.
    ///
    /// `op` is called once. On success, returns `FallbackOutcome::Ok`.
    /// On failure, the policy for `(component, category)` determines behavior:
    /// terminal policies apply immediately; retry policies re-invoke `op`
    /// up to `attempts` additional times with backoff delays.
    pub async fn execute<T, F, Fut>(
        &self,
        component: &str,
        op: F,
    ) -> FallbackOutcome<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, (FailureCategory, String)>>,
    {
        // Initial call
        match op().await {
            Ok(val) => {
                info!(component = component, "operation succeeded");
                return FallbackOutcome::Ok(val);
            }
            Err((cat, reason)) => {
                let policy = self.policy_for(component, cat);
                self.apply_policy(component, cat, &reason, policy, &op).await
            }
        }
    }

    /// Apply a resolved policy after a failure.
    async fn apply_policy<T, F, Fut>(
        &self,
        component: &str,
        cat: FailureCategory,
        reason: &str,
        policy: Policy,
        op: &F,
    ) -> FallbackOutcome<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, (FailureCategory, String)>>,
    {
        match policy {
            Policy::FailStartup | Policy::FailClosed => {
                error!(
                    component = component,
                    category = ?cat,
                    reason = reason,
                    "operation blocked by policy"
                );
                FallbackOutcome::Blocked(format!(
                    "{}: {:?} — {}",
                    component, cat, reason
                ))
            }
            Policy::FailOpen | Policy::DegradeWithWarn => {
                warn!(
                    component = component,
                    category = ?cat,
                    reason = reason,
                    "operation skipped by policy"
                );
                FallbackOutcome::Skipped
            }
            Policy::RetryThenFailClosed { attempts, backoff_ms } => {
                match Self::retry_loop(component, cat, attempts, &backoff_ms, op).await {
                    Some(outcome) => outcome,
                    None => {
                        error!(
                            component = component,
                            category = ?cat,
                            reason = reason,
                            attempts = attempts,
                            "retries exhausted, blocking"
                        );
                        FallbackOutcome::Blocked(format!(
                            "{}: retries exhausted ({} attempts) — {}",
                            component, attempts, reason
                        ))
                    }
                }
            }
            Policy::RetryThenFailOpen { attempts, backoff_ms } => {
                match Self::retry_loop(component, cat, attempts, &backoff_ms, op).await {
                    Some(outcome) => outcome,
                    None => {
                        warn!(
                            component = component,
                            category = ?cat,
                            reason = reason,
                            attempts = attempts,
                            "retries exhausted, skipping"
                        );
                        FallbackOutcome::Skipped
                    }
                }
            }
            Policy::RetryThenDegrade { attempts, backoff_ms } => {
                match Self::retry_loop(component, cat, attempts, &backoff_ms, op).await {
                    Some(outcome) => outcome,
                    None => {
                        warn!(
                            component = component,
                            category = ?cat,
                            reason = reason,
                            attempts = attempts,
                            "retries exhausted, degrading"
                        );
                        FallbackOutcome::Skipped
                    }
                }
            }
        }
    }

    /// Retry `op` up to `attempts` times with backoff delays between each attempt.
    ///
    /// Returns `Some(FallbackOutcome::Ok(val))` on success, `None` if all retries exhausted.
    async fn retry_loop<T, F, Fut>(
        component: &str,
        cat: FailureCategory,
        attempts: u32,
        backoff_ms: &[u64],
        op: &F,
    ) -> Option<FallbackOutcome<T>>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, (FailureCategory, String)>>,
    {
        for i in 0..attempts {
            let delay = backoff_ms
                .get(i as usize)
                .copied()
                .unwrap_or_else(|| *backoff_ms.last().unwrap_or(&1000));
            tokio::time::sleep(Duration::from_millis(delay)).await;
            match op().await {
                Ok(val) => {
                    info!(
                        component = component,
                        category = ?cat,
                        retry = i + 1,
                        "retry succeeded"
                    );
                    return Some(FallbackOutcome::Ok(val));
                }
                Err((new_cat, new_reason)) => {
                    warn!(
                        component = component,
                        category = ?new_cat,
                        retry = i + 1,
                        reason = new_reason.as_str(),
                        "retry failed"
                    );
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::collections::HashMap;

    /// Helper: build a FallbackConfig with the given default on_persistent_error policy.
    fn config_with_persistent(policy: Policy) -> FallbackConfig {
        FallbackConfig {
            minimums: Minimums::default(),
            default: PolicySet {
                on_config_error: Policy::FailStartup,
                on_dependency_missing: Policy::FailStartup,
                on_transient_error: Policy::RetryThenFailClosed {
                    attempts: 3,
                    backoff_ms: vec![500, 2000, 8000],
                },
                on_persistent_error: policy,
                retry: None,
            },
            components: HashMap::new(),
        }
    }

    /// Helper: build a FallbackConfig with a specific component override for transient errors.
    fn config_with_component_transient(component: &str, policy: Policy) -> FallbackConfig {
        let mut components = HashMap::new();
        components.insert(
            component.to_string(),
            ComponentPolicy {
                category: None,
                on_config_error: None,
                on_dependency_missing: None,
                on_transient_error: Some(policy),
                on_persistent_error: None,
                retry: None,
            },
        );
        FallbackConfig {
            minimums: Minimums::default(),
            default: PolicySet {
                on_config_error: Policy::FailStartup,
                on_dependency_missing: Policy::FailStartup,
                on_transient_error: Policy::FailClosed,
                on_persistent_error: Policy::FailClosed,
                retry: None,
            },
            components,
        }
    }

    // ---- Test 1: execute_success_returns_ok ----
    #[tokio::test]
    async fn execute_success_returns_ok() {
        let cfg = FallbackConfig::default_config();
        let result = cfg
            .execute("test_comp", || async {
                Ok::<i32, (FailureCategory, String)>(42)
            })
            .await;

        match result {
            FallbackOutcome::Ok(v) => assert_eq!(v, 42),
            other => panic!(
                "expected Ok(42), got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    // ---- Test 2: execute_persistent_error_blocks ----
    #[tokio::test]
    async fn execute_persistent_error_blocks() {
        let cfg = config_with_persistent(Policy::FailClosed);
        let result = cfg
            .execute("test_comp", || async {
                Err::<i32, _>((
                    FailureCategory::PersistentError,
                    "db unreachable".to_string(),
                ))
            })
            .await;

        match result {
            FallbackOutcome::Blocked(msg) => {
                assert!(
                    msg.contains("test_comp"),
                    "blocked message should contain component name, got: {}",
                    msg
                );
            }
            other => panic!(
                "expected Blocked, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    // ---- Test 3: execute_retry_succeeds_on_second_attempt ----
    #[tokio::test]
    async fn execute_retry_succeeds_on_second_attempt() {
        let call_count = Arc::new(AtomicU32::new(0));
        let cfg = config_with_component_transient(
            "retry_comp",
            Policy::RetryThenFailClosed {
                attempts: 3,
                backoff_ms: vec![10, 20, 30],
            },
        );

        let cc = call_count.clone();
        let result = cfg
            .execute("retry_comp", move || {
                let cc = cc.clone();
                async move {
                    let n = cc.fetch_add(1, Ordering::SeqCst) + 1;
                    if n == 1 {
                        Err((
                            FailureCategory::TransientError,
                            "transient failure".to_string(),
                        ))
                    } else {
                        Ok(99)
                    }
                }
            })
            .await;

        match result {
            FallbackOutcome::Ok(v) => assert_eq!(v, 99),
            other => panic!(
                "expected Ok(99), got {:?}",
                std::mem::discriminant(&other)
            ),
        }
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            2,
            "op should have been called exactly twice (1 initial + 1 retry)"
        );
    }

    // ---- Test 4: execute_retry_exhausted_blocks ----
    #[tokio::test]
    async fn execute_retry_exhausted_blocks() {
        let call_count = Arc::new(AtomicU32::new(0));
        let cfg = config_with_component_transient(
            "retry_comp",
            Policy::RetryThenFailClosed {
                attempts: 3,
                backoff_ms: vec![10, 20, 30],
            },
        );

        let cc = call_count.clone();
        let result = cfg
            .execute("retry_comp", move || {
                let cc = cc.clone();
                async move {
                    cc.fetch_add(1, Ordering::SeqCst);
                    Err::<i32, _>((
                        FailureCategory::TransientError,
                        "still failing".to_string(),
                    ))
                }
            })
            .await;

        match result {
            FallbackOutcome::Blocked(msg) => {
                assert!(
                    msg.contains("retries exhausted"),
                    "blocked message should mention retries exhausted, got: {}",
                    msg
                );
            }
            other => panic!(
                "expected Blocked, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            4,
            "op should have been called 4 times (1 initial + 3 retries)"
        );
    }

    // ---- Test 5: execute_fail_open_skips ----
    #[tokio::test]
    async fn execute_fail_open_skips() {
        let cfg = config_with_persistent(Policy::FailOpen);
        let result = cfg
            .execute("open_comp", || async {
                Err::<i32, _>((
                    FailureCategory::PersistentError,
                    "not critical".to_string(),
                ))
            })
            .await;

        match result {
            FallbackOutcome::Skipped => {} // expected
            other => panic!(
                "expected Skipped, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    // ---- Test 6: execute_degrade_with_warn_skips ----
    #[tokio::test]
    async fn execute_degrade_with_warn_skips() {
        let cfg = config_with_persistent(Policy::DegradeWithWarn);
        let result = cfg
            .execute("degrade_comp", || async {
                Err::<i32, _>((
                    FailureCategory::PersistentError,
                    "degraded".to_string(),
                ))
            })
            .await;

        match result {
            FallbackOutcome::Skipped => {} // expected
            other => panic!(
                "expected Skipped, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }
}
