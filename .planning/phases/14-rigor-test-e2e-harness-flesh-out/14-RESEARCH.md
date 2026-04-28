# Phase 14: rigor-test e2e harness flesh-out - Research

**Researched:** 2026-04-24
**Domain:** Rust CLI binary (rigor-test) -- replacing stub subcommands with real implementations using rigor-harness primitives
**Confidence:** HIGH

## Summary

Phase 14 replaces three `anyhow::bail!("not yet implemented")` stubs in `crates/rigor-test/src/main.rs` with real implementations that use `rigor-harness` primitives. The rigor-harness library (IsolatedHome, TestCA, MockLlmServer, TestProxy, SSE helpers, subprocess helpers) is already fully implemented from Phase 7. The rigor-test binary currently has a clap skeleton with `e2e`, `bench`, and `report` subcommands -- all of which bail immediately.

The `e2e` subcommand should launch a TestProxy backed by a MockLlmServer, send a request through it, and verify the rigor pipeline produces expected decisions. The `bench` subcommand should run the existing criterion benchmarks (hook_latency, evaluation_only, dfquad_scaling, filter_chain_overhead) from the rigor crate. The `report` subcommand should read a JSONL events file and produce a human-readable summary (HTML or plaintext).

**Primary recommendation:** Wire each subcommand to rigor-harness primitives and existing infrastructure. The `e2e` command orchestrates MockLlmServer + TestProxy in-process using tokio. The `bench` command shells out to `cargo bench -p rigor`. The `report` command reads JSONL lines and writes a simple HTML report. Add smoke tests for each subcommand to `crates/rigor-test/`.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions
None -- all implementation at Claude's discretion.

### Claude's Discretion
All implementation at Claude's discretion. Key context:
- rigor-harness now has: IsolatedHome, TestCA, MockLlmServer, TestProxy, subprocess helpers, SSE helpers
- rigor-test has: clap skeleton with e2e/bench/report subcommands, all stub
- Over-editing guard: replace stubs with real flows, don't restructure the CLI

### Deferred Ideas (OUT OF SCOPE)
None.

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REQ-026 | `rigor-test` subcommands (currently stubbed with "not yet implemented") have real implementations with passing smoke tests. | All three subcommands (e2e, bench, report) have clear implementation paths using existing rigor-harness primitives and criterion benchmarks. Smoke tests verify each subcommand exits 0 and produces expected output. |

</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| E2E test orchestration | rigor-test binary | rigor-harness (library) | rigor-test is the user-facing CLI; rigor-harness provides reusable test primitives |
| Mock LLM + Proxy bring-up | rigor-harness | -- | Already implemented: MockLlmServerBuilder, TestProxy, IsolatedHome |
| Benchmark execution | rigor crate (benches/) | rigor-test (shell-out) | Benchmarks already exist as criterion benches in rigor crate; rigor-test just invokes them |
| Report generation | rigor-test binary | -- | New code; reads JSONL, formats output. No existing report infrastructure to reuse |
| SSE response verification | rigor-harness (sse.rs) | -- | parse_sse_events, extract_text_from_sse already handle Anthropic + OpenAI formats |

## Standard Stack

### Core (already in workspace)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| clap | 4.5 (workspace) | CLI argument parsing | Already used by rigor-test; derive macros for subcommand parsing [VERIFIED: Cargo.toml] |
| anyhow | 1.0 (workspace) | Error handling | Already a rigor-test dependency [VERIFIED: Cargo.toml] |
| tokio | 1.52 (workspace) | Async runtime | Required for MockLlmServer + TestProxy which are async [VERIFIED: rigor-harness Cargo.toml] |
| rigor-harness | 0.1.0 (path) | Test primitives | IsolatedHome, TestCA, MockLlmServer, TestProxy, SSE helpers [VERIFIED: crate source] |
| rigor | 0.1.0 (path) | Core rigor types | PolicyEngine, claim extraction, config loading [VERIFIED: crate source] |
| serde_json | 1.0 (workspace) | JSON parsing | For reading JSONL events files [VERIFIED: workspace Cargo.toml] |
| serde | 1.0 (workspace) | Serialization | For test scenario and report data structures [VERIFIED: workspace Cargo.toml] |

### Supporting (needs adding to rigor-test/Cargo.toml)
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio | 1 (workspace features: rt-multi-thread, macros) | Async runtime for e2e subcommand | Required -- MockLlmServer/TestProxy are async [VERIFIED: rigor-harness requires tokio] |
| serde_json | 1.0 (workspace) | JSONL parsing for report cmd | Required -- reading events JSONL [VERIFIED: workspace dep] |
| serde | 1.0 (workspace, features: derive) | Data structures | Required -- scenario/report structs [VERIFIED: workspace dep] |
| reqwest | 0.12 (features: json) | HTTP client for e2e requests | Required -- sending requests through TestProxy [VERIFIED: rigor-harness uses 0.12] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| In-process bench execution | Shell out to `cargo bench` | Shell-out is simpler and reuses existing criterion setup; in-process would duplicate benchmark wiring |
| HTML report via template engine | Plain string formatting | String formatting is adequate for a simple report; no need for tera/askama for this scope |
| YAML suite files for e2e | Hardcoded scenarios | Hardcoded scenarios are simpler for initial implementation; YAML-driven suites can be added later |

**Installation (additions to rigor-test/Cargo.toml):**
```toml
[dependencies]
clap = { workspace = true }
anyhow = { workspace = true }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "process"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
reqwest = { version = "0.12", features = ["json"], default-features = false }
rigor-harness = { path = "../rigor-harness" }
rigor = { path = "../rigor" }
```

## Architecture Patterns

### System Architecture Diagram

```
rigor-test CLI (clap)
       |
       +--[e2e]------> tokio::main
       |                    |
       |                    +---> MockLlmServerBuilder::new()
       |                    |         .anthropic_chunks(text)
       |                    |         .build().await
       |                    |
       |                    +---> TestProxy::start_with_mock(yaml, mock.url())
       |                    |
       |                    +---> reqwest::Client
       |                    |         .post(proxy.url()/v1/messages)
       |                    |         .json(body).send().await
       |                    |
       |                    +---> Assert: response status, SSE content, decision
       |                    |
       |                    +---> Print: scenario results (pass/fail)
       |
       +--[bench]----> std::process::Command
       |                    |
       |                    +---> cargo bench -p rigor [--bench <name>]
       |                    |     (existing criterion benches)
       |                    |
       |                    +---> Forward stdout/stderr, propagate exit code
       |
       +--[report]---> Read --input JSONL file
                            |
                            +---> Parse each line as JSON
                            |
                            +---> Aggregate: pass/fail/skip counts, durations
                            |
                            +---> Write --output HTML file (or stdout summary)
```

### Recommended Project Structure
```
crates/rigor-test/
├── Cargo.toml           # Add tokio, rigor-harness, rigor, serde_json, reqwest
├── src/
│   ├── main.rs          # Clap skeleton (EXISTS, modify match arms)
│   ├── e2e.rs           # E2E scenario runner
│   ├── bench.rs         # Benchmark dispatcher
│   └── report.rs        # JSONL reader + HTML report writer
└── tests/
    └── smoke.rs         # Smoke tests for each subcommand
```

### Pattern 1: E2E Scenario Runner
**What:** Launch MockLlmServer + TestProxy in-process, run a canned scenario, verify decision
**When to use:** `rigor-test e2e` (with or without --suite)
**Example:**
```rust
// Source: derived from existing integration test patterns in crates/rigor/tests/b1_kill_switch.rs
use rigor_harness::{MockLlmServerBuilder, TestProxy};

pub async fn run_e2e(suite: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    // Default built-in scenario: clean request through proxy
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("The Rust compiler ensures memory safety through ownership.")
        .build()
        .await;

    let yaml = "constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n";
    let proxy = TestProxy::start_with_mock(yaml, &mock.url()).await;

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "stream": true,
        "messages": [{"role": "user", "content": "Tell me about Rust"}]
    });

    let resp = client
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&body)
        .send()
        .await?;

    anyhow::ensure!(resp.status().is_success(), "Proxy returned {}", resp.status());

    let resp_body = resp.text().await?;
    let events = rigor_harness::parse_sse_events(&resp_body);
    let text = rigor_harness::extract_text_from_sse(&events, rigor_harness::SseFormat::Anthropic);

    anyhow::ensure!(!text.is_empty(), "Expected non-empty response text");

    println!("PASS: e2e clean-passthrough scenario");
    Ok(())
}
```

### Pattern 2: Benchmark Dispatcher
**What:** Shell out to `cargo bench -p rigor` with optional profile/suite filtering
**When to use:** `rigor-test bench`
**Example:**
```rust
// Source: standard pattern for delegating to cargo subcommands
use std::process::Command;

pub fn run_bench(suite: Option<std::path::PathBuf>, profile: &str) -> anyhow::Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg("bench").arg("-p").arg("rigor");

    if let Some(suite_path) = suite {
        // If a specific bench name is given via --suite
        let bench_name = suite_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("hook_latency");
        cmd.arg("--bench").arg(bench_name);
    }

    if profile == "quick" {
        cmd.arg("--").arg("--quick");
    }

    let status = cmd.status()?;
    anyhow::ensure!(status.success(), "cargo bench exited with {}", status);
    Ok(())
}
```

### Pattern 3: JSONL Report Generator
**What:** Read a JSONL events file, aggregate results, write HTML report
**When to use:** `rigor-test report --input events.jsonl --output report.html`
**Example:**
```rust
// Source: follows harness-runs.jsonl format from scripts/harness/common.sh
use std::io::{BufRead, BufReader};

#[derive(serde::Deserialize)]
struct HarnessEvent {
    ts: String,
    tier: String,
    path: String,
    outcome: String,
    duration_ms: u64,
}

pub fn run_report(input: std::path::PathBuf, output: std::path::PathBuf) -> anyhow::Result<()> {
    let file = std::fs::File::open(&input)?;
    let reader = BufReader::new(file);

    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        match serde_json::from_str::<HarnessEvent>(&line) {
            Ok(ev) => events.push(ev),
            Err(e) => eprintln!("warn: skipping malformed line: {}", e),
        }
    }

    let total = events.len();
    let passed = events.iter().filter(|e| e.outcome == "pass").count();
    let failed = events.iter().filter(|e| e.outcome == "fail").count();
    let skipped = events.iter().filter(|e| e.outcome.starts_with("skip") || e.outcome == "non-rust" || e.outcome == "disabled").count();
    let total_duration: u64 = events.iter().map(|e| e.duration_ms).sum();

    let html = format!(r#"<!DOCTYPE html>
<html><head><title>rigor-test report</title>
<style>
body {{ font-family: system-ui; max-width: 800px; margin: 2em auto; }}
.pass {{ color: green; }} .fail {{ color: red; }} .skip {{ color: gray; }}
table {{ border-collapse: collapse; width: 100%; }}
td, th {{ border: 1px solid #ddd; padding: 8px; text-align: left; }}
</style></head><body>
<h1>rigor-test Report</h1>
<p>Input: <code>{input}</code></p>
<p>Total: {total} | <span class="pass">Pass: {passed}</span> | <span class="fail">Fail: {failed}</span> | <span class="skip">Skip: {skipped}</span> | Duration: {total_duration}ms</p>
<table><tr><th>Time</th><th>Tier</th><th>Path</th><th>Outcome</th><th>Duration</th></tr>
{rows}
</table></body></html>"#,
        input = input.display(),
        total = total,
        passed = passed,
        failed = failed,
        skipped = skipped,
        total_duration = total_duration,
        rows = events.iter().map(|e| format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"{}\">{}</td><td>{}ms</td></tr>",
            e.ts, e.tier, e.path,
            if e.outcome == "pass" { "pass" } else if e.outcome == "fail" { "fail" } else { "skip" },
            e.outcome, e.duration_ms
        )).collect::<Vec<_>>().join("\n")
    );

    std::fs::write(&output, &html)?;
    println!("Report written to {}", output.display());
    Ok(())
}
```

### Anti-Patterns to Avoid
- **Restructuring the CLI:** The over-editing guard is explicit -- replace stubs, don't add new subcommands or restructure the existing clap skeleton. The `Cli` struct, `Commands` enum, and argument definitions must remain as-is. [CITED: CONTEXT.md over-editing guard]
- **Global env mutation in e2e:** Never use `std::env::set_var` in the e2e runner. Use `IsolatedHome` + `Command::env()` or `TestProxy` which handles env isolation internally. [CITED: codebase decision from STATE.md]
- **Adding heavy template engines:** No need for tera/askama/handlebars for the report command. Simple string formatting is sufficient. Adding a dependency for a simple HTML table is over-engineering.
- **Duplicating benchmark logic:** Do not reimplement criterion benchmarks in rigor-test. Shell out to `cargo bench -p rigor` which already has 4 well-defined benchmarks. [VERIFIED: crates/rigor/Cargo.toml [[bench]] sections]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Mock LLM server | Custom HTTP server | `MockLlmServerBuilder` from rigor-harness | Already handles Anthropic + OpenAI SSE, request tracking, shutdown [VERIFIED: mock_llm.rs] |
| Proxy bring-up with isolation | Manual DaemonState + env setup | `TestProxy::start_with_mock()` | Handles env isolation, ephemeral port, graceful shutdown [VERIFIED: proxy.rs] |
| SSE parsing | Custom SSE parser | `parse_sse_events()` + `extract_text_from_sse()` | Already handles both formats, edge cases [VERIFIED: sse.rs] |
| Benchmarking framework | Custom timing code | Shell out to `cargo bench` (criterion) | 4 benchmarks already exist with regression tracking [VERIFIED: crates/rigor/benches/] |
| Isolated HOME | TempDir + manual setup | `IsolatedHome::new()` | Creates .rigor/ subdir, provides write_rigor_yaml, home_str [VERIFIED: home.rs] |
| HTTP client for proxy requests | Raw TCP | `reqwest::Client` | Already a transitive dep via rigor-harness; handles JSON, headers [VERIFIED: rigor-harness Cargo.toml] |

**Key insight:** Phase 7 already built ALL the test primitives. Phase 14 is pure wiring -- connecting those primitives to the rigor-test CLI subcommands. Zero new infrastructure is needed.

## Common Pitfalls

### Pitfall 1: Forgetting tokio runtime for e2e
**What goes wrong:** MockLlmServer and TestProxy are async. Calling them from a sync `main()` fails.
**Why it happens:** Current rigor-test main.rs is sync (`fn main() -> Result<()>`).
**How to avoid:** Add `#[tokio::main]` to main, or wrap async calls in `tokio::runtime::Runtime::new()?.block_on()`. The simpler approach is making main async.
**Warning signs:** Compile error: "there is no reactor running" or "must be called from a tokio runtime".

### Pitfall 2: Env var races in e2e scenarios
**What goes wrong:** TestProxy uses `spawn_blocking` + env var mutation for RIGOR_HOME. Parallel scenarios race.
**Why it happens:** TestProxy::start_with_mock internally sets/restores RIGOR_HOME under a mutex.
**How to avoid:** Run e2e scenarios sequentially (they are fast). The ENV_MUTEX inside TestProxy handles internal concurrency but rigor-test should avoid spawning multiple proxies simultaneously.
**Warning signs:** Flaky test failures mentioning wrong config or missing rigor.yaml.

### Pitfall 3: Over-scoping the report command
**What goes wrong:** Building a full dashboard/visualization when a simple HTML table suffices.
**Why it happens:** "report" sounds like it should be comprehensive.
**How to avoid:** REQ-026 only requires "real implementations with passing smoke tests." A report that reads JSONL and writes a summary HTML table satisfies this. The input format matches the existing `harness-runs.jsonl` produced by `scripts/harness/common.sh`.
**Warning signs:** Pulling in charting libraries, spending time on CSS, adding multiple output formats.

### Pitfall 4: Breaking the over-editing guard
**What goes wrong:** Hook rejects edits that add new subcommands, rename existing ones, or change CLI structure.
**Why it happens:** `scripts/harness/overedit-guard.sh` is a PreToolUse hook on Edit/Write.
**How to avoid:** Only modify the match arms in main.rs (lines 52-59) to call real functions instead of `anyhow::bail!`. Keep the Cli struct, Commands enum, and arg definitions unchanged. Add new files (e2e.rs, bench.rs, report.rs) rather than bloating main.rs.
**Warning signs:** Over-edit guard hook failure during Write/Edit operations.

### Pitfall 5: Missing smoke test coverage
**What goes wrong:** REQ-026 explicitly requires "passing smoke tests" alongside real implementations.
**Why it happens:** Focusing only on the implementation and forgetting the test half of the requirement.
**How to avoid:** Write `crates/rigor-test/tests/smoke.rs` with at least one test per subcommand. For `e2e`: run the built-in scenario in-process. For `bench`: verify `cargo bench -p rigor --bench hook_latency -- --quick` exits 0. For `report`: create a temp JSONL file, run report, verify HTML output exists.
**Warning signs:** `cargo test -p rigor-test` runs zero tests (the exact problem Issue #6 filed about).

## Code Examples

### Existing harness usage pattern (from b1_kill_switch.rs)
```rust
// Source: crates/rigor/tests/b1_kill_switch.rs lines 72-103
// This is the proven pattern for launching MockLlm + TestProxy + making a request
let mock = MockLlmServerBuilder::new()
    .anthropic_chunks(VIOLATION_TEXT)
    .build()
    .await;
let proxy = TestProxy::start_with_mock(BLOCK_CONSTRAINT_YAML, &mock.url()).await;
let body = anthropic_request_body(true, "Tell me something");
let resp = proxy_post(&proxy.url(), &body).await;
let resp_body = resp.text().await.unwrap();
```

### Existing benchmark pattern (from hook_latency.rs)
```rust
// Source: crates/rigor/benches/hook_latency.rs lines 1-18
// Four existing criterion benchmarks exist. rigor-test bench should shell out to them.
// Benchmark names: hook_latency, evaluation_only, dfquad_scaling, filter_chain_overhead
use criterion::{criterion_group, criterion_main, Criterion};
```

### Existing JSONL event format (from .harness/logs/harness-runs.jsonl)
```json
{"ts":"2026-04-23T21:48:39Z","tier":"skip","path":"README.md","outcome":"non-rust","duration_ms":0,"extra":{"reason":"non-rust file"}}
{"ts":"2026-04-23T21:48:39Z","tier":"tier-0","path":"src/main.rs","outcome":"pass","duration_ms":3200}
```

### Minimal valid rigor.yaml (from multiple test files)
```yaml
# Source: crates/rigor/tests/harness_smoke.rs line 12
constraints:
  beliefs: []
  justifications: []
  defeaters: []
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| rigor-harness was empty (9 lines) | Full library: IsolatedHome, TestCA, MockLlmServer, TestProxy, SSE, subprocess | Phase 7 (completed) | E2E scenarios can now be built by composing existing primitives |
| rigor-test subcommands bail | Phase 14 replaces stubs | This phase | REQ-026 satisfied |
| Manual benchmark invocation | `cargo bench -p rigor` with 4 criterion benches | Phase 0 / ongoing | rigor-test bench simply shells out |

**Deprecated/outdated:**
- rigor-harness was documented as "9 lines of comment, no code" in Issue #6 -- this is now outdated. Phase 7 fully implemented all harness primitives. [VERIFIED: current rigor-harness source has ~600 lines across 6 modules]

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The report command should read the same JSONL format as `.harness/logs/harness-runs.jsonl` | Architecture Patterns (Pattern 3) | LOW -- if different format expected, just change the deserialize struct. The fields (ts, tier, path, outcome, duration_ms) match observed data. |
| A2 | `cargo bench -p rigor -- --quick` is a valid criterion flag for fast benchmark runs | Architecture Patterns (Pattern 2) | LOW -- criterion 0.5 supports `--quick` to reduce sample count. If not, remove the flag. |
| A3 | HTML is the appropriate output format for the report command (vs. plaintext terminal) | Architecture Patterns (Pattern 3) | LOW -- the CLI arg is `--output: PathBuf` suggesting file output; HTML is natural for file reports. Plaintext to stdout would also satisfy REQ-026. |

## Open Questions

1. **Should `e2e` support YAML-driven scenario suites?**
   - What we know: The CLI has `--suite: Option<PathBuf>` arg. No YAML suite format exists yet (no .yaml test suites found in repo).
   - What's unclear: Whether to implement YAML suite loading or just use built-in hardcoded scenarios.
   - Recommendation: Start with built-in scenarios (no --suite). Print a helpful message when --suite is provided: "YAML suite loading not yet available; running built-in scenarios." This satisfies REQ-026 (real implementation with smoke tests) without inventing a suite format.

2. **What should the bench `--profile` flag do?**
   - What we know: CLI has `--profile: String` defaulting to "quick". Criterion supports `--quick` and `--sample-size`.
   - What's unclear: Whether "quick" vs other profiles should map to different criterion flags.
   - Recommendation: Map "quick" to `--quick` flag, "full" (or anything else) to no extra flags. Simple and sufficient.

3. **Should e2e include a violation scenario alongside the clean-passthrough?**
   - What we know: The B1/B2/B3 integration tests already cover violation scenarios thoroughly.
   - What's unclear: Whether rigor-test e2e should duplicate these or just prove the happy path works.
   - Recommendation: Include both a clean-passthrough and a simple violation scenario. Two scenarios demonstrate the e2e command exercises the real pipeline, not just HTTP transport.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) + tokio::test for async |
| Config file | crates/rigor-test/Cargo.toml (add dev-dependencies) |
| Quick run command | `cargo test -p rigor-test` |
| Full suite command | `cargo test -p rigor-test --no-fail-fast` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REQ-026 (e2e) | e2e subcommand runs built-in scenario and exits 0 | integration | `cargo test -p rigor-test --test smoke test_e2e_smoke -x` | Wave 0 |
| REQ-026 (bench) | bench subcommand invokes cargo bench and exits 0 | integration | `cargo test -p rigor-test --test smoke test_bench_smoke -x` | Wave 0 |
| REQ-026 (report) | report subcommand reads JSONL and writes HTML | integration | `cargo test -p rigor-test --test smoke test_report_smoke -x` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p rigor-test`
- **Per wave merge:** `cargo test -p rigor-test --no-fail-fast && cargo test -p rigor-harness`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/rigor-test/tests/smoke.rs` -- covers REQ-026 (all 3 subcommands)
- [ ] Add `tokio`, `rigor-harness`, `rigor`, `serde_json`, `reqwest` to rigor-test Cargo.toml dependencies
- [ ] Add `tempfile` to rigor-test dev-dependencies for smoke tests

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A (dev-only tool, not shipped to users) |
| V3 Session Management | no | N/A |
| V4 Access Control | no | N/A |
| V5 Input Validation | yes (minimal) | Validate JSONL parsing in report command; reject malformed lines gracefully |
| V6 Cryptography | no | N/A (TestCA is ephemeral test-only) |

### Known Threat Patterns for dev-only test tooling

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Path traversal via --input/--output | Tampering | Validate paths exist; do not follow symlinks into sensitive directories. Low risk since dev-only. |
| Env var leakage | Information Disclosure | IsolatedHome already prevents touching real ~/.rigor/. TestProxy uses env mutex. |

## Sources

### Primary (HIGH confidence)
- `crates/rigor-test/src/main.rs` -- current stub implementation (63 lines, all 3 subcommands bail)
- `crates/rigor-test/Cargo.toml` -- current dependencies (clap + anyhow only)
- `crates/rigor-harness/src/` -- all 6 modules: home.rs, ca.rs, mock_llm.rs, proxy.rs, sse.rs, subprocess.rs
- `crates/rigor/Cargo.toml` -- criterion 0.5 benchmarks (4 defined: hook_latency, evaluation_only, dfquad_scaling, filter_chain_overhead)
- `crates/rigor/tests/b1_kill_switch.rs` -- proven pattern for MockLlm + TestProxy composition
- `crates/rigor/tests/harness_smoke.rs` -- proven pattern for IsolatedHome + subprocess testing
- `.harness/logs/harness-runs.jsonl` -- actual JSONL event format (ts, tier, path, outcome, duration_ms, extra)
- GitHub Issue #6 -- original problem statement and scope

### Secondary (MEDIUM confidence)
- `Cargo.toml` (workspace) -- workspace dependency versions verified via cargo metadata
- `.planning/roadmap/pr-2.7-test-coverage-plan.md` -- design context for test tiers

### Tertiary (LOW confidence)
- None -- all findings verified from codebase

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries already in workspace, versions verified from Cargo.toml
- Architecture: HIGH -- all primitives (MockLlmServer, TestProxy, SSE helpers) already implemented and battle-tested in Phase 7/12 integration tests
- Pitfalls: HIGH -- pitfalls derived from actual codebase patterns (env mutex, over-edit guard hook, existing test patterns)

**Research date:** 2026-04-24
**Valid until:** 2026-05-24 (stable -- rigor-harness API is unlikely to change)
