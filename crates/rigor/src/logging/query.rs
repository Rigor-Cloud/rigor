use crate::logging::types::ViolationLogEntry;
use std::collections::HashMap;

/// Filter to the last N entries, most recent first.
pub fn filter_last(entries: &[ViolationLogEntry], count: usize) -> Vec<&ViolationLogEntry> {
    let start = if entries.len() > count {
        entries.len() - count
    } else {
        0
    };

    entries[start..].iter().rev().collect()
}

/// Filter entries by constraint_id.
pub fn filter_by_constraint<'a>(
    entries: &'a [ViolationLogEntry],
    constraint_id: &str,
) -> Vec<&'a ViolationLogEntry> {
    entries
        .iter()
        .filter(|e| e.constraint_id == constraint_id)
        .collect()
}

/// Filter entries by session_id.
pub fn filter_by_session<'a>(
    entries: &'a [ViolationLogEntry],
    session_id: &str,
) -> Vec<&'a ViolationLogEntry> {
    entries
        .iter()
        .filter(|e| e.session.session_id == session_id)
        .collect()
}

/// Aggregated statistics from violation log.
#[derive(Debug, Clone)]
pub struct LogStats {
    /// Total number of violations
    pub total_violations: usize,
    /// Count of violations per constraint
    pub unique_constraints: HashMap<String, usize>,
    /// Number of unique sessions
    pub unique_sessions: usize,
    /// Count per severity level
    pub severity_counts: SeverityCounts,
    /// First violation timestamp
    pub first_timestamp: Option<String>,
    /// Last violation timestamp
    pub last_timestamp: Option<String>,
    /// Number of false positives
    pub false_positive_count: usize,
}

/// Count per severity level.
#[derive(Debug, Clone, Default)]
pub struct SeverityCounts {
    pub block: usize,
    pub warn: usize,
    pub allow: usize,
}

/// Compute statistics from violation entries.
pub fn compute_stats(entries: &[ViolationLogEntry]) -> LogStats {
    if entries.is_empty() {
        return LogStats {
            total_violations: 0,
            unique_constraints: HashMap::new(),
            unique_sessions: 0,
            severity_counts: SeverityCounts::default(),
            first_timestamp: None,
            last_timestamp: None,
            false_positive_count: 0,
        };
    }

    let mut constraint_counts: HashMap<String, usize> = HashMap::new();
    let mut session_ids = std::collections::HashSet::new();
    let mut severity_counts = SeverityCounts::default();
    let mut false_positive_count = 0;

    for entry in entries {
        // Count by constraint
        *constraint_counts
            .entry(entry.constraint_id.clone())
            .or_insert(0) += 1;

        // Track unique sessions
        session_ids.insert(entry.session.session_id.clone());

        // Count by severity
        match entry.severity.as_str() {
            "block" => severity_counts.block += 1,
            "warn" => severity_counts.warn += 1,
            "allow" => severity_counts.allow += 1,
            _ => {} // Unknown severity
        }

        // Count false positives
        if entry.false_positive == Some(true) {
            false_positive_count += 1;
        }
    }

    LogStats {
        total_violations: entries.len(),
        unique_constraints: constraint_counts,
        unique_sessions: session_ids.len(),
        severity_counts,
        first_timestamp: entries.first().map(|e| e.session.timestamp.clone()),
        last_timestamp: entries.last().map(|e| e.session.timestamp.clone()),
        false_positive_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::types::SessionMetadata;

    fn create_test_entry(
        constraint_id: &str,
        session_id: &str,
        timestamp: &str,
        severity: &str,
        false_positive: Option<bool>,
    ) -> ViolationLogEntry {
        ViolationLogEntry {
            session: SessionMetadata {
                session_id: session_id.to_string(),
                timestamp: timestamp.to_string(),
                git_commit: Some("abc12345".to_string()),
                git_dirty: false,
            },
            constraint_id: constraint_id.to_string(),
            constraint_name: format!("constraint_{}", constraint_id),
            claim_ids: vec![],
            claim_text: vec![],
            base_strength: 0.8,
            computed_strength: 0.75,
            severity: severity.to_string(),
            decision: "block".to_string(),
            message: "Test violation".to_string(),
            supporters: vec![],
            attackers: vec![],
            total_claims: 1,
            total_constraints: 1,
            transcript_path: None,
            claim_confidence: None,
            claim_type: None,
            claim_source: None,
            false_positive,
            annotation_note: None,
        }
    }

    #[test]
    fn test_filter_last() {
        let entries = vec![
            create_test_entry("c1", "s1", "2026-01-29T01:00:00Z", "block", None),
            create_test_entry("c2", "s1", "2026-01-29T02:00:00Z", "warn", None),
            create_test_entry("c3", "s2", "2026-01-29T03:00:00Z", "block", None),
            create_test_entry("c4", "s2", "2026-01-29T04:00:00Z", "allow", None),
        ];

        let result = filter_last(&entries, 2);
        assert_eq!(result.len(), 2);
        // Should be most recent first
        assert_eq!(result[0].constraint_id, "c4");
        assert_eq!(result[1].constraint_id, "c3");
    }

    #[test]
    fn test_filter_last_more_than_available() {
        let entries = vec![
            create_test_entry("c1", "s1", "2026-01-29T01:00:00Z", "block", None),
            create_test_entry("c2", "s1", "2026-01-29T02:00:00Z", "warn", None),
        ];

        let result = filter_last(&entries, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].constraint_id, "c2");
        assert_eq!(result[1].constraint_id, "c1");
    }

    #[test]
    fn test_filter_by_constraint() {
        let entries = vec![
            create_test_entry("c1", "s1", "2026-01-29T01:00:00Z", "block", None),
            create_test_entry("c2", "s1", "2026-01-29T02:00:00Z", "warn", None),
            create_test_entry("c1", "s2", "2026-01-29T03:00:00Z", "block", None),
        ];

        let result = filter_by_constraint(&entries, "c1");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].constraint_id, "c1");
        assert_eq!(result[1].constraint_id, "c1");
    }

    #[test]
    fn test_filter_by_session() {
        let entries = vec![
            create_test_entry("c1", "s1", "2026-01-29T01:00:00Z", "block", None),
            create_test_entry("c2", "s1", "2026-01-29T02:00:00Z", "warn", None),
            create_test_entry("c3", "s2", "2026-01-29T03:00:00Z", "block", None),
        ];

        let result = filter_by_session(&entries, "s1");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].session.session_id, "s1");
        assert_eq!(result[1].session.session_id, "s1");
    }

    #[test]
    fn test_compute_stats() {
        let entries = vec![
            create_test_entry("c1", "s1", "2026-01-29T01:00:00Z", "block", None),
            create_test_entry("c2", "s1", "2026-01-29T02:00:00Z", "warn", None),
            create_test_entry("c1", "s2", "2026-01-29T03:00:00Z", "block", Some(true)),
            create_test_entry("c3", "s2", "2026-01-29T04:00:00Z", "allow", None),
        ];

        let stats = compute_stats(&entries);
        assert_eq!(stats.total_violations, 4);
        assert_eq!(stats.unique_constraints.len(), 3);
        assert_eq!(*stats.unique_constraints.get("c1").unwrap(), 2);
        assert_eq!(*stats.unique_constraints.get("c2").unwrap(), 1);
        assert_eq!(stats.unique_sessions, 2);
        assert_eq!(stats.severity_counts.block, 2);
        assert_eq!(stats.severity_counts.warn, 1);
        assert_eq!(stats.severity_counts.allow, 1);
        assert_eq!(
            stats.first_timestamp,
            Some("2026-01-29T01:00:00Z".to_string())
        );
        assert_eq!(
            stats.last_timestamp,
            Some("2026-01-29T04:00:00Z".to_string())
        );
        assert_eq!(stats.false_positive_count, 1);
    }

    #[test]
    fn test_compute_stats_empty() {
        let entries: Vec<ViolationLogEntry> = vec![];
        let stats = compute_stats(&entries);
        assert_eq!(stats.total_violations, 0);
        assert_eq!(stats.unique_constraints.len(), 0);
        assert_eq!(stats.unique_sessions, 0);
        assert_eq!(stats.false_positive_count, 0);
        assert!(stats.first_timestamp.is_none());
        assert!(stats.last_timestamp.is_none());
    }
}
