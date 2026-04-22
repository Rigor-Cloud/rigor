//! `rigor alert` — manage webhook alert rules.

use anyhow::{bail, Result};
use clap::Subcommand;
use std::io::IsTerminal;

use crate::alerting::{self, AlertTrigger};

#[derive(Subcommand)]
pub enum AlertCommands {
    /// Add a new alert rule.
    Add {
        /// Webhook URL (e.g. Slack incoming webhook).
        #[arg(long)]
        webhook: String,
        /// Trigger: `violation` | `constraint` | `severity`.
        #[arg(long, default_value = "violation")]
        on: String,
        /// Minimum violations in a batch to fire (violation trigger).
        #[arg(long, default_value = "1")]
        threshold: usize,
        /// Constraint id to match (constraint trigger).
        #[arg(long)]
        constraint: Option<String>,
        /// Severity filter: `block` | `warn` | `allow`.
        #[arg(long)]
        severity: Option<String>,
    },
    /// List configured alert rules.
    List,
    /// Remove a rule by id (or id prefix).
    Remove { id: String },
    /// Send a synthetic test ping to all configured webhooks.
    Test,
}

pub fn run_alert(cmd: AlertCommands) -> Result<()> {
    match cmd {
        AlertCommands::Add {
            webhook,
            on,
            threshold,
            constraint,
            severity,
        } => add(webhook, on, threshold, constraint, severity),
        AlertCommands::List => list(),
        AlertCommands::Remove { id } => remove(id),
        AlertCommands::Test => test(),
    }
}

fn parse_trigger(s: &str) -> Result<AlertTrigger> {
    match s {
        "violation" | "violations" => Ok(AlertTrigger::Violation),
        "constraint" => Ok(AlertTrigger::Constraint),
        "severity" => Ok(AlertTrigger::Severity),
        other => bail!(
            "Unknown --on value: {} (expected: violation | constraint | severity)",
            other
        ),
    }
}

fn add(
    webhook: String,
    on: String,
    threshold: usize,
    constraint: Option<String>,
    severity: Option<String>,
) -> Result<()> {
    let trigger = parse_trigger(&on)?;

    if matches!(trigger, AlertTrigger::Constraint) && constraint.is_none() {
        bail!("--constraint is required when --on constraint");
    }
    if matches!(trigger, AlertTrigger::Severity) && severity.is_none() {
        bail!("--severity is required when --on severity");
    }

    let rule = alerting::add_rule(webhook, trigger, threshold, constraint, severity)?;
    println!("✓ Added alert rule {}", rule.id);
    println!("  webhook:  {}", rule.webhook);
    println!("  trigger:  {}", rule.trigger.as_str());
    if let Some(c) = &rule.constraint {
        println!("  constraint: {}", c);
    }
    if let Some(s) = &rule.severity {
        println!("  severity: {}", s);
    }
    if matches!(rule.trigger, AlertTrigger::Violation) {
        println!("  threshold: {}", rule.threshold);
    }
    Ok(())
}

fn list() -> Result<()> {
    let rules = alerting::read_rules()?;
    if rules.is_empty() {
        println!("No alert rules configured.");
        println!("Add one with: rigor alert add --webhook <URL> --on violation --threshold 3");
        return Ok(());
    }

    let color = std::io::stdout().is_terminal();
    println!("Configured alert rules ({}):", rules.len());
    println!();
    for r in &rules {
        let id = if color {
            format!("\x1b[1;36m{}\x1b[0m", r.id)
        } else {
            r.id.clone()
        };
        println!("  {} [{}]", id, r.trigger.as_str());
        println!("    webhook:   {}", r.webhook);
        if let Some(c) = &r.constraint {
            println!("    constraint: {}", c);
        }
        if let Some(s) = &r.severity {
            println!("    severity:  {}", s);
        }
        if matches!(r.trigger, AlertTrigger::Violation) {
            println!("    threshold: {}", r.threshold);
        }
        if let Some(ts) = &r.created_at {
            println!("    created:   {}", ts);
        }
        println!();
    }
    Ok(())
}

fn remove(id: String) -> Result<()> {
    let removed = alerting::remove_rule(&id)?;
    if removed {
        println!("✓ Removed alert rule matching '{}'", id);
    } else {
        println!("No alert rule matched '{}'", id);
    }
    Ok(())
}

fn test() -> Result<()> {
    let rules = alerting::read_rules()?;
    if rules.is_empty() {
        println!("No alert rules configured — nothing to test.");
        return Ok(());
    }

    // Run the async send on a throwaway single-thread runtime.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let ok = rt.block_on(alerting::send_test())?;
    println!("✓ Test pings delivered: {}/{}", ok, rules.len());
    Ok(())
}
