# EC-1: SQLite substrate — dependencies, pragmas, writer lock, first DDL, canonical event-ID format

> Part of umbrella: #34 [UMBRELLA] Epistemic Cortex
> Lands in: `crates/rigor/src/memory/epistemic/store/` and `crates/rigor/src/memory/epistemic/canonical.rs`

## Scope

Foundational infrastructure for the Epistemic Cortex. Ships zero user-visible behavior. After this lands:

- `~/.rigor/rigor.db` can be opened, created, and closed by the daemon and by CLI reader processes simultaneously.
- Schema migrations are managed via `refinery` + `PRAGMA user_version`.
- The writer-lock discipline prevents two daemon instances from both attempting writes.
- The `CanonicalHash` trait is in place with locked enum tag bytes for the first set of primitive types and the `BeliefEvent` envelope.
- A round-trip golden-file test exists for canonical-hash stability.

Nothing reads or writes belief events yet — that's EC-2. This issue only delivers the substrate.

## Design constraints pinned from the design thread

- **SQLite as primary store.** Not a dedicated vector DB, not a dedicated graph engine. `WITH RECURSIVE` for graph traversal; `sqlite-vec` for vectors (wired in EC-6); relational primitives for everything else.
- **Single shared DB** at `~/.rigor/rigor.db`. All sessions, all graphs, all events. Session is a read-time scoping column, not a filename.
- **WAL mode, single-writer (daemon), many-readers (CLI).** Writer discipline extends the existing `~/.rigor/daemon.pid` pattern.
- **`rusqlite` with `bundled` feature.** No system libsqlite dependency. Not `sqlx` (macro overhead not needed).
- **`refinery` for migrations.** Versioned SQL files, `PRAGMA user_version` tracked.
- **`r2d2` + `r2d2_sqlite` for connection pooling.** Needed so the daemon's async context can obtain sync SQLite connections via `spawn_blocking`.
- **Custom `CanonicalHash`, not `serde_jcs`.** ~10× perf target: <1μs per event vs. ~5–10μs for JCS. Streaming SHA-256 over length-prefixed typed fields with locked enum tag bytes. No intermediate String allocation; no JSON escaping; no sorted-key walk.
- **Tag bytes are permanent.** Once `BeliefAsserted = 0x01`, it is 0x01 forever. New variants get new tags; removed variants leave gaps.
- **`CANONICAL_FORMAT_VERSION` is the first byte hashed.** Version `0x01` is the initial value. Allows future format revisions without losing ID stability for old events.
- **Portable across sessions, not users.** No signing, no cross-user trust envelope. Schema is dialect-portable so eventual Postgres backend swap (Phase 4D) is a trait-impl change, not a schema rewrite.

## What lands

```
crates/rigor/Cargo.toml                                     (new deps)
crates/rigor/src/memory/mod.rs                              (re-export epistemic)
crates/rigor/src/memory/epistemic/
  ├── mod.rs                                                (pub use of submodules)
  ├── canonical.rs                                          (trait + primitive impls + tag registry)
  └── store/
      ├── mod.rs                                            (EpistemicStoreConfig, path resolution)
      ├── sqlite.rs                                         (connection manager + pool + writer lock)
      ├── schema.rs                                         (DDL string constants; loaded by refinery)
      └── migrations/
          └── V1__init.sql                                  (initial schema)

tests/
  └── epistemic_substrate.rs                                (e2e substrate tests)

benches/
  └── canonical_hash.rs                                     (perf benches vs. serde_jcs)
```

### Cargo additions

```toml
# crates/rigor/Cargo.toml
[dependencies]
# ... existing ...
rusqlite      = { version = "0.32", features = ["bundled", "blob", "functions", "hooks", "modern_sqlite", "backup", "serde_json"] }
sqlite-vec    = "0.1"                      # loaded at runtime; not wired in EC-1
refinery      = { version = "0.8", features = ["rusqlite"] }
r2d2          = "0.8"
r2d2_sqlite   = "0.25"
fs2           = "0.4"                       # file lock for writer discipline

[dev-dependencies]
serde_jcs     = "0.1"                       # comparison benchmark only
criterion     = { version = "0.5", features = ["html_reports"] }
tempfile      = "3.10"

[[bench]]
name    = "canonical_hash"
harness = false
```

## Schema contributions

Initial DDL (`migrations/V1__init.sql`). Later migrations add event-log and projection tables (EC-2), session and source tables (EC-3/EC-4), vector tables (EC-6). This file contains only the substrate tables: schema metadata and the writer lock row.

```sql
-- V1__init.sql
PRAGMA application_id = 0x52474F52;   -- 'RGOR' magic number for file-type detection
PRAGMA user_version   = 1;
PRAGMA foreign_keys   = ON;

-- Refinery's own migrations table is created automatically.

-- Writer discipline: only one process may hold the writer_lock row at a time.
-- Complementary to ~/.rigor/daemon.pid (fs-level) — database-level sanity check.
CREATE TABLE writer_lock (
  singleton   INTEGER PRIMARY KEY CHECK (singleton = 1),
  pid         INTEGER NOT NULL,
  acquired_at INTEGER NOT NULL,
  host        TEXT NOT NULL
) STRICT;
```

Pragmas applied at every connection open (set in Rust code, not DDL, because WAL and busy_timeout are per-connection settings):

```rust
// crates/rigor/src/memory/epistemic/store/sqlite.rs
const CONN_PRAGMAS: &[&str] = &[
    "PRAGMA journal_mode = WAL",
    "PRAGMA synchronous = NORMAL",
    "PRAGMA foreign_keys = ON",
    "PRAGMA busy_timeout = 5000",
    "PRAGMA temp_store = MEMORY",
    "PRAGMA mmap_size = 268435456",         // 256 MiB mmap for read-heavy access
    "PRAGMA wal_autocheckpoint = 1000",
];
```

## Trait surfaces

```rust
// crates/rigor/src/memory/epistemic/canonical.rs

use sha2::{Digest, Sha256};

/// Current format version byte. Hashed first in every canonical encoding.
/// Locked once released; revved only via coordinated migration.
pub const CANONICAL_FORMAT_VERSION: u8 = 0x01;

/// Stream canonical bytes directly into a SHA-256 hasher.
/// No serialization to String, no intermediate allocation.
pub trait CanonicalHash {
    fn hash_into(&self, h: &mut Sha256);
}

// Primitive impls shipped in EC-1. Payload-specific impls land in EC-2.

impl CanonicalHash for u8 {
    fn hash_into(&self, h: &mut Sha256) { h.update(&[*self]); }
}

impl CanonicalHash for u32 {
    fn hash_into(&self, h: &mut Sha256) { h.update(&self.to_le_bytes()); }
}

impl CanonicalHash for u64 {
    fn hash_into(&self, h: &mut Sha256) { h.update(&self.to_le_bytes()); }
}

impl CanonicalHash for i64 {
    fn hash_into(&self, h: &mut Sha256) { h.update(&self.to_le_bytes()); }
}

impl CanonicalHash for f64 {
    fn hash_into(&self, h: &mut Sha256) {
        assert!(!self.is_nan(), "non-canonical: NaN in event field");
        h.update(&self.to_bits().to_le_bytes());   // bit-exact; no locale or formatting drift
    }
}

impl CanonicalHash for bool {
    fn hash_into(&self, h: &mut Sha256) { h.update(&[*self as u8]); }
}

impl CanonicalHash for str {
    fn hash_into(&self, h: &mut Sha256) {
        h.update(&(self.len() as u32).to_le_bytes());
        h.update(self.as_bytes());
    }
}

impl CanonicalHash for String {
    fn hash_into(&self, h: &mut Sha256) { self.as_str().hash_into(h); }
}

impl<T: CanonicalHash> CanonicalHash for Option<T> {
    fn hash_into(&self, h: &mut Sha256) {
        match self {
            None => h.update(&[0u8]),
            Some(v) => { h.update(&[1u8]); v.hash_into(h); }
        }
    }
}

impl<T: CanonicalHash> CanonicalHash for Vec<T> {
    fn hash_into(&self, h: &mut Sha256) {
        h.update(&(self.len() as u32).to_le_bytes());
        for item in self { item.hash_into(h); }
    }
}

impl CanonicalHash for [u8; 32] {
    fn hash_into(&self, h: &mut Sha256) { h.update(self); }
}

/// Compute canonical SHA-256 of a CanonicalHash value.
/// Prepends CANONICAL_FORMAT_VERSION so future format revs don't collide.
pub fn canonical_id<T: CanonicalHash>(v: &T) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(&[CANONICAL_FORMAT_VERSION]);
    v.hash_into(&mut h);
    h.finalize().into()
}
```

```rust
// crates/rigor/src/memory/epistemic/store/sqlite.rs

use std::path::{Path, PathBuf};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

pub struct EpistemicStoreConfig {
    pub db_path: PathBuf,           // default: ~/.rigor/rigor.db
    pub read_only: bool,            // CLI readers pass true; daemon passes false
    pub pool_size: u32,             // default 4
}

pub struct SqliteSubstrate {
    pool: Pool<SqliteConnectionManager>,
    writer_lock: Option<WriterLock>,
}

impl SqliteSubstrate {
    /// Open substrate. Applies migrations. Acquires writer lock if !read_only.
    pub fn open(cfg: EpistemicStoreConfig) -> anyhow::Result<Self>;

    /// Acquire a connection from the pool. Read-only or writer depending on config.
    pub fn conn(&self) -> anyhow::Result<r2d2::PooledConnection<SqliteConnectionManager>>;

    /// Run a closure with a transaction. Only available to writers.
    pub fn with_tx<F, T>(&self, f: F) -> anyhow::Result<T>
    where F: FnOnce(&rusqlite::Transaction) -> anyhow::Result<T>;

    /// Invoke PRAGMA wal_checkpoint(PASSIVE). Called periodically by daemon.
    pub fn checkpoint(&self) -> anyhow::Result<()>;

    /// Backup/export to a target file. Uses SQLite BACKUP API (online, non-blocking readers).
    pub fn backup_to(&self, dest: &Path) -> anyhow::Result<()>;
}

struct WriterLock {
    /// fs2 file lock on ~/.rigor/rigor.db.writer.lock (separate file from daemon.pid).
    /// Dropped on drop. Additionally we write a row into the writer_lock DB table
    /// as a defence-in-depth check against stale fs locks.
    _file: std::fs::File,
    path: PathBuf,
}
```

## Event types introduced

**None in EC-1.** EC-2 introduces `BeliefEvent` and its variants. This issue only delivers the `CanonicalHash` trait and primitive-type impls; the envelope type and enum tag registry land in EC-2.

## Implementation notes & invariants

**Invariant 1: pragmas are applied on every connection.** `r2d2_sqlite`'s connection customizer runs the `CONN_PRAGMAS` block at every connection open. WAL mode persists across opens (it's a database-level setting after the first write in WAL), but `busy_timeout`, `foreign_keys`, `temp_store`, `mmap_size` are per-connection.

**Invariant 2: writer lock is two-layered.** Both layers must succeed for daemon startup:
1. `fs2::FileExt::try_lock_exclusive` on `~/.rigor/rigor.db.writer.lock`. Catches another daemon running on same machine.
2. `INSERT INTO writer_lock VALUES (1, ?, ?, ?) ON CONFLICT DO NOTHING`. Catches stale fs locks (rare; occurs if a prior daemon was SIGKILL'd).
If either fails, the daemon refuses to start with a clear error mentioning the existing holder PID and host.

**Invariant 3: read-only connections set `query_only`.** `PRAGMA query_only = 1` on every read-only pool connection. Defense against a CLI process accidentally attempting a write.

**Invariant 4: `CANONICAL_FORMAT_VERSION = 0x01` is permanent.** It cannot change in this landing. Any future change requires a new byte value (0x02) and a migration plan for replay.

**Invariant 5: no `Debug`-derived hashing.** `CanonicalHash` is always hand-written; no `#[derive(CanonicalHash)]` macro in EC-1. Tag bytes must remain visible in source code for review.

**Invariant 6: `NaN` rejection in `f64::hash_into`.** The function panics on NaN. There are no legitimate uses of NaN in any epistemic event field; surfacing the bug loudly is better than hashing whatever `to_bits()` produced.

**Operational detail: migration ordering.** `refinery` runs `V1__init.sql` once. Subsequent landings append `V2`, `V3`, etc. Migrations are idempotent and never re-edit published files.

**Operational detail: `application_id`.** SQLite's `PRAGMA application_id` is stored in the header and unchanged across writes. This allows `file` and other tools to identify rigor databases. The value `0x52474F52` is 'RGOR' in ASCII.

**Operational detail: WAL autocheckpoint.** `wal_autocheckpoint = 1000` means SQLite auto-checkpoints every 1000 pages. For rigor's write volume this keeps the WAL bounded at <4 MiB typical. A periodic `PRAGMA wal_checkpoint(PASSIVE)` from the daemon's tick loop (added in EC-9) handles edge cases.

## Unit testing plan

All tests live in `crates/rigor/src/memory/epistemic/canonical.rs` (tests module) or `crates/rigor/src/memory/epistemic/store/sqlite.rs` (tests module) for co-located unit tests, with integration-spanning cases in `tests/epistemic_substrate.rs`.

**`canonical.rs` tests:**

- `test_canonical_primitive_u8_round_trip` — u8 hashes deterministically; same input → same bytes.
- `test_canonical_primitive_u32_le_order` — u32 hashes as little-endian; platform-independent.
- `test_canonical_primitive_i64_negative` — negative i64 round-trips via to_le_bytes.
- `test_canonical_primitive_f64_nan_panics` — NaN f64 panics (asserts `panic::catch_unwind` catches the expected message).
- `test_canonical_primitive_f64_bit_exact` — 0.1 + 0.2 f64 produces the exact bit pattern every run.
- `test_canonical_primitive_bool` — true = 0x01, false = 0x00.
- `test_canonical_string_len_prefixed` — "hello" hashes as [5,0,0,0,'h','e','l','l','o'].
- `test_canonical_empty_string` — "" hashes as [0,0,0,0].
- `test_canonical_option_none_tag` — None begins with byte 0x00.
- `test_canonical_option_some_tag` — Some begins with byte 0x01.
- `test_canonical_option_vs_none_hash_differ` — Some(0u32) ≠ None (length-prefixed distinguishes).
- `test_canonical_vec_len_prefixed` — vec![1u32, 2u32, 3u32] begins with len=3 u32 LE.
- `test_canonical_vec_empty` — empty vec hashes as [0,0,0,0].
- `test_canonical_bytes32` — `[u8;32]` hashes as the bytes themselves (no length prefix; fixed-size).
- `test_canonical_format_version_prepended` — `canonical_id(&42u32)` first byte fed into hasher is `CANONICAL_FORMAT_VERSION`.
- `test_canonical_id_stable_across_runs` — golden file at `tests/fixtures/canonical_golden.bin`. Test recomputes hashes for a fixed set of primitive values and compares to the stored bytes. Any drift → test fails → review is forced.

**`store/sqlite.rs` tests (in-module):**

- `test_config_default_path_is_home_rigor_db` — `EpistemicStoreConfig::default().db_path` is `~/.rigor/rigor.db`.
- `test_open_creates_db_file_if_absent` — on first open, the file is created and migration V1 runs.
- `test_open_applies_pragmas` — after open, `SELECT * FROM pragma_journal_mode` returns "wal"; `pragma_foreign_keys` returns 1; `pragma_busy_timeout` returns 5000.
- `test_open_sets_application_id` — `SELECT * FROM pragma_application_id` returns 0x52474F52.
- `test_open_sets_user_version_1` — `SELECT * FROM pragma_user_version` returns 1.
- `test_writer_lock_acquired_on_writer_open` — after opening as writer, the `writer_lock` row exists with the current PID.
- `test_writer_lock_rejected_when_held` — second writer open on same path returns an error naming the existing holder PID.
- `test_read_only_does_not_acquire_writer_lock` — read-only open does not insert into `writer_lock` and coexists with an existing writer.
- `test_read_only_rejects_writes` — a `PRAGMA query_only` violation is reported if a read-only conn attempts an INSERT.
- `test_pool_size_respected` — pool at size 4 caps connections at 4.
- `test_backup_to_produces_valid_db` — `backup_to(temp)` then open `temp` read-only and `SELECT 1` succeeds.
- `test_checkpoint_is_noop_without_writes` — running checkpoint on an empty WAL succeeds and returns.

## E2E testing plan

Tests live in `tests/epistemic_substrate.rs` — a full crate-level integration test. Uses `tempfile::tempdir()` for isolated DB paths so tests run in parallel without interference.

**`e2e_substrate_migrations_idempotent_on_reopen`:**
- Open substrate; verify V1 migration runs.
- Close.
- Reopen; verify V1 is *not* re-run (refinery's applied-migrations table shows the same row count).
- Assert pragmas still set, writer_lock row present with updated PID.

**`e2e_substrate_writer_plus_reader_concurrency`:**
- Daemon-role process opens as writer; creates a test row in `writer_lock` via side-channel.
- Spawn three reader processes (tokio tasks in the same test) that each open as read-only and query `SELECT COUNT(*) FROM writer_lock`.
- All three readers succeed; writer lock unaffected.
- Writer closes; readers still return correct snapshot state until their own connections close.

**`e2e_substrate_stale_fs_lock_reclaim`:**
- Open as writer; obtain `WriterLock`.
- Simulate SIGKILL: forget the WriterLock struct without dropping it (leaves fs lock but daemon PID disappears).
- Open again as writer, pre-passing a recovery flag that checks if the PID in `writer_lock` table is alive via `kill(pid, 0)`.
- On dead PID: recovery succeeds, writer lock is reclaimed. (The recovery flag is opt-in and surfaced in CLI as `rigor db reclaim`.)

**`e2e_substrate_wal_checkpoint_bounded`:**
- Open as writer.
- Insert 10,000 rows into `writer_lock` via transient stress test (relax the singleton constraint for test only, or create a throwaway stress table).
- Verify WAL file size stays below 16 MiB due to `wal_autocheckpoint = 1000`.

**`e2e_substrate_canonical_golden_file`:**
- Load `tests/fixtures/canonical_golden.bin` (pre-computed test vectors from a known-good run).
- Recompute each vector's canonical SHA-256 via `canonical_id`.
- All must match byte-for-byte.
- On mismatch, test fails with a diff showing expected vs. actual bytes. This is the canary for accidental format drift.

**`e2e_substrate_crash_mid_write_rolls_back`:**
- Open as writer.
- Begin a transaction; insert into `writer_lock` (via test helper, not production path).
- Drop the transaction without committing; assert the row is not present in the reader's subsequent SELECT.
- This verifies SQLite's atomic-commit semantics are actually in force (confirms WAL is doing its job).

## Performance testing plan

Benches live in `benches/canonical_hash.rs` using `criterion`. Run via `cargo bench --bench canonical_hash`.

**Benchmark 1: CanonicalHash vs serde_jcs for a representative payload.**

Synthetic struct with 5 String fields (mean 32 bytes), 3 i64 fields, 2 f64 fields, 1 Option<String>, 1 Vec<String> of 3 items. Size ≈ 300 bytes.

- `bench_canonical_hash_primitive_payload` — hashes the payload via `canonical_id` 1M iters.
- `bench_serde_jcs_hash_primitive_payload` — serializes via `serde_jcs` then `SHA-256` the result, 1M iters.

**Thresholds:**
- `canonical_hash` p50 ≤ **500ns** per invocation.
- `canonical_hash` p99 ≤ **1.5μs** per invocation.
- `serde_jcs + sha256` should land at **5–15μs** range (baseline for comparison; no strict threshold, just illustration).
- Speedup factor `jcs / canonical` ≥ **8×**.

**Benchmark 2: allocation counts.**

Using `dhat-rs` or instrumented allocator. Measures allocations per single `canonical_id(&payload)` call.

- **Threshold:** zero allocations beyond the initial Sha256 state. `canonical_id` must not allocate Strings, Vecs, or maps during hashing.

**Benchmark 3: WAL write throughput baseline.**

`bench_wal_insert_writer_lock_noop_tx` — single-threaded writer inserting 10,000 rows into a throwaway stress table inside a single transaction.

- **Threshold:** ≥ **10,000 rows/sec** on a modern SSD with `synchronous = NORMAL`. Included as a substrate sanity check, not a production write pattern (EC-2 introduces the production path).

**Benchmark 4: connection acquisition latency.**

`bench_pool_acquire_read_only_concurrent` — 4 concurrent tasks each acquiring a read-only connection from the pool.

- **Threshold:** p99 ≤ **1ms** under no contention; p99 ≤ **5ms** under 4-way contention.

**Benchmark 5: open/close latency.**

`bench_substrate_cold_open` — cold open of a 100 MiB database (pre-populated with test rows).

- **Threshold:** cold open ≤ **100ms**. Warm reopen (OS page cache hot) ≤ **20ms**.

Bench results are tracked via `criterion`'s HTML reports and committed alongside the benchmark code as `benches/baselines/canonical_hash.json`. CI asserts no regression ≥ 20% vs. the committed baseline.

## Acceptance criteria

- [ ] Cargo deps added with correct feature flags. `cargo build --release` succeeds on Linux + macOS.
- [ ] `crates/rigor/src/memory/epistemic/mod.rs` re-exports `store`, `canonical`.
- [ ] `CanonicalHash` trait defined with primitive impls for `u8`, `u32`, `u64`, `i64`, `f64`, `bool`, `str`, `String`, `Option<T>`, `Vec<T>`, `[u8; 32]`.
- [ ] `CANONICAL_FORMAT_VERSION = 0x01` constant exported.
- [ ] `canonical_id` helper prepends the version byte before hashing.
- [ ] `f64::hash_into` panics on NaN with a descriptive message.
- [ ] `SqliteSubstrate::open` creates or opens `~/.rigor/rigor.db`.
- [ ] `V1__init.sql` migration applied on first open; not re-applied on reopen.
- [ ] All 7 `CONN_PRAGMAS` set on every connection.
- [ ] `application_id = 0x52474F52` and `user_version = 1` persisted.
- [ ] Writer lock acquired on writer open; released on drop.
- [ ] Writer lock table row correctly populated with PID, host, acquired_at.
- [ ] Read-only connections set `query_only = 1`.
- [ ] Two concurrent daemon opens → second returns an error referencing the first's PID.
- [ ] Unit tests pass: all 25 tests listed in the Unit Testing Plan.
- [ ] E2E tests pass: all 6 tests listed in the E2E Testing Plan.
- [ ] Perf benchmarks meet all 5 thresholds; baseline committed.
- [ ] Golden-file test `canonical_golden.bin` exists and passes.
- [ ] No new `unsafe` code.
- [ ] `cargo clippy -- -D warnings` clean.
- [ ] `cargo fmt --check` clean.

## Additional items surfaced in review

- **`SqliteSubstrate::backup_to` test coverage.** Add `test_backup_to_produces_valid_db` (already in the plan — confirm implementation includes reading the exported file and a smoke query). Add `test_backup_to_atomicity` — simulate interruption mid-backup; assert source DB unaffected.
- **Read-only filesystem handling.** `test_open_on_read_only_filesystem_surfaces_clear_error` — simulate by pointing at a path under `/tmp` with chmod 444 (or Windows read-only attribute). Daemon must refuse to start with a message naming the path and the OS error. Must not crash.
- **Cross-platform CI note.** Benchmarks run on Linux x86_64 as primary; macOS arm64 as secondary. Windows best-effort per X-4 in umbrella. `fs2` file locking verified on both POSIX and Windows.
- **Observability hooks (X-1).** `cortex.substrate.open` span on `SqliteSubstrate::open` with attributes `db_path`, `read_only`, `pool_size`, `cold_open_ms`. Writer-lock acquisition emits `cortex.substrate.lock_acquired` with `pid`, `host`. Future layers build on this.
- **Schema version forward-migration (X-5).** Test `test_open_against_newer_user_version_refuses_clearly` — create DB with `PRAGMA user_version = 999`, attempt to open, assert clear error naming both the DB version and the binary's supported version range.

## Dependencies

**Blocks:** EC-2, EC-3, EC-4, EC-5, EC-6, EC-7, EC-8, EC-9, EC-10, EC-11, EC-12.
**Blocked by:** None.

## References

- Umbrella: [UMBRELLA] Epistemic Cortex
- `.planning/roadmap/epistemic-expansion-plan.md`
- `src/memory/content_store.rs` — existing pluggable-backend pattern
- `src/daemon/governance.rs` — existing file-lock discipline for daemon.pid
- Project memory: `feedback_tdd.md`
