# Phase 8: `$HOME/.rigor` test isolation - Research

**Researched:** 2026-04-24
**Domain:** Rust path indirection, test isolation, CI guardrails
**Confidence:** HIGH

## Summary

Phase 8 introduces a `rigor_home()` indirection function that checks the `RIGOR_HOME` env var before falling back to `dirs::home_dir()/.rigor/`. All 21 production call sites that construct `~/.rigor/` paths must route through this function. The existing Phase 7 `IsolatedHome` fixture already creates TempDir + `.rigor/` subdirectories; after this phase, it can set `RIGOR_HOME` instead of the current unsafe `std::env::set_var("HOME", ...)` pattern used in TestProxy.

The scope is deliberately narrow: one new function, mechanical call-site updates, and a CI grep guard. No refactoring, no feature additions, no structural changes. The over-editing guard from the issue is paramount.

**Primary recommendation:** Add `pub fn rigor_home() -> PathBuf` to a new `crates/rigor/src/paths.rs` module. Check `RIGOR_HOME` first, fall back to `dirs::home_dir().join(".rigor")`. Replace all 17 `.rigor/`-constructing call sites. Leave 4 non-`.rigor/` HOME usages (2x `.claude/settings.json`, 1x `Library/Keychains/`, 1x `is_rigor_bin_in_path`) untouched.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
None explicitly locked -- all implementation choices are at Claude's discretion.

### Claude's Discretion
All implementation choices are at Claude's discretion. Key guidance from GitHub issue #15:

- Introduce `rigor_home()` function with `RIGOR_HOME` env var override (or `RigorPaths` struct)
- Update call sites: `daemon/mod.rs` (daemon_pid_file, daemon_alive), `daemon/tls.rs` (ca_cert_path, ca_key_path), logging.rs, session log writers
- Add CI grep that fails on new `dirs::home_dir()` / raw `$HOME` reads in `crates/rigor/src/`
- Use Phase 7's IsolatedHome from rigor-harness for test fixtures

### Deferred Ideas (OUT OF SCOPE)
None -- discuss phase skipped.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REQ-018 | No test writes to real `$HOME/.rigor/`. All tests touching daemon lifecycle, CA cert, or violations log use a `TempDir`-based fixture from the test-support library (REQ-015). Verified by a CI check that greps test output for the real `$HOME` path. | rigor_home() indirection + RIGOR_HOME env var enables IsolatedHome to redirect all paths without unsafe global env mutation. CI grep guard prevents regression. |
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Path resolution (`rigor_home()`) | Library (crates/rigor/src) | -- | All production code that constructs `~/.rigor/` paths lives in the rigor crate; the function must be in the same crate |
| Test isolation fixture | Test harness (crates/rigor-harness) | -- | IsolatedHome already exists in rigor-harness; no changes needed there beyond using RIGOR_HOME instead of HOME |
| CI guard | CI (.github/workflows) | -- | Grep-based check runs in CI pipeline, separate from Rust code |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| dirs | 5.0.1 (in Cargo.lock) | `home_dir()` fallback in `rigor_home()` | Already used throughout codebase [VERIFIED: Cargo.lock] |
| tempfile | 3.x | TempDir for test isolation | Already a dev-dependency [VERIFIED: Cargo.toml] |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| rigor-harness | workspace | IsolatedHome, TestProxy | All integration tests touching `~/.rigor/` paths |

No new dependencies are needed. This phase uses only what is already in the workspace.

## Architecture Patterns

### System Architecture Diagram

```
Production path resolution:
  caller code  --->  rigor_home()  --->  RIGOR_HOME env var set?
                                              |           |
                                             YES          NO
                                              |           |
                                        PathBuf::from   dirs::home_dir()
                                        (RIGOR_HOME)      .join(".rigor")
                                              |           |
                                              v           v
                                         <resolved PathBuf>
                                              |
                                    caller appends subpath
                                    (e.g., "daemon.pid", "ca.pem")

Test path resolution:
  IsolatedHome::new() ---> TempDir ---> set RIGOR_HOME = tempdir/.rigor
                                              |
  test code calls production code --->  rigor_home() picks up RIGOR_HOME
                                              |
                                         all I/O goes to TempDir
```

### Recommended Module Location

```
crates/rigor/src/
  paths.rs           # NEW: rigor_home() + rigor_home_subpath()
  lib.rs             # Add: pub mod paths;
  daemon/
    mod.rs           # UPDATE: daemon_pid_file() uses paths::rigor_home()
    tls.rs           # UPDATE: ca_cert_path(), ca_key_path() use paths::rigor_home()
  logging/
    violation_log.rs # UPDATE: ViolationLogger::new() uses paths::rigor_home()
    session_registry.rs # UPDATE: registry_path(), session_log_dir() use paths::rigor_home()
  observability/
    tracing.rs       # UPDATE: init_tracing() uses paths::rigor_home()
  alerting/
    mod.rs           # UPDATE: alerts_path() uses paths::rigor_home()
  memory/
    episodic.rs      # UPDATE: MemoryStore::path() uses paths::rigor_home()
  cli/
    config.rs        # UPDATE: config_path() uses paths::rigor_home()
    serve.rs         # UPDATE: serve_pid_file(), run_background() use paths::rigor_home()
    refine.rs        # UPDATE: rigor_dir() uses paths::rigor_home()
    eval.rs          # UPDATE: rigor_dir() uses paths::rigor_home()
    trust.rs         # UPDATE: rigor_bin_dir(), ensure_ca_bundle(), is_rigor_bin_in_path() use paths::rigor_home()
```

### Pattern 1: The `rigor_home()` Function

**What:** A single function that returns the `~/.rigor/` directory path, checking `RIGOR_HOME` env var first.
**When to use:** Every time production code needs a path under `~/.rigor/`.
**Example:**

```rust
// crates/rigor/src/paths.rs
use std::path::PathBuf;

/// Returns the rigor data directory (`~/.rigor/` by default).
///
/// Resolution order:
/// 1. `RIGOR_HOME` env var (if set and non-empty)
/// 2. `dirs::home_dir()/.rigor/` (production fallback)
/// 3. Panics if neither is available (home_dir returns None and RIGOR_HOME unset)
///
/// Tests set `RIGOR_HOME` to a TempDir to avoid touching the real `~/.rigor/`.
pub fn rigor_home() -> PathBuf {
    if let Ok(val) = std::env::var("RIGOR_HOME") {
        if !val.is_empty() {
            return PathBuf::from(val);
        }
    }
    dirs::home_dir()
        .expect("Cannot determine home directory")
        .join(".rigor")
}
```

[VERIFIED: codebase grep confirms 17 .rigor/ path constructions and 4 non-.rigor/ HOME usages]

### Pattern 2: Call Site Replacement (Mechanical)

**What:** Replace each `dirs::home_dir().join(".rigor")` or equivalent with `crate::paths::rigor_home()`.
**Example before:**

```rust
fn ca_cert_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".rigor")
        .join("ca.pem")
}
```

**Example after:**

```rust
fn ca_cert_path() -> PathBuf {
    crate::paths::rigor_home().join("ca.pem")
}
```

### Anti-Patterns to Avoid

- **Over-editing surrounding code:** The issue explicitly warns against this. Change ONLY the path resolution mechanism. Do not refactor error handling, rename variables, restructure modules, or add features.
- **Threading a RigorPaths struct:** The issue mentions this as an alternative but it would require touching every function signature in the call chain. The env-var approach is far less invasive and matches the existing pattern (RIGOR_TARGET_API, RIGOR_DEBUG, etc.).
- **Using `std::env::set_var("HOME", ...)` in tests:** This is unsafe in Rust 2024 edition and already causes race conditions with parallel tests. The `RIGOR_HOME` env var avoids this entirely for `.rigor/` paths.

## Complete Call-Site Inventory

### Category A: `.rigor/` path construction (MUST update -- 17 sites)

| # | File | Line | Current Code | Subpath |
|---|------|------|-------------|---------|
| A1 | daemon/mod.rs | 25 | `env::var_os("HOME").map(\|h\| PathBuf::from(h).join(".rigor/daemon.pid"))` | `daemon.pid` |
| A2 | daemon/tls.rs | 20 | `dirs::home_dir()...join(".rigor").join("ca.pem")` | `ca.pem` |
| A3 | daemon/tls.rs | 27 | `dirs::home_dir()...join(".rigor").join("ca-key.pem")` | `ca-key.pem` |
| A4 | logging/violation_log.rs | 23 | `dirs::home_dir()...join(".rigor")` | `violations.jsonl` |
| A5 | logging/session_registry.rs | 55 | `dirs::home_dir().map(\|h\| h.join(".rigor/sessions.jsonl"))` | `sessions.jsonl` |
| A6 | logging/session_registry.rs | 60 | `dirs::home_dir().map(\|h\| h.join(".rigor/sessions/{id}"))` | `sessions/{id}` |
| A7 | observability/tracing.rs | 13 | `dirs::home_dir().map(\|h\| h.join(".rigor"))` | `rigor.log` |
| A8 | alerting/mod.rs | 63 | `dirs::home_dir()...join(".rigor")` | `alerts.json` |
| A9 | memory/episodic.rs | 69 | `dirs::home_dir()...join(".rigor")` | `memory.json` |
| A10 | cli/config.rs | 14 | `env::var("HOME")...join(".rigor").join("config")` | `config` |
| A11 | cli/serve.rs | 30 | `dirs::home_dir().map(\|h\| h.join(".rigor/serve.pid"))` | `serve.pid` |
| A12 | cli/serve.rs | 142 | `dirs::home_dir().map(\|h\| h.join(".rigor/serve.log"))` | `serve.log` |
| A13 | cli/refine.rs | 45 | `dirs::home_dir()...join(".rigor")` | `refinements.jsonl` |
| A14 | cli/eval.rs | 152 | `dirs::home_dir()...join(".rigor")` | `eval-baseline.json` |
| A15 | cli/trust.rs | 19 | `dirs::home_dir()...join(".rigor/bin")` | `bin/` |
| A16 | cli/trust.rs | 97 | `dirs::home_dir()...join(".rigor")` | `ca.pem`, `ca-bundle.pem` |
| A17 | cli/trust.rs | 64 | `dirs::home_dir().map(\|h\| h.join(".rigor/bin")...)` | `bin/` (for PATH check) |

### Category B: Non-`.rigor/` HOME usage (DO NOT change -- 4 sites)

| # | File | Line | What It Does | Why Excluded |
|---|------|------|-------------|-------------|
| B1 | cli/gate.rs | 516 | `env::var("HOME").join(".claude/settings.json")` | `.claude/` path, not `.rigor/` |
| B2 | cli/scan.rs | 190 | `env::var("HOME").join(".claude/settings.json")` | `.claude/` path, not `.rigor/` |
| B3 | daemon/tls.rs | 199 | `dirs::home_dir()...Library/Keychains/` | macOS keychain path, not `.rigor/` |
| B4 | cli/trust.rs | 49 | `dirs::home_dir()?.join(".zshrc")` / `.bashrc` | Shell profile path, not `.rigor/` |

### Signature Impact Analysis

Most call sites are local helpers that can be updated in isolation. The key functions and their return-type changes:

| Function | Current Return | New Return | Breaking? |
|----------|---------------|------------|-----------|
| `daemon_pid_file()` | `Option<PathBuf>` | `Option<PathBuf>` (no change -- rigor_home() always succeeds or panics) | See note below |
| `ca_cert_path()` | `PathBuf` | `PathBuf` | No |
| `ca_key_path()` | `PathBuf` | `PathBuf` | No |
| `registry_path()` | `Option<PathBuf>` | Can become `PathBuf` but SHOULD NOT change to avoid over-editing | No |
| `serve_pid_file()` | `Option<PathBuf>` | Same discussion | No |

**Note on `Option<PathBuf>` functions:** Several functions return `Option<PathBuf>` because `dirs::home_dir()` can return `None`. With `rigor_home()`, the home dir is always available (or panics). However, changing return types from `Option<PathBuf>` to `PathBuf` would cascade into caller code. The over-editing guard says: keep the return type, just wrap `rigor_home()` in `Some(...)`. This is the minimum viable change.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Path resolution | Custom struct threaded through all functions | `RIGOR_HOME` env var checked in one function | Env var is the standard pattern for XDG-style overrides. Struct threading touches every function signature. |
| Test HOME isolation | `unsafe { std::env::set_var("HOME", ...) }` | `RIGOR_HOME` env var via `Command::env()` or scoped `set_var` | Avoids race conditions in parallel tests. Current TestProxy already has the unsafe pattern; RIGOR_HOME makes it safe. |
| CI regression guard | Custom clippy lint plugin | `grep -rn` in CI step | Grep is simple, zero build cost, immediately understandable. A clippy plugin would be over-engineering. |

## Common Pitfalls

### Pitfall 1: Over-Editing Beyond Path Resolution
**What goes wrong:** Changing function signatures, error handling, or surrounding code while updating call sites.
**Why it happens:** Natural refactoring impulse when touching many files.
**How to avoid:** Each call site change should be exactly: replace `dirs::home_dir()...join(".rigor")` with `crate::paths::rigor_home()`. No other changes in the same diff hunk.
**Warning signs:** Changing `Option<PathBuf>` return types to `PathBuf`, modifying error messages, renaming local variables.

### Pitfall 2: CI Guard False Positives on Category B Sites
**What goes wrong:** The CI grep catches `dirs::home_dir()` in `trust.rs:49` (shell profile), `tls.rs:199` (keychain), `gate.rs:516` (`.claude/`), `scan.rs:190` (`.claude/`).
**Why it happens:** Grep is too broad -- it catches all `dirs::home_dir()` / `env::var("HOME")` regardless of whether the result targets `.rigor/`.
**How to avoid:** The CI grep should either:
  - Exclude specific files/lines with allowlist comments (e.g., `# rigor-home-ok`)
  - Use a more targeted pattern that checks for `.rigor` in the same context
  - Only flag NEW occurrences via `git diff` rather than scanning all existing code
**Warning signs:** CI fails on existing code that legitimately uses HOME for non-`.rigor/` paths.

### Pitfall 3: TestProxy Still Needs `set_var` for HOME
**What goes wrong:** Assuming RIGOR_HOME fully eliminates the need for HOME manipulation in TestProxy.
**Why it happens:** TestProxy runs `DaemonState::load` in-process, and `DaemonState::load` calls `RigorCA::load_or_generate()` which calls `ca_cert_path()`. After this phase, `ca_cert_path()` uses `rigor_home()` which reads `RIGOR_HOME`. But `RIGOR_HOME` must be set as a process env var (not just subprocess env) for in-process code.
**How to avoid:** TestProxy should set `RIGOR_HOME` (not `HOME`) with the same `spawn_blocking + set_var/restore` pattern. This is still technically unsafe but scoped to the blocking thread, and it only affects the `.rigor/` paths, not the entire HOME. Alternatively, if the env var approach is unacceptable, refactor TestProxy to use subprocess-based daemon startup instead.
**Warning signs:** Tests pass individually but fail when run in parallel.

### Pitfall 4: Panic vs. Graceful Error in `rigor_home()`
**What goes wrong:** If `rigor_home()` panics when HOME is unset and RIGOR_HOME is unset, it crashes the daemon.
**Why it happens:** `dirs::home_dir()` returns `None` in some CI environments or containers.
**How to avoid:** Use `expect()` with a clear message, or return `Result<PathBuf>` and let callers handle it. Since most callers already handle `Option`/`Result`, either approach works. The minimum-change path uses `expect()` because changing to `Result` would modify function signatures.
**Warning signs:** CI crashes in containerized environments where HOME is unset.

### Pitfall 5: Forgetting `create_dir_all` After Switching to `rigor_home()`
**What goes wrong:** `rigor_home()` returns a path that doesn't exist yet. Callers that previously called `create_dir_all` on `home.join(".rigor")` now get the dir from `rigor_home()` but forget to create it.
**Why it happens:** Some callers (`ViolationLogger::new`, `alerting::alerts_path`, `MemoryStore::path`) currently do `create_dir_all(&rigor_dir)` after constructing the path. After the switch, they must still call `create_dir_all(rigor_home())`.
**How to avoid:** Audit each call site to preserve existing `create_dir_all` calls. The minimum-change approach: keep the `create_dir_all` exactly where it was, just change the path it creates.
**Warning signs:** "No such file or directory" errors when RIGOR_HOME is set to a non-existent path.

## Code Examples

### The `rigor_home()` Function

```rust
// crates/rigor/src/paths.rs
// Source: designed for this codebase, following XDG_*_HOME env var pattern

use std::path::PathBuf;

/// Returns the rigor data directory.
///
/// Resolution order:
/// 1. `RIGOR_HOME` environment variable (if set and non-empty)
/// 2. `$HOME/.rigor/` via `dirs::home_dir()`
///
/// Panics if neither is available. In practice, HOME is always set on
/// macOS and Linux; RIGOR_HOME is set by test fixtures.
pub fn rigor_home() -> PathBuf {
    if let Ok(val) = std::env::var("RIGOR_HOME") {
        if !val.is_empty() {
            return PathBuf::from(val);
        }
    }
    dirs::home_dir()
        .expect("Cannot determine home directory (set RIGOR_HOME or HOME)")
        .join(".rigor")
}
```

### Call Site Update Example (daemon/tls.rs)

```rust
// Before:
fn ca_cert_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".rigor")
        .join("ca.pem")
}

// After:
fn ca_cert_path() -> PathBuf {
    crate::paths::rigor_home().join("ca.pem")
}
```

### Call Site Update Example (daemon/mod.rs -- Option-returning)

```rust
// Before:
pub fn daemon_pid_file() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".rigor/daemon.pid"))
}

// After (minimum change -- preserve Option return type):
pub fn daemon_pid_file() -> Option<PathBuf> {
    Some(crate::paths::rigor_home().join("daemon.pid"))
}
```

### CI Grep Guard

```yaml
# .github/workflows/ci.yml -- new job or step
- name: Guard against raw HOME usage for .rigor paths
  run: |
    # Find dirs::home_dir() or env::var("HOME") usages in production code
    # that construct .rigor/ paths. The rigor_home() indirection in paths.rs
    # is the only allowed call site.
    VIOLATIONS=$(grep -rn 'dirs::home_dir\|env::var("HOME")\|env::var_os("HOME")' \
      crates/rigor/src/ \
      --include='*.rs' \
      | grep -v 'src/paths.rs' \
      | grep -v '// rigor-home-ok' \
      || true)
    if [ -n "$VIOLATIONS" ]; then
      echo "ERROR: Raw HOME access found outside paths.rs."
      echo "Use crate::paths::rigor_home() instead."
      echo ""
      echo "$VIOLATIONS"
      exit 1
    fi
```

**Note on allowlisting:** The 4 Category B sites (gate.rs, scan.rs, tls.rs, trust.rs) that legitimately access HOME for non-`.rigor/` purposes should get a `// rigor-home-ok` comment to pass the guard. This is cheaper than a complex regex that tries to distinguish `.rigor/` from `.claude/` paths.

### IsolatedHome Integration

```rust
// In TestProxy (crates/rigor-harness/src/proxy.rs), after this phase:
tokio::task::spawn_blocking(move || {
    let original = std::env::var("RIGOR_HOME").ok();
    // Set RIGOR_HOME instead of HOME -- only affects .rigor/ paths
    unsafe { std::env::set_var("RIGOR_HOME", &home_str) };
    let result = rigor::daemon::DaemonState::load(yaml_path, event_tx);
    match original {
        Some(h) => unsafe { std::env::set_var("RIGOR_HOME", h) },
        None => unsafe { std::env::remove_var("RIGOR_HOME") },
    }
    result.expect("DaemonState::load failed")
})
```

This is still technically unsafe (process-wide env mutation) but is narrower than mutating HOME. A future improvement could use a `OnceCell` or `thread_local!` but that exceeds phase scope.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `dirs::home_dir().join(".rigor")` sprinkled across 17 sites | `rigor_home()` with `RIGOR_HOME` env var override | This phase | Enables test isolation without unsafe HOME mutation |
| `unsafe { std::env::set_var("HOME", ...) }` in TestProxy | `RIGOR_HOME` scoped env var | This phase | Reduces blast radius of env mutation from all-of-HOME to just .rigor paths |

**Deprecated/outdated:**
- `std::env::set_var` is `unsafe` since Rust 1.66 (stable since 2022-12). The current TestProxy uses it inside `spawn_blocking` which is acceptable but fragile. [VERIFIED: Rust 1.66 release notes]

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `rigor_home()` should panic on failure rather than returning `Result<PathBuf>` | Code Examples | If we return Result, all 17 call sites need error handling changes, which violates the over-editing guard. Panic is acceptable since HOME is always set in practice. |
| A2 | Category B sites (gate.rs, scan.rs, tls.rs, trust.rs) should be allowlisted with comments, not converted to use a generic home_dir helper | Call-Site Inventory | If the user wants ALL HOME references centralized, the scope grows significantly. The issue specifically says "rigor_home()" not "all home_dir usage." |
| A3 | TestProxy should switch from mutating HOME to mutating RIGOR_HOME | Code Examples | If RIGOR_HOME is not picked up by some transitive dependency that also reads HOME for .rigor paths, tests could still leak. But since rigor_home() is the only .rigor path resolver after this phase, this should be safe. |

## Open Questions

1. **Should `rigor_home()` create the directory if it doesn't exist?**
   - What we know: Many callers currently do `create_dir_all` after getting the path. If `rigor_home()` did it, callers could drop those calls. But that changes behavior (side effect in a path-resolution function).
   - What's unclear: Whether simplifying callers is worth the side effect.
   - Recommendation: Do NOT add `create_dir_all` to `rigor_home()`. Keep it a pure path resolver. Callers retain their existing `create_dir_all` calls. This minimizes behavioral changes.

2. **Should the CI grep guard be a separate job or a step in the existing clippy job?**
   - What we know: It's a one-line grep, runs in milliseconds.
   - What's unclear: Organizational preference.
   - Recommendation: Add as a step in the existing `clippy` job. No need for a separate job for a grep.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) |
| Config file | Cargo.toml (workspace) |
| Quick run command | `cargo test -p rigor --lib paths` |
| Full suite command | `cargo test --all-features` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REQ-018 | rigor_home() returns RIGOR_HOME when set | unit | `cargo test -p rigor --lib paths` | Wave 0 |
| REQ-018 | rigor_home() falls back to dirs::home_dir()/.rigor | unit | `cargo test -p rigor --lib paths` | Wave 0 |
| REQ-018 | CI grep guard catches new raw HOME usage | integration | CI workflow grep step | Wave 0 |
| REQ-018 | TestProxy uses RIGOR_HOME not HOME | integration | `cargo test -p rigor-harness` | Already exists (needs update) |

### Sampling Rate
- **Per task commit:** `cargo test -p rigor --lib paths && cargo test -p rigor-harness`
- **Per wave merge:** `cargo test --all-features`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/rigor/src/paths.rs` -- new module with rigor_home() + unit tests
- [ ] CI grep step in `.github/workflows/ci.yml`

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | -- |
| V3 Session Management | no | -- |
| V4 Access Control | no | -- |
| V5 Input Validation | yes (env var input) | Validate RIGOR_HOME is non-empty before using; no path traversal risk since it's a directory root |
| V6 Cryptography | no | -- |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Malicious RIGOR_HOME pointing to attacker-controlled dir | Tampering | RIGOR_HOME is only set by the user or test fixtures. No external untrusted input. Same trust model as HOME itself. |

## Sources

### Primary (HIGH confidence)
- Codebase grep of all 21 `dirs::home_dir()` / `env::var("HOME")` / `env::var_os("HOME")` call sites -- verified line-by-line [VERIFIED: codebase grep]
- `crates/rigor-harness/src/home.rs` -- IsolatedHome implementation [VERIFIED: file read]
- `crates/rigor-harness/src/proxy.rs` -- TestProxy with current HOME mutation pattern [VERIFIED: file read]
- `crates/rigor/Cargo.toml` -- dirs 5.0, tempfile 3 dependencies [VERIFIED: file read]
- `Cargo.lock` -- dirs 5.0.1 actual resolved version [VERIFIED: lockfile grep]
- `.github/workflows/ci.yml` -- existing CI structure with clippy, rustfmt, test jobs [VERIFIED: file read]
- Issue #15 acceptance criteria from phase description [VERIFIED: provided context]

### Secondary (MEDIUM confidence)
- XDG_*_HOME env var override pattern (e.g., XDG_CONFIG_HOME, XDG_DATA_HOME) as precedent for RIGOR_HOME [ASSUMED: standard Unix pattern]

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, verified existing versions
- Architecture: HIGH -- pure mechanical refactoring, all call sites verified line-by-line
- Pitfalls: HIGH -- based on direct code analysis of existing patterns and Rust safety rules

**Research date:** 2026-04-24
**Valid until:** 2026-05-24 (stable -- no framework version sensitivity)
