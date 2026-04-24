# Phase 18: pr-injection-scan.yml self-regression corpus - Context

**Gathered:** 2026-04-24
**Status:** Ready for planning
**Mode:** Auto-generated (discuss skipped per autonomous mode)

<domain>
## Phase Boundary

Add fixture corpus that exercises all 9 regex patterns + 30KB-capped judge path. Create positives (known-bad) and negatives (benign) fixture directories with a regression test script.

Requirements: REQ-033

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All at Claude's discretion. Per issue #14:
- Create fixtures directory with positives/ (20+ known-bad) and negatives/ (20+ benign)
- Add regression test script that runs regex patterns against fixtures
- Over-editing guard: don't modify pr-injection-scan.yml itself, only add fixtures + test

</decisions>

<code_context>
## Existing Code Insights

- .github/workflows/pr-injection-scan.yml has 9 regex patterns at lines 52-63
- 30KB cap for semantic scan
- No existing fixtures directory

</code_context>

<specifics>
## Specific Ideas

None beyond issue #14.

</specifics>

<deferred>
## Deferred Ideas

None.

</deferred>
