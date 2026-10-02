---
contract: adr/v1
kind: decision
verb: constrain
capability: [install, config, attend]
amends: ADR-179#Decision
basis:
  - operator: aaronsb
    level: authored
    said: I want to avoid fallbacks that persist beyond our work here. when we're done we shouldn't have any legacy compatibility
    via: chat, session 6881527e, 2026-10-01, reviewing the decisions raised by the consolidation PRs (#713, #714, #715)
  - evidence: ADR-505, the duplication audit, which lists the fallback paths and old names the consolidation touches
  - precedent: ADR-179
  - precedent: ADR-505
agent:
  name: Claude
  model: claude-opus-5-5
status: accepted
date: 2026-10-01
deciders:
  - aaronsb
related:
  - ADR-179
  - ADR-503
  - ADR-504
  - ADR-505
---

# ADR-506: The consolidation ends with no legacy compatibility

## Summary

- **Decided:** when the consolidation round (issues #691 to #705) closes, no code reads an old name, an old path, an old file format or an old config layer for compatibility. A transition read may exist while the round is open, and the step that introduces it names the step that removes it. ADR-179's kept fallbacks are removed in this round.
- **Trades away:** a pre-1.0 install, an old config file, an old attend tray or an old scene key stops being read. Its owner moves it by hand, using the release notes.
- **One-way?** Expensive, not one-way. A fallback can be restored from git history, but an install that has moved on will not need it, so restoring one serves only installs that skipped the release.
- **Probes:** *Confident:* the operator wants no compatibility code left when the round closes, including the ADR-179 fallbacks. *Not confident:* whether layering that is part of the design, such as project settings over user settings over defaults, counts as a fallback. This record treats it as design, not compatibility.
- **Inversion:** one end keeps every old reader forever, so nothing ever breaks and every path is read twice. The other end removes old readers the moment the new form lands, so a release in the middle of the round can strand state. This sits at the end of the round, not at either end.

## Context

The consolidation (ADR-503, ADR-504, evidence in ADR-505) replaces duplicated implementations with one settings model, one TUI engine, one theme and one session crate. Each step that renames something meets the same choice: read the old form for a while, or not.

ADR-179 chose to keep the pre-1.0 transition fallbacks in `ways-core/src/paths.rs` (the `claude-ways` cache and the old events log) and the legacy config layers (`~/.claude/ways.json`, `$XDG_CONFIG_HOME/ways/config.yaml`) as a safety net for un-migrated installs. The consolidation PRs added more of the same kind: attend reading old-named trays and registries for one release, settings clamping values a new range rejects, and the old command output kept as aliases.

Each fallback is cheap alone. Together they double the paths every reader takes, and a "for one release" fallback without an owner stays.

## Decision

1. When the consolidation round closes, no code path exists only to read a name, path, format or config layer from before the round. That includes:
   - ADR-179's fallbacks: the `claude-ways` cache, the old events log, `~/.claude/ways.json`, `$XDG_CONFIG_HOME/ways/config.yaml`, and the pre-1.0 in-place guards that exist only to route a legacy install to the migrator tag.
   - Old-named attend trays, instance registries and the scene `rooms:` key.
   - Value clamping kept only so an old out-of-range value still loads.
   - The old output of `ways config show`, `disable`, `enable` and `ways-agent use|mode`, which ADR-503 §9 already retires.
2. A transition read may exist while the round is open, when two steps of the round would otherwise strand state between them. The PR that adds it names, in its body and in a code comment, the issue in this round that removes it.
3. Layering that is part of the design is not compatibility. Project over user over defaults (ADR-503), and the 16-colour default theme a chosen theme falls back to on a smaller terminal (ADR-504), stay.
4. The release that ships the closed round lists every removed reader and how an owner moves their state by hand.
5. The round closes only after a check finds no compatibility reader left. The check reads the code for the names listed in point 1 and for any `legacy`, `old_` or `compat` reader added during the round.

This amends ADR-179's Decision, which kept the fallbacks.

## Consequences

### Positive

- Each reader has one path, so tests cover what runs.
- No "for one release" fallback outlives its release.

### Negative

- An install that skips the release that ships the round loses the old state it held: an un-migrated pre-1.0 install, an old `ways.json`, unread messages in an old-named tray. The release notes are the only bridge.
- Steps in the round that rename state need their transition read removed by a named later step, which adds an ordering constraint on the plan.

### Neutral

- The pre-1.0 migrator stays reachable at the `ways-v1.8.3` tag, as ADR-179 left it.

## Alternatives Considered

- **Keep ADR-179's fallbacks and remove only the round's own transition reads.** Rejected: the operator asked for no legacy compatibility at the end, and ADR-179's fallbacks are the oldest of it.
- **Remove each old reader in the PR that adds the new form, with no transition read.** Rejected: a mixed-version window inside the round (an `attend run` process started before an update) would lose messages with no way to recover them.
- **Keep fallbacks behind a flag.** Rejected: a flag keeps the code and its tests, which is the cost the decision removes.
