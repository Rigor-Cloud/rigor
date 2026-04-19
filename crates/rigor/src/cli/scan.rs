//! `rigor scan` — PII/secrets detector + UserPromptSubmit hook.
//!
//! Four modes selected by mutually-exclusive flags:
//!
//! - default (no flags): read stdin or `--file`, print findings for humans
//! - `--hook`: act as a Claude Code UserPromptSubmit hook — read hook JSON,
//!   scan the user's prompt, emit a hook response, block if secrets found
//! - `--install` / `--uninstall`: register or deregister the hook in
//!   `~/.claude/settings.json` alongside any other hooks
//! - `--status`: report whether the hook is currently installed
//!
//! The hook is intentionally separate from the action-gate hooks and doesn't
//! require the daemon — PII detection is a local-only concern and should work
//! even when rigor-personal isn't running.

use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::daemon::proxy::{detect_pii, is_likely_real_secret, prettify_kind, redact_with_tags};

/// Top-level dispatcher called from CLI parsing. Exactly one of
/// `hook|install|uninstall|status` may be set (clap enforces this via
/// `conflicts_with_all`).
pub fn run_scan(
    file: Option<String>,
    block: bool,
    json: bool,
    hook: bool,
    install: bool,
    uninstall: bool,
    status: bool,
    smart: bool,
) -> Result<()> {
    if install {
        return install_hook();
    }
    if uninstall {
        return uninstall_hook();
    }
    if status {
        return print_status();
    }
    if hook {
        return run_hook_mode(smart);
    }
    run_check(file, block, json, smart)
}

// ---------------------------------------------------------------------------
// Default mode: check stdin/file, print findings
// ---------------------------------------------------------------------------

fn run_check(file: Option<String>, block: bool, json: bool, smart: bool) -> Result<()> {
    let text = match file {
        Some(path) if path == "-" => read_stdin()?,
        Some(path) => std::fs::read_to_string(&path)
            .with_context(|| format!("reading {path}"))?,
        None => read_stdin()?,
    };

    let findings = detect_pii(&text);
    let findings: Vec<_> = if smart {
        findings.into_iter()
            .filter(|(_, m)| is_likely_real_secret(&text, m))
            .collect()
    } else { findings };

    if json {
        let arr: Vec<serde_json::Value> = findings
            .iter()
            .map(|(kind, matched)| {
                serde_json::json!({ "kind": kind, "matched": matched })
            })
            .collect();
        let out = serde_json::json!({
            "count": findings.len(),
            "findings": arr,
        });
        println!("{}", serde_json::to_string(&out)?);
    } else if findings.is_empty() {
        eprintln!("rigor scan: clean (no PII/secrets detected)");
    } else {
        eprintln!("rigor scan: {} finding(s)", findings.len());
        for (kind, matched) in &findings {
            eprintln!("  [{kind}] {}", redact_preview(matched));
        }
    }

    if block && !findings.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Hook mode: UserPromptSubmit protocol
// ---------------------------------------------------------------------------

/// The subset of UserPromptSubmit hook input we actually use. Claude Code
/// sends more fields (session_id, transcript_path, cwd) but we only need
/// the prompt text to scan.
#[derive(Debug, Deserialize)]
struct UserPromptInput {
    prompt: String,
}

fn run_hook_mode(smart: bool) -> Result<()> {
    let raw = read_stdin()?;
    // If stdin isn't the expected hook JSON, fail open (allow the prompt).
    // Better to under-protect than to block the user due to a contract
    // mismatch we might have introduced ourselves.
    let input: UserPromptInput = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            // Emit empty allow response so Claude Code doesn't interpret
            // garbage output as additional context.
            println!("{}", serde_json::json!({}));
            return Ok(());
        }
    };

    let findings = detect_pii(&input.prompt);
    let findings: Vec<(String, String)> = if smart {
        findings.into_iter()
            .filter(|(_, m)| is_likely_real_secret(&input.prompt, m))
            .collect()
    } else { findings };
    if findings.is_empty() {
        // Empty JSON = "no decision, just continue" in Claude Code's hook
        // model. Keeps logs clean — only flagged prompts produce output.
        println!("{}", serde_json::json!({}));
        return Ok(());
    }

    // Build a structured block reason that lets the user:
    //   1. see each finding tagged by type (OpenRouter, AnthropicApiKey, …)
    //   2. see a redacted-preview of the specific value that triggered it
    //   3. copy-paste a fully-sanitized version of their own prompt and
    //      resubmit without retyping
    //
    // Claude Code's UserPromptSubmit hook contract does NOT support mutating
    // the submitted prompt. So "tag + replace" at this layer means: block and
    // show the user what to resubmit. The MITM proxy handles actual in-flight
    // redaction as a second line of defense (commit 8715eb44).
    let redacted_prompt = redact_with_tags(&input.prompt);

    let mut lines = vec![
        format!(
            "Rigor blocked this prompt — {} secret(s) / PII detected.",
            findings.len()
        ),
        String::new(),
        "Found:".to_string(),
    ];
    for (kind, matched) in &findings {
        lines.push(format!(
            "  • [{}] {}",
            prettify_kind(kind),
            redact_preview(matched)
        ));
    }
    lines.push(String::new());
    lines.push("Redacted version (safe to resubmit):".to_string());
    lines.push(format!("  {redacted_prompt}"));
    lines.push(String::new());
    lines.push(
        "To disable this safeguard: `rigor scan --uninstall`.".to_string(),
    );

    let resp = serde_json::json!({
        "decision": "block",
        "reason": lines.join("\n"),
    });
    println!("{resp}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Hook install / uninstall / status — operates on ~/.claude/settings.json
// ---------------------------------------------------------------------------

const HOOK_COMMAND: &str = "rigor scan --hook --smart";
const HOOK_EVENT: &str = "UserPromptSubmit";

fn claude_settings_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME env var not set")?;
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

fn load_settings(path: &PathBuf) -> serde_json::Value {
    let s = std::fs::read_to_string(path).unwrap_or_else(|_| "{}".to_string());
    serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({}))
}

fn is_rigor_scan_cmd(cmd: &str) -> bool {
    // Match any command that starts with "rigor scan" so variants like
    // `rigor scan --hook` or `rigor scan --hook --block` all round-trip.
    cmd.trim_start().starts_with("rigor scan")
}

fn install_hook() -> Result<()> {
    let path = claude_settings_path()?;
    let mut settings = load_settings(&path);

    if !settings.get("hooks").map(|h| h.is_object()).unwrap_or(false) {
        settings["hooks"] = serde_json::json!({});
    }
    let hooks = settings["hooks"].as_object_mut().unwrap();

    let arr = hooks
        .entry(HOOK_EVENT.to_string())
        .or_insert_with(|| serde_json::json!([]));

    let Some(arr) = arr.as_array_mut() else {
        anyhow::bail!("settings.hooks.{HOOK_EVENT} is not an array");
    };

    // Drop any previous rigor scan entries so we don't stack duplicates.
    arr.retain(|entry| {
        let has_rigor = entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|hs| {
                hs.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .map(is_rigor_scan_cmd)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        !has_rigor
    });

    arr.push(serde_json::json!({
        "hooks": [{ "type": "command", "command": HOOK_COMMAND }]
    }));

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&settings)?)?;
    eprintln!(
        "rigor: installed UserPromptSubmit scan hook in {}",
        path.display()
    );
    Ok(())
}

fn uninstall_hook() -> Result<()> {
    let path = claude_settings_path()?;
    let mut settings = load_settings(&path);

    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        eprintln!("rigor: no hooks configured in {}", path.display());
        return Ok(());
    };

    let mut removed = 0usize;
    if let Some(arr) = hooks
        .get_mut(HOOK_EVENT)
        .and_then(|v| v.as_array_mut())
    {
        let before = arr.len();
        arr.retain(|entry| {
            let has_rigor = entry
                .get("hooks")
                .and_then(|h| h.as_array())
                .map(|hs| {
                    hs.iter().any(|h| {
                        h.get("command")
                            .and_then(|c| c.as_str())
                            .map(is_rigor_scan_cmd)
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            !has_rigor
        });
        removed += before - arr.len();

        if arr.is_empty() {
            hooks.remove(HOOK_EVENT);
        }
    }

    if hooks.is_empty() {
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("hooks");
        }
    }

    std::fs::write(&path, serde_json::to_string_pretty(&settings)?)?;
    if removed > 0 {
        eprintln!(
            "rigor: removed {} scan hook entry/entries from {}",
            removed,
            path.display()
        );
    } else {
        eprintln!("rigor: no scan hooks found in {}", path.display());
    }
    Ok(())
}

fn print_status() -> Result<()> {
    let path = claude_settings_path()?;
    let settings = load_settings(&path);

    let installed = settings
        .get("hooks")
        .and_then(|h| h.as_object())
        .and_then(|o| o.get(HOOK_EVENT))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().any(|entry| {
                entry
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|hs| {
                        hs.iter().any(|h| {
                            h.get("command")
                                .and_then(|c| c.as_str())
                                .map(is_rigor_scan_cmd)
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    println!("rigor scan hook ({}):", path.display());
    println!(
        "  UserPromptSubmit (scan prompts for PII/secrets before send): {}",
        if installed { "enabled" } else { "disabled" }
    );
    println!();
    println!("Use `rigor scan --install` to enable, `rigor scan --uninstall` to disable.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading stdin")?;
    Ok(buf)
}

/// Show first 4 and last 4 chars of a secret with middle elided.
fn redact_preview(s: &str) -> String {
    let s = s.trim();
    if s.len() <= 12 {
        "*".repeat(s.len())
    } else {
        format!("{}...{}", &s[..4], &s[s.len() - 4..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_preview_short_values_fully_starred() {
        assert_eq!(redact_preview("abc"), "***");
        assert_eq!(redact_preview("twelve-chars"), "************");
    }

    #[test]
    fn redact_preview_long_values_keep_head_and_tail() {
        let s = "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";
        let out = redact_preview(s);
        assert!(out.starts_with("sk-o"));
        assert!(out.ends_with("e575"));
        assert!(out.contains("..."));
        assert!(out.len() < s.len());
    }

    #[test]
    fn is_rigor_scan_cmd_matches_bare_and_flagged() {
        assert!(is_rigor_scan_cmd("rigor scan"));
        assert!(is_rigor_scan_cmd("rigor scan --hook"));
        assert!(is_rigor_scan_cmd("rigor scan --hook --json"));
        assert!(is_rigor_scan_cmd("  rigor scan  "));
        assert!(!is_rigor_scan_cmd("rigor gate pre-tool"));
        assert!(!is_rigor_scan_cmd("node /some/other/hook.js"));
    }
}
