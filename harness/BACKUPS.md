# Backup Log

Append one line per backup. Rule: backups are event-driven (end of every
execution plan / every output that changed something), never fixed-interval
(see `WORKFLOW.md`).

| UTC timestamp | Trigger | Branch | Commit | Notes |
| --- | --- | --- | --- | --- |
| 2026-11-13T10:42:52Z | S1 close: harness + Material You extensions commit | sandbox-backup | new | scripts+harness+manifest |
