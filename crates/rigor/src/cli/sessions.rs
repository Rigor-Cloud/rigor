//! `rigor sessions` CLI subcommand — list and inspect grounding sessions.

use anyhow::Result;
use std::io::IsTerminal;

use crate::logging::session_registry::{self, SessionEntry};

pub fn run_sessions(active_only: bool, last: Option<usize>) -> Result<()> {
    let sessions = session_registry::read_all_sessions()?;

    if sessions.is_empty() {
        println!("No sessions recorded yet.");
        println!("Run: rigor ground -- opencode");
        return Ok(());
    }

    let filtered: Vec<&SessionEntry> = if active_only {
        sessions.iter().filter(|s| session_registry::is_session_alive(s)).collect()
    } else {
        let n = last.unwrap_or(10);
        sessions.iter().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect()
    };

    if filtered.is_empty() {
        if active_only {
            println!("No active sessions.");
        } else {
            println!("No sessions found.");
        }
        return Ok(());
    }

    let use_colors = std::io::stdout().is_terminal();

    // Header
    println!(
        "{:<24} {:<12} {:<10} {:<20} {:>6} {:>6}",
        "NAME", "AGENT", "STATUS", "STARTED", "REQS", "VIOLS"
    );
    println!("{}", "-".repeat(82));

    for entry in &filtered {
        let alive = session_registry::is_session_alive(entry);
        let status = if alive {
            if use_colors {
                "\x1b[32m● active\x1b[0m".to_string()
            } else {
                "● active".to_string()
            }
        } else if entry.exit_code == Some(0) {
            "  done".to_string()
        } else if entry.exit_code.is_some() {
            if use_colors {
                format!("\x1b[31m  exit {}\x1b[0m", entry.exit_code.unwrap())
            } else {
                format!("  exit {}", entry.exit_code.unwrap())
            }
        } else {
            "  ended".to_string()
        };

        let started = if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&entry.started_at) {
            dt.format("%Y-%m-%d %H:%M").to_string()
        } else {
            entry.started_at[..16].to_string()
        };

        let reqs = entry.requests.map(|r| r.to_string()).unwrap_or_else(|| "-".to_string());
        let viols = entry.violations.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string());

        println!(
            "{:<24} {:<12} {:<10} {:<20} {:>6} {:>6}",
            entry.name, entry.agent, status, started, reqs, viols
        );
    }

    println!();
    println!("View logs: rigor logs --session <name>");
    println!("Live tail: rigor logs --follow");

    Ok(())
}
