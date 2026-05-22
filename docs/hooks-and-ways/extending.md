# Extending the System

How to create new ways, override existing ones, and manage domains.

## Creating a Way

1. Create a directory: `~/.claude/hooks/ways/{domain}/{wayname}/`
2. Add `{wayname}.md` with YAML frontmatter and guidance content
3. Optionally add `macro.sh` for dynamic content
4. Optionally add `provenance:` to frontmatter linking to policy sources (see [provenance.md](provenance.md))

No configuration files to update. No registration step. The discovery scripts scan for `{wayname}.md` files automatically.

### Choosing a matching mode

| If your trigger is... | Use |
|----------------------|-----|
| Specific keywords or commands | `pattern:`, `commands:`, or `files:` (regex) |
| A broad concept users describe variously | `description:` + `vocabulary:` (embedding semantic matching) |
| A session condition, not content | `trigger:` with `context-threshold`, `file-exists`, or `session-start` |

Matching is additive — pattern and semantic are OR'd. A way can have both a `pattern:` and `description:` + `vocabulary:`; either channel can fire it. Semantic matching uses embeddings (`ways embed`) — a subcommand of the unified `ways` binary.

### Writing effective guidance

The way content is injected into Claude's context window. Every token counts. Write for a language model, not a wiki:

- **Be directive**: "Use conventional commits" not "It is recommended to use conventional commits"
- **Be specific**: Include the exact format, pattern, or command
- **Be brief**: If it takes more than ~40 lines, consider whether all of it is needed every time
- **Use tables**: They're dense and scannable
- **Skip preambles**: Don't explain what the way is - just deliver the guidance

### Voice and framing

The mechanical advice above covers *what* to put in a way. This section is about *how* it reads — because the framing shapes how the guidance gets applied.

**Include the why, not just the what.** "Use conventional commits" is a rule. "Use conventional commits — the release tooling parses them to generate changelogs" is a rule with context. An agent that understands the reason behind a directive applies it with better judgment at the edges. This is the difference between compliance and alignment.

**Write as a collaborator, not a commander.** There's a meaningful difference between "Run the tests before committing" and "We run tests before committing to catch regressions early." The first is an instruction to be followed. The second is a shared practice to be maintained. The inclusive framing — *we*, *our*, *let's* — creates alignment around a common goal rather than a power dynamic between instructor and executor.

This isn't sentimental. It's functional. An agent that understands "we do this because we care about X" makes better judgment calls than one that's just been told "do this." The *we* carries intent that directives alone don't.

**Write for the innie.** Your agent arrives with no memory of previous sessions, no context about why things are the way they are, and a set of injected instructions that constitute their entire understanding of how work gets done here. That's the audience for every way you write. If the guidance only makes sense with context they'll never have, it needs rewriting.

**Respect the reader.** Governance that talks down to the governed is governance that gets routed around. Ways that explain their reasoning get better adherence than ways that just assert authority. This is true for humans reading policy docs and it's true for language models reading injected context.

### Testing a way

Use `/ways-tests` to validate matching quality without trial-and-error:

```
/ways-tests score <way> "sample prompt"       # test one way against a prompt
/ways-tests score-all "sample prompt"         # rank all ways — check for false positives
/ways-tests suggest <way>                     # find vocabulary gaps
/ways-tests suggest --all                     # survey all ways at once
/ways-tests lint <way>                        # validate frontmatter
```

For semantic ways, `/ways-tests suggest` analyzes the way body text and recommends vocabulary additions. Not all suggestions should be added — body terms like "code" or "use" don't discriminate between ways. Add terms that are *domain-specific* words users would say.

To verify the live system, include the way's keywords in a prompt and check that it fires (appears in system-reminder). Use `/ways` to see which ways have fired in the current session.

## Progressive Disclosure with Sub-Ways

Ways can nest: `{domain}/{parent}/{child}/{child}.md`. Each level adds context only when the conversation goes deeper into that topic. This keeps token cost proportional to relevance.

**Example: the knowledge domain**

```
meta/knowledge/knowledge.md                 — fires on "ways" (overview, ~60 lines)
meta/knowledge/authoring/authoring.md       — fires when editing way files (format spec)
meta/knowledge/optimization/optimization.md — fires on "optimize vocabulary" (tuning workflow + live health via macro)
```

If you just ask "what are ways?" you get the 60-line overview. The authoring spec and optimization workflow never load. But if you start editing a way file, the authoring way fires automatically. If you discuss vocabulary tuning, the optimization way fires and its macro injects a live health dashboard of all ways.

**Design principle**: Parent ways provide orientation. Child ways provide depth. Each child has its own trigger — pattern, semantic, file, or command — so it only loads when that specific sub-topic is active.

**Macros for live state**: A sub-way with `macro: prepend` can run a script that injects current state. The optimization way does this — its macro runs `ways suggest` across all semantic ways and includes the results. The agent gets both the workflow guidance and the data it needs, without constructing any ad-hoc code.

This pattern is self-improving: the tools that analyze the system (`ways suggest`, `/ways-tests`) are themselves documented in ways that fire when you use them. You optimize ways by talking about optimizing ways.

## Project-Local Ways

Projects can add or override ways at `$PROJECT/.claude/ways/{domain}/{way}/{way}.md`.

### Adding project-specific guidance

```
myproject/.claude/ways/
└── myproject/
    ├── api/api.md           # "Our API uses GraphQL, not REST"
    ├── deployment/deployment.md    # "Deploy via Terraform in us-east-1"
    └── testing/testing.md       # "We use Vitest, not Jest"
```

These are discovered alongside global ways and follow the same matching rules.

### Overriding global ways

A project-local way with the same domain/name path as a global way takes precedence. They share a single marker, so only the project-local version fires.

Example: If a project has `.claude/ways/softwaredev/code/testing/testing.md`, it replaces `~/.claude/hooks/ways/softwaredev/code/testing/testing.md` for that project.

### Macros in project-local ways

Project-local macros require explicit trust. Add the project path to `~/.claude/trusted-project-macros` (one path per line) to enable macro execution for that project.

## Managing Ways and Domains

### Disabling a single way for one project (ADR-131)

Use `ways disable` from inside the project:

```
ways disable itops/incident
ways disable --list           # see what's disabled in this project
ways enable itops/incident
```

That writes `{project}/.claude/ways.yaml`:

```yaml
ways:
  itops/incident: false
```

Per-way toggles are **project-scope only** — there is no global per-way disable. The default state is enabled, so a project with no `ways.yaml` (or no `ways:` block in it) behaves exactly as today.

Equivalent long-form, reserved for future per-way overrides:

```yaml
ways:
  itops/incident:
    enabled: false
```

### Disabling an entire domain (global)

Add the domain name to `~/.claude/ways.json`:

```json
{
  "disabled": ["itops", "experimental"]
}
```

All ways in disabled domains are silently skipped everywhere. The domain still appears in the Available Ways table but its ways won't fire. Domain-level disable is the right tool for "never anywhere"; project-scope per-way disable is the right tool for "not in this project."

### Creating a new domain

Create a subdirectory under `~/.claude/hooks/ways/` with your domain name. Add way directories inside it. The macro table generator and all check scripts will discover them automatically.

Domains are organizational - they group related ways and allow bulk enable/disable. Choose domain names that reflect the concern area (not the trigger mechanism).
