# External Integrations

**Analysis Date:** 2026-04-19

## APIs & External Services

**LLM Provider APIs (upstream targets of the rigor proxy/MITM):**

- **Anthropic Messages API** — `https://api.anthropic.com/v1/messages`. Default upstream set at `crates/rigor/src/daemon/mod.rs:184`. Proxy handler: `proxy::anthropic_proxy` routed at `crates/rigor/src/daemon/mod.rs:431`. Auth: `x-api-key` header for `sk-ant-api*` keys, `Authorization: Bearer` for `sk-ant-oat*` OAuth tokens — dispatched by `apply_provider_auth` at `crates/rigor/src/daemon/proxy.rs:37`.
- **OpenAI Chat Completions API** — `https://api.openai.com/v1/chat/completions`. Handler: `proxy::openai_proxy` at `crates/rigor/src/daemon/mod.rs:432`. Auth: `Authorization: Bearer` for `sk-proj-*` / `sk-*` keys.
- **Google Vertex AI (aiplatform)** — Multi-region endpoints: `us-east5-aiplatform.googleapis.com`, `us-central1-aiplatform.googleapis.com`, `us-west1-aiplatform.googleapis.com`, `europe-west1-aiplatform.googleapis.com`, `europe-west4-aiplatform.googleapis.com`, `asia-southeast1-aiplatform.googleapis.com`, `aiplatform.googleapis.com`. Listed in `MITM_HOSTS` at `crates/rigor/src/daemon/mod.rs:82`.
- **Azure OpenAI** — `openai.azure.com` (`crates/rigor/src/daemon/mod.rs:91`).
- **Ollama** — `localhost` intercept (opt-in via `RIGOR_INTERCEPT_HOSTS`) at `layer/src/lib.rs:108`.
- **OpenRouter** — `https://openrouter.ai/api`. Default LLM-as-judge endpoint used to score claim calibration (`crates/rigor/src/cli/config.rs:71`). Default judge model: `anthropic/claude-sonnet-4-6`. Auth: `Authorization: Bearer` (OpenRouter uses `sk-or-*` key format, detected at `crates/rigor/src/daemon/proxy.rs:2868`).

**Claude Code (host integration):**
- Not an HTTP API — Rigor integrates as a `Stop` hook (`examples/claude-hooks.json`), a `UserPromptSubmit` hook (`rigor scan --install`), and as a sub-process launched by `rigor ground claude …`. Reads the Claude Code session transcript JSONL via the path supplied on stdin (`StopHookInput.transcript_path`, `crates/rigor/src/hook/input.rs`).
- Session IDs read from `CLAUDE_CODE_SESSION_ID` / `CLAUDE_SESSION_ID` (`crates/rigor/src/cli/gate.rs:46`).

**Traffic interception architecture:**

Three interception modes (`crates/rigor/src/cli/ground.rs:107`):

1. **LD_PRELOAD / DYLD_INSERT_LIBRARIES** — The `layer/` crate (cdylib `librigor_layer.{so,dylib}`) hooks `getaddrinfo`, `freeaddrinfo`, `gethostbyname`, `connect`, `connectx` (macOS), `SecTrustEvaluateWithError` (macOS), `dns_configuration_copy` (macOS) using `frida-gum`. Redirects DNS of LLM hosts (`INTERCEPT_HOSTS` at `layer/src/lib.rs:91`) to `127.0.0.1:<DAEMON_PORT>`.
2. **HTTP proxy env vars** — Sets `HTTPS_PROXY`/`HTTP_PROXY` + SDK base-URL overrides (`ANTHROPIC_BASE_URL`, `OPENAI_BASE_URL`, `CLOUD_ML_API_ENDPOINT`) on the child (`crates/rigor/src/cli/ground.rs:150`).
3. **Transparent mode** (`--transparent` / `RIGOR_TRANSPARENT=1`) — Layer's `connect()` hook redirects ALL outbound :443 to the daemon's TLS port; daemon peeks the TLS ClientHello SNI (`crates/rigor/src/daemon/sni.rs`) to decide MITM vs blind tunnel.

TLS MITM is performed via a rigor-generated CA (`RigorCA::load_or_generate`, `crates/rigor/src/daemon/tls.rs`) that signs per-host certs on demand. `rigor trust` / `rigor untrust` install/remove the CA in the macOS login keychain via the `security` CLI.

## Data Storage

**Databases:**
- None. Rigor does not use any database (no SQL, no KV store, no ORM).

**File Storage:**
- Local filesystem only.
- `~/.rigor/` — daemon PID (`daemon.pid`), structured logs (`rigor.log`), global config (`config`), CA keypair/cert, violation log JSONL. Created by `crates/rigor/src/daemon/mod.rs:32` and `crates/rigor/src/observability/tracing.rs:13`.
- `/tmp/rigor-ground.log` — redirected daemon stderr during `rigor ground` runs (`crates/rigor/src/cli/ground.rs:231`).
- `./rigor.yaml` — project-level constraint config (located by walking up the directory tree, `crates/rigor/src/config/lookup.rs`).
- `./rigor.lock` — legacy config format, still detected but optional (`crates/rigor/src/lib.rs:78`).
- Transcripts are READ from paths supplied by Claude Code in the Stop hook payload; rigor does not own transcript storage.

**Caching:**
- In-memory only.
- Pre-compiled `PolicyEngine` held on `DaemonState` and cloned per request to avoid re-parsing Rego (`crates/rigor/src/daemon/mod.rs:189`).
- `reqwest::Client` with `pool_max_idle_per_host(4)` shared across requests for upstream connection pooling (`crates/rigor/src/daemon/mod.rs:219`).
- `once_cell::Lazy` used for hot paths (intercept host set, debug flags, daemon port) at `layer/src/lib.rs:68-127`.

## Authentication & Identity

**Auth Provider:**
- None. Rigor has no user accounts, sessions, or auth of its own.
- Rigor is itself a bump-in-the-wire for *other* services' auth:
  - Preserves upstream provider auth by forwarding `x-api-key` / `Authorization: Bearer` as dispatched by `apply_provider_auth` (`crates/rigor/src/daemon/proxy.rs:37`).
  - Defaults to blind-tunnel mode so OAuth flows (Claude Max/Pro, Anthropic OAuth tokens `sk-ant-oat*`) keep end-to-end TLS and don't break cert pinning. MITM is opt-in via `rigor ground --mitm` (`crates/rigor/src/cli/ground.rs:277`).
- Trust establishment for local MITM: `rigor trust` installs the rigor CA in the macOS login keychain (`crates/rigor/src/daemon/tls.rs:install_ca_trust`).

## Monitoring & Observability

**Error Tracking:**
- No hosted error tracker (no Sentry, Bugsnag, etc.).
- Error handling is "fail-open" by default — errors are logged via `tracing::warn!` and the hook still returns allow (`crates/rigor/src/lib.rs:113,127,138,190`). `RIGOR_FAIL_CLOSED=1` flips to exit code 2 (`crates/rigor/src/main.rs:8`).

**Logs:**
- `tracing` 0.1 + `tracing-subscriber` 0.3 with a multi-writer layer (stderr + `~/.rigor/rigor.log`). Configured in `crates/rigor/src/observability/tracing.rs:25`.
- Log levels: default `rigor=info`; `rigor=debug` when `RIGOR_DEBUG` is set. `RUST_LOG` env filter also respected.
- Violation logs are written separately as JSONL at `~/.rigor/violations.jsonl` via `ViolationLogger` (`crates/rigor/src/logging/violation_log.rs`). Queryable via `rigor log` subcommands (`crates/rigor/src/cli/log.rs`).
- Session metadata (git HEAD, dirty state, branch) captured by `SessionMetadata::capture` using `git2` (`crates/rigor/src/logging/session.rs:3`).

**OpenTelemetry:**
- OTLP span export over gRPC (tonic) when `OTEL_EXPORTER_OTLP_ENDPOINT` is set. Configured in `setup_otel_layer` at `crates/rigor/src/observability/tracing.rs:70`. Resource attributes: `service.name=rigor`, `service.version=<CARGO_PKG_VERSION>`. Gracefully degrades to stderr-only tracing when the endpoint is not set or exporter build fails.

## CI/CD & Deployment

**Hosting:**
- None (not a hosted service). Rigor is a local CLI + daemon installed per-developer.

**CI Pipeline:**
- GitHub Actions — `.github/workflows/ci.yml`. Runs on every `push` and `pull_request`.
- Jobs: `test` (`cargo test --all-features`), `clippy` (`cargo clippy --all-targets --all-features -- -D warnings`), `rustfmt` (`cargo fmt -- --check`), `rigor-validate` (builds release and runs `./target/release/rigor validate rigor.yaml` — rigor self-validating against its own constraint config).
- Runner: `ubuntu-latest`.
- Actions used: `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`.

## Environment Configuration

**Required env vars:**
- None are strictly required for the hook (it silently no-ops when `~/.rigor/daemon.pid` is missing, `crates/rigor/src/lib.rs:51`).
- For the daemon/proxy path: `ANTHROPIC_API_KEY` (or equivalent), captured at daemon startup (`crates/rigor/src/daemon/mod.rs:186`).
- For LLM-as-judge: `RIGOR_JUDGE_API_KEY` or `judge.api_key` in `~/.rigor/config` (`crates/rigor/src/cli/config.rs:67`).

**Optional / tuning env vars** — see STACK.md for the full list (`RIGOR_*`, `ANTHROPIC_BASE_URL`, `OPENAI_BASE_URL`, `HTTPS_PROXY`, `OTEL_EXPORTER_OTLP_ENDPOINT`, etc.).

**Secrets location:**
- `~/.rigor/config` — `judge.api_key` stored plaintext. Masked on display by `mask_key` (`crates/rigor/src/cli/config.rs:120`).
- `ANTHROPIC_API_KEY` read from process environment; not persisted by rigor.
- No `.env` file support in the codebase.
- `rigor scan` detects leaked keys/PII in stdin, files, or prompt-submit payloads (`crates/rigor/src/cli/scan.rs`) using the `sanitize-pii` crate.

## Webhooks & Callbacks

**Incoming:**

The daemon's axum router (`crates/rigor/src/daemon/mod.rs:424`) exposes:

- `POST /v1/messages` — Anthropic proxy passthrough (handler: `proxy::anthropic_proxy`).
- `POST /v1/chat/completions` — OpenAI proxy passthrough (handler: `proxy::openai_proxy`).
- `GET  /api/governance/constraints` — list constraints with current toggle state.
- `POST /api/governance/constraints/{id}/toggle` — enable/disable a constraint from the dashboard.
- `POST /api/governance/pause` — flip proxy pass-through.
- `POST /api/governance/block-next` — force-block the next response (testing).
- `POST /api/gate/register-snapshot` — gate: snapshot affected paths before a tool runs.
- `POST /api/gate/tool-completed` — gate: mark a tool invocation complete.
- `GET  /api/gate/decision/{session_id}` — gate: poll for approve/reject.
- `POST /api/gate/{gate_id}/approve` — human approves a held action.
- `POST /api/gate/{gate_id}/reject` — human rejects a held action.
- `POST /api/chat` — dashboard-originated chat routed through rigor's proxy.
- `GET  /ws` — WebSocket stream of `DaemonEvent`s to the dashboard (`crates/rigor/src/daemon/ws.rs`).
- `GET  /health` — returns `"ok"`.
- `GET  /` — serves the embedded dashboard (`viewer/index.html`).
- `GET  /graph.json` — constraint graph data for the 3D viewer.
- `GET  /assets/*path` — embedded viewer assets.
- Catch-all fallback — `proxy::catch_all_proxy` handles any other path, used for LD_PRELOAD-intercepted traffic to Vertex / Azure / other LLM endpoints (`crates/rigor/src/daemon/mod.rs:448`).

Listeners:
- HTTP on `127.0.0.1:8787` (default; configurable via `--port` / `RIGOR_DAEMON_PORT`).
- HTTPS on `127.0.0.1:443` (default; configurable via `RIGOR_DAEMON_TLS_PORT`) with rustls + a multi-SAN self-signed cert or the rigor CA's per-host certs.

**Outgoing:**
- Upstream LLM provider calls (Anthropic, OpenAI, Vertex, Azure) — forwarded by the proxy handlers using the shared `reqwest::Client`.
- LLM-as-judge calls to OpenRouter (or a configured alternative) for claim calibration (`crates/rigor/src/daemon/proxy.rs:2485`).
- Language-server subprocesses (`rust-analyzer`, `typescript-language-server`, `pyright-langserver`, `gopls`) spawned via `Command::new` and spoken to over JSON-RPC 2.0 on stdin/stdout (`crates/rigor/src/lsp/client.rs:57`).
- `grep` subprocess for fast-path anchor reference scanning (`crates/rigor/src/lsp/mod.rs:199`).
- `security` CLI subprocess on macOS for keychain trust (`crates/rigor/src/daemon/tls.rs`).
- `open` crate — launches the system browser for `rigor graph --web` (`crates/rigor/src/cli/web.rs`).
- OTLP/gRPC span export to `OTEL_EXPORTER_OTLP_ENDPOINT` (optional, `crates/rigor/src/observability/tracing.rs:93`).

---

*Integration audit: 2026-04-19*
