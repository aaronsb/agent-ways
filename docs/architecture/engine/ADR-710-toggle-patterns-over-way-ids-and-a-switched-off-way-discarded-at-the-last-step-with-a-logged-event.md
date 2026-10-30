---
contract: adr/v1
kind: decision
verb: change
capability: [config, matching]
basis:
  - operator: aaronsb
    level: directed
    said: "I think that a way that actually matched up and fired should be recorded in our statistics, but if that branch of a way, or a sensor, or a macro etc is disabled, we discard the actual injection or action event at the last possible step. this means that we're still logging the event selection result, but the additional property is that it was discarded due to a project scope configuration \"filter\""
    via: session 2026-10-30
  - operator: aaronsb
    level: guided
    said: "otherwise if we've not got the discard system correct, then none of these project scope or activated/deactivated ways actually respect the ways configuration"
    via: session 2026-10-30
  - operator: aaronsb
    level: directed
    said: "I think a bare toggle should only shut off that specific item. the reason is a little ambiguous. ideally, ways should be authored as a progressive disclosure. but realistically, it doesn't matter if they're authored progressively or not. there is a baked in assumption that a hierarchical file system should contain child subsets. the actual reality is that we are at some point planning to use a graph relationship overlay, and therefore, we can take both types of toggles"
    via: session 2026-10-30
  - operator: aaronsb
    level: directed
    said: "ok, so we won't call it a 'gitignore pattern', because it's not really gitignore"
    via: session 2026-10-30
  - evidence: "tools/ways-core/src/config.rs:586-600, winning_toggle: a key ending in /* switches off the way at the prefix itself and every way under it at any depth, and an exact key beats any prefix"
  - evidence: "tools/ways-cli/src/cmd/scan/candidates.rs:181-187 drops a switched-off way from the candidates; late_interaction.rs:572-590, mask_to_enabled, drops its rows before the softmax; candidate_log.rs lists enabled ways only. No event records that a switched-off way would have fired"
  - evidence: "live run, 2026-10-30: with documentation/adr: false in a project's ways.yaml, documentation/adr was switched off and documentation/adr/migration fired, where with no configuration it had been suppressed"
  - evidence: "the project ways.yaml files on the operator's machine, 2026-10-30: none uses a key ending in /*"
  - precedent: ADR-701
  - precedent: ADR-131
agent:
  name: claude
  model: claude-opus-5-5
amends:
  - ADR-701#1-masking-and-toggles
  - ADR-701#2-logging-and-history
  - ADR-131#schema
  - ADR-131#enforcement
considered:
  - operator: aaronsb
    said: "discard should show in fire statistics, because the discard was an operator choice specific for that project"
    via: session 2026-10-30, answering the discard-count probe in chat; the record was changed so a discard is a way_fired event with a discarded property, counted in the fire statistics
    covers: [discard-count]
status: accepted
date: 2026-10-30
deciders:
  - aaronsb
related:
  - ADR-701
  - ADR-131
  - ADR-702
  - ADR-115
  - ADR-518
observable:
  - "run: with documentation/adr: false in a project's .claude/ways.yaml, a prompt that fires documentation/adr with no configuration writes a way_fired event whose discarded property names filter toggle and toggle documentation/adr, ways stats counts it under documentation/adr as discarded, and the way is not injected"
  - "run: ways settings lint on a ways.yaml holding dir/*: false reports the key and names dir: false plus dir/**: false"
  - "run: two ways.yaml files holding the same keys in different orders switch off the same ways"
---

# ADR-710: Toggle patterns over way ids, and a switched-off way discarded at the last step with a logged event

## Summary

- **Decided:** keys under `ways:` in a project's `.claude/ways.yaml` are toggle patterns matched against way ids. A key with no wildcard names one way, `*` matches one segment, `**` matches one or more segments, and the most specific matching pattern wins whatever the key order. A trailing `dir/*` now covers direct children only, and `ways settings lint` names the replacement for its old meaning. A switched-off way stays out of the competition that decides what fires. A shadow pass over the same scores finds each switched-off way that would have fired and logs it as a fire with a `discarded` property naming the filter, so it counts in the fire statistics and nothing of it reaches the session.
- **Trades away:** a `dir/*` key changes meaning. Each scan repeats its score aggregation once for each switched-off way near the top and tests the frontmatter patterns of switched-off ways. The event stream gains one line per discarded fire.
- **One-way?** No. Toggle patterns are configuration syntax and the event is additive. Lint names the one key whose meaning changes.
- **Probes:** *Confident (discard-logged):* when a way, a domain or a whole project is switched off, the statistics show each time a switched-off way would have fired, marked as discarded with the setting that discarded it, and nothing of that way reaches the session. *Not confident (discard-count):* a discarded way is counted apart from the ways that fired, so a fire count still means guidance that reached the session.
- **Inversion:** one end removes a switched-off way before matching, which costs nothing and leaves no trace. The other lets it compete and drops it afterwards, which records it but lets it take share from enabled ways. The decision keeps the first for what fires and adds a shadow pass for the record. For keys, the ends are directory inheritance, where a key covers its subtree, and exact ids only. The decision makes a key exact unless it carries a wildcard.

## Context

ADR-131 added per-way toggles to the project overlay, keyed by way id. ADR-701 §1 added a prefix form, `dir/*`, and decided that a switched-off way is masked before any competition. `winning_toggle` (`tools/ways-core/src/config.rs:586-600`) applies `dir/*` to `dir` itself and to every way under it at any depth, with an exact key beating any prefix.

A switched-off way leaves the scan in three places. Candidate collection drops it (`tools/ways-cli/src/cmd/scan/candidates.rs:181-187`). The semantic lane drops its rows before the softmax (`mask_to_enabled`, `tools/ways-cli/src/cmd/scan/late_interaction.rs:572-590`). The candidate log lists enabled ways only (`tools/ways-cli/src/cmd/scan/candidate_log.rs`). No event records that a switched-off way would have fired, so nothing shows whether a project's configuration is respected.

A way id is a path under a ways root, so today a way id and a directory coincide. A toggle that covers a subtree assumes the directory tree holds child subsets. The ways graph is planned to gain a relationship overlay whose edges need not follow directories (ADR-702).

On 2026-10-30, a project with `documentation/adr: false` switched off `documentation/adr`, and `documentation/adr/migration` fired where it had been suppressed with no configuration. Masking the parent left its share to the child.

## Decision

### 1. Toggle patterns

Keys under `ways:` in a project's `.claude/ways.yaml` are toggle patterns, matched against way ids and not against directories.

- A pattern with no wildcard names exactly one way.
- `*` matches exactly one id segment.
- `**` matches one or more whole segments.
- `ways settings lint` reports a segment that mixes text and a wildcard, such as `doc*` or `a*b`, as an error. The matcher treats such a key literally, so it matches only a way id spelled with that asterisk, which no way has.
- The most specific matching pattern wins, independent of key order. An exact id beats any wildcard pattern. Among wildcard patterns, more literal segments win, then fewer `**`. A tie between `true` and `false` resolves to `false`, and `ways settings lint` warns about the tie.
- `true` turns a way back on.
- A way no pattern matches stays on, as ADR-131 decided.

| Pattern | Covers |
|---|---|
| `documentation/adr` | `documentation/adr` only |
| `documentation/adr/*` | the direct children of `documentation/adr`, such as `documentation/adr/migration` |
| `documentation/adr/**` | every way under `documentation/adr`, at any depth, and not `documentation/adr` |
| `documentation/*/migration` | `documentation/adr/migration` and any other `migration` one level under a `documentation` child |

With `documentation/**: false` and `documentation/adr: true`, every way under `documentation` is off except `documentation/adr`.

This record calls them toggle patterns. They are not gitignore patterns and do not follow gitignore's rules: in gitignore a bare path covers a directory's contents and the last matching line wins, and neither holds here.

Toggle patterns select over way ids so that a selector over the planned graph overlay can be added later beside them.

User-scope `disabled_domains` is unchanged.

### 2. The `/*` key changes meaning

Under ADR-701 §1, a trailing `dir/*` switched off `dir` and its whole subtree at any depth. Under toggle patterns it covers the direct children of `dir` only. `ways settings lint` reports each existing `/*` key and names the replacement for the old meaning: `dir: false` plus `dir/**: false`. No project on the operator's machine uses a `/*` key.

### 3. A switched-off way is discarded at the last step

- The masked pass decides what fires. ADR-701 §1 stands: a switched-off way takes no softmax share and no survivor slot from an enabled way. The top-candidate log of ADR-701 §2 keeps the masked pass's ranking.
- Candidate collection keeps a switched-off way, tagged with the filter that switched it off.
- On the semantic lane, the shadow pass runs once per switched-off way that appears in any chunk's top-K window. Each run repeats the aggregation over the same per-chunk scores with the enabled set plus that one way, and the way would have fired when it clears the same gates. The corpus already embeds every way, so the shadow pass is arithmetic only, with no embedding work.
- The keyword, command, file and tool lanes test a switched-off way's frontmatter patterns with the gates they apply to an enabled way, and log a hit the same way.
- Each switched-off way that would have fired is logged as a `way_fired` event with a `discarded` property, and its injection is dropped. A switched-off way that would not have fired is not logged, so a switched-off domain does not flood the log.
- The relevance judge is not asked about a switched-off way. Would have fired means it cleared the matcher's gates.
- On the late-interaction lane a switched-off way is confirmed against the body sidecar at the way's current hash. With no such entry it is not logged, since confirming it on the call would embed.
- A keyword hit that the floor vetoes is re-checked against the prompt-only scores only when the enabled pass already computed them.
- A state-lane discard is logged once per session per way.
- In a project with `enabled: false`, only the pattern lanes log discards, since nothing is embedded there. Testing the corpus's patterns costs about 35 ms per prompt in such a project.
- Macros and sensors follow the same rule as they gain filters: the event is selected and logged, and the action is dropped at the last step with the filter named.

### 4. A discarded fire

A discarded fire is a `way_fired` event in the events stream with a `discarded` property. A delivered fire has no `discarded` property. The discard was the operator's choice for that project, so the fire statistics count it, and `ways stats` shows each way's discarded fires by filter beside its delivered ones. Every reader that means guidance reached the session excludes discarded fires: the injected text, token and spend accounting, refire and disclosure state, and any delivery-based learning signal.

| Field | Value |
|---|---|
| `way` | the way id |
| `discarded.filter` | what discarded it: `toggle`; `domain` for user-scope `disabled_domains`; `project_off` for `enabled: false`. `isolation` and `untrusted` are added by ADR-518. |
| `discarded.toggle` | the winning pattern, when the filter is `toggle` |
| `discarded.scope` | `user` or `project`, the settings layer the filter came from, when it is known |
| `lane` | `prompt`, `command`, `file`, `tool`, `task`, `state`, `check`, `pull` or `subagent` |
| scores | on the semantic lane, the cosine, share and margin the shadow pass computed, under the candidate log's names |

### 5. A single-way pattern switches off that way only

The ways under a switched-off way compete without it. In the run of 2026-10-30, `documentation/adr/migration` fired under `documentation/adr: false` where it had been suppressed with no configuration. That outcome is intended. A project that wants the subtree off as well adds `documentation/adr/**: false`.

## Consequences

### Positive

- The event log shows each fire a configuration held back and the key that held it, so a project's toggles can be checked against what a session did.
- A key switches off what it names. A parent way can be switched off while its children stay on.
- Reordering keys in `ways.yaml` changes nothing.
- Patterns over ids leave room for a graph selector beside them.

### Negative

- A `/*` key changes meaning. A project that uses one switches off fewer ways after the upgrade until it adds `dir: false` and `dir/**: false`, and lint names the fix.
- Each semantic scan repeats the aggregation once for every switched-off way in a top-K window, and the pattern lanes test the patterns of switched-off ways.
- Switching off a parent way can raise its children's fire rate.
- The specificity order is a rule a user must learn, and a `true`/`false` tie shows only in lint.

### Neutral

- `way_toggled_off` and the settings screens move to the toggle-pattern resolver, so what they show is what a session does.
- Every reader of `way_fired` events is sorted into one of two kinds. Fire statistics count discarded fires. Readers of delivered guidance skip them.
- `disabled_domains` and `enabled: false` keep their meaning and gain a logged discard.
- ADR-131 said a disabled way is skipped entirely, with no scoring. A switched-off way is now scored in the shadow pass and logged, and it still never reaches the session.
- ADR-702, proposed, has the enabled state follow the parent graph. Under this record, following the graph is the job of a selector beside toggle patterns, and a bare key does not inherit.

## Alternatives Considered

- **A separate `way_discarded` event, counted apart from fires.** Rejected by the operator: the discard is a choice made for one project, and the fire statistics should show it.
- **Gitignore's rules.** Rejected: a bare path would cover its subtree, and the last matching line would win, so a key's effect would depend on its position in the file.
- **A bare key covers its subtree.** Rejected: it assumes the directory tree holds child subsets, and the planned graph overlay does not follow directories. `dir/**` covers a subtree when that is wanted.
- **Keep `dir/*` as the subtree form and add `**` beside it.** Rejected: `*` would match one segment inside a pattern and a whole subtree at its end.
- **Let switched-off ways compete and drop them after the fire decision.** Rejected: a switched-off way would take share and survivor slots from enabled ways (ADR-701 §1).
- **One shadow pass with every filter lifted.** Rejected: switched-off ways would compete with each other, so when several matched one prompt they would split the share and a way that would have fired alone could go unlogged. The per-way runs cost arithmetic only.
- **Log every switched-off candidate, or its near misses.** Rejected: a switched-off domain would write lines on every scan.
- **Ask the judge about a would-have-fired discard.** Rejected: it spends a judge call on a way that will not be shown.
