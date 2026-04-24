//! Hash-keyed content-addressable store for Phase 0E.
//!
//! Backs four distinct use cases via the [`Category`] partition:
//!
//! | Category | TTL | Used by |
//! |----------|-----|---------|
//! | `Audit` | permanent | Phase 1 request/response audit trail |
//! | `Compression` | 5 min | Phase 1 CCR compressor originals |
//! | `Verdict` | 24 h | Phase 2E persistent evaluator verdict cache |
//! | `Annotation` | permanent | Phase 3 GEPA annotation corpus |
//!
//! The [`ContentStoreBackend`] trait is the abstraction boundary. Phase 0I's
//! Postgres impl (deferred to Phase 4D) slots in via a second implementor
//! without touching call sites.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use moka::sync::Cache;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// =============================================================================
// Types
// =============================================================================

/// SHA-256 content address.
pub type Hash = [u8; 32];

/// Compute the content address of `bytes`.
pub fn hash_bytes(bytes: &[u8]) -> Hash {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Hex-encode a hash for display / logging.
pub fn hash_hex(h: &Hash) -> String {
    let mut s = String::with_capacity(64);
    for b in h {
        use std::fmt::Write;
        write!(&mut s, "{:02x}", b).expect("write to String is infallible");
    }
    s
}

/// Storage partition. TTL and retention semantics differ per category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Permanent audit trail of every proxy request/response.
    Audit,
    /// CCR compression originals — 5-minute default TTL.
    Compression,
    /// Evaluator verdicts cached by claim-constraint pair — 24-hour TTL.
    Verdict,
    /// GEPA annotation corpus — permanent.
    Annotation,
}

/// One entry in the content store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredContent {
    pub bytes: Vec<u8>,
    pub category: Category,
    pub stored_at: DateTime<Utc>,
    /// Optional correlation key for Phase 1D TOIN — ties a stored entry to
    /// the tool-signature hash it was produced for.
    #[serde(default)]
    pub tool_signature_hash: Option<String>,
}

/// One hit from [`ContentStoreBackend::search`].
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub hash: Hash,
    pub score: f64,
    pub fragment: String,
}

// =============================================================================
// Trait
// =============================================================================

/// Pluggable backend for the content store. In-memory (Phase 0) and Postgres
/// (Phase 4D) will both implement this trait.
#[async_trait]
pub trait ContentStoreBackend: Send + Sync {
    /// Store `bytes` under `category`. Returns the SHA-256 content address.
    ///
    /// `ttl_override` lets the caller shorten/extend the category default;
    /// `None` uses the category default (permanent for Audit/Annotation,
    /// 5min/24h for Compression/Verdict).
    async fn store(
        &self,
        bytes: Vec<u8>,
        category: Category,
        ttl_override: Option<std::time::Duration>,
        tool_signature_hash: Option<String>,
    ) -> anyhow::Result<Hash>;

    /// Retrieve by content address across all categories.
    async fn retrieve(&self, hash: &Hash) -> anyhow::Result<Option<StoredContent>>;

    /// Search stored content by query. `category` filters to a single
    /// partition when `Some`. Placeholder substring scoring for Phase 0;
    /// Phase 1B upgrades to BM25.
    async fn search(
        &self,
        query: &str,
        category: Option<Category>,
    ) -> anyhow::Result<Vec<SearchResult>>;

    /// List every hash currently stored in `category`.
    async fn list_by_category(&self, category: Category) -> anyhow::Result<Vec<Hash>>;
}

// =============================================================================
// InMemoryBackend
// =============================================================================

/// Default in-memory backend.
///
/// - Audit and Annotation use `DashMap` (permanent).
/// - Compression and Verdict use `moka::sync::Cache` with TTL eviction.
pub struct InMemoryBackend {
    audit: Arc<DashMap<Hash, StoredContent>>,
    annotation: Arc<DashMap<Hash, StoredContent>>,
    compression: Cache<Hash, StoredContent>,
    verdict: Cache<Hash, StoredContent>,
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self {
            audit: Arc::new(DashMap::new()),
            annotation: Arc::new(DashMap::new()),
            compression: Cache::builder()
                .time_to_live(std::time::Duration::from_secs(5 * 60))
                .max_capacity(10_000)
                .build(),
            verdict: Cache::builder()
                .time_to_live(std::time::Duration::from_secs(24 * 60 * 60))
                .max_capacity(100_000)
                .build(),
        }
    }
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test-only builder that lets callers shorten TTLs for eviction tests.
    #[cfg(test)]
    fn with_ttls(compression: std::time::Duration, verdict: std::time::Duration) -> Self {
        Self {
            audit: Arc::new(DashMap::new()),
            annotation: Arc::new(DashMap::new()),
            compression: Cache::builder()
                .time_to_live(compression)
                .max_capacity(10_000)
                .build(),
            verdict: Cache::builder()
                .time_to_live(verdict)
                .max_capacity(100_000)
                .build(),
        }
    }

    fn score_bytes_against_query(bytes: &[u8], query_tokens: &[&str]) -> Option<SearchResult> {
        let text = std::str::from_utf8(bytes).ok()?;
        let text_lower = text.to_lowercase();
        let matched: usize = query_tokens
            .iter()
            .filter(|t| text_lower.contains(**t))
            .count();
        if matched == 0 {
            return None;
        }
        let score = matched as f64 / query_tokens.len() as f64;
        let fragment_end = text.len().min(200);
        Some(SearchResult {
            hash: hash_bytes(bytes),
            score,
            fragment: text[..fragment_end].to_string(),
        })
    }
}

#[async_trait]
impl ContentStoreBackend for InMemoryBackend {
    async fn store(
        &self,
        bytes: Vec<u8>,
        category: Category,
        _ttl_override: Option<std::time::Duration>,
        tool_signature_hash: Option<String>,
    ) -> anyhow::Result<Hash> {
        let hash = hash_bytes(&bytes);
        let content = StoredContent {
            bytes,
            category,
            stored_at: Utc::now(),
            tool_signature_hash,
        };
        match category {
            Category::Audit => {
                self.audit.insert(hash, content);
            }
            Category::Annotation => {
                self.annotation.insert(hash, content);
            }
            Category::Compression => {
                self.compression.insert(hash, content);
            }
            Category::Verdict => {
                self.verdict.insert(hash, content);
            }
        }
        Ok(hash)
    }

    async fn retrieve(&self, hash: &Hash) -> anyhow::Result<Option<StoredContent>> {
        if let Some(c) = self.audit.get(hash) {
            return Ok(Some(c.clone()));
        }
        if let Some(c) = self.annotation.get(hash) {
            return Ok(Some(c.clone()));
        }
        if let Some(c) = self.compression.get(hash) {
            return Ok(Some(c));
        }
        if let Some(c) = self.verdict.get(hash) {
            return Ok(Some(c));
        }
        Ok(None)
    }

    async fn search(
        &self,
        query: &str,
        category: Option<Category>,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let query_lower = query.to_lowercase();
        let tokens: Vec<&str> = query_lower.split_whitespace().collect();
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();

        let want = |c: Category| category.is_none_or(|filter| filter == c);

        if want(Category::Audit) {
            for entry in self.audit.iter() {
                if let Some(r) = Self::score_bytes_against_query(&entry.value().bytes, &tokens) {
                    results.push(r);
                }
            }
        }
        if want(Category::Annotation) {
            for entry in self.annotation.iter() {
                if let Some(r) = Self::score_bytes_against_query(&entry.value().bytes, &tokens) {
                    results.push(r);
                }
            }
        }
        if want(Category::Compression) {
            for (_hash, content) in &self.compression {
                if let Some(r) = Self::score_bytes_against_query(&content.bytes, &tokens) {
                    results.push(r);
                }
            }
        }
        if want(Category::Verdict) {
            for (_hash, content) in &self.verdict {
                if let Some(r) = Self::score_bytes_against_query(&content.bytes, &tokens) {
                    results.push(r);
                }
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    async fn list_by_category(&self, category: Category) -> anyhow::Result<Vec<Hash>> {
        let hashes = match category {
            Category::Audit => self.audit.iter().map(|e| *e.key()).collect(),
            Category::Annotation => self.annotation.iter().map(|e| *e.key()).collect(),
            Category::Compression => self.compression.iter().map(|(k, _)| *k).collect(),
            Category::Verdict => self.verdict.iter().map(|(k, _)| *k).collect(),
        };
        Ok(hashes)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        let h1 = hash_bytes(b"hello");
        let h2 = hash_bytes(b"hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_inputs_produce_different_hashes() {
        assert_ne!(hash_bytes(b"hello"), hash_bytes(b"world"));
    }

    #[test]
    fn hash_hex_is_64_chars() {
        let hex = hash_hex(&hash_bytes(b"hello"));
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn category_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&Category::Compression).unwrap(),
            "\"compression\""
        );
    }

    #[tokio::test]
    async fn store_then_retrieve_roundtrip() {
        let store = InMemoryBackend::new();
        let payload = b"the quick brown fox".to_vec();
        let hash = store
            .store(payload.clone(), Category::Audit, None, None)
            .await
            .unwrap();
        let got = store.retrieve(&hash).await.unwrap().unwrap();
        assert_eq!(got.bytes, payload);
        assert_eq!(got.category, Category::Audit);
    }

    #[tokio::test]
    async fn retrieve_missing_returns_none() {
        let store = InMemoryBackend::new();
        let fake_hash = hash_bytes(b"never stored");
        assert!(store.retrieve(&fake_hash).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_by_category_isolates_partitions() {
        let store = InMemoryBackend::new();
        store
            .store(b"audit 1".to_vec(), Category::Audit, None, None)
            .await
            .unwrap();
        store
            .store(b"audit 2".to_vec(), Category::Audit, None, None)
            .await
            .unwrap();
        store
            .store(b"annotation 1".to_vec(), Category::Annotation, None, None)
            .await
            .unwrap();

        let audit = store.list_by_category(Category::Audit).await.unwrap();
        let annotation = store.list_by_category(Category::Annotation).await.unwrap();
        let verdict = store.list_by_category(Category::Verdict).await.unwrap();
        assert_eq!(audit.len(), 2);
        assert_eq!(annotation.len(), 1);
        assert_eq!(verdict.len(), 0);
    }

    #[tokio::test]
    async fn same_bytes_different_categories_have_same_hash_but_separate_entries() {
        let store = InMemoryBackend::new();
        let payload = b"shared".to_vec();
        let h1 = store
            .store(payload.clone(), Category::Audit, None, None)
            .await
            .unwrap();
        let h2 = store
            .store(payload.clone(), Category::Annotation, None, None)
            .await
            .unwrap();
        assert_eq!(
            h1, h2,
            "hash is content-addressable, not category-qualified"
        );
        assert_eq!(
            store.list_by_category(Category::Audit).await.unwrap().len(),
            1
        );
        assert_eq!(
            store
                .list_by_category(Category::Annotation)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn tool_signature_hash_round_trips() {
        let store = InMemoryBackend::new();
        let hash = store
            .store(
                b"payload".to_vec(),
                Category::Audit,
                None,
                Some("toolsig-abc".into()),
            )
            .await
            .unwrap();
        let got = store.retrieve(&hash).await.unwrap().unwrap();
        assert_eq!(got.tool_signature_hash.as_deref(), Some("toolsig-abc"));
    }

    #[tokio::test]
    async fn compression_ttl_evicts_after_deadline() {
        // Deliberately short TTL so the test completes quickly.
        let store = InMemoryBackend::with_ttls(
            std::time::Duration::from_millis(50),
            std::time::Duration::from_secs(60),
        );
        let hash = store
            .store(b"short-lived".to_vec(), Category::Compression, None, None)
            .await
            .unwrap();
        assert!(store.retrieve(&hash).await.unwrap().is_some());

        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        // moka evicts on access; retrieve returns None once TTL has elapsed.
        assert!(store.retrieve(&hash).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn audit_does_not_evict() {
        // Short TTL on compression proves TTL works; audit uses dashmap,
        // so it should never evict even when we wait.
        let store = InMemoryBackend::with_ttls(
            std::time::Duration::from_millis(50),
            std::time::Duration::from_millis(50),
        );
        let hash = store
            .store(b"permanent".to_vec(), Category::Audit, None, None)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        assert!(store.retrieve(&hash).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn search_substring_matches_with_score() {
        let store = InMemoryBackend::new();
        store
            .store(
                b"fabricated function foo bar".to_vec(),
                Category::Audit,
                None,
                None,
            )
            .await
            .unwrap();
        store
            .store(
                b"completely unrelated content".to_vec(),
                Category::Audit,
                None,
                None,
            )
            .await
            .unwrap();

        let hits = store.search("fabricated function", None).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].score > 0.0);
        assert!(hits[0].fragment.contains("fabricated"));
    }

    #[tokio::test]
    async fn search_category_filter_excludes_others() {
        let store = InMemoryBackend::new();
        store
            .store(b"audit payload".to_vec(), Category::Audit, None, None)
            .await
            .unwrap();
        store
            .store(
                b"annotation payload".to_vec(),
                Category::Annotation,
                None,
                None,
            )
            .await
            .unwrap();

        let hits = store
            .search("payload", Some(Category::Audit))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].fragment.contains("audit"));
    }

    #[tokio::test]
    async fn search_empty_query_returns_empty() {
        let store = InMemoryBackend::new();
        store
            .store(b"anything".to_vec(), Category::Audit, None, None)
            .await
            .unwrap();
        let hits = store.search("   ", None).await.unwrap();
        assert!(hits.is_empty());
    }

    // ── Gap 9: Verdict TTL eviction ─────────────────────────────────────

    #[tokio::test]
    async fn verdict_ttl_evicts_after_deadline() {
        // Short verdict TTL, long compression TTL — mirrors compression_ttl_evicts_after_deadline.
        let store = InMemoryBackend::with_ttls(
            std::time::Duration::from_secs(60),
            std::time::Duration::from_millis(50),
        );
        let hash = store
            .store(b"verdict-short-lived".to_vec(), Category::Verdict, None, None)
            .await
            .unwrap();
        assert!(
            store.retrieve(&hash).await.unwrap().is_some(),
            "verdict should be retrievable immediately after store"
        );

        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        // moka evicts lazily on access; retrieve returns None once TTL has elapsed.
        assert!(
            store.retrieve(&hash).await.unwrap().is_none(),
            "verdict should be evicted after TTL deadline"
        );
    }

    #[tokio::test]
    async fn annotation_does_not_evict() {
        // Short TTLs on both caches; annotation uses DashMap (permanent), so
        // it should survive past the TTL window.
        let store = InMemoryBackend::with_ttls(
            std::time::Duration::from_millis(50),
            std::time::Duration::from_millis(50),
        );
        let hash = store
            .store(b"permanent-annotation".to_vec(), Category::Annotation, None, None)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        assert!(
            store.retrieve(&hash).await.unwrap().is_some(),
            "annotation (DashMap-backed) should never be evicted by TTL"
        );
    }

    // ── Gap 9: Concurrency tests ────────────────────────────────────────

    #[tokio::test]
    async fn concurrent_stores_no_corruption() {
        let store = Arc::new(InMemoryBackend::new());
        let barrier = Arc::new(tokio::sync::Barrier::new(10));
        let mut handles = Vec::new();

        for i in 0..10 {
            let s = Arc::clone(&store);
            let b = Arc::clone(&barrier);
            handles.push(tokio::spawn(async move {
                b.wait().await;
                let payload = format!("payload-{}", i).into_bytes();
                s.store(payload, Category::Audit, None, None).await.unwrap()
            }));
        }

        let mut hashes = Vec::new();
        for h in handles {
            hashes.push(h.await.unwrap());
        }

        // All 10 entries should exist.
        let listed = store.list_by_category(Category::Audit).await.unwrap();
        assert_eq!(listed.len(), 10, "expected 10 audit entries from concurrent stores");

        // Each hash should retrieve its original payload.
        for (i, hash) in hashes.iter().enumerate() {
            let entry = store.retrieve(hash).await.unwrap().unwrap();
            assert_eq!(
                entry.bytes,
                format!("payload-{}", i).into_bytes(),
                "payload mismatch for concurrent store #{i}"
            );
        }
    }

    #[tokio::test]
    async fn concurrent_retrieve_during_ttl_window() {
        // Short compression TTL; verdict is long so it does not interfere.
        let store = Arc::new(InMemoryBackend::with_ttls(
            std::time::Duration::from_millis(100),
            std::time::Duration::from_secs(60),
        ));
        let hash = store
            .store(b"ttl-race-entry".to_vec(), Category::Compression, None, None)
            .await
            .unwrap();

        // Spawn 5 reader tasks that all retrieve before TTL expires.
        let mut handles = Vec::new();
        for _ in 0..5 {
            let s = Arc::clone(&store);
            let h = hash;
            handles.push(tokio::spawn(async move {
                s.retrieve(&h).await.unwrap()
            }));
        }

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(
                result.is_some(),
                "concurrent readers should all see the entry before TTL expires"
            );
        }

        // Wait past TTL.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            store.retrieve(&hash).await.unwrap().is_none(),
            "entry should be evicted after TTL, even after concurrent reads"
        );
    }
}
