//! Pluggable backend trait for the violation log (Phase 0I).
//!
//! The existing [`ViolationLogger`] ships with a JSONL file backend at
//! `~/.rigor/violations.jsonl`. Phase 4D adds a Postgres backend behind the
//! same trait so call sites switch storage by configuration, not by code
//! change. Today both impls are sync; the trait is async so the future
//! Postgres path (sqlx) drops in without signature churn.
//!
//! The trait is wider than the current call sites use — `annotate` and
//! `rewrite` mirror helpers in [`crate::logging::annotate`] so every
//! backend can implement them consistently. JSONL's `rewrite` is an atomic
//! tempfile-rename; Postgres's will be an `UPDATE`.
//!
//! No existing call sites are changed in this phase — the trait just
//! becomes available for future phases to consume.

use anyhow::Result;
use async_trait::async_trait;

use super::annotate::{annotate_entry, rewrite_log};
use super::query as log_query;
use super::types::ViolationLogEntry;
use super::violation_log::ViolationLogger;

// =============================================================================
// Query shape
// =============================================================================

/// Common query filters for [`ViolationLogBackend::query`]. Missing fields are
/// treated as "no filter". Filters combine with AND semantics.
#[derive(Debug, Default, Clone)]
pub struct LogQuery {
    pub constraint_id: Option<String>,
    pub session_id: Option<String>,
    /// When set, return at most this many most-recent entries (reverse chrono).
    pub last_n: Option<usize>,
}

// =============================================================================
// Trait
// =============================================================================

/// Pluggable backend for the rigor violation log.
#[async_trait]
pub trait ViolationLogBackend: Send + Sync {
    /// Append a single entry.
    async fn append(&self, entry: &ViolationLogEntry) -> Result<()>;

    /// Read every entry currently stored, oldest first.
    async fn read_all(&self) -> Result<Vec<ViolationLogEntry>>;

    /// Run a filtered query.
    async fn query(&self, filter: LogQuery) -> Result<Vec<ViolationLogEntry>>;

    /// Mark entry at 1-based `index` as false-positive / not, with optional
    /// human note. Mirrors [`crate::logging::annotate::annotate_entry`].
    async fn annotate(
        &self,
        index: usize,
        false_positive: bool,
        note: Option<String>,
    ) -> Result<()>;

    /// Atomically replace the full log with `entries`. Used by annotation
    /// rewrites and future bulk-corrections from Phase 3C review UI.
    async fn rewrite(&self, entries: &[ViolationLogEntry]) -> Result<()>;
}

// =============================================================================
// JSONL implementation on the existing ViolationLogger
// =============================================================================

#[async_trait]
impl ViolationLogBackend for ViolationLogger {
    async fn append(&self, entry: &ViolationLogEntry) -> Result<()> {
        self.log(entry)
    }

    async fn read_all(&self) -> Result<Vec<ViolationLogEntry>> {
        self.read_all()
    }

    async fn query(&self, filter: LogQuery) -> Result<Vec<ViolationLogEntry>> {
        let mut entries = self.read_all()?;

        if let Some(ref cid) = filter.constraint_id {
            entries.retain(|e| &e.constraint_id == cid);
        }
        if let Some(ref sid) = filter.session_id {
            entries.retain(|e| &e.session.session_id == sid);
        }
        if let Some(n) = filter.last_n {
            // filter_last yields references — materialize to owned.
            let owned: Vec<ViolationLogEntry> = log_query::filter_last(&entries, n)
                .into_iter()
                .cloned()
                .collect();
            return Ok(owned);
        }

        Ok(entries)
    }

    async fn annotate(
        &self,
        index: usize,
        false_positive: bool,
        note: Option<String>,
    ) -> Result<()> {
        let mut entries = self.read_all()?;
        annotate_entry(&mut entries, index, false_positive, note)?;
        rewrite_log(self, &entries)?;
        Ok(())
    }

    async fn rewrite(&self, entries: &[ViolationLogEntry]) -> Result<()> {
        rewrite_log(self, entries)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::types::SessionMetadata;
    use tempfile::TempDir;

    fn make_entry(constraint_id: &str, session_id: &str) -> ViolationLogEntry {
        ViolationLogEntry {
            session: SessionMetadata {
                session_id: session_id.to_string(),
                timestamp: "2026-04-22T00:00:00Z".to_string(),
                git_commit: Some("abc123".to_string()),
                git_dirty: false,
            },
            constraint_id: constraint_id.to_string(),
            constraint_name: format!("constraint_{}", constraint_id),
            claim_ids: vec![],
            claim_text: vec![],
            base_strength: 0.8,
            computed_strength: 0.75,
            severity: "block".to_string(),
            decision: "block".to_string(),
            message: "Test".to_string(),
            supporters: vec![],
            attackers: vec![],
            total_claims: 1,
            total_constraints: 1,
            transcript_path: None,
            claim_confidence: None,
            claim_type: None,
            claim_source: None,
            false_positive: None,
            annotation_note: None,
            model: None,
        }
    }

    fn make_logger(tmp: &TempDir) -> ViolationLogger {
        ViolationLogger::with_path(tmp.path().join("violations.jsonl"))
    }

    #[tokio::test]
    async fn append_then_read_all_via_trait() {
        let tmp = TempDir::new().unwrap();
        let logger = make_logger(&tmp);

        let e1 = make_entry("c1", "s1");
        let e2 = make_entry("c2", "s1");
        logger.append(&e1).await.unwrap();
        logger.append(&e2).await.unwrap();

        let got = ViolationLogBackend::read_all(&logger).await.unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].constraint_id, "c1");
        assert_eq!(got[1].constraint_id, "c2");
    }

    #[tokio::test]
    async fn query_filter_by_constraint_id() {
        let tmp = TempDir::new().unwrap();
        let logger = make_logger(&tmp);
        logger.append(&make_entry("c1", "s1")).await.unwrap();
        logger.append(&make_entry("c2", "s1")).await.unwrap();
        logger.append(&make_entry("c1", "s2")).await.unwrap();

        let hits = logger
            .query(LogQuery {
                constraint_id: Some("c1".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|e| e.constraint_id == "c1"));
    }

    #[tokio::test]
    async fn query_filter_by_session_id() {
        let tmp = TempDir::new().unwrap();
        let logger = make_logger(&tmp);
        logger.append(&make_entry("c1", "s1")).await.unwrap();
        logger.append(&make_entry("c2", "s2")).await.unwrap();

        let hits = logger
            .query(LogQuery {
                session_id: Some("s2".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].constraint_id, "c2");
    }

    #[tokio::test]
    async fn query_last_n_returns_most_recent_reverse_chrono() {
        let tmp = TempDir::new().unwrap();
        let logger = make_logger(&tmp);
        for i in 0..5 {
            logger
                .append(&make_entry(&format!("c{i}"), "s1"))
                .await
                .unwrap();
        }

        let hits = logger
            .query(LogQuery {
                last_n: Some(2),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(hits.len(), 2);
        // filter_last reverses — most recent first.
        assert_eq!(hits[0].constraint_id, "c4");
        assert_eq!(hits[1].constraint_id, "c3");
    }

    #[tokio::test]
    async fn annotate_round_trips_through_trait() {
        let tmp = TempDir::new().unwrap();
        let logger = make_logger(&tmp);
        logger.append(&make_entry("c1", "s1")).await.unwrap();
        logger.append(&make_entry("c2", "s1")).await.unwrap();

        logger
            .annotate(1, true, Some("noisy".into()))
            .await
            .unwrap();

        let got = ViolationLogBackend::read_all(&logger).await.unwrap();
        assert_eq!(got[0].false_positive, Some(true));
        assert_eq!(got[0].annotation_note.as_deref(), Some("noisy"));
        assert_eq!(got[1].false_positive, None);
    }

    #[tokio::test]
    async fn rewrite_replaces_full_log() {
        let tmp = TempDir::new().unwrap();
        let logger = make_logger(&tmp);
        logger.append(&make_entry("c1", "s1")).await.unwrap();
        logger.append(&make_entry("c2", "s1")).await.unwrap();
        logger.append(&make_entry("c3", "s1")).await.unwrap();

        let replacement = vec![make_entry("only", "s1")];
        logger.rewrite(&replacement).await.unwrap();

        let got = ViolationLogBackend::read_all(&logger).await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].constraint_id, "only");
    }
}
