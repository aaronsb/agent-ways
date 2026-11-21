# ways-judge

The relevance gate's Claude Code internal engine. It judges the gate's jobs through the session's own model, so the gate needs no Anthropic or OpenRouter key.

`ways judge` setup installs and enables it when the operator chooses "Claude Code internal", and sets `gate.engine: claude-code`. Nothing else installs it. When the operator picks another engine or no judge in `ways judge` setup, the setup removes the mod. Any other write of `gate.engine`, such as `ways settings set gate.engine anthropic`, marks the mod as not wanted, and the next `ways manage mod repair ways-judge --commit` or `ways manage update --commit` removes it. Until then the mod stays loaded and calls no model, but its hooks still run on every prompt and on every call of the tools listed below, and those tools keep the costs described under [What it costs](#what-it-costs).

## What it does

The gate runs inside the settings hooks. Under `gate.engine: claude-code` the ways scan writes each judge request as a job in the session's directory and waits for the answer. A job that no host claims within half a second is withdrawn, and the gate fails open with `gate_fallback` reason `deadline: unclaimed`.

The mod wraps the two events whose chains run those hooks:

- `prompt.submit`, whose `next(e)` runs the UserPromptSubmit hooks: the prompt lane and the state lane;
- `tool.call`, whose `next(e)` runs the PreToolUse hooks, the tool itself and the PostToolUse or PostToolUseFailure hooks: the Bash, file, task, tool and post-tool lanes. It fires for the main loop and for every subagent's loop, and a subagent's call carries the session's id.

The `tool.call` hook is registered only for the tools whose settings hooks run one of those lanes: Agent, Bash, Edit, EnterPlanMode, ExitPlanMode, WebFetch, Workflow and Write. The list is `JUDGED_TOOLS` in `hooks/lib.ts`, and `make test-plugins` fails when it no longer matches the matchers in `settings.json`, which name the Agent tool by its former name, Task. Claude Code does not run the hook for any other tool.

At each prompt, whatever its origin, and at each call of a listed tool (the settings hooks decide which of those calls write a job), the hook:

1. registers the mod as the session's judge host with `ways judge host --session <id>`, once per session id;
2. starts the event's own chain, `next(e)`, without awaiting it;
3. while that runs, keeps one `ways judge take --session <id> --wait` waiting;
4. on a job, starts the next take at once, so a second job written meanwhile is claimed within the window, sends the job's request through `$.model.complete` unchanged, and pipes `{ text, usage }`, or `{ reason, usage }` when the model gave no text, to `ways judge answer --session <id> --job <job>` as JSON;
5. on `standby`, calls no model and starts no further take;
6. when `next(e)` settles, stops the take that is still waiting and returns, waiting at most 3 seconds for the take to end.

A take claims the session's oldest job, so a job one call's take claims may be another call's. Each claimed job is judged apart from the event that claimed it: the mod holds the model call itself, under its own signal, and answers the job by its id. The claiming call does not wait for that judge, and interrupting it does not cancel that judge. A take that is stopped just as it claims a job either puts the job back for the next take, or hands it over, and the mod judges it all the same. The mod cuts any judge call still running when the session ends.

## What it costs

A call of a listed tool that writes no job costs one take process, stopped when the call settles.

A mod hook on a tool, even one that only passes the call on, turns off for that tool what Claude Code carries past a tool hook. For the listed tools that means:

- a running Bash command or foreground Agent call cannot be moved to the background, neither by Ctrl+B nor when a Bash command reaches its timeout, so the turn waits for it to end;
- a call's live output is not shown while it runs; its output appears when it ends;
- in Code Mode, a stopped call's result is not waited for.

The other tools keep all three, since the hook is not registered for them. The cost holds whenever the mod is loaded, whichever engine the session judges on, until the mod is removed.

## Budgets

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
