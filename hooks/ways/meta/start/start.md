---
description: Recognize the beginning of a working session and route to the /start skill — orient to where work was left off, run a structured interview on a greenfield repo, check the context gauge to confirm it really is session-start, then recommend planning with context already warm. Reorienting is read-only. Report where we left things, what state the repo is in, and what was committed to but not finished.
vocabulary: start begin open session kick off pick up where we left off resume orient greenfield new project fresh start what were we doing catch up state of play onboard warm context ready to work get going first thing slate toggles reorient read-only branch commits ahead behind
pattern: /start\b|let'?s (get )?start|pick(ing)? up where|where (we|i) left off|start(ing)? (the |a )?(session|work|fresh)|new (work )?session|what were we (doing|working on)
refire: 0.15
scope: agent
macro: prepend
requires: ["Bash(ways:*)"]
---
<!-- epistemic: convention -->
# Starting

The operator is opening a working session — they want to get oriented and *begin*, not to be handed a cold blank slate. This is what the **`start` skill** is for. Pull it up (`Skill: start`) rather than improvising a "what should we do?" — the skill enforces the parts that are easy to skip when you eyeball it.

`start` is the opening bookend to `wrap`. They are **inverse gauge-aware siblings**: both read the same instrument (`ways context`), but `wrap` confirms the session is near its *end* and scales the handoff down; `start` confirms the session is near its *beginning* — and if it isn't, it says so. (The symmetry is at the skill layer; `start` additionally carries a macro that whispers the gauge on disclosure, because it needs a *proactive* nudge to dissuade a mid-session start — where `wrap` leans on the `compaction-checkpoint` way firing on its own near the limit.)

## Why the skill, not a freehand "where do we begin?"

Three things go wrong when the open is done by feel:

- **You start in the wrong place.** A repo with work in flight, a clean `main`, an empty directory, and no repo at all each demand a different opening move. The skill checks the actual state — git status, the TaskList and the open issues it mirrors, the CLAUDE.md where it was invoked — before proposing anything, instead of guessing.
- **Greenfield gets no interview.** With nothing to pick up, the temptation is to invent a plausible-looking task. The skill runs a *structured interview* instead — the referent of "let's start" is itself the uncertainty; name it, don't hunt for it.
- **Planning starts cold.** The skill gathers state *first*, so when it recommends planning the context is already warm — plan mode inherits an oriented situation, not a blank page. That's the whole reason the order is start-then-plan.

## Reorientation is read-only

Whether or not the skill runs, rebuilding context changes nothing. Edit, create, and commit nothing until the user asks for a change. If you spot a fix while summarizing, note it and ask after the report.

- **Confirm the context.** Check the working directory, `git remote -v`, and the active cloud and GitHub accounts. Users with many similar repos and accounts hit a mismatch often. If the request seems to belong to another repo or account, say so before writing code.
- **Read the state.** Report the branch, the last several commits with dates, `git status`, and ahead or behind after a `git fetch`. Read plan files, the task list, open issues, and the prior session transcripts for this directory if the harness keeps them.
- **Check doc freshness.** Compare the last commit date of the project instructions, README, and any relevant skill file against recent code commits. Flag a doc that lags a change it describes and offer to update it. See freshness(softwaredev).
- **Report briefly.** Give branch, recent commits, sync status, and where the work stands. For a next-steps request, list only items committed to but not finished, open questions, and risks. Do not pad it with generic next steps.

When the user asks for something they did or wrote before ("find the exact prompt I used"), search the session history and local files and return the verbatim original with its source and date. Do not reconstruct an approximation when the original can be found. A recap covers the original prompt (quoted), what was tried, what worked, and the lessons. Keep it in chat.

## The gauge guard

Starting is a beginning-of-session act. If the context gauge shows the session is already half or three-quarters spent, `start` **dissuades** rather than pretends — opening fresh work deep into a used window strands it against the next compaction. The right move there is usually `wrap` (close and hand off) or `/clear` and a fresh window, not `start`.

## The planning caveat

Like `wrap` and `/compact`, `start` **cannot** invoke plan mode for you — Claude Code forbids skills and hooks from firing `/` commands. So `start` does the orienting work and then *recommends* planning (or runs a lightweight planning interview inline). It prepares the ground; the operator toggles plan mode. Don't claim to have entered a mode you can't.

## See Also

- start (skill) — the procedure this routes to.
- wrap(meta) — the closing bookend; same gauge, opposite pole.
- develop(meta) — once oriented, the loop `start` opens into.
- delivery/issues(softwaredev) — the GitHub issues the session task list mirrors (ADR-180); the continuity check leans on them.
- freshness(softwaredev) — docs that lag the code they describe.
- context-status (skill) — the gauge `start` reads to decide it's really session-start.
