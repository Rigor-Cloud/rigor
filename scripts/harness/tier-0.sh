#!/usr/bin/env bash
# scripts/harness/tier-0.sh — fast local feedback (<5s target).
# Runs: cargo check (affected crate) + cargo fmt --check (changed file) + cargo clippy (changed module).
# Called automatically by PostToolUse hook via strategic-test.sh, or manually.
#
# Usage: tier-0.sh [<file_path>]   — if no path given, runs across whole rigor crate.

set -uo pipefail
source "$(dirname "$0")/common.sh"

TARGET_PATH="${1:-}"
START_MS=$(date +%s%N)
OUTCOME="pass"

print_section "Tier 0 (fast) — target: ${TARGET_PATH:-<all>}"

cd "$REPO_ROOT" || { echo "repo root missing" >&2; exit 2; }

# 1. cargo check on the rigor crate. Fastest signal; catches type errors.
if ! time_cmd "cargo check -p rigor" cargo check -p rigor 2>&1 | tail -20; then
    OUTCOME="fail"
fi

# 2. cargo fmt --check on the specific file (if given), else whole crate.
if [[ -n "$TARGET_PATH" && "$TARGET_PATH" == *.rs ]]; then
    if ! time_cmd "cargo fmt --check ($TARGET_PATH)" cargo fmt --check -- "$TARGET_PATH" 2>&1 | tail -10; then
        OUTCOME="fail"
    fi
else
    if ! time_cmd "cargo fmt --check" cargo fmt --check -p rigor 2>&1 | tail -10; then
        OUTCOME="fail"
    fi
fi

# 3. cargo clippy on the crate. Warning-level only to avoid blocking edits on pedantic lints.
#    Use -D warnings at Tier-1 or pre-commit.
if ! time_cmd "cargo clippy -p rigor" cargo clippy -p rigor --all-targets -- -W clippy::all 2>&1 | tail -15; then
    OUTCOME="fail"
fi

END_MS=$(date +%s%N)
DURATION=$(( (END_MS - START_MS) / 1000000 ))
log_run "tier-0" "${TARGET_PATH:-<all>}" "$OUTCOME" "$DURATION"

print_section "Tier 0 done (${DURATION}ms, $OUTCOME)"
[[ "$OUTCOME" == "pass" ]]
