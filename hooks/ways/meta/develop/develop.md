---
description: How we carry a piece of work through the development loop — a variable front (design, prototype, ADR, ordered by where the uncertainty lives) and a stable tail (build, review, fix, merge). Route to the /develop skill, which picks the shape and borrows the stage skills rather than reimplementing them.
vocabulary: develop development loop workflow build feature implement carry work through iterate design prototype adr plan review fix merge order sequence what first where to start shape front tail claim evidence uncertainty method process the way we work
pattern: /develop\b|develop (this|the|a|our)|work through (this|the|a)|(build|tackle) (this|the) feature|the (dev|development) loop|how (do|should) we (build|develop|approach|tackle)|where (do|should) (we|i) start|what (comes |do we do |should we do )first
refire: 0.2
scope: agent
---
<!-- epistemic: convention -->
# Developing

This is the core loop — the working self between `start` (open the session) and
`wrap` (close it). The **`develop` skill** carries a piece of work through the loop;
this way is *when and why* to reach for it, and the one idea it turns on.

## Variable front, stable tail

The loop is not one fixed order. Its **tail is stable** — build → review → fix →
merge runs the same way almost every time. Its **front is variable** — design,
prototype, and ADR reorder depending on the work, and forcing one order is the
mistake:

- **Prototype-first** when the load-bearing claim is *outside your repo* — an external API's behavior, a latency budget, a payload size. Reasoning can't settle it; probe the real system, then record what you measured. (`prototype`)
- **Design-first** when the trade-offs are *open* — several plausible shapes, none yet obviously right. Deliberate, converge, then commit. (`design`)
- **ADR-first** when the point *is* the direction — you already know the shape and need the decision on record before anyone builds on it. (`adr`)

Pick the front order by **where the uncertainty lives** — the same uncertainty-
location map `core.md` uses (in the artifacts, in the instructions, in the external
world). The stage that answers the load-bearing question goes first.

## ADR is part of the method, not the whole of it

This loop is deliberately *not* "ADR-driven development." Prose — ADRs, design notes, specs — states **claims**; claims are held to the evidence the running system produces. An ADR accepted from reasoning alone about an external system is an aspiration wearing a decision's clothes. Record the decision, then earn it: a passing test, an exercised flow, a measured number. The ADR is one stage among several.

## `develop` borrows; it does not re-teach

`develop` establishes the loop, selects the front order, and lays the TaskList — then it **delegates**. Each stage already has its own way and, where it runs a procedure, its own skill: design, prototype, adr, implement/plan, code review, `merge`, `release`. `develop` calls those; it does not reimplement them. When you hit a stage, that stage's way discloses on its own. Keep `develop` thin — a router over the corpus you already have, not a monolith that swallows it.

## See Also

- develop (skill) — the procedure this routes to.
- architecture/design(softwaredev) — the design-first front.
- architecture/design/prototype(softwaredev) — the prototype-first front, for external claims.
- adr(documentation) — the adr-first front, when direction is the point.
- delivery/implement(softwaredev) — the plan/briefing stage before the build.
- delivery/merge(softwaredev) — the stable tail: the review gate and landing.
- start(meta) / wrap(meta) — the session bookends this loop runs between.
