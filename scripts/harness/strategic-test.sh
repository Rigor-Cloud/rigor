#!/usr/bin/env bash
# scripts/harness/strategic-test.sh — PostToolUse hook dispatcher.
# Reads the edited file path from stdin JSON (Claude Code's PostToolUse event shape),
# decides whether to run any tests, which tier, and scopes to affected modules.
#
# Design choices that keep this cheap enough to run on every edit:
#   - Only responds to Edit / Write / NotebookEdit on `.rs` files (skips everything else).
#   - Runs Tier 0 by default: cargo check + fmt --check + clippy (target ≤5s).
#   - For test files, ALSO runs that single test (cheap, immediate feedback).
#   - Skips when the edit is to markdown, YAML, lock files, etc.
#   - Logs every invocation (even no-ops) so the harness is auditable.
#
# Environment overrides:
#   HARNESS_SKIP_POSTTOOL=1       — disable this hook entirely.
#   HARNESS_TIER0_ONLY=1          — skip the test-file Tier-1 extension.
#   HARNESS_LOG_VERBOSE=1         — also echo dispatch reasoning to stderr.

set -uo pipefail
source "$(dirname "$0")/common.sh"

if [[ "${HARNESS_SKIP_POSTTOOL:-0}" == "1" ]]; then
    log_run "skip" "<hook>" "disabled" 0 '{"reason":"HARNESS_SKIP_POSTTOOL"}'
    exit 0
fi

FP=$(extract_edited_path)
if [[ -z "$FP" ]]; then
    # Not a file-mutation tool call. No-op.
    exit 0
fi

# Only react to Rust source edits.
if [[ "$FP" != *.rs ]]; then
    log_run "skip" "$FP" "non-rust" 0 '{"reason":"non-rust file"}'
    exit 0
fi

# Skip target/ and vendored paths.
case "$FP" in
    */target/*|*/node_modules/*|*/vendor/*)
        log_run "skip" "$FP" "ignored-path" 0 '{"reason":"ignored path"}'
        exit 0
        ;;
esac

[[ "${HARNESS_LOG_VERBOSE:-0}" == "1" ]] && echo "[harness] dispatching for $FP" >&2

# Run Tier 0 (fast).
bash "$(dirname "$0")/tier-0.sh" "$FP"
T0_RC=$?

# If the edited file is itself a test, run just that test — cheap and gives direct feedback.
if [[ "${HARNESS_TIER0_ONLY:-0}" != "1" && "$FP" == *"/tests/"*".rs" ]]; then
    test_name=$(basename "$FP" .rs)
    START_MS=$(date +%s%N)
    OUTCOME="pass"
    unset OPENROUTER_API_KEY  # keep test execution free under the hook.
    if ! time_cmd "cargo test --test $test_name (live)" \
            cargo test -p rigor --test "$test_name" --no-fail-fast 2>&1 | tail -20; then
        OUTCOME="fail"
    fi
    END_MS=$(date +%s%N)
    DURATION=$(( (END_MS - START_MS) / 1000000 ))
    log_run "tier-1-single" "$FP" "$OUTCOME" "$DURATION"
fi

# Return tier-0's result as the hook's exit. A non-zero exit here becomes a prompt
# to Claude via Claude Code's hook feedback mechanism.
exit $T0_RC
