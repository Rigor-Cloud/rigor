# Phase 7: crates/rigor/tests/ integration test infrastructure - Pattern Map

**Mapped:** 2026-04-24
**Files analyzed:** 8 new/modified files
**Analogs found:** 8 / 8

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/rigor-harness/Cargo.toml` | config | N/A | `crates/rigor/Cargo.toml` | role-match |
| `crates/rigor-harness/src/lib.rs` | module-root | re-export | `crates/rigor/src/daemon/egress/mod.rs` | exact |
| `crates/rigor-harness/src/home.rs` | utility | file-I/O | `crates/rigor/tests/invariants.rs` (B10) | exact |
| `crates/rigor-harness/src/ca.rs` | utility | transform | `crates/rigor/src/daemon/tls.rs` | exact |
| `crates/rigor-harness/src/mock_llm.rs` | service | streaming (SSE) | `crates/rigor/src/daemon/proxy.rs` (SSE format) + `crates/rigor/tests/egress_integration.rs` (test server pattern) | role-match |
| `crates/rigor-harness/src/proxy.rs` | service | request-response | `crates/rigor/src/daemon/mod.rs` (build_router + DaemonState) | exact |
| `crates/rigor-harness/src/subprocess.rs` | utility | request-response | `crates/rigor/tests/support/mod.rs` + `crates/rigor/tests/true_e2e.rs` | exact |
| `crates/rigor-harness/src/sse.rs` | utility | streaming | `crates/rigor/src/daemon/proxy.rs` (extract_sse_assistant_text) | exact |

## Pattern Assignments

### `crates/rigor-harness/Cargo.toml` (config)

**Analog:** `crates/rigor/Cargo.toml` (lines 1-7) and workspace root `Cargo.toml`

**Workspace member pattern** (workspace root Cargo.toml):
```toml
[workspace]
members = [
    "crates/rigor",
    "crates/rigor-harness",
    "crates/rigor-test",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"
```

**Existing harness Cargo.toml** (`crates/rigor-harness/Cargo.toml` lines 1-12):
```toml
[package]
name = "rigor-harness"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Test harness primitives for rigor: MockAgent, MockLLM, TestDaemon, and friends. Dev-dependency and publishable for future adapter authors."

[lib]
path = "src/lib.rs"

[dependencies]
```

**Key dependencies to add** (all already resolved in workspace lockfile -- zero new deps):
- `rigor` (path dep to `../rigor`) -- for `DaemonState`, `build_router`, `RigorCA`, `FilterChain`, `SseChunk`, `ConversationCtx`
- `tokio` with `rt-multi-thread`, `macros`, `net`, `sync` features
- `axum` 0.8 with SSE support
- `rcgen` 0.13 with `pem` feature
- `rustls` 0.23 with `ring` feature
- `tokio-rustls` 0.26
- `reqwest` 0.12 with `stream`, `rustls-tls` features
- `tempfile` 3
- `serde_json` 1.0
- `futures-util` 0.3
- `tokio-stream` 0.1
- `anyhow` 1.0

**Consumer pattern** (`crates/rigor/Cargo.toml` dev-dependencies, lines 75-77):
```toml
[dev-dependencies]
tempfile = "3"
criterion = { version = "0.5", features = ["html_reports"] }
```
Add `rigor-harness = { path = "../rigor-harness" }` here.

---

### `crates/rigor-harness/src/lib.rs` (module-root, re-export)

**Analog:** `crates/rigor/src/daemon/egress/mod.rs` (lines 1-9)

**Module declaration + re-export pattern:**
```rust
pub mod chain;
pub mod claim_injection;
pub mod ctx;
pub mod frozen;

pub use chain::*;
pub use claim_injection::*;
pub use ctx::*;
pub use frozen::*;
```

**Apply as:** Declare submodules `home`, `ca`, `mock_llm`, `proxy`, `sse`, `subprocess` and re-export their primary types. The existing lib.rs (lines 1-8) has only doc comments -- replace the body while preserving the `//!` crate-level docs.

---

### `crates/rigor-harness/src/home.rs` (utility, file-I/O) -- IsolatedHome

**Analog:** `crates/rigor/tests/invariants.rs` lines 144-175 (B10 test)

**TempDir + fake HOME pattern** (invariants.rs lines 147-155):
```rust
let temp = tempfile::TempDir::new().unwrap();
// Explicitly avoid copying rigor.yaml -- we want the no-config path.
assert!(
    !temp.path().join("rigor.yaml").exists(),
    "sanity: temp dir should have no rigor.yaml"
);

// Fake HOME so ~/.rigor/daemon.pid certainly doesn't exist.
let fake_home = temp.path().join("home");
fs::create_dir_all(&fake_home).unwrap();
```

**Command env override pattern** (invariants.rs lines 166-174):
```rust
let mut cmd = Command::new(env!("CARGO_BIN_EXE_rigor"));
cmd.current_dir(temp.path())
    .env("HOME", fake_home.to_string_lossy().to_string())
    // RIGOR_TEST_CLAIMS is intentionally unset here
    .env_remove("RIGOR_TEST_CLAIMS")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
```

**Production HOME usage pattern** (`daemon/tls.rs` lines 19-24, `daemon/mod.rs` lines 24-26):
```rust
// tls.rs -- both ca_cert_path() and ca_key_path() derive from HOME
fn ca_cert_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".rigor")
        .join("ca.pem")
}

// mod.rs -- daemon_pid_file() uses HOME
pub fn daemon_pid_file() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".rigor/daemon.pid"))
}
```

**Key design constraint:** `IsolatedHome` must NOT call `std::env::set_var("HOME", ...)` globally because `cargo test` runs tests in parallel threads. Instead:
1. For subprocess tests: pass path via `Command::env("HOME", ...)` (already safe)
2. Provide `home_str()` method returning the path string for callers to use with `Command::env()`
3. Create `.rigor/` directory inside the TempDir for CA and PID file isolation

---

### `crates/rigor-harness/src/ca.rs` (utility, transform) -- TestCA

**Analog:** `crates/rigor/src/daemon/tls.rs` lines 34-168

**CryptoProvider installation pattern** (tls.rs line 45):
```rust
let _ = rustls::crypto::ring::default_provider().install_default();
```
The `let _ =` suppresses the error when already installed (prevents double-install panic).

**CA cert generation pattern** (tls.rs lines 74-108):
```rust
let mut ca_params = rcgen::CertificateParams::default();
ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
ca_params
    .distinguished_name
    .push(rcgen::DnType::CommonName, "rigor CA".to_string());
ca_params
    .distinguished_name
    .push(rcgen::DnType::OrganizationName, "rigor".to_string());
ca_params.key_usages = vec![
    rcgen::KeyUsagePurpose::KeyCertSign,
    rcgen::KeyUsagePurpose::CrlSign,
];

let ca_key = rcgen::KeyPair::generate().context("Failed to generate CA key")?;
let ca_cert_signed = ca_params
    .self_signed(&ca_key)
    .context("Failed to self-sign CA cert")?;
```

**Per-host cert signing pattern** (tls.rs lines 129-168):
```rust
let mut params = rcgen::CertificateParams::new(vec![hostname.to_string()])
    .context("Failed to create cert params")?;
params
    .distinguished_name
    .push(rcgen::DnType::CommonName, hostname.to_string());
params
    .distinguished_name
    .push(rcgen::DnType::OrganizationName, "rigor".to_string());

let host_key = rcgen::KeyPair::generate().context("Failed to generate host key")?;
let host_cert = params
    .signed_by(&host_key, &self.ca_cert_signed, &self.ca_key)
    .context("Failed to sign host cert with CA")?;

let host_cert_der = host_cert.der().clone();
let ca_cert_der = self.ca_cert_signed.der().clone();
let host_key_der = host_key.serialize_der();

// Build cert chain: host cert + CA cert
let certs = vec![
    rustls::pki_types::CertificateDer::from(host_cert_der.to_vec()),
    rustls::pki_types::CertificateDer::from(ca_cert_der.to_vec()),
];
let key = rustls::pki_types::PrivateKeyDer::try_from(host_key_der)
    .map_err(|e| anyhow::anyhow!("failed to parse host key: {}", e))?;

let config = ServerConfig::builder()
    .with_no_client_auth()
    .with_single_cert(certs, key)?;
```

**Key difference from production:** TestCA is ephemeral (in-memory only, never persists to disk). Production `RigorCA::load_or_generate()` persists to `~/.rigor/ca.pem` + `ca-key.pem`. TestCA should skip the persistence logic entirely and also provide `reqwest_client()` returning a reqwest::Client that trusts the test CA (via `ca_cert.pem()` and `reqwest::tls::Certificate::from_pem()`).

---

### `crates/rigor-harness/src/mock_llm.rs` (service, streaming/SSE) -- MockLlmServer

**Analog (SSE format):** `crates/rigor/src/daemon/proxy.rs` lines 3812-3855

**Anthropic SSE parsing pattern** (proxy.rs lines 3812-3855):
```rust
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
                        if let Some(text) = json
                            .get("delta")
                            .and_then(|d| d.get("text"))
                            .and_then(|t| t.as_str())
                        {
                            text_parts.push(text.to_string());
                        }
                    }
                } else {
                    // OpenAI streaming: choices[0].delta.content
                    if let Some(content) = json
                        .get("choices")
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
    if full_text.is_empty() { None } else { Some(full_text) }
}
```

**Analog (test filter pattern):** `crates/rigor/tests/egress_integration.rs` lines 1-9, 41-66

**Integration test import pattern** (egress_integration.rs lines 1-8):
```rust
use async_trait::async_trait;
use serde_json::Value as Json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rigor::daemon::egress::*;
```

**Tokio test + axum serve pattern** (derived from production `daemon/mod.rs` lines 449-468):
```rust
let rt = tokio::runtime::Runtime::new()?;
rt.block_on(async move {
    let app = build_router(shared);
    let http_app = app.clone();
    let http_handle = tokio::spawn(async move {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, http_app).await.unwrap();
    });
```

**MockLlmServer must produce SSE in both formats:**
1. Anthropic format: `message_start`, `content_block_start`, `content_block_delta` (with `type:text_delta`), `content_block_stop`, `message_delta`, `message_stop`
2. OpenAI format: `choices[].delta.role`, `choices[].delta.content`, `[DONE]`

Use `axum::response::sse::{Event, Sse}` and bind to `127.0.0.1:0` for ephemeral port. Use `oneshot::Sender` for graceful shutdown via `with_graceful_shutdown`.

---

### `crates/rigor-harness/src/proxy.rs` (service, request-response) -- TestProxy

**Analog:** `crates/rigor/src/daemon/mod.rs` lines 163-406 (DaemonState) and 573-659 (build_router)

**DaemonState::load() pattern** (mod.rs lines 209-315):
```rust
impl DaemonState {
    pub fn load(yaml_path: PathBuf, event_tx: EventSender) -> Result<Self> {
        let config = load_rigor_config(&yaml_path)?;
        let mut graph = ArgumentationGraph::from_config(&config);
        graph.compute_strengths()?;

        let target_api = std::env::var("RIGOR_TARGET_API")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        // ...
```

**EventSender creation** (`daemon/ws.rs` line 301, 304):
```rust
pub type EventSender = broadcast::Sender<DaemonEvent>;
pub fn create_event_channel() -> (EventSender, broadcast::Receiver<DaemonEvent>) {
```

**build_router pattern** (mod.rs lines 573-659):
```rust
pub fn build_router(state: SharedState) -> Router {
    use crate::cli::web;
    let event_tx = state.lock().unwrap().event_tx.clone();
    Router::new()
        .route("/v1/messages", post(proxy::anthropic_proxy))
        .route("/v1/chat/completions", post(proxy::openai_proxy))
        // ... many routes ...
        .with_state(state)
}
```

**Axum serve on ephemeral port** (mod.rs lines 460-468):
```rust
let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
axum::serve(listener, http_app).await.unwrap();
```

**TestProxy must:**
1. Create `IsolatedHome`, write `rigor.yaml` into it
2. Set `HOME` on the environment for the DaemonState (thread-safety caveat applies -- use `unsafe { std::env::set_var() }` guarded or pass explicitly)
3. Create `DaemonState::load()` with event channel
4. Call `build_router()` with `SharedState`
5. Bind to `127.0.0.1:0`, expose `addr()` and `url()`
6. Shutdown via `oneshot::Sender` on Drop

---

### `crates/rigor-harness/src/subprocess.rs` (utility, request-response)

**Analog:** `crates/rigor/tests/support/mod.rs` lines 80-124 and `crates/rigor/tests/true_e2e.rs` lines 14-45

**Subprocess spawn pattern** (support/mod.rs lines 80-124):
```rust
pub fn run_rigor_with_fixture(fixture: &Fixture) -> (String, String, i32) {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let rigor_yaml = production_rigor_yaml();
    fs::write(temp.path().join("rigor.yaml"), rigor_yaml).expect("write rigor.yaml");

    let claims = json!([{
        "id": "c1",
        "text": fixture.text,
        "confidence": fixture.confidence,
        "claim_type": fixture.claim_type,
    }]);

    let input = json!({
        "session_id": "pr-2.6-fixture",
        "transcript_path": temp.path().join("transcript.jsonl").to_string_lossy(),
        "cwd": temp.path().to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": false,
    });

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rigor"));
    cmd.current_dir(temp.path())
        .env("RIGOR_TEST_CLAIMS", claims.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn rigor");
    child.stdin.as_mut().expect("open stdin")
        .write_all(input.to_string().as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait rigor");

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}
```

**E2E variant without RIGOR_TEST_CLAIMS** (true_e2e.rs lines 14-45):
```rust
fn run_rigor_e2e(dir: &std::path::Path) -> (String, String, i32) {
    let input = json!({
        "session_id": "e2e-test",
        "transcript_path": dir.join("transcript.jsonl").to_string_lossy(),
        "cwd": dir.to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "stop",
        "stop_hook_active": false
    });

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rigor"));
    cmd.current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.env_remove("RIGOR_TEST_CLAIMS");
    // ...
}
```

**JSON response parsing** (true_e2e.rs lines 47-50):
```rust
fn parse_response(stdout: &str) -> Value {
    serde_json::from_str(stdout)
        .unwrap_or_else(|_| panic!("Failed to parse JSON response: {}", stdout))
}
```

**Decision extraction** (support/mod.rs lines 137-150):
```rust
pub fn extract_decision(stdout: &str) -> Option<String> {
    let value: Value = serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("failed to parse hook response: {}\nstdout: {}", e, stdout));
    value.get("decision").and_then(|d| d.as_str()).map(|s| s.to_string())
}

pub fn decision_or_none(stdout: &str) -> String {
    extract_decision(stdout).unwrap_or_else(|| "none".to_string())
}
```

**Consolidation note:** The harness `subprocess.rs` should provide a `run_rigor()` builder/function that accepts an `IsolatedHome` reference (for HOME isolation) and abstracts the common `Command::new(env!("CARGO_BIN_EXE_rigor"))` + stdin-pipe + stdout-capture pattern. The existing `tests/support/mod.rs` should NOT be merged -- it serves a separate purpose (fixture loading, `require_openrouter!` macro).

---

### `crates/rigor-harness/src/sse.rs` (utility, streaming)

**Analog:** `crates/rigor/src/daemon/proxy.rs` lines 3812-3855 and `crates/rigor/src/daemon/egress/chain.rs` lines 12-17

**SseChunk type** (chain.rs lines 12-17):
```rust
/// Wraps a raw SSE `data:` line.
#[derive(Debug, Clone)]
pub struct SseChunk {
    pub data: String,
}
```

**SSE text extraction** (proxy.rs lines 3812-3855 -- full excerpt in mock_llm.rs section above).

**SSE test data construction** (egress_integration.rs lines 266-270):
```rust
let raw_chunks = [
    "data: {\"delta\":{\"text\":\"hello\"}}\n\n",
    "data: {\"delta\":{\"text\":\" world\"}}\n\n",
    "data: [DONE]\n\n",
];
```

**SSE client helpers should provide:**
1. `parse_sse_stream(response: reqwest::Response) -> Vec<String>` -- collect SSE data lines from a streaming response
2. `extract_text_from_sse(events: &[String], format: SseFormat) -> String` -- extract accumulated text using the dual-format parsing from proxy.rs
3. An `SseFormat` enum: `Anthropic` / `OpenAI`

---

## Shared Patterns

### Error Handling
**Source:** Consistent `anyhow::Result` + `.context()` throughout daemon code
**Apply to:** All harness modules (`ca.rs`, `proxy.rs`, `subprocess.rs`)

```rust
// daemon/tls.rs lines 15-16, 44-46
use anyhow::{Context, Result};

let ca_key = rcgen::KeyPair::generate().context("Failed to generate CA key")?;
let ca_cert_signed = ca_params
    .self_signed(&ca_key)
    .context("Failed to self-sign CA cert")?;
```

However, for test utilities the convention is `expect("descriptive message")` for unrecoverable failures (as seen in `tests/support/mod.rs` line 81: `tempfile::TempDir::new().expect("tempdir")`). Use `expect()` for setup/teardown operations that should panic with a clear message if they fail. Use `Result` only for operations callers might want to handle.

### TLS CryptoProvider Initialization
**Source:** `crates/rigor/src/daemon/tls.rs` line 45
**Apply to:** `ca.rs` (TestCA::new())

```rust
let _ = rustls::crypto::ring::default_provider().install_default();
```

The `let _ =` is critical -- it prevents panics from double-installation when multiple tests create TestCA instances.

### Tokio Test Runtime
**Source:** `crates/rigor/tests/egress_integration.rs` lines 41, 69, 258
**Apply to:** All async test code consuming the harness

```rust
#[tokio::test]
async fn test_name() {
    // test body
}
```

### Graceful Shutdown via Oneshot Channel
**Source:** Derived from `daemon/mod.rs` axum serve pattern (lines 460-468)
**Apply to:** `mock_llm.rs` (MockLlmServer), `proxy.rs` (TestProxy)

```rust
let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
let handle = tokio::spawn(async move {
    axum::serve(listener, app)
        .with_graceful_shutdown(async { let _ = shutdown_rx.await; })
        .await
        .unwrap();
});
// In Drop: let _ = shutdown_tx.send(());
```

### Ephemeral Port Binding
**Source:** Standard Rust/tokio pattern used across daemon code
**Apply to:** `mock_llm.rs`, `proxy.rs`

```rust
let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
let addr = listener.local_addr().unwrap();
```

Never hardcode ports. Always bind to port 0 and extract the assigned address.

### Import Conventions
**Source:** `crates/rigor/tests/invariants.rs` lines 1-8, `egress_integration.rs` lines 1-8
**Apply to:** All harness modules and consuming test code

```rust
// For production type imports (tests using rigor crate):
use rigor::daemon::egress::*;
use rigor::daemon::ws::create_event_channel;
use rigor::daemon::{DaemonState, SharedState, build_router};

// For std imports:
use std::sync::Arc;
use std::path::PathBuf;

// For external crate imports:
use serde_json::{json, Value};
```

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| (none) | -- | -- | All files have strong analogs in production or existing test code |

## Metadata

**Analog search scope:** `crates/rigor/src/daemon/` (production code), `crates/rigor/tests/` (existing test infrastructure), `crates/rigor-harness/` (existing placeholder)
**Files scanned:** 12 test files, 6 daemon modules, 2 Cargo.toml files
**Pattern extraction date:** 2026-04-24
