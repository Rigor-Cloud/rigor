use crate::logging::types::ViolationLogEntry;
use crate::logging::violation_log::ViolationLogger;
use anyhow::{bail, Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;

/// Annotate an entry in the violation log.
///
/// The index is 1-based (as displayed in `rigor log last`).
pub fn annotate_entry(
    entries: &mut [ViolationLogEntry],
    index: usize,
    false_positive: bool,
    note: Option<String>,
) -> Result<()> {
    if index == 0 {
        bail!("Index must be 1-based (1, 2, 3, ...), not 0");
    }

    let array_index = index - 1;
    if array_index >= entries.len() {
        bail!(
            "Index {} out of range (log has {} entries)",
            index,
            entries.len()
        );
    }

    let entry = &mut entries[array_index];
    entry.false_positive = Some(false_positive);
    entry.annotation_note = note;

    Ok(())
}

/// Rewrite the violation log atomically with updated entries.
///
/// This writes to a temporary file and then renames it to the actual log path.
pub fn rewrite_log(logger: &ViolationLogger, entries: &[ViolationLogEntry]) -> Result<()> {
    let log_path = logger.log_path();
    let temp_path = log_path.with_extension("jsonl.tmp");

    // Write to temporary file
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)
            .context("Failed to create temporary log file")?;

        for entry in entries {
            let json =
                serde_json::to_string(entry).context("Failed to serialize ViolationLogEntry")?;
            writeln!(file, "{}", json).context("Failed to write to temporary log file")?;
        }

        file.sync_all()
            .context("Failed to sync temporary log file to disk")?;
    }

    // Atomically replace the original file
    fs::rename(&temp_path, log_path).context("Failed to replace log file with updated version")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::types::SessionMetadata;
    use tempfile::TempDir;

    fn create_test_entry(constraint_id: &str) -> ViolationLogEntry {
        ViolationLogEntry {
            session: SessionMetadata {
                session_id: "test-session".to_string(),
                timestamp: "2026-01-29T06:00:00Z".to_string(),
                git_commit: Some("abc12345".to_string()),
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
            message: "Test violation".to_string(),
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
        }
    }

    #[test]
    fn test_annotate_entry() {
        let mut entries = vec![
            create_test_entry("c1"),
            create_test_entry("c2"),
            create_test_entry("c3"),
        ];

        // Annotate the second entry (1-based index = 2)
        annotate_entry(
            &mut entries,
            2,
            true,
            Some("This was a false positive".to_string()),
        )
        .unwrap();

        assert_eq!(entries[1].false_positive, Some(true));
        assert_eq!(
            entries[1].annotation_note,
            Some("This was a false positive".to_string())
        );

        // First and third entries should be unchanged
        assert_eq!(entries[0].false_positive, None);
        assert_eq!(entries[2].false_positive, None);
    }

    #[test]
    fn test_annotate_entry_out_of_range() {
        let mut entries = vec![create_test_entry("c1")];

        let result = annotate_entry(&mut entries, 5, true, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of range"));
    }

    #[test]
    fn test_annotate_entry_zero_index() {
        let mut entries = vec![create_test_entry("c1")];

        let result = annotate_entry(&mut entries, 0, true, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("1-based"));
    }

    #[test]
    fn test_annotate_round_trip() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("violations.jsonl");
        let logger = ViolationLogger::with_path(log_path);

        // Write initial entries
        let entry1 = create_test_entry("c1");
        let entry2 = create_test_entry("c2");
        logger.log(&entry1).unwrap();
        logger.log(&entry2).unwrap();

        // Read them back
        let mut entries = logger.read_all().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].false_positive, None);

        // Annotate the first entry
        annotate_entry(&mut entries, 1, true, Some("False alarm".to_string())).unwrap();

        // Rewrite the log
        rewrite_log(&logger, &entries).unwrap();

        // Read again and verify annotation persists
        let entries_after = logger.read_all().unwrap();
        assert_eq!(entries_after.len(), 2);
        assert_eq!(entries_after[0].false_positive, Some(true));
        assert_eq!(
            entries_after[0].annotation_note,
            Some("False alarm".to_string())
        );
        assert_eq!(entries_after[1].false_positive, None);
    }
}
