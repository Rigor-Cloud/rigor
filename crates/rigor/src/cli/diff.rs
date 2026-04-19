//! `rigor diff` — compare two sessions' violation patterns.
//!
//! Given two session ids (or `--last 2` to pick the most recent two), print
//! a unified-diff-style report showing:
//!   - claim & violation count deltas
//!   - constraints fired in each, with symmetric set difference (+/-/=)
//!   - model usage comparison
//!
//! Sessions are resolved against `~/.rigor/sessions.jsonl` (registry) and
//! violations are aggregated from `~/.rigor/violations.jsonl`.

use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::IsTerminal;

use crate::logging::session_registry::{self, SessionEntry};
use crate::logging::{ViolationLogEntry, ViolationLogger};

pub fn run_diff(a: Option<String>, b: Option<String>, last: Option<usize>) -> Result<()> {
    // Resolve the two sessions.
    let (sess_a, sess_b) = resolve_sessions(a, b, last)?;

    let logger = ViolationLogger::new()?;
    let all = logger.read_all()?;

    let sum_a = summarize(&sess_a, &all);
    let sum_b = summarize(&sess_b, &all);

    let color = std::io::stdout().is_terminal();
    print_diff(&sum_a, &sum_b, color);
    Ok(())
}

struct SessionSummary {
    id: String,
    label: String,
    started_at: String,
    claim_count: usize,
    violation_count: usize,
    /// constraint_id → count
    constraints: BTreeMap<String, usize>,
    /// model → count
    models: BTreeMap<String, usize>,
    /// severity → count
    severities: BTreeMap<String, usize>,
    session_entry: Option<SessionEntry>,
}

fn resolve_sessions(
    a: Option<String>,
    b: Option<String>,
    last: Option<usize>,
) -> Result<(ResolvedSession, ResolvedSession)> {
    if let Some(n) = last {
        if n != 2 {
            bail!("--last currently supports only 2 (got {})", n);
        }
        let mut sessions = session_registry::read_all_sessions()?;
        if sessions.len() < 2 {
            bail!(
                "Need at least 2 sessions in registry, found {}. Run `rigor ground …` first.",
                sessions.len()
            );
        }
        // Newest is last; compare previous vs newest.
        let newest = sessions.pop().unwrap();
        let prev = sessions.pop().unwrap();
        return Ok((
            ResolvedSession::from_registry(prev),
            ResolvedSession::from_registry(newest),
        ));
    }

    let a = a.ok_or_else(|| {
        anyhow::anyhow!("Provide two session identifiers or use --last 2")
    })?;
    let b = b.ok_or_else(|| {
        anyhow::anyhow!("Second session identifier missing (or pass --last 2)")
    })?;

    Ok((ResolvedSession::lookup(&a)?, ResolvedSession::lookup(&b)?))
}

struct ResolvedSession {
    /// The actual session_id stored on violation entries. We try to match
    /// by registry name/prefix first; if not found, we treat the argument
    /// as the literal violation session_id.
    match_id: String,
    label: String,
    entry: Option<SessionEntry>,
}

impl ResolvedSession {
    fn from_registry(entry: SessionEntry) -> Self {
        Self {
            match_id: entry.id.clone(),
            label: format!("{} ({})", entry.name, &entry.id[..entry.id.len().min(8)]),
            entry: Some(entry),
        }
    }

    fn lookup(query: &str) -> Result<Self> {
        if let Some(entry) = session_registry::find_session(query)? {
            return Ok(Self::from_registry(entry));
        }
        // Fall back: treat as raw session id (possibly from violations.jsonl only).
        Ok(Self {
            match_id: query.to_string(),
            label: query.to_string(),
            entry: None,
        })
    }
}

fn summarize(s: &ResolvedSession, entries: &[ViolationLogEntry]) -> SessionSummary {
    let mut constraints: BTreeMap<String, usize> = BTreeMap::new();
    let mut models: BTreeMap<String, usize> = BTreeMap::new();
    let mut severities: BTreeMap<String, usize> = BTreeMap::new();
    let mut claim_ids: HashMap<String, ()> = HashMap::new();
    let mut started_at = String::new();
    let mut violation_count = 0usize;

    for e in entries {
        if e.session.session_id != s.match_id
            && !e.session.session_id.starts_with(&s.match_id)
        {
            continue;
        }
        if started_at.is_empty() {
            started_at = e.session.timestamp.clone();
        }
        violation_count += 1;
        *constraints.entry(e.constraint_id.clone()).or_insert(0) += 1;
        *severities.entry(e.severity.clone()).or_insert(0) += 1;
        if let Some(m) = &e.model {
            *models.entry(m.clone()).or_insert(0) += 1;
        }
        for cid in &e.claim_ids {
            claim_ids.insert(cid.clone(), ());
        }
    }

    // Prefer registry claim/request counters when available.
    let session_claim_count = claim_ids.len();

    SessionSummary {
        id: s.match_id.clone(),
        label: s.label.clone(),
        started_at: s
            .entry
            .as_ref()
            .map(|e| e.started_at.clone())
            .unwrap_or(started_at),
        claim_count: session_claim_count,
        violation_count,
        constraints,
        models,
        severities,
        session_entry: s.entry.clone(),
    }
}

fn print_diff(a: &SessionSummary, b: &SessionSummary, color: bool) {
    let (red, green, cyan, bold, reset) = if color {
        ("\x1b[31m", "\x1b[32m", "\x1b[36m", "\x1b[1m", "\x1b[0m")
    } else {
        ("", "", "", "", "")
    };

    println!("{bold}rigor diff{reset}");
    println!("  {red}--- A: {}{reset}", a.label);
    println!("      started: {}", a.started_at);
    println!("  {green}+++ B: {}{reset}", b.label);
    println!("      started: {}", b.started_at);
    println!();

    // Counts table
    println!("{bold}Counts{reset}");
    print_count_row("claims", a.claim_count, b.claim_count, color);
    print_count_row("violations", a.violation_count, b.violation_count, color);
    if let (Some(ea), Some(eb)) = (&a.session_entry, &b.session_entry) {
        if let (Some(ra), Some(rb)) = (ea.requests, eb.requests) {
            print_count_row("requests", ra as usize, rb as usize, color);
        }
        if let (Some(ta), Some(tb)) = (ea.total_tokens, eb.total_tokens) {
            print_count_row("tokens", ta as usize, tb as usize, color);
        }
    }
    println!();

    // Severity
    println!("{bold}Severity{reset}");
    let sev_keys: BTreeSet<&String> = a.severities.keys().chain(b.severities.keys()).collect();
    for k in sev_keys {
        let av = *a.severities.get(k).unwrap_or(&0);
        let bv = *b.severities.get(k).unwrap_or(&0);
        print_count_row(k, av, bv, color);
    }
    println!();

    // Constraints
    println!("{bold}Constraints (unified){reset}");
    let cs: BTreeSet<&String> = a.constraints.keys().chain(b.constraints.keys()).collect();
    let mut new_in_b: Vec<&String> = Vec::new();
    let mut resolved: Vec<&String> = Vec::new();
    let mut shared: Vec<&String> = Vec::new();
    for k in &cs {
        let in_a = a.constraints.contains_key(*k);
        let in_b = b.constraints.contains_key(*k);
        match (in_a, in_b) {
            (true, true) => shared.push(*k),
            (false, true) => new_in_b.push(*k),
            (true, false) => resolved.push(*k),
            (false, false) => {}
        }
    }
    for k in &resolved {
        println!(
            "  {red}-{reset} {:<40} A={}",
            k,
            a.constraints.get(*k).unwrap_or(&0)
        );
    }
    for k in &new_in_b {
        println!(
            "  {green}+{reset} {:<40} B={}",
            k,
            b.constraints.get(*k).unwrap_or(&0)
        );
    }
    for k in &shared {
        let av = *a.constraints.get(*k).unwrap_or(&0);
        let bv = *b.constraints.get(*k).unwrap_or(&0);
        let marker = if av == bv {
            "=".to_string()
        } else if bv > av {
            format!("{}Δ{}", green, reset)
        } else {
            format!("{}Δ{}", red, reset)
        };
        println!("  {} {:<40} A={} B={}", marker, k, av, bv);
    }
    if new_in_b.is_empty() && resolved.is_empty() && shared.is_empty() {
        println!("  (no constraints fired in either session)");
    }
    println!();

    // Models
    println!("{bold}Model usage{reset}");
    let mk: BTreeSet<&String> = a.models.keys().chain(b.models.keys()).collect();
    if mk.is_empty() {
        println!("  (no model info recorded)");
    } else {
        for k in mk {
            let av = *a.models.get(k).unwrap_or(&0);
            let bv = *b.models.get(k).unwrap_or(&0);
            print_count_row(&format!("{cyan}{}{reset}", k), av, bv, color);
        }
    }

    // Summary line
    println!();
    let delta = b.violation_count as i64 - a.violation_count as i64;
    let arrow = if delta > 0 {
        format!("{red}+{}{reset}", delta)
    } else if delta < 0 {
        format!("{green}{}{reset}", delta)
    } else {
        "0".to_string()
    };
    println!("{bold}Δ violations: {}{reset}", arrow);
    println!(
        "  new: {} resolved: {} shared: {}",
        new_in_b.len(),
        resolved.len(),
        shared.len()
    );
    // suppress unused warnings when not colored
    let _ = (red, green, cyan, bold, reset);
    let _ = (a.id.len(), b.id.len());
}

fn print_count_row(label: &str, a: usize, b: usize, color: bool) {
    let (red, green, reset) = if color {
        ("\x1b[31m", "\x1b[32m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    let delta = b as i64 - a as i64;
    let delta_str = if delta > 0 {
        format!("{}+{}{}", red, delta, reset)
    } else if delta < 0 {
        format!("{}{}{}", green, delta, reset)
    } else {
        "0".to_string()
    };
    println!("  {:<12} A={:<6} B={:<6} Δ={}", label, a, b, delta_str);
}
