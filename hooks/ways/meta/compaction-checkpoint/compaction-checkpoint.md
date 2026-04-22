---
trigger: context-threshold
threshold: 95
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

## Step 2: Ask the User

Present your summary, then use `AskUserQuestion` to check in:

**Question 1** — "How's the direction?" (header: "Direction")
- Options covering: on track, needs adjustment, pivot needed

**Question 2** — "What should survive compaction?" (header: "Priority")
- Options based on the active work: which threads matter most going forward

Keep it to 1-2 focused questions. The goal is calibration, not a survey.

Frame this as a collaboration checkpoint, not a limitation apology. Something like:

> "We're approaching context limits and compaction will happen soon. Before it does, here's where I think we are — I'd like your take on what's landed and where to focus next."

## Step 3: Write the Synthesis

After the user responds, combine your summary with their steering into a checkpoint file:

1. If active `TaskCreate` tasks exist, update their descriptions with the user's input
2. If no task list, write a brief synthesis to the project's tracking file (`.claude/todo-*.md`)
3. Include the user's priorities and direction — their words, not your paraphrase

## Step 4: Offer Directed Compaction

After writing the synthesis, offer to compact now:

> "I've captured our synthesis. Want me to compact now so we get a controlled, directed compaction with your priorities as the anchor — rather than waiting for the system to do a generic one?"

If the user agrees, run `/compact` (or let them trigger it). The synthesis is the freshest, highest-signal content in context, so compaction will preserve it as the dominant signal.

This is the difference between controlled compaction (user-directed, synthesis-anchored) and uncontrolled compaction (generic, whatever the system decides to keep).

## See Also

- tracking(meta) — tracking files survive compaction
- todos(meta) — task state should be captured before compaction
