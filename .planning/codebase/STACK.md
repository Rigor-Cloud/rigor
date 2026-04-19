# Technology Stack

**Analysis Date:** 2026-04-19

## Languages

**Primary:**
- Rust 2021 edition - Core epistemic constraint enforcement framework and daemon

**Secondary:**
- TypeScript/JavaScript - OpenCode plugin integration at `.opencode/`

## Runtime

**Environment:**
- POSIX/Unix-based systems (macOS, Linux)
- Platform-agnostic tooling with platform detection via libc

**Package Manager:**
- Cargo (Rust) - Primary build and dependency management
- Lockfile: `Cargo.lock` present and committed

## Frameworks

**Core:**
- Axum 0.8 - HTTP server and router for daemon API (`crates/rigor/Cargo.toml`)
- Tokio 1 - Async runtime for multi-threaded concurrent operations

**Testing:**
- Criterion 0.5 - Benchmark harness for performance testing (`crates/rigor/benches/`)
- Tokio test macros - Async unit and integration test support

**Build/Dev:**
- Cargo workspace - Multi-crate project structure across `crates/rigor`, `crates/rigor-test`, `crates/rigor-harness`

## Key Dependencies

**Critical:**

- Serde 1.0 (with derive) - Serialization/deserialization framework
- serde_json 1.0 - JSON parsing and generation
- serde_yml 0.0.12 - YAML parsing for rigor.yaml constraint configuration
- Regorus 0.2 - Rego policy language execution engine for constraint evaluation
- Clap 4.5 - Command-line argument parsing for CLI
- Anyhow 1.0 - Error handling and context propagation

**Infrastructure:**

- Reqwest 0.12 - HTTP client with TLS, gzip, brotli, and deflate compression support
- Axum-extra 0.10 - HTTP header handling and additional Axum utilities
- Hyper 1 - Low-level HTTP server primitives (HTTP/1 and HTTP/2)
- Hyper-util 0.1 - Utilities for Hyper server operations
- Tower 0.5 - Middleware and service utilities

**Cryptography & TLS:**

- Rustls 0.23 - Pure Rust TLS implementation (no OpenSSL dependency)
- Tokio-rustls 0.26 - Async TLS layer on top of Rustls
- Rcgen 0.13 - Certificate generation for CA-based MITM (pem and x509-parser features)
- SHA2 0.10 - Cryptographic hash for LSP protocol

**Observability & Logging:**

- Tracing 0.1 - Structured logging framework
- Tracing-subscriber 0.3 - Log formatting with JSON and env-filter support
- OpenTelemetry 0.27 - Distributed tracing SDK
- OpenTelemetry-OTLP 0.27 - OTLP export support
- OpenTelemetry-stdout 0.27 - Stdout trace exporter (fallback)
- Tracing-opentelemetry 0.28 - Bridge between tracing and OpenTelemetry

**Data Processing:**

- Serde-jsonlines 0.7 - JSONL parsing for episodic memory
- Unicode-segmentation 1.12 - Claim extraction with proper Unicode boundaries
- Regex 1.11 - Pattern matching for policy evaluation and claim extraction
- UUID 1.11 (with v4 and serde) - Session and violation identifiers

**Platform Integration:**

- Git2 0.19 - Repository metadata capture for claims
- Chrono 0.4 (with serde) - Timestamp generation for sessions
- Dirs 5.0 - Cross-platform home directory resolution
- Shellexpand 3.1 - Environment variable expansion in paths
- Libc 0.2 - POSIX system calls (process signaling, kill(pid, 0))

**UI/Daemon:**

- Rust-embed 8 - Static file embedding for web viewer assets
- Open 5 - Desktop browser opening for viewer
- Tokio-stream 0.1 - Async stream utilities for WebSocket events
- Futures-util 0.3 - Future combinators for async operations
- Bytes 1 - Efficient byte buffer handling
- Flate2 1 - Gzip compression
- Brotli 8 - Brotli compression

**Error Handling & Type Safety:**

- Thiserror 2 - Derive macros for custom error types
- Async-trait 0.1 - Async trait support

## Configuration

**Environment:**

- Rigor configuration stored in `~/.rigor/config` with key=value format
- Judge configuration via environment variables:
  - `RIGOR_JUDGE_API_KEY` - LLM judge API authentication
  - `RIGOR_JUDGE_API_URL` - LLM judge endpoint (default: https://openrouter.ai/api)
  - `RIGOR_JUDGE_MODEL` - Model for judge evaluation (default: anthropic/claude-sonnet-4-6)
- Daemon port configuration via `RIGOR_DAEMON_PORT` environment variable
- LLM proxy redirection via `ANTHROPIC_BASE_URL` and `OPENAI_BASE_URL`
- Target API override via `RIGOR_TARGET_API` (default: https://api.anthropic.com)

**Build:**

- Workspace configuration: `/Cargo.toml` defines shared dependencies and workspace settings
- Main crate configuration: `crates/rigor/Cargo.toml`
- Test orchestrator: `crates/rigor-test/Cargo.toml`
- Test harness library: `crates/rigor-harness/Cargo.toml`

## Platform Requirements

**Development:**

- Rust toolchain (stable 2021 edition)
- Cargo package manager
- Unix-like environment (macOS, Linux)
- OpenSSL or compatible TLS stack (code uses pure Rust via Rustls)

**Production:**

- Multi-threaded runtime environment for daemon operation
- Network access to LLM endpoints (Anthropic, OpenAI, Google Vertex AI, Azure OpenAI, OpenRouter)
- Filesystem access for configuration at `~/.rigor/`
- Port binding capability for daemon HTTP server (default: 8787)

## Notable Technical Choices

**Async Rust:** Project leverages Tokio's multi-threaded runtime for high-concurrency proxy operations and WebSocket streaming.

**Pure Rust TLS:** Uses Rustls (not OpenSSL) for MITM certificate generation and TLS termination in LD_PRELOAD mode.

**Fail-Open Pattern:** Critical paths (constraint evaluation, daemon connectivity) gracefully degrade rather than block operations.

**Rego-Based Policies:** Constraint evaluation uses Regorus (Rego policy language) executed through regex fallbacks.

---

*Stack analysis: 2026-04-19*
