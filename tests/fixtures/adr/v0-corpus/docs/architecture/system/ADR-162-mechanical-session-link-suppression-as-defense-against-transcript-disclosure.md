---
status: Superseded
date: 2026-07-06
revised: 2026-07-16
deciders:
  - aaronsb
  - claude
related:
  - 152
  - 163
  - 167
superseded_by: ADR-167
---

# ADR-162: Mechanical session-link suppression as defense against transcript disclosure

## Status: Superseded by ADR-167

**Superseded 2026-07-16.** The threat model below is correct and load-bearing: the
session link resolves to the full transcript, and publishing it on a public repo is an
accidental-disclosure surface. The **hook** this ADR designed also survives unchanged —
deny rather than rewrite, at user scope, trailer/footer position only. Its *rationale*
does not.

This ADR's central claim — that no setting can govern the link, so suppression must be
mechanical — was **false when written**. It rests on a mis-citation: `#18253` is the
Co-Authored-By/footer bug ([CLOSED/COMPLETED]), not the session link; the issue meant
was `#41873` ([CLOSED/NOT_PLANNED]). And `#41873` had already been superseded by
`attribution.sessionUrl`, shipped in **v2.1.183 (2026-06-19)** — seventeen days before
this ADR was dated. A controlled experiment on 2026-07-16 (same repo, same v2.1.212,
only the key differing) confirms the setting governs the trailer.

[[ADR-167]] carries the corrected decision: `attribution.sessionUrl: false` is the
primary control, and the hook below is retained as a **backstop** rather than the sole
defense. Read this ADR for the threat model and the hook's design; read ADR-167 for
what actually defends against it.

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
commits, PRs, or issues — `git … commit`, `gh … pr create|edit|comment`,
`gh … issue create|edit|comment`, matched on token presence (newline-collapsed) so
intervening flags such as `git -C <path> commit` or `gh <global-flags> pr create`
cannot evade the gate — and **denies** any that carry a `Claude-Session:` trailer or
a bare `claude.ai/code/session_…` URL **in trailer/footer position** (line-leading),
returning a remediation message that instructs the model to remove the link and
re-issue. A session URL mentioned *inline in prose* is not a leak and passes, so a
repo can still discuss `Claude-Session` as subject matter without a deny-loop. The
clean re-issue passes through the normal permission flow. The link is blocked on the
covered command forms (coverage gaps are listed in Consequences).

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

The `attribution.sessionUrl: false` key (undocumented, v2.1.183+) is the harness's
own would-be control; **this change does not set it** — it is defeated by #18253
today, and coupling to an undocumented key risks tripping settings validation for
no working benefit. The hook is the load-bearing control. The existing report-only
macro block is retained; it reports the effective `attribution` state alongside a
hook that actually enforces suppression. An operator may set `sessionUrl: false`
additionally once upstream honors it.

Scope boundary: this ADR governs suppression of the **session link** specifically.
It does not remove the Co-Authored-By footer (an independent operator preference
already handled by `attribution.commit`/`.pr`).

## Consequences

### Positive

- Every repository is protected regardless of local config, the harness injection
  bug, or model obedience. The unrelated-repo leak that motivated this ADR — a
  trailer/footer link on a commit or PR — is closed on the covered command forms at
  the layer we control.
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
- Detection is bounded by what the hook can see in the command string, so the
  control is **best-effort, not a guarantee**. Inline `-m` and heredoc bodies are
  covered (this is how the model formats commits/PRs by default). Uncovered vectors,
  each of which would slip a link through:
  - a body/message passed via `--body-file`/`-F <path>` — the text lives in a file
    the hook can't read from the command string;
  - a bare `git commit` that opens `$EDITOR` — the message isn't in the command at
    all;
  - message-carrying siblings not in scope — `git tag -m`, `gh release create`,
    `gh gist create`.
  These are documented residual gaps, not silent ones.
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
- The hook shares the `Bash` PreToolUse matcher with the disclosure gate
  `check-bash-pre.sh`. Multiple matching hooks all run and the most-restrictive
  result wins (`deny > defer > ask > allow`), independent of array order, so the
  deny takes effect regardless of position — no ordering dependency.

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
