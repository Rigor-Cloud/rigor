#!/usr/bin/env bash
# scripts/harness/budget.sh — record and report OpenRouter spend.
#
# Usage:
#   budget.sh record --model <slug> --tokens-in N --tokens-out N --cost USD [--note "..."]
#   budget.sh today                           — print today's spend
#   budget.sh week                            — print last 7d spend
#   budget.sh all                             — print all-time spend
#   budget.sh tail [N]                        — show last N spend entries (default 10)

set -uo pipefail
source "$(dirname "$0")/common.sh"

CMD="${1:-today}"
shift || true

case "$CMD" in
    record)
        MODEL="" TIN=0 TOUT=0 COST=0 NOTE=""
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --model)      MODEL="$2"; shift 2 ;;
                --tokens-in)  TIN="$2";   shift 2 ;;
                --tokens-out) TOUT="$2";  shift 2 ;;
                --cost)       COST="$2";  shift 2 ;;
                --note)       NOTE="$2";  shift 2 ;;
                *) shift ;;
            esac
        done
        TS=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
        printf '{"ts":"%s","model":"%s","tokens_in":%s,"tokens_out":%s,"cost_usd":%s,"note":"%s"}\n' \
            "$TS" "$MODEL" "$TIN" "$TOUT" "$COST" "$NOTE" >> "$SPEND_LOG"
        echo "recorded: \$${COST} on $MODEL" >&2
        ;;
    today)
        T=$(date -u +"%Y-%m-%d")
        python3 -c "
import json, os, sys
total=0.0; n=0
if os.path.exists('$SPEND_LOG'):
    for line in open('$SPEND_LOG'):
        try:
            r=json.loads(line)
            if r.get('ts','').startswith('$T'):
                total+=float(r.get('cost_usd',0)); n+=1
        except: pass
print(f'today ($T):  \${total:.4f}  ({n} calls)  cap \${float(\"$DAILY_CAP\"):.2f}')
"
        ;;
    week)
        python3 -c "
import json, os, datetime
cutoff=(datetime.datetime.utcnow()-datetime.timedelta(days=7)).isoformat()+'Z'
total=0.0; n=0
if os.path.exists('$SPEND_LOG'):
    for line in open('$SPEND_LOG'):
        try:
            r=json.loads(line)
            if r.get('ts','') >= cutoff:
                total+=float(r.get('cost_usd',0)); n+=1
        except: pass
print(f'last 7d:  \${total:.4f}  ({n} calls)')
"
        ;;
    all)
        python3 -c "
import json, os
total=0.0; n=0
if os.path.exists('$SPEND_LOG'):
    for line in open('$SPEND_LOG'):
        try:
            r=json.loads(line); total+=float(r.get('cost_usd',0)); n+=1
        except: pass
print(f'all time: \${total:.4f}  ({n} calls)')
"
        ;;
    tail)
        N="${1:-10}"
        [[ -f "$SPEND_LOG" ]] && tail -n "$N" "$SPEND_LOG" || echo "no spend log yet"
        ;;
    *)
        echo "usage: budget.sh {record|today|week|all|tail}" >&2
        exit 1
        ;;
esac
