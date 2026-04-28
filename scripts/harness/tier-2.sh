#!/usr/bin/env bash
# scripts/harness/tier-2.sh — deterministic e2e replay via recorded corpus (zero network cost).
# Runs: corpus_replay, true_e2e, claim_extraction_e2e, dogfooding, egress_integration, firing_matrix, invariants.
# These tests use the PR-2.7 recorded-sample corpus OR local fixtures — NO live LLM calls.
# Target: <10min. Run on PR creation or pre-merge.
#
# Usage: tier-2.sh        — run all Tier-2 integration tests.

set -uo pipefail
source "$(dirname "$0")/common.sh"

START_MS=$(date +%s%N)
OUTCOME="pass"

print_section "Tier 2 (deterministic e2e, no network)"

cd "$REPO_ROOT" || { echo "repo root missing" >&2; exit 2; }

# Explicitly unset OPENROUTER_API_KEY for this tier so real_llm.rs skips cleanly.
# Keeps Tier-2 guaranteed-free regardless of caller's shell state.
unset OPENROUTER_API_KEY

TIER2_TESTS=(
    corpus_replay
    true_e2e
    claim_extraction_e2e
    dogfooding
    egress_integration
    firing_matrix
    invariants
    integration_constraint
    integration_hook
    fallback_integration
    false_positive
)

for t in "${TIER2_TESTS[@]}"; do
    if ! time_cmd "cargo test --test $t" cargo test -p rigor --test "$t" --no-fail-fast 2>&1 | tail -12; then
        OUTCOME="fail"
        echo "  ! $t failed; continuing Tier 2" >&2
    fi
done

END_MS=$(date +%s%N)
DURATION=$(( (END_MS - START_MS) / 1000000 ))
log_run "tier-2" "<all>" "$OUTCOME" "$DURATION" "{\"tests\":${#TIER2_TESTS[@]}}"

print_section "Tier 2 done (${DURATION}ms, $OUTCOME)"
[[ "$OUTCOME" == "pass" ]]
