---
contract: adr/v1
kind: decision
verb: add
capability: install
extends: [ADR-184]
basis:
  - operator: aaronsb
    level: directed
    said: "let's add the ways uninstall tool to close the loop - let's publish this"
    via: chat, session 02e97f86, 2026-10-01, after a full remove and clean install on a second machine
  - operator: aaronsb
    level: directed
    said: "we should be able to use the agent-ways lifecycle tools ... I think"
    via: chat, session 02e97f86, 2026-10-01, while removing agent-ways from a machine by hand
  - precedent: ADR-184
  - precedent: ADR-142
agent:
  name: Claude
  model: claude-opus-5-5
status: accepted
date: 2026-10-01
deciders:
  - aaronsb
related: [ADR-144, ADR-500]
---

# ADR-198: Uninstall is a lifecycle command that keeps what the operator owns

## Summary

- **Decided:** `ways uninstall` removes agent-ways. It withdraws from every target and from `~/.claude`, stops the ways agent, removes the command links that point into the app, and deletes the app and every cache it has used. Without `--yes` it prints that plan and changes nothing. The operator's config and state stay unless `--purge` is given.
- **Trades away:** A single command that leaves nothing behind. Kept config and state outlive the app until the operator purges them.
- **One-way?** Running it with `--purge` is: it deletes the operator's ways and API keys. The default run is not; reinstalling restores everything it removed.
- **Probes:** *Confident (keep):* you would rather a plain uninstall keep your own ways and keys, and delete them only when you ask. *Not confident (state):* events and probe data count as yours to keep, not as app data to delete.
- **Inversion:** Between removing only the links, as `make uninstall` did, and deleting everything the app ever wrote. The decision removes all the app owns and keeps what the operator owns.

## Context

agent-ways had an install, an update, a repair (`ways reconcile`) and target withdrawal (`ways config target remove`, ADR-184), but no removal. `make uninstall` unlinked four commands. Taking agent-ways off a machine meant withdrawing the projection, then deleting the XDG directories, the links on `PATH` and caches from before the 1.0 rename by hand, and knowing which were the app's and which the operator's. A cache left from a pre-1.0 install changed the behaviour of the next clean install.

## Decision

1. **One command.** `ways uninstall` takes the machine back to before the install, apart from what the operator owns.
2. **Withdraw first.** It withdraws from every recorded target and from `~/.claude` the way a disabled target is withdrawn: our links, our hooks and permissions through the settings merge base (ADR-500), and our MCP entry. The installer projects into `~/.claude` whatever the target list says, so `~/.claude` is always included. Withdrawal leaves the config's target list unchanged, so a kept config activates the same targets on a later install. A failed withdrawal stops the command before anything is deleted.
3. **What is removed.** The command links in the user bin directory that point into the app, the app (`$XDG_DATA_HOME/agent-ways`), and every cache dir the app has used (`agent-ways`, and the pre-1.0 `claude-ways`). The ways agent is asked to stop.
4. **What is kept.** The operator's config (`$XDG_CONFIG_HOME/agent-ways`: their ways, API keys, settings) and state (`$XDG_STATE_HOME/agent-ways`: events, probe data). `--purge` deletes both.
5. **Plan first.** Without `--yes` the command lists each path under withdraw, unlink, delete or keep, and exits having changed nothing.

## Consequences

### Positive

- Removing agent-ways and reinstalling it are each one command, and a clean install can be demonstrated on a machine that had one.
- Nothing the operator wrote is lost by default.

### Negative

- A machine where the app was deleted by hand and the projection left in place cannot be withdrawn by this command, because withdrawal identifies our links by the app they point into. Reinstalling and then uninstalling clears it.
- Kept config and state are invisible clutter to an operator who expected everything gone; the plan lists them and names `--purge`.

### Neutral

- `make uninstall` remains the Makefile's unlink step for a source checkout.

## Alternatives Considered

- **Delete everything by default.** One command with no leftovers, but a plain uninstall would delete API keys and the operator's own ways. Rejected: deleting what the operator owns needs their explicit ask.
- **Withdraw through `ways config target remove`.** It drops the target from the config, and removing the last one writes an empty list, so a kept config would install inactive. Rejected for the disable-style withdrawal, which leaves the list as it was.
- **A shell script beside `install.sh`.** It would duplicate the withdrawal logic the binary already owns, and drift from it.
