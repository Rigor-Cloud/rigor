# Using Rigor with OpenCode

Rigor can epistemically ground OpenCode sessions, enforcing constraints on LLM
responses in real-time.

## Quick Start

```bash
# Build rigor
cargo build --release

# Ground an OpenCode session with your constraints
./target/release/rigor ground -- opencode
```

This will:
1. Start the rigor daemon (HTTP proxy + dashboard)
2. Launch OpenCode with LLM traffic redirected through rigor
3. Open the rigor dashboard at `http://127.0.0.1:8787`

All LLM API calls from OpenCode (Anthropic, OpenAI, Vertex, etc.) flow through
rigor's proxy where constraints are evaluated.

## How It Works

When you run `rigor ground opencode`:

- **Proxy env vars** (`HTTPS_PROXY`, `ANTHROPIC_BASE_URL`, `OPENAI_BASE_URL`)
  redirect OpenCode's LLM traffic to rigor's local daemon
- **TLS bypass** (`NODE_TLS_REJECT_UNAUTHORIZED=0`) lets OpenCode accept
  rigor's MITM certificate
- **Session tracking** (`OPENCODE_SESSION_ID`) correlates events from this
  OpenCode session in the dashboard and logs
- **NO_PROXY** ensures OpenCode's internal TUI↔server communication isn't
  intercepted

## With MITM Inspection

By default, rigor uses blind-tunnel mode (no body inspection). To enable
constraint evaluation on streaming responses:

```bash
./target/release/rigor ground --mitm -- opencode
```

This inspects LLM response bodies, extracts claims, and can block/warn on
constraint violations.

## Observability

### Dashboard

The rigor dashboard at `http://127.0.0.1:8787` shows:
- Live request/response traffic
- Extracted claims and violations
- Constraint graph with strengths
- Token usage

### OpenTelemetry

Export spans to an OTLP collector:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
  ./target/release/rigor ground -- opencode
```

Spans include `rigor.grounded_client=opencode` as a resource attribute.

### Logs

All daemon activity is logged to `/tmp/rigor-ground.log`:

```bash
tail -f /tmp/rigor-ground.log
```

Violation records are appended to `~/.rigor/violations.jsonl`.

## Configuration

Create a `rigor.yaml` in your project root with your constraints:

```yaml
constraints:
  beliefs:
    - id: no-unsafe-unwrap
      description: "Never use .unwrap() on Result/Option in production code"
      rego: |
        violation[msg] {
          some claim in input.claims
          contains(claim.text, "unwrap()")
          msg := sprintf("Claim references unwrap(): %s", [claim.text])
        }
```

See `examples/rigor.yaml` for a complete example.

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `OPENCODE_SESSION_ID` | Auto-set by rigor; correlates gate events |
| `RIGOR_DAEMON_PORT` | Override daemon HTTP port (default: 8787) |
| `RIGOR_DAEMON_TLS_PORT` | Override TLS port (default: 443) |
| `RIGOR_DEBUG` | Enable verbose logging |
| `RIGOR_FAIL_CLOSED` | Block on error instead of fail-open |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Enable OTLP span export |

## Comparison with Claude Code Integration

| Feature | Claude Code | OpenCode |
|---------|-------------|----------|
| Traffic interception | `rigor ground claude` | `rigor ground opencode` |
| Stop hook | JSON stdin/stdout | Not yet (plugin planned) |
| Session tracking | `CLAUDE_CODE_SESSION_ID` | `OPENCODE_SESSION_ID` |
| LD_PRELOAD | Yes (Bun/libc) | Yes (Bun/libc) |
| HTTP Proxy | Yes | Yes |
| Dashboard | Yes | Yes |
| OTEL | Yes | Yes |
