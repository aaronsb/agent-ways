---
description: setting a way's firing cadence with the refire field — how soon a way re-discloses after it fires, as a fraction of the context window or a named preset
vocabulary: refire cadence redisclose re-disclosure half-life preset once rare normal frequent refire_presets window fraction fire-bearing fire-time exempt refusal portability
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Firing Cadence (`refire:`)

Fire-bearing ways (ways with description + vocabulary that participate in semantic matching) should carry a `refire:` field. This controls re-disclosure — how quickly the way becomes eligible to fire again after a fire. Per ADR-126 the value is a fraction of the session's context window, resolved at fire time against the model's actual window (so way files stay portable across model generations and frameworks).

Two forms are accepted:

```yaml
refire: 0.15         # direct: half-life = 15% of session window
```
```yaml
refire: normal       # preset: resolved via config.refire_presets
```

- **Numeric form** (`0.0 – 1.0+`) pins the cadence to today's model. Use when you want precise control or when the intent is model-specific.
- **Preset form** (string name) looks up the project's `refire_presets` config section. Built-in defaults: `once` (1.0), `rare` (0.4), `normal` (0.15), `frequent` (0.05). Use for portability — re-tuning happens globally via one config edit.

Common choices (numeric ↔ preset, matching the built-in defaults):

| Intent | Numeric | Preset |
|---|---|---|
| Static-heavy payloads (heuristic tables, long checklists) | `0.4` | `rare` |
| Load-bearing guidance (typical case, ~3 fires per session) | `0.15` | `normal` |
| Procedural event handlers (fires often relative to session) | `0.05` | `frequent` |
| Disclose once per session | `1.0` | `once` |

Numeric values between these presets are fine — for example, the 14 ways migrated from the PR #70 1M-Opus narrow-tune (ADR-126) sit at `refire: 0.2` (between `normal` and `rare`), deliberately pinned to today's model.

Missing `refire:` on a fire-bearing way means the firing gate refuses it, so it never reaches the agent; only a Task dispatch to a subagent bypasses the gate. `ways author lint` reports it as an error. Use `refire: once` for a way that should fire once per session. Check files and `trigger: attend` handlers are exempt (checks ride on parent way firing; attend handlers are signal-triggered).

The legacy `curve:` block (ADR-123) is no longer part of the schema. Writing `curve:` in new ways will trigger a lint UNKNOWN/foreign-field warning.

## See Also

- knowledge/authoring(meta) — parent: way format and the other frontmatter fields
