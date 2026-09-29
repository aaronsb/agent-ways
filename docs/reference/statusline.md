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
  "command": "${HOME}/.local/share/agent-ways/statusline.sh"
}
```

The path resolves on every host that has agent-ways installed, and `ways update` refreshes the script along with the rest of the app. Set `XDG_DATA_HOME` and the app lives under that directory instead of `~/.local/share`.

To pin a version, copy the file into your own config (dotfiles, `~/.claude/`) and point at the copy. A copy does not pick up later fixes; diff it against the repo when you update.

## Build on it

Source the script from your own status line and call the segments you want:

```bash
#!/usr/bin/env bash
source "${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/statusline.sh"

echo "$(sl_agent)$(sl_dir)$(sl_git)| $(my_own_segment)"
```

Sourcing defines the functions and prints nothing. The script renders a line only when executed.

| Function | Prints |
|---|---|
| `sl_agent_name` | The attend display name alone, or nothing |
| `sl_agent` | `🤖 <name> `, or nothing without attend |
| `sl_dir` | `📁 <basename of cwd> ` |
| `sl_git` | `🔀 <branch> `, with `*` on a dirty tree; nothing outside a repo |
| `sl_remote` | `📡 <owner/repo> ` from the `origin` URL; nothing without one |
| `sl_time` | `🕐 HH:MM ` |
| `statusline_render` | The full line above |

Every segment ends in a space or prints nothing, so they concatenate without separator logic. `sl_agent_name` prints the bare name for a layout of your own.

Each external call runs under `SL_TIMEOUT` seconds (default 1) through `timeout`, or `gtimeout` on macOS with Homebrew coreutils. Without either it runs unbounded. Set `SL_TIMEOUT` before sourcing to change it.

## The agent name

The name comes from `attend whoami --display`. `whoami` derives it from the session record's origin path (ADR-171), so it stays the same when the shell's working directory changes during the session. The instance suffix (`Chaucer-2`) appears when several sessions share one origin.

Read the name through the CLI and never from attend's state files. `attend whoami` is the supported surface. On attend builds before `--display`, the script parses the `display` row of the `whoami` table.

The display name is presentation. A script that keys state on the session should read `attend whoami --machine`, which prints `session_id`, `origin_path` and `resolved`, and leaves the name out.

## Tests

`make test-statusline` runs `tests/statusline-test.sh`. It checks the agent segment against stub attend builds, one with `--display` and one without, and with attend absent. It also checks that sourcing prints nothing under bash and zsh, and that an executed run renders a full line.
