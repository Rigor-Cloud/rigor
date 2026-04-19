use crate::logging::types::ViolationLogEntry;
use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// ViolationLogger manages append-only JSONL logging to ~/.rigor/violations.jsonl
pub struct ViolationLogger {
    log_path: PathBuf,
}

impl ViolationLogger {
    /// Create a new ViolationLogger.
    ///
    /// This will:
    /// - Resolve the home directory
    /// - Create ~/.rigor/ if it doesn't exist
    /// - Set the log path to ~/.rigor/violations.jsonl
    /// Create a ViolationLogger with a custom log path (for testing).
    pub fn with_path(log_path: PathBuf) -> Self {
        Self { log_path }
    }

    pub fn new() -> Result<Self> {
        let home_dir = dirs::home_dir().context("Failed to get home directory")?;

        let rigor_dir = home_dir.join(".rigor");
        fs::create_dir_all(&rigor_dir).context("Failed to create ~/.rigor directory")?;

        let log_path = rigor_dir.join("violations.jsonl");

        Ok(Self { log_path })
    }

    /// Log a violation entry to the JSONL file.
    ///
    /// This appends a single JSON line to the log file.
    pub fn log(&self, entry: &ViolationLogEntry) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .context("Failed to open violations.jsonl for append")?;

        let json = serde_json::to_string(entry).context("Failed to serialize ViolationLogEntry")?;

        writeln!(file, "{}", json).context("Failed to write violation log entry")?;

        Ok(())
    }

    /// Read all violation entries from the log file.
    ///
    /// This function:
    /// - Returns an empty vector if the file doesn't exist
    /// - Parses each line as a ViolationLogEntry
    /// - Skips lines that fail to parse (forward compatibility)
    pub fn read_all(&self) -> Result<Vec<ViolationLogEntry>> {
        if !self.log_path.exists() {
            return Ok(Vec::new());
        }

        let file = OpenOptions::new()
            .read(true)
            .open(&self.log_path)
            .context("Failed to open violations.jsonl for reading")?;

        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line.context("Failed to read line from violations.jsonl")?;

            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            // Try to parse the line as a ViolationLogEntry
            match serde_json::from_str::<ViolationLogEntry>(&line) {
                Ok(entry) => entries.push(entry),
                Err(_) => {
                    // Skip malformed lines (forward compatibility)
                    continue;
                }
            }
        }

        Ok(entries)
    }

    /// Get the path to the log file (for CLI display).
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::types::SessionMetadata;
    use tempfile::TempDir;

    fn create_test_logger(temp_dir: &TempDir) -> ViolationLogger {
        let log_path = temp_dir.path().join("violations.jsonl");
        ViolationLogger { log_path }
    }

    fn create_test_entry() -> ViolationLogEntry {
        ViolationLogEntry {
            session: SessionMetadata {
                session_id: "test-session-123".to_string(),
                timestamp: "2026-01-29T06:00:00Z".to_string(),
                git_commit: Some("abc12345".to_string()),
                git_dirty: false,
            },
            constraint_id: "c1".to_string(),
            constraint_name: "no_hallucinations".to_string(),
            claim_ids: vec!["claim1".to_string()],
            claim_text: vec!["The sky is green".to_string()],
            base_strength: 0.8,
            computed_strength: 0.75,
            severity: "block".to_string(),
            decision: "block".to_string(),
            message: "Constraint violated".to_string(),
            supporters: vec![],
            attackers: vec!["c2".to_string()],
            total_claims: 5,
            total_constraints: 3,
            transcript_path: None,
            claim_confidence: None,
            claim_type: None,
            claim_source: None,
            false_positive: None,
            annotation_note: None,
            model: None,
        }
    }

    #[test]
    fn test_write_then_read_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let logger = create_test_logger(&temp_dir);

        // Write an entry
        let entry = create_test_entry();
        logger.log(&entry).unwrap();

        // Read it back
        let entries = logger.read_all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].constraint_id, "c1");
        assert_eq!(entries[0].constraint_name, "no_hallucinations");
        assert_eq!(entries[0].session.session_id, "test-session-123");
    }

    #[test]
    fn test_read_all_nonexistent_file_returns_empty() {
        let temp_dir = TempDir::new().unwrap();
        let logger = create_test_logger(&temp_dir);

        // File doesn't exist yet
        let entries = logger.read_all().unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_read_all_skips_malformed_lines() {
        let temp_dir = TempDir::new().unwrap();
        let logger = create_test_logger(&temp_dir);

        // Write a valid entry
        let entry = create_test_entry();
        logger.log(&entry).unwrap();

        // Manually append a malformed line
        let mut file = OpenOptions::new()
            .append(true)
            .open(logger.log_path())
            .unwrap();
        writeln!(file, "{{\"invalid\": \"json\"}}").unwrap();

        // Write another valid entry
        let entry2 = ViolationLogEntry {
            session: SessionMetadata {
                session_id: "test-session-456".to_string(),
                timestamp: "2026-01-29T07:00:00Z".to_string(),
                git_commit: Some("def67890".to_string()),
                git_dirty: true,
            },
            constraint_id: "c2".to_string(),
            constraint_name: "no_speculation".to_string(),
            claim_ids: vec![],
            claim_text: vec![],
            base_strength: 0.9,
            computed_strength: 0.85,
            severity: "warn".to_string(),
            decision: "allow".to_string(),
            message: "Warning".to_string(),
            supporters: vec![],
            attackers: vec![],
            total_claims: 3,
            total_constraints: 2,
            transcript_path: None,
            claim_confidence: None,
            claim_type: None,
            claim_source: None,
            false_positive: Some(false),
            annotation_note: Some("Legit warning".to_string()),
            model: None,
        };
        logger.log(&entry2).unwrap();

        // Read all entries - should skip the malformed line
        let entries = logger.read_all().unwrap();
        assert_eq!(
            entries.len(),
            2,
            "Should read 2 valid entries, skipping malformed line"
        );
        assert_eq!(entries[0].constraint_id, "c1");
        assert_eq!(entries[1].constraint_id, "c2");
    }
}
