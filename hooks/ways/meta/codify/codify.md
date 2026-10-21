---
description: Writing down a standing rule or a method that finally worked so later sessions follow it without being told again. The user says from now on always do X, or a deploy or process finally worked and the exact steps should be written down for next time. Put it in the narrowest durable home, apply it now, and list the failed attempts.
vocabulary: from now on moving forward always never codify standing rule preference write down what worked exact steps next time finally worked recipe runbook playbook learnings failed attempts dead end skill update registry never repeat again
pattern: (moving|going) forward,? (please )?(always|never|don'?t|make sure|let'?s always)|from now on|\bcodify\b|learnings\.md|document how (we|you) did (this|that|it)|write (this |it )?up (as|in) a (learnings|runbook|playbook)|so (that )?i don'?t (need|have) to (repeat|remember)
refire: 0.15
scope: agent
---
<!-- epistemic: convention -->
# Codify What Worked

Every session starts cold. Write down what worked so the user never has to say it twice.

## Standing preferences

When the user says "from now on", "moving forward, always...", "I always ignore X", or "I hate X":

1. Write the rule into the narrowest durable home now.
2. Apply it to this turn's work. Regenerate any output that broke it.
3. If a script can check the rule, add a test or a lint check.
4. Confirm in one line, for example "Saved to the project instructions so later sessions follow it."

## Pick the home before saving

| Content | Home |
|---|---|
| Repo facts and environment details | The project instructions file, or a repo artifact |
| Procedures | A skill, runbook, or playbook |
| Behavior that should fire on a trigger | A way |
| Cross-project user facts and standing ignore lists | Memory |
| Status narrative and stakeholder changes | A log file, never memory or the instructions file |

Use the routing table in memory(meta) before any memory write. Keep existing rules when you update a file, and fix stale notes when you see them. Strip private details from steps that go into public docs.

## When a process finally works

- Record the method, not just the result. Include the commands, auth and re-auth steps, payload shapes, account choices, gotchas, and the path that worked.
- Update a skill only after the new method has worked end to end.
- Date verified recipes and known failure signatures ("seen 2026-01-15"). Mark a recipe verified only after real proof.
- List failed approaches with dates so nobody retries them.
- Commit the write-up with the work. Open issues for any tech debt it surfaced.
- When an experiment fails, reset to the last good commit if the user asks. Then write a learnings file that walks through what was tried and why each attempt was dropped.

## Registries

When the session produces a new canonical fact, add it to the registry table in the skill or instructions file in the same session. Examples are customer-to-workspace mappings, verified API quirks, and the names of keys created this session. Record key names and locations, never key values.

## Encode repeated wins

After a task has gone well by hand about three times, offer to encode it as a project subagent or skill with a short, memorable name.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I'll remember it for the rest of the session" | The next session starts cold. Write it down now. |
| "The skill is close enough" | Update it only once the new method works, then update it the same day. |
| "Nobody will retry the dead end" | A later session will. Date it and list it. |

## See Also

- memory(meta) — the surprise test and what belongs in memory
- skills(meta) — choosing a skill as the home for a procedure
- knowledge/authoring(meta) — choosing a way as the home for behavior
