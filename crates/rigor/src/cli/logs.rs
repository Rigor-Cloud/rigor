//! `rigor logs` CLI subcommand — view session logs.

use anyhow::Result;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

use crate::logging::session_registry::{self, SessionEntry};

pub fn run_logs(session: Option<String>, follow: bool, lines: usize) -> Result<()> {
    // Resolve which session to show logs for
    let entry = if let Some(ref query) = session {
        session_registry::find_session(query)?
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", query))?
    } else {
        // Default to latest session
        session_registry::latest_session()?
            .ok_or_else(|| anyhow::anyhow!("No sessions recorded yet. Run: rigor ground -- opencode"))?
    };

    let log_path = session_registry::session_log_path(&entry.id)
        .ok_or_else(|| anyhow::anyhow!("Cannot determine log path"))?;

    if !log_path.exists() {
        // Fall back to /tmp/rigor-ground.log for backward compatibility
        let fallback = std::path::PathBuf::from("/tmp/rigor-ground.log");
        if fallback.exists() {
            eprintln!("(Using fallback log: /tmp/rigor-ground.log)");
            return display_log(&fallback, follow, lines);
        }
        anyhow::bail!("No log file found for session: {} ({})", entry.name, entry.id);
    }

    // Print session header
    let alive = session_registry::is_session_alive(&entry);
    let status = if alive { "active" } else { "ended" };
    eprintln!("Session: {} ({}) [{}]", entry.name, &entry.id[..8], status);
    eprintln!("Agent: {} | Constraints: {} | Started: {}",
        entry.agent, entry.constraints, &entry.started_at[..19]);
    eprintln!("---");

    display_log(&log_path, follow, lines)
}

fn display_log(path: &std::path::Path, follow: bool, tail_lines: usize) -> Result<()> {
    if follow {
        // Follow mode — like tail -f
        let mut file = std::fs::File::open(path)?;

        // Seek to end minus a buffer for context
        let metadata = file.metadata()?;
        let file_size = metadata.len();
        let seek_back = std::cmp::min(file_size, 4096);
        file.seek(SeekFrom::End(-(seek_back as i64)))?;

        // Read and discard partial first line
        let mut buf = String::new();
        let mut reader = BufReader::new(file);
        let _ = reader.read_line(&mut buf);
        buf.clear();

        // Print remaining lines as context
        loop {
            buf.clear();
            match reader.read_line(&mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    let clean = sanitize_line(&buf);
                    if !clean.is_empty() {
                        print!("{}", clean);
                    }
                }
                Err(_) => break,
            }
        }

        // Now follow new content
        loop {
            buf.clear();
            match reader.read_line(&mut buf) {
                Ok(0) => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Ok(_) => {
                    let clean = sanitize_line(&buf);
                    if !clean.is_empty() {
                        print!("{}", clean);
                    }
                }
                Err(e) => {
                    eprintln!("Read error: {}", e);
                    break;
                }
            }
        }
    } else {
        // Show last N lines
        let content = std::fs::read(path)?;
        let lines_vec: Vec<&str> = content
            .split(|&b| b == b'\n')
            .filter_map(|line| std::str::from_utf8(line).ok())
            .filter(|line| !line.trim().is_empty())
            .filter(|line| {
                // Filter out binary-heavy lines
                line.chars().filter(|c| c.is_control() && *c != '\t').count() == 0
            })
            .collect();

        let start = lines_vec.len().saturating_sub(tail_lines);
        for line in &lines_vec[start..] {
            println!("{}", line);
        }
    }

    Ok(())
}

/// Clean non-printable characters from a log line.
fn sanitize_line(line: &str) -> String {
    if line.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        // Has binary data — try to extract printable portions
        let clean: String = line.chars()
            .map(|c| if c.is_control() && c != '\n' && c != '\t' { ' ' } else { c })
            .collect();
        // Only return if there's meaningful content
        let trimmed = clean.trim();
        if trimmed.len() > 10 && trimmed.contains("rigor") {
            format!("{}\n", trimmed)
        } else {
            String::new()
        }
    } else {
        line.to_string()
    }
}
