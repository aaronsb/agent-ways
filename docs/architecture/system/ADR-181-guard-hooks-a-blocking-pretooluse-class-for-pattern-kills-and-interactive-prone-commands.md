---
status: Accepted
date: 2026-09-10
deciders:
  - aaronsb
  - claude
related:
  - ADR-162
  - ADR-178
---

# ADR-181: Guard hooks: a blocking PreToolUse class, shipped deactivated

## Context

Every hook this project ships injects context and exits zero. One departs from that: `strip-session-link-pre.sh` (ADR-162) exits 2 to refuse a commit that would publish a session link. It is the only hook that can stop a tool call, and nothing in the corpus names the class it belongs to.

The Cypress survey (`docs/design-notes/cypress-survey.md`, issue #465) proposed a second refusing hook, ported from a guard that blocks any shell command lacking an explicit `timeout` or a detached launch. That guard was written for harnesses that run foreground commands unbounded. This harness is different on three points:

1. **Every foreground command is already bounded.** The Bash tool enforces a timeout, 120 seconds by default and 600 at most, and refuses a bare foreground `sleep`.
2. **Detached work has a first-class form.** `run_in_background: true` on the Bash tool, and the Monitor tool for watching a condition. The hook sees `run_in_background` in `tool_input`.
3. **The permission system already gates Bash** by command prefix, and the operator tunes that list per project.

What the harness leaves open is narrower: pattern kills (`pkill`, `killall`, `kill` by name) that can match the agent's own shell or an operator process; interactive-prone commands (`sudo` without `-n`, `ssh` without `BatchMode`, `docker exec -it`, package managers without their flag) that sit on a prompt until the timeout; log followers and attached containers that never return in the foreground; and pipe-to-shell.

The operator's read: Bash execution is already guarded enough, and another refusal wired in by default is unwanted.

## Decision

**Establish guard hooks as a named class with a contract. Ship `check-bash-bound.py` as a member, deactivated.**

A guard hook:

1. Refuses by exit 2 with the reason and the accepted form on stderr. The model receives the reason as the tool result and chooses again.
2. Exits only 0 or 2. Every internal failure path exits 0 with one line on stderr. A bug in a guard degrades to no guard.
3. Is wired without `|| true` when it is wired at all.
4. Refuses a closed list named in the script, each entry with its repair. A guard has no semantic lane.
5. Honors the harness's own forms: `run_in_background` and a `timeout` prefix exempt the never-returns class.

`check-bash-bound.py` refuses the four classes above and lives at `hooks/ways/check-bash-bound.py` with its verdict test at `tests/test-bash-bound.sh`, which runs in the suite. **It is not wired in `settings.json`.** An operator who wants it adds one entry under `hooks.PreToolUse` for matcher `Bash`:

```json
{ "type": "command", "command": "${HOME}/.claude/hooks/ways/check-bash-bound.py" }
```

**Bounded execution is guidance.** The `softwaredev/environment/bounded-execution` way fires on the same command classes through the ordinary inject path and carries the discipline: background the never-returning command, give the interactive one its flag, kill by pid, download an installer before running it, and claim running only on a liveness signal.

Activating the guard by default needs its own ADR, and the bar is ADR-162's: an irreversible or disclosing outcome that injected guidance has been observed to fail to prevent.

## Consequences

### Positive

- Bash stays as permissive as the harness and the operator's permission list make it. No hook runs in front of every shell call unless the operator asks.
- The class has a name, a contract, and a tested reference member. A project that wants the refusal gets it with one settings line.
- The discipline still reaches the model at the moment it is about to run one of those commands.

### Negative

- A pattern kill or a pipe-to-shell that the model decides on despite the guidance runs. The harness timeout and the permission prompt are the remaining stops.
- A deactivated script drifts. Its test runs in the suite, which keeps it working; whether its refused list still matches the harness is checked only when someone activates it.

### Neutral

- `strip-session-link-pre.sh` is retroactively a member of the class. Its contract already matches.
- The Makefile marks `.py` hooks executable alongside `.sh`, so the opt-in path works after `make install`.
- The `code/security/guards` way, which says a guard that cannot block is decoration, describes controls a project ships in its own code. It does not argue for wiring this one.

## Alternatives Considered

- **Wire the guard by default (the first draft of this ADR).** Rejected by the operator: Bash execution is already guarded enough.
- **Withdraw the guard entirely.** Rejected in favor of shipping it deactivated: the port and its forty-case test exist, and a project with a different risk posture can opt in without re-deriving them.
- **Port the Cypress guard unchanged, refusing any command without an explicit bound.** Rejected. The harness already bounds foreground commands; the rule would refuse ordinary builds and installs.
- **Fold the guard into the `ways` binary's scan path.** Rejected for now; the scan path is an inject path with a semantic lane, and a deactivated script is easier to read and to opt into.
