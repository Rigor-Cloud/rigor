# Rigor

**Epistemic constraint enforcement for LLM outputs.**

Rigor sits between your AI coding agent and the LLM it calls. It extracts claims from every response, evaluates them against a constraint graph grounded in your project's facts, and blocks, warns, or rewrites outputs that violate what you've declared true. One daemon, any tool — Claude Code, OpenCode, anything that speaks Anthropic/OpenAI/Vertex/Azure/OpenRouter.

Rigor is not alignment. Alignment tries to change the model's weights; rigor runs outside the model and verifies what it says against evidence you control.

## What it does

```
┌─────────────┐    intercepts    ┌────────────────┐    proxies    ┌─────────────┐
│ Claude Code │─────── HTTPS ───▶│ rigor daemon   │─────── API ──▶│  Anthropic  │
│  OpenCode   │   (LD_PRELOAD +  │  (:8787 HTTP,  │               │   OpenAI    │
│  anything   │    TLS MITM)     │   :443 TLS)    │◀──── stream ──│   Vertex…   │
└─────────────┘                  └────────────────┘               └─────────────┘
                                          │
                                          ▼
                              ┌───────────────────────┐
                              │ rigor.yaml (beliefs,  │
                              │ justifications,       │
                              │ defeaters, Rego)      │
                              └───────────────────────┘
                                          │
                                          ▼
                              ┌───────────────────────┐
                              │ chunk-level eval →    │
                              │ block / warn / allow  │
                              │ + auto-retry on BLOCK │
                              └───────────────────────┘
```

Every response stream is evaluated at sentence boundaries. If an accumulated claim violates a constraint with strength ≥ 0.7, rigor drops the upstream connection (stops Anthropic from billing further tokens), injects a formatted error into the SSE stream, and auto-retries the request with violation feedback appended to the system prompt so the model self-corrects.

## Install

```bash
brew tap Rigor-Cloud/rigor && brew install rigor
```

Or build from source:

```bash
git clone https://github.com/waveywaves/rigor-opencode-hackathon
cd rigor-opencode-hackathon
cargo build --release
```

Requires a Unix-like OS. macOS (Apple Silicon + Intel) and Linux supported. Uses Rustls throughout — no OpenSSL dependency.

## Quickstart

```bash
# 1. Generate rigor.yaml for your project (reads language, dependencies,
#    drops a Claude Code skill you can invoke with /rigor:map)
rigor init

# 2. Install rigor's MITM CA into your system trust store
rigor trust

# 3. Run your AI agent through rigor
rigor ground -- claude       # or: rigor ground -- opencode
```

That's it. Your agent now makes its LLM calls through rigor's proxy. Every stream is inspected, every claim is scored, and a dashboard at `http://127.0.0.1:8787` shows violations, cost, and the live constraint graph in 3D.

Alternative: run the daemon once in the background (`rigor serve --background`) and point tools at it manually via `HTTPS_PROXY=http://127.0.0.1:8787` — for the Claude Code case, use `rigor trust claude` to install a wrapper shim that does this for you.

## How it's different

Most "AI guardrail" projects do one of three things: post-hoc output filtering (regex or a classifier), alignment training (changing the model), or a sandbox limited to one specific tool. Rigor does none of these.

- **Runtime, not training.** Rigor operates at the network layer. The model ships as-is; rigor observes every request and response and enforces constraints externally. Nothing to retrain, no weights to manage, no per-model integration.
- **Epistemic, not pattern-matching.** Rigor's model is beliefs, justifications, and defeaters — the argumentation structure of bipolar argumentation frameworks (Cayrol & Lagasquie-Schiex, 2005). Constraint strengths are computed with DF-QuAD (Rago et al., 2016) using product-of-complements aggregation, so multiple attackers compound correctly instead of averaging out. Severity is graduated (Block ≥ 0.7, Warn ≥ 0.4, Allow < 0.4) — not binary.
- **Grounded, not generic.** Constraints live in `rigor.yaml` per project, anchored to source files and updated with every `rigor map`. "No fabricated APIs" is fine in the abstract, but rigor enforces "no fabricated APIs *in **your** codebase*" by pinning constraints to grep-verifiable or LSP-verifiable code anchors.
- **Stream-aware, not post-hoc.** Bad output is caught at ~100 tokens into the response, not after 2000. The upstream connection is dropped. Auto-retry with violation feedback lets the model try again without the user ever seeing the original bad response.

## The constraint model

```yaml
constraints:
  beliefs:
    - id: no-fabricated-apis
      epistemic_type: belief
      name: "No Fabricated APIs"
      rego: |
        violation contains v if {
          some c in input.claims
          # … Rego rule …
        }
      message: "Claim references an API that doesn't exist in this project."

  justifications:
    - id: test-evidence
      epistemic_type: justification
      rego: |
        # …evidence rule…

  defeaters:
    - id: prototype-exception
      epistemic_type: defeater
      rego: |
        # …exception rule…

relations:
  - from: test-evidence
    to: no-fabricated-apis
    relation_type: supports
  - from: prototype-exception
    to: no-fabricated-apis
    relation_type: attacks
```

Rego bodies are evaluated by [regorus](https://github.com/microsoft/regorus), a Rust-native OPA implementation. Each `violation` emitted by a matching rule becomes a candidate. Rigor then runs DF-QuAD over the argumentation graph to compute each constraint's final strength, maps that to a severity, and produces a decision.

See `docs/constraint-authoring.md` for the full Rego pattern library, and `docs/epistemic-foundations.md` for the theoretical grounding.

## Architecture

- **`crates/rigor/`** — the `rigor` binary and library. Two entry points: a stop-hook subprocess (driven by Claude Code's JSON stdin/stdout contract, runs the full evaluation pipeline against a transcript), and a long-lived daemon (HTTP on `:8787` + TLS MITM on `:443`, proxies LLM traffic and serves the dashboard + websocket).
- **`layer/`** — an `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES` shared library using frida-gum to hook `getaddrinfo`, `connect`, `getpeername`, and (on macOS) `SecTrustEvaluateWithError`. This is what makes rigor universal: any process it wraps sees LLM hostnames resolve to `127.0.0.1`, with `getpeername` faking the original peer address so the client's TLS stack doesn't reject the session.
- **`viewer/`** — the 3D dashboard. Uses [3d-force-graph](https://github.com/vasturiano/3d-force-graph) (Three.js) for a Gource-style live visualization: claims fly in as bubbles, violations flash red, ALLOW decisions drift away green.
- **`policies/builtin/`** — reference Rego policies (calibrated confidence, no fabricated APIs, require justification).
- **`examples/`** — sample `rigor.yaml` files from basic to adversarial.
- **`docs/`** — configuration reference, constraint authoring guide, and the epistemic foundations write-up.

## CLI surface

Key commands (run `rigor <cmd> --help` for options):

| Command | What it does |
|---|---|
| `rigor init` | Generate `rigor.yaml` for the current project |
| `rigor validate` | Schema-check `rigor.yaml` and compile all Rego bodies |
| `rigor show` | Print all constraints with computed DF-QuAD strengths |
| `rigor graph --web` | Launch the 3D constraint graph explorer |
| `rigor serve --background` | Start the daemon as a background process |
| `rigor ground -- <cmd>` | Run `<cmd>` with the layer injected + daemon running |
| `rigor trust [tool]` | Install the CA into system trust (no arg) or create a wrapper shim for `opencode`/`claude` |
| `rigor scan --install` | Register `rigor` as a UserPromptSubmit PII/secrets hook in Claude Code |
| `rigor gate install` | Register action gates (Pre/PostToolUse) in Claude Code |
| `rigor setup` | Interactive onboarding wizard |
| `rigor eval` | Compute precision / recall for all constraints against the violation log |
| `rigor refine` | Suggest (or apply) refinements for high-false-positive constraints |
| `rigor logs` / `rigor sessions` / `rigor search` | Browse the violation log |

## Integrations that ship

- **Claude Code.** Stop hook (`lib.rs:run_hook`), UserPromptSubmit PII scan (`cli/scan.rs`), PreToolUse + PostToolUse action gates (`cli/gate.rs`), `/rigor:map` skill auto-installed by `rigor init`.
- **OpenCode.** Plugin at `.opencode/plugins/rigor.ts` installed by `rigor setup` — hooks `shell.env`, `session.created`, `session.idle` to register the project with the daemon and inject proxy env into subprocesses.
- **Anthropic, OpenAI, Google Vertex AI, Azure OpenAI, OpenCode Zen, OpenRouter.** MITM allowlist in `daemon/mod.rs:82`. Everything else blind-tunnels end-to-end so OAuth flows and telemetry aren't affected.

## Status

This repo is the **hackathon fork** of rigor, cut from v0.1.0 of the parent project (see `CHANGELOG.md`). Core shipped:

- DF-QuAD argumentation graph with product-of-complements aggregation and regression test guard
- Rego-based policy engine via regorus
- LLM-as-judge semantic evaluator (configurable model + provider)
- TLS MITM proxy with per-host certs signed by the rigor CA
- LD_PRELOAD/DYLD_INSERT_LIBRARIES interception with getpeername/getsockname fakery
- Chunk-level SSE evaluation with stream killswitch and 1-retry auto-correction
- Action gates (real-time + retroactive) with snapshot + revert
- PII / secrets scanning via `sanitize_pii` with provider-specific detectors
- 3D dashboard, websocket event feed, cost tracking per model
- Hookups for Claude Code and OpenCode

The hackathon shortlist (in `.planning/roadmap/ROADMAP.md`) covers hedge-softening rewrites in the egress chain, anti-enshittification policy packs, a one-shot `rigor protect` install, and a "blocked today" dashboard counter.

## Development

```bash
cargo build --release       # build everything
cargo test                  # unit + integration tests
cargo test --lib claim      # just the claim-extraction tests
cargo bench                 # criterion benchmarks for hook latency
```

Before touching `constraint/graph.rs`, run the regression test at `graph.rs:447` — it guards the product-of-complements formula against accidental reversion to mean aggregation.

See `.planning/codebase/` for the full architecture + conventions + concerns write-ups (generated by `/gsd:map-codebase`).

## License

Apache 2.0. See `LICENSE`.

## Credits & reading

- Cayrol & Lagasquie-Schiex (2005), *On the Acceptability of Arguments in Bipolar Argumentation Frameworks* — the bipolar argumentation foundation.
- Rago, Toni, Aurisicchio, Baroni (2016), *Discontinuity-Free Decision Support with Quantitative Argumentation Debates* — the DF-QuAD semantics rigor implements.
- Pollock (1987), *Defeasible Reasoning* — rebutting vs. undercutting defeaters.
- Aphyr (Kyle Kingsbury), *The future of everything is lies, I guess safety* — the framing that alignment is cosmetic and epistemic accountability is the alternative.
- Architecturally patterned on [mirrord](https://github.com/metalbear-co/mirrord): CLI → Layer → Agent, with DNS-level interception and frida-gum function hooking.
