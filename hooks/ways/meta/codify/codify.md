---
description: The user sets a standing rule (from now on, always do this), or corrects the same preference of theirs again, or a process that took several tries finally worked. Write the rule or the working method into the home that will enforce it, apply it now, and record the attempts that failed.
vocabulary: from now on going forward standing rule preference I keep telling you keep correcting you stop making me say it write it down make it stick steps that worked recipe failed attempts dead ends encode as a skill hook lint check
pattern: \b(told|asked) you (this |that )?(\w+ )?(times|before)\b|\bstop making me (say|repeat)\b|\bfrom now on\b
refire: 0.15
scope: agent
---
<!-- epistemic: convention -->
# Codify What Worked

Every session starts cold. A rule the user states once in chat is gone next session, and the user has to state it again.

## A standing rule or a repeated correction

When the user says "from now on", or corrects the same thing a second time:

1. Pick the home by the routing table in memory(meta). A rule a script can check becomes a hook, lint rule or test. A rule that should fire on a kind of work becomes a way. A procedure becomes a skill. A cross-project preference is memory.
2. Write it there now, keeping the rules already in that file.
3. Apply it to this turn's work, and redo any output from this session that broke it.
4. Say in one line where it went.

## A method that finally worked

- Record the method, with the commands, the order, the account or environment it needs and the gotchas, in the skill or runbook that owns the procedure. Do this only after it has worked end to end.
- List the approaches that failed, with the date and the reason, so a later session does not retry them.
- Commit the write-up with the work. Open an issue for any debt it surfaced.
- Keep secrets out: record a key's name and where it lives, never its value.

When the same task has been done by hand about three times, offer to encode it as a skill.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I'll remember it for the rest of the session" | The next session starts cold. Write it down now. |
| "Memory is the quickest place for it" | Memory is for cross-project user facts. A checkable rule enforces itself; memory only reminds. |
| "Nobody will retry the dead end" | A later session will. Date it and list it. |

## See Also

- memory(meta) — the routing table and what belongs in memory
- skills(meta) — a skill as the home for a procedure
- knowledge/authoring(meta) — a way as the home for behaviour that fires on a kind of work
