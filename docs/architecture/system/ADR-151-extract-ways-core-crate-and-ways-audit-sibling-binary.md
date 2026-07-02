---
status: Accepted
date: 2026-07-02
deciders:
  - aaronsb
  - claude
related:
  - ADR-110
  - ADR-111
  - ADR-142
  - ADR-200
supersedes: []
---

# ADR-151: Extract ways-core crate and ways-audit sibling binary

## Context

ADR-200 reframes the compliance subsystem into an operator-invoked claim/finding
formation layer, backed by a purpose-built toolkit rather than the core `ways` binary.
This ADR decides how that toolkit is packaged.

The relevant facts about the current code:

- `tools/` is a Cargo **workspace** (a set of crates built together), and shared
  library crates are already the house style here — `sensor-trait`, `agent-fmt`, and
  `agent-identity` are exactly that pattern: small libraries several binaries depend on.
- `ways-cli` is a **binary-only crate** (it produces the `ways` executable and has no
  `[lib]` target). The compliance logic — the `governance`/`provenance` modules, ~1,100
  lines — lives *inside* that binary crate and reaches into its private siblings
  (`crate::util`, `crate::cmd`). It is not a reusable library; it is trapped in the
  executable.
- That ~1,100-line engine has **zero tests**, including the integrity linter that is
  supposed to audit the claims. Being buried in a binary with no library boundary is
  part of why: there is no clean seam to unit-test against.
- ADR-111 consolidated a sprawl of shell scripts (`governance.sh`, `provenance-scan.py`,
  …) into the single `ways` binary. That was the right call for *script sprawl*. The
  question here is different: the compliance concern is a **distinct operator surface**,
  invoked deliberately (like `/ship` or `/wrap`), which the subsystem's own `governance/README.md` already sketches as
  separable (its "Making This Its Own Repo" section — a "perforated pop-out"). It should
  not be re-scattered — but it also
  should not bloat the core tool everyone runs.

So the packaging question: how does `ways-audit` reuse the engine without rewriting it,
keep the core binary lean, and give the untested engine a home worth testing?

## Decision

### 1. Extract a `ways-core` library crate

Pull the reusable engine out of the `ways` binary into a new **`ways-core`** library
crate in the workspace: way discovery and scanning, frontmatter parsing, path and
projection resolution, the firing-event log reader, and the claim (`provenance.yaml`)
sidecar model and manifest builder. Both `ways` (binary) and `ways-audit` (binary)
depend on it. This is the established house pattern (`sensor-trait` et al.), not a new
paradigm.

The extraction pays for itself independently of the compliance work: a library boundary
is exactly what makes the previously-untested engine unit-testable. `ways-core` ships
with tests; the core tool gets healthier whether or not anyone ever runs `ways-audit`.

### 2. Add `ways-audit` as a sibling binary

A new **`ways-audit`** binary crate, depending on `ways-core`, owns the whole compliance
surface:

- The reporting commands presently under `ways governance` (report, trace, control,
  gaps, matrix, lint), migrated and renamed to the compliance vocabulary of ADR-200
  (claim / finding / POA&M).
- The **finding pipeline**: read a claim + its firing events + the session transcript,
  produce an assessment finding with a determination, and append it to the finding
  ledger.
- The **claim-authoring assist**: propose candidate control mappings for a way
  (agent-assisted, human-grounded per ADR-200 §1).

The core `ways` binary **sheds** the `governance` subcommand. It goes back to being the
runtime — matching, hooks, session lifecycle — with the compliance concern living next
to it, not inside it.

### 3. The claim schema lives in `ways-core`, built for scale and assessability

`ways-core` defines the claim type stored in the `provenance.yaml` sidecar (ADR-110).
Per ADR-200 §1 the schema carries a **determination criterion** — the observable
behavior that would let an assessor mark the claim *satisfied* / *other than satisfied*
— so a claim is assessable rather than a bare assertion. `ways-audit` consumes that
type both to *assess* (produce findings) and to *suggest* (candidate mappings). Defining
it in the shared crate keeps the format single-sourced as claims are seeded across the
corpus at scale (ADR-200 §7).

### 4. Distribution reuses the per-component release machinery

`ways-audit` slots into the existing component-parameterized release flow: a
`ways-audit-v*` tag series, its own CI build job, and a download entry, exactly as
`ways` has. The marginal cost is one CI job and one download path — the same
prebuilt-binary machinery already planned for the other app binaries (ADR-142/ADR-146).

## Relationship to ADR-111

ADR-111 folded shell-script sprawl into one binary because the *abstraction layer* — one
tool, consistent surface — was the value. This ADR does not reverse that: it introduces
**one** additional binary for **one** cohesive, separable concern, and the two binaries
share a real library (`ways-core`) rather than duplicating logic. The distinction is
concern-separation with a shared abstraction, not a return to sprawl. ADR-111's own
"the abstraction layer is the value" argument is what `ways-core` embodies.

## Consequences

### Positive

- The core `ways` binary stays lean — the compliance concern is isolated and optional.
- `ways-core` gives the previously-untested engine a testable boundary; the extraction
  improves the main tool on its own merits.
- The claim schema is single-sourced and scale-ready, with assessability built in.
- Extraction to a standalone repo later (already sketched in `governance/README.md`) becomes a small
  move — the library seam is already drawn — without paying that cost now.
- Distribution is nearly free: the release/CI machinery already handles N components.

### Negative

- A real refactor: extract `ways-core`, move the modules off `crate::util`/`crate::cmd`
  onto the library API, and fix imports across the workspace.
- A second binary to build, ship, and version.
- `ways-core` now has a public API surface to keep stable for two consumers.

### Neutral

- `ways-audit` can version independently of `ways` (different cadence, own tag series).
- The `ways governance` command path is retired from the core binary; a one-release
  deprecation pointer to `ways-audit` can be kept if any operator scripted against it,
  though it was never a consumed surface.
- Repo extraction remains an available future option, deliberately not taken now.

## Alternatives Considered

- **Keep compliance inside the `ways` binary (status quo / strict ADR-111).** Rejected:
  it bloats the core runtime with an optional, deliberately-invoked concern, offers no
  separation seam, and couples the compliance release cadence to the core tool.
- **A new binary that copies the engine (no shared crate).** Rejected: duplication and
  drift. The workspace already solves this with shared library crates; not using one
  here would be the anomaly.
- **Extract the whole subsystem to a separate repository now.** Rejected as premature:
  a same-workspace sibling is cheaper, keeps the `ways-core` refactor in one place, and
  still leaves repo extraction open once the toolkit proves itself. Drawing the library
  seam now is the reversible half of that decision.

## References

- **ADR-200** — the compliance claim/finding model this toolkit backs.
- **ADR-110** — the `provenance.yaml` sidecar storage the claim schema extends.
- **ADR-111** — the single-binary consolidation this refines (one sibling, shared lib).
- **ADR-142 / ADR-146** — XDG application distribution and installer binary handling the
  new binary plugs into.
- **The Cargo Book — Workspaces** — the shared-crate mechanism used here.
  https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html
