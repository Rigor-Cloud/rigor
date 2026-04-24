# Phase 4: `rigor corpus` CLI subcommand wiring - Research

**Researched:** 2026-04-24
**Domain:** Rust CLI wiring (clap 4.5 + tokio runtime bridge)
**Confidence:** HIGH

## Summary

Phase 4 is a pure CLI surface wiring task. All library logic already exists and is tested in `crates/rigor/src/corpus/` (merged in PR #5 as `c6f885c`). The work is: (1) create `cli/corpus.rs` with a `CorpusCommands` enum containing `Record`, `Stats`, `Validate` variants, (2) add `Commands::Corpus` to `cli/mod.rs`, and (3) dispatch each variant to the existing library functions.

The codebase has a well-established pattern for nested subcommands: `RefineCommands` in `cli/refine.rs`, `LogCommands` in `cli/log.rs`, and `AlertCommands` in `cli/alert.rs` all use `#[derive(Subcommand)]` with a dispatcher function. The corpus CLI follows this pattern exactly.

**Primary recommendation:** Follow the `cli/refine.rs` pattern. Create `cli/corpus.rs` with `CorpusCommands` enum, a `run_corpus_command(cmd)` dispatcher, and per-variant handler functions. The `record` handler needs a tokio runtime (use `tokio::runtime::Runtime::new().unwrap().block_on(...)` matching `cli/ground.rs`). The `stats` and `validate` handlers are sync.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions
None -- all decisions are at Claude's discretion per CONTEXT.md.

### Claude's Discretion
- Add Commands::Corpus with CorpusCommands enum (Record, Stats, Validate)
- Create cli/corpus.rs with clap-derived subcommands
- Dispatch to existing library API (corpus::record_prompt, corpus::compute_stats, etc.)
- Stats can start with JSON output (pretty-print is Phase 6)
- Over-editing guard: only add cli/corpus.rs and wire into mod.rs

### Deferred Ideas (OUT OF SCOPE)
- Pretty-print stats table -- Phase 6
- Seed corpus recording -- Phase 5

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REQ-010 | `rigor corpus record` CLI subcommand dispatches to `ChatClient::record` and writes to `~/.rigor/corpus/` | Library function `record_prompt` exists in `corpus/record.rs`; `OpenRouterClient::from_env()` in `corpus/client.rs`; `load_prompts` in `corpus/mod.rs` |
| REQ-011 | `rigor corpus stats` reads corpus recordings and emits per-model/per-prompt summary | Library functions `compute_stats` + `aggregate_by_model` in `corpus/stats.rs`; `load_recordings` in `corpus/mod.rs`; replay pattern in `tests/corpus_replay.rs` |
| REQ-012 | `rigor corpus validate` verifies integrity (SHA-256, schema) of recorded corpus entries | `compute_prompt_hash` in `corpus/record.rs` produces `sha256:...` hashes; each `RecordedSample` stores `prompt_hash`; validation compares stored hash vs. recomputed hash from manifest |

</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| CLI argument parsing | CLI (clap) | -- | clap derive macros handle all arg parsing |
| Corpus recording | API / Backend (library) | CLI (dispatch) | `record_prompt` in corpus/record.rs owns the logic; CLI provides args + runtime |
| Stats computation | API / Backend (library) | CLI (output) | `compute_stats` + `aggregate_by_model` in corpus/stats.rs; CLI serializes to JSON |
| Integrity validation | API / Backend (library) | CLI (orchestration) | `compute_prompt_hash` exists; CLI loads manifests + recordings and cross-checks |
| OpenRouter HTTP | API / Backend (library) | -- | `OpenRouterClient::from_env()` already handles HTTP |
| Filesystem layout | API / Backend (library) | -- | `load_prompts`, `load_recordings` already handle directory walking |

## Standard Stack

### Core (already in Cargo.toml -- no new dependencies)

| Library | Version | Purpose | Why Standard | Confidence |
|---------|---------|---------|--------------|------------|
| clap | 4.5 | CLI arg parsing with derive | Already in Cargo.toml, used by all existing subcommands | HIGH [VERIFIED: Cargo.toml] |
| tokio | 1.x (multi-thread) | Async runtime for `record` subcommand | Already in Cargo.toml, needed because `record_prompt` is async | HIGH [VERIFIED: Cargo.toml] |
| serde_json | (in tree) | JSON serialization for stats output | Already a dependency; stats output is JSON per CONTEXT.md | HIGH [VERIFIED: Cargo.toml] |
| sha2 | (in tree) | SHA-256 hash recomputation for validate | Already used by `compute_prompt_hash` in corpus/record.rs | HIGH [VERIFIED: corpus/record.rs:7] |

**Installation:** No new dependencies. All required crates are already in `Cargo.toml`.

## Architecture Patterns

### System Architecture Diagram

```
                   rigor corpus <subcommand>
                           |
                     [clap parse]
                           |
              +------------+------------+
              |            |            |
           Record        Stats      Validate
              |            |            |
     [tokio runtime]  [sync call]  [sync call]
              |            |            |
  OpenRouterClient   load_recordings  load_prompts
  ::from_env()            |          load_recordings
       |            compute_stats         |
  load_prompts          |         compute_prompt_hash
       |         aggregate_by_model    (compare)
  record_prompt         |               |
       |          [JSON to stdout]  [OK/error report]
  [summary to stderr]
```

### Recommended Project Structure

```
crates/rigor/src/cli/
  corpus.rs         # NEW — CorpusCommands enum + dispatch + handlers
  mod.rs            # MODIFY — add `pub mod corpus;`, Commands::Corpus variant, dispatch arm
```

No other files should be touched. The over-editing guard explicitly limits changes to these two files.

### Pattern 1: Nested Subcommand Enum (reference: cli/refine.rs)

**What:** A `#[derive(Subcommand)]` enum inside the CLI module, dispatched from a top-level `run_*_command` function.
**When to use:** Any CLI command with sub-commands (corpus record/stats/validate).

```rust
// Source: crates/rigor/src/cli/refine.rs:249-284
#[derive(Subcommand)]
pub enum RefineCommands {
    Suggest { /* args */ },
    Export { /* args */ },
}

pub fn run_refine_command(cmd: RefineCommands) -> Result<()> {
    match cmd {
        RefineCommands::Suggest { apply, dry_run } => run_refine(apply, dry_run),
        RefineCommands::Export { constraint, since, out } => run_export(constraint, since, out),
    }
}
```

[VERIFIED: crates/rigor/src/cli/refine.rs lines 249-284]

### Pattern 2: Commands Enum Variant with Nested Subcommand (reference: cli/mod.rs)

**What:** Top-level `Commands` enum uses `#[command(subcommand)]` to nest the module's enum.
**When to use:** Wiring a new subcommand module into the root CLI.

```rust
// Source: crates/rigor/src/cli/mod.rs:334-337
Refine {
    #[command(subcommand)]
    command: refine::RefineCommands,
},
```

Dispatch arm (line 451):
```rust
Some(Commands::Refine { command }) => refine::run_refine_command(command),
```

[VERIFIED: crates/rigor/src/cli/mod.rs lines 334-337 and 451]

### Pattern 3: Sync-to-Async Bridge (reference: cli/ground.rs)

**What:** When `run_cli()` is sync but the library function is async, build a tokio runtime inline.
**When to use:** The `record` subcommand -- `record_prompt` is `async fn`.

```rust
// Source: crates/rigor/src/cli/ground.rs:467-468
let rt = tokio::runtime::Runtime::new().unwrap();
rt.block_on(async move {
    // ... call async library functions here
});
```

[VERIFIED: crates/rigor/src/cli/ground.rs lines 467-468]

### Pattern 4: Comma-Separated Model List Parsing

**What:** Issue #21 specifies comma-separated model slugs: `--models "deepseek/deepseek-r1,anthropic/claude-sonnet-4-6"`.
**When to use:** The `record` subcommand `--models` argument.

```rust
// Parse comma-separated model slugs
let models: Vec<String> = raw_models
    .split(',')
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .collect();
```

[ASSUMED -- standard Rust string splitting, no library-specific API]

### Anti-Patterns to Avoid

- **Over-editing:** Do NOT touch any file besides `cli/corpus.rs` (new) and `cli/mod.rs` (add variant + dispatch). The library code is already complete and tested.
- **Implementing replay logic in the CLI:** The `compute_stats` function takes a `replay_fn` closure. The CLI should provide a real replay function (load rigor.yaml, build PolicyEngine, extract claims, check for violations). Do NOT inline the replay logic -- reference the pattern from `tests/corpus_replay.rs`.
- **Custom error handling:** Use `anyhow::Result` throughout, matching every other CLI module. Do NOT create custom error types.
- **Pretty-printing stats:** Phase 6 handles this. Output JSON to stdout. Emit human-readable summary to stderr if needed.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Arg parsing | Manual arg parsing | clap derive macros | Already used everywhere, gives --help for free |
| Hash computation | Custom SHA-256 | `corpus::record::compute_prompt_hash` | Already exists, tested, uses sha2 crate |
| Directory walking | Custom fs traversal | `corpus::load_prompts`, `corpus::load_recordings` | Already exist, handle edge cases |
| HTTP client | Custom reqwest calls | `OpenRouterClient::from_env()` | Already wraps reqwest with timeout, auth, error handling |
| Stats aggregation | Custom counting | `corpus::compute_stats`, `corpus::aggregate_by_model` | Already exist with sort stability guarantees |

## Common Pitfalls

### Pitfall 1: Forgetting the Tokio Runtime for `record`

**What goes wrong:** `record_prompt` is `async fn`. Calling it from the sync `run_cli()` dispatcher causes a compile error.
**Why it happens:** `main()` is not `#[tokio::main]`, and `run_cli()` returns `Result<()>` synchronously.
**How to avoid:** Build a runtime inline: `tokio::runtime::Runtime::new().unwrap().block_on(async { ... })`. This matches `cli/ground.rs:467`.
**Warning signs:** Compile error about `async fn` in sync context.

### Pitfall 2: Stats Replay Function Complexity

**What goes wrong:** `compute_stats` requires a `FnMut(&RecordedSample) -> bool` replay function. The CLI handler must provide a real one that loads rigor.yaml, builds a PolicyEngine, extracts claims, and checks for violations.
**Why it happens:** The stats module is intentionally decoupled from the evaluator. The caller provides the replay oracle.
**How to avoid:** Follow the exact pattern from `tests/corpus_replay.rs:36-48`: load config, build PolicyEngine, extract claims via `extract_claims_from_text`, check `violated` flag.
**Warning signs:** Trying to pass a trivial `|_| false` closure -- that gives useless stats.

### Pitfall 3: Validate Hash Recomputation Needs Temperature

**What goes wrong:** `compute_prompt_hash` takes `(manifest, model, temperature)`. The RecordedSample stores `temperature` but the validate handler needs to extract it from the sample, not assume a default.
**Why it happens:** Temperature is part of the hash input. Different temperatures produce different hashes.
**How to avoid:** Read `sample.temperature` from each RecordedSample when recomputing the expected hash.
**Warning signs:** Validate reports hash mismatches even for unmodified recordings.

### Pitfall 4: Default Corpus Paths

**What goes wrong:** REQ-010 says "writes to `~/.rigor/corpus/`" but issue #21's CLI surface uses `--output .planning/corpus/recordings/` (project-relative). The prompts dir defaults differ too.
**Why it happens:** Corpus data can live in the project tree (committed seed) or user home (runtime recordings).
**How to avoid:** Use `--prompts` and `--output`/`--recordings` flags with sensible defaults. For `record`, default output to `~/.rigor/corpus/recordings/` (per REQ-010). For `stats` and `validate`, accept `--recordings` and `--prompts` flags.
**Warning signs:** Recordings scattered in unexpected locations.

### Pitfall 5: Model Slug Contains Slashes

**What goes wrong:** Model slugs like `anthropic/claude-sonnet-4-6` contain `/` which can't be a directory name.
**Why it happens:** OpenRouter uses `provider/model-name` format.
**How to avoid:** `slugify_model` in `corpus/record.rs` already handles this (replaces `/` with `_`). The CLI doesn't need to do anything -- the library handles it.
**Warning signs:** N/A -- already handled by the library.

## Code Examples

### Example 1: CorpusCommands Enum

```rust
// New file: crates/rigor/src/cli/corpus.rs
use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum CorpusCommands {
    /// Record LLM responses for corpus prompts via OpenRouter
    Record {
        /// Directory containing prompt manifests (YAML)
        #[arg(long, default_value = ".planning/corpus/prompts")]
        prompts: PathBuf,
        /// Comma-separated model slugs
        #[arg(long)]
        models: String,
        /// Number of samples per (prompt, model) pair
        #[arg(long, default_value = "10")]
        samples: u32,
        /// Sampling temperature
        #[arg(long, default_value = "0.7")]
        temperature: f64,
        /// Max tokens per response
        #[arg(long, default_value = "512")]
        max_tokens: u32,
        /// Output directory for recordings
        #[arg(long, default_value = ".planning/corpus/recordings")]
        output: PathBuf,
        /// Skip samples that already exist on disk
        #[arg(long)]
        resume: bool,
        /// Record only this prompt ID (default: all)
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Show per-model/per-prompt corpus statistics
    Stats {
        /// Directory containing recordings
        #[arg(long, default_value = ".planning/corpus/recordings")]
        recordings: PathBuf,
    },
    /// Verify integrity (SHA-256, schema) of recorded corpus entries
    Validate {
        /// Directory containing prompt manifests
        #[arg(long, default_value = ".planning/corpus/prompts")]
        prompts: PathBuf,
        /// Directory containing recordings
        #[arg(long, default_value = ".planning/corpus/recordings")]
        recordings: PathBuf,
    },
}
```

[ASSUMED -- synthesized from issue #21 CLI surface spec + codebase patterns]

### Example 2: Commands::Corpus Variant in mod.rs

```rust
// In cli/mod.rs Commands enum:
/// Recorded-LLM corpus management: record, stats, validate
Corpus {
    #[command(subcommand)]
    command: corpus::CorpusCommands,
},

// In run_cli() match:
Some(Commands::Corpus { command }) => corpus::run_corpus_command(command),
```

[VERIFIED: pattern matches cli/refine.rs wiring exactly]

### Example 3: Record Handler with Tokio Runtime

```rust
fn run_record(
    prompts_dir: PathBuf,
    raw_models: String,
    samples: u32,
    temperature: f64,
    max_tokens: u32,
    output_dir: PathBuf,
    resume: bool,
    prompt_filter: Option<String>,
) -> Result<()> {
    let models: Vec<String> = raw_models
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if models.is_empty() {
        anyhow::bail!("--models requires at least one model slug");
    }

    let manifests = crate::corpus::load_prompts(&prompts_dir)?;
    let client = crate::corpus::OpenRouterClient::from_env()?;

    let cfg = crate::corpus::RecordConfig {
        models: &models,
        samples,
        temperature,
        max_tokens,
        resume,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        for manifest in &manifests {
            if let Some(ref filter) = prompt_filter {
                if &manifest.id != filter {
                    continue;
                }
            }
            eprintln!("Recording: {} ...", manifest.id);
            let stats = crate::corpus::record_prompt(&client, manifest, &output_dir, &cfg).await?;
            eprintln!(
                "  recorded={}, skipped={}",
                stats.recorded, stats.skipped
            );
        }
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
```

[ASSUMED -- synthesized from record_prompt signature + issue #21 spec]

### Example 4: Validate Handler

```rust
fn run_validate(prompts_dir: PathBuf, recordings_dir: PathBuf) -> Result<()> {
    let manifests = crate::corpus::load_prompts(&prompts_dir)?;
    let recordings = crate::corpus::load_recordings(&recordings_dir)?;

    let manifest_map: std::collections::HashMap<&str, &crate::corpus::PromptManifest> =
        manifests.iter().map(|m| (m.id.as_str(), m)).collect();

    let mut errors = Vec::new();
    let mut checked = 0u32;

    for (prompt_id, per_model) in &recordings {
        let manifest = match manifest_map.get(prompt_id.as_str()) {
            Some(m) => m,
            None => {
                errors.push(format!("{}: no matching prompt manifest", prompt_id));
                continue;
            }
        };
        for (model_slug, samples) in per_model {
            // Reverse the slugify to get the original model name
            let model = model_slug.replace('_', "/");
            for sample in samples {
                checked += 1;
                let expected_hash = crate::corpus::record::compute_prompt_hash(
                    manifest,
                    &model,
                    sample.temperature,
                );
                if sample.prompt_hash != expected_hash {
                    errors.push(format!(
                        "{}/{}/sample_{:03}: hash mismatch (stored={}, expected={})",
                        prompt_id, model_slug, sample.sample_index + 1,
                        sample.prompt_hash, expected_hash,
                    ));
                }
                // Schema check: verify required fields are non-empty
                if sample.response_text.is_empty() {
                    errors.push(format!(
                        "{}/{}/sample_{:03}: empty response_text",
                        prompt_id, model_slug, sample.sample_index + 1,
                    ));
                }
            }
        }
    }

    if errors.is_empty() {
        println!("OK: {} recordings validated, 0 errors", checked);
        Ok(())
    } else {
        for e in &errors {
            eprintln!("ERROR: {}", e);
        }
        anyhow::bail!("{} validation error(s) in {} recordings", errors.len(), checked)
    }
}
```

[ASSUMED -- synthesized from compute_prompt_hash signature + REQ-012 spec]

## Key API Reference

### Public Functions (corpus module)

| Function | Signature | Async | Module |
|----------|-----------|-------|--------|
| `load_prompts` | `(prompts_dir: &Path) -> Result<Vec<PromptManifest>>` | No | corpus/mod.rs |
| `load_recordings` | `(recordings_dir: &Path) -> Result<BTreeMap<String, BTreeMap<String, Vec<RecordedSample>>>>` | No | corpus/mod.rs |
| `record_prompt` | `(client: &C, manifest: &PromptManifest, output_dir: &Path, cfg: &RecordConfig) -> Result<RecordStats>` | **Yes** | corpus/record.rs |
| `compute_prompt_hash` | `(m: &PromptManifest, model: &str, temperature: f64) -> String` | No | corpus/record.rs |
| `compute_stats` | `(recordings: &BTreeMap<...>, replay_fn: F) -> Vec<ModelStats>` | No | corpus/stats.rs |
| `aggregate_by_model` | `(rows: &[ModelStats]) -> Vec<PerModelAggregate>` | No | corpus/stats.rs |
| `OpenRouterClient::from_env` | `() -> Result<Self>` | No | corpus/client.rs |
| `slugify_model` | `(model: &str) -> String` | No | corpus/record.rs |

[VERIFIED: all signatures confirmed by reading source files directly]

### Key Data Types

| Type | Fields | Module |
|------|--------|--------|
| `RecordConfig<'a>` | `models: &'a [String]`, `samples: u32`, `temperature: f64`, `max_tokens: u32`, `resume: bool` | corpus/record.rs |
| `RecordStats` | `recorded: u32`, `skipped: u32` | corpus/record.rs |
| `RecordedSample` | `prompt_id`, `prompt_hash`, `model`, `sample_index`, `recorded_at`, `temperature`, `response_text`, `tokens`, `cost_usd`, `openrouter_response_id` | corpus/recording.rs |
| `ModelStats` | `prompt_id`, `model`, `samples: u32`, `blocks: u32` + `block_rate()` | corpus/stats.rs |
| `PerModelAggregate` | `model`, `total_samples: u32`, `total_blocks: u32` + `block_rate()` | corpus/stats.rs |
| `PromptManifest` | `id`, `prompt`, `system_prompt`, `tags`, `expected: ExpectationSet`, `notes` | corpus/manifest.rs |

[VERIFIED: all types confirmed by reading source files directly]

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No corpus CLI | Library functions only (PR #5, c6f885c) | 2026-04-22 | Need CLI wiring (this phase) |
| No validate | `prompt_hash` field stored in recordings | 2026-04-22 | Validate can recompute and compare |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Default paths for `--prompts` and `--recordings` should be `.planning/corpus/prompts` and `.planning/corpus/recordings` (project-relative) based on issue #21 CLI surface | Code Examples | Low -- paths are CLI args, user can override. REQ-010 says `~/.rigor/corpus/` for record output, which conflicts with issue #21's `--output .planning/corpus/recordings/`. Planner should pick one default. |
| A2 | `model_slug.replace('_', "/")` correctly reverses `slugify_model` | Code Examples (validate) | Medium -- if model names contain underscores natively (e.g. `google/gemini_2.0_flash`), the reverse mapping would be wrong. Should verify against known models or store original model name in RecordedSample (it does -- `sample.model` field). |
| A3 | Stats replay can use the same approach as `tests/corpus_replay.rs` (PolicyEngine + extract_claims_from_text) | Pitfalls | Low -- this is the established pattern |
| A4 | Scaffold placeholder hashes like `sha256:scaffold-placeholder-001` will fail validate (by design -- they're not real hashes) | Code Examples (validate) | Low -- expected behavior, user re-records to fix |

**Important correction for A2:** On closer inspection, `RecordedSample` already stores the original model name in `sample.model` (e.g. `"anthropic/claude-sonnet-4-6"`). The validate handler should use `sample.model` directly rather than reversing the slug. This eliminates the reversal risk entirely.

## Open Questions

1. **Default output directory for `record`**
   - What we know: REQ-010 says `~/.rigor/corpus/`. Issue #21 CLI surface shows `--output .planning/corpus/recordings/`.
   - What's unclear: Which should be the default? The project-local path (for committed seed) vs home dir (for user-specific recordings).
   - Recommendation: Use project-relative `.planning/corpus/recordings/` as default per issue #21, since the seed corpus (Phase 5) commits there. The `--output` flag lets users redirect.

2. **Stats replay function: full PolicyEngine or stub?**
   - What we know: Issue #21 says "Use the same approach as `tests/corpus_replay.rs`" (full PolicyEngine).
   - What's unclear: This requires loading `rigor.yaml` which may not exist or be findable. What if the user doesn't have rigor.yaml?
   - Recommendation: Stats handler should accept an optional `--rigor-yaml` path, using `find_rigor_yaml(None)` as default. If no rigor.yaml found, fall back to a pass-through replay (`|_| false`) and warn the user.

3. **`rigor corpus stats` output format**
   - What we know: CONTEXT.md says "Stats can start with JSON output (pretty-print is Phase 6)".
   - What's unclear: Exact JSON shape.
   - Recommendation: Serialize `Vec<ModelStats>` + `Vec<PerModelAggregate>` as a JSON object with `"per_prompt"` and `"per_model"` keys. This is the natural library output shape.

## Sources

### Primary (HIGH confidence)
- `crates/rigor/src/corpus/mod.rs` -- load_prompts, load_recordings signatures
- `crates/rigor/src/corpus/record.rs` -- record_prompt, RecordConfig, compute_prompt_hash
- `crates/rigor/src/corpus/stats.rs` -- compute_stats, aggregate_by_model, ModelStats
- `crates/rigor/src/corpus/client.rs` -- ChatClient trait, OpenRouterClient::from_env
- `crates/rigor/src/corpus/recording.rs` -- RecordedSample, TokenCounts
- `crates/rigor/src/corpus/manifest.rs` -- PromptManifest, ExpectedVerdict
- `crates/rigor/src/cli/mod.rs` -- Commands enum, dispatch pattern
- `crates/rigor/src/cli/refine.rs` -- RefineCommands subcommand pattern (primary reference)
- `crates/rigor/src/cli/log.rs` -- LogCommands subcommand pattern (secondary reference)
- `crates/rigor/src/cli/alert.rs` -- AlertCommands subcommand pattern (secondary reference)
- `crates/rigor/src/cli/ground.rs` -- tokio runtime bridge pattern (line 467)
- `crates/rigor/tests/corpus_replay.rs` -- replay_one_sample pattern for stats
- GitHub issue #21 -- full specification and CLI surface design

### Secondary (MEDIUM confidence)
- `Cargo.toml` -- clap 4.5, tokio 1.x versions confirmed

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all already in Cargo.toml
- Architecture: HIGH -- following exact pattern from 3+ existing subcommand modules
- Pitfalls: HIGH -- all identified from direct codebase reading
- API signatures: HIGH -- every function signature verified from source

**Research date:** 2026-04-24
**Valid until:** 2026-05-24 (stable -- internal codebase, not fast-moving external dependency)
