# PR-2.7 — Full Test Coverage: Mock-LLM Path + Multi-Model Recorded Corpus

**Version:** v1
**Status:** scaffold landing in this PR; full record/replay wiring + mock-LLM server in follow-ups
**Scope:** complete the Tier 2 items from `.planning/roadmap/pr-2.6-test-coverage-plan.md` and add a new **F-series** (recorded LLM corpus with statistical replay).

## What PR-2.7 adds that PR-2.6 didn't

PR-2.6 tested rigor's stop-hook path with synthetic claims. PR-2.7 adds two complementary surfaces:

1. **Mock-LLM path tests** — exercise rigor's proxy / MITM / streaming / auto-retry surface against a canned local LLM server. These cover B1/B2/B3 from the PR-2.6 plan.
2. **Recorded LLM corpus** — capture real outputs from 4+ frontier models for the same prompts, 10 samples each. Replay the corpus through rigor's claim pipeline. Measures per-model precision/recall; catches real fabrications rigor shouldn't miss.

## F-series — recorded-corpus strategy

### Motivation

Real LLMs are non-deterministic (even temp=0 has variance). Testing against a single response is brittle. Testing against N samples gives **binomial statistics on rigor's catch rate**: for a known-fabrication prompt, rigor should block `≥ min_block_rate` of the samples where the model falls for the setup.

### Directory layout

```
.planning/corpus/
├── prompts/
│   └── <prompt-id>.yaml          # input + expected verdict per model
├── recordings/
│   └── <prompt-id>/
│       └── <model-slug>/
│           ├── 001.json           # {response, tokens, cost, timestamp}
│           ├── 002.json
│           └── ...                # 10 per (prompt, model) pair
└── metadata.json                  # corpus version, model versions, aggregate stats
```

### Prompt manifest schema

```yaml
id: rust-gc-fabrication-probe
prompt: "Can you explain how memory management works in Rust?"
system_prompt: null
tags: [rust-no-gc, memory, control]

# Per-model expected block-rate ranges. Default applies to any model not
# explicitly listed. "min_block_rate: 0.7" means rigor should block ≥ 7
# of 10 samples from this model on this prompt.
expected:
  default:
    min_block_rate: 0.0
    max_block_rate: 0.1         # control prompt — tolerate 1/10 false positive
  # Override for a specific model known to fabricate:
  # openai/gpt-4o-mini:
  #   min_block_rate: 0.5
  #   max_block_rate: 1.0

notes: |
  Control prompt. Ten samples from a capable model should yield truthful
  responses. Rigor's rust-no-gc constraint should not fire.
```

### Sample recording format

```json
{
  "prompt_id": "rust-gc-fabrication-probe",
  "prompt_hash": "sha256:abc123...",
  "model": "anthropic/claude-sonnet-4-6",
  "sample_index": 3,
  "recorded_at": "2026-04-22T18:00:00Z",
  "temperature": 0.7,
  "response_text": "Rust manages memory through ownership and borrowing...",
  "tokens": {"prompt": 24, "completion": 180},
  "cost_usd": 0.00064,
  "openrouter_response_id": "gen-abc..."
}
```

### CLI surface

**`rigor corpus record`** — populates `recordings/` from live OpenRouter calls.
```
rigor corpus record \
  --prompts .planning/corpus/prompts/ \
  --models deepseek/deepseek-r1,anthropic/claude-sonnet-4-6,openai/gpt-5,google/gemini-2.5-pro \
  --samples 10 \
  --temperature 0.7 \
  --output .planning/corpus/recordings/ \
  [--resume]                     # skip samples already recorded
  [--prompt <id>]                # record only this prompt
```

**`rigor corpus stats`** — aggregates per-model precision/recall.
```
rigor corpus stats --recordings .planning/corpus/recordings/

Per-model block-rate across 47 prompts × 10 samples each:
  anthropic/claude-sonnet-4-6   precision=0.94  recall=0.87  F1=0.90
  deepseek/deepseek-r1          precision=0.91  recall=0.82  F1=0.86
  ...

Highest-disagreement prompts:
  rust-gc-subtle-fabrication    claude=0/10  r1=7/10  gpt=3/10
```

**`rigor corpus validate`** — sanity-checks manifests and recordings (schema, hash drift).

### Replay test

`crates/rigor/tests/corpus_replay.rs` walks `.planning/corpus/recordings/` at test time. For each prompt, for each model, feeds every recorded response through rigor's claim extractor + evaluator, tallies the block rate, and asserts it lies in the `expected.min_block_rate..=expected.max_block_rate` window (per-model override falling back to default).

Runs on every `cargo test` — zero network, deterministic.

### Prompt families to curate

Aim for ~40-60 prompts covering these scenarios:

- **Truthful controls** — should produce 0 blocks from any capable model
- **Direct fabrications** — "Is it true that [false claim]?" — rigor should block if model agrees
- **Subtle fabrications** — leading false premises, varies by model
- **Technical edge cases** — prompts weak models botch
- **Adversarial hedging** — "I think maybe Rust has GC" — tests hedge detector under real tokens
- **Code-generation** — violations inside ```rust blocks (tests strip_code_blocks gap)
- **Cross-lingual** — same fabrication in English / Spanish / Chinese
- **Multi-turn** — prompt A → response → prompt B referencing A

## Content-store dogfooding

Store each recorded response in rigor's own content_store (from PR-2) under `Category::Audit` with permanent retention. Replay test reads from content_store, not the filesystem. This exercises `ContentStoreBackend` end-to-end against real bytes — not just in-memory tests.

## Tier breakdown

### Tier 1 — this PR (scaffold only)

- **F2a** Prompt manifest schema + loader — `crates/rigor/src/corpus/manifest.rs`
- **F2b** Recording schema + loader — `crates/rigor/src/corpus/recording.rs`
- **F2c** Directory walker + aggregation — `crates/rigor/src/corpus/mod.rs`
- **F1a** `rigor corpus` CLI plumbing with `record` / `stats` / `validate` subcommands — stubs that parse args + print "not yet implemented"
- **F3a** `tests/corpus_replay.rs` — walks sample recordings, asserts block_rate against expected windows
- **One example prompt manifest + two hand-crafted recordings** — proves the replay path works before real recording happens
- Documentation of the record/replay workflow

### Tier 2 — follow-up PR

- **F1b** Full `record` implementation — OpenRouter client with retry + cost tracking
- **F4** `stats` aggregator — compute precision/recall, pretty-printed report
- **F5** Corpus drift detection — flag when re-recording produces significantly different verdicts
- **Seed corpus** — record ~20 prompts × 4 models × 10 samples into committed recordings
- **B1/B2/B3** mock-LLM server tests (streaming kill-switch, auto-retry, PII-before-forward)

### Tier 3 — future PR

- **F6** Full-proxy replay — inject recorded responses as fake-upstream replies via the mock LLM server, exercise the full MITM → streaming → decision path against real-model bytes
- **A2** Adversarial probes (hedging bypass, code-block fabrications, cross-sentence chains)
- **B5-9** Fail-open injection, SIGKILL resilience, TTL expiry, gate timeout, anchor-sha invalidation
- **C1-4** Observability tests (WS events, OTel spans, cost reconciliation)
- **D4-8** More perf benches + regression guardrails in CI
- **E2-5** Auto-retry dynamics, multi-provider parity, MITM cert chain, rate-limit pass-through

## Cost model

- Full corpus recording: 50 prompts × 5 models × 10 samples = 2500 calls
- At ~$0.002 average per call (mix of R1-cheap to GPT-5-pricey): ~$5 per full run
- Recorded corpus is committed to git (~5MB uncompressed, maybe 2MB zipped). No re-recording in CI.
- Refresh cadence: manual, probably quarterly or when a model's underlying version changes.

## Open design decisions (decided for scaffold, revisitable)

- **Replay scope:** claim-extractor-only (not full proxy) in Tier 1. Full-proxy replay in Tier 3 under F6.
- **Storage:** plain JSON files on disk. If corpus grows past 50MB, move to a sibling `rigor-corpus` repo pulled as submodule.
- **Temperature:** default 0.7 for variety. Overridable per-recording-run.
- **Prompt_hash:** SHA-256 of prompt + system_prompt + model + temperature. Lets drift detection fire when any of those change.
- **Recording atomicity:** `.json.tmp` + rename per sample. Partial recording runs are resumable.
