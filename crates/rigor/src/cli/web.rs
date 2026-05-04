use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::Result;
use axum::http::header;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rust_embed::Embed;

use crate::constraint::graph::ArgumentationGraph;
use crate::constraint::loader::load_rigor_config;
use crate::constraint::types::{EpistemicType, RelationType};
use crate::logging::ViolationLogger;

use super::find_rigor_yaml;

#[derive(Embed)]
#[folder = "../../viewer-legacy/"]
struct ViewerAssets;

/// Graph data serialized for 3d-force-graph.
#[derive(serde::Serialize)]
struct GraphData {
    nodes: Vec<GraphNode>,
    links: Vec<GraphLink>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct GraphNode {
    id: String,
    #[serde(rename = "type")]
    node_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    epistemic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    strength: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_strength: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    claim_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    git_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    constraint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    supporters: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attackers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transcript_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    claim_source: Option<ClaimSourceData>,
    val: f64,
    color: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaimSourceData {
    message_index: usize,
    sentence_index: usize,
}

#[derive(serde::Serialize)]
struct GraphLink {
    source: String,
    target: String,
    relation: String,
    color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    curvature: Option<f64>,
}

fn build_graph_data(yaml_path: &Path) -> Result<GraphData> {
    let config = load_rigor_config(yaml_path)?;
    let mut arg_graph = ArgumentationGraph::from_config(&config);
    arg_graph.compute_strengths()?;

    let mut nodes = Vec::new();
    let mut links = Vec::new();

    // Add constraint nodes
    for c in config.all_constraints() {
        let strength = arg_graph
            .get_strength(&c.id)
            .unwrap_or(c.epistemic_type.base_strength());
        let (epistemic_label, color) = match c.epistemic_type {
            EpistemicType::Belief => ("belief", "#ff6b6b"),
            EpistemicType::Justification => ("justification", "#51cf66"),
            EpistemicType::Defeater => ("defeater", "#748ffc"),
        };

        nodes.push(GraphNode {
            id: c.id.clone(),
            node_type: "constraint".to_string(),
            name: Some(c.name.clone()),
            epistemic: Some(epistemic_label.to_string()),
            description: Some(c.description.clone()),
            strength: Some(strength),
            base_strength: Some(c.epistemic_type.base_strength()),
            tags: Some(c.tags.clone()),
            domain: c.domain.clone(),
            text: None,
            confidence: None,
            claim_type: None,
            severity: None,
            decision: None,
            reason: None,
            session: None,
            session_id: None,
            git_commit: None,
            constraint: None,
            supporters: None,
            attackers: None,
            transcript_path: None,
            claim_source: None,
            val: strength * 2.0 + 1.0,
            color: color.to_string(),
        });
    }

    // Add relation links
    for r in &config.relations {
        let (relation_label, color) = match r.relation_type {
            RelationType::Supports => ("supports", "#51cf66"),
            RelationType::Attacks => ("attacks", "#ff6b6b"),
            RelationType::Undercuts => ("undercuts", "#ff922b"),
        };

        links.push(GraphLink {
            source: r.from.clone(),
            target: r.to.clone(),
            relation: relation_label.to_string(),
            color: color.to_string(),
            curvature: Some(0.1),
        });
    }

    // Collect constraint IDs for filtering violations
    let constraint_ids: std::collections::HashSet<String> =
        nodes.iter().map(|n| n.id.clone()).collect();

    // Add violation claims from log
    if let Ok(logger) = ViolationLogger::new() {
        if let Ok(entries) = logger.read_all() {
            // Deduplicate by claim text to avoid flooding the graph
            let mut seen_claims: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            for entry in &entries {
                // Skip violations referencing constraints not in this config
                if !constraint_ids.contains(&entry.constraint_id) {
                    continue;
                }

                for claim_text in &entry.claim_text {
                    if seen_claims.contains(claim_text) {
                        continue;
                    }
                    seen_claims.insert(claim_text.clone());

                    let claim_id = format!("claim-{}", nodes.len());

                    nodes.push(GraphNode {
                        id: claim_id.clone(),
                        node_type: "claim".to_string(),
                        name: None,
                        epistemic: None,
                        description: None,
                        strength: None,
                        base_strength: None,
                        tags: None,
                        domain: None,
                        text: Some(claim_text.clone()),
                        confidence: Some(entry.computed_strength),
                        claim_type: None,
                        severity: Some(entry.severity.clone()),
                        decision: Some(entry.decision.clone()),
                        reason: Some(entry.message.clone()),
                        session: Some(entry.session.timestamp.clone()),
                        session_id: Some(entry.session.session_id.clone()),
                        git_commit: entry.session.git_commit.clone(),
                        constraint: Some(entry.constraint_name.clone()),
                        supporters: Some(entry.supporters.clone()),
                        attackers: Some(entry.attackers.clone()),
                        transcript_path: entry.transcript_path.clone(),
                        claim_source: entry.claim_source.as_ref().map(|s| ClaimSourceData {
                            message_index: s.message_index,
                            sentence_index: s.sentence_index,
                        }),
                        val: 1.0,
                        color: "#ffd93d".to_string(),
                    });

                    links.push(GraphLink {
                        source: claim_id,
                        target: entry.constraint_id.clone(),
                        relation: "violates".to_string(),
                        color: "#ff922b".to_string(),
                        curvature: Some(0.2),
                    });
                }
            }
        }
    }

    Ok(GraphData { nodes, links })
}

fn serve_asset(path: &str) -> Response {
    let mime = if path.ends_with(".js") || path.ends_with(".jsx") {
        "application/javascript"
    } else if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".html") {
        "text/html"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    };

    match ViewerAssets::get(path) {
        Some(content) => ([(header::CONTENT_TYPE, mime)], content.data.to_vec()).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

pub fn run_web(path: Option<PathBuf>, port: u16) -> Result<()> {
    let yaml_path = find_rigor_yaml(path)?;
    let yaml_path_clone = yaml_path.clone();

    eprintln!("rigor: loading constraints from {}", yaml_path.display());

    // Verify config loads before starting server
    let data = build_graph_data(&yaml_path)?;
    eprintln!(
        "rigor: {} nodes, {} links",
        data.nodes.len(),
        data.links.len()
    );

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let app = Router::new()
            .route("/", get(|| async { Html(serve_asset("index.html")) }))
            .route(
                "/graph.json",
                get(move || {
                    let yaml = yaml_path_clone.clone();
                    async move {
                        match build_graph_data(&yaml) {
                            Ok(data) => axum::Json(data).into_response(),
                            Err(e) => (
                                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Failed to build graph: {}", e),
                            )
                                .into_response(),
                        }
                    }
                }),
            )
            .route(
                "/assets/{*path}",
                get(
                    |axum::extract::Path(path): axum::extract::Path<String>| async move {
                        serve_asset(&path)
                    },
                ),
            );

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        eprintln!("rigor: graph explorer at http://127.0.0.1:{}", port);
        eprintln!("rigor: press Ctrl+C to stop");

        // Open browser
        let url = format!("http://rigor.local:{}", port);
        let _ = open::that(&url);

        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
    });

    Ok(())
}

impl EpistemicType {
    fn base_strength(&self) -> f64 {
        match self {
            EpistemicType::Belief => 0.8,
            EpistemicType::Justification => 0.9,
            EpistemicType::Defeater => 0.7,
        }
    }
}

// === Public handlers for daemon reuse ===

/// Serve index.html
pub async fn serve_index() -> axum::response::Html<String> {
    axum::response::Html(
        ViewerAssets::get("index.html")
            .map(|f| String::from_utf8_lossy(&f.data).to_string())
            .unwrap_or_else(|| "Not found".to_string()),
    )
}

/// Serve viewer asset files
pub async fn serve_viewer_asset(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    serve_asset(&path)
}

/// Build graph JSON from shared daemon state
pub async fn graph_json_from_state(state: crate::daemon::SharedState) -> Response {
    let yaml_path = {
        let st = state.lock().unwrap();
        st.yaml_path.clone()
    };

    match build_graph_data(&yaml_path) {
        Ok(data) => axum::Json(data).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to build graph: {}", e),
        )
            .into_response(),
    }
}
