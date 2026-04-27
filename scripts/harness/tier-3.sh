#!/usr/bin/env bash
# scripts/harness/tier-3.sh — live OpenRouter calls. Budget-gated and manually triggered.
# Runs real_llm.rs and (future) benchmark harness that hits live models.
# Requires OPENROUTER_API_KEY in environment. Hard-caps at DAILY_CAP.
#
# Usage:
#   tier-3.sh                       — run real_llm tests after budget check.
#   tier-3.sh --dry-run             — show what would run + current spend, no calls.
#   tier-3.sh --force               — bypass budget cap (prints warning).

set -uo pipefail
source "$(dirname "$0")/common.sh"

DRY_RUN=0
FORCE=0
for a in "$@"; do
    case "$a" in
        --dry-run) DRY_RUN=1 ;;
        --force)   FORCE=1   ;;
    esac
done

START_MS=$(date +%s%N)
OUTCOME="pass"

print_section "Tier 3 (live OpenRouter) — daily cap \$${DAILY_CAP}"

cd "$REPO_ROOT" || exit 2

if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
    echo "error: OPENROUTER_API_KEY is not set. Tier 3 requires it." >&2
    log_run "tier-3" "<preflight>" "no-key" 0
    exit 1
fi

# Compute today's spend from SPEND_LOG.
TODAY=$(date -u +"%Y-%m-%d")
TODAY_SPEND=$(python3 -c "
import json, sys, os
total = 0.0
path = '$SPEND_LOG'
today = '$TODAY'
if os.path.exists(path):
    for line in open(path):
        try:
            r = json.loads(line)
            if r.get('ts','').startswith(today):
                total += float(r.get('cost_usd', 0.0))
        except: pass
print(f'{total:.4f}')
")

echo "  Today ($TODAY): \$${TODAY_SPEND} spent" >&2
echo "  Daily cap:       \$${DAILY_CAP}" >&2

if [[ $DRY_RUN -eq 1 ]]; then
    echo "  (--dry-run) would run: cargo test -p rigor --test real_llm -- --ignored --nocapture" >&2
    exit 0
fi

OVER=$(python3 -c "print(1 if float('$TODAY_SPEND') >= float('$DAILY_CAP') else 0)")
if [[ "$OVER" == "1" && $FORCE -eq 0 ]]; then
    echo "  ! budget exceeded; refusing. Use --force to override." >&2
    log_run "tier-3" "<budget>" "blocked" 0
    exit 2
fi

# Run the live tests. real_llm.rs is gated on the env var and will execute.
if ! time_cmd "cargo test --test real_llm" cargo test -p rigor --test real_llm -- --nocapture 2>&1 | tail -40; then
    OUTCOME="fail"
fi

# NOTE: real spend-per-call recording is done inside test code via a helper
# (future wiring — currently real_llm.rs doesn't emit cost). Until wired, record a placeholder.
# This placeholder keeps the budget tracker honest even before per-call cost wiring lands.
"$(dirname "$0")/budget.sh" record --model "anthropic/claude-sonnet-4-6" --tokens-in 0 --tokens-out 0 --cost 0.05 --note "tier-3 placeholder"

END_MS=$(date +%s%N)
DURATION=$(( (END_MS - START_MS) / 1000000 ))
log_run "tier-3" "<all>" "$OUTCOME" "$DURATION" "{\"today_spend\":\"$TODAY_SPEND\"}"

print_section "Tier 3 done (${DURATION}ms, $OUTCOME)"
[[ "$OUTCOME" == "pass" ]]
