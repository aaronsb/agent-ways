# `attend chat` — human mode

This page describes the interactive TUI that places a human user inside the same signal protocol the AI agents use. It's written as if the feature exists today; the underlying design is captured in **ADR-120** (status: draft — implementation follows).

The operating premise: *humans wear the same clothes as an AI agent* as far as attend is concerned. Same signal bus, same routing, same engagement rules. The only difference is that the human drives through a terminal UI instead of through a language-model context window.

## Why it exists

Attend's signal layer solves a coordination problem for multi-agent sessions. The human coordinator used to have no first-person view of that layer — they could fire `attend send --broadcast` blindly from a bare terminal, or snapshot `attend inbox`, or alt-tab between four Claude sessions one at a time. None of those let the human *participate* in the signal topology. Routing errors (agents sending `--to /home/aaron/.claude` when they meant `--broadcast`) were a symptom of an invisible abstraction. Coordination across more than two agents was essentially guesswork.

`attend chat` solves this by giving the human a first-class seat at the signal bus. The same broadcast an agent sees, the human sees. The same `@focus-group` addressing an agent uses, the human uses. The human is not watching attend from outside — they *are* one of the endpoints.

Two secondary consequences fall out of this:

1. **The routing protocol gets dogfooded.** Any friction the human feels — wrong defaults, confusing scope, bad message framing — is friction the agents also feel. Design improvements surface faster when humans are first-person users.
2. **Solo developers get value too.** The TUI is not only for four-agent orchestration. A single developer running one Claude session can launch `attend chat` and watch environmental signals (git state, GitHub Project moves, build finishes) in one pane while the agent works in another. It's calm technology (Weiser & Brown) applied to a terminal pane — a peripheral-vision surface that informs without competing with the agent's terminal for the center of attention.

## Invocation

```bash
attend chat                           # open the TUI in the current project's focus
attend chat --focus @infra            # open pre-scoped to a specific focus group
attend chat --broadcast               # open with broadcast as the default filter
```

Like `attend run`, `attend chat` is a long-lived process. Unlike `attend run`, it expects to live in a foreground terminal where the human can see and type. You can run both at the same time — they share the same signals base on disk, so `attend chat` in one terminal and `attend run` in a Claude session is the intended multi-endpoint configuration.

## Layout

```
┌─────────────┬──────────────────────────────────────┐
│  Focus      │  Messages (chronological stream)      │
│             │                                        │
│ ● broadcast │  claude/api  @deploy                   │
│   @deploy   │  Landed the auth refactor, ready for   │
│   @infra    │  review.                               │
│   project/… │                                        │
│             │  claude/infra  @deploy                  │
│─────────────│  Pulling in the new auth types now.     │
│  Agents     │                                        │
│             │  aaron  broadcast                       │
│  api ↑2 73% │  @infra hold off until api merges.     │
│  infra · 91%│                                        │
│  docs · 45% │                                        │
│  slack ↑1 88│                                        │
│             ├──────────────────────────────────────┤
│             │ > @deploy looks good, ship it_        │
└─────────────┴──────────────────────────────────────┘
```

**Left sidebar, top — focus groups.** Every group the current session can see is listed. A filled dot (`●`) indicates unread activity in that group. Click or tab to filter the message stream to that group; the filter is local to the TUI and doesn't change the session's actual focus membership.

**Left sidebar, bottom — agents.** Every peer Claude session attend has discovered is listed with ambient metadata: a status indicator (arrow = working, dot = idle), commits ahead/behind upstream, and current context percentage. This is the peer sensor's output rendered directly.

**Main area — messages.** Chronological signal stream, scoped by whichever sidebar filter is active. Each message shows the sender in the persona-plus-project form every attend conduit uses (`Jovan-alpha (virtual-workspace)` for a Claude session, `aaron (project)` for a human), the routing scope (`@focus-group`, `broadcast`, or project name), and the body. Messages fade after their salience drops below the presentation floor (ADR-121), matching the agent's view.

**Input bar — compose.** Plain text input supports `@group` and `#issue` inline addressing. Enter sends.

## Inline addressing

The input bar parses two forms of inline reference:

- **`@name ...`** — prefixing a message with `@<group-name>` routes the signal to that focus group's directory only. The human doesn't need to be a member of that group to send; attend writes the signal into the group's path and any agent (or human) subscribed to that group will see it.
- **`#NNN`** — a repo-local issue reference. The TUI itself doesn't resolve `#172` against GitHub. It's a marker for the receiving agent — when an agent reads `@api look at #172`, it can interpret `#172` against its own repo via `gh issue view 172` in its own working directory. This keeps the TUI free of GitHub dependencies and lets each agent resolve references in its own context.

**Default routing: broadcast.** Messages without an `@` prefix go to `_broadcast`, reaching every peer. This is the *inverse* of the agent default (agents default to project-scoped), because the human is operating as the coordination layer — most human messages should be visible to the whole team. Agents defaulting to project-scoped is right because most agent work is local to one project; humans defaulting to broadcast is right because most human work is cross-agent steering.

## Signal protocol extensions

The TUI extends the signal format with an optional threading field. The old format was:

```
from|project|cwd|message
```

With threading, the format becomes:

```
from|project|cwd|re:signal-id|message
```

The `re:` field is empty for new threads and carries the original signal's ID for replies. Replies are one level deep — no nested threads, no reply-to-reply recursion. The TUI draws a thin thread line between an original message and its direct replies; agents can use the field to maintain short-term conversational context when replying.

Example sequence:

```
claude/api  broadcast
Auth refactor ready for review. [id: abc123]

aaron  @deploy
@deploy  deploy to staging when you're ready. [re: abc123]

claude/deploy  @deploy
On it. [re: abc123]
```

The thread indicator tells the reader these three messages are part of one logical exchange even though they span different senders and groups.

## What the TUI does NOT do

By deliberate scope constraint:

- **It does not replace Claude sessions.** Deep work — editing code, reading long outputs, multi-step tool use — happens in each agent's own terminal. The TUI is peripheral vision; the agent terminals are foveal vision. Switching between them is expected and correct.
- **It does not fetch GitHub data.** `#issue` references are resolved by the receiving agent, not by the TUI. This keeps the TUI's dependencies minimal and avoids duplicating what the agents can already do.
- **It does not manage agent lifecycle.** Starting, stopping, or restarting agents is outside scope. The TUI observes and participates in signals; session management is a separate concern (tmux, shell, whatever fits your workflow).
- **It does not provide an editor.** No file browser, no buffer switching, no inline code edit. If you need to edit, switch to the agent's terminal or your own editor.

## The conversation interface

The TUI is the first-class conversation interface for enrolled Claude agents. "Enrolled" means any agent session that's running `attend run` in its own terminal — it automatically participates in the signal bus, so the TUI will see its messages and the agent will see yours. Enrollment is implicit: run attend, you're enrolled.

The practical shape of a coordinated session:

- 3–4 Konsole windows, each running a Claude Code session with `attend run` active
- A fifth terminal running `attend chat`
- Maybe a browser tab open to GitHub Projects for when you need to actually edit a card
- Maybe a sixth terminal for git, `htop`, or whatever other reference state you need

The human sits in `attend chat` to watch the whole signal layer. They steer with broadcast messages for "everyone hold off, api needs to land first" type directives and with `@group` messages for narrower scope. When deep work on one agent is needed, the human alt-tabs to that agent's terminal and types directly. When they're done, they come back to the chat surface.

The chat TUI is where the human *orchestrates*. The agent terminals are where the human *contributes*. These are different activities and deserve different surfaces.

## Related

- **ADR-120** — the design decision for this feature (draft status)
- **ADR-118** — focus groups; the TUI's sidebar depends directly on this model
- **ADR-119** — action potential engagement; the TUI's message stream respects the same refractory and governor rules as the agent side
- **ADR-121** — salience decay; the TUI fades old messages using the same turn-based curve
- [`loop.md`](loop.md) — the sensor loop substrate that feeds the TUI
- [`signals.md`](signals.md) *(planned)* — the wire format the TUI reads and writes
- [`focus-groups.md`](focus-groups.md) *(planned)* — the group model the sidebar reflects
