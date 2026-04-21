//! `rigor trust <tool>` — install a wrapper shim so a tool auto-routes through rigor.
//!
//! Creates ~/.rigor/bin/<tool> that sets proxy env vars and execs the real binary.
//! The user adds ~/.rigor/bin to the FRONT of their PATH (we offer to do this).
//!
//!   rigor trust opencode   -> creates ~/.rigor/bin/opencode
//!   rigor trust claude     -> creates ~/.rigor/bin/claude
//!   rigor trust codex      -> creates ~/.rigor/bin/codex
//!   rigor untrust opencode -> removes ~/.rigor/bin/opencode

use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const SUPPORTED_TOOLS: &[&str] = &["opencode", "claude", "codex"];

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
        tool,
        tool
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

/// System CA bundle path, OS-specific. Returned path is what we concatenate
/// with rigor's CA to build `~/.rigor/ca-bundle.pem`. Falls back to None if
/// we can't find a system bundle — the generated bundle will contain only
/// rigor's CA, which means non-MITM'd hosts will fail to verify. Not great,
/// but better than failing the `rigor trust` install.
fn system_ca_bundle_path() -> Option<PathBuf> {
    // macOS ships one at /etc/ssl/cert.pem (Homebrew OpenSSL compat);
    // most Linux distros ship /etc/ssl/certs/ca-certificates.crt.
    for candidate in ["/etc/ssl/cert.pem", "/etc/ssl/certs/ca-certificates.crt"] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Ensure `~/.rigor/ca-bundle.pem` exists and contains rigor's CA plus the
/// system trust anchors, so Rust-native-TLS clients (rustls via
/// `rustls-native-certs`, reqwest, Codex) trust both rigor's MITM certs and
/// everyone else's real certs.
///
/// Why this exists: pointing `SSL_CERT_FILE` at *only* `~/.rigor/ca.pem`
/// works for MITM'd hosts but breaks every blind-tunneled host because the
/// upstream's real cert is no longer rooted in any trusted CA. We need both
/// sets in one file.
fn ensure_ca_bundle() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let rigor_dir = home.join(".rigor");
    fs::create_dir_all(&rigor_dir)?;
    let rigor_ca = rigor_dir.join("ca.pem");
    let bundle = rigor_dir.join("ca-bundle.pem");

    if !rigor_ca.exists() {
        anyhow::bail!(
            "rigor CA not found at {}. Run `rigor serve` once to generate it.",
            rigor_ca.display()
        );
    }

    let mut contents = fs::read(&rigor_ca)?;
    if !contents.ends_with(b"\n") {
        contents.push(b'\n');
    }
    if let Some(sys) = system_ca_bundle_path() {
        contents.extend(fs::read(&sys)?);
    }
    fs::write(&bundle, &contents)?;
    Ok(bundle)
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
    // Generate a combined CA bundle so Rust-native-TLS clients (Codex, any
    // reqwest / rustls-native-certs based tool) trust rigor's MITM certs
    // without breaking trust for blind-tunneled hosts.
    let ca_bundle = ensure_ca_bundle()?;
    let ca_bundle_str = ca_bundle.to_string_lossy();

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

# Accept rigor's MITM cert across all TLS stacks:
#   Node/Bun   → NODE_TLS_REJECT_UNAUTHORIZED=0 (skip verification entirely)
#   Rust/rustls → SSL_CERT_FILE pointing at a bundle of rigor CA + system CAs
#                 (Codex, reqwest-based tools, anything using rustls-native-certs)
export NODE_TLS_REJECT_UNAUTHORIZED=0
export SSL_CERT_FILE="${{SSL_CERT_FILE:-{ca_bundle}}}"

# Session tracking
export OPENCODE_SESSION_ID="${{OPENCODE_SESSION_ID:-$(uuidgen 2>/dev/null || cat /proc/sys/kernel/random/uuid 2>/dev/null || echo rigor-$$)}}"
export RIGOR_ROUTED=1

exec {real_binary} "$@"
"#,
        tool = tool,
        real_binary = real_binary,
        ca_bundle = ca_bundle_str,
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
                writeln!(
                    file,
                    "\n# rigor — route AI tools through epistemic constraint proxy"
                )?;
                writeln!(file, "{}", export_line)?;
                println!("  Added to {}", profile.display());
                println!("  Run: source {}", profile.display());
            }
        } else {
            println!("  export PATH=\"{}:$PATH\"", rigor_bin_str);
        }
    }

    println!();
    println!(
        "Now run '{}' normally — traffic routes through rigor.",
        tool
    );
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
