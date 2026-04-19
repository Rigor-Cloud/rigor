# Codebase Concerns

**Analysis Date:** 2026-04-19

## Tech Debt

**PII Sanitizer Silent Defect (Fixed but Pattern Risk):**
- Issue: The `PII_SANITIZER` in `crates/rigor/src/daemon/proxy.rs` was historically built with `Sanitizer::builder().build()` (zero detectors) instead of `Sanitizer::default()`, causing silent detection failures for months.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 102-150)
- Impact: Secrets in request bodies were not redacted before forwarding to upstream APIs, creating potential for credential leaks to API providers.
- Fix approach: Currently uses `Sanitizer::default()` with custom provider-specific patterns. Add unit tests to verify each detector (email, credit_card, Anthropic OAuth, etc.) fires as expected. Consider integration test that patches a request with known secrets and verifies redaction.

**Pervasive `.lock().unwrap()` Pattern:**
- Issue: 56+ instances of `.lock().unwrap()` on `Mutex<DaemonState>` throughout proxy, context, and gate handler code paths.
- Files: `crates/rigor/src/daemon/proxy.rs`, `crates/rigor/src/daemon/mod.rs`, `crates/rigor/src/daemon/context.rs`, and others
- Impact: Any panic inside the lock (from external code, malformed JSON, etc.) poisons the mutex, causing all subsequent requests to panic on lock acquisition. The daemon terminates without graceful shutdown.
- Fix approach: Replace `.unwrap()` with `.expect("DaemonState lock poisoned")` for better error visibility. Consider defensive guard: `if mtx.is_poisoned() { restart daemon }`. Alternatively, use `parking_lot::Mutex` which doesn't support poisoning. Test mutex recovery under controlled panic scenarios.

**Synchronous Lock in Async Gate Timeout Loop:**
- Issue: The retroactive gate handler (lines 791–830 in `proxy.rs`) polls `state.lock()` in a tight loop with only 500ms sleep between iterations.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 791–830)
- Impact: If a gate decision is never received, the proxy request blocks for `GATE_TIMEOUT_SECS` (appears to be ~5–10 seconds), holding a potentially poisoned lock. Under high concurrency, this cascades into thread starvation.
- Fix approach: Replace polling loop with `tokio::sync::Notify` or a condition variable. Gate decision writer signals the notify; waiter unblocks immediately. Set timeout with `tokio::time::timeout()` wrapper around the notify wait.

**Regex Pattern Compilation in Hot Path:**
- Issue: Regex patterns in `crates/rigor/src/claim/heuristic.rs` are compiled with `Lazy::new()` (fine), but the patterns are re-applied to every transcript message on every hook invocation.
- Files: `crates/rigor/src/claim/heuristic.rs` (lines 18–29), called from `crates/rigor/src/lib.rs` (claim extraction)
- Impact: Transcript messages with hundreds of sentences trigger N regex matches per sentence. For long conversations, this is O(N * M) where N = sentences, M = patterns.
- Fix approach: Compile regexes once globally (already done). Profile claim extraction time on large transcripts (100+ messages). Consider caching sentence boundaries and hedge detection results if profiling shows >5ms spend.

**Hardcoded MITM Host Allowlist:**
- Issue: `MITM_HOSTS` constant in `crates/rigor/src/daemon/mod.rs` is hardcoded (82–98). New API providers must be code-modified and recompiled.
- Files: `crates/rigor/src/daemon/mod.rs` (lines 82–98)
- Impact: Cannot dynamically add providers without rebuild. Users of custom LLM endpoints (self-hosted, enterprise APIs) cannot use MITM interception without editing source.
- Fix approach: Load `MITM_HOSTS` from `~/.rigor/config` or environment variable `RIGOR_MITM_HOSTS` (comma-separated). Fall back to hardcoded list if not configured. Add a `rigor config set mitm-hosts <hosts>` command.

## Known Bugs

**PII Sanitizer False Negatives on Entropy Edge Cases:**
- Symptoms: API keys with irregular character distributions or novel prefixes are not detected.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 102–150)
- Trigger: A custom provider with prefix `sk-custom-xyz...` or a GitHub token variant not matching the regex.
- Workaround: Add custom detector via `.custom("CustomProvider", r"...")` in the sanitizer builder.

**Gate Timeout Auto-Reject Loses Context:**
- Symptoms: When a retroactive gate times out, the log message "Retroactive gate {id} timeout — auto-rejected" is emitted, but the original request details (which constraint, why the gate was requested) are not included.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 823–824)
- Trigger: Gate requested by action rule; user does not approve within timeout window.
- Workaround: None — the decision is logged but context is lost.

## Security Considerations

**TLS MITM via LD_PRELOAD Attack Surface:**
- Risk: The `layer/src/lib.rs` uses `frida-gum` for inline function hooking (getaddrinfo, connect, SecTrustEvaluateWithError). This enables MITM of any TLS connection by injecting a fake CA certificate and redirecting DNS to localhost.
- Files: `layer/src/lib.rs` (entire crate), particularly:
  - Line 160–166: Original function pointers stored in `OnceLock`
  - Lines 200–250+: Detour implementations
- Current mitigation: 
  - Hooks are installed only if `rigor ground --mitm` is explicitly passed.
  - TLS verification bypass (SecTrustEvaluateWithError hook) is macOS-specific and requires a valid CA certificate chain.
  - By default, MITM is disabled; blind tunneling is used.
- Recommendations:
  - Document that `rigor ground --mitm` on a multi-user system allows a process running under the same user to inspect all TLS traffic (including secrets, tokens, PII).
  - Require explicit user confirmation when enabling MITM (e.g., "This will decrypt all HTTPS traffic. Continue? [y/N]").
  - Add audit logging: log every time a TLS handshake is MITM'd with full SANs and certificate details.
  - Consider restricting MITM to a whitelist of hosts (already partially done with `MITM_HOSTS`).

**Unsafe Code in DNS/Socket Hooks:**
- Risk: `layer/src/lib.rs` uses extensive `unsafe extern "C"` code to hook libc functions. Memory safety depends on correct pointer usage, struct layout, and call conventions.
- Files: `layer/src/lib.rs` (lines 133–250+)
- Current mitigation:
  - Detours follow the mirrord pattern, which is battle-tested.
  - Re-entrancy guards prevent recursive hook calls (DetourGuard pattern, lines 40–59).
  - OnceLock usage prevents data races on original function pointers.
- Recommendations:
  - Add fuzz testing for malformed addrinfo inputs and edge cases (null pointers, invalid family, truncated structs).
  - Use miri to detect undefined behavior in unsafe code paths (requires nightl Rust).
  - Document invariants: "All sockaddr pointers must be aligned to 8 bytes and valid for the lifetime of the hook call."

**API Key Exposure in Error Messages:**
- Risk: Error handling code may echo API keys or credentials in `eprintln!` or log messages.
- Files: `crates/rigor/src/daemon/proxy.rs`, `crates/rigor/src/daemon/mod.rs`, `crates/rigor/src/lib.rs`
- Current mitigation:
  - Error messages are generally generic ("Failed to reach upstream API: {}", "Failed to load rigor.yaml").
  - The `sanitize_pii` crate is used to redact PII from request bodies before forwarding.
  - Secrets in `ANTHROPIC_API_KEY` env var are not logged directly.
- Recommendations:
  - Add a lint rule or pre-commit hook to block `eprintln!` containing `api_key`, `ANTHROPIC_API_KEY`, or token-like strings.
  - Wrap all error formats with a sanitization pass: `.map_err(|e| sanitize_error_msg(&e))`.

## Performance Bottlenecks

**Mutex Contention on High-Volume Proxy Traffic:**
- Problem: The shared `state: Arc<Mutex<DaemonState>>` is locked 34+ times per proxy request (see `proxy.rs` lines 738, 745, 784, 796, 802, 819, etc.). Under concurrent requests, this becomes a bottleneck.
- Files: `crates/rigor/src/daemon/proxy.rs` (entire file, especially lines 730–900)
- Cause: All daemon state (gates, decisions, logs, cost tracking, blocked requests) is behind a single coarse-grained lock. Fine-grained locking or lock-free data structures would help.
- Improvement path:
  - Measure lock hold times with a custom tracing span around critical sections.
  - Split state into multiple independent RwLocks or DashMap instances (gate decisions, cost tracking, blocked requests are read-heavy).
  - Use `parking_lot::Mutex` (no poisoning, faster, smaller) in place of `std::sync::Mutex`.
  - Consider lock-free data structures for read-heavy fields like `active_streams`, `disabled_constraints`, `blocked_requests`.

**Streaming Response Accumulation:**
- Problem: In `proxy.rs` lines 1176–1300+, the streaming response evaluator accumulates entire chunks in memory before evaluating them for constraint violations. For a 100KB streaming response, this could consume 100KB of buffer.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 1163–1250)
- Cause: No explicit limit on chunk accumulation; only bounded by the 64-slot MPSC channel capacity.
- Improvement path:
  - Add a configurable max buffer size (e.g., 1MB) for streaming accumulation.
  - If buffer exceeds limit, emit a warning and switch to pass-through mode (no evaluation).
  - Profile real-world streaming response sizes (Claude with streaming typically 1–10KB per SSE event).

**Lazy Static Initialization Order:**
- Problem: Multiple `Lazy::new()` statics (DEBUG, DAEMON_PORT, INTERCEPT_HOSTS in `layer/src/lib.rs`) are initialized on first access, potentially during performance-sensitive code paths.
- Files: `layer/src/lib.rs` (lines 68, 75, 81, 91)
- Cause: Lazy initialization is convenient but adds unpredictable latency on first use.
- Improvement path:
  - Move env var reads to library initialization (called once at startup via `_init_rigor()` or similar).
  - Cache results in `OnceLock` before the first hook invocation.

## Fragile Areas

**Claim Extraction Pipeline (HeuristicExtractor):**
- Files: `crates/rigor/src/claim/heuristic.rs` (entire file, ~450 lines), `crates/rigor/src/claim/extractor.rs`
- Why fragile: 
  - Relies on regex patterns for sentence splitting, hedge detection, and action intent extraction.
  - Patterns are tuned for English assistant responses; non-English or code-heavy responses may produce false positives/negatives.
  - No fallback if regex fails; assumes UTF-8 and valid Unicode.
- Safe modification: 
  - Before changing any regex pattern, run the full test suite (`cargo test --lib claim`).
  - Add test cases for edge cases: mixed English/code, non-ASCII, very long sentences (>10KB).
  - Profile extraction time on real Claude outputs (use `RIGOR_DEBUG=1` to see extracted claims).
- Test coverage: Good unit test coverage in `heuristic.rs` (40+ tests), but missing integration tests with real transcripts.

**Rego Policy Engine (PolicyEngine / regorus):**
- Files: `crates/rigor/src/policy/` (multiple files), depends on `regorus` crate (0.2)
- Why fragile:
  - Regorus is a third-party OPA (Open Policy Agent) implementation; bugs in Rego parsing or execution could silently produce wrong verdicts.
  - Constraint semantics are expressed in Rego; a typo in a constraint's Rego snippet causes silent allow (fail-open).
  - Version pinned to 0.2; breaking changes in regorus would require migration.
- Safe modification:
  - Test any Rego snippet changes in isolation (`rigor eval` command, if available).
  - Add property-based tests: generate synthetic claims and verify constraint verdicts match expected values.
- Test coverage: Integration tests in `tests/integration_constraint.rs` (355 lines) cover basic Rego scenarios but not all edge cases.

**Daemon TLS Certificate Generation (tls::RigorCA):**
- Files: `crates/rigor/src/daemon/tls.rs` (not directly visible but referenced in mod.rs lines 226–232)
- Why fragile:
  - Self-signed certificate generation is cryptographic; bugs could produce invalid certs that browsers reject.
  - CA key storage in `~/.rigor/` is not encrypted; anyone with local filesystem access can steal the CA key.
  - OpenSSL command fallback (if rcgen fails) adds a shell execution dependency.
- Safe modification:
  - Do not change certificate generation logic without extensive testing on all target macOS versions.
  - Consider using `rustls-platform-verifier` for platform-native cert validation.
- Test coverage: Minimal; no visible tests for cert generation or validity.

## Scaling Limits

**Constraint Graph Strength Computation:**
- Current capacity: Tested up to ~50 constraints (from examples/ directory).
- Limit: Graph algorithms (ArgumentationGraph::compute_strengths) are O(N^2) or O(N^3) in constraint count due to transitivity closure.
- Scaling path:
  - Profile strength computation on 100, 500, 1000 constraints.
  - Identify bottleneck: constraint loading, argumentation graph cycles, or strength solver.
  - Implement caching: precompute strengths at startup and invalidate only on constraint edits (not on every request).

**Concurrent Proxy Requests:**
- Current capacity: Tested with ~10 concurrent Claude Code sessions; daemon remains responsive.
- Limit: Mutex contention (discussed above) becomes severe at 50+ concurrent requests. Streaming responses hold the mutex for seconds.
- Scaling path:
  - Refactor to per-request state (independent of daemon-global lock).
  - Use async/await throughout (already done for most paths).
  - Consider load-shedding: reject requests if queue depth exceeds threshold (e.g., >100 pending).

**Streaming Response Chunk Throughput:**
- Current capacity: ~100 chunks/sec (estimated), each evaluated for constraints.
- Limit: Chunk accumulation buffer and Rego evaluation become bottlenecks at >1000 chunks/sec.
- Scaling path:
  - Batch chunks: accumulate 10 chunks before evaluating (reduces Rego calls).
  - Add sampling: evaluate only every Nth chunk on high-volume streams.
  - Use streaming Rego evaluation (if regorus supports it) instead of accumulating.

## Dependencies at Risk

**regorus 0.2 (Rego/OPA Implementation):**
- Risk: Regorus is an incomplete OPA port; not all OPA features are supported. Version 0.2 is pinned and has low adoption outside rigor.
- Impact: If a future constraint uses unsupported OPA syntax, evaluation silently fails and defaults to allow (fail-open). This defeats the constraint.
- Migration plan: 
  - Monitor regorus issues and PRs; if stalled, evaluate `opa` (Go binary) as alternative (slower, but fully compatible).
  - Provide a `rigor test-policy` command that validates a constraint's Rego snippet against regorus before deployment.

**frida-gum (Function Hooking Framework):**
- Risk: Frida-gum is a mobile/security testing framework; using it for production TLS interception is unconventional. New macOS versions (15+) may introduce stricter code signing requirements that break LD_PRELOAD.
- Impact: If frida-gum stops working, MITM mode fails (but blind-tunneling mode is unaffected).
- Migration plan:
  - Evaluate alternatives: `dyld_insert_libraries` (macOS native), `mach_override` (deprecated), or custom kernel extension (too heavy).
  - For Phase 2: investigate macOS System Extension (requires user approval) as a future replacement.

**sanitize_pii 0.1.1 (PII Detection):**
- Risk: Old version with limited detector coverage. Regex patterns may not match newer API key formats.
- Impact: Custom or exotic API keys (e.g., internal enterprise formats) are not detected and leaking.
- Migration plan:
  - Upgrade to latest when available; maintain custom detectors for internal formats.
  - Add tests for each new detector before deploying.

## Missing Critical Features

**Audit Logging:**
- Problem: No persistent audit trail of which constraints blocked which requests, or when gates were approved/rejected.
- Blocks: Compliance workflows, debugging production issues, understanding user behavior.
- Recommendation: Add optional audit log file (`~/.rigor/audit.log`) that records:
  - Every constraint violation (claim, constraint ID, severity, timestamp)
  - Every gate decision (approved/rejected, timestamp, user info if available)
  - Every MITM certificate generation (timestamp, SANs)

**Dynamic Constraint Reloading:**
- Problem: Changing `rigor.yaml` requires restarting the daemon (`rigor ground` process).
- Blocks: Rapid iteration during development, A/B testing constraints without downtime.
- Recommendation: Implement file watch on `rigor.yaml` with graceful reload. Invalidate precompiled policy engine and recompile on change.

**Cost Tracking Persistence:**
- Problem: Cost tracking (cumulative_cost_usd, cost_by_model) lives only in daemon memory; restarting daemon loses data.
- Blocks: Accurate cost accounting, budget enforcement across sessions.
- Recommendation: Persist cost data to `~/.rigor/costs.json` and reload on startup. Implement a `rigor cost` command to view cumulative spend.

**Webhook Callbacks:**
- Problem: No way for external systems (CI, logging, alerting) to react to constraint violations.
- Blocks: Integration with SIEM, Slack/email alerts, automated remediation workflows.
- Recommendation: Add optional `webhooks` section to `rigor.yaml` with event types (violation, gate_decision, error) and HTTP endpoint URLs.

## Test Coverage Gaps

**PII Sanitizer Coverage:**
- What's not tested: Individual detector regex patterns (email, credit_card, provider-specific keys) are assumed correct but not unit tested.
- Files: `crates/rigor/src/daemon/proxy.rs` (PII_SANITIZER)
- Risk: A typo in a custom regex (e.g., `sk-proj-[A-Za-z0-9_-]{40,}` missing the word boundary) could cause silent false negatives.
- Priority: High — this is a security boundary.

**Streaming Response Evaluation:**
- What's not tested: The full path from streaming chunks → claim extraction → Rego evaluation is tested only in `tests/true_e2e.rs`, which requires a live LLM.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 1163–1300), `tests/true_e2e.rs`
- Risk: If streaming evaluation silently skips a constraint (e.g., due to an error in the chunk accumulation loop), violations are missed.
- Priority: High — affects real-world usage.

**Gate Timeout and Decision Logic:**
- What's not tested: The retroactive gate polling loop and timeout logic (lines 791–830 in proxy.rs) is not unit tested; only exercised by e2e tests if gates are actually triggered.
- Files: `crates/rigor/src/daemon/proxy.rs`
- Risk: Deadlocks or infinite loops in gate decision logic are not caught.
- Priority: Medium — gate feature is new and less stable than core constraint evaluation.

**TLS Certificate Validation:**
- What's not tested: The rcgen-generated certificates are never validated against the actual TLS handshake flow. certs could be invalid.
- Files: `crates/rigor/src/daemon/tls.rs` (inferred)
- Risk: If cert generation is broken, MITM connections fail silently and revert to blind tunneling.
- Priority: Medium — affects MITM feature only.

**Large Transcript Claim Extraction:**
- What's not tested: Claim extraction on transcripts with 100+ messages or 1MB+ total size.
- Files: `crates/rigor/src/claim/heuristic.rs`
- Risk: Memory exhaustion, regex timeouts, or incorrect extraction under stress conditions.
- Priority: Medium — affects long-running sessions.

---

*Concerns audit: 2026-04-19*
