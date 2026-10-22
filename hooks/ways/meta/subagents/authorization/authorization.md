---
description: Whether a spawn has been asked for, given the system prompt's rule that the Agent tool, workflows and deep-research need the user, a CLAUDE.md file or a skill to ask for them; the three sources, why a way's own text is not one, the suggested CLAUDE.md line, and the independence spawn
vocabulary: authorization permission allowed asked for unless the user asks spawn agent tool restriction claude.md line skill in use independence spawn cold read held-out refuter workflow deep-research propose one line width six agents solo
pattern: \b(permission|allowed|authori[sz]ation|authori[sz]ed) (to|for) (spawn|delegat|use the agent)|unless the user.{0,40}asks? for it|restrict.{0,30}agent tool|asked for (a|the) (spawn|delegation)
refire: 0.15
scope: agent
---
<!-- epistemic: convention -->
# Asking to Spawn

Claude Code 2.1.306 puts one sentence in the system prompt: *"Do not use the Agent tool, workflows, or deep-research unless the user, a CLAUDE.md file, or a skill asks for it"*. By default it reaches claude-opus-5 only, and a server can serve it to other models. A server can also still serve the older sentence, which names only the user. Where no such text is present, the payoff section of the parent way is the check, and the rules below do not apply.

## Three sources ask for a spawn

- The user, in the conversation. Asking to land a branch, review a PR or sweep a topic asks for the delegation that task specifies.
- A skill in use. `/start`, `/develop` and `/merge` carry a clause at their own dispatch sites naming the delegation their stages call for. A built-in command grants nothing.
- A CLAUDE.md file, user-level or project-level.

When one of them covers the spawn, spawn it. Do not ask again and do not describe the gate.

A way's own text is not a request. It names the spawn, the width and the payoff. When none of the three sources covers the step and your instructions carry the restriction, say in one line what you would spawn and how wide, then wait or do the work inline.

The operator can cover every such step with one line in their CLAUDE.md. The install never writes it. Suggested:

> Delegation that an agent-ways skill or way directs at its own step counts as asked for. Spawn what the step names, at the width it states, and ask past about six agents.

## Independence spawns

An independence spawn is one whose payoff is a reader, writer or judge who is free of the author's knowledge. Examples are a cold read of documentation, held-out cases, a refutation pass, a recomputation of key figures, a blind judge and a fresh-context review of your own change. The author cannot do these inline.

When the user asks for a check of that kind, the request asks for the independent party. Spawn it and name the payoff in a clause. When a way for such a check discloses during other work, such as an edit to a README, nobody asked for the check, so propose it. Where an instrument that spawns nothing can do the job, such as a stop-hook script, lead with that.

Every other spawn is an efficiency spawn: fan-out reads, log reading, parallel edits, an artifact review. It has an inline substitute and, where the restriction applies, needs one of the three sources.

## Width and when to ask

The sources cover whether to delegate. You set the width, say the number before spawning, and ask past roughly half a dozen.

Still ask first when the delegation is not part of an invoked procedure, when it spends significant tokens outside the stated task, or when the operator has said to work solo.

## Workflows and deep-research

The sentence names them with the same three sources. Invoke them only when the user, or a CLAUDE.md line that names them, asks; otherwise propose and discuss. The suggested line above does not cover them. See ADR-175 and ADR-608.

## See Also

- subagents(meta) — parent: how to brief, what comes back, and the payoff clause
- trust/delegation(meta) — the same sources decide commits, pushes and issue writes (ADR-609)
