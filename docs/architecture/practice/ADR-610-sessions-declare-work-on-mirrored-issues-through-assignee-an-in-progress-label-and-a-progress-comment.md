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
  - standard: 'Claude Code hooks reference, https://code.claude.com/docs/en/hooks, read 2026-10-23: PreToolUse input carries tool_name, tool_input and tool_use_id, and agent_id with agent_type inside a subagent; SessionEnd fires on clear, resume, logout, prompt_input_exit and other, and the reference names no behaviour on a crash or kill'
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
  - see: on an issue assigned to another login, the spawn proceeds with no prompt, the issue gains this login beside the other, and the whisper carries one line naming the other holder
  - see: with gh unauthenticated or the network down, the spawn proceeds and the next whisper carries one line saying the claim was not written
  - see: after /wrap, the assignee and the label stay, one comment carries the marker line and the fixed fields, and a new session's /start lists the issue as in progress with its next-step line
    run: gh issue list --label tasklist --label in-progress --assignee @me --state open
  - see: "`gh-tasks release N --park` removes the label and keeps the assignee, and /start lists the issue under parked"
  - see: "the pull after a claim whispers no `relabeled #N` for the session's own write"
  - see: with the config switch off, no assign, label or comment is written, and the whisper offers the claim once per issue
---

# ADR-610: Sessions declare work on mirrored issues through assignee, an in-progress label and a progress comment

## Summary

- **Decided:** Working a `tasklist` issue follows a convention the bridge carries out at the door. On the way in, a session declares the work: the issue gets the session's GitHub login as an assignee and an `in-progress` label. While the work continues, each `/wrap` posts a progress comment. On the way out, the label comes off when the issue closes, or when the operator parks the issue (assignee kept) or drops it (assignee removed). Work starts through a subagent spawn whose description carries `[gh#N]`, a task update on the mirrored task, or a manual verb, and the declaration is written once per issue per session. Nothing blocks, prompts or waits. When someone else already holds the issue, the declaration is still written and the session is told in one line. The GitHub state is a projection for other people and for rebuilding state later. Agents of one login coordinate inside the session. `/wrap` is a relay to the next context and keeps the assignee and the label, and it posts the comment whoever triggered the wrap. The hooks write only while the operator's config switch is on.
- **Trades away:** Nothing stops two people declaring the same issue. A crashed or abandoned session leaves `in-progress` on its issues until someone parks, drops or closes them, so other people can read work as live that is not. Every wrap with claimed issues posts a comment that notifies subscribers, and the comment is public on a public repository. The first trigger per issue per session costs up to about a second.
- **One-way?** No. It is reversible: two hooks and two verbs, which revert in one session, and labels and assignees can be removed. Comments already posted stay public, and their notifications have gone out.
- **Probes:** *Confident (monday):* When you come back after a break, `/start` lists the issues still assigned to you and marked in progress, each with the next step from its latest progress comment, and lists parked issues separately. Is that the list you want to start from? *Not confident (unattended wrap):* When a session wraps on its own near the end of its context, with no one typing `/wrap`, it still posts the progress comment on each issue it holds, and everyone watching those issues is notified. Is that acceptable, or should a wrap you did not start leave no comment?
- **Inversion:** At one end, the GitHub state is a lock: a claim blocks or prompts anyone else who starts the issue, and agents check it before each step. At the other end, GitHub shows nothing and the state of the work lives only in sessions. This decision sits between them: a declaration on the way in, a progress note at each relay and a clearing on the way out, read by people and later sessions, which gates nothing.

## Context

ADR-180 mirrors `tasklist` issues into the session task list. Its push increment, still open in #457, covers closing an issue on Stop and an in-progress label from the session's `in_progress` status. It has no rule for assignees and none for subagents. Most work on an issue in this project runs in subagents, the bridge's hooks act for the head agent only, and subagents keep their comms tools read-only (collaboration/teams/handback). Nothing on GitHub shows who has taken up an issue or where the work stands. The operator asked that a subagent taking on an issue update its status and assign it. They described the GitHub state as an outward projection, for other people and for rebuilding state after a break, and the claim as a declaration with no interlocking.

These facts shape the design:

- **PreToolUse sees every claiming call.** Its input carries the tool name and input, and `agent_id` with `agent_type` inside a subagent, so one hook sees a spawn, a task update and the agent that made either (hooks reference).
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

4. **A declaration is written once per issue per session.** The first trigger on an issue in a session adds this login as an assignee, beside any existing ones, and sets the label. It writes only what is missing. The session records the declaration in its bridge state, and later triggers on that issue make no GitHub call.

5. **Other holders are reported.** The first trigger reads the issue's assignees and labels, the time of its last `labeled` event, and the session's login from `gh api user`, cached for the session.
   - When another login is already assigned, the declaration is still written, and the whisper carries one line naming the other holder.
   - When this login already holds the issue with the label on, the whisper carries one line with the label's age.
   - When `gh` is missing, unauthenticated, past its timeout, or refused (a rate limit, a fork without triage, a token without issue write), nothing is written and the whisper carries one line.

6. **`/wrap` relays the work.** The successor context takes the work up, so `/wrap` leaves the assignee and the label in place. For each declared issue still open, it posts one progress comment. The bridge builds the comment from fixed fields: a marker line, status `in progress`, the branch, the last commit and whether it is pushed, and one next-step line of at most 200 characters, which the model writes. A secret scan runs over the whole comment. If it finds anything, the bridge drops the comment and whispers one line, with no redaction. The comment carries no subagent output, no paths outside the repository, no environment values and no text from the issue body. The comment is posted whoever triggered the wrap: the operator typing `/wrap`, or the session wrapping itself near the end of its context. `/clear`, `/resume` and compaction leave the projection as it is.

7. **What clears the declaration.** Closing the issue clears it. The bridge removes the label if it is still there and leaves the assignee, and closing stays with ADR-180's push. An explicit release clears it as well:
   - `gh-tasks release <n> --park` removes the label and keeps the assignee.
   - `gh-tasks release <n> --drop` removes the label and this login's assignment.

   Only the operator, or a model the operator asked, parks or drops an issue. Nothing parks an issue on its own.

8. **Rebuilding state.** `/start` lists the open `tasklist` issues assigned to this login with the label. For each one it gives the next-step line from the latest comment carrying the marker, and the dates of that comment and the label. This list is where resumed work starts. Parked issues are listed separately.

9. **One writer for the label.** The declaration and the release verbs are the only writers of `in-progress`, and a declaration never labels a closed issue. In ADR-180's field-ownership table, the `in_progress` row ("push sets a label; never on a closed issue") becomes "the declaration sets the live label; closing or an explicit release clears it". Push keeps closing and description-edit comments.

10. **Self-echo.** The bridge state records each label or assignee write per issue as `claimed_state`, beside ADR-180's `pushed_state`. The whisper drops a delta that matches a write this session made, and reports changes made by anyone else.

11. **Authority for the writes.** The bridge writes the assignee, the label and the progress comment under one switch that the operator sets in the agent-ways user config and the install never writes. This is a tool's write under an opt-in the operator set (ADR-609 item 3). Creating the label on a repository is one of these writes. With the switch off, declarations run read-only. The session keeps its declarations locally, the whisper offers the write once per issue, and `/wrap` shows the comment and offers it. A model that runs `gh-tasks claim` or `release` by hand needs the user's ask or a CLAUDE.md line. The suggested line is: *Assign, label and comment on `tasklist` issues when work on them starts, relays or is parked.*

12. **Subagents stay read-only.** Every write here runs in a hook or a verb on the lead's side. A subagent's comms tools stay read-only, and the declaration is made for it on the lead's behalf.

## Consequences

### Positive

- `assignee:@me label:in-progress` lists the work a login has taken up, and `assignee:@me -label:in-progress` lists what it has parked, for people and for later sessions alike.
- After a break, `/start` rebuilds the session's starting point from GitHub: the issues in progress, their branches and their next steps.
- A crashed session leaves a projection that is close enough: the issue still reads as in progress, with a dated last comment saying where the work stood.
- Declaring never slows or stops a spawn beyond the first round trip, and GitHub being unreachable costs one whisper line.
- Each issue costs at most one declaration per session and one comment per wrap. No write tracks agents starting and finishing.
- The label half of ADR-180's push increment is done. The label has one writer, and the declaration covers the lead's own `in_progress` status.
- Subagent briefs, the handback rule and the head-agent-only pull stay as they are.

### Negative

- A label left by a crashed or abandoned session stays until someone parks, drops or closes the issue. Other people then read work as live when it is not, and this is the main cost of the design. The label's age in the whisper and in `/start`, and the date of the last comment, are the only signals that it has gone stale.
- Two people can declare the same issue and both work it. Each sees the other as an assignee, and the session sees one whisper line; deciding who goes on is left to them.
- GitHub notifies the assignee of each assignment, and each progress comment notifies every subscriber. A long session that wraps several times posts one comment per declared issue each time.
- On a public repository each progress comment is public. The fixed fields and the scan limit what goes out, but the branch name and the next-step line are still published.
- The first trigger per issue per session costs one or two `gh` round trips, about 0.5 to 1 s, before the spawn.
- Only spawns that carry the prefix declare. An agent briefed on an issue without the prefix works undeclared. The issues way teaches the prefix.

### Neutral

- In a repository with one login, every declaration names that login, and the label separates work taken up from work parked.
- The bridge adds assignees on a declaration and removes one only on `--drop`, and then only this login.
- Wiring: PreToolUse on `TaskUpdate`, and a second hook on the PreToolUse `Task` matcher beside the ways stash, both calling the declaration. `/wrap` gains the progress comment and `/start` gains the in-progress and parked lists. No other hook event is wired for this.
- `gh-tasks` gains `claim` and `release` and grows past its current 790 lines. ADR-180's note on moving it to a Rust crate under `tools/` applies.
- The marker comment is a fixed format that the relay in #975 and the structured handoff in #857 can read.
- Implementation probes one question the documentation leaves open: does an in-process teammate's `TaskUpdate` fire PreToolUse in the lead's settings? The record is amended with what the probe finds.

## Alternatives Considered

- **A lock that blocks or prompts when someone else holds the issue.** It would stop two holders working one issue, through a deny or a permission prompt, for another login or for another session of the same login. Rejected: the operator described the claim as "simply declaring working on it", with "no interlocking or blocking or waiting for a prompt response". A prompt would also interrupt unattended runs and the operator's own relays.
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
