---
status: Accepted
date: 2026-07-02
deciders:
  - aaronsb
  - claude
related:
  - ADR-142
  - ADR-147
supersedes: []
---

# ADR-152: Framework-default secret-path deny baseline

## Context

agent-ways already owns a slice of Claude Code's `settings.json`. During
`ways reconcile` the settings three-way merge (ADR-142, ADR-147) set-unions a
fixed list of permission strings — `WAYS_PERMS` — into `permissions.allow`
(`Bash(ways:*)`, `Edit(~/.claude/**)`, …) and tracks them in a base so entries it
stops owning are cleaned up. The user's own `permissions` entries are held
invariant beside ours.

`permissions.deny` is currently empty. Nothing stops the agent's **file tools**
(`Read`, `Edit`, `Write`) from reaching credential material — `~/.ssh` private
keys, `~/.aws/credentials`, GPG keyrings, a project `.env`. A single mistaken
tool call can read a private key into the transcript, where it is then persisted
and, if the transcript is ever shared, leaked. The framework projects itself into
every session; it should also project a floor of protection for the paths that
are almost never a legitimate read target for an agent.

Claude Code's permission model makes this expressible: `permissions.deny` is a
list of `Tool(pattern)` rules (`Read(./.env)`, `Read(~/.ssh/**)`,
`Read(//abs/path/**)`), and **deny takes precedence over allow** — a deny is a
hard block, not a prompt. That precedence is the whole point (a broad
`Read(~/**)` allow cannot re-open a denied secret path) and also the constraint
this ADR must respect: a forced deny the user cannot override via `allow` needs a
deliberate, documented escape hatch, or it becomes a cage.

## Decision

### 1. Ship a curated secret-path deny baseline, reconciler-owned

Add a `WAYS_DENY` list, the deny-side sibling of `WAYS_PERMS`, set-unioned into
`permissions.deny` by the same merge and tracked in the same base. The baseline
is intentionally **narrow and high-confidence** — paths an agent has essentially
no legitimate reason to read or write:

```
Read(~/.ssh/**)      Edit(~/.ssh/**)      Write(~/.ssh/**)
Read(~/.aws/**)      Read(~/.gnupg/**)    Read(~/.config/gcloud/**)
Read(~/.kube/config) Read(~/.netrc)
Read(./.env)         Read(./.env.local)
```

`~/.ssh` denies read **and** write (the agent should neither exfiltrate a key nor
tamper with `authorized_keys`/`config`). The credential stores deny read.
Project env files deny the secret ones (`.env`, `.env.local`) but **not**
`.env.example` / `.env.sample`, which are meant to be read. The list is a
starting floor, expected to grow through the same review as any owned-permission
change — not a claim of completeness.

### 2. Secure by default, with an explicit opt-out

The baseline applies by default. Because a Claude Code deny cannot be overridden
by a user `allow`, autonomy is preserved through a config flag, not settings
editing: `secret_path_deny: false` in `$XDG_CONFIG_HOME/agent-ways/config.yaml`
suppresses the baseline entirely (default `true`). A user who genuinely needs the
agent to read a protected path opts the whole baseline out deliberately, rather
than having it silently re-asserted on the next reconcile. This mirrors the
`disabled_domains` pattern: framework behavior on by default, off by one explicit
line.

### 3. Honest scope — this gates tools, not the shell

The deny binds the `Read`/`Edit`/`Write` **tools**. It does **not** stop a shell
command — `Bash(cat ~/.ssh/id_ed25519)` reads the same file through a different
door, and command-string denial is unreliable (countless spellings). Bash-level
exfiltration is a distinct, harder problem addressed elsewhere (the adversarial
`contributions` way, the hardened code-reviewer). This baseline is a meaningful
floor against the most common accident — the agent's own file tools wandering
into a secret — not a claim of exfiltration-proofing. Overselling it would be the
exact overclaim the compliance work (ADR-200) exists to avoid.

## Consequences

### Positive

- Secret material is protected from the agent's file tools by default, in every
  reconciled session, at zero user effort.
- Reuses the proven owned-permission merge machinery — no new projection seam.
- The opt-out keeps the default honest: secure, but not a cage.

### Negative

- A forced deny can surprise a user whose task legitimately needs a protected
  path; they must know about the config opt-out. Mitigated by documenting it
  where the deny is described.
- The curated list is a maintenance surface and a judgment call — too broad
  breaks workflows, too narrow misses secrets. It ships deliberately conservative.

### Neutral

- Bash-path exfiltration remains out of scope, by design and stated plainly.
- The list can grow; each addition is an owned-permission change reviewed like any
  other, and the three-way base cleans up entries later removed.

## Alternatives Considered

- **Seed once, never re-assert.** Add the deny on first reconcile but let a user
  removal stick. Rejected: it makes the security floor silently erodible and
  diverges the deny logic from the forced-allow logic for no clear gain; the
  config opt-out gives the same autonomy explicitly.
- **`ask` instead of `deny`.** Use `permissions.ask` so secret paths prompt.
  Rejected for the baseline: a prompt is the right tool for *ambiguous* paths, but
  for private keys and credential stores a hard default block is the safer floor.
  `ask` remains available to users for their own softer cases.
- **Do nothing / leave to the user.** Rejected: the framework already projects
  permissions; declining to project the one that protects secrets, when the
  syntax makes it a few lines, is a missed default.

## References

- **ADR-142 / ADR-147** — the projection and the settings fragment/merge machinery
  this extends.
- **Claude Code permissions** — `permissions.deny`, rule syntax, and deny-over-allow
  precedence. https://code.claude.com/docs/en/permissions
