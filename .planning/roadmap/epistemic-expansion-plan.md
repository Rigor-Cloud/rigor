# Epistemic Expansion Plan

**Version:** v4.1
**Status:** proposed — awaiting approval on Phase 0
**Scope:** integrate Headroom-style compression, Graphify-style knowledge graph (non-multimodal), GEPA-calibrated evaluators, Rigor Learn offline analyzer, and the forward epistemology-expansion roadmap into a single coherent plan.

## Table of Contents

1. [Overview](#1-overview)
2. [Current State — Existing Epistemic System](#2-current-state--existing-epistemic-system)
3. [Target State — Future Epistemic System](#3-target-state--future-epistemic-system)
4. [Integration Thesis](#4-integration-thesis)
5. [Phase 0 — Schema and Infrastructure](#5-phase-0--schema-and-infrastructure)
6. [Phase 1 — Headroom Compression and CCR Audit](#6-phase-1--headroom-compression-and-ccr-audit)
7. [Phase 1.5 — Rigor Learn](#7-phase-15--rigor-learn)
8. [Phase 2 — Graphify / Knowledge Types / Gettier Guards](#8-phase-2--graphify--knowledge-types--gettier-guards)
9. [Phase 3 — GEPA Rust-Native Optimizer](#9-phase-3--gepa-rust-native-optimizer)
10. [Phase 4 — Forward Epistemology Integration](#10-phase-4--forward-epistemology-integration)
11. [Dependency Graph](#11-dependency-graph)
12. [Shipping Order](#12-shipping-order)
13. [Out of Scope](#13-out-of-scope)
14. [Appendix — Key References](#14-appendix--key-references)

---

## 1. Overview

Rigor today sits between AI coding agents and LLM providers as an LD_PRELOAD + TLS MITM proxy, extracts claims from streamed responses, evaluates them against an argumentation graph (DF-QuAD), and blocks / warns / allows based on computed strengths. The core pipeline is in place; the epistemic system underneath it is flat (no knowledge-type taxonomy, no credibility weighting, no dynamic base strength, hardcoded DF-QuAD defaults).

This plan brings four related initiatives together under a single architecture:

- **Headroom** (`github.com/chopratejas/headroom`) — content-aware request compression, reversible via content-addressable retrieval, with prompt-cache alignment. Adopted as rigor's audit-trail substrate rather than a bolt-on compressor.
- **Graphify** (non-multimodal parts only) — knowledge-type classification, confidence-tagged edges, Leiden clustering, Gettier guards. Adopted as rigor's implementation of the epistemology-expansion roadmap's schema layer.
- **GEPA** (Agarwal et al., 2024 — Genetic-Pareto prompt optimization) — calibrate LLM-as-judge prompts against human-annotated violation data. Adopted as rigor's evaluator-calibration loop, ported natively to Rust.
- **Rigor Learn** — offline analyzer that reads rigor's own session data + agent conversation logs and emits recommendations to CLAUDE.md / MEMORY.md / rigor.yaml. Patterned after `headroom learn`.

Every item in this plan plugs into one of three existing extension points:

- **Request-side:** new `EgressFilter` impls registered with the `FilterChain` at `crates/rigor/src/daemon/proxy.rs:1135`. Trait at `crates/rigor/src/daemon/egress/chain.rs:42`.
- **Response-side:** `apply_response_chunk` / `finalize_response` on the same trait (`chain.rs:56-71`), plus the inline SSE handler in `proxy.rs`.
- **Evaluator-side:** new `ClaimEvaluator` impls registered via `EvaluatorPipeline::register` (`crates/rigor/src/evaluator/pipeline.rs:280`).

Cross-stage state flows through `ConversationCtx::scratch` (`crates/rigor/src/daemon/egress/ctx.rs:76`) — a typed key/value map already implemented.

---

## 2. Current State — Existing Epistemic System

### 2.1 Core domain types

**`Claim`** — `crates/rigor/src/claim/types.rs:11-23`
- `id: String` (UUID), `text: String`, `domain: Option<String>`, `confidence: f64`
- `claim_type: ClaimType` (enum at `claim/types.rs:48-55`: `Assertion | Negation | CodeReference | ArchitecturalDecision | DependencyClaim | ActionIntent`) — **rule-derived intent axis, not an epistemology axis**.
- `source: Option<SourceLocation>` where `SourceLocation = { message_index, sentence_index }` (`claim/types.rs:5-8`). **Only these two fields — no span offsets, no token ranges.**
- `source_line: Option<usize>` is declared but never populated (dead field).

**`Constraint`** — `crates/rigor/src/constraint/types.rs:36-53`
- `id`, `epistemic_type: EpistemicType` (`Belief | Justification | Defeater` at `types.rs:76-80`)
- `name`, `description`, `message`, `rego`, `domain` — all strings
- `tags: Vec<String>` (route to `SemanticEvaluator` via tag `semantic`)
- `references: Vec<String>`
- `source: Vec<SourceAnchor>` where `SourceAnchor = { path, lines, anchor, context }` (`types.rs:59-71`). **No file hash, no commit pin, no last_verified.**

**`RigorConfig`** — `crates/rigor/src/constraint/types.rs:4-10`
- Just `constraints: ConstraintsSection` (beliefs / justifications / defeaters) + `relations: Vec<Relation>`.
- **No schema version, no clusters, no knowledge-type groupings.**

**`Relation`** — `crates/rigor/src/constraint/types.rs:84-88`
- `from`, `to`, `relation_type: Supports | Attacks | Undercuts`.
- **No confidence, no weight** — all edges implicitly weight-1.0.

### 2.2 DF-QuAD engine

`crates/rigor/src/constraint/graph.rs`:
- Product-of-complements aggregation at `:101-109` (preserves current correctness per `project_dfquad_formula.md`).
- Two-case influence function at `:115-123`.
- **Base strengths hardcoded per `EpistemicType` at `:50-54`** (Belief=0.8, Justification=0.9, Defeater=0.7). No per-constraint override.
- Deterministic via `BTreeMap`, fixed-point iteration `EPSILON=0.001`, max 100 iterations.
- Regression guard test at `:447` verifies product-vs-mean.
- `Undercuts` is simplified to "attack on target node" for v0.1 (`:249-253`).

### 2.3 Claim extraction

Stop-hook path: `lib.rs:112-407` → `claim/transcript.rs:34-80` → `claim/extractor.rs:43-56` → `claim/heuristic.rs:157-185`:
1. `strip_code_blocks`
2. `unicode_sentences()` from `unicode-segmentation`
3. Filter via `is_assertion` (`heuristic.rs:48-97`)
4. Filter via `!is_hedged` (`hedge_detector.rs:9-18`)
5. Assign `confidence` (`confidence.rs:24-37`) — negation=0.8 / definitive=0.9 / default=0.7
6. Classify `claim_type` via keyword-based `classify_claim_type` (`heuristic.rs:100-147`)

Streaming path: `daemon/proxy.rs:3074+` (`extract_sse_assistant_text`) parses Anthropic + OpenAI SSE; claims are produced incrementally inside the streaming handler (`proxy.rs:1517-1542`) using the same batch extractor against accumulated text.

### 2.4 Evaluator pipeline

`crates/rigor/src/evaluator/pipeline.rs`:
- `ClaimEvaluator` trait at `:65-77`, `EvalResult` at `:33-55`.
- `RegexEvaluator` at `:82-148` — catch-all for constraints with non-empty `rego`.
- `SemanticEvaluator` at `:168-231` — reads cached verdicts from `RelevanceLookup` (`relevance.rs:35-39`).
- Routing at `:301-327` — first `can_evaluate` match wins, fallback `RegexEvaluator`.

LLM-as-judge: `score_claim_relevance` at `proxy.rs:2871-3070`, prompt at `:2931-2938`. Rate-limited single-concurrent via `SimpleSemaphore` (`proxy.rs:2836-2847`). `RELEVANCE_CACHE` at `proxy.rs:2850-2851` is a `Mutex<HashMap>` keyed by **claim text**, not claim ID. Lost on daemon restart.

### 2.5 Audit trail

**`~/.rigor/violations.jsonl`** — `ViolationLogEntry` at `logging/types.rs:30-78`:
- Session metadata (id, timestamp, git_commit, git_dirty)
- `constraint_id`, `constraint_name`, `claim_ids`, `claim_text`
- `base_strength`, `computed_strength`, `severity`, `decision`, `message`
- `supporters`, `attackers` — constraint IDs adjacent in the graph
- `transcript_path`, `claim_confidence`, `claim_type`, `claim_source`
- `false_positive: Option<bool>`, `annotation_note: Option<String>` — human correction fields
- `model: Option<String>` — which LLM produced the response

**Original request/response bodies are NOT persisted.** Only claim text and message.

**`~/.rigor/memory.json`** — `MemoryStore` (`memory/episodic.rs:67-93`), rebuilt deterministically from violations.jsonl on every `build_epistemic_context` call (`memory/episodic.rs:96-102`). This is the existing feedback loop: annotations automatically re-enter the system prompt on subsequent requests.

### 2.6 Request pipeline plumbing

- `FilterChain` applied only on request for system-prompt injection. `apply_response_chunk` / `finalize_response` trait methods exist but are **never invoked by the proxy** today — response-side work lives directly in `proxy.rs:1517-1644` as inline code.
- Only one filter registered: `ClaimInjectionFilter` at `egress/claim_injection.rs:15-62`.
- Action gates (`daemon/gate.rs`) fire on `ClaimType::ActionIntent` — **orthogonal** to constraint evaluation.

### 2.7 LSP

`crates/rigor/src/lsp/` scaffolding exists but is **not wired into constraint verification**.

---

## 3. Target State — Future Epistemic System

From memory files `project_epistemology_expansion.md`, `project_epistemic_sandbox.md`, `project_rigor_as_platform.md`:

1. **Knowledge types on claims and constraints:** empirical (code-anchored, verified) > rational (DF-QuAD derived) > testimonial (docs/LLM/README) > memory (prior `rigor map` run).
2. **Justification tracking:** empirical (grep/LSP verifies anchor), rational (DF-QuAD propagation), constructivist (`/rigor:map`).
3. **Gettier guards:** anchor patterns ensure the justification connects to truth, not just "justified + true but not known."
4. **Induction tracking:** `last_verified`, `verification_count`, `verified_at_commit` — a 50-verification-count constraint is inductively stronger.
5. **Credibility scoring for testimony:** which model judged (Opus > Sonnet > Haiku), `credibility_weight`, timestamp.
6. **Dynamic DF-QuAD base strength:** empirical+recent → high; testimonial+low-credibility → low; memory+old → decayed.
7. **LSP over tree-sitter** for anchor verification — decision already made. Rust (rust-analyzer), TS/JS (typescript-language-server), Python (pyright), Go (gopls).
8. **Audit trail → Postgres** (Rigor Cloud).
9. **Custom judge model on Modal.**
10. **Action gates integrated with constraint eval.**

---

## 4. Integration Thesis

**Headroom, Graphify, and GEPA are not three parallel features — they are three implementation layers of the epistemology-expansion roadmap.** Mapping:

| Future epistemology piece | Implemented by |
|---|---|
| Knowledge types on claims + constraints | Graphify Phase 2A (classifier) + schema in Phase 0A |
| Confidence-tagged edges | Graphify schema Phase 0C |
| Induction tracking (last_verified, verification_count) | GEPA Phase 3A annotation emission + Phase 3F promotion |
| Credibility scoring | GEPA Phase 3B prompt registry versions credibility |
| Dynamic DF-QuAD base strength | Phase 4B formula, Phase 4G calibration |
| Gettier guards | Graphify Phase 2D + Phase 4A LSP wiring |
| Audit trail (content-addressable, reversible) | Headroom Phase 1 content_store = audit substrate |
| Postgres migration | Phase 4D (Phase 0I trait isolates the change) |
| Custom judge on Modal | Phase 4E (feeds from Phase 3E GEPA output) |
| LSP anchor verification | Phase 4A (seeded by Phase 2D partial use) |

**The unifying substrate is `ConversationCtx::scratch` + the content store + the violation log.** Every stage reads and writes through these three; every stage contributes to the audit trail.

---

## 5. Phase 0 — Schema and Infrastructure

All subsequent phases depend on Phase 0. Backward-compatible because serde_yml silently ignores unknown keys today.

### 5.1 [0A] Knowledge-type taxonomy

New enum alongside existing `ClaimType`:

```rust
// crates/rigor/src/claim/types.rs
pub enum KnowledgeType {
    Empirical,   // Code-anchored, verified by grep/LSP
    Rational,    // Derived via DF-QuAD propagation
    Testimonial, // From docs, LLM, README
    Memory,      // Reused from prior rigor map run or prior session
}
```

- Add `knowledge_type: Option<KnowledgeType>` to `Claim` (`claim/types.rs:11-23`).
- Add `knowledge_type: Option<KnowledgeType>` to `Constraint` (`constraint/types.rs:36-53`).
- **Do not modify** `ClaimType` — it remains the intent axis. `KnowledgeType` is the epistemology axis.

### 5.2 [0B] Dynamic strength fields on Constraint

Additions to `Constraint`, all `Option<T>` for backward compatibility:

```rust
base_strength_override: Option<f64>,      // Overrides graph.rs:50-54 defaults
last_verified: Option<DateTime<Utc>>,
verification_count: u64,                  // serde default=0
verified_at_commit: Option<String>,
credibility_weight: Option<f64>,          // For testimonial constraints
cluster_id: Option<String>,               // Phase 2C Leiden
```

### 5.3 [0C] Confidence-tagged edges

Additions to `Relation` (`constraint/types.rs:84-88`):

```rust
confidence: f64,                          // serde default=1.0
extraction_method: Option<ExtractionMethod>,  // Ast | Llm | Inferred | Manual
```

DF-QuAD changes in `constraint/graph.rs:169-187`: multiply each attacker/supporter node strength by the incoming edge's `confidence` before computing the complement. Default=1.0 preserves existing tests. Add new `test_dfquad_weighted_edges` alongside `:447`.

### 5.4 [0D] Source fingerprinting on SourceAnchor

Additions to `SourceAnchor` (`constraint/types.rs:59-71`):

```rust
file_sha256: Option<String>,       // Populated by rigor map
anchor_sha256: Option<String>,     // Hash of anchor substring at verification
```

Invalidation rule: if `file_sha256` changes and `anchor_sha256` no longer matches on re-verify, reset `last_verified` and stop incrementing `verification_count`.

### 5.5 [0E] Content store

New `crates/rigor/src/memory/content_store.rs`.

- Hash-keyed (`[u8; 32]` SHA256) with categorized partitions: `audit` (permanent), `compression` (5min TTL), `verdict` (24h TTL), `annotation` (permanent).
- In-memory backend: `dashmap` + `moka`.
- API: `store(bytes, category, ttl) -> hash`, `retrieve(hash) -> Option<bytes>`, `search(hash, query) -> ranked fragments` via BM25.
- One instance on `DaemonState`, handed to filters via `Arc`.
- Extends `memory/episodic.rs:163-179` — `MemoryStore::from_entries` gains an optional second pass that hydrates content hashes referenced by violation entries, so audited claims can be replayed with original context in future system-prompt injection.

### 5.6 [0F] Frozen-prefix invariant + canonicalizer

New `crates/rigor/src/daemon/egress/frozen.rs`:

```rust
pub struct FrozenPrefix {
    pub message_count: usize,
    pub byte_checksum: u64,
}
```

Stored via `ctx.scratch_set` (`egress/ctx.rs:76`). Post-chain verifier in `FilterChain::apply_request` recomputes checksum over `messages[0..message_count]`; divergence panics in debug, logs and rejects in release.

New `crates/rigor/src/daemon/egress/canonical.rs`:
- `CanonicalizeToolsFilter` — stable-sort `tools[]` using `serde_json_canonicalizer`
- `DynamicContentFilter` — extract UUIDs, timestamps, trace-IDs from system prompt into `---\n[Dynamic Context]\n` tail

Both run as first stages in the chain — downstream filters see canonical input.

### 5.7 [0G] Wire FilterChain into response path

Today `apply_response_chunk` / `finalize_response` are trait methods nobody calls. Add the chain invocation at the relevant points in `proxy.rs` around `:1517-1644`. Existing inline code continues to work because the only registered filter's response methods are no-op defaults.

This unlocks Phases 1B, 3A without duplicating SSE handler logic.

### 5.8 [0H] ONNX host

New `crates/rigor/src/memory/onnx_host.rs`.

- Wraps `ort` crate (Rust onnxruntime) with HF-Hub download + SHA-verified local cache at `~/.rigor/models/<hash>/`.
- Shared between Phase 1D Kompress and Phase 4F safety discriminator so `ort` is compiled once.
- Feature-gated: `compression-ml` OR `safety-discriminator` enables it.

### 5.9 [0I] Backend abstraction traits

Enables Phase 4D Postgres drop-in.

```rust
pub trait ContentStoreBackend: Send + Sync {
    async fn store(&self, bytes: &[u8], category: Category, ttl: Option<Duration>) -> Result<Hash>;
    async fn retrieve(&self, hash: &Hash) -> Result<Option<Vec<u8>>>;
    async fn search(&self, hash: &Hash, query: &str) -> Result<Vec<SearchResult>>;
    async fn list_by_category(&self, category: Category) -> Result<Vec<Hash>>;
}

pub trait ViolationLogBackend: Send + Sync {
    async fn append(&self, entry: &ViolationLogEntry) -> Result<()>;
    async fn query(&self, filter: LogFilter) -> Result<Vec<ViolationLogEntry>>;
    async fn annotate(&self, id: LogId, false_positive: Option<bool>, note: Option<String>) -> Result<()>;
    async fn rewrite(&self, transform: impl Fn(&ViolationLogEntry) -> Option<ViolationLogEntry>) -> Result<()>;
}

pub trait SessionRegistryBackend: Send + Sync { ... }
```

All three ship in Phase 0 with JSONL / in-memory default impls (today's behavior). Postgres impls deferred to 4D.

### 5.10 [0J] Corpus exporter

Needed by Phase 3E (GEPA training) and Phase 4E (Modal training).

New `crates/rigor/src/refine/corpus.rs`:

```rust
pub struct CorpusRow {
    pub request_hash: String,              // → content_store audit category
    pub claim_text: String,
    pub constraint_id: String,
    pub knowledge_type: Option<KnowledgeType>,
    pub label: VerdictLabel,               // Block | Warn | Allow
    pub human_corrected: Option<bool>,     // From false_positive
    pub reasoning: String,                 // From ViolationLogEntry.message
    pub model_that_produced: Option<String>,
    pub evaluator_version: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn export_corpus(
    cfg: ExportConfig,
    log_backend: &dyn ViolationLogBackend,
) -> Result<impl Stream<Item = CorpusRow>>;
```

CLI: `rigor refine export --constraint X --since <date> [--format jsonl|parquet]`

---

## 6. Phase 1 — Headroom Compression and CCR Audit

**Reframing:** every compression is an audit event; every CCR retrieval is a provenance lookup. This phase turns rigor's proxy into a byte-for-byte replayable audit layer.

### 6.1 [1A] Request-side filter chain

Filters pushed into the chain at `proxy.rs:1135` after existing `ClaimInjectionFilter`:

```
[existing] ClaimInjectionFilter
[1A-1]     CanonicalizeToolsFilter          -> Phase 0F
[1A-2]     DynamicContentFilter             -> Phase 0F
[1A-3]     RetrieveToolInjectionFilter      -> rigor_retrieve tool
[1A-4]     ReadOutlineFilter                -> ast-grep outlines (tree-sitter, hot path)
[1A-5]     ContentRouterFilter -> dispatch:
              |-- SmartCrusherFilter        (JSON tool-results only)
              |-- CodeCompressorFilter      (code blocks, tree-sitter)
              +-- KompressFilter            (text, ONNX, feature-gated)
[1A-6]     RollingWindowFilter              -> final token-budget enforcement
[1A-7]     AuditFilter                      -> writes request hash+bytes to content_store, category=audit
```

**Filter specifications:**

**RetrieveToolInjectionFilter** — new file mirroring `claim_injection.rs:15-62`. Inserts `rigor_retrieve(hash, query?)` tool into `body["tools"]`. Anthropic + OpenAI shapes. Namespaced `rigor_` prefix so no collision with agent's own tools. Skipped when rigor's own MCP server is configured (mirrors `headroom/ccr/__init__.py:15-17`).

**ReadOutlineFilter** — runs before ContentRouter. Detects `Read` tool-results across Claude Code / OpenCode / Codex. Uses tree-sitter (not LSP — speed-sensitive hot path) to emit file outlines (signatures + top-level symbols) instead of full bodies. Stores original in content_store. Gated by env `RIGOR_INTERCEPT_READ`.

**ContentRouterFilter** — walks `messages[]`, operates only on `role=="tool"` or `content[*].type=="tool_result"`. Classifies via `magika-rs` + regex fallback. Splits mixed content, routes each part, reassembles. Skips messages at index `< frozen_message_count` (Phase 0F guard).

**SmartCrusherFilter** — JSON-array trimming: first-k / last-k / outliers / items matching last-user-message relevance via existing `evaluator/relevance.rs`. Adaptive-k via Kneedle. Inline marker `[N items compressed to M. hash=<hex>]`.

**CodeCompressorFilter** — tree-sitter per-language (Python / JS / TS / Go / Rust / Java / C / C++) behind `compression-code` feature. Data-driven `LangConfig` (pattern from `headroom/transforms/code_compressor.py:206-297`): keep imports, type defs, decorators, signatures, first-line docstring; drop function bodies beyond `max_body_lines`. Auto-detect language by parse-error minimum.

**KompressFilter** — behind `compression-ml` feature. Loads `chopratejas/kompress-base` INT8 ONNX via Phase 0H host. Per-token keep/discard via argmax + span-head boost. Local inference only.

**RollingWindowFilter** — runs last. If estimated input tokens > model context budget, drops oldest non-frozen non-user non-system messages. Emits `CompressionOverflow` WS event.

**AuditFilter** — innermost; writes the final upstream-bound request bytes to content_store under `category=audit` (no TTL). Hash lives in `ctx.scratch` for the response handler to reference.

### 6.2 [1B] Response-side — CCR loop and audit

Enabled by Phase 0G wiring the FilterChain into the response path.

**CcrRetrievalFilter** — `apply_response_chunk` detects `tool_use` blocks where `name == "rigor_retrieve"`, pulls bytes from content_store, synthesizes a `tool_result` message, re-issues upstream. Loop cap 3, tracked in `ctx.scratch`.

**ResponseAuditFilter** — `finalize_response` writes full response body + extracted claims to content_store under `category=audit`, linked to request hash from 1A-7.

### 6.3 [1C] ContextTracker — proactive retrieval

New `crates/rigor/src/memory/context_tracker.rs`. Per-conversation state `{ hash -> (compressed_at, retrieval_count, tokens_saved) }`.

On turn N+1, score relevance of the new user message against every open hash; if score > threshold, pre-inline original before the model needs to call `rigor_retrieve`. Reuses existing `RelevanceLookup` trait — same mechanism that scores claims against constraints scores queries against compressed hashes.

Mirrors `headroom/ccr/context_tracker.py:35`.

### 6.4 [1D] TOIN — hashed-field preserve-list learning

New `crates/rigor/src/memory/toin.rs`.

Atomically-written JSON at `~/.rigor/toin.json`. Schema:

```
tool_signature_hash -> {
    field_hashes,
    retrieval_counts,
    preserve_hints,
}
```

SmartCrusherFilter + ReadOutlineFilter write signatures. ContextTracker writes back retrieval feedback.

**Rigor-specific extension:** preserve_hints is fed by constraint graph anchors. Any field name referenced in an active `Constraint.source[].anchor` gets unconditionally preserved. Compression decisions become constraint-aware, closing one of the future-epistemology gaps.

### 6.5 [1E] Admin endpoints

- `POST /v1/retrieve { hash, query? }`
- `GET /v1/compression/stats` — hit rates, tokens saved, per-filter metrics (wires into existing observability tab)

### 6.6 Phase 1 out of scope

- `headroom learn` equivalent is Phase 1.5 (separate phase, not request/response-path).
- Multi-provider parity (Gemini batches, WebSocket Codex) — separate observability ticket.

---

## 7. Phase 1.5 — Rigor Learn

Offline analyzer patterned after `headroom learn` but specialized for rigor's data. Not on the request/response path.

### 7.1 [1.5A] Multi-source scanner

Unlike headroom (single agent-log source), rigor has multiple substrates:

- `~/.rigor/violations.jsonl` — primary signal
- `~/.rigor/sessions/<id>/rigor.log` — per-session trace logs
- `~/.rigor/memory.json` — `MemoryStore` episodes and semantic memory
- Agent conversation logs via a `SessionScanner` trait (pattern from `headroom/learn/base.py:15-30`). Built-in plugins for Claude Code JSONL, Codex transcripts, Gemini logs.
- `rigor.yaml` — current constraint set (for diff-able output)
- `CLAUDE.md` / `AGENTS.md` / `MEMORY.md` in project root (avoid duplicate recommendations)

### 7.2 [1.5B] Hybrid analyzer — rule-based + LLM

Where headroom chose pure-LLM, rigor has structured violation data. Hybrid approach.

**Rule-based pass** (`crates/rigor/src/learn/rules.rs`):
- Constraints with false_positive rate > 30% → candidate "tune or retire"
- Constraints never fired in N sessions → candidate "remove or re-anchor"
- Recurring claim patterns that fire no constraint → candidate "add new constraint" (the Friday-garbage-collection pattern from `project_refine_v2.md`)
- Anchors failing LSP verification repeatedly → candidate "re-anchor or remove"
- High-strength constraints with low verification_count → candidate "verify or downgrade"

**LLM pass** (`crates/rigor/src/learn/analyzer.rs`):
- Takes rule-based findings + session digest
- LLM judges each candidate, writes human-readable recommendation
- Same model-selection heuristic as headroom: env key priority → CLI fallback
- Uses the live rigor proxy so the call itself is audited

### 7.3 [1.5C] Writer — two targets

**Markdown target** — marker-delimited blocks `<!-- rigor:learn:start --> ... <!-- rigor:learn:end -->` into CLAUDE.md / AGENTS.md / MEMORY.md. Pattern ported from `headroom/learn/writer.py:21-23`.

**rigor.yaml target** — for candidate new constraints / tuning recommendations, emit `rigor.learn.yaml` for user review. **Never auto-merge into `rigor.yaml`**; user runs `rigor refine apply <file>`. Inverse of headroom's dry-run default — rigor never edits the source of truth silently.

### 7.4 [1.5D] Integration with existing modules

- Reuses `evaluator::relevance::HttpLookup` for querying live daemon during analysis
- Reuses Phase 0J corpus exporter for training-data-oriented recommendations
- Writes `LearnRunSummary` entry back into `ViolationLogger` tying recommendations to the violations they derived from

### 7.5 [1.5E] CLI

```
rigor learn [--project <path>] [--all] [--apply] [--model <name>] [--since <date>]
```

Dry-run default. `--apply` writes markdown targets. rigor.yaml changes always go to `rigor.learn.yaml` regardless of `--apply`.

---

## 8. Phase 2 — Graphify / Knowledge Types / Gettier Guards

**Reframing:** this phase implements most of `project_epistemology_expansion.md`.

### 8.1 [2A] Knowledge-type classifier

Extend `crates/rigor/src/claim/extractor.rs`. Rule-based classifier that derives `KnowledgeType` for each extracted claim:

- Code-anchored `SourceAnchor`? → `Empirical`
- Derived from other claims via DF-QuAD propagation? → `Rational` (set by evaluator, not extractor)
- Doc-anchored or LLM-generated? → `Testimonial`
- Reused from prior `rigor map` run? → `Memory`

Testimonial claims get `credibility_weight` based on source model: Opus=1.0, Sonnet=0.85, Haiku=0.6, human=1.0.

### 8.2 [2B] Cluster-aware context injection

New `ClusterSelectorFilter` runs before `ClaimInjectionFilter` in the request chain.

- Embeds last user message + recent tool calls
- Picks top-N clusters by cosine similarity against cluster centroids stored in `RigorConfig.clusters[]`
- Narrows context to those clusters only — writes narrowed list into `ctx.scratch`
- `ClaimInjectionFilter` + `build_epistemic_context` (`daemon/context.rs:10-124`) read the scratch list and inject only cluster-relevant constraints

Cluster-selected context goes into the `[Dynamic Context]` tail from Phase 0F so the stable prefix still caches. Expected ~10× shorter injected context.

### 8.3 [2C] Leiden clustering in `rigor map`

Offline, blocks 2B.

New pipeline stage in `rigor map`:
- Build claim-cooccurrence graph (constraints as nodes, co-occurrence in files/claims as edges)
- Run Leiden via `petgraph` + Leiden crate (`leiden-rs` if available, hand-roll from paper otherwise)
- Write `clusters[]` back to rigor.yaml
- Cluster centroid = mean of embeddings of constituent constraint `description` fields (reuse whatever embedding model `rigor map` already uses)

### 8.4 [2D] Knowledge-type-routed cached-verdict evaluator

New `CachedSemanticEvaluator` registered before `SemanticEvaluator` in `EvaluatorPipeline`:

| Knowledge type | Strategy |
|---|---|
| Empirical | grep/LSP verify anchor (via existing `lsp/` scaffolding + new `verify_anchor` method). **Never call LLM.** Binary hit/miss. |
| Testimonial | Cache key `(claim_hash, source_sha256, constraint_id)`. Invalidate on source_sha256 change. Fall through to `SemanticEvaluator` on miss. |
| Rational | Deterministic DF-QuAD, computed once per constraint graph build. No per-claim cache. |
| Memory | Time-decay: stored verdict with `verified_at` timestamp; re-verify if `age > decay_window`. |

**Gettier guard** — before returning a cache hit: re-verify `source_anchor` still exists (LSP `textDocument/references`), and its `anchor_sha256` still matches. Stale anchor → invalidate cache entry, fall through to evaluator, emit `DaemonEvent::GettierGuardFired`.

### 8.5 [2E] Persistent verdict cache

Replace `RELEVANCE_CACHE` at `proxy.rs:2850-2851` with content_store-backed storage (category `verdict`).

- Survives daemon restarts — previous judge verdicts don't cost re-query
- `HttpLookup` (`evaluator/relevance.rs:92-203`) points at the same backing store via admin endpoint

### 8.6 [2F] Cached retry-verifier

Extension of 2D. Key schema extended: `(claim_hash, constraint_id, retry_context_hash) -> verdict`.

When auto-retry at `proxy.rs:1665-1699` produces corrected text and the retry verifier scores it clean, cache the verdict. Next occurrence of the same pattern skips re-verification.

### 8.7 [2G] DF-QuAD edge weights wired

Modify `constraint/graph.rs:169-187` to read `Relation.confidence` (Phase 0C) when building attacker/supporter products. Default=1.0 preserves existing tests.

---

## 9. Phase 3 — GEPA Rust-Native Optimizer

GEPA ported natively to Rust — no Python subprocess. Rigor stays single-binary.

### 9.1 [3A] Annotation emission

**Do not create parallel annotation storage.** `ViolationLogEntry` at `logging/types.rs:30-78` is already the substrate:

- `claim_ids` + `claim_text` — inputs
- `message` — reasoning (auto-retry already uses this field)
- `false_positive` — human-corrected label
- `annotation_note` — human reasoning
- `model` — which LLM produced the verdict, feeds `credibility_weight`

**Additions to the schema:**

```rust
request_hash: Option<String>,                       // → content_store audit entry
evaluator_version: Option<String>,                  // Which prompt version produced this
judge_verdict_pre_calibration: Option<String>,      // Raw judge output, for baseline
```

New `AnnotationFilter` in egress chain writes these fields during `finalize_response` (Phase 0G makes this invokable).

### 9.2 [3B] Versioned prompt registry

New `crates/rigor/src/evaluator/prompt_registry.rs`:

```rust
pub struct PromptRegistry {
    active: ArcSwap<Prompt>,                            // Lock-free reads
    candidates: ArcSwap<HashMap<VersionId, Prompt>>,
}
```

Per-knowledge-type prompt families — distinct seeds for Empirical (grep-match wrapper), Testimonial (policy-adherence style), Rational (DF-QuAD computation).

`SemanticEvaluator` reads the active prompt for the constraint's `knowledge_type`.

**Admin endpoints:**
- `POST /v1/evaluator/candidate` — register new prompt from GEPA output
- `POST /v1/evaluator/promote` — swap candidate → active atomically
- `POST /v1/evaluator/shadow { version_id }` — run candidate alongside active, log disagreements, verdict unaffected
- `GET /v1/evaluator/versions` — list + diff
- `POST /v1/evaluator/grade-single` — single-claim grading, used by Phase 3E's batch evaluator

Promoted version updates `Constraint.credibility_weight` based on cross-validation performance, closing the loop on dynamic base strength.

### 9.3 [3C] Annotation review UI + CLI

Per the GEPA transcript: data quality matters more than algorithm.

- Dashboard tab **ANNOTATIONS** alongside LIVE / SEARCH / EVAL / OBSERVABILITY
- Filter by constraint_id, verdict, date; edit `false_positive` and `annotation_note` inline (uses existing `logging/annotate.rs:10-66`)
- CLI `rigor annotations list | show | fix`
- Pre-GEPA check: block optimization if <X% of entries have `annotation_note` populated

### 9.4 [3D] Baseline measurement

```
rigor refine baseline --constraint <constraint_id>
```

- Loads entries from violations.jsonl for the constraint
- Runs current active prompt via `/v1/evaluator/grade-single`
- Reports accuracy / precision / recall / confusion matrix
- Writes `~/.rigor/baselines/<constraint_id>-<date>.json`

No optimization without knowing starting point. Also guards against regression.

### 9.5 [3E] GEPA algorithm — native Rust

New crate subtree `crates/rigor/src/refine/`:

```
crates/rigor/src/refine/
|-- mod.rs
|-- corpus.rs           # Phase 0J
|-- candidate.rs        # Prompt candidate + per-task scores
|-- pareto.rs           # Pareto-frontier selection
|-- mutator.rs          # Prompt mutation via reflection LLM
|-- merger.rs           # Prompt merge via LLM
|-- evaluator.rs        # Batch evaluation of a candidate across corpus
|-- budget.rs           # Cost + iteration tracking
|-- reflection.rs       # Opinionated reflection templates, shipped as include_str!
|-- optimizer.rs        # Main GEPA loop
+-- cli.rs              # rigor refine optimize subcommand
```

**Core types:**

```rust
pub struct Candidate {
    pub id: Uuid,
    pub prompt: String,
    pub parent_ids: Vec<Uuid>,
    pub origin: CandidateOrigin,          // Seed | Mutation | Merge
    pub scores_per_task: BTreeMap<TaskId, EvalOutcome>,
    pub mean_score: f64,
    pub total_cost_usd: f64,
    pub created_at: DateTime<Utc>,
}

pub struct ParetoFrontier {
    per_task_best: BTreeMap<TaskId, Uuid>,
    frontier: HashSet<Uuid>,
}

pub trait PromptMutator: Send + Sync {
    async fn mutate(
        &self,
        candidate: &Candidate,
        reflection_context: &ReflectionContext,
    ) -> Result<Candidate>;
}

pub trait PromptMerger: Send + Sync {
    async fn merge(
        &self,
        a: &Candidate,
        b: &Candidate,
        reflection_context: &ReflectionContext,
    ) -> Result<Candidate>;
}
```

**Main loop (`optimizer.rs`):**

```rust
pub async fn optimize(cfg: OptimizeConfig) -> Result<OptimizeOutcome> {
    let mut pool = vec![seed_candidate(&cfg)];
    let mut budget = Budget::new(cfg.max_cost_usd, cfg.iteration_budget);
    let corpus = load_corpus(&cfg)?;
    let (train, val) = split(&corpus, cfg.split_strategy);

    // Seed evaluation
    pool[0] = evaluate_batch(&pool[0], &train).await?;
    let mut frontier = ParetoFrontier::from(&pool);
    let mut stagnation = 0;

    while !budget.exhausted() && stagnation < cfg.early_stop_plateau {
        budget.tick_iteration();

        let new_candidates = sample_candidates(
            &pool, &frontier, cfg.candidates_per_iteration,
            &cfg.mutator, &cfg.merger,
        ).await?;

        let evaluated = evaluate_candidates(
            new_candidates, &train, cfg.batch_size, &mut budget,
        ).await?;

        let prev_size = frontier.frontier.len();
        pool.extend(evaluated);
        frontier = ParetoFrontier::from(&pool);

        if frontier.frontier.len() == prev_size { stagnation += 1; }
        else { stagnation = 0; }

        emit_ws_event(OptimizeProgress { /* ... */ });
    }

    let best = select_final_candidate(&pool, &val).await?;
    Ok(OptimizeOutcome { best, pool, frontier, budget })
}
```

**Sampling strategy:** 60% mutation, 40% merge (when ≥2 frontier members exist). Mutation selects parents weighted by recent score improvement.

**Pareto frontier:** for each task, track the candidate with highest score. Frontier = union of per-task bests. Straightforward O(n·tasks) rebuild per iteration.

**Mutator:** calls refiner LLM (default Opus) with reflection template + current prompt + recent eval outcomes as few-shot. Parses new prompt from delimited response block.

**Merger:** two inputs + merge-oriented reflection template.

**Batch evaluator:** hits daemon's `POST /v1/evaluator/grade-single`, parallelized via `futures::stream::buffer_unordered(cfg.concurrency)`.

**Budget:** tracks cost via response headers (`anthropic-ratelimit-*`) and iteration count. Early-stops on exhaustion or Pareto plateau.

**Reflection templates:** shipped as `include_str!` constants in the binary. Per-knowledge-type, aware of DF-QuAD and rigor schema. Override via `~/.rigor/reflection/<knowledge_type>.md` if present.

**CLI:**

```
rigor refine optimize --constraint <id>
    [--split task|random|temporal]
    [--iteration-budget N]
    [--max-cost-usd X]
    [--candidates-per-iter N]
    [--batch-size N]
    [--concurrency N]
    [--refiner-model <name>]
    [--judge-model <name>]
    [--early-stop-plateau N]
    [--dry-run]
    [--resume <run-id>]
```

**Dashboard integration:**
- New `RefineRun` event stream
- `/v1/refine/runs` admin endpoint for listing + cancellation
- Per-run Pareto plot in **REFINE** dashboard tab

### 9.6 [3F] Promotion and induction tracking

User reviews candidate via dashboard diff, optionally runs shadow mode, then `/promote`.

**Promotion writes back to the constraint itself:**

- `Constraint.verification_count` += 1 (counter of consistent judge outputs)
- `Constraint.last_verified` = now
- `Constraint.credibility_weight` = f(candidate vs baseline delta)
- If delta is strong enough, DF-QuAD recomputes with new base strengths

Promoted version recorded in violations.jsonl with `evaluator_version`, GEPA config, delta — full audit trail.

---

## 10. Phase 4 — Forward Epistemology Integration

All items previously marked "out of scope" in v3 are now in scope.

### 10.1 [4A] LSP verification fully wired

Extends `crates/rigor/src/lsp/` scaffolding.

**LSP client manager** — one long-lived subprocess per language server (rust-analyzer, pyright, gopls, typescript-language-server). Lazy-start on first verification request, idle shutdown after 5 min. Pool cached in `DaemonState`.

**Verification ops:**
- `verify_anchor_exists(source_anchor) -> bool` — uses `textDocument/documentSymbol` + string match on `anchor`
- `resolve_references(path, line, col) -> Vec<Location>` — `textDocument/references`
- `get_definition(path, line, col) -> Option<Location>` — `textDocument/definition`
- `get_hover(path, line, col) -> Option<String>` — `textDocument/hover`

**Integration points:**
- Phase 2D Gettier guard — pre-serve cache validation
- `rigor map --verify` — batch verification for all constraints; writes back `last_verified` / `verification_count` / `verified_at_commit`
- New WS event `DaemonEvent::AnchorVerified { constraint_id, ok, method }`

**Fallback:** if LSP unavailable (language server not installed), fall back to grep-based match with a warning in observability.

### 10.2 [4B] Dynamic DF-QuAD base strength

Modifies `constraint/graph.rs:50-54`:

```rust
fn compute_base_strength(c: &Constraint, cfg: &StrengthConfig) -> f64 {
    // 1. Start from override or type default
    let base = c.base_strength_override.unwrap_or_else(|| match c.epistemic_type {
        EpistemicType::Belief => 0.8,
        EpistemicType::Justification => 0.9,
        EpistemicType::Defeater => 0.7,
    });

    // 2. Apply credibility weight
    let credibility = c.credibility_weight.unwrap_or(1.0);

    // 3. Induction bonus: log-scale on verification_count
    let induction_bonus = match c.verification_count {
        0 => 0.0,
        n => (n as f64).ln() / cfg.induction_denominator,
    };

    // 4. Decay for stale verifications (Memory type mainly)
    let decay = c.last_verified
        .map(|t| {
            let age_days = (Utc::now() - t).num_days() as f64;
            (-age_days / cfg.decay_half_life_days).exp()
        })
        .unwrap_or(1.0);

    (base * credibility * decay + induction_bonus).clamp(0.0, 1.0)
}
```

Regression test: reproduces hardcoded defaults when all optional fields are None. Keeps `graph.rs:447` green.

### 10.3 [4C] Action-gate ↔ constraint-evaluation integration

Today action gates (`daemon/gate.rs`) fire on `ClaimType::ActionIntent` and are orthogonal to Rego / DF-QuAD. Closing the loop.

**Gate outcomes become epistemic events:**

- **Approved gate** → emit `Claim { text: "action <X> was approved on <date>", knowledge_type: Memory, confidence: 1.0, source: SessionSource }`. Stored in `MemoryStore` so next session's `build_epistemic_context` surfaces it as prior evidence.
- **Rejected gate** → auto-synthesize `Constraint { epistemic_type: Defeater, knowledge_type: Memory, message: "Previously-rejected action: <X>" }`. Persists to `~/.rigor/learned_defeaters.yaml` (distinct from user-authored rigor.yaml).
- **Gate decision in violation log** — add `gate_decision: Option<GateDecision>` to `ViolationLogEntry`, populate when an action was gated.

**Plumbing:**

New `crates/rigor/src/gate/epistemic.rs`:
- `fn emit_approval_memory(approval: &GateApproval) -> Claim`
- `fn synthesize_rejection_defeater(rejection: &GateRejection) -> Constraint`
- `fn persist_learned_defeater(def: &Constraint)` — merges into `learned_defeaters.yaml` with dedup on action-text hash

Hook in `daemon/gate.rs:99-120` (`apply_decision`): after existing persistence, invoke `epistemic::record_gate_outcome(decision, ctx)`.

Modification to `build_epistemic_context` (`daemon/context.rs:10-124`): append new section `PREVIOUSLY-REJECTED ACTIONS` from `learned_defeaters.yaml`, same shape as existing `KNOWN CONTRADICTIONS`.

DF-QuAD incorporates the learned defeaters automatically (they're Constraints with `epistemic_type=Defeater`).

**Safety:** user can `rigor constraint disable <id>` on a learned defeater same as any other. Rejections don't silently compound.

**Dashboard:** GATES tab gets "Learned from gates" subpanel listing synthesized defeaters and approved-action memories.

### 10.4 [4D] Postgres audit backend

**Why now:** Rigor Cloud (`project_rigor_cloud.md`) requires it, and Phase 0I trait boundary isolated the change.

**Dependencies:** `sqlx` with features `["postgres", "runtime-tokio-rustls", "uuid", "chrono", "json"]`.

**Migrations:** `crates/rigor/migrations/`:
```
20260422_01_content_store.sql
20260422_02_violation_log.sql
20260422_03_sessions.sql
20260422_04_annotations.sql
20260422_05_learn_runs.sql
20260422_06_refine_runs.sql
20260422_07_learned_defeaters.sql
```

**Schema highlights:**

```sql
CREATE TABLE content_store (
    hash            BYTEA PRIMARY KEY,
    category        TEXT NOT NULL,
    payload         BYTEA NOT NULL,
    size_bytes      INTEGER NOT NULL,
    stored_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ttl_expires_at  TIMESTAMPTZ,
    access_count    INTEGER NOT NULL DEFAULT 0,
    tool_signature_hash TEXT
);
CREATE INDEX content_store_category_ttl ON content_store (category, ttl_expires_at);

CREATE TABLE violation_log (
    id              BIGSERIAL PRIMARY KEY,
    session_id      UUID NOT NULL,
    timestamp       TIMESTAMPTZ NOT NULL,
    git_commit      TEXT,
    git_dirty       BOOLEAN,
    constraint_id   TEXT NOT NULL,
    constraint_name TEXT NOT NULL,
    claim_ids       TEXT[] NOT NULL,
    claim_text      TEXT[] NOT NULL,
    base_strength   REAL NOT NULL,
    computed_strength REAL NOT NULL,
    severity        TEXT NOT NULL,
    decision        TEXT NOT NULL,
    message         TEXT NOT NULL,
    supporters      TEXT[] NOT NULL DEFAULT '{}',
    attackers       TEXT[] NOT NULL DEFAULT '{}',
    model           TEXT,
    transcript_path TEXT,
    claim_confidence REAL,
    claim_type      TEXT,
    false_positive  BOOLEAN,
    annotation_note TEXT,
    request_hash    BYTEA REFERENCES content_store(hash),
    evaluator_version TEXT,
    gate_decision   JSONB
);
CREATE INDEX violation_log_session ON violation_log (session_id);
CREATE INDEX violation_log_constraint_time ON violation_log (constraint_id, timestamp DESC);
CREATE INDEX violation_log_annotations ON violation_log (false_positive) WHERE false_positive IS NOT NULL;
```

**Backend impls:**

- `PostgresContentBackend: ContentStoreBackend` — `INSERT ... ON CONFLICT DO NOTHING`, prepared statement caching, TTL cleanup via background task (delete WHERE `ttl_expires_at < NOW()` every 5 min)
- `PostgresLogBackend: ViolationLogBackend` — append via `INSERT`, indexed scans for queries, `UPDATE` for annotate

**Deployment:**

- Single-binary still works — Postgres is feature-gated `["postgres-backend"]`; without feature, `PostgresBackend` is unavailable and only JSONL ships
- Configuration: `RIGOR_POSTGRES_URL=postgres://...` env var on startup
- Migration: `rigor migrate` subcommand runs sqlx migrations

### 10.5 [4E] Modal judge training

**Architecture split — two repos:**

```
rigor/                               # Rust (this repo)
+-- crates/rigor/src/
    |-- refine/corpus.rs             # Phase 0J: export to JSONL
    +-- evaluator/modal_judge.rs     # ClaimEvaluator impl calling Modal endpoint

rigor-modal/                         # NEW sibling repo, Python
|-- pyproject.toml                   # modal, transformers, datasets
|-- modal_app.py                     # Modal app definition
|-- train.py                         # Fine-tuning script
|-- serve.py                         # Inference endpoint
+-- eval.py                          # Offline eval on held-out set
```

**Training pipeline:**

1. Rust-side: `rigor refine export --constraint X --format jsonl > corpus.jsonl`
2. Upload: `modal volume put rigor-corpus corpus.jsonl`
3. Train: `modal run rigor-modal::train --base-model answerdotai/ModernBERT-base --corpus corpus.jsonl`
4. Modal trains on A100, serializes to `modal_volume:/models/<run_id>/`
5. Deploy: `modal deploy rigor-modal::serve --model-run-id <run_id>`
6. Register: `rigor evaluator register modal --url <modal-url> --model-name <run_id>`

**Inference endpoint:**

- Modal-hosted HTTPS `POST /predict`
- Body: `{ claim_text, constraint_id, constraint_description, knowledge_type }`
- Response: `{ verdict: Allow | Warn | Block, confidence: f64, reasoning: String }`
- Autoscaling via Modal; cold-start mitigated by `min_containers=1`

**Rust integration:**

New `ModalJudgeEvaluator: ClaimEvaluator` in `crates/rigor/src/evaluator/modal_judge.rs`:

- `can_evaluate` — true when constraint has tag `modal` or ModalJudge is default for the constraint's `KnowledgeType`
- `evaluate` — async HTTP call with 2s timeout, caches in Phase 2E's persistent verdict cache
- Fails through to `SemanticEvaluator` on Modal unavailability (fail-open)
- Registered between `CachedSemanticEvaluator` (2D) and `SemanticEvaluator` in pipeline order

**Cost estimates:**

- Training: ~$5-50 per constraint depending on corpus size and base model
- Inference: ~$0.001 per call on ModernBERT-base (vs ~$0.01 for Sonnet)
- Modal billed per container-second; idle minimal with autoscale

**Feature flag:** `modal-judge`. Off by default. When off, `modal_judge.rs` compiles to a stub never registered.

### 10.6 [4F] Safety discriminator

Per `project_safety_discriminator.md`. Same infra path as 4E (ONNX via Phase 0H, or Modal-hosted via 4E).

- Detects prompt injection / GCG / MCP asymmetry / RAG poisoning — categories where LLM-as-judge is too slow
- Plugs in as `ClaimEvaluator` earlier in pipeline (before expensive evaluators) with ~35ms latency

### 10.7 [4G] StrengthConfig calibration

Calibrate 4B's knobs against the annotation corpus. Not just config — full calibration pipeline with safety guardrails.

**Calibration target:**

```rust
pub struct StrengthConfig {
    pub decay_half_life_days: f64,
    pub induction_denominator: f64,
    pub base_strength_overrides: BTreeMap<EpistemicType, f64>,
    pub calibrated_at: DateTime<Utc>,
    pub calibration_run_id: Option<Uuid>,
    pub corpus_size: usize,
}
```

Loaded at daemon start from `~/.rigor/strength_config.yaml`. Absent → hardcoded defaults. Hot-reloadable via admin endpoint.

**Algorithm — grid search then gradient:**

New `crates/rigor/src/refine/calibrate.rs`:

```rust
pub async fn calibrate(cfg: CalibrateConfig) -> Result<CalibrationOutcome> {
    let rows = load_labelled_corpus(&cfg)?;
    if rows.len() < cfg.min_corpus_size {
        return Err(CalibrationError::InsufficientData { /* ... */ });
    }
    let (train, val) = split_temporal(&rows, cfg.val_fraction);

    let grid = StrengthConfigGrid {
        decay_half_life_days: vec![7.0, 14.0, 30.0, 60.0, 90.0, 180.0, 365.0],
        induction_denominator: vec![5.0, 10.0, 20.0, 50.0, 100.0],
    };

    let mut best = CalibrationResult::default();
    for candidate in grid.iter() {
        let metrics = replay_and_score(&candidate, &train);
        if metrics.f1 > best.metrics.f1 {
            best = CalibrationResult { config: candidate, metrics };
        }
    }

    if cfg.refine_after_grid {
        best = refine_nelder_mead(best, &train);
    }

    let val_metrics = replay_and_score(&best.config, &val);
    Ok(CalibrationOutcome {
        candidate: best.config,
        train_metrics: best.metrics,
        val_metrics,
        corpus_size: rows.len(),
        run_id: Uuid::new_v4(),
    })
}
```

`replay_and_score` reconstructs what strength 4B would have produced at each historical row's timestamp, given the candidate's knobs. Compares predicted verdict (strength ≥ 0.7 → Block) against `human_corrected_label`. F1 on "should block" class.

Uses the row's timestamp as "now" and reconstructs `verification_count` / `last_verified` from the violation log up to that point — prevents lookahead bias.

**Safety guardrails:**

- **Minimum corpus threshold:** reject if `labelled_rows < 200` (configurable)
- **Never overwrite** `~/.rigor/strength_config.yaml` in one step. Calibration writes to `~/.rigor/strength_config.candidate.yaml`. User promotes via `rigor refine apply-calibration <run_id>`.
- **Shadow-mode required before promotion.** `POST /v1/strength/shadow { run_id }` runs DF-QuAD with both active and candidate configs for default 24h, logs disagreement to annotation log under `strength_shadow` category.
- **Regression guard:** promotion rejected if `candidate.val_metrics.f1 < active.val_metrics.f1 - 0.02`. Forces meaningful improvement, not noise.
- **Rollback:** promotion stores previous config at `~/.rigor/strength_config.previous.yaml`. `rigor refine rollback-calibration` restores.

**CLI + admin:**

```
rigor refine calibrate
    [--min-corpus-size N]        # default 200
    [--val-fraction F]           # default 0.2
    [--include-base-strength]
    [--refine-after-grid]
    [--since <date>]
    [--dry-run]

rigor refine apply-calibration <run_id>
    [--skip-shadow]              # dangerous
    [--shadow-duration-hours H]  # default 24

rigor refine rollback-calibration
rigor refine show-calibration
```

Admin endpoints: `POST /v1/strength/calibrate`, `POST /v1/strength/shadow`, `POST /v1/strength/promote`, `GET /v1/strength/config`.

**Dashboard:** new subpanel in REFINE tab:
- Current `StrengthConfig` + calibrated-at
- Last run: corpus size, train/val F1, per-parameter grid heatmap
- Shadow-mode disagreement rate
- "Calibration stale" warning when corpus grew >50% since last calibration (hook for Phase 1.5 Rigor Learn)

**Integration:**

- Phase 4B `compute_base_strength` reads from `StrengthConfig` (loaded from file, hardcoded defaults if absent)
- Phase 3A annotations provide training labels
- Phase 1.5 Rigor Learn surfaces "calibration N months old, corpus grew X%" as a recommendation
- Phase 4D Postgres — calibration queries go through `ViolationLogBackend::query_labelled`, works identically on JSONL or Postgres

---

## 11. Dependency Graph

```
Phase 0 (schema + infra + abstractions)
|-- 0A  KnowledgeType enum                  -> blocks 2A, 3B
|-- 0B  Dynamic strength fields             -> blocks 2D, 3F, 4B, 4G
|-- 0C  Confidence on Relation              -> blocks 2G
|-- 0D  SourceAnchor fingerprinting         -> blocks 2D Gettier
|-- 0E  Content store (categorized)         -> blocks 1A-7, 1B, 1C, 2E, 3A, 4D
|-- 0F  Frozen prefix + canonicalizer       -> blocks 1A
|-- 0G  Wire FilterChain into response      -> blocks 1B, 3A finalize path
|-- 0H  ONNX host                           -> blocks 1D Kompress, 4F
|-- 0I  Backend abstraction traits          -> blocks 4D Postgres
+-- 0J  Corpus exporter                     -> blocks 3E, 4E

Phase 1 (Headroom = audit + compression)
|-- 1A  Request chain (uses 0A-0H)
|-- 1B  Response CCR loop (uses 0G)
|-- 1C  ContextTracker (uses 1A+1B+RelevanceLookup)
|-- 1D  TOIN (uses 0E, reads constraint graph)
+-- 1E  Admin endpoints

Phase 1.5 (Rigor Learn)
|-- 1.5A  Multi-source scanner
|-- 1.5B  Hybrid analyzer (uses 0J corpus exporter)
|-- 1.5C  Dual-target writer
|-- 1.5D  Integration with existing modules
+-- 1.5E  CLI

Phase 2 (Graphify = knowledge types + Gettier)
|-- 2A  Knowledge-type classifier (uses 0A)
|-- 2B  Cluster-aware context (needs 2C)
|-- 2C  Leiden in rigor map (offline)
|-- 2D  Cached-verdict evaluator (uses 0B, 0D, partial LSP -> opens 4A)
|-- 2E  Persistent verdict cache (replaces RELEVANCE_CACHE, uses 0E)
|-- 2F  Cached retry-verifier (extends 2D)
+-- 2G  DF-QuAD edge weights (uses 0C)

Phase 3 (GEPA = calibration + induction)
|-- 3A  Annotation emission (extends ViolationLogEntry, uses 0G)
|-- 3B  Prompt registry per knowledge type (uses 0A)
|-- 3C  Annotation review UI
|-- 3D  Baseline measurement
|-- 3E  Native Rust GEPA (uses 0J, 3B)
+-- 3F  Promotion -> induction tracking (updates 0B -> opens 4B)

Phase 4 (forward integration)
|-- 4A  LSP verification fully wired (extends existing lsp/, seeded by 2D)
|-- 4B  Dynamic DF-QuAD base strength (needs 0B + 4A + 3F)   -> 4G calibrates
|-- 4C  Action-gate <-> constraint integration (needs 0B, extends gate.rs)
|-- 4D  Postgres backend (uses 0I; adds sqlx + migrations)
|-- 4E  Modal judge (needs 0J corpus + rigor-modal Python repo)
|-- 4F  Safety discriminator (uses 0H, optionally 4E Modal)
+-- 4G  StrengthConfig calibration (needs 4B + 3A)
```

**Critical path for three-track delivery:**

```
0A + 0B + 0C + 0D + 0E + 0F + 0G + 0I + 0J
         |
         |-- 1A + 1B (audit + CCR)                    smallest PR with full audit trail
         |-- 2A + 2D + 2E (knowledge types + Gettier + persistent cache)
         +-- 3A + 3B (annotation + registry)
```

Everything else is additive.

---

## 12. Shipping Order

1. **Phase 0A-0J** — schema + infra + abstractions. One or two large PRs, no behavior change. Maximum test coverage possible against existing flows.
2. **Phase 1A+1B+1E** — audit trail + CCR. Every request becomes content-addressable and retrievable. No compression yet (routers in place but pass-through). Proves the audit contract.
3. **Phase 2E** — persistent verdict cache replaces `RELEVANCE_CACHE`. Small change given 0E exists. Recovers restart-lost judge cost immediately.
4. **Phase 3A+3B** — annotation schema extensions + prompt registry. Unlocks GEPA work without requiring optimizer. `credibility_weight` starts populating.
5. **Phase 2A+2D (Empirical + Testimonial only)** — knowledge-type classifier + knowledge-type-routed evaluator. Largest token-economy win per `project_token_economy.md`.
6. **Phase 1.5** — Rigor Learn. Consumes violation log + agent logs; useful independently.
7. **Phase 1A-5 (SmartCrusher + RetrieveTool)** — first real compressor. JSON tool-results are the highest token leak in agentic sessions.
8. **Phase 2G** — DF-QuAD edge weights wired. Small change, opens richer argumentation graphs.
9. **Phase 4A + 4B** — LSP verification + dynamic DF-QuAD. Unlocks Phase 2D Gettier guards in full.
10. **Phase 4C** — action-gate↔constraint integration. Closes the sandbox loop.
11. **Phase 4G** — StrengthConfig calibration. Needs 4B (from step 9) + 3A annotations (from step 4). Meaningfully improves DF-QuAD quality once meaningful corpus exists.
12. **Phase 2C + 2B** — Leiden + cluster-aware injection. Needs embedding infra; after foundations stable.
13. **Phase 3C-F** — full GEPA Rust optimizer + review UI + baseline + promotion.
14. **Phase 4D** — Postgres backend. Largely a backend-swap at this point.
15. **Phase 4E** — Modal judge training. Needs accumulated corpus from steps 8-13.
16. **Phase 1A-4 CodeCompressor, 1C ContextTracker, 1D TOIN, 1A-4 ReadOutline, 1A-6 RollingWindow** — headroom polish. Order by observed pain.
17. **Phase 0H + 1D Kompress + 4F Safety Discriminator** — ONNX-dependent stack ships once.

Phases 14-15 (Postgres, Modal) benefit from significant accumulated data — shipping late is deliberate.

---

## 13. Out of Scope

The following are explicitly deferred out of this plan:

- **Multimodal corpus ingestion** (PDFs, images, videos). Part of full graphify scope, deferred per user direction. Revisit after Phase 4G lands.
- **Multi-provider observability parity** (Gemini batches, WebSocket Codex). Separate observability ticket.
- **Memory-claim decay-window defaults** — 4G calibrates `decay_half_life_days` empirically. Picking the *initial* default before calibration runs uses a reasonable prior (e.g., 90 days); fine-tuning is 4G's job.

---

## 14. Appendix — Key References

### 14.1 Source material

- **Headroom** — `github.com/chopratejas/headroom`. Full architecture map cached at `reference_headroom.md` in project memory. Clone at `/Users/vibhavbobade/go/src/github.com/chopratejas/headroom/` for cross-reference.
- **Graphify** — `github.com/safishamsi/graphify`. Competitor per `project_graphify_integration.md`. Rigor builds equivalents natively, not as a dependency.
- **GEPA paper** — Agarwal et al., *GEPA: Reflective Prompt Evolution Can Outperform Reinforcement Learning* (2024). Python reference impl in `gepa` PyPI package. **Not a dependency** in this plan — Phase 3E ports the algorithm natively to Rust.
- **GEPA transcript** — Mahmoud (Agenta), *Judge the Judge*. Practical application walkthrough cached in conversation history. Key insight: data quality matters more than algorithm choice. Drives Phase 3C annotation review UI priority.
- **DF-QuAD** — Rago et al., *Discontinuity-Free Decision Support with Quantitative Argumentation Debates* (IJCAI 2016). Current implementation at `crates/rigor/src/constraint/graph.rs` per `project_dfquad_formula.md`.

### 14.2 Memory files consumed

All in `/Users/vibhavbobade/.claude/projects/-Users-vibhavbobade-go-src-github-com-rigor-cloud-rigor/memory/`:

- `project_epistemology_expansion.md` — knowledge types, justification, Gettier, induction, credibility, dynamic strength
- `project_epistemic_sandbox.md` — five-component sandbox vision
- `project_rigor_as_platform.md` — three pillars, roadmap
- `project_dfquad_formula.md` — preservation constraints on the DF-QuAD implementation
- `project_token_economy.md` — seven reduction strategies, 60-80% fewer LLM calls target
- `project_graphify_integration.md` — six capabilities to build natively
- `project_refine_v2.md` — violation-log mining, Friday garbage collection pattern
- `project_safety_discriminator.md` — ModernBERT classifier as third evaluator
- `project_rigor_cloud.md` — Postgres migration motivation
- `reference_headroom.md` — architectural summary, five portable ideas
- `project_gepa_evaluators.md` — integration with existing constraint / evaluator model
- `feedback_subagent_model.md` — always use Opus for subagent dispatches (applies to GEPA refiner model choice)
- `feedback_tdd.md` — TDD required for all development in this plan
- `feedback_mirrord_pattern.md` — check mirrord before inventing interception workarounds

### 14.3 Codebase anchors

Key files touched by this plan (with line numbers referenced above):

- `crates/rigor/src/claim/types.rs` — Claim, ClaimType, SourceLocation
- `crates/rigor/src/claim/heuristic.rs:157-185` — extraction pipeline
- `crates/rigor/src/constraint/types.rs` — Constraint, Relation, SourceAnchor, RigorConfig
- `crates/rigor/src/constraint/graph.rs` — DF-QuAD engine (preserve `:447` regression test)
- `crates/rigor/src/evaluator/pipeline.rs` — ClaimEvaluator trait, registry
- `crates/rigor/src/evaluator/relevance.rs` — RelevanceLookup, InProcess + Http impls
- `crates/rigor/src/daemon/egress/chain.rs` — FilterChain, EgressFilter trait
- `crates/rigor/src/daemon/egress/ctx.rs` — ConversationCtx, scratch storage
- `crates/rigor/src/daemon/egress/claim_injection.rs` — existing filter (template for new ones)
- `crates/rigor/src/daemon/proxy.rs:1135` — FilterChain construction point
- `crates/rigor/src/daemon/proxy.rs:1517-1644` — streaming response handler (Phase 0G wires FilterChain here)
- `crates/rigor/src/daemon/proxy.rs:2871-3070` — LLM-as-judge implementation
- `crates/rigor/src/daemon/gate.rs` — action gates (extended in Phase 4C)
- `crates/rigor/src/memory/episodic.rs` — MemoryStore, rebuild-from-log pattern
- `crates/rigor/src/logging/violation_log.rs`, `logging/types.rs:30-78` — audit log substrate
- `crates/rigor/src/logging/annotate.rs` — annotate + rewrite (reused by Phase 3C)
- `crates/rigor/src/lsp/` — scaffolding wired fully in Phase 4A

---

**End of plan. Next step: Phase 0 implementation.**
