---
contract: adr/v1
kind: decision
verb: change
capability:
  - attend
  - cli
  - install
basis:
  - evidence: ADR-402
  - precedent: ADR-187
  - operator: aaronsb
    level: directed
    said: "perhaps we just have an agent-ways mcp server that serves attend and keepalive and other items. let's revert back changes from this keepalive experiment and instead build our initial shape of the actual agent-ways mcp server (I'm pivoting here from calling it an attend mcp server)"
    via: "session 2026-09-30, after a dotfiles keepalive prototype built on a Claude Code channel (ADR-402)"
  - operator: aaronsb
    level: directed
    said: "since attend and ways are sort of shared crates, it feels like the mcp should be a seperate binary and crate. ways cli stays with context disclosure control and guidance control, attend gets thinned out to a minimal signal channell, and ways-mcp becomes the mcp server"
    via: "session 2026-09-30, the operator's own answer in place of two agent-written options (a `ways mcp` subcommand, or a new crate and binary)"
  - operator: aaronsb
    level: directed
    said: "let's put a bounded keepalive - 8 hours. if at 8 hours nothing happens, then ask the agent to wrap and stop the keep alive"
    via: "session 2026-09-30, after asking whether a keepalive is within the terms of service; the agent's answer recommended a bounded, operator-armed window"
  - operator: aaronsb
    level: guided
    said: "ADR + skeleton (Recommended)"
    via: "session 2026-09-30, selected from agent-written options for the first increment; the label was written by the agent"
  - operator: aaronsb
    level: guided
    said: "Supersede it (Recommended)"
    via: "session 2026-09-30, selected from agent-written options for how this record relates to ADR-187; the label was written by the agent"
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-09-30
deciders:
  - aaronsb
  - claude
related:
  - ADR-129
  - ADR-169
  - ADR-171
  - ADR-172
  - ADR-173
  - ADR-182
  - ADR-184
  - ADR-185
  - ADR-402
---

# ADR-501: The agent-ways MCP server: one server for attend, keepalive and later modules, inbound through channels

## Summary

- **Decided:** agent-ways ships one MCP server, the `ways-mcp` binary in its own crate, registered as `agent-ways`. It hosts modules. The attend module carries the outbound and query tools ADR-187 designed. A keepalive module keeps an idle session's prompt cache warm for a bounded window and asks the agent to wrap when the window closes. Claude Code channels become an optional third inbound conduit beside the Monitor sensor loop and the Stop-hook drain. The crates split by role: the ways CLI keeps context disclosure and guidance control, attend narrows to the signal bus and its sensor loop, and `ways-mcp` is the MCP frontend over their libraries.
- **Trades away:** a third binary to build, release, link and register; a dependency on a Claude Code research-preview extension for inbound delivery, which needs a launch flag no setting can supply.
- **One-way?** No. Removing the server deletes a crate and its settings entries, and the Monitor and Stop-hook conduits stay whole.
- **Probes:** *Confident (umbrella):* you want one server named for agent-ways that later modules join, not an attend server that other features borrow. *Not confident (keepalive-end):* when the eight-hour window closes you want the agent to run the wrap skill and the session to stay quiet until you type in it, not the session to end.
- **Inversion:** between one server per feature (attend, keepalive, ways each their own) and one server for everything agent-ways does; the answer here is one server with modules, which keeps a single registration and a single permission prefix.

## Context

ADR-187 designed `attend mcp`: attend's message verbs as typed MCP tools over one library shared with the CLI, with inbound delivery left on the Monitor-hosted sensor loop and the Stop-hook drain (ADR-172). Its item 1 held that split to be forced, because the MCP protocol revision of 2026-07-28 has no path that places content into the model's turn.

ADR-402 records that Claude Code's channel extension is such a path. A server that declares the experimental `claude/channel` capability can send `notifications/claude/channel`, and Claude Code delivers each one into the model's context as a `<channel>` event. The event starts a turn in an idle session. During the research preview a custom channel loads only when the session is launched with `--dangerously-load-development-channels server:<name>`.

The motivating case was a keepalive. The prompt cache expires an hour after the last request, and a session left idle past that pays a full cache write on its next turn. ADR-182's keepwarm answers this with a Monitor wake at fifty idle minutes, which needs the sensor loop running in that session. A channel can wake a session with no loop. A keepalive also invites unbounded background use, so the operator set a limit: eight hours without an operator prompt, then the agent wraps and the keepalive stops.

The operator also widened the scope. The server is agent-ways' server, and attend is one module in it.

## Decision

1. **One server, its own crate.** A new workspace crate `tools/ways-mcp` builds the `ways-mcp` binary. `ways reconcile` registers it in each enabled target (ADR-184) as the MCP server `agent-ways`, over stdio, launched once per session by Claude Code. The installer builds and links it like the other binaries. It is resident for the life of the session and follows ADR-187 item 5: every tool call derives its identity afresh and holds nothing for the next call.

2. **Crates split by role.** The ways CLI (`ways-cli`, `ways-core`) owns context disclosure and guidance control. Attend narrows to the signal bus: the signal files, the instance registry, the sensor loop that owns session state (ADR-187 item 11), and the Stop-hook drain. `ways-mcp` depends on their libraries and owns no state of its own beyond a presence marker. Moving attend's verb bodies into a library target (ADR-187 item 3) is the precondition for the attend module.

3. **Modules.** A module contributes tools, inbound event sources, or both. Tool names carry the module name, `attend_send` or `keepalive_status`, so permission entries read `mcp__agent-ways__attend_send` and a module's tools can be allowed or denied as a group by prefix. The server's `instructions` string is assembled from the modules that are live in the session.

4. **The attend module is ADR-187's design, carried forward.** Its items 3 (one library, two frontends, one description source), 4 (shared `reply` and `inbox` state), 5 (fresh invocation per call), 7 (mechanical guidance in tool descriptions, judgment guidance in the way and skill), 9 (envelope fields as typed parameters), 10 (the tool surface is the contract), 11 (ownership by role) and 12 (one state contract, then a trial) hold as written, with `ways-mcp` in place of `attend mcp` and the tool and permission names of item 3 above. Its item 6's permission rules hold with the new names. Its item 8, the interim guard hooks, holds unchanged.

5. **Channels are an optional inbound conduit.** The server declares `claude/channel` in every session. In a session launched with the development-channels flag its events reach the model; elsewhere Claude Code drops them and the Monitor and Stop-hook conduits carry inbound alone. No module depends on the channel for correctness. A module that pushes an event decides the session state it depends on before pushing and sends a direct instruction, because a woken model answers from its context rather than inspecting the session (ADR-402 finding 4).

6. **The keepalive module.** It is ADR-182's keepwarm delivered through the channel conduit, with an end:
   - The operator arms it; it is off by default.
   - When an idle session's cache enters the last quarter of its time to live, the module pushes one event asking for a one-word reply and no tools. That turn reads the cache and resets its expiry.
   - The window is eight hours from the operator's last typed prompt in the session, read from Claude Code's prompt history, which records typed prompts and not channel events. The length is configurable; eight hours is the default.
   - When the window closes, the module pushes one event asking the agent to run the wrap skill, then pushes nothing more for that session until the operator types in it again.
   - A session whose cache has already expired is not woken; the next turn pays the cold write either way.
   The status line displays keepalive state and makes no decisions; the at-risk judgment lives in the module.

7. **What Claude Code speaks, not the newest revision.** Claude Code 2.1.285 opens with `initialize` and `notifications/initialized` (ADR-402), which the 2026-07-28 revision removes. The server implements the handshake the client sends and tracks the client, not the specification's head.

8. **First increment.** This record, ADR-402, and a skeleton `ways-mcp`: stdio JSON-RPC, the module registry, `claude/channel` declared, and one read-only tool in the core module `ways`, `ways_status`, reporting the server version, the session id derived by ancestry (ADR-171), whether the session loaded the channel conduit, and the live modules. The attend and keepalive modules, the reconcile registration and the install wiring follow in later increments.

Reversibility: reversible. Deleting the crate and its settings entries restores the CLI-only surface, and the Monitor and Stop-hook conduits are untouched throughout.

## Consequences

### Positive

- One registration, one permission prefix and one presence marker for everything agent-ways exposes over MCP. A later module (way queries, the corpus, the adr tool) joins the server instead of adding one.
- Inbound delivery gains a path that needs no sensor loop in the woken session, which makes a keepalive possible for any session launched with the flag.
- The keepalive has a hard end the operator chose, measured from their own activity, so it cannot run indefinitely on idle sessions.
- ADR-187's work on the attend tool surface carries over intact.

### Negative

- A third binary: build, release, checksums, PATH linking and the reconcile registration all grow by one entry.
- The channel conduit depends on a research-preview extension. Its capability name, notification method and launch flag can change, and while it lasts the operator must launch sessions with a flag named `dangerously`.
- A session launched without the flag gets no keepalive and no channel delivery, silently. `ways_status` reports which conduit a session has so the gap is visible.
- The keepalive spends a cache read per hour per armed idle session. That is cheaper than the cold write it prevents and counts against usage limits like any other turn.

### Neutral

- ADR-187 is abandoned in favour of this record, never having been accepted; the items listed in Decision 4 continue here.
- Module-prefixed tool names change ADR-187's permission names from `mcp__attend__send` to `mcp__agent-ways__attend_send`. The ADR-169 baseline amendment ADR-187 described applies with the new names.
- ADR-182's Monitor-based keepwarm stays for sessions without the channel conduit.

## Alternatives Considered

- **`ways mcp` as a subcommand of the ways CLI.** Rejected by the operator: the ways CLI keeps context disclosure and guidance control, and a separate crate keeps the MCP frontend and its dependencies out of the binary every hook runs.
- **`attend mcp`, as ADR-187 designed.** Rejected: it names the server for one module, and keepalive and later modules would either borrow attend's server or add their own.
- **One server per feature.** Rejected: each would need its own registration, permission prefix, presence marker and process per session.
- **Status-line-driven keepalive with a standalone channel script**, as prototyped in dotfiles. Rejected: it split the at-risk judgment across a shell renderer and a script, lived outside agent-ways, and needed hand registration.
- **Keepalive with no end.** Rejected by the operator in favour of a bounded window that ends in a wrap.
- **An MCP SDK crate for the transport.** Not taken for the skeleton: the surface Claude Code uses is a handful of messages over newline-delimited JSON-RPC, and no SDK models the channel extension. Revisit when a module needs protocol features beyond tools and notifications.
