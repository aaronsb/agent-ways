---
status: Draft
date: 2026-07-06
deciders:
  - aaronsb
  - claude
related:
  - 152
  - 163
---

# ADR-162: Mechanical session-link suppression as defense against transcript disclosure

## Context

Claude Code appends a session link to git commit messages (`Claude-Session:
https://claude.ai/code/session_…`) and to PR bodies. That link resolves to the
**full session transcript**. A transcript routinely contains material that
scrolled through the conversation — file contents, environment values, tokens,
internal paths. Publishing the link therefore places a single click between a
public commit and whatever secret happened to pass through the session. On a
public repository this is an accidental-disclosure surface, not a convenience.

A prior attempt to close this (commit `3b7f04c`, "give the session-link trailer a
control surface") set `attribution.commit` and `attribution.pr` to `""` and added
a *report-only* block to the GitHub delivery macro. Subsequent investigation shows
that attempt did not govern the session link at all:

- `attribution.commit` / `attribution.pr` only suppress the **Co-Authored-By /
  "Generated with Claude Code"** footers. They never controlled the session link.
- The key that does govern it, `attribution.sessionUrl`, is **undocumented**
  (added in v2.1.183) and is **not reliably injected into the model's context**
  (upstream issue [#18253](https://github.com/anthropics/claude-code/issues/18253),
  marked *not planned*). The system-prompt instruction to append the link survives
  the setting, and the model follows it.
- Claude Code has **no mechanism that strips** such a link after it is written. The
  entire design is preventive ("do not instruct the model"), and the injection bug
  defeats prevention.

The failure is observable: a PR was authored in an unrelated repository that had
**no local `.claude/settings.json`** — it inherited the global
`attribution: {commit:"", pr:""}` — and the session link still landed in the PR
body. The setting layer neither reached the model nor covered a repo without local
config.

The conclusion is that any control depending on the harness honoring a setting, or
on the model choosing to obey it, is not load-bearing. Suppression must be
mechanical and must live in a layer we own.

## Decision

Add a **PreToolUse hook on `Bash`** that inspects command invocations which author
commits or pull requests — `git commit`, `gh pr create`, `gh pr edit` (and the
`git commit -m` / heredoc / `--body` forms) — and prevents any `Claude-Session:`
trailer or `claude.ai/code/session_…` URL from landing. Where the harness supports
rewriting tool input, the hook strips the offending lines in place; otherwise it
denies the call with a remediation message instructing removal. Either path
guarantees the link does not reach git or GitHub.

The hook must operate at **user scope** — covering **every** repository the
operator works in, independent of whether a repo carries local config — rather
than as a per-project hook. That is the property this decision requires; *how*
the hook is distributed to achieve it (dotfile source-of-truth, the settings
projection, cross-host travel, key-vs-file ownership) is the province of ADR-163,
which carries this hook as the primitive it distributes. What matters here is the
guarantee, not the delivery: suppression does not depend on the harness injecting
the `attribution` setting, on the model obeying it, or on a per-project note.

As belt-and-suspenders — cheap and correct in intent even while #18253 stands — we
also set `attribution.sessionUrl: false` in the settings store. The existing
report-only macro block is retained; it now reports the true effective state
alongside a hook that actually enforces it.

Scope boundary: this ADR governs suppression of the **session link** specifically.
It does not remove the Co-Authored-By footer (an independent operator preference
already handled by `attribution.commit`/`.pr`).

## Consequences

### Positive

- Every repository is protected regardless of local config, the harness injection
  bug, or model obedience. The unrelated-repo leak that motivated this ADR is
  closed at the layer we control.
- Enforcement is auditable and testable — a hook script with explicit patterns,
  not a soft instruction competing with the system prompt.
- Defense-in-depth composes with ADR-152's secret-path deny baseline: 152 keeps
  secret *files* out of the model's reach; 162 keeps a *pointer to the whole
  transcript* out of published history.

### Negative

- A PreToolUse hook adds a small latency and a failure surface to every `git`/`gh`
  Bash call; it must fail open on parse it cannot understand rather than block
  legitimate commits.
- Interception is bounded by what the hook can see. Inline `-m` and heredoc bodies
  are greppable in the command string; a body passed via `--body-file`/`-F <path>`
  requires the hook to read and rewrite that file, which is a second code path to
  maintain.
- If Claude Code changes the link format, the hook's patterns need updating — a
  maintenance coupling to an undocumented upstream string.

### Neutral

- The change belongs in the GitHub delivery way alongside the existing attribution
  report block. The hook entry lives in the **reconcile-owned `settings.json` hooks
  block** (beside the existing `check-bash-pre.sh` gate), not the settings-fragment
  store — `ways settings project` deliberately skips the hooks/permissions slices,
  so a fragment-authored hook would silently never fire. Its cross-host distribution
  is owned by ADR-163.
- The hook rewrites the command in place via PreToolUse `updatedInput`
  (`permissionDecision: allow`), a confirmed Claude Code capability, so the commit
  flow is never interrupted. The alternative — block-with-remediation (exit 2) — was
  available but rejected as needless friction once silent rewrite was confirmed.

## Alternatives Considered

- **Rely on `attribution.sessionUrl: false` alone.** Rejected: undocumented and
  defeated by injection bug #18253, with no coverage for repos where the setting
  never reaches the model. Kept only as a secondary layer.
- **A way / memory instruction telling the model to omit the link.** Rejected:
  soft, competes with the system-prompt instruction, and scoped per project. This
  is precisely what already failed — the leaking repo had no such note.
- **Post-hoc history rewrite / secret rotation.** Rejected as the primary control:
  reactive, triggered only after the link (and any secret) is already pushed.
  Remains the incident response if a link ever slips past the hook.
- **Do nothing / accept the setting layer as sufficient.** Rejected: the motivating
  leak proves the setting layer is not load-bearing, and the exposure is a security
  posture, not a preference.
