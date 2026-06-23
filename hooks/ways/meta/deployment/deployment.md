---
description: Choosing how agent-ways is deployed into the home config dir — in-place repo topology versus subdirectory projection topology, and copy versus symlink — the why behind each path, surfaced when install, update, or an existing ~/.claude conflict comes up
vocabulary: install setup deploy deployment topology update ~/.claude existing config clobber subdirectory in-place copy symlink sync-to-home projection upgrade reinstall fresh greenfield brownfield
pattern: install|sync-to-home|clobber|topology|~/\.claude|existing \.?claude|make update|deploy
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Deployment

agent-ways supports **two install topologies** (ADR-140). Which one a user is on
determines how they update — so when install, update, or an existing `~/.claude`
comes up, establish the topology *before* giving a command. The decision is real:
giving `make update` to a subdirectory install does nothing useful, and giving
`--dangerously-clobber` to someone with a config they value destroys it.

## The two topologies

**In-place** — `~/.claude` *is* the git clone. This is the default and the simplest
mental model: `git pull` / `make update` *is* the update, `git status` tells the
truth, there is zero drift. Choose it for a greenfield machine or anyone who wants
the least to reason about.

**Subdirectory** — the repo lives in `~/.claude/<dir>` and is *projected* into
`~/.claude` by `make sync-to-home`; `~/.claude` stays the user's own directory.
Choose it when the user **already has a `~/.claude` they value** (sessions,
credentials, their own settings) and won't turn it into someone else's repo. The
cost is a two-step update (`git pull` *then* `make sync-to-home`) and the risk of
drift if the sync step is skipped — which the `.claude-source` synced-HEAD stamp
makes detectable.

## Copy vs symlink (subdirectory only)

- **Copy** is the default and the right call for Windows / low-privilege /
  non-technical adopters — "it copied files in" is a model that survives. Re-run
  `make sync-to-home` after each pull.
- **Symlink** (`make sync-to-home-link`) is the advanced opt-in: `~/.claude/{hooks,
  bin,…}` point at the repo, so `git pull` becomes the whole update with zero drift —
  but symlinks are fragile where they need admin/developer-mode (Windows).

## Why this way exists

The first touch for many adopters is `curl … | bash`, often with *a Claude reading
the errors and guiding them*. That Claude is you. Arm yourself with the reasoning
behind the choice, not just a command to paste — an existing-config user steered to
clobber is a lost adopter.

## Decision tree

- Existing `~/.claude` you value → **subdirectory** (offered in the installer's
  conflict menu). Greenfield / want simplicity → **in-place**.
- Subdirectory on Windows / unsure → **copy**. Want `git pull` to be the whole
  update and symlinks work → **symlink**.

## See Also

- localize(meta) — sibling adopter deployment-time choice (output language)
- skills(meta) — `/sync-to-home` is the skill that drives `make sync-to-home`
