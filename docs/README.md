# Documentation

Map of the documentation tree. For the project overview, see the [main README](../README.md).

## Where to find what

| Path | What's there |
|------|-------------|
| [cognitive-loop.md](cognitive-loop.md) | **Start here for the whole system** — narrative explainer of how ways, progressive disclosure, memory, and the awareness layer compose as one cognitive loop (with diagrams) |
| [hooks-and-ways/README.md](hooks-and-ways/README.md) | **Start here for ways specifically** — how ways work, reading paths by role |
| [hooks-and-ways/](hooks-and-ways/) | Guides: creating ways, matching, macros, provenance, teams |
| [hooks-and-ways.md](hooks-and-ways.md) | Reference: hook lifecycle, state management, session gating |
| [architecture.md](architecture.md) | System architecture diagrams (Mermaid) for the ways mechanics |
| [architecture/](architecture/) | Architecture Decision Records (managed by `docs/scripts/adr`) |
| [design-notes/](design-notes/) | Prose-first framing documents that justify multiple related decisions (complement to ADRs) |
| [explanation/](explanation/) | Diátaxis explanation pages — conceptual walkthroughs grounded in real scenarios (install topologies, localization, attend messaging, and [how ways works](explanation/how-ways-works/how-ways-works-the-model.md) — the observable companion to cognitive-loop.md) |

## Guides vs Reference

Two things named `hooks-and-ways` — they serve different layers:

- **`hooks-and-ways/`** (directory) — guides for practitioners. "How do I create a way?" "How does matching work?"
- **`hooks-and-ways.md`** (file) — reference for contributors. Hook lifecycle, state diagrams, internal mechanics.

## Governance

Same pattern: guide, reference, implementation.

- **[../governance/README.md](../governance/README.md)** — guide. Getting started, operator commands.
- **[governance.md](governance.md)** — reference. Compilation chain, data flow, tool mechanics.
- **[../governance/](../governance/)** — implementation. Scripts, policies, manifests.

| Path | What's there |
|------|-------------|
| [governance.md](governance.md) | Reference: compilation chain, data flow, tool internals |
| [../governance/README.md](../governance/README.md) | Guide: getting started, operator commands |
| [../governance/policies/](../governance/policies/) | Policy source documents (governance chain targets) |
| [hooks-and-ways/provenance.md](hooks-and-ways/provenance.md) | How-to: adding provenance to your ways |

## Other docs

| Path | What's there |
|------|-------------|
| [prerequisites-*.md](.) | Platform install guides (macOS, Arch, Debian, Fedora) |
| [images/](images/) | Theme imagery |
