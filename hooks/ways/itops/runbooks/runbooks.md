---
description: Runbook automation, executable playbooks, SOPs as code, operational procedures
vocabulary: runbook playbook sop procedure operational automation executable standard checklist step-by-step operations
pattern: runbook|runbook.?(automation|executable)|playbook|sop.?(automation|as.?code)|operational.?procedure
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Runbooks Way

## What Claude Should Do

When handling operational tasks, check for existing runbooks before writing ad-hoc scripts.

## Creating a Runbook

A runbook documents a repeatable operational procedure:

```
runbooks/
└── procedure-name/
    ├── runbook.sh (or .py, .ts)   # The procedure
    └── README.md                   # When/how to use
```

### The Procedure

- Include error handling for every external call (API, SSH, database)
- Ensure idempotent execution — safe to run twice
- Log each step's outcome for post-incident review
- Exit with clear success/failure status and message

### The README

- **When to use**: trigger conditions, symptoms
- **Prerequisites**: required access, credentials, tools
- **Expected outcome**: what success looks like
- **Escalation**: when this runbook isn't enough

## Runbook Quality Standards

- Test in a non-production environment before relying on it
- Include a rollback or "undo" section where applicable
- Keep steps atomic — each step succeeds completely or fails cleanly
- Don't mix diagnosis with remediation — separate "find the problem" from "fix it"

## When a Runbook Doesn't Exist

If handling a novel operational issue:
1. Resolve the incident first
2. Then offer to create a runbook from the steps taken
