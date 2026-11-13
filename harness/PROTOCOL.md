# Adversarial Consensus Protocol

Every stage of the roadmap (see `ROADMAP.md`) must pass a comprehensive
assessment by adversarial agents **before** work proceeds to the next
stage. This document defines the roles, the process, and what "consensus"
means.

## Roles

Three independent roles are assigned to three separate agents. Agents do
not communicate with each other, do not see each other's verdicts, and
receive identical inputs (the stage brief + repository access).

### RED — the Adversary

Goal: **stop bad work from shipping.** Attacks the stage from every angle:

- Correctness: logic errors, API misuse, broken invariants.
- Build/CI risk: will the required checks actually pass? (fmt, clippy
  `-D warnings`, tests, cross-compilation, dependency availability).
- Release risk: asset naming, tag wiring, workflow gating conditions.
- Security: secrets, injection, unsafe behavior, supply chain.
- Honesty: docs/README claims that the code does not deliver.

RED must produce concrete findings — each with a file path (and line if
possible) and a suggested fix. Vague unease is not a finding.

### BLUE — the Advocate

Goal: **verify the work delivers what was asked.** Checks requirements
coverage point by point, confirms strengths with evidence, and — crucially —
may concede weaknesses. BLUE is not allowed to wave away a RED-style
concern it independently notices; it must report it.

### AUDITOR — the Neutral Fact-Checker

Goal: **verify claims against repository reality.** Independently checks
the mechanical facts: files exist, names match exactly (CI job names,
required-check names, asset names), tags point at the right commits,
counts are right, tests actually pass. AUDITOR verdicts must cite the
command or file used for each fact.

## Verdicts

Each agent returns exactly one verdict:

| Verdict | Meaning |
| --- | --- |
| `PROCEED` | No blocking findings. |
| `PROCEED-WITH-CONDITIONS` | Non-blocking findings that must be recorded and addressed. |
| `BLOCK` | At least one finding makes proceeding wrong or dangerous. |

## Consensus

- **Consensus = zero `BLOCK` verdicts** across all three agents.
- `PROCEED-WITH-CONDITIONS` findings are recorded in the assessment record
  and must be addressed (or explicitly deferred with justification)
  before the *next* gate.
- Unanimity is not required; minority opinions are recorded verbatim.
- If consensus fails: fix the blocking findings, then re-run the **whole**
  panel (all three roles, fresh).

## Records

Each gate produces `ASSESSMENTS/NNNN-<stage>.md` containing: date, stage,
the three verdicts with findings, the consensus outcome, and what was done
about the conditions. The gate is only closed when the record exists.
