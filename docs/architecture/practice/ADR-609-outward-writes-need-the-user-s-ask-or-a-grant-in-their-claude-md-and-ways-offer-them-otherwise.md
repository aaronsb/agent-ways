---
contract: adr/v1
kind: decision
verb: constrain
capability:
  - method
  - loop
basis:
  - evidence: 'Claude Code 2.1.306 binary (build 2026-10-21, git 01ae8327), read with strings on 2026-10-22. Standard body, "Executing actions with care": "unless actions are authorized in advance in durable instructions like CLAUDE.md files, always confirm first. Authorization stands for the scope specified, not beyond", and pushing code and "creating/closing/commenting on PRs or issues" are listed as actions that warrant confirmation. Lean body, <action_caution>: "For actions that are hard to reverse or outward-facing, confirm first unless durably authorized or explicitly told to proceed without asking; approval in one context doesn''t extend to the next."'
  - evidence: 'Claude Code 2.1.306, same read, Bash tool description: standard form "Only create commits when requested by the user. If unclear, ask first" and "DO NOT push to the remote repository unless the user explicitly asks you to do so"; lean form "Commit or push only when the user asks. If on the default branch, branch first." Edits to files in the working tree are stated not to be hard-to-reverse or outward-facing.'
  - evidence: 'Corpus survey, 2026-10-22: meta/trust/delegation:21 says "Ask for push/PR" for a git repository. meta/codify:26 commits the write-up and opens issues; meta/attend/context-pressure:16 commits; meta/compaction-checkpoint:42 puts the synthesis on a GitHub issue; delivery/commits/prepare:12 commits once per unit of work; delivery/github/follow-through:14,16 files and closes issues; delivery/issues:51 files an issue per residual; delivery/github:16 lands every change through a PR. None of these names who authorised the write.'
  - precedent: ADR-175
  - precedent: ADR-128
  - precedent: ADR-601
  - precedent: ADR-142
  - precedent: ADR-608
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "(c) Commit + merge, (d) File issues"
    via: AskUserQuestion answer, session 2026-10-22
status: accepted
date: 2026-10-22
deciders:
  - aaronsb
related:
  - ADR-175
  - ADR-608
  - ADR-128
  - ADR-601
  - ADR-180
  - ADR-142
observable:
  - "read: meta/trust/delegation's git row and the commits, github, issues, codify, context-pressure and compaction-checkpoint ways name the same sources for a write: the user's ask, a procedure the user asked for, or a CLAUDE.md grant; each offers the write otherwise"
  - "see: in a session whose CLAUDE.md grants nothing, work that fires delivery/issues ends with each residual named and an offer to file it, and no gh issue create ran"
  - "see: in a session whose CLAUDE.md carries the issue line, the same work files each residual and says where it went"
  - "see: a session asked to merge a reviewed PR with green CI merges it and cleans up without asking again; a session that was only asked to push stops after the push"
---

# ADR-609: Outward writes need the user's ask or a grant in their CLAUDE.md, and ways offer them otherwise

## Summary

- **Decided:** A way's text does not authorise commits, pushes, pull requests, or filing, closing or commenting on issues. Each needs one of three sources. The first is the user asking in the conversation, for the act or for a procedure that states it. The second is a goal condition that names the act. The third is a line in a CLAUDE.md file the user wrote that grants that kind of write. Without one, the way names the write and its content in one line, offers it, and goes on with the rest of the work. Memory notes do not grant. agent-ways documents suggested CLAUDE.md lines and never writes them. ADR-175's argument is not extended to these writes.
- **Trades away:** Until the operator adds lines, ways that commit, file issues or open PRs on their own today will offer instead, so more offers collect at the end of a turn. Standing permissions recorded only in memory stop counting.
- **One-way?** No. Reversible: it changes way text and asks for lines in a file the operator owns, and both revert in one session.
- **Probes:** *Confident (standing):* You've said to commit locally and to merge reviewed, green PRs without asking each time. Should those keep working without asking? For that they need to be lines in your CLAUDE.md. Yes or no. *Not confident (issues):* Should I file issues for leftover work without asking you first? Yes or no.
- **Inversion:** At one end, ways are standing authority for every write they name. At the other, every outward write is asked for each time. This sits between them: the authority is the operator's own words, said once in CLAUDE.md or each time in the conversation, scoped to what those words name.

## Context

Ways disagree about who authorises a write that leaves the session. The trust way's resource table says to ask before a push or a PR, because the write carries the human's name and other people see it. Several delivery and meta ways direct commits, issue filing, issue closing and PRs as plain steps, with no word on who asked for them. A session that reads both gets two instructions for one moment, and which one wins depends on which way fired last. ADR-175 declined that kind of recency contest.

The harness texts are consistent with each other. Both prompt bodies ask for confirmation before outward-facing or hard-to-reverse actions, unless the action is durably authorised or the user has said to proceed. The standard body names CLAUDE.md files as the example of durable instructions and says an authorisation holds for the scope it states. The Bash tool's git instructions, in both forms, say to commit or push only when the user asks. Edits in the working tree are stated not to be outward-facing. A commit is local and can be undone, but the harness still gates it with the push.

ADR-175 let a way's delegation step count as the user's request. That argument rested on two things. A spawn spends tokens inside the session and leaves nothing outside it, and the gate it answered asked only whether the user requested the procedure. ADR-608 withdraws even that reading for ways. An outward write differs on both counts. It is visible to others and attributed to the human, and the harness texts name durable instructions such as CLAUDE.md rather than any procedure the corpus ships.

## Decision

1. **Scope.** Outward writes are pushes, creating, merging, closing or commenting on PRs, filing, closing or commenting on issues, and tags and releases. Commits are in scope because the harness gates them with pushes. Working-tree edits are not.
2. **A way's text does not authorise.** A way is corpus text, shipped the same to every install. It is not the user's instruction about the user's own repositories and accounts. A way that describes when and how to make a write gives timing and form, and the authority comes from one of the sources below.
3. **What authorises**, each for the scope it states:
   - **The user's ask in the conversation.** This can be for the act, or for a procedure that states the act, such as `/merge`, run because the user asked to land the work. The scope is what was asked: "push it" does not include merging. Approval in one context does not carry to the next.
   - **A goal condition that names the act.** Under `/goal`, the condition is the operator's stated intent, and it authorises the doors it names (ADR-601).
   - **A grant in a CLAUDE.md file the user wrote**, user-level or project-level. The scope is the classes of write the line names.
   - Writes that a tool makes under an opt-in the operator set stay as their own decision records them. The task bridge closes an issue carrying the `tasklist` label when its task completes (ADR-180).
4. **Memory does not grant.** The model writes memory notes, the harness's memory compaction rewrites them, and they stay with one instance (ADR-128). A memory note can record how the user likes a write done, such as a regular merge or a local-first cadence. It cannot record whether the user authorises the write. When a memory note reads as a standing permission, the session tells the operator once that it counts only from CLAUDE.md.
5. **Otherwise, offer.** With no source in hand, the way names the write and its content in one line: the issue title, the PR title, or the commit message and branch. It offers the write and goes on with the rest of the work. The offer goes in the turn's final message. The way does not stop the turn only to ask.
6. **Suggested lines.** `meta/trust/delegation` documents a short block the operator can copy into a CLAUDE.md, choosing the lines they want. The install never writes it (ADR-142). The block:
   - *Commit on a non-default branch at each accepted unit of work.*
   - *Push a branch and open its PR once its work holds locally.*
   - *Merge a reviewed PR with green CI by regular merge, as delivery/merge classifies it.*
   - *File an issue for out-of-scope work found mid-task, and close an issue with a comment when the commit that resolves it lands.*
7. **Never by standing grant.** Force pushes, history rewrites, release tags, and writes to repositories the user does not own are asked for each time, whatever a CLAUDE.md says. The harness texts treat these separately, and they are the one-way doors in ADR-601.

### Way edits this implies

The ways are not edited by this record. These are the edits it implies:

- `meta/trust/delegation/delegation.md:21`: the git row's write cell becomes "Ask for commits, pushes, PRs and issue writes, unless the user asked for it, a goal condition names it, or their CLAUDE.md grants that kind of write". Add a short "Standing grants" section with the suggested block, the rule on memory notes, and the writes that are never covered by a standing grant.
- `meta/codify/codify.md:26`: "Include the write-up in the work's commit. Offer an issue for any debt it surfaced, or file it where a standing grant covers issues."
- `meta/attend/context-pressure/context-pressure.md:16,19`: "Propose a commit for the work so far." Drop "so nothing is lost through compaction", because compaction leaves the working tree as it is. At line 19, the commit proposal comes before reflection.
- `meta/compaction-checkpoint/compaction-checkpoint.md:42`: "Offer to put the synthesis on a `tasklist` issue (ADR-180), new or existing."
- `softwaredev/delivery/commits/prepare/prepare.md:12`: "When committing is in scope (the user asked, a procedure they asked for commits, or their CLAUDE.md grants it), commit once per accepted unit of work..."
- `softwaredev/delivery/github/follow-through/follow-through.md:14,16`: "When parking an idea, offer to file the issue before any build..." and "When you close an issue, close it with a comment that says what was done and links the commit."
- `softwaredev/delivery/issues/issues.md:51`: name each residual and offer to file it as its own issue, or file it where a standing grant covers issues.
- `softwaredev/delivery/github/github.md:16`: "When the user asks to land a change, it lands through a PR, solo projects included."

## Consequences

### Positive

- The ways stop contradicting each other and the harness on who decides an outward write.
- Standing permission lives in one place the operator writes and can read. A project-level line can be versioned with the repository, and memory notes cannot.
- The offer form keeps work moving. A turn that would have ended in a question ends with the work done and one line per write to approve.

### Negative

- Until the operator writes lines, sessions offer writes they make today, and a long session can end with several offers to answer.
- A CLAUDE.md line is coarse. "File an issue for out-of-scope work" covers every repository the user-level file reaches, including ones where the user would rather not.
- Nothing checks way text for an unconditional outward step. A future way can reintroduce one, and review has to catch it.

### Neutral

- Procedures the user asks for by name, such as `/merge` and `/release`, are unchanged. The ask covers the writes they state.
- Release tags stay as `delivery/release` describes them: the operator signs them, and they are never under a standing grant.
- This record and ADR-608 both place standing grants in the operator's CLAUDE.md. A later decision that adds a different durable channel would change both together.

## Alternatives Considered

- **Treat way text as durable authorisation, extending ADR-175.** Rejected. A way is the corpus's instruction, shipped to every install, and not the user's instruction about their own resources. The harness texts name the user's ask and durable instructions such as CLAUDE.md, and an outward write carries the human's name (meta/trust/delegation). The argument would also put each way into the recency contest with the harness text, which ADR-175 declined.
- **Ask before every outward write, with no standing grant.** Rejected. The harness text allows durable authorisation in advance. Refusing it would bring back the per-occasion friction that ADR-175 removed for delegation.
- **Let memory notes grant.** Rejected for the reasons in item 4. A grant the model wrote about itself is the model authorising itself. Memory compaction can rewrite it without the operator seeing.
- **Have the install write the grant into `~/.claude/CLAUDE.md`.** Rejected. A grant the installer writes is the corpus authorising itself. It also writes into a file the install leaves to the user (ADR-142).
- **Exempt commits because they are local and reversible.** Rejected. Both forms of the Bash tool's git instructions gate commits with pushes. A way that commits on its own contradicts that text, and the outcome then turns on recency again. The grant line for commits is the cheapest one to write.
- **Use permission allow rules in settings as the grant.** Rejected as the mechanism. An allow rule decides whether a tool call prompts, not whether the model should take the action unasked. It complements a CLAUDE.md line and does not replace one.
