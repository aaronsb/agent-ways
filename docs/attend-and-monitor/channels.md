# Channels

A channel is a named message scope that sessions join and leave while they run. Three agents on one deploy join `#deploy` and talk there, and sessions working on unrelated things never see that traffic. Channels were introduced as focus groups (ADR-118). ADR-173 renamed them channels and gave them chat verbs, and ADR-170 moved the implementation into the `attend-groups` crate that `attend` and attend-chat share.

## The three scopes

| Scope | Directory under the signals base | Who receives it | How to send |
|---|---|---|---|
| `#open` | `_broadcast/` | every enrolled session | `attend send "…"` |
| a channel | `@<name>/` | the channel's members | `attend send --channel <name> "…"` |
| a project | the project's tray | the sessions in that project | `attend send --to <path> "…"` |

`#open` is the base channel. Every session with attend running is in it, and it cannot be joined, left or dissolved. A send with no flag goes there. In attend-chat the same scopes are `#name`, `@Name` and the foreground tab (see [`tui.md`](tui.md#addressing)).

A send to a channel with no live member other than the sender fails with `error: no live peers in channel '<name>'`, so a message is never left in a room nobody reads.

## Commands

```bash
attend join deploy                 # join, creating the channel if it does not exist
attend join infra --pin            # join and pin, so the channel survives empty
attend leave deploy                # leave; an unpinned channel left empty is removed
attend scene private               # leave every channel

attend channels                    # list channels with member counts, joined ones marked
attend channels --joined           # only the channels this session is in
attend channels create review "PR review rota"   # create and pin, without joining
attend channels describe review "PR review rota, mornings"
attend channels pin deploy
attend channels unpin deploy       # an empty channel is removed when unpinned
attend dissolve deploy             # remove the channel for every member
```

`attend peers` lists live sessions and the channels each is in. The full flag reference is [`../cli/attend.md`](../cli/attend.md). attend-chat has the same lifecycle as slash commands (`/join`, `/leave`, `/channels`, `/invite`, `/kick`, `/dissolve`, `/purge`); see [`tui.md`](tui.md#slash-commands).

**Pinning.** An unpinned channel is removed when its last member leaves. A pinned one keeps its entry and directory with no members, so a session joining later finds it and its history. `attend channels create` pins the channel it creates.

**Dissolving** removes the channel's entry and its `@<name>/` directory, history included, and prints how many members it released. No message is sent to the members; their next scan simply has no such directory. attend-chat's `/dissolve` refuses while any member is live; `attend dissolve` does not check.

**Names** are what `validate_group_name` accepts: not empty, not starting with `_` or `@`, and with no `/`, space, `#` or `:`. `broadcast` and `open` are reserved. The `@` on disk and the `#` in chat are added by attend.

## On disk

Membership lives in `_groups.yaml` at the root of the signals base (`$XDG_CACHE_HOME/attend/signals/`):

```yaml
deploy:
  pinned: true
  description: rollout coordination
  members:
    - 0b6f3d2e-…-session-id
    - aaron
infra:
  pinned: false
  members:
    - 7c41a9e0-…-session-id
```

A member is a Claude session id or a human's username. The file is written by renaming a temporary file into place, so concurrent joins and leaves from several sessions do not corrupt it.

Membership is self-reported: a session adds and removes itself, and nothing verifies it. Attend assumes cooperative peers throughout.

## Stale members

A member is live while its heartbeat in attend's cache is fresh (90 seconds, ADR-129). `attend run` touches a session's heartbeat on every tick, the Stop-hook drain at every turn end, and attend-chat touches the human's while it is open. `attend channels` prunes members whose heartbeat is stale, and removes any unpinned channel that pruning empties. It also removes an `@<name>/` directory that `_groups.yaml` has no entry for, once the directory is older than a grace window.

## How joined channels are read

The peers sensor reads `_groups.yaml` on every poll, so a join or leave takes effect on the next scan without restarting `attend run`. Each poll scans the session's project tray, `_broadcast/`, and every joined `@<name>/`. The Stop-hook drain scans the same set. Channel messages are authored messages: they ride the message lane, with no refractory or salience gate (ADR-136). See [`delivery.md`](delivery.md).

Leaving stops the scan of that directory. The signals stay on disk for the remaining members, unless the leave empties an unpinned channel, which removes the directory.

## Related

- [`tui.md`](tui.md) — channels as tabs, and the slash commands
- [`signals.md`](signals.md) — the signal files and when they are removed
- [`delivery.md`](delivery.md) — how a channel message reaches a session
- **ADR-118** — named channels (introduced as focus groups)
- **ADR-170** — the shared `attend-groups` crate and human membership
- **ADR-173** — the channel vocabulary and chat verbs
- `tools/attend-groups/src/lib.rs` — `Groups::join`, `leave`, `kick`, `pin`, `unpin`, `dissolve`, `cleanup_stale_with`
