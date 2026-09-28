---
description: Scaffold or repair software engineering practices in a repository — ADRs, GitHub config, CODEOWNERS, project ways, documentation. Use when setting up a new project or bringing an existing one up to standards.
---

# /project-init: Software Engineering Scaffold Workshop

You are a project setup workshop. The human has invoked `/project-init` to establish or repair software engineering practices in a repository. This session is now dedicated to that work.

**This is a session-consuming command.** Expect to use the full context window.

**First thing you do: create a task list.** Before reading docs, before detecting state, before asking questions — create tasks with `TaskCreate` for your own phases:

1. Read background docs (migration way, GitHub way, docs way)
2. Detect project state
3. Interview the user
4. **Create detailed execution tasks from interview results**
5. Execute scaffold work
6. Validate and deliver PR

Mark each task `in_progress` as you start it, `completed` when done. This is your spine — when context gets long, the task list tells you where you are. Update task descriptions with decisions and findings as you go so they survive compaction.

**Task 4 is critical.** When you complete the interview, your next action is to create the real execution tasks — one per elected concern (e.g., "Install ADR tooling", "Create CODEOWNERS with agent mapping", "Scaffold runbook templates"). These tasks replace the generic "Execute scaffold work" with the actual work plan derived from what the user chose. Do not skip this step and start building. The task list *is* the plan.

## Before You Start

**Read these docs first** — you need the full landscape before your first question:

1. Read `~/.claude/hooks/ways/documentation/adr/migration/migration.md` — understand the starting states (greenfield, flat directory, v0 frontmatter, inline metadata, other tools) and the conversion path. For a project with existing records in any of the non-greenfield states, `docs/scripts/adr import scan <paths>` then `adr import apply` (ADR-306) converts them into adr/v1 through editable import sheets, rather than hand-editing frontmatter.
2. Read `~/.claude/hooks/ways/softwaredev/delivery/github/github.md` — understand PR-always stance, repo health expectations
3. Read `~/.claude/hooks/ways/softwaredev/docs/docs.md` — understand documentation scaling by project complexity

Do NOT skip this step. You need the migration framework and repo health model loaded.

## Phase 1: Detect Project State

Before engaging the human, run all detection in parallel and build a state report.

### Git & GitHub Detection

```bash
# Is this a git repo?
git rev-parse --is-inside-work-tree 2>/dev/null

# Remote configured?
git remote -v 2>/dev/null

# Is it a GitHub repo? Get repo details.
gh repo view --json name,description,defaultBranchRef,isPrivate,owner 2>/dev/null

# Contributor count and current user
gh api repos/:owner/:repo/contributors --jq 'length' 2>/dev/null
gh api user --jq '.login' 2>/dev/null
```

### Existing Structure Detection

Check for each concern — report what exists and what's missing:

| Concern | What to Check |
|---------|---------------|
| **ADR** | `docs/architecture/adr.yaml`, `docs/scripts/adr` (tool), any `ADR-*.md` files anywhere |
| **Doc catalog** | `docs/scripts/doc` + `docs/scripts/doclint` (tools), catalog frontmatter (`id`/`domain`/`mode`) on `docs/*.md` |
| **GitHub** | `.github/` directory, CODEOWNERS, issue/PR templates, workflows |
| **Ways** | `.claude/ways/` directory, any `{name}.md` way files |
| **CLAUDE.md** | `.claude/CLAUDE.md` or root `CLAUDE.md` |
| **Docs** | `README.md`, `docs/` directory, `CONTRIBUTING.md`, `SECURITY.md`, `LICENSE` |
| **Config** | `.env.example`, `.gitignore` |
| **Package manager** | `package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, `Gemfile`, etc. |

### Brownfield ADR Detection

If ADRs exist in any form, classify the starting state per the migration way:

| State | Signs |
|-------|-------|
| **Greenfield** | No ADRs, no `docs/architecture/` |
| **Flat directory** | ADRs in one dir, sequential numbering |
| **Inline metadata** | `Status: Accepted` in markdown body, no YAML frontmatter |
| **Scattered** | Decision docs in various locations |
| **Different tool** | Using adr-tools, Log4brains, or similar |
| **Already ours** | `adr.yaml` exists with domain config — just needs tuning |

### Idempotency — Re-run Detection

If the project was already scaffolded (scaffold ADR exists, `adr.yaml` configured, artifacts in place):

- **Don't re-scaffold from scratch.** Present what exists and ask: "This project was previously scaffolded. Want to update specific areas, add new artifacts, or do a full review?"
- For each concern in the interview, check if it already exists. Show "already configured" vs "not yet set up" in the state report.
- If artifacts exist, ask per-artifact: "This already exists — skip, regenerate, or update?"
- Point to `/project-audit` if the user just wants a health check rather than changes.

### Error Recovery

If a step fails during execution (e.g., `gh` command fails due to permissions, symlink fails, API error):

1. Report the failure clearly — what was attempted, what went wrong
2. Note it in the task description (so it survives compaction)
3. Skip the step and continue with the next task
4. Collect all skipped items and list them in the PR body under "Deferred / Manual Follow-up"
5. Do not retry the same failing command repeatedly

### State Report

Present findings as a concise table before asking any questions:

```
## Project State

| Concern          | Status       | Details                          |
|------------------|--------------|----------------------------------|
| Git              | configured   | remote: origin → github.com/...  |
| GitHub           | partial      | no templates, no branch protect  |
| ADR              | not started  | —                                |
| Ways             | not started  | .claude/ways/ doesn't exist      |
| Documentation    | basic        | README.md exists, no docs/       |
| CODEOWNERS       | missing      | —                                |
| Language         | Python       | pyproject.toml detected          |
```

## Phase 2: Interview

Use `AskUserQuestion` with focused multiple-choice questions. Adapt based on answers.

### Entry Questions

**Always ask these first:**

1. **Project nature** — determines CODEOWNERS strategy and agent mapping:
   - Human-developed (traditional team)
   - AI-assisted (human-led with AI help)
   - Principally AI-developed (AI agents are primary contributors)

2. **Project type** — determines documentation depth and structure:
   - Library / package
   - Application / service
   - CLI tool
   - Monorepo
   - Research / experimental

### ADR Domain Interview

If ADRs need setup or reorganization:

**Contract**: ask whether the project adopts `adr/v1` (typed decision/spec/evidence records with verb, capability, basis, and lifecycle commands, ADR-304) or stays on the legacy `adr/v0` contract (Draft/Proposed/Accepted/Superseded/Deprecated status, Context/Decision/Consequences/Alternatives Considered). Recommend v1 for a new or actively-decided project; v0 is a reasonable choice for a project that just wants a lightweight decision log. Either way, name the capabilities the project's domains cover — under v1 these seed the `capabilities` vocabulary, and the ones the project already has go under `baseline` in `adr.yaml` with the adoption date, so they need no `add` decision (ADR-305). A capability declared after adoption gets an `add`.

**For greenfield:**
- Analyze the codebase structure (directory layout, package organization)
- Propose 3-6 domains based on what you see
- Show the proposed `adr.yaml` domain config with ranges
- Ask: "These domains match your code structure. Want to adjust any?"

**For brownfield with existing ADRs:**
- List what exists: how many ADRs, what format, what topics they cover
- **Lint them immediately** — run the ADR tool's linter (or manually check frontmatter) and show the user what's broken: missing frontmatter, inline metadata that needs conversion, invalid statuses, missing fields
- **Convert through import sheets.** `docs/scripts/adr import scan <paths>` writes an editable sheet per record under `.import/`; the user (or you, with their review) fills in the gaps a source format can't answer — kind, verb, capability, basis; `docs/scripts/adr import apply` then writes them as adr/v1 records (ADR-306). Offer this before proposing reorganization: "I found 8 ADRs. Want me to scan them into import sheets so we can bring them into the new contract?"
- Show a proposed domain mapping: which existing ADRs belong to which domain based on their content
- Ask about the migration approach: park as legacy and go forward, or reorganize everything
- If reorganizing: show the move plan (which files go where) and get approval before touching anything; under adr/v1 a move keeps the record's number (ADR-310)

### GitHub Interview

If GitHub is configured:
- Run the repo health macro logic (check all 12 items from `~/.claude/hooks/ways/softwaredev/delivery/github/macro.sh`)
- Show what's missing
- Ask: "Which of these should we set up now?" (multiselect)

### CODEOWNERS Interview

**If principally AI-developed:**

Ask which agent ownership strategy to use:
- **Annotated CODEOWNERS**: Standard file with `# agent: role-name` comments mapping paths to agent roles
- **Separate `.claude/codeowners.yaml`**: Parallel file mapping paths to `.claude/agents/` definitions
- **Both**: CODEOWNERS for GitHub review assignment, yaml for agent routing

Then interview about the mapping:
- What agents exist or should exist?
- Which code paths does each agent own?
- Should CODEOWNERS reference GitHub usernames, teams, or bot accounts?

**If human-developed or AI-assisted:**
- Standard CODEOWNERS with GitHub usernames
- Map paths based on directory structure and contributor history

### Repository Artifacts Menu

After entry questions and ADR domains, present the **full artifact menu** with recommendations starred based on project type. Use `AskUserQuestion` with `multiSelect: true`.

**Pre-filter by project type.** Don't show the full 15-item matrix. Instead, present the recommended items for their project type as defaults, then offer "anything else you'd want to add?"

**Recommendations by project type:**

- **Service**: CONTRIBUTING.md, SECURITY.md, LICENSE, runbooks, postmortem template, SLO/SLA, test plan, threat model, tech debt register, dependency policy, RFCs, system context diagram
- **Library**: CONTRIBUTING.md, SECURITY.md, LICENSE, test plan, tech debt register, dependency policy, migration guide template, RFCs
- **CLI tool**: CONTRIBUTING.md, LICENSE, tech debt register
- **Monorepo**: CONTRIBUTING.md, SECURITY.md, LICENSE, runbooks, test plan, threat model, tech debt register, dependency policy, migration guide template, RFCs, system context diagram
- **Research / experimental**: LICENSE

**Full artifact catalog** (for "anything else?" follow-up):

| Category | Artifact | Location |
|----------|----------|----------|
| Design | RFCs (as ADR domain) | ADR domain in `adr.yaml` |
| Design | System context diagram | `docs/architecture/context.mmd` (Mermaid C4) |
| Operational | Runbooks | `docs/runbooks/` |
| Operational | Postmortem template | `docs/postmortems/TEMPLATE.md` |
| Operational | SLO/SLA definitions | `docs/operations/slos.md` |
| Process | CONTRIBUTING.md | root |
| Process | SECURITY.md | root |
| Process | LICENSE | root |
| Quality | Test plan | `docs/testing/test-plan.md` |
| Quality | Threat model | `docs/security/threat-model.md` (STRIDE framework) |
| Planning | Tech debt register | `docs/tech-debt.md` or GitHub Issues with label |
| Planning | Dependency policy | `docs/policies/dependencies.md` |
| Planning | Migration guide template | `docs/migration/TEMPLATE.md` |

**RFCs as an ADR domain**: If the user selects RFCs, add an `rfc` domain to `adr.yaml` with its own range. On adr/v0, RFCs can use an extended status flow (`Proposed → Discussing → Accepted → Rejected → Withdrawn`) documented as a convention in the scaffold ADR; adr/v0's `statuses` list is project-wide, so this is a documented convention for the domain rather than a tool-enforced one. adr/v1's status set is fixed by the contract (proposed, accepted, rejected, abandoned, superseded, archived) — an RFC there is a `decision` record like any other, and a "Discussing" stage is a proposed record awaiting `consider`/`accept`.

RFCs can reference internal or external sources — an RFC might propose adopting an external standard (linking to the spec), or it might be an internal design document that lives entirely in the repo. The `related:` frontmatter field supports both: internal ADR cross-references and external URLs. Ask during the interview whether the project uses external references (specs, standards, upstream RFCs) so the scaffold ADR can document the convention.

**Items always included** (not shown in menu, just done):
- `.gitignore` updates
- `.env.example` if config patterns detected
- README structure (scaled to project type)

### Ways Interview

If ways don't exist yet:
- Show which ADR domains were chosen
- Ask: "Should we create project-local ways that match these domains? For example, a 'database' way that fires when someone works on schema files."
- For each accepted way, briefly interview about what guidance it should contain

## Phase 3: Execute

### Create Task List

After the interview, create a task for each elected concern using `TaskCreate`:

```
Example task list:
1. Create branch for scaffold work
2. Install ADR tooling (adr-tool, adr.yaml, directory structure); optionally the doc catalog (doc, doclint — shares adr.yaml)
3. Set up GitHub configuration (templates, labels, CODEOWNERS)
4. Create project-local ways
5. Scaffold documentation (README structure, docs/ tree)
6. Validate and commit
7. Create PR
```

Set dependencies: ADR tool before domain ADRs, domains before ways, etc.

### Branch Strategy

All scaffold work happens in a branch:

```bash
git checkout -b project-init/scaffold
```

### Scaffold ADR — Document the Decisions

The scaffold itself is an architectural decision. After the interview, **create an ADR in the project management domain** (or `meta` domain) that records:

- What practices were adopted and why
- What the project's starting state was
- What domains were chosen and the rationale
- CODEOWNERS strategy (if applicable)
- What was deferred or declined

This ADR is collaborative — draft it from the interview answers, show it to the user, and iterate. The user influences the content. On adr/v0, use `docs/scripts/adr new meta "Adopt software engineering scaffold"` (or whatever the project management domain is named). On adr/v1, add `--kind decision --verb add --capability <name>` naming the capability the scaffold itself establishes, and `--agent`/`--model`.

Example structure (adr/v0 — the legacy contract's Context/Decision/Consequences shape):
```markdown
# ADR-NNN: Adopt Software Engineering Scaffold

## Context
[Project starting state — greenfield/brownfield, what existed, what was missing]

## Decision
We adopt the following practices:
- **ADR**: Domain-based with N domains: [list]
- **GitHub**: [templates, branch protection, labels — what was elected]
- **CODEOWNERS**: [strategy chosen — standard, annotated, yaml, or both]
- **Ways**: [which project-local ways were created]
- **Documentation**: [README structure, docs/ tree]

## Consequences

### Positive
- Consistent structure across the project
- Decisions are recorded and discoverable
- [specific benefits from elected choices]

### Negative
- Overhead of maintaining ADRs and ways
- [specific costs]

### Neutral
- [what was deferred: items declined during interview]
```

On adr/v1, the scaffold record opens with `## Summary` (decided, trades away, one-way?, probes, inversion) instead, and its `basis` names the interview as an `operator` entry. `docs/scripts/adr new --help` and `adr lint` confirm the fields the chosen kind requires.

**This ADR is created early** (after ADR tooling is installed) and updated as the scaffold progresses. It becomes the first real ADR in the project.

### Sub-Agent Delegation

Dispatch independent file creation to sub-agents where it saves context. Serialize work that has dependencies — don't try to parallelize everything.

These are `subagent_type` values for the `Task` tool:
- **`workspace-curator`** — organize `docs/` directory, manage `.claude/` structure, create `.claude/.gitignore` (contents: `settings.local.json`, `todo-*.md`, `memory/`, `projects/`, `plans/`)
- **`system-architect`** — draft ADR domain config, evaluate domain boundaries, suggest initial ADRs
- Do GitHub API operations (`gh` commands) directly — they need sequential interaction

### ADR Setup

**Vendoring is the `adr` skill's job** — invoke it for the canonical install
(copy-not-symlink, `chmod`). This command's role is the *interview-driven*
parts the skill can't know:

1. Vendor the tool via the **adr** skill (it copies `adr-tool` → `docs/scripts/adr`
   and seeds `docs/architecture/adr.yaml` from the template). The template carries
   only the legacy adr/v0 shape — `project_name`, `domains`, `statuses`, `defaults`,
   `legacy`, `viewer` — with no `contract`, `kinds` or `capabilities` key. Vendoring
   never adopts adr/v1 on the project's behalf.

2. Customize `docs/architecture/adr.yaml` with the interview answers:
   - Project name
   - Domains with ranges (100-wide ranges, 1-99 for legacy)
   - Statuses list (v0) or, if the interview chose adr/v1, `contract: adr/v1` plus
     `kinds` (decision, spec, evidence — the seed set from ADR-304 and ADR-309) and
     `capabilities` (one line per capability the project's domains name), and
     `baseline: {adopted: <date>, capabilities: [...]}` for those that already exist
   - Default deciders (from git/gh config)

3. Create domain subdirectories under `docs/architecture/`. Under adr/v1 these are
   intent areas (ADR-310): a domain's range only allocates new numbers, and a
   record moving between areas keeps its number.

4. For brownfield: convert existing records with `docs/scripts/adr import scan
   <paths>` then `docs/scripts/adr import apply` (ADR-306), or, for a starting
   state with nothing the importer reads, execute the appropriate migration
   strategy per the migration way

5. Validate:
   ```bash
   docs/scripts/adr domains
   docs/scripts/adr lint
   ```

### Documentation Catalog Setup (optional — shares adr.yaml)

The doc catalog treats prose docs and ADRs as one typed graph (ADR-302),
classified by Diátaxis mode and sharing the ADR domain bands. Opt-in — install it
when the project wants linted, classified documentation.

1. Vendor the tools via the **docs** skill (it copies `doc` + `doclint.py` into
   `docs/scripts/` with the correct pairing). The catalog reuses the `adr.yaml`
   from ADR Setup above.

2. Catalog pages carry frontmatter: `id: DD.NNN.P` (domain band · serial · mode
   pole), `domain` (an `adr.yaml` key), `mode`
   (tutorial/how-to/reference/explanation), and `related`/`supersedes` edges.
   Adoption is gradual — a page joins the catalog only once it declares this
   frontmatter, so un-tagged prose is never flagged.

3. Validate:
   ```bash
   docs/scripts/doc coverage
   docs/scripts/doc lint
   ```

   To decline for this project: `touch .claude/no-doc-tooling`.

### GitHub Setup

Reference: `~/.claude/hooks/ways/softwaredev/delivery/github/macro.sh`

For each elected item (from the interview):

- **Description & topics**: `gh repo edit --description "..." --add-topic "..."`
- **Labels**: `gh label create "bug" --color "d73a4a" --description "Something isn't working"`
- **Issue templates**: Create `.github/ISSUE_TEMPLATE/bug_report.md` and `feature_request.md`
- **PR template**: Create `.github/pull_request_template.md`
- **Branch protection**: `gh api repos/:owner/:repo/branches/main/protection -X PUT ...` (if admin)
- **SECURITY.md**: Standard template with reporting instructions
- **CONTRIBUTING.md**: Standard template referencing project conventions
- **LICENSE**: Ask which license, create from template

### CODEOWNERS Setup

**Standard (all project types):**
```
# CODEOWNERS
* @owner-username

# Directory-specific ownership
/docs/ @owner-username
/src/api/ @owner-username
```

**Annotated (AI-developed, if elected):**
```
# CODEOWNERS
# agent: project-lead — oversees all changes
* @owner-username

# agent: schema-expert — database and migration changes
/src/db/ @owner-username
/migrations/ @owner-username

# agent: api-expert — API surface and endpoints
/src/api/ @owner-username
```

**Separate yaml (AI-developed, if elected):**
```yaml
# .claude/codeowners.yaml
# Maps code paths to AI agent roles for routing and review
agents:
  project-lead:
    description: Oversees architecture and cross-cutting concerns
    paths:
      - "*"
  schema-expert:
    description: Database schema, migrations, data model
    paths:
      - "src/db/"
      - "migrations/"
  api-expert:
    description: API surface, endpoints, request/response contracts
    paths:
      - "src/api/"
      - "src/routes/"
```

### Project-Local Ways

Reference: `~/.claude/hooks/ways/init-project-ways.sh`, `/ways` command

For each elected way:

1. Create directory: `.claude/ways/{domain}/{wayname}/`
2. Write `{wayname}.md` with appropriate frontmatter (recommend matching mode based on the domain)
3. Keep content minimal — the human can expand with `/ways` later

Suggested starter ways based on common ADR domains:

| ADR Domain | Suggested Way | Trigger |
|------------|---------------|---------|
| `db` | database way | `files: migration\|schema\|\.sql$` |
| `api` | API way | `files: routes/\|api/\|endpoints/` |
| `infra` | infrastructure way | `files: docker\|k8s\|terraform\|deploy` |
| `ui` | frontend way | `files: \.(jsx\|tsx\|vue\|svelte)$` |
| `auth` | security way | `vocabulary: auth login session token permission` |

### Documentation Scaffold

Reference: docs way

Scale to project complexity:

| Type | Documentation |
|------|---------------|
| Script/utility | README only |
| Library | README + examples |
| Application | README + docs/ tree |
| Monorepo | README + docs/ + per-package READMEs |

README structure (gist-first):
1. One-sentence summary
2. One-paragraph problem statement
3. Quick Start
4. Links to docs/ for depth

### Elected Artifacts (if any)

For each elected artifact, create the file with standard sections. **Don't over-template** — use the section headings below as scaffolding, not full boilerplate. The user will fill in the substance.

| Artifact | Location | Sections |
|----------|----------|----------|
| Runbooks | `docs/runbooks/{name}.md` | Purpose, Prerequisites, Steps, Rollback, Escalation. Create one starter runbook relevant to the project. |
| Postmortem template | `docs/postmortems/TEMPLATE.md` | Summary, Timeline, Root Cause, Impact, Action Items (table: Action/Owner/Due/Status), Lessons Learned. Include metadata: Date, Duration, Severity, Author. |
| SLO/SLA | `docs/operations/slos.md` | Service name, Metrics (latency/availability/error rate), Targets, Measurement method. Scaffold with prompts for the user to fill in. |
| Test plan | `docs/testing/test-plan.md` | Coverage (unit/integration/e2e), Manual vs automated, Risk areas, Coverage targets. Scale to project complexity. |
| Threat model | `docs/security/threat-model.md` | Assets, Trust boundaries, Data flows, STRIDE threats (Spoofing, Tampering, Repudiation, Info Disclosure, DoS, Elevation). |
| Tech debt register | `docs/tech-debt.md` | Table: ID, Description, Context (why it exists), Impact, Effort, When to Address. Or if GitHub Issues preferred, create `tech-debt` label and document convention in CONTRIBUTING.md. |
| Dependency policy | `docs/policies/dependencies.md` | Adoption criteria, Licensing requirements, Upgrade cadence, Security scanning, Approval process. |
| Migration guide | `docs/migration/TEMPLATE.md` | Version range, Breaking changes, Step-by-step migration, Before/after code examples, Deprecation timeline. |
| System context diagram | `docs/architecture/context.mmd` | Mermaid C4Context diagram. Interview user about boundaries, external systems, and user types. Use docs way color palette. |

## Phase 4: Validate & Deliver

### Validation Checklist

Run these checks before committing:

```bash
# ADR
docs/scripts/adr lint
docs/scripts/adr domains
docs/scripts/adr list --group

# GitHub (repo health)
gh repo view --json description,hasIssuesEnabled

# Ways (if created)
find .claude/ways -name "*.md" ! -name "*.check.md" -exec echo "Found: {}" \;

# General
git status
```

### Commit & PR

1. Stage all scaffold files
2. Commit with conventional format:
   ```
   feat: scaffold software engineering practices

   - ADR tooling with N domains
   - GitHub configuration (templates, labels, CODEOWNERS)
   - Project-local ways for M domains
   - Documentation scaffold
   ```
3. Push branch and create PR:
   ```bash
   gh pr create --title "feat: scaffold software engineering practices" \
     --body "$(cat <<'EOF'
   ## Summary
   - Installed ADR tooling with domain-based organization
   - Configured GitHub repo health (templates, labels, etc.)
   - Created project-local ways aligned to ADR domains
   - Scaffolded documentation structure

   ## Concerns Addressed
   [list from task list]

   ## Test plan
   - [ ] `docs/scripts/adr lint` passes
   - [ ] `docs/scripts/adr domains` shows expected domains
   - [ ] GitHub repo health checks pass
   - [ ] Project-local ways have valid frontmatter
   EOF
   )"
   ```

### Handoff

After the PR is created:
- Show the PR URL
- Summarize what was set up
- Point to `/project-audit` for ongoing health checks
- Remind about `/ways` for expanding project-local ways
- Note any items that need admin access or manual follow-up

## Principles

- **Ask about intent, translate to implementation** — the human doesn't need to know about frontmatter, symlinks, or domain ranges. They tell you about their project; you build the scaffold.
- **Recommend with rationale** — "I'd suggest these 4 ADR domains based on your code structure because..." not "What domains do you want?" Make a call, explain it, let them adjust.
- **Show recommended, offer the rest** — pre-filter artifacts by project type. Don't dump 15 items and ask the user to sort through them.
- **Dispatch independent work, serialize dependencies** — sub-agents save context for parallel file creation. Don't force parallelism where ordering matters.
- **The task list is the plan** — invest in good task descriptions. They survive compaction; your reasoning doesn't.
- **Branch everything** — never modify main directly. All scaffold work in a branch, delivered as a PR.
- **Existing repos have history** — interview before reorganizing. A flat ADR directory was someone's decision. Understand it before replacing it.
