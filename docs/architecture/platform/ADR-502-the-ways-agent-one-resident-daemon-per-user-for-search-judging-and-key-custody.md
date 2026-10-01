---
contract: adr/v1
kind: decision
verb: add
capability: install
basis:
  - operator: aaronsb
    level: directed
    said: "the other thing to consider is maybe it's time to stop invoking ways over and over to dynamically check and instead use a daemon with sockets to access i t"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "it would have to support concurrent connections"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "I often have many claude code sessions running on a machine"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "we need to think of a scheme that might look similar to say, an ssh key daemon"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "I think the socket daemon would run two services: the embedding model searcher ( so it's not loading each time) as well as the judge engine. the judge engine could have a local mode (actual compute) or remote mode (haiku)"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "yeah, we need to have a configurator that is tuned. that can be packaged with the agent-ways system, and allows us to edit it ( and provide support for more models and apis possibly)"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "the lifecycle management of agent-ways (including installation and update) will need to have a way to acquire and store the api key and check it"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "let's get minimum viability using the mode 0600 in a 0700 directory method."
    via: chat, session 418e1be3, 2026-09-30/10-01
  - operator: aaronsb
    level: directed
    said: "then we need to be able to log but silently degrade to essentially the decision model we have right now"
    via: chat, session 418e1be3, 2026-09-30/10-01
  - evidence: ADR-195
  - precedent: ADR-501
agent:
  name: claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "yes"
    via: "chat, session 418e1be3, 2026-10-01, answering the probes on PR #667"
    covers: [shape]
  - operator: aaronsb
    said: "embedding search for now remains as just the atomic ways invocation as it is now. we'll track moving that as an issue that is medium to high priority"
    via: "chat, session 418e1be3, 2026-10-01, answering the probes on PR #667"
    covers: [scope]
  - operator: aaronsb
    said: "yes"
    via: "chat, session 418e1be3, 2026-10-01, answering the probes on PR #667; on the daemon's crate being named ways-agent"
status: accepted
date: 2026-10-01
deciders:
  - aaronsb
related:
  - ADR-142
  - ADR-184
  - ADR-195
  - ADR-196
  - ADR-501
  - ADR-604
---
# ADR-502: The ways agent: one resident daemon per user for search, judging and key custody

## Summary

- **Decided:** agent-ways runs one resident daemon per user, the ways agent, modelled on ssh-agent. It listens on a socket under `$XDG_RUNTIME_DIR`, serves every session and agent tool on the machine concurrently, holds the matcher's embedder and corpus embeddings (the search service, from a later increment) and the relevance judge (the judge service, ADR-196), and is the only process that reads the provider API key. Engine profiles are shipped tuned and overridden in a user layer. The key is acquired, stored and checked through the lifecycle commands; the first release stores it in a 0600 file inside a 0700 directory. Hooks fall back silently to today's behaviour whenever the daemon cannot answer.
- **Trades away:** A long-lived process to start, upgrade and supervise, and a socket protocol to version, in exchange for loading models once and keeping provider connections warm.
- **One-way?** No. Hooks keep today's path as the fallback, so removing the daemon restores current behaviour.
- **Probes:** *Confident (shape):* one shared daemon for all your sessions and tools, used like ssh-agent, is what you had in mind. *Not confident (scope):* the first release ships the judge service only, with search moving into the daemon in a later increment.
- **Inversion:** Each hook loads what it needs, or one process holds everything. The decision holds the expensive and sensitive parts in one process and keeps hooks thin, with the hook's own path as the fallback.

## Context

Hooks start a process per call. The current embedder is cheap to load: one `way-embed` match, process start to scores, takes about 30 ms on the probe machine. Larger models are not: loading a 0.6B reranker and judging six ways took 2.3 s (ADR-195). An operator commonly runs many Claude Code sessions on one machine, each firing hooks, several in parallel per event, and agent-ways aims to serve other agent tools as well. A hosted judge (ADR-196) adds a provider key that should not be read by every hook, and a TLS handshake that a fresh process pays on every call. `ways-mcp` (ADR-501) is per session and Claude Code-specific, so it cannot hold shared state for the machine.

## Decision

1. **One per user, on the ssh-agent model.** The socket defaults to `$XDG_RUNTIME_DIR/agent-ways.sock`, overridable by `WAYS_AGENT_SOCK`. The socket is mode 0600 and every connection is checked for the same uid by peer credentials. It is started by a systemd user unit or launchd where installed, otherwise by the first hook that finds none, under a lock so two hooks never start two daemons. It exits after an idle period.
2. **Two services behind one socket.** *search* holds the embedder and the corpus embeddings and answers which ways match a text. *judge* holds the configured engine and answers P(yes) per candidate (ADR-196). A hook asks search and then judge on one connection. Each service loads and reports separately. The first release ships judge only. Search stays in each hook's own `ways` invocation, as today, and moves into the daemon in a later increment (#668).
3. **Concurrency.** Requests are self-contained: session id, agent tool, turns, candidates. The daemon holds no per-session state beyond optional context keyed by tool and session. It serves many clients at once, keeps provider connections open, caps CPU threads for local engines with one machine-wide budget, and gives every request a deadline. A request past its deadline gets the fallback.
4. **Versioning.** Client and daemon exchange versions on connect. A daemon from another release is replaced, not served from, so an update takes effect without a manual restart.
5. **Engine profiles.** Profiles ship with tuned defaults (ADR-196 §5). A user layer overrides any field and survives updates. Providers are adapters behind one request and response contract; the first are Anthropic and OpenRouter.
6. **Key lifecycle.** `ways agent key add --provider <p>` reads the key from a no-echo prompt, stdin or a file. The installer and `ways update` offer it as an optional step; skipping it leaves the gate off. The key is stored at `$XDG_CONFIG_HOME/agent-ways/keys/<provider>`, the file 0600 inside a 0700 directory, written atomically; `key add` refuses when it cannot set those modes. `ANTHROPIC_API_KEY` or `OPENROUTER_API_KEY` in the daemon's environment override the file. `ways agent key check` makes a zero-cost call to the provider and reports valid, invalid, no credit, rate-limited or model unavailable; it runs at install, after update, at daemon start and in `ways status`, and warns when the key file is readable by other accounts (ADR-604 rule 6). The profile records only the key's source. `ways status` never shows more than the key's last four characters. OS keychain storage and a key command are later increments.
7. **Control.** `ways agent status` reports liveness, services, loaded engines, queue depth, the thread budget and the last key check. `ways agent load` and `unload` manage engines.
8. **Failure.** When the daemon is absent, busy past the deadline, or cannot reach its engine, the hook uses today's behaviour and the fallback is logged. Nothing interrupts the user.

## Consequences

### Positive

- Models load once per machine; provider connections stay warm; hooks stay small.
- The key is read by one process and stored with fixed permissions.
- Every session and agent tool shares one search and one judge, sized once.

### Negative

- A resident process to supervise, with a lock, a version handshake and an idle exit to get right.
- A socket protocol that changes with the daemon and must stay compatible across a release.

### Neutral

- `ways-mcp` remains the per-session MCP front end and becomes another client of the daemon.
- The socket can be forwarded into containers and remote sessions, as ssh-agent's is.

## Alternatives Considered

- **Load per hook, as today.** Rejected: model loading dominates each call, and the key would be read by every hook.
- **Host the services in `ways-mcp`.** Rejected: it runs once per Claude Code session, so models and connections would be duplicated per session, and other agent tools cannot use it.
- **One daemon per session.** Rejected for the same duplication, which grows with the number of sessions an operator runs.
- **OS keychain only for the key.** Deferred: it adds a platform dependency per OS. The 0600 file is the minimum viable store, with the keychain as a later backend.
