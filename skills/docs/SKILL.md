---
name: docs
description: Author and manage documentation-catalog pages using the project's doc CLI tool. Use when the user wants to write, scaffold, list, or lint catalog docs (tutorials, how-to guides, reference, explanation), or asks "write docs for X", "new doc page", "what docs exist", "document this". Pairs with the adr skill — docs and ADRs share domain bands.
allowed-tools: Bash, Read, Grep, Glob
---

# Documentation Catalog Authoring

Operate catalog pages through the `docs/scripts/doc` CLI tool. **Never hand-write
catalog frontmatter** — the tool computes the `DD.NNN.P` id, picks the next
in-domain serial, and emits a lint-clean page. Reverse-engineering the id scheme
from the linter is the smell this skill exists to remove. If the tool isn't
present in the project yet, vendor it first — see [Vendoring the tool into a
project](#vendoring-the-tool-into-a-project).

Docs and ADRs are **one typed graph** sharing domain bands (see the project's
documentation-catalog ADR). This skill is the docs half; the `adr` skill is the
decisions half.

## Commands

```bash
# Discover
docs/scripts/doc domains                       # domain bands (shared with ADRs)
docs/scripts/doc coverage                       # domain × mode matrix — where the gaps are
docs/scripts/doc list [--domain D] [--mode M]   # list catalog pages
docs/scripts/doc gaps                           # empty cells + doc/ADR imbalance

# Create
docs/scripts/doc new <domain> <mode> "Title"    # scaffold (correct id, frontmatter, H1)
#   mode ∈ tutorial | how-to | reference | explanation
#   --dir <subdir>   land it somewhere specific (may nest, e.g. explanation/attend-messaging)
#   --related ADR-NNN / --related DD.NNN.P   add edges up front (repeatable)

# Maintain
docs/scripts/doc lint [--strict]                # lint the catalog graph (the test)
```

## Workflow

1. **Classify the mode first.** Decide Tutorial / How-to / Reference / Explanation
   *before* scaffolding — it's the one judgment the tool can't make. The
   **diataxis** way carries the 2×2; the short form: *studying vs working* ×
   *practical steps vs theoretical knowledge*. One mode per page.
2. **Check domains**: `docs/scripts/doc domains` — domains and bands vary per project.
3. **Scaffold**: `docs/scripts/doc new <domain> <mode> "Title"`. The tool assigns
   the next id and writes the frontmatter; you never type an id by hand.
4. **Write the body** faithful to its mode (a tutorial teaches; a reference
   describes; don't blend).
5. **Link edges**: add `related:`/`supersedes:` as Obsidian wikilinks
   (`[[ADR-136]]`, `[[01.003.E]]`) — these are the graph edges the linter checks.
6. **Lint**: `docs/scripts/doc lint` before committing (dangling edges, malformed
   ids, mode/pole mismatches all surface here).
7. **Validate after a substantial change** (a new page set, a rewritten tutorial,
   a moved entry point). Lint says the graph is well-formed; a cold read says the
   pages work. The **validate** way (`documentation/validate`) carries the
   method: spawn a fresh agent (never a fork), hand it only the entry point, have
   it declare what it loaded, ask a few newcomer questions plus one false-premise
   question, and grade every wrong answer as a docs defect.

## Page format

The tool generates catalog frontmatter — identity is the `id`, not the path:

```markdown
---
id: 01.003.E          # DD.NNN.P — domain band . serial . mode pole
domain: system
mode: explanation     # tutorial=T  how-to=H  reference=R  explanation=E
related: []           # wikilink edges: [[ADR-136]], [[01.001.E]]
aliases: []
---

# Title
```

## Vendoring the tool into a project

`docs/scripts/doc` is **not** part of a project by default — it's vendored from
the agent-ways install. When it's missing (the `documentation` way's macro will
observe this and remind you), install a standalone **copy** — never a symlink, as
a symlink into `~/.claude` breaks for collaborators and CI:

The tools are a **pair**: `doc` (the librarian) does `import doclint` and shells
out to a sibling, so the linter **must be copied as a file named `doclint.py`
beside `doc`** — not renamed, not symlinked into `~/.claude`.

```bash
mkdir -p docs/scripts
cp ~/.claude/hooks/ways/documentation/linting/doc-tool docs/scripts/doc
cp ~/.claude/hooks/ways/documentation/linting/doclint.py docs/scripts/doclint.py
chmod +x docs/scripts/doc docs/scripts/doclint.py
# Optional: a relative, in-repo symlink so `doclint` works as a bare command
# (points at a sibling that travels with the repo — NOT into ~/.claude):
ln -s doclint.py docs/scripts/doclint
```

The catalog **requires `docs/architecture/adr.yaml`** — it reuses the ADR domain
bands. If that's missing, vendor the ADR half too (see the **adr** skill).
Validate: `docs/scripts/doc coverage && docs/scripts/doc lint`. For a full repo
scaffold, run `/project-init`. To decline the catalog for a project:
`touch .claude/no-doc-tooling`.

## Key rules

- **Always use the CLI** — never hand-author `id`/`domain`/`mode` frontmatter.
- **One mode per page** — if it won't fit one Diátaxis quadrant, it's two pages.
- **Identity is the id, the path is a view** — the filesystem is a serialization;
  move/rename freely, the `id` is what the graph keys on.
- **Lint before commit** — `docs/scripts/doc lint` is the catalog's test.

## Not for

- Architecture Decision Records — that's the **adr** skill (the decisions half of the same catalog).
- Hand-authoring `id`/`domain`/`mode` frontmatter — the CLI computes it.
- Prose that isn't joining the catalog — untagged docs need no tool and are never linted.

## See also

- the **diataxis** way — picking the mode (the 2×2)
- the **documentation** way — the typed-graph model
- the **validate** way — the cold read after a substantial docs change
- the **adr** skill — the decisions half of the same catalog
