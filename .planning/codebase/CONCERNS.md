# Codebase Concerns

**Analysis Date:** 2026-04-19

## Tech Debt

**proxy.rs is a 3,092-line monolith:**
- Issue: `crates/rigor/src/daemon/proxy.rs` at 3,092 lines is the single largest file in the codebase by 4.5x and concentrates request handling, response streaming, claim evaluation, PII detection, SSE parsing, retry logic, action gating, relevance scoring, and token counting into one file.
- Files: `crates/rigor/src/daemon/proxy.rs`
- Impact: Extremely hard to navigate, test, or modify without risking regressions. Contributors must understand 3K lines of context to change any proxy behavior. No test isolation — all proxy tests are at the bottom in a single `#[cfg(test)]` module.
- Fix approach: Extract into sub-modules: `proxy/pii.rs` (PII detection + redaction, lines 90–267), `proxy/streaming.rs` (SSE evaluation loop, lines 1138–2502), `proxy/relevance.rs` (LLM-as-judge scoring, lines 2504–2722), `proxy/auth.rs` (provider auth routing, lines 22–46). Keep `proxy/mod.rs` as the thin routing layer.

**`std::sync::Mutex` everywhere with `.unwrap()` on lock:**
- Issue: `SharedState` is `Arc<Mutex<DaemonState>>` and every handler calls `state.lock().unwrap()`. There are 29 `lock().unwrap()` calls in `proxy.rs` alone. A panic in any handler poisons the mutex, making all subsequent requests panic too — cascading failure.
- Files: `crates/rigor/src/daemon/proxy.rs`, `crates/rigor/src/daemon/mod.rs`, `crates/rigor/src/daemon/governance.rs`, `crates/rigor/src/daemon/gate.rs`, `crates/rigor/src/daemon/gate_api.rs`
- Impact: A single poisoned mutex takes down the entire daemon. In production proxy scenarios, this is catastrophic — all proxied LLM calls fail.
- Fix approach: Either (a) switch to `parking_lot::Mutex` which doesn't poison, or (b) replace `.unwrap()` with `.lock().unwrap_or_else(|e| e.into_inner())` to recover from poisoned mutexes, or (c) restructure state to use `tokio::sync::RwLock` with finer-grained locks per subsystem.

**Excessive `.clone()` in the hot proxy path:**
- Issue: 177 `.clone()` calls in `proxy.rs` alone, including cloning entire `serde_json::Value` request bodies, full `RigorConfig`, `HashMap<String, f64>` strengths maps, and `PolicyEngine` per request. The streaming evaluation loop (lines 1150–1210) clones `state`, `headers`, `modified_body`, `model`, `http_client`, `user_message`, `session_id`, and multiple event senders.
- Files: `crates/rigor/src/daemon/proxy.rs`
- Impact: Memory allocation pressure scales linearly with concurrent requests. For streaming responses with in-flight evaluation, each request holds cloned copies of the entire config + constraint metadata throughout the stream duration.
- Fix approach: Use `Arc<RigorConfig>` and `Arc<HashMap<String, f64>>` for config/strengths (clone the Arc, not the data). Pre-compute constraint metadata once at startup into `Arc<HashMap<String, ConstraintMeta>>` instead of rebuilding per-request.

**`rigor-harness` and `rigor-test` are empty stubs:**
- Issue: Both workspace crates (`crates/rigor-harness/src/lib.rs` — 8 lines, `crates/rigor-test/src/main.rs` — 62 lines) are scaffolding for a planned test infrastructure ("Plan D.3") that hasn't been implemented.
- Files: `crates/rigor-harness/src/lib.rs`, `crates/rigor-test/src/main.rs`
- Impact: No structured E2E test harness exists. The crates compile and pass CI but provide zero functionality. This blocks systematic regression testing of the daemon/proxy pipeline.
- Fix approach: Either implement the planned D.3 harness or remove the stubs to avoid confusion.

**Hardcoded MITM host list duplicated in two places:**
- Issue: The LLM API hostname list is defined as `MITM_HOSTS` in `crates/rigor/src/daemon/mod.rs` (lines 81–92) and again in `layer/src/lib.rs` as `INTERCEPT_HOSTS` (lines 91–). Adding a new provider requires changing both.
- Files: `crates/rigor/src/daemon/mod.rs`, `layer/src/lib.rs`
- Impact: Easy to add a host in one location but not the other, causing the layer to redirect traffic that the daemon doesn't know how to handle (or vice versa).
- Fix approach: Extract the host list into a shared const or config file that both crates reference. Alternatively, make it configurable via `rigor.yaml`.

**Legacy TLS config generated at startup alongside proper CA-based system:**
- Issue: `DaemonState::load()` generates BOTH a legacy self-signed multi-SAN cert (`tls_config`) AND a proper CA-based cert system (`rigor_ca`). The legacy code exists "for backward compatibility" (line 200-207 of `daemon/mod.rs`) and `generate_tls_config()` is called twice — once in state init and once in the TLS listener task (lines 329-336). Same host list is hardcoded in both calls.
- Files: `crates/rigor/src/daemon/mod.rs`, `crates/rigor/src/daemon/tls.rs`
- Impact: Unnecessary startup cost, confusing dual TLS systems, host list duplication within the same file.
- Fix approach: Remove the legacy `tls_config` field and `generate_tls_config()` function entirely. The dedicated TLS listener should use `rigor_ca` exclusively.

**`judge_config()` called three times during `DaemonState` initialization:**
- Issue: In `DaemonState::load()` (lines 255-266 of `daemon/mod.rs`), `crate::cli::config::judge_config()` is called three separate times — once for each of `judge_api_url`, `judge_api_key`, and `judge_model`. Each call re-reads `~/.rigor/config` from disk.
- Files: `crates/rigor/src/daemon/mod.rs`, `crates/rigor/src/cli/config.rs`
- Impact: Three unnecessary file reads at daemon startup. Minor performance issue but indicates rushed code.
- Fix approach: Call `judge_config()` once, destructure the tuple.

## Known Bugs

**PII redaction incomplete for structured Anthropic content blocks:**
- Symptoms: The PII-IN redaction in `proxy.rs` (lines 939-975) only scans `last_user_msg`, which is extracted from plain-string content. Anthropic structured messages (`content: [{type: "text", text: "..."}, {type: "image", ...}]`) have their text extracted correctly by `replace_last_user_content()`, but `last_user_msg` extraction (lines 872-877) uses byte-length truncation (`&s[..200]`) which can panic on multi-byte UTF-8 boundaries.
- Files: `crates/rigor/src/daemon/proxy.rs` (line 877)
- Trigger: User sends a message where byte position 200 falls inside a multi-byte UTF-8 character (e.g., emoji or CJK text).
- Workaround: The code comment (line 949) explicitly tracks this as a follow-up.

**`redact_for_display` slices on byte boundaries, not char boundaries:**
- Symptoms: `&s[..4]` and `&s[s.len() - 4..]` in `redact_for_display()` (lines 206-211) will panic if the first 4 or last 4 bytes of a secret happen to split a multi-byte character.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 206-211)
- Trigger: A detected secret containing non-ASCII characters near its start or end.
- Workaround: Unlikely in practice (most secrets are ASCII), but should use `.chars()` iteration.

**Stale PID detection is unreliable:**
- Symptoms: `daemon_alive()` in `crates/rigor/src/daemon/mod.rs` (lines 57-64) uses `kill(pid, 0)` which only checks process existence. If the OS recycles the PID to another process, the function returns `true` incorrectly.
- Files: `crates/rigor/src/daemon/mod.rs` (line 63)
- Trigger: Daemon crashes, OS assigns the same PID to a new process.
- Workaround: The code comments (line 55-56) acknowledge this and defer to "Phase 2 session registration checks."

## Security Considerations

**Unsafe code for file descriptor manipulation:**
- Risk: `crates/rigor/src/cli/ground.rs` uses 8 `unsafe` blocks (lines 241-263, 598-600) for `libc::dup`, `libc::dup2`, `libc::write`, and `std::process::Stdio::from_raw_fd`. `crates/rigor/src/daemon/mod.rs` uses `unsafe { libc::kill(pid, 0) }` (line 63). These are necessary for process control but introduce memory safety risks if file descriptors are invalid.
- Files: `crates/rigor/src/cli/ground.rs`, `crates/rigor/src/daemon/mod.rs`
- Current mitigation: Error checks on dup/dup2 return values; `mem::forget(log_file)` prevents double-close.
- Recommendations: Add `// SAFETY:` comments documenting invariants. Consider wrapping the fd operations in a dedicated `SafeFd` type.

**CA private key stored with mode 0600 but no encryption:**
- Risk: The rigor CA private key (`~/.rigor/ca-key.pem`) is persisted unencrypted to disk. Any process running as the user can read it and generate trusted certificates for arbitrary hosts.
- Files: `crates/rigor/src/daemon/tls.rs` (lines 95-102)
- Current mitigation: File permissions set to 0600 on Unix.
- Recommendations: Consider encrypting the key at rest, or at minimum warn users about the trust implications. The CA installation via `rigor trust` adds a root CA to the macOS login keychain — this is equivalent to trusting a proxy CA.

**`~/.rigor/config` stores API keys in plaintext:**
- Risk: The global config file stores `judge.api_key` as plain text (e.g., OpenRouter API keys). Any process running as the user can read `~/.rigor/config` and extract API keys.
- Files: `crates/rigor/src/cli/config.rs` (lines 36-53)
- Current mitigation: `mask_key()` function hides keys in CLI output. No protection for the file itself.
- Recommendations: Use OS keychain (macOS Keychain, Linux Secret Service) for API key storage, or at minimum set file permissions to 0600.

**No authentication on governance API endpoints:**
- Risk: The daemon's HTTP API endpoints (`/api/governance/*`, `/api/gate/*`, `/api/chat`) have zero authentication. Any process on localhost can toggle constraints, pause the proxy, approve/reject action gates, or force-block the next request.
- Files: `crates/rigor/src/daemon/governance.rs`, `crates/rigor/src/daemon/gate_api.rs`, `crates/rigor/src/daemon/chat.rs`
- Current mitigation: Binds to 127.0.0.1 only (line 320 of `daemon/mod.rs`).
- Recommendations: Add a shared secret or session token to governance API requests. Even localhost-only services should authenticate when they control security policy.

**Binary patching disables macOS library validation:**
- Risk: `patch_sip_binary()` in `crates/rigor/src/cli/ground.rs` (lines 21-100) copies binaries to `/tmp/rigor-patched/`, re-signs them ad-hoc with entitlements that disable `com.apple.security.cs.disable-library-validation` and `allow-unsigned-executable-memory`. This weakens the security posture of the patched binary.
- Files: `crates/rigor/src/cli/ground.rs` (lines 52-70)
- Current mitigation: Only applies to the specific binary being wrapped. Patched copies live in /tmp.
- Recommendations: Document the security implications clearly. Warn users that `rigor ground` weakens Hardened Runtime protections on the target binary.

**Captured API key stored in mutable shared state:**
- Risk: The proxy captures the user's API key from proxied request headers (line 882-888 of `proxy.rs`) and stores it in `DaemonState.api_key` for later use by the LLM-as-judge system. This key is held in memory for the daemon's lifetime and accessible from any handler.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 882-898)
- Current mitigation: None. The key is captured silently and used for judge calls.
- Recommendations: Audit the blast radius of a state dump. Consider only holding a hash or token reference instead of the raw key. At minimum, zero the key on daemon shutdown.

## Performance Bottlenecks

**`std::sync::Mutex` contention on hot path:**
- Problem: Every proxy request acquires `state.lock().unwrap()` multiple times — for streaming requests, the lock is acquired in a tight loop (every chunk) to check `blocked_requests`, `active_streams`, and `block_next` flags.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 712-714, 758-759, 766-803, 892, 1168, 1186, 1209, 1267, etc.)
- Cause: Using a single coarse-grained `Mutex<DaemonState>` for all shared state. The lock is held while building epistemic context, cloning config, and computing constraint metadata.
- Improvement path: Split `DaemonState` into fine-grained components: `Arc<AtomicBool>` for simple flags (`proxy_paused`, `block_next`), `Arc<RwLock<_>>` for config/graph (read-heavy), `Arc<DashMap<_>>` for action_gates/gate_decisions (concurrent access).

**Relevance scoring uses a global `SimpleSemaphore` limiting to 1 concurrent call:**
- Problem: `RELEVANCE_SEMAPHORE` (line 2515 of `proxy.rs`) limits LLM-as-judge relevance scoring to a single in-flight call across the entire daemon. If multiple requests need scoring simultaneously, all but one are silently skipped.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 2504-2515)
- Cause: Rate limiting protection against burning API credits, but it means concurrent requests get no relevance analysis.
- Improvement path: Use `tokio::sync::Semaphore` with a configurable permit count (e.g., 3). Queue requests instead of dropping them.

**`RELEVANCE_CACHE` is unbounded:**
- Problem: The `RELEVANCE_CACHE` (line 2518) is a `HashMap<String, Vec<(String, String, String)>>` behind a `Mutex` with no eviction policy. For long-running daemon sessions with diverse claims, this grows without bound.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 2517-2519)
- Cause: No TTL or LRU eviction implemented.
- Improvement path: Use an LRU cache (e.g., `lru` crate) with a max size of ~1000 entries, or add a TTL-based eviction sweep.

**Per-request constraint keyword extraction from config:**
- Problem: For every streaming request, the proxy extracts constraint keywords from ALL constraint names and descriptions (lines 1167-1182), building a `HashSet` and then collecting to `Vec`. This happens inside `state.lock()`.
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 1167-1182)
- Cause: Keywords aren't precomputed at config load time.
- Improvement path: Precompute keyword set once in `DaemonState::load()` and store as `Arc<Vec<String>>`.

## Fragile Areas

**Streaming SSE evaluation loop (proxy.rs lines 1214-2500):**
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 1214-2500)
- Why fragile: This 1,300-line `tokio::spawn` block handles SSE parsing, incremental text extraction, keyword matching, claim extraction, policy evaluation, action gating, stream blocking, retry-with-correction, and PII detection on response — all interleaved. It mixes Anthropic and OpenAI SSE formats inline. The `text_so_far` accumulator, `sse_parse_offset`, `last_eval_len`, `last_stream_text_len`, and `blocked` state variables create complex control flow.
- Safe modification: Do NOT add new concerns to this loop. Extract them as `EgressFilter` implementations in the `egress/` module. Any change requires careful testing with both Anthropic and OpenAI streaming responses.
- Test coverage: No direct unit tests for the streaming evaluation loop. The only streaming-related tests are SSE parser tests (`test_extract_sse_anthropic`, `test_extract_sse_openai`).

**`ground.rs` process lifecycle management:**
- Files: `crates/rigor/src/cli/ground.rs`
- Why fragile: Manages file descriptor duplication, binary patching, interception mode selection (LD_PRELOAD vs HTTP proxy vs transparent), child process spawning, and signal handling across macOS and Linux. The `unsafe` fd manipulation and `mem::forget(log_file)` create non-obvious ownership semantics.
- Safe modification: Test any changes on both macOS (arm64 + arm64e) and Linux. The SIP binary patching path is macOS-only and requires `codesign`.
- Test coverage: Zero tests for process lifecycle. The `ground` subcommand is only exercised manually.

**Action gate timeout polling loop:**
- Files: `crates/rigor/src/daemon/proxy.rs` (lines 766-805)
- Why fragile: Retroactive action gates use a polling loop with `sleep(500ms)` and repeated `state.lock().unwrap()` to wait for user decisions. This holds a tokio task alive for up to 60 seconds per pending gate, acquiring the global mutex every 500ms.
- Safe modification: Replace with `tokio::sync::watch` or `tokio::sync::Notify` for event-driven wake-up instead of polling.
- Test coverage: No tests for gate timeout behavior.

## Scaling Limits

**Global `Mutex<DaemonState>` serializes all requests:**
- Current capacity: Works for single-user, single-session usage (1-5 concurrent LLM requests).
- Limit: Under high concurrency, mutex contention becomes the bottleneck. The streaming evaluation loop holds references to cloned state, but the lock is still acquired per-chunk for flag checks.
- Scaling path: Decompose `DaemonState` into independent atomic/concurrent components (see Performance section).

**Broadcast channel fixed at 256 events:**
- Current capacity: `ws::create_event_channel()` creates a channel with buffer size 256. Dashboard clients that fall behind lose events.
- Limit: High-frequency streaming responses with claim extraction + relevance scoring can generate 50+ events per request. A few concurrent requests could overflow the buffer.
- Scaling path: Increase buffer or switch to per-client buffering with backpressure.

## Dependencies at Risk

**`serde_yml` at 0.0.12 — pre-1.0 crate:**
- Risk: Version 0.0.x indicates this is experimental. Breaking changes are expected without semver guarantees.
- Impact: YAML parsing of `rigor.yaml` and `FallbackConfig` relies on this crate.
- Migration plan: Evaluate `serde_yaml` (the canonical crate) or pin to exact version with thorough testing before any upgrade.

**`sanitize-pii` at 0.1.1 — very early-stage:**
- Risk: At version 0.1.1, the crate has limited adoption and the API may change. The comment in `proxy.rs` (lines 90-96) documents that `Sanitizer::default()` vs `Sanitizer::builder().build()` had a "silent latent defect for months."
- Impact: Core PII detection depends on this. False positives or missed patterns directly affect user experience.
- Migration plan: Monitor crate development. Consider adding rigor's own regex patterns as a fallback layer.

**`frida-gum` in layer crate — platform-specific FFI:**
- Risk: The `layer/` crate depends on `frida-gum` for function hooking. This is a C FFI dependency with platform-specific behavior (macOS arm64 vs arm64e, Linux LD_PRELOAD semantics).
- Impact: The entire interception layer (DNS/connect hooking) depends on this. Build failures on new platforms or Rust editions are possible.
- Migration plan: None obvious — frida-gum is the standard for userspace function hooking (same as mirrord uses).

## Missing Critical Features

**No rate limiting on proxy endpoints:**
- Problem: The daemon proxy endpoints accept unlimited requests. A misconfigured or runaway client could exhaust upstream API quotas by hammering the proxy.
- Blocks: Safe deployment in shared or multi-user environments.

**No graceful shutdown with in-flight request draining:**
- Problem: `start_daemon()` uses `tokio::select!` on listener handles with no shutdown signal handler. Active streams and pending gate decisions are abandoned on Ctrl+C.
- Blocks: Clean daemon restart without losing in-progress evaluations.

**No config hot-reload:**
- Problem: The `RigorConfig` and `ArgumentationGraph` are loaded once at startup. Changing `rigor.yaml` requires daemon restart. The governance API can toggle individual constraints, but structural changes (new constraints, new relations) require restart.
- Blocks: Iterative constraint development workflow.

## Test Coverage Gaps

**Daemon/proxy pipeline has zero integration tests:**
- What's not tested: The entire HTTP proxy pipeline (request reception, epistemic injection, upstream forwarding, response streaming, claim evaluation, PII detection, action gating, retry logic) is untested. No test starts an actual `axum` server.
- Files: `crates/rigor/src/daemon/proxy.rs`, `crates/rigor/src/daemon/mod.rs`
- Risk: Any change to the proxy's 3,000+ lines could break production behavior undetected.
- Priority: High

**Streaming SSE evaluation loop completely untested:**
- What's not tested: The 1,300-line streaming evaluation task (proxy.rs lines 1214-2500) — incremental claim extraction during SSE streaming, mid-stream blocking, retry-with-correction injection, PII-OUT detection on streamed responses.
- Files: `crates/rigor/src/daemon/proxy.rs`
- Risk: The most complex code path has zero coverage. A regression here silently passes CI.
- Priority: High

**`ground.rs` process orchestration untested:**
- What's not tested: Binary patching (`patch_sip_binary`), interception mode selection, fd duplication/redirection, child process spawning with environment setup, signal propagation.
- Files: `crates/rigor/src/cli/ground.rs` (614 lines)
- Risk: macOS-specific SIP workarounds and fd manipulation could break silently on OS updates.
- Priority: Medium

**No tests for governance API handlers:**
- What's not tested: `toggle_constraint`, `toggle_pause`, `toggle_block_next`, `list_constraints` — all governance endpoints lack tests beyond what `gate_api.rs` covers for its pure `compute_decision_response()` function.
- Files: `crates/rigor/src/daemon/governance.rs`
- Risk: Governance state changes could silently break constraint enforcement.
- Priority: Medium

**No tests for WebSocket event broadcasting:**
- What's not tested: `ws_handler`, `handle_socket`, event serialization, channel overflow behavior (lagged clients).
- Files: `crates/rigor/src/daemon/ws.rs`
- Risk: Dashboard connectivity issues would go unnoticed.
- Priority: Low

**Layer (LD_PRELOAD/DYLD_INSERT) crate has zero tests:**
- What's not tested: The entire `layer/src/lib.rs` (957 lines) — DNS hooking, connect hooking, SecTrust bypass, re-entrancy protection, transparent mode.
- Files: `layer/src/lib.rs`
- Risk: Platform-specific hooking code has no automated coverage. Breakage requires manual testing on target OS/architecture.
- Priority: Medium

---

*Concerns audit: 2026-04-19*
