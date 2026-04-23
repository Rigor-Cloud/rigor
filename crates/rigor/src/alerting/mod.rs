//! Alerting: webhook notifications when violation conditions are met.
//!
//! Config lives at `~/.rigor/alerts.json` as a JSON array of `AlertRule`.
//! Rules evaluate against newly persisted `ViolationLogEntry` batches and
//! fire as HTTP POSTs with a JSON payload describing the trigger.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use crate::logging::ViolationLogEntry;

/// What condition should cause the alert to fire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlertTrigger {
    /// Fire when at least `threshold` violations appear in a single batch.
    Violation,
    /// Fire when a specific constraint id fires (any count).
    Constraint,
    /// Fire on a specific severity level.
    Severity,
}

impl AlertTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertTrigger::Violation => "violation",
            AlertTrigger::Constraint => "constraint",
            AlertTrigger::Severity => "severity",
        }
    }
}

/// A single alert rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub webhook: String,
    pub trigger: AlertTrigger,
    /// Minimum violation count in a batch to fire (Violation trigger). Default 1.
    #[serde(default = "default_threshold")]
    pub threshold: usize,
    /// Constraint id filter (for Constraint trigger).
    #[serde(default)]
    pub constraint: Option<String>,
    /// Severity filter ("block" | "warn" | "allow") — applies to any trigger.
    #[serde(default)]
    pub severity: Option<String>,
    /// Created-at ISO 8601 timestamp.
    #[serde(default)]
    pub created_at: Option<String>,
}

fn default_threshold() -> usize {
    1
}

/// Path to `~/.rigor/alerts.json`.
pub fn alerts_path() -> Result<PathBuf> {
    let dir = crate::paths::rigor_home();
    fs::create_dir_all(&dir).ok();
    Ok(dir.join("alerts.json"))
}

/// Read all configured alert rules. Returns empty list if file is absent.
pub fn read_rules() -> Result<Vec<AlertRule>> {
    let path = alerts_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(&path).context("Failed to read alerts.json")?;
    if data.trim().is_empty() {
        return Ok(Vec::new());
    }
    let rules: Vec<AlertRule> = serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(rules)
}

/// Atomically rewrite the rules file.
pub fn write_rules(rules: &[AlertRule]) -> Result<()> {
    let path = alerts_path()?;
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(rules)?;
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp)?;
    f.write_all(json.as_bytes())?;
    f.flush()?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// Add a new rule. Returns the generated id.
pub fn add_rule(
    webhook: String,
    trigger: AlertTrigger,
    threshold: usize,
    constraint: Option<String>,
    severity: Option<String>,
) -> Result<AlertRule> {
    let mut rules = read_rules()?;
    let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let rule = AlertRule {
        id: id.clone(),
        webhook,
        trigger,
        threshold,
        constraint,
        severity,
        created_at: Some(chrono::Utc::now().to_rfc3339()),
    };
    rules.push(rule.clone());
    write_rules(&rules)?;
    Ok(rule)
}

/// Remove a rule by id prefix. Returns true if removed.
pub fn remove_rule(id: &str) -> Result<bool> {
    let mut rules = read_rules()?;
    let before = rules.len();
    rules.retain(|r| !r.id.starts_with(id));
    let removed = rules.len() != before;
    if removed {
        write_rules(&rules)?;
    }
    Ok(removed)
}

/// Webhook payload sent to configured endpoints.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload<'a> {
    pub rule_id: &'a str,
    pub trigger: &'a str,
    pub timestamp: String,
    pub violation_count: usize,
    pub session_id: &'a str,
    pub git_commit: Option<&'a str>,
    pub violations: Vec<WebhookViolation<'a>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookViolation<'a> {
    pub constraint_id: &'a str,
    pub constraint_name: &'a str,
    pub severity: &'a str,
    pub decision: &'a str,
    pub message: &'a str,
    pub claim_text: &'a [String],
    pub model: Option<&'a str>,
}

/// Decide whether a rule matches a violation batch.
pub fn rule_matches(rule: &AlertRule, batch: &[ViolationLogEntry]) -> bool {
    if batch.is_empty() {
        return false;
    }

    // Optional severity filter applies to all triggers.
    let filter_by_sev = |e: &&ViolationLogEntry| match &rule.severity {
        Some(s) => &e.severity == s,
        None => true,
    };

    match rule.trigger {
        AlertTrigger::Violation => {
            let matching = batch.iter().filter(filter_by_sev).count();
            matching >= rule.threshold.max(1)
        }
        AlertTrigger::Constraint => {
            let cid = match &rule.constraint {
                Some(c) => c,
                None => return false,
            };
            batch
                .iter()
                .filter(filter_by_sev)
                .any(|e| &e.constraint_id == cid)
        }
        AlertTrigger::Severity => {
            let sev = match &rule.severity {
                Some(s) => s,
                None => return false,
            };
            batch.iter().any(|e| &e.severity == sev)
        }
    }
}

/// Build a payload for a rule + batch.
pub fn build_payload<'a>(
    rule: &'a AlertRule,
    batch: &'a [ViolationLogEntry],
) -> WebhookPayload<'a> {
    let first = &batch[0];
    let matching: Vec<&ViolationLogEntry> = batch
        .iter()
        .filter(|e| match &rule.severity {
            Some(s) => &e.severity == s,
            None => true,
        })
        .collect();

    WebhookPayload {
        rule_id: &rule.id,
        trigger: rule.trigger.as_str(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        violation_count: matching.len(),
        session_id: &first.session.session_id,
        git_commit: first.session.git_commit.as_deref(),
        violations: matching
            .iter()
            .map(|e| WebhookViolation {
                constraint_id: &e.constraint_id,
                constraint_name: &e.constraint_name,
                severity: &e.severity,
                decision: &e.decision,
                message: &e.message,
                claim_text: &e.claim_text,
                model: e.model.as_deref(),
            })
            .collect(),
    }
}

/// Fire all matching rules for a batch of violations. Best-effort; errors are
/// logged via eprintln! but never returned to the caller.
pub async fn fire_for_violations(batch: &[ViolationLogEntry]) -> Result<()> {
    let rules = match read_rules() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("rigor alert: failed to read rules: {}", e);
            return Ok(());
        }
    };
    if rules.is_empty() {
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok();

    for rule in &rules {
        if !rule_matches(rule, batch) {
            continue;
        }
        let payload = build_payload(rule, batch);
        let Some(client) = &client else { continue };
        match client.post(&rule.webhook).json(&payload).send().await {
            Ok(r) => {
                if !r.status().is_success() {
                    eprintln!("rigor alert: {} webhook returned {}", rule.id, r.status());
                }
            }
            Err(e) => {
                eprintln!("rigor alert: {} webhook failed: {}", rule.id, e);
            }
        }
    }
    Ok(())
}

/// Send a synthetic test alert to every rule's webhook.
pub async fn send_test() -> Result<usize> {
    let rules = read_rules()?;
    if rules.is_empty() {
        return Ok(0);
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let mut ok = 0usize;
    for rule in &rules {
        let payload = serde_json::json!({
            "rule_id": rule.id,
            "trigger": rule.trigger.as_str(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "test": true,
            "message": "rigor alert test ping",
        });
        match client.post(&rule.webhook).json(&payload).send().await {
            Ok(r) if r.status().is_success() => ok += 1,
            Ok(r) => eprintln!("rigor alert test: {} → {}", rule.id, r.status()),
            Err(e) => eprintln!("rigor alert test: {} → {}", rule.id, e),
        }
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::types::SessionMetadata;

    fn mk_entry(constraint: &str, severity: &str) -> ViolationLogEntry {
        ViolationLogEntry {
            session: SessionMetadata {
                session_id: "sess".into(),
                timestamp: "2026-01-29T00:00:00Z".into(),
                git_commit: None,
                git_dirty: false,
            },
            constraint_id: constraint.into(),
            constraint_name: constraint.into(),
            claim_ids: vec![],
            claim_text: vec![],
            base_strength: 0.8,
            computed_strength: 0.8,
            severity: severity.into(),
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
            false_positive: None,
            annotation_note: None,
            model: None,
        }
    }

    #[test]
    fn violation_trigger_respects_threshold() {
        let rule = AlertRule {
            id: "a".into(),
            webhook: "http://x".into(),
            trigger: AlertTrigger::Violation,
            threshold: 3,
            constraint: None,
            severity: None,
            created_at: None,
        };
        let batch = vec![mk_entry("c1", "block"), mk_entry("c2", "block")];
        assert!(!rule_matches(&rule, &batch));
        let batch3 = vec![
            mk_entry("c1", "block"),
            mk_entry("c2", "block"),
            mk_entry("c3", "warn"),
        ];
        assert!(rule_matches(&rule, &batch3));
    }

    #[test]
    fn constraint_trigger_matches_id() {
        let rule = AlertRule {
            id: "b".into(),
            webhook: "http://x".into(),
            trigger: AlertTrigger::Constraint,
            threshold: 1,
            constraint: Some("rust-no-gc".into()),
            severity: None,
            created_at: None,
        };
        let batch = vec![mk_entry("something-else", "block")];
        assert!(!rule_matches(&rule, &batch));
        let batch = vec![mk_entry("rust-no-gc", "warn")];
        assert!(rule_matches(&rule, &batch));
    }

    #[test]
    fn severity_trigger_matches_level() {
        let rule = AlertRule {
            id: "c".into(),
            webhook: "http://x".into(),
            trigger: AlertTrigger::Severity,
            threshold: 1,
            constraint: None,
            severity: Some("block".into()),
            created_at: None,
        };
        let batch = vec![mk_entry("c1", "warn")];
        assert!(!rule_matches(&rule, &batch));
        let batch = vec![mk_entry("c1", "block")];
        assert!(rule_matches(&rule, &batch));
    }
}
