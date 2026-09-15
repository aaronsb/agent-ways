---
description: readiness of the area about to change — whether it can be built, verified, delivered, and recovered today — and holding new skills, agent definitions, prompts, and abstractions until that foundation exists
vocabulary: groundwork readiness foundation prerequisite premature primitive onboarding packet inherited neglected repair baseline before we change it can we deploy this deliverable recoverable
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: premise -->
# Groundwork

A skill, agent definition, prompt, or new abstraction is a description of how work gets done here. It becomes a **premature primitive** when it describes a process that cannot run yet: no command that proves the behaviour, no repeatable deploy, no way back. It looks like progress in the repository. Nothing the customer receives has changed. We wrote the onboarding packet before we fixed the building.

The order we keep is: make the area deliverable, then change it, then write the instructions that describe what now works.

## The four conditions

Before we change product code in an area, or accept someone else's change into it, we can answer each row for *that area* — not the whole codebase:

| Condition | We can say |
|---|---|
| **Build** | One command produces the artifact from a clean checkout |
| **Verify** | Automated checks exercise the existing behaviour we are about to touch, and they run in CI |
| **Deliver** | One repeatable path moves the artifact to a test environment and to production |
| **Recover** | We have demonstrated rollback, not described it |

Where a row is missing, that is the first piece of work, and it is a finding to report — not a gap to build around with another agent or a longer prompt. A row that depends on a person remembering last month is missing.

## Why this comes first

Recovered coding time is real but small. If coding is two days and everything after it is eight, halving coding saves one day of ten. The eight are where the return lives, and no primitive that changes how we code reaches them.

Once the area meets the four conditions, capture the working commands, gates, and deploy path in the project instructions. Now the skill describes something we can actually do.

## Children

| Concern | Way |
|---|---|
| A contribution's account of itself versus what it demonstrates | `groundwork/claims` |
| Ordering a queue of open changes against readiness | `groundwork/sequence` |
| Work clustering on what people are permitted to edit | `groundwork/permission` |
| Following one change to find where it waits | `groundwork/stall` |

## See Also

- code/testing/gates(softwaredev) — each gate reported as executed, discovered, or absent
- freshness/groundtruth(softwaredev) — capturing the baseline before a refactor
- environment/recovery(softwaredev) — right-sizing an increment when gates go red
- delivery/implement(softwaredev) — the failing test and rollback path each task carries
- knowledge/authoring(meta) — writing the instructions once the process exists
