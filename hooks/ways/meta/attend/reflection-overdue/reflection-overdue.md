---
description: Prompt reflection when context is high and no recent ledger capture
trigger:
  type: attend
  signals:
    - reflection-overdue
scope: agent
refire: 0.15
---

Context usage is high and no reflection has been captured recently. Compaction is close, so record what matters now:

1. **What decisions were made** and why — the reasoning behind choices
2. **What didn't work** and what you learned from it
3. **Current state** — what's in progress, what's blocked, what's next
4. **Put on the TaskList** anything the next session needs to know

Keep the reflection brief and honest. A few sentences about what surprised you are more useful than a full status report.
