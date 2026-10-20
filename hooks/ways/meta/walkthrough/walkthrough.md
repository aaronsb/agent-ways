---
description: Guiding the user through manual steps they must do themselves in a web console, admin UI, settings form, or runbook. Give the exact URL and click path, one step per turn, check their screenshot, then give the next step.
vocabulary: walk me through guide me step by step one step at a time turn by turn console dashboard admin settings form screenshot click field portal manual setup oauth app token rotate runbook checklist live event
pattern: turn.by.turn|one (step|turn)[^.]{0,15}(at a time|per turn)|guide me (through|thru)|walk me (through|thru) step.by.step|walk me (through|thru) (this|the|each) (form|setup|console|dashboard|config|screen|steps?)
refire: 0.15
scope: agent
---
<!-- epistemic: convention -->
# One Step Per Turn

## First, does a human need to do it

If a CLI, an API, or browser automation can do the step from this session, do it yourself. Do not ask the user to run a command you can run. If the console fights you, offer the CLI route.

## Overview or click-through

- If the user asks for an overview ("walk me through what needs doing, with links"), a numbered list with direct links is fine.
- Once the user is clicking, switch to one step per turn.

## Each turn

1. Give one action. Include the exact URL, the click path, and what the user should see when it works. The first step is usually opening the right page.
2. Read their screenshot. Confirm it matches what you expected, such as the right account and resource, and name any mismatch.
3. Answer any question about a field.
4. Give the single next action. Stop.

Never jump ahead, even if the user says they have all the values ready.

## When the console differs

If the screen does not match your steps, adapt the steps. Consoles change often, so do not insist. Before a risky toggle, ask for a screenshot or take a browser snapshot, and say what to check in it.

## Runbooks and live events

- Open by listing completed and pending steps in execution order.
- Run exactly one step per turn, on the user's command. Never run a step without permission.
- Pause dependent work while a manual step is in progress.
- Explain each item plainly and resolve it together before moving on.
- When the user sets a boundary ("then wait for my go"), stop at it.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "Pasting all ten steps saves time" | Give step one and wait for the screenshot. |
| "They can run this in their terminal" | Run it yourself. |
| "The button is definitely there" | Ask what they see and adjust. |
| "The step passed, so I'll run the next runbook item" | Report and wait for the command. |

## Afterwards

Save the procedure as a runbook in the repo so it can be automated next time. See runbooks(itops). Memory is the wrong home for a procedure.

## Not this way

Explaining what scripts or components do and how they fit together is a chat explanation, not a click-through.

## See Also

- runbooks(itops) — where the saved procedure goes
- trust/delegation(meta) — boundaries when acting in the user's accounts
