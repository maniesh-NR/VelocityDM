#!/usr/bin/env bash
# Poll VelocityDM workflow runs for a given commit until all are finished or
# the loop budget ends. The poll window is ANCHORED to the given commit's
# runs (default: the repo HEAD) — historical failures on other commits are
# printed as context but never fail the gate.
# Exit codes: 0 = all anchored runs completed and green, 1 = anchored
# failures found, 2 = still pending when the budget ran out (TIMEOUT).
# Usage: poll-runs.sh [seconds] [expected_head_sha]
TOK=$(cat /home/z/my-project/.gh_token)
API=https://api.github.com/repos/maniesh-NR/VelocityDM
BUDGET=${1:-1800}
EXPECT_SHA=${2:-$(git -C /home/z/my-project/velocitydm-repo rev-parse HEAD 2>/dev/null)}
[ -n "$EXPECT_SHA" ] || { echo "no expected sha (pass one or run inside the repo)"; exit 2; }
END=$(( $(date +%s) + BUDGET ))
STATUS=2
while [ "$(date +%s)" -lt "$END" ]; do
  curl -s -H "Authorization: Bearer $TOK" "$API/actions/runs?per_page=60" -o /tmp/runs.json
  python3 - "$EXPECT_SHA" <<'PYEOF'
import json, sys
expect = sys.argv[1]
d = json.load(open('/tmp/runs.json'))
runs = d.get('workflow_runs', [])
anchored = [r for r in runs if r['head_sha'] == expect]
pending = [r for r in anchored if r['status'] != 'completed']
failed = [r for r in anchored if r['conclusion'] not in ('success', 'skipped', None)]
lines = []
for r in runs:
    mark = (' ' if r['head_sha'] != expect
            else ('*' if r['status'] != 'completed'
                  else ('+' if r['conclusion'] == 'success'
                        else ('-' if r['conclusion'] in ('skipped', None) else 'X'))))
    lines.append(f"{mark} #{r['run_number']} id={r['id']} {r['name']:10} {r['event']:18} {r['head_branch']:22} {r['head_sha'][:8]} {r['conclusion'] or r['status']}")
print('\n'.join(lines))
print(f"ANCHORED (head_sha {expect[:8]}): {len(anchored)} runs, PENDING={len(pending)}, FAILED={len(failed)}")
open('/tmp/pending_count', 'w').write(str(len(pending)))
open('/tmp/failed_count', 'w').write(str(len(failed)))
open('/tmp/anchored_count', 'w').write(str(len(anchored)))
PYEOF
  if [ "$(cat /tmp/failed_count)" != "0" ]; then
    echo "FAILURES DETECTED for $EXPECT_SHA — see the X rows above"
    STATUS=1
    break
  fi
  # An empty anchored set is NOT green: no runs registered yet for this sha
  # (registration race, a typo'd sha, or HEAD advanced past the pushed one) —
  # keep polling; the budget ends in TIMEOUT, never a false ALL DONE.
  if [ "$(cat /tmp/pending_count)" = "0" ] && [ "$(cat /tmp/anchored_count)" -ge 1 ]; then
    echo "ALL DONE AND GREEN for $EXPECT_SHA ($(cat /tmp/anchored_count) runs)"
    STATUS=0
    break
  fi
  sleep 30
done
if [ "$STATUS" = "2" ]; then
  echo "TIMEOUT: workflows still pending after ${BUDGET}s — run this script again to continue polling"
fi
exit $STATUS
