#!/usr/bin/env bash
# scripts/harness/tier-1.sh — targeted unit + integration tests (<2min target).
# Runs cargo test scoped to affected module when given a file path;
# full crate tests when invoked without args.
#
# Usage: tier-1.sh [<file_path>]

set -uo pipefail
source "$(dirname "$0")/common.sh"

TARGET_PATH="${1:-}"
START_MS=$(date +%s%N)
OUTCOME="pass"

print_section "Tier 1 (unit + integration) — target: ${TARGET_PATH:-<all>}"

cd "$REPO_ROOT" || { echo "repo root missing" >&2; exit 2; }

# Decide which test scope to run, based on file path.
TEST_ARGS=()
if [[ -n "$TARGET_PATH" ]]; then
    case "$TARGET_PATH" in
        */src/memory/*)             TEST_ARGS=(-p rigor --lib memory:: ) ;;
        */src/constraint/*)         TEST_ARGS=(-p rigor --lib constraint:: ) ;;
        */src/claim/*)              TEST_ARGS=(-p rigor --lib claim:: ) ;;
        */src/corpus/*)             TEST_ARGS=(-p rigor --lib corpus:: ) ;;
        */src/daemon/*)             TEST_ARGS=(-p rigor --lib daemon:: ) ;;
        */src/policy/*)             TEST_ARGS=(-p rigor --lib policy:: ) ;;
        */src/evaluator/*)          TEST_ARGS=(-p rigor --lib evaluator:: ) ;;
        */src/violation/*)          TEST_ARGS=(-p rigor --lib violation:: ) ;;
        */src/logging/*)            TEST_ARGS=(-p rigor --lib logging:: ) ;;
        */src/config/*)             TEST_ARGS=(-p rigor --lib config:: ) ;;
        */src/fallback/*)           TEST_ARGS=(-p rigor --lib fallback:: ) ;;
        */src/hook/*)               TEST_ARGS=(-p rigor --lib hook:: ) ;;
        */src/alerting/*)           TEST_ARGS=(-p rigor --lib alerting:: ) ;;
        */src/observability/*)      TEST_ARGS=(-p rigor --lib observability:: ) ;;
        */src/lsp/*)                TEST_ARGS=(-p rigor --lib lsp:: ) ;;
        */tests/*.rs)
            # Specific integration test file.
            test_name=$(basename "$TARGET_PATH" .rs)
            TEST_ARGS=(-p rigor --test "$test_name")
            ;;
        */src/cli/*|*/src/defaults/*|*/src/main.rs|*/src/lib.rs)
            # CLI/lib entry points → run the whole crate's lib tests, keep it bounded.
            TEST_ARGS=(-p rigor --lib)
            ;;
        *)                          TEST_ARGS=(-p rigor --lib) ;;
    esac
else
    TEST_ARGS=(-p rigor)
fi

# Exclude real_llm.rs by default — that's Tier 3 territory.
# cargo test doesn't have a clean "exclude test" flag; instead rely on real_llm.rs's env-gate.
if ! time_cmd "cargo test ${TEST_ARGS[*]}" cargo test "${TEST_ARGS[@]}" --no-fail-fast 2>&1 | tail -30; then
    OUTCOME="fail"
fi

END_MS=$(date +%s%N)
DURATION=$(( (END_MS - START_MS) / 1000000 ))
log_run "tier-1" "${TARGET_PATH:-<all>}" "$OUTCOME" "$DURATION"

print_section "Tier 1 done (${DURATION}ms, $OUTCOME)"
[[ "$OUTCOME" == "pass" ]]
