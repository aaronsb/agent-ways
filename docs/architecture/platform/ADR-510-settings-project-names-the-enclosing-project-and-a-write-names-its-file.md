---
contract: adr/v1
kind: decision
verb: change
capability: [config, cli]
basis:
  - operator: aaronsb
    level: directed
    said: "perhaps it's just a matter of offering an easier to use --project parameter path for settings (ways settings --project <PROJECT>) where <PROJECT> can be a path. if I specify '.' then the current directory (which would be a project I know is valid) could be resolved to the actual project. so for instance if I used the current dir \".\" and invoked --project . (or --project \".\") in /home/aaron/Projects/ai/harness/agent-ways, then the project would resolve correctly"
    via: "an attend message to the head session on 2026-10-12, 09:04, on #open"
  - evidence: "Run from agent-ways/tools, `echo '{\"attend_mod\":{\"keepwarm\":\"off\"}}' | ways settings apply --dry-run --project .` (ways 1.39.0) reported it would write ./.claude/ways.yaml, which is tools/.claude/ways.yaml and not the project's. The same day the operator wrote a key to the user file while meaning the project, and nothing on screen said which file was touched."
  - evidence: "McKay, a peer session, 2026-10-12: `--project` accepted a scratch directory under /tmp with exit 0 and no warning, and the TUI did not show which layer a write would land in. Code review of PR #935, the same day: the attend mod (plugins/attend/hooks/lib.ts) and keepwarm (tools/attend/src/cmd/keepwarm.rs) read `ways settings list attend_mod --json --effective --project <session dir>` from the home directory and scratch directories. Exit 2 there makes both fall back to defaults and lose the user layer. Widening the shared working-directory walk with .git changed `ways author template`, stats and replay scope for nested checkouts."
  - precedent: ADR-503
agent:
  name: Claude
  model: claude-opus-5-5
amends:
  - ADR-503#Decision
status: accepted
date: 2026-10-12
deciders:
  - aaronsb
related:
  - ADR-503
observable:
  - "`ways settings set <key> <value> --project .` run from a subdirectory of a project writes the project root's .claude/ways.yaml, and stderr says `project: <absolute path>`"
  - "`ways settings list attend_mod --json --effective --project ~` exits 0 with the user file's values"
  - "`ways settings set <key> <value> --project /tmp/scratch` exits 2 and creates no .claude/"
---

# ADR-510: Settings --project names the enclosing project, and a write names its file

## Summary

- **Decided:** `--project <path>` on `ways settings` names the project that encloses the path, not the path itself, so `--project .` from a subdirectory reaches the project's root. Reads of a path no project encloses have no project layer and succeed. Writes to one exit 2 and write nothing. Every write names its file on stderr, on success too. This amends ADR-503 §8, where `--project <dir>` named the directory written, and §9, where `set` and `unset` printed nothing.
- **Trades away:** `--project` no longer writes into an arbitrary directory, and a fresh directory with no project markers and no git checkout cannot take a project write until it has one. A successful write is no longer silent on stderr, though stdout still is.
- **One-way?** No. The resolution and the stderr line live in the settings verbs alone, and no file format changes.
- **Probes:** *Confident (root):* you run `--project .` anywhere inside a project and the project's own file is the one read and written, and the output names it. *Not confident (fresh repo):* a repository with no `.claude/` takes its first project write at its git top level, which may not be where you meant to start one.
- **Inversion:** The two ends are a path taken literally (any directory takes a `.claude/`) and a project found only from markers (a fresh repository can never be written). The settings rule takes markers first and a git checkout as the fallback.

## Context

ADR-503 §8 lets a key with both scopes be written to a project with `--project <dir>`, and the CLI took `<dir>` as the project. Run from a subdirectory, `--project .` wrote a `.claude/ways.yaml` there, which no session reads. ADR-503 §9 has `set` print nothing on success. When a key landed in the user file instead of the project's, nothing said so.

The attend mod and its keepwarm card read their settings with the session's directory as `--project`. A session may start in the home directory or a scratch one, so a read of a directory in no project must still return the user's layers.

## Decision

1. **A path names its enclosing project.** `--project <path>` is resolved to an absolute path with links followed, then walked up. The settings rule is:
   1. the nearest ancestor with a `.claude/` holding `settings.json`, `settings.local.json`, `ways.yaml` or `attend.yaml`, or with a `.claude/` beside a `CLAUDE.md`;
   2. failing that, the nearest git checkout's top level.

   An outer project with markers wins over a checkout nested in it. The home directory is never a project, since its `.claude/` is the user's. The markers are one predicate shared with the walk from the working directory, which takes no overlays and no git fallback and is unchanged.
2. **Without `--project`.** `CLAUDE_PROJECT_DIR` names the project, as the hooks read it. Otherwise the settings rule walks from the working directory.
3. **Reads are lenient.** `get`, `list`, `emit`, `lint` and the screens read no project layer for a path no project encloses, a path that does not exist included. The user, target and default layers resolve as usual, and the exit code is 0. On the screens, a key bound for a project's file reads as not applicable and takes no write.
4. **Writes are strict.** `set`, `unset`, `apply` and `fix` exit 2 for a `--project` that does not exist or that no project encloses, and name the markers in the message. A project-scope key written with no `--project` outside any project is refused the same way. Nothing creates a `.claude/` in a directory that is not a project's.
5. **A write names its file.** Each write prints one line per file on stderr: `project: <path>`, `user: <path>` or `target: <path>`, with ` (dry run, nothing written)` after it for `apply --dry-run`. Stdout keeps ADR-503 §9: `set` and `unset` print nothing there, and `apply`'s JSON report keys `written` by each file's absolute path. The screens' title names the layers each tab writes.

## Consequences

### Positive

- `--project .` works from anywhere inside a project, and read and write agree on which file that is.
- A write to the wrong layer shows as one: the stderr line, or the screens' title, names it before the operator has to look.
- A session in the home directory or a scratch one still reads the user's settings.

### Negative

- A script that took `--project <dir>` to mean "write into this exact directory" now writes to the enclosing project, or exits 2.
- A tool reading stderr from a successful `set` sees a line where there was none.

### Neutral

- The working-directory walk that `ways author template`, stats and replay scope use keeps its markers. Only the settings verbs count the overlays and fall back to git.

## Alternatives Considered

- **Take the path literally and print the file.** It makes a wrong write visible, but leaves `--project .` writing a file no session reads.
- **Exit 2 on every verb for a path in no project.** It drops the user layer for the attend mod's reads from the home directory, so a user-level `listening: active` came up dormant.
- **Add `.git` to the shared walk.** It changed what template, stats and replay found for nested checkouts, where only the settings verbs needed a fallback.
- **Name the file on stdout.** It breaks the stdout contract `set` scripts rely on; stderr carries it instead.
