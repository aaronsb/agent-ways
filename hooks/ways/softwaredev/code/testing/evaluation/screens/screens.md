---
description: a visual evaluation loop for terminal UI screens; snapshot each screen at the terminal sizes users run, reach each state with a scripted key sequence, review every golden image diff of a screen capture row by row, and exercise external commands through a stand-in runner
vocabulary: tui terminal ui screen screenshot snapshot capture pane frame golden image diff render layout 80x25 100x30 columns rows small terminal resize keys keypress keystroke key sequence modal popup overlay scroll wrap truncation clipped overflow stand-in runner fake command long output exit code visual tmux pty pseudo-terminal multiplexer truncated footer
pattern: \btui\b|\b(80|100|120|132)x(24|25|30|40|43|50)\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Screen Evaluation

A terminal screen is evaluated by looking at it in the states users reach, at the sizes users run. Run the program under a terminal multiplexer or a pseudo-terminal harness in a private session, send it keys, and capture the pane as text or as an image. When the project ships a harness or a snapshot skill for this, use it.

## Capture at the sizes users have

Capture each screen at a small terminal and at a common one, for example 80x25 and 100x30. Layout defects live at the edges: a line that wraps, a footer pushed off the bottom, a column truncated, a modal wider than the terminal. Add a size whenever a report names one.

## Reach each state with scripted keys

Script the key sequence that reaches each state under test and keep it beside the golden image. The script drives the real input loop, as the parent way's headless-driver rule requires.

## Review every golden diff row by row

Read each changed golden image or text capture row by row and name what moved in each changed row. The assertions way decides whether each change is accepted. Confirm the capture shows the state its name claims.

## Stand in for external commands

When a screen shows the output of an external command, run it against a stand-in runner that produces the edge case on purpose. A runner that prints 25 long lines and exits with status 3 exercises wrapping, scrolling, overflow, and the error path in one capture. Keep stand-ins in the test fixtures so each edge is reproducible.

## See Also

- code/testing/evaluation(softwaredev) — parent: reproduce as reported, drive the real path, re-review, count what ran
- code/testing/gates/assertions(softwaredev) — golden baselines, masked volatile leaves, and the enumerated-delta rule
- code/testing/mocking(softwaredev) — test doubles for the commands a stand-in runner replaces
