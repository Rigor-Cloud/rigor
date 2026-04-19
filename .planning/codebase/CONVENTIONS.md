# Coding Conventions

**Analysis Date:** 2026-04-19

## Language & Edition

**Rust:**
- Edition: 2021
- Primary language for all core logic
- Located in: `crates/rigor/src/`, `crates/rigor-harness/src/`

## Naming Patterns

**Modules:**
- Snake_case directory names: `claim/`, `evaluator/`, `constraint/`, `policy/`
- Module files mimic module name: `evaluator/pipeline.rs` exports `pub mod pipeline`
- Re-exports in `mod.rs`: public items from submodules are re-exported at module level for cleaner public API

**Functions:**
- Snake_case: `run_hook()`, `evaluate_constraints()`, `extract_claims_from_transcript()`
- Builder/constructor pattern: `new()`, `from_config()`, `from_engine()`
- Getter methods: `get_all_strengths()`, `get_assistant_messages()`, `get_latest_assistant_message()`

**Types & Structs:**
- PascalCase: `RigorConfig`, `Constraint`, `EpistemicType`, `ClaimEvaluator`, `EvaluatorPipeline`
- Enum variants: PascalCase like `Supports`, `Attacks`, `Undercuts`
- Enum variant values: lowercase in serialized form via `#[serde(rename_all = "lowercase")]`

**Variables:**
- Snake_case: `constraint_count`, `violation_count`, `temp_dir`, `yaml_path`
- Shorthand acceptable when unambiguous: `config`, `claims`, `engine`, `graph`

**Constants:**
- Uppercase: `RIGOR_TEST_CLAIMS`, `RIGOR_DEBUG`
- Environment variables always SCREAMING_SNAKE_CASE

## Code Style

**Formatting:**
- No explicit formatter configured in repository
- Follows Rust convention of 4-space indentation
- Lines appear to follow reasonable length (80-100 chars typical)

**Linting:**
- No `.clippy.toml` or `clippy.toml` configured
- Default Rust/Clippy conventions assumed

**Module Structure:**
- Flat modules for domain concepts: `claim::`, `constraint::`, `evaluator::`, `violation::`
- Type definition files: `types.rs` contains core data structures
- Implementation/logic files: `loader.rs`, `extractor.rs`, `pipeline.rs`, `mod.rs`
- Module visibility: exported via `pub use` in `mod.rs`

## Import Organization

**Order Pattern:**
1. Standard library imports (`use std::...`)
2. External crate imports (`use serde::...`, `use anyhow::...`)
3. Internal crate imports (`use crate::claim::...`)
4. Re-exports of submodule items (`pub use pipeline::...`)

**Example from `lib.rs`:**
```rust
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info, info_span, warn};

use claim::{Claim, ClaimExtractor, HeuristicExtractor};
use config::find_rigor_lock;
use constraint::graph::ArgumentationGraph;
```

**Path Aliases:**
- Absolute paths: `crate::module::Type` for absolute reference
- Relative imports: `use module::Type` when in sibling modules
- No shorthand aliases; explicit paths preferred for clarity

## Error Handling

**Result-based:**
- All fallible operations return `Result<T>` or `anyhow::Result<T>`
- Never use `.unwrap()` or `.expect()` in production code paths
- Test code may use `.unwrap()` / `.expect()` with clear intent

**Fail-Open Pattern:**
- Critical design principle: when constraint evaluation fails, always allow/pass through
- Example from `lib.rs` (lines 118-125):
  ```rust
  if let Some(yaml_path) = yaml_path {
      match constraint::loader::load_rigor_config(yaml_path) {
          Ok(config) => config,
          Err(e) => {
              warn!(error = %e, "Failed to load rigor.yaml, failing open");
              let response = HookResponse::allow();
              response.write_stdout()?;
              return Ok(());
          }
      }
  }
  ```
- Applied consistently: pipeline construction, evaluator initialization, logging

**Error Wrapping:**
- Use `anyhow::Result` for context chains
- Log detailed errors with `warn!()` macro before returning allow decision
- Include error in log span for traceability

## Comments & Documentation

**Module-Level Docs (//!):**
- All modules start with `//!` block describing purpose
- Include design rationale when module abstracts a pattern
- Reference key types with `[`Type`]` markdown-like syntax for rustdoc linking

**Example from `evaluator/pipeline.rs`:**
```rust
//! Pluggable claim evaluator pipeline.
//!
//! Defines the [`ClaimEvaluator`] trait, the [`EvalResult`] return type, and
//! the [`EvaluatorPipeline`] which routes a claim/constraint pair to the
//! first registered evaluator that [`ClaimEvaluator::can_evaluate`] it.
```

**Doc Comments (///):**
- All public types use `///` comments
- Trait methods documented with purpose and behavior expectations
- Include examples for complex types when helpful

**Inline Comments (// or block /* */):**
- Used sparingly for non-obvious logic
- Prefer self-documenting code (descriptive names, clear structure)
- Used for architectural decisions: `// CRITICAL: ...`, `// Step N: ...`

**Example from `lib.rs` (line 74):**
```rust
// CRITICAL: Check stop_hook_active to prevent infinite loops
if input.stop_hook_active {
```

## Function Design

**Size:**
- Typically 50-150 lines for core logic functions
- Longer functions (200+ lines) organized with step comments (`// Step 1:`, `// Step 2:`)
- Example: `evaluate_constraints()` is 275 lines with clear step markers

**Parameters:**
- Prefer borrowing (`&T`) over ownership (`T`) except when consuming is semantically correct
- Trait objects use trait bounds: `impl ClaimEvaluator`, `Arc<dyn RelevanceLookup>`
- Builder pattern for complex construction: `EvaluatorPipeline::with_default_fallback(config)`

**Return Values:**
- Always use `Result<T>` for fallible operations, never bare `Option<T>` for errors
- Return self on builders: `pub fn register(mut self, ...) -> Self`
- Constructor errors propagated to caller; fail-open decisions made in calling layer

## Type Design

**Struct Fields:**
- Public fields acceptable when type is data carrier
- Private fields with accessor methods when logic guards access
- Serde derives on config types: `#[derive(Debug, Clone, Serialize, Deserialize)]`

**Enum Design:**
- Variants represent discrete states: `Decision::Block`, `Decision::Warn`, `Decision::Allow`
- Serde renames for serialization control: `#[serde(rename_all = "lowercase")]`
- Match exhaustiveness enforced by compiler

**Trait Design:**
- Single responsibility: `ClaimEvaluator` trait is only "can you evaluate this claim?"
- Default implementations minimal; prefer concrete types
- Trait bounds on generics preferred: `fn register(evaluator: Box<dyn ClaimEvaluator>)`

## Logging

**Framework:** `tracing` crate with structured logging

**Spans:**
- Major operations enclosed in `info_span!()` or `debug_span!()`
- Example from `lib.rs`:
  ```rust
  let span = info_span!("rigor_hook");
  let _guard = span.enter();
  ```

**Logging Levels:**
- `info!()` for normal operation milestones: "Hook invoked", "Found rigor.yaml"
- `warn!()` for recoverable errors and fail-open scenarios
- `debug!()` for step-by-step detail (conditional on `RIGOR_DEBUG` env var)
- Never use `error!()` unless the process is terminating

**Structured Fields:**
- Always use field=value syntax: `info!(config = %yaml_path.display(), "Found rigor.yaml")`
- Use `%` prefix for `Display` types, no prefix for serde::Serialize types
- Descriptive field names: `session_id`, `constraint_count`, `error`

**Example from `lib.rs` (lines 68-72):**
```rust
info!(
    session_id = %input.session_id,
    stop_hook_active = input.stop_hook_active,
    "Hook invoked"
);
```

## Trait & Impl Organization

**Trait Definitions:**
- Placed in same file as primary type
- When trait is reusable pattern (e.g., `ClaimEvaluator`), given dedicated doc comment with contract

**Impl Blocks:**
- Associated methods (constructors) grouped together at top
- Trait implementations separated: `impl ClaimEvaluator for RegexEvaluator { ... }`
- Multiple impl blocks acceptable when logically separated (e.g., public API vs internal helpers)

## Module Visibility

**Private by Default:**
- All module items private unless explicitly `pub`
- Submodule types only re-exported if part of public API
- Test helpers marked `#[cfg(test)]` when test-only

**Example from `violation/mod.rs`:**
```rust
pub mod collector;
pub mod formatter;
pub mod types;

pub use collector::{collect_violations, determine_decision, ConstraintMeta, Decision};
pub use formatter::ViolationFormatter;
pub use types::*;
```

## Testing Conventions

**Test Function Naming:**
- Test functions use descriptive names: `test_valid_config_loads()`, `test_stop_hook_active_allows_immediately()`
- Pattern: `test_<subject>_<expected_outcome>()`

**Test Modules:**
- Integration tests in `crates/rigor/tests/` directory (separate from source)
- Each test file starts with module doc comment explaining test scope
- Helpers extracted to shared functions with `fn` prefix

**Test Fixtures:**
- Use `tempfile::TempDir` for isolated filesystem operations
- Minimal setup per test; avoid test interdependencies
- Helper builders: `default_input()`, `create_test_claims()`

---

*Convention analysis: 2026-04-19*
