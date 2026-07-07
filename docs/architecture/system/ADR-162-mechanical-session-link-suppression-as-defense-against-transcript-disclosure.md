---
status: Accepted
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
commits, PRs, or issues — `git commit`, `gh pr create|edit|comment`,
`gh issue create|edit|comment` — and **denies** any that carry a `Claude-Session:`
trailer or `claude.ai/code/session_…` URL, returning a remediation message that
instructs the model to remove the link and re-issue. The clean re-issue passes
through the normal permission flow. The link never reaches git or GitHub.

The hook **denies rather than rewrites**. Rewriting a command in place (PreToolUse
`updatedInput`) is honored *only* when the hook also sets `permissionDecision:
allow` — `updatedInput` alone is ignored, and no pass-through value exists
(upstream FR #381). Forcing `allow` would bypass the operator's own
Deny/Allow/Ask rules for that call, including on outward-facing `gh pr create` /
`gh issue create` — silently defeating a review gate the operator may rely on.
Deny bypasses nothing: it costs one extra round-trip, after which the model
re-issues cleanly (and adapts within the session to stop appending the link).

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

- A PreToolUse hook adds a small latency to every `git`/`gh` Bash call. On its
  no-op paths (no command, out of scope, no link) it fails **open**; once a link is
  actually detected it fails **closed** (deny), so a hook error there blocks rather
  than leaks.
- Deny costs one extra round-trip each time the model appends a link — until it
  adapts within the session. Benign but visible friction on the first commit/PR.
- Detection is bounded by what the hook can see. Inline `-m` and heredoc bodies are
  greppable in the command string; a link in a body passed via `--body-file`/`-F
  <path>` lives in a file the hook cannot inspect from the command string, so it is
  a coverage gap that would slip through. Inline/heredoc is how the model formats
  these by default, so the common case is covered.
- If Claude Code changes the link format, the hook's patterns need updating — a
  maintenance coupling to an undocumented upstream string.

### Neutral

- The change belongs in the GitHub delivery way alongside the existing attribution
  report block. The hook entry lives in the **reconcile-owned `settings.json` hooks
  block** (beside the existing `check-bash-pre.sh` gate), not the settings-fragment
  store — `ways settings project` deliberately skips the hooks/permissions slices,
  so a fragment-authored hook would silently never fire. Its cross-host distribution
  is owned by ADR-163.
- The hook enforces by **deny** (`permissionDecision: deny` with a
  `permissionDecisionReason`, falling back to exit 2 + stderr). It deliberately does
  *not* rewrite the command in place: see the Decision and the rejected silent-strip
  alternative for why forcing `permissionDecision: allow` — the only way to make
  `updatedInput` take effect — is unacceptable here.

## Alternatives Considered

- **Rely on `attribution.sessionUrl: false` alone.** Rejected: undocumented and
  defeated by injection bug #18253, with no coverage for repos where the setting
  never reaches the model. Kept only as a secondary layer.
- **Silently rewrite the command in place (PreToolUse `updatedInput`).** Rejected:
  `updatedInput` is honored only alongside `permissionDecision: allow` — it is
  ignored on its own, and no pass-through value exists (upstream FR #381). The
  rewrite would therefore force-approve the command and bypass the operator's
  Deny/Allow/Ask gates, including on outward-facing `gh pr create` / `gh issue
  create`. Cleaner UX, but a real permission regression on exactly the commands
  worth gating; deny preserves the gate for the price of one retry. (This was the
  initially-ratified choice, reversed once the `updatedInput`→`allow` coupling was
  confirmed.)
- **A way / memory instruction telling the model to omit the link.** Rejected:
  soft, competes with the system-prompt instruction, and scoped per project. This
  is precisely what already failed — the leaking repo had no such note.
- **Post-hoc history rewrite / secret rotation.** Rejected as the primary control:
  reactive, triggered only after the link (and any secret) is already pushed.
  Remains the incident response if a link ever slips past the hook.
- **Do nothing / accept the setting layer as sufficient.** Rejected: the motivating
  leak proves the setting layer is not load-bearing, and the exposure is a security
  posture, not a preference.
