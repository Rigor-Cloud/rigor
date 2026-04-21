use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::logging::{query, ViolationLogEntry, ViolationLogger};

/// Per-constraint eval metrics derived from the violation log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintMetric {
    pub constraint_id: String,
    pub constraint_name: String,
    pub hits: usize,
    pub confirmed_true_positives: usize,
    pub false_positives: usize,
    pub unannotated: usize,
    pub false_positive_rate: f64,
    pub precision: f64,
}

/// Aggregate evaluation metrics over the entire violation log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalMetrics {
    pub generated_at: String,
    pub total_violations: usize,
    pub total_sessions: usize,
    pub annotated_violations: usize,
    pub false_positives: usize,
    pub true_positives: usize,
    pub precision: f64,
    pub violations_per_session: f64,
    pub per_constraint: Vec<ConstraintMetric>,
    /// constraints defined but which never fired (if we can detect them from log only,
    /// we cannot—so we record constraints with zero hits seen across the log).
    pub silent_constraints: Vec<String>,
    /// Rolling-window recall trend: violations per session over the most recent sessions.
    pub recall_trend: Vec<SessionBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBucket {
    pub session_id: String,
    pub timestamp: String,
    pub violation_count: usize,
}

/// Compute `EvalMetrics` from a slice of violation log entries.
pub fn compute_metrics(entries: &[ViolationLogEntry]) -> EvalMetrics {
    let stats = query::compute_stats(entries);

    let mut per_constraint_map: HashMap<String, ConstraintMetric> = HashMap::new();
    let mut annotated = 0usize;
    let mut fp_total = 0usize;
    let mut tp_total = 0usize;

    for entry in entries {
        let m = per_constraint_map
            .entry(entry.constraint_id.clone())
            .or_insert_with(|| ConstraintMetric {
                constraint_id: entry.constraint_id.clone(),
                constraint_name: entry.constraint_name.clone(),
                hits: 0,
                confirmed_true_positives: 0,
                false_positives: 0,
                unannotated: 0,
                false_positive_rate: 0.0,
                precision: 0.0,
            });
        m.hits += 1;
        match entry.false_positive {
            Some(true) => {
                m.false_positives += 1;
                fp_total += 1;
                annotated += 1;
            }
            Some(false) => {
                m.confirmed_true_positives += 1;
                tp_total += 1;
                annotated += 1;
            }
            None => {
                m.unannotated += 1;
            }
        }
    }

    for m in per_constraint_map.values_mut() {
        let annotated_hits = m.confirmed_true_positives + m.false_positives;
        if annotated_hits > 0 {
            m.false_positive_rate = m.false_positives as f64 / annotated_hits as f64;
            m.precision = m.confirmed_true_positives as f64 / annotated_hits as f64;
        } else {
            // If nothing is annotated, assume neutral 0.0 FP rate (unknown)
            m.false_positive_rate = 0.0;
            m.precision = 0.0;
        }
    }

    let mut per_constraint: Vec<ConstraintMetric> = per_constraint_map.into_values().collect();
    per_constraint.sort_by(|a, b| b.hits.cmp(&a.hits));

    // Build a per-session trend (chronologically ordered by first-seen timestamp)
    let mut session_order: Vec<(String, String)> = Vec::new();
    let mut session_counts: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        let sid = &entry.session.session_id;
        if !session_counts.contains_key(sid) {
            session_order.push((sid.clone(), entry.session.timestamp.clone()));
        }
        *session_counts.entry(sid.clone()).or_insert(0) += 1;
    }
    let recall_trend: Vec<SessionBucket> = session_order
        .into_iter()
        .map(|(sid, ts)| SessionBucket {
            violation_count: *session_counts.get(&sid).unwrap_or(&0),
            session_id: sid,
            timestamp: ts,
        })
        .collect();

    let violations_per_session = if stats.unique_sessions > 0 {
        stats.total_violations as f64 / stats.unique_sessions as f64
    } else {
        0.0
    };

    let annotated_hits = fp_total + tp_total;
    let precision = if annotated_hits > 0 {
        tp_total as f64 / annotated_hits as f64
    } else {
        0.0
    };

    EvalMetrics {
        generated_at: chrono::Utc::now().to_rfc3339(),
        total_violations: stats.total_violations,
        total_sessions: stats.unique_sessions,
        annotated_violations: annotated,
        false_positives: fp_total,
        true_positives: tp_total,
        precision,
        violations_per_session,
        per_constraint,
        silent_constraints: Vec::new(),
        recall_trend,
    }
}

/// Path helpers
fn rigor_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let dir = home.join(".rigor");
    fs::create_dir_all(&dir).ok();
    Ok(dir)
}

fn baseline_path() -> Result<PathBuf> {
    Ok(rigor_dir()?.join("eval-baseline.json"))
}

fn save_baseline(metrics: &EvalMetrics) -> Result<PathBuf> {
    let path = baseline_path()?;
    let json = serde_json::to_string_pretty(metrics)?;
    fs::write(&path, json).context("Failed to write eval baseline")?;
    Ok(path)
}

fn load_baseline() -> Result<Option<EvalMetrics>> {
    let path = baseline_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(&path)?;
    let m: EvalMetrics = serde_json::from_str(&s)?;
    Ok(Some(m))
}

fn write_report(metrics: &EvalMetrics) -> Result<PathBuf> {
    let report_dir = PathBuf::from(".rigor");
    fs::create_dir_all(&report_dir).ok();
    let path = report_dir.join("eval-report.md");

    let mut md = String::new();
    md.push_str("# Rigor Evaluation Report\n\n");
    md.push_str(&format!("_Generated: {}_\n\n", metrics.generated_at));
    md.push_str("## Summary\n\n");
    md.push_str(&format!(
        "- Total violations: **{}**\n",
        metrics.total_violations
    ));
    md.push_str(&format!(
        "- Total sessions: **{}**\n",
        metrics.total_sessions
    ));
    md.push_str(&format!(
        "- Violations per session (recall proxy): **{:.2}**\n",
        metrics.violations_per_session
    ));
    md.push_str(&format!(
        "- Annotated: **{}** ({} TP / {} FP)\n",
        metrics.annotated_violations, metrics.true_positives, metrics.false_positives
    ));
    md.push_str(&format!(
        "- Precision: **{:.2}%**\n\n",
        metrics.precision * 100.0
    ));

    md.push_str("## Per-Constraint\n\n");
    md.push_str("| Constraint | Hits | TP | FP | Unannotated | FP Rate | Precision |\n");
    md.push_str("|---|---:|---:|---:|---:|---:|---:|\n");
    for c in &metrics.per_constraint {
        md.push_str(&format!(
            "| `{}` ({}) | {} | {} | {} | {} | {:.1}% | {:.1}% |\n",
            c.constraint_id,
            c.constraint_name,
            c.hits,
            c.confirmed_true_positives,
            c.false_positives,
            c.unannotated,
            c.false_positive_rate * 100.0,
            c.precision * 100.0,
        ));
    }
    md.push('\n');

    md.push_str("## Recall Trend (violations per session)\n\n");
    md.push_str("| Session | Timestamp | Violations |\n|---|---|---:|\n");
    for b in &metrics.recall_trend {
        md.push_str(&format!(
            "| `{}` | {} | {} |\n",
            &b.session_id[..b.session_id.len().min(8)],
            b.timestamp,
            b.violation_count
        ));
    }

    fs::write(&path, md).context("Failed to write eval report")?;
    Ok(path)
}

fn compare_metrics(current: &EvalMetrics, baseline: &EvalMetrics) {
    println!(
        "Comparison vs baseline (generated {}):",
        baseline.generated_at
    );
    println!();
    fn delta(cur: f64, base: f64) -> String {
        let d = cur - base;
        let sign = if d >= 0.0 { "+" } else { "" };
        format!("{}{:.3}", sign, d)
    }
    println!(
        "  Total violations:      {} -> {} ({})",
        baseline.total_violations,
        current.total_violations,
        delta(
            current.total_violations as f64,
            baseline.total_violations as f64
        )
    );
    println!(
        "  Violations/session:    {:.2} -> {:.2} ({})",
        baseline.violations_per_session,
        current.violations_per_session,
        delta(
            current.violations_per_session,
            baseline.violations_per_session
        )
    );
    println!(
        "  Precision:             {:.2}% -> {:.2}% ({})",
        baseline.precision * 100.0,
        current.precision * 100.0,
        delta(current.precision * 100.0, baseline.precision * 100.0)
    );
    println!(
        "  FP count:              {} -> {} ({})",
        baseline.false_positives,
        current.false_positives,
        delta(
            current.false_positives as f64,
            baseline.false_positives as f64
        )
    );
}

fn print_summary(metrics: &EvalMetrics) {
    println!("Rigor Evaluation");
    println!("================");
    println!("Total violations:        {}", metrics.total_violations);
    println!("Total sessions:          {}", metrics.total_sessions);
    println!(
        "Violations/session:      {:.2}",
        metrics.violations_per_session
    );
    println!(
        "Annotated: {} (TP: {}, FP: {})",
        metrics.annotated_violations, metrics.true_positives, metrics.false_positives
    );
    println!("Precision:               {:.2}%", metrics.precision * 100.0);
    println!();
    println!("Top constraints by hit rate:");
    for c in metrics.per_constraint.iter().take(10) {
        println!(
            "  {:<32} hits={:<4} fp_rate={:>5.1}%  precision={:>5.1}%",
            c.constraint_id,
            c.hits,
            c.false_positive_rate * 100.0,
            c.precision * 100.0
        );
    }
}

/// Entry point for `rigor eval`.
pub fn run_eval(report: bool, baseline: bool, compare: bool) -> Result<()> {
    let logger = ViolationLogger::new()?;
    let entries = logger.read_all()?;
    let metrics = compute_metrics(&entries);

    print_summary(&metrics);

    if report {
        let p = write_report(&metrics)?;
        println!();
        println!("Report written to: {}", p.display());
    }

    if baseline {
        let p = save_baseline(&metrics)?;
        println!();
        println!("Baseline saved to: {}", p.display());
    }

    if compare {
        println!();
        match load_baseline()? {
            Some(base) => compare_metrics(&metrics, &base),
            None => println!("No baseline found. Run `rigor eval --baseline` first."),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::types::SessionMetadata;

    fn mk(constraint: &str, session: &str, fp: Option<bool>) -> ViolationLogEntry {
        ViolationLogEntry {
            session: SessionMetadata {
                session_id: session.to_string(),
                timestamp: "2026-04-19T00:00:00Z".to_string(),
                git_commit: None,
                git_dirty: false,
            },
            constraint_id: constraint.to_string(),
            constraint_name: format!("name_{}", constraint),
            claim_ids: vec!["cl1".to_string()],
            claim_text: vec!["x".to_string()],
            base_strength: 0.8,
            computed_strength: 0.8,
            severity: "block".to_string(),
            decision: "block".to_string(),
            message: "m".to_string(),
            supporters: vec![],
            attackers: vec![],
            total_claims: 1,
            total_constraints: 1,
            transcript_path: None,
            claim_confidence: None,
            claim_type: None,
            claim_source: None,
            false_positive: fp,
            annotation_note: None,
            model: None,
        }
    }

    #[test]
    fn test_compute_metrics_precision() {
        let entries = vec![
            mk("c1", "s1", Some(true)),  // FP
            mk("c1", "s1", Some(false)), // TP
            mk("c2", "s2", None),        // unannotated
        ];
        let m = compute_metrics(&entries);
        assert_eq!(m.total_violations, 3);
        assert_eq!(m.total_sessions, 2);
        assert_eq!(m.false_positives, 1);
        assert_eq!(m.true_positives, 1);
        assert!((m.precision - 0.5).abs() < 1e-9);
        let c1 = m
            .per_constraint
            .iter()
            .find(|x| x.constraint_id == "c1")
            .unwrap();
        assert_eq!(c1.hits, 2);
        assert!((c1.false_positive_rate - 0.5).abs() < 1e-9);
    }
}
