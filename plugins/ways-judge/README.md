# ways-judge

The relevance gate's Claude Code internal engine (ADR-708). It judges the gate's jobs through the session's own model, so the gate needs no Anthropic or OpenRouter key.

`ways judge` setup installs and enables it when the operator chooses "Claude Code internal", and sets `gate.engine: claude-code`. Nothing else installs it. Under any other engine the mod stays loaded and calls no model.

## What it does

On each prompt, whatever its origin (the settings hook that writes the job cannot see the origin), the `prompt.submit` hook:

1. registers the mod as the session's judge host with `ways judge host --session <id>`, once per session id;
2. starts the prompt's own chain, `next(e)`, which runs the settings UserPromptSubmit hooks and the ways scan, without awaiting it;
3. beside it, waits on `ways judge take --session <id> --wait`;
4. on a job, sends the job's request through `$.model.complete` unchanged and pipes `{ text, usage }`, or `{ reason, usage }` when the model gave no text, to `ways judge answer --session <id>` as JSON;
5. on `standby`, calls no model;
6. when `next(e)` settles, stops a take that is still waiting.

The mod carries text and usage only. The hook side parses and validates the verdict, and it fails open at its deadline (ADR-708 §4, §5).

It hooks no `classic.*` event, which an organization's policy plugin passes over.

## Versions

The plugin carries the `ways` version, since it calls `ways judge` verbs: `scripts/release.sh bump ways` moves both, and `make test-plugins` checks them.

## Trying a checkout

```bash
make dev-install
ways settings set gate.engine claude-code
claude --plugin-dir <checkout>/plugins/ways-judge
```
