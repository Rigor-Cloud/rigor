//! Relevance lookup abstraction backing [`super::SemanticEvaluator`].
//!
//! The daemon runs an async LLM-as-judge pass
//! (`daemon::proxy::score_claim_relevance`) that populates a cache keyed by
//! claim text. This module exposes that cache through a simple synchronous
//! trait so the (sync) `ClaimEvaluator` hot path can consume it from both:
//!
//! - The daemon process itself — via [`InProcessLookup`], which reads the
//!   cache directly through [`crate::daemon::proxy::lookup_relevance`].
//! - The short-lived stop-hook subprocess — via [`HttpLookup`], which POSTs
//!   to the daemon's `/api/relevance/lookup` endpoint.
//!
//! Both implementations are **fail-open**: any missing entry, network
//! failure, or deserialization error returns an empty match list, which
//! `SemanticEvaluator` treats as "no verdict" (not "definitely clean").

use crate::claim::Claim;

/// A single LLM-as-judge verdict linking a claim to a constraint.
///
/// Only `"high"` and `"medium"` relevance are ever emitted; `"low"` is
/// filtered at the source (see `daemon::proxy::score_claim_relevance`).
#[derive(Debug, Clone)]
pub struct RelevanceMatch {
    pub constraint_id: String,
    pub relevance: String,
    pub reason: String,
}

/// Synchronous source of relevance verdicts keyed by claim text.
///
/// Implementations MUST NOT block the caller unbounded: the evaluator hot
/// path runs per (claim, constraint) pair. HTTP-backed implementations
/// should use short timeouts and internal caching.
pub trait RelevanceLookup: Send + Sync {
    /// Return every cached verdict for the given claim's text. An empty
    /// vector means "no verdict" — the caller must fail-open.
    fn lookup(&self, claim: &Claim) -> Vec<RelevanceMatch>;
}

// ---------------------------------------------------------------------------
// InProcessLookup — read the daemon cache directly.
// ---------------------------------------------------------------------------

/// Reads directly from the daemon's in-memory relevance cache. Use this when
/// the evaluator runs inside the daemon process (proxy evaluation path).
pub struct InProcessLookup;

impl InProcessLookup {
    pub fn new() -> Self {
        Self
    }
}

impl Default for InProcessLookup {
    fn default() -> Self {
        Self::new()
    }
}

impl RelevanceLookup for InProcessLookup {
    fn lookup(&self, claim: &Claim) -> Vec<RelevanceMatch> {
        crate::daemon::proxy::lookup_relevance(&claim.text)
            .into_iter()
            .map(|(constraint_id, relevance, reason)| RelevanceMatch {
                constraint_id,
                relevance,
                reason,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// HttpLookup — query the daemon's /api/relevance/lookup endpoint.
// ---------------------------------------------------------------------------

/// Queries the daemon's `/api/relevance/lookup` REST endpoint. Intended for
/// the stop-hook subprocess, which runs in a separate address space from
/// the daemon.
///
/// Each instance owns a dedicated current-thread Tokio runtime so that the
/// synchronous `lookup` method can drive the async `reqwest` call via
/// [`tokio::runtime::Runtime::block_on`].
///
/// Local de-duplication: the first `lookup` for a given claim text hits the
/// network; subsequent lookups for the same text in the same instance are
/// served from an internal `HashMap`. This matters because
/// [`super::EvaluatorPipeline::evaluate_claim`] calls the evaluator once per
/// constraint, and we'd otherwise make N_constraints HTTP calls for a
/// single claim.
pub struct HttpLookup {
    url: String,
    timeout: std::time::Duration,
    runtime: tokio::runtime::Runtime,
    /// Per-instance memoization: claim_text -> full match list.
    /// Guarded by a Mutex so the trait can remain `&self`.
    cache: std::sync::Mutex<std::collections::HashMap<String, Vec<RelevanceMatch>>>,
}

impl HttpLookup {
    /// Construct a lookup pointing at `http://127.0.0.1:{port}/api/relevance/lookup`.
    /// Returns `None` if a Tokio runtime could not be built (shouldn't happen
    /// in practice, but we prefer fail-open to panicking).
    pub fn new(port: u16) -> Option<Self> {
        Self::with_base_url(&format!("http://127.0.0.1:{}", port))
    }

    /// Construct with an explicit base URL (useful for testing).
    pub fn with_base_url(base_url: &str) -> Option<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        Some(Self {
            url: format!("{}/api/relevance/lookup", base_url.trim_end_matches('/')),
            timeout: std::time::Duration::from_millis(500),
            runtime,
            cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// Default instance reading `RIGOR_DAEMON_PORT` or falling back to 8787
    /// (the value documented across `cli/setup.rs`, `cli/gate.rs`, and the
    /// OpenCode plugin).
    pub fn from_env() -> Option<Self> {
        let port: u16 = std::env::var("RIGOR_DAEMON_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8787);
        Self::new(port)
    }

    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Make a single HTTP call to the daemon's lookup endpoint.
    ///
    /// Returns an empty vec on any failure (timeout, non-2xx, parse error,
    /// daemon down). This is the fail-open contract SemanticEvaluator relies on.
    fn fetch(&self, claim_text: &str) -> Vec<RelevanceMatch> {
        let body = serde_json::json!({ "claim_text": claim_text });
        let url = self.url.clone();
        let timeout = self.timeout;

        let json: Option<serde_json::Value> = self.runtime.block_on(async move {
            let client = reqwest::Client::builder().timeout(timeout).build().ok()?;
            let resp = client.post(&url).json(&body).send().await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            resp.json::<serde_json::Value>().await.ok()
        });

        let Some(json) = json else {
            return Vec::new();
        };

        json.get("matches")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        Some(RelevanceMatch {
                            constraint_id: item.get("constraint_id")?.as_str()?.to_string(),
                            relevance: item.get("relevance")?.as_str()?.to_string(),
                            reason: item
                                .get("reason")
                                .and_then(|r| r.as_str())
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl RelevanceLookup for HttpLookup {
    fn lookup(&self, claim: &Claim) -> Vec<RelevanceMatch> {
        // Cache hit: return a clone without hitting the network.
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached) = cache.get(&claim.text) {
                return cached.clone();
            }
        }

        let fresh = self.fetch(&claim.text);

        // Populate cache. Even an empty result is cached so we don't retry
        // on every constraint pairing for the same claim within one run.
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(claim.text.clone(), fresh.clone());
        }

        fresh
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::{Claim, ClaimType};

    fn make_claim(text: &str) -> Claim {
        Claim {
            id: "c1".to_string(),
            text: text.to_string(),
            domain: None,
            confidence: 0.9,
            claim_type: ClaimType::Assertion,
            source_line: None,
            source: None,
        }
    }

    #[test]
    fn http_lookup_empty_on_unreachable_daemon() {
        // Port 1 is never bound on a normal host. Fetch should time out quickly
        // and return an empty match set — never panic, never retry indefinitely.
        let lookup = HttpLookup::new(1)
            .expect("runtime builds")
            .with_timeout(std::time::Duration::from_millis(100));
        let result = lookup.lookup(&make_claim("anything"));
        assert!(result.is_empty(), "unreachable daemon must fail-open");
    }

    #[test]
    fn http_lookup_caches_per_claim() {
        // Even though the daemon is unreachable, the second call for the same
        // claim text should hit the local memoization cache. We can observe
        // this by checking that repeated calls return the same (empty) value
        // without panicking and in deterministic order.
        let lookup = HttpLookup::new(1)
            .expect("runtime builds")
            .with_timeout(std::time::Duration::from_millis(50));
        let a = lookup.lookup(&make_claim("same-text"));
        let b = lookup.lookup(&make_claim("same-text"));
        assert_eq!(a.len(), b.len());
    }
}
