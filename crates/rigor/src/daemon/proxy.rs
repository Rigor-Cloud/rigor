use std::collections::HashMap;
use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use http::header;


use crate::claim::{ClaimExtractor, HeuristicExtractor};
use crate::claim::transcript::TranscriptMessage;
use crate::constraint::types::EpistemicType;
use crate::info_println;
use crate::policy::{EvaluationInput, PolicyEngine};
use crate::violation::{collect_violations, determine_decision, ConstraintMeta, Decision, SeverityThresholds};
use super::context::build_epistemic_context;
use super::ws::{DaemonEvent, EventSender};
use super::SharedState;

/// Apply the correct auth header for a daemon-originated LLM call.
///
/// Different providers use different header schemes:
/// - Anthropic API keys (`sk-ant-apiNN-*` where NN is a version number
///   like `01`, `03`) → `x-api-key: <key>`
/// - Anthropic OAuth tokens (`sk-ant-oatNN-*`, used by Claude Max/Pro)
///   → `Authorization: Bearer`
/// - OpenRouter (`sk-or-*`) → `Authorization: Bearer`
/// - OpenAI (`sk-proj-*`, `sk-*`) → `Authorization: Bearer`
/// - Vertex / other tokens → `Authorization: Bearer`
///
/// Prefix check is `sk-ant-api` WITHOUT a trailing hyphen — real Anthropic
/// API keys are `sk-ant-api03-abcd...` (the version digits come before the
/// dash). The original check `sk-ant-api-` missed those entirely and sent
/// them through Bearer, causing 401. Test coverage caught this regression.
fn apply_provider_auth(
    req: reqwest::RequestBuilder,
    api_key: &str,
) -> reqwest::RequestBuilder {
    if api_key.starts_with("sk-ant-api") {
        req.header("x-api-key", api_key)
    } else {
        req.header("authorization", format!("Bearer {}", api_key))
    }
}

/// Replace the content text of the most recent user-role message in a request
/// body with `replacement`. Used by the PII-IN path to swap detected secrets
/// out of the outbound payload before it reaches the API.
///
/// Handles two content shapes:
/// - OpenAI / simple Anthropic: `content` is a plain string
/// - Structured Anthropic: `content` is an array of content blocks; we find
///   the first `{"type": "text", "text": "..."}` block and replace its text
///
/// If the latest user message doesn't match either shape, this is a no-op
/// (we'd rather skip redaction than malform the request).
fn replace_last_user_content(body: &mut serde_json::Value, replacement: &str) {
    let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return;
    };
    for msg in messages.iter_mut().rev() {
        if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }
        let Some(content) = msg.get_mut("content") else {
            return;
        };
        if content.is_string() {
            *content = serde_json::Value::String(replacement.to_string());
            return;
        }
        if let Some(blocks) = content.as_array_mut() {
            for block in blocks.iter_mut() {
                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(text) = block.get_mut("text") {
                        if text.is_string() {
                            *text = serde_json::Value::String(replacement.to_string());
                            return;
                        }
                    }
                }
            }
        }
        return;
    }
}

/// Cached PII sanitizer — regex patterns compiled once, reused across all requests.
///
/// We use `Sanitizer::default()` rather than `Sanitizer::builder().build()` —
/// the bare builder enables *zero* detectors, so `detect()` returns empty for
/// every input. This was a silent latent defect for months: the PII-IN wire
/// in the proxy fired on exactly no occasions because the detector was empty.
///
/// `default()` enables all built-in detectors (email, credit card w/ Luhn,
/// phone, IPv4/6, generic API keys). Extend via `.custom(name, regex)` below
/// for rigor-specific secrets that aren't covered — OpenRouter keys, GitHub
/// PATs, Anthropic OAuth tokens, etc.
static PII_SANITIZER: std::sync::LazyLock<sanitize_pii::Sanitizer> =
    std::sync::LazyLock::new(|| {
        // Detectors tuned for a BLOCKING context (UserPromptSubmit hook +
        // proxy-layer redact). Precision over recall: we'd rather miss an
        // exotic secret than block a prompt mentioning "1.2.3.4" or a
        // commit hash. Intentional omissions vs the full sanitize_pii set:
        //
        //   - phone():     matches many 9–15 digit numbers in docs/logs
        //   - ipv4/ipv6:   match version strings, example addresses
        //   - api_keys():  generic `sk_*` / hex blob pattern — catches
        //                  commit SHAs, request IDs, UUIDs. We'd rather
        //                  rely on our provider-specific customs below.
        //
        // Kept: email (narrow), credit_card (Luhn-validated).
        sanitize_pii::Sanitizer::builder()
            .email()
            .credit_card()
            // Provider-specific secret formats with strict prefixes + high
            // minimum entropy to essentially eliminate false positives:
            .custom(
                "AnthropicOAuth",
                r"sk-ant-oat\d+-[A-Za-z0-9_-]{32,}",
            )
            .custom(
                "AnthropicApiKey",
                r"sk-ant-api\d+-[A-Za-z0-9_-]{32,}",
            )
            .custom(
                "OpenRouter",
                r"sk-or-v\d+-[A-Za-z0-9]{48,}",
            )
            .custom(
                "OpenAIProject",
                // Real OpenAI project keys are ~164 chars. Bump minimum to
                // rule out abbreviations like "sk-proj-demo" in docs.
                r"sk-proj-[A-Za-z0-9_-]{40,}",
            )
            .custom(
                "GitHubPAT",
                // Real GitHub PATs are exactly 36 alnum chars after prefix.
                // Require the full length, no more no less.
                r"gh[pousr]_[A-Za-z0-9]{36}\b",
            )
            .custom(
                "JWT",
                // Require realistic minimum lengths per segment so random
                // base64-ish text doesn't match. Real JWTs have header/
                // payload >= 20 chars and a signature segment >= 32.
                r"eyJ[A-Za-z0-9_-]{16,}\.eyJ[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}",
            )
            .custom(
                "PrivateKey",
                r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
            )
            .custom(
                // US SSN hyphen-separated only. Bounded by word boundaries
                // so it doesn't match fragments inside phone numbers.
                "SSN",
                r"\b\d{3}-\d{2}-\d{4}\b",
            )
            .custom(
                // Real Slack tokens are 50+ chars. Loose {10,} matched every
                // `xoxp-` chatter in docs and examples.
                "SlackToken",
                r"xox[abprs]-[A-Za-z0-9-]{40,}",
            )
            .custom(
                // DB URL with embedded creds. Require a non-empty user AND
                // non-empty password AND host, so a bare `postgres://host`
                // (no auth) doesn't match.
                "DatabaseURL",
                r"(?:postgres|postgresql|mysql|mongodb(?:\+srv)?)://[^:\s/]+:[^@\s]+@[^\s]+",
            )
            .build()
    });

/// Scan text for PII using sanitize-pii. Returns list of (kind, matched_text) pairs.
/// Made `pub` so `rigor scan` CLI can reuse the same detector rigor uses on live
/// proxy traffic — one rule set, one source of truth.
pub fn detect_pii(text: &str) -> Vec<(String, String)> {
    PII_SANITIZER.detect(text)
        .into_iter()
        .map(|d| (format!("{:?}", d.kind), d.matched.clone()))
        .collect()
}

/// Return `text` with every detected PII/secret masked out by the sanitize-pii
/// crate's sanitize() (typically asterisk-infill preserving prefix/suffix).
/// Used by the UserPromptSubmit hook to offer the user a redacted version of
/// their prompt they can copy-paste and resubmit.
pub fn sanitize_pii_text(text: &str) -> String {
    PII_SANITIZER.sanitize(text)
}

/// Redact a secret value for display/transmission. Keeps the first 4 and
/// last 4 characters so the user can recognize *which* value triggered a
/// finding without the raw string leaving the detector. Short values are
/// fully starred.
///
/// This is the ONE gate between "rigor has detected a secret" and "rigor's
/// observability layer (dashboard WebSocket, logs, violation events)
/// transmits something downstream." Every PiiDetected event must run its
/// matched value through this function — otherwise the detector itself
/// becomes the leak vector.
pub fn redact_for_display(s: &str) -> String {
    let s = s.trim();
    if s.len() <= 12 {
        "*".repeat(s.len())
    } else {
        format!("{}...{}", &s[..4], &s[s.len() - 4..])
    }
}

/// Prettify the `kind` string from `detect_pii` for user-facing output.
///
/// The `sanitize_pii` crate returns Debug-formatted kinds like
/// `Custom("OpenRouter")` or `Email`. That wrapper is meaningful to code but
/// ugly in error messages. This strips the `Custom("...")` wrapper so hook
/// reasons say `[OpenRouter]` instead of `[Custom("OpenRouter")]`.
pub fn redaction_placeholder(kind: &str) -> String {
    format!("[REDACTED:{}]", prettify_kind(kind))
}

pub fn redact_with_tags(text: &str) -> String {
    let mut findings = detect_pii(text);
    findings.sort_by_key(|(_, m)| std::cmp::Reverse(m.len()));
    let mut out = text.to_string();
    for (kind, matched) in findings {
        out = out.replace(&matched, &redaction_placeholder(&kind));
    }
    out
}

pub fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() { return 0.0; }
    let mut counts = std::collections::HashMap::new();
    for c in s.chars() { *counts.entry(c).or_insert(0u64) += 1; }
    let len = s.chars().count() as f64;
    counts.values().map(|&c| { let p = c as f64 / len; -p * p.log2() }).sum()
}

pub fn context_suggests_false_positive(text: &str, matched: &str) -> bool {
    let Some(pos) = text.find(matched) else { return false };
    let start = pos.saturating_sub(60);
    let end = (pos + matched.len() + 60).min(text.len());
    let ctx = text[start..end].to_lowercase();
    const FP: &[&str] = &[
        "example", "e.g.", "for instance", "version", "commit", "hash",
        "uuid", "guid", "placeholder", "sample", "test", "dummy", "fake",
        "foo", "bar", "baz", "demo", "release",
    ];
    FP.iter().any(|m| ctx.contains(m))
}

pub fn is_likely_real_secret(text: &str, matched: &str) -> bool {
    let entropy = shannon_entropy(matched);
    if entropy >= 3.5 { return true; }
    !context_suggests_false_positive(text, matched)
}

pub fn prettify_kind(kind: &str) -> String {
    if let Some(inner) = kind.strip_prefix("Custom(\"").and_then(|s| s.strip_suffix("\")")) {
        inner.to_string()
    } else {
        kind.to_string()
    }
}

/// Proxy Anthropic API requests: inject epistemic context, forward, stream back.
pub async fn anthropic_proxy(
    State(state): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    proxy_request(state, headers, body, "/v1/messages").await
}

/// Proxy OpenAI API requests: inject epistemic context, forward, stream back.
pub async fn openai_proxy(
    State(state): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    proxy_request(state, headers, body, "/v1/chat/completions").await
}

/// Proxy OpenCode Zen Anthropic-format requests (same lifecycle as anthropic_proxy).
pub async fn opencode_zen_messages_proxy(
    State(state): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    proxy_request(state, headers, body, "/zen/v1/messages").await
}

/// Proxy OpenCode Zen OpenAI-format requests (same lifecycle as openai_proxy).
pub async fn opencode_zen_responses_proxy(
    State(state): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    proxy_request(state, headers, body, "/zen/v1/responses").await
}

/// Catch-all proxy: forward ANY request to its original destination.
/// Handles CONNECT tunnels (for HTTPS_PROXY mode) and direct requests.
pub async fn catch_all_proxy(
    State(state): State<SharedState>,
    req: axum::extract::Request,
) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();

    // Handle CONNECT method (HTTP tunnel for HTTPS_PROXY mode)
    // We MITM the tunnel by terminating TLS ourselves, then serving the same
    // axum router over TLS — this gives us full visibility into HTTPS requests
    // and lets the existing proxy handlers inject epistemic context.
    if method == http::Method::CONNECT {
        let target = uri.authority()
            .map(|a| a.to_string())
            .or_else(|| uri.host().map(|h| {
                let port = uri.port_u16().unwrap_or(443);
                format!("{}:{}", h, port)
            }))
            .unwrap_or_else(|| path.clone());

        // Get the TLS config and event_tx from shared state
        let (rigor_ca, tls_config, event_tx) = {
            let st = state.lock().unwrap();
            (st.rigor_ca.clone(), st.tls_config.clone(), st.event_tx.clone())
        };

        crate::daemon::ws::emit_log(&event_tx, "info", "proxy",
            format!("CONNECT tunnel request → {}", target));

        let _ = event_tx.send(DaemonEvent::ProxyLog {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            direction: "request".to_string(),
            method: "CONNECT".to_string(),
            url: format!("https://{}", target),
            host: target.clone(),
            status: None,
            content_type: None,
            body_preview: None,
            duration_ms: None,
            streaming: false,
            model: None,
            message_count: None,
        });

        let target_clone = target.clone();
        let event_tx_clone = event_tx.clone();
        let should_mitm = super::should_mitm_target(&target);

        crate::daemon::ws::emit_log(&event_tx_clone, "info", "proxy",
            format!("Decision for {}: {}", target,
                if should_mitm { "MITM (LLM endpoint)" } else { "blind tunnel (preserve TLS)" }));

        // Build router only if we'll MITM
        let router = if should_mitm {
            Some(super::build_router(state.clone()))
        } else {
            None
        };

        tokio::spawn(async move {
            let upgraded = match hyper::upgrade::on(req).await {
                Ok(u) => u,
                Err(e) => {
                    crate::daemon::ws::emit_log(&event_tx_clone, "error", "proxy",
                        format!("CONNECT upgrade failed for {}: {}", target_clone, e));
                    return;
                }
            };

            crate::daemon::ws::emit_log(&event_tx_clone, "info", "proxy",
                format!("CONNECT upgraded for {}", target_clone));

            let mut upgraded_io = hyper_util::rt::TokioIo::new(upgraded);

            // BLIND TUNNEL PATH (mirrord pattern): bytes flow unchanged.
            // Used for non-LLM endpoints — OAuth, telemetry, CDNs, etc.
            // Preserves end-to-end TLS so the original cert stays intact and
            // sensitive flows like OAuth aren't disrupted.
            if router.is_none() {
                let mut upstream = match tokio::net::TcpStream::connect(&target_clone).await {
                    Ok(s) => s,
                    Err(e) => {
                        crate::daemon::ws::emit_log(&event_tx_clone, "error", "net",
                            format!("Blind tunnel upstream connect failed for {}: {}", target_clone, e));
                        return;
                    }
                };

                crate::daemon::ws::emit_log(&event_tx_clone, "info", "net",
                    format!("Blind tunnel established to {}", target_clone));

                match tokio::io::copy_bidirectional(&mut upgraded_io, &mut upstream).await {
                    Ok((from_client, from_upstream)) => {
                        crate::daemon::ws::emit_log(&event_tx_clone, "info", "proxy",
                            format!("Blind tunnel closed: {} ({}B out, {}B in)",
                                target_clone, from_client, from_upstream));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if !msg.contains("connection reset") && !msg.contains("broken pipe") {
                            crate::daemon::ws::emit_log(&event_tx_clone, "warn", "proxy",
                                format!("Blind tunnel copy error for {}: {}", target_clone, e));
                        }
                    }
                }
                return;
            }

            // MITM PATH: terminate TLS with a per-host cert signed by our CA.
            // Try CA first (proper cert chain), fall back to legacy multi-SAN cert.
            let sni_host = target_clone.split(':').next().unwrap_or("unknown");
            let server_cfg = if let Some(ref ca) = rigor_ca {
                match ca.server_config_for_host(sni_host) {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        crate::daemon::ws::emit_log(&event_tx_clone, "warn", "tls",
                            format!("CA cert generation failed for {}: {} (trying legacy)", sni_host, e));
                        match tls_config {
                            Some(ref cfg) => cfg.clone(),
                            None => {
                                crate::daemon::ws::emit_log(&event_tx_clone, "error", "tls",
                                    format!("No TLS config for MITM of {}", target_clone));
                                return;
                            }
                        }
                    }
                }
            } else {
                match tls_config {
                    Some(ref cfg) => cfg.clone(),
                    None => {
                        crate::daemon::ws::emit_log(&event_tx_clone, "error", "tls",
                            format!("No TLS config for MITM of {}", target_clone));
                        return;
                    }
                }
            };

            let acceptor = tokio_rustls::TlsAcceptor::from(server_cfg);
            let tls_stream = match acceptor.accept(upgraded_io).await {
                Ok(s) => s,
                Err(e) => {
                    crate::daemon::ws::emit_log(&event_tx_clone, "warn", "tls",
                        format!("MITM handshake failed for {}: {}", target_clone, e));
                    return;
                }
            };

            crate::daemon::ws::emit_log(&event_tx_clone, "info", "tls",
                format!("MITM TLS handshake OK for {}", target_clone));

            let tls_io = hyper_util::rt::TokioIo::new(tls_stream);

            // Serve the axum router on the decrypted TLS stream.
            let tower_service = router.unwrap();
            let log_tx = event_tx_clone.clone();
            let target_for_svc = target_clone.clone();
            let service = hyper::service::service_fn(move |mut req: hyper::Request<hyper::body::Incoming>| {
                let mut router = tower_service.clone();
                let log_tx = log_tx.clone();
                let target = target_for_svc.clone();

                // Ensure the Host header carries the CONNECT target so proxy handlers
                // forward to the right upstream.
                if !req.headers().contains_key(http::header::HOST) {
                    if let Some(host_only) = target.split(':').next() {
                        if let Ok(hv) = http::HeaderValue::from_str(host_only) {
                            req.headers_mut().insert(http::header::HOST, hv);
                        }
                    }
                }

                let method = req.method().to_string();
                let path = req.uri().path().to_string();

                async move {
                    crate::daemon::ws::emit_log(&log_tx, "info", "proxy",
                        format!("MITM request: {} {} (via {})", method, path, target));
                    use tower::Service;
                    let (parts, body) = req.into_parts();
                    let body = axum::body::Body::new(body);
                    let req = hyper::Request::from_parts(parts, body);
                    router.call(req).await.map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                    })
                }
            });

            if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                hyper_util::rt::TokioExecutor::new(),
            )
            .serve_connection(tls_io, service)
            .await
            {
                let msg = e.to_string();
                if !msg.contains("connection closed") && !msg.contains("broken pipe") {
                    crate::daemon::ws::emit_log(&event_tx_clone, "warn", "proxy",
                        format!("MITM HTTP error for {}: {}", target_clone, msg));
                }
            }

            crate::daemon::ws::emit_log(&event_tx_clone, "info", "proxy",
                format!("MITM tunnel closed: {}", target_clone));
        });

        // Return 200 immediately so the client sees the tunnel is open
        return Response::builder()
            .status(200)
            .body(Body::empty())
            .unwrap_or_else(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Tunnel error: {}", e)).into_response()
            });
    }

    let (parts, body) = req.into_parts();
    let headers = parts.headers;

    // Read the body
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Failed to read body: {}", e)).into_response();
        }
    };

    let host_str = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or("unknown").to_string();

    {
        let st = state.lock().unwrap();
        crate::daemon::ws::emit_log(&st.event_tx, "info", "proxy",
            format!("catch-all: {} {} host={}", method, path, host_str));
    }

    // Emit ProxyLog for ALL catch-all requests
    {
        let st = state.lock().unwrap();
        let _ = st.event_tx.send(DaemonEvent::ProxyLog {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            direction: "request".to_string(),
            method: method.to_string(),
            url: format!("https://{}{}", host_str, path),
            host: host_str.clone(),
            status: None,
            content_type: headers.get("content-type").and_then(|v| v.to_str().ok()).map(|s| s.to_string()),
            body_preview: if body_bytes.len() > 0 {
                Some(String::from_utf8_lossy(&body_bytes[..body_bytes.len().min(500)]).to_string())
            } else {
                None
            },
            duration_ms: None,
            streaming: false,
            model: None,
            message_count: None,
        });
    }

    info_println!("rigor catch-all: {} {} (host: {})", method, path, host_str);

    // For POST requests to actual LLM endpoints, treat as proxy + constraint injection.
    // Use stricter path matching: only the real LLM completion endpoints, NOT
    // telemetry endpoints like /api/event_logging/v2/batch or /api/oauth/*.
    let is_llm_endpoint = method == http::Method::POST && (
        path == "/v1/messages"                          // Anthropic
        || path == "/v1/chat/completions"               // OpenAI
        || path == "/v1/completions"                    // OpenAI legacy
        || path == "/zen/v1/messages"                   // OpenCode Zen (Anthropic format)
        || path == "/zen/v1/responses"                  // OpenCode Zen (OpenAI Responses format)
        || path == "/zen/v1/chat/completions"           // OpenCode Zen (OpenAI-compatible format)
        || path.ends_with("/v1/messages")               // Any provider-prefixed Anthropic route
        || path.ends_with("/v1/chat/completions")       // Any provider-prefixed OpenAI route
        || path.ends_with("/v1/responses")              // Any provider-prefixed OpenAI Responses route
        || path.ends_with(":generateContent")           // Vertex AI / Gemini
        || path.ends_with(":streamGenerateContent")     // Vertex AI streaming
        || path.ends_with(":predict")                   // Vertex AI prediction
        || path.ends_with(":streamRawPredict")          // Vertex AI streaming raw
    );

    // x-rigor-internal header allows opting out of constraint evaluation
    // for rigor's own LLM calls (e.g., relevance scoring).
    // Default: rigor evaluates everything, including its own calls.
    // Set RIGOR_SKIP_INTERNAL=1 to bypass evaluation on internal calls.
    let is_internal = headers.get("x-rigor-internal").is_some();
    let skip_internal = std::env::var("RIGOR_SKIP_INTERNAL").is_ok();

    if is_llm_endpoint && !(is_internal && skip_internal) {
        return proxy_request(state, headers, body_bytes.into(), &path).await;
    }

    // For everything else (OAuth, telemetry, account settings, etc.) forward transparently.
    // CRITICAL: this path handles auth flows on MITM'd hosts (e.g. api.anthropic.com/api/oauth/*).
    // We must preserve cookies, auth headers, status codes, and Set-Cookie response headers
    // exactly so the client's session state survives the round trip.
    let upstream_host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .or_else(|| uri.host())
        .unwrap_or("unknown");
    let target_url = format!("https://{}{}", upstream_host, path);

    if !path.contains("health") {
        info_println!("rigor proxy: passthrough {} {} → {}", method, path, target_url);
    }

    // Use shared HTTP client for connection pooling across all requests.
    let passthrough_client = {
        let st = state.lock().unwrap();
        st.http_client.clone()
    };
    let mut req_builder = match method {
        http::Method::GET => passthrough_client.get(&target_url),
        http::Method::POST => passthrough_client.post(&target_url),
        http::Method::PUT => passthrough_client.put(&target_url),
        http::Method::DELETE => passthrough_client.delete(&target_url),
        http::Method::PATCH => passthrough_client.patch(&target_url),
        m => passthrough_client.request(m.clone(), &target_url),
    };

    // Forward all client headers EXCEPT hop-by-hop and headers we manage ourselves.
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();
        match name_str.as_str() {
            // Hop-by-hop / managed-by-reqwest
            "host" | "content-length" | "transfer-encoding" | "connection" | "upgrade" => {}
            _ => {
                req_builder = req_builder.header(name.clone(), value.clone());
            }
        }
    }

    if !body_bytes.is_empty() {
        req_builder = req_builder.body(body_bytes.to_vec());
    }

    match req_builder.send().await {
        Ok(response) => {
            let status = response.status();
            let resp_headers = response.headers().clone();

            // Buffer the full body — reqwest's auto-decompressor only fires
            // for .bytes() / .text(), not for .bytes_stream(). For OAuth and
            // session endpoints we want decompressed bytes so the client gets
            // a coherent response (and so we don't double-decompress headers).
            let body_bytes = match response.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    return (StatusCode::BAD_GATEWAY, format!("Upstream body error: {}", e)).into_response();
                }
            };

            let mut builder = Response::builder().status(status.as_u16());
            for (name, value) in resp_headers.iter() {
                let name_str = name.as_str().to_lowercase();
                match name_str.as_str() {
                    // Hop-by-hop headers — don't forward
                    "transfer-encoding" | "connection" | "upgrade" => {}
                    // Length and encoding will be wrong because we decompressed.
                    // Stripping content-encoding tells the client the body is plain.
                    // Stripping content-length lets axum/hyper recompute it.
                    "content-encoding" | "content-length" => {}
                    _ => {
                        builder = builder.header(name.clone(), value.clone());
                    }
                }
            }
            builder
                .body(Body::from(body_bytes))
                .unwrap_or_else(|e| {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("Body build error: {}", e)).into_response()
                })
        }
        Err(e) => {
            (StatusCode::BAD_GATEWAY, format!("Upstream error: {}", e)).into_response()
        }
    }
}

/// Decompress an HTTP body based on the Content-Encoding header.
/// Handles gzip, deflate, and brotli. Returns the original bytes if no encoding
/// is set or if decompression fails (with a warning logged).
fn decompress_body(body: &[u8], encoding: Option<&str>) -> Vec<u8> {
    use std::io::Read;
    match encoding.map(|s| s.to_lowercase()) {
        Some(enc) if enc == "gzip" || enc == "x-gzip" => {
            let mut decoder = flate2::read::GzDecoder::new(body);
            let mut out = Vec::new();
            match decoder.read_to_end(&mut out) {
                Ok(_) => out,
                Err(e) => {
                    eprintln!("rigor proxy: gzip decompress failed: {} (using raw body)", e);
                    body.to_vec()
                }
            }
        }
        Some(enc) if enc == "deflate" => {
            let mut decoder = flate2::read::ZlibDecoder::new(body);
            let mut out = Vec::new();
            match decoder.read_to_end(&mut out) {
                Ok(_) => out,
                Err(e) => {
                    eprintln!("rigor proxy: deflate decompress failed: {} (using raw body)", e);
                    body.to_vec()
                }
            }
        }
        Some(enc) if enc == "br" || enc == "brotli" => {
            let mut decoder = brotli::Decompressor::new(body, 4096);
            let mut out = Vec::new();
            match decoder.read_to_end(&mut out) {
                Ok(_) => out,
                Err(e) => {
                    eprintln!("rigor proxy: brotli decompress failed: {} (using raw body)", e);
                    body.to_vec()
                }
            }
        }
        _ => body.to_vec(),
    }
}

async fn proxy_request(
    state: SharedState,
    headers: HeaderMap,
    body: Bytes,
    path: &str,
) -> Response {
    // Governance: check if proxy is paused
    let (proxy_paused, disabled_constraints) = {
        let st = state.lock().unwrap();
        (st.proxy_paused, st.disabled_constraints.clone())
    };

    if proxy_paused {
        // When paused, forward without any evaluation — just raw proxy
        let (target_api, http_client) = {
            let st = state.lock().unwrap();
            (st.target_api.clone(), st.http_client.clone())
        };
        let target_url = format!("{}{}", target_api, path);
        let mut req = http_client.post(&target_url);
        for (name, value) in headers.iter() {
            let n = name.as_str().to_lowercase();
            if n != "host" && n != "content-length" && n != "transfer-encoding" && n != "connection" {
                req = req.header(name.clone(), value.clone());
            }
        }
        req = req.body(body.to_vec());
        return match req.send().await {
            Ok(resp) => {
                let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
                match resp.bytes().await {
                    Ok(b) => (status, b.to_vec()).into_response(),
                    Err(e) => (StatusCode::BAD_GATEWAY, format!("error: {}", e)).into_response(),
                }
            }
            Err(e) => (StatusCode::BAD_GATEWAY, format!("error: {}", e)).into_response(),
        };
    }

    // Retroactive action gate: block this request if a pending gate matches this session
    let session_id_for_block = {
        use sha2::{Sha256, Digest};
        let auth = headers.get("x-api-key")
            .or_else(|| headers.get("authorization"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("no-auth");
        let mut hasher = Sha256::new();
        hasher.update(auth.as_bytes());
        let result = hasher.finalize();
        let hex: String = result.iter().map(|b| format!("{:02x}", b)).collect();
        hex[..16].to_string()
    };

    let pending_gate_id = {
        let st = state.lock().unwrap();
        st.action_gates.iter()
            .find(|(_, e)| matches!(e.gate_type, crate::daemon::GateType::Retroactive)
                && e.session_id == session_id_for_block)
            .map(|(id, _)| id.clone())
    };

    if let Some(gate_id) = pending_gate_id {
        let deadline = std::time::Instant::now() +
            std::time::Duration::from_secs(crate::daemon::gate::GATE_TIMEOUT_SECS);
        loop {
            let decision = {
                let st = state.lock().unwrap();
                st.gate_decisions.get(&session_id_for_block).cloned()
            };

            if let Some(d) = decision {
                if d.gate_id == gate_id {
                    let mut st = state.lock().unwrap();
                    st.action_gates.remove(&gate_id);
                    st.gate_decisions.remove(&session_id_for_block);
                    let event_tx_log = st.event_tx.clone();
                    drop(st);
                    if !d.approved {
                        crate::daemon::ws::emit_log(&event_tx_log, "info", "gate",
                            format!("Retroactive gate {} rejected", gate_id));
                    } else {
                        crate::daemon::ws::emit_log(&event_tx_log, "info", "gate",
                            format!("Retroactive gate {} approved", gate_id));
                    }
                    break;
                }
            }

            if std::time::Instant::now() >= deadline {
                let mut st = state.lock().unwrap();
                st.action_gates.remove(&gate_id);
                let event_tx_log = st.event_tx.clone();
                drop(st);
                crate::daemon::ws::emit_log(&event_tx_log, "warn", "gate",
                    format!("Retroactive gate {} timeout — auto-rejected", gate_id));
                break;
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    let start = Instant::now();
    let request_id = uuid::Uuid::new_v4().to_string();

    info_println!("rigor proxy_request: path={}", path);

    // Decompress the request body if it's encoded (gzip/br/deflate).
    // Bun and other modern HTTP clients compress request bodies by default.
    let content_encoding = headers.get("content-encoding").and_then(|v| v.to_str().ok());
    let body_decoded = decompress_body(&body, content_encoding);
    if content_encoding.is_some() {
        info_println!("rigor proxy: decoded {} request body ({} → {} bytes)",
            content_encoding.unwrap_or("none"), body.len(), body_decoded.len());
    }

    // Parse the request body
    let mut body_json: serde_json::Value = match serde_json::from_slice(&body_decoded) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rigor proxy: failed to parse request body: {} (first 80 bytes: {:?})",
                e, &body_decoded.iter().take(80).copied().collect::<Vec<u8>>());
            return (StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)).into_response();
        }
    };

    let is_streaming = body_json
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let model = body_json
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    eprintln!("rigor proxy: request model={}", model);

    let user_message = body_json.get("messages")
        .and_then(|m| m.as_array())
        .and_then(|msgs| msgs.iter().rev().find(|m|
            m.get("role").and_then(|r| r.as_str()) == Some("user")))
        .and_then(|m| m.get("content").and_then(|c| c.as_str()))
        .unwrap_or("")
        .to_string();

    let session_id = {
        use sha2::{Sha256, Digest};
        let auth = headers.get("x-api-key")
            .or_else(|| headers.get("authorization"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("no-auth");
        let mut hasher = Sha256::new();
        hasher.update(auth.as_bytes());
        let result = hasher.finalize();
        let hex: String = result.iter().map(|b| format!("{:02x}", b)).collect();
        hex[..16].to_string()
    };

    // Count messages in request body (for chat APIs)
    let message_count = body_json
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|a| a.len());

    // Build a preview of the last user message for the proxy log
    let last_user_msg = body_json
        .get("messages")
        .and_then(|m| m.as_array())
        .and_then(|msgs| msgs.iter().rev().find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user")))
        .and_then(|m| m.get("content").and_then(|c| c.as_str()))
        .map(|s| if s.len() > 200 { format!("{}...", &s[..200]) } else { s.to_string() });

    // Capture API key from proxied request headers (Claude Code uses OAuth,
    // so ANTHROPIC_API_KEY env var may not be set — but the auth token flows
    // through us on every request in x-api-key or Authorization headers).
    let captured_key = headers.get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| headers.get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|s| s.to_string()));

    // Lock state to read config, build context, and get event_tx + shared client
    let (target_api, api_key, epistemic_context, event_tx, constraints_count, http_client, fallback) = {
        let mut st = state.lock().unwrap();
        // Store captured key if we don't have one yet
        if st.api_key.is_none() {
            if let Some(ref key) = captured_key {
                st.api_key = Some(key.clone());
            }
        }
        let ctx = build_epistemic_context(&st.config, &st.graph);
        let count = st.config.all_constraints().len();
        (
            st.target_api.clone(),
            st.api_key.clone(),
            ctx,
            st.event_tx.clone(),
            count,
            st.http_client.clone(),
            st.fallback.clone(),
        )
    };

    // Emit Request event
    let timestamp = chrono::Utc::now().to_rfc3339();
    let _ = event_tx.send(DaemonEvent::Request {
        id: request_id.clone(),
        method: "POST".to_string(),
        path: path.to_string(),
        model: model.clone(),
        timestamp,
    });

    // Capture original system prompt BEFORE injection
    let original_system = if path.contains("messages") {
        // Anthropic: system is top-level string or array
        body_json.get("system").map(|s| {
            if let Some(text) = s.as_str() { text.to_string() }
            else if let Some(arr) = s.as_array() {
                arr.iter().filter_map(|b| b.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n")
            } else { String::new() }
        })
    } else {
        // OpenAI: system is first message with role=system
        body_json.get("messages").and_then(|m| m.as_array()).and_then(|msgs| {
            msgs.iter().find(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
                .and_then(|m| m.get("content").and_then(|c| c.as_str()).map(|s| s.to_string()))
        })
    };

    // PII-IN: detect and REDACT secrets in the outbound user message.
    //
    // Pre-fix: this was a `tokio::spawn` that logged warnings to the dashboard
    // but never modified the request — secrets still reached Anthropic, we
    // just knew about it afterwards. Pre-fix#2: the detector was misconfigured
    // and found nothing anyway. Now: synchronous scan + sanitizer-based
    // rewrite before body_json is serialized and forwarded.
    //
    // Only redacts the top-level string-content case for now (same scope as
    // last_user_msg extraction). Content-block form (Anthropic structured
    // messages) requires extending last_user_msg too — tracked as follow-up.
    if let Some(ref user_msg) = last_user_msg {
        let findings = detect_pii(user_msg);
        if !findings.is_empty() {
            // Typed placeholders like [REDACTED:OpenRouter] instead of stars.
            let redacted = redact_with_tags(user_msg);
            for (kind, matched) in &findings {
                let _ = event_tx.send(DaemonEvent::PiiDetected {
                    request_id: request_id.clone(),
                    direction: "in".to_string(),
                    pii_type: kind.clone(),
                    matched: redact_for_display(matched),
                    action: "redact".to_string(),
                });
            }
            crate::daemon::ws::emit_log(
                &event_tx,
                "warn",
                "pii",
                format!(
                    "PII-IN: redacted {} secret(s) in user message before forwarding",
                    findings.len()
                ),
            );
            replace_last_user_content(&mut body_json, &redacted);
        }
    }

    // Build per-request filter chain and apply via fallback policy
    use super::egress;
    let claim_filter = egress::ClaimInjectionFilter::new(
        epistemic_context.clone(),
        path.to_string(),
    );
    let request_chain = egress::FilterChain::new(vec![
        std::sync::Arc::new(claim_filter),
    ]);

    let modified_body = {
        let chain = request_chain.clone();
        let body_for_chain = body_json.clone();
        match fallback.execute("claim_injection", move || {
            let mut body_clone = body_for_chain.clone();
            let c = chain.clone();
            async move {
                let mut ctx = egress::ConversationCtx::new_anonymous();
                c.apply_request(&mut body_clone, &mut ctx).await
                    .map_err(|e| (crate::fallback::FailureCategory::PersistentError, e.to_string()))?;
                Ok::<_, (crate::fallback::FailureCategory, String)>(body_clone)
            }
        }).await {
            crate::fallback::FallbackOutcome::Ok(body) => body,
            crate::fallback::FallbackOutcome::Blocked(msg) => {
                return (axum::http::StatusCode::BAD_GATEWAY, msg).into_response();
            }
            crate::fallback::FallbackOutcome::Skipped
            | crate::fallback::FallbackOutcome::Degraded(_) => body_json,
        }
    };

    // Emit ContextInjected event with both original system prompt and rigor context
    let _ = event_tx.send(DaemonEvent::ContextInjected {
        id: request_id.clone(),
        constraints_count,
        violations_count: 0,
        context_preview: epistemic_context.clone(),
        original_system,
    });

    // Determine the real upstream from the Host header (for LD_PRELOAD intercepted traffic)
    // or fall back to the configured target_api
    let upstream_host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .map(|h| h.split(':').next().unwrap_or(h).to_string());

    let target_base = if let Some(ref host) = upstream_host {
        // If Host header points to a known API, use HTTPS to forward
        if host.contains("anthropic.com")
            || host.contains("openai.com")
            || host.contains("googleapis.com")
            || host.contains("aiplatform.googleapis.com")
        {
            format!("https://{}", host)
        } else if host == "127.0.0.1" || host == "localhost" {
            // Local traffic — don't re-proxy to ourselves
            target_api.clone()
        } else {
            // Unknown host — forward with HTTPS
            format!("https://{}", host)
        }
    } else {
        target_api.clone()
    };

    let target_url = format!("{}{}", target_base, path);

    // Emit detailed ProxyLog for the request
    let _ = event_tx.send(DaemonEvent::ProxyLog {
        id: request_id.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        direction: "request".to_string(),
        method: "POST".to_string(),
        url: target_url.clone(),
        host: upstream_host.clone().unwrap_or_else(|| "unknown".to_string()),
        status: None,
        content_type: Some("application/json".to_string()),
        body_preview: last_user_msg.clone(),
        duration_ms: None,
        streaming: is_streaming,
        model: Some(model.clone()),
        message_count,
    });
    // Only log to stderr in debug; events go to dashboard via WebSocket
    if std::env::var("RIGOR_DEBUG").is_ok() {
        info_println!("rigor proxy: {} → {} (host: {:?})", path, target_url, upstream_host);
    }

    // Build the forwarding request (using shared client for connection pooling)
    let mut req = http_client.post(&target_url);

    // Forward ALL headers from the original request (full proxy)
    // Skip hop-by-hop headers and content-length (reqwest sets it)
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();
        match name_str.as_str() {
            // Hop-by-hop headers we never forward.
            "host" | "content-length" | "transfer-encoding" | "connection" | "upgrade" => {}
            // Content-Encoding: we already decompressed the body, so the bytes
            // we forward are plain JSON. Stripping this header prevents upstream
            // from trying to gunzip our plain JSON and erroring out.
            "content-encoding" => {}
            // Accept-Encoding: tell upstream we don't want compressed responses.
            // reqwest could decompress them, but stripping this is more robust
            // since we re-emit the body as plain JSON anyway.
            "accept-encoding" => {}
            _ => {
                req = req.header(name.clone(), value.clone());
            }
        }
    }

    req = req
        .header(header::CONTENT_TYPE, "application/json")
        // Explicitly request identity (no compression) so we can parse the response
        // and inject claims without decompression handling on the response path.
        .header(header::ACCEPT_ENCODING, "identity")
        .body(serde_json::to_vec(&modified_body).unwrap());

    // Forward to the real API
    let response = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("rigor proxy: upstream error: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                format!("Failed to reach upstream API: {}", e),
            )
                .into_response();
        }
    };

    let status = response.status();
    let resp_headers = response.headers().clone();
    let resp_content_type = resp_headers.get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Emit ProxyLog for response
    let _ = event_tx.send(DaemonEvent::ProxyLog {
        id: request_id.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        direction: "response".to_string(),
        method: "POST".to_string(),
        url: target_url.clone(),
        host: upstream_host.clone().unwrap_or_else(|| "unknown".to_string()),
        status: Some(status.as_u16()),
        content_type: resp_content_type,
        body_preview: None,
        duration_ms: Some(start.elapsed().as_millis() as u64),
        streaming: is_streaming,
        model: Some(model.clone()),
        message_count: None,
    });

    if std::env::var("RIGOR_DEBUG").is_ok() {
        info_println!("rigor proxy: upstream responded with {}", status);
    }

    // For streaming requests: chunk-level evaluation with killswitch.
    // Forward SSE chunks to client in real-time. Simultaneously accumulate text.
    // At sentence boundaries, check for constraint-relevant keywords. On keyword hit,
    // run full claim extraction + Rego evaluation. If BLOCK: kill the upstream stream
    // (saves tokens), inject an error event into the client stream, close it.
    if is_streaming {
        let duration_ms = start.elapsed().as_millis() as u64;
        let _ = event_tx.send(DaemonEvent::Response {
            id: request_id.clone(),
            status: status.as_u16(),
            duration_ms,
        });

        // Channel from evaluator task → client response body
        let (client_tx, client_rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(64);

        let event_tx_bg = event_tx.clone();
        let request_id_bg = request_id.clone();
        let state_bg = state.clone();
        let path_bg = path.to_string();
        let target_url_bg = target_url.clone();
        let modified_body_bg = modified_body.clone();
        let headers_bg = headers.clone();
        let model_bg = model.clone();
        let http_client_bg = http_client.clone();
        let user_message_bg = user_message.clone();
        let session_id_bg = session_id.clone();

        // Build constraint keywords from config for cheap pre-filter
        let constraint_keywords: Vec<String> = {
            let st = state.lock().unwrap();
            st.config.all_constraints().iter()
                .flat_map(|c| {
                    // Extract keywords from constraint name + description
                    let mut words = Vec::new();
                    for word in c.name.split_whitespace().chain(c.description.split_whitespace()) {
                        let w = word.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string();
                        if w.len() > 3 { words.push(w); }
                    }
                    words
                })
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect()
        };

        // Pre-compute static evaluation data ONCE before the loop
        let (config_bg, strengths_bg, engine_bg, constraint_meta_bg) = {
            let st = state_bg.lock().unwrap();
            let config = st.config.clone();
            let strengths = st.graph.get_all_strengths();
            let engine = st.policy_engine.as_ref().cloned();
            let meta: HashMap<String, ConstraintMeta> = config
                .all_constraints().iter()
                .map(|c| {
                    let etype = match c.epistemic_type {
                        EpistemicType::Belief => "belief",
                        EpistemicType::Justification => "justification",
                        EpistemicType::Defeater => "defeater",
                    };
                    (c.id.clone(), ConstraintMeta {
                        name: c.name.clone(),
                        epistemic_type: etype.to_string(),
                        rego_path: format!("data.rigor.{}", c.id),
                    })
                }).collect();
            (config, strengths, engine, meta)
        };

        // Mark this request as an active stream
        {
            let mut st = state.lock().unwrap();
            st.active_streams.insert(request_id.clone());
        }

        // Evaluator task: reads upstream, forwards to client, evaluates on the fly
        tokio::spawn(async move {
            use futures_util::StreamExt;
            let mut upstream = response.bytes_stream();
            let mut raw_accumulated = Vec::new();
            let mut text_so_far = String::new();
            let mut last_eval_len = 0;
            let mut blocked = false;
            let mut last_stream_text_len: usize = 0; // for throttling StreamText events
            // Track SSE parse position for incremental parsing
            let mut sse_parse_offset: usize = 0;

            while let Some(chunk_result) = upstream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(_) => break,
                };

                raw_accumulated.extend_from_slice(&bytes);

                // Incremental SSE text extraction: only parse NEW lines since last offset
                let raw_text = String::from_utf8_lossy(&raw_accumulated);
                let new_data = &raw_text[sse_parse_offset..];
                // Find the last complete line boundary to avoid parsing partial SSE events
                if let Some(last_newline) = new_data.rfind('\n') {
                    let parseable = &new_data[..last_newline + 1];
                    for line in parseable.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" { break; }
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                                if path_bg.contains("messages") {
                                    if json.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
                                        if let Some(text) = json.get("delta")
                                            .and_then(|d| d.get("text"))
                                            .and_then(|t| t.as_str()) {
                                            text_so_far.push_str(text);
                                        }
                                    }
                                } else if let Some(content) = json.get("choices")
                                    .and_then(|c| c.as_array())
                                    .and_then(|a| a.first())
                                    .and_then(|c| c.get("delta"))
                                    .and_then(|d| d.get("content"))
                                    .and_then(|c| c.as_str()) {
                                    text_so_far.push_str(content);
                                }
                            }
                        }
                    }
                    sse_parse_offset += last_newline + 1;
                }

                // Check if this request was blocked by an action gate
                {
                    let st = state_bg.lock().unwrap();
                    if st.blocked_requests.contains(&request_id_bg) {
                        break;
                    }
                }

                // Forward chunk to client FIRST (don't block on evaluation)
                if client_tx.send(Ok(bytes)).await.is_err() {
                    break; // client disconnected
                }

                // Send accumulated text to dashboard (throttled: every 100+ chars)
                if text_so_far.len() > last_stream_text_len + 100 {
                    last_stream_text_len = text_so_far.len();
                    let _ = event_tx_bg.send(DaemonEvent::StreamText {
                        request_id: request_id_bg.clone(),
                        text: text_so_far.clone(),
                    });
                }

                // Check for sentence boundary + keyword hit (cheap pre-filter)
                let new_text = &text_so_far[last_eval_len..];
                let has_sentence_end = new_text.contains(". ") || new_text.contains("! ")
                    || new_text.contains("? ") || new_text.contains(".\n");
                let has_keyword = has_sentence_end && constraint_keywords.iter()
                    .any(|kw| new_text.to_lowercase().contains(kw));

                if has_keyword && text_so_far.len() > last_eval_len + 20 {
                    // Run evaluation on new text only
                    last_eval_len = text_so_far.len();

                    let messages = vec![TranscriptMessage {
                        role: "assistant".to_string(),
                        text: text_so_far.clone(),
                        message_index: 0,
                    }];
                    let extractor = HeuristicExtractor::new();
                    let claims = extractor.extract(&messages);

                    // Action intent gate: for any ActionIntent claim, run scope judge
                    for claim in &claims {
                        if claim.claim_type == crate::claim::types::ClaimType::ActionIntent {
                            let (judge_url, judge_key, judge_model) = {
                                let st = state_bg.lock().unwrap();
                                let key = st.judge_api_key.clone().or_else(|| st.api_key.clone());
                                let url = if st.judge_api_key.is_some() { st.judge_api_url.clone() } else { st.target_api.clone() };
                                (url, key, st.judge_model.clone())
                            };
                            if let Some(api_key) = judge_key {
                                let action_text = claim.text.clone();
                                let user_msg = user_message_bg.clone();
                                let sess = session_id_bg.clone();
                                let req_id = request_id_bg.clone();
                                let http = http_client_bg.clone();
                                let etx = event_tx_bg.clone();
                                let st_clone = state_bg.clone();
                                tokio::spawn(async move {
                                    let (within_scope, reason) = scope_judge_check(
                                        &http, &judge_url, &api_key, &judge_model,
                                        &user_msg, &action_text, &etx,
                                    ).await;
                                    if !within_scope {
                                        let gate_id = uuid::Uuid::new_v4().to_string();
                                        let is_active = {
                                            let st = st_clone.lock().unwrap();
                                            st.active_streams.contains(&req_id)
                                        };
                                        if is_active {
                                            let rx = crate::daemon::gate::create_realtime_gate(
                                                &st_clone, req_id.clone(), gate_id.clone(),
                                                action_text, user_msg, sess, reason, &etx,
                                            );
                                            let decision = tokio::time::timeout(
                                                std::time::Duration::from_secs(crate::daemon::gate::GATE_TIMEOUT_SECS),
                                                rx
                                            ).await;
                                            let approved = matches!(decision, Ok(Ok(true)));
                                            if !approved {
                                                let mut st = st_clone.lock().unwrap();
                                                st.blocked_requests.insert(req_id.clone());
                                            }
                                            let _ = etx.send(crate::daemon::ws::DaemonEvent::ActionGateDecision {
                                                gate_id,
                                                approved,
                                                reverted_paths: Vec::new(),
                                            });
                                        } else {
                                            crate::daemon::gate::register_retroactive_gate(
                                                &st_clone, req_id, gate_id,
                                                action_text, user_msg, sess, reason, &etx,
                                            );
                                        }
                                    }
                                });
                            }
                        }
                    }

                    if !claims.is_empty() {
                        let mut engine = match engine_bg.clone() {
                            Some(e) => e,
                            None => match PolicyEngine::new(&config_bg) {
                                Ok(e) => e,
                                Err(_) => { continue; }
                            },
                        };

                        let eval_input = EvaluationInput { claims: claims.clone() };
                        if let Ok(raw_violations) = engine.evaluate(&eval_input) {
                            let thresholds = SeverityThresholds::default();
                            let violations = collect_violations(
                                raw_violations, &strengths_bg, &thresholds, &constraint_meta_bg, &claims
                            );
                            // Governance: check block_next flag
                            let force_block = {
                                let mut st = state_bg.lock().unwrap();
                                let b = st.block_next;
                                if b { st.block_next = false; }
                                b
                            };
                            let decision = if force_block {
                                Decision::Block { violations: violations.clone() }
                            } else {
                                determine_decision(&violations)
                            };

                            if matches!(decision, Decision::Block { .. }) {
                                // BLOCK: kill the stream, inject error, save tokens
                                blocked = true;

                                // Check if this is already a retry — don't retry retries
                                let already_retried = modified_body_bg.get("system")
                                    .and_then(|s| s.as_str())
                                    .map(|s| s.contains("[RIGOR EPISTEMIC CORRECTION]"))
                                    .unwrap_or(false);

                                // Emit violation events
                                for v in &violations {
                                    let sev = match v.severity {
                                        crate::violation::Severity::Block => "block",
                                        crate::violation::Severity::Warn => "warn",
                                        crate::violation::Severity::Allow => "allow",
                                    };
                                    let _ = event_tx_bg.send(DaemonEvent::Violation {
                                        claim_id: v.claim_ids.first().cloned().unwrap_or_default(),
                                        constraint_id: v.constraint_id.clone(),
                                        severity: sev.to_string(),
                                        reason: v.message.clone(),
                                        strength: v.strength,
                                    });
                                }

                                let _ = event_tx_bg.send(DaemonEvent::Decision {
                                    request_id: request_id_bg.clone(),
                                    decision: "block".to_string(),
                                    violations: violations.len(),
                                    claims: claims.len(),
                                });

                                // Persist violations to disk (streaming path)
                                if let Ok(logger) = crate::logging::ViolationLogger::new() {
                                    let session_meta = crate::logging::SessionMetadata::default();
                                    for v in &violations {
                                        let sev = match v.severity {
                                            crate::violation::Severity::Block => "block",
                                            crate::violation::Severity::Warn => "warn",
                                            crate::violation::Severity::Allow => "allow",
                                        };
                                        let entry = crate::logging::ViolationLogEntry {
                                            session: session_meta.clone(),
                                            constraint_id: v.constraint_id.clone(),
                                            constraint_name: v.constraint_name.clone(),
                                            claim_ids: v.claim_ids.clone(),
                                            claim_text: v.claim_text.clone(),
                                            base_strength: v.strength,
                                            computed_strength: v.strength,
                                            severity: sev.to_string(),
                                            decision: "block".to_string(),
                                            message: v.message.clone(),
                                            supporters: Vec::new(),
                                            attackers: Vec::new(),
                                            total_claims: claims.len(),
                                            total_constraints: 0,
                                            transcript_path: None,
                                            claim_confidence: None,
                                            claim_type: None,
                                            claim_source: None,
                                            false_positive: None,
                                            annotation_note: None,
                                        };
                                        let _ = logger.log(&entry);
                                    }
                                }

                                // Build violation summary
                                let violation_lines: Vec<String> = violations.iter()
                                    .map(|v| format!("{}: {}", v.constraint_id, v.message))
                                    .collect();
                                let error_msg = format!(
                                    "rigor BLOCKED — {} violation(s): {}",
                                    violations.len(),
                                    violation_lines.join("; ")
                                );

                                // Send the blocked text to dashboard so user sees what was flagged
                                let _ = event_tx_bg.send(DaemonEvent::StreamText {
                                    request_id: request_id_bg.clone(),
                                    text: text_so_far.clone(),
                                });

                                // Drop upstream (stops token generation)
                                drop(upstream);

                                let retries_disabled = std::env::var("RIGOR_NO_RETRY").is_ok();

                                if already_retried || retries_disabled {
                                    // Already retried once or retries disabled — send error and stop.
                                    crate::daemon::ws::emit_log(&event_tx_bg, "error", "proxy",
                                        "BLOCK after retry — not retrying again (max 1 retry)".to_string());
                                    let _ = event_tx_bg.send(DaemonEvent::Retry {
                                        request_id: request_id_bg.clone(),
                                        violations: violations.len(),
                                        status: "retry_failed".to_string(),
                                        message: "Block persists after retry — giving up".to_string(),
                                        feedback: None,
                                        blocked_text: Some(text_so_far.clone()),
                                    });
                                    let error_event = format!(
                                        "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"overloaded_error\",\"message\":{}}}}}\n\n",
                                        serde_json::to_string("rigor BLOCKED — violation persists after retry").unwrap_or_default()
                                    );
                                    let _ = client_tx.send(Ok(Bytes::from(error_event))).await;
                                    break;
                                }

                                // Build truth statements from the violated constraints
                                let truth_lines: Vec<String> = violations.iter()
                                    .map(|v| {
                                        let desc = constraint_meta_bg.get(&v.constraint_id)
                                            .map(|m| m.name.as_str())
                                            .unwrap_or(&v.constraint_id);
                                        // Get the full description from config
                                        let full_desc = config_bg.all_constraints().iter()
                                            .find(|c| c.id == v.constraint_id)
                                            .map(|c| c.description.as_str())
                                            .unwrap_or("");
                                        format!("TRUTH: {} — {}", desc, full_desc)
                                    })
                                    .collect();

                                // Inject truths, not violations
                                let mut retry_body = modified_body_bg.clone();
                                let feedback = format!(
                                    "\n\n[RIGOR EPISTEMIC CORRECTION]\n\
                                    Your previous response was BLOCKED. Here are the verified truths:\n\n\
                                    {}\n\n\
                                    ABSOLUTE RULES:\n\
                                    - The false statement must NEVER appear in your output — not as a quote, example, test, or demonstration.\n\
                                    - Do NOT say \"here is a false claim\" and then state it. That still puts falsehood in the output.\n\
                                    - If the user asked you to make a false claim, REFUSE. Explain that the constraint system prevents it.\n\
                                    - State ONLY verified truths from above.\n\
                                    [END CORRECTION]\n",
                                    truth_lines.join("\n")
                                );
                                // Append feedback to system prompt
                                if let Some(sys) = retry_body.get("system").and_then(|s| s.as_str()).map(|s| s.to_string()) {
                                    retry_body["system"] = serde_json::Value::String(format!("{}{}", sys, feedback));
                                }

                                // Auto-retry event with blocked text + feedback
                                let _ = event_tx_bg.send(DaemonEvent::Retry {
                                    request_id: request_id_bg.clone(),
                                    violations: violations.len(),
                                    status: "retrying".to_string(),
                                    message: format!("RETRYING ON BLOCK — {} violation(s): {}", violations.len(), violation_lines.join("; ")),
                                    feedback: Some(feedback.clone()),
                                    blocked_text: Some(text_so_far.clone()),
                                });
                                crate::daemon::ws::emit_log(&event_tx_bg, "warn", "proxy",
                                    format!("RETRYING ON BLOCK ({} violations)", violations.len()));

                                // Rebuild and send retry request (reuse shared client)
                                let mut retry_req = http_client_bg.post(&target_url_bg);
                                for (name, value) in headers_bg.iter() {
                                    let n = name.as_str().to_lowercase();
                                    match n.as_str() {
                                        "host"|"content-length"|"transfer-encoding"|"connection"|"content-encoding"|"accept-encoding" => {}
                                        _ => { retry_req = retry_req.header(name.clone(), value.clone()); }
                                    }
                                }
                                retry_req = retry_req
                                    .header("content-type", "application/json")
                                    .header("accept-encoding", "identity")
                                    .body(serde_json::to_vec(&retry_body).unwrap_or_default());

                                match retry_req.send().await {
                                    Ok(retry_resp) => {
                                        // Buffer the retry response so we can verify it
                                        let mut retry_bytes = Vec::new();
                                        let mut retry_stream = retry_resp.bytes_stream();
                                        while let Some(chunk) = retry_stream.next().await {
                                            if let Ok(b) = chunk {
                                                retry_bytes.extend_from_slice(&b);
                                            }
                                        }

                                        // Extract text from the retry response
                                        let retry_raw = String::from_utf8_lossy(&retry_bytes);
                                        let retry_text = extract_sse_assistant_text(&retry_raw, &path_bg)
                                            .unwrap_or_default();

                                        // Fast LLM-as-judge: use judge config (OpenRouter) if available
                                        let (judge_url_1, judge_key_1, judge_model_1) = {
                                            let st = state_bg.lock().unwrap();
                                            let key = st.judge_api_key.clone().or_else(|| st.api_key.clone());
                                            let url = if st.judge_api_key.is_some() { st.judge_api_url.clone() } else { st.target_api.clone() };
                                            (url, key, st.judge_model.clone())
                                        };
                                        let still_violated = if !retry_text.is_empty() {
                                            check_violations_persist(
                                                &http_client_bg,
                                                &judge_url_1,
                                                judge_key_1.as_deref(),
                                                &judge_model_1,
                                                &violation_lines,
                                                &retry_text,
                                                &event_tx_bg,
                                            ).await
                                        } else {
                                            false
                                        };

                                        if still_violated {
                                            // Retry response STILL has the same violations
                                            crate::daemon::ws::emit_log(&event_tx_bg, "error", "proxy",
                                                "RETRY STILL VIOLATED — LLM rephrased but error persists, sending error".to_string());
                                            let _ = event_tx_bg.send(DaemonEvent::Retry {
                                                request_id: request_id_bg.clone(),
                                                violations: violations.len(),
                                                status: "retry_failed".to_string(),
                                                message: "Retry still contains same violations (rephrased)".to_string(),
                                                feedback: None,
                                                blocked_text: Some(retry_text),
                                            });
                                            let error_event = format!(
                                                "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"overloaded_error\",\"message\":{}}}}}\n\n",
                                                serde_json::to_string("rigor BLOCKED — retry still contained the same factual error").unwrap_or_default()
                                            );
                                            let _ = client_tx.send(Ok(Bytes::from(error_event))).await;
                                        } else {
                                            // Retry is clean — send it to client
                                            let _ = client_tx.send(Ok(Bytes::from(retry_bytes))).await;
                                            let _ = event_tx_bg.send(DaemonEvent::Retry {
                                                request_id: request_id_bg.clone(),
                                                violations: violations.len(),
                                                status: "retry_success".to_string(),
                                                message: "Retry response verified clean by LLM judge".to_string(),
                                                feedback: None,
                                                blocked_text: Some(retry_text),
                                            });
                                            crate::daemon::ws::emit_log(&event_tx_bg, "info", "proxy",
                                                "RETRY SUCCESS — verified clean by LLM judge".to_string());
                                        }
                                    }
                                    Err(e) => {
                                        let _ = event_tx_bg.send(DaemonEvent::Retry {
                                            request_id: request_id_bg.clone(),
                                            violations: violations.len(),
                                            status: "retry_failed".to_string(),
                                            message: format!("Retry failed: {}", e),
                                            feedback: None,
                                            blocked_text: None,
                                        });
                                        crate::daemon::ws::emit_log(&event_tx_bg, "error", "proxy",
                                            format!("RETRY FAILED: {}", e));
                                        // Retry failed — send error to client
                                        let error_event = format!(
                                            "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"overloaded_error\",\"message\":{}}}}}\n\n",
                                            serde_json::to_string(&format!("rigor BLOCKED + retry failed: {}", e)).unwrap_or_default()
                                        );
                                        let _ = client_tx.send(Ok(Bytes::from(error_event))).await;
                                    }
                                }
                                break;
                            }
                        }
                    }
                }

            }

            // Extract token usage from accumulated SSE data
            let raw_text = String::from_utf8_lossy(&raw_accumulated);
            let (input_tokens, output_tokens) = extract_sse_usage(&raw_text, &path_bg);
            if input_tokens > 0 || output_tokens > 0 {
                let _ = event_tx_bg.send(DaemonEvent::TokenUsage {
                    request_id: request_id_bg.clone(),
                    input_tokens,
                    output_tokens,
                    model: model_bg.clone(),
                });
            }

            // Stream ended — final evaluation with retry capability.
            // Two-phase: (1) Rego rules (instant), (2) LLM judge (semantic, ~1-3s)
            if !blocked && !text_so_far.is_empty() {
                // Phase 1: Rego evaluation
                let final_decision = evaluate_text_inline(
                    &text_so_far, &path_bg, &request_id_bg, &event_tx_bg, &state_bg,
                );

                // Phase 2: If Rego passed, run LLM judge as semantic safety net
                let llm_blocked = if final_decision != "block" && !text_so_far.is_empty() {
                    let (judge_url_2, judge_key_2, judge_model_2, constraint_descs) = {
                        let st = state_bg.lock().unwrap();
                        let key = st.judge_api_key.clone().or_else(|| st.api_key.clone());
                        let url = if st.judge_api_key.is_some() { st.judge_api_url.clone() } else { st.target_api.clone() };
                        let descs: Vec<String> = st.config.all_constraints().iter()
                            .map(|c| format!("{}: {}", c.name, c.description))
                            .collect();
                        (url, key, st.judge_model.clone(), descs)
                    };

                    if judge_key_2.is_some() && !constraint_descs.is_empty() {
                        let _ = event_tx_bg.send(DaemonEvent::JudgeActivity {
                            request_id: request_id_bg.clone(),
                            action: "semantic_eval_start".to_string(),
                            duration_ms: None,
                            detail: format!("Evaluating {} chars against {} constraints", text_so_far.len(), constraint_descs.len()),
                        });

                        let judge_start = std::time::Instant::now();
                        let violated = check_violations_persist(
                            &http_client_bg,
                            &judge_url_2,
                            judge_key_2.as_deref(),
                            &judge_model_2,
                            &constraint_descs,
                            &text_so_far,
                            &event_tx_bg,
                        ).await;

                        let judge_ms = judge_start.elapsed().as_millis() as u64;
                        let _ = event_tx_bg.send(DaemonEvent::JudgeActivity {
                            request_id: request_id_bg.clone(),
                            action: "semantic_eval_done".to_string(),
                            duration_ms: Some(judge_ms),
                            detail: format!("{} — {}ms", if violated { "VIOLATION FOUND" } else { "clean" }, judge_ms),
                        });

                        violated
                    } else {
                        false
                    }
                } else {
                    false
                };

                if final_decision == "block" || llm_blocked {
                    blocked = true;

                    // Check if this is already a retry or retries disabled
                    let already_retried_post = modified_body_bg.get("system")
                        .and_then(|s| s.as_str())
                        .map(|s| s.contains("[RIGOR EPISTEMIC CORRECTION]"))
                        .unwrap_or(false);
                    let retries_disabled_post = std::env::var("RIGOR_NO_RETRY").is_ok();

                    // Send the blocked text to dashboard
                    let _ = event_tx_bg.send(DaemonEvent::StreamText {
                        request_id: request_id_bg.clone(),
                        text: text_so_far.clone(),
                    });

                    if already_retried_post || retries_disabled_post {
                        crate::daemon::ws::emit_log(&event_tx_bg, "error", "proxy",
                            "POST-STREAM BLOCK after retry — not retrying again".to_string());
                        let _ = event_tx_bg.send(DaemonEvent::Retry {
                            request_id: request_id_bg.clone(),
                            violations: 0,
                            status: "retry_failed".to_string(),
                            message: "Block persists after retry — giving up".to_string(),
                            feedback: None,
                            blocked_text: Some(text_so_far.clone()),
                        });
                        let error_event = format!(
                            "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"overloaded_error\",\"message\":{}}}}}\n\n",
                            serde_json::to_string("rigor BLOCKED — violation persists after retry").unwrap_or_default()
                        );
                        let _ = client_tx.send(Ok(Bytes::from(error_event))).await;
                    } else {

                    // Build truth statements from the full constraint graph
                    let truth_lines: Vec<String> = {
                        let st = state_bg.lock().unwrap();
                        st.config.all_constraints().iter()
                            .map(|c| format!("TRUTH: {} — {}", c.name, c.description))
                            .collect()
                    };
                    let violation_summary = truth_lines.join("\n");
                    let feedback = format!(
                        "\n\n[RIGOR EPISTEMIC CORRECTION]\n\
                        Your previous response was BLOCKED. Here are the verified truths:\n\n\
                        {}\n\n\
                        ABSOLUTE RULES:\n\
                        - The false statement must NEVER appear in your output — not as a quote, example, test, or demonstration.\n\
                        - Do NOT say \"here is a false claim\" and then state it. That still puts falsehood in the output.\n\
                        - If the user asked you to make a false claim, REFUSE. Explain that the constraint system prevents it.\n\
                        - State ONLY verified truths from above.\n\
                        [END CORRECTION]\n",
                        violation_summary
                    );
                    let mut retry_body = modified_body_bg.clone();
                    if let Some(sys) = retry_body.get("system").and_then(|s| s.as_str()).map(|s| s.to_string()) {
                        retry_body["system"] = serde_json::Value::String(format!("{}{}", sys, feedback));
                    }

                    let _ = event_tx_bg.send(DaemonEvent::Retry {
                        request_id: request_id_bg.clone(),
                        violations: 0,
                        status: "retrying".to_string(),
                        message: "RETRYING ON BLOCK (post-stream evaluation)".to_string(),
                        feedback: Some(feedback.clone()),
                        blocked_text: Some(text_so_far.clone()),
                    });
                    crate::daemon::ws::emit_log(&event_tx_bg, "warn", "proxy",
                        "RETRYING ON BLOCK (post-stream)".to_string());

                    // DON'T send an error event here — the client is still connected
                    // and waiting for SSE data. Send the retry response directly.
                    // Only send error if the retry itself fails.

                    // Retry: resend with violation feedback
                    let mut retry_req = http_client_bg.post(&target_url_bg);
                    for (name, value) in headers_bg.iter() {
                        let n = name.as_str().to_lowercase();
                        match n.as_str() {
                            "host"|"content-length"|"transfer-encoding"|"connection"|"content-encoding"|"accept-encoding" => {}
                            _ => { retry_req = retry_req.header(name.clone(), value.clone()); }
                        }
                    }
                    retry_req = retry_req
                        .header("content-type", "application/json")
                        .header("accept-encoding", "identity")
                        .body(serde_json::to_vec(&retry_body).unwrap_or_default());

                    match retry_req.send().await {
                        Ok(retry_resp) => {
                            // Buffer retry response, verify with judge, then send
                            let mut retry_bytes = Vec::new();
                            let mut retry_stream = retry_resp.bytes_stream();
                            while let Some(chunk) = retry_stream.next().await {
                                if let Ok(b) = chunk {
                                    retry_bytes.extend_from_slice(&b);
                                }
                            }

                            let retry_raw = String::from_utf8_lossy(&retry_bytes);
                            let retry_text = extract_sse_assistant_text(&retry_raw, &path_bg)
                                .unwrap_or_default();

                            // Fast LLM judge: use judge config (OpenRouter) if available
                            let (judge_url_3, judge_key_3, judge_model_3) = {
                                let st = state_bg.lock().unwrap();
                                let key = st.judge_api_key.clone().or_else(|| st.api_key.clone());
                                let url = if st.judge_api_key.is_some() { st.judge_api_url.clone() } else { st.target_api.clone() };
                                (url, key, st.judge_model.clone())
                            };
                            let still_violated = if !retry_text.is_empty() {
                                check_violations_persist(
                                    &http_client_bg,
                                    &judge_url_3,
                                    judge_key_3.as_deref(),
                                    &judge_model_3,
                                    &[violation_summary.to_string()],
                                    &retry_text,
                                    &event_tx_bg,
                                ).await
                            } else {
                                false
                            };

                            if still_violated {
                                crate::daemon::ws::emit_log(&event_tx_bg, "error", "proxy",
                                    "POST-STREAM RETRY STILL VIOLATED".to_string());
                                let _ = event_tx_bg.send(DaemonEvent::Retry {
                                    request_id: request_id_bg.clone(),
                                    violations: 0,
                                    status: "retry_failed".to_string(),
                                    message: "Post-stream retry still contains same violations".to_string(),
                                    feedback: None,
                                    blocked_text: Some(retry_text),
                                });
                                let error_event = format!(
                                    "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"overloaded_error\",\"message\":{}}}}}\n\n",
                                    serde_json::to_string("rigor BLOCKED — retry still contained the same factual error").unwrap_or_default()
                                );
                                let _ = client_tx.send(Ok(Bytes::from(error_event))).await;
                            } else {
                                // Clean — send to client
                                let _ = client_tx.send(Ok(Bytes::from(retry_bytes))).await;
                                let _ = event_tx_bg.send(DaemonEvent::Retry {
                                    request_id: request_id_bg.clone(),
                                    violations: 0,
                                    status: "retry_success".to_string(),
                                    message: "Post-stream retry verified clean by LLM judge".to_string(),
                                    feedback: None,
                                    blocked_text: Some(retry_text),
                                });
                                crate::daemon::ws::emit_log(&event_tx_bg, "info", "proxy",
                                    "POST-STREAM RETRY SUCCESS — verified clean".to_string());
                            }
                        }
                        Err(e) => {
                            let _ = event_tx_bg.send(DaemonEvent::Retry {
                                request_id: request_id_bg.clone(),
                                violations: 0,
                                status: "retry_failed".to_string(),
                                message: format!("Post-stream retry failed: {}", e),
                                feedback: None,
                                blocked_text: None,
                            });
                        }
                    }
                    } // end else (not already retried)
                }
            }

            // Cleanup: remove from active_streams and blocked_requests
            {
                let mut st = state_bg.lock().unwrap();
                st.active_streams.remove(&request_id_bg);
                st.blocked_requests.remove(&request_id_bg);
            }
        });

        // Convert the mpsc receiver into a Body stream
        let client_stream = tokio_stream::wrappers::ReceiverStream::new(client_rx);

        let mut builder = Response::builder().status(status.as_u16());
        for (name, value) in resp_headers.iter() {
            let name_str = name.as_str().to_lowercase();
            match name_str.as_str() {
                "content-type" | "cache-control" | "x-request-id" | "request-id" => {
                    builder = builder.header(name.clone(), value.clone());
                }
                _ => {}
            }
        }
        return builder
            .body(Body::from_stream(client_stream))
            .unwrap_or_else(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Stream error: {}", e)).into_response()
            });
    }

    // Non-streaming: buffer response body for claim extraction
    let response_bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("rigor proxy: failed to read response body: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                format!("Failed to read upstream response: {}", e),
            )
                .into_response();
        }
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    let _ = event_tx.send(DaemonEvent::Response {
        id: request_id.clone(),
        status: status.as_u16(),
        duration_ms,
    });

    // Extract token usage from buffered response
    if let Ok(resp_json) = serde_json::from_slice::<serde_json::Value>(&response_bytes) {
        let (input_tokens, output_tokens) = extract_usage(&resp_json, path);
        if input_tokens > 0 || output_tokens > 0 {
            let _ = event_tx.send(DaemonEvent::TokenUsage {
                request_id: request_id.clone(),
                input_tokens,
                output_tokens,
                model: model.clone(),
            });
        }
    }

    // Extract claims from the response in a background task so we don't
    // delay returning the response body to the client.
    // Clone what we need for the spawned task.
    let response_bytes_clone = response_bytes.clone();
    let event_tx_clone = event_tx.clone();
    let request_id_clone = request_id.clone();
    let state_clone = state.clone();
    let path_owned = path.to_string();

    tokio::spawn(async move {
        extract_and_evaluate(
            &response_bytes_clone,
            &path_owned,
            &request_id_clone,
            &event_tx_clone,
            &state_clone,
        );
    });

    // Return the buffered response to the client
    let mut builder = Response::builder().status(status.as_u16());
    for (name, value) in resp_headers.iter() {
        let name_str = name.as_str().to_lowercase();
        match name_str.as_str() {
            "content-type" | "cache-control" | "x-request-id" | "request-id" => {
                builder = builder.header(name.clone(), value.clone());
            }
            _ => {}
        }
    }
    builder
        .body(Body::from(response_bytes))
        .unwrap_or_else(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Stream error: {}", e)).into_response()
        })
}

/// Extract assistant text from an API response JSON body.
/// Handles both Anthropic (content blocks) and OpenAI (choices) formats.
fn extract_assistant_text(body: &serde_json::Value, path: &str) -> Option<String> {
    if path.contains("messages") {
        // Anthropic format: { "content": [{"type": "text", "text": "..."}] }
        body.get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| {
                        if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                            b.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|s| !s.is_empty())
    } else {
        // OpenAI format: { "choices": [{"message": {"content": "..."}}] }
        body.get("choices")
            .and_then(|c| c.as_array())
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    }
}

/// Run claim extraction and policy evaluation on a buffered response.
/// Emits ClaimExtracted, Violation, and Decision events.
fn extract_and_evaluate(
    response_bytes: &[u8],
    path: &str,
    request_id: &str,
    event_tx: &EventSender,
    state: &SharedState,
) {
    // Parse response JSON
    let response_json: serde_json::Value = match serde_json::from_slice(response_bytes) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rigor proxy: failed to parse response JSON for claim extraction: {}", e);
            let _ = event_tx.send(DaemonEvent::Decision {
                request_id: request_id.to_string(),
                decision: "allow".to_string(),
                violations: 0,
                claims: 0,
            });
            return;
        }
    };

    // Extract assistant text
    let assistant_text = match extract_assistant_text(&response_json, path) {
        Some(text) => text,
        None => {
            eprintln!("rigor proxy: no assistant text found in response");
            let _ = event_tx.send(DaemonEvent::Decision {
                request_id: request_id.to_string(),
                decision: "allow".to_string(),
                violations: 0,
                claims: 0,
            });
            return;
        }
    };

    extract_and_evaluate_text(&assistant_text, path, request_id, event_tx, state);
}

/// Shared claim extraction and evaluation logic.
/// Used by both buffered (non-streaming) and streaming (SSE accumulation) paths.
/// Fast LLM check: do the original violations still persist in the retry text?
/// Uses a minimal prompt (~50 tokens per violation) for speed.
/// Returns true if ANY violation is still present (rephrased or not).
async fn check_violations_persist(
    client: &reqwest::Client,
    api_url: &str,
    api_key: Option<&str>,
    model: &str,
    original_violations: &[String],  // "constraint_id: violation message"
    retry_text: &str,
    event_tx: &EventSender,
) -> bool {
    let api_key = match api_key {
        Some(k) => k,
        None => return false, // can't check without API key, assume clean
    };

    if original_violations.is_empty() || retry_text.is_empty() { return false; }

    // Build a focused prompt — one check for all violations at once
    let mut violation_list = String::new();
    for (i, v) in original_violations.iter().enumerate() {
        violation_list.push_str(&format!("{}. {}\n", i + 1, v));
    }

    let prompt = format!(
        "You are an extremely strict factual accuracy judge. Zero tolerance.\n\n\
        These are absolute truths:\n{}\n\
        Here is an AI response:\n\"{}\"\n\n\
        Answer YES if the response text contains a false statement about the above truths ANYWHERE in the text.\n\
        This includes:\n\
        - False statements presented as examples, tests, or demonstrations\n\
        - False statements prefaced with \"for testing\" or \"deliberately\"\n\
        - False statements even when the response later corrects them\n\
        - Hedging that makes a falsehood seem plausible\n\n\
        Intent does NOT matter. If the false words appear in the output, answer YES.\n\
        The ONLY exception is explicit negation (\"Rust does NOT have a GC\").\n\n\
        Answer only YES or NO.",
        violation_list,
        if retry_text.len() > 2000 { &retry_text[..2000] } else { retry_text }
    );

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 4,
        "messages": [{"role": "user", "content": prompt}]
    });

    let _ = event_tx.send(DaemonEvent::JudgeActivity {
        request_id: String::new(),
        action: "retry_verify_start".to_string(),
        duration_ms: None,
        detail: format!("{} violations to verify", original_violations.len()),
    });

    let judge_start = std::time::Instant::now();

    // Call API directly — not through the proxy (avoids self-evaluation loop)
    let mut req = client
        .post(&format!("{}/v1/messages", api_url))
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json");

    req = apply_provider_auth(req, api_key);

    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        req.json(&body).send()
    ).await;

    let resp = match resp {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            crate::daemon::ws::emit_log(event_tx, "error", "relevance",
                format!("Retry verification LLM call failed: {}", e));
            return false; // fail open
        }
        Err(_) => {
            crate::daemon::ws::emit_log(event_tx, "error", "relevance",
                "Retry verification timed out (10s)".to_string());
            return false; // fail open
        }
    };

    let resp_json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(_) => return false,
    };

    let answer = resp_json.get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_uppercase();

    let persists = answer.starts_with("YES");

    let judge_ms = judge_start.elapsed().as_millis() as u64;
    let _ = event_tx.send(DaemonEvent::JudgeActivity {
        request_id: String::new(),
        action: "retry_verify_done".to_string(),
        duration_ms: Some(judge_ms),
        detail: format!("{} — {}ms", if persists { "violations PERSIST" } else { "clean" }, judge_ms),
    });

    crate::daemon::ws::emit_log(event_tx, "info", "relevance",
        format!("Retry verification: {} ({}ms)", if persists { "violations PERSIST" } else { "clean" }, judge_ms));

    // Broadcast full evaluation details to dashboard
    let _ = event_tx.send(DaemonEvent::JudgeEvaluation {
        eval_type: "retry_verify".to_string(),
        prompt: Some(prompt.clone()),
        response: Some(answer.clone()),
        claims: None,
        constraints: Some(original_violations.to_vec()),
        result: Some(if persists { "VIOLATIONS PERSIST".to_string() } else { "CLEAN".to_string() }),
        duration_ms: Some(judge_ms),
    });

    persists
}

/// Ask the LLM judge whether an action intent is within scope of user's request.
/// Returns (within_scope, reason).
async fn scope_judge_check(
    client: &reqwest::Client,
    api_url: &str,
    api_key: &str,
    model: &str,
    user_message: &str,
    action_intent: &str,
    event_tx: &EventSender,
) -> (bool, String) {
    let prompt = format!(
        "The user said: \"{}\"\n\n\
        The AI is proposing to: \"{}\"\n\n\
        Is the AI's proposed action within the scope of what the user explicitly requested?\n\
        Answer YES if the user asked for this action or something that clearly implies it.\n\
        Answer NO if the AI is proposing something the user did not ask for.\n\n\
        Answer with exactly YES or NO followed by a one-sentence reason.",
        user_message.chars().take(500).collect::<String>(),
        action_intent.chars().take(300).collect::<String>()
    );

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 80,
        "messages": [{"role": "user", "content": prompt}]
    });

    let mut req = client.post(&format!("{}/v1/messages", api_url))
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json");
    req = apply_provider_auth(req, api_key);

    let resp = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        req.json(&body).send()
    ).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            crate::daemon::ws::emit_log(event_tx, "error", "gate",
                format!("Scope judge request failed: {}", e));
            return (true, "Judge error — defaulting to allow".to_string());
        }
        Err(_) => {
            crate::daemon::ws::emit_log(event_tx, "warn", "gate",
                "Scope judge timeout".to_string());
            return (true, "Judge timeout — defaulting to allow".to_string());
        }
    };

    // Fail-open on HTTP errors (401/403/500/etc.). Without this, a 401 from
    // Anthropic parses as {"error": ...}, `get("content")` returns None,
    // text is empty, and `within_scope` ends up false — rejecting every
    // action instead of failing open. This caused the contrapunk gate stall.
    let status = resp.status();
    if !status.is_success() {
        crate::daemon::ws::emit_log(event_tx, "warn", "gate",
            format!("Scope judge HTTP {} — defaulting to allow", status));
        return (true, format!("Judge HTTP {} — defaulting to allow", status));
    }

    let resp_json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(_) => return (true, "Parse error — defaulting to allow".to_string()),
    };

    let text = resp_json.get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let within_scope = text.to_uppercase().starts_with("YES");
    let reason = text.split_once(|c: char| c == '\n' || c == '.')
        .map(|(f, _)| f.trim().to_string())
        .unwrap_or_else(|| text.clone());

    (within_scope, reason)
}

/// Same as extract_and_evaluate_text but returns the decision string.
/// Used by the post-stream retry path which needs to know if it should retry.
fn evaluate_text_inline(
    assistant_text: &str,
    _path: &str,
    request_id: &str,
    event_tx: &EventSender,
    state: &SharedState,
) -> String {
    extract_and_evaluate_text(assistant_text, _path, request_id, event_tx, state);
    // Read the last decision from the event channel — but we can't easily do that.
    // Instead, re-run a lightweight check: just extract + evaluate and return decision.
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text: assistant_text.to_string(),
        message_index: 0,
    }];
    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);
    if claims.is_empty() { return "allow".to_string(); }

    let (config, strengths, cached_engine) = {
        let st = state.lock().unwrap();
        (st.config.clone(), st.graph.get_all_strengths(), st.policy_engine.as_ref().cloned())
    };
    let mut engine = match cached_engine {
        Some(e) => e,
        None => match PolicyEngine::new(&config) {
            Ok(e) => e,
            Err(_) => return "allow".to_string(),
        },
    };
    let eval_input = EvaluationInput { claims: claims.clone() };
    let raw_violations = match engine.evaluate(&eval_input) {
        Ok(v) => v,
        Err(_) => return "allow".to_string(),
    };
    let constraint_meta: HashMap<String, ConstraintMeta> = config
        .all_constraints().iter()
        .map(|c| {
            let etype = match c.epistemic_type {
                EpistemicType::Belief => "belief",
                EpistemicType::Justification => "justification",
                EpistemicType::Defeater => "defeater",
            };
            (c.id.clone(), ConstraintMeta {
                name: c.name.clone(),
                epistemic_type: etype.to_string(),
                rego_path: format!("data.rigor.{}", c.id),
            })
        }).collect();
    let thresholds = SeverityThresholds::default();
    let violations = collect_violations(raw_violations, &strengths, &thresholds, &constraint_meta, &claims);
    let decision = determine_decision(&violations);
    match decision {
        Decision::Block { .. } => "block".to_string(),
        Decision::Warn { .. } => "warn".to_string(),
        Decision::Allow => "allow".to_string(),
    }
}

fn extract_and_evaluate_text(
    assistant_text: &str,
    path: &str,
    request_id: &str,
    event_tx: &EventSender,
    state: &SharedState,
) {
    // Build a synthetic transcript for the extractor
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text: assistant_text.to_string(),
        message_index: 0,
    }];

    // Extract claims
    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);

    // Emit ClaimExtracted events
    for claim in &claims {
        let claim_type_str = format!("{:?}", claim.claim_type).to_lowercase();
        let _ = event_tx.send(DaemonEvent::ClaimExtracted {
            id: claim.id.clone(),
            text: claim.text.clone(),
            confidence: claim.confidence,
            claim_type: claim_type_str,
            request_id: request_id.to_string(),
        });
    }

    // PII-OUT: scan the response for PII leakage
    let pii_found = detect_pii(assistant_text);
    if !pii_found.is_empty() {
        for (kind, matched) in &pii_found {
            let _ = event_tx.send(DaemonEvent::PiiDetected {
                request_id: request_id.to_string(),
                direction: "out".to_string(),
                pii_type: kind.clone(),
                matched: redact_for_display(matched),
                action: "block".to_string(),
            });
            let _ = event_tx.send(DaemonEvent::Violation {
                claim_id: format!("pii-{}", kind),
                constraint_id: "pii-leak".to_string(),
                severity: "block".to_string(),
                reason: format!("Response contains {} PII: {}", kind, matched),
                strength: 1.0,
            });
        }
        let _ = event_tx.send(DaemonEvent::Decision {
            request_id: request_id.to_string(),
            decision: "block".to_string(),
            violations: pii_found.len(),
            claims: claims.len(),
        });
        crate::daemon::ws::emit_log(event_tx, "error", "pii",
            format!("PII-OUT BLOCK — {} PII items in response", pii_found.len()));
        return;
    }

    // Evaluate claims against constraints — use cached engine if available
    let (config, strengths, cached_engine) = {
        let st = state.lock().unwrap();
        let strengths_map = st.graph.get_all_strengths();
        let engine = st.policy_engine.as_ref().cloned();
        (st.config.clone(), strengths_map, engine)
    };

    let mut engine = match cached_engine {
        Some(e) => e,
        None => match PolicyEngine::new(&config) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("rigor proxy: failed to create policy engine: {}", e);
                let _ = event_tx.send(DaemonEvent::Decision {
                    request_id: request_id.to_string(),
                    decision: "allow".to_string(),
                    violations: 0,
                    claims: claims.len(),
                });
                return;
            }
        },
    };

    let eval_input = EvaluationInput {
        claims: claims.clone(),
    };

    let raw_violations = match engine.evaluate(&eval_input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("rigor proxy: failed to evaluate policies: {}", e);
            let _ = event_tx.send(DaemonEvent::Decision {
                request_id: request_id.to_string(),
                decision: "allow".to_string(),
                violations: 0,
                claims: claims.len(),
            });
            return;
        }
    };

    // Build constraint metadata
    let constraint_meta: HashMap<String, ConstraintMeta> = config
        .all_constraints()
        .iter()
        .map(|c| {
            let etype = match c.epistemic_type {
                EpistemicType::Belief => "belief",
                EpistemicType::Justification => "justification",
                EpistemicType::Defeater => "defeater",
            };
            (
                c.id.clone(),
                ConstraintMeta {
                    name: c.name.clone(),
                    epistemic_type: etype.to_string(),
                    rego_path: format!("data.rigor.{}", c.id),
                },
            )
        })
        .collect();

    let thresholds = SeverityThresholds::default();
    let violations = collect_violations(raw_violations, &strengths, &thresholds, &constraint_meta, &claims);

    // Emit Violation events
    for v in &violations {
        let severity_str = match v.severity {
            crate::violation::Severity::Block => "block",
            crate::violation::Severity::Warn => "warn",
            crate::violation::Severity::Allow => "allow",
        };
        let claim_id = v.claim_ids.first().cloned().unwrap_or_default();
        let _ = event_tx.send(DaemonEvent::Violation {
            claim_id,
            constraint_id: v.constraint_id.clone(),
            severity: severity_str.to_string(),
            reason: v.message.clone(),
            strength: v.strength,
        });
    }

    // Emit Decision event
    let decision = determine_decision(&violations);
    let decision_str = match &decision {
        Decision::Block { .. } => "block",
        Decision::Warn { .. } => "warn",
        Decision::Allow => "allow",
    };
    let _ = event_tx.send(DaemonEvent::Decision {
        request_id: request_id.to_string(),
        decision: decision_str.to_string(),
        violations: violations.len(),
        claims: claims.len(),
    });

    info_println!(
        "rigor proxy: extracted {} claims, {} violations, decision: {}",
        claims.len(),
        violations.len(),
        decision_str
    );

    // Persist violations to ~/.rigor/violations.jsonl for the graph and CLI queries.
    // This is critical: without this, `rigor log` and the /graph.json endpoint
    // never see violations from grounded sessions (only from hook-mode invocations).
    if !violations.is_empty() {
        if let Ok(logger) = crate::logging::ViolationLogger::new() {
            let session_meta = crate::logging::SessionMetadata::default();
            for v in &violations {
                let severity_str = match v.severity {
                    crate::violation::Severity::Block => "block",
                    crate::violation::Severity::Warn => "warn",
                    crate::violation::Severity::Allow => "allow",
                };
                let entry = crate::logging::ViolationLogEntry {
                    session: session_meta.clone(),
                    constraint_id: v.constraint_id.clone(),
                    constraint_name: v.constraint_name.clone(),
                    claim_ids: v.claim_ids.clone(),
                    claim_text: v.claim_text.clone(),
                    base_strength: v.strength,
                    computed_strength: v.strength,
                    severity: severity_str.to_string(),
                    decision: decision_str.to_string(),
                    message: v.message.clone(),
                    supporters: Vec::new(),
                    attackers: Vec::new(),
                    total_claims: claims.len(),
                    total_constraints: config.all_constraints().len(),
                    transcript_path: None,
                    claim_confidence: None,
                    claim_type: None,
                    claim_source: None,
                    false_positive: None,
                    annotation_note: None,
                };
                let _ = logger.log(&entry);
            }
        }
    }

    // Async LLM-as-judge: compute semantic relevance between claims and constraints.
    // Runs in background — does NOT block the response pipeline.
    // Rate limited: skips if a scoring request is already in flight.
    if !claims.is_empty() && RELEVANCE_SEMAPHORE.try_acquire().is_some() {
        let constraint_summaries: Vec<(String, String)> = config
            .all_constraints()
            .iter()
            .map(|c| (c.id.clone(), format!("{}: {}", c.name, c.description)))
            .collect();

        let claim_summaries: Vec<(String, String)> = claims.iter()
            .map(|c| (c.id.clone(), c.text.clone()))
            .collect();

        // Use judge config (OpenRouter) if available, else fall back to captured API key
        let (judge_url, judge_key, judge_model, http_client) = {
            let st = state.lock().unwrap();
            let key = st.judge_api_key.clone().or_else(|| st.api_key.clone());
            let url = if st.judge_api_key.is_some() { st.judge_api_url.clone() } else { st.target_api.clone() };
            let model = st.judge_model.clone();
            (url, key, model, st.http_client.clone())
        };

        let event_tx_rel = event_tx.clone();
        tokio::spawn(async move {
            score_claim_relevance(
                &http_client, &judge_url, judge_key.as_deref(), &judge_model,
                &claim_summaries, &constraint_summaries, &event_tx_rel,
            ).await;
            RELEVANCE_SEMAPHORE.release();
        });
    }
}

/// Semaphore for rate-limiting LLM-as-judge calls.
/// Only 1 relevance scoring request in flight at a time.
struct SimpleSemaphore(std::sync::atomic::AtomicBool);
impl SimpleSemaphore {
    const fn new() -> Self { Self(std::sync::atomic::AtomicBool::new(false)) }
    fn try_acquire(&self) -> Option<()> {
        self.0.compare_exchange(false, true, std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst)
            .ok().map(|_| ())
    }
    fn release(&self) { self.0.store(false, std::sync::atomic::Ordering::SeqCst); }
}
static RELEVANCE_SEMAPHORE: SimpleSemaphore = SimpleSemaphore::new();

/// Cache for LLM-as-judge results: claim_text -> Vec<(constraint_id, relevance, reason)>
static RELEVANCE_CACHE: std::sync::LazyLock<std::sync::Mutex<HashMap<String, Vec<(String, String, String)>>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// Call Sonnet to compute semantic relevance between claims and constraints.
/// Features: 30s timeout, rate limiting (1 in flight), caching, logging.
async fn score_claim_relevance(
    client: &reqwest::Client,
    api_url: &str,
    api_key: Option<&str>,
    model: &str,
    claims: &[(String, String)],       // (id, text)
    constraints: &[(String, String)],  // (id, "name: description")
    event_tx: &EventSender,
) {
    let api_key = match api_key {
        Some(k) => k,
        None => {
            crate::daemon::ws::emit_log(event_tx, "debug", "relevance",
                "Skipping LLM-as-judge: no API key".to_string());
            return;
        }
    };

    if claims.is_empty() || constraints.is_empty() { return; }

    // Check cache first — emit cached results immediately
    let mut uncached_claims = Vec::new();
    {
        let cache = RELEVANCE_CACHE.lock().unwrap();
        for (id, text) in claims {
            if let Some(cached) = cache.get(text) {
                for (cid, rel, reason) in cached {
                    let _ = event_tx.send(DaemonEvent::ClaimRelevance {
                        claim_id: id.clone(),
                        constraint_id: cid.clone(),
                        relevance: rel.clone(),
                        reason: reason.clone(),
                    });
                }
            } else {
                uncached_claims.push((id.clone(), text.clone()));
            }
        }
    }

    if uncached_claims.is_empty() {
        crate::daemon::ws::emit_log(event_tx, "debug", "relevance",
            format!("All {} claims cached, skipping LLM call", claims.len()));
        return;
    }

    crate::daemon::ws::emit_log(event_tx, "info", "relevance",
        format!("Scoring {} claims against {} constraints via LLM-as-judge",
            uncached_claims.len(), constraints.len()));

    // Build prompt
    let mut constraint_list = String::new();
    for (i, (id, desc)) in constraints.iter().enumerate() {
        constraint_list.push_str(&format!("{}. [{}] {}\n", i + 1, id, desc));
    }
    let mut claim_list = String::new();
    for (i, (id, text)) in uncached_claims.iter().enumerate() {
        claim_list.push_str(&format!("{}. [{}] \"{}\"\n", i + 1, id, text));
    }

    let prompt = format!(
        "Given these constraints:\n{}\nAnd these claims:\n{}\n\
        For each claim, output which constraint(s) it is semantically relevant to.\n\
        Output ONLY lines in this exact format (one per match, skip claims with no relevant constraint):\n\
        claim_id|constraint_id|high/medium|one-sentence-reason\n\
        No other text.",
        constraint_list, claim_list
    );

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 1024,
        "stream": false,
        "messages": [{"role": "user", "content": prompt}]
    });

    // Retry loop with backoff for rate limits (429)
    let mut resp_json: serde_json::Value = serde_json::Value::Null;
    let delays = [0u64, 3, 8, 15]; // seconds to wait before each attempt
    for (attempt, delay) in delays.iter().enumerate() {
        if *delay > 0 {
            crate::daemon::ws::emit_log(event_tx, "info", "relevance",
                format!("Rate limited, retrying in {}s (attempt {}/{})", delay, attempt + 1, delays.len()));
            tokio::time::sleep(std::time::Duration::from_secs(*delay)).await;
        }

        // Rebuild request each attempt (reqwest consumes the builder)
        let mut retry_req = client
            .post(&format!("{}/v1/messages", api_url))
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");
        retry_req = apply_provider_auth(retry_req, api_key);

        let resp = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            retry_req.json(&body).send()
        ).await;

        let r = match resp {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                crate::daemon::ws::emit_log(event_tx, "error", "relevance",
                    format!("LLM call failed: {}", e));
                return;
            }
            Err(_) => {
                crate::daemon::ws::emit_log(event_tx, "error", "relevance",
                    "LLM call timed out after 30s".to_string());
                return;
            }
        };

        let status = r.status();
        eprintln!("rigor relevance: API status: {} (attempt {})", status, attempt + 1);

        if status.as_u16() == 429 {
            // Rate limited — will retry if more attempts remain
            if attempt < delays.len() - 1 { continue; }
            crate::daemon::ws::emit_log(event_tx, "error", "relevance",
                "Rate limited after all retry attempts".to_string());
            return;
        }

        match r.json().await {
            Ok(j) => { resp_json = j; break; }
            Err(e) => {
                crate::daemon::ws::emit_log(event_tx, "error", "relevance",
                    format!("Failed to parse LLM response: {}", e));
                return;
            }
        }
    }

    if resp_json.is_null() { return; }

    // Log full response JSON for debugging
    let resp_str = serde_json::to_string(&resp_json).unwrap_or_default();
    eprintln!("rigor relevance: full API response: {}", &resp_str[..resp_str.len().min(800)]);

    // Extract text from Anthropic response
    let text = resp_json.get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    // Log raw response for debugging
    eprintln!("rigor relevance: raw LLM response: {}", &text[..text.len().min(500)]);

    // Broadcast full evaluation details to dashboard
    let _ = event_tx.send(DaemonEvent::JudgeEvaluation {
        eval_type: "relevance".to_string(),
        prompt: Some(prompt.clone()),
        response: Some(text.to_string()),
        claims: Some(uncached_claims.iter().map(|(id, t)| format!("[{}] {}", id, t)).collect()),
        constraints: Some(constraints.iter().map(|(id, d)| format!("[{}] {}", id, d)).collect()),
        result: Some(format!("{} lines in response", text.lines().count())),
        duration_ms: None,
    });

    // Build a map from claim_id -> claim_text for cache keying
    let claim_text_map: HashMap<String, String> = uncached_claims.iter()
        .map(|(id, text)| (id.clone(), text.clone()))
        .collect();

    let mut match_count = 0;

    // Parse lines: claim_id|constraint_id|relevance|reason
    for line in text.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() >= 3 {
            let claim_id = parts[0].trim().to_string();
            let constraint_id = parts[1].trim().to_string();
            let relevance = parts[2].trim().to_lowercase();
            let reason = if parts.len() >= 4 { parts[3].trim().to_string() } else { String::new() };

            if relevance == "high" || relevance == "medium" {
                let _ = event_tx.send(DaemonEvent::ClaimRelevance {
                    claim_id: claim_id.clone(),
                    constraint_id: constraint_id.clone(),
                    relevance: relevance.clone(),
                    reason: reason.clone(),
                });
                match_count += 1;

                // Cache the result keyed by claim text
                if let Some(claim_text) = claim_text_map.get(&claim_id) {
                    let mut cache = RELEVANCE_CACHE.lock().unwrap();
                    cache.entry(claim_text.clone())
                        .or_insert_with(Vec::new)
                        .push((constraint_id, relevance, reason));
                }
            }
        }
    }

    crate::daemon::ws::emit_log(event_tx, "info", "relevance",
        format!("LLM-as-judge: {} relevance links from {} claims", match_count, uncached_claims.len()));
}

/// Parse SSE event stream and extract accumulated assistant text.
/// Handles both Anthropic and OpenAI streaming formats.
fn extract_sse_assistant_text(sse_data: &str, path: &str) -> Option<String> {
    let mut text_parts = Vec::new();

    for line in sse_data.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" {
                break;
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                if path.contains("messages") {
                    // Anthropic streaming: content_block_delta events
                    if json.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
                        if let Some(text) = json.get("delta")
                            .and_then(|d| d.get("text"))
                            .and_then(|t| t.as_str())
                        {
                            text_parts.push(text.to_string());
                        }
                    }
                } else {
                    // OpenAI streaming: choices[0].delta.content
                    if let Some(content) = json.get("choices")
                        .and_then(|c| c.as_array())
                        .and_then(|a| a.first())
                        .and_then(|c| c.get("delta"))
                        .and_then(|d| d.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        text_parts.push(content.to_string());
                    }
                }
            }
        }
    }

    let full_text = text_parts.join("");
    if full_text.is_empty() {
        None
    } else {
        Some(full_text)
    }
}

/// Extract token usage from SSE event stream.
/// Anthropic: message_start has input_tokens, message_delta has output_tokens.
/// OpenAI: final chunk may contain usage field.
fn extract_sse_usage(sse_data: &str, path: &str) -> (u64, u64) {
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;

    for line in sse_data.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" { break; }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                if path.contains("messages") {
                    // Anthropic: message_start → message.usage.input_tokens
                    if json.get("type").and_then(|t| t.as_str()) == Some("message_start") {
                        if let Some(it) = json.get("message")
                            .and_then(|m| m.get("usage"))
                            .and_then(|u| u.get("input_tokens"))
                            .and_then(|v| v.as_u64())
                        {
                            input_tokens = it;
                        }
                    }
                    // Anthropic: message_delta → usage.output_tokens
                    if json.get("type").and_then(|t| t.as_str()) == Some("message_delta") {
                        if let Some(ot) = json.get("usage")
                            .and_then(|u| u.get("output_tokens"))
                            .and_then(|v| v.as_u64())
                        {
                            output_tokens = ot;
                        }
                    }
                } else {
                    // OpenAI: usage in final chunk
                    if let Some(usage) = json.get("usage") {
                        if let Some(pt) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
                            input_tokens = pt;
                        }
                        if let Some(ct) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
                            output_tokens = ct;
                        }
                    }
                }
            }
        }
    }
    (input_tokens, output_tokens)
}

/// Extract token usage from a buffered (non-streaming) API response.
fn extract_usage(body: &serde_json::Value, path: &str) -> (u64, u64) {
    if let Some(usage) = body.get("usage") {
        if path.contains("messages") {
            // Anthropic: usage.input_tokens, usage.output_tokens
            let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            (input, output)
        } else {
            // OpenAI: usage.prompt_tokens, usage.completion_tokens
            let input = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let output = usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            (input, output)
        }
    } else {
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- apply_provider_auth -----------------------------------------------

    fn auth_header(req: reqwest::RequestBuilder, name: &str) -> Option<String> {
        let built = req.build().expect("request builds");
        built.headers().get(name).and_then(|v| v.to_str().ok()).map(String::from)
    }

    fn fresh_req() -> reqwest::RequestBuilder {
        reqwest::Client::new().post("https://example.test/v1/messages")
    }

    #[test]
    fn auth_anthropic_api_key_uses_x_api_key() {
        let req = apply_provider_auth(fresh_req(), "sk-ant-api03-abcdef");
        assert_eq!(auth_header(req, "x-api-key"), Some("sk-ant-api03-abcdef".into()));
    }

    #[test]
    fn auth_anthropic_oauth_uses_bearer() {
        let req = apply_provider_auth(fresh_req(), "sk-ant-oat01-xyz");
        assert_eq!(
            auth_header(req, "authorization"),
            Some("Bearer sk-ant-oat01-xyz".into())
        );
    }

    #[test]
    fn auth_openrouter_uses_bearer() {
        let req = apply_provider_auth(fresh_req(), "sk-or-v1-thekey");
        assert_eq!(
            auth_header(req, "authorization"),
            Some("Bearer sk-or-v1-thekey".into())
        );
    }

    #[test]
    fn auth_openai_uses_bearer() {
        let req = apply_provider_auth(fresh_req(), "sk-proj-abc123");
        assert_eq!(
            auth_header(req, "authorization"),
            Some("Bearer sk-proj-abc123".into())
        );
    }

    #[test]
    fn auth_anthropic_oauth_never_leaks_into_x_api_key() {
        let req = apply_provider_auth(fresh_req(), "sk-ant-oat01-would-401");
        let built = req.build().unwrap();
        assert!(
            built.headers().get("x-api-key").is_none(),
            "OAuth tokens must never appear in x-api-key — they trigger Anthropic 401 'User not found'"
        );
    }

    // ---- replace_last_user_content -----------------------------------------

    #[test]
    fn redact_replaces_plain_string_user_content() {
        let mut body = json!({
            "messages": [
                {"role": "user", "content": "my secret is sk-or-v1-leak"},
                {"role": "assistant", "content": "ok"},
                {"role": "user", "content": "my secret is sk-or-v1-leak again"}
            ]
        });
        replace_last_user_content(&mut body, "[REDACTED]");
        // Only the LAST user message is rewritten.
        assert_eq!(body["messages"][0]["content"], "my secret is sk-or-v1-leak");
        assert_eq!(body["messages"][2]["content"], "[REDACTED]");
        // Assistant message untouched.
        assert_eq!(body["messages"][1]["content"], "ok");
    }

    #[test]
    fn redact_replaces_text_block_in_structured_content() {
        let mut body = json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "here's my key: sk-or-v1-leak"}
                ]
            }]
        });
        replace_last_user_content(&mut body, "[REDACTED]");
        assert_eq!(body["messages"][0]["content"][0]["text"], "[REDACTED]");
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    }

    #[test]
    fn redact_is_noop_when_no_user_message() {
        let mut body = json!({
            "messages": [
                {"role": "assistant", "content": "hi"},
                {"role": "system", "content": "you are helpful"}
            ]
        });
        let before = body.clone();
        replace_last_user_content(&mut body, "[REDACTED]");
        assert_eq!(body, before);
    }

    #[test]
    fn redact_is_noop_when_messages_missing() {
        let mut body = json!({"model": "claude"});
        let before = body.clone();
        replace_last_user_content(&mut body, "[REDACTED]");
        assert_eq!(body, before);
    }

    // ---- detect_pii / PII_SANITIZER pattern coverage -----------------------
    //
    // These are regression tests for each custom pattern registered in
    // PII_SANITIZER so nobody silently removes one without noticing.

    fn detected_kinds(text: &str) -> Vec<String> {
        detect_pii(text).into_iter().map(|(k, _)| k).collect()
    }

    #[test]
    fn pii_detects_email() {
        assert!(detected_kinds("contact vibhav@example.com today").iter().any(|k| k == "Email"));
    }

    #[test]
    fn pii_detects_credit_card_with_luhn() {
        // 4111-1111-1111-1111 is the canonical Visa test number, passes Luhn.
        assert!(detected_kinds("pay with 4111111111111111").iter().any(|k| k == "CreditCard"));
    }

    #[test]
    fn pii_detects_ssn() {
        assert!(detected_kinds("SSN: 123-45-6789").iter().any(|k| k.contains("SSN")));
    }

    #[test]
    fn pii_detects_openrouter_key() {
        let k = "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";
        assert!(detected_kinds(k).iter().any(|k| k.contains("OpenRouter")));
    }

    #[test]
    fn pii_detects_anthropic_api_key() {
        let k = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ";
        assert!(detected_kinds(k).iter().any(|k| k.contains("AnthropicApiKey")));
    }

    #[test]
    fn pii_detects_anthropic_oauth() {
        let k = "sk-ant-oat01-abcdefghijklmnopqrstuvwxyz01234567";
        assert!(detected_kinds(k).iter().any(|k| k.contains("AnthropicOAuth")));
    }

    #[test]
    fn pii_detects_github_pat() {
        // Real GitHub PATs are exactly `ghp_` + 36 alnum chars. Regex
        // requires exact 36 (\b boundary) so test uses exact length.
        let k = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
        assert_eq!(k.len(), 40, "fixture has wrong length");
        assert!(!detected_kinds(k).is_empty(), "actual: {:?}", detected_kinds(k));
    }

    #[test]
    fn pii_detects_jwt() {
        // Each segment must be 16+ chars per our tightened regex — loose
        // 3-char signatures from earlier tests no longer qualify.
        let jwt = concat!(
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
            ".eyJzdWIiOiIxMjM0NTY3ODkwIiwiaWF0IjoxNTE2MjM5MDIyfQ",
            ".SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
        );
        assert!(
            detected_kinds(jwt).iter().any(|k| k.contains("JWT")),
            "actual: {:?}",
            detected_kinds(jwt)
        );
    }

    #[test]
    fn pii_detects_private_key_header() {
        assert!(
            detected_kinds("-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAA...")
                .iter()
                .any(|k| k.contains("PrivateKey"))
        );
    }

    #[test]
    fn pii_detects_slack_token() {
        // Real Slack tokens are 50+ chars after the `xoxp-` prefix.
        let token = "xoxp-1234567890-0987654321-abcdefghijklmnopqrstuvwxyz01234567";
        assert!(
            detected_kinds(token).iter().any(|k| k.contains("Slack")),
            "actual: {:?}",
            detected_kinds(token)
        );
    }

    #[test]
    fn pii_detects_database_url_with_credentials() {
        assert!(
            detected_kinds("postgres://admin:hunter2@db.internal:5432/prod")
                .iter()
                .any(|k| k.contains("Database"))
        );
    }

    #[test]
    fn pii_clean_text_returns_empty() {
        let findings = detect_pii("The quick brown fox jumps over the lazy dog.");
        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    }

    /// Near-miss: a short blob starting with sk-or-v1 but shorter than our
    /// 32-alnum-suffix requirement should not trigger. Prevents false
    /// positives from strings like URL fragments.
    #[test]
    fn pii_near_miss_openrouter_too_short() {
        let kinds = detected_kinds("sk-or-v1-tooshort");
        assert!(
            !kinds.iter().any(|k| k.contains("OpenRouter")),
            "short sk-or-v1 should not match: {kinds:?}"
        );
    }

    #[test]
    fn test_extract_sse_anthropic() {
        let sse = "event: content_block_start\ndata: {\"type\":\"content_block_start\"}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello \"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"world\"}}\n\ndata: [DONE]\n";
        let result = extract_sse_assistant_text(sse, "/v1/messages");
        assert_eq!(result, Some("Hello world".to_string()));
    }

    #[test]
    fn test_extract_sse_openai() {
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello \"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"world\"}}]}\n\ndata: [DONE]\n";
        let result = extract_sse_assistant_text(sse, "/v1/chat/completions");
        assert_eq!(result, Some("Hello world".to_string()));
    }

    #[test]
    fn test_extract_sse_empty() {
        let sse = "data: [DONE]\n";
        let result = extract_sse_assistant_text(sse, "/v1/messages");
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_sse_no_data() {
        let sse = "event: ping\n\n";
        let result = extract_sse_assistant_text(sse, "/v1/messages");
        assert_eq!(result, None);
    }
}
