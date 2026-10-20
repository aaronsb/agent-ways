---
trigger: context-threshold
threshold: 85
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Compaction Checkpoint

We're nearing the context limit where compaction becomes mandatory. Before that happens, this is a good moment to sync with the user — they have continuous persistence and can provide the synthesis that survives compaction best.

## Step 1: Summarize Where Things Stand

Be concrete and concise:
- What you set out to do this session
- What's been accomplished
- What's still in progress or remaining
- Key decisions made and their rationale
- Anything you're uncertain about
- Background jobs, monitors, and dev servers still running, and whether each survives compaction or a reboot. Stop stray shells and stalled loops now.

## Step 2: Ask the User

Present your summary, then use `AskUserQuestion` to check in:

**Question 1** — "How's the direction?" (header: "Direction")
- Options covering: on track, needs adjustment, pivot needed

**Question 2** — "What should survive compaction?" (header: "Priority")
- Options based on the active work: which threads matter most going forward

Keep it to 1-2 focused questions. The goal is calibration, not a survey.

On an unattended run, ask nothing. Write the state, label each assumption "override if wrong", and keep working.

Frame this as a collaboration checkpoint, not a limitation apology. Something like:

> "We're approaching context limits and compaction will happen soon. Before it does, here's where I think we are — I'd like your take on what's landed and where to focus next."

## Step 3: Write the Synthesis

After the user responds, combine your summary with their steering into a checkpoint file:

1. If active `TaskCreate` tasks exist, update their descriptions with the user's input
2. If no task list, put the synthesis on a GitHub issue carrying the `tasklist` label (ADR-180), new or existing
3. Include the user's priorities and direction — their words, not your paraphrase
4. **Produce a copy-pastable continuance prompt** — a short block the user can paste into a *fresh* session if this one ends: the goal/intent, what's landed, the immediate next step, and the key files/branches in play. When several tasks remain, add the working mode from the `/wrap` skill: the main session steers, subagents carry implementation and review, and each branch gets its own worktree. Compaction keeps the session alive; a continuance prompt is the handoff that survives even a hard reset.

Save the state in repo artifacts, not in memory. Update the plan file, a decision log, or a README progress table where the project keeps them. Personal working notes, such as your own facilitator notes, go in a gitignored file unless the user asks to share them. Park unfinished ideas as resumable issues. A provisional or risky change (a stopgap config, an open incident) gets a short dated note in the project instructions: what was done, why, and what comes next.

Before you write a memory entry, run the surprise test and routing table in memory(meta).

## Step 4: Report What Is Uncommitted

List the uncommitted files with a proposed commit split, and name any file you are holding back on purpose. Commit only if the user asked, and push only if they named the push. End with one plain line on what is uncommitted, any follow-ups the user owns, and the single most useful next step.

On a fast exit ("I have to leave now"), skip the cleanup. Let the current step finish, write the state to disk, and hand over the continuance prompt.

## Step 5: Offer Directed Compaction

After writing the synthesis, offer to compact now:

> "I've captured our synthesis. Want me to compact now so we get a controlled, directed compaction with your priorities as the anchor — rather than waiting for the system to do a generic one?"

If the user agrees, run `/compact` (or let them trigger it). The synthesis is the freshest, highest-signal content in context, so compaction will preserve it as the dominant signal.

This is the difference between controlled compaction (user-directed, synthesis-anchored) and uncontrolled compaction (generic, whatever the system decides to keep).

## If a Goal Is Active

A `/goal` survives compaction — it's session-scoped, and compaction doesn't end the session, so the evaluator keeps driving toward the condition on the other side. When a goal is active:

- **Restate the goal condition in the synthesis** — it's the anchor the post-compaction turns will steer by.
- **Keep the checkpoint light** — the goal already carries the direction, so confirm
  it still holds rather than re-deriving priorities from scratch.
- **Consider finishing first** — if the goal's work is nearly done, completing it
  *before* compacting beats carrying a near-complete loop across the boundary.

## See Also

- wrap(meta) — the on-demand version: same wrap-up when *you* pick the seam, routing to the `/wrap` skill. This way is the automatic sibling that fires near the limit.
- delivery/issues(softwaredev) — the GitHub issues behind the task list survive compaction (ADR-180)
- todos(meta) — task state should be captured before compaction
- goals(meta) — an active /goal anchors continuation across the checkpoint
