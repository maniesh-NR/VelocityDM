#!/usr/bin/env bash
# Poll GitHub check-runs for a commit until all complete (gh-free).
# Usage: bash scripts/poll-checks.sh <sha> [timeout_s]
# Exit 0 = all green/skipped/neutral; 1 = failures; 2 = budget exhausted.
set -euo pipefail
SHA="${1:?sha required}"
BUDGET="${2:-2400}"
TOKEN_FILE="${TOKEN_FILE:-/home/z/my-project/.gh_token}"
TOK=$(cat "$TOKEN_FILE")
AUTH="Authorization: Bearer $TOK"
API="https://api.github.com/repos/maniesh-NR/VelocityDM"
START=$(( $(date +%s) ))

while true; do
  JSON=$(curl -s -H "$AUTH" -H "Accept: application/vnd.github+json" \
    "$API/commits/$SHA/check-runs?per_page=100")
  ROWS=$(echo "$JSON" | python3 -c '
import json, sys
d = json.load(sys.stdin)
for c in d.get("check_runs", []):
    print(c["status"], c.get("conclusion") or "-", c["name"])
')
  TOTAL=$(echo "$ROWS" | wc -l)
  DONE=$(echo "$ROWS" | awk '$1 == "completed"' | wc -l)
  if [ "$TOTAL" -ge 1 ] && [ "$DONE" -eq "$TOTAL" ]; then
    echo "$ROWS"
    echo "ALL $TOTAL CHECKS COMPLETED"
    BAD=$(echo "$ROWS" | awk '$2 != "success" && $2 != "skipped" && $2 != "-" && $2 != "neutral"')
    if [ -n "$BAD" ]; then
      echo "FAILURES:"
      echo "$BAD"
      exit 1
    fi
    exit 0
  fi
  NOW=$(( $(date +%s) ))
  if [ $(( NOW - START )) -gt "$BUDGET" ]; then
    echo "$ROWS"
    echo "BUDGET EXHAUSTED ($DONE/$TOTAL done)"
    exit 2
  fi
  echo "waiting: $DONE/$TOTAL completed ..."
  sleep 45
done
