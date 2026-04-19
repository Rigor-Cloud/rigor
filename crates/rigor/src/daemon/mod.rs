pub mod chat;
pub mod context;
pub mod gate;
pub mod gate_api;
pub mod governance;
pub mod proxy;
pub mod sni;
pub mod tls;
pub mod ws;
pub mod egress;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::routing::{get, post};
use axum::Router;

/// Path to the running-daemon PID file: `~/.rigor/daemon.pid`.
/// Hooks (Stop hook in `lib.rs`, gate pre-/post-tool in `cli/gate.rs`) check
/// this file + a `kill(pid, 0)` liveness test to decide whether to run at all.
/// When no rigor-personal daemon is alive, hooks short-circuit to no-op.
pub fn daemon_pid_file() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".rigor/daemon.pid"))
}

/// Write the current process PID to `~/.rigor/daemon.pid`. Called once at the
/// top of `start_daemon` so hooks can find us. Stale files (daemon crashed
/// without cleanup) are handled by the `daemon_alive` liveness check, so we
/// don't worry about atomic write or locking here.
pub fn write_pid_file() -> std::io::Result<()> {
    if let Some(path) = daemon_pid_file() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, format!("{}\n", std::process::id()))?;
    }
    Ok(())
}

/// Remove `~/.rigor/daemon.pid`. Best-effort — called from signal handlers
/// and Drop guards; failures are silent because we'd rather have a stale file
/// than panic during shutdown.
pub fn remove_pid_file() {
    if let Some(path) = daemon_pid_file() {
        let _ = std::fs::remove_file(path);
    }
}

/// Is a rigor daemon currently alive on this machine?
///
/// Reads `~/.rigor/daemon.pid` and tests the PID with `kill(pid, 0)`, which
/// checks process existence without signaling. Stale PIDs (daemon crashed,
/// OS recycled the PID to something else) are handled as best we can — we
/// can't definitively tell a stale-but-recycled PID from a live daemon just
/// from `kill(0)`. For that, Phase 2 adds session registration checks.
pub fn daemon_alive() -> bool {
    let Some(path) = daemon_pid_file() else { return false };
    let Ok(contents) = std::fs::read_to_string(&path) else { return false };
    let Ok(pid) = contents.trim().parse::<i32>() else { return false };
    // kill(pid, 0) returns 0 if the process exists (we can signal it),
    // or -1 with errno set otherwise. We just need to know it exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

use crate::cli::find_rigor_yaml;
use crate::constraint::graph::ArgumentationGraph;
use crate::constraint::loader::load_rigor_config;
use crate::constraint::types::RigorConfig;
use crate::fallback::FallbackConfig;
use crate::info_println;
use crate::policy::PolicyEngine;

use self::ws::EventSender;

/// LLM API hostnames where we MITM the TLS tunnel to inspect requests/responses,
/// inject epistemic context, and extract claims. Following mirrord's "remote filter"
/// pattern: only THESE hosts get our special handling. Everything else (OAuth,
/// telemetry, CDN downloads, etc.) gets a blind tunnel — bytes pass through unchanged
/// so the original TLS stays end-to-end and OAuth/auth flows aren't broken.
pub const MITM_HOSTS: &[&str] = &[
    "api.anthropic.com",
    "api.openai.com",
    "us-east5-aiplatform.googleapis.com",
    "us-central1-aiplatform.googleapis.com",
    "us-west1-aiplatform.googleapis.com",
    "europe-west1-aiplatform.googleapis.com",
    "europe-west4-aiplatform.googleapis.com",
    "asia-southeast1-aiplatform.googleapis.com",
    "aiplatform.googleapis.com",
    "openai.azure.com",
    // OpenCode Zen and OpenCode Go provider endpoints
    "opencode.ai",
    "api.opencode.ai",
    // OpenRouter (used by both OpenCode and rigor's LLM-as-judge)
    "openrouter.ai",
];

/// Decide whether a CONNECT target host should be MITM'd or blind-tunneled.
/// Takes a CONNECT target like "api.anthropic.com:443" or just "host:port".
///
/// Returns true ONLY if:
///   1. The global MITM flag is enabled (via `rigor ground --mitm`)
///   2. AND the host matches our MITM allowlist (LLM endpoints)
///
/// Default behavior: blind-tunnel everything. This preserves end-to-end TLS
/// and keeps OAuth/cert-pinning/auth flows working out of the box.
pub fn should_mitm_target(target: &str) -> bool {
    if !ws::is_mitm_enabled() {
        return false;
    }
    let host = target.split(':').next().unwrap_or(target);
    MITM_HOSTS.iter().any(|&h| host == h || host.ends_with(&format!(".{}", h)))
}

#[derive(Debug, Clone)]
pub enum GateType {
    RealTime,
    Retroactive,
}

pub struct ActionGateEntry {
    pub gate_type: GateType,
    pub decision_tx: Option<tokio::sync::oneshot::Sender<bool>>,
    pub action_text: String,
    pub user_message: String,
    pub session_id: String,
    pub created_at: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct SnapshotEntry {
    pub snapshot_id: String,
    pub affected_paths: Vec<String>,
    pub tool_name: String,
    pub created_at: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct GateDecision {
    pub approved: bool,
    pub gate_id: String,
    pub decided_at: std::time::Instant,
}

/// Shared daemon state accessible from all route handlers.
pub struct DaemonState {
    pub config: RigorConfig,
    pub graph: ArgumentationGraph,
    pub yaml_path: PathBuf,
    pub target_api: String,
    pub api_key: Option<String>,
    pub event_tx: EventSender,
    /// Pre-compiled policy engine — clone per request to avoid re-parsing Rego.
    pub policy_engine: Option<PolicyEngine>,
    /// Legacy TLS config (self-signed multi-SAN cert) for the dedicated TLS listener.
    pub tls_config: Option<Arc<rustls::ServerConfig>>,
    /// CA-based MITM: generates per-host certs signed by the rigor CA.
    /// Used by the CONNECT MITM path for proper cert chains.
    pub rigor_ca: Option<Arc<tls::RigorCA>>,
    /// Shared HTTP client for upstream requests — reuses connection pools across requests.
    pub http_client: reqwest::Client,
    /// Judge config: (api_url, api_key, model) — from ~/.rigor/config or env vars
    pub judge_api_url: String,
    pub judge_api_key: Option<String>,
    pub judge_model: String,
    /// Set of constraint IDs toggled off from the dashboard.
    pub disabled_constraints: std::collections::HashSet<String>,
    /// When true, proxy passes traffic through without evaluation.
    pub proxy_paused: bool,
    /// When true, next response is force-blocked (for testing).
    pub block_next: bool,
    pub action_gates: std::collections::HashMap<String, ActionGateEntry>,
    pub gate_snapshots: std::collections::HashMap<String, Vec<SnapshotEntry>>,
    pub gate_decisions: std::collections::HashMap<String, GateDecision>,
    pub active_streams: std::collections::HashSet<String>,
    pub blocked_requests: std::collections::HashSet<String>,
    /// Fallback policy config — governs error handling for each pipeline component.
    pub fallback: FallbackConfig,
}

impl DaemonState {
    pub fn load(yaml_path: PathBuf, event_tx: EventSender) -> Result<Self> {
        let config = load_rigor_config(&yaml_path)?;
        let mut graph = ArgumentationGraph::from_config(&config);
        graph.compute_strengths()?;

        let target_api = std::env::var("RIGOR_TARGET_API")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());

        let api_key = std::env::var("ANTHROPIC_API_KEY").ok();

        // Pre-compile policy engine at startup (clone per request)
        let policy_engine = match PolicyEngine::new(&config) {
            Ok(engine) => {
                info_println!("rigor daemon: pre-compiled policy engine ({} constraints)", engine.loaded_constraints().len());
                Some(engine)
            }
            Err(e) => {
                eprintln!("rigor daemon: failed to pre-compile policy engine: {} (will create per-request)", e);
                None
            }
        };

        // Legacy TLS config for the dedicated TLS listener
        let tls_config = match tls::generate_tls_config(MITM_HOSTS) {
            Ok(cfg) => Some(Arc::new(cfg)),
            Err(e) => {
                eprintln!("rigor daemon: legacy TLS config failed: {} (non-critical)", e);
                None
            }
        };

        // Load or generate the rigor CA for proper MITM cert chains
        let rigor_ca = match tls::RigorCA::load_or_generate() {
            Ok(ca) => Some(Arc::new(ca)),
            Err(e) => {
                eprintln!("rigor daemon: CA setup failed: {} (MITM will use legacy self-signed certs)", e);
                None
            }
        };

        // Shared HTTP client with connection pooling for upstream requests
        let http_client = reqwest::Client::builder()
            .pool_max_idle_per_host(4)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        // Load fallback config from rigor.yaml or use defaults
        let fallback = FallbackConfig::from_yaml(&yaml_path)
            .unwrap_or_else(|e| {
                eprintln!("rigor daemon: fallback config error: {} (using defaults)", e);
                FallbackConfig::default_config()
            });
        // Validate at startup — fails the daemon if minimums are violated
        if let Err(e) = fallback.validate() {
            anyhow::bail!("fallback config validation failed: {}", e);
        }

        Ok(Self {
            config,
            graph,
            yaml_path,
            target_api,
            api_key,
            event_tx,
            policy_engine,
            tls_config,
            rigor_ca,
            http_client,
            disabled_constraints: std::collections::HashSet::new(),
            proxy_paused: false,
            block_next: false,
            action_gates: std::collections::HashMap::new(),
            gate_snapshots: std::collections::HashMap::new(),
            gate_decisions: std::collections::HashMap::new(),
            active_streams: std::collections::HashSet::new(),
            blocked_requests: std::collections::HashSet::new(),
            fallback,
            judge_api_url: {
                let (url, _, _) = crate::cli::config::judge_config();
                url
            },
            judge_api_key: {
                let (_, key, _) = crate::cli::config::judge_config();
                key
            },
            judge_model: {
                let (_, _, model) = crate::cli::config::judge_config();
                model
            },
        })
    }
}

pub type SharedState = Arc<Mutex<DaemonState>>;

/// Start the rigor daemon with both HTTP and HTTPS listeners.
///
/// HTTP port (default 8787): dashboard, graph viewer, WebSocket, plaintext proxy
/// HTTPS port (HTTP port + 1, default 8788): TLS-terminated proxy for LD_PRELOAD traffic
pub fn start_daemon(yaml_path: Option<PathBuf>, port: u16) -> Result<()> {
    let yaml_path = find_rigor_yaml(yaml_path)?;

    // Write ~/.rigor/daemon.pid so hooks can detect us. Best-effort: a write
    // failure (e.g., $HOME unset) doesn't block the daemon from starting —
    // hooks just won't know we're here, which is no worse than the pre-fix
    // behavior.
    if let Err(e) = write_pid_file() {
        eprintln!("rigor daemon: warning — could not write pid file: {}", e);
    }

    let (event_tx, _event_rx) = ws::create_event_channel();
    let state = DaemonState::load(yaml_path.clone(), event_tx.clone())?;

    let constraint_count = state.config.all_constraints().len();
    let strengths: Vec<(String, f64)> = state.graph.get_all_strengths().into_iter().collect();

    eprintln!(
        "rigor daemon: loaded {} constraints from {}",
        constraint_count,
        yaml_path.display()
    );
    if std::env::var("RIGOR_DEBUG").is_ok() {
        for (id, s) in &strengths {
            let severity = if *s >= 0.7 { "BLOCK" } else if *s >= 0.4 { "WARN" } else { "ALLOW" };
            eprintln!("  {} ({:.2}) [{}]", id, s, severity);
        }
    }

    let shared = Arc::new(Mutex::new(state));

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let app = build_router(shared);
        // TLS port: default 443, configurable via RIGOR_DAEMON_TLS_PORT for non-root usage
        let tls_port: u16 = std::env::var("RIGOR_DAEMON_TLS_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(443);

        // HTTP listener (dashboard + plaintext proxy)
        let http_app = app.clone();
        let http_handle = tokio::spawn(async move {
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            eprintln!("rigor daemon: HTTP on http://127.0.0.1:{} (dashboard + proxy)", port);
            axum::serve(listener, http_app).await.unwrap();
        });

        // HTTPS listener (TLS-terminated proxy for LD_PRELOAD intercepted traffic)
        let tls_app = app;
        let tls_handle = tokio::spawn(async move {
            let tls_config = match tls::generate_tls_config(&[
                "api.anthropic.com",
                "api.openai.com",
                "us-east5-aiplatform.googleapis.com",
                "us-central1-aiplatform.googleapis.com",
                "aiplatform.googleapis.com",
            ]) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("rigor daemon: TLS config failed: {} — HTTPS proxy disabled", e);
                    return;
                }
            };

            let tls_acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls_config));
            let listener = tokio::net::TcpListener::bind(
                format!("127.0.0.1:{}", tls_port),
            )
            .await
            .unwrap();

            eprintln!(
                "rigor daemon: HTTPS on https://127.0.0.1:{} (LD_PRELOAD intercepted traffic)",
                tls_port
            );

            // Accept TLS connections and serve them with the same axum app
            loop {
                let (stream, _addr) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(e) => {
                        eprintln!("rigor daemon: accept error: {}", e);
                        continue;
                    }
                };

                let acceptor = tls_acceptor.clone();
                let app = tls_app.clone();

                tokio::spawn(async move {
                    match acceptor.accept(stream).await {
                        Ok(tls_stream) => {
                            eprintln!("rigor daemon: TLS connection accepted");
                            let io = hyper_util::rt::TokioIo::new(tls_stream);

                            let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                let app = app.clone();
                                let path = req.uri().path().to_string();
                                let method = req.method().to_string();
                                eprintln!("rigor daemon: TLS request: {} {}", method, path);

                                async move {
                                    use tower::Service;
                                    let (parts, incoming) = req.into_parts();
                                    let body = axum::body::Body::new(incoming);
                                    let req = axum::http::Request::from_parts(parts, body);
                                    let mut svc = app;
                                    let resp = svc.call(req).await.unwrap();
                                    Ok::<_, std::convert::Infallible>(resp)
                                }
                            });

                            if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                                hyper_util::rt::TokioExecutor::new(),
                            )
                            .serve_connection_with_upgrades(io, service)
                            .await
                            {
                                let msg = e.to_string();
                                if !msg.contains("connection closed") && !msg.contains("broken pipe") {
                                    eprintln!("rigor daemon: TLS error: {}", msg);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("rigor daemon: TLS handshake failed: {}", e);
                        }
                    }
                });
            }
        });

        eprintln!("rigor daemon: websocket at ws://127.0.0.1:{}/ws", port);
        eprintln!("rigor daemon: Ctrl+C to stop");

        // Wait for either listener to finish (shouldn't happen normally)
        tokio::select! {
            _ = http_handle => {},
            _ = tls_handle => {},
        }
    });

    Ok(())
}

pub fn build_router(state: SharedState) -> Router {
    use crate::cli::web;

    let event_tx = state.lock().unwrap().event_tx.clone();

    Router::new()
        // Known LLM API proxy routes (with claim extraction + constraint evaluation)
        .route("/v1/messages", post(proxy::anthropic_proxy))
        .route("/v1/chat/completions", post(proxy::openai_proxy))
        // OpenCode Zen provider routes (same format, prefixed path)
        .route("/zen/v1/messages", post(proxy::opencode_zen_messages_proxy))
        .route("/zen/v1/responses", post(proxy::opencode_zen_responses_proxy))
        // Governance API endpoints
        .route("/api/governance/constraints", get(governance::list_constraints))
        .route("/api/governance/constraints/{id}/toggle", post(governance::toggle_constraint))
        .route("/api/governance/pause", post(governance::toggle_pause))
        .route("/api/governance/block-next", post(governance::toggle_block_next))
        // Gate API endpoints
        .route("/api/gate/register-snapshot", post(gate_api::register_snapshot))
        .route("/api/gate/tool-completed", post(gate_api::tool_completed))
        .route("/api/gate/decision/{session_id}", get(gate_api::get_decision))
        .route("/api/gate/{gate_id}/approve", post(gate_api::approve_gate))
        .route("/api/gate/{gate_id}/reject", post(gate_api::reject_gate))
        // Chat endpoint — lets the dashboard send messages to Claude through rigor's proxy
        .route("/api/chat", post(chat::chat_handler))
        // Catch-all proxy for ANY other API path (Vertex AI, Azure, etc.)
        // This handles LD_PRELOAD intercepted traffic to unknown endpoints
        .fallback(proxy::catch_all_proxy)
        // WebSocket for live events
        .route("/ws", get(ws::ws_handler).with_state(event_tx))
        // Health
        .route("/health", get(|| async { "ok" }))
        // Graph viewer
        .route("/", get(web::serve_index))
        .route(
            "/graph.json",
            get({
                let state_clone = state.clone();
                move || web::graph_json_from_state(state_clone.clone())
            }),
        )
        .route("/assets/{*path}", get(web::serve_viewer_asset))
        .with_state(state)
}
