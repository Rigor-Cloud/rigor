# Phase 2: PR-4 Corpus Exporter (`rigor refine export`) - Research

**Researched:** 2026-04-24
**Domain:** CLI extension / JSONL streaming export from violation log
**Confidence:** HIGH

## Summary

Phase 2 adds `rigor refine export` to emit training-ready JSONL from the violation log at `~/.rigor/violations.jsonl`. This is a narrowly-scoped CLI feature that transforms existing `ViolationLogEntry` records into a downstream-consumable `CorpusRow` JSONL format, consumed by Phase 3E (GEPA prompt optimization) and Phase 4E (Modal discriminator training).

The implementation is straightforward: read lines from the JSONL violation log using `BufReader`, apply `--constraint` and `--since` filters line-by-line (no full-file materialization), transform each `ViolationLogEntry` into a `CorpusRow`, and serialize to the output destination. The existing codebase already has every dependency needed: `serde`/`serde_json` for serialization, `chrono` for date parsing, `clap` for CLI, and `BufReader`/`BufWriter` for streaming I/O. No new crate dependencies are required.

**Primary recommendation:** Extend the existing `cli/refine.rs` module by converting `Refine` from a flat command to a subcommand enum (`Refine { Suggest, Export }`), add a new `CorpusRow` struct in `cli/refine.rs`, and implement line-by-line streaming export. Keep all new code in the existing file to honor the over-editing guard.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
None explicitly locked -- all at Claude's discretion per issue #19.

### Claude's Discretion
- New modules: refine/mod.rs, refine/corpus.rs (or extend existing cli/refine.rs)
- CLI: `rigor refine export --constraint <id> --since <date> --format jsonl --output <path>`
- Read through ViolationLogBackend trait
- Start with JSONL only (Parquet later)
- Over-editing guard: don't refactor existing violation log code

### Deferred Ideas (OUT OF SCOPE)
- Parquet format -- defer
- rigor refine optimize (GEPA) -- Phase 3E
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REQ-006 | `rigor refine export` MUST emit JSONL where each line is one training record (violation + context + ground-truth decision + metadata) | CorpusRow struct maps all ViolationLogEntry fields to training-record shape; JSONL via serde_json::to_string per line |
| REQ-007 | Exporter MUST be streaming (does not load the full violations log into memory); output path MUST be `--out <path>` with stdout fallback | BufReader line iteration + BufWriter output; `--out` flag with Box<dyn Write> pattern for stdout/file |
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| JSONL corpus export | CLI (binary) | -- | Pure data transformation, no daemon/server involvement |
| Violation log reading | CLI (file I/O) | -- | Direct JSONL file read via BufReader, no backend trait needed for sync read |
| Date filtering | CLI | -- | chrono date parsing, same pattern as search.rs |
| Output routing | CLI | -- | stdout/file via --out flag |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde + serde_json | 1.0 / 1.0 | CorpusRow serialization | Already in Cargo.toml, project standard [VERIFIED: Cargo.toml] |
| chrono | 0.4 | `--since` date parsing + `created_at` field | Already in Cargo.toml with serde feature [VERIFIED: Cargo.toml] |
| clap | 4.5 | Subcommand definition | Already in Cargo.toml with derive feature [VERIFIED: Cargo.toml] |
| anyhow | 1.0 | Error handling | Project-wide standard [VERIFIED: Cargo.toml] |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| std::io::BufReader | stdlib | Line-by-line violation log reading | Always -- required for streaming (REQ-007) |
| std::io::BufWriter | stdlib | Buffered output writing | Always -- wrap stdout or File for performance |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Manual BufReader line iteration | serde-jsonlines 0.7 (already in Cargo.toml) | serde-jsonlines loads a Vec, breaking streaming requirement; BufReader + lines() is simpler and streaming |
| Manual serde_json::to_string per line | csv crate | JSONL not CSV; wrong format |

**Installation:** No new dependencies needed. Everything is already in `crates/rigor/Cargo.toml`. [VERIFIED: Cargo.toml]

## Architecture Patterns

### System Architecture Diagram

```
~/.rigor/violations.jsonl
        |
        v
  [BufReader::lines()]  -- streaming line-by-line read
        |
        v
  [serde_json::from_str::<ViolationLogEntry>]  -- parse, skip malformed
        |
        v
  [Filter: --constraint, --since]  -- drop non-matching entries
        |
        v
  [Transform: ViolationLogEntry -> CorpusRow]  -- reshape fields
        |
        v
  [serde_json::to_string(&row)]  -- serialize one JSONL line
        |
        v
  [BufWriter -> --out file OR stdout]  -- write, flush
```

### Recommended Project Structure

The canonical spec (section 0J) proposes `crates/rigor/src/refine/corpus.rs` as a new module subtree. However, the current `refine` is a single file at `cli/refine.rs` (not a module directory). Two options:

**Option A (recommended): Extend `cli/refine.rs` in-place**
- Add `CorpusRow` struct and `run_export()` function to the existing file
- Convert `Refine` command from flat flags to a subcommand enum
- Minimal disruption, honors over-editing guard
- cli/refine.rs grows from ~540 lines to ~700 lines -- still manageable

**Option B: Create `refine/` module directory**
- Rename `cli/refine.rs` to `cli/refine/mod.rs`, add `cli/refine/corpus.rs`
- Cleaner separation, matches Phase 3E expansion plan structure
- More file churn, risks the over-editing guard

**Recommendation:** Option A for Phase 2. Phase 3E can restructure into a module directory when it adds optimizer/evaluator/mutator modules.

### Pattern 1: Subcommand Conversion for `rigor refine`

**What:** The current `Refine` variant uses flat flags (`--apply`, `--dry-run`). Adding `export` requires either a new top-level command or subcommands. Subcommands are the clap-idiomatic approach and match existing patterns (`Log`, `Alert`). [VERIFIED: cli/mod.rs lines 166-167, 307-308]

**When to use:** When a CLI command gains multiple distinct operations.

**Example:**
```rust
// Source: existing pattern in cli/mod.rs + cli/log.rs
#[derive(Subcommand)]
pub enum RefineCommands {
    /// Analyze violation patterns and suggest constraint refinements
    Suggest {
        #[arg(long)]
        apply: bool,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Export violations as training-ready JSONL corpus
    Export {
        /// Filter to a single constraint ID
        #[arg(long)]
        constraint: Option<String>,
        /// Only include violations at or after this date (YYYY-MM-DD or RFC3339)
        #[arg(long)]
        since: Option<String>,
        /// Output file path (default: stdout)
        #[arg(long)]
        out: Option<PathBuf>,
    },
}
```

**CLI dispatch change in mod.rs:**
```rust
// Before:
Refine { apply, dry_run } => refine::run_refine(apply, dry_run),

// After:
Refine { command } => refine::run_refine_command(command),
```

This is a backward-incompatible change: `rigor refine --apply` becomes `rigor refine suggest --apply`. Since rigor is pre-1.0 and internal-use, this is acceptable. [ASSUMED]

### Pattern 2: Streaming Line-by-Line Export (REQ-007)

**What:** Read violations.jsonl line-by-line via BufReader, transform in-place, write each line to output. Never collect into a Vec.

**Why:** The violation log can grow unbounded. REQ-007 explicitly requires no full-file materialization.

**Example:**
```rust
// Source: pattern from existing ViolationLogger::read_all (violation_log.rs:54-86)
// Modified to NOT collect into Vec
use std::io::{BufRead, BufReader, BufWriter, Write};

pub fn run_export(
    constraint: Option<String>,
    since: Option<String>,
    out: Option<PathBuf>,
) -> Result<()> {
    let logger = ViolationLogger::new()?;
    let file = std::fs::File::open(logger.log_path())
        .context("Failed to open violations.jsonl")?;
    let reader = BufReader::new(file);

    let since_ts = since.as_deref().map(parse_since).transpose()?;

    let mut writer: BufWriter<Box<dyn Write>> = match out {
        Some(ref path) => BufWriter::new(Box::new(
            std::fs::File::create(path).context("Failed to create output file")?
        )),
        None => BufWriter::new(Box::new(std::io::stdout().lock())),
    };

    let mut count = 0usize;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }

        let entry: ViolationLogEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue, // skip malformed, forward compat
        };

        // Apply filters
        if let Some(ref cid) = constraint {
            if &entry.constraint_id != cid { continue; }
        }
        if let Some(ts) = since_ts {
            if let Ok(entry_ts) = chrono::DateTime::parse_from_rfc3339(&entry.session.timestamp) {
                if entry_ts.with_timezone(&chrono::Utc) < ts { continue; }
            } else { continue; }
        }

        let row = CorpusRow::from_violation(&entry);
        writeln!(writer, "{}", serde_json::to_string(&row)?)?;
        count += 1;
    }

    // Summary to stderr so it doesn't pollute stdout JSONL
    eprintln!("Exported {} record(s)", count);
    Ok(())
}
```

### Pattern 3: CorpusRow Design

**What:** The output record shape, designed to satisfy Phase 3E (GEPA) and Phase 4E (Modal training) downstream consumers.

**Source:** Section 0J of epistemic-expansion-plan.md (lines 300-311) [VERIFIED: .planning/roadmap/epistemic-expansion-plan.md]

```rust
use serde::{Deserialize, Serialize};

/// One training record in the exported corpus JSONL.
/// Schema designed for Phase 3E (GEPA) and Phase 4E (Modal training).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusRow {
    /// Claim text that triggered the violation
    pub claim_text: String,
    /// Constraint that was violated
    pub constraint_id: String,
    /// Constraint human-readable name
    pub constraint_name: String,
    /// Ground-truth label from the evaluator decision
    pub label: String,  // "block" | "warn" | "allow"
    /// Whether a human corrected the evaluator's decision
    pub human_corrected: Option<bool>,
    /// Evaluator reasoning / violation message
    pub reasoning: String,
    /// Model that produced the original output (if known)
    pub model: Option<String>,
    /// Epistemic knowledge type of the claim (if tagged)
    pub knowledge_type: Option<String>,
    /// Claim confidence score (if available)
    pub claim_confidence: Option<f64>,
    /// ISO 8601 timestamp from the session
    pub created_at: String,
    /// Session ID for provenance
    pub session_id: String,
}
```

**Field mapping from ViolationLogEntry:**

| CorpusRow field | ViolationLogEntry source | Notes |
|-----------------|-------------------------|-------|
| claim_text | claim_text[0] (first claim) | Multi-claim entries emit one row per claim |
| constraint_id | constraint_id | Direct |
| constraint_name | constraint_name | Direct |
| label | decision | "block" or "allow" |
| human_corrected | false_positive | If Some(true), human overrode the evaluator |
| reasoning | message | Evaluator's violation message |
| model | model | Optional, from proxy |
| knowledge_type | claim_type | String, optional |
| claim_confidence | claim_confidence | Optional f64 |
| created_at | session.timestamp | RFC3339 |
| session_id | session.session_id | UUID |

**Design decisions:**

1. **One row per claim, not per violation:** A ViolationLogEntry can have multiple `claim_text` entries. The CorpusRow should emit one row per claim for clean training data. This matches the GEPA evaluator which grades single claims. [VERIFIED: expansion plan line 575: `POST /v1/evaluator/grade-single -- single-claim grading`]

2. **`request_hash` omitted:** The spec mentions `request_hash` referencing content_store, but content_store is ephemeral (in-memory with TTL). The violation log has no request_hash field. Omit for now; Phase 3E can add it when content_store gets persistent audit writes. [VERIFIED: ViolationLogEntry has no request_hash field]

3. **`evaluator_version` omitted:** Does not exist in the codebase yet. Phase 3B adds it. [VERIFIED: grep found zero matches for evaluator_version in src/]

4. **`label` as String not enum:** The downstream consumer (Python/Modal) will read JSONL directly. A string "block"/"warn"/"allow" is more portable than a Rust enum. The ViolationLogEntry already stores `decision` as String. [VERIFIED: types.rs line 48]

5. **`knowledge_type` as String:** ViolationLogEntry has `claim_type: Option<String>` -- note this is the claim_type field, NOT KnowledgeType. The ViolationLogEntry does NOT carry a KnowledgeType field. We export `claim_type` as-is. [VERIFIED: types.rs lines 67]

### Anti-Patterns to Avoid

- **Loading all entries then filtering:** `ViolationLogger::read_all()` collects into `Vec<ViolationLogEntry>`. The export must NOT use this method -- it must read line-by-line from the file directly. This is the core of REQ-007.
- **Printing to stdout AND writing summary to stdout:** Export JSONL goes to stdout (default). Summary counts go to stderr so pipes work: `rigor refine export | head -5`.
- **Restructuring refine.rs into a module directory:** Premature -- Phase 3E will do that. For now, append to the existing file.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Date parsing | Custom date parser | chrono + existing `parse_since` from search.rs | Already handles YYYY-MM-DD and RFC3339, proven [VERIFIED: cli/search.rs:106-119] |
| Violation log reading | Custom file reader | BufReader + serde_json (same pattern as ViolationLogger) | Proven pattern, skip-malformed behavior [VERIFIED: violation_log.rs:54-86] |
| Subcommand CLI | Custom argument parsing | clap derive Subcommand enum | Project standard, existing examples [VERIFIED: cli/log.rs:7-31] |

**Key insight:** This feature is pure data transformation -- read JSONL, filter, reshape, write JSONL. Every piece of infrastructure already exists in the codebase. The risk is over-engineering, not under-engineering.

## Common Pitfalls

### Pitfall 1: Breaking `rigor refine` Backward Compatibility
**What goes wrong:** Existing usage `rigor refine --apply` breaks when converting to subcommands.
**Why it happens:** Subcommand conversion changes the CLI grammar.
**How to avoid:** Pre-1.0 software, acceptable breakage. But document in the PR description and add a clear error message if someone uses the old syntax. Consider whether `rigor refine` with no subcommand should default to `suggest` behavior.
**Warning signs:** CI tests that call `rigor refine --apply` directly.

### Pitfall 2: Stdout Pollution
**What goes wrong:** Summary output ("Exported 42 records") mixed with JSONL data on stdout.
**Why it happens:** println! defaults to stdout.
**How to avoid:** All non-JSONL output (counts, progress, errors) goes to stderr via `eprintln!`. JSONL-only on stdout.
**Warning signs:** `rigor refine export | jq .` fails on non-JSON lines.

### Pitfall 3: Multi-Claim Entries
**What goes wrong:** A ViolationLogEntry with 3 claims in `claim_text` emits only 1 row.
**Why it happens:** Treating ViolationLogEntry as 1:1 with CorpusRow.
**How to avoid:** Iterate over `entry.claim_text` and emit one CorpusRow per claim. If `claim_text` is empty, skip the entry (no claim = no training signal).
**Warning signs:** Corpus has fewer rows than expected; GEPA sees repeated claim texts.

### Pitfall 4: File Not Found When Log is Empty
**What goes wrong:** `File::open` fails when `~/.rigor/violations.jsonl` does not exist.
**Why it happens:** New installation, no violations recorded yet.
**How to avoid:** Check existence first. If file does not exist, emit 0 records and print message to stderr. Same pattern as ViolationLogger::read_all which returns empty Vec. [VERIFIED: violation_log.rs:54-56]
**Warning signs:** Error on fresh install.

### Pitfall 5: Forgetting `--out` flag name (REQ-007)
**What goes wrong:** Using `--output` instead of `--out` as specified in REQ-007.
**Why it happens:** REQ-007 explicitly states `--out <path>`.
**How to avoid:** Use `--out` as the argument name. Can alias `--output` with clap `#[arg(long, alias = "output")]`.
**Warning signs:** Failing requirement traceability check.

## Code Examples

### Reuse: parse_since from search.rs

The `parse_since` function in `cli/search.rs` (lines 106-119) already handles YYYY-MM-DD and RFC3339 parsing. [VERIFIED: cli/search.rs]

Two options:
1. **Copy the function into refine.rs** (simple, no cross-module dependency)
2. **Extract to a shared utility** (cleaner, but more file churn)

Recommendation: Copy for now. It is 13 lines. Phase 3E can deduplicate when it restructures the refine module.

```rust
// Source: cli/search.rs:106-119 — copy into cli/refine.rs
fn parse_since(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&chrono::Utc));
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let ndt = d.and_hms_opt(0, 0, 0).context("Invalid date")?;
        return Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(ndt, chrono::Utc));
    }
    anyhow::bail!("Expected YYYY-MM-DD or RFC3339 timestamp")
}
```

### ViolationLogEntry to CorpusRow Transform

```rust
impl CorpusRow {
    /// Convert a ViolationLogEntry into zero or more CorpusRows (one per claim).
    fn from_violation(entry: &ViolationLogEntry) -> Vec<CorpusRow> {
        if entry.claim_text.is_empty() {
            return vec![];
        }
        entry.claim_text.iter().map(|claim| CorpusRow {
            claim_text: claim.clone(),
            constraint_id: entry.constraint_id.clone(),
            constraint_name: entry.constraint_name.clone(),
            label: entry.decision.clone(),
            human_corrected: entry.false_positive,
            reasoning: entry.message.clone(),
            model: entry.model.clone(),
            knowledge_type: entry.claim_type.clone(),
            claim_confidence: entry.claim_confidence,
            created_at: entry.session.timestamp.clone(),
            session_id: entry.session.session_id.clone(),
        }).collect()
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `ViolationLogger::read_all()` collects Vec | New streaming BufReader iteration for export | This phase | Enables large-log export without OOM |
| `Refine` flat command | `Refine` with subcommands (Suggest, Export) | This phase | Extensible for Phase 3E optimize |

**Not deprecated:**
- `ViolationLogger::read_all()` remains for existing callers (suggest, search, etc.) -- only export needs streaming

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Breaking `rigor refine --apply` -> `rigor refine suggest --apply` is acceptable pre-1.0 | Pattern 1 | Users with scripts calling old syntax must update; low risk since rigor is internal tooling |
| A2 | One CorpusRow per claim (not per ViolationLogEntry) is the correct granularity for GEPA/Modal training | Pattern 3 | If downstream expects one-per-violation, schema mismatch; mitigated by the expansion plan's `grade-single` endpoint which operates on single claims |
| A3 | `claim_type` on ViolationLogEntry is the right proxy for `knowledge_type` in CorpusRow | Pattern 3 | If GEPA expects the enum-typed KnowledgeType, the String won't match; low risk since ViolationLogEntry predates KnowledgeType and downstream can parse |

## Open Questions

1. **Should `rigor refine` with no subcommand default to `suggest`?**
   - What we know: Current behavior is `rigor refine` (no args) runs suggest logic.
   - What is unclear: Whether to require `rigor refine suggest` explicitly or make it the default.
   - Recommendation: Make `suggest` the default subcommand to minimize breakage. Clap supports `#[command(flatten)]` or default subcommand patterns.

2. **Should `--format jsonl` flag be included even though Parquet is deferred?**
   - What we know: The spec mentions `--format jsonl|parquet` but Parquet is deferred.
   - What is unclear: Whether to add a `--format` flag that only accepts `jsonl`.
   - Recommendation: Omit `--format` entirely for now. JSONL-only. When Parquet ships, add the flag then. YAGNI.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) |
| Config file | None (default) |
| Quick run command | `cargo test -p rigor refine -- --test-threads=1` |
| Full suite command | `cargo test -p rigor` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REQ-006 | Export JSONL with correct CorpusRow schema | unit | `cargo test -p rigor test_export_produces_valid_corpus_row -- --exact` | Wave 0 |
| REQ-006 | Multi-claim entries emit one row per claim | unit | `cargo test -p rigor test_export_multi_claim_emits_per_claim -- --exact` | Wave 0 |
| REQ-006 | Malformed lines skipped | unit | `cargo test -p rigor test_export_skips_malformed_lines -- --exact` | Wave 0 |
| REQ-007 | Streaming: no Vec collection | unit | `cargo test -p rigor test_export_streams_line_by_line -- --exact` | Wave 0 |
| REQ-007 | --out writes to file | unit | `cargo test -p rigor test_export_writes_to_file -- --exact` | Wave 0 |
| REQ-007 | stdout fallback when no --out | unit | `cargo test -p rigor test_export_defaults_to_stdout -- --exact` | Wave 0 |
| REQ-006 | --constraint filter works | unit | `cargo test -p rigor test_export_constraint_filter -- --exact` | Wave 0 |
| REQ-006 | --since filter works | unit | `cargo test -p rigor test_export_since_filter -- --exact` | Wave 0 |
| REQ-006 | Empty log produces 0 records | unit | `cargo test -p rigor test_export_empty_log -- --exact` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p rigor refine`
- **Per wave merge:** `cargo test -p rigor`
- **Phase gate:** Full suite green before verify

### Wave 0 Gaps
- [ ] All test functions above are new -- they will be implemented alongside the feature code in cli/refine.rs `#[cfg(test)] mod tests`
- [ ] Test helper: write temp violations.jsonl with known entries (can reuse `mk()` helper already in refine.rs tests, line 484)

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | -- |
| V3 Session Management | no | -- |
| V4 Access Control | no | File permissions inherited from OS |
| V5 Input Validation | yes | serde_json deserialization rejects malformed input; chrono rejects invalid dates |
| V6 Cryptography | no | -- |

### Known Threat Patterns for CLI Export

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Path traversal via --out | Tampering | std::fs::File::create handles OS-level path validation; no symlink following needed for append-only export |
| Malformed JSONL injection | Tampering | serde_json::from_str fails on malformed lines; skip, do not panic |

Low threat surface -- this is a read-only export of local files owned by the current user.

## Sources

### Primary (HIGH confidence)
- `crates/rigor/src/cli/refine.rs` -- existing refine implementation, 543 lines
- `crates/rigor/src/cli/mod.rs` -- CLI dispatch, Refine command definition
- `crates/rigor/src/logging/types.rs` -- ViolationLogEntry struct (30-78)
- `crates/rigor/src/logging/violation_log.rs` -- ViolationLogger read_all pattern
- `crates/rigor/src/logging/backend.rs` -- ViolationLogBackend trait
- `crates/rigor/src/cli/search.rs` -- parse_since, filter patterns
- `crates/rigor/src/cli/log.rs` -- subcommand enum pattern
- `.planning/roadmap/epistemic-expansion-plan.md` section 0J (lines 293-319) -- CorpusRow spec
- `.planning/roadmap/epistemic-expansion-plan.md` section 4E (lines 910-959) -- downstream consumer
- `crates/rigor/Cargo.toml` -- dependency versions

### Secondary (MEDIUM confidence)
- `.planning/roadmap/epistemic-expansion-plan.md` section 3E (lines 601-698) -- GEPA corpus expectations

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all deps already in Cargo.toml, verified
- Architecture: HIGH -- pure data transform, well-understood pattern, all source code read
- Pitfalls: HIGH -- based on direct codebase analysis of existing patterns

**Research date:** 2026-04-24
**Valid until:** 2026-05-24 (stable -- no external dependencies or fast-moving APIs)
