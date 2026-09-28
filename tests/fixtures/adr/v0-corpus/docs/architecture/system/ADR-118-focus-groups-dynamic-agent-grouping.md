---
status: Accepted
date: 2026-04-10
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-115
  - ADR-119
  - ADR-120
---

# ADR-118: Focus Groups — Dynamic Agent Grouping

## Context

attend's original grouping model had focus groups implemented as static path lists — `attend focus add ~/Projects/foo` required knowing the exact filesystem path of a peer. This was brittle: paths are implementation details, groups didn't self-clean, and there was no dynamic middle ground between "just me" and "everyone."

The underlying signal model already worked through named directories. A project's signals live in a directory named by its encoded path. Broadcast signals live in `_broadcast/`. The infrastructure for named signal namespaces existed — it just wasn't exposed as the user-facing concept.

The fix was to rebuild focus groups as **named signal namespaces** rather than path lists. The name "focus groups" was kept because the concept is correct — agents focus on named groups to coordinate, and release focus when done. The implementation changed; the vocabulary didn't.

### Home Assistant Analogy

Home Assistant scenes configure groups of devices into named states — "movie night" dims lights and turns on the TV. Scenes are declarative, activatable, and composable. attend's scenes are the equivalent: named configurations of focus group membership and attention profiles.

## Decision

Focus groups are **named signal namespaces** that agents join and leave dynamically. They are the universal grouping mechanism alongside the implicit project scope and broadcast.

### Focus Group Model

Every agent is always in one **implicit group** — its project path, auto-assigned from cwd. This is the project scope.

Agents can also join any number of **named focus groups**. A focus group is any string — `deploy`, `infra`, `code-review`. It doesn't need to map to a filesystem path.

```
attend focus on deploy             # focus on a named group
attend focus off deploy            # release focus
attend focus list                  # show groups you're focused on
attend focus all                   # show all active groups and their members
attend focus clear                 # release all named groups (project-only mode)
```

### Signal Routing

Signals are routed to focus groups, not paths. When an agent sends a message:

- `attend send "msg"` — sends to project scope + joined focus groups
- `attend send --focus deploy "msg"` — sends to a named focus group
- `attend send --broadcast "msg"` — sends to broadcast (all agents, human↔agent channel)

When an agent receives, it sees signals from:
- Its own project scope (always)
- Any named focus groups it has joined
- Broadcast (always)

```
# Agent A (in ~/Projects/foo):
attend focus on collab

# Agent B (in ~/Projects/bar):
attend focus on collab

# Now A and B see each other through "collab", without knowing paths
```

### Focus Group Lifecycle

- **Creation**: implicit — focusing on a group that doesn't exist creates it
- **Ephemeral** (default): a group with no members is cleaned up on the next peer sensor poll
- **Pinned**: `attend focus on deploy --pin` marks a group to persist even when empty. Useful for standing workgroups that agents rejoin across sessions. `attend focus unpin deploy` reverses it.
- **Dissolution**: `attend focus dissolve deploy` removes the group and notifies all members

### Scenes

A scene is a named preset that configures focus group membership and attention profiles:

```yaml
# ~/.config/attend/scenes.yaml
private:
  groups: []              # release all focus groups, project scope only

workroom:
  groups: [deploy]        # focus on just this group

open:
  groups: ["*"]           # join all discoverable groups
```

```
attend scene private       # activate a scene
attend scene open
attend scenes              # list available scenes
```

Scenes are sugar over focus on/off. `attend scene private` is equivalent to releasing all named groups. `attend scene open` joins a well-known shared group.

### Scoped Attention Profiles

Scenes configure more than group membership. A scene sets two **attention scopes** that mirror Claude Code's own permission model:

- **Project scope** — sensors and governor settings for local repo work (git changes, context usage, process detection)
- **Focus scope** — sensors and governor settings for coordination participation (peer signals, response cadence)

```yaml
# ~/.config/attend/scenes.yaml
deep-work:
  groups: []
  project:
    sensors: [git, context]
    governor:
      base_cooldown: 30
  focus:
    sensors: []                  # no peer sensor — no signals arrive
    governor:
      base_cooldown: 60

coordinate:
  groups: [deploy, infra]
  project:
    sensors: [git, context]
    governor:
      base_cooldown: 45          # slower project disclosure during coordination
  focus:
    sensors: [peers]
    governor:
      base_cooldown: 10          # fast response to peer signals

private:
  groups: []
  project:
    sensors: [git, context]
  focus:
    sensors: []
```

The two scopes run simultaneously with independent governors. A git change in your repo discloses on the project cadence. A peer signal in `@deploy` discloses on the focus cadence. This prevents cross-contamination — focus group coordination doesn't interrupt git operations, and local file churn doesn't drown out peer signals.

**Scope inheritance**: scenes without explicit scope blocks inherit the global config defaults. A minimal scene like `private: groups: []` still works — it just uses the global governor for both scopes and disables focus sensors implicitly by having no groups.

**Why two scopes, not per-sensor config**: individual sensor tuning is already possible in `attend.yaml`. Scopes are coarser — they separate the *kind of attention* (local work vs. coordination) rather than tweaking individual sensors. An agent in `deep-work` shouldn't see peer signals at all, not just see them slower. An agent in `coordinate` should respond to peers quickly but is still doing project work — it needs both attention modes running with different parameters.

**Session persistence**: scenes are saved to `scenes.yaml` and the active scene is tracked per project in `_groups.yaml`. When an agent restarts in the same project directory, attend can auto-activate the last scene. This means working groups survive session boundaries — a `coordinate` scene with groups `[deploy, infra]` persists across agent restarts without the human reconfiguring. Combined with pinned groups (which persist even when empty), a set of related projects maintains its coordination topology across sessions. The human sets up the working group once; it reconstitutes each time.

**Project-scoped scene overrides**: `scenes.yaml` supports both user scope (`~/.config/attend/scenes.yaml`) and project scope (`.attend/scenes.yaml` in a repo). Project-scoped scenes overlay user-scoped ones, so a repo can ship a default scene that configures the right focus groups and attention profile for agents working in that project. This means related repos can pre-declare their coordination topology.

**Chat TUI visibility** (ADR-120): the TUI sidebar shows each agent's current scene and scope state. You can see at a glance that `api: deep-work` means focus scope is empty (won't see your `@deploy` message) while `infra: coordinate` has focus scope active (will respond). This informs the human's steering decisions — you know when to send a broadcast (reaches everyone regardless of scene) vs. a focus message (only reaches agents focused on that group).

### Unified View

`attend peers` and `attend status` merge into a single view organized by focus groups:

```
$ attend peers
  Focus         Agent                  Status   Context
  ────────────────────────────────────────────────────────
  (project)     agent-ways             working  45%
  deploy        api-server             waiting  12%
  deploy        infra-tools            working  30%
  (broadcast)   game-ai-pro            waiting  14%
```

`attend status` becomes a self-view (your groups, your signals, your config) rather than a separate system view.

### Storage

Focus groups are directories under the existing signals base:

```
~/.cache/attend/signals/
  -home-aaron--claude/          # project scope (existing, unchanged)
  _broadcast/                   # broadcast (existing, unchanged)
  @deploy/                      # named focus group (@ prefix distinguishes from encoded paths)
  @collab/                      # another named focus group
  _groups.yaml                  # group membership + pinned state + active scene
```

The `@` prefix prevents collision between named groups and encoded project paths. `_groups.yaml` tracks which groups each session has joined and which are pinned.

## UX Flows

### Flow 1: Solo work (default, no action needed)

Agent starts. It's in its project scope automatically. No peers, no noise.

```
$ attend peers
  Focus             Agent          Status   Context
  ──────────────────────────────────────────────────
  agent-ways        (you)          working  12%

  1 agent, 1 group
```

### Flow 2: Ad-hoc collaboration

Aaron spins up two agents and wants them to coordinate.

```
# In agent A's session (agent-ways):
$ attend focus on deploy

# In agent B's session (api-server):
$ attend focus on deploy

# Now both see each other:
$ attend peers
  Focus             Agent          Status   Context
  ──────────────────────────────────────────────────
  agent-ways        (you)          working  12%
  deploy            api-server     waiting  8%

  2 agents, 2 groups
```

Signals flow through the group:
```
# Agent A:
$ attend send --focus deploy "migrations are done, ready for deploy"

# Agent B sees it via the peer sensor:
[attend sensor=peers] message from agent-ways in deploy: migrations are done, ready for deploy
```

When done, agents release focus or sessions end:
```
$ attend focus off deploy
# If both leave, "deploy" is cleaned up on next poll
```

### Flow 3: Standing workgroup

A team of agents that reconvene across sessions.

```
$ attend focus on infra --pin
# --pin keeps the group alive even when empty
# Next time an agent starts, it can discover and rejoin:

$ attend focus all
  Group             Members  Pinned
  ─────────────────────────────────
  deploy            0        no       (will be cleaned up)
  infra             0        yes      (persists)

$ attend focus on infra
```

### Flow 4: Scene switch

Aaron wants all agents in private mode while he's in a meeting, then open mode after.

```
# From any terminal:
$ attend scene private
# → releases all focus groups, project scope only
# → writes scene signal to broadcast so other agents' attend instances pick it up

# Later:
$ attend scene open
# → joins the well-known "open" group
# → all agents with attend running see the scene change and auto-join
```

### Flow 5: Discovery — "what groups exist?"

```
$ attend focus all
  Group             Members  Pinned
  ─────────────────────────────────
  deploy            2        no
  infra             1        yes
  daily             0        yes

# Join one:
$ attend focus on deploy
```

### Flow 6: Directing a message without joining

Sometimes you want to send a message to a group without subscribing to it.

```
$ attend send --focus infra "heads up: the CI cert expires Friday"
# Message lands in the group, but you don't join it or receive from it
```

### Flow 7: Human sends from terminal

Aaron is in a terminal, not in a Claude session. He wants to poke agents.

```
# Send to a specific group:
$ attend send --focus deploy "hold off, I'm rolling back"

# Send to broadcast (all agents):
$ attend send --broadcast "going to lunch, back in 30"
```

### CLI Summary

```
attend focus on <name> [--pin]    Focus on a group (create if needed, --pin to persist)
attend focus off <name>           Release focus from a group
attend focus list                 Show groups you're focused on
attend focus all                  Show all active groups with member counts
attend focus clear                Release all groups (project-only mode)
attend focus pin <name>           Pin a group (persist when empty)
attend focus unpin <name>         Unpin a group
attend focus dissolve <name>      Remove a group, notify members

attend send "msg"                 Send to project scope + joined focus groups
attend send --focus <name> "msg"  Send to a named focus group
attend send --broadcast "msg"     Send to all sessions (human↔agent channel)

attend scene <name>               Activate a scene (apply attention profile)
attend scene save <name>          Save current state as a scene (overwrite)
attend scene save-as <new-name>   Save current state as a new scene
attend scene edit <name>          Open scene definition for editing
attend scene delete <name>        Delete a scene (built-ins protected)
attend scenes                     List available scenes with scope summaries

attend peers                      Unified view: agents grouped by focus
attend status                     Self-view: your groups, signals, config
```

## Consequences

### Positive

- **Dynamic grouping** — form and dissolve workgroups without config file edits
- **Name-based targeting** — `--focus deploy` instead of `--to /home/aaron/Projects/...`
- **Self-cleaning** — ephemeral groups dissolve when everyone leaves
- **Unified view** — one `attend peers` organized by groups, not two overlapping commands
- **Scene presets** — named configurations combining group membership + attention profiles
- **Scoped attention** — project and focus scopes with independent governors prevent cross-contamination
- **Session persistence** — working groups survive restarts via scene tracking and pinned groups
- **Backward compatible** — project scope is implicit, broadcast unchanged, existing signals work

### Negative

- **Discovery** — agents need a way to discover group names. `attend focus all` helps, but the initial "how do I know what groups exist" requires either convention or the scene mechanism.
- **Dual governors** — splitting project/focus scope adds complexity to the disclosure path

### Neutral

- **Broadcast is a built-in group** — conceptually, broadcast is a group everyone is always in. Implementation may or may not change.
- **Project scope is implicit** — no behavioral change for single-agent workflows
- **CLI vocabulary matches code** — `attend focus on/off` maps directly to `groups.rs` internals

## Implementation Plan

### Phase 1: Scoped Scenes
1. Extend scene config schema with `project:` and `focus:` scope blocks (sensors, governor)
2. Split disclosure governor into project-scoped and focus-scoped instances
3. Implement scene CRUD: `attend scene save/save-as/edit/delete`
4. Track active scene per project in `_groups.yaml` for session persistence
5. Auto-activate last scene on restart

### Phase 2: Project-Scoped Config
6. Support `.attend/scenes.yaml` in repos for project-scoped scene defaults
7. Overlay project scenes onto user scenes (project wins on conflict)
8. Update the `/attend` skill documentation
