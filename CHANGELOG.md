# Changelog

All notable changes to rigor are documented here. Release notes for each
version are extracted from this file by the release workflow.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## v0.1.0 — 2026-04-18

### Added
- Initial public release via `brew tap Rigor-Cloud/rigor`.
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
