# Rigor Roadmap

**Last updated:** 2026-04-19
**Source codebase map:** `.planning/codebase/`
**Inspiration:** geohot's zappa MITM proxy (2026-04-15) — structurally identical architecture (CA + MITM + LLM mediation), pointed at inbound web traffic instead of outbound LLM traffic. Ideas below are transplanted from zappa to rigor.

Tags used below:
- `[HACKATHON]` — shortlisted for this hackathon effort; demo-shaped, single-sitting scope
- `[ARCH]` — architectural shift, non-trivial, touches core assumptions
- `[UX]` — onboarding / ergonomics / developer experience
- `[PRINCIPLE]` — philosophical/framing change that reshapes how rigor is positioned

Status markers: `[ ]` todo · `[~]` in progress · `[x]` done · `[-]` dropped

---

## Hackathon shortlist

Four items that compose into a demo narrated as *"rigor, but explicitly user-aligned against AI enshittification"* without rewriting the core.

### 1. `HedgeSofteningFilter` in the egress chain `[HACKATHON]`

- [ ] Add a response-side egress filter that rewrites overconfident assertions mid-stream ("X is Y" → "X appears to be Y").
- **Where:** new file `crates/rigor/src/daemon/egress/hedge_softening.rs`; register in `crates/rigor/src/daemon/egress/mod.rs`; wire into the proxy SSE pipeline in `crates/rigor/src/daemon/proxy.rs`.
- **Why:** demonstrates mid-stream *rewriting* (the zappa move) not just blocking. Builds on the existing `EgressFilter` trait in `crates/rigor/src/daemon/egress/chain.rs:41` and the precedent set by `claim_injection.rs` (which already mutates request bodies).
- **Risk:** mid-stream token rewriting can confuse client parsers. Start with whole-sentence replacement at SSE chunk boundaries, not token-level.
- **Test:** extend `crates/rigor/tests/egress_integration.rs` with a hand-rolled mock filter (pattern already in that file).

### 2. Three anti-enshittification Rego packs `[HACKATHON]`

- [ ] `policies/builtin/no-sycophancy.rego` — detects claims matching "Great question!", "Excellent!", "Absolutely!", etc.
- [ ] `policies/builtin/no-cya-hedging.rego` — detects "As an AI, I should note…", "I cannot…", "please consult a professional" in non-legal contexts.
- [ ] `policies/builtin/no-upsell.rego` — detects "Claude Pro", "upgrade to", "premium tier", etc. in assistant output.
- **Where:** new `.rego` files under `policies/builtin/`; example rigor.yaml fragment added to `examples/` directory.
- **Why:** zappa targets *"ads, popups, bright colors, moving things, enshittified crap"*. The AI equivalents are sycophancy, CYA, and upsells. These become first-class constraint categories beyond rigor's current belief/justification/defeater epistemic focus.
- **Pattern to follow:** copy structure from `policies/builtin/calibrated-confidence.rego` and `policies/builtin/no-fabricated-apis.rego`. Use Rego v1 syntax (`violation contains v if`).

### 3. `rigor protect` one-shot install `[HACKATHON]` `[UX]`

- [ ] New CLI subcommand that runs `init` → `trust` → `ground` with sane defaults in one go.
- **Where:** add `Commands::Protect` variant to `crates/rigor/src/cli/mod.rs`; new file `crates/rigor/src/cli/protect.rs` with `run_protect(...)`.
- **Why:** zappa's prompt installs mitmproxy + configures Firefox SOCKS + installs CA + plugin in one prompt. Rigor currently needs 5 separate steps. A single command aligns with zappa's "just works" energy.
- **Pattern to follow:** existing CLI subcommand conventions per `.planning/codebase/STRUCTURE.md` → *"New CLI Subcommand"* section (lines 261–265).

### 4. Dashboard "blocked today" counter `[HACKATHON]` `[UX]`

- [ ] Running count of dark patterns blocked this session (sycophancy hits, upsells stripped, hedges softened). Shows before/after diffs of rewritten text.
- **Where:** new `DaemonEvent` variant in `crates/rigor/src/daemon/ws.rs`; counter state added to `DaemonState` in `crates/rigor/src/daemon/mod.rs`; dashboard JS in `viewer/index.html` + `viewer/style.css` consuming the events.
- **Why:** concrete evidence of value delivered. The "this is what your AI would have said" diff view makes the filtering visible and tangible — good demo material.

---

## Architectural ideas (post-hackathon)

### 5. Fail-LOUD as the default `[ARCH]` `[PRINCIPLE]`

- [ ] Flip the default posture for the daemon path. Keep fail-open for the Stop hook only (to avoid breaking Claude Code).
- [ ] Make degraded state visible in the dashboard as a red banner, not just a stderr log line.
- [ ] Treat `RIGOR_FAIL_CLOSED` as sane default; add `RIGOR_FAIL_OPEN` as explicit opt-out.
- **Where:** `crates/rigor/src/main.rs:8` (env var handling); `crates/rigor/src/lib.rs:111–194` (per-step fail-open warn! → decision); `crates/rigor/src/daemon/ws.rs` (new banner event).
- **Why:** zappa's framing — *"If the AI returns an error, pass that error along to the user, do not return pages without AI transformation"* — is that fail-open IS the anti-user choice. The user thinks they're protected but isn't. Rigor's current behavior (fail-open everywhere per `.planning/codebase/CONVENTIONS.md` "Fail-open discipline" section) is the opposite.
- **Risk:** changing the Stop-hook default would break Claude Code workflows. Scope the flip to daemon/proxy only at first.

### 6. Cheap parallel judge `[ARCH]`

- [ ] Default judge model → a small/fast OpenRouter model or configurable Cerebras-speed endpoint.
- [ ] Raise `RELEVANCE_SEMAPHORE` permit pool from 1 to 3–5; queue instead of drop.
- [ ] Add LRU eviction to `RELEVANCE_CACHE`.
- **Where:** `crates/rigor/src/daemon/proxy.rs:2504–2722` (judge system); `crates/rigor/src/daemon/proxy.rs:2515` (semaphore); `crates/rigor/src/daemon/proxy.rs:2517–2519` (cache); `crates/rigor/src/cli/config.rs:63` (judge config resolution).
- **Why:** zappa leans on Cerebras-speed Qwen to make per-page LLM calls tractable — *"Imagine a skilled software engineer running in 100x real time"*. Rigor's judge currently runs serial (permit=1), against a frontier model (`anthropic/claude-sonnet-4-6`), with an unbounded cache. `.planning/codebase/CONCERNS.md` already flags all three as concerns (sections "Relevance scoring…", "RELEVANCE_CACHE is unbounded"). Zappa's answer — small model, cheap inference, run it on everything — is the right posture flip.
- **Stretch:** judge-per-SSE-chunk becomes affordable once inference is cheap.

### 7. Shareable constraint packs (uBlock filter-list model) `[ARCH]`

- [ ] `rigor subscribe <url>` pulls a signed `.yaml` bundle.
- [ ] Author-signed packs; extend `rigor trust` from CA-only to author-key trust store.
- [ ] A discoverable index / registry.
- **Where:** new `crates/rigor/src/cli/subscribe.rs`; extend `crates/rigor/src/daemon/tls.rs` trust metaphor to a separate `crates/rigor/src/trust/` module covering both CA and author keys; new `~/.rigor/trusted-authors.pem`.
- **Why:** zappa's *"people can share prompts like they share uBlock Origin filter lists"* maps directly onto `policies/builtin/` + `rigor.yaml` fragments. Only distribution + signing is missing.
- **Pattern overload note:** the word "trust" already means "install the CA" in rigor today. Be deliberate about extending it vs. using a distinct verb.

### 8. Per-project / per-agent memory `[ARCH]`

- [ ] Persist a "this agent previously claimed X, refuted" index keyed off `session_id` + git SHA.
- [ ] Feed history into the judge prompt: "this agent has a track record of fabricating crate APIs; weight skepticism higher."
- **Where:** new `crates/rigor/src/memory/` module, JSONL-backed like `~/.rigor/violations.jsonl`. Uses `SessionMetadata::capture` already present in `crates/rigor/src/logging/session.rs:3` (git SHA + branch already captured).
- **Why:** zappa says *"it should use tools and keep per site state"*. Rigor already has `ConversationCtx` in the egress chain and session IDs but no persistent per-agent history.

---

## Practical / UX ideas

### 9. Natural-language constraints `[UX]`

- [ ] Add `constraints.prose:` section in `rigor.yaml` evaluated directly by the judge LLM (no Rego authoring).
- **Where:** extend `crates/rigor/src/constraint/types.rs::RigorConfig`; new extractor-adjacent trait in `crates/rigor/src/policy/`; bolts onto existing judge infrastructure at `crates/rigor/src/daemon/proxy.rs:2504–2722`.
- **Why:** zappa is *a single English prompt*. Rigor requires Rego + YAML + an argumentation graph tutorial (`docs/constraint-authoring.md` is 20kB). English-only constraints massively lower the onboarding cliff. Judge-evaluated so existing infra handles it.
- **Risk:** non-deterministic evaluation (LLM calls). Keep the Rego path as the canonical/reproducible one; mark prose constraints as "advisory" tier by default.

---

## Explicitly NOT doing

Stated here so future contributors don't re-debate:

- [-] Browser-extension framing for rigor. Rigor's LD_PRELOAD + MITM layer (`layer/src/lib.rs`, `crates/rigor/src/daemon/tls.rs`) is strictly more powerful than an extension; no need to retreat.
- [-] "Agentic" self-modifying constraint packs. Zappa hand-waves this. Rigor's DF-QuAD argumentation graph (`crates/rigor/src/constraint/graph.rs`) is already a more principled substrate than an LLM-with-scratchpad.

---

## Cross-references

- Codebase map: `.planning/codebase/{STACK,ARCHITECTURE,STRUCTURE,CONVENTIONS,INTEGRATIONS,TESTING,CONCERNS}.md`
- Original inspiration: https://geohot.github.io/blog/jekyll/update/2026/04/15/zappa-mitmproxy.html
- GSD workflow commands: `/gsd/plan-phase`, `/gsd/execute-phase` when ready to break a roadmap item into a phase plan.
