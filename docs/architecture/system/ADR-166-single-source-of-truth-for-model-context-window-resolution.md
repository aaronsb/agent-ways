---
status: Draft
date: 2026-07-12
deciders:
  - aaronsb
  - claude
related:
  - ADR-126
  - ADR-151
  - ADR-153
---

# ADR-166: Single source of truth for model context-window resolution

## Context

The context window is the denominator of nearly everything the toolchain reports
or decides. `ways context` divides by it to render the usage gauge. ADR-126 makes
way refire *window-relative*: a way's half-life is expressed as a fraction of the
window, so a wrong window rescales the entire disclosure curve. `sensor-peers`
divides by it to show peer session pressure.

Three separate implementations answered that one question, and they disagreed:

| Site | Rule |
|---|---|
| `ways-cli/src/cmd/context.rs:239` (`model_to_window`) | `opus-4` → 1M; `sonnet`\|`haiku` → 200K; else env override, else 200K |
| `ways-cli/src/session.rs:322` (`context_window_from_transcript`) | `opus-4` → 1M; else env override, else 200K |
| `sensor-peers/src/lib.rs:567` | `[1m]`\|`opus-4`\|`sonnet-4` → 1M; `-` → 0; else 200K |

A fourth site, `ways-cli/src/cmd/show/mod.rs:88`, hardcodes `.unwrap_or(200_000)`
on the failure path.

Each is a substring allowlist written against the model lineup of its day, and
each has since gone stale. The lineup they encode no longer matches the models in
use:

- `claude-fable-5` matches no branch in any of them and falls to the 200K default.
  It has a 1M window. A session observed 212,899 tokens with no forced compaction
  while `ways context` reported `tokens_total: 200000` and `pct_used: 106`.
- `claude-sonnet-5` has a 1M window; `context.rs` classifies it 200K on the
  `sonnet` substring.
- `sonnet-4` resolves to 1M in `sensor-peers` and 200K in `context.rs`. Both
  cannot be right.
- Conversely, `opus-4` is matched as a bare substring, so any `opus-4*` id is
  called 1M whether or not that is true of the specific model.

Two structural defects compound the staleness:

**The `[1m]` suffix is never present.** `sensor-peers` tests for it, but Claude
Code writes the bare model id to the transcript (`claude-opus-4-8`, not
`claude-opus-4-8[1m]`) — confirmed across ~78,000 model records in local
transcript history. That branch has never matched. Detection therefore cannot
observe the harness's window setting at all; it can only observe the model id.

**The documented override does not work where it is most needed.** In both
`context.rs` and `session.rs`, `CLAUDE_CONTEXT_WINDOW` is read only from the
fallback arm. Any model that matches a substring branch — every Sonnet and Haiku
in `context.rs` — ignores the override entirely, contradicting
`skills/context-status/SKILL.md`, which presents it as the general escape hatch.

The failure is silent by construction. A resolved window carries no indication of
whether it was detected or defaulted, so a wrong denominator is indistinguishable
from a right one at every consumer.

## Decision

Resolution of the context window becomes a single function in `ways-core`
(`ways_core::context_window`), and every site calls it. No consumer computes a
window itself.

**Resolution order**, applied in this order, first match wins:

1. `CLAUDE_CONTEXT_WINDOW`, if set and parseable — unconditionally, ahead of all
   detection. Detection cannot see the harness's active window (see Context), so
   the operator override must always outrank the model table, never merely
   backstop it.
2. An explicit model table, matched against the full model id rather than by
   loose substring.
3. A conservative 200K default.

**The result carries its provenance.** The resolver returns the window together
with a `WindowSource` (`EnvOverride` / `ModelTable` / `Default`), and
`ways context --json` emits it as `window_source`. A default is thereby reported
as a default rather than presented as a detection. This is the property that makes
the next stale-table failure observable instead of silent: the Fable session above
would have read `window_source: "default"` at the moment it was wrong.

**The table is explicit and enumerated**, not a substring heuristic. Substring
matching is what failed: `sonnet` swallowing `sonnet-5`, `opus-4` swallowing every
Opus 4.x regardless of window. Unknown models fall to the default and say so,
which is a correctable, visible state — unlike a wrong match, which is not.

Ids are matched as a **boundary-delimited component** of the model string rather
than anchored at its start, because other harnesses wrap the same id in provider
prefixes and version suffixes (`us.anthropic.claude-opus-4-8-v1:0`,
`claude-opus-4-8@20260115`) and this repo supports those deployments. A rule
anchored at byte 0 would regress every Bedrock and Vertex session to the default.
The boundary requirement is what keeps this from degenerating back into substring
matching: `claude-sonnet-5` is not found inside `claude-sonnet-55`, because the
trailing `5` is alphanumeric and therefore a different id, not a qualified form of
this one. Bare family aliases (`opus`, `sonnet`) are matched on **exact equality
only** — an alias is a whole model reference, not a family stem, and prefix-matching
one would resolve `claude-sonnet-4-5` to the current Sonnet's window and report it
as a confident detection.

**Sentinels are absences, not unknown models.** Claude Code writes
`"model": "<synthetic>"` for interrupt and API-error turns; `sensor-peers` uses `-`
as its no-model placeholder. A transcript whose newest assistant turn is an
interrupt still has a real model behind it, so the scanners skip sentinel turns and
keep walking back rather than resolving the sentinel to the default. Nine
transcripts in local history end on a `<synthetic>` turn; under a naive scan each
would have handed a live 1M session a 200K window.

**A peer's window is resolved without the operator's override.** `sensor-peers`
reads *other* sessions' transcripts, and `CLAUDE_CONTEXT_WINDOW` states the window
of the process that set it. Applying it to a peer would compute that peer's fill
against the observer's window — an operator with the override at 1M would see a
Haiku peer at 190K/200K, genuinely about to compact, rendered as 19% full. Foreign
sessions therefore resolve through the model table alone
(`resolve_for_foreign_session`).

The table is a hardcoded enumeration rather than a live Models API lookup
(`GET /v1/models/{id}` exposes `max_input_tokens`). The resolver runs in
`UserPromptSubmit` hooks on every turn; it must be synchronous, offline, and
credential-free. A network call on that path is not acceptable, and a cache of a
network call reintroduces the staleness this ADR exists to remove, with added
failure modes. The table is therefore accepted as a maintenance obligation at
model launch — made tractable by the fact that there is now exactly one of them.

## Consequences

### Positive

- One place to update when a model ships. The present bug required four edits in
  three crates to fix correctly, which is why it was never fixed at all.
- Way refire dynamics (ADR-126) are correctly scaled on every model. On Fable 5 the
  half-life had been computed against a 200K window inside a 1M one, compressing
  the disclosure curve by 5x.
- A wrong window becomes visible at the point of use via `window_source`.
- `CLAUDE_CONTEXT_WINDOW` behaves as documented, on every model.

### Negative

- The table must be updated when a model launches or a window changes. This is a
  real recurring obligation; nothing about the design removes it. It is bounded to
  one function and covered by tests that pin each known model id.
- An unknown model still resolves to 200K, which will be wrong for any future 1M
  model until the table is updated. It is reported as `window_source: "default"`,
  making it diagnosable, but a diagnosable wrong answer is still a wrong answer.

### Neutral

- `sensor-peers` takes a dependency on `ways-core`. Both are already workspace
  members, so this is a manifest line, not a structural change.
- The `[1m]` suffix test in `sensor-peers` was dead code and is removed. The
  marker appears in the *system prompt text* (and so, as prose, inside transcript
  message content — 414 occurrences locally), but never as a `message.model` value:
  across ~78,000 model records the field is always the bare id. Component matching
  nonetheless tolerates a `claude-opus-4-8[1m]` id, so if Claude Code ever does
  begin writing the marker, it resolves rather than silently defaulting.
- Models absent from the table now resolve to the default where the old `opus-4`
  substring gave them 1M — `claude-opus-4-5`, `claude-opus-4-1`, `claude-opus-4-0`.
  This is a correction, not a regression: the 1M window arrived with the 4.6
  generation, so calling Opus 4.1 a 1M model was exactly the over-broad match this
  ADR removes. None appear in local transcript history. They can be pinned
  explicitly once their true windows are confirmed.

## Alternatives Considered

- **Fix the four sites in place, keep them separate.** Rejected: it repairs this
  instance and preserves the mechanism that produced it. Three resolvers already
  drifted into three different answers for `sonnet-4`; nothing prevents a fourth
  divergence at the next model launch.
- **Resolve from the Models API at runtime.** Rejected: the resolver is on the
  per-turn hook path and must be synchronous, offline, and credential-free. See
  Decision.
- **Fetch the table from the Models API at build time.** Rejected for now: it
  moves the staleness from source to release cadence without removing it, and
  couples the build to network and credentials. Reconsider if the table proves to
  churn faster than releases.
- **Default unknown models to 1M rather than 200K.** Rejected: it is right for
  the current lineup but fails unsafely. Over-reporting the window suppresses way
  disclosure and under-reports usage — the gauge reads comfortable while the
  session is in fact near its limit. Under-reporting is the conservative error,
  and `window_source` makes it visible rather than silent.
