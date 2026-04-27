# EC-3: `SessionResolver` — hook endpoints + prefix-hash fallback + turn counter

> Part of umbrella: #34 [UMBRELLA] Epistemic Cortex
> Depends on: **EC-1**, **EC-2**
> Lands in: `crates/rigor/src/memory/epistemic/session.rs`, extensions to `crates/rigor/src/daemon/`

## Scope

Session identification on top of the event-sourced substrate. After this lands:

- Rigor can reliably determine which user-facing Claude conversation each proxy request belongs to.
- Session detection has two tiers: **(1) Claude Code hooks** (primary) via new `/api/hooks/session/{start,end}` endpoints, **(2) prefix-hash fallback** over the first N user messages of the request body.
- `sessions` table persists session rows with `detection_method`, `started_at`, `ended_at`, `prefix_hash`, `turn_count`, `agent_kind`.
- Every proxy request increments `sessions.turn_count` — the logical clock used by EC-5's working-memory decay and EC-9's verification-pass interval.
- `SessionStarted` and `SessionEnded` events land in the event log.
- **No wall-clock session timeouts.** If the prefix hash drifts, a new session begins. `/clear` in Claude Code naturally produces a new hash.

## Design constraints pinned from the design thread

- **Session = user-facing Claude conversation.** Multiple daemon restarts can occur within one session; multiple sessions can run through one daemon.
- **Primary detection: hooks.** Claude Code supports hooks; rigor exposes `/api/hooks/session/start` and `/api/hooks/session/end` on the daemon's existing axum router.
- **Fallback detection: prefix hash.** SHA-256 over the first N canonical-serialized user messages. Same hash = same session; different hash = new session.
- **No wall-clock anywhere.** No "30-minute extension" heuristic — if the hash drifts, it's a new session. Simpler, epistemically cleaner, fewer edge cases.
- **Turn counter is first-class and event-based.** `sessions.turn_count` increments on every proxy request. No background event (verification, decay sweep) ticks it. Only agent-initiated interaction counts.
- **Hook correlation window.** When a hook fires `SessionStart`, the resolver holds a short-lived (5 second) in-memory map entry `{hook_session_id → (client_fingerprint, timestamp)}`. The next proxy request from the matching client within 5 seconds correlates to this hook-reported session.
- **Proxy-only integration.** The hooks ride the existing daemon HTTP API (`127.0.0.1:8787`). No new binary; no separate service; no MCP.

## What lands

```
crates/rigor/src/memory/epistemic/
  ├── session.rs                                (SessionResolver trait + impls)

crates/rigor/src/daemon/
  ├── mod.rs                                    (router wiring for new endpoints)
  ├── session_api.rs                            (NEW: /api/hooks/session/{start,end} handlers)
  └── proxy.rs                                  (resolver invocation at request entry)

crates/rigor/src/memory/epistemic/store/migrations/
  └── V3__sessions.sql                          (sessions table; upgrades V2 stub)

tests/
  ├── epistemic_session_resolver.rs
  └── epistemic_session_api_e2e.rs

benches/
  └── session_resolve.rs
```

## Schema contributions

**`V3__sessions.sql`** — replaces the stub from EC-2 with the full sessions table.

```sql
-- Full sessions table (replacing any stub). Turn counter is explicit.
CREATE TABLE IF NOT EXISTS sessions (
  session_id        TEXT PRIMARY KEY,
  started_at        INTEGER NOT NULL,                     -- unix-epoch-ms
  ended_at          INTEGER,
  detection_method  TEXT NOT NULL,                        -- 'hook'|'prefix_hash'
  prefix_hash       BLOB,                                 -- SHA-256 of first N user messages; NULL if hook-based
  agent_kind        TEXT,                                 -- 'claude-code'|'opencode'|'unknown'
  agent_version     TEXT,                                 -- from User-Agent or hook body
  client_fingerprint TEXT NOT NULL,                       -- hash of (source_ip + user_agent)
  turn_count        INTEGER NOT NULL DEFAULT 0,
  hook_correlation_id TEXT                                -- the hook-reported session id, for hook-detected sessions
) STRICT;
CREATE INDEX idx_sessions_prefix    ON sessions(prefix_hash) WHERE prefix_hash IS NOT NULL;
CREATE INDEX idx_sessions_fingerprint ON sessions(client_fingerprint, ended_at);
CREATE INDEX idx_sessions_active    ON sessions(ended_at) WHERE ended_at IS NULL;
```

## Trait surfaces

```rust
// crates/rigor/src/memory/epistemic/session.rs

use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait SessionResolver: Send + Sync {
    /// Resolve a session for an incoming proxy request.
    /// Increments turn_count before returning.
    async fn resolve(&self, req: &RequestFingerprint) -> Result<ResolvedSession>;

    /// Called by the hook endpoint when Claude Code's SessionStart fires.
    async fn hook_session_start(&self, hook_session_id: &str, client: ClientInfo) -> Result<()>;

    /// Called by the hook endpoint when Claude Code's SessionEnd fires.
    async fn hook_session_end(&self, hook_session_id: &str) -> Result<()>;

    /// Introspection: list active sessions for diagnostics (`rigor sessions --active`).
    async fn list_active(&self) -> Result<Vec<SessionRow>>;
}

pub struct RequestFingerprint {
    pub source_ip: std::net::IpAddr,
    pub user_agent: Option<String>,
    pub first_n_user_messages: Vec<String>,    // canonical-serialized, bounded at N=3 default
}

pub struct ResolvedSession {
    pub session_id: SessionId,
    pub is_new: bool,
    pub detection_method: SessionDetectionMethod,
    pub turn_count_after_increment: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionDetectionMethod { Hook, PrefixHash }

pub struct ClientInfo {
    pub source_ip: std::net::IpAddr,
    pub user_agent: Option<String>,
    pub agent_kind: AgentKind,
    pub agent_version: Option<String>,
}

pub enum AgentKind { ClaudeCode, OpenCode, Unknown }

/// SQLite-backed implementation used by the daemon.
pub struct SqliteSessionResolver {
    store: Arc<dyn EpistemicStore>,
    /// In-memory correlation map: {hook_session_id → (fingerprint, arrived_at_ms)}.
    /// Entries expire after HOOK_CORRELATION_WINDOW_MS (5_000).
    pending_hooks: Arc<tokio::sync::Mutex<HashMap<String, PendingHook>>>,
}

struct PendingHook {
    fingerprint: String,
    client: ClientInfo,
    arrived_at_ms: i64,
}

const HOOK_CORRELATION_WINDOW_MS: i64 = 5_000;
const DEFAULT_PREFIX_HASH_MESSAGE_COUNT: usize = 3;
```

## Event types introduced / wired

Two events (already declared in EC-2's `EventPayload`, but this is where they first get persisted):

- `SessionStarted { detection_method, agent_kind }` — emitted by `resolve` when a new session is created, or by `hook_session_start` when the hook-provided session is correlated.
- `SessionEnded` — emitted by `hook_session_end` or by explicit daemon shutdown on behalf of a client disconnect.

## Session detection flow (authoritative)

Pseudocode for `resolve`:

```
resolve(req):
    fingerprint = sha256(source_ip || user_agent)
    now = epoch_ms()

    # 1. Hook path
    pending = pending_hooks.get_matching(fingerprint, since=now - HOOK_CORRELATION_WINDOW_MS)
    if pending:
        session = find_or_create_session(
            hook_correlation_id = pending.hook_session_id,
            detection_method = Hook,
            prefix_hash = None,
            client_fingerprint = fingerprint,
            agent_kind = pending.client.agent_kind,
            agent_version = pending.client.agent_version,
        )
        pending_hooks.remove(pending.hook_session_id)
        if session.is_new:
            emit SessionStarted{ detection_method=Hook, agent_kind }
        increment turn_count
        return ResolvedSession{ session_id, is_new, detection_method=Hook, turn_count_after_increment }

    # 2. Prefix-hash fallback
    prefix_hash = sha256(canonical_serialize(req.first_n_user_messages))
    existing = SELECT * FROM sessions WHERE prefix_hash = ? AND ended_at IS NULL
    if existing:
        session = existing
        is_new = false
    else:
        session = INSERT INTO sessions (new row with prefix_hash, detection_method=PrefixHash, ...)
        is_new = true
        emit SessionStarted{ detection_method=PrefixHash, agent_kind=Unknown }

    increment turn_count
    return ResolvedSession{ session_id, is_new, detection_method=PrefixHash, turn_count_after_increment }
```

## API endpoints

New routes on the existing daemon axum router (rooted at `127.0.0.1:8787`):

```
POST /api/hooks/session/start
Body: { "hook_session_id": "abc123", "agent": "claude-code", "version": "0.12.0" }
Response: 204 No Content
```

```
POST /api/hooks/session/end
Body: { "hook_session_id": "abc123" }
Response: 204 No Content
```

- Both endpoints bind to loopback only (already the case for the daemon).
- Both are idempotent: duplicate start is a no-op (overrides existing pending entry); duplicate end against unknown session logs a warning but returns 204.
- No auth — loopback-only is the trust boundary.

Claude Code's hook config writes entries pointing at these URLs. Rigor's trust-shim installer (existing) adds these entries on `rigor trust claude` (scope for a follow-up issue, not in EC-3 — EC-3 exposes the endpoints; user enables them).

## Implementation notes & invariants

**Invariant 1: turn_count increments atomically per request.** The increment is inside the same transaction as the session lookup/insert, preventing races when two concurrent requests from the same session arrive.

**Invariant 2: hook correlation window is hard-capped at 5 seconds.** Entries older than 5 seconds are swept from `pending_hooks` via an eager cleanup on every `resolve` call (no background timer needed).

**Invariant 3: unmatched hooks don't create phantom sessions.** If a hook fires but no matching proxy request arrives within 5 seconds, the pending entry is dropped. No session row is written. Phantom sessions would pollute analytics.

**Invariant 4: prefix_hash uses canonical serialization.** Messages are normalized (trim trailing whitespace, canonicalize JSON key order if the message content is JSON) before hashing. Ensures same hash across clients that serialize differently.

**Invariant 5: `client_fingerprint` is NOT a security primitive.** It's a correlation key only. Spoofing it would at worst misattribute a request to another session — not a privilege escalation.

**Invariant 6: hook-detected sessions have `prefix_hash = NULL`.** They're keyed by `hook_correlation_id`. If hook detection fails mid-session (e.g., Claude Code restarts and the hook stops firing), the next request falls back to prefix_hash and may start a new session — that's acceptable; the old session just ends naturally when timed out by the caller.

**Operational detail: session list eviction.** No retention policy for sessions in EC-3. Future `rigor sessions gc` command can prune ended sessions older than N (out of scope here).

**Operational detail: fingerprint derivation.** `sha256(source_ip_octets || user_agent_bytes || b"rigor-session-fingerprint-v1")` with a rigor-specific tag to prevent accidental collision with other hashes in the system.

## Unit testing plan

Tests in `session.rs` module and `tests/epistemic_session_resolver.rs`.

### `session.rs` tests

- `test_prefix_hash_stable_across_invocations` — same first-N messages → same hash.
- `test_prefix_hash_differs_on_any_message_change` — modifying any message in first N changes the hash.
- `test_prefix_hash_truncation_at_default_n` — first-N message count defaults to 3; message 4 is ignored for hash.
- `test_prefix_hash_normalizes_whitespace` — trailing whitespace on user messages doesn't affect hash.
- `test_client_fingerprint_domain_separated` — fingerprint of same (ip, ua) differs from raw `sha256(ip||ua)` thanks to rigor-specific tag.
- `test_hook_correlation_expires_after_5s` — inserting a pending hook then advancing time 6 seconds (mocked clock via test helper) — `resolve` from same fingerprint no longer correlates.
- `test_hook_correlation_matches_within_5s` — inserting at t=0 and resolving at t=3000ms correlates.
- `test_hook_correlation_consumed_on_match` — after `resolve` matches a hook, a subsequent request with the same fingerprint does not re-correlate to the consumed hook.
- `test_resolve_creates_session_on_first_request` — new prefix hash → new session row with turn_count=1.
- `test_resolve_reuses_session_on_matching_prefix` — second request with same prefix → same session_id; turn_count=2.
- `test_resolve_creates_new_session_on_prefix_drift` — different prefix → different session_id.
- `test_session_started_event_emitted_on_new_session` — verify the event log has one SessionStarted event per new session.
- `test_session_ended_event_emitted_on_hook_end` — call `hook_session_end`, verify event + `sessions.ended_at` populated.
- `test_turn_count_monotonically_increases` — 100 resolves for the same session → turn_count=100.
- `test_turn_count_not_incremented_by_background_events` — applying verification events in EC-9 doesn't bump turn_count (enforced by only `resolve` having write access to `turn_count`).
- `test_idempotent_hook_start` — two calls to `hook_session_start` with same id within window replaces the entry (no error).

## E2E testing plan

`tests/epistemic_session_api_e2e.rs`:

**`e2e_hook_first_then_request_correlates`**:
- Start test daemon on loopback.
- POST `/api/hooks/session/start { hook_session_id: "h1", agent: "claude-code" }`.
- Within 5 seconds, issue a proxy request from fingerprint F.
- Query `SELECT * FROM sessions WHERE hook_correlation_id='h1'`; assert one row with detection_method='hook', turn_count=1.
- Issue second proxy request from F; turn_count becomes 2, no new session.
- POST `/api/hooks/session/end { hook_session_id: "h1" }`; assert `ended_at` populated.

**`e2e_hook_without_request_yields_no_session`**:
- POST `/api/hooks/session/start`; wait 6 seconds; issue proxy request from F.
- Assert session created via `prefix_hash`, NOT `hook`. Hook entry was expired.

**`e2e_prefix_hash_fallback_same_session_across_requests`**:
- No hook.
- Send 5 proxy requests with identical first-3-message prefix.
- Assert 1 session_id returned across all 5; turn_count=5.

**`e2e_prefix_hash_drift_starts_new_session`**:
- Send request A; capture session_id_A.
- Send request B with different first message (simulating `/clear`); capture session_id_B.
- Assert `session_id_A != session_id_B`; A is still in `sessions` with no `ended_at` (naturally leaked; EC's `rigor sessions gc` would clean later).

**`e2e_concurrent_requests_same_prefix_single_session`**:
- Fire 10 parallel requests with identical prefix.
- Assert exactly one session row inserted (race-safe via transaction).
- turn_count = 10.

**`e2e_hook_endpoint_rejects_non_loopback`**:
- Bind daemon; simulate request from a non-loopback IP (via test harness setting `RemoteAddr` explicitly).
- Endpoint returns 403 or drops connection. (Verifies daemon's existing loopback-only posture, doesn't add new guards in EC-3.)

**`e2e_session_started_event_persisted_across_restart`**:
- Start daemon; create session via hook.
- Shut down; restart daemon with same DB path.
- `SELECT * FROM belief_events WHERE event_type='session_started'` returns the pre-restart event.
- Issuing a new request with same prefix reuses the pre-restart session (still not ended).

## Performance testing plan

`benches/session_resolve.rs`:

**Benchmark 1: hook-correlated resolve.**
- `bench_resolve_hook_cached` — pre-register hook; `resolve` measures lookup + session upsert + turn_count increment + event emit.
- **Threshold:** p99 ≤ **1ms**.

**Benchmark 2: prefix-hash resolve (cache hit).**
- `bench_resolve_prefix_hash_existing` — resolve against existing session.
- **Threshold:** p99 ≤ **1ms**.

**Benchmark 3: prefix-hash resolve (new session).**
- `bench_resolve_prefix_hash_new` — each iteration uses unique prefix, creates new session.
- **Threshold:** p99 ≤ **2ms**. Includes INSERT + event emit.

**Benchmark 4: concurrent resolve under 8-way contention.**
- `bench_resolve_concurrent_8_same_prefix` — 8 tasks resolving same prefix concurrently; one session created, others reuse.
- **Threshold:** p99 per task ≤ **5ms**.

**Benchmark 5: hook correlation map overhead.**
- `bench_pending_hook_insertion_and_sweep` — register 1000 hooks, sweep expired, measure total time.
- **Threshold:** 1000 hook registrations + sweep ≤ **50ms**.

## Acceptance criteria

- [ ] `SessionResolver` trait defined with `resolve`, `hook_session_start`, `hook_session_end`, `list_active`.
- [ ] `SqliteSessionResolver` implements the trait with transactional semantics.
- [ ] `V3__sessions.sql` migration applied; `sessions` table has all columns including `turn_count` and `hook_correlation_id`.
- [ ] `/api/hooks/session/start` and `/api/hooks/session/end` routes live on daemon.
- [ ] Endpoints return 204 on success, log-and-ignore on unknown session end.
- [ ] Hook correlation window = 5 seconds, hard-capped.
- [ ] Prefix-hash fallback uses canonical serialization.
- [ ] `turn_count` increments atomically per `resolve` call.
- [ ] Background events (verification, decay) never touch `turn_count`.
- [ ] `SessionStarted` event emitted on new session creation.
- [ ] `SessionEnded` event emitted on hook_session_end.
- [ ] No wall-clock session timeout anywhere in the resolver.
- [ ] All 16 unit tests pass.
- [ ] All 7 e2e tests pass.
- [ ] All 5 perf benchmarks meet thresholds; baselines committed.
- [ ] `cargo clippy -- -D warnings` clean.

## Additional items surfaced in review

- **Race condition: hook fires AFTER proxy request.** Current design only handles hook-first-then-request. Add `test_resolve_then_hook_arrives_late_does_not_re_session` — proxy request at t=0 (creates prefix-hash session); hook arrives at t=2000ms for the same conversation. Correct behavior: the pending hook entry is consumed on the NEXT request from the same fingerprint (not retroactively attached to the earlier session). This means a conversation can flip from `prefix_hash` to `hook` detection mid-session; document and allow it.
- **Expose `HOOK_CORRELATION_WINDOW_MS` as config.** Move from hardcoded constant to `epistemic.session.hook_correlation_window_ms` in rigor.yaml; default 5000. Update `EpistemicConfig` to carry this.
- **`client_fingerprint` collision handling.** Two independent clients on the same machine behind the same user agent produce identical fingerprints. Add `test_two_fingerprint_collisions_get_separate_sessions` — first request establishes session A (prefix_hash P1); second request from "different" client with same fingerprint but different prefix_hash P2 creates session B. Fingerprint + prefix_hash together disambiguate.
- **Prefix hash canonical serialization spec.** Make the canonicalization rule explicit: for each user message, strip trailing whitespace from every line, normalize CR/LF to LF, strip leading/trailing blank lines. Concatenate with `\n\n`. Hash the UTF-8 bytes. Add `test_prefix_hash_canonicalization_cases` covering each rule.
- **Session end detection without a hook.** If the agent stops sending requests, the session never receives `SessionEnded`. Document: sessions without `ended_at` are "open indefinitely" and rely on a future `rigor sessions gc` command (out of scope). This is acceptable for session-scoped-only portability.
- **Observability hooks (X-1).** `cortex.session.resolve` span per call with `detection_method`, `is_new`, `turn_count_after`, `resolve_ms`. Hook endpoints emit `cortex.session.hook_received` with `source_ip`, `agent`.

## Dependencies

**Blocks:** EC-5, EC-6, EC-8, EC-9, EC-10.
**Blocked by:** EC-1, EC-2.
**Parallelizable with:** EC-4.

## References

- Umbrella: [UMBRELLA] Epistemic Cortex
- EC-1, EC-2
- `src/daemon/mod.rs` — existing axum router
- `src/daemon/governance.rs` — existing HTTP API pattern for new endpoints
- Project memory: `feedback_tdd.md`
