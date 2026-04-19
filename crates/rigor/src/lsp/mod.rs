//! LSP client for code-anchored constraint verification.
//!
//! Spawns a language server (rust-analyzer, tsserver, pyright, gopls)
//! and queries it to verify source anchors: find definitions, references,
//! and effective values of symbols that ground epistemic constraints.

pub mod client;

use anyhow::{Context, Result};
use std::path::Path;

/// Detected project language and its LSP server command.
#[derive(Debug, Clone)]
pub struct LanguageServer {
    pub language: String,
    pub command: String,
    pub args: Vec<String>,
}

/// Detect the project language and return the appropriate LSP server config.
pub fn detect_language_server(project_root: &Path) -> Option<LanguageServer> {
    if project_root.join("Cargo.toml").exists() {
        Some(LanguageServer {
            language: "rust".to_string(),
            command: "rust-analyzer".to_string(),
            args: vec![],
        })
    } else if project_root.join("tsconfig.json").exists() || project_root.join("package.json").exists() {
        Some(LanguageServer {
            language: "typescript".to_string(),
            command: "typescript-language-server".to_string(),
            args: vec!["--stdio".to_string()],
        })
    } else if project_root.join("pyproject.toml").exists() || project_root.join("setup.py").exists() {
        Some(LanguageServer {
            language: "python".to_string(),
            command: "pyright-langserver".to_string(),
            args: vec!["--stdio".to_string()],
        })
    } else if project_root.join("go.mod").exists() {
        Some(LanguageServer {
            language: "go".to_string(),
            command: "gopls".to_string(),
            args: vec!["serve".to_string()],
        })
    } else {
        None
    }
}

/// Result of verifying a single source anchor via LSP.
#[derive(Debug)]
pub struct AnchorVerification {
    pub constraint_id: String,
    pub anchor_path: String,
    pub anchor_text: Option<String>,
    pub status: AnchorStatus,
    pub definition: Option<SymbolInfo>,
    pub references: Vec<ReferenceInfo>,
    pub overrides: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum AnchorStatus {
    /// Anchor text found at expected location
    Stable,
    /// Anchor text found but at a different line
    Drifted { expected_line: u32, actual_line: u32 },
    /// Anchor text not found in file
    Gone,
    /// File itself doesn't exist
    FileNotFound,
}

#[derive(Debug)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub type_info: Option<String>,
}

#[derive(Debug)]
pub struct ReferenceInfo {
    pub file: String,
    pub line: u32,
    pub context: String,
}

/// Verify all source anchors in a rigor config using grep (fast path)
/// and optionally LSP (deep path).
pub fn verify_anchors_grep(
    project_root: &Path,
    config: &crate::constraint::types::RigorConfig,
) -> Vec<AnchorVerification> {
    let mut results = Vec::new();

    for constraint in config.all_constraints() {
        for anchor in &constraint.source {
            let file_path = project_root.join(&anchor.path);

            if !file_path.exists() {
                results.push(AnchorVerification {
                    constraint_id: constraint.id.clone(),
                    anchor_path: anchor.path.clone(),
                    anchor_text: anchor.anchor.clone(),
                    status: AnchorStatus::FileNotFound,
                    definition: None,
                    references: vec![],
                    overrides: vec![],
                });
                continue;
            }

            let status = if let Some(ref anchor_text) = anchor.anchor {
                match verify_anchor_text(&file_path, anchor_text, &anchor.lines) {
                    Ok(s) => s,
                    Err(_) => AnchorStatus::Gone,
                }
            } else {
                // No anchor text — just check file exists (already confirmed)
                AnchorStatus::Stable
            };

            // Grep for references across the project
            let references = if let Some(ref anchor_text) = anchor.anchor {
                // Extract a greppable identifier from the anchor
                let ident = extract_identifier(anchor_text);
                if let Some(ident) = ident {
                    find_references_grep(project_root, &ident, &anchor.path)
                } else {
                    vec![]
                }
            } else {
                vec![]
            };

            results.push(AnchorVerification {
                constraint_id: constraint.id.clone(),
                anchor_path: anchor.path.clone(),
                anchor_text: anchor.anchor.clone(),
                status,
                definition: None,
                references,
                overrides: vec![],
            });
        }
    }

    results
}

/// Check if anchor text exists in file at expected lines.
fn verify_anchor_text(file_path: &Path, anchor_text: &str, expected_lines: &[u32]) -> Result<AnchorStatus> {
    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read {}", file_path.display()))?;

    let mut found_line = None;
    for (i, line) in content.lines().enumerate() {
        if line.contains(anchor_text) {
            found_line = Some((i + 1) as u32);
            break;
        }
    }

    match found_line {
        Some(actual_line) => {
            if expected_lines.is_empty() || expected_lines.contains(&actual_line) {
                Ok(AnchorStatus::Stable)
            } else {
                Ok(AnchorStatus::Drifted {
                    expected_line: expected_lines[0],
                    actual_line,
                })
            }
        }
        None => Ok(AnchorStatus::Gone),
    }
}

/// Extract a likely identifier from anchor text for grep-based reference finding.
fn extract_identifier(anchor: &str) -> Option<String> {
    // Try to find a Rust/code identifier in the anchor
    for word in anchor.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let w = word.trim();
        if w.len() > 3 && w.chars().all(|c| c.is_alphanumeric() || c == '_') {
            // Skip common keywords and values
            if !matches!(w, "true" | "false" | "self" | "None" | "Some" | "impl" | "pub" | "fn" | "let" | "mut" | "const" | "struct") {
                return Some(w.to_string());
            }
        }
    }
    None
}

/// Find references to an identifier across the project using grep.
fn find_references_grep(project_root: &Path, identifier: &str, source_file: &str) -> Vec<ReferenceInfo> {
    let output = std::process::Command::new("grep")
        .args(["-rn", "--include=*.rs", "--include=*.ts", "--include=*.py", "--include=*.go",
               identifier, project_root.to_str().unwrap_or(".")])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(_) => return vec![],
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut refs = Vec::new();

    for line in stdout.lines() {
        // Format: /path/to/file:42:    let x = block_threshold;
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        if parts.len() >= 3 {
            let file = parts[0];
            let line_num: u32 = parts[1].parse().unwrap_or(0);
            let context = parts[2].trim().to_string();

            // Make path relative to project root
            let rel_path = file
                .strip_prefix(project_root.to_str().unwrap_or(""))
                .unwrap_or(file)
                .trim_start_matches('/');

            // Skip the source file itself (we already know about it)
            if rel_path == source_file {
                continue;
            }

            // Skip target/ and .git/ directories
            if rel_path.starts_with("target/") || rel_path.starts_with(".git/") {
                continue;
            }

            refs.push(ReferenceInfo {
                file: rel_path.to_string(),
                line: line_num,
                context,
            });
        }
    }

    refs
}
