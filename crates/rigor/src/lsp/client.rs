//! LSP client implementation — speaks JSON-RPC 2.0 over stdin/stdout
//! to a language server subprocess.

use anyhow::{anyhow, Context, Result};
use lsp_types::*;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};

use super::{AnchorStatus, AnchorVerification, LanguageServer, ReferenceInfo, SymbolInfo};

/// Convert a file path to an LSP Uri.
fn file_uri(path: &Path) -> Result<Uri> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let uri_str = format!("file://{}", abs.display());
    uri_str
        .parse()
        .map_err(|e| anyhow!("Invalid URI {}: {}", uri_str, e))
}

/// Extract file path from an LSP Uri string.
fn uri_to_path(uri: &Uri) -> Option<std::path::PathBuf> {
    let s = uri.as_str();
    s.strip_prefix("file://").map(std::path::PathBuf::from)
}

/// A client that communicates with a language server over stdin/stdout.
pub struct LspClient {
    process: Child,
    next_id: AtomicI64,
}

#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: i64,
    method: String,
    params: serde_json::Value,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<i64>,
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}

impl LspClient {
    /// Spawn a language server and perform the initialize handshake.
    pub fn start(server: &LanguageServer, project_root: &Path) -> Result<Self> {
        let child = Command::new(&server.command)
            .args(&server.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .current_dir(project_root)
            .spawn()
            .with_context(|| format!("Failed to start LSP server: {}", server.command))?;

        let mut client = Self {
            process: child,
            next_id: AtomicI64::new(1),
        };

        // Send initialize request
        let root_uri = file_uri(project_root)?;

        #[allow(deprecated)]
        let init_params = InitializeParams {
            // `root_uri` is deprecated in favor of `workspace_folders`. Migration
            // tracked alongside Phase 4A (LSP verification fully wired).
            root_uri: Some(root_uri.clone()),
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities {
                    references: Some(DynamicRegistrationClientCapabilities {
                        dynamic_registration: Some(false),
                    }),
                    definition: Some(GotoCapability {
                        dynamic_registration: Some(false),
                        link_support: Some(false),
                    }),
                    hover: Some(HoverClientCapabilities {
                        dynamic_registration: Some(false),
                        content_format: Some(vec![MarkupKind::PlainText]),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let _response = client.send_request("initialize", serde_json::to_value(init_params)?)?;

        // Send initialized notification (no response expected)
        client.send_notification("initialized", serde_json::json!({}))?;

        Ok(client)
    }

    /// Open a file in the language server (required before making queries on it).
    pub fn open_file(&mut self, file_path: &Path) -> Result<()> {
        let uri = file_uri(file_path)?;

        let content = std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;

        let language_id = match file_path.extension().and_then(|e| e.to_str()) {
            Some("rs") => "rust",
            Some("ts" | "tsx") => "typescript",
            Some("js" | "jsx") => "javascript",
            Some("py") => "python",
            Some("go") => "go",
            _ => "plaintext",
        };

        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: language_id.to_string(),
                version: 1,
                text: content,
            },
        };

        self.send_notification("textDocument/didOpen", serde_json::to_value(params)?)
    }

    /// Find all references to the symbol at a given position.
    pub fn find_references(
        &mut self,
        file_path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>> {
        let uri = file_uri(file_path)?;

        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        };

        let response =
            self.send_request("textDocument/references", serde_json::to_value(params)?)?;

        match response {
            Some(value) => {
                let locations: Vec<Location> = serde_json::from_value(value)?;
                Ok(locations)
            }
            None => Ok(vec![]),
        }
    }

    /// Get hover information (type, docs) for a symbol at a given position.
    pub fn hover(&mut self, file_path: &Path, line: u32, character: u32) -> Result<Option<String>> {
        let uri = file_uri(file_path)?;

        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: Default::default(),
        };

        let response = self.send_request("textDocument/hover", serde_json::to_value(params)?)?;

        match response {
            Some(value) => {
                let hover: Hover = serde_json::from_value(value)?;
                let text = match hover.contents {
                    HoverContents::Scalar(MarkedString::String(s)) => Some(s),
                    HoverContents::Scalar(MarkedString::LanguageString(ls)) => Some(ls.value),
                    HoverContents::Array(items) => {
                        let texts: Vec<String> = items
                            .into_iter()
                            .map(|i| match i {
                                MarkedString::String(s) => s,
                                MarkedString::LanguageString(ls) => ls.value,
                            })
                            .collect();
                        Some(texts.join("\n"))
                    }
                    HoverContents::Markup(mc) => Some(mc.value),
                };
                Ok(text)
            }
            None => Ok(None),
        }
    }

    /// Go to definition of the symbol at a given position.
    pub fn goto_definition(
        &mut self,
        file_path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>> {
        let uri = file_uri(file_path)?;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let response =
            self.send_request("textDocument/definition", serde_json::to_value(params)?)?;

        match response {
            Some(value) => {
                // Definition can be a single Location, an array, or LocationLink array
                if let Ok(loc) = serde_json::from_value::<Location>(value.clone()) {
                    return Ok(vec![loc]);
                }
                if let Ok(locs) = serde_json::from_value::<Vec<Location>>(value.clone()) {
                    return Ok(locs);
                }
                if let Ok(links) = serde_json::from_value::<Vec<LocationLink>>(value) {
                    return Ok(links
                        .into_iter()
                        .map(|l| Location {
                            uri: l.target_uri,
                            range: l.target_range,
                        })
                        .collect());
                }
                Ok(vec![])
            }
            None => Ok(vec![]),
        }
    }

    /// Shut down the language server gracefully.
    pub fn shutdown(mut self) -> Result<()> {
        let _ = self.send_request("shutdown", serde_json::json!(null));
        let _ = self.send_notification("exit", serde_json::json!(null));
        let _ = self.process.wait();
        Ok(())
    }

    /// Send a JSON-RPC request and wait for the response.
    fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let body = serde_json::to_string(&request)?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        let stdin = self
            .process
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("LSP stdin not available"))?;
        stdin.write_all(message.as_bytes())?;
        stdin.flush()?;

        // Read response — keep reading until we get one with matching id
        let stdout = self
            .process
            .stdout
            .as_mut()
            .ok_or_else(|| anyhow!("LSP stdout not available"))?;

        loop {
            let response = read_lsp_message(stdout)?;
            let parsed: JsonRpcResponse = serde_json::from_str(&response).with_context(|| {
                format!(
                    "Failed to parse LSP response: {}",
                    &response[..response.len().min(200)]
                )
            })?;

            // Skip notifications (no id)
            if parsed.id.is_none() {
                continue;
            }

            if parsed.id == Some(id) {
                if let Some(error) = parsed.error {
                    return Err(anyhow!("LSP error: {}", error));
                }
                return Ok(parsed.result);
            }
            // Response for a different request id — skip it
        }
    }

    /// Send a JSON-RPC notification (no response expected).
    fn send_notification(&mut self, method: &str, params: serde_json::Value) -> Result<()> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        let body = serde_json::to_string(&notification)?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        let stdin = self
            .process
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("LSP stdin not available"))?;
        stdin.write_all(message.as_bytes())?;
        stdin.flush()?;
        Ok(())
    }
}

/// Read a single LSP message from stdout (Content-Length header + body).
fn read_lsp_message(stdout: &mut std::process::ChildStdout) -> Result<String> {
    let mut reader = BufReader::new(stdout);
    let mut content_length: usize = 0;

    // Read headers
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let header = header.trim();

        if header.is_empty() {
            break;
        }

        if let Some(len_str) = header.strip_prefix("Content-Length: ") {
            content_length = len_str.parse()?;
        }
    }

    if content_length == 0 {
        return Err(anyhow!("LSP message with zero content length"));
    }

    // Read body
    let mut body = vec![0u8; content_length];
    std::io::Read::read_exact(&mut reader, &mut body)?;

    Ok(String::from_utf8(body)?)
}

/// Verify source anchors using a full LSP server.
/// Spawns the language server, opens anchored files, queries references.
pub fn verify_anchors_lsp(
    project_root: &Path,
    server: &LanguageServer,
    config: &crate::constraint::types::RigorConfig,
) -> Result<Vec<AnchorVerification>> {
    eprintln!(
        "rigor: starting {} for deep anchor verification...",
        server.command
    );
    let mut client = LspClient::start(server, project_root)?;

    // Give the server a moment to index
    std::thread::sleep(std::time::Duration::from_secs(2));

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

            // First check anchor text (grep-level)
            let status = if let Some(ref anchor_text) = anchor.anchor {
                match super::verify_anchor_text(&file_path, anchor_text, &anchor.lines) {
                    Ok(s) => s,
                    Err(_) => AnchorStatus::Gone,
                }
            } else {
                AnchorStatus::Stable
            };

            // Open the file in the LSP server
            if let Err(e) = client.open_file(&file_path) {
                eprintln!("rigor: LSP failed to open {}: {}", anchor.path, e);
                results.push(AnchorVerification {
                    constraint_id: constraint.id.clone(),
                    anchor_path: anchor.path.clone(),
                    anchor_text: anchor.anchor.clone(),
                    status,
                    definition: None,
                    references: vec![],
                    overrides: vec![],
                });
                continue;
            }

            // Find the anchor position (line, character) for LSP queries
            let (anchor_line, anchor_char) = find_anchor_position(&file_path, anchor);

            // Query LSP for references
            let lsp_refs = if anchor_line > 0 {
                match client.find_references(&file_path, anchor_line - 1, anchor_char) {
                    Ok(locs) => locs
                        .into_iter()
                        .map(|loc| {
                            let ref_path = uri_to_path(&loc.uri)
                                .map(|p| {
                                    p.strip_prefix(project_root)
                                        .unwrap_or(&p)
                                        .to_string_lossy()
                                        .to_string()
                                })
                                .unwrap_or_else(|| loc.uri.as_str().to_string());
                            ReferenceInfo {
                                file: ref_path,
                                line: loc.range.start.line + 1,
                                context: String::new(),
                            }
                        })
                        .collect(),
                    Err(e) => {
                        eprintln!("rigor: LSP references failed for {}: {}", anchor.path, e);
                        vec![]
                    }
                }
            } else {
                vec![]
            };

            // Query LSP for hover info (type, docs)
            let type_info = if anchor_line > 0 {
                client
                    .hover(&file_path, anchor_line - 1, anchor_char)
                    .unwrap_or(None)
            } else {
                None
            };

            let definition = if anchor_line > 0 {
                Some(SymbolInfo {
                    name: anchor.anchor.clone().unwrap_or_default(),
                    kind: "unknown".to_string(),
                    file: anchor.path.clone(),
                    line: anchor_line,
                    type_info,
                })
            } else {
                None
            };

            results.push(AnchorVerification {
                constraint_id: constraint.id.clone(),
                anchor_path: anchor.path.clone(),
                anchor_text: anchor.anchor.clone(),
                status,
                definition,
                references: lsp_refs,
                overrides: vec![],
            });
        }
    }

    client.shutdown()?;
    Ok(results)
}

/// Find the line and character position of an anchor in a file.
fn find_anchor_position(
    file_path: &Path,
    anchor: &crate::constraint::types::SourceAnchor,
) -> (u32, u32) {
    // If we have explicit lines, use the first one
    if !anchor.lines.is_empty() {
        // Try to find the identifier character offset on that line
        if let Ok(content) = std::fs::read_to_string(file_path) {
            let target_line = anchor.lines[0] as usize;
            if let Some(line_text) = content.lines().nth(target_line.saturating_sub(1)) {
                // Find the first identifier-like token on the line
                if let Some(ref anchor_text) = anchor.anchor {
                    if let Some(ident) = super::extract_identifier(anchor_text) {
                        if let Some(pos) = line_text.find(&ident) {
                            return (anchor.lines[0], pos as u32);
                        }
                    }
                }
            }
        }
        return (anchor.lines[0], 0);
    }

    // No explicit lines — search for anchor text
    if let Some(ref anchor_text) = anchor.anchor {
        if let Ok(content) = std::fs::read_to_string(file_path) {
            for (i, line) in content.lines().enumerate() {
                if line.contains(anchor_text.as_str()) {
                    if let Some(ident) = super::extract_identifier(anchor_text) {
                        if let Some(pos) = line.find(&ident) {
                            return ((i + 1) as u32, pos as u32);
                        }
                    }
                    return ((i + 1) as u32, 0);
                }
            }
        }
    }

    (0, 0)
}
