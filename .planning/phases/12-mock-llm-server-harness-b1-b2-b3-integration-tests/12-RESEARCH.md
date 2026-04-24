# Phase 12: Mock-LLM server + B1/B2/B3 integration tests - Research

**Researched:** 2026-04-24
**Domain:** Rust integration testing / streaming proxy evaluation / PII redaction
**Confidence:** HIGH

## Summary

Phase 12 builds three integration tests (B1, B2, B3) that exercise the production proxy's BLOCK kill-switch, auto-retry, and PII redact-before-forward paths using MockLlmServer and TestProxy from rigor-harness. The existing harness primitives (MockLlmServer, TestProxy, IsolatedHome, SSE helpers) are well-suited for B1 and B2. For B3, MockLlmServer needs enhancement to track received request bodies so tests can inspect what the proxy actually forwarded upstream.

The core mechanism is fully understood from the codebase: (1) the streaming proxy accumulates text from SSE chunks, (2) at sentence boundaries it checks for constraint keyword matches, (3) on match it runs Rego evaluation and DF-QuAD severity scoring, (4) if severity >= 0.7 (block threshold) it drops the upstream connection and either retries (B2) or injects an error SSE event (B1). PII redaction (B3) runs synchronously before the request is forwarded, replacing the last user message content with `[REDACTED:Kind]` tagged text.

**Primary recommendation:** Enhance MockLlmServer with `Arc<Mutex<Vec<ReceivedRequest>>>` request tracking and a `received_requests()` accessor. Write three test files (`b1_kill_switch.rs`, `b2_auto_retry.rs`, `b3_pii_redact.rs`) in `crates/rigor/tests/`. Use Rego keyword-match constraints (pattern from `stop_hook_e2e.rs`) to trigger deterministic BLOCKs. Use `RIGOR_NO_RETRY=1` for B1 to isolate the kill-switch path. For B3, send requests containing known PII patterns (email, SSN) and assert the mock received redacted content.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
None explicitly locked -- all implementation at Claude's discretion.

### Claude's Discretion
All implementation at Claude's discretion. Key constraints:
- MockLlmServer already exists in rigor-harness (Phase 7) -- enhance, don't rebuild
- TestProxy with CONNECT support already exists (Phase 11)
- Tests use TestProxy::start_with_mock() to route proxy to MockLlmServer
- May need request tracking in MockLlmServer (Arc<Mutex<Vec<ReceivedRequest>>>)
- Over-editing guard: rigor-harness changes OK (test crate), no crates/rigor/src/ changes

### Deferred Ideas (OUT OF SCOPE)
- F6 full-proxy corpus replay -- Phase 13
- Performance benchmarks -- out of scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REQ-022 | Mock-LLM server harness in `crates/rigor/tests/support/` serves deterministic SSE responses configurable per-test | MockLlmServer already exists in rigor-harness with builder pattern, anthropic/openai chunks, and ephemeral port. Needs request tracking for B3. |
| REQ-023 | B1: streaming kill-switch test -- daemon BLOCK drops upstream within N ms of decision | Proxy.rs drops upstream via `drop(upstream)` at line 2122, then injects error SSE `event: error\ndata: {"type":"error",...}`. Set `RIGOR_NO_RETRY=1` to isolate kill path. |
| REQ-024 | B2: auto-retry exactly-once test -- on BLOCK, one retry with violation-feedback-injected prompt, not two | Retry marker `[RIGOR EPISTEMIC CORRECTION]` injected into system prompt (line 2174). Second BLOCK with marker present skips retry (line 2126). MockLlmServer must serve both violation-triggering and clean responses. |
| REQ-025a | B3: PII redact-before-forward -- sanitizer modifies request body before upstream send (not after, not in parallel) | PII-IN scan at line 1397 runs `detect_pii()` on user message, then `replace_last_user_content()` rewrites body_json before serialization. MockLlmServer request tracking verifies upstream received redacted text. |
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| BLOCK kill-switch | API / Backend (proxy.rs streaming loop) | -- | Proxy evaluates SSE chunks mid-stream, drops upstream TCP, injects error SSE into client stream |
| Auto-retry | API / Backend (proxy.rs retry path) | -- | Proxy rebuilds request with `[RIGOR EPISTEMIC CORRECTION]` feedback, resends to upstream, verifies retry response |
| PII redaction | API / Backend (proxy.rs PII-IN) | -- | Synchronous scan + rewrite of request body before `http_client.post()` send |
| MockLlmServer | Test infrastructure (rigor-harness) | -- | Serves deterministic SSE on ephemeral port, tracks received requests |
| TestProxy | Test infrastructure (rigor-harness) | -- | Wraps production build_router + DaemonState with IsolatedHome |

## Standard Stack

### Core (already in use)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rigor-harness | workspace | MockLlmServer, TestProxy, IsolatedHome, SSE helpers | Project test infrastructure (Phase 7) |
| tokio | 1.x | Async runtime for test servers | Already in workspace |
| reqwest | 0.12 | HTTP client in integration tests | Already used in proxy_hotpath.rs tests |
| axum | 0.8 | MockLlmServer routing with SSE support | Already used in harness |

### Supporting (already in use)
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde_json | 1.0 | Request body construction and inspection | Every test |
| bytes | 1 | SSE chunk handling | B1/B2 response inspection |
| futures-util | 0.3 | Stream iteration in tests | B1 streaming verification |

**Installation:** No new dependencies needed. All libraries already in rigor-harness and rigor workspace.

## Architecture Patterns

### System Architecture Diagram

```
Test (B1/B2/B3)
    |
    | HTTP POST /v1/messages  (with optional PII in user message)
    v
TestProxy (production router on ephemeral port)
    |
    |  1. Parse JSON body
    |  2. PII-IN scan: detect_pii() -> replace_last_user_content()  <-- B3 verifies this
    |  3. FilterChain (claim injection)
    |  4. Forward modified body to MockLlmServer
    v
MockLlmServer (SSE response)
    |  Records received request body  <-- B3 inspects this
    |  Serves deterministic SSE chunks
    v
TestProxy streaming loop:
    |  5. Accumulate text from SSE chunks
    |  6. At sentence boundary + keyword match: Rego evaluation
    |  7a. If BLOCK + RIGOR_NO_RETRY: drop upstream, inject error SSE  <-- B1
    |  7b. If BLOCK + retries enabled: drop upstream, rebuild with
    |      [RIGOR EPISTEMIC CORRECTION], resend to MockLlmServer  <-- B2
    v
Client receives: SSE chunks (partial) + error event (B1)
                 OR retry response (B2)
```

### Recommended Project Structure
```
crates/rigor-harness/src/
    mock_llm.rs         # Enhanced with request tracking
    proxy.rs            # No changes needed
    sse.rs              # No changes needed
    lib.rs              # Re-export ReceivedRequest

crates/rigor/tests/
    b1_kill_switch.rs   # B1: streaming BLOCK drops upstream
    b2_auto_retry.rs    # B2: exactly-once retry with feedback
    b3_pii_redact.rs    # B3: PII redacted before upstream send
```

### Pattern 1: Keyword-Match Rego Constraint for Deterministic BLOCKs

**What:** A Rego constraint that fires `violated: true` when claim text contains a specific keyword. Combined with the streaming keyword pre-filter (which checks constraint name/description words against accumulated text), this deterministically triggers the BLOCK path.

**When to use:** Every B1/B2 test needs a constraint that reliably triggers BLOCK.

**Example:**
```rust
// Source: crates/rigor/tests/stop_hook_e2e.rs (existing pattern)
const BLOCK_CONSTRAINT_YAML: &str = r#"constraints:
  beliefs:
    - id: block-trigger
      epistemic_type: belief
      name: Block trigger constraint
      description: Blocks output containing VIOLATION_MARKER keyword
      rego: |
        violation contains v if {
          some c in input.claims
          contains(c.text, "VIOLATION_MARKER")
          v := {"constraint_id": "block-trigger", "violated": true, "claims": [c.id], "reason": "violation marker found"}
        }
      message: Violation marker detected
  justifications: []
  defeaters: []
"#;
```

**Critical detail for streaming trigger:** The constraint keyword pre-filter (proxy.rs line 1632-1658) extracts words from constraint `name` + `description`, lowercased, >3 chars. The SSE text must contain one of these keywords at a sentence boundary (`. `, `! `, `? `, `.\n`). The word `violation_marker` (lowercased from "VIOLATION_MARKER" in description) will be extracted as a keyword. The SSE text must contain "violation_marker" (case-insensitive) at a sentence boundary. [VERIFIED: proxy.rs lines 1632-1658, 1848-1858]

**MockLlmServer text for BLOCK trigger:**
```rust
// This text must: (1) contain the keyword "VIOLATION_MARKER" (case-insensitive match)
// (2) have a sentence boundary (". ") for evaluation to fire
// (3) be an assertion (not question/hypothetical) for claim extraction
MockLlmServerBuilder::new()
    .anthropic_chunks("The system contains VIOLATION_MARKER in its output. This is a factual statement.")
    .build()
    .await
```

### Pattern 2: MockLlmServer Request Tracking for B3

**What:** Enhance MockLlmServer to capture all received request bodies so tests can assert what the proxy actually forwarded upstream.

**Example:**
```rust
// Source: CONTEXT.md suggestion + codebase patterns
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct ReceivedRequest {
    pub body: serde_json::Value,
    pub headers: Vec<(String, String)>,
}

pub struct MockLlmServer {
    addr: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
    received: Arc<Mutex<Vec<ReceivedRequest>>>,  // NEW
}

impl MockLlmServer {
    pub fn received_requests(&self) -> Vec<ReceivedRequest> {
        self.received.lock().unwrap().clone()
    }
}
```

### Pattern 3: RIGOR_NO_RETRY for B1 Isolation

**What:** The env var `RIGOR_NO_RETRY` disables the auto-retry path (proxy.rs line 2124-2126). Setting it isolates the kill-switch behavior for B1. [VERIFIED: proxy.rs lines 2124-2126, 2492]

**When to use:** B1 tests need to verify the BLOCK kills the stream and injects an error event WITHOUT retrying.

**Example:**
```rust
// In TestProxy::start_with_mock, RIGOR_NO_RETRY can be set via env before DaemonState::load
// OR: use std::env::set_var in the test before creating TestProxy
// The proxy checks std::env::var("RIGOR_NO_RETRY").is_ok() at BLOCK time (not at startup)
// So setting it before the request is sent is sufficient.
unsafe { std::env::set_var("RIGOR_NO_RETRY", "1") };
```

### Pattern 4: Two-Response MockLlmServer for B2

**What:** B2 tests need the mock to serve a violation-triggering response on the first call, then a clean response on the retry. This requires the mock to track call count and serve different responses.

**Example:**
```rust
// Enhanced MockLlmServer with response-per-call-index
pub struct MockLlmServerBuilder {
    responses: Vec<Vec<String>>,  // index 0 = first call, index 1 = retry
    // ...
}

impl MockLlmServerBuilder {
    pub fn response_sequence(mut self, responses: Vec<Vec<String>>) -> Self {
        self.responses = responses;
        self
    }
}
// Handler uses AtomicUsize call counter to select response
```

### Anti-Patterns to Avoid
- **Relying on block_next governance flag:** While `block_next` forces BLOCK, it requires violations to exist first (line 1999 `violations.clone()`). The evaluation only runs on keyword match + sentence boundary. Use a proper Rego constraint instead. [VERIFIED: proxy.rs lines 1988-2003]
- **Expecting the LLM judge to fire in tests:** In test contexts, `judge_api_key` is captured from request headers (line 1303-1310) and the judge URL falls back to `target_api` (the mock). The mock serves SSE, which `check_violations_persist` can't parse as JSON -- it returns `false`. This is acceptable: the retry is deemed "clean" without judge verification. Don't try to mock the judge separately. [VERIFIED: proxy.rs lines 2252-2278, cli/config.rs lines 61-82]
- **Using `std::env::set_var` without serialization:** TestProxy already uses `ENV_MUTEX` for env var races. Tests setting `RIGOR_NO_RETRY` must either use the same mutex pattern or ensure no parallel tests conflict. [VERIFIED: rigor-harness/proxy.rs line 9]
- **Testing PII on the response path:** The B3 requirement is "redact-before-forward" (request path). The PII-IN scan runs at proxy.rs line 1397. Don't confuse with PII-OUT (response path, line 3288) which is a different feature.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SSE generation | Manual JSON string building | `anthropic_sse_chunks()` / `openai_sse_chunks()` from rigor-harness/sse.rs | Exact format matching production SSE parsing in proxy.rs |
| PII detection in tests | Custom regex matching | `rigor::daemon::proxy::detect_pii()` | Same detector the proxy uses; test the real function |
| Isolated HOME | Manual tempdir management | `IsolatedHome::new()` from rigor-harness | Handles rigor.yaml placement, .rigor/ directory structure |
| Proxy + state setup | Manual DaemonState construction | `TestProxy::start_with_mock()` | Handles RIGOR_HOME, RIGOR_TARGET_API, env mutex, full production router |

## Common Pitfalls

### Pitfall 1: Streaming Evaluation Never Fires
**What goes wrong:** The test sets up a constraint and sends a request, but BLOCK never triggers because the streaming keyword pre-filter doesn't match.
**Why it happens:** The pre-filter (proxy.rs line 1848-1858) requires: (a) sentence boundary in new text, (b) keyword from constraint name/description present, (c) text length > `last_eval_len + 20`. If any condition fails, Rego evaluation is skipped entirely.
**How to avoid:** Ensure the MockLlmServer response text: (1) contains a word from the constraint name/description (lowercased, >3 chars), (2) has at least one sentence boundary (`. `), (3) is longer than 20 characters. Use the `VIOLATION_MARKER` pattern from stop_hook_e2e.rs which is proven to work.
**Warning signs:** Test passes (200 with full SSE stream) when it should BLOCK. Add debug logging or check for error SSE event in response.

### Pitfall 2: HeuristicExtractor Filters Out Claims
**What goes wrong:** The text reaches Rego evaluation but `claims` is empty because the heuristic extractor filtered it.
**Why it happens:** `is_assertion()` filters questions (`?`), hypotheticals (`if...`), code blocks, and short text. `is_hedged()` filters uncertain language. Conversational starters ("let me", "sure", etc.) are also filtered.
**How to avoid:** Use declarative, factual-sounding text: "The system contains VIOLATION_MARKER in its output." NOT "If the system has VIOLATION_MARKER..." or "Let me check for VIOLATION_MARKER."
**Warning signs:** Rego produces no violations despite keyword match. Check claim extraction.

### Pitfall 3: Retry Calls the Mock Instead of a Real Judge
**What goes wrong:** B2 retry fires and `check_violations_persist` calls the mock server, which serves SSE instead of a JSON response, causing the judge parse to fail silently.
**Why it happens:** In test context, `st.judge_api_key` is None (from env), so the proxy falls back to `st.api_key` (captured from request headers). The judge URL falls back to `st.target_api` (the mock URL). The mock serves SSE, not JSON.
**How to avoid:** Accept this behavior for B2 -- the judge parse failure causes `check_violations_persist` to return `false`, which means "retry is clean." This effectively tests the retry-send path without the judge verification. Alternatively, make MockLlmServer serve a JSON response on a separate route for judge calls.
**Warning signs:** None -- the current behavior silently succeeds. The risk is asserting on judge-specific DaemonEvents that never fire.

### Pitfall 4: Env Var Race Conditions in Parallel Tests
**What goes wrong:** `RIGOR_NO_RETRY` set for B1 leaks into B2 tests, disabling retries.
**Why it happens:** `std::env::set_var` is process-global. Cargo test runs tests in parallel.
**How to avoid:** Use `#[serial]` from `serial_test` crate, OR scope `RIGOR_NO_RETRY` via save/restore pattern (like TestProxy's ENV_MUTEX), OR pass it through TestProxy's spawn_blocking env setup so it's only set during DaemonState::load (but `RIGOR_NO_RETRY` is checked at runtime, not startup, so this doesn't work). Best approach: set `RIGOR_NO_RETRY` inside the test, use the ENV_MUTEX, and restore after.
**Warning signs:** B2 test intermittently fails to retry.

### Pitfall 5: MockLlmServer Serves Same Response on Retry (B2)
**What goes wrong:** B2 retry sends a new request to MockLlmServer, but the mock serves the same violation-triggering response, causing double BLOCK.
**Why it happens:** Default MockLlmServer serves the same response on every call. The retry needs a clean response.
**How to avoid:** Implement response-per-call-index in MockLlmServer (call 0 = violation text, call 1 = clean text). Use an `AtomicUsize` counter.
**Warning signs:** B2 test shows "retry_failed" instead of "retry_success".

### Pitfall 6: PII False Positives in Test Assertions (B3)
**What goes wrong:** `detect_pii()` doesn't detect the PII string used in the test because the sanitizer has been tightened to reduce false positives.
**Why it happens:** The PII sanitizer (proxy.rs line 174-236) intentionally excludes phone numbers, IPv4/IPv6, and generic API keys. Only email, credit_card (Luhn-validated), and provider-specific secret patterns are enabled.
**How to avoid:** Use PII patterns that are reliably detected: email addresses (`user@example.com`), SSN (`123-45-6789`), or Anthropic API keys matching `sk-ant-api\d+-[A-Za-z0-9_-]{32,}`. Do NOT use phone numbers, IP addresses, or generic hex strings.
**Warning signs:** B3 test shows mock received un-redacted PII. Verify with `detect_pii()` in a unit test first.

## Code Examples

### B1: Kill-Switch Test Pattern
```rust
// Source: Derived from proxy.rs streaming BLOCK path (lines 2005-2150)
// and proxy_hotpath.rs test patterns
#[tokio::test]
async fn b1_block_drops_upstream_and_injects_error_sse() {
    // Set RIGOR_NO_RETRY to isolate kill-switch (no retry attempt)
    // ... env mutex pattern ...

    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks(
            "The system contains VIOLATION_MARKER in its output. This is a factual statement."
        )
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(BLOCK_CONSTRAINT_YAML, &mock.url()).await;
    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "stream": true,
        "messages": [{"role": "user", "content": "Tell me something"}]
    });

    let resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&body)
        .send()
        .await
        .unwrap();

    let resp_body = resp.text().await.unwrap();
    // BLOCK injects error SSE event
    assert!(resp_body.contains("event: error"), "Should contain error SSE event");
    assert!(resp_body.contains("rigor BLOCKED"), "Error should mention BLOCK");
}
```

### B3: PII Redact-Before-Forward Pattern
```rust
// Source: Derived from proxy.rs PII-IN path (lines 1385-1421)
#[tokio::test]
async fn b3_pii_redacted_before_upstream_send() {
    let mock = MockLlmServerBuilder::new()
        .anthropic_chunks("I understand your request.")
        .build()
        .await;

    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;

    // Request body containing PII (email + SSN)
    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 100,
        "stream": false,
        "messages": [{
            "role": "user",
            "content": "My SSN is 123-45-6789 and my email is secret@example.com"
        }]
    });

    let _resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", proxy.url()))
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-api03-test")
        .json(&body)
        .send()
        .await
        .unwrap();

    // Inspect what MockLlmServer received
    let received = mock.received_requests();
    assert!(!received.is_empty(), "Mock should have received at least one request");

    let received_body = &received[0].body;
    let user_content = received_body["messages"]
        .as_array().unwrap()
        .iter().rev()
        .find(|m| m["role"] == "user")
        .unwrap()["content"]
        .as_str().unwrap();

    // PII should be redacted
    assert!(!user_content.contains("123-45-6789"), "SSN should be redacted");
    assert!(!user_content.contains("secret@example.com"), "Email should be redacted");
    assert!(user_content.contains("[REDACTED:"), "Should contain redaction tags");
}
```

### MockLlmServer Enhancement: Request Tracking
```rust
// Source: Derived from CONTEXT.md suggestion + existing mock_llm.rs patterns
use std::sync::{Arc, Mutex};
use axum::extract::State as AxumState;

#[derive(Debug, Clone)]
pub struct ReceivedRequest {
    pub body: serde_json::Value,
}

// In build():
let received: Arc<Mutex<Vec<ReceivedRequest>>> = Arc::new(Mutex::new(Vec::new()));
let received_clone = received.clone();

let handler = move |body: axum::body::Bytes| {
    let received = received_clone.clone();
    let chunks = chunks_clone.clone();
    async move {
        // Track received request
        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body) {
            received.lock().unwrap().push(ReceivedRequest { body: json });
        }
        // Serve SSE response
        let events: Vec<Result<Event, std::convert::Infallible>> = chunks
            .iter()
            .map(|data| Ok(Event::default().data(data)))
            .collect();
        Sse::new(stream::iter(events))
    }
};
```

### MockLlmServer Enhancement: Response Sequence (for B2)
```rust
// Source: Derived from B2 requirement -- first call = violation, second = clean
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct MockLlmServerBuilder {
    responses: Vec<Vec<String>>,  // Multiple response sets
    // ...
}

impl MockLlmServerBuilder {
    pub fn response_sequence(mut self, responses: Vec<Vec<String>>) -> Self {
        self.responses = responses;
        self
    }
}

// In build(), handler selects response by call index:
let call_count = Arc::new(AtomicUsize::new(0));
let handler = move || {
    let call_idx = call_count.fetch_add(1, Ordering::SeqCst);
    let chunks = if call_idx < responses.len() {
        responses[call_idx].clone()
    } else {
        responses.last().unwrap().clone()  // repeat last
    };
    // ... serve SSE with selected chunks
};
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No request tracking in MockLlmServer | Need to add `received_requests()` | Phase 12 | Enables B3 PII verification |
| Single response per mock | Need response-per-call-index | Phase 12 | Enables B2 retry testing |
| Manual env var management for test isolation | TestProxy ENV_MUTEX pattern | Phase 7/11 | Must extend for RIGOR_NO_RETRY |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `check_violations_persist` returns false when mock serves SSE instead of JSON (judge parse fails silently) | Pitfall 3 | B2 test behavior may differ -- the retry could be flagged as "still violated" instead of "clean". Would need mock to serve a proper JSON judge response on a separate route. |
| A2 | The keyword pre-filter matches "violation_marker" from the constraint description "Blocks output containing VIOLATION_MARKER keyword" | Pattern 1 | If keyword extraction skips compound words or strips differently, the pre-filter won't match and Rego evaluation won't fire. Mitigation: use single common words in constraint description. |
| A3 | `RIGOR_NO_RETRY` is checked at BLOCK time (runtime), not at proxy startup | Pattern 3 / Pitfall 4 | If checked at startup, TestProxy env setup would need to include it. But code shows `std::env::var("RIGOR_NO_RETRY").is_ok()` inline at line 2124. |

## Open Questions

1. **Response sequence vs. response function**
   - What we know: B2 needs different responses per call (violation then clean). Two approaches: (a) pre-defined response sequence with AtomicUsize counter, (b) closure-based response function.
   - What's unclear: Which is cleaner given the axum handler constraints.
   - Recommendation: Use response sequence (approach a) -- simpler, deterministic, matches existing builder pattern.

2. **Header tracking in ReceivedRequest**
   - What we know: B3 primarily needs the request body (to check user message content). Headers could also be useful for verifying auth forwarding.
   - What's unclear: Whether axum handler can cheaply access all request headers.
   - Recommendation: Start with body-only tracking. Add headers if needed later.

3. **Test isolation for RIGOR_NO_RETRY**
   - What we know: ENV_MUTEX exists in TestProxy for RIGOR_HOME/RIGOR_TARGET_API serialization. RIGOR_NO_RETRY needs similar protection.
   - What's unclear: Whether to extend ENV_MUTEX or create a separate mutex.
   - Recommendation: Reuse ENV_MUTEX -- all env var mutations should serialize through the same lock. B1 tests that set RIGOR_NO_RETRY should acquire ENV_MUTEX.

## Environment Availability

Step 2.6: SKIPPED (no external dependencies identified -- all testing uses in-process Rust test infrastructure).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test --test b1_kill_switch --test b2_auto_retry --test b3_pii_redact -- --nocapture` |
| Full suite command | `cargo test -p rigor --test '*'` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REQ-022 | MockLlmServer serves deterministic configurable SSE | integration | `cargo test -p rigor-harness -- mock_llm` | Existing (mock_llm.rs mod tests) |
| REQ-023 | B1: BLOCK drops upstream, client gets error SSE | integration | `cargo test --test b1_kill_switch` | Wave 0 |
| REQ-024 | B2: BLOCK triggers exactly-one retry with feedback | integration | `cargo test --test b2_auto_retry` | Wave 0 |
| REQ-025a | B3: PII redacted before upstream send | integration | `cargo test --test b3_pii_redact` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test --test b1_kill_switch --test b2_auto_retry --test b3_pii_redact`
- **Per wave merge:** `cargo test -p rigor --test '*'`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/rigor/tests/b1_kill_switch.rs` -- covers REQ-023
- [ ] `crates/rigor/tests/b2_auto_retry.rs` -- covers REQ-024
- [ ] `crates/rigor/tests/b3_pii_redact.rs` -- covers REQ-025a
- [ ] MockLlmServer request tracking enhancement in `crates/rigor-harness/src/mock_llm.rs`
- [ ] MockLlmServer response sequence support in `crates/rigor-harness/src/mock_llm.rs`

## Sources

### Primary (HIGH confidence)
- `crates/rigor/src/daemon/proxy.rs` -- Full streaming BLOCK/retry/PII flow (lines 1081-2530, 3180-3255)
- `crates/rigor-harness/src/mock_llm.rs` -- Existing MockLlmServer implementation
- `crates/rigor-harness/src/proxy.rs` -- TestProxy with env mutex pattern
- `crates/rigor-harness/src/sse.rs` -- SSE generation and parsing helpers
- `crates/rigor/tests/proxy_hotpath.rs` -- Existing proxy integration test patterns
- `crates/rigor/tests/stop_hook_e2e.rs` -- Keyword-match Rego constraint pattern
- `crates/rigor/tests/connect_tunnel.rs` -- TestProxy CONNECT test patterns
- `crates/rigor/src/violation/types.rs` -- SeverityThresholds (block >= 0.7, warn >= 0.4)
- `crates/rigor/src/violation/collector.rs` -- determine_decision() logic
- `crates/rigor/src/claim/heuristic.rs` -- Claim extraction filtering rules

### Secondary (MEDIUM confidence)
- `crates/rigor/src/policy/engine.rs` -- Rego policy evaluation via regorus
- `crates/rigor/src/daemon/governance.rs` -- block_next toggle API

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries already in workspace, no new dependencies
- Architecture: HIGH -- full codebase read of proxy.rs BLOCK/retry/PII paths
- Pitfalls: HIGH -- identified from code path analysis and existing test patterns
- MockLlmServer enhancement: HIGH -- straightforward axum handler + Arc<Mutex<Vec>>

**Research date:** 2026-04-24
**Valid until:** 2026-05-24 (stable -- internal test infrastructure, no external dependencies)
