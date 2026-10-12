---
description: verifying a change to a terminal UI screen in this repo (attend-chat, agent-tui, the ways settings, replay and projects screens) by driving it with tui-harness, taking screenshots at fixed sizes, and reading each one before the PR
vocabulary: tui screen screenshot shot png tui-harness tmux scenario verify keys render layout sidebar pane tab picker attend-chat agent-tui settings replay projects 80x25 120x35
files: tools/(attend-chat|agent-tui)/|tools/ways-cli/src/cmd/(introspect|projects|settings|screen_host)
scope: agent, subagent
refire: 0.25
---
<!-- epistemic: convention -->
# TUI Verification

After changing a TUI, drive it and look at it before the PR.

1. Build the harness and the binaries: `cargo build --manifest-path tools/Cargo.toml -p tui-harness -p ways -p ways-agent`, plus `-p attend-chat` for that TUI.
2. Run the TUI's scenario script. It reaches each screen state with real keys and clicks at 80x25 and 120x35, waits on what the screen shows rather than on the clock, and writes a PNG and a text capture per state under `target/tui-verify/<tui>/<cols>x<rows>/`.

   | TUI | Script |
   |---|---|
   | `ways` settings, replay, projects | `tools/ways-cli/shots/verify.sh` |
   | `attend-chat` column, menus, paste | `tools/attend-chat/shots/shots.sh` |

3. Read each PNG of a state the change touches. Say what it shows: the layout, what is clipped or wrapped at 80x25, colour and focus, and anything that differs from the intent.
4. List the shots you looked at in the PR description, with what each showed.

A new screen state adds a scenario to the script's list in the same change. `cargo test -p ways --test tui_smoke` checks key states of the `ways` screens in CI; extend it when a state is load-bearing.

## Provisional golden frames

While a screen's layout is in a rapid iteration phase (code/testing/gates/assertions), list its unsettled frames in a committed `PROVISIONAL` file in the golden directory, one exact frame name per line, size suffix included. A listed frame is never asserted and never blessed, in any run, CI included. A drift or a missing golden prints one note. Settled frames are always asserted. A listed name with no `.frame` file that no check reaches prints a stale-entry warning. Once the operator accepts the layout, delete the frame's line, bless it, and review it. `tools/attend-chat/tests/golden/PROVISIONAL` is the current list (ADR-407).
