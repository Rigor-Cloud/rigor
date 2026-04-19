# Coding Conventions

**Analysis Date:** 2026-04-19

## Naming Patterns

**Files:**
- `snake_case.rs` for all modules: `claim_injection.rs`, `hedge_detector.rs`, `violation_log.rs`, `gate_api.rs`
- Module directories mirror the domain concept: `crates/rigor/src/claim/`, `crates/rigor/src/constraint/`, `crates/rigor/src/violation/`
- Each module directory has a `mod.rs` that re-exports the public surface
- Integration test files use `snake_case.rs` with a descriptive suffix: `integration_hook.rs`, `integration_constraint.rs`, `true_e2e.rs`, `dogfooding.rs`, `fallback_integration.rs`, `egress_integration.rs`, `claim_extraction_e2e.rs`
- Benchmark files go under `benches/` with suffix describing scope: `hook_latency.rs`, `evaluation_only.rs`

**Functions:**
- `snake_case` for all functions, including helpers: `extract_claims_from_text`, `assign_confidence`, `collect_violations`, `determine_decision`, `find_rigor_yaml`
- Constructors named `new()` (e.g. `PolicyEngine::new(&config)`, `ArgumentationGraph::new()`)
- Alternate constructors use `from_*` or `from_stdin`: `ArgumentationGraph::from_config(&config)`, `StopHookInput::from_stdin()`
- Predicate helpers prefixed with `is_`: `is_assertion`, `is_hedged`, `is_action_intent`, `daemon_alive`
- CLI entry points prefixed with `run_`: `run_cli`, `run_init`, `run_validate`, `run_show`, `run_graph`
- I/O helpers explicit about direction: `write_stdout`, `write_pid_file`, `remove_pid_file`

**Variables:**
- `snake_case` everywhere, including function parameters and locals
- Booleans use `is_`/`has_`/prefix style (`is_hedged`, `has_system`, `stop_hook_active`)
- Counters and sizes are descriptive: `constraint_count`, `violation_count`, `message_index`, `sentence_index`

**Types:**
- `UpperCamelCase` for `struct`/`enum`/`trait`: `Claim`, `Violation`, `EgressFilter`, `PolicyEngine`, `ArgumentationGraph`, `HookResponse`, `FallbackOutcome`
- Enum variants are `UpperCamelCase`: `ClaimType::Assertion`, `Severity::Block`, `EpistemicType::Belief`, `RelationType::Attacks`
- Trait names describe the capability: `EgressFilter`, `ClaimExtractor`
- When serialized to external formats the enum uses `#[serde(rename_all = "lowercase")]` or `#[serde(rename_all = "snake_case")]` — see `crates/rigor/src/constraint/types.rs:74` and `crates/rigor/src/claim/types.rs:47`

**Constants:**
- `SCREAMING_SNAKE_CASE` at module scope: `MAX_ITERATIONS`, `EPSILON`, `MITM_HOSTS`, `PRODUCTION_RIGOR_YAML`
- Global lazy regex uses `once_cell::sync::Lazy` with the same convention: `CODE_BLOCK_PATTERN`, `HYPOTHETICAL_PATTERN`, `DEFINITIVE_PATTERN` (see `crates/rigor/src/claim/heuristic.rs:17-29`)

## Code Style

**Formatting:**
- `cargo fmt` enforced — CI runs `cargo fmt -- --check` in `.github/workflows/ci.yml:55`
- No custom `rustfmt.toml`: default rustfmt profile (4-space indent, 100-char line limit)
- Function signatures wrap at the opening paren with one argument per line when long — see `fn collect_violations(...)` in `crates/rigor/src/violation/collector.rs:33`

**Linting:**
- `cargo clippy --all-targets --all-features -- -D warnings` — all clippy warnings are errors in CI (`.github/workflows/ci.yml:40`)
- Edition: `2021` (set at workspace level in `Cargo.toml:11`)

**Module declarations:**
- `pub mod <name>;` declarations go at the top of `mod.rs`
- Followed by `pub use` re-exports for the crate's public surface
- Example: `crates/rigor/src/claim/mod.rs` declares 6 sub-modules then re-exports `ClaimExtractor`, `HeuristicExtractor`, `parse_transcript`, `TranscriptMessage`, and `types::*`

## Import Organization

**Order (top-down):**
1. Standard library (`std::...`)
2. External crates (`anyhow`, `serde`, `tracing`, `regex`, etc.)
3. Internal crate items (`crate::...`, `super::...`)

**Real example from `crates/rigor/src/lib.rs:15-27`:**
```rust
use anyhow::Result;
use std::path::Path;
use tracing::{debug, info, info_span, warn};

use claim::{Claim, ClaimExtractor, HeuristicExtractor};
use config::find_rigor_lock;
use config::find_rigor_yaml;
use constraint::graph::ArgumentationGraph;
use hook::{HookResponse, StopHookInput};
use policy::{EvaluationInput, PolicyEngine};
use violation::{
    collect_violations, determine_decision, Decision, SeverityThresholds, ViolationFormatter,
};
```

- Multi-use from same crate consolidated into a single `use name::{A, B, C};`
- No path aliases — crate paths are used verbatim
- Macro imports (`info!`, `warn!`, etc.) come in via `use tracing::{info, warn};`

## Error Handling

**Primary pattern:** `anyhow::Result<T>` for all fallible functions.

**Context is mandatory on I/O boundaries:**
```rust
// crates/rigor/src/constraint/loader.rs:11
let content = fs::read_to_string(path)
    .with_context(|| format!("Failed to read rigor.yaml at {}", path.display()))?;

// crates/rigor/src/hook/input.rs:26
let input = serde_json::from_str(&buffer)
    .context("Failed to parse hook input JSON")?;
```

Use `.context("static string")` for fixed messages and `.with_context(|| format!(...))` when the message interpolates runtime data (61 matches across `crates/rigor/src/`).

**Error returns:** use `anyhow::bail!` for early exits with a formatted message:
```rust
// crates/rigor/src/cli/mod.rs:254
anyhow::bail!("No rigor.yaml found in current directory or any parent directory")
```

**Typed errors for library boundaries:** use `thiserror` when errors need to be matched on externally. Example in `crates/rigor/src/daemon/egress/chain.rs:23`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("filter `{filter}` blocked the request: {reason}")]
    Blocked { filter: String, reason: String },
    #[error("filter `{filter}` encountered an error: {reason}")]
    Internal { filter: String, reason: String },
}
```

**Fail-open discipline (critical project convention):**
Every step in the hook pipeline that can fail must fall through to `HookResponse::allow()` — never panic, never block on internal errors. See the repeating pattern in `crates/rigor/src/lib.rs:111-194`:
```rust
let config = match constraint::loader::load_rigor_config(yaml_path) {
    Ok(config) => config,
    Err(e) => {
        warn!(error = %e, "Failed to load rigor.yaml, failing open");
        let response = HookResponse::allow();
        response.write_stdout()?;
        return Ok(());
    }
};
```

**Fail-closed escape hatch:** `main()` in `crates/rigor/src/main.rs` checks `RIGOR_FAIL_CLOSED` env var. If set, any top-level error exits with code 2 (Claude Code treats this as blocking). Otherwise, errors are serialized into a `HookResponse::error(...)` JSON reply and the process exits 0.

**No `.unwrap()` on user/runtime data:** `.unwrap()` and `.expect(...)` appear only in:
- Test code (`.unwrap()` on test fixtures is fine)
- `once_cell::Lazy::new(|| Regex::new(...).expect("Valid pattern"))` — compile-time-known regex that will never fail at runtime
- Clap-derived types where parsing is guaranteed successful

## Logging

**Framework:** `tracing` 0.1 with `tracing-subscriber` (JSON + env-filter features). Initialized in `crates/rigor/src/observability/tracing.rs`.

**Output:** stderr + `~/.rigor/rigor.log` (multi-writer). Optional OpenTelemetry OTLP exporter if `OTEL_EXPORTER_OTLP_ENDPOINT` is set (graceful degrade if not).

**Levels:**
- `info!` — significant state transitions, decisions reached, counts reported
- `warn!` — fail-open events, retries, policy-skipped operations
- `error!` — terminal policy fires (`FailClosed`, retries exhausted)
- `debug!` — per-claim / per-iteration diagnostics; enabled via `RIGOR_DEBUG=1` or `RUST_LOG`
- `trace!` — essentially unused

**Structured fields are mandatory** (no string interpolation):
```rust
// crates/rigor/src/lib.rs:62
info!(
    session_id = %input.session_id,
    stop_hook_active = input.stop_hook_active,
    "Hook invoked"
);

// crates/rigor/src/lib.rs:114
warn!(error = %e, "Failed to load rigor.yaml, failing open");
```

Use `%value` for `Display`, `?value` for `Debug`, or plain `name = value` for primitives.

**Spans:** wrap top-level operations in `info_span!(...)`.  Example in `crates/rigor/src/lib.rs:44`:
```rust
let span = info_span!("rigor_hook");
let _guard = span.enter();
```

**Never use `println!` for diagnostics.** `println!` is reserved for structured output (hook JSON response on stdout — see `HookResponse::write_stdout` in `crates/rigor/src/hook/output.rs:70`). `eprintln!` is used for human-facing status messages shown to the end user (`crates/rigor/src/lib.rs:359`, `crates/rigor/src/main.rs:15`).

## Comments

**When to Comment:**
- Rationale, not restatement — explain WHY a decision was made, not what the code does
- Cross-reference related files or project phases when behavior is coupled (see `crates/rigor/src/daemon/mod.rs:19` pointing at the hook code that reads the PID file)
- Non-obvious invariants (the fail-open comment in `crates/rigor/src/lib.rs:48-57`, the stop-hook-active loop guard in `crates/rigor/src/lib.rs:68`)

**Module-level doc comments:** every non-trivial file starts with `//! ...` describing the module's purpose. Example from `crates/rigor/src/claim/heuristic.rs:3-11`:
```rust
/// Heuristic claim extraction from natural language text.
///
/// Extracts sentence-level claims using:
/// - Sentence segmentation (unicode-segmentation)
/// - Assertion filtering (questions, hypotheticals, code)
/// - Hedge detection (filter uncertain statements)
/// ...
```

Integration tests use `//!` module-level docs describing the test file's scope — see `crates/rigor/tests/integration_hook.rs:1-3` and `tests/dogfooding.rs:1-7`.

**TSDoc / rustdoc on public items:**
- `///` above every `pub fn`, `pub struct`, `pub enum` (enforced in practice across the codebase)
- First line is a one-sentence summary; subsequent paragraphs describe invariants, failure modes, or cross-references
- Use `` `backticks` `` for type names and identifiers
- Describe fail-open behavior explicitly when it applies (e.g. `crates/rigor/src/lib.rs:104-106`, `crates/rigor/src/policy/engine.rs:27-29`)

## Function Design

**Size:** most functions are 10–40 lines. The long orchestrator `evaluate_constraints` in `crates/rigor/src/lib.rs:106-364` (~260 lines) is an intentional exception — it's the top-level pipeline and is explicitly decomposed into numbered steps (`// Step 1:` ... `// Step 8:`).

**Parameters:**
- Prefer borrowed references (`&Config`, `&Path`, `&[Claim]`) over owned values except when the function consumes the value (`Vec<RawViolation>` in `collect_violations`)
- Use `impl Into<String>` for ergonomic string APIs where ownership is needed: `HookResponse::block(reason: impl Into<String>)` in `crates/rigor/src/hook/output.rs:40`
- Never take more than ~6 positional arguments; group related fields into a struct (see `ViolationLogEntry`, `EvaluationInput`)

**Return values:**
- Fallible operations return `anyhow::Result<T>` at binary/integration boundaries; typed errors (`thiserror`) at library boundaries like the egress filter chain
- Constructors return `Self` directly when infallible (`HookResponse::allow`), `Result<Self>` when they may fail (`PolicyEngine::new`)
- Iterator-style collection preferred over mutable accumulation — see the `filter/filter/map/collect` chain in `crates/rigor/src/claim/heuristic.rs:160-184`

**Defaults:** implement `Default` for any struct with a meaningful zero/neutral value (`ArgumentationGraph::default()` in `crates/rigor/src/constraint/graph.rs:34`, `SeverityThresholds::default()` in `crates/rigor/src/violation/types.rs:33`). The latter also doubles as the canonical thresholds (`block=0.7, warn=0.4`).

## Module Design

**Layout:** every domain area is a directory module with submodules:
```
crates/rigor/src/claim/
├── mod.rs             # declares submodules, re-exports public API
├── types.rs           # data structures (Claim, ClaimType, SourceLocation)
├── extractor.rs       # trait + heuristic implementation
├── heuristic.rs       # sentence extraction logic
├── hedge_detector.rs  # hedging pattern detection
├── confidence.rs      # rule-based confidence scoring
└── transcript.rs      # JSONL transcript parsing
```

**Re-exports:** `mod.rs` exposes the minimal public API — consumers import from `claim::` not `claim::types::`. See `crates/rigor/src/claim/mod.rs:8-12`:
```rust
pub use extractor::{ClaimExtractor, HeuristicExtractor};
pub use transcript::{
    get_assistant_messages, get_latest_assistant_message, parse_transcript, TranscriptMessage,
};
pub use types::*;
```

**Barrel files:** sparingly — only at module boundaries, not at crate root. `crates/rigor/src/lib.rs` declares top-level modules as `pub mod ...;` without wildcarding.

**Workspace layout:** three crates coordinated from root `Cargo.toml`:
- `crates/rigor/` — production binary + library (all runtime code)
- `crates/rigor-harness/` — test primitives intended to be publishable (currently empty stub per `crates/rigor-harness/src/lib.rs`)
- `crates/rigor-test/` — dev-only test orchestrator binary (`crates/rigor-test/src/main.rs`)

**Serde conventions:**
- `#[derive(Debug, Clone, Serialize, Deserialize)]` is the default derive set for data types
- `#[serde(default)]` on `Option<T>` / `Vec<T>` fields so missing YAML/JSON keys don't fail (`crates/rigor/src/constraint/types.rs:7,42-51`)
- `#[serde(rename_all = "lowercase")]` or `"snake_case"` for enum variants that ship over the wire (`crates/rigor/src/violation/types.rs:19`, `crates/rigor/src/constraint/types.rs:74,91`)
- `#[serde(skip_serializing_if = "Option::is_none")]` to omit null fields from JSON output (`crates/rigor/src/hook/output.rs:8-11,16-19`)

## Async Patterns

**Runtime:** `tokio` with `rt-multi-thread` + `macros` features (see `crates/rigor/Cargo.toml:27`).

**`async` functions** live primarily in `daemon/` (proxy, TLS, HTTP) and `fallback/` (retry loop). Synchronous code is the default; async is opt-in per subsystem.

**Traits that need async methods use `async-trait`:**
```rust
// crates/rigor/src/daemon/egress/chain.rs:41
#[async_trait]
pub trait EgressFilter: Send + Sync {
    fn name(&self) -> &'static str;
    async fn apply_request(
        &self,
        body: &mut Json,
        ctx: &mut ConversationCtx,
    ) -> Result<(), FilterError>;
}
```

**Shared state:** `Arc<T>` for read-only sharing of filters, `Arc<Mutex<T>>` sparingly for mutable state (see `crates/rigor/src/observability/tracing.rs:25`).

---

*Convention analysis: 2026-04-19*
