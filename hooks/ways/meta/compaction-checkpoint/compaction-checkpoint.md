---
trigger: context-threshold
threshold: 85
scope: agent
refire: 0.15
---
<!-- epistemic: convention -->
# Compaction Checkpoint

The session is nearing the context limit, where compaction becomes mandatory. Before it runs, check in with the user. The user remembers the whole session, and compaction does not touch that memory, so the user's view of what matters survives compaction best.

## Step 1: Summarize Where Things Stand

Be concrete and concise:
- What you set out to do this session
- What's been accomplished
- What's still in progress or remaining
- Key decisions made and their rationale
- Anything you're uncertain about

## Step 2: Ask the User

Put the summary and the questions below in your final message, and keep working while the user reads them. These questions meet the asking bar in core.md:

**Question 1** — "How's the direction?" (header: "Direction")
- Options covering: on track, needs adjustment, pivot needed

**Question 2** — "What should survive compaction?" (header: "Priority")
- Options based on the active work: which threads matter most going forward

Ask one or two focused questions, enough to check the direction.

Present this as a check-in on the work, and do not apologize for the context limit. For example:

> "We're approaching context limits and compaction will happen soon. Before it does, here's where I think we are — I'd like your take on what's landed and where to focus next."

## Step 3: Write the Synthesis

After the user responds, combine your summary with their steering into a synthesis:

1. Keep the synthesis available for the successor session, in the convention this session started with: the task list, a `tasklist` issue (ADR-180), or a handoff file the project already uses
2. With no known convention, add the synthesis to `.claude/handoff.wrap.md` when `/wrap` has written one, and otherwise write `.claude/handoff.local.md`, both at the repository root. When `.gitignore` does not cover the file, list it in `.git/info/exclude`
3. Include the user's priorities and direction — their words, not your paraphrase
4. **Produce a copy-pastable continuance prompt** — a short block the user can paste into a *fresh* session if this one ends: the goal/intent, what's landed, the immediate next step, and the key files/branches in play. When several tasks remain, add the working mode from the `/wrap` skill: the main session steers, subagents carry implementation and review, and each branch gets its own worktree. Compaction keeps the session running, and the continuance prompt still works if the session ends.

## Step 4: Hand Over Directed Compaction

After writing the synthesis, hand the user a ready-to-run `/compact <focus>` line, with the focus naming their priorities. No way or skill can run a slash command, so the user runs it (see wrap(meta)). The synthesis is the newest and most informative text in context, so compaction gives it the most weight in the summary. Without a focus line, compaction keeps whatever its generic summary picks.

## If a Goal Is Active

A `/goal` survives compaction. The goal belongs to the session, and compaction does not end the session, so the goal's evaluator keeps working toward the condition afterwards. When a goal is active:

- **Restate the goal condition in the synthesis.** The turns after compaction steer by it.
- **Keep the checkpoint light.** The goal already sets the direction, so confirm that it still holds instead of working out the priorities again.
- **Consider finishing first.** If the goal's work is nearly done, complete it *before* compacting.

## See Also

- wrap(meta) — the on-demand version: same wrap-up when *you* pick the seam, routing to the `/wrap` skill. This way is the automatic sibling that fires near the limit.
- compaction-checkpoint/in-flight(meta) — child: what is still running or uncommitted at the checkpoint
- delivery/issues(softwaredev) — the GitHub issues behind the task list survive compaction (ADR-180)
- todos(meta) — task state should be captured before compaction
- goals(meta) — an active /goal anchors continuation across the checkpoint
