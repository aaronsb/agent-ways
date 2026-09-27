---
status: Accepted
date: 2026-06-21
deciders:
  - aaronsb
  - claude
related: []
---

# ADR-138: Skills own the how, ways own the 5W

## Context

An agent trying to vendor the doc-management tools (`adr`, `doc`) into a fresh
project couldn't reliably find the install procedure. Investigation showed the
way+macro disclosure path actually works — in a real empty project the
`adr`/`documentation` ways fire and their macros surface the "tooling available"
observation. But the *procedure itself* — the `cp` of `adr-tool`/`doc-tool` from
`~/.claude`, the copy-not-symlink rule, the `doclint.py` pairing — had been
copied into **four** places: the two macros, the `migration` way, and the
`project-init` command. The `adr` and `docs` **skills**, which are the
front-and-center entry points (they trigger on "create an ADR", "new doc page")
and bypass the macro system entirely, carried *none* of it. An agent reaching the
tooling via the skill in an un-provisioned project hit a dead end.

The deeper issue is a missing authoring boundary. Without a rule for what belongs
in a skill versus a way, an imperative procedure leaks into every artifact that
references it and drifts — exactly the consistency drift the freshness way warns
about. We need one home for each procedure, and a principle that says where.

## Decision

Adopt a single authoring boundary across the corpus:

- **Skills own the *how*** — the executable procedure. One canonical, ideally
  **parametric** home per procedure (one skill with modes, not N near-duplicate
  skills). Skills are invoked deliberately and must be self-sufficient: they may
  not assume a way fired first.
- **Ways own the *who / what / where / when / why*** — disclosure and context.
  A way observes state ("this tool isn't installed and should be / its state is
  inconsistent"), explains why it matters, and fires at the right moment. A way
  (or its macro) must **point to** the skill for the procedure; it must not be the
  canonical home of one.

A way's macro is an *observation*, not an instruction sheet. When it detects a
gap, it names the gap and hands off to the skill.

This change implements the boundary for doc-tooling vendoring as the motivating
prototype: the `cp` procedure now lives only in the `adr` and `docs` skills; the
macros, `migration` way, and `project-init` command all defer to them. The
principle then governs the wider sweep — consolidating clone-skills into
parametric ones and relocating any *how* still smuggled inside a way.

## Consequences

### Positive

- One home per procedure — drift surface collapses from four copies to one.
- Skills become safe to invoke standalone; the dead-end on the skill path closes.
- A clear test for every future artifact: imperative steps → skill; observation,
  rationale, timing → way. Authoring decisions stop being ad hoc.
- Parametric consolidation reduces the skill count and the surface to maintain.

### Negative

- Existing clone-skills and ways-carrying-procedures need a migration sweep.
- A way that observes a gap now costs one extra hop (way → skill) to act on it,
  versus an inline command.

### Neutral

- Pairs with the prototype-before-accept principle: vendoring was prototyped,
  this ADR records the intent; acceptance follows the sweep.
- Suggests a companion meta-way on *authoring skills vs ways* that fires when
  editing `skills/` or `hooks/ways/`, enforcing the split going forward.

## Alternatives Considered

- **Add the procedure to the skills but leave the other three copies** — closes
  the reported dead-end but keeps the four-way drift. Rejected: treats the symptom.
- **Keep the how in the macros, point skills at the macros** — inverts the natural
  roles (a deliberate-invocation artifact depending on a passive disclosure one)
  and macros can't be invoked on demand. Rejected.
- **Memory note instead of an ADR** — a cross-cutting authoring principle that
  changes how every skill and way is written is architecture, not a narrow fact.
  Memory would bypass the friction this decision deserves.
