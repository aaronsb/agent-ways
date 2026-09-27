---
status: Accepted
date: 2026-07-04
deciders:
  - aaronsb
  - claude
related:
  - ADR-123
  - ADR-126
  - ADR-134
---

# ADR-159: Remove ways tune-curves and the legacy curve: cadence field

## Context

`ways tune-curves` (ADR-123 Phase E) reads fire telemetry, computes the median
token-distance between a way's firings, and — with `--apply` — rewrites each
way's frontmatter to a `curve:` block carrying an absolute `half_life`.

That output is now **broken**. ADR-126 replaced the `curve:` block with
`refire:` (a *fraction of the context window*, resolved per fire against the
model's actual window). The migration moved every way to `refire:`, `curve:` was
dropped from the frontmatter schema, and `ways lint` flags a written `curve:`
block as an UNKNOWN field. So `ways tune-curves --apply` produces frontmatter
that the project's own linter rejects — a command that corrupts way files.

The command is not worth repairing:

- **Its model is superseded.** It suggests an absolute `half_life` in tokens.
  `refire:` is deliberately a *fraction*, so way files stay portable across model
  window sizes (ADR-126). Translating an observed token cadence into a fraction
  requires dividing by the window it was observed under — reintroducing exactly
  the model-specific coupling ADR-126 removed. A "portable" fraction derived from
  one model's window is a fiction.
- **Its successor is a different, deferred design.** Telemetry-driven cadence
  tuning is ADR-134 (empirical auto-tuning from fire/near-miss streams), which is
  deferred. `tune-curves` is a half-built manual precursor to it, not a
  standalone capability worth carrying.
- **Nothing uses the legacy path.** No shipped way carries a `curve:` block; the
  schema rejects it. The `curve:` frontmatter field and its read-fallback are
  dead code kept alive only for a command that produces lint-failing output.

## Decision

Remove `ways tune-curves` and retire the legacy `curve:` frontmatter field
entirely.

- Delete the `tune_curves` command, its module, and its CLI wiring.
- Remove the `Frontmatter.curve` field and its read-fallback in
  `resolved_curve` — resolution now comes solely from `refire:`.
- Remove the now-dead `curve:` readers (`show`, `list` doc comments) and the
  lint special-case that warned about `refire:`+`curve:` coexistence; a stray
  `curve:` block falls through to the generic UNKNOWN-field warning, which is the
  correct treatment for a retired field.
- Update the docs that presented `tune-curves` as a workflow (`stats.md`,
  `reference/ways-cli.md`) and any `curve:`-as-current references.

**Kept:** the runtime `Curve` type and `RefireSpec::to_curve`. `Curve` is the
concrete decay representation that `refire:` *resolves into* at fire time
(`fraction × window → Curve::Exponential`); it is the engine's internal shape,
not the retired authoring field. Retiring `curve:` the *frontmatter field* does
not touch `Curve` the *runtime type*.

## Consequences

### Positive

- No command can emit lint-failing frontmatter; the footgun is gone.
- One cadence model, not two: `refire:` is the sole authored cadence field, with
  no dead legacy path shadowing it in the parser, `show`, `list`, and lint.
- Less code to carry toward the eventual ADR-134 auto-tuner, which will target
  `refire:` directly rather than inheriting `tune-curves`' `half_life` model.

### Negative

- Users lose the observed-cadence *suggestion* helper. In practice `refire:` is
  a small, human-judged knob (the `once`/`rare`/`normal`/`frequent` presets), and
  the telemetry that fed `tune-curves` still exists for the future ADR-134 work.
- Removing a shipped subcommand is a visible CLI surface change (documented
  here and in the release notes).

### Neutral

- The frontmatter schema already excludes `curve:`; this change makes the code
  match the schema. Any hypothetical old file still carrying `curve:` now simply
  gets the UNKNOWN-field lint warning and falls back to the missing-cadence
  default, rather than being silently honored.

## Alternatives Considered

- **Fix `tune-curves` to write `refire:`** (translate `half_life` → window
  fraction). Rejected: the translation reintroduces the model-window coupling
  ADR-126 removed, and it invests in a manual precursor to the deferred ADR-134
  auto-tuner rather than retiring it.
- **Leave the command, only silence the lint.** Rejected: that legitimizes a
  retired field and keeps two cadence models alive; the lint is correct to reject
  `curve:`.
- **Delete the command but keep the `curve:` read-fallback.** Rejected: with no
  way using `curve:` and the schema rejecting it, the fallback is pure dead code —
  the kind of drift the project retires on sight.
