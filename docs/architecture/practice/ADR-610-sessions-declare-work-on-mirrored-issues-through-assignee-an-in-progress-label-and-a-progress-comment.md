---
contract: adr/v1
kind: decision
verb: change
capability: loop
extends: [ADR-180]
amends: ['ADR-180#Field ownership, not merge']
basis:
  - operator: aaronsb
    level: guided
    said: "The thing I want to make sure we do is when a subagent takes on work on an issue, they at a minimum update the status of the issue, and hopefully, 'assign it to themselves' (which, honestly usually means the user who is accessing github). In true multi user repos and projects, this would be valuable. in a single user repo, everything would be assigned to the same user regardless of subagents etc but thats ok too. if a subagent is done working but not done with the issue (session wrap, other circumstances, anything really) at a minimum if the issue was assigned to the same user the subagent is using to work the issue, then they can pick it up and continue"
    via: Claude Code session, 2026-10-23
  - operator: aaronsb
    level: guided
    said: "the wrap skill is typically invoked at the end of all the context for the top level agent (you, for example) and any subagent that reaches context end and has to transfer to the next likely takes up the mantle of the work again. the actual state of an issue in github is actually more of an outward projection of state than an inward need. claude (and subagents and workflow swarms) generally are fine collaborating. But externally projecting assignee and state (in progress etc) is helpful to others. This is actually also helpful when rebuilding state at a later time (for instance the operator stops on a friday and comes back on a monday morning)"
    via: Claude Code session 2026-10-23, detail behind the 'No, needs a change' answer to the paused probe
  - operator: aaronsb
    level: guided
    said: "it's not even really a prompt it's more of a 'I got this work' so the behavior is simply declaring working on it. there's no interlocking or blocking or waiting for a prompt response. it's just a convention, the same as taking your shoes off when coming into the house and putting them on when you leave"
    via: Claude Code session 2026-10-23, relayed verbatim by the lead after the collision design was put to the operator
  - precedent: ADR-180
  - precedent: ADR-609
  - precedent: ADR-705
  - operator: aaronsb
    level: guided
    said: "something the hook/macro does is let the agent know (if they are working on an issue) its assignment state - unassigned, or the user it's currenty assigned to. knowing that should be enough context to give the agent a chance to reason about working on it, or asking the dispatching agent (if a sub agent) or the operator (if the operator said to work on it) that someone else was assigned the work"
    via: Claude Code session 2026-10-23, sent while the revised draft was being committed
  - standard: 'Claude Code hooks reference, https://code.claude.com/docs/en/hooks, read 2026-10-23: PreToolUse input carries tool_name, tool_input and tool_use_id, and agent_id with agent_type inside a subagent; PreToolUse additionalContext is "String added to Claude''s context alongside the tool result"; PreToolUse updatedInput "Replaces the entire input object" and is documented in combination with "allow" or "ask", with nothing said about the Agent tool or about a hook that returns no decision; SubagentStart additionalContext is "String added to the subagent''s context at the start of its conversation, before its first prompt", and SubagentStart input carries only agent_id and agent_type beyond the common fields; SessionEnd fires on clear, resume, logout, prompt_input_exit and other, and the reference names no behaviour on a crash or kill'
  - evidence: 'Code read on 2026-10-23: tools/ways-cli/src/cmd/hook/subagent.rs claims the oldest stash at SubagentStart, so the ways injection ties a spawn to its subagent first in, first out'
  - evidence: 'Code read on 2026-10-23: hooks/ways/issues-pull.sh exits when agent_id is set, so the bridge hooks act for the head agent; gh-tasks pull already stores assignees and labels in task metadata and diffs labels in the whisper'
  - evidence: 'Issue #687: mirrored tasks are owned by team-lead so an idle teammate is not offered them; handing one to a teammate means setting its owner'
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "One config switch"
    via: Claude Code session 2026-10-23, choice tool answer to the write-switch probe
    covers: [switch]
  - operator: aaronsb
    said: "No, needs a change"
    via: Claude Code session 2026-10-23, choice tool answer to the paused probe; detail not yet given
    covers: [paused]
  - operator: aaronsb
    said: "Label only (Recommended)"
    via: "Claude Code session 2026-10-23, choice tool answer: when the session wraps itself (keepwarm idle wrap) rather than the operator typing /wrap, pause the issue without posting the handoff comment"
  - operator: aaronsb
    said: "the wrap skill is typically invoked at the end of all the context for the top level agent (you, for example) and any subagent that reaches context end and has to transfer to the next likely takes up the mantle of the work again. the actual state of an issue in github is actually more of an outward projection of state than an inward need. claude (and subagents and workflow swarms) generally are fine collaborating. But externally projecting assignee and state (in progress etc) is helpful to others. This is actually also helpful when rebuilding state at a later time (for instance the operator stops on a friday and comes back on a monday morning)"
    via: Claude Code session 2026-10-23, detail behind the 'No, needs a change' answer to the paused probe
    covers: [paused]
  - operator: aaronsb
    said: "it's not even really a prompt it's more of a 'I got this work' so the behavior is simply declaring working on it. there's no interlocking or blocking or waiting for a prompt response. it's just a convention, the same as taking your shoes off when coming into the house and putting them on when you leave"
    via: Claude Code session 2026-10-23, relayed verbatim by the lead after the collision design was put to the operator
  - operator: aaronsb
    said: "something the hook/macro does is let the agent know (if they are working on an issue) its assignment state - unassigned, or the user it's currenty assigned to. knowing that should be enough context to give the agent a chance to reason about working on it, or asking the dispatching agent (if a sub agent) or the operator (if the operator said to work on it) that someone else was assigned the work"
    via: Claude Code session 2026-10-23, sent while the revised draft was being committed
  - operator: aaronsb
    said: "like 'you are x, issue is assigned to y'"
    via: Claude Code session 2026-10-23, the shape of the assignment-state line in entry 6
  - operator: aaronsb
    said: "not necessairly that exactly but thats the gist"
    via: Claude Code session 2026-10-23, relayed verbatim by the lead as a correction to the state-line shape in the previous entry
status: proposed
date: 2026-10-23
deciders:
  - aaronsb
related:
  - ADR-180
  - ADR-609
  - ADR-705
  - ADR-608
  - ADR-172
observable:
  - see: on an open, unassigned `tasklist` issue, an Agent spawn of an editing type whose description starts with `[gh#N]` leaves the issue assigned to the session's login and labelled `in-progress` before the agent starts
    run: gh issue view N --json assignees,labels
  - see: a second `[gh#N]` spawn in the same session makes no GitHub call
  - see: the same spawn with `subagent_type` Explore, or a brief that names the issue without the prefix, writes nothing to the issue
  - see: on an open, unassigned `tasklist` issue, the subagent's context opens with a state line giving the session's login, that the issue was unassigned, the label and that the work was declared
  - see: on an issue assigned to another login, the spawn proceeds with no prompt, nothing is written to the issue, and the subagent's context opens with a state line giving the session's login, the other assignee, the label and its age, and that nothing was declared
    run: gh issue view N --json assignees,labels
  - see: in a session with a `gh-N` task `in_progress` and no declaration, or a branch named for the issue with nothing declared, the next prompt's whisper carries one line saying the issue is worked with no declaration and naming `gh-tasks claim N`, and nothing is written to the issue
  - see: with gh unauthenticated or the network down, the spawn proceeds and the next whisper carries one line saying the claim was not written
  - see: after /wrap, the assignee and the label stay, one comment carries the marker line and the fixed fields, and a new session's /start lists the issue as in progress with its next-step line
    run: gh issue list --label tasklist --label in-progress --assignee @me --state open
  - see: "`gh-tasks release N --park` removes the label and keeps the assignee, and /start lists the issue under parked"
  - see: "the pull after a claim whispers no `relabeled #N` for the session's own write"
  - see: with the config switch off, no assign, label or comment is written, and the whisper offers the claim once per issue
---

# ADR-610: Sessions declare work on mirrored issues through assignee, an in-progress label and a progress comment

## Summary

- **Decided:** Working a `tasklist` issue follows a convention the bridge carries out at the door. On the way in, a session declares the work: the issue gets the session's GitHub login as an assignee and an `in-progress` label. While the work continues, each `/wrap` posts a progress comment. On the way out, the label comes off when the issue closes, or when the operator parks the issue (assignee kept) or drops it (assignee removed). Work starts through a subagent spawn whose description carries `[gh#N]`, a task update on the mirrored task, or a manual verb, and the declaration is written once per issue per session. Nothing blocks, prompts or waits. The agent doing the work is told the issue's assignment state in one line: its own login, the assignees or "unassigned", and the label. When another login already holds the issue, nothing is written, and the agent decides whether to go on or to raise it with whoever dispatched it. Work that starts with no trigger is reported by a whisper that never declares. The GitHub state is a projection for other people and for rebuilding state later. Agents of one login coordinate inside the session. `/wrap` is a relay to the next context and keeps the assignee and the label, and it posts the comment whoever triggered the wrap. The hooks write only while the operator's config switch is on.
- **Trades away:** Nothing stops an agent working an issue someone else holds; the state line only gives it the chance to stop and ask. A crashed or abandoned session leaves `in-progress` on its issues until someone parks, drops or closes them, so other people can read work as live that is not. Every wrap with claimed issues posts a comment that notifies subscribers, and the comment is public on a public repository. The first trigger per issue per session costs up to about a second.
- **One-way?** No. It is reversible: two hooks and two verbs, which revert in one session, and labels and assignees can be removed. Comments already posted stay public, and their notifications have gone out.
- **Probes:** *Confident (monday):* When you come back after a break, `/start` lists the issues still assigned to you and marked in progress, each with the next step from its latest progress comment, and lists parked issues separately. Is that the list you want to start from? *Not confident (unattended wrap):* When a session wraps on its own near the end of its context, with no one typing `/wrap`, it still posts the progress comment on each issue it holds, and everyone watching those issues is notified. Is that acceptable, or should a wrap you did not start leave no comment?
- **Inversion:** At one end, the GitHub state is a lock: a claim blocks or prompts anyone else who starts the issue, and agents check it before each step. At the other end, GitHub shows nothing and the state of the work lives only in sessions. This decision sits between them: a declaration on the way in, a progress note at each relay and a clearing on the way out, read by people and later sessions, which gates nothing.

## Context

ADR-180 mirrors `tasklist` issues into the session task list. Its push increment, still open in #457, covers closing an issue on Stop and an in-progress label from the session's `in_progress` status. It has no rule for assignees and none for subagents. Most work on an issue in this project runs in subagents, the bridge's hooks act for the head agent only, and subagents keep their comms tools read-only (collaboration/teams/handback). Nothing on GitHub shows who has taken up an issue or where the work stands. The operator asked that a subagent taking on an issue update its status and assign it. They described the GitHub state as an outward projection, for other people and for rebuilding state after a break, and the claim as a declaration with no interlocking. They also asked that the agent working an issue be told its assignment state, so it can reason about going on or raise it with whoever dispatched it.

These facts shape the design:

- **PreToolUse sees every claiming call.** Its input carries the tool name and input, and `agent_id` with `agent_type` inside a subagent, so one hook sees a spawn, a task update and the agent that made either (hooks reference).
- **Context reaches a subagent at its start.** SubagentStart `additionalContext` is added to the subagent's context before its first prompt. SubagentStart carries only `agent_id` and `agent_type`, so a hook ties it to a spawn by order and type, as the ways injection does first in, first out (hooks reference; `subagent.rs`). PreToolUse `additionalContext` reaches the agent that made the call, beside the tool result.
- **Rewriting a spawn's prompt is not documented for the Agent tool.** PreToolUse `updatedInput` replaces the whole tool input and is documented with an `allow` or `ask` decision. The reference says nothing about it on the Agent tool, or about a hook that returns no decision (hooks reference).
- **SessionEnd fires on `/clear` and `/resume`.** Its reasons are `clear`, `resume`, `logout`, `prompt_input_exit` and `other`, and the reference names no behaviour on a crash or a kill (hooks reference).
- **An owner keeps a mirrored task out of the teammate pool.** Mirrored tasks are owned by `team-lead`, and work is handed to a teammate by setting the owner (#687). That handoff goes through `TaskUpdate` and involves no Agent spawn.
- **Outward writes need a source of authority.** Assigning, labelling and commenting are outward writes under ADR-609. The operator chose one config switch as that authority for the bridge.

An earlier draft of this record treated a wrap as a pause that removed the label. Under that frame the operator chose to post no comment when a session wraps itself ("Label only"). Under the frame decided here, a wrap keeps the label and posts the comment whoever triggered it, so that choice goes back to the operator as the unattended-wrap probe.

## Decision

1. **A convention, carried out on entry and exit.** A session declares the work when it starts an issue, posts a progress comment at each wrap, and the declaration is cleared when the issue closes or is released by hand. The assignee, the label and the latest progress comment tell other people, and a session rebuilding state later, who has taken an issue up and what comes next. There is no interlocking: no claim blocks, denies, prompts or waits on anyone, and the spawn or task update that triggered it always proceeds. Agents of one login coordinate inside the session through the task list, their briefs and the lead.

2. **The states.** For an open issue carrying `tasklist`, an assignee has taken it up. The `in-progress` label means it is being worked, across context relays, `/clear`, `/resume` and restarts. A closed issue is done (ADR-180). An issue that is assigned and has no label is parked: the assignee keeps it and is not working it. This uses a label because ADR-180 rejected Projects. The bridge creates the label on a repository at the first declaration that needs it, and `GH_TASKS_LIVE_LABEL` names it, defaulting to `in-progress`.

3. **What declares.** Three triggers declare work. Each runs on PreToolUse and fires whichever agent made the call:
   - An `Agent` call declares when its `description` starts with `[gh#N]`, #N is mirrored, and the agent type can edit files: its definition grants Edit, Write or NotebookEdit, or it has no tool list. Explore, Plan, and defined agents without those tools, such as code-reviewer and skeptic, never declare. An issue named anywhere else in a brief is never declared.
   - A `TaskUpdate` on `gh-N` declares when it sets status `in_progress` or sets an owner other than `team-lead`. This trigger carries the lead's own work and an in-process teammate's work, since neither starts with an Agent spawn.
   - `gh-tasks claim <n>` declares by hand. The hooks call the same code.

   An issue without `tasklist` is never declared.

4. **The first trigger reads the assignment state.** The first trigger on an issue in a session reads the issue's assignees and labels, the time of its last `labeled` event, and the session's login from `gh api user`, cached for the session. The session records the result in its bridge state, and later triggers on that issue make no GitHub call.
   - When the issue is unassigned, or this login is its only assignee, the trigger declares the work: it assigns this login and sets the label, writing only what is missing.
   - When any other login is assigned, the trigger writes nothing.
   - When `gh` is missing, unauthenticated, past its timeout, or refused (a rate limit, a fork without triage, a token without issue write), nothing is written and the state is reported as unknown.

5. **The working agent is told the state.** Every declaring trigger gives the agent doing the work one state line. That agent is the subagent for an Agent spawn, and the agent that made the call for a `TaskUpdate` or a manual claim. The line carries the session's login, the issue's assignees or "unassigned", the label and its age, and whether a declaration was written. When no declaration was written, the line also carries the next step: a subagent raises the issue with its lead before working it, and the lead declares; the lead runs `gh-tasks claim <n>` once the work goes ahead. A declared issue's line carries no next step. An illustration of the content, which is not a fixed format: `gh#457: you are aaronsb; assigned to octocat; in-progress since 2026-10-21; not declared; raise it with your lead before working it`.
   - A subagent receives the line through SubagentStart `additionalContext`. The PreToolUse hook queues the line, and SubagentStart hands the oldest queued line for the starting agent's type to that agent. The line names its issue, so a subagent that receives a line for an issue other than its brief's can see the mismatch.
   - The agent that made a `TaskUpdate` receives the line through PreToolUse `additionalContext`. `gh-tasks claim` prints it.
   - The lead's next whisper carries the same line when nothing was declared.
   - Rewriting the spawn's prompt through PreToolUse `updatedInput` would tie the line to the exact spawn. It replaces SubagentStart delivery once a probe shows that it works on the Agent tool, and that it leaves the spawn's permission outcome and the ways stash's input as they were.

6. **Another holder is the working agent's to reason about.** With the state line in hand, the agent may go on, or raise it with whoever dispatched it: a subagent asks its lead, and a lead whose work the operator directed asks the operator. A subagent raises it in its hand-back or a message to the lead and writes nothing to GitHub. When the work goes ahead beside the other holder, the lead declares it with `gh-tasks claim <n>`, on the operator's word or its own call under item 13. That manual claim adds this login beside the existing assignees.

7. **Work that starts with no declaration.** Three paths reach no trigger: the lead edits for an issue without marking its task `in_progress`, a spawn's brief names the issue but its description lacks `[gh#N]`, and a model without the task tools works an issue (ADR-180, Neutral). The issues way teaches the two habits that close the first two: mark the mirrored task `in_progress` when starting it, and put `[gh#N]` at the start of the Agent description. A backstop reports what the habits miss and writes nothing. The UserPromptSubmit pull, and `/wrap` before it posts comments, look for work on a mirrored issue this session has not declared. The signals are a `gh-N` task `in_progress` with no declaration recorded, and the current branch or a commit made since the session started naming the issue number. Each issue found gets one whisper line: `working gh#N with no declaration; gh-tasks claim N if you have taken it up`. The backstop never declares, because a branch name or a commit reference is a guess at what is being worked.

8. **`/wrap` relays the work.** The successor context takes the work up, so `/wrap` leaves the assignee and the label in place. For each declared issue still open, it posts one progress comment. The bridge builds the comment from fixed fields: a marker line, status `in progress`, the branch, the last commit and whether it is pushed, and one next-step line of at most 200 characters, which the model writes. A secret scan runs over the whole comment. If it finds anything, the bridge drops the comment and whispers one line, with no redaction. The comment carries no subagent output, no paths outside the repository, no environment values and no text from the issue body. The comment is posted whoever triggered the wrap: the operator typing `/wrap`, or the session wrapping itself near the end of its context. `/clear`, `/resume` and compaction leave the projection as it is.

9. **What clears the declaration.** Closing the issue clears it. The bridge removes the label if it is still there and leaves the assignee, and closing stays with ADR-180's push. An explicit release clears it as well:
   - `gh-tasks release <n> --park` removes the label and keeps the assignee.
   - `gh-tasks release <n> --drop` removes the label and this login's assignment.

   Only the operator, or a model the operator asked, parks or drops an issue. Nothing parks an issue on its own.

10. **Rebuilding state.** `/start` lists the open `tasklist` issues assigned to this login with the label. For each one it gives the next-step line from the latest comment carrying the marker, and the dates of that comment and the label. This list is where resumed work starts. Parked issues are listed separately.

11. **One writer for the label.** The declaration and the release verbs are the only writers of `in-progress`, and a declaration never labels a closed issue. In ADR-180's field-ownership table, the `in_progress` row ("push sets a label; never on a closed issue") becomes "the declaration sets the live label; closing or an explicit release clears it". Push keeps closing and description-edit comments.

12. **Self-echo.** The bridge state records each label or assignee write per issue as `claimed_state`, beside ADR-180's `pushed_state`. The whisper drops a delta that matches a write this session made, and reports changes made by anyone else.

13. **Authority for the writes.** The bridge writes the assignee, the label and the progress comment under one switch that the operator sets in the agent-ways user config and the install never writes. This is a tool's write under an opt-in the operator set (ADR-609 item 3). Creating the label on a repository is one of these writes. With the switch off, declarations run read-only. The session keeps its declarations locally, the whisper offers the write once per issue, and `/wrap` shows the comment and offers it. A model that runs `gh-tasks claim` or `release` by hand needs the user's ask or a CLAUDE.md line. The suggested line is: *Assign, label and comment on `tasklist` issues when work on them starts, relays or is parked.*

14. **Subagents stay read-only.** Every write here runs in a hook or a verb on the lead's side. A subagent's comms tools stay read-only, and the declaration is made for it on the lead's behalf.

## Consequences

### Positive

- `assignee:@me label:in-progress` lists the work a login has taken up, and `assignee:@me -label:in-progress` lists what it has parked, for people and for later sessions alike.
- After a break, `/start` rebuilds the session's starting point from GitHub: the issues in progress, their branches and their next steps.
- A crashed session leaves a projection that is close enough: the issue still reads as in progress, with a dated last comment saying where the work stood.
- Declaring never slows or stops a spawn beyond the first round trip, and GitHub being unreachable costs one whisper line.
- The agent doing the work knows who holds the issue before it starts, and can ask the agent or the operator who dispatched it.
- Each issue costs at most one declaration per session and one comment per wrap. No write tracks agents starting and finishing.
- The label half of ADR-180's push increment is done. The label has one writer, and the declaration covers the lead's own `in_progress` status.
- Subagent briefs, the handback rule and the head-agent-only pull stay as they are.

### Negative

- A label left by a crashed or abandoned session stays until someone parks, drops or closes the issue. Other people then read work as live when it is not, and this is the main cost of the design. The label's age in the whisper and in `/start`, and the date of the last comment, are the only signals that it has gone stale.
- An agent can go on beside another holder without raising it. The issue then shows only the other holder until the lead declares, and the lead learns of it from the whisper.
- A state line reaches a subagent by order and type. Parallel spawns of one agent type that name different issues can receive each other's lines. The issue number in the line shows the mismatch to a subagent that reads it against its brief.
- Work with no trigger is caught only as far as the backstop's signals reach. A lead that edits for an issue without marking its task or naming it in the branch or a commit stays undeclared and unreported.
- GitHub notifies the assignee of each assignment, and each progress comment notifies every subscriber. A long session that wraps several times posts one comment per declared issue each time.
- On a public repository each progress comment is public. The fixed fields and the scan limit what goes out, but the branch name and the next-step line are still published.
- The first trigger per issue per session costs one or two `gh` round trips, about 0.5 to 1 s, before the spawn.
- Only spawns that carry the prefix declare. An agent briefed on an issue without the prefix works undeclared. The issues way teaches the prefix.

### Neutral

- In a repository with one login, every declaration names that login, and the label separates work taken up from work parked.
- The bridge adds assignees on a declaration and removes one only on `--drop`, and then only this login.
- Wiring: PreToolUse on `TaskUpdate`, and a second hook on the PreToolUse `Task` matcher beside the ways stash, both reading the state and declaring. A SubagentStart hook beside `inject-subagent.sh` delivers the queued state line. The UserPromptSubmit pull gains the backstop check. `/wrap` gains the backstop check and the progress comment, and `/start` gains the in-progress and parked lists. No other hook event is wired for this.
- `gh-tasks` gains `claim` and `release` and grows past its current 790 lines. ADR-180's note on moving it to a Rust crate under `tools/` applies.
- The marker comment is a fixed format that the relay in #975 and the structured handoff in #857 can read.
- Implementation probes two questions the documentation leaves open. Does an in-process teammate's `TaskUpdate` fire PreToolUse in the lead's settings? Does PreToolUse `updatedInput` rewrite an Agent call's prompt, and with what decision, without changing the spawn's permission outcome or colliding with the ways stash? The record is amended with what the probes find.

## Alternatives Considered

- **A lock that blocks or prompts when someone else holds the issue.** It would stop two holders working one issue, through a deny or a permission prompt, for another login or for another session of the same login. Rejected: the operator described the claim as "simply declaring working on it", with "no interlocking or blocking or waiting for a prompt response". A prompt would also interrupt unattended runs and the operator's own relays.
- **Declare beside another holder automatically.** It would keep the projection complete without a manual step. Rejected: the operator asked that the agent be told the assignment state so it can "reason about working on it, or asking the dispatching agent (if a sub agent) or the operator (if the operator said to work on it) that someone else was assigned the work". An automatic declaration would make that choice before the agent saw the state.
- **Declare automatically from the branch or commit signals.** It would close the gaps the habits leave without any agent acting. Rejected: a branch name or a commit reference is a guess at what is being worked, and a wrong guess publishes a claim that someone else reads as fact.
- **A wrap pauses the issue.** It would make "assigned, not in progress" mean "stopped mid-work". Rejected: a wrap hands the work to the next context, which takes it up, so removing the label would show live work as stopped. It would also put the label through a remove-and-add cycle at every relay.
- **SessionEnd releases the issue.** It would need no `/wrap` step. Rejected: SessionEnd fires on `/clear` and `/resume`, which continue the work. The reference names no behaviour on a crash, and a hook cannot write the next step.
- **Clear the label when the last agent working the issue finishes.** It would track liveness per agent. Rejected: the projection would flip with every delegation, and it needs hooks that tie each spawn to its agent. The label marks the work, and agent liveness stays inside the session.
- **A Projects status field.** It has a richer status model and touches no labels. Rejected for the reason ADR-180 rejected Projects: a second object to keep in step, and this project does not use boards.
- **Assignee only.** It has the fewest writes. Rejected: work taken up and work parked would look the same.
- **A label and no assignee.** It sends no assignment notifications. Rejected: the operator asked for assignment, the hold would not survive a park, and `assignee:@me` is the search GitHub already offers.
- **A comment each time an agent finishes.** It is precise per agent. Rejected: one comment per finished delegation notifies every subscriber and buries the progress comment among routine ones.
- **Declare on any mention of an issue in a brief.** It needs no prefix discipline. Rejected: reviews and context reads would declare issues nobody is working on.
- **Subagents declare with their own `gh` calls.** It needs no hooks. Rejected: it breaks the read-only comms rule for subagents and spreads outward writes across briefs.
- **A per-agent GitHub identity, so each agent assigns itself.** It is the literal reading of "assign it to themselves". Rejected: no identity infrastructure exists for it, and the operator named the login accessing GitHub as the usual assignee.
