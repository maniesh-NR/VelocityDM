#!/usr/bin/env bash
# Sandbox backup: snapshot the agent sandbox state to GitHub.
#
# THE RULE (harness/WORKFLOW.md): backups are event-driven — the last step
# of every execution plan — never a fixed time interval.
#
# Pushes to the 'sandbox-backup' branch of maniesh-NR/VelocityDM:
#   sandbox/scripts/        sandbox tooling (this script's siblings)
#   sandbox/release-notes/  generated release notes (if present)
#   sandbox/harness/        live agent-memory snapshot (from the repo tree)
#   sandbox/MANIFEST.txt   repo HEAD, tags, release-asset checksums
#
# Also appends one line to the repo's harness/BACKUPS.md (committed to main
# with the next stage commit; carried in the snapshot immediately).
#
# NEVER included: secrets (.gh_token), release binaries, the repo itself.
set -euo pipefail

TOKEN_FILE="${TOKEN_FILE:-/home/z/my-project/.gh_token}"
REPO="maniesh-NR/VelocityDM"
BRANCH="sandbox-backup"
ROOT="/home/z/my-project"
REPO_DIR="$ROOT/velocitydm-repo"
WORK="/tmp/vdm-sandbox-backup"

[ -f "$TOKEN_FILE" ] || { echo "BACKUP FAILED: no token file $TOKEN_FILE"; exit 1; }
TOK=$(cat "$TOKEN_FILE")
TOKB64=$(printf 'maniesh-NR:%s' "$TOK" | base64 -w0)
AUTH=(-c "http.extraheader=Authorization: Basic $TOKB64")

STAMP=$(date -u +%Y-%m-%dT%H:%M:%SZ)
TRIGGER="${1:-agent execution plan}"

rm -rf "$WORK"
mkdir -p "$WORK"
cd "$WORK"
git init -q
git remote add origin "https://github.com/$REPO.git"

# Continue the branch history when it already exists.
if git "${AUTH[@]}" fetch -q origin "$BRANCH" 2>/dev/null; then
  git checkout -q -b "$BRANCH" FETCH_HEAD
else
  git checkout -q -b "$BRANCH"
fi

mkdir -p sandbox
rm -rf sandbox/scripts sandbox/release-notes sandbox/harness
cp -r "$ROOT/scripts" sandbox/scripts
if [ -d "$ROOT/release-notes" ]; then
  cp -r "$ROOT/release-notes" sandbox/release-notes
fi
# Live harness memory snapshot (MEMORY/ROADMAP/BACKUPS/ASSESSMENTS).
if [ -d "$REPO_DIR/harness" ]; then
  cp -r "$REPO_DIR/harness" sandbox/harness
fi

{
  echo "backup: $STAMP"
  REPO_HEAD=$(git -C "$REPO_DIR" rev-parse HEAD 2>/dev/null || echo unknown)
  echo "repo HEAD: $REPO_HEAD"
  echo "repo branch: $(git -C "$REPO_DIR" rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"
  echo "tags: $(git -C "$REPO_DIR" tag 2>/dev/null | tr '\n' ' ')"
  echo "dirty files: $(git -C "$REPO_DIR" status --porcelain 2>/dev/null | wc -l)"
  echo
  echo "release-assets (sha256):"
  ( cd "$ROOT/release-assets" 2>/dev/null && sha256sum -- * 2>/dev/null ) || echo "  none"
} > sandbox/MANIFEST.txt

# Record the run in the repo backup log (reaches main with the next commit).
BACKUP_SHA="pending"
log_entry() {
  printf '| %s | %s | %s | %s | %s |\n' "$STAMP" "$TRIGGER" "$BRANCH" "$BACKUP_SHA" "scripts+harness+manifest"
}

git add -A
if git diff --cached --quiet; then
  echo "BACKUP NO-OP: nothing changed on $BRANCH ($STAMP)"
  exit 0
fi
BACKUP_SHA=$(git rev-parse --short HEAD 2>/dev/null || echo new)
if [ -f "$REPO_DIR/harness/BACKUPS.md" ]; then
  log_entry >> "$REPO_DIR/harness/BACKUPS.md"
fi
if [ -f "$WORK/sandbox/harness/BACKUPS.md" ]; then
  log_entry >> "$WORK/sandbox/harness/BACKUPS.md"
fi
git add -A
git -c user.name="vdm-harness" -c user.email="harness@velocitydm.invalid" \
  commit -q -m "sandbox backup $STAMP"
git "${AUTH[@]}" push -q origin "$BRANCH"
echo "BACKUP OK $BRANCH $(git rev-parse --short HEAD) ($STAMP)"
