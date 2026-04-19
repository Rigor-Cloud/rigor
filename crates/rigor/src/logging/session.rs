use crate::logging::types::SessionMetadata;
use chrono::Utc;
use git2::{Repository, StatusOptions};
use uuid::Uuid;

impl SessionMetadata {
    /// Capture the current session metadata.
    ///
    /// This function:
    /// - Generates a UUID v4 session ID
    /// - Records the current timestamp in ISO 8601 format
    /// - Attempts to read git repository information (commit hash, dirty flag)
    /// - Fails open if git operations fail (returns None for git fields)
    pub fn capture() -> Self {
        let session_id = Uuid::new_v4().to_string();
        let timestamp = Utc::now().to_rfc3339();

        // Attempt to discover git repository
        let (git_commit, git_dirty) = match Repository::discover(".") {
            Ok(repo) => {
                let commit = Self::get_head_commit(&repo);
                let dirty = Self::is_working_tree_dirty(&repo);
                (commit, dirty)
            }
            Err(_) => {
                // Not in a git repo, or git operations failed - fail open
                (None, false)
            }
        };

        Self {
            session_id,
            timestamp,
            git_commit,
            git_dirty,
        }
    }

    /// Get the short hash (8 chars) of the HEAD commit.
    fn get_head_commit(repo: &Repository) -> Option<String> {
        let head = repo.head().ok()?;
        let commit = head.peel_to_commit().ok()?;
        let oid = commit.id();

        // Short hash (8 characters)
        Some(oid.to_string()[..8].to_string())
    }

    /// Check if the working tree has uncommitted changes.
    fn is_working_tree_dirty(repo: &Repository) -> bool {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        opts.include_ignored(false);

        match repo.statuses(Some(&mut opts)) {
            Ok(statuses) => !statuses.is_empty(),
            Err(_) => false, // Fail open if status check fails
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    #[test]
    fn test_capture_returns_valid_uuid() {
        let session = SessionMetadata::capture();

        // UUID v4 format: 8-4-4-4-12 hex digits
        let uuid_regex =
            Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
                .unwrap();
        assert!(
            uuid_regex.is_match(&session.session_id),
            "Invalid UUID format: {}",
            session.session_id
        );
    }

    #[test]
    fn test_capture_returns_valid_iso8601_timestamp() {
        let session = SessionMetadata::capture();

        // ISO 8601 with timezone, e.g., "2026-01-29T06:00:00+00:00"
        let iso_regex = Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}").unwrap();
        assert!(
            iso_regex.is_match(&session.timestamp),
            "Invalid ISO 8601 timestamp: {}",
            session.timestamp
        );
    }

    #[test]
    fn test_capture_git_info() {
        let session = SessionMetadata::capture();

        // We're in a git repo, so we should have commit info
        // (If this test fails, it might be running outside a git repo)
        if let Some(commit) = &session.git_commit {
            assert_eq!(commit.len(), 8, "Git commit hash should be 8 characters");
            assert!(
                commit.chars().all(|c| c.is_ascii_hexdigit()),
                "Git commit should be hex"
            );
        }

        // git_dirty is a boolean, no specific assertion beyond type check
        let _dirty: bool = session.git_dirty;
    }
}
