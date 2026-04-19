use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
// SinkExt is needed for socket.send() in handle_socket
#[allow(unused_imports)]
use futures_util::SinkExt;
use tokio::sync::broadcast;

/// Events broadcast from the daemon to connected WebSocket clients.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type")]
pub enum DaemonEvent {
    Request {
        id: String,
        method: String,
        path: String,
        model: String,
        timestamp: String,
    },
    Response {
        id: String,
        status: u16,
        duration_ms: u64,
    },
    ContextInjected {
        id: String,
        constraints_count: usize,
        violations_count: usize,
        context_preview: String,
        original_system: Option<String>,
    },
    ClaimExtracted {
        id: String,
        text: String,
        confidence: f64,
        claim_type: String,
        request_id: String,
    },
    Violation {
        claim_id: String,
        constraint_id: String,
        severity: String,
        reason: String,
        strength: f64,
    },
    Decision {
        request_id: String,
        decision: String,
        violations: usize,
        claims: usize,
    },
    /// Retry event — emitted when rigor auto-retries after a BLOCK decision
    Retry {
        request_id: String,
        violations: usize,
        status: String, // "retrying" | "retry_success" | "retry_failed"
        message: String,
        /// The violation feedback injected into the system prompt for retry
        #[serde(skip_serializing_if = "Option::is_none")]
        feedback: Option<String>,
        /// The original text that was blocked (before retry)
        #[serde(skip_serializing_if = "Option::is_none")]
        blocked_text: Option<String>,
    },
    /// PII detection event — emitted when PII is found in request or response
    PiiDetected {
        request_id: String,
        direction: String, // "in" | "out"
        pii_type: String,
        matched: String,
        action: String, // "warn" | "block"
    },
    /// Token usage from API response — tracks what we're being charged for
    TokenUsage {
        request_id: String,
        input_tokens: u64,
        output_tokens: u64,
        model: String,
    },
    /// Semantic relevance link between a claim and a constraint (from LLM-as-judge)
    ClaimRelevance {
        claim_id: String,
        constraint_id: String,
        relevance: String,  // "high" | "medium" | "low"
        reason: String,
    },
    /// LLM-as-judge activity — shows when the judge is working and how long it took
    JudgeActivity {
        request_id: String,
        action: String,   // "relevance_start" | "relevance_done" | "retry_verify_start" | "retry_verify_done"
        duration_ms: Option<u64>,
        detail: String,
    },
    /// Full judge evaluation details — prompt, response, claims, constraints
    JudgeEvaluation {
        eval_type: String,    // "relevance" | "retry_verify" | "semantic_eval"
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        response: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        claims: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        constraints: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    /// Accumulated streamed response text — sent periodically so dashboard
    /// can show the actual text that's being evaluated.
    StreamText {
        request_id: String,
        text: String,
    },
    /// Detailed proxy log entry — shows full HTTP request/response flowing through rigor
    ProxyLog {
        id: String,
        timestamp: String,
        direction: String,
        method: String,
        url: String,
        host: String,
        status: Option<u16>,
        content_type: Option<String>,
        body_preview: Option<String>,
        duration_ms: Option<u64>,
        streaming: bool,
        model: Option<String>,
        /// Number of messages in the request (for chat APIs)
        message_count: Option<usize>,
    },
    /// Low-level daemon log — any internal event (TLS accept, handshake, errors, etc.)
    DaemonLog {
        timestamp: String,
        level: String,  // "info" | "warn" | "error" | "debug"
        category: String, // "tls" | "proxy" | "claim" | "policy" | "layer" | "net"
        message: String,
    },
    /// Governance state change — broadcast to all dashboard clients
    GovernanceState {
        action: String,   // "toggle_constraint" | "pause" | "block_next"
        detail: String,
    },
    /// Chat response — from dashboard chat
    ChatResponse {
        chat_id: String,
        chunk: String,
        done: bool,
    },
    /// AI coding agent activity — mirrored for dashboard (Claude Code, OpenCode, etc.)
    AgentEvent {
        request_id: String,
        agent_type: String,  // "claude_code" | "opencode"
        event_type: String,  // "tool_use" | "text" | "thinking" | "session_start" | "session_end"
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// Claude Code activity — mirrored for dashboard (legacy, kept for compatibility)
    ClaudeCodeEvent {
        request_id: String,
        event_type: String,  // "tool_use" | "text" | "thinking"
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
    },
    /// Action gate fired — AI proposed an unsolicited action, awaiting approval
    ActionGate {
        request_id: String,
        gate_id: String,
        gate_type: String,  // "realtime" | "retroactive"
        action_text: String,
        user_message: String,
        reason: String,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        revertable_paths: Vec<String>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        non_revertable: Vec<String>,
    },
    /// Action gate decision — approved or rejected
    ActionGateDecision {
        gate_id: String,
        approved: bool,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        reverted_paths: Vec<String>,
    },
}

/// The type of AI coding agent being grounded by `rigor ground`.
/// Used for observability tagging (OTEL spans, dashboard events, logs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroundedClient {
    ClaudeCode,
    OpenCode,
    Unknown,
}

impl GroundedClient {
    pub fn as_str(&self) -> &'static str {
        match self {
            GroundedClient::ClaudeCode => "claude_code",
            GroundedClient::OpenCode => "opencode",
            GroundedClient::Unknown => "unknown",
        }
    }
}

/// Global grounded client type — set once during `rigor ground` startup.
static GROUNDED_CLIENT: std::sync::OnceLock<GroundedClient> = std::sync::OnceLock::new();

/// Set the grounded client type. Called from `rigor ground` after detecting
/// the target command.
pub fn set_grounded_client(client: GroundedClient) {
    let _ = GROUNDED_CLIENT.set(client);
}

/// Get the grounded client type.
pub fn grounded_client() -> &'static GroundedClient {
    GROUNDED_CLIENT.get_or_init(|| GroundedClient::Unknown)
}

/// Global quiet flag — when true, info-level eprintln is suppressed.
/// Warnings and errors always print. The WebSocket broadcast is unaffected,
/// so the dashboard still receives all events.
pub static QUIET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set the global quiet flag. Called once from `rigor ground --quiet`.
pub fn set_quiet(q: bool) {
    QUIET.store(q, std::sync::atomic::Ordering::Relaxed);
}

/// Returns true if rigor should suppress info-level terminal output.
pub fn is_quiet() -> bool {
    QUIET.load(std::sync::atomic::Ordering::Relaxed)
}

/// Global MITM flag — when true, the CONNECT handler attempts to MITM
/// LLM endpoints. When false (default), all CONNECT tunnels are blind-tunneled
/// for maximum compatibility with OAuth, cert pinning, and other security flows.
pub static MITM_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set the global MITM flag. Called once from `rigor ground --mitm`.
pub fn set_mitm_enabled(m: bool) {
    MITM_ENABLED.store(m, std::sync::atomic::Ordering::Relaxed);
}

/// Returns true if MITM is enabled for LLM endpoint hosts.
pub fn is_mitm_enabled() -> bool {
    MITM_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Global TRANSPARENT flag — when true, the layer redirects ALL outbound
/// port 443 connections to rigor's TLS listener instead of relying on
/// HTTPS_PROXY env var. Required for clients (Claude Code) that disable
/// OAuth when they detect HTTPS_PROXY in the environment.
pub static TRANSPARENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_transparent(t: bool) {
    TRANSPARENT.store(t, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_transparent() -> bool {
    TRANSPARENT.load(std::sync::atomic::Ordering::Relaxed)
}

/// Print an info-level message to stderr (which is the log file after dup2).
/// Always writes to the log file. The quiet flag is irrelevant here because
/// stderr IS the log file — the original terminal is on saved_stderr which
/// only the child process uses.
#[macro_export]
macro_rules! info_println {
    ($($arg:tt)*) => {
        eprintln!($($arg)*)
    };
}

/// Helper to emit a daemon log event to both stderr (log file) and WebSocket.
/// In quiet mode, stderr is the log file (via dup2 redirect), so we ALWAYS
/// write to it. The quiet flag only affects the original terminal (which the
/// child process uses). This was a bug: quiet was suppressing log file writes.
pub fn emit_log(event_tx: &EventSender, level: &str, category: &str, message: impl Into<String>) {
    let msg = message.into();
    // Always write to stderr (which is the log file after dup2 redirect)
    eprintln!("rigor {}: [{}] {}", category, level, msg);
    let _ = event_tx.send(DaemonEvent::DaemonLog {
        timestamp: chrono::Utc::now().to_rfc3339(),
        level: level.to_string(),
        category: category.to_string(),
        message: msg,
    });
}

pub type EventSender = broadcast::Sender<DaemonEvent>;

/// Create a broadcast channel for daemon events.
pub fn create_event_channel() -> (EventSender, broadcast::Receiver<DaemonEvent>) {
    broadcast::channel(256)
}

/// WebSocket upgrade handler. Subscribes to the event broadcast channel
/// and forwards serialized events to the connected client.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(tx): State<EventSender>,
) -> Response {
    let rx = tx.subscribe();
    ws.on_upgrade(|socket| handle_socket(socket, rx))
}

async fn handle_socket(mut socket: WebSocket, mut rx: broadcast::Receiver<DaemonEvent>) {
    // Forward broadcast events to this WebSocket client
    loop {
        match rx.recv().await {
            Ok(event) => {
                let json = match serde_json::to_string(&event) {
                    Ok(j) => j,
                    Err(e) => {
                        eprintln!("rigor ws: failed to serialize event: {}", e);
                        continue;
                    }
                };
                if socket.send(Message::Text(json.into())).await.is_err() {
                    // Client disconnected
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                eprintln!("rigor ws: client lagged, skipped {} events", n);
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => {
                break;
            }
        }
    }
}
