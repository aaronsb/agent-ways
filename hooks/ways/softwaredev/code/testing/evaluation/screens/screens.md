---
description: a visual evaluation loop for terminal UI screens; capture each screen at the terminal sizes users run, reach each state with a scripted key sequence, review every golden image diff row by row, and exercise external commands through a stand-in runner
vocabulary: tui terminal ui screen screenshot snapshot capture pane frame golden image diff render layout 80x25 100x30 columns rows small terminal resize keys keypress keystroke key sequence modal popup overlay scroll wrap truncation clipped overflow stand-in runner fake command long output exit code visual
pattern: \btui\b|\b\d{2,3}x\d{2,3}\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Screen Evaluation

A terminal screen is evaluated by looking at it in the states users reach, at the sizes users run. Run the program under a terminal multiplexer or a pseudo-terminal harness in a private session, send it keys, and capture the pane as text or as an image. When the project ships a harness or a snapshot skill for this, use it.

## Capture at the sizes users have

Capture each screen at a small terminal and at a common one, for example 80x25 and 100x30. Layout defects live at the edges: a line that wraps, a footer pushed off the bottom, a column truncated, a modal wider than the terminal. Add a size whenever a report names one.

## Reach each state with scripted keys

Script the key sequence that reaches each state under test and keep it beside the golden image. The script drives the real input loop, so the program draws and ticks between keys as it does for a user. A state reached by setting fields directly can show a frame the user never sees.

## Review every golden diff row by row

Read each changed golden image or text capture row by row and name what moved in each changed row. Every change matches an intended delta, or it is a defect to explain before the golden is accepted. Re-recording a golden to turn the check green throws the review away. A golden captured from the wrong frame approves the wrong frame, so confirm the capture shows the state its name claims.

## Stand in for external commands

When a screen shows the output of an external command, run it against a stand-in runner that produces the edge case on purpose. A runner that prints 25 long lines and exits with status 3 exercises wrapping, scrolling, overflow, and the error path in one capture. Keep stand-ins in the test fixtures so each edge is reproducible.

## See Also

- code/testing/evaluation(softwaredev) — parent: reproduce as reported, drive the real path, re-review, count what ran
- code/testing/gates/assertions(softwaredev) — golden baselines, masked volatile leaves, and the enumerated-delta rule
- code/testing/mocking(softwaredev) — test doubles for the commands a stand-in runner replaces
