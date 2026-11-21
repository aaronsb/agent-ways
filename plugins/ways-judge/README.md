# ways-judge

The relevance gate's Claude Code internal engine. It judges the gate's jobs through the session's own model, so the gate needs no Anthropic or OpenRouter key.

`ways judge` setup installs and enables it when the operator chooses "Claude Code internal", and sets `gate.engine: claude-code`. Nothing else installs it. Under any other engine the mod stays loaded and calls no model.

## What it does

The gate runs inside the settings hooks. Under `gate.engine: claude-code` the ways scan writes each judge request as a job in the session's directory and waits for the answer. A job that no host claims within half a second is withdrawn, and the gate fails open with `gate_fallback` reason `deadline: unclaimed`.

The mod wraps the two events whose chains run those hooks:

- `prompt.submit`, whose `next(e)` runs the UserPromptSubmit hooks: the prompt lane and the state lane;
- `tool.call`, whose `next(e)` runs the PreToolUse hooks, the tool itself and the PostToolUse or PostToolUseFailure hooks: the Bash, file, task, tool and post-tool lanes. It fires for the main loop and for every subagent's loop, and a subagent's call carries the session's id.

At each of them, for every prompt and every tool call, whatever its origin (the settings hooks' matchers decide which calls write a job, and the mod cannot read them), the hook:

1. registers the mod as the session's judge host with `ways judge host --session <id>`, once per session id;
2. starts the event's own chain, `next(e)`, without awaiting it;
3. while that runs, keeps one `ways judge take --session <id> --wait` waiting;
4. on a job, starts the next take at once, so a second job written meanwhile is claimed within the window, sends the job's request through `$.model.complete` unchanged, and pipes `{ text, usage }`, or `{ reason, usage }` when the model gave no text, to `ways judge answer --session <id> --job <job>` as JSON;
5. on `standby`, calls no model and starts no further take;
6. when `next(e)` settles, stops the take that is still waiting, and finishes answering any job already claimed: a take claims the session's oldest job, so the gate waiting on it may be another call's.

A call that writes no job costs one take process, stopped when the call settles. Hooking `tool.call` for every tool also turns off what Claude Code carries past a tool hook for those tools, such as moving a running call to the background.

The mod carries text and usage only. The hook side parses and validates the verdict, and it fails open at its deadline. The tool lanes give the judge at most 6 seconds inside their 9 second hook deadline, and the job's own `timeout_ms`, which the model call takes as its limit, is what remains of that budget when the job is written.

It hooks no `classic.*` event, which an organization's policy plugin passes over. No event a mod can hook wraps the SessionStart hooks, so under this engine the state lane at SessionStart writes no job and shows its candidates unjudged, as with the gate off.

## Versions

The plugin carries the `ways` version, since it calls `ways judge` verbs: `scripts/release.sh bump ways` moves both, and `make test-plugins` checks them.

## Trying a checkout

```bash
make dev-install
ways settings set gate.engine claude-code
claude --plugin-dir <checkout>/plugins/ways-judge
```

## References

- ADR-708: the Claude Code internal engine, its host registration, the job handoff, and the bracket around `next(e)` with its stop by `return()`.
- ADR-712: every lane and agent thread writes judge jobs, and the tool lanes' budgets.
