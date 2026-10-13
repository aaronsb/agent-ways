# Status Line

`statusline.sh` at the repo root is the agent-ways reference status line for Claude Code. It shows the session's attend name, the directory, the git branch and remote, and the time:

```
🤖 Chaucer 📁 agent-ways 🔀 main* 📡 aaronsb/agent-ways | 🕐 11:53
```

agent-ways does not install it or write `statusLine` into `settings.json`. User settings belong to the operator (ADR-169), so each operator opts in with one of the two setups below.

## Use it as is

Point `statusLine` at the copy inside the app install:

```json
"statusLine": {
  "type": "command",
  "command": "${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/statusline.sh"
}
```

The shell expands the command on each host, so it finds the install wherever `XDG_DATA_HOME` puts it, and `ways manage update` refreshes the script along with the rest of the app.

## When dotfiles manage your config

ADR-164 requires a file carried through a config pipeline such as dotfiles to travel by value. A symlink from dotfiles into the app directory dangles on a host with a different layout, and it leaves the app as the owner of the content. Two setups meet that rule:

- **Copy.** Put a copy of `statusline.sh` in your dotfiles and point `statusLine` at the deployed copy. It stays fixed until you update it; diff it against the repo after `ways manage update`.
- **Source.** Keep your own script in dotfiles and source the app's copy from it, as below. Your script owns the layout, it resolves the app path on each host, and the segments update with the app. Guard the `source` with `[[ -r ... ]]` when a host may lack agent-ways.

The plain `statusLine` command above carries no file through the pipeline, only a setting whose path the shell expands per host.

## Build on it

Source the script from your own status line and call the segments you want:

```bash
#!/usr/bin/env bash
SL="${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/statusline.sh"
[[ -r "$SL" ]] && source "$SL"

echo "$(sl_agent)$(sl_dir)$(sl_git)| $(my_own_segment)"
```

Sourcing defines the functions and prints nothing, from bash or zsh. The script renders a line only when executed. Segments return 0, so a caller under `set -euo pipefail` survives a failed command.

| Function | Prints |
|---|---|
| `sl_agent_name` | The attend display name alone, or nothing |
| `sl_agent` | `🤖 <name> `, or nothing without attend |
| `sl_dir` | `📁 <basename of cwd> ` |
| `sl_git` | `🔀 <branch> `, with `*` on a dirty tree; nothing outside a repo |
| `sl_remote` | `📡 <owner/repo> ` from the `origin` URL, credentials dropped; nothing when there is no origin or the URL does not parse |
| `sl_time` | `🕐 HH:MM ` |
| `statusline_render` | The full line above |

Every segment ends in a space or prints nothing, so they concatenate without separator logic. `sl_agent_name` prints the bare name for a layout of your own.

Each external call runs under `SL_TIMEOUT` seconds (default 1) through `timeout`, or `gtimeout` on macOS with Homebrew coreutils. Without either it runs unbounded. A hung attend costs one `SL_TIMEOUT`. Set `SL_TIMEOUT` before sourcing to change it.

## The agent name

The name comes from `attend peer whoami --display`. `whoami` derives it from the session record's origin path (ADR-171), so it stays the same when the shell's working directory changes during the session. The instance suffix (`Chaucer-2`) appears when several sessions share one origin.

Read the name through the CLI and never from attend's state files. `attend peer whoami --display` is the supported surface. An attend without that command shows no name.

The display name is presentation. A script that keys state on the session should read `attend peer whoami --machine`, which prints `session_id`, `origin_path` and `resolved`, and leaves the name out.

## Tests

`make test-statusline` runs `tests/statusline-test.sh`. It checks:

- the agent segment against stub attend builds: one with `peer whoami --display`, one without it, one that hangs, and none at all
- a strict-mode caller surviving failed commands
- sourcing under bash, zsh, and zsh with `nounset`, and a segment called from zsh
- the branch, dirty and detached-HEAD forms of `sl_git` in a throwaway repo
- `sl_remote` across SSH, HTTPS, `ssh://`, port, trailing-slash and credential URLs
- a full line from an executed run
