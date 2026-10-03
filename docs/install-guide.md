# Installation Guide

Most installs are a straight line. The one-liner in the [README](../README.md#quick-start) stages the app, builds it, and projects it into `~/.claude`:

```bash
curl -sL https://raw.githubusercontent.com/aaronsb/agent-ways/main/scripts/install.sh | bash -s -- --bootstrap
```

This guide covers the paths that aren't straight: an existing `~/.claude` you care about, a previous install, or a fork you want to keep in sync. It also says what the relevance judge sends off your machine.

## What an install touches

`~/.claude` is a thin **projection** of an XDG application whose source lives in `$XDG_DATA_HOME/agent-ways` ([ADR-142](architecture/platform/ADR-142-agent-ways-1-0-xdg-application-distribution.md)). The installer:

- stages the app source in `$XDG_DATA_HOME/agent-ways` and builds or downloads the binaries;
- links the suite binaries listed in `tools/suite-bins` into `~/.local/bin`;
- symlinks the projected roots (`skills/`, `agents/`, `commands/`, `hooks/ways/`, built binaries) into `~/.claude`;
- three-way-merges its owned slices into your `settings.json`: the hooks block, its `permissions.allow` entries (its own binaries), and a `permissions.deny` secret-path baseline (`~/.ssh`, `~/.aws`, `.env`, … — [ADR-152](architecture/platform/ADR-152-framework-default-secret-path-deny-baseline.md); opt out with `ways settings set install.secret_path_deny false`);
- registers the `agent-ways` MCP server in `~/.claude.json` through `claude mcp add-json`;
- checks for a relevance-judge key and, on a terminal, offers to add one (see [The relevance judge](#the-relevance-judge)).

Everything else in `~/.claude` (`settings.json` values you set, `.credentials.json`, `projects/`, `memory/`, `CLAUDE.md`) is preserved, because the install never replaces your directory. `settings.json` is backed up before the merge. `scripts/`, `tools/` and the rest of the app stay in `$XDG_DATA` and are not projected.

The one case that needs your attention: if a projected root path (`~/.claude/skills`, `agents`, `commands`, `hooks/ways`, or a file under `~/.claude/bin`) is already a real directory or file of your own, `ways reconcile` stops before touching anything and names it. Nothing is deleted. Move it aside yourself (copy anything you want to keep into a project's `.claude/skills/`), or run `ways reconcile --force` to rename each such path to a timestamped sibling (`skills.ways-backup-<seconds>`) and then link.

`ways uninstall` reverses all of this, including the MCP registration. It lists what it would do; `ways uninstall --yes` does it. Your own ways, API keys and event log stay unless you add `--purge`.

## Activation is separate from installation

Installing stages the app and builds the binaries. Activation is what puts the projection into a Claude Code config directory, and it is recorded as a **target** in your user config ([ADR-184](architecture/platform/ADR-184-installation-and-activation-are-separate-states-targets-as-the-unit-of-activation.md)). With no `targets` key the one target is `~/.claude`, enabled, which is what every install before this model behaved as; the next `ways reconcile` or `ways update` records it, and the install is explicit from then on.

Before activating a directory, ask what it would do:

```
ways target plan ~/.claude-work
```

The plan lists every projected root as linked, to link, to relink, or refused, and shows the settings merge: the hook entries of yours it keeps, the entries it adds, and anything it would replace or remove. `ways target add <dir>` prints the same plan and stops when something of yours would be refused or removed. `ways target disable <dir>` withdraws the links and our hooks block through the same merge base that wrote them, and `ways target list` shows where agent-ways is active. `ways status` says the same on its first line.

Each target can carry its own configuration set, a `config.yaml` under `$XDG_CONFIG_HOME/agent-ways/targets/<key>/` with the same keys as the user config, layered over it for sessions under that directory. Two profiles on one machine can run different languages or disabled domains.

Claude Code relocated through `CLAUDE_CONFIG_DIR` is a second config directory. It can be a second target once the hook commands stop naming `~/.claude` (issue #503); until then the honest target is the default directory.

## The relevance judge

The judge decides which of the ways matched on your prompt are injected ([ADR-196](architecture/ways/ADR-196-a-yes-no-relevance-gate-on-way-injection-judged-by-a-hosted-model.md)). It runs in `ways-agent`, a per-user daemon that holds the key, and it calls a hosted model.

**What leaves your machine.** For each prompt, and each message you queue while Claude works, that matches at least one way, `ways-agent` sends one request to the provider whose key you stored: Anthropic (`api.anthropic.com`) or OpenRouter (`openrouter.ai`). With the shipped profiles the request carries that text and, for each matched way, its path in the ways tree and its `description`. Ways matched on commands and file edits are never judged. [What the judge sends and costs](explanation/relevance-judge/what-the-judge-sends-and-costs.md) lists every field, what is never sent, and the cost.

**Defaults.** The judge runs once a stored key passes its check, and its mode defaults to `enforce`: a way it judges irrelevant is held back. With no key the gate is off and every matched way is injected. If a call fails, the matcher's decision stands. Only the key file counts: an agent started by a hook never reads `$ANTHROPIC_API_KEY` or `$OPENROUTER_API_KEY`.

**Setup.** The installer and `ways update` end by checking each stored key with a call that costs nothing. When none passes, they say so and, on a terminal, offer to add an Anthropic or OpenRouter key; the installer reads that answer from `/dev/tty`, so it works under `curl … | bash`. Off a terminal they print the fix instead. `ways --help` repeats the warning until the judge works or you turn it off.

```bash
ways agent key add --provider anthropic    # or openrouter
ways agent status                          # engine, model, requests, fallbacks
ways settings set gate.mode shadow         # judge and log, block nothing
ways settings set gate.mode off            # no calls at all
```

`shadow` still calls and bills the provider. Only `gate.mode off` or removing the key (`ways agent key remove`) stops the calls. The `gate.*` settings live in `$XDG_CONFIG_HOME/agent-ways/agent.yaml`.

## Scenario: you already have a `~/.claude` you value

**Signs:** `~/.claude/` has `settings.json`, `projects/`, credentials, or sessions — with or without its own `.git/`.

Run the one-liner. The projection coexists with your directory: your files are untouched and your `settings.json` keeps its model, theme, plugins, and your own permissions. The one exception is a `skills/`, `agents/` or `commands/` directory of your own in `~/.claude`; reconcile stops and tells you, as described above.

If `~/.claude/` is your **own** git repo (you version-control your config), that still works: the projection adds symlinks alongside your tracked files, as long as the repo does not itself track directories at the projected root paths. If it does, reconcile refuses rather than replacing them. Add the projected roots to your `.gitignore` if you don't want them tracked.

## Scenario: a previous agent-ways install

**A projection install** (the app source is a git checkout in `$XDG_DATA_HOME/agent-ways`). Update in place:

```bash
ways update              # pull, refresh binaries, regenerate the corpus, reproject
ways update --dry-run    # show what it would run
ways update --ref REF    # pin to a branch, tag or commit; build the suite from source
ways update --ref main   # return to the release channel
```

`ways update` pulls with `--ff-only`, stashing local changes in the app dir first and restoring them after. It refreshes the binaries (prebuilt first, source build as fallback), regenerates the corpus, relinks, and reprojects `~/.claude`. Use it rather than `make setup`, which skips binaries that already exist and would leave you on the old build.

**An install last updated before ways 1.28.0.** 1.28.0 removed the readers for pre-1.0 locations ([ADR-506](architecture/platform/ADR-506-the-consolidation-ends-with-no-legacy-compatibility.md)). If any of these still exist, move them by hand; the commands spell out the XDG defaults:

| Old location | Move it |
|---|---|
| `${XDG_CACHE_HOME:-$HOME/.cache}/claude-ways/` | Derived data. Run `make setup` in the app dir, then `rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/claude-ways"`. |
| `~/.claude/stats/events.jsonl` | `s=${XDG_STATE_HOME:-$HOME/.local/state}; mkdir -p "$s/agent-ways" && cat ~/.claude/stats/events.jsonl >> "$s/agent-ways/events.jsonl"` |
| `~/.claude/ways.json` | Copy `disabled` to `disabled_domains` and `output_language` to `language` in `${XDG_CONFIG_HOME:-$HOME/.config}/agent-ways/config.yaml`. |
| `${XDG_CONFIG_HOME:-$HOME/.config}/ways/config.yaml` | Copy its keys into `agent-ways/config.yaml`; a key already there wins. Do not `mv` it over that file, which holds your `targets:`. |
| `~/.claude/.claude-upstream` | Nothing reads it. Delete it. |

attend's config files keep their paths and are now read through the settings schema. `ways settings lint` names any old form by file and line, and `ways settings fix attend.<section>` repairs most of them. [attend's configuration page](attend-and-monitor/configuration.md) says how each retired form reads now, including the `+name:` and `-name:` sensor prefixes. The `claude-projects` script is now `ways projects`, with the same subcommands.

**A pre-1.0 in-place clone** (`~/.claude` *is* the agent-ways git repo). Do not `git pull` it. The migrator ships only at the `ways-v1.8.3` tag; its [migration guide](https://github.com/aaronsb/agent-ways/blob/ways-v1.8.3/docs/migration-1.0.md) at that tag has the steps.

## Scenario: you want a fork

Fork when you want to change the shipped corpus. For ways of your own, the user root `$XDG_CONFIG_HOME/agent-ways/ways/` is enough and needs no fork. To install *from your fork*, make it the app source:

```bash
# 1. Fork on GitHub (web UI), then clone your fork as the app source
git clone https://github.com/YOUR-USERNAME/agent-ways "$XDG_DATA_HOME/agent-ways"
cd "$XDG_DATA_HOME/agent-ways"

# 2. Track upstream for later
git remote add upstream https://github.com/aaronsb/agent-ways

# 3. Install from the fork — builds, links the binaries onto PATH, and projects into ~/.claude
./scripts/install.sh
```

Running the installer from inside the app dir is what links the suite binaries in `tools/suite-bins` (`ways`, `ways-audit`, `ways-mcp`, `ways-agent`, `attend`, `attend-chat`) onto your `PATH`; `make setup` alone builds them but does not. Pull upstream improvements later:

```bash
cd "$XDG_DATA_HOME/agent-ways"
git fetch upstream && git merge upstream/main   # resolve conflicts in your custom ways
make update-binaries && ways reconcile          # force-rebuild (make setup would skip existing binaries)
```

If you're actively *developing* agent-ways, use a standalone dev checkout instead and dogfood via reconcile. See [development.md](development.md).

## After installing

1. **Restart Claude Code.** Ways activate on session start.
2. **Check status.** `ways status` shows the install target, the engine, the judge (`Gate:`), the binaries, the model and the corpus.
3. **Finish semantic matching if needed.** If `ways status` says `Engine: none`, follow [Finishing an install](finish-install.md).
4. **Configure.** `ways settings` on a terminal opens the settings screens; `ways settings list` prints every key.

## What gets downloaded

`make setup` acquires binaries and the embedding model. Downloads go through `gh`, so it must be installed and logged in (`gh auth login`); without it every binary builds from source. Downloaded artifacts live in XDG locations, outside `~/.claude/`. A suite binary or `way-embed` is checked against the release's `checksums.txt` when the release has one, and installs with a warning when it does not. The model is checked against a SHA-256 pinned in `tools/way-embed/download-model.sh`.

| Artifact | Location | Source | Required |
|----------|----------|--------|----------|
| `ways`, `ways-audit`, `attend`, `attend-chat` | `$XDG_DATA_HOME/agent-ways/bin/` (linked onto `PATH`) | GitHub Releases, or built from source | Yes |
| `ways-agent` | same | same | No: without it the judge stays off |
| `ways-mcp` | same | same | No: without it the MCP server stays unregistered |
| `way-embed` binary | XDG cache (`agent-ways/user/`) | GitHub Releases, or built from source | No: without it only pattern, command and file triggers fire |
| `minilm-l6-v2.gguf` model (~21MB) | XDG cache (`agent-ways/user/`) | GitHub Releases (or HuggingFace) | With `way-embed` |
| `mmaid` diagram renderer | XDG cache (`agent-ways/user/`) | `aaronsb/mmaid-go` releases, through `gh` | No |

If the embedding engine or model is missing, [Finishing an install](finish-install.md) walks through building it.
