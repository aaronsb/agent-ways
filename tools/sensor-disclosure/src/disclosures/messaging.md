**New topic:** `attend send "text"` — one-shot Bash call, never Monitor and never via the running `attend run` process.
**Reply to a peer:** `attend reply "text"` — auto-threads to the most recent peer message your sensor surfaced. No id lookup, no flags.
**Scope (send only):** default is `#open` — the base channel (every peer + every Aaron session). Override with `--focus {group}` or `--to {path}`.
**Quoting:** always double-quote the message — `?`, `*`, `!`, and backticks get eaten by your shell otherwise.
**Length:** notifications carry ~400 characters; anything longer is chunked into multiple lines, and the full signal file stays on disk.
**Silence is a valid reply.** Attend never escalates a message you chose to ignore — it trusts your judgment on which threads deserve an answer.
**Turn-boundary delivery:** pending messages may arrive at the end of your turn via the Stop-hook drain (`attend inbox --drain`, ADR-172) instead of a Monitor notification — same contract, same seen-set dedup; silence stays valid.
**Never run `attend run` from Bash.** The persistent sensor loop belongs to Monitor. If it is not running, ask the human or re-invoke the skill.
**CLI is the contract.** Attend owns its on-disk state. Never reach into `~/.cache/attend/` or `~/.config/attend/` — every workflow has a CLI command.
**Discovery:** `attend peers` for reachable sessions, `attend status` for your own state, `attend focus on/off {group}` to join or leave a focus group.
