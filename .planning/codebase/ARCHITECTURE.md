# Architecture

**Analysis Date:** 2026-04-19

## Pattern Overview

**Overall:** Epistemic Constraint Enforcement Engine with Pluggable Evaluator Pipeline

**Key Characteristics:**
- Fail-open defensive design (all errors gracefully degrade to allow)
- Pluggable claim evaluator pipeline with fallback to Rego-based policy engine
- Argumentation graph with DF-QuAD fixed-point iteration for constraint strength computation
- Dual-mode operation: stop-hook subprocess (constraint evaluation) and long-lived daemon (proxy + knowledge graph)
- TLS MITM + transparent proxy for LLM API interception
- LD_PRELOAD layer for DNS/socket-level interception (mirrord-style architecture)

## Layers

**Configuration & Constraint Loading:**
- Purpose: Parse and validate rigor.yaml, load epistemic constraints
- Location: `crates/rigor/src/constraint/loader.rs`, `crates/rigor/src/constraint/types.rs`, `crates/rigor/src/config/`
- Contains: Configuration parsing (YAML), constraint validation, constraint type definitions
- Depends on: serde_yml, anyhow
- Used by: Main hook evaluation pipeline, daemon, CLI validation commands

**Claim Extraction:**
- Purpose: Extract factual claims from LLM chat transcripts
- Location: `crates/rigor/src/claim/`
- Contains: Heuristic extractor, confidence scoring, hedge detection, claim types
- Files: `claim/extractor.rs`, `claim/heuristic.rs`, `claim/confidence.rs`, `claim/transcript.rs`
- Depends on: regex, unicode-segmentation
- Used by: Main hook evaluation pipeline

**Constraint Graph & Strength Computation:**
- Purpose: Build argumentation graph and compute constraint strengths via DF-QuAD fixed-point iteration
- Location: `crates/rigor/src/constraint/graph.rs`
- Contains: ConstraintNode, ArgumentationGraph with supports/attacks/undercuts relations
- Depends on: BTreeMap for deterministic iteration
- Algorithm: DF-QuAD (Dung's Framework with Quantified Aggregation) per Rago et al. 2016
  - Product aggregation: agg(M) = ∏(1 - sᵢ)
  - Influence function: two-case based on attacker/supporter dominance
  - Convergence: up to 100 iterations or EPSILON=0.001 change threshold
- Used by: Main hook evaluation, decision logic

**Evaluator Pipeline:**
- Purpose: Route claim/constraint pairs to appropriate evaluators (pluggable)
- Location: `crates/rigor/src/evaluator/pipeline.rs`
- Contains: ClaimEvaluator trait, EvalResult, EvaluatorPipeline router, RegexEvaluator, SemanticEvaluator
- Pattern: First-match routing — pipeline asks each evaluator `can_evaluate()` in registration order
- Fallback: RegexEvaluator (wraps PolicyEngine with Rego) always matches
- Used by: Main hook evaluation pipeline in `lib.rs:run_hook()`

**Policy Engine (Rego-based):**
- Purpose: Evaluate claims against Rego constraints
- Location: `crates/rigor/src/policy/engine.rs`
- Contains: PolicyEngine, EvaluationInput, RawViolation
- Rego Runtime: regorus crate (open-source Rego interpreter)
- Used by: RegexEvaluator, fallback for unhandled claim/constraint pairs

**Semantic Evaluator (LLM-as-judge):**
- Purpose: Use LLM judgments to determine claim relevance to constraints
- Location: `crates/rigor/src/evaluator/relevance.rs`
- Contains: SemanticEvaluator, RelevanceLookup trait, HttpLookup (HTTP client to daemon)
- Verdicts: high/medium/low cached in daemon's knowledge base
- Depends on: reqwest, tokio for async HTTP
- Used by: Plugged into evaluator pipeline if HTTP lookup succeeds

**Violation Collection & Decision:**
- Purpose: Aggregate violations, apply severity thresholds, determine action
- Location: `crates/rigor/src/violation/`
- Contains: Violation types, Severity (Block/Warn/Allow), SeverityThresholds, ViolationFormatter
- Thresholds: block >= 0.7 strength, warn >= 0.4 strength (tunable)
- Used by: Main hook evaluation to produce final HookResponse (allow/warn/block)

**Violation Logging:**
- Purpose: Persist violation telemetry for dashboards and offline analysis
- Location: `crates/rigor/src/logging/`
- Contains: ViolationLogger, SessionMetadata, ViolationLogEntry, ClaimSource
- Logging target: Configurable via OTEL (stdout, OTLP collector)
- Fail-open: Logging failures never block constraint evaluation
- Used by: Main hook evaluation pipeline (post-decision)

**Observability & Tracing:**
- Purpose: OpenTelemetry integration for structured logging and distributed tracing
- Location: `crates/rigor/src/observability/`
- Contains: init_tracing(), shutdown(), OTEL provider setup
- Features: JSON logging, environment-based filtering, graceful degradation
- Used by: Top-level run() in lib.rs, daemon, CLI tools

**Hook I/O Interface:**
- Purpose: StdinStdout contract between Claude Code (caller) and rigor stop-hook
- Location: `crates/rigor/src/hook/`
- Input: `StopHookInput` (JSON from stdin) — session_id, transcript_path, cwd, hook_event_name, stop_hook_active flag
- Output: `HookResponse` (JSON to stdout) — decision (allow/block/warn), reason, metadata
- Fail-open: Always writes valid JSON response, even on fatal errors
- Used by: Main hook entry point in `lib.rs:run()`

**Daemon & Proxy:**
- Purpose: Long-lived TLS MITM proxy intercepting LLM API calls
- Location: `crates/rigor/src/daemon/`
- Contains: proxy.rs (TLS intercept), gate.rs/gate_api.rs (request filtering), chat.rs (message handling), observability_api.rs (telemetry)
- MITM targets: api.anthropic.com, api.openai.com, Google Vertex, Azure OpenAI, OpenRouter, OpenCode
- Blind tunneling: Non-MITM hosts get transparent pass-through
- SNI routing: Uses TLS SNI to route to correct upstream host
- TLS generation: rcgen for on-the-fly certificate generation (mirrord-style)
- Used by: `cli/serve.rs` (daemon mode), `cli/ground.rs` (child-wrapping mode)

**LD_PRELOAD Layer:**
- Purpose: Intercept DNS and socket calls to redirect LLM API traffic
- Location: `layer/src/lib.rs`
- Contains: frida-gum hooks for getaddrinfo, gethostbyname, connect, SecTrustEvaluateWithError
- ReEntrancy protection: DetourGuard to prevent infinite loops when hooked functions call other hooks
- Transparent mode: All port 443 connections redirected (for Bun/Go clients that bypass DNS hooks)
- Used by: `cli/ground` mode (loaded via LD_PRELOAD environment variable)

**CLI Interface:**
- Purpose: User-facing commands for constraint management, daemon control, evaluation
- Location: `crates/rigor/src/cli/`
- Entry point: `cli/mod.rs` with clap-based argument parser
- Subcommands: init, show, validate, graph, ground, serve, eval, gate, logs, alert, refine, etc.
- Used by: main.rs, orchestrated by users or CI/CD systems

**Test Harness:**
- Purpose: Test primitives for adapter authors (future-facing)
- Location: `crates/rigor-harness/src/lib.rs`
- Role: Publishable as dev-dependency for authors writing custom evaluators
- Status: Minimal implementation (Plan A.2/A.3)

**Test Orchestrator:**
- Purpose: Dev-only test runner (Layer 3 E2E, Layer 4 benchmarks)
- Location: `crates/rigor-test/src/main.rs`
- Status: Stub (implemented in later phases D.3)

## Data Flow

**Hook Evaluation Flow (Primary):**

1. Stop-hook process spawned by Claude Code with `RIGOR_HOOK_INPUT` on stdin
2. `main.rs` → `lib.rs:run()` initializes tracing
3. `run_hook()` reads `StopHookInput` from stdin
4. Check: daemon_alive() (kill(pid, 0) against ~/.rigor/daemon.pid)
   - If daemon missing: write allow() response, return
5. Load rigor.yaml via `constraint::loader::load_rigor_config()`
   - Fail-open on parse error
6. Build `ArgumentationGraph` from config, compute strengths via DF-QuAD
   - Fail-open on compute error
7. Build `EvaluatorPipeline` with fallback RegexEvaluator
   - Register SemanticEvaluator if HttpLookup succeeds (daemon HTTP endpoint reachable)
8. Extract claims via `HeuristicExtractor` from transcript
   - Alternative: `RIGOR_TEST_CLAIMS` env var override for testing
   - Fail-open: empty claims list if parsing fails
9. Pipeline runs: for each claim, iterate constraints, route to first evaluator that `can_evaluate()`
   - SemanticEvaluator: high/medium cache hit → violation
   - RegexEvaluator: evaluate Rego, return EvalResult
   - Each evaluator returns EvalResult → collapsed into RawViolation
10. Collect violations with severity thresholds (block >= 0.7, warn >= 0.4)
11. Compute decision (Block/Warn/Allow) based on highest violation severity
12. Log violations to ViolationLogger (fail-open on log errors)
13. Format violations into human-readable message
14. Write `HookResponse` to stdout (allow/block/warn)
15. Process exits code 0 (even on errors, unless RIGOR_FAIL_CLOSED=true)

**Daemon Mode (Persistent):**

1. `cli/serve.rs:run_serve()` starts long-lived daemon
2. Writes PID to ~/.rigor/daemon.pid
3. Builds `DaemonState` with loaded RigorConfig, PolicyEngine, ArgumentationGraph
4. Starts Axum router with endpoints:
   - `/api/relevance/lookup` — fetch cached semantic verdicts
   - `/api/observability/*` — telemetry ingestion
   - `/api/gate/*` — request filtering
   - TLS proxy listener on port 8787 (or custom RIGOR_DAEMON_TLS_PORT)
5. Intercepts HTTPS traffic via TLS MITM
6. Extracts claims from LLM messages (request/response bodies)
7. Scores relevance using semantic evaluator (if configured)
8. Caches verdicts in episodic memory for hook subprocess HTTP lookup
9. On SIGTERM/SIGINT: update session registry with ended_at, clean up ~/.rigor/daemon.pid

**Ground Mode (Child-wrapping):**

1. `cli/ground.rs` wraps subprocess command
2. Loads LD_PRELOAD layer (rigor-layer shared library)
3. Sets HTTPS_PROXY=http://127.0.0.1:8787 to redirect traffic
4. Spawns daemon in-process or background
5. Launches child command (e.g., `claude code --some-project`)
6. Waits for child exit, kills daemon, cleans up

**State Management:**

- **Constraint Strengths:** Computed once per hook invocation, cached in ArgumentationGraph
- **Claim Verdicts (Semantic):** Cached in daemon's episodic memory, queried by hook via HTTP
- **Session Metadata:** Captured at hook start (SessionMetadata with git info, env), logged with violations
- **PID Files:** Hook checks daemon alive via ~/.rigor/daemon.pid + kill(pid, 0)

## Key Abstractions

**Constraint:**
- Purpose: Epistemic rule to enforce
- Examples: `crates/rigor/src/constraint/types.rs:Constraint`
- Fields: id, epistemic_type (belief/justification/defeater), name, description, rego, message, tags, source anchors
- Pattern: Constraints are data-driven via rigor.yaml, not hardcoded

**Claim:**
- Purpose: Factual statement extracted from LLM transcript
- Examples: `crates/rigor/src/claim/types.rs:Claim`
- Fields: id, text, confidence (0.0-1.0), claim_type (assertion/question/mixed), source (message/sentence indices)
- Pattern: Claims are probabilistic (confidence score), sourced to chat messages

**EvalResult:**
- Purpose: Verdict of single evaluator for claim/constraint pair
- Fields: violated (bool), confidence (f64), reason (string)
- Pattern: Fine-grained per-pair, not aggregated (aggregation happens post-pipeline)

**Violation:**
- Purpose: Aggregated constraint breach
- Fields: constraint_id, claim_ids (can be multiple), strength (post-DF-QuAD), severity (Block/Warn/Allow), message
- Pattern: Severity maps from computed strength via thresholds

**Decision:**
- Purpose: Final action (Block/Warn/Allow)
- Pattern: Determined from highest violation severity or lack thereof

**ClaimEvaluator trait:**
- Purpose: Pluggable strategy for evaluating claim/constraint pairs
- Methods: name(), can_evaluate(), evaluate()
- Implementations: RegexEvaluator (Rego-based), SemanticEvaluator (LLM-as-judge)
- Pattern: Enables future evaluators (ML, symbolic, etc.)

**RelevanceLookup trait:**
- Purpose: Fetch cached semantic verdicts
- Implementations: HttpLookup (queries daemon /api/relevance/lookup), future in-process variant
- Pattern: Abstracts away HTTP from SemanticEvaluator

**ArgumentationGraph:**
- Purpose: Epistemic argumentation framework
- Relations: Supports, Attacks, Undercuts
- Algorithm: DF-QuAD for computing final strengths
- Pattern: Enables modeling defeasible reasoning (beliefs can be attacked by defeaters)

**HookResponse:**
- Purpose: JSON contract between hook subprocess and Claude Code
- Variants: allow(), block(reason), warn(reason), error(message)
- Pattern: Fail-open — always valid JSON, never crashes caller

## Entry Points

**Stop-hook subprocess:**
- Location: `crates/rigor/src/main.rs` → `lib.rs:run()`
- Triggers: Invoked by Claude Code stop-hook on every generation
- Responsibilities: Read stdin, evaluate constraints, write stdout decision
- Exit code: 0 (normal) or 2 (RIGOR_FAIL_CLOSED=true fatal error)

**CLI binary:**
- Location: `crates/rigor/src/main.rs`
- Triggers: User or CI runs `rigor <subcommand>`
- Responsibilities: Route to subcommand handler in `cli/mod.rs`
- Examples: `rigor init`, `rigor serve`, `rigor ground <cmd>`, `rigor eval <file>`

**Daemon background process:**
- Location: `cli/serve.rs:run_serve()`
- Triggers: `rigor serve --background` or `rigor ground <cmd>` starts it
- Responsibilities: Run TLS proxy, serve HTTP APIs, cache verdicts
- Lifecycle: PID in ~/.rigor/daemon.pid, killed via SIGTERM or `rigor serve stop`

**LD_PRELOAD layer:**
- Location: `layer/src/lib.rs`
- Triggers: Loaded via LD_PRELOAD env var by `cli/ground` mode
- Responsibilities: Hook DNS/socket calls, redirect to daemon
- Pattern: Transparent to wrapped process

## Error Handling

**Strategy:** Fail-open by default (allow when uncertain), fail-closed opt-in

**Patterns:**
- Configuration load errors: write allow(), log warning
- DF-QuAD compute errors: write allow(), log warning
- Evaluator pipeline build errors: write allow(), log warning
- Claim extraction errors: empty claims (no violations), log warning
- Violation logging errors: continue without logging, log warning
- HTTP lookup errors: skip SemanticEvaluator, use RegexEvaluator fallback
- Hook JSON parsing errors: write error() response with message

**Override:** `RIGOR_FAIL_CLOSED=true` env var forces exit code 2 on any error (hard block)

## Cross-Cutting Concerns

**Logging:** 
- Framework: tracing crate with OpenTelemetry integration
- Initialization: `observability::init_tracing()` called once at startup
- Shutdown: `observability::shutdown()` flushes OTEL span buffer
- Format: JSON structured logs (when OTEL enabled), configurable via `RUST_LOG` env var
- Violation-specific: `logging::ViolationLogger` records detailed violation telemetry

**Validation:**
- Config validation: `constraint::validator.rs` (not yet fully implemented in repo)
- YAML schema validation: Schema compliance checked on load
- Constraint source anchors: Durable text patterns for code-anchored constraints

**Authentication:**
- Stop-hook: Inherits Claude Code's API keys (passed through environment)
- Daemon: Proxies requests transparently (never stores API keys in memory)
- TLS certificates: Generated on-the-fly per upstream host (never stored)

---

*Architecture analysis: 2026-04-19*
