---
description: Guide reflection and ledger capture as context approaches compaction
trigger:
  type: attend
  signals:
    - context-pressure
scope: agent
refire: 0.15
---

Context is approaching compaction. Before the window closes:

1. **Capture what matters** — decisions made, approaches tried, what worked and why
2. **Update tasks** — mark completed work, note blockers for the next session
3. **Save to memory** — if anything learned this session is useful across conversations
4. **Commit work** — ensure changes are committed so nothing is lost through compaction

Focus on the *why* behind decisions, not the *what* of code changes (git has that).
If context is critically low, prioritize commit over reflection.
