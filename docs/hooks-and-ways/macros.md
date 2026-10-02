# Macros

Dynamic content generation for ways.

## What Macros Do

A way's static content (`{name}.md`) is the same every time it fires. Macros add dynamic content by running a shell script at trigger time. The script's stdout is combined with the static content based on the `macro:` frontmatter field.

```yaml
macro: prepend   # macro output appears before static content
macro: append    # macro output appears after static content
```

The macro script lives alongside the way file as `macro.sh` in the same directory.

## What a Macro Receives

A macro gets no stdin and no arguments. The `ways` binary runs it with bash and exports what it may need about the session:

| Variable | Value |
|----------|-------|
| `CLAUDE_SESSION_ID` | The session the way fires for. Inside a subagent this is the parent's id, since subagent hooks report it |
| `CLAUDE_PROJECT_DIR` | The resolved project directory, never empty |
| `WAYS_SESSIONS_ROOT` | The per-user root of session state (`{root}/{session}/…`) |
| `WAYS_SCOPE` | `agent`, `teammate` or `subagent`: who the way is rendered for. A macro that consumes session state skips `subagent`, or it would take the parent's |
| `WAYS_CONTEXT_USED`, `WAYS_CONTEXT_REMAINING`, `WAYS_CONTEXT_PCT_REMAINING` | The context budget, read from the session's transcript |
| `WAYS_ENABLED_PLUGINS` | Enabled plugin ids (`name@marketplace`), one per line, from the `enabledPlugins` maps in Claude Code's settings files |
| `WAYS_ADR_TOOL`, `WAYS_DOC_TOOL` | The project-relative path of the first executable `docs/scripts/`, `scripts/` or `tools/` `adr` (`doc`), when there is one |

The last three rows cost a transcript read, a settings read or a few stats, so the binary computes each only for a macro whose source names the variable. A variable with no value is unset. A `postcheck.sh` gets `CLAUDE_SESSION_ID`, `CLAUDE_PROJECT_DIR` and `WAYS_SESSIONS_ROOT`, with the hook payload on stdin.

## Why Macros Exist

Some guidance depends on project state that can't be known at authoring time:

- Does this project use ADR tooling? (adr/macro.sh checks for `docs/scripts/adr`)
- Is this a solo project or a team project? (github/macro.sh checks contributor count)
- Which files in this project are too long? (quality/macro.sh scans the codebase)

Static way content handles the universal guidance ("here's how to write good commits"). Macros handle the situational guidance ("this project has 3 files over 800 lines - here they are").

## Examples

### ADR Tooling Detection (documentation/adr)

Implements tri-state detection:

1. **Declined**: `.claude/no-adr-tooling` exists. The macro outputs a one-liner noting the project opted out, then the v0 record format, and stops suggesting installation.
2. **Installed**: `docs/scripts/adr` exists. The macro reads the vendored tool's `TOOL_VERSION` and the `contract:` in `docs/architecture/adr.yaml`, and outputs the guidance for that combination (ADR-304 §10): the v0 commands and record format, the same plus a note that adopting adr/v1 is a decision, the adr/v1 guide, or a warning that the tool cannot enforce the declared contract.
3. **Available**: Neither file exists. The macro suggests installing ADR tooling, with setup instructions, and outputs the v0 record format.

The way body stays contract-neutral; everything that depends on the contract comes from the macro. `tests/adr-macro-test.sh` covers each state.

### Team Detection (softwaredev/github)

Queries the GitHub API for contributor count:
- Solo/pair project (1-2 contributors): relaxes PR requirements
- Team project (3+ contributors): recommends PRs, shows potential reviewers

Adapts the formality of GitHub workflow guidance to the actual collaboration context.

### File Length Scanner (softwaredev/quality)

Scans `git ls-files` for files exceeding length thresholds:
- **Priority** (>800 lines): files that likely need decomposition
- **Review** (>500 lines): files worth monitoring

Uses the `scan_exclude:` frontmatter field to skip files that are legitimately long (lock files, generated code, markdown).

## The Core Macro

`macro.sh` at the ways root (`~/.claude/hooks/ways/macro.sh`) generates the Available Ways table shown at session start. It scans all way files, extracts their trigger patterns from frontmatter, and formats them as a markdown table grouped by domain.

This is invoked by `ways show core` during SessionStart and referenced by `core.md` via `macro: prepend`.

## Security Model

### Global macros

Macros in `~/.claude/hooks/ways/` always execute. The user controls this directory, so trust is implicit.

### Project-local macros

Macros in `$PROJECT/.claude/ways/` are potentially untrusted (a cloned repo could include malicious scripts). These only execute if the project path is explicitly listed in `~/.claude/trusted-project-macros`.

If a project-local macro exists but the project isn't trusted, the way outputs a note:
> **Note**: Project-local macro skipped (add /path/to/project to ~/.claude/trusted-project-macros to enable)

This prevents supply-chain attacks through project-local way macros while allowing teams that trust their repos to use the full feature.
