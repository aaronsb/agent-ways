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
    via: "session 2026-09-30, selected from agent-written options for how this record relates to ADR-187; the label was written by the agent. The adr tool supersedes only an accepted record, and ADR-187 was proposed, so it was abandoned with a pointer to this record instead"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "lets merge it"
    via: "session 2026-09-30, approving the merge of PR #651 after the review remediation; the two probes (umbrella, keepalive-end) were asked twice and not answered"
status: accepted
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

- **Decided:** agent-ways ships one MCP server, the `ways-mcp` binary in its own crate, registered as `agent-ways`. It hosts modules. The attend module carries the outbound and query tools ADR-187 designed. A keepalive module keeps an idle session's prompt cache warm for a bounded window and asks the agent to wrap when the window closes. Claude Code channels become an optional third inbound conduit beside the Monitor sensor loop and the Stop-hook drain. The crates split by role: the ways CLI keeps context disclosure and guidance control, attend narrows to the signal bus and its sensor loop, and `ways-mcp` is the MCP frontend over their libraries. `ways-mcp` installs, updates and releases like the other suite binaries, and `ways reconcile` registers it with Claude Code through Claude Code's own CLI.
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

1. **One server, its own crate.** A new workspace crate `tools/ways-mcp` builds the `ways-mcp` binary. Claude Code launches it once per session over stdio as the MCP server `agent-ways`; item 9 covers how it is installed, registered and updated. It is resident for the life of the session and follows ADR-187 item 5: every tool call derives its identity afresh and holds nothing for the next call.

2. **Crates split by role.** The ways CLI (`ways-cli`, `ways-core`) owns context disclosure and guidance control. Attend narrows to the signal bus: the signal files, the instance registry, the sensor loop that owns session state (ADR-187 item 11), and the Stop-hook drain. `ways-mcp` depends on their libraries and owns no state of its own beyond a presence marker and the keepalive module's per-session record (item 6). Moving attend's verb bodies into a library target (ADR-187 item 3) is the precondition for the attend module.

3. **Modules.** A module contributes tools, inbound event sources, or both. Tool names carry the module name, `attend_send` or `keepalive_status`, so permission entries read `mcp__agent-ways__attend_send` and a module's tools can be allowed or denied as a group by prefix. The server's `instructions` string is assembled from the modules that are live in the session.

4. **The attend module is ADR-187's design, carried forward.** Its items 3 (one library, two frontends, one description source), 4 (shared `reply` and `inbox` state), 5 (fresh invocation per call), 7 (mechanical guidance in tool descriptions, judgment guidance in the way and skill), 8 (the interim guard hooks), 9 (envelope fields as typed parameters), 10 (the tool surface is the contract), 11 (ownership by role) and 12 (one state contract, then a trial) hold as written, with `ways-mcp` in place of `attend mcp`. Its item 2's tool set becomes the attend module's tools under item 3's names (`attend_send`, `attend_reply`, …); `keepwarm`, which item 2 kept on the CLI, moves into the keepalive module (item 6). Its item 6's per-tool permission rules hold under the new names, written by the settings.json merge as before. Its item 6's registration does not: settings.json cannot declare an MCP server, and item 9 replaces it.

5. **Channels are an optional inbound conduit.** The server declares `claude/channel` in every session. In a session launched with the development-channels flag its events reach the model; elsewhere Claude Code drops them and the Monitor and Stop-hook conduits carry inbound alone. No module depends on the channel for correctness. A module that pushes an event decides the session state it depends on before pushing and sends a direct instruction, because a woken model answers from its context rather than inspecting the session (ADR-402 finding 4).

6. **The keepalive module.** It is ADR-182's keepwarm delivered through the channel conduit, with an end:
   - The operator arms it; it is off by default.
   - When an idle session's cache enters the last quarter of its time to live, the module pushes one event asking for a one-word reply and no tools. That turn reads the cache and resets its expiry.
   - The window is eight hours from the operator's last typed prompt in the session, read from Claude Code's prompt history, which records typed prompts and not channel events. The length is configurable; eight hours is the default.
   - When the window closes, the module pushes one event asking the agent to run the wrap skill, then pushes nothing more for that session until the operator types in it again.
   - A session whose cache has already expired is not woken; the next turn pays the cold write either way.
   - Whether keepalive is armed, and the window length, live in the ways user config. The one piece of per-session state, that the wrap event was sent for a given last typed prompt, is a record keyed on the session id under the ways state root, so a restarted server does not send the wrap twice.
   The status line displays keepalive state and makes no decisions; the at-risk judgment lives in the module.

7. **What Claude Code speaks, not the newest revision.** Claude Code 2.1.285 opens with `initialize` and `notifications/initialized` (ADR-402), which the 2026-07-28 revision removes. The server implements the handshake the client sends and tracks the client, not the specification's head. It answers `initialize` from a list of the revisions it implements, and 2026-07-28 is not on it: Claude Code does not register a channel server that connects on that revision, so accepting it would keep the tools and lose the channel without an error.

8. **First increment.** This record, ADR-402, and a skeleton `ways-mcp`: stdio JSON-RPC, the module registry, `claude/channel` declared, and one read-only tool in the core module `ways`, `ways_status`, reporting the server version, the session id derived by ancestry (ADR-171), whether the session loaded the channel conduit, and the live modules. The lifecycle of item 9 lands with it, so the server never ships without a way to install and update it. The attend and keepalive modules follow in later increments.

9. **Lifecycle.** `ways-mcp` changes often, so it follows the path every suite binary takes rather than one of its own:
   - **Build and install.** `make ways-mcp` downloads the prebuilt for the platform and falls back to cargo; the install links it onto PATH, and reconcile projects it into each target as `bin/ways-mcp`. Until a module depends on the server, an install that can neither download nor build it warns and continues.
   - **Register.** A user-scope MCP server lives in Claude Code's `.claude.json`, which running sessions rewrite and settings.json cannot declare. `ways reconcile` therefore changes it only through `claude mcp add-json` and `claude mcp remove` at user scope: without `CLAUDE_CONFIG_DIR` for the default target, whose file is `~/.claude.json`, and with it for any other target, whose file is `<target>/.claude.json`. An operator who exports `CLAUDE_CONFIG_DIR` naming the default target gets the same treatment as any other target, since that is the file their sessions read; a legacy `.config.json` in the directory is read in place of `.claude.json`, as Claude Code does. The registered command is the target's own `bin/ways-mcp`. A current registration is left alone, one naming another `ways-mcp` path is replaced, and an `agent-ways` entry that runs anything else is reported and kept. A missing `claude` binary or build is reported and never fails the reconcile.
   - **Update.** `ways update` refreshes it with the other cargo components. The registration names the projected symlink, so it does not change on update. A running session keeps the server it started with until it restarts.
   - **Release.** `ways-mcp` is a release component with its own tags (`ways-mcp-vX`), build workflow and checksums, cut through `make cut-release` like the others.
   - **See.** `ways status` reports the installed version and each enabled target's registration; `ways_status` reports the version a session is running, which shows a session still on an older server.
   - **Remove.** Disabling or removing a target removes our registration before its links; `make uninstall` removes the PATH link.

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
- Module-prefixed tool names change ADR-187's permission names from `mcp__attend__send` to `mcp__agent-ways__attend_send`. The ADR-169 baseline amendment ADR-187 described applies to those allow entries with the new names; its `mcpServers` block is replaced by item 9's registration.
- ADR-182's Monitor-based keepwarm stays for sessions without the channel conduit.

## Alternatives Considered

- **`ways mcp` as a subcommand of the ways CLI.** Rejected by the operator: the ways CLI keeps context disclosure and guidance control, and a separate crate keeps the MCP frontend and its dependencies out of the binary every hook runs.
- **`attend mcp`, as ADR-187 designed.** Rejected: it names the server for one module, and keepalive and later modules would either borrow attend's server or add their own.
- **One server per feature.** Rejected: each would need its own registration, permission prefix, presence marker and process per session.
- **Status-line-driven keepalive with a standalone channel script**, as prototyped in dotfiles. Rejected: it split the at-risk judgment across a shell renderer and a script, lived outside agent-ways, and needed hand registration.
- **Keepalive with no end.** Rejected by the operator in favour of a bounded window that ends in a wrap.
- **An MCP SDK crate for the transport.** Not taken for the skeleton: the surface Claude Code uses is a handful of messages over newline-delimited JSON-RPC, and no SDK models the channel extension. Revisit when a module needs protocol features beyond tools and notifications.
