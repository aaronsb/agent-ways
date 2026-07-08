---
description: Recognize the beginning of a working session and route to the /start skill — orient to where work was left off, run a structured interview on a greenfield repo, check the context gauge to confirm it really is session-start, then recommend planning with context already warm.
vocabulary: start begin open session kick off pick up where we left off resume orient greenfield new project fresh start what were we doing catch up state of play onboard warm context ready to work get going first thing
pattern: /start\b|let'?s (get )?start|pick(ing)? up where|where (we|i) left off|start(ing)? (the |a )?(session|work|fresh)|new (work )?session|what were we (doing|working on)
refire: 0.15
scope: agent
macro: prepend
requires: ["Bash(ways:*)"]
---
<!-- epistemic: convention -->
# Starting

The operator is opening a working session — they want to get oriented and *begin*,
not to be handed a cold blank slate. This is what the **`start` skill** is for. Pull
it up (`Skill: start`) rather than improvising a "what should we do?" — the skill
enforces the parts that are easy to skip when you eyeball it.

`start` is the opening bookend to `wrap`. They are **inverse gauge-aware siblings**:
both read the same instrument (`ways context`), but `wrap` confirms the session is
near its *end* and scales the handoff down; `start` confirms the session is near its
*beginning* — and if it isn't, it says so. (The symmetry is at the skill layer;
`start` additionally carries a macro that whispers the gauge on disclosure, because it
needs a *proactive* nudge to dissuade a mid-session start — where `wrap` leans on the
`compaction-checkpoint` way firing on its own near the limit.)

## Why the skill, not a freehand "where do we begin?"

Three things go wrong when the open is done by feel:

- **You start in the wrong place.** A repo with work in flight, a clean `main`, an
  empty directory, and no repo at all each demand a different opening move. The skill
  checks the actual state — tracking markers, git status, the TaskList, the CLAUDE.md
  where it was invoked — before proposing anything, instead of guessing.
- **Greenfield gets no interview.** With nothing to pick up, the temptation is to
  invent a plausible-looking task. The skill runs a *structured interview* instead —
  the referent of "let's start" is itself the uncertainty; name it, don't hunt for it.
- **Planning starts cold.** The skill gathers state *first*, so when it recommends
  planning the context is already warm — plan mode inherits an oriented situation, not
  a blank page. That's the whole reason the order is start-then-plan.

## The gauge guard

Starting is a beginning-of-session act. If the context gauge shows the session is
already half or three-quarters spent, `start` **dissuades** rather than pretends —
opening fresh work deep into a used window strands it against the next compaction.
The right move there is usually `wrap` (close and hand off) or `/clear` and a fresh
window, not `start`.

## The planning caveat

Like `wrap` and `/compact`, `start` **cannot** invoke plan mode for you — Claude Code
forbids skills and hooks from firing `/` commands. So `start` does the orienting work
and then *recommends* planning (or runs a lightweight planning interview inline). It
prepares the ground; the operator toggles plan mode. Don't claim to have entered a
mode you can't.

## See Also

- start (skill) — the procedure this routes to.
- wrap(meta) — the closing bookend; same gauge, opposite pole.
- develop(meta) — once oriented, the loop `start` opens into.
- tracking(meta) — the session-start check for existing tracking files this leans on.
- context-status (skill) — the gauge `start` reads to decide it's really session-start.
