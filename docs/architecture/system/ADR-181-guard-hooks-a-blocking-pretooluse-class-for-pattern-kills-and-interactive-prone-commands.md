---
status: Proposed
date: 2026-09-10
deciders:
  - aaronsb
  - claude
related:
  - ADR-162
  - ADR-178
---

# ADR-181: Guard hooks: a blocking PreToolUse class for pattern kills and interactive-prone commands

## Context

Every hook this project ships injects context and exits zero. `check-bash-pre.sh` scores a command against the ways corpus and prints guidance; the model reads it and decides. One hook departs from that: `strip-session-link-pre.sh` (ADR-162) exits 2 to refuse a commit that would publish a session link. It is the only hook that can stop a tool call, and nothing in the corpus names the class it belongs to.

The Cypress survey (`docs/design-notes/cypress-survey.md`, issue #465) proposed a second refusing hook, ported from a guard that blocks any shell command lacking an explicit `timeout` or a detached launch. That guard was written for harnesses that run foreground commands unbounded. This harness is different on three points, verified against the Bash tool's contract:

1. **Every foreground command is already bounded.** The Bash tool enforces a timeout, 120 seconds by default and 600 at most, and refuses a bare foreground `sleep`. A command that never returns burns its timeout and comes back with an error. Nothing hangs the session.
2. **Detached work has a first-class form.** `run_in_background: true` on the Bash tool, and the Monitor tool for watching a condition, replace the `setsid nohup ... & echo $! > pid` idiom the ported guard demanded. The hook sees `run_in_background` in `tool_input`.
3. **The corpus carries the liveness discipline as guidance.** The `softwaredev/environment/recovery` and `code/testing/gates` ways landed this week already say how to treat a command that returned without doing its work.

So the ported guard's central rule, "no bound, no run", is satisfied by the harness. What the harness leaves open is narrower:

- **Pattern kills.** `pkill`, `killall`, and `kill` with a name or pattern can match the shell issuing the kill, a subagent's process, or an unrelated process of the operator's. No timeout repairs that, and no injected guidance reliably stops a model that has decided a process is stuck.
- **Interactive-prone commands.** `sudo` without `-n`, `ssh` without `BatchMode`, `docker exec -it`, and package managers without their non-interactive flag sit on a prompt until the timeout, then return garbage. The cost is bounded and the outcome is always wrong.
- **Log followers and attached containers.** `tail -f`, `journalctl -f`, and `docker run` without `-d` in the foreground never return. They belong in the background.
- **Pipe to shell.** `curl ... | sh` runs whatever arrived. The supply-chain ways say to download, read, and then run; the guidance is advisory and this is the one place a refusal is cheap and exact.

The question this ADR settles is whether those four classes justify a second refusing hook, and what contract refusing hooks share.

## Decision

**Establish "guard hooks" as a named class, with a contract, and ship `check-bash-bound.py` as its second member.**

A guard hook:

1. **Refuses by exit 2 with the reason and the accepted form on stderr.** The model receives the reason as the tool result and chooses again. Nothing else in the session changes.
2. **Exits only 0 or 2.** Every internal failure path (stdin unparseable, tool not Bash, an exception) exits 0 with one line on stderr. A bug in a guard degrades to no guard; it never blocks everything.
3. **Is wired without `|| true`.** A guard that cannot block is decoration (the `code/security/guards` way). The asymmetry is paid inside the script by rule 2.
4. **Refuses a closed list, named in the script, each entry with its repair.** Extending the list is a one-line change and a lint-visible one; a guard has no semantic lane.
5. **Honors the harness's own forms.** A command sent with `run_in_background: true`, or carrying a `timeout` prefix in the same segment, is exempt from the never-returns classes. Pattern kills and pipe-to-shell have no exemption; their repair is a different command.

`check-bash-bound.py` refuses:

| Class | Refused | Exempt when | Repair offered |
|---|---|---|---|
| Pattern kill | `pkill`, `killall`, `kill -SIG <pattern>` | `kill` given a literal pid or `$(cat *.pid)` | kill by recorded or literal pid |
| Interactive prompt | `sudo`, `ssh`, `docker exec -it`, `apt`, `pacman`, `yay` | `sudo -n`, `-o BatchMode=yes`, `-y` or `--noconfirm`, no `-it` | add the non-interactive flag |
| Never returns | `tail -f`, `journalctl -f`, `docker run` (attached) | `run_in_background`, `timeout N` prefix, `docker run -d` | run it in the background |
| Pipe to shell | `... \| sh`, `... \| bash` | none | download to a file, read it, run the file |

Builds, installs, and test runs are **not** refused. They are bounded by the harness timeout, and the `softwaredev/environment/bounded-execution` way, which fires on the same command classes through the ordinary inject path, carries the liveness discipline: running is claimed only on an observed liveness signal, and completion is the marker, never the absence of output or a timeout.

The way and the guard are a pair. The way fires first through `check-bash-pre.sh` and teaches; the guard runs after it and refuses the four classes the teaching cannot be trusted to prevent.

## Consequences

### Positive

- A pattern kill that could take out the agent's own shell, a subagent, or an operator process is stopped before it runs, with the pid-based form handed back.
- Interactive prompts stop burning the full timeout and returning a misleading error.
- The class now has a name and a contract. A third guard, when one is justified, has a shape to follow and a place to be cited.
- The refused list is closed and visible in one file. Reviewing what the harness can stop is a read of that list.

### Negative

- A second hook runs before every Bash call. The script is regex over one string and exits in milliseconds; the cost is real and small.
- A closed list misses what it does not name. That is the design: a guard with a semantic lane would refuse on a guess.
- An operator who legitimately wants `pkill` in a session must run it outside the agent or add an exemption. The reason line says so.

### Neutral

- `strip-session-link-pre.sh` is retroactively a member of the class. Its contract already matches; no change is needed.
- The Cypress guard's `setsid nohup` detached form is accepted as an exemption for compatibility, and the way recommends `run_in_background` instead.
- The way's `commands:` trigger overlaps the guard's list on purpose, so the model has read the discipline before it meets the refusal.

## Alternatives Considered

- **Port the Cypress guard unchanged: refuse any command without an explicit bound.** Rejected. The harness already bounds every foreground command, so the rule would refuse `make`, `cargo build`, and `npm install` in their ordinary bounded form and teach the model to prefix `timeout` to everything, which adds nothing the harness does not already do.
- **Guidance only, no refusal.** Rejected for the four classes above. The recovery way already tells the model to classify a stuck process before acting, and a model under time pressure still reaches for `pkill`. A pattern kill is irreversible and the repair is exact, which is the profile that justifies a refusal over a hint.
- **Implement the guard inside the `ways` binary's `scan command` path.** Rejected for now. The scan path is an inject path with a semantic lane; the guard needs neither, and keeping it a separate script keeps the refused list readable without building Rust. Folding it in later is an ordinary refactor once a third guard exists.
- **Refuse pipe-to-shell only with a bound, as Cypress does.** Rejected. The hazard is provenance, and a `timeout` does nothing for it.
