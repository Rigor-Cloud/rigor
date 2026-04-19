//! `rigor search` — full-text search across the violation log.
//!
//! Searches `~/.rigor/violations.jsonl`, matching `query` (case-insensitive)
//! against `claim_text`, `constraint_id`, `constraint_name`, and `message`.
//! Supports filters for constraint, severity, since-date, and model.

use anyhow::{Context, Result};
use std::io::IsTerminal;

use crate::logging::{ViolationLogEntry, ViolationLogger};

/// Run the search command. `query` may be empty (then only filters apply).
pub fn run_search(
    query: Option<String>,
    constraint: Option<String>,
    severity: Option<String>,
    since: Option<String>,
    model: Option<String>,
    limit: usize,
) -> Result<()> {
    let logger = ViolationLogger::new()?;
    let entries = logger.read_all()?;

    if entries.is_empty() {
        println!("No violations recorded yet.");
        return Ok(());
    }

    // Lowercase query for case-insensitive substring match.
    let needle = query.as_deref().unwrap_or("").to_lowercase();

    // Parse `since` — accept YYYY-MM-DD or full RFC3339. Treat YYYY-MM-DD as UTC midnight.
    let since_ts: Option<chrono::DateTime<chrono::Utc>> = match since.as_deref() {
        None => None,
        Some(s) => Some(parse_since(s).with_context(|| format!("Invalid --since: {}", s))?),
    };

    let matches: Vec<&ViolationLogEntry> = entries
        .iter()
        .filter(|e| {
            if let Some(c) = &constraint {
                if &e.constraint_id != c {
                    return false;
                }
            }
            if let Some(sv) = &severity {
                if &e.severity != sv {
                    return false;
                }
            }
            if let Some(m) = &model {
                match &e.model {
                    Some(em) => {
                        if !em.to_lowercase().contains(&m.to_lowercase()) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            if let Some(ts) = since_ts {
                match chrono::DateTime::parse_from_rfc3339(&e.session.timestamp) {
                    Ok(entry_ts) => {
                        if entry_ts.with_timezone(&chrono::Utc) < ts {
                            return false;
                        }
                    }
                    Err(_) => return false,
                }
            }
            if needle.is_empty() {
                return true;
            }
            matches_text(e, &needle)
        })
        .collect();

    if matches.is_empty() {
        println!("No matches.");
        return Ok(());
    }

    let use_colors = std::io::stdout().is_terminal();
    let total = matches.len();
    // Most recent first.
    let to_show: Vec<&&ViolationLogEntry> = matches.iter().rev().take(limit).collect();

    println!(
        "Found {} match(es){}",
        total,
        if total > to_show.len() {
            format!(" (showing {})", to_show.len())
        } else {
            String::new()
        }
    );
    println!();

    for e in to_show {
        print_match(e, &needle, use_colors);
    }

    Ok(())
}

fn parse_since(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    // Try RFC3339 first.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&chrono::Utc));
    }
    // Try YYYY-MM-DD → assume UTC midnight.
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let ndt = d
            .and_hms_opt(0, 0, 0)
            .context("Invalid date")?;
        return Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
            ndt,
            chrono::Utc,
        ));
    }
    anyhow::bail!("Expected YYYY-MM-DD or RFC3339 timestamp")
}

fn matches_text(e: &ViolationLogEntry, needle: &str) -> bool {
    if e.constraint_id.to_lowercase().contains(needle) {
        return true;
    }
    if e.constraint_name.to_lowercase().contains(needle) {
        return true;
    }
    if e.message.to_lowercase().contains(needle) {
        return true;
    }
    if e.claim_text
        .iter()
        .any(|c| c.to_lowercase().contains(needle))
    {
        return true;
    }
    false
}

fn print_match(e: &ViolationLogEntry, needle: &str, color: bool) {
    let sev = if color {
        match e.severity.as_str() {
            "block" => format!("\x1b[31m{:>5}\x1b[0m", e.severity),
            "warn" => format!("\x1b[33m{:>5}\x1b[0m", e.severity),
            "allow" => format!("\x1b[32m{:>5}\x1b[0m", e.severity),
            _ => format!("{:>5}", e.severity),
        }
    } else {
        format!("{:>5}", e.severity)
    };

    let short_sess = if e.session.session_id.len() >= 8 {
        &e.session.session_id[..8]
    } else {
        &e.session.session_id
    };

    let model_tag = match &e.model {
        Some(m) => format!(" model={}", m),
        None => String::new(),
    };

    println!(
        "[{}] {} {} session={}{}",
        e.session.timestamp, sev, e.constraint_id, short_sess, model_tag
    );
    println!("  {}", highlight(&e.message, needle, color));
    for claim in &e.claim_text {
        println!("  · {}", highlight(claim, needle, color));
    }
    if e.false_positive == Some(true) {
        println!("  [FALSE POSITIVE]");
    }
    println!();
}

fn highlight(text: &str, needle: &str, color: bool) -> String {
    if !color || needle.is_empty() {
        return text.to_string();
    }
    // Case-insensitive highlighting — walk the haystack in lowercase, splice
    // from the original to preserve casing.
    let lower = text.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while let Some(i) = lower[cursor..].find(needle) {
        let start = cursor + i;
        let end = start + needle.len();
        out.push_str(&text[cursor..start]);
        out.push_str("\x1b[1;33m");
        out.push_str(&text[start..end]);
        out.push_str("\x1b[0m");
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}
