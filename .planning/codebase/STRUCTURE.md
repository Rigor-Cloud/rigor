# Codebase Structure

**Analysis Date:** 2026-04-19

## Directory Layout

```
rigor-opencode-hackathon/
├── Cargo.toml                    # Workspace manifest (members: rigor, rigor-harness, rigor-test)
├── Cargo.lock                    # Workspace lockfile
├── README.md                     # Minimal; points at `cargo build --release`
├── rigor.yaml                    # Dogfooded rigor config for this project itself
├── .github/workflows/ci.yml      # CI: cargo test, clippy, fmt, rigor validate self-check
├── .gitignore
├── crates/                       # Cargo workspace members
│   ├── rigor/                    # Primary crate: CLI + stop hook + daemon library
│   │   ├── Cargo.toml
│   │   ├── benches/              # Criterion benchmarks
│   │   │   ├── hook_latency.rs
│   │   │   └── evaluation_only.rs
│   │   ├── tests/                # Integration tests (one file per concern)
│   │   │   ├── integration_hook.rs
│   │   │   ├── integration_constraint.rs
│   │   │   ├── claim_extraction_e2e.rs
│   │   │   ├── egress_integration.rs
│   │   │   ├── fallback_integration.rs
│   │   │   ├── true_e2e.rs
│   │   │   └── dogfooding.rs
│   │   └── src/
│   │       ├── main.rs           # Binary entry — calls cli::run_cli
│   │       ├── lib.rs            # Library entry — `run()` + `run_hook()` pipeline
│   │       ├── claim/            # Transcript parsing + heuristic claim extraction
│   │       ├── cli/              # Clap subcommands (one file per command)
│   │       ├── config/           # rigor.yaml lookup (walks parent dirs)
│   │       ├── constraint/       # Typed config + DF-QuAD argumentation graph
│   │       ├── daemon/           # Axum HTTP/HTTPS/WS server + MITM proxy
│   │       │   └── egress/       # SSE filter chain for streaming responses
│   │       ├── defaults/         # Built-in constraints (rust.rs, go.rs, deps.rs)
│   │       ├── fallback/         # Error-policy engine (retry/fail-open/fail-closed)
│   │       ├── hook/             # Claude Code Stop hook I/O (StopHookInput, HookResponse)
│   │       ├── logging/          # JSONL violation log + session metadata
│   │       ├── lsp/              # LSP client for source-anchor verification
│   │       ├── observability/    # tracing + OTLP setup
│   │       ├── policy/           # regorus (OPA) policy engine wrapper
│   │       └── violation/        # Violation types, collector, formatter
│   ├── rigor-harness/            # [placeholder] test-harness primitives (empty lib)
│   │   └── src/lib.rs
│   └── rigor-test/               # [placeholder] dev-only test orchestrator binary
│       └── src/main.rs
├── layer/                        # NOT a workspace member — separate cdylib
│   ├── Cargo.toml                # crate-type = ["cdylib"]
│   ├── Cargo.lock
│   └── src/lib.rs                # LD_PRELOAD / DYLD_INSERT_LIBRARIES frida-gum hooks
├── policies/                     # Rego policy assets (not Rust)
│   ├── helpers.rego              # Embedded via include_str! into PolicyEngine
│   └── builtin/                  # Reusable constitutional constraints
│       ├── calibrated-confidence.rego
│       ├── no-fabricated-apis.rego
│       ├── require-justification.rego
│       └── README.md
├── examples/                     # End-user rigor.yaml samples
│   ├── README.md
│   ├── rigor.yaml
│   ├── claude-hooks.json
│   ├── basic/
│   ├── beliefs-focused/
│   │   └── policies/
│   └── defeaters-focused/
├── docs/                         # End-user Markdown docs
│   ├── configuration.md
│   ├── constraint-authoring.md
│   └── epistemic-foundations.md
├── viewer/                       # Embedded dashboard assets (rust-embed)
│   ├── index.html
│   ├── style.css
│   ├── 3d-force-graph.min.js
│   ├── cytoscape.min.js
│   ├── cytoscape-dagre.js
│   └── dagre.min.js
├── target/                       # Cargo build output (gitignored)
├── .claude/                      # Claude Code project settings
└── .planning/                    # GSD planning artifacts (this directory)
    └── codebase/                 # Codebase map documents
```

## Directory Purposes

**`crates/rigor/src/`:**
- Purpose: All runtime code for the `rigor` binary and library.
- Contains: Rust modules organized by concern (claim, constraint, policy, violation, daemon, etc.).
- Key files: `lib.rs` (pipeline orchestration), `main.rs` (binary entry), `cli/mod.rs` (Clap tree).

**`crates/rigor/src/claim/`:**
- Purpose: Extract structured claims from Claude transcripts.
- Contains: `transcript.rs` (JSONL parsing), `extractor.rs` (trait + v1 impl), `heuristic.rs` (rules), `hedge_detector.rs`, `confidence.rs`, `types.rs` (`Claim`, `ClaimType`).
- Key files: `extractor.rs`, `heuristic.rs` (354 LOC — main extraction logic).

**`crates/rigor/src/cli/`:**
- Purpose: One file per `rigor` subcommand plus shared `Cli` struct.
- Contains: `mod.rs` (Clap definitions, `run_cli`, `find_rigor_yaml`), `init.rs`, `show.rs`, `validate.rs`, `graph.rs`, `ground.rs`, `log.rs`, `config.rs`, `map.rs`, `gate.rs`, `scan.rs`, `web.rs`.
- Key files: `mod.rs` (routing), `ground.rs` (614 LOC — proxy orchestrator), `init.rs` (686 LOC — project scaffolding), `scan.rs` (PII hook).

**`crates/rigor/src/config/`:**
- Purpose: Locate `rigor.yaml` / `rigor.lock` by walking parent directories.
- Contains: `lookup.rs` only.

**`crates/rigor/src/constraint/`:**
- Purpose: Domain model for constraints and the argumentation graph.
- Contains: `types.rs` (`RigorConfig`, `Constraint`, `Relation`, `SourceAnchor`, `EpistemicType`, `RelationType`), `loader.rs` (YAML → `RigorConfig`), `validator.rs`, `graph.rs` (`ArgumentationGraph` with DF-QuAD).
- Key files: `graph.rs` (526 LOC — argumentation semantics), `types.rs` (schema).

**`crates/rigor/src/daemon/`:**
- Purpose: Long-running HTTP/HTTPS proxy + dashboard server.
- Contains: `mod.rs` (`DaemonState`, router, PID-file), `proxy.rs` (LLM proxies + SSE), `tls.rs` (CA + cert signing + macOS trust), `sni.rs` (TLS ClientHello peek), `ws.rs` (WebSocket events), `gate.rs`/`gate_api.rs` (action gates), `governance.rs` (REST endpoints), `chat.rs`, `context.rs`, `egress/` (filter chain).
- Key files: `proxy.rs` (3092 LOC — largest file in repo), `mod.rs` (464 LOC — state + router), `tls.rs` (266 LOC — CA management).

**`crates/rigor/src/daemon/egress/`:**
- Purpose: Composable SSE body filter chain.
- Contains: `chain.rs` (filter trait, `SseChunk`, `FilterError`), `claim_injection.rs`, `ctx.rs` (`ConversationCtx`).

**`crates/rigor/src/defaults/`:**
- Purpose: Ship built-in language- and dependency-level constraints.
- Contains: `rust.rs`, `go.rs`, `deps.rs`, `mod.rs`.

**`crates/rigor/src/fallback/`:**
- Purpose: Policy-driven error handling.
- Contains: `types.rs` (policies), `config.rs` (YAML loading + resolution), `minimums.rs` (guardrails), `mod.rs` (`execute` + tests).

**`crates/rigor/src/hook/`:**
- Purpose: Claude Code Stop-hook JSON I/O.
- Contains: `input.rs` (`StopHookInput::from_stdin`), `output.rs` (`HookResponse`, `Metadata`).

**`crates/rigor/src/logging/`:**
- Purpose: Structured violation audit trail.
- Contains: `types.rs` (`ViolationLogEntry`, `SessionMetadata`, `ClaimSource`), `violation_log.rs` (`ViolationLogger`), `session.rs` (git metadata), `query.rs` (`rigor log query`), `annotate.rs` (`rigor log annotate`).

**`crates/rigor/src/lsp/`:**
- Purpose: Verify `SourceAnchor`s in code via LSP or grep.
- Contains: `mod.rs` (detection + grep fallback + `AnchorVerification`), `client.rs` (JSON-RPC LSP client, 490 LOC).

**`crates/rigor/src/observability/`:**
- Purpose: Tracing + OpenTelemetry initialization.
- Contains: `tracing.rs` (`init_tracing`, `shutdown`).

**`crates/rigor/src/policy/`:**
- Purpose: Wrap `regorus` (OPA) for per-constraint evaluation.
- Contains: `engine.rs` (`PolicyEngine`, `RawViolation`), `input.rs` (`EvaluationInput`).

**`crates/rigor/src/violation/`:**
- Purpose: Transform raw Rego results into typed decisions and formatted output.
- Contains: `types.rs` (`Violation`, `Severity`, `SeverityThresholds`), `collector.rs` (`collect_violations`, `determine_decision`, `Decision`), `formatter.rs` (524 LOC — terminal formatting with `owo-colors`).

**`crates/rigor/tests/`:**
- Purpose: Cargo integration tests (separate binary per file).
- Contains: one `*.rs` file per integration surface — `integration_hook.rs`, `integration_constraint.rs`, `claim_extraction_e2e.rs`, `egress_integration.rs`, `fallback_integration.rs`, `true_e2e.rs`, `dogfooding.rs`.

**`crates/rigor/benches/`:**
- Purpose: Criterion microbenchmarks.
- Contains: `hook_latency.rs`, `evaluation_only.rs`. Registered in `Cargo.toml` as `[[bench]]` with `harness = false`.

**`crates/rigor-harness/`:**
- Purpose: Planned future test primitives (MockAgent, MockLLM, TestDaemon, MockLSP, EventCapture).
- Contains: Currently only a docstring `lib.rs`. Spec referenced: `docs/superpowers/specs/2026-04-15-test-harness-architecture-design.md`.

**`crates/rigor-test/`:**
- Purpose: Dev-only test orchestrator binary.
- Contains: `main.rs` with Clap `e2e` / `bench` / `report` subcommands stubbed (bail with "not yet implemented").

**`layer/`:**
- Purpose: Out-of-workspace cdylib loaded via `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES`.
- Contains: `Cargo.toml` (standalone), `src/lib.rs` (957 LOC — frida-gum inline hooks).
- Not a workspace member; intentionally built separately by `cli::ground`.

**`policies/`:**
- Purpose: Rego policy assets shipped with the binary.
- Contains: `helpers.rego` (embedded at compile time via `include_str!("../../../../policies/helpers.rego")` in `policy/engine.rs`), `builtin/*.rego` (reusable templates).
- Generated: No. Committed: Yes.

**`examples/`:**
- Purpose: End-user `rigor.yaml` samples with progressive complexity.
- Contains: `basic/`, `beliefs-focused/` (also has its own `policies/` subdir), `defeaters-focused/`, plus `claude-hooks.json` (sample Claude Code hook configuration).

**`docs/`:**
- Purpose: End-user documentation (no internal architecture docs here).
- Contains: `configuration.md`, `constraint-authoring.md`, `epistemic-foundations.md`.

**`viewer/`:**
- Purpose: Static assets for the web dashboard (embedded into the binary).
- Contains: `index.html`, `style.css`, Cytoscape + D3-force-graph + Dagre JS libs.
- Served by: `daemon::build_router` + `cli::web::serve_viewer_asset` via `rust-embed`.
- Generated: No. Committed: Yes (vendored JS libraries).

**`target/`:**
- Purpose: Cargo build output.
- Generated: Yes. Committed: No (gitignored).

**`.github/workflows/`:**
- Purpose: GitHub Actions CI.
- Contains: `ci.yml` — `cargo test --all-features`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`, and `./target/release/rigor validate rigor.yaml` (self-dogfood).

**`.planning/codebase/`:**
- Purpose: GSD codebase map artifacts (this directory).
- Generated: Yes (by `/gsd/map-codebase`). Committed: project-dependent.

## Key File Locations

**Entry Points:**
- `crates/rigor/src/main.rs`: `rigor` binary entry; delegates to `cli::run_cli`, handles `RIGOR_FAIL_CLOSED`.
- `crates/rigor/src/lib.rs`: Library entry; `run()` (observability-wrapped) and `run_hook()` (full stop-hook pipeline).
- `crates/rigor/src/cli/mod.rs`: Clap `Cli` struct + `run_cli()` dispatcher.
- `crates/rigor-test/src/main.rs`: Separate dev-only binary.
- `layer/src/lib.rs`: cdylib with `#[used] static INIT` constructor in `__DATA,__mod_init_func` (macOS) / `.init_array` (Linux).

**Configuration:**
- `Cargo.toml`: Workspace manifest — members + shared dependency versions.
- `crates/rigor/Cargo.toml`: Primary crate deps; `[[bench]]` registrations.
- `layer/Cargo.toml`: cdylib config; frida-gum dep.
- `rigor.yaml`: Self-dogfooded rigor constraints (validated in CI).
- `.github/workflows/ci.yml`: CI matrix.

**Core Logic:**
- `crates/rigor/src/lib.rs::run_hook` and `evaluate_constraints`: Canonical stop-hook pipeline.
- `crates/rigor/src/policy/engine.rs::PolicyEngine::new`: Per-constraint Rego module compilation.
- `crates/rigor/src/constraint/graph.rs::ArgumentationGraph::compute_strengths`: DF-QuAD fixed-point iteration.
- `crates/rigor/src/violation/collector.rs::collect_violations` + `determine_decision`: Severity assignment.
- `crates/rigor/src/daemon/mod.rs::start_daemon` + `build_router`: Daemon bootstrap.
- `crates/rigor/src/daemon/proxy.rs`: Anthropic/OpenAI/catch-all proxy handlers.
- `crates/rigor/src/claim/heuristic.rs::extract_claims_from_text`: Main claim-extraction pass.

**Policy Assets:**
- `policies/helpers.rego`: Shared Rego helpers loaded into every `PolicyEngine` (embedded at compile time).
- `policies/builtin/*.rego`: Ship with binary; reusable constitutional constraints.

**Testing:**
- `crates/rigor/tests/*.rs`: Integration test entry points (one binary each).
- `crates/rigor/benches/*.rs`: Criterion benches (registered in `Cargo.toml` with `harness = false`).
- Unit tests: inline `#[cfg(test)] mod tests { ... }` inside each source file (see `crates/rigor/src/fallback/mod.rs`, `crates/rigor/src/violation/types.rs`).

## Naming Conventions

**Files:**
- Rust modules: snake_case, one concern per file (e.g., `hedge_detector.rs`, `violation_log.rs`, `gate_api.rs`).
- Submodule roots: `mod.rs` per directory; re-exports common types via `pub use types::*;` pattern.
- Integration tests: `integration_<area>.rs` or `<area>_e2e.rs` or descriptive names like `dogfooding.rs`, `true_e2e.rs`.
- Benchmarks: `<target>_<aspect>.rs` (e.g., `hook_latency.rs`, `evaluation_only.rs`).
- Rego files: kebab-case.rego (e.g., `no-fabricated-apis.rego`, `calibrated-confidence.rego`).

**Directories:**
- Crate roots: kebab-case (`rigor-harness`, `rigor-test`) per Cargo convention.
- Source directories: snake_case matching their module name.
- Policy/example subdirectories: kebab-case (`beliefs-focused`, `defeaters-focused`).

**Modules/Types:**
- Structs/enums: `UpperCamelCase` (`RigorConfig`, `HookResponse`, `ArgumentationGraph`, `FallbackOutcome`).
- Enums with serde: `#[serde(rename_all = "lowercase")]` or `"snake_case"` (see `EpistemicType`, `RelationType`, `ClaimType`, `Severity`).
- Constants: `SCREAMING_SNAKE_CASE` (`MAX_ITERATIONS`, `EPSILON`, `MITM_HOSTS`).
- Functions: `snake_case` (`run_hook`, `collect_violations`, `should_mitm_target`).

## Where to Add New Code

**New CLI Subcommand:**
- Add variant to `Commands` enum in `crates/rigor/src/cli/mod.rs`.
- Create new file `crates/rigor/src/cli/<name>.rs` with a public `run_<name>(...)` entry function.
- Declare the module at the top of `crates/rigor/src/cli/mod.rs` (`pub mod <name>;`).
- Wire the match arm in `run_cli`.

**New Constraint Built-in:**
- Rust-level defaults: extend `crates/rigor/src/defaults/rust.rs` / `go.rs` / `deps.rs` (add new `defaults/<lang>.rs` if needed and re-export from `defaults/mod.rs`).
- Shareable Rego: add `policies/builtin/<name>.rego` and reference it from example configs.

**New Claim Extractor:**
- Implement the `ClaimExtractor` trait from `crates/rigor/src/claim/extractor.rs`.
- Add a new file under `crates/rigor/src/claim/` (e.g., `llm_extractor.rs`).
- Declare in `crates/rigor/src/claim/mod.rs`.
- Swap into `lib.rs::extract_claims_from_transcript` when wiring up.

**New Daemon Route / Governance Endpoint:**
- Implement handler in appropriate `crates/rigor/src/daemon/<area>.rs` (or create one).
- Register in `crates/rigor/src/daemon/mod.rs::build_router`.
- For WebSocket-streamed events, emit via `state.event_tx` — see `crates/rigor/src/daemon/ws.rs`.

**New Egress Filter:**
- Add file under `crates/rigor/src/daemon/egress/` implementing the filter trait from `chain.rs`.
- Re-export from `crates/rigor/src/daemon/egress/mod.rs`.
- Wire into proxy SSE pipeline in `crates/rigor/src/daemon/proxy.rs`.

**New Fallback-Governed Operation:**
- Wrap the fallible async op in `state.fallback.execute("component_name", || async { ... })`.
- Add a corresponding `ComponentPolicy` override in the fallback YAML section of `rigor.yaml` if needed.
- Ensure any new component is represented in `fallback::minimums::Minimums` validation if it has hard requirements.

**New Integration Test:**
- Create `crates/rigor/tests/<name>.rs` — each file compiles as a separate test binary.
- Use `tempfile` for filesystem fixtures (already a workspace dev-dep).

**New Bench:**
- Create `crates/rigor/benches/<name>.rs`.
- Register in `crates/rigor/Cargo.toml` as `[[bench]] name = "<name>" harness = false`.

**New Violation Type / Severity Tier:**
- Extend `crates/rigor/src/violation/types.rs` (`Severity`, `SeverityThresholds`).
- Update `crates/rigor/src/violation/collector.rs::determine_decision` to map the new severity.
- Adjust `crates/rigor/src/violation/formatter.rs` for rendering.

**New Interception Target (libc hook):**
- Add detour function in `layer/src/lib.rs` following the `DetourGuard` + `OnceLock<Fn>` pattern used for existing hooks.
- Register in `install_hooks()` via `Module::find_global_export_by_name` + `interceptor.replace`.

## Special Directories

**`target/`:**
- Purpose: Cargo build artifacts.
- Generated: Yes. Committed: No.

**`viewer/`:**
- Purpose: Vendored JS dashboard assets embedded into the `rigor` binary.
- Generated: No (vendored libraries committed directly). Committed: Yes.

**`policies/`:**
- Purpose: Rego assets referenced from Rust source via `include_str!` + shipped as runtime-loadable templates.
- Generated: No. Committed: Yes.
- Note: `policies/helpers.rego` is compile-time-embedded; changing it requires a rebuild.

**`layer/`:**
- Purpose: Intentionally excluded from the workspace so it can be built with a separate target/feature set (cdylib only, different dep tree).
- Generated: No. Committed: Yes.

**`.planning/`:**
- Purpose: GSD workflow artifacts (plans, codebase maps, phase outputs).
- Generated: Yes (by GSD commands). Committed: project-dependent.

**`.claude/`:**
- Purpose: Claude Code project settings and agent scratchpads.
- Generated: Yes (by Claude Code). Committed: project-dependent.

---

*Structure analysis: 2026-04-19*
