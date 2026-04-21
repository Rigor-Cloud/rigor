use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::logging::{ViolationLogEntry, ViolationLogger};

const FP_RATE_THRESHOLD: f64 = 0.30;
const MIN_ANNOTATED_HITS: usize = 3;
/// Threshold for auto-refinement: higher bar than suggestions.
const AUTO_FP_RATE_THRESHOLD: f64 = 0.30;
/// Minimum annotations required before auto-refinement triggers.
const AUTO_MIN_ANNOTATED_HITS: usize = 5;

/// A suggested refinement to a constraint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Refinement {
    pub constraint_id: String,
    pub constraint_name: String,
    pub false_positive_rate: f64,
    pub annotated_hits: usize,
    pub false_positive_count: usize,
    /// Example claim texts that were flagged as false positives.
    pub false_positive_examples: Vec<String>,
    /// Suggested regex negative-lookahead style hint; the actual .rego change
    /// is added as a YAML comment so operators can review.
    pub suggested_hint: String,
    /// The note we append into rigor.yaml as a comment.
    pub yaml_note: String,
    /// Timestamp when suggestion was generated.
    pub generated_at: String,
}

/// Persist a refinement history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefinementHistoryEntry {
    pub timestamp: String,
    pub mode: String, // "suggest" | "apply" | "dry-run"
    pub refinements: Vec<Refinement>,
}

fn rigor_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let dir = home.join(".rigor");
    fs::create_dir_all(&dir).ok();
    Ok(dir)
}

fn refinements_log_path() -> Result<PathBuf> {
    Ok(rigor_dir()?.join("refinements.jsonl"))
}

/// Group entries by constraint and compute per-constraint refinement candidates.
pub fn compute_refinements(entries: &[ViolationLogEntry]) -> Vec<Refinement> {
    #[derive(Default)]
    struct Bucket {
        name: String,
        fp: usize,
        tp: usize,
        examples: Vec<String>,
    }
    let mut map: HashMap<String, Bucket> = HashMap::new();

    for e in entries {
        let b = map.entry(e.constraint_id.clone()).or_default();
        b.name = e.constraint_name.clone();
        match e.false_positive {
            Some(true) => {
                b.fp += 1;
                if let Some(t) = e.claim_text.first() {
                    if b.examples.len() < 5 {
                        b.examples.push(t.clone());
                    }
                }
            }
            Some(false) => b.tp += 1,
            None => {}
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut out = Vec::new();
    for (cid, b) in map {
        let annotated = b.fp + b.tp;
        if annotated < MIN_ANNOTATED_HITS {
            continue;
        }
        let fp_rate = b.fp as f64 / annotated as f64;
        if fp_rate <= FP_RATE_THRESHOLD {
            continue;
        }
        let hint = build_regex_hint(&b.examples);
        let yaml_note = format!(
            "# rigor-refine: constraint `{}` has FP rate {:.1}% ({} FP / {} annotated). Consider tightening regex, e.g. add negative lookahead: {}",
            cid,
            fp_rate * 100.0,
            b.fp,
            annotated,
            hint
        );
        out.push(Refinement {
            constraint_id: cid,
            constraint_name: b.name,
            false_positive_rate: fp_rate,
            annotated_hits: annotated,
            false_positive_count: b.fp,
            false_positive_examples: b.examples,
            suggested_hint: hint,
            yaml_note,
            generated_at: now.clone(),
        });
    }
    out.sort_by(|a, b| {
        b.false_positive_rate
            .partial_cmp(&a.false_positive_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// Build a crude regex hint derived from distinctive tokens in FP examples.
fn build_regex_hint(examples: &[String]) -> String {
    if examples.is_empty() {
        return r"(?i)(test|example|mock)".to_string();
    }
    // Pick the most frequent non-trivial word across examples as a negative-lookahead hint.
    let mut freq: HashMap<String, usize> = HashMap::new();
    for ex in examples {
        for word in ex.split(|c: char| !c.is_alphanumeric()) {
            let w = word.to_lowercase();
            if w.len() < 4 {
                continue;
            }
            *freq.entry(w).or_insert(0) += 1;
        }
    }
    let mut words: Vec<(String, usize)> = freq.into_iter().collect();
    words.sort_by(|a, b| b.1.cmp(&a.1));
    let top: Vec<String> = words.into_iter().take(3).map(|(w, _)| w).collect();
    if top.is_empty() {
        r"(?i)(test|example|mock)".to_string()
    } else {
        format!("(?i)({})", top.join("|"))
    }
}

fn append_history(mode: &str, refs: &[Refinement]) -> Result<()> {
    if refs.is_empty() {
        return Ok(());
    }
    let path = refinements_log_path()?;
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    let entry = RefinementHistoryEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        mode: mode.to_string(),
        refinements: refs.to_vec(),
    };
    writeln!(f, "{}", serde_json::to_string(&entry)?)?;
    Ok(())
}

/// Apply refinements by inserting YAML comment blocks just above each matching constraint id line.
/// Returns a unified-ish diff string.
fn apply_to_yaml(yaml_path: &PathBuf, refs: &[Refinement]) -> Result<(String, String)> {
    let original = fs::read_to_string(yaml_path).context("Failed to read rigor.yaml")?;
    let mut lines: Vec<String> = original.lines().map(|s| s.to_string()).collect();

    // Map constraint_id -> refinement
    let mut by_id: HashMap<&str, &Refinement> = HashMap::new();
    for r in refs {
        by_id.insert(r.constraint_id.as_str(), r);
    }

    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        if let Some(rest) = trimmed.strip_prefix("- id:") {
            let id = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if let Some(r) = by_id.get(id.as_str()) {
                // Already refined? Skip if the previous line already has rigor-refine marker.
                let already = i > 0 && lines[i - 1].contains("rigor-refine:");
                if !already {
                    // Determine leading whitespace of the `- id:` line so the comment aligns.
                    let indent: String =
                        lines[i].chars().take_while(|c| c.is_whitespace()).collect();
                    lines.insert(i, format!("{}{}", indent, r.yaml_note));
                    i += 1; // skip over newly inserted line
                }
            }
        }
        i += 1;
    }

    let new = lines.join("\n");
    let new = if original.ends_with('\n') && !new.ends_with('\n') {
        format!("{}\n", new)
    } else {
        new
    };

    let diff = simple_diff(&original, &new);
    Ok((new, diff))
}

/// Very small line-level diff: prints `-` for lines removed, `+` for added.
fn simple_diff(a: &str, b: &str) -> String {
    let av: Vec<&str> = a.lines().collect();
    let bv: Vec<&str> = b.lines().collect();
    let mut i = 0usize;
    let mut j = 0usize;
    let mut out = String::new();
    while i < av.len() || j < bv.len() {
        match (av.get(i), bv.get(j)) {
            (Some(x), Some(y)) if x == y => {
                i += 1;
                j += 1;
            }
            (Some(_), Some(y)) if y.contains("rigor-refine:") => {
                out.push_str(&format!("+ {}\n", y));
                j += 1;
            }
            (Some(x), Some(y)) => {
                out.push_str(&format!("- {}\n", x));
                out.push_str(&format!("+ {}\n", y));
                i += 1;
                j += 1;
            }
            (Some(x), None) => {
                out.push_str(&format!("- {}\n", x));
                i += 1;
            }
            (None, Some(y)) => {
                out.push_str(&format!("+ {}\n", y));
                j += 1;
            }
            (None, None) => break,
        }
    }
    out
}

fn find_rigor_yaml_path() -> Result<PathBuf> {
    // Reuse CLI helper for consistent search.
    super::find_rigor_yaml(None)
}

/// Entry point for `rigor refine`.
pub fn run_refine(apply: bool, dry_run: bool) -> Result<()> {
    let logger = ViolationLogger::new()?;
    let entries = logger.read_all()?;
    let refs = compute_refinements(&entries);

    if refs.is_empty() {
        println!("No refinement suggestions. No constraint exceeds {:.0}% FP rate with at least {} annotations.",
            FP_RATE_THRESHOLD * 100.0, MIN_ANNOTATED_HITS);
        append_history("suggest", &refs).ok();
        return Ok(());
    }

    println!(
        "Refinement suggestions ({} constraint(s) above {:.0}% FP rate):",
        refs.len(),
        FP_RATE_THRESHOLD * 100.0
    );
    println!();
    for r in &refs {
        println!(
            "• {}  FP rate: {:.1}%  ({} FP / {} annotated)",
            r.constraint_id,
            r.false_positive_rate * 100.0,
            r.false_positive_count,
            r.annotated_hits
        );
        println!("    hint: {}", r.suggested_hint);
        if !r.false_positive_examples.is_empty() {
            println!("    examples:");
            for ex in &r.false_positive_examples {
                let clip: String = ex.chars().take(120).collect();
                println!("      - {}", clip);
            }
        }
    }
    println!();

    let yaml_path = match find_rigor_yaml_path() {
        Ok(p) => p,
        Err(e) => {
            println!("Cannot locate rigor.yaml to modify: {}", e);
            append_history("suggest", &refs).ok();
            return Ok(());
        }
    };

    let (new_content, diff) = apply_to_yaml(&yaml_path, &refs)?;

    if dry_run {
        println!("-- dry-run diff for {} --", yaml_path.display());
        println!("{}", diff);
        append_history("dry-run", &refs).ok();
        return Ok(());
    }

    if apply {
        fs::write(&yaml_path, &new_content).context("Failed to write updated rigor.yaml")?;
        println!("Applied refinement annotations to {}", yaml_path.display());
        println!("{}", diff);
        append_history("apply", &refs).ok();
    } else {
        println!("Run with --apply to persist changes, or --dry-run to preview the diff.");
        append_history("suggest", &refs).ok();
    }

    Ok(())
}

/// Closed-loop auto-refinement: reads violations.jsonl, identifies constraints
/// with FP rate > 30% and >= 5 annotations, generates a regex exclusion line,
/// updates rigor.yaml in place, and records the refinement to ~/.rigor/refinements.jsonl.
///
/// Returns the list of refined constraint IDs.
pub fn auto_refine_if_needed(yaml_path: &Path) -> Result<Vec<String>> {
    let logger = ViolationLogger::new()?;
    let entries = logger.read_all()?;

    // Group entries by constraint and compute per-constraint FP stats
    #[derive(Default)]
    struct Bucket {
        name: String,
        fp: usize,
        tp: usize,
        fp_texts: Vec<String>,
    }
    let mut map: HashMap<String, Bucket> = HashMap::new();

    for e in &entries {
        let b = map.entry(e.constraint_id.clone()).or_default();
        b.name = e.constraint_name.clone();
        match e.false_positive {
            Some(true) => {
                b.fp += 1;
                // Collect claim texts from false positives for pattern analysis
                for t in &e.claim_text {
                    if b.fp_texts.len() < 10 {
                        b.fp_texts.push(t.clone());
                    }
                }
            }
            Some(false) => b.tp += 1,
            None => {}
        }
    }

    let mut refined_ids = Vec::new();
    let mut refinements = Vec::new();
    let now = chrono::Utc::now().to_rfc3339();

    for (cid, b) in &map {
        let annotated = b.fp + b.tp;
        if annotated < AUTO_MIN_ANNOTATED_HITS {
            continue;
        }
        let fp_rate = b.fp as f64 / annotated as f64;
        if fp_rate <= AUTO_FP_RATE_THRESHOLD {
            continue;
        }

        // Analyze false-positive claim texts to find common patterns
        let exclusion_pattern = build_regex_hint(&b.fp_texts);

        let yaml_note = format!(
            "# rigor-refine: constraint `{}` has FP rate {:.1}% ({} FP / {} annotated). Consider tightening regex, e.g. add negative lookahead: {}",
            cid,
            fp_rate * 100.0,
            b.fp,
            annotated,
            exclusion_pattern
        );

        refinements.push(Refinement {
            constraint_id: cid.clone(),
            constraint_name: b.name.clone(),
            false_positive_rate: fp_rate,
            annotated_hits: annotated,
            false_positive_count: b.fp,
            false_positive_examples: b.fp_texts.clone(),
            suggested_hint: exclusion_pattern.clone(),
            yaml_note,
            generated_at: now.clone(),
        });

        refined_ids.push(cid.clone());
    }

    if refinements.is_empty() {
        return Ok(refined_ids);
    }

    // Apply refinements to rigor.yaml
    let yaml_pb = yaml_path.to_path_buf();
    let (new_content, _diff) = apply_to_yaml(&yaml_pb, &refinements)?;
    fs::write(yaml_path, &new_content)
        .context("Failed to write updated rigor.yaml during auto-refine")?;

    // Also update the rego blocks: add a `not regex.match(...)` exclusion line
    // for each refined constraint directly into the rego block in rigor.yaml.
    let mut content = fs::read_to_string(yaml_path)?;
    for r in &refinements {
        content = inject_rego_exclusion(&content, &r.constraint_id, &r.suggested_hint);
    }
    fs::write(yaml_path, &content)?;

    // Record to ~/.rigor/refinements.jsonl
    append_history("auto", &refinements).ok();

    Ok(refined_ids)
}

/// Inject a `not regex.match(...)` exclusion line into a constraint's rego block
/// in the raw YAML content. Finds the constraint's rego block by looking for the
/// `- id: <constraint_id>` line, then locating the first `not regex.match` line
/// or the line before `v := {`, and inserts a new exclusion.
fn inject_rego_exclusion(content: &str, constraint_id: &str, pattern: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();
    let mut in_target_constraint = false;
    let mut in_rego_block = false;
    let mut exclusion_injected = false;

    for line in lines.iter() {
        let trimmed = line.trim();

        // Detect the target constraint block
        if trimmed.starts_with("- id:") {
            let id = trimmed
                .strip_prefix("- id:")
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            in_target_constraint = id == constraint_id;
            in_rego_block = false;
            exclusion_injected = false;
        }

        // Detect rego block start
        if in_target_constraint && (trimmed == "rego: |" || trimmed == "rego: >") {
            in_rego_block = true;
        }

        // Within rego block of target constraint, find insertion point
        if in_target_constraint && in_rego_block && !exclusion_injected {
            // Insert before the `v := {` line
            if trimmed.starts_with("v := {") {
                // Determine indentation of the v := line
                let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                let exclusion_line = format!("{}not regex.match(`{}`, c.text)", indent, pattern);
                // Check we haven't already added this exact exclusion
                let already_present = lines.iter().any(|l| l.trim() == exclusion_line.trim());
                if !already_present {
                    result.push(exclusion_line);
                }
                exclusion_injected = true;
            }
        }

        result.push(line.to_string());
    }

    // Ensure trailing newline
    let joined = result.join("\n");
    if content.ends_with('\n') && !joined.ends_with('\n') {
        format!("{}\n", joined)
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::types::SessionMetadata;

    fn mk(cid: &str, fp: Option<bool>, text: &str) -> ViolationLogEntry {
        ViolationLogEntry {
            session: SessionMetadata {
                session_id: "s".into(),
                timestamp: "2026-04-19T00:00:00Z".into(),
                git_commit: None,
                git_dirty: false,
            },
            constraint_id: cid.into(),
            constraint_name: format!("name_{}", cid),
            claim_ids: vec!["c".into()],
            claim_text: vec![text.into()],
            base_strength: 0.8,
            computed_strength: 0.8,
            severity: "block".into(),
            decision: "block".into(),
            message: "m".into(),
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
    fn test_refinement_triggers_above_threshold() {
        let entries = vec![
            mk("c1", Some(true), "rust has garbage collection example"),
            mk("c1", Some(true), "example rust garbage collect test"),
            mk("c1", Some(true), "mock claim garbage collect"),
            mk("c1", Some(false), "rust has garbage collection"),
        ];
        let refs = compute_refinements(&entries);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].constraint_id, "c1");
        assert!(refs[0].false_positive_rate > 0.5);
    }

    #[test]
    fn test_refinement_skipped_below_threshold() {
        // 1 FP out of 5 annotated = 20% — below 30% threshold.
        let entries = vec![
            mk("c1", Some(false), "real violation"),
            mk("c1", Some(false), "real violation 2"),
            mk("c1", Some(false), "real violation 3"),
            mk("c1", Some(false), "real violation 4"),
            mk("c1", Some(true), "mock"),
        ];
        let refs = compute_refinements(&entries);
        assert!(refs.is_empty());
    }
}
