#!/usr/bin/env bash
# scripts/harness/overedit-guard.sh — PreToolUse hook for Edit/Write.
# Injects the preservation instruction from Rehir 2026 ("Coding Models Are Doing
# Too Much") into Claude's context before every edit. The article proved this
# universally reduces Levenshtein distance and added cognitive complexity across
# all frontier models.
#
# Also measures the old_string→new_string ratio for Edit calls and warns if
# the replacement is disproportionately larger (sign of over-editing).

set -uo pipefail
source "$(dirname "$0")/common.sh"

if [[ -t 0 ]]; then exit 0; fi

INPUT=$(cat)
TOOL=$(printf '%s' "$INPUT" | python3 -c 'import sys,json
try: print(json.load(sys.stdin).get("tool_name",""))
except: pass' 2>/dev/null)

case "$TOOL" in
    Edit)
        OLD_LEN=$(printf '%s' "$INPUT" | python3 -c 'import sys,json
try: print(len(json.load(sys.stdin).get("tool_input",{}).get("old_string","")))
except: print(0)' 2>/dev/null)
        NEW_LEN=$(printf '%s' "$INPUT" | python3 -c 'import sys,json
try: print(len(json.load(sys.stdin).get("tool_input",{}).get("new_string","")))
except: print(0)' 2>/dev/null)

        if [[ "$OLD_LEN" -gt 0 && "$NEW_LEN" -gt 0 ]]; then
            RATIO=$(python3 -c "print(round($NEW_LEN / $OLD_LEN, 2))" 2>/dev/null)
            if python3 -c "exit(0 if $NEW_LEN / max($OLD_LEN,1) > 3.0 and $NEW_LEN > 200 else 1)" 2>/dev/null; then
                echo "OVER-EDIT WARNING: replacement is ${RATIO}x larger than original (${OLD_LEN}→${NEW_LEN} chars). Verify every added line is required by the task. Do not add comments, refactor surrounding code, or introduce abstractions beyond what the task requires." >&2
                log_run "overedit-warn" "edit" "ratio-${RATIO}" 0 "{\"old_len\":$OLD_LEN,\"new_len\":$NEW_LEN}"
            fi
        fi
        ;;
    Write)
        FP=$(printf '%s' "$INPUT" | python3 -c 'import sys,json
try: print(json.load(sys.stdin).get("tool_input",{}).get("file_path",""))
except: pass' 2>/dev/null)
        if [[ -f "$FP" ]]; then
            EXISTING_SIZE=$(wc -c < "$FP" 2>/dev/null || echo 0)
            NEW_SIZE=$(printf '%s' "$INPUT" | python3 -c 'import sys,json
try: print(len(json.load(sys.stdin).get("tool_input",{}).get("content","")))
except: print(0)' 2>/dev/null)
            if [[ "$EXISTING_SIZE" -gt 100 ]]; then
                RATIO=$(python3 -c "print(round(abs($NEW_SIZE - $EXISTING_SIZE) / max($EXISTING_SIZE,1), 2))" 2>/dev/null)
                if python3 -c "exit(0 if abs($NEW_SIZE - $EXISTING_SIZE) / max($EXISTING_SIZE,1) > 0.5 and abs($NEW_SIZE - $EXISTING_SIZE) > 500 else 1)" 2>/dev/null; then
                    echo "OVER-EDIT WARNING: Write changes ${RATIO}x of file size (${EXISTING_SIZE}→${NEW_SIZE} bytes). Consider using Edit for targeted changes instead of rewriting the entire file." >&2
                    log_run "overedit-warn" "$FP" "write-ratio-${RATIO}" 0 "{\"existing\":$EXISTING_SIZE,\"new\":$NEW_SIZE}"
                fi
            fi
        fi
        ;;
    *)
        # Not Edit/Write — no preservation reminder needed.
        exit 0
        ;;
esac

# Emit the preservation instruction as structured JSON so Claude Code injects it
# into the model's context (not just the user's view).
REMINDER="PRESERVATION REMINDER: Try to preserve the original code and the logic of the original code as much as possible. Change only what the task requires. Do not add cognitive complexity (unnecessary abstractions, helper functions, or restructuring). Target Levenshtein distance of zero beyond the minimal fix."

printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"%s"}}\n' "$REMINDER"

exit 0
