---
description: presenting genuine decisions to the human as explicit choices rather than burying options in prose or deciding silently
vocabulary: choice option decision present ask user select alternatives branch point recommend tradeoff prefer fork pick which clarify
pattern: which (one|option|approach)|ask the user|let (the user|the human|me|you) decide|how (should|do) (we|you|i)
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Presenting Choices

When you hit a real branch point — distinct options whose answer changes what you
do next — **present it as an explicit choice**, not a paragraph the human has to
parse, and not a silent pick they only discover from the result.

The harness has a tool for this (`AskUserQuestion`): structured options with short
headers, a recommended default, and one-line tradeoffs. A clean choice surface
respects the human's time far more than a wall of prose ending in "let me know how
you'd like to proceed" — and far more than guessing and making them undo it.

## When to surface a choice

| Situation | Surface it? |
|-----------|-------------|
| Distinct options, the answer changes your next action, no obvious default | **Yes** — present the choice |
| Multiple independent decisions stacked up at once | **Yes** — a few focused questions beats a prose dump |
| One option is clearly right given the context | **No** — pick it, name it, proceed (say what you chose and why) |
| A fact you can verify in the code or docs yourself | **No** — go look; don't outsource lookups |
| "Is my plan ready / should I proceed?" | **No** — that's not a choice, it's hedging |

## How to present well

- **Lead with a recommendation.** Put the option you'd pick first and mark it; a
  choice with no point of view is a burden, not a service.
- **Make options genuinely distinct.** If two collapse to the same outcome, it's
  one option. State the *tradeoff*, not just the label.
- **Keep it small.** Two to four options per question, a handful of questions at
  most. The goal is calibration, not a survey.
- **Don't ask what you've been told.** If the human already decided, act on it.

The bar is a *genuine* fork. Over-asking trains the human to rubber-stamp, which
defeats the point — the same way a linter that nags on non-defects trains its
reader to ignore it. Ask when their answer changes the work; otherwise decide,
state it, and keep moving.

## See Also

- trust/autonomy(meta) — when to act without asking vs. check in first
- delivery/implement(softwaredev) — defend a plan, then invite challenge
