//! `rigor setup` — interactive, non-engineer-friendly onboarding wizard.
//!
//! Walks the user through four steps:
//!
//!   1. Daemon check            — is `rigor serve` running? offer to start it.
//!   2. Project config          — does `rigor.yaml` exist? run `rigor init` if not.
//!   3. OpenCode plugin install — drop `rigor.ts` into `.opencode/plugins/`.
//!   4. Verify end-to-end       — hit `/health` on the daemon.
//!
//! Each step prints a short status line with a trailing "✓" on success or
//! a "✗" with a concrete remediation hint on failure. Nothing in this file
//! panics; every failure is converted to a printed warning and the wizard
//! continues so the user can see every issue in one run.
//!
//! This command is intentionally chatty — it targets users who have never
//! touched a terminal. Engineers who want scripting should keep using
//! `rigor serve --background` + `rigor init` directly.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;

/// Content of the OpenCode plugin that ships with rigor. Embedded at build
/// time so `rigor setup` works even when invoked from a directory that
/// doesn't have a checkout of the repo.
const RIGOR_OPENCODE_PLUGIN: &str = include_str!("../../../../.opencode/plugins/rigor.ts");

/// Default port used by `rigor serve`. Kept in sync with the clap default
/// for `Commands::Serve`.
const DEFAULT_PORT: u16 = 8787;

/// Entry point wired from `cli/mod.rs`.
pub fn run_setup() -> Result<()> {
    println!();
    println!("Welcome to rigor — epistemic constraint enforcement for AI agents.");
    println!();

    let port = DEFAULT_PORT;

    step_one_daemon(port)?;
    let project_dir = step_two_project()?;
    step_three_plugin(&project_dir)?;
    step_four_verify(port)?;

    println!();
    println!("Setup complete! Open a new OpenCode session and your LLM traffic");
    println!("will automatically flow through rigor.");
    println!();
    Ok(())
}

// ─── Step 1: daemon ──────────────────────────────────────────────────────

fn step_one_daemon(port: u16) -> Result<()> {
    println!("Step 1/4: Install rigor daemon");

    if is_daemon_running(port) {
        println!("  rigor serve is already running ✓");
        return Ok(());
    }

    println!("  rigor serve is not running.");
    if !prompt_yes_no("  Start it in the background now?", true) {
        println!("  Skipping daemon start. Run `rigor serve --background` when ready.");
        return Ok(());
    }

    match start_daemon_background() {
        Ok(()) => {
            // Poll briefly for /health so the user sees a real confirmation.
            if wait_for_daemon(port, Duration::from_secs(5)) {
                println!("  rigor serve started ✓");
            } else {
                println!(
                    "  rigor serve was launched but did not respond within 5s — \
                     check `rigor logs` for details."
                );
            }
        }
        Err(e) => {
            println!("  Failed to start rigor serve: {}", e);
            println!("  You can start it manually with: rigor serve --background");
        }
    }

    Ok(())
}

/// Fork-exec `rigor serve --background`. We re-invoke the current binary
/// rather than calling `crate::cli::serve::run_serve` directly because the
/// background mode does its own daemonize/fork dance and we want clean
/// process isolation.
fn start_daemon_background() -> Result<()> {
    let exe = std::env::current_exe()?;
    let status = std::process::Command::new(&exe)
        .arg("serve")
        .arg("--background")
        .status()?;
    if !status.success() {
        anyhow::bail!("`rigor serve --background` exited with {}", status);
    }
    Ok(())
}

fn is_daemon_running(port: u16) -> bool {
    health_check(port, Duration::from_millis(300))
}

fn wait_for_daemon(port: u16, total: Duration) -> bool {
    let deadline = std::time::Instant::now() + total;
    while std::time::Instant::now() < deadline {
        if health_check(port, Duration::from_millis(250)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

/// Blocking HTTP GET to `http://127.0.0.1:<port>/health`. Returns true
/// only on a 2xx response.
fn health_check(port: u16, timeout: Duration) -> bool {
    let url = format!("http://127.0.0.1:{}/health", port);
    match reqwest::blocking::Client::builder()
        .timeout(timeout)
        // The daemon may serve /health over either HTTP or HTTPS depending
        // on the listener; we talk plain HTTP because /health is mounted on
        // the unencrypted admin router.
        .no_proxy()
        .build()
    {
        Ok(client) => match client.get(&url).send() {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        },
        Err(_) => false,
    }
}

// ─── Step 2: project config ──────────────────────────────────────────────

fn step_two_project() -> Result<PathBuf> {
    println!("Step 2/4: Detect project");

    let cwd = std::env::current_dir()?;
    let yaml_path = cwd.join("rigor.yaml");

    if yaml_path.exists() {
        let count = count_constraints(&yaml_path);
        println!(
            "  Found rigor.yaml in current directory ✓"
        );
        println!("  {} constraints loaded", count);
        return Ok(cwd);
    }

    println!("  No rigor.yaml in current directory.");
    if prompt_yes_no("  Generate one now with `rigor init`?", true) {
        if let Err(e) = super::init::run_init(Some(cwd.clone()), false) {
            println!("  `rigor init` failed: {}", e);
            println!("  You can run it manually later with: rigor init");
        } else if yaml_path.exists() {
            let count = count_constraints(&yaml_path);
            println!("  {} constraints loaded", count);
        }
    } else {
        println!("  Skipping. Run `rigor init` when you're ready.");
    }

    Ok(cwd)
}

/// Count constraints declared in `rigor.yaml` by summing belief +
/// justification + defeater entries. Falls back to zero on parse errors —
/// this is display-only so strict accuracy isn't worth erroring out.
fn count_constraints(path: &Path) -> usize {
    match crate::constraint::loader::load_rigor_config(path) {
        Ok(cfg) => cfg.all_constraints().len(),
        Err(_) => 0,
    }
}

// ─── Step 3: OpenCode plugin ─────────────────────────────────────────────

fn step_three_plugin(project_dir: &Path) -> Result<()> {
    println!("Step 3/4: Install OpenCode plugin");

    let plugins_dir = project_dir.join(".opencode").join("plugins");
    let plugin_path = plugins_dir.join("rigor.ts");

    if let Err(e) = std::fs::create_dir_all(&plugins_dir) {
        println!(
            "  Could not create {}: {}",
            plugins_dir.display(),
            e
        );
        return Ok(());
    }

    // Only overwrite if missing or content has changed, so we don't stomp
    // on user-local edits unnecessarily.
    let needs_write = match std::fs::read_to_string(&plugin_path) {
        Ok(existing) => existing != RIGOR_OPENCODE_PLUGIN,
        Err(_) => true,
    };

    if needs_write {
        if let Err(e) = std::fs::write(&plugin_path, RIGOR_OPENCODE_PLUGIN) {
            println!("  Failed to write {}: {}", plugin_path.display(), e);
            return Ok(());
        }
        println!(
            "  Plugin installed to {} ✓",
            relative_display(project_dir, &plugin_path)
        );
    } else {
        println!(
            "  Plugin already up to date at {} ✓",
            relative_display(project_dir, &plugin_path)
        );
    }

    Ok(())
}

fn relative_display(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

// ─── Step 4: verify ──────────────────────────────────────────────────────

fn step_four_verify(port: u16) -> Result<()> {
    println!("Step 4/4: Verify connection");
    print!("  Sending test request through rigor... ");
    let _ = io::stdout().flush();

    if health_check(port, Duration::from_secs(2)) {
        println!("✓");
        println!(
            "  Dashboard available at http://127.0.0.1:{}",
            port
        );
    } else {
        println!("✗");
        println!(
            "  Could not reach http://127.0.0.1:{}/health. \
             Run `rigor serve --background` and try again.",
            port
        );
    }

    Ok(())
}

// ─── Prompt helper ───────────────────────────────────────────────────────

/// Minimal Y/n prompt — falls back to `default` if stdin isn't a TTY or the
/// user just hits enter. We don't pull in `dialoguer` because `rigor setup`
/// should keep its dependency footprint tiny.
fn prompt_yes_no(question: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    print!("{} {} ", question, hint);
    let _ = io::stdout().flush();

    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) => default_yes, // EOF (non-interactive) — use default
        Ok(_) => {
            let trimmed = line.trim().to_ascii_lowercase();
            if trimmed.is_empty() {
                default_yes
            } else {
                matches!(trimmed.as_str(), "y" | "yes")
            }
        }
        Err(_) => default_yes,
    }
}
