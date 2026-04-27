#!/usr/bin/env bash
# scripts/harness/status.sh — human-readable dashboard of recent harness activity.
#
# Usage:
#   status.sh              — summary: last 10 runs + today's spend.
#   status.sh runs [N]     — last N runs as JSONL.
#   status.sh tiers        — count runs by tier.
#   status.sh failures     — tail of failed runs.

set -uo pipefail
source "$(dirname "$0")/common.sh"

CMD="${1:-summary}"

case "$CMD" in
    summary)
        echo "── Rigor harness status ──"
        echo
        echo "Runs log:  $RUNS_LOG"
        echo "Spend log: $SPEND_LOG"
        echo
        bash "$(dirname "$0")/budget.sh" today
        bash "$(dirname "$0")/budget.sh" week
        echo
        echo "Last 10 runs:"
        [[ -f "$RUNS_LOG" ]] && tail -n 10 "$RUNS_LOG" | python3 -c "
import sys, json
for line in sys.stdin:
    try:
        r = json.loads(line)
        print(f\"  [{r['ts'][:19]}] {r['tier']:<14} {r['outcome']:<8} {r['duration_ms']:>6}ms  {r['path']}\")
    except: print('  (parse error)')
" || echo "  (no runs yet)"
        ;;
    runs)
        N="${2:-20}"
        [[ -f "$RUNS_LOG" ]] && tail -n "$N" "$RUNS_LOG" || echo "no runs"
        ;;
    tiers)
        [[ -f "$RUNS_LOG" ]] && python3 -c "
import json, collections, sys
c=collections.Counter()
for line in open('$RUNS_LOG'):
    try: c[json.loads(line).get('tier','?')] += 1
    except: pass
for k,v in c.most_common(): print(f'  {k:<18} {v}')
" || echo "no runs yet"
        ;;
    failures)
        [[ -f "$RUNS_LOG" ]] && grep '"outcome":"fail"' "$RUNS_LOG" | tail -20 || echo "no failures logged"
        ;;
    *)
        echo "usage: status.sh {summary|runs [N]|tiers|failures}" >&2
        exit 1
        ;;
esac
