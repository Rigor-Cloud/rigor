#!/usr/bin/env bash
# scripts/harness/common.sh — shared helpers sourced by every harness script.
set -u

# Resolve repo root. Hooks run with CLAUDE_PROJECT_DIR; manual runs use git.
REPO_ROOT="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
LOG_DIR="$REPO_ROOT/.harness/logs"
RUNS_LOG="$LOG_DIR/harness-runs.jsonl"
SPEND_LOG="$LOG_DIR/openrouter-spend.jsonl"

# Daily Tier-3 cap in USD. Override via HARNESS_TIER3_DAILY_CAP env.
DAILY_CAP="${HARNESS_TIER3_DAILY_CAP:-5.00}"

mkdir -p "$LOG_DIR"

# log_run <tier> <path> <outcome> <duration_ms> [extra_json]
log_run() {
    local tier="$1" path="$2" outcome="$3" duration_ms="$4" extra="${5:-{\}}"
    local ts
    ts=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    printf '{"ts":"%s","tier":"%s","path":"%s","outcome":"%s","duration_ms":%d,"extra":%s}\n' \
        "$ts" "$tier" "$path" "$outcome" "$duration_ms" "$extra" >> "$RUNS_LOG"
}

# time_cmd <label> <cmd...>  — runs cmd, prints label+duration, returns cmd exit code.
time_cmd() {
    local label="$1"; shift
    local start end duration rc
    start=$(date +%s%N)
    "$@"
    rc=$?
    end=$(date +%s%N)
    duration=$(( (end - start) / 1000000 ))
    if [[ $rc -eq 0 ]]; then
        printf "  ✓ %-30s (%dms)\n" "$label" "$duration" >&2
    else
        printf "  ✗ %-30s (%dms, rc=%d)\n" "$label" "$duration" "$rc" >&2
    fi
    return $rc
}

# print_section <title>
print_section() {
    printf "\n── %s ──\n" "$1" >&2
}

# extract file_path from PostToolUse stdin JSON. Returns empty string if not Edit/Write/NotebookEdit.
extract_edited_path() {
    if [[ -t 0 ]]; then return 0; fi
    local input tool fp
    input=$(cat)
    tool=$(printf '%s' "$input" | python3 -c 'import sys,json
try: print(json.load(sys.stdin).get("tool_name",""))
except: pass' 2>/dev/null)
    case "$tool" in
        Edit|Write|NotebookEdit)
            printf '%s' "$input" | python3 -c 'import sys,json
try:
    d=json.load(sys.stdin)
    print(d.get("tool_input",{}).get("file_path") or d.get("tool_input",{}).get("notebook_path") or "")
except: pass' 2>/dev/null
            ;;
        *) : ;;
    esac
}

export REPO_ROOT LOG_DIR RUNS_LOG SPEND_LOG DAILY_CAP
