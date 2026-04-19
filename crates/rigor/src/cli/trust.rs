//! `rigor trust <tool>` — install a wrapper shim so a tool auto-routes through rigor.
//!
//! Creates ~/.rigor/bin/<tool> that sets proxy env vars and execs the real binary.
//! The user adds ~/.rigor/bin to the FRONT of their PATH (we offer to do this).
//!
//!   rigor trust opencode   -> creates ~/.rigor/bin/opencode
//!   rigor trust claude     -> creates ~/.rigor/bin/claude
//!   rigor untrust opencode -> removes ~/.rigor/bin/opencode

use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const SUPPORTED_TOOLS: &[&str] = &["opencode", "claude"];

fn rigor_bin_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let dir = home.join(".rigor/bin");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn find_real_binary(tool: &str) -> Result<String> {
    let rigor_bin = rigor_bin_dir()?;
    let rigor_shim = rigor_bin.join(tool);

    // Search PATH but skip our own shim
    let path_var = std::env::var("PATH").unwrap_or_default();
    for dir in path_var.split(':') {
        let candidate = PathBuf::from(dir).join(tool);
        if candidate == rigor_shim {
            continue; // skip our own wrapper
        }
        if candidate.exists() && candidate.is_file() {
            return Ok(candidate.to_string_lossy().to_string());
        }
    }

    anyhow::bail!(
        "'{}' not found in PATH. Install it first, then run: rigor trust {}",
        tool, tool
    )
}

fn shell_profile_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    // Prefer zshrc on macOS
    let zshrc = home.join(".zshrc");
    if zshrc.exists() {
        return Some(zshrc);
    }
    let bashrc = home.join(".bashrc");
    if bashrc.exists() {
        return Some(bashrc);
    }
    Some(zshrc) // default to creating .zshrc
}

fn is_rigor_bin_in_path() -> bool {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let rigor_bin = dirs::home_dir()
        .map(|h| h.join(".rigor/bin").to_string_lossy().to_string())
        .unwrap_or_default();
    path_var.split(':').any(|p| p == rigor_bin)
}

pub fn install_tool_wrapper(tool: &str) -> Result<()> {
    if !SUPPORTED_TOOLS.contains(&tool) {
        anyhow::bail!(
            "Unknown tool: '{}'. Supported: {}",
            tool,
            SUPPORTED_TOOLS.join(", ")
        );
    }

    let real_binary = find_real_binary(tool)?;
    let bin_dir = rigor_bin_dir()?;
    let wrapper_path = bin_dir.join(tool);

    // Generate the wrapper script
    let wrapper = format!(
        r#"#!/bin/bash
# rigor wrapper for {tool} — auto-routes LLM traffic through rigor proxy.
# Installed by: rigor trust {tool}
# Remove with:  rigor untrust {tool}

# Rigor proxy settings
export HTTPS_PROXY="${{HTTPS_PROXY:-http://127.0.0.1:8787}}"
export HTTP_PROXY="${{HTTP_PROXY:-http://127.0.0.1:8787}}"
export https_proxy="${{https_proxy:-http://127.0.0.1:8787}}"
export http_proxy="${{http_proxy:-http://127.0.0.1:8787}}"
export NO_PROXY="${{NO_PROXY:-localhost,127.0.0.1,::1}}"
export no_proxy="${{no_proxy:-localhost,127.0.0.1,::1}}"

# SDK-specific overrides
export ANTHROPIC_BASE_URL="${{ANTHROPIC_BASE_URL:-http://127.0.0.1:8787}}"
export OPENAI_BASE_URL="${{OPENAI_BASE_URL:-http://127.0.0.1:8787}}"

# Accept rigor's MITM cert
export NODE_TLS_REJECT_UNAUTHORIZED=0

# Session tracking
export OPENCODE_SESSION_ID="${{OPENCODE_SESSION_ID:-$(uuidgen 2>/dev/null || cat /proc/sys/kernel/random/uuid 2>/dev/null || echo rigor-$$)}}"
export RIGOR_ROUTED=1

exec {real_binary} "$@"
"#,
        tool = tool,
        real_binary = real_binary
    );

    fs::write(&wrapper_path, &wrapper)?;
    fs::set_permissions(&wrapper_path, fs::Permissions::from_mode(0o755))?;

    println!("rigor: installed wrapper for '{}'", tool);
    println!("  {} -> {}", wrapper_path.display(), real_binary);

    // Check if ~/.rigor/bin is in PATH
    if !is_rigor_bin_in_path() {
        let rigor_bin_str = bin_dir.to_string_lossy();
        println!();
        println!("rigor: add ~/.rigor/bin to your PATH (before other entries):");
        println!();

        if let Some(profile) = shell_profile_path() {
            let export_line = format!("export PATH=\"{}:$PATH\"", rigor_bin_str);

            // Check if already in profile
            let contents = fs::read_to_string(&profile).unwrap_or_default();
            if contents.contains(".rigor/bin") {
                println!("  Already in {}", profile.display());
            } else {
                // Append to profile
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&profile)?;
                use std::io::Write;
                writeln!(file, "\n# rigor — route AI tools through epistemic constraint proxy")?;
                writeln!(file, "{}", export_line)?;
                println!("  Added to {}", profile.display());
                println!("  Run: source {}", profile.display());
            }
        } else {
            println!("  export PATH=\"{}:$PATH\"", rigor_bin_str);
        }
    }

    println!();
    println!("Now run '{}' normally — traffic routes through rigor.", tool);
    println!("Make sure rigor is running: rigor serve --background");

    Ok(())
}

pub fn remove_tool_wrapper(tool: &str) -> Result<()> {
    let bin_dir = rigor_bin_dir()?;
    let wrapper_path = bin_dir.join(tool);

    if wrapper_path.exists() {
        fs::remove_file(&wrapper_path)?;
        println!("rigor: removed wrapper for '{}'", tool);
        println!("  {} deleted", wrapper_path.display());
        println!("  '{}' now runs directly (no rigor proxy)", tool);
    } else {
        println!("rigor: no wrapper found for '{}' (nothing to remove)", tool);
    }

    Ok(())
}
