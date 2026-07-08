---
description: Recognize an end-of-session signal and route to the /wrap skill — square the TaskList honestly, write a continuation prompt, and hand off to a gauge-aware directed compaction.
vocabulary: wrap session end checkpoint handoff continuation compact close out done for the day pause stopping point sign off end of session wind down
pattern: wrap.?(up|this up|it up|things up)|wrapping up|let'?s wrap|end of (the )?session|wrap.{0,10}(session|for the day|for today)|checkpoint.{0,15}compact|/wrap
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Wrapping Up

The user is signaling the session is ending — they want to close out, not keep building.
This is exactly what the **`wrap` skill** is for. Pull it up (`Skill: wrap`) rather than
improvising a summary; the skill enforces the parts that are easy to skip when you eyeball it.

## Why the skill, not a freehand summary

Three things go wrong when an end-of-session wrap is done by feel:

- **The TaskList lies.** Finished work sits marked pending; abandoned ideas linger as
  tasks; real remaining work has no entry. Post-reset-you inherits that list with zero
  memory of this conversation. The skill makes squaring it honestly the load-bearing step —
  close done, retire stale, write what's real with enough detail to resume cold.
- **The handoff is too thin to resume from.** The skill produces a dense, copy-paste
  continuation prompt that survives even a hard reset (`/clear` + paste), not just compaction.
- **Compaction is mis-timed.** Wrapping at 30% is not wrapping at 90%. The skill reads the
  context gauge *first* (`ways context`) and lets early-vs-late shape how heavy the handoff
  needs to be — and whether `/compact` is even worth it (early, it reclaims little and ends
  the live thread).

## The compaction caveat

Neither a way nor a skill can trigger `/compact` — Claude Code forbids invoking `/` commands
programmatically. So `/wrap` *prepares* everything and hands the user a ready-to-run
`/compact <focus>` line. Don't claim the session was compacted; you set it up, the user pulls
the trigger.

## Relation to the automatic sibling

The `compaction-checkpoint` way fires **on its own** as context nears the limit. This way
fires when the **user says so** — an early, deliberate close. Same destination, different
trigger: one is the smoke alarm, the other is choosing to leave.

## See Also

- wrap (skill) — the procedure this routes to.
- compaction-checkpoint(meta) — the automatic, threshold-triggered sibling.
- todos(meta) — the TaskList-at-compaction discipline the skill enforces on demand.
- start(meta) — the opening bookend; same gauge (`ways context`), opposite pole.
- merge(softwaredev) — landing an increment is iterative (many per session); wrapping is terminal (once).
