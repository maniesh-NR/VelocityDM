# VelocityDM Agent Harness

This directory is the **agent harness**: persistent memory, execution
protocol, adversarial consensus gates, and the sandbox-backup workflow that
agents (human or AI) must follow when working on VelocityDM.

> If you are an agent starting a session: read `MEMORY.md`, `ROADMAP.md`,
> `PROTOCOL.md` and `WORKFLOW.md` — in that order — before touching
> anything. End every execution plan with a backup (see `WORKFLOW.md`).

## What lives here

| File | Purpose |
| --- | --- |
| `MEMORY.md` | Durable project memory: environment, state, learnings, commands |
| `ROADMAP.md` | Staged roadmap; each stage transition requires a consensus gate |
| `PROTOCOL.md` | Adversarial consensus protocol (roles, verdicts, gate rules) |
| `WORKFLOW.md` | Execution-plan lifecycle; **backup is always the last step** |
| `BACKUPS.md` | Append-only backup log |
| `ASSESSMENTS/` | One record per stage gate: the panel verdicts (three-role or five-role panel), findings, outcome |

## The three rules

1. **Consensus before stages.** No stage of the roadmap proceeds without a
   comprehensive adversarial assessment and independent consensus
   (`PROTOCOL.md`).
2. **Backup last, always.** Every execution plan ends with a sandbox backup
   to GitHub — event-driven, never a fixed time interval (`WORKFLOW.md`).
3. **Memory before code.** Session bootstraps from `MEMORY.md`; session
   outcomes are written back to it.

## Related infrastructure

- `scripts/backup-sandbox.sh` (repo copy) — pushes the agent sandbox state
  to the `sandbox-backup` branch of this repository.
- `.github/workflows/sandbox-backup.yml` — on-demand (manual dispatch)
  repository snapshot; intentionally **not** scheduled, per rule 2.
