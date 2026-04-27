# Rigor Dev Harness

Tiered test automation wrapping `cargo` for rigor's local development loop. Designed to give fast feedback on every edit without burning OpenRouter credits on routine work.

## Four tiers

| Tier | Purpose | Trigger | Cost | Target wall time |
|---|---|---|---|---|
| **0** | `cargo check` + `fmt --check` + `clippy` on changed crate | PostToolUse on every `.rs` Edit/Write | $0 | ≤5 s |
| **1** | Path-scoped `cargo test` on the affected module | Inside Tier 0 for test files; manual otherwise | $0 | ≤2 min |
| **2** | Deterministic E2E via PR-2.7 recorded corpus + all integration tests | Manual; PR creation; pre-merge | $0 (no network) | ≤10 min |
| **3** | Live OpenRouter calls (`real_llm.rs`, future benchmarks) | Manual only; daily budget-capped | $ (hard cap) | ≤30 min |

Guiding principle: **routine edits get Tier 0 for free; everything expensive is explicit.**

## Files

```
scripts/harness/
├── common.sh              — shared helpers (logging, timing, stdin parsing)
├── strategic-test.sh      — PostToolUse dispatcher (reads stdin, decides)
├── tier-0.sh              — cargo check + fmt + clippy
├── tier-1.sh              — path-scoped cargo test
├── tier-2.sh              — corpus replay + all integration tests (no network)
├── tier-3.sh              — real_llm.rs + future live benches (budget-gated)
├── budget.sh              — OpenRouter spend tracker (record + query)
└── status.sh              — human-readable dashboard of recent runs

.harness/logs/             — JSONL activity log (gitignored)
├── harness-runs.jsonl     — every hook invocation with path, tier, outcome, ms
└── openrouter-spend.jsonl — every Tier-3 call with model, tokens, cost
```

## How it fires

- **`.claude/settings.json`** registers a `PostToolUse` hook on `Edit|Write|NotebookEdit` that invokes `strategic-test.sh`.
- **`.claude/settings.local.json`** keeps the existing `Stop` hook running `rigor` (dogfood; untouched by this harness).
- The hook receives `{tool_name, tool_input}` on stdin. `strategic-test.sh` parses `tool_input.file_path`, decides:
  - Not `.rs` → log + exit 0.
  - In `target/`, `node_modules/`, `vendor/` → log + exit 0.
  - Otherwise → run Tier 0 on the file's crate.
  - If the edited file is a test file (`.../tests/*.rs`) → also run Tier 1 for that single test (cheap, direct feedback).

## Manual commands

```bash
# Tier 0 on a specific file (or whole crate if no arg).
bash scripts/harness/tier-0.sh [<path>]

# Tier 1 path-scoped tests.
bash scripts/harness/tier-1.sh [<path>]

# Tier 2 — all deterministic integration tests; no network.
bash scripts/harness/tier-2.sh

# Tier 3 — LIVE OpenRouter calls. Requires OPENROUTER_API_KEY + daily budget OK.
bash scripts/harness/tier-3.sh [--dry-run] [--force]

# Spend tracker.
bash scripts/harness/budget.sh today
bash scripts/harness/budget.sh week
bash scripts/harness/budget.sh all
bash scripts/harness/budget.sh tail 20

# Dashboard.
bash scripts/harness/status.sh           # summary: recent runs + today's spend
bash scripts/harness/status.sh runs 50   # last 50 run entries raw
bash scripts/harness/status.sh tiers     # count by tier
bash scripts/harness/status.sh failures  # failed runs only
```

## Budget controls

Tier 3 enforces a **daily USD cap** (default `$5.00`, override with `HARNESS_TIER3_DAILY_CAP=N.NN`).

- `tier-3.sh` computes today's spend from `.harness/logs/openrouter-spend.jsonl` before running.
- Over-cap runs refuse by default. Pass `--force` to override (prints warning).
- Every Tier-3 call records `{ts, model, tokens_in, tokens_out, cost_usd, note}` via `budget.sh record`.
- Cost-per-call recording is currently a placeholder (~$0.05/run) — full token-cost wiring lands when the evaluator pipeline exposes `reqwest` response headers. Placeholder is conservative; real numbers will only go lower.

## Path → tier-scope map

`tier-1.sh` uses this dispatch when invoked with a file path:

| Path pattern | Test scope |
|---|---|
| `src/memory/*` | `--lib memory::` |
| `src/constraint/*` | `--lib constraint::` |
| `src/claim/*` | `--lib claim::` |
| `src/corpus/*` | `--lib corpus::` |
| `src/daemon/*` | `--lib daemon::` |
| `src/policy/*` | `--lib policy::` |
| `src/evaluator/*` | `--lib evaluator::` |
| `src/violation/*` | `--lib violation::` |
| `src/logging/*` | `--lib logging::` |
| `src/config/*` | `--lib config::` |
| `src/fallback/*`, `src/hook/*`, `src/alerting/*`, `src/observability/*`, `src/lsp/*` | `--lib <module>::` |
| `tests/<name>.rs` | `--test <name>` |
| `src/cli/*`, `src/defaults/*`, `src/main.rs`, `src/lib.rs` | `--lib` (bounded full lib tests) |

To extend: add a new case to the `case "$TARGET_PATH" in` block in `tier-1.sh`.

## Environment overrides

| Variable | Effect |
|---|---|
| `HARNESS_SKIP_POSTTOOL=1` | Disable the PostToolUse hook entirely. Useful when iterating on non-code files. |
| `HARNESS_TIER0_ONLY=1` | On test-file edits, skip the Tier-1 single-test re-run. |
| `HARNESS_LOG_VERBOSE=1` | Echo dispatch reasoning to stderr. |
| `HARNESS_TIER3_DAILY_CAP=N.NN` | Override Tier-3 daily USD cap. |
| `CLAUDE_PROJECT_DIR` | Set automatically by Claude Code. Scripts use git root as fallback. |

## Tier-2 test set (deterministic, no network)

All tests listed below run without `OPENROUTER_API_KEY` — `tier-2.sh` explicitly `unset`s it.

- `corpus_replay` — PR-2.7 recorded-sample replay
- `true_e2e` — full proxy + evaluator flow
- `claim_extraction_e2e` — extraction + classification
- `dogfooding` — rigor evaluating rigor's own output
- `egress_integration` — response-path filter chain
- `firing_matrix` — constraint-firing coverage
- `invariants` — behavioral invariants
- `integration_constraint` — constraint system integration
- `integration_hook` — stop-hook evaluation
- `fallback_integration` — fallback-config integration
- `false_positive` — false-positive probes

## Tier-3 test set (live OpenRouter)

- `real_llm` — issues a real completion request using current model config; auto-skips when `OPENROUTER_API_KEY` is unset.
- **Future:** `rigor bench` harness (post-EC-11) with held-out constraints against a fresh model pass.

## Why this harness exists

Rigor's long-term answer to "catch hallucinations before they commit" is the Epistemic Cortex (umbrella #34). Until the cortex ships, this harness provides the minimum test-automation discipline: every edit gets type-checked and linted; every test-file change runs its test; full integration runs are one command away; live-LLM regression is explicit and budgeted.

When the cortex lands, this harness likely gets subsumed — `rigor tick` will replace `strategic-test.sh`, the event log will replace `.harness/logs/`, and the cortex's OTel spans will replace the JSONL timing records. Until then, these scripts are the transitional scaffolding.

## Gotchas

- **Hooks don't fire immediately in the first session after adding them.** Claude Code's settings watcher only watches directories that had a settings file at session start. After editing `.claude/settings.json`, either open `/hooks` once (reloads config) or restart the session.
- **Clippy warnings at Tier 0 are `-W` not `-D`.** A warning won't block an edit; a hard error will. For stricter gating before commit, run `cargo clippy -- -D warnings` manually or via pre-commit.
- **Logs grow unbounded.** `.harness/logs/*.jsonl` is never truncated by this harness. Rotate manually with standard tools (`logrotate`, `find -mtime +30 -delete`) if they get large.
- **`cargo check` assumes a populated target/ cache.** Cold builds take minutes; warm `cargo check` is seconds. Tier 0's ≤5s target assumes a warm cache. First run of the day will overshoot; subsequent runs stay fast.
- **Tier 2 is NOT run automatically.** If you want it to run on every Stop, add a separate Stop hook in `.claude/settings.json` invoking `scripts/harness/tier-2.sh` — but note it takes minutes and will delay session close.

## Next improvements (not yet built)

- Hook a pre-commit git hook to Tier 1 + clippy `-D warnings` for pre-commit enforcement.
- Per-path test-file existence check in `strategic-test.sh` — warn (not block) when editing non-test source without a corresponding test file. Reinforces TDD discipline from `feedback_tdd.md` memory.
- Wire real token/cost telemetry from the evaluator pipeline into `budget.sh record` so Tier-3 tracking reflects actual OpenRouter spend, not placeholders.
- Roll this into the cortex once EC-10 ships — replace logs with `belief_events`, replace strategic-test.sh with `rigor tick`.
