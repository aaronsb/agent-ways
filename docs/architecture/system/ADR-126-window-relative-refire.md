---
status: Draft
date: 2026-04-22
deciders:
  - aaronsb
  - claude
related:
  - ADR-115
  - ADR-121
  - ADR-123
---

# ADR-126: Window-relative refire with named presets

## Context

Way frontmatter currently carries a raw token count for refractory decay:

```yaml
curve:
  type: Exponential
  half_life: 30000
```

That number was calibrated when Claude Code's context window was 200k and
compaction forced rotation at ~160k. In a 200k session, `half_life: 30000` gave
ways ~3 re-fire chances across the full session — the intended "load-bearing
but not spammy" cadence.

The context window has since grown. Opus-4 runs 1M-token sessions. In a
4.3-hour, 1M-token session on 2026-04-20, the `softwaredev/code/quality` way
fired 5 times — its static heuristic table and rationalization table were
re-injected into context roughly every 200k tokens. The reporter described this
as "light cognitive overhead per incident, cumulative across the session." The
mechanism is working as designed; the tuning is stale.

The stale-tuning problem is not a one-time mistake that a sweep fixes. Every
context-window expansion re-invalidates every hand-tuned `half_life` in the
tree. With 95 ways at `half_life: 30000` today, each future window change is a
95-file sweep against a moving target.

The deeper issue is that `half_life` as a raw token count mixes two concerns in
the frontmatter:

1. **Author intent** — "this payload should re-disclose rarely" vs "this
   payload fires on each new occurrence of its trigger."
2. **Host calibration** — "what does 'rarely' mean in tokens given this
   session's context window?"

Authors are forced to know the host's context window to express intent. That
coupling breaks every time the host's window changes.

ADR-123 already made ways' axis unit-agnostic at the engine level:
`EngagementState` consumes a monotonic `Tick`, and callers supply the unit.
Callers in practice supply `get_token_position(session_id)` — token position on
a known context window. That's enough primitive to express intent as a
fraction rather than a count.

A narrow-tune remediation shipped on 2026-04-22 (PR #70, ADR-127): 14
static-heavy ways in `softwaredev/code/*`, `docs/standards`, and `architecture`
parents bumped from `half_life: 30000` → `half_life: 200000`. That addressed
today's pain for the confirmed problem class without prejudging this ADR. The
sweep was a patch; this ADR is the structural fix.

## Decision

Introduce a `refire:` frontmatter field that accepts either a number or a
preset name. The engine resolves to a concrete `half_life` in caller ticks at
fire-evaluation time, so ADR-123's unit-agnostic boundary is preserved.

### Frontmatter shape

`refire:` accepts two forms. Both are valid, choice expresses intent:

```yaml
refire: 0.2           # direct: fraction of context window, pinned to today's model
```
```yaml
refire: rare          # reference: tracks the project's preset config
```

- A **number** in approximately `[0.0, 1.0+]` is interpreted as a fraction of
  the session's context window. Half-life = `refire × window_size`. Writing a
  number is an explicit choice to pin the cadence to today's model — it does
  not auto-scale if the operator later swaps to a model with different
  attention characteristics.
- A **string** is looked up in the `refire_presets` config section at fire
  time. Unknown names fail closed at two upstream gates — `ways lint` and
  `ways corpus` both reject unknown preset names and abort/warn. Fire-time
  resolution has a fail-soft fallback to `normal` (0.15) with a stderr
  warning, so a bypassed lint doesn't crash a live session, but the error
  path is intended to be caught upstream.

### Preset configuration

A new section in the existing config file (`$XDG_CONFIG_HOME/ways/config.yaml`
with `$PROJECT/.claude/ways.yaml` overlay — see ADR-115):

```yaml
# Refire presets. Each is a fraction of the session context window.
# Half-life = preset × context_window. See ADR-126.
refire_presets:
  once: 1.0       # effectively once per session (never re-fires before end)
  rare: 0.4       # static-heavy, 1–2 fires per session
  normal: 0.15    # load-bearing, ~3 fires per session (matches pre-ADR default)
  frequent: 0.05  # procedural, fires often relative to session
```

Built-in defaults ship with these four presets. User and project configs can
override individual values or add new preset names (e.g., `perpetual: 0.01`).
No schema migration needed.

### Portability

**Framework portability (strong claim).** Expressing refire as a fraction of
session capacity generalizes across any agent harness that can report its
capacity. Every agent harness has a finite context, and fractions generalize
over finite capacities by construction. A way tagged `refire: 0.15` means
"15% of whatever this framework calls a session" and works wherever
`session_capacity()` is callable. Ways become portable across LLM vendors and
agent frameworks, not just across Anthropic model generations.

**Model-generation portability for presets (weaker sub-claim).** The preset
table embeds an additional assumption: the relative cadence between presets
(`rare` < `normal` < `frequent`) stays roughly portable across Anthropic model
generations, even as absolute attention characteristics shift. If a future
model shows materially different mid-context recall, the preset values are
re-tuned globally in the config file — no way-file sweep. If the sub-claim
breaks (e.g., one model wants the *ordering* inverted), the presets move to
per-model tables. Not needed now; structurally available later.

Authors who don't trust the preset sub-claim for a specific way write a
number instead of a name. That's the escape hatch.

### Resolution semantics

Preset names resolve **at fire time**, not parse time. The flow:

1. Frontmatter loader reads `refire:` as an enum: `Numeric(f64)` or `Preset(String)`.
2. Each fire evaluation:
   - Fetch current context window via `cmd/context.rs::model_to_window()`.
   - If `Numeric(v)`: `half_life = (v × window).round() as u64`.
   - If `Preset(name)`: look up in `config::global().refire_presets[name]`,
     then multiply by window.
   - Build a fresh `Curve::Exponential { half_life }` and hand it to the engine.
3. Engine (sensor-trait) sees only concrete `Curve::Exponential`. No new
   variant. No context-window threading through `way_fire_outcome`.

Fire-time resolution means config edits take effect mid-session — operators can
tune presets, re-run, observe, without restarting. The config file is tiny;
re-reading or mtime-caching per fire is negligible.

### Engine changes

None required to `sensor-trait`. The resolver lives entirely in `ways-cli`.
`Curve::Exponential` is the only shape the engine ever sees for refire-bearing
ways. `attend`'s exhaustive matches on `Curve` are untouched.

### Frontmatter and lint changes

1. **`frontmatter.rs`** — add `RefireSpec` enum:

   ```rust
   pub enum RefireSpec {
       Numeric(f64),
       Preset(String),
   }
   ```

   Parse `refire:` as a scalar; if it's a float, `Numeric`; if it's a string,
   `Preset`. If `curve:` is also present, `refire:` wins and lint emits a
   warning about the duplication.

2. **`config.rs`** — extend `Config` with `refire_presets: HashMap<String, f64>`,
   populated from the new YAML section. Built-in defaults match the table
   above.

3. **`ways lint`** — four diagnostics covering presence, shape, and drift:
   - **Warning** when a fire-bearing way (any trigger channel wired:
     description+vocabulary, pattern, files, commands, or trigger; not an
     attend handler; not a check file) has no `refire:` field.
   - **Warning** when both `refire:` and legacy `curve:` coexist, pointing
     authors at the `curve:` block to remove.
   - **UNKNOWN (foreign-field warning)** when a `curve:` block is present
     alone. The schema no longer lists `curve:` as a valid field, so the
     existing unknown-field logic flags it without special-case code.
   - **Error** when `refire:` is malformed — a numeric value outside
     `[0.0, 10.0]` (e.g., a raw token count like `30000` accidentally
     pasted in) or a preset name not present in `config.refire_presets`.
     Fail-closed: lint is the primary typo gate.

4. **`ways corpus`** — echoes the malformed-refire check as a stderr
   WARNING during corpus generation. Corpus is run frequently (CI, local
   rebuilds) and hits every way file, so catching typos here prevents them
   from reaching a live session even when lint isn't invoked.

   These checks give authors an unambiguous signal about whether a way file
   conforms to the post-ADR-126 shape at two independent gates (lint +
   corpus), with a fire-time fallback that keeps sessions running even
   when both gates are bypassed.

## Migration

**Phase 1 — engine and parser (now).** Ship `RefireSpec` parsing, config
extension, fire-time resolution. Accept both `refire:` (new) and `curve:`
(legacy) unchanged. No way files change yet.

**Phase 2 — mechanical numerical conversion.** Convert each way using its
window-at-tuning-time as the reference:

```
refire = half_life / window_at_tuning_time
```

Two buckets exist in the tree:

- **The 14 files tuned on 1M Opus** (ADR-127 narrow-tune, committed in
  PR #70 `f93bb74`): `code.md`, `quality.md`, `errors.md`, `performance.md`,
  `security.md`, `auth.md`, `injection.md`, `secrets.md`, `supplychain.md`,
  `mocking.md`, `tdd.md`, `testing.md`, `architecture.md`, `standards.md` →
  `refire = half_life / 1_000_000` → `refire: 0.2`.
- **All other ways, still at 200k-era values**: `refire = half_life / 200_000`
  → `refire: 0.15` for `half_life: 30000`, proportional for other values.
- **Raw `curve:` blocks** for `Flat`, `ActionPotential`, `ProgressiveStaircase`
  stay untouched — `refire:` is specifically the Exponential shorthand.

The rule captures each author's real intent at the moment of tuning as a
fraction of the window they were thinking in. This preserves original design
intent across the tree.

Side-effect: the 81 unhacked files that are currently broken on 1M (firing
~22 times per session instead of the designed ~3) will fire ~4 times once the
resolver multiplies `refire: 0.15` by the actual 1M window. An unintentional
but welcome fix, consistent with the intent the author expressed when they
wrote `half_life: 30000` against a 200k window.

Second-order effect: three attend handlers (`meta/attend/build-complete`,
`context-pressure`, `reflection-overdue`) inherited `refire: 0.15` from the
migration. On 1M Opus, their effective suppression grows from 30k to 150k
half-life — a signal-debounce change, not a disclosure-decay change. This may
be too sticky for rapid-signal handlers; individual attend handlers can be
re-tuned per-signal (e.g. `refire: 0.03` to preserve the pre-migration 30k
behavior on 1M). Separate concern from the primary way-disclosure cadence
this ADR addresses.

**Phase 3 — opt into presets (per-way judgment).** For ways whose intent is
genuinely model-portable ("be load-bearing in any session"), authors replace
the number with a preset name. This is per-way authorial judgment, not a
mechanical sweep. The lint suggestion surfaces candidates.

**Phase 4 — deprecate raw `half_life:`.** `ways lint` promotes the
`half_life:`-in-frontmatter warning to an error in a later minor release. Raw
`curve:` blocks remain valid for non-Exponential shapes.

## Consequences

### Positive

- Way files stop encoding host-specific token counts when the author wants
  portability. The translation to tokens happens at fire time.
- Context-window growth no longer invalidates portable-intent tuning. A future
  2M-token model re-calibrates every preset automatically.
- Mis-tuned ways become observable at the field level — a way tagged `once`
  that fires 10 times is a preset mis-match, not a magic-number arithmetic
  error.
- The preset table is a single-file tuning surface. If cadence needs global
  adjustment, it's one config edit, not 95 files.
- Numerical and preset forms coexist. Model-specific tuning stays available
  without forcing every way through the preset table.
- Config-edit-mid-session lets operators tune presets interactively.
- Engine boundary preserved. Sensor-trait unchanged. `attend` unaffected.

### Negative

- Small parser complexity: `refire:` accepts two types. Lints must detect
  numeric-matches-preset and unknown-preset-name cases.
- Fire-time resolution re-reads or mtime-caches the config file. Overhead is
  negligible but non-zero.
- The weaker preset sub-claim (relative cadence stable across models) is
  unverified empirically. First re-tuning against a new model will either
  validate it or force per-model preset tables. The stronger framework
  portability claim is essentially definitional and doesn't need validation.
- Requires `model_to_window()` to cover the operator's model. Unknown models
  fall back to a default window; an explicit `CLAUDE_CONTEXT_WINDOW` env
  override is needed for unknown models. Silent fallback can produce
  surprising behavior — `ways lint` should warn when model is unrecognized.

### Neutral

- Raw `half_life:` inside `curve:` blocks remains valid indefinitely as an
  escape hatch, primarily for non-Exponential shapes.
- Preset vocabulary is fixed at four values by default. Users add custom
  names in their config (`perpetual`, `transient`, project-specific names)
  without a schema change.

## Alternatives Considered

### Keep raw `half_life`, sweep values periodically

Operationally cheapest right now (ADR-127 the narrow-tune sweep is exactly
this). Rejected as the primary answer because the tuning debt recurs with
every context-window change. The sweep is a patch, not a fix.

### Bands only (named presets, no numeric form)

Earlier draft of this ADR. Rejected after gaming out the migration: the
2026-04-22 hack (`half_life: 200000` on a 1M window → sigma 0.2) doesn't map
cleanly to any band. Forcing every way through a band loses fidelity where
authors have already tuned carefully. The numeric form is the primitive;
presets sit on top as optional portability.

### Numeric only (no presets)

Simplest possible shape. Rejected because it forces every future
model-generation change into a 95-file sweep — exactly the problem this ADR
is trying to eliminate. Presets are the portability layer.

### "Fires per session" as the author-facing unit

`fires_per_session: 3` is the most intent-aligned expression. Rejected because
it's coupled to the refire floor (currently 0.35) — if the floor changes
later, the meaning of `3` shifts. Fractional fires (`2.5`) are also awkward.
Fraction-of-window has cleaner semantics at the cost of slightly less
intuitive numbers.

### `ExponentialBanded { sigma }` as a new `Curve` variant

Original draft proposed this. Rejected because it breaks the unit-agnostic
engine boundary established by ADR-123 — `Curve` would need to know about
context windows. Resolving to `Curve::Exponential` at the ways-cli layer keeps
the engine pristine and is a much smaller diff.

### Per-model preset tables from day one

Rejected as speculative. The preset portability sub-claim is unverified;
premature per-model tables buy complexity before the premise is tested.
Structurally available if needed (the config loader can grow a per-model
key), but not the initial design.

### Mechanical divide-by-1M for every file

Tempting for its simplicity — one rule, zero judgment. Rejected because it
conflates the value with the intent. Files tuned on 200k (the 81 unhacked
ways) have `half_life: 30000` meaning "re-fire ~3 times per 200k session."
Dividing by 1M gives `refire: 0.03`, which preserves the broken 22-fires
behavior currently observed on Opus rather than the original 3-fires intent.
Using each file's window-at-tuning-time as the reference captures intent
faithfully at a cost of two buckets in Phase 2 (hacked and unhacked).
