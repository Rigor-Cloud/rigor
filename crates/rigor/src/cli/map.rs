//! `rigor map` — verify source anchors and track code-grounded constraints.
//!
//! Two modes:
//! - `rigor map --check` — verify all anchors are stable (fast, grep-based)
//! - `rigor map --check --deep` — verify with LSP (semantic references, types)
//!
//! Without --check, opens the /rigor:map interactive skill (needs Claude Code).

use anyhow::Result;
use std::path::PathBuf;

use crate::lsp::{self, AnchorStatus};

pub fn run_map(
    path: Option<PathBuf>,
    codebase: Option<PathBuf>,
    check: bool,
    deep: bool,
) -> Result<()> {
    let yaml_path = super::find_rigor_yaml(path)?;
    let project_root = codebase.unwrap_or_else(|| {
        yaml_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf()
    });

    let config = crate::constraint::loader::load_rigor_config(&yaml_path)?;

    let has_anchors = config
        .all_constraints()
        .iter()
        .any(|c| !c.source.is_empty());

    if !has_anchors {
        eprintln!("rigor map: no source anchors found in rigor.yaml");
        eprintln!("rigor map: run /rigor:map in Claude Code to generate code-anchored constraints");
        return Ok(());
    }

    if !check {
        // Without --check, just show status and suggest the skill
        eprintln!("rigor map: use --check to verify source anchors");
        eprintln!("rigor map: use /rigor:map in Claude Code for interactive constraint generation");
        return Ok(());
    }

    let results = if deep {
        // LSP-based deep verification
        match lsp::detect_language_server(&project_root) {
            Some(server) => {
                match lsp::client::verify_anchors_lsp(&project_root, &server, &config) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!(
                            "rigor map: LSP verification failed: {} (falling back to grep)",
                            e
                        );
                        lsp::verify_anchors_grep(&project_root, &config)
                    }
                }
            }
            None => {
                eprintln!(
                    "rigor map: no LSP server detected for this project (falling back to grep)"
                );
                lsp::verify_anchors_grep(&project_root, &config)
            }
        }
    } else {
        // Fast grep-based verification
        lsp::verify_anchors_grep(&project_root, &config)
    };

    // Report results
    let mut stable = 0;
    let mut drifted = 0;
    let mut gone = 0;
    let mut not_found = 0;

    for result in &results {
        let status_icon = match &result.status {
            AnchorStatus::Stable => {
                stable += 1;
                "\x1b[32m✓\x1b[0m"
            }
            AnchorStatus::Drifted { .. } => {
                drifted += 1;
                "\x1b[33m~\x1b[0m"
            }
            AnchorStatus::Gone => {
                gone += 1;
                "\x1b[31m✗\x1b[0m"
            }
            AnchorStatus::FileNotFound => {
                not_found += 1;
                "\x1b[31m!\x1b[0m"
            }
        };

        eprintln!(
            "  {} {} :: {}",
            status_icon, result.constraint_id, result.anchor_path
        );

        match &result.status {
            AnchorStatus::Stable => {
                if let Some(ref text) = result.anchor_text {
                    eprintln!("    anchor: \"{}\"", truncate(text, 60));
                }
            }
            AnchorStatus::Drifted {
                expected_line,
                actual_line,
            } => {
                eprintln!(
                    "    \x1b[33mDRIFTED: expected line {}, found at line {}\x1b[0m",
                    expected_line, actual_line
                );
                if let Some(ref text) = result.anchor_text {
                    eprintln!("    anchor: \"{}\"", truncate(text, 60));
                }
            }
            AnchorStatus::Gone => {
                eprintln!("    \x1b[31mGONE: anchor text not found in file\x1b[0m");
                if let Some(ref text) = result.anchor_text {
                    eprintln!("    was: \"{}\"", truncate(text, 60));
                }
                eprintln!(
                    "    \x1b[31m⚠ The truth behind this constraint may have changed!\x1b[0m"
                );
            }
            AnchorStatus::FileNotFound => {
                eprintln!("    \x1b[31mFILE NOT FOUND: {}\x1b[0m", result.anchor_path);
                eprintln!("    \x1b[31m⚠ The source file was deleted or moved!\x1b[0m");
            }
        }

        // Show references if we have them (from --deep LSP or grep)
        if !result.references.is_empty() {
            eprintln!("    references: {} found", result.references.len());
            for r in result.references.iter().take(5) {
                let ctx = if r.context.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", truncate(&r.context, 50))
                };
                eprintln!("      {}:{}{}", r.file, r.line, ctx);
            }
            if result.references.len() > 5 {
                eprintln!("      ... and {} more", result.references.len() - 5);
            }
        }

        // Show LSP definition info if available
        if let Some(ref def) = result.definition {
            if let Some(ref type_info) = def.type_info {
                eprintln!("    type: {}", truncate(type_info, 80));
            }
        }
    }

    // Summary
    eprintln!();
    let total = stable + drifted + gone + not_found;
    let mode = if deep { "LSP" } else { "grep" };
    eprintln!("rigor map --check ({}): {} anchors checked", mode, total);

    if stable > 0 {
        eprintln!("  \x1b[32m✓ {} stable\x1b[0m", stable);
    }
    if drifted > 0 {
        eprintln!(
            "  \x1b[33m~ {} drifted (update line numbers in rigor.yaml)\x1b[0m",
            drifted
        );
    }
    if gone > 0 {
        eprintln!(
            "  \x1b[31m✗ {} gone (truth may have changed — review constraints!)\x1b[0m",
            gone
        );
    }
    if not_found > 0 {
        eprintln!(
            "  \x1b[31m! {} files not found (deleted or moved)\x1b[0m",
            not_found
        );
    }

    if gone > 0 || not_found > 0 {
        eprintln!();
        eprintln!("  Run /rigor:map in Claude Code to review and update broken constraints.");
        std::process::exit(1);
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max])
    } else {
        s.to_string()
    }
}
