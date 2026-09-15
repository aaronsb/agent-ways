---
description: readiness of the area about to change, whether it can be built, verified, delivered, and recovered today, and holding new skills, agent definitions, prompts, and abstractions until that foundation exists
vocabulary: groundwork readiness foundation prerequisite premature primitive onboarding packet inherited neglected legacy untested no tests no ci no deploy path clean checkout repair baseline before we change it can we build this can we deploy this deliverable recoverable rollback where do we start
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: premise -->
# Groundwork

A skill, agent definition, prompt, or new abstraction is a description of how work gets done here. It becomes a **premature primitive** when it describes a process that cannot run yet: no command that proves the behaviour, no repeatable deploy, no way back. It looks like progress in the repository. Nothing the customer receives has changed. We wrote the onboarding packet before we fixed the building.

The order we keep is: make the area deliverable, then change it, then write the instructions that describe what now works.

## The four conditions

Before we change product code in an area, or accept someone else's change into it, we can answer each row for that area. The rest of the codebase can wait.

| Condition | We can say |
|---|---|
| **Build** | One command produces the artifact from a clean checkout |
| **Verify** | Automated checks exercise the existing behaviour we are about to touch, and they run in CI |
| **Deliver** | One repeatable path moves the artifact to a test environment and to production |
| **Recover** | Rollback has run at least once, on purpose |

Where a row is missing, filling it is the first piece of work, and it is a finding to report. Another agent or a longer prompt built around the gap hides it. A row that depends on a person remembering last month is missing. A row that holds on the authoring host and nowhere else is missing too (see `environment/hostparity`).

## Why this comes first

Writing the change is the short part of getting it to a customer. Every primitive that changes how we write code lands on that short part. None of it reaches the review, the environment request, the approval, or the release window where the change waits. `groundwork/stall` measures the split for one real change.

Once the area meets the four conditions, capture the working commands, gates, and deploy path in the project instructions. Now the skill describes something we can do.

## Children

| Concern | Way |
|---|---|
| A contribution's account of itself versus what it demonstrates | `groundwork/claims` |
| Ordering a queue of open changes against readiness | `groundwork/sequence` |
| Work clustering on what people are permitted to edit | `groundwork/permission` |
| Following one change to find where it waits | `groundwork/stall` |

## See Also

- code/testing/gates(softwaredev) — each gate reported as executed, discovered, or absent; an inherited codebase gets every gate on its own absent line
- environment/hostparity(softwaredev) — a green result on the authoring host is evidence about that host only
- freshness/groundtruth(softwaredev) — capturing the baseline before a refactor
- environment/recovery(softwaredev) — right-sizing an increment when gates go red
- delivery/implement(softwaredev) — the failing test and rollback path each task carries
- knowledge/authoring(meta) — writing the instructions once the process exists
