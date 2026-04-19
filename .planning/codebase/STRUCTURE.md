# Codebase Structure

**Analysis Date:** 2026-04-19

## Directory Layout

```
rigor-opencode-hackathon/
├── crates/                          # Rust workspace with multiple crates
│   ├── rigor/                       # Main Rigor library and binary
│   │   ├── src/
│   │   │   ├── main.rs              # Entry point (CLI and hook binary)
│   │   │   ├── lib.rs               # Hook evaluation pipeline
│   │   │   ├── alerting/            # Alert generation
│   │   │   ├── claim/               # Claim extraction and types
│   │   │   ├── cli/                 # CLI subcommand handlers
│   │   │   ├── config/              # Configuration lookup
│   │   │   ├── constraint/          # Constraint types, loading, graph
│   │   │   ├── cost.rs              # Token cost utilities
│   │   │   ├── daemon/              # TLS proxy and HTTP APIs
│   │   │   ├── defaults/            # Language-specific defaults
│   │   │   ├── evaluator/           # Pluggable evaluator pipeline
│   │   │   ├── fallback/            # Fallback strategies
│   │   │   ├── hook/                # StdinStdout I/O interface
│   │   │   ├── logging/             # Violation telemetry
│   │   │   ├── lsp/                 # LSP client
│   │   │   ├── memory/              # Episodic memory for cache
│   │   │   ├── observability/       # OpenTelemetry integration
│   │   │   ├── policy/              # Rego-based policy engine
│   │   │   └── violation/           # Violation types and decisions
│   │   ├── benches/                 # Criterion benchmarks
│   │   │   ├── hook_latency.rs      # Stop-hook latency benchmark
│   │   │   └── evaluation_only.rs   # Evaluator pipeline benchmark
│   │   ├── tests/                   # Integration tests
│   │   │   ├── true_e2e.rs
│   │   │   ├── claim_extraction_e2e.rs
│   │   │   ├── integration_constraint.rs
│   │   │   ├── integration_hook.rs
│   │   │   ├── egress_integration.rs
│   │   │   ├── fallback_integration.rs
│   │   │   └── dogfooding.rs        # Self-testing constraints
│   │   ├── Cargo.toml               # Package manifest
│   │   └── benches/ + tests/        # Test fixtures
│   │
│   ├── rigor-harness/               # Test harness library (future-facing)
│   │   ├── src/lib.rs               # Mock types for adapter authors
│   │   └── Cargo.toml
│   │
│   └── rigor-test/                  # Dev-only test orchestrator
│       ├── src/main.rs              # Test runner (E2E, bench, report)
│       └── Cargo.toml
│
├── layer/                           # LD_PRELOAD shared library
│   ├── src/lib.rs                   # frida-gum hooks for DNS/socket interception
│   └── Cargo.toml
│
├── viewer/                          # Web UI for constraint graph visualization
│   ├── index.html                   # Interactive 3D graph (via 3d-force-graph.js)
│   ├── style.css                    # Visualization styling
│   └── *.js libs                    # Cytoscape, Dagre, 3D Force Graph
│
├── examples/                        # Example configurations and use cases
│   ├── basic/                       # Inline Rego constraints
│   ├── beliefs-focused/             # Belief-heavy examples
│   ├── defeaters-focused/           # Defeater-heavy examples
│   └── rigor.yaml                   # Common example config
│
├── policies/                        # Built-in policy templates
│   └── builtin/                     # Reference policies
│
├── docs/                            # Documentation
│
├── scripts/                         # Utility scripts
│
├── .claude/                         # Claude Code configuration
│   └── commands/                    # Custom Claude Code commands
│
├── .opencode/                       # OpenCode configuration
│
├── .planning/                       # Planning documents
│   ├── codebase/                    # Auto-generated architecture docs
│   └── roadmap/                     # Feature roadmap
│
├── .github/                         # GitHub Actions
│   └── workflows/                   # CI/CD pipelines
│
├── Cargo.toml                       # Workspace manifest
├── Cargo.lock                       # Workspace lock file
├── LICENSE                          # Apache 2.0
├── CHANGELOG.md                     # Release notes
├── README.md                        # Project overview
├── rigor.yaml                       # Self-apply dogfooding config
└── rigor.ai-generated.yaml          # AI-refined config
```

## Directory Purposes

**crates/rigor/src/:**
- Purpose: Core Rigor implementation
- Contains: Library code + CLI entry point
- Key files: `lib.rs` (hook pipeline), `main.rs` (CLI/binary entry)

**crates/rigor/src/claim/:**
- Purpose: Claim extraction from LLM transcripts
- Contains: Heuristic extractor, confidence scoring, hedge detection
- Key files: 
  - `extractor.rs` — ClaimExtractor trait, HeuristicExtractor implementation
  - `heuristic.rs` — Rule-based claim detection
  - `transcript.rs` — JSON-L transcript parsing
  - `types.rs` — Claim struct definition
  - `confidence.rs` — Confidence scoring algorithms
  - `hedge_detector.rs` — Linguistic uncertainty detection

**crates/rigor/src/constraint/:**
- Purpose: Constraint definitions and graph computation
- Contains: Types, loading, validation, argumentation graph
- Key files:
  - `types.rs` — Constraint, Relation, EpistemicType, SourceAnchor definitions
  - `loader.rs` — Load rigor.yaml, parse Rego, populate config
  - `graph.rs` — ArgumentationGraph with DF-QuAD strength computation
  - `validator.rs` — Constraint validation (stub)

**crates/rigor/src/evaluator/:**
- Purpose: Pluggable evaluator pipeline and implementations
- Contains: ClaimEvaluator trait, EvaluatorPipeline router, RegexEvaluator, SemanticEvaluator
- Key files:
  - `pipeline.rs` — EvaluatorPipeline, ClaimEvaluator trait, routing logic
  - `mod.rs` — RegexEvaluator (wraps PolicyEngine)
  - `relevance.rs` — SemanticEvaluator (LLM-as-judge), RelevanceLookup trait, HttpLookup

**crates/rigor/src/policy/:**
- Purpose: Rego-based policy evaluation
- Contains: PolicyEngine, regorus integration
- Key files:
  - `engine.rs` — PolicyEngine wrapping regorus for Rego evaluation
  - `input.rs` — EvaluationInput (claims for Rego)

**crates/rigor/src/violation/:**
- Purpose: Violation aggregation, severity, decision logic
- Contains: Violation types, SeverityThresholds, ViolationFormatter, Decision logic
- Key files:
  - `types.rs` — Violation, Severity, SeverityThresholds
  - `collector.rs` — collect_violations(), determine_decision()
  - `formatter.rs` — ViolationFormatter for human-readable output

**crates/rigor/src/logging/:**
- Purpose: Violation telemetry and session tracking
- Contains: ViolationLogger, SessionMetadata, session registry
- Key files:
  - `violation_log.rs` — ViolationLogger, ViolationLogEntry
  - `session.rs` — SessionMetadata (git commit, user, env)
  - `session_registry.rs` — Session lifecycle tracking
  - `types.rs` — Type definitions
  - `query.rs` — Log querying (not yet implemented)
  - `annotate.rs` — Log annotation (not yet implemented)

**crates/rigor/src/daemon/:**
- Purpose: TLS MITM proxy and HTTP APIs
- Contains: Request/response interception, semantic caching, governance
- Key files:
  - `mod.rs` — Daemon setup, MITM host list, PID file management
  - `proxy.rs` — TLS MITM implementation, certificate generation
  - `sni.rs` — SNI parsing and routing
  - `tls.rs` — TLS handshake interception
  - `gate.rs` / `gate_api.rs` — Request filtering and decision logic
  - `chat.rs` — LLM message processing
  - `context.rs` — DaemonState, request context
  - `governance.rs` — Policy enforcement
  - `observability_api.rs` — Telemetry endpoint
  - `ws.rs` — WebSocket for real-time updates
  - `egress/` — Outbound request handling

**crates/rigor/src/cli/:**
- Purpose: User-facing commands
- Contains: Subcommand handlers (20+ commands)
- Key files:
  - `mod.rs` — Clap CLI parser, command enum
  - `init.rs` — Initialize rigor.yaml (language detection + AI)
  - `show.rs` — Display constraints with strengths
  - `validate.rs` — Validate rigor.yaml
  - `graph.rs` — Output constraint graph (DOT or 3D web viewer)
  - `ground.rs` — Wrap subprocess with LD_PRELOAD + daemon
  - `serve.rs` — Start long-lived daemon
  - `eval.rs` — Evaluate transcript against config
  - `gate.rs` — Pre/post-tool enforcement
  - `alert.rs` — Alert management
  - `logs.rs` — Violation log viewer
  - `refine.rs` — AI-assisted constraint refinement
  - `search.rs` — Search constraints
  - `map.rs` — Semantic mapping
  - `scan.rs` — Project scanning
  - `sessions.rs` — Session management
  - `setup.rs` — First-time setup
  - `trust.rs` — Trust mode for Claude Code
  - `web.rs` — Web UI launcher
  - `diff.rs` — Config diffing
  - `config.rs` — Config inspection
  - `log.rs` — Logging utilities (not subcommand)

**crates/rigor/src/hook/:**
- Purpose: StdinStdout interface contract
- Contains: Input/output serialization
- Key files:
  - `input.rs` — StopHookInput (from Claude Code)
  - `output.rs` — HookResponse (to Claude Code)

**crates/rigor/src/observability/:**
- Purpose: OpenTelemetry integration
- Contains: Tracing initialization, OTEL provider setup
- Key files:
  - `mod.rs` — init_tracing(), shutdown()
  - `tracing.rs` — OTEL configuration

**crates/rigor/src/memory/:**
- Purpose: In-process caching of semantic verdicts
- Contains: Episodic memory store
- Key files:
  - `episodic.rs` — VerdictsCache for semantic evaluator

**crates/rigor/src/defaults/:**
- Purpose: Language-specific default constraints
- Contains: Pre-built constraint templates
- Key files:
  - `mod.rs` — Registry of language defaults
  - `rust.rs` — Rust-specific constraints
  - `go.rs` — Go-specific constraints
  - `deps.rs` — Dependency constraints

**crates/rigor/src/fallback/:**
- Purpose: Graceful degradation when features unavailable
- Contains: Fallback evaluators, minimum strength thresholds
- Key files:
  - `mod.rs` — FallbackConfig
  - `config.rs` — Load fallback settings
  - `types.rs` — Fallback types
  - `minimums.rs` — Threshold minimums

**crates/rigor/src/config/:**
- Purpose: Configuration file discovery and parsing
- Contains: Lookup helpers
- Key files:
  - `mod.rs` — Config parsing
  - `lookup.rs` — find_rigor_yaml(), find_rigor_lock()

**crates/rigor/src/alerting/:**
- Purpose: Alert generation (future)
- Contains: Alert types
- Key files: `mod.rs`

**crates/rigor/src/lsp/:**
- Purpose: LSP client for code-anchored verification (future)
- Contains: LSP client
- Key files:
  - `mod.rs` — LSP setup
  - `client.rs` — LSP client implementation

**crates/rigor/src/cost.rs:**
- Purpose: Token cost estimation
- Contains: Cost calculation utilities

**crates/rigor/benches/:**
- Purpose: Performance benchmarks
- Contains: Criterion benchmarks
- Key files:
  - `hook_latency.rs` — Stop-hook execution time
  - `evaluation_only.rs` — Evaluator pipeline throughput

**crates/rigor/tests/:**
- Purpose: Integration tests
- Contains: End-to-end test scenarios
- Key files:
  - `true_e2e.rs` — Full pipeline E2E
  - `claim_extraction_e2e.rs` — Claim extraction testing
  - `integration_constraint.rs` — Constraint evaluation
  - `integration_hook.rs` — Hook I/O testing
  - `egress_integration.rs` — Daemon egress testing
  - `fallback_integration.rs` — Fallback behavior
  - `dogfooding.rs` — Self-testing (apply Rigor to Rigor)

**crates/rigor-harness/:**
- Purpose: Test primitives library
- Contains: MockAgent, MockLLM, TestDaemon (future)
- Status: Minimal implementation (expanded in later phases)

**crates/rigor-test/:**
- Purpose: Dev-only test orchestrator
- Contains: E2E runner, benchmark runner, report generator
- Status: Stub (implemented in Plan D.3)

**layer/:**
- Purpose: LD_PRELOAD shared library for DNS/socket interception
- Contains: frida-gum hooks
- Key files: `src/lib.rs` — all hooks in single file (by design for LD_PRELOAD)

**viewer/:**
- Purpose: Web UI for constraint graph visualization
- Contains: Static assets + interactive JavaScript
- Key files:
  - `index.html` — 3D graph viewer (3d-force-graph.js)
  - `style.css` — Styling
  - `*.js` — Dependencies (Cytoscape, Dagre)
- Generated by: `rigor graph --web` command

**examples/:**
- Purpose: Configuration examples and reference policies
- Contains: rigor.yaml files for different use cases
- Key examples:
  - `basic/rigor.yaml` — Inline Rego constraints
  - `beliefs-focused/rigor.yaml` — Belief-heavy configuration
  - `defeaters-focused/rigor.yaml` — Defeater patterns

**policies/builtin/:**
- Purpose: Built-in policy templates (future)
- Contains: Reference policy files

## Key File Locations

**Entry Points:**
- `crates/rigor/src/main.rs` — CLI/hook binary entry (dispatches to lib.rs:run() or cli::run_cli())
- `crates/rigor/src/lib.rs` — Hook evaluation pipeline (pub fn run())

**Configuration:**
- `Cargo.toml` — Workspace manifest (3 crates)
- `rigor.yaml` — Self-apply example (dogfooding)
- `examples/**/rigor.yaml` — Example configurations

**Core Logic:**
- `crates/rigor/src/lib.rs` — Constraint evaluation pipeline (all 8 steps)
- `crates/rigor/src/constraint/graph.rs` — DF-QuAD algorithm
- `crates/rigor/src/evaluator/pipeline.rs` — Evaluator routing
- `crates/rigor/src/policy/engine.rs` — Rego evaluation via regorus
- `crates/rigor/src/violation/collector.rs` — Violation aggregation

**Testing:**
- `crates/rigor/tests/` — Integration test directory (7 test files)
- `crates/rigor/benches/` — Benchmarks (2 Criterion tests)
- `crates/rigor-test/` — Test orchestrator (not yet implemented)

## Naming Conventions

**Files:**
- Snake case: `claim_extraction.rs`, `policy_engine.rs`
- Submodule: `mod.rs` (aggregates multiple related types)
- Types module: `types.rs` (contains type definitions only)
- Tests: `*_test.rs` in same directory or `tests/` subdirectory
- Benchmarks: `*_bench.rs` in `benches/` subdirectory
- Integration tests: `tests/*.rs` (one file per scenario)

**Directories:**
- Snake case: `src/claim/`, `src/constraint/`, `src/evaluator/`
- Grouped by domain: `claim/`, `constraint/`, `policy/` not scattered
- Daemon subsystem: `daemon/` with sub-modules (proxy, sni, tls, gate, etc.)
- CLI commands: `cli/` with one file per subcommand (e.g., `cli/serve.rs`)

**Functions:**
- Snake case: `run_hook()`, `extract_claims_from_transcript()`, `compute_strengths()`
- Public entry points: `run()` at module level
- Internal helpers: Prefixed or in private submodules

**Types:**
- PascalCase: `Constraint`, `Claim`, `ArgumentationGraph`, `HookResponse`
- Trait names: `ClaimEvaluator`, `RelevanceLookup`, `ClaimExtractor`
- Enum variants: PascalCase: `EpistemicType::Belief`, `Severity::Block`

**Constants:**
- SCREAMING_SNAKE_CASE: `MITM_HOSTS`, `MAX_ITERATIONS`, `EPSILON`
- Module-level statics: `Lazy`, `OnceLock` for initialization

## Where to Add New Code

**New Evaluator (e.g., ML-based):**
- Primary code: `crates/rigor/src/evaluator/ml_evaluator.rs` (implements ClaimEvaluator)
- Export: Add `pub mod ml_evaluator;` to `crates/rigor/src/evaluator/mod.rs`
- Registration: `crates/rigor/src/lib.rs:evaluate_constraints()` → build pipeline, register after semantic
- Tests: `crates/rigor/tests/integration_evaluator.rs` (new or expand existing)

**New CLI Subcommand (e.g., `rigor audit`):**
- Implementation: `crates/rigor/src/cli/audit.rs`
- Route: Add to `Commands` enum in `crates/rigor/src/cli/mod.rs`
- Handler: Add match arm in `run_cli()` function
- Tests: `crates/rigor/tests/integration_cli.rs` (new or expand)

**New Constraint Type or Epistemic Category:**
- Types: Update `crates/rigor/src/constraint/types.rs` (add enum variant, update struct)
- Config loader: Update `crates/rigor/src/constraint/loader.rs`
- Graph: Update `crates/rigor/src/constraint/graph.rs` (base strength mappings)
- Validation: Update `crates/rigor/src/constraint/validator.rs`
- Tests: `crates/rigor/tests/integration_constraint.rs`

**New Daemon Endpoint (e.g., `/api/foo`):**
- Handler: `crates/rigor/src/daemon/foo_api.rs` (new file)
- Router: Update `crates/rigor/src/daemon/mod.rs:build_router()` to add route
- Tests: `crates/rigor/tests/egress_integration.rs` (HTTP testing)

**New Language Defaults:**
- Templates: `crates/rigor/src/defaults/new_language.rs`
- Registry: Update `crates/rigor/src/defaults/mod.rs` to include
- Tests: `crates/rigor/tests/integration_defaults.rs` (new or expand)

**New Logging Feature:**
- Types: `crates/rigor/src/logging/new_feature.rs`
- Export: Add to `crates/rigor/src/logging/mod.rs`
- Integration: Update `crates/rigor/src/lib.rs` violation logging block (Step 7.5)
- Tests: `crates/rigor/tests/integration_logging.rs` (new or expand)

**Test Harness Utilities (for future adapter authors):**
- Primitives: `crates/rigor-harness/src/lib.rs`
- Export: Public `pub fn mock_*()` functions
- Docs: Code examples in pub fn docs (rustdoc)

## Special Directories

**crates/rigor/target/:**
- Purpose: Cargo build artifacts
- Generated: Yes (by `cargo build`)
- Committed: No (.gitignore)

**.planning/codebase/:**
- Purpose: Auto-generated architecture documentation
- Files: ARCHITECTURE.md, STRUCTURE.md, CONVENTIONS.md, TESTING.md, CONCERNS.md, STACK.md, INTEGRATIONS.md
- Generated: By gsd:map-codebase agent
- Committed: Yes (tracked in git)

**.planning/roadmap/:**
- Purpose: Feature roadmap and phase plans
- Generated: By gsd:plan-phase agent
- Committed: Yes

**.opencode/ and .claude/:**
- Purpose: IDE configuration for OpenCode and Claude Code
- plugins/: OpenCode plugin registry
- commands/: Custom Claude Code commands
- Committed: Yes (for team collaboration)

**.github/workflows/:**
- Purpose: GitHub Actions CI/CD
- Committed: Yes

---

*Structure analysis: 2026-04-19*
