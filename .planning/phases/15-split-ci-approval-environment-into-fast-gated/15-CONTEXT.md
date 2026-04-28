# Phase 15: Split ci-approval environment into fast/gated - Context

**Gathered:** 2026-04-24
**Status:** Already complete
**Mode:** Pre-existing work detected

<domain>
## Phase Boundary

Already shipped in commit `beffd81` (PR #47). Only 1 `ci-approval` reference remains in ci.yml (the LLM-credit-touching job). clippy/rustfmt/test/rigor-validate run without approval gate.

</domain>
