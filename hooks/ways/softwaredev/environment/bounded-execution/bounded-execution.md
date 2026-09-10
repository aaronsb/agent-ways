---
description: running long or interactive shell commands from an agent; use run_in_background for anything that follows a log or serves, give sudo, ssh, and package managers their non-interactive flag, stop a process by its pid and never by a pattern, claim running only on a liveness signal
vocabulary: long running command hang hangs hung stuck process background run_in_background nohup setsid timeout tail -f follow logs docker run docker exec sudo ssh apt pacman install build make cargo build npm install kill pkill killall pid pidfile liveness still running completed
commands: \b(pkill|killall|kill\s+-|sudo|ssh\s|journalctl|tail\s+-f|docker\s+(run|exec)|apt(-get)?\s+install|pacman\s+-S|npm\s+(install|ci)|pip3?\s+install|cargo\s+build|make)\b
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Bounded Execution

The Bash tool bounds every foreground command by a timeout and refuses a bare `sleep`. Within that bound four shapes still go wrong: a command that sits on a prompt, a command that never returns, a kill that matches the wrong process, and a launch that returned without the work being done. The guard hook (ADR-181) refuses the first three outright. This way covers how to run them instead, and how to read the fourth.

## Pick the form before you run

| The command | Run it as |
|---|---|
| Follows a log or serves a port (`tail -f`, `journalctl -f`, a dev server, `docker run` attached) | `run_in_background: true` on the Bash tool, then read its output file; or `docker run -d` |
| Builds, installs, test suites | Foreground, under the tool timeout; raise the timeout parameter for a known long build rather than backgrounding it |
| Asks for input (`sudo`, `ssh`, `docker exec -it`, package managers) | The non-interactive flag: `sudo -n`, `ssh -o BatchMode=yes`, `docker exec` without `-it`, `apt -y`, `pacman --noconfirm`. If the flag makes it fail, the credential or config is the problem, and the report says so. |
| Waits for a condition (a port to open, a file to appear, CI to finish) | The Monitor tool, or a bounded poll with `timeout` |
| Downloads and runs an installer | Download to a file, read it, run the file |

The `setsid nohup ... &` idiom from other harnesses works here and is accepted by the guard. Prefer `run_in_background`, which the harness tracks and reports on.

## Stop a process by its pid

`pkill` and `killall` match by name or pattern, and the pattern can match this shell, a subagent, or a process of the operator's. Find the pid first (`pgrep -f <pattern>` and read the list), confirm it is yours, then `kill -TERM <pid>`. A process you launched in the background has a pid the harness recorded, or one you wrote to a pidfile.

## Claim running only on a liveness signal

A launch that returned proves the launch. Before reporting a server, watcher, or job as up, observe it: a port answering, a log line after the launch, a pid that is still present a few seconds later. Before reporting a job as done, find its completion marker: the artifact it writes, its exit status, the last line its log is known to print. The absence of output is not completion, and a timeout is not completion. A command that exited zero having done nothing is recorded as a failed attempt (see environment/recovery).

## When the tool timeout fires

The harness killed a foreground command at its bound. Read what it produced before the kill. If the work was a build or install, it may be half applied: check the lockfile, the `node_modules` or `target` state, and rerun with a longer timeout parameter rather than backgrounding a command that expects a terminal. If the command was waiting on input, that is the prompt class above.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I'll just pkill it, it's obviously mine" | The pattern decides what is yours. `pgrep -f` first, then kill the pid. |
| "It returned, so it's running" | A launch returning proves the launch. Look for the port, the log line, the pid. |
| "No errors in the output, so it finished" | No output is what a stalled job prints. Find the completion marker. |
| "sudo will use the cached credential" | Then `sudo -n` succeeds. Without `-n` a missing cache burns the timeout. |
| "The timeout is annoying, I'll background the build" | A backgrounded build fails silently. Raise the timeout parameter instead. |

## See Also

- environment/recovery(softwaredev) — a no-progress attempt is a failed attempt; classify before retrying
- environment/container-safety(softwaredev) — the container the attached run would hold
- environment/ssh(softwaredev) — batch mode and key setup for remote shells
- code/security/guards(softwaredev) — why the guard hook refuses rather than warns
