# Phase 7: crates/rigor/tests/ integration test infrastructure - Research

**Researched:** 2026-04-24
**Domain:** Rust integration test infrastructure -- TCP servers, TLS/rustls, SSE streaming, HOME isolation
**Confidence:** HIGH

## Summary

This phase builds the shared test-support library that Phases 9--12 depend on. The codebase has 12 integration test files with significant duplication: `run_rigor_*()` and `parse_response()` are copy-pasted across 5+ files, only 1 of 12 tests isolates `$HOME`, and zero tests exercise real TCP/TLS/SSE paths. The `crates/rigor-harness/` workspace member exists as an empty placeholder with the right Cargo.toml metadata already in place.

The production codebase already depends on every library needed for the test harness (tokio, axum, hyper, rustls, tokio-rustls, rcgen, reqwest, futures-util, tokio-stream), so no new crate dependencies are required. The harness should be built as a library crate inside `crates/rigor-harness/` and consumed as a dev-dependency by `crates/rigor/`. The existing `tests/support/mod.rs` remains for fixture-specific helpers (Fixture struct, walk_fixtures, require_openrouter! macro) and should NOT be merged into the harness -- it serves a different purpose (fixture file loading for subprocess tests).

**Primary recommendation:** Build `rigor-harness` as a dev-dependency library exposing four primary primitives: `IsolatedHome` (TempDir-based $HOME override), `TestCA` (in-memory rcgen CA for tests), `MockLlmServer` (axum-based SSE server on ephemeral port), and `TestProxy` (bring up the rigor daemon proxy on an ephemeral port with isolated HOME). All primitives use the Builder pattern and Drop-based cleanup.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
None -- infrastructure phase. All implementation choices at Claude's discretion.

### Claude's Discretion
All implementation choices are at Claude's discretion. Use ROADMAP phase goal, success criteria, and codebase conventions to guide decisions.

Key codebase context informing decisions:
- `crates/rigor-harness/` exists as empty placeholder -- intended for MockAgent, MockLLM, TestDaemon, etc.
- Existing `tests/support/mod.rs` provides Fixture, run_rigor_with_fixture, walk_fixtures -- subprocess-based helpers
- `RigorCA::load_or_generate()` and `daemon_pid_file()` both use `$HOME/.rigor/`
- Production code already depends on tokio, rustls, tokio-rustls, rcgen, axum, hyper, reqwest
- Only `invariants.rs:B10` manually isolates `$HOME`; all other tests risk touching real `~/.rigor/`
- `run_rigor_*()` and `parse_response()` duplicated across 5+ test files

### Deferred Ideas (OUT OF SCOPE)
None.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REQ-015 | `crates/rigor/tests/` contains a shared test-support library exposing: real TCP proxy bring-up, rustls CA generation, SSE client, isolated HOME fixture | Architecture section: IsolatedHome, TestCA, MockLlmServer, TestProxy primitives. All use existing production deps (axum, rcgen, rustls, tokio). Delivered via `crates/rigor-harness/` as dev-dependency. |
| REQ-016 | Each integration test can be run alone (`cargo test --test <name>`) without leaking state into the real `$HOME` | IsolatedHome pattern: TempDir + HOME env var override. `dirs::home_dir()` respects HOME on Unix. Guard struct with Drop cleanup. Tests using IsolatedHome are fully isolated. |
| REQ-017 | Test support library reuses production types where possible; fixtures stub network (mock-LLM) but not internal logic | MockLlmServer stubs the upstream HTTP endpoint only. Tests import and use production FilterChain, SseChunk, ConversationCtx, RigorCA, PolicyEngine directly. No internal logic is mocked. |
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| HOME isolation | Test harness (rigor-harness) | -- | Environment variable manipulation is a test concern, not production code |
| CA cert generation for tests | Test harness | Production TLS module | Reuses rcgen patterns from `daemon/tls.rs` but generates ephemeral (in-memory) certs |
| Mock LLM HTTP server | Test harness | -- | Axum server serving deterministic SSE responses; purely a test double |
| TCP proxy bring-up | Test harness | Production daemon | Instantiates production `build_router()` + `DaemonState` on ephemeral port |
| SSE client for assertions | Test harness | -- | Thin wrapper around reqwest streaming for test assertions |
| Fixture loading | Existing tests/support/ | -- | Remains separate; fixture schema is subprocess-test-specific |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.49.0 | Async runtime for test servers | Already a production dep; `#[tokio::test]` for async tests [VERIFIED: cargo metadata] |
| axum | 0.8.8 | Mock LLM server framework | Already a production dep; SSE support built-in via `axum::response::sse` [VERIFIED: cargo metadata] |
| rustls | 0.23.37 | TLS for test proxy | Already a production dep [VERIFIED: cargo metadata] |
| tokio-rustls | 0.26.4 | Async TLS acceptor | Already a production dep [VERIFIED: cargo metadata] |
| rcgen | 0.13.2 | Test CA certificate generation | Already a production dep; same API as `daemon/tls.rs` [VERIFIED: cargo metadata] |
| reqwest | 0.12.28 | SSE client for test assertions | Already a production dep; streaming support [VERIFIED: cargo metadata] |
| tempfile | 3.24.0 | TempDir for HOME isolation | Already a dev-dep [VERIFIED: cargo metadata] |
| futures-util | 0.3.31 | Stream combinators for SSE | Already a production dep [VERIFIED: cargo metadata] |
| tokio-stream | 0.1.18 | Stream utilities for SSE responses | Already a production dep [VERIFIED: cargo metadata] |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| hyper | 1.8.1 | Low-level HTTP server for proxy | Already a production dep; used for TLS listener setup [VERIFIED: cargo metadata] |
| hyper-util | 0.1.x | Server connection handling | Already a production dep [VERIFIED: cargo metadata] |
| serde_json | 1.0.x | JSON construction for test payloads | Already a production dep [VERIFIED: cargo metadata] |
| bytes | 1.x | Byte buffer handling in SSE streams | Already a production dep [VERIFIED: cargo metadata] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| axum for mock server | wiremock-rs | Adds new dep; axum already in tree and matches production patterns |
| reqwest for SSE client | eventsource-client crate | Adds new dep; reqwest streaming is sufficient for test assertions |
| Manual TempDir | test_dir crate | Adds new dep; tempfile already in tree and well-understood |

**Installation:**
```bash
# No new dependencies needed. Update rigor-harness/Cargo.toml to depend on workspace crates:
# [dependencies] section in crates/rigor-harness/Cargo.toml
```

**Version verification:** All versions confirmed via `cargo metadata --format-version 1` against the resolved lockfile on 2026-04-24. [VERIFIED: cargo metadata]

## Architecture Patterns

### System Architecture Diagram

```
Test Code (crates/rigor/tests/*.rs)
    |
    v
rigor-harness (dev-dependency)
    |
    +---> IsolatedHome
    |       |-- TempDir as fake $HOME
    |       |-- Sets HOME env var
    |       |-- Drop restores original HOME
    |       '-- Creates ~/.rigor/ structure
    |
    +---> TestCA
    |       |-- Ephemeral rcgen CA (in-memory)
    |       |-- server_config_for_host(hostname)
    |       |-- client_config() -> rustls ClientConfig trusting this CA
    |       '-- ca_cert_pem() for reqwest custom CA
    |
    +---> MockLlmServer
    |       |-- axum::Router with SSE endpoints
    |       |-- Configurable per-test response scenarios
    |       |-- Binds to 127.0.0.1:0 (ephemeral port)
    |       |-- Returns SocketAddr for test client
    |       '-- Drop shuts down server
    |
    +---> TestProxy
    |       |-- Wraps production build_router() + DaemonState
    |       |-- Binds to ephemeral port
    |       |-- Uses IsolatedHome + TestCA
    |       |-- Returns proxy SocketAddr
    |       '-- Drop shuts down + cleans up
    |
    '-- Helper functions
            |-- wait_for_port(addr, timeout)
            |-- parse_sse_stream(response) -> Vec<SseChunk>
            |-- assert_decision(response, expected)
            '-- run_rigor_subprocess(home, args) -> Output
```

### Recommended Project Structure
```
crates/rigor-harness/
  Cargo.toml              # Workspace deps: tokio, axum, rcgen, rustls, etc.
  src/
    lib.rs                # Re-exports all primitives
    home.rs               # IsolatedHome -- TempDir + HOME override
    ca.rs                 # TestCA -- ephemeral CA cert generation
    mock_llm.rs           # MockLlmServer -- axum SSE server
    proxy.rs              # TestProxy -- production proxy on ephemeral port
    sse.rs                # SSE client helpers (parse_sse_stream, etc.)
    subprocess.rs         # Consolidated run_rigor_*() and parse_response()

crates/rigor/tests/
  support/mod.rs          # KEPT AS-IS: Fixture, walk_fixtures, require_openrouter!
  (existing test files)   # Gradually migrated to use rigor-harness
```

### Pattern 1: IsolatedHome -- TempDir-based HOME Override
**What:** A guard struct that sets `HOME` to a TempDir path, creating the expected `~/.rigor/` directory structure. On Drop, restores the original `HOME`.
**When to use:** Every test that touches `RigorCA`, `daemon_pid_file()`, violation log, or any `dirs::home_dir()` / `std::env::var("HOME")` code path.
**Key insight:** Both `dirs::home_dir()` and `std::env::var("HOME")` respect the `HOME` environment variable on Unix. [VERIFIED: dirs crate docs + Rust std docs]

**Critical caveat:** `std::env::set_var` is NOT thread-safe. Since `cargo test` runs tests in parallel threads within the same process, two tests setting `HOME` concurrently will race. The solution is one of:
1. Use `#[serial_test::serial]` (would add a dependency)
2. Use subprocess isolation (spawn a child process with `HOME` set)
3. Accept the race for in-process tests and document that tests using `IsolatedHome` with env-var mutation must use `--test-threads=1` or be in their own test binary

**Recommended approach:** For subprocess-based tests (which already spawn a child process), set `HOME` on the `Command` -- this is already safe and is the pattern used by `invariants.rs:B10`. For in-process tests, use a per-test `IsolatedHome` that generates a unique TempDir but does NOT globally mutate `HOME`. Instead, pass the isolated home path explicitly to functions that need it, or use the subprocess approach. [ASSUMED -- architecture decision]

**Example:**
```rust
// Source: pattern derived from invariants.rs:B10
pub struct IsolatedHome {
    _temp: tempfile::TempDir,
    pub path: std::path::PathBuf,
    pub rigor_dir: std::path::PathBuf,
}

impl IsolatedHome {
    pub fn new() -> Self {
        let temp = tempfile::TempDir::new().expect("failed to create temp HOME");
        let path = temp.path().to_path_buf();
        let rigor_dir = path.join(".rigor");
        std::fs::create_dir_all(&rigor_dir).expect("failed to create .rigor dir");
        Self {
            _temp: temp,
            path,
            rigor_dir,
        }
    }

    /// Write a rigor.yaml into the isolated home's working directory
    pub fn write_rigor_yaml(&self, content: &str) -> std::path::PathBuf {
        let yaml_path = self.path.join("rigor.yaml");
        std::fs::write(&yaml_path, content).expect("write rigor.yaml");
        yaml_path
    }

    /// Get HOME value suitable for Command::env("HOME", ...)
    pub fn home_str(&self) -> String {
        self.path.to_string_lossy().to_string()
    }
}
```

### Pattern 2: TestCA -- Ephemeral CA for TLS Tests
**What:** A wrapper around rcgen that generates an ephemeral CA cert + key in memory (never touches disk), and can produce per-host ServerConfigs and a client-side trust store.
**When to use:** Tests that need TLS handshakes (MITM tests, proxy tests, TLS cert validation).
**Example:**
```rust
// Source: pattern derived from daemon/tls.rs RigorCA
pub struct TestCA {
    ca_key: rcgen::KeyPair,
    ca_cert: rcgen::Certificate,
}

impl TestCA {
    pub fn new() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut params = rcgen::CertificateParams::default();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.distinguished_name
            .push(rcgen::DnType::CommonName, "rigor-test-ca".to_string());
        let ca_key = rcgen::KeyPair::generate().expect("generate CA key");
        let ca_cert = params.self_signed(&ca_key).expect("self-sign CA");
        Self { ca_key, ca_cert }
    }

    /// Build a rustls ServerConfig for the given hostname
    pub fn server_config_for_host(&self, hostname: &str) -> Arc<rustls::ServerConfig> {
        // Same logic as RigorCA::server_config_for_host but no caching needed
        // ...
    }

    /// Build a reqwest Client that trusts this CA
    pub fn reqwest_client(&self) -> reqwest::Client {
        let ca_pem = self.ca_cert.pem();
        let cert = reqwest::tls::Certificate::from_pem(ca_pem.as_bytes())
            .expect("parse test CA cert");
        reqwest::Client::builder()
            .add_root_certificate(cert)
            .build()
            .expect("build reqwest client with test CA")
    }
}
```

### Pattern 3: MockLlmServer -- Axum SSE Server
**What:** An axum-based HTTP server that serves deterministic SSE responses configurable per-test. Binds to `127.0.0.1:0` for an ephemeral port.
**When to use:** Tests that need a mock upstream LLM API (Phases 9, 11, 12).
**Example:**
```rust
// Source: axum SSE docs (https://docs.rs/axum/0.8.8/axum/response/sse/index.html)
use axum::{Router, routing::post, response::sse::{Event, Sse}};
use futures_util::stream;
use tokio_stream::StreamExt;
use std::convert::Infallible;

pub struct MockLlmServer {
    addr: std::net::SocketAddr,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockLlmServer {
    pub async fn start(chunks: Vec<String>) -> Self {
        let app = Router::new()
            .route("/v1/messages", post(move || {
                let chunks = chunks.clone();
                async move {
                    let stream = stream::iter(chunks.into_iter().map(|c| {
                        Ok::<_, Infallible>(Event::default().data(c))
                    }));
                    Sse::new(stream)
                }
            }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await.expect("bind mock LLM");
        let addr = listener.local_addr().expect("local addr");
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async { let _ = shutdown_rx.await; })
                .await
                .unwrap();
        });

        Self { addr, shutdown_tx, _handle: handle }
    }

    pub fn addr(&self) -> std::net::SocketAddr { self.addr }
    pub fn url(&self) -> String { format!("http://{}", self.addr) }
}

impl Drop for MockLlmServer {
    fn drop(&mut self) {
        // Trigger graceful shutdown; ignore error if already shut down
    }
}
```

### Pattern 4: TestProxy -- Production Proxy on Ephemeral Port
**What:** Brings up the actual production router (`build_router()`) with a `DaemonState` on an ephemeral port. Uses `IsolatedHome` and optionally `TestCA`.
**When to use:** Integration tests that need the full proxy pipeline (Phases 9, 11, 12, 13).
**Example:**
```rust
// Source: derived from daemon/mod.rs start_daemon()
pub struct TestProxy {
    pub addr: std::net::SocketAddr,
    pub home: IsolatedHome,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    _handle: tokio::task::JoinHandle<()>,
}

impl TestProxy {
    pub async fn start(rigor_yaml: &str) -> Self {
        let home = IsolatedHome::new();
        let yaml_path = home.write_rigor_yaml(rigor_yaml);

        let (event_tx, _) = rigor::daemon::ws::create_event_channel();
        let state = rigor::daemon::DaemonState::load(yaml_path, event_tx)
            .expect("load DaemonState");
        let shared = std::sync::Arc::new(std::sync::Mutex::new(state));
        let app = rigor::daemon::build_router(shared);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await.expect("bind test proxy");
        let addr = listener.local_addr().expect("local addr");
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async { let _ = shutdown_rx.await; })
                .await
                .unwrap();
        });

        Self { addr, home, shutdown_tx, _handle: handle }
    }

    pub fn url(&self) -> String { format!("http://{}", self.addr) }
}
```

### Anti-Patterns to Avoid
- **Global HOME mutation in parallel tests:** `std::env::set_var("HOME", ...)` is not thread-safe. Use subprocess-based isolation (setting HOME on Command) or pass paths explicitly. Never call `set_var` in a `#[test]` that runs in parallel.
- **Hardcoded ports:** Always use `127.0.0.1:0` for ephemeral ports. Hardcoded ports cause CI flakiness from port conflicts.
- **Leaking TempDir via std::mem::forget:** Always let TempDir's Drop clean up. If you need the path to outlive the struct, clone it before dropping.
- **Blocking the tokio runtime:** Test servers must run in `tokio::spawn`, not on the test thread. Use `#[tokio::test]` and spawn servers as tasks.
- **Mocking internal logic:** REQ-017 says "stub network but not internal logic." Never mock PolicyEngine, EvaluatorPipeline, or FilterChain -- use the real production types.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| TLS cert generation | Custom OpenSSL bindings | rcgen (already in tree) | rcgen handles all X.509 edge cases; production code already uses it |
| HTTP server for mocks | Raw TCP socket handler | axum (already in tree) | SSE support, routing, graceful shutdown all built-in |
| Temp directory management | Manual mkdir + cleanup | tempfile::TempDir (already in tree) | Drop-based cleanup, cross-platform, handles edge cases |
| SSE parsing in assertions | Line-by-line string parsing | reqwest streaming + serde_json | Production-grade HTTP client already in tree |
| Port availability checks | Manual TCP connect loops | `TcpListener::bind("127.0.0.1:0")` | OS assigns available port; no polling needed |

**Key insight:** Every library needed for this test harness is already a dependency of the production crate. Adding zero new dependencies keeps the build fast and avoids supply-chain risk.

## Common Pitfalls

### Pitfall 1: env::set_var Thread Safety
**What goes wrong:** `std::env::set_var("HOME", ...)` is not thread-safe. Rust 1.66+ warns about this, and it was made unsafe in edition 2024. Two tests mutating HOME concurrently will race.
**Why it happens:** `cargo test` runs tests in parallel threads by default.
**How to avoid:** For subprocess tests, set HOME via `Command::env()` (already safe). For in-process tests, either pass paths explicitly or use `--test-threads=1`.
**Warning signs:** Flaky tests that pass alone but fail when run together. [VERIFIED: Rust edition 2024 changes to env::set_var]

### Pitfall 2: Ephemeral Port Races
**What goes wrong:** Test binds to port 0, gets port N, but by the time the client connects, another process took port N.
**Why it happens:** Between `bind()` and the first `accept()`, there's a window.
**How to avoid:** Bind the listener first, extract the address, then share it with the client. The listener holds the port open. This is the standard pattern with `TcpListener::bind("127.0.0.1:0")`.
**Warning signs:** "Connection refused" errors that are intermittent. [ASSUMED]

### Pitfall 3: DaemonState::load() Touches Real HOME
**What goes wrong:** `DaemonState::load()` calls `RigorCA::load_or_generate()` which writes to `~/.rigor/ca.pem`. Tests that construct DaemonState without HOME isolation will modify the user's real CA.
**Why it happens:** The CA path is computed from `dirs::home_dir()` which reads the HOME env var.
**How to avoid:** Always construct DaemonState within an IsolatedHome context. For subprocess tests, set HOME on the Command. For in-process tests, set HOME before calling DaemonState::load (but beware thread-safety -- see Pitfall 1).
**Warning signs:** Tests create `~/.rigor/ca.pem` or `~/.rigor/daemon.pid` in the real home directory. [VERIFIED: codebase inspection of daemon/tls.rs and daemon/mod.rs]

### Pitfall 4: rustls CryptoProvider Double-Install
**What goes wrong:** `rustls::crypto::ring::default_provider().install_default()` panics if called twice in the same process.
**Why it happens:** Multiple test functions each call it. The second call panics.
**How to avoid:** Use `let _ = install_default()` -- the `let _` suppresses the error when it's already installed. The production code already uses this pattern. Make sure TestCA uses the same pattern.
**Warning signs:** Panic with "CryptoProvider already installed". [VERIFIED: production code in daemon/tls.rs line 45 uses `let _ =`]

### Pitfall 5: Axum Serve Graceful Shutdown
**What goes wrong:** `axum::serve()` runs forever. If the test doesn't shut it down, the tokio task leaks and the test hangs.
**Why it happens:** No shutdown signal configured.
**How to avoid:** Use `axum::serve(listener, app).with_graceful_shutdown(async { ... })` with a oneshot channel. Send on the channel in Drop or explicitly.
**Warning signs:** Tests hang after assertions pass. [VERIFIED: axum 0.8 docs -- Serve::with_graceful_shutdown]

### Pitfall 6: SSE Response Format Differences
**What goes wrong:** Mock LLM returns SSE in wrong format; proxy code can't parse it.
**Why it happens:** Anthropic and OpenAI use different SSE schemas. The proxy has two parsing paths (see `extract_sse_assistant_text` in proxy.rs).
**How to avoid:** MockLlmServer should support both Anthropic and OpenAI SSE formats. Provide builder methods like `.anthropic_format()` and `.openai_format()`.
**Warning signs:** Test assertions fail on empty extracted text despite SSE data being sent. [VERIFIED: proxy.rs lines 3810-3855 shows dual-format parsing]

## Code Examples

### Example 1: Anthropic SSE Format (for MockLlmServer)
```rust
// Source: proxy.rs extract_sse_assistant_text() lines 3810-3855
// Anthropic streaming format:
// data: {"type":"message_start","message":{"id":"msg_01",...}}
// data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}
// data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}
// data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}
// data: {"type":"content_block_stop","index":0}
// data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}
// data: {"type":"message_stop"}

fn anthropic_sse_chunks(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut chunks = vec![
        r#"{"type":"message_start","message":{"id":"msg_test","type":"message","role":"assistant","model":"claude-sonnet-4-20250514","content":[],"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}"#.to_string(),
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string(),
    ];
    for word in &words {
        chunks.push(format!(
            r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{} "}}}}"#,
            word
        ));
    }
    chunks.push(r#"{"type":"content_block_stop","index":0}"#.to_string());
    chunks.push(format!(
        r#"{{"type":"message_delta","delta":{{"stop_reason":"end_turn"}},"usage":{{"output_tokens":{}}}}}"#,
        words.len()
    ));
    chunks.push(r#"{"type":"message_stop"}"#.to_string());
    chunks
}
```

### Example 2: OpenAI SSE Format (for MockLlmServer)
```rust
// Source: proxy.rs extract_sse_assistant_text() lines 3833-3843
// OpenAI streaming format:
// data: {"choices":[{"delta":{"role":"assistant"},"index":0}]}
// data: {"choices":[{"delta":{"content":"Hello"},"index":0}]}
// data: {"choices":[{"delta":{"content":" world"},"index":0}]}
// data: [DONE]

fn openai_sse_chunks(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut chunks = vec![
        r#"{"choices":[{"delta":{"role":"assistant"},"index":0}]}"#.to_string(),
    ];
    for word in &words {
        chunks.push(format!(
            r#"{{"choices":[{{"delta":{{"content":"{} "}},"index":0}}]}}"#,
            word
        ));
    }
    chunks.push("[DONE]".to_string());
    chunks
}
```

### Example 3: Subprocess with Isolated HOME
```rust
// Source: derived from invariants.rs:B10 (lines 144-168)
use std::process::Command;

fn run_rigor_with_home(home: &IsolatedHome, input: &serde_json::Value) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rigor"));
    cmd.current_dir(&home.path)
        .env("HOME", home.home_str())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().expect("spawn rigor");
    child.stdin.as_mut().unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}
```

### Example 4: Consuming MockLlmServer from a Test
```rust
// Source: composition of axum patterns and reqwest streaming
#[tokio::test]
async fn proxy_forwards_sse_to_client() {
    let chunks = anthropic_sse_chunks("Rust uses ownership for memory safety.");
    let mock = MockLlmServer::start(chunks).await;

    // Point the proxy's RIGOR_TARGET_API at the mock
    std::env::set_var("RIGOR_TARGET_API", mock.url());

    let proxy = TestProxy::start(MINIMAL_RIGOR_YAML).await;

    let client = reqwest::Client::new();
    let resp = client.post(format!("{}/v1/messages", proxy.url()))
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    // Read SSE stream and verify content...
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `std::env::set_var` (safe) | `std::env::set_var` (unsafe in edition 2024) | Rust 1.83 / edition 2024 | Must use careful patterns; subprocess isolation preferred [VERIFIED: Rust edition guide] |
| wiremock-rs for mock servers | axum with ephemeral ports | N/A (project choice) | Zero new deps; matches production stack |
| `tests/support/mod.rs` includes | Separate `rigor-harness` crate | This phase | Proper crate boundaries; publishable for future adapter authors |

**Deprecated/outdated:**
- `std::env::home_dir()` -- deprecated since Rust 1.29; `dirs::home_dir()` is the replacement (already used in production). [VERIFIED: Rust std docs]
- Copy-pasting `run_rigor_*()` and `parse_response()` across test files -- replaced by consolidated helpers in rigor-harness.

## Assumptions Log

> List all claims tagged `[ASSUMED]` in this research.

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | IsolatedHome should NOT globally mutate HOME for in-process tests; use subprocess isolation or explicit path passing instead | Architecture Pattern 1 | If wrong, tests may need `--test-threads=1` or a different isolation strategy |
| A2 | Ephemeral port window race is rare enough to not need special handling beyond standard bind-then-connect pattern | Pitfall 2 | If wrong, may need retry logic in port binding |

## Open Questions

1. **Should rigor-harness depend on rigor crate directly?**
   - What we know: rigor-harness needs access to `DaemonState`, `build_router`, `RigorCA`, `FilterChain`, `SseChunk` etc. These are all in the `rigor` crate.
   - What's unclear: Circular dependency risk. If rigor has rigor-harness as dev-dep and rigor-harness has rigor as dep, that's fine (dev-deps don't create cycles). But it means rigor-harness can't be in rigor's regular deps.
   - Recommendation: rigor-harness depends on rigor (regular dep). rigor depends on rigor-harness (dev-dep only). This is the standard Rust pattern for test utility crates. No circular dependency because dev-deps are not transitive.

2. **How should existing test files migrate to rigor-harness?**
   - What we know: 12 test files exist. 5+ have duplicated helpers. Migration could be gradual.
   - What's unclear: Whether to migrate existing tests in this phase or defer to Phases 8-12.
   - Recommendation: This phase builds the harness. Add one example integration test demonstrating each primitive. Defer migration of existing tests to Phase 8 (which explicitly requires REQ-018: "No test writes to real $HOME").

3. **Thread safety for in-process HOME override**
   - What we know: env::set_var is not thread-safe. Subprocess isolation works but is slower.
   - What's unclear: Whether the project is using Rust edition 2024 (where set_var is unsafe) or 2021.
   - Recommendation: The workspace uses `edition = "2021"` [VERIFIED: Cargo.toml]. set_var is still safe in 2021 but logically unsound in parallel tests. Use subprocess isolation for HOME-sensitive tests; this matches the existing invariants.rs pattern.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) |
| Config file | None (Cargo.toml `[dev-dependencies]`) |
| Quick run command | `cargo test -p rigor-harness` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REQ-015 | Shared test-support library with TCP, TLS, SSE, HOME primitives | unit + integration | `cargo test -p rigor-harness` | No -- Wave 0 |
| REQ-016 | Tests run alone without leaking to real $HOME | integration | `cargo test --test harness_smoke` | No -- Wave 0 |
| REQ-017 | Reuse production types, stub network only | integration | `cargo test --test harness_smoke` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p rigor-harness && cargo test -p rigor --test harness_smoke`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/rigor-harness/src/lib.rs` -- implement all four primitives (IsolatedHome, TestCA, MockLlmServer, TestProxy)
- [ ] `crates/rigor-harness/Cargo.toml` -- add workspace dependencies
- [ ] `crates/rigor/tests/harness_smoke.rs` -- smoke test demonstrating each primitive
- [ ] `crates/rigor/Cargo.toml` -- add `rigor-harness` as dev-dependency

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A (test infrastructure) |
| V3 Session Management | no | N/A |
| V4 Access Control | no | N/A |
| V5 Input Validation | no | N/A (test code, not user-facing) |
| V6 Cryptography | yes (test CA certs) | rcgen for cert generation; never persist test CA to disk outside TempDir |

### Known Threat Patterns for Test Infrastructure

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Test CA cert leaking to real trust store | Elevation of Privilege | IsolatedHome ensures CA never written to real ~/.rigor/; TempDir cleanup on Drop |
| Tests modifying real ~/.rigor/ state | Tampering | HOME isolation via subprocess env or IsolatedHome |
| Ephemeral port used by attacker process | Spoofing | Bind to 127.0.0.1 only; tests verify expected responses |

## Sources

### Primary (HIGH confidence)
- Codebase inspection: `crates/rigor/tests/` (12 files), `crates/rigor/src/daemon/` (tls.rs, mod.rs, proxy.rs, sni.rs, egress/chain.rs)
- `cargo metadata --format-version 1` -- resolved dependency versions
- axum 0.8.8 docs (Context7 /websites/rs_axum_0_8_8_axum) -- SSE, serve, testing patterns

### Secondary (MEDIUM confidence)
- Rust std::env::home_dir and dirs crate HOME behavior: https://doc.rust-lang.org/std/env/fn.home_dir.html, https://docs.rs/home/latest/home/fn.home_dir.html
- Rust edition 2024 env::set_var safety: https://github.com/rust-lang/rust/issues/71684

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all deps verified via cargo metadata; zero new deps needed
- Architecture: HIGH -- patterns derived from existing production code + established Rust testing conventions
- Pitfalls: HIGH -- verified via codebase inspection and Rust documentation

**Research date:** 2026-04-24
**Valid until:** 2026-05-24 (stable domain; Rust testing patterns change slowly)
