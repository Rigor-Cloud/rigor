# Architecture

**Analysis Date:** 2026-04-19

## Pattern Overview

**Overall:** Cargo workspace with a layered pipeline architecture around a central constraint-evaluation engine, plus a long-running async daemon that MITM-proxies LLM API traffic. Two primary execution modes share the same evaluation core: a short-lived CLI/stop-hook process and a persistent HTTP/HTTPS/WebSocket daemon.

**Key Characteristics:**
- Multi-crate Cargo workspace (`rigor`, `rigor-harness`, `rigor-test`) with a separate out-of-workspace `layer` crate (LD_PRELOAD cdylib).
- Fail-open by default: every optional pipeline stage (config load, graph compute, policy eval, claim extraction, violation logging) degrades to "allow" on error. Controlled via `RIGOR_FAIL_CLOSED` env var.
- Pipeline-and-engine: CLI/hook entrypoint drives a fixed ordered pipeline (load config → build argumentation graph → compile policies → extract claims → evaluate → collect violations → decide → respond/log). The daemon reuses the same stages on proxied request/response bodies.
- Embedded Rego (OPA) policy engine via `regorus` for per-constraint evaluation; helpers and constraint snippets wrapped into generated policy modules at runtime.
- Frida-gum inline hooking in `layer/` to redirect LLM API traffic at the libc level (getaddrinfo/connect/gethostbyname/SecTrustEvaluateWithError) to the local daemon.
- Axum-based HTTP/WebSocket server in the daemon with TLS termination (rustls + rcgen CA) for MITM of configured LLM hosts.
- Filter-chain pattern for egress processing (see `crates/rigor/src/daemon/egress/chain.rs`): each filter can pass, transform, or block SSE chunks.

## Layers

**CLI / Entry Layer:**
- Purpose: Parse arguments; dispatch to subcommands or fall through to Claude Code stop-hook mode.
- Location: `crates/rigor/src/main.rs`, `crates/rigor/src/cli/mod.rs`, `crates/rigor/src/cli/*.rs`
- Contains: Clap command definitions (`init`, `show`, `validate`, `graph`, `ground`, `daemon`, `log`, `trust/untrust`, `config`, `map`, `gate`, `scan`), per-subcommand handlers, shared `find_rigor_yaml` helper.
- Depends on: `constraint`, `policy`, `daemon`, `claim`, `logging`, `lsp`, `defaults`.
- Used by: `main.rs` binary.

**Hook Layer (Stop-Hook Protocol):**
- Purpose: Implement the Claude Code Stop hook JSON-in/JSON-out contract over stdin/stdout.
- Location: `crates/rigor/src/hook/input.rs`, `crates/rigor/src/hook/output.rs`, orchestrated from `crates/rigor/src/lib.rs::run`.
- Contains: `StopHookInput`, `HookResponse`, `Metadata`. Read stdin → run pipeline → write JSON `{decision, reason, metadata}` to stdout.
- Depends on: `claim`, `constraint`, `policy`, `violation`, `logging`, `observability`, `daemon::daemon_alive`.
- Used by: `cli::run_cli` (when no subcommand given), `lib.rs::run`.

**Config Layer:**
- Purpose: Locate and load `rigor.yaml` (and legacy `rigor.lock`) by walking up the directory tree from CWD.
- Location: `crates/rigor/src/config/mod.rs`, `crates/rigor/src/config/lookup.rs`
- Contains: `find_rigor_yaml`, `find_rigor_yaml_from`, `find_rigor_lock`.
- Depends on: `std::env`, filesystem only.
- Used by: Every entrypoint (CLI subcommands, hook, daemon).

**Constraint Layer (Domain Core):**
- Purpose: Parse `rigor.yaml` into typed `RigorConfig`; validate structure; build and compute the DF-QuAD argumentation graph.
- Location: `crates/rigor/src/constraint/`
  - `types.rs`: `RigorConfig`, `Constraint`, `Relation`, `EpistemicType`, `RelationType`, `SourceAnchor`.
  - `loader.rs`: YAML → `RigorConfig`.
  - `validator.rs`: Structural/semantic checks on configs.
  - `graph.rs`: `ArgumentationGraph` with `compute_strengths()` (Rago et al. 2016 DF-QuAD fixed-point iteration, `MAX_ITERATIONS=100`, `EPSILON=0.001`).
- Depends on: `serde`, `serde_yml` (via loader).
- Used by: `policy`, `daemon`, CLI (`show`, `validate`, `graph`).

**Claim Layer:**
- Purpose: Parse assistant transcripts and extract structured `Claim`s for policy evaluation.
- Location: `crates/rigor/src/claim/`
  - `transcript.rs`: Parse JSONL transcripts; `TranscriptMessage`, `get_assistant_messages`.
  - `types.rs`: `Claim`, `ClaimType`, `SourceLocation`.
  - `extractor.rs`: `ClaimExtractor` trait + `HeuristicExtractor` (rule-based v1).
  - `heuristic.rs`: Sentence-level extraction with hedge detection.
  - `hedge_detector.rs`: Hedge-phrase pattern matching.
  - `confidence.rs`: Confidence scoring rules.
- Depends on: `unicode-segmentation`, `regex`, `once_cell`, `uuid`.
- Used by: `lib.rs::run_hook`, daemon proxy (claim-injection filter).

**Policy Layer:**
- Purpose: Compile constraint Rego snippets into a `regorus::Engine` and evaluate claims.
- Location: `crates/rigor/src/policy/`
  - `engine.rs`: `PolicyEngine` (wraps `regorus::Engine`; loads `policies/helpers.rego` + per-constraint generated modules `package rigor.constraint_<safe_id>`).
  - `input.rs`: `EvaluationInput { claims }`.
- Depends on: `regorus`, `serde`.
- Used by: `lib.rs::run_hook`, `daemon::DaemonState` (pre-compiled at startup).

**Violation Layer:**
- Purpose: Transform raw Rego results into typed, prioritized violations; decide block/warn/allow; format user-facing messages.
- Location: `crates/rigor/src/violation/`
  - `types.rs`: `Violation`, `Severity`, `SeverityThresholds` (block `>=0.7`, warn `>=0.4`).
  - `collector.rs`: `collect_violations`, `determine_decision`, `Decision`, `ConstraintMeta`.
  - `formatter.rs`: `ViolationFormatter` (terminal output with `owo-colors`).
- Depends on: `policy`, `claim`, `constraint`.
- Used by: `lib.rs::run_hook`, daemon egress filters.

**Daemon Layer:**
- Purpose: Long-running HTTP/HTTPS/WebSocket server that proxies LLM API calls, performs MITM on allowlisted hosts, runs the same claim→policy→violation pipeline on responses, and serves the dashboard.
- Location: `crates/rigor/src/daemon/`
  - `mod.rs`: `DaemonState`, `SharedState`, `start_daemon`, `build_router`, `daemon_alive` / PID file management (`~/.rigor/daemon.pid`), `MITM_HOSTS`, `should_mitm_target`, gate-related state types.
  - `proxy.rs` (3092 lines): Anthropic/OpenAI/Vertex/Azure-aware proxy handlers; catch-all proxy; SSE streaming; claim extraction; `/v1/messages` and `/v1/chat/completions` routes plus fallback.
  - `tls.rs`: `RigorCA` (persistent CA via rcgen), per-host cert signing, macOS keychain install/remove (`install_ca_trust` / `remove_ca_trust`), legacy multi-SAN self-signed fallback.
  - `sni.rs`: TLS ClientHello SNI peeking for transparent-mode routing.
  - `ws.rs`: WebSocket event broadcast channel (`EventSender`) + dashboard event protocol.
  - `gate.rs` / `gate_api.rs`: Action-gate state machine (real-time vs retroactive approval flows).
  - `governance.rs`: Dashboard REST endpoints (`/api/governance/*` for toggling constraints, pausing, block-next).
  - `chat.rs`: `/api/chat` — dashboard-initiated LLM calls routed through the proxy.
  - `context.rs`: Per-session/per-conversation context tracking.
  - `egress/`: Filter-chain architecture for SSE body processing.
    - `chain.rs`: `SseChunk`, `FilterError`, filter trait and chain runner.
    - `claim_injection.rs`: Filter that extracts/injects claims into the stream.
    - `ctx.rs`: `ConversationCtx` shared across filters.
- Depends on: `axum`, `hyper`, `rustls`, `tokio-rustls`, `rcgen`, `reqwest`, `tower`, `tokio`, plus all inner layers.
- Used by: `cli::ground` (spawns daemon + target process), `cli::daemon` (standalone), trust/untrust CLI commands.

**Fallback Layer:**
- Purpose: Policy-driven error handling for each pipeline component — decides retry/fail-open/fail-closed per `(component, FailureCategory)`.
- Location: `crates/rigor/src/fallback/`
  - `types.rs`: `Policy`, `PolicySet`, `ComponentPolicy`, `FallbackConfig`, `FallbackOutcome`, `FailureCategory`.
  - `config.rs`: YAML loading, policy resolution `policy_for(component, category)`.
  - `minimums.rs`: Minimum-policy guardrails validated at daemon startup.
  - `mod.rs`: `FallbackConfig::execute` — single async entrypoint that wraps any fallible operation in a retry/backoff loop governed by its policy.
- Depends on: `tokio`, `tracing`, `serde`.
- Used by: `daemon::DaemonState::load` (validated at startup), proxy filter chain.

**LSP Layer:**
- Purpose: Verify source-anchor constraints by querying a language server (rust-analyzer / tsserver / pyright / gopls).
- Location: `crates/rigor/src/lsp/mod.rs` (detection, grep-based fallback, anchor verification), `crates/rigor/src/lsp/client.rs` (JSON-RPC LSP client).
- Depends on: `lsp-types`, `sha2`, `std::process::Command`.
- Used by: `cli::map`.

**Logging Layer:**
- Purpose: Structured JSONL violation logging to `~/.rigor/violations.jsonl` with session metadata (git SHA, branch, CWD).
- Location: `crates/rigor/src/logging/`
  - `types.rs`: `ViolationLogEntry`, `SessionMetadata`, `ClaimSource`.
  - `session.rs`: Capture git + CWD metadata via `git2`.
  - `violation_log.rs`: `ViolationLogger` (append-only JSONL).
  - `query.rs`: Filter/query logged violations (used by `rigor log`).
  - `annotate.rs`: Mark violations as false-positive / add notes.
- Depends on: `git2`, `chrono`, `serde-jsonlines`.
- Used by: `lib.rs::run_hook`, `cli::log`.

**Observability Layer:**
- Purpose: Initialize `tracing` + OpenTelemetry OTLP export with graceful degradation.
- Location: `crates/rigor/src/observability/tracing.rs`
- Depends on: `tracing`, `tracing-subscriber`, `tracing-opentelemetry`, `opentelemetry*`.
- Used by: `lib.rs::run` (startup + shutdown), daemon.

**Defaults Layer:**
- Purpose: Language- and dependency-level built-in constraints shipped with the binary.
- Location: `crates/rigor/src/defaults/` (`rust.rs`, `go.rs`, `deps.rs`).
- Used by: `cli::init`.

**Hook Layer (Gate, separate from Stop hook):**
- Purpose: Pre-tool / post-tool Claude Code hooks for the action-gate feature.
- Location: `crates/rigor/src/cli/gate.rs` + `crates/rigor/src/daemon/gate.rs` + `crates/rigor/src/daemon/gate_api.rs`.

**Scan Layer:**
- Purpose: PII/secrets scanning for user prompts via `sanitize-pii`; installable as a Claude Code `UserPromptSubmit` hook.
- Location: `crates/rigor/src/cli/scan.rs`.

**Network Interception Layer (out-of-workspace crate):**
- Purpose: Dynamic-library (LD_PRELOAD / DYLD_INSERT_LIBRARIES) that redirects LLM API connections to the local daemon.
- Location: `layer/src/lib.rs` (957 lines; `crate-type = ["cdylib"]`).
- Pattern: Frida-gum inline hooks on `getaddrinfo`, `freeaddrinfo`, `gethostbyname`, `connect`, `connectx` (macOS), `getpeername`, `getsockname`, `SecTrustEvaluateWithError` (macOS TLS bypass), `dns_configuration_copy/free` (macOS DNS bypass prevention).
- Depends on: `frida-gum`, `libc`, `once_cell`.
- Used by: `cli::ground` (built separately, loaded via env vars).

**Test/Harness Crates:**
- `crates/rigor-harness/src/lib.rs`: Placeholder for future `MockAgent`, `MockLLM`, `TestDaemon`, `TestGitRepo`, `MockLSP`, `EventCapture` primitives (empty in current state).
- `crates/rigor-test/src/main.rs`: Dev-only test orchestrator CLI; `e2e`, `bench`, `report` subcommands stubbed (not yet implemented).

## Data Flow

**Stop-Hook Flow (`crates/rigor/src/lib.rs::run_hook`):**

1. `observability::init_tracing()` sets up tracing + optional OTLP.
2. Check `daemon_alive()` — if no daemon, drain stdin and emit `allow` (silent no-op).
3. `StopHookInput::from_stdin()` parses JSON hook input (`session_id`, `transcript_path`, `stop_hook_active`, etc.).
4. If `stop_hook_active == true` → emit `allow` (loop-prevention).
5. `find_rigor_yaml()` / `find_rigor_lock()` walk parents for config.
6. `SessionMetadata::capture()` records git SHA/branch/CWD.
7. `constraint::loader::load_rigor_config` → `RigorConfig`.
8. `ArgumentationGraph::from_config(&config).compute_strengths()` → per-constraint strengths (DF-QuAD fixed-point).
9. `PolicyEngine::new(&config)` loads `policies/helpers.rego` and wraps each constraint's Rego snippet in a generated module.
10. Claims: either `RIGOR_TEST_CLAIMS` env override or `HeuristicExtractor::extract(&transcript_messages)`.
11. `engine.evaluate(&EvaluationInput { claims })` → `Vec<RawViolation>`.
12. `collect_violations(raw, &strengths, &thresholds, &constraint_meta, &claims)` produces typed `Violation`s with severity.
13. `determine_decision(&violations)` → `Decision::{Block, Warn, Allow}`.
14. For each violation, `ViolationLogger::log()` appends to `~/.rigor/violations.jsonl`.
15. `ViolationFormatter::format_violations` builds reason string.
16. `HookResponse::{allow, block}` → JSON on stdout, status line on stderr.
17. `observability::shutdown()` flushes OTEL spans.

**Daemon Proxy Flow (`crates/rigor/src/daemon/proxy.rs` routes):**

1. Client (via `layer/` hooks or `HTTPS_PROXY`) sends request to `127.0.0.1:PORT` or `127.0.0.1:443`.
2. Axum router dispatches: known paths (`/v1/messages`, `/v1/chat/completions`) → typed proxy handlers; unknown paths → `catch_all_proxy`.
3. `should_mitm_target(host)` checks `MITM_HOSTS` allowlist. Non-allowlisted → blind tunnel (bytes passed through).
4. MITM path: decrypt with per-host cert signed by `RigorCA`; inspect request body; optionally inject epistemic context.
5. Forward upstream via shared `reqwest::Client` (pool size 4 per host).
6. For SSE streaming responses: feed chunks through `egress::chain` filter pipeline (e.g., `claim_injection`).
7. `claim_injection` filter extracts claims from streamed assistant text; passes them through the same `PolicyEngine` pre-compiled in `DaemonState`.
8. Violations broadcast to WebSocket subscribers via `EventSender` (dashboard updates live).
9. `DaemonState.block_next` / `disabled_constraints` / `proxy_paused` flags (toggled via `/api/governance/*`) influence behavior.

**Network Interception Flow (`layer/src/lib.rs`):**

1. Library loaded at process start via `DYLD_INSERT_LIBRARIES` (macOS) or `LD_PRELOAD` (Linux) by `cli::ground`.
2. `#[used] static INIT` constructor runs `install_hooks()`: Frida-gum inline patches libc exports.
3. `getaddrinfo_detour` intercepts DNS lookups for `INTERCEPT_HOSTS` (api.anthropic.com, api.openai.com, Vertex/Azure endpoints) → returns `127.0.0.1:DAEMON_PORT`.
4. `connect_detour` redirects `127.0.0.1:443` (or in `--transparent` mode, all `:443` connections) to `127.0.0.1:DAEMON_PORT`.
5. `getpeername_detour` returns the ORIGINAL destination so TLS libs don't detect the redirect.
6. On macOS, `SecTrustEvaluateWithError_detour` forces cert validation to succeed, making the daemon's MITM cert trusted without keychain changes.
7. Env vars `DYLD_INSERT_LIBRARIES` / `LD_PRELOAD` are cleared post-install so child processes don't inherit the library (hooks persist in-process via frida-gum patches).

**State Management:**
- Stop-hook: stateless (fresh process per Claude Code stop event).
- Daemon: `SharedState = Arc<Mutex<DaemonState>>` passed to all Axum handlers. Contains pre-compiled `PolicyEngine`, `ArgumentationGraph`, `RigorCA`, `reqwest::Client`, gate state maps, disabled-constraint set, fallback config.
- Persistent artifacts: `~/.rigor/daemon.pid`, `~/.rigor/violations.jsonl`, `~/.rigor/config` (judge API settings), CA cert/key (location managed by `tls::RigorCA`).

## Key Abstractions

**`RigorConfig` (domain root):**
- Purpose: Typed representation of `rigor.yaml`.
- Examples: `crates/rigor/src/constraint/types.rs`
- Pattern: Serde derive; `all_constraints()` flattens beliefs + justifications + defeaters.

**`Claim`:**
- Purpose: A single extracted assertion with confidence, type, and optional source location. Primary input to the Rego engine.
- Examples: `crates/rigor/src/claim/types.rs`
- Pattern: UUID-v4 IDs; tagged enum `ClaimType` (`assertion`, `negation`, `code_reference`, `architectural_decision`, `dependency_claim`, `action_intent`); serde rename-all snake_case.

**`ArgumentationGraph`:**
- Purpose: DF-QuAD semantics over constraint nodes related by `supports` / `attacks` / `undercuts`. Computes a strength ∈ [0,1] per constraint.
- Examples: `crates/rigor/src/constraint/graph.rs`
- Pattern: `BTreeMap<String, ConstraintNode>` for deterministic iteration; fixed-point iteration with epsilon convergence; base strength by epistemic type (belief 0.8, justification 0.9, defeater 0.7).

**`PolicyEngine`:**
- Purpose: Embedded OPA/Rego evaluator with a compiled set of per-constraint modules plus shared helpers.
- Examples: `crates/rigor/src/policy/engine.rs`
- Pattern: Wraps `regorus::Engine`; generates `package rigor.constraint_<safe_id>` per constraint; `helpers.rego` embedded via `include_str!`. Cloneable so each request gets its own mutable engine.

**`Violation` + `Decision`:**
- Purpose: Typed result of evaluation with severity and user-facing message; `Decision::{Allow, Warn, Block}` gates hook response.
- Examples: `crates/rigor/src/violation/types.rs`, `crates/rigor/src/violation/collector.rs`
- Pattern: Severity derived from strength thresholds (0.7 / 0.4). `Decision` is the highest-severity violation wins.

**`HookResponse`:**
- Purpose: Claude Code Stop-hook output schema.
- Examples: `crates/rigor/src/hook/output.rs`
- Pattern: `{decision?: "block", reason?, metadata: {version, constraint_count, claim_count, error?, error_message?}}`; `allow()` / `block()` / `error()` constructors.

**`DaemonState`:**
- Purpose: Shared mutable runtime state for the Axum server.
- Examples: `crates/rigor/src/daemon/mod.rs`
- Pattern: `Arc<Mutex<DaemonState>>`; pre-compiled `PolicyEngine`; `EventSender` for WebSocket broadcasting; governance toggles (`disabled_constraints`, `proxy_paused`, `block_next`); action-gate HashMaps.

**`FallbackConfig` + `FallbackOutcome`:**
- Purpose: Declarative error-policy for each pipeline component.
- Examples: `crates/rigor/src/fallback/types.rs`, `crates/rigor/src/fallback/mod.rs`
- Pattern: `config.execute("component", || async { op })` returns `Ok(T) | Skipped | Blocked(String)`; retries with backoff per policy.

**`SseChunk` + `FilterError` (egress filter trait):**
- Purpose: Composable pipeline for SSE body transformations.
- Examples: `crates/rigor/src/daemon/egress/chain.rs`
- Pattern: `thiserror`-derived errors (`Blocked { filter, reason }`, `Internal`); async filters with shared `ConversationCtx`.

**`SourceAnchor`:**
- Purpose: Ground a constraint in specific code locations (file, lines, text pattern).
- Examples: `crates/rigor/src/constraint/types.rs`
- Pattern: Anchor text preferred over line numbers (survives edits); verified via LSP or grep (`cli::map`).

## Entry Points

**`rigor` binary (stop hook, default):**
- Location: `crates/rigor/src/main.rs` → `cli::run_cli()` with no subcommand → `crate::run()` → `lib.rs::run_hook`.
- Triggers: Claude Code Stop event (piped JSON to stdin).
- Responsibilities: Full constraint-evaluation pipeline; JSON response on stdout.

**`rigor <subcommand>`:**
- Location: `crates/rigor/src/cli/mod.rs::run_cli`
- Triggers: Manual user invocation.
- Subcommands: `init`, `show`, `validate`, `graph [--web]`, `ground -- <cmd>`, `daemon`, `log <sub>`, `trust`, `untrust`, `config <action>`, `map [--deep] [--check]`, `gate <sub>`, `scan [--hook|--install|--uninstall|--status]`.

**`rigor daemon` / `rigor ground -- <cmd>`:**
- Location: `crates/rigor/src/daemon/mod.rs::start_daemon`
- Triggers: User wants a persistent proxy; `ground` additionally spawns the target command with LD_PRELOAD/DYLD_INSERT_LIBRARIES pointing at the built `layer` dylib.
- Responsibilities: HTTP listener (port 8787 default), HTTPS listener (port 443 default, `RIGOR_DAEMON_TLS_PORT` override), WebSocket, dashboard, proxy handlers, governance API.

**`rigor-layer` (cdylib):**
- Location: `layer/src/lib.rs`
- Triggers: Loaded into a target process via `DYLD_INSERT_LIBRARIES` / `LD_PRELOAD` (set by `cli::ground`).
- Responsibilities: libc-level redirection of LLM API connections to the daemon.

**`rigor-test` (dev-only):**
- Location: `crates/rigor-test/src/main.rs`
- Triggers: Manual; scaffolded for future E2E / benchmark orchestration.

## Error Handling

**Strategy:** Fail-open by default at every pipeline stage in the stop-hook path (`crates/rigor/src/lib.rs::evaluate_constraints`). Each fallible step emits `warn!` via `tracing` and returns an `allow` `HookResponse`. The top-level `main.rs` wraps `run_cli()`; on error, if `RIGOR_FAIL_CLOSED=true|1` it emits stderr + exit code 2 (Claude Code blocking), otherwise emits an `error()` hook response on stdout and exits 0.

**Patterns:**
- `anyhow::Result<T>` + `.context(...)` at all fallible boundaries.
- Per-component policy governance via `fallback::FallbackConfig::execute` — replaces ad-hoc retry/fail logic with declarative policies (`FailOpen`, `FailClosed`, `FailStartup`, `DegradeWithWarn`, `RetryThenFailOpen`, `RetryThenFailClosed`, `RetryThenDegrade`).
- `thiserror` for typed errors in the egress filter chain (`FilterError::{Blocked, Internal}`).
- Graceful degradation in daemon startup: missing TLS config → HTTPS disabled (not fatal); missing CA → fall back to legacy self-signed multi-SAN cert; `reqwest::Client::builder()` failures → fall back to `Client::new()`.
- `RIGOR_TEST_CLAIMS` env var bypasses transcript extraction for deterministic testing; `RIGOR_DEBUG` enables verbose stderr + raw-input logging.

## Cross-Cutting Concerns

**Logging:** `tracing` crate with `tracing-subscriber` for text/JSON output; `tracing-opentelemetry` exports to OTLP when `OTEL_*` env vars set. `info_span!("rigor_hook")` per hook invocation; `debug!` / `info!` / `warn!` / `error!` throughout the pipeline. See `crates/rigor/src/observability/tracing.rs`. A separate structured `ViolationLogger` writes JSONL audit records to `~/.rigor/violations.jsonl`.

**Validation:** `constraint::validator` enforces structural invariants on `RigorConfig`; `fallback::FallbackConfig::validate()` enforces minimum policies at daemon startup (aborts if violated). Serde deserialization provides schema-level validation.

**Authentication:** No built-in user auth — daemon binds `127.0.0.1` only. Upstream auth via forwarded `ANTHROPIC_API_KEY` / per-request headers. Judge API config (`~/.rigor/config` via `cli::config::judge_config`) stores `judge_api_url`, `judge_api_key`, `judge_model`. MITM cert trust managed via macOS keychain (`daemon::tls::install_ca_trust` / `remove_ca_trust`).

**Concurrency:** Short-lived CLI/hook process is single-threaded synchronous. Daemon uses `tokio` multi-thread runtime (`rt-multi-thread`); Axum handlers are async; shared state behind `Arc<Mutex<DaemonState>>`; `DETOUR_BYPASS` thread-local in `layer/` prevents re-entrant hook recursion.

**Embedding:** `policies/helpers.rego` is embedded at compile time via `include_str!` in `crates/rigor/src/policy/engine.rs`. Viewer assets (`viewer/*`) are embedded into the binary via `rust-embed` for the `rigor graph --web` and daemon dashboard routes.

---

*Architecture analysis: 2026-04-19*
