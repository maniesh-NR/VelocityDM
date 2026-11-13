# Execution-Plan Workflow (agents)

The lifecycle every execution plan must follow. This is the contract
between the agent and the repository.

```
1. PLAN        Read MEMORY.md + ROADMAP.md; write a TODO plan.
2. EXECUTE     Do the work; keep the tree committable.
3. ASSESS      Adversarial panel examines the work (PROTOCOL.md).
4. GATE        Independent consensus required before the next stage.
5. COMMIT      Commit; push main (+ tags when releasing).
6. BACKUP      ALWAYS THE LAST STEP: scripts/backup-sandbox.sh
```

## The backup rule

**Backups are event-driven, not time-driven.**

- Every execution plan ends with a backup.
- Every output to a prompt ends with a backup whenever anything changed —
  new commits, new harness memory, new sandbox state.
- Never schedule backups at a fixed time interval (no cron). The event is
  "work happened", not "the clock ticked".
- If nothing changed, the backup is a no-op and the response simply says so.

### What gets backed up

| Included | Excluded (always) |
| --- | --- |
| `scripts/` (sandbox tooling) | secrets — tokens, credentials (`.gh_token`) |
| `release-notes/` | release binaries (reproducible from CI) |
| harness memory state | the repository itself (already pushed) |
| `MANIFEST.txt` (repo HEAD, tags, asset checksums) | |

### Mechanics

`scripts/backup-sandbox.sh` snapshots the sandbox into the
`sandbox-backup` branch of `maniesh-NR/VelocityDM`:

```sh
bash scripts/backup-sandbox.sh      # prints: BACKUP OK <branch> <sha>
```

The script authenticates with a token file (never committed, never echoed),
appends a commit, and pushes. `harness/BACKUPS.md` records each run.

## Updating memory

At the end of a plan (after the gate, before the backup):

- Append a dated entry to `MEMORY.md` (what changed, what was learned).
- Update `ROADMAP.md` stage statuses.
- Record gate outcomes in `ASSESSMENTS/` and `BACKUPS.md`.
