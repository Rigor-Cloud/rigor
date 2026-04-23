//! `rigor gate` CLI subcommands.
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;

const DAEMON_URL: &str = "http://127.0.0.1:8787";

#[derive(Deserialize)]
struct ToolInput {
    tool_name: String,
    tool_input: serde_json::Value,
    #[serde(default)]
    session_id: Option<String>,
}

pub fn run_gate(subcommand: &str) -> Result<()> {
    match subcommand {
        "pre-tool" => run_pre_tool(),
        "post-tool" => run_post_tool(),
        "install-hook" => install_hook(),
        "uninstall-hook" | "remove-hook" | "disable" => uninstall_hook(),
        "status" | "hook-status" => hook_status(),
        _ => anyhow::bail!(
            "Unknown gate subcommand: {}\n\
             Available: pre-tool | post-tool | install-hook | uninstall-hook | status",
            subcommand
        ),
    }
}

fn read_stdin_json() -> Result<ToolInput> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    Ok(serde_json::from_str(&buf)?)
}

fn derive_session_id() -> String {
    // Fallback session ID when stdin JSON is missing a `session_id`.
    // Checks Claude Code, OpenCode, and generic session ID env vars.
    std::env::var("CLAUDE_CODE_SESSION_ID")
        .or_else(|_| std::env::var("CLAUDE_SESSION_ID"))
        .or_else(|_| std::env::var("OPENCODE_SESSION_ID"))
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "no-session".to_string())
}

fn run_pre_tool() -> Result<()> {
    // Fix 1 (2026-04-15): Gate is opt-in per session. If `RIGOR_GATE_ENABLED=1`
    // is not set, this hook is a no-op even when the daemon is running. Without
    // this check the hook would participate in EVERY Claude Code session on the
    // machine the moment a rigor daemon existed, silently reverting or deleting
    // files on sessions the user never intended to gate.
    if !is_gate_session_enabled() {
        return Ok(());
    }
    // No-op if no rigor daemon is running. Rigor hooks require an active
    // `rigor-personal` session; outside of one, the gate has nothing to
    // register against and should not touch the working tree.
    if !crate::daemon::daemon_alive() {
        return Ok(());
    }
    let input = read_stdin_json().unwrap_or(ToolInput {
        tool_name: "unknown".to_string(),
        tool_input: serde_json::json!({}),
        session_id: None,
    });
    let session_id = input.session_id.unwrap_or_else(derive_session_id);

    let affected_paths: Vec<String> = match input.tool_name.as_str() {
        "Edit" | "Write" => input
            .tool_input
            .get("file_path")
            .and_then(|p| p.as_str())
            .map(|s| vec![s.to_string()])
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    // Fix 3 (2026-04-15): Refuse to engage the gate when any affected path has
    // uncommitted changes. Pre-tool stashes the path, post-tool's "approved"
    // branch drops the stash — if the user had unsaved work there, drop_stash
    // permanently loses it (silent data loss on the happy path). Safer to skip
    // the gate entirely in that case and let the tool proceed untouched.
    if !affected_paths.is_empty() {
        let cwd = std::env::current_dir().unwrap_or_default();
        let dirty = affected_paths_dirty_in(&cwd, &affected_paths);
        if !dirty.is_empty() {
            eprintln!(
                "rigor gate: affected path(s) have uncommitted changes, \
                 skipping gate to avoid data loss: {:?}",
                dirty
            );
            return Ok(());
        }
    }

    // Decide whether to stash: only when there's a concrete path inside a git
    // repo. We stash ONLY the affected paths (not the whole working tree) so
    // unrelated manual work-in-progress is left alone.
    let want_stash = !affected_paths.is_empty() && is_git_repo();
    let stash_msg = format!("rigor-gate-{}", uuid::Uuid::new_v4());
    let snapshot_id = if want_stash {
        let mut args: Vec<String> = vec![
            "stash".into(),
            "push".into(),
            "--include-untracked".into(),
            "-m".into(),
            stash_msg.clone(),
            "--".into(),
        ];
        args.extend(affected_paths.iter().cloned());
        let output = Command::new("git").args(&args).output();
        match output {
            Ok(o) if o.status.success() => stash_msg.clone(),
            _ => "no-stash".to_string(),
        }
    } else {
        "no-stash".to_string()
    };

    // Register with the daemon. If the daemon is unreachable (connection
    // refused, timeout, or non-2xx) we MUST undo the stash we just created —
    // otherwise it sits orphaned forever because post-tool will also fail
    // and never call drop_stash. This was the root cause of the ~22 stashes
    // that accumulated in contrapunk during the pre-fix stall.
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;
    let body = serde_json::json!({
        "session_id": session_id,
        "snapshot_id": snapshot_id,
        "tool_name": input.tool_name,
        "affected_paths": affected_paths,
    });
    let reg = client
        .post(format!("{}/api/gate/register-snapshot", DAEMON_URL))
        .json(&body)
        .send();

    let daemon_alive = reg
        .as_ref()
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    if !daemon_alive && snapshot_id != "no-stash" {
        // Daemon didn't take the registration. Put the working tree back the
        // way we found it so no orphan stash remains.
        pop_stash_by_message(&stash_msg);
    }

    Ok(())
}

fn run_post_tool() -> Result<()> {
    // Fix 1 (2026-04-15): Gate is opt-in per session. Mirror of the pre-tool
    // check — if `RIGOR_GATE_ENABLED=1` is not set, post-tool is a no-op and
    // there is no 60-second decision-poll to wait on for unrelated sessions.
    if !is_gate_session_enabled() {
        return Ok(());
    }
    // No-op if no rigor daemon is running. Without a daemon there's no
    // decision to poll for; pre-tool also would have short-circuited so
    // there's nothing to reconcile here either.
    if !crate::daemon::daemon_alive() {
        return Ok(());
    }
    // Previously discarded stdin and fell through to derive_session_id(),
    // which returned "no-session" for every event. Pre-tool snapshots were
    // registered under the real session ID, so this poll never matched and
    // stalled the full 60-second window on every tool call.
    let input = read_stdin_json().unwrap_or(ToolInput {
        tool_name: "unknown".to_string(),
        tool_input: serde_json::json!({}),
        session_id: None,
    });
    let session_id = input
        .session_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(derive_session_id);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(65))
        .build()?;
    let _ = client
        .post(format!("{}/api/gate/tool-completed", DAEMON_URL))
        .json(&serde_json::json!({"session_id": session_id}))
        .send();

    // Poll for a decision up to 60 seconds. If we see three consecutive
    // connection errors we stop early — the daemon is plainly not running,
    // so there's no point polling a dead endpoint for the full minute.
    // (Pre-tool's fail-fast means no stash should exist in this case either.)
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let mut consecutive_conn_errors = 0u32;
    loop {
        let resp = client
            .get(format!("{}/api/gate/decision/{}", DAEMON_URL, session_id))
            .send();
        match resp {
            Ok(r) => {
                consecutive_conn_errors = 0;
                if let Ok(json) = r.json::<serde_json::Value>() {
                    let status = json
                        .get("status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("pending");
                    match status {
                        "approved" => {
                            if let Some(snap_id) = json.get("snapshot_id").and_then(|s| s.as_str())
                            {
                                drop_stash(snap_id);
                            }
                            eprintln!("rigor gate: approved");
                            return Ok(());
                        }
                        "no_session" => {
                            // Fix 4 (2026-04-15): Daemon has no record of this
                            // session (pre-tool either didn't register or was
                            // skipped via the opt-in gate). Exit immediately
                            // rather than poll a dead-end endpoint for 60s.
                            return Ok(());
                        }
                        "rejected" => {
                            let paths: Vec<String> = json
                                .get("affected_paths")
                                .and_then(|p| p.as_array())
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .unwrap_or_default();
                            // Fix 2 (2026-04-15): `git checkout HEAD -- <path>`
                            // DELETES untracked new files (since they don't
                            // exist at HEAD). For those paths, explicitly
                            // remove the file. For tracked files, checkout
                            // HEAD restores to committed state as before.
                            let cwd = std::env::current_dir().unwrap_or_default();
                            for p in &paths {
                                if path_existed_at_head_in(&cwd, p) {
                                    let _ = Command::new("git")
                                        .args(["checkout", "HEAD", "--", p])
                                        .output();
                                } else {
                                    let _ = std::fs::remove_file(p);
                                }
                            }
                            if let Some(snap_id) = json.get("snapshot_id").and_then(|s| s.as_str())
                            {
                                drop_stash(snap_id);
                            }
                            eprintln!("rigor gate: rejected — reverted {} files", paths.len());
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }
            Err(_) => {
                consecutive_conn_errors += 1;
                if consecutive_conn_errors >= 3 {
                    eprintln!("rigor gate: daemon unreachable, skipping decision");
                    return Ok(());
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            eprintln!("rigor gate: no decision within 60s");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn drop_stash(stash_ref: &str) {
    if let Ok(out) = Command::new("git").args(["stash", "list"]).output() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            if line.contains(stash_ref) {
                if let Some(idx) = line.split(':').next() {
                    let _ = Command::new("git").args(["stash", "drop", idx]).output();
                    return;
                }
            }
        }
    }
}

/// Restore a stash created by pre-tool back into the working tree, then drop
/// it. Used for pre-tool fail-fast: if we couldn't register with the daemon,
/// we put the user's changes back where we found them so no orphan stash is
/// left behind.
fn pop_stash_by_message(stash_ref: &str) {
    if let Ok(out) = Command::new("git").args(["stash", "list"]).output() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            if line.contains(stash_ref) {
                if let Some(idx) = line.split(':').next() {
                    // `pop` applies + drops atomically. If the apply fails
                    // (conflict), stash is preserved by git — that's the
                    // safer failure mode than losing work.
                    let _ = Command::new("git").args(["stash", "pop", idx]).output();
                    return;
                }
            }
        }
    }
}

fn is_git_repo() -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn install_hook() -> Result<()> {
    let settings_path = claude_settings_path()?;
    let settings_str = std::fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".to_string());
    let mut settings: serde_json::Value =
        serde_json::from_str(&settings_str).unwrap_or_else(|_| serde_json::json!({}));

    // Ensure settings.hooks is an object (preserve existing keys like SessionStart, Stop, etc.)
    if !settings
        .get("hooks")
        .map(|h| h.is_object())
        .unwrap_or(false)
    {
        settings["hooks"] = serde_json::json!({});
    }

    // Helper: add our rigor hook entry to an array, avoiding duplicates.
    // Rigor entries are matched by the command string starting with "rigor gate".
    let add_rigor_entry = |arr: &mut Vec<serde_json::Value>, matcher: &str, command: &str| {
        // Remove any existing rigor gate entries to avoid duplicates/stale commands
        arr.retain(|entry| {
            let hooks = entry.get("hooks").and_then(|h| h.as_array());
            let has_rigor = hooks
                .map(|hs| {
                    hs.iter().any(|h| {
                        h.get("command")
                            .and_then(|c| c.as_str())
                            .map(|s| s.starts_with("rigor gate"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            !has_rigor
        });
        arr.push(serde_json::json!({
            "matcher": matcher,
            "hooks": [{"type": "command", "command": command}]
        }));
    };

    let hooks_obj = settings["hooks"].as_object_mut().unwrap();

    // PreToolUse
    let pre_arr = hooks_obj
        .entry("PreToolUse".to_string())
        .or_insert_with(|| serde_json::json!([]));
    if let Some(arr) = pre_arr.as_array_mut() {
        add_rigor_entry(arr, "Edit|Write|Bash", "rigor gate pre-tool");
    }

    // PostToolUse
    let post_arr = hooks_obj
        .entry("PostToolUse".to_string())
        .or_insert_with(|| serde_json::json!([]));
    if let Some(arr) = post_arr.as_array_mut() {
        add_rigor_entry(arr, "Edit|Write|Bash", "rigor gate post-tool");
    }

    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
    eprintln!(
        "rigor: installed action gate hooks in {} (merged with existing hooks)",
        settings_path.display()
    );
    Ok(())
}

/// Remove rigor's PreToolUse / PostToolUse action-gate hooks from the Claude
/// Code settings file. Other hooks (GSD plugins, Stop hooks, SessionStart,
/// etc.) are preserved. Idempotent — safe to run when no rigor hooks exist.
pub fn uninstall_hook() -> Result<()> {
    let settings_path = claude_settings_path()?;
    let settings_str = std::fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".to_string());
    let mut settings: serde_json::Value =
        serde_json::from_str(&settings_str).unwrap_or_else(|_| serde_json::json!({}));

    let hooks_obj = match settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        Some(o) => o,
        None => {
            eprintln!("rigor: no hooks configured in {}", settings_path.display());
            return Ok(());
        }
    };

    let mut removed = 0usize;
    for key in ["PreToolUse", "PostToolUse"] {
        if let Some(arr) = hooks_obj.get_mut(key).and_then(|v| v.as_array_mut()) {
            let before = arr.len();
            arr.retain(|entry| {
                let hooks = entry.get("hooks").and_then(|h| h.as_array());
                let has_rigor = hooks
                    .map(|hs| {
                        hs.iter().any(|h| {
                            h.get("command")
                                .and_then(|c| c.as_str())
                                .map(|s| s.starts_with("rigor gate"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false);
                !has_rigor
            });
            removed += before - arr.len();
        }
    }

    // Prune empty arrays (cleaner settings file)
    for key in ["PreToolUse", "PostToolUse"] {
        let is_empty = hooks_obj
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(false);
        if is_empty {
            hooks_obj.remove(key);
        }
    }

    // Prune empty hooks object
    let hooks_empty = hooks_obj.is_empty();
    if hooks_empty {
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("hooks");
        }
    }

    std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
    if removed > 0 {
        eprintln!(
            "rigor: removed {} action gate hook(s) from {}",
            removed,
            settings_path.display()
        );
    } else {
        eprintln!(
            "rigor: no action gate hooks found in {}",
            settings_path.display()
        );
    }
    Ok(())
}

/// Print whether rigor's PreToolUse / PostToolUse action-gate hooks are
/// currently installed in the Claude Code settings file.
pub fn hook_status() -> Result<()> {
    let settings_path = claude_settings_path()?;
    let settings_str = std::fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".to_string());
    let settings: serde_json::Value =
        serde_json::from_str(&settings_str).unwrap_or_else(|_| serde_json::json!({}));

    let check = |key: &str| -> bool {
        settings
            .get("hooks")
            .and_then(|h| h.as_object())
            .and_then(|o| o.get(key))
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
                                    .map(|s| s.starts_with("rigor gate"))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    };

    let pre = check("PreToolUse");
    let post = check("PostToolUse");

    println!("rigor action gate hooks ({}):", settings_path.display());
    println!(
        "  PreToolUse  (stash snapshot before Edit|Write|Bash): {}",
        if pre { "enabled" } else { "disabled" }
    );
    println!(
        "  PostToolUse (await decision, revert if rejected):    {}",
        if post { "enabled" } else { "disabled" }
    );
    println!();
    println!("Use `rigor gate install-hook` to enable, `rigor gate uninstall-hook` to disable.");
    Ok(())
}

fn claude_settings_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")?; // rigor-home-ok
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

// ============================================================================
// Gate bug hotfix (2026-04-15) — pure helpers with unit tests.
// See CONCERNS.md "Action-Gate Scoped Stash Mechanism Can Lose Working-Tree Data"
// and commit bef9cd19 for diagnosis context.
// ============================================================================

/// Pure: decide if the gate should participate in this session based on the
/// RIGOR_GATE_ENABLED environment variable. Takes an env-reader closure so the
/// test suite can inject arbitrary env state without mutating process globals.
fn is_gate_session_enabled_from<F: Fn(&str) -> Option<String>>(env_read: F) -> bool {
    env_read("RIGOR_GATE_ENABLED").as_deref() == Some("1")
}

fn is_gate_session_enabled() -> bool {
    is_gate_session_enabled_from(|k| std::env::var(k).ok())
}

/// Returns true if `path` exists at HEAD in the git repo rooted at `repo`.
/// Used to decide whether the "rejected" revert branch can safely use
/// `git checkout HEAD -- <path>` (which deletes untracked new files).
fn path_existed_at_head_in(repo: &std::path::Path, path: &str) -> bool {
    std::process::Command::new("git")
        .current_dir(repo)
        .args(["cat-file", "-e", &format!("HEAD:{}", path)])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[allow(dead_code)]
fn path_existed_at_head(path: &str) -> bool {
    path_existed_at_head_in(&std::env::current_dir().unwrap_or_default(), path)
}

/// Pure: subset of `paths` that have uncommitted changes (tracked modified
/// or untracked) in the repo rooted at `repo`. Pre-tool refuses to engage
/// the gate when any affected path is dirty, since stashing and dropping
/// user's pre-edit work is silent data loss.
fn affected_paths_dirty_in(repo: &std::path::Path, paths: &[String]) -> Vec<String> {
    let mut dirty = Vec::new();
    for p in paths {
        let output = std::process::Command::new("git")
            .current_dir(repo)
            .args(["status", "--porcelain", "--", p])
            .output();
        if let Ok(o) = output {
            if !o.stdout.is_empty() {
                dirty.push(p.clone());
            }
        }
    }
    dirty
}

#[allow(dead_code)]
fn affected_paths_dirty(paths: &[String]) -> Vec<String> {
    affected_paths_dirty_in(&std::env::current_dir().unwrap_or_default(), paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn setup_git_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        Command::new("git")
            .current_dir(repo)
            .args(["init", "-q"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "commit.gpgsign", "false"])
            .output()
            .unwrap();
        tmp
    }

    fn commit_file(repo: &std::path::Path, name: &str, content: &str) {
        std::fs::write(repo.join(name), content).unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["add", name])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["commit", "-q", "-m", "fixture"])
            .output()
            .unwrap();
    }

    // ---- Fix 1: env var opt-in ----

    #[test]
    fn gate_disabled_when_env_var_unset() {
        assert!(!is_gate_session_enabled_from(|_| None));
    }

    #[test]
    fn gate_enabled_when_env_var_is_one() {
        assert!(is_gate_session_enabled_from(|k| {
            if k == "RIGOR_GATE_ENABLED" {
                Some("1".to_string())
            } else {
                None
            }
        }));
    }

    #[test]
    fn gate_disabled_when_env_var_is_not_one() {
        for val in &["true", "yes", "0", "", "2"] {
            let v = (*val).to_string();
            assert!(
                !is_gate_session_enabled_from(|k| {
                    if k == "RIGOR_GATE_ENABLED" {
                        Some(v.clone())
                    } else {
                        None
                    }
                }),
                "should be disabled for value {:?}",
                val
            );
        }
    }

    // ---- Fix 2: path_existed_at_head for safe revert ----

    #[test]
    fn path_existed_at_head_true_for_tracked_file() {
        let tmp = setup_git_repo();
        commit_file(tmp.path(), "foo.md", "hello");
        assert!(path_existed_at_head_in(tmp.path(), "foo.md"));
    }

    #[test]
    fn path_existed_at_head_false_for_untracked_new_file() {
        let tmp = setup_git_repo();
        commit_file(tmp.path(), "existing.md", "hello");
        std::fs::write(tmp.path().join("new.md"), "brand new").unwrap();
        assert!(!path_existed_at_head_in(tmp.path(), "new.md"));
    }

    #[test]
    fn path_existed_at_head_false_on_empty_repo() {
        let tmp = setup_git_repo();
        std::fs::write(tmp.path().join("foo.md"), "no commits yet").unwrap();
        assert!(!path_existed_at_head_in(tmp.path(), "foo.md"));
    }

    // ---- Fix 3: affected_paths_dirty detects uncommitted changes ----

    #[test]
    fn affected_paths_dirty_empty_on_clean_repo() {
        let tmp = setup_git_repo();
        commit_file(tmp.path(), "foo.md", "clean");
        let dirty = affected_paths_dirty_in(tmp.path(), &["foo.md".to_string()]);
        assert!(dirty.is_empty(), "expected empty, got {:?}", dirty);
    }

    #[test]
    fn affected_paths_dirty_detects_modified_tracked_file() {
        let tmp = setup_git_repo();
        commit_file(tmp.path(), "foo.md", "v1");
        std::fs::write(tmp.path().join("foo.md"), "v2 unsaved").unwrap();
        let dirty = affected_paths_dirty_in(tmp.path(), &["foo.md".to_string()]);
        assert_eq!(dirty, vec!["foo.md".to_string()]);
    }

    #[test]
    fn affected_paths_dirty_detects_untracked_new_file() {
        let tmp = setup_git_repo();
        commit_file(tmp.path(), "existing.md", "hello");
        std::fs::write(tmp.path().join("new.md"), "untracked").unwrap();
        let dirty = affected_paths_dirty_in(tmp.path(), &["new.md".to_string()]);
        assert_eq!(dirty, vec!["new.md".to_string()]);
    }

    #[test]
    fn affected_paths_dirty_ignores_unrelated_dirty_paths() {
        let tmp = setup_git_repo();
        commit_file(tmp.path(), "foo.md", "clean");
        commit_file(tmp.path(), "bar.md", "clean");
        std::fs::write(tmp.path().join("bar.md"), "bar is dirty").unwrap();
        // Only querying about foo.md; bar's dirtiness is irrelevant
        let dirty = affected_paths_dirty_in(tmp.path(), &["foo.md".to_string()]);
        assert!(dirty.is_empty(), "foo should be clean, got {:?}", dirty);
    }
}
