//! Session registry — tracks all rigor ground sessions with metadata.
//!
//! Sessions are stored in `~/.rigor/sessions.jsonl` (one JSON object per line).
//! Each session has per-session logs in `~/.rigor/sessions/<id>/rigor.log`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// A session entry in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    /// Unique session ID (UUID v4)
    pub id: String,
    /// Human-friendly session name (auto-generated or user-provided)
    pub name: String,
    /// Type of AI agent being grounded ("opencode" | "claude_code" | "unknown")
    pub agent: String,
    /// ISO 8601 timestamp when session started
    pub started_at: String,
    /// ISO 8601 timestamp when session ended (None if still running)
    pub ended_at: Option<String>,
    /// Process ID of the rigor ground process
    pub pid: u32,
    /// Number of constraints loaded
    pub constraints: usize,
    /// Path to rigor.yaml used
    pub config_path: String,
    /// Working directory where rigor ground was invoked
    pub cwd: String,
    /// Total requests proxied (updated on session end)
    pub requests: Option<u64>,
    /// Total violations found (updated on session end)
    pub violations: Option<u64>,
    /// Total tokens used (updated on session end)
    pub total_tokens: Option<u64>,
    /// Exit status of the child process
    pub exit_code: Option<i32>,
}

impl SessionEntry {
    /// Generate a human-friendly session name from context.
    /// Format: "{agent}-{short_id}" or user-provided name.
    pub fn auto_name(agent: &str, id: &str) -> String {
        let short_id = &id[..8];
        let now = chrono::Local::now();
        format!("{}-{}-{}", agent, now.format("%H%M"), short_id)
    }
}

/// Path to the session registry file.
pub fn registry_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".rigor/sessions.jsonl"))
}

/// Path to per-session log directory.
pub fn session_log_dir(session_id: &str) -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(format!(".rigor/sessions/{}", session_id)))
}

/// Path to per-session log file.
pub fn session_log_path(session_id: &str) -> Option<PathBuf> {
    session_log_dir(session_id).map(|d| d.join("rigor.log"))
}

/// Register a new session (append to registry).
pub fn register_session(entry: &SessionEntry) -> Result<()> {
    let path = registry_path().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Create per-session log directory
    if let Some(log_dir) = session_log_dir(&entry.id) {
        fs::create_dir_all(&log_dir)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    let json = serde_json::to_string(entry)?;
    writeln!(file, "{}", json)?;
    Ok(())
}

/// Update a session entry (rewrite the line in the registry).
pub fn update_session(
    session_id: &str,
    mut update_fn: impl FnMut(&mut SessionEntry),
) -> Result<()> {
    let path = registry_path().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    if !path.exists() {
        return Ok(());
    }

    let contents = fs::read_to_string(&path)?;
    let mut lines: Vec<String> = Vec::new();
    let mut applied = false;

    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut entry: SessionEntry = serde_json::from_str(line)?;
        if entry.id == session_id && !applied {
            update_fn(&mut entry);
            applied = true;
        }
        lines.push(serde_json::to_string(&entry)?);
    }

    fs::write(&path, lines.join("\n") + "\n")?;
    Ok(())
}

/// Read all session entries from the registry.
pub fn read_all_sessions() -> Result<Vec<SessionEntry>> {
    let path = registry_path().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(&path)?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<SessionEntry>(&line) {
            Ok(entry) => entries.push(entry),
            Err(_) => continue, // Skip malformed lines
        }
    }

    Ok(entries)
}

/// Check if a session is still alive (process exists).
pub fn is_session_alive(entry: &SessionEntry) -> bool {
    if entry.ended_at.is_some() {
        return false;
    }
    unsafe { libc::kill(entry.pid as i32, 0) == 0 }
}

/// Find a session by name or ID prefix.
pub fn find_session(query: &str) -> Result<Option<SessionEntry>> {
    let sessions = read_all_sessions()?;
    // Try exact name match first
    if let Some(entry) = sessions.iter().find(|s| s.name == query) {
        return Ok(Some(entry.clone()));
    }
    // Try ID prefix match
    if let Some(entry) = sessions.iter().find(|s| s.id.starts_with(query)) {
        return Ok(Some(entry.clone()));
    }
    Ok(None)
}

/// Get the most recent session.
pub fn latest_session() -> Result<Option<SessionEntry>> {
    let sessions = read_all_sessions()?;
    Ok(sessions.into_iter().last())
}
