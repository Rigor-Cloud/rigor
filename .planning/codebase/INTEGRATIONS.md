# External Integrations

**Analysis Date:** 2026-04-19

## APIs & External Services

**LLM Endpoints (Proxied via Daemon):**
- Anthropic Claude - `/v1/messages` endpoint
  - SDK/Client: reqwest 0.12
  - Auth: `ANTHROPIC_API_KEY` environment variable
  - Base URL override: `ANTHROPIC_BASE_URL`
  - Default: https://api.anthropic.com

- OpenAI - `/v1/chat/completions` endpoint
  - SDK/Client: reqwest 0.12
  - Base URL override: `OPENAI_BASE_URL`
  - Default: https://api.openai.com

- Google Vertex AI - Multiple regional endpoints
  - Supported endpoints: us-east5, us-central1, us-west1, europe-west1, europe-west4, asia-southeast1
  - SDK/Client: reqwest 0.12
  - Hostname pattern: `*-aiplatform.googleapis.com`

- Azure OpenAI
  - Hostname: openai.azure.com
  - SDK/Client: reqwest 0.12

- OpenCode (Zen provider)
  - Endpoints: `/zen/v1/messages` and `/zen/v1/responses`
  - Hostname: opencode.ai, api.opencode.ai
  - SDK/Client: reqwest 0.12

**LLM-as-Judge Service:**
- OpenRouter (default judge provider)
  - Default endpoint: https://openrouter.ai/api
  - Configurable via `RIGOR_JUDGE_API_URL` environment variable
  - Auth: `RIGOR_JUDGE_API_KEY` configuration or environment variable
  - Model: Default `anthropic/claude-sonnet-4-6` (configurable via `RIGOR_JUDGE_MODEL`)
  - Config file: `~/.rigor/config`
  - Uses Reqwest client with timeout support

## Data Storage

**Databases:**
- No persistent database required
- Session metadata captured in-memory during constraint evaluation
- Cost tracking maintained in daemon state: cumulative tokens and USD costs per-model

**File Storage:**
- Local filesystem only
- Configuration stored in: `~/.rigor/config` (key=value format)
- Daemon PID file: `~/.rigor/daemon.pid` (for liveness detection)
- YAML constraint configuration: `rigor.yaml` in project root
- Legacy lock file: `rigor.lock` (deprecated, fallback support)
- Episodic memory: In-memory episodic cache during session
- WebSocket events streamed to connected clients

**Caching:**
- No external caching service
- In-memory caching via Arc<Mutex<>> for constraint strengths and decision caches
- Per-request episodic memory: `crates/rigor/src/memory/episodic.rs`
- Active stream tracking: HashSet<String> of session IDs in daemon state

## Authentication & Identity

**Auth Provider:**
- Custom ephemeral auth
- Stop hook reads from stdin (Claude Code provides credentials in request payload)
- Daemon checks PID file for process liveness before accepting requests
- No user login required — based on local process identity

**Credentials Flow:**
- LLM API keys: Captured from Claude Code's HTTP headers and forwarded to upstream
- Judge API key: Loaded from `~/.rigor/config` or `RIGOR_JUDGE_API_KEY` env var
- Sensitive data detection via `sanitize-pii` crate to redact database URLs and credentials

## Monitoring & Observability

**Error Tracking:**
- Structured logging via Tracing crate with JSON formatting
- OpenTelemetry integration for distributed tracing (graceful degradation if not configured)
- Trace export: OTLP (configurable) or stdout fallback
- Session tracking: Each session gets unique ID for violation correlation

**Logs:**
- JSON structured logs via `tracing-subscriber` with `json` feature
- Log filtering via `env-filter` (controlled by `RUST_LOG` env var)
- Fallback to stdout when OpenTelemetry export fails
- Git2 integration: Repository metadata for context in logs

**Metrics:**
- Cost tracking: Cumulative input/output token counts per model
- Cost calculation: USD estimation with per-model breakdown in `cost_by_model`
- Session metadata: Captured at start of constraint evaluation

## CI/CD & Deployment

**Hosting:**
- Daemon serves on HTTP (port 8787 by default, configurable)
- LD_PRELOAD mode: Daemon runs as background process with PID file tracking
- TLS termination: Runs Tokio-rustls server with CA-based certificate generation

**CI Pipeline:**
- GitHub Actions workflows in `.github/workflows/`
- CI jobs: Rust test, Clippy linting, Rustfmt check
- Release workflow: Binary compilation and publishing

## Environment Configuration

**Required env vars:**

For daemon operation:
- `RIGOR_TARGET_API` - Target LLM API base URL (default: https://api.anthropic.com)
- `ANTHROPIC_API_KEY` - API key for Anthropic Claude (optional if provided in request headers)
- `RIGOR_DAEMON_PORT` - Port for daemon to listen on (default: 8787)

For LLM-as-judge:
- `RIGOR_JUDGE_API_KEY` - OpenRouter API key (or read from `~/.rigor/config`)
- `RIGOR_JUDGE_API_URL` - Judge endpoint (default: https://openrouter.ai/api)
- `RIGOR_JUDGE_MODEL` - Model for judge (default: anthropic/claude-sonnet-4-6)

For proxy/routing:
- `ANTHROPIC_BASE_URL` - Override Anthropic endpoint for testing/proxy
- `OPENAI_BASE_URL` - Override OpenAI endpoint for testing/proxy

For observability:
- `RUST_LOG` - Log level filtering (e.g., `RUST_LOG=rigor=debug`)

**Secrets location:**
- `~/.rigor/config` - Local configuration file (not committed)
- Environment variables (ephemeral, per-process)
- Request headers from Claude Code (not persisted)

## Webhooks & Callbacks

**Incoming:**

The daemon exposes REST API endpoints for control and observability:

- `POST /v1/messages` - Anthropic proxy (request interception)
- `POST /v1/chat/completions` - OpenAI proxy (request interception)
- `POST /zen/v1/messages` - OpenCode Zen messages proxy
- `POST /zen/v1/responses` - OpenCode Zen responses proxy
- `POST /api/governance/constraints` - List constraints (GET)
- `POST /api/governance/constraints/{id}/toggle` - Toggle constraint enforcement
- `POST /api/governance/pause` - Pause proxy evaluation
- `POST /api/governance/block-next` - Force-block next response
- `POST /api/gate/register-snapshot` - Register file snapshot for tool gate
- `POST /api/gate/tool-completed` - Signal tool completion
- `GET /api/gate/decision/{session_id}` - Get gate decision
- `POST /api/gate/{gate_id}/approve` - Approve action gate
- `POST /api/gate/{gate_id}/reject` - Reject action gate
- `POST /api/chat` - Internal chat endpoint for viewer
- `GET /api/sessions` - List sessions and violations
- `GET /api/violations` - Search violations with filters
- `GET /api/eval` - Evaluator statistics
- `GET /api/cost` - Cost tracking statistics
- `POST /api/project/register` - Register project path for dashboard
- `POST /api/relevance/lookup` - LLM-as-judge verdict lookup
- `GET /ws` - WebSocket upgrade for event streaming
- `GET /health` - Health check endpoint
- `GET /` - Viewer UI index
- `GET /assets/{*path}` - Static viewer assets

**Outgoing:**

The daemon makes HTTP requests to:

- LLM endpoints (Anthropic, OpenAI, Vertex AI, Azure, OpenCode, OpenRouter)
  - Proxy pass-through requests with constraint injection
  - Request/response streaming support
  
- OpenRouter for LLM-as-judge evaluation
  - API endpoint: `RIGOR_JUDGE_API_URL`
  - Uses standard OpenRouter API format
  - Authenticated with `RIGOR_JUDGE_API_KEY`

**WebSocket Streaming:**
- Real-time event streaming to connected dashboard clients
- Event types: constraint violations, cost updates, gate decisions, session events
- Implemented in `crates/rigor/src/daemon/ws.rs`

## TLS/Certificate Management

**MITM Mode:**
- CA-based certificate generation via Rcgen (pem + x509-parser)
- Per-host certificates signed by rigor CA
- CA certificate stored in `~/.rigor/` (auto-generated on first run)
- Supports modern TLS 1.3 via Rustls

**Hosts intercepted for inspection:**
- api.anthropic.com
- api.openai.com
- Vertex AI endpoints (us-east5, us-central1, us-west1, europe-west1, europe-west4, asia-southeast1, generic)
- openai.azure.com
- opencode.ai, api.opencode.ai
- openrouter.ai

**Blind tunneling (end-to-end TLS preserved):**
- All other hosts
- OAuth, CDN, telemetry endpoints unaffected
- Default behavior: preserve original TLS

---

*Integration audit: 2026-04-19*
