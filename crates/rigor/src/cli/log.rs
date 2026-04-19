use anyhow::Result;
use clap::Subcommand;
use std::io::IsTerminal;

use crate::logging::{annotate, query, ViolationLogger};

#[derive(Subcommand)]
pub enum LogCommands {
    /// Display the last N violations (default 10)
    Last {
        #[arg(default_value = "10")]
        count: usize,
    },
    /// Filter violations by constraint ID
    Constraint { constraint_id: String },
    /// Filter violations by session ID
    Session { session_id: String },
    /// Display statistics about violations
    Stats,
    /// Annotate a violation entry
    Annotate {
        /// Index of violation in log (1-based, from `rigor log last`)
        index: usize,
        /// Mark as false positive
        #[arg(long)]
        false_positive: bool,
        /// Optional note
        #[arg(long)]
        note: Option<String>,
    },
}

pub fn run_log(cmd: LogCommands) -> Result<()> {
    let logger = ViolationLogger::new()?;
    let entries = logger.read_all()?;

    if entries.is_empty() && !matches!(cmd, LogCommands::Stats) {
        println!("No violations recorded yet.");
        return Ok(());
    }

    match cmd {
        LogCommands::Last { count } => {
            let filtered = query::filter_last(&entries, count);
            display_entries(&filtered);
        }
        LogCommands::Constraint { constraint_id } => {
            let filtered = query::filter_by_constraint(&entries, &constraint_id);
            if filtered.is_empty() {
                println!("No violations found for constraint: {}", constraint_id);
            } else {
                display_entries(&filtered);
            }
        }
        LogCommands::Session { session_id } => {
            let filtered = query::filter_by_session(&entries, &session_id);
            if filtered.is_empty() {
                println!("No violations found for session: {}", session_id);
            } else {
                display_entries(&filtered);
            }
        }
        LogCommands::Stats => {
            let stats = query::compute_stats(&entries);
            display_stats(&stats);
        }
        LogCommands::Annotate {
            index,
            false_positive,
            note,
        } => {
            let mut entries_mut = entries;
            annotate::annotate_entry(&mut entries_mut, index, false_positive, note.clone())?;
            annotate::rewrite_log(&logger, &entries_mut)?;

            let annotation_type = if false_positive {
                "false positive"
            } else {
                "not false positive"
            };

            println!("✓ Annotated violation #{} as {}", index, annotation_type);
            if let Some(n) = note {
                println!("  Note: {}", n);
            }
        }
    }

    Ok(())
}

/// Display violation entries in formatted output.
fn display_entries(entries: &[&crate::logging::ViolationLogEntry]) {
    let use_colors = std::io::stdout().is_terminal();

    for entry in entries {
        let severity_display = if use_colors {
            match entry.severity.as_str() {
                "block" => format!("\x1b[31m{}\x1b[0m", entry.severity), // red
                "warn" => format!("\x1b[33m{}\x1b[0m", entry.severity),  // yellow
                "allow" => format!("\x1b[32m{}\x1b[0m", entry.severity), // green
                _ => entry.severity.clone(),
            }
        } else {
            entry.severity.clone()
        };

        println!(
            "[{}] {} {}: {}",
            entry.session.timestamp, severity_display, entry.constraint_id, entry.message
        );

        // Display claims if any
        if !entry.claim_text.is_empty() {
            for claim in &entry.claim_text {
                println!("  Claim: {}", claim);
            }
        }

        // Display annotation if present
        if let Some(fp) = entry.false_positive {
            let fp_text = if fp { "FALSE POSITIVE" } else { "Verified" };
            print!("  Annotation: {}", fp_text);
            if let Some(note) = &entry.annotation_note {
                print!(" - {}", note);
            }
            println!();
        }
    }
}

/// Display statistics in table format.
fn display_stats(stats: &query::LogStats) {
    if stats.total_violations == 0 {
        println!("No violations recorded yet.");
        return;
    }

    println!("Violation Statistics");
    println!("===================");
    println!();
    println!("Total violations:     {}", stats.total_violations);
    println!("Unique constraints:   {}", stats.unique_constraints.len());
    println!("Unique sessions:      {}", stats.unique_sessions);
    println!();

    println!("By Severity:");
    println!("  Block:  {}", stats.severity_counts.block);
    println!("  Warn:   {}", stats.severity_counts.warn);
    println!("  Allow:  {}", stats.severity_counts.allow);
    println!();

    println!("By Constraint:");
    let mut constraint_vec: Vec<_> = stats.unique_constraints.iter().collect();
    constraint_vec.sort_by(|a, b| b.1.cmp(a.1)); // Sort by count descending
    for (constraint_id, count) in constraint_vec {
        println!("  {}: {}", constraint_id, count);
    }
    println!();

    if let (Some(first), Some(last)) = (&stats.first_timestamp, &stats.last_timestamp) {
        println!("Date Range:");
        println!("  First: {}", first);
        println!("  Last:  {}", last);
        println!();
    }

    if stats.false_positive_count > 0 {
        let fp_rate = (stats.false_positive_count as f64 / stats.total_violations as f64) * 100.0;
        println!("False Positives:");
        println!("  Count: {}", stats.false_positive_count);
        println!("  Rate:  {:.1}%", fp_rate);
    }
}
