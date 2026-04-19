# Technology Stack

**Analysis Date:** 2026-04-19

## Languages

**Primary:**
- Rust (edition 2021) — All binaries, libraries, and the LD_PRELOAD layer. Declared in `Cargo.toml` (workspace), `crates/rigor/Cargo.toml`, `crates/rigor-harness/Cargo.toml`, `crates/rigor-test/Cargo.toml`, `layer/Cargo.toml`.

**Secondary:**
- Rego (OPA policy language) — Embedded inline within `rigor.yaml` constraint definitions and standalone `.rego` files in `policies/builtin/` (e.g. `policies/builtin/calibrated-confidence.rego`, `policies/helpers.rego`). Evaluated by the `regorus` crate.
- YAML — Constraint configuration format (`rigor.yaml`, `examples/rigor.yaml`). Parsed via `serde_yml`.
- JavaScript (vendored only) — Dashboard/graph viewer assets in `viewer/` (`3d-force-graph.min.js`, `cytoscape.min.js`, `cytoscape-dagre.js`, `dagre.min.js`). Embedded into the binary via `rust-embed`.
- HTML/CSS — `viewer/index.html`, `viewer/style.css` served by the daemon.

## Runtime

**Environment:**
- Native Rust binary compiled via `cargo build --release`. Targets Linux and macOS (the LD_PRELOAD layer has `#[cfg(target_os = "macos")]` branches at `layer/src/lib.rs:144`).
- Async runtime: `tokio` 1.x (multi-threaded, `rt-multi-thread` + `macros` features) at `crates/rigor/Cargo.toml:27`.

**Package Manager:**
- Cargo (the Rust toolchain's package manager).
- Lockfile: present — `Cargo.lock` at repo root (102 KB) and `layer/Cargo.lock`.

## Frameworks

**Core:**
- `axum` 0.8 (with `ws` feature) — HTTP/WebSocket server for the daemon (`crates/rigor/src/daemon/mod.rs`). Built on `tower` + `hyper`.
- `hyper` 1 (with `server`, `http1`, `http2` features) — Low-level HTTP server used directly for TLS-terminated proxy connections (`crates/rigor/src/daemon/mod.rs:391`).
- `hyper-util` 0.1 — Tokio adapters for hyper.
- `tower` 0.5 — Middleware / service trait.
- `clap` 4.5 (derive feature) — CLI argument parsing (`crates/rigor/src/cli/mod.rs:16`).
- `regorus` 0.2 — Rust implementation of OPA/Rego (subset); constraint evaluation engine (`crates/rigor/src/policy/engine.rs`).

**Testing:**
- Built-in `cargo test` (standard Rust test harness) — tests in `crates/rigor/tests/*.rs`.
- `criterion` 0.5 (with `html_reports`) — benchmarks at `crates/rigor/benches/hook_latency.rs`, `crates/rigor/benches/evaluation_only.rs`.
- `tempfile` 3 — Scratch directories for integration tests.
- `rigor-harness` (workspace crate, `crates/rigor-harness/`) — test primitives (MockAgent, MockLLM, TestDaemon) for adapter authors.
- `rigor-test` (workspace crate, `crates/rigor-test/`) — dev-only E2E / benchmark orchestrator that emits HTML reports.

**Build/Dev:**
- `cargo fmt` + `cargo clippy` enforced in CI (`.github/workflows/ci.yml:40,55`).
- `frida-gum` 0.17 (with `auto-download`) — inline function hooking for the LD_PRELOAD layer (`layer/Cargo.toml:13`). Same library mirrord uses.

## Key Dependencies

**Critical:**
- `regorus` 0.2 — Rego policy evaluator. Subset of OPA; does not support `http.send`, `opa.runtime`, etc. Core of constraint evaluation.
- `reqwest` 0.12 — HTTP client for upstream LLM API calls and judge calls. Built with `rustls-tls`, `stream`, `json`, `gzip`, `brotli`, `deflate`, and `blocking` features (no default features). `crates/rigor/Cargo.toml:45`.
- `serde` 1.0 (derive) + `serde_json` 1.0 + `serde_yml` 0.0.12 — Serialization for transcripts, hook I/O, config.
- `tokio` 1 — Async runtime powering daemon, proxy, TLS listener.
- `anyhow` 1.0 — Error handling throughout the codebase.
- `clap` 4.5 — CLI surface (`rigor init`, `rigor ground`, `rigor daemon`, `rigor show`, `rigor validate`, `rigor graph`, `rigor log`, `rigor trust`, `rigor untrust`, `rigor config`, `rigor map`, `rigor gate`, `rigor scan`).

**Infrastructure:**
- `rustls` 0.23 (with `ring` feature) — TLS implementation for MITM listener.
- `tokio-rustls` 0.26 — Async TLS accept.
- `rcgen` 0.13 (with `pem`, `x509-parser`) — Generates the rigor CA and per-host MITM certificates (`crates/rigor/src/daemon/tls.rs`).
- `sha2` 0.10 — Hashing.
- `uuid` 1.11 (v4, serde) — Claim IDs, session IDs.
- `regex` 1.11 + `once_cell` 1.19 — Pattern detection (PII scanning, claim extraction).
- `unicode-segmentation` 1.12 — Sentence splitting for claim extraction.
- `serde-jsonlines` 0.7 — JSONL transcript + violation log I/O (`crates/rigor/src/logging/violation_log.rs`).
- `git2` 0.19 — Reads git HEAD / dirty state for session metadata (`crates/rigor/src/logging/session.rs:3`).
- `chrono` 0.4 (serde) — Timestamps in logs.
- `owo-colors` 4 — Terminal colouring for the violation formatter (`crates/rigor/src/violation/formatter.rs`).
- `lsp-types` 0.97 — Spawns and talks to `rust-analyzer`, `tsserver`, `pyright-langserver`, `gopls` for deep anchor verification (`crates/rigor/src/lsp/mod.rs`, `crates/rigor/src/lsp/client.rs`).
- `dirs` 5.0 + `shellexpand` 3.1 — Home directory / path expansion.
- `rust-embed` 8 (with `interpolate-folder-path`) — Embeds the `viewer/` assets into the binary (`crates/rigor/src/cli/web.rs:9`).
- `open` 5 — Launches the system browser for `rigor graph --web`.
- `axum-extra` 0.10 (`typed-header`), `bytes` 1, `http` 1, `flate2` 1, `brotli` 8, `tokio-stream` 0.1, `futures-util` 0.3 — HTTP plumbing and streaming body handling.
- `sanitize-pii` 0.1.1 — PII/secret detection (`rigor scan`).
- `libc` 0.2 — Direct `kill(pid, 0)` liveness check + fd duplication in `rigor ground` (`crates/rigor/src/cli/ground.rs:241`).
- `thiserror` 2, `async-trait` 0.1 — Error types and async traits in the egress filter chain (`crates/rigor/src/daemon/egress/chain.rs`).

**Observability:**
- `tracing` 0.1 + `tracing-subscriber` 0.3 (`json`, `env-filter`) — Structured logging to stderr and `~/.rigor/rigor.log`.
- `tracing-opentelemetry` 0.28 — Bridges tracing to OTEL.
- `opentelemetry` 0.27 + `opentelemetry_sdk` 0.27 (`rt-tokio`) + `opentelemetry-otlp` 0.27 + `opentelemetry-stdout` 0.27 — OTLP span export with graceful degradation when `OTEL_EXPORTER_OTLP_ENDPOINT` is unset (`crates/rigor/src/observability/tracing.rs:70`).

## Configuration

**Environment:**

Consumed by `std::env::var` across the codebase:

- `RIGOR_FAIL_CLOSED` — If `true`/`1`, hook returns exit code 2 on error instead of failing open (`crates/rigor/src/main.rs:8`).
- `RIGOR_DEBUG` — Enables debug-level tracing and claim visualization (`crates/rigor/src/lib.rs:169`, `crates/rigor/src/observability/tracing.rs:28`).
- `RIGOR_TEST_CLAIMS` — JSON-encoded claim list that overrides transcript extraction (for testing) (`crates/rigor/src/lib.rs:145`).
- `RIGOR_TARGET_API` — Upstream LLM base URL (default `https://api.anthropic.com`) (`crates/rigor/src/daemon/mod.rs:183`).
- `RIGOR_DAEMON_PORT` / `RIGOR_DAEMON_TLS_PORT` — Override HTTP / HTTPS listener ports (defaults: HTTP 8787, TLS 443).
- `RIGOR_TRANSPARENT` — mirrord-style transparent interception of all outbound :443 (`layer/src/lib.rs:81`, `crates/rigor/src/cli/ground.rs:200`).
- `RIGOR_INTERCEPT_HOSTS` — Comma-separated extra hosts for the layer to hook (`layer/src/lib.rs:114`).
- `RIGOR_LAYER_DEBUG` — Enables stderr logging from the LD_PRELOAD layer (`layer/src/lib.rs:75`).
- `RIGOR_SKIP_INTERNAL` — Bypass evaluation on rigor-originated internal calls (`crates/rigor/src/daemon/proxy.rs:568`).
- `RIGOR_NO_RETRY` — Disables upstream retry (`crates/rigor/src/daemon/proxy.rs:1445`).
- `RIGOR_GATE_ENABLED` — Opt-in per-session action gate (`crates/rigor/src/cli/gate.rs:529`).
- `RIGOR_JUDGE_API_URL` / `RIGOR_JUDGE_API_KEY` / `RIGOR_JUDGE_MODEL` — LLM-as-judge configuration; falls back to `~/.rigor/config` then hard-coded defaults (`crates/rigor/src/cli/config.rs:63`).
- `RIGOR_AI_COMMAND` — Custom AI command for `rigor init --ai` (`crates/rigor/src/cli/init.rs:404`).
- `ANTHROPIC_API_KEY` — Captured at daemon startup for daemon-originated Anthropic calls (`crates/rigor/src/daemon/mod.rs:186`).
- `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL` / `CLOUD_ML_API_ENDPOINT` — Set by `rigor ground` on the child process to redirect SDK traffic (`crates/rigor/src/cli/ground.rs:150`).
- `HTTPS_PROXY` / `HTTP_PROXY` / `NO_PROXY` (+ lowercase) — Set on the child when not in transparent mode (`crates/rigor/src/cli/ground.rs:203`).
- `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES` — Hook-library injection path set by `rigor ground` when the layer `.so`/`.dylib` is found.
- `NODE_TLS_REJECT_UNAUTHORIZED=0` — Set on the child to accept rigor's MITM cert (`crates/rigor/src/cli/ground.rs:171`).
- `CLAUDE_CODE_SESSION_ID` / `CLAUDE_SESSION_ID` — Read by the gate subsystem to identify the Claude Code session (`crates/rigor/src/cli/gate.rs:46`).
- `OTEL_EXPORTER_OTLP_ENDPOINT` — Enables OpenTelemetry span export (`crates/rigor/src/observability/tracing.rs:82`).
- `RUST_LOG` — Standard `tracing-subscriber` env filter.
- `HOME`, `SHELL`, `TERM`, `NO_COLOR` — Standard Unix env.

**Global config file:** `~/.rigor/config` — simple `key = value` text format. Supported keys: `judge.api_key`, `judge.api_url`, `judge.model` (`crates/rigor/src/cli/config.rs:11`).

**Project config file:** `rigor.yaml` at project root (discovered by walking up the directory tree). Schema: `constraints: { beliefs, justifications, defeaters }` + `relations` (`rigor.yaml`, `crates/rigor/src/constraint/loader.rs`).

**Runtime state directory:** `~/.rigor/` — stores `daemon.pid`, `rigor.log`, `config`, CA certs, violation logs.

**Build:**
- `Cargo.toml` (workspace root, 20 lines) + per-crate `Cargo.toml`.
- No `build.rs` files.
- Release target: `./target/release/rigor`.

## Platform Requirements

**Development:**
- Rust toolchain — CI uses `dtolnay/rust-toolchain@stable` (`.github/workflows/ci.yml:16`). No `rust-toolchain.toml` pin.
- Linux or macOS. The LD_PRELOAD layer has macOS-specific hooks (`connectx`, `SecTrustEvaluateWithError`, `dns_configuration_copy`) gated on `#[cfg(target_os = "macos")]`.
- For `rigor map --deep`: requires the appropriate language server on `PATH` (`rust-analyzer`, `typescript-language-server`, `pyright-langserver`, `gopls`).
- For `rigor trust` / `rigor untrust`: macOS `security` CLI (login keychain integration).

**Production:**
- Distributed as a single binary (the workspace publishes `rigor` with `publish = false`). Users install via `cargo build --release` per the README.
- Companion shared library `librigor_layer.{so,dylib}` optionally built from the `layer/` crate (cdylib) and discovered at runtime by `rigor ground` (`crates/rigor/src/cli/ground.rs:115`).
- Hook integration: a `command` entry under Claude Code's `Stop` hook that invokes the `rigor` binary (`examples/claude-hooks.json`).

---

*Stack analysis: 2026-04-19*
