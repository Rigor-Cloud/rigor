# Changelog

All notable changes to rigor are documented here. Release notes for each
version are extracted from this file by the release workflow.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- **Semantic evaluator.** Constraints tagged `semantic` are routed to a
  `SemanticEvaluator` that consumes LLM-as-judge relevance verdicts
  instead of running a Rego snippet. Verdicts are produced once by the
  daemon's judge pass and shared with the stop-hook subprocess via a new
  `/api/relevance/lookup` endpoint. Fail-open in both paths.
- `rigor trust opencode` — wrapper shim for zero-friction routing on
  environments where a classic `HTTPS_PROXY` export would disable
  OpenCode's OAuth.
- `rigor serve --background` — persistent daemon mode that any LLM tool
  can proxy through. Dashboard at `http://rigor.local:8787` with 3D
  constraint graph, live traffic, sessions, search, eval.
- **OpenCode integration** with session tracking and violation
  persistence. `.opencode/plugins/` auto-routes OpenCode traffic through
  rigor.
- **Dashboard UX**: session dropdown in the header bar (filters the
  LIVE view); SESSIONS / SEARCH / EVAL promoted to top-level tabs.
- rigor SVG logo + avatar assets.

### Changed
- **License: MIT → Apache 2.0.** Explicit patent grant and clearer
  contributor-IP story. No downstream friction expected; Apache is the
  standard for serious Rust projects and all our dependencies use
  Apache or Apache+MIT dual.
- `rigor serve` runs globally without a project context (was previously
  required).
- `rigor serve` works without a `rigor.yaml` (falls back to builtin
  packs); `rigor.local` hostname supported.

### Infra
- Repo consolidated at `Rigor-Cloud/rigor` (previously
  `waveywaves/rigor`).
- Release pipeline, LICENSE, and CHANGELOG imported into this repo.
- Marketing site live at rigorcloud.com (source: `Rigor-Cloud/website`)
  with waitlist + DodoPayments integration for Priority ($19) and
  Design Partner ($199) tiers.

## v0.1.0 — 2026-04-18

### Added
- Initial public release. Install with
  `brew tap Rigor-Cloud/tap https://github.com/Rigor-Cloud/rigor-releases`
  and `brew install rigor`.
- Epistemic constraint enforcement for LLM outputs: beliefs, justifications,
  defeaters, DF-QuAD argumentation semantics.
- Claude Code Stop hook integration.
- PII redaction, violation logging, OpenTelemetry tracing.
- Marketing site live at rigorcloud.com.

### Known limitations
- macOS only (arm64 + x86_64). Linux and Windows not yet supported.
- Binaries are not codesigned; Homebrew strips quarantine so install works,
  but double-clicking the binary outside of brew will trigger Gatekeeper.

## v0.0.0-rc1 (unreleased)

### Added
- Initial release candidate for Homebrew distribution pipeline smoke test.
- macOS arm64 + x86_64 binaries shipped via `Rigor-Cloud/homebrew-rigor`.
