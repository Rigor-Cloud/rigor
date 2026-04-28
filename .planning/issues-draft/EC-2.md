# EC-2: Event log + projections + `EpistemicStore` trait + SqliteEpistemicStore

> Part of umbrella: #34 [UMBRELLA] Epistemic Cortex
> Depends on: **EC-1** (SQLite substrate + canonical format)
> Lands in: `crates/rigor/src/memory/epistemic/event.rs`, `projection.rs`, `store/`

## Scope

Event-sourced substrate for all belief state. After this lands:

- `BeliefEvent` is the universal state-change envelope; every mutation in the system flows through it.
- Every `EventPayload` variant has a locked `CanonicalHash` tag byte.
- The `EpistemicStore` trait is the single contract for persistent belief-state access.
- `SqliteEpistemicStore` implements it with transactional event-append + projection-update pairs. Event insert and projection change commit together or neither commits.
- `InMemoryEpistemicStore` implements it for unit tests; both impls must pass the same property test suite.
- The `belief_events` table is append-only; `belief_state_current` and `belief_edges` are projections derivable from the events.
- Projections can be rebuilt from events (`rebuild_projections()`) and verified for consistency (`verify_projections()`).

No retrieval, no working memory, no sessions yet — those are EC-3 through EC-6. This issue delivers only the event-sourced storage contract.

## Design constraints pinned from the design thread

- **Event log is the source of truth.** `belief_events` is append-only. Never updated. Never deleted outside explicit retention policy (no retention policy in EC-2; TBD in a future issue).
- **Projections are caches.** `belief_state_current` and `belief_edges` derive from events; rebuildable by replay.
- **Single transaction per event.** Event insert and projection update commit together. SQLite transactional guarantees prevent drift.
- **Direct projection UPDATE is forbidden outside `projection::apply_in_tx`.** The store module is the only writer of projection tables; enforced by module-private functions.
- **Tag bytes are permanent.** `BeliefAsserted = 0x01`, `BeliefVerified = 0x02`, etc. Once assigned, never re-used, never reordered. New variants get new tags. Removed variants leave gaps.
- **Canonical event_id.** `event_id = canonical_id(&BeliefEvent)`. Same logical event → same event_id regardless of machine. Uses the EC-1 `CanonicalHash` trait.
- **Pluggable trait contract.** `EpistemicStore` trait has two implementations (`SqliteEpistemicStore`, `InMemoryEpistemicStore`). Both must satisfy the same property-test suite. Future Postgres impl (Phase 4D) adds a third.
- **Dialect-portable SQL.** `BLOB`, unix-epoch-ms `INTEGER`, JSON stored as `TEXT`, `WITH RECURSIVE` — all portable to Postgres with column-type swap only.

## What lands

```
crates/rigor/src/memory/epistemic/
  ├── event.rs                                (BeliefEvent + EventPayload + CanonicalHash impls)
  ├── projection.rs                           (ProjectionBuilder + apply_in_tx + rebuild)
  └── store/
      ├── mod.rs                              (EpistemicStore trait)
      ├── sqlite.rs                           (SqliteEpistemicStore — extends EC-1 SqliteSubstrate)
      ├── in_memory.rs                        (InMemoryEpistemicStore)
      └── migrations/
          └── V2__event_log_and_projections.sql

tests/
  ├── epistemic_event_log.rs                  (trait contract tests, run against both impls)
  └── epistemic_projection_replay.rs          (replay == apply property tests)

benches/
  └── epistemic_event_throughput.rs           (append+projection perf)
```

## Schema contributions

**`V2__event_log_and_projections.sql`** — extends the EC-1 substrate with the event log and projection tables.

```sql
-- ========================================================================
-- SOURCE OF TRUTH: belief events (append-only)
-- ========================================================================
CREATE TABLE belief_events (
  event_id       BLOB PRIMARY KEY,
  belief_id      TEXT NOT NULL,
  event_type     TEXT NOT NULL,
  payload_json   TEXT NOT NULL,                 -- serde_json of EventPayload (diagnostic; authoritative state is the canonical hash)
  session_id     TEXT NOT NULL,
  timestamp      INTEGER NOT NULL,              -- unix-epoch-ms
  git_commit     TEXT,
  git_dirty      INTEGER NOT NULL DEFAULT 0,
  caused_by      BLOB,
  schema_version INTEGER NOT NULL,
  FOREIGN KEY (caused_by) REFERENCES belief_events(event_id)
) STRICT;
CREATE INDEX idx_evt_belief  ON belief_events(belief_id, timestamp);
CREATE INDEX idx_evt_session ON belief_events(session_id, timestamp);
CREATE INDEX idx_evt_type    ON belief_events(event_type, timestamp);

-- ========================================================================
-- PROJECTION: current belief state
-- ========================================================================
CREATE TABLE belief_state_current (
  belief_id            TEXT PRIMARY KEY,
  kind                 TEXT NOT NULL,            -- 'claim'|'constraint'|'justification'
  knowledge_type       TEXT NOT NULL,            -- 'empirical'|'rational'|'testimonial'|'memory'
  payload_json         TEXT NOT NULL,
  current_strength     REAL NOT NULL,
  base_strength        REAL NOT NULL,
  confidence_grade     TEXT NOT NULL,            -- 'fresh'|'stale'|'inhibited'|'contradicted'|'unverified'
  verification_count   INTEGER NOT NULL DEFAULT 0,
  contradiction_count  INTEGER NOT NULL DEFAULT 0,
  last_verified_at     INTEGER,
  last_verified_commit TEXT,
  source_id            TEXT,
  inhibited_until      INTEGER,
  last_event_id        BLOB NOT NULL,
  created_at           INTEGER NOT NULL,
  updated_at           INTEGER NOT NULL,
  FOREIGN KEY (last_event_id) REFERENCES belief_events(event_id)
) STRICT;
CREATE INDEX idx_bsc_kind  ON belief_state_current(kind, confidence_grade);
CREATE INDEX idx_bsc_kt    ON belief_state_current(knowledge_type, current_strength);
CREATE INDEX idx_bsc_stale ON belief_state_current(confidence_grade, last_verified_at);

-- ========================================================================
-- PROJECTION: belief edges (relations + anchors + cross-graph references)
-- ========================================================================
CREATE TABLE belief_edges (
  from_id           TEXT NOT NULL,
  to_id             TEXT NOT NULL,
  relation_type     TEXT NOT NULL,               -- 'supports'|'attacks'|'undercuts'|'anchors_at'|'justified_by'|'derives_from'|'contradicts'|'semantically_similar_to'
  confidence        REAL NOT NULL DEFAULT 1.0,
  extraction_method TEXT,                        -- 'ast'|'llm'|'inferred'|'manual'
  weight            REAL NOT NULL DEFAULT 1.0,
  payload_json      TEXT,
  created_at        INTEGER NOT NULL,
  last_event_id     BLOB NOT NULL,
  PRIMARY KEY (from_id, to_id, relation_type),
  FOREIGN KEY (from_id) REFERENCES belief_state_current(belief_id) ON DELETE CASCADE,
  FOREIGN KEY (to_id)   REFERENCES belief_state_current(belief_id) ON DELETE CASCADE,
  FOREIGN KEY (last_event_id) REFERENCES belief_events(event_id)
) STRICT;
CREATE INDEX idx_edge_from ON belief_edges(from_id, relation_type);
CREATE INDEX idx_edge_to   ON belief_edges(to_id, relation_type);
```

## Trait surfaces

### `event.rs`

```rust
use crate::memory::epistemic::canonical::{CanonicalHash, canonical_id};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

pub type EventId  = [u8; 32];
pub type BeliefId = String;
pub type SessionId = String;

/// Locked tag registry. Every value is permanent; never reuse, never reorder.
/// Gaps from removed variants are acceptable.
#[repr(u8)]
pub enum EventTag {
    BeliefAsserted              = 0x01,
    BeliefVerified              = 0x02,
    BeliefDrifted               = 0x03,
    BeliefMissing               = 0x04,
    StrengthUpdated             = 0x05,
    Inhibited                   = 0x06,
    UnInhibited                 = 0x07,
    EdgeAsserted                = 0x08,
    EdgeRemoved                 = 0x09,
    Contradicted                = 0x0A,
    WorkingMemoryActivated      = 0x0B,
    WorkingMemoryTouched        = 0x0C,
    SourceCredibilityAdjusted   = 0x0D,
    SessionStarted              = 0x0E,
    SessionEnded                = 0x0F,
    GoalExtracted               = 0x10,
    GoalCompleted               = 0x11,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefEvent {
    pub event_id: EventId,
    pub belief_id: BeliefId,
    pub payload: EventPayload,
    pub session_id: SessionId,
    pub timestamp: i64,
    pub git_commit: Option<String>,
    pub git_dirty: bool,
    pub caused_by: Option<EventId>,
    pub schema_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    BeliefAsserted {
        kind: BeliefKind,
        knowledge_type: KnowledgeType,
        payload_json: String,                     // serialized Claim/Constraint/Justification
        base_strength: f64,
        source_id: String,
    },
    BeliefVerified {
        method: VerificationMethod,
        anchor_sha256: Option<[u8; 32]>,
        file_sha256:   Option<[u8; 32]>,
        at_commit: Option<String>,
    },
    BeliefDrifted {
        method: VerificationMethod,
        new_anchor_sha256: [u8; 32],
        prior_anchor_sha256: [u8; 32],
    },
    BeliefMissing {
        method: VerificationMethod,
    },
    StrengthUpdated {
        prior: f64,
        new: f64,
        reason: StrengthChangeReason,
        iteration_count: u32,
    },
    Inhibited {
        reason: InhibitionReason,
        until: Option<i64>,
    },
    UnInhibited {
        lifted_reason: InhibitionReason,
    },
    EdgeAsserted {
        to: BeliefId,
        relation_type: RelationType,
        confidence: f64,
        extraction_method: Option<ExtractionMethod>,
    },
    EdgeRemoved {
        to: BeliefId,
        relation_type: RelationType,
    },
    Contradicted {
        by: BeliefId,
        evidence_json: String,
    },
    WorkingMemoryActivated {
        role: ActivationRole,
        initial_activation: f64,
        activated_at_turn: i64,
    },
    WorkingMemoryTouched {
        activation_delta: f64,
        touched_at_turn: i64,
    },
    SourceCredibilityAdjusted {
        source_id: String,
        prior: f64,
        new: f64,
        reason: String,
    },
    SessionStarted {
        detection_method: SessionDetectionMethod,
        agent_kind: Option<String>,
    },
    SessionEnded,
    GoalExtracted {
        goal_id: String,
        goal_text: String,
    },
    GoalCompleted {
        goal_id: String,
    },
}

impl BeliefEvent {
    /// Compute canonical event_id from the payload. Uses EC-1 `CanonicalHash`.
    pub fn compute_id(
        belief_id: &str,
        payload: &EventPayload,
        session_id: &str,
        timestamp: i64,
        git_commit: Option<&str>,
        git_dirty: bool,
        caused_by: Option<&EventId>,
        schema_version: u32,
    ) -> EventId {
        let mut h = Sha256::new();
        h.update(&[crate::memory::epistemic::canonical::CANONICAL_FORMAT_VERSION]);
        belief_id.hash_into(&mut h);
        payload.hash_into(&mut h);
        session_id.hash_into(&mut h);
        timestamp.hash_into(&mut h);
        git_commit.map(|s| s.to_string()).hash_into(&mut h);
        git_dirty.hash_into(&mut h);
        caused_by.map(|b| *b).hash_into(&mut h);
        schema_version.hash_into(&mut h);
        h.finalize().into()
    }
}

impl CanonicalHash for EventPayload {
    fn hash_into(&self, h: &mut Sha256) {
        match self {
            EventPayload::BeliefAsserted { kind, knowledge_type, payload_json, base_strength, source_id } => {
                h.update(&[EventTag::BeliefAsserted as u8]);
                kind.hash_into(h);
                knowledge_type.hash_into(h);
                payload_json.hash_into(h);
                base_strength.hash_into(h);
                source_id.hash_into(h);
            }
            EventPayload::BeliefVerified { method, anchor_sha256, file_sha256, at_commit } => {
                h.update(&[EventTag::BeliefVerified as u8]);
                method.hash_into(h);
                anchor_sha256.hash_into(h);
                file_sha256.hash_into(h);
                at_commit.hash_into(h);
            }
            // ... every variant implemented. Code review gate: tag byte must match EventTag.
            _ => unimplemented!("to be filled during implementation"),
        }
    }
}
```

### `store/mod.rs`

```rust
use async_trait::async_trait;
use anyhow::Result;

#[async_trait]
pub trait EpistemicStore: Send + Sync {
    /// Append one event. Transactionally updates projection.
    /// Returns the canonical event_id (idempotent — same logical event returns same id).
    async fn append_event(&self, event: BeliefEvent) -> Result<EventId>;

    /// Append a batch in a single transaction. Used by bulk paths (migrations, replay).
    async fn append_batch(&self, events: Vec<BeliefEvent>) -> Result<Vec<EventId>>;

    /// Fetch events for a belief, optionally filtered by timestamp.
    async fn events_for_belief(&self, id: &BeliefId, since: Option<i64>) -> Result<Vec<BeliefEvent>>;

    /// Fetch events for a session, optionally filtered by timestamp.
    async fn events_for_session(&self, id: &SessionId, since: Option<i64>) -> Result<Vec<BeliefEvent>>;

    /// Iterate events in timestamp order. Used by replay/rebuild.
    async fn scan_events(&self, from: Option<i64>, to: Option<i64>, page_size: u32) -> Result<EventStream>;

    /// Read current projected belief state.
    async fn get_belief(&self, id: &BeliefId) -> Result<Option<BeliefState>>;

    /// Query by kind + filter.
    async fn beliefs_by_kind(&self, kind: BeliefKind, filter: BeliefFilter) -> Result<Vec<BeliefState>>;

    /// Read edges.
    async fn edges_from(&self, id: &BeliefId, rel: Option<RelationType>) -> Result<Vec<Edge>>;
    async fn edges_to(&self,   id: &BeliefId, rel: Option<RelationType>) -> Result<Vec<Edge>>;

    /// Traverse the belief graph via WITH RECURSIVE.
    async fn traverse(&self, from: &BeliefId, policy: TraversalPolicy) -> Result<Subgraph>;

    /// Rebuild projection tables from event log. Idempotent.
    async fn rebuild_projections(&self) -> Result<RebuildStats>;

    /// Consistency check: does the current projection match replay(events)?
    async fn verify_projections(&self) -> Result<VerificationReport>;
}
```

### `projection.rs`

```rust
pub struct ProjectionBuilder<'tx> {
    tx: &'tx rusqlite::Transaction<'tx>,
}

impl<'tx> ProjectionBuilder<'tx> {
    /// Apply a single event's effect to projection tables within the open transaction.
    /// MUST be called inside the same transaction that appended the event.
    pub fn apply_in_tx(&mut self, event: &BeliefEvent) -> Result<()> {
        match &event.payload {
            EventPayload::BeliefAsserted { kind, knowledge_type, payload_json, base_strength, source_id } => {
                // INSERT OR REPLACE into belief_state_current with confidence_grade='unverified'.
                // If belief already exists, bump updated_at but leave verification_count, etc. intact.
                // ...
            }
            EventPayload::BeliefVerified { method, anchor_sha256, file_sha256, at_commit } => {
                // UPDATE belief_state_current:
                //   SET last_verified_at = event.timestamp,
                //       last_verified_commit = at_commit,
                //       verification_count = verification_count + 1,
                //       confidence_grade = 'fresh',
                //       last_event_id = event.event_id,
                //       updated_at = event.timestamp
                //   WHERE belief_id = event.belief_id
                // INSERT INTO verification_events (EC-9 table; stub OK in EC-2).
                // ...
            }
            EventPayload::BeliefDrifted { .. } => {
                // UPDATE belief_state_current SET confidence_grade = 'stale'.
                // INSERT INTO inhibitions (auto-inhibit).
                // ...
            }
            // ... each variant has a deterministic apply rule. Full table below.
            _ => unimplemented!("to be filled during implementation"),
        }
        Ok(())
    }
}

/// Full rebuild — blow away projection tables and replay all events in timestamp order.
/// Used after schema migrations that change projection shape, or after corruption recovery.
pub async fn rebuild_projections(store: &impl EpistemicStore) -> Result<RebuildStats>;
```

## Event → projection mutation table (authoritative)

| Event | Target table(s) | Mutation |
|---|---|---|
| `BeliefAsserted` | `belief_state_current` | INSERT OR REPLACE; `confidence_grade='unverified'`; `verification_count=0`; `last_event_id=event_id` |
| `BeliefVerified` | `belief_state_current` + `verification_events` | UPDATE `last_verified_at`/`last_verified_commit`/`verification_count+=1`/`confidence_grade='fresh'`; INSERT verification_events row |
| `BeliefDrifted` | `belief_state_current` + `inhibitions` | UPDATE `confidence_grade='stale'`; INSERT inhibition with `reason='anchor_stale'` |
| `BeliefMissing` | `belief_state_current` + `inhibitions` | UPDATE `confidence_grade='stale'`; INSERT inhibition with `reason='anchor_missing'` |
| `StrengthUpdated` | `belief_state_current` | UPDATE `current_strength`; also `base_strength` if reason is 'manual_override' |
| `Inhibited` | `inhibitions` | INSERT row (composite PK allows multiple history entries); UPDATE `belief_state_current.inhibited_until` and `confidence_grade='inhibited'` |
| `UnInhibited` | `inhibitions` | UPDATE `lifted_at` on the open row (WHERE lifted_at IS NULL); if no more active inhibitions, clear `belief_state_current.inhibited_until` and re-derive `confidence_grade` |
| `EdgeAsserted` | `belief_edges` | INSERT OR REPLACE |
| `EdgeRemoved` | `belief_edges` | DELETE |
| `Contradicted` | `belief_state_current` + `belief_edges` | UPDATE `contradiction_count+=1`; INSERT edge with `relation_type='contradicts'` |
| `WorkingMemoryActivated` | `working_memory` (EC-5 table; stub OK in EC-2) | INSERT |
| `WorkingMemoryTouched` | `working_memory` | UPDATE activation + touch_count |
| `SourceCredibilityAdjusted` | `sources` (EC-4 table; stub OK in EC-2) | UPDATE credibility_weight |
| `SessionStarted` | `sessions` (EC-3 table; stub OK in EC-2) | INSERT |
| `SessionEnded` | `sessions` | UPDATE ended_at |
| `GoalExtracted` | `session_goals` (EC-4 table; stub OK in EC-2) | INSERT |
| `GoalCompleted` | `session_goals` | UPDATE completed_at |

Note: Events for tables owned by later issues (working_memory, sources, sessions, session_goals, verification_events, inhibitions) have their `apply_in_tx` branch implemented as a stub that's a no-op until the owning table lands. Those stubs get filled by the owning issue. Events still persist into `belief_events` regardless — the event log is the source of truth; projections catch up as tables come online.

## Implementation notes & invariants

**Invariant 1: single transaction per event.** `SqliteEpistemicStore::append_event` opens a transaction, inserts the event row, calls `ProjectionBuilder::apply_in_tx`, and commits. No split.

**Invariant 2: idempotent appends.** If the same logical event is appended twice, the second append is a no-op (INSERT OR IGNORE on event_id PK). Returns the existing event_id.

**Invariant 3: event_id determinism.** `compute_id` must produce the same 32-byte hash for logically identical events across machines. Golden-file tests enforce this.

**Invariant 4: projection tables never UPDATE outside `apply_in_tx`.** The `SqliteEpistemicStore` struct has only one place that executes UPDATE/INSERT against projection tables — inside `apply_in_tx`. Enforced by keeping the method private and only `append_event` calling it.

**Invariant 5: `payload_json` is diagnostic, not authoritative.** The JSON column is serialized `EventPayload` for `rigor log` queries and `SELECT * FROM belief_events` debugging. The canonical state is in the columns; the event_id is over the struct. `payload_json` is a pretty view, not a hash input.

**Invariant 6: `caused_by` chains are within-session or forward-dated.** An event's `caused_by` references an event that must have a strictly earlier timestamp. Enforced by a CHECK at INSERT time (rejected if `caused_by.timestamp >= new_event.timestamp`).

**Invariant 7: replay is deterministic under fixed event order.** Given the event log ordered by `(timestamp, event_id)` (event_id breaks ties), replay produces a byte-identical projection. `verify_projections()` asserts this invariant by rebuilding into a temp table and diffing.

**Operational detail: `InMemoryEpistemicStore`.** Implements the same trait with `Vec<BeliefEvent>` + `HashMap`-backed projections. Used in unit tests. Must pass the same property test suite as `SqliteEpistemicStore`.

**Operational detail: scan_events pagination.** `scan_events` returns an async stream that pages via `LIMIT page_size OFFSET N` under the hood. Used by rebuild and by future `rigor log` / `rigor refine` queries.

## Unit testing plan

Tests in `event.rs` (co-located), `projection.rs` (co-located), and `tests/epistemic_event_log.rs` for the trait-contract suite run against both implementations.

### `event.rs` tests

- `test_event_tag_values_locked` — every tag byte matches a documented constant in a separate "locked_tags" module, and a compile-time assertion covers all enum variants.
- `test_belief_asserted_canonical_hash_golden` — fixed input `BeliefAsserted{ ... }` hashes to the committed golden value in `tests/fixtures/event_golden.bin`.
- `test_all_variants_have_distinct_tags` — build-time check iterates `EventTag` and asserts no duplicate values.
- `test_event_payload_serde_round_trip` — every variant serializes via serde_json and deserializes back to `==`.
- `test_compute_id_deterministic` — same input → same output across 1000 iterations.
- `test_compute_id_sensitive_to_all_fields` — mutating `belief_id`, `timestamp`, `git_commit`, each payload field in turn changes the event_id.
- `test_compute_id_not_sensitive_to_serialization_order` — serde_json output order doesn't affect event_id because payload_json isn't hashed; canonical bytes are.

### `projection.rs` tests

- `test_apply_belief_asserted_inserts_row` — InMemoryStore.
- `test_apply_belief_verified_bumps_count` — InMemoryStore.
- `test_apply_belief_drifted_marks_stale_and_inhibits` — InMemoryStore.
- `test_apply_inhibited_inserts_inhibition_row` — InMemoryStore.
- `test_apply_uninhibited_lifts_active_inhibition` — InMemoryStore.
- `test_apply_edge_asserted_inserts_edge` — InMemoryStore.
- `test_apply_contradicted_increments_counter_and_inserts_edge` — InMemoryStore.
- `test_apply_idempotent_on_duplicate_event_id` — applying same event twice produces same state.
- `test_apply_order_sensitive_for_dependent_events` — BeliefVerified without prior BeliefAsserted is a recoverable error with logged warning, not a crash (defensive replay).

### Trait contract tests (`tests/epistemic_event_log.rs`)

These run against BOTH `SqliteEpistemicStore` and `InMemoryEpistemicStore` via a generic test harness. Each test is parameterized across the two impls:

- `contract_append_event_returns_stable_id`
- `contract_append_batch_is_transactional` — if any event in the batch fails (e.g., canonical hash collision that shouldn't happen but is tested via mocked compute_id), ALL events roll back.
- `contract_events_for_belief_filters_correctly`
- `contract_events_for_session_sorted_by_timestamp`
- `contract_scan_events_pagination_correct`
- `contract_get_belief_returns_projection`
- `contract_beliefs_by_kind_filtered`
- `contract_edges_from_and_to_symmetric`
- `contract_traverse_bfs_to_depth_3`
- `contract_rebuild_projections_matches_live_state` — write 100 events, rebuild, compare projection tables byte-for-byte.
- `contract_verify_projections_detects_corruption` — manually corrupt a projection row, verify catches it.

## E2E testing plan

**`e2e_append_replay_identity`** (tests/epistemic_projection_replay.rs):
- Create a fresh SqliteEpistemicStore.
- Append 1,000 events across 50 beliefs and 10 sessions.
- Snapshot projections (checksum of each row).
- Call `rebuild_projections()`.
- Recompute checksums; assert identical.

**`e2e_crash_recovery_rolls_back_partial_write`**:
- Spawn a child process that opens the store, begins a transaction, inserts an event row but not projection update, then panics.
- Parent process reopens the store and verifies the event row is absent (SQLite's atomic commit).

**`e2e_two_writer_attempts_rejected`**:
- Process A opens SqliteEpistemicStore as writer.
- Process B attempts the same; gets a clear error with A's PID.

**`e2e_reader_sees_committed_state_during_writer_activity`**:
- Writer process appends events in a loop (100 events/sec).
- Reader process runs `events_for_session` continuously.
- Reader never blocks the writer; reader sees monotonically increasing event counts.

**`e2e_hash_stability_across_restarts`**:
- Append event A; record event_id.
- Shut down; wipe in-memory state; reopen.
- Append logically identical event A again; verify same event_id returned (idempotent append).

**`e2e_verify_projections_green_on_fresh_db`**:
- Fresh DB.
- Append 10,000 events.
- `verify_projections()` returns OK.

**`e2e_verify_projections_red_on_corruption`**:
- Append 10,000 events.
- Manually `UPDATE belief_state_current SET current_strength = 999 WHERE ...` bypassing `apply_in_tx`.
- `verify_projections()` returns error naming the divergent belief_id.

## Performance testing plan

Benches in `benches/epistemic_event_throughput.rs`:

**Benchmark 1: single-event append + projection.**
- `bench_append_belief_asserted` — 10,000 appends, individual transactions.
- **Threshold:** p50 ≤ **1ms**, p99 ≤ **2ms**. Includes event_id compute + INSERT + projection apply + commit.

**Benchmark 2: batched append.**
- `bench_append_batch_100` — 100 events in a single transaction, 100 iterations.
- **Threshold:** ≤ **10ms per batch** p99. Amortized per-event ≤ 100μs.

**Benchmark 3: replay / rebuild throughput.**
- `bench_rebuild_100k_events` — populate DB with 100k events of mixed variants, then call `rebuild_projections()`.
- **Threshold:** ≤ **10 seconds**. Proves replay is usable as a recovery tool.

**Benchmark 4: `events_for_belief` query.**
- `bench_events_for_belief_recent` — random lookup of the last N events for a random belief from a pool of 10k beliefs.
- **Threshold:** p99 ≤ **5ms** with the `idx_evt_belief` index.

**Benchmark 5: traversal.**
- `bench_traverse_bfs_depth_3` — `WITH RECURSIVE` traversal from a random belief to depth 3 in a graph of 10k beliefs / 30k edges.
- **Threshold:** p99 ≤ **20ms**.

**Benchmark 6: concurrent reader throughput under writer load.**
- `bench_concurrent_8_readers_1_writer` — 8 tokio tasks running `get_belief` + `events_for_belief` concurrently while one task is appending 500 events/sec.
- **Threshold:** reader p99 ≤ **10ms**; writer throughput ≥ **400 events/sec** (80% of no-contention rate).

Baselines committed at `benches/baselines/epistemic_event.json`. CI fails on ≥ 20% regression.

## Acceptance criteria

- [ ] `event.rs` defines `BeliefEvent`, `EventPayload` (all 17 variants from the design thread), `EventTag` registry with locked bytes.
- [ ] Every `EventPayload` variant has a `CanonicalHash` impl matching the `EventTag` byte.
- [ ] `BeliefEvent::compute_id` uses `CanonicalHash` and prepends `CANONICAL_FORMAT_VERSION`.
- [ ] `projection.rs` implements `ProjectionBuilder::apply_in_tx` for every variant (with stubs for not-yet-landed projection tables).
- [ ] `projection.rs` implements `rebuild_projections()` and `verify_projections()`.
- [ ] `store/mod.rs` defines `EpistemicStore` trait with 10 methods as specified.
- [ ] `store/sqlite.rs` implements `SqliteEpistemicStore`: single-tx append + projection, batch append, reads, traverse, rebuild, verify.
- [ ] `store/in_memory.rs` implements the trait for test use.
- [ ] `V2__event_log_and_projections.sql` applied on existing DBs; new columns and indexes materialize.
- [ ] All unit tests pass (including golden-file for event canonical hashes).
- [ ] All e2e tests pass.
- [ ] All 6 perf benchmarks meet thresholds; baselines committed.
- [ ] Trait-contract suite passes against both impls.
- [ ] No direct projection UPDATE/INSERT outside `apply_in_tx`; enforced by keeping the function module-private.
- [ ] `cargo clippy -- -D warnings` clean.
- [ ] `cargo fmt --check` clean.

## Additional items surfaced in review

- **`payload_json` is diagnostic, not authoritative — add explicit test.** `test_payload_json_mutation_does_not_change_event_id` — manually UPDATE the `payload_json` column of an existing row to a different JSON shape, call `canonical_id` against the reconstructed `BeliefEvent`, assert event_id is unchanged. Proves the invariant that the canonical ID is over struct fields, not over the JSON text.
- **Replay tiebreaker for identical timestamps.** When two events share `timestamp`, replay order is `(timestamp ASC, event_id ASC)`. Add `test_replay_deterministic_with_tied_timestamps` — insert 100 events sharing the same `timestamp` millisecond, blow away projections, rebuild, assert projection state matches a second rebuild (deterministic under replay).
- **`caused_by` lineage queries.** Add `test_events_for_belief_follows_caused_by_chain` — insert event A; insert event B with `caused_by=A`; query "what events caused B" → returns `[A]`. Needed for future forensic tooling.
- **Observability hooks (X-1).** `cortex.append_event` span per `append_event` call with attributes `event_type`, `belief_id`, `session_id`, `canonical_ms` (hash compute time), `tx_ms` (insert + projection apply). `cortex.projection.apply` sub-span per `apply_in_tx` call with `event_type` and `projection_ms`.
- **No-recursion header propagation (X-2).** `append_event` is not itself an LLM call, but any event produced by rigor-internal calls (goal extraction, embedder, contradiction judge) must carry a `caused_by` reference to the parent request's canonical event if applicable, so debugging can trace the full chain.
- **Write-amplification benchmark.** `bench_append_event_write_amplification` — measure how many SQLite pages each event touches on average. Baseline for future capacity planning. Threshold: ≤ **5 pages/event** on typical mixed workload.
- **Forward-migration boundary test (X-5).** `test_open_with_schema_version_beyond_binary_refuses` — insert `PRAGMA user_version = 999`; open; daemon refuses with clear error.

## Dependencies

**Blocks:** EC-3, EC-4, EC-5, EC-6, EC-7, EC-8, EC-9, EC-10, EC-11, EC-12.
**Blocked by:** EC-1.

## References

- Umbrella: [UMBRELLA] Epistemic Cortex
- EC-1 (SQLite substrate + canonical format)
- `.planning/roadmap/epistemic-expansion-plan.md`
- Project memory: `project_dfquad_formula.md`, `project_epistemology_expansion.md`, `feedback_tdd.md`
