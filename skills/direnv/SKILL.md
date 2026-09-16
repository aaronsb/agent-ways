---
name: direnv
description: Set up direnv/.envrc for per-project Claude Code environments — switch API keys, providers, models, and feature flags automatically when entering a directory. Use when the user wants to manage multiple Claude accounts, switch between Anthropic/Bedrock/Vertex per project, or configure per-directory environment variables.
allowed-tools: Bash, Read, Write, Edit, Glob
---

# direnv — Per-Project Claude Code Environments

Set up `.envrc` files so Claude Code automatically loads the right API keys, provider, model, and feature flags when you `cd` into a project directory.

## Prerequisites

Check that direnv is installed and hooked into the shell:

```bash
which direnv && direnv version
# If missing: suggest installation for their platform
```

Check the shell hook is active:

```bash
grep -q 'direnv' ~/.zshrc ~/.bashrc 2>/dev/null && echo "hook found" || echo "hook missing"
```

If the hook is missing, add it:
- **zsh**: `echo 'eval "$(direnv hook zsh)"' >> ~/.zshrc`
- **bash**: `echo 'eval "$(direnv hook bash)"' >> ~/.bashrc`

## Creating a .envrc

Ask the user which scenario they need, then generate the `.envrc`:

### Direct API Key (personal/hobby)

```bash
# .envrc
export ANTHROPIC_API_KEY="sk-ant-..."
```

### AWS Bedrock

```bash
# .envrc
export CLAUDE_CODE_USE_BEDROCK=1
export AWS_PROFILE=my-bedrock-profile
export AWS_REGION=us-east-1
# Optional: override model
# export ANTHROPIC_MODEL="anthropic.claude-opus-5"
```

### Google Vertex AI

```bash
# .envrc
export CLAUDE_CODE_USE_VERTEX=1
export CLOUD_ML_REGION=us-east5
export ANTHROPIC_VERTEX_PROJECT_ID=my-project-id
```

### Microsoft Azure Foundry

```bash
# .envrc
export CLAUDE_CODE_USE_FOUNDRY=1
export ANTHROPIC_FOUNDRY_BASE_URL="https://my-resource.services.ai.azure.com/api"
export ANTHROPIC_FOUNDRY_API_KEY="..."
```

### Feature Flags / Tuning

```bash
# .envrc — append to any of the above
export ANTHROPIC_MODEL="claude-opus-5"     # pin a specific model snapshot; update as releases land
export CLAUDE_CODE_MAX_OUTPUT_TOKENS=64000
export CLAUDE_CODE_EFFORT_LEVEL=high
export CLAUDE_CODE_AUTOCOMPACT_PCT_OVERRIDE=99
```

## Security Checklist

After creating the `.envrc`:

1. **Allow it**: `direnv allow` (required after every edit)
2. **Gitignore it**: Verify `.envrc` is in `.gitignore` — it may contain secrets

```bash
grep -q '.envrc' .gitignore 2>/dev/null || echo '.envrc' >> .gitignore
```

3. **Never commit API keys** — if the project is shared, use `direnv allow` locally and keep `.envrc` out of version control.

## Verifying

After setup, verify the environment loads:

```bash
cd /path/to/project
direnv allow
env | grep -E 'ANTHROPIC|CLAUDE_CODE|AWS_PROFILE|CLOUD_ML'
```

Then start `claude` and confirm the right provider/model is active.

## Common Patterns

### Layered configs with `.envrc.local`

```bash
# .envrc (committed, non-secret defaults)
export CLAUDE_CODE_EFFORT_LEVEL=high
export CLAUDE_CODE_MAX_OUTPUT_TOKENS=64000

# Source local overrides if present
source_env_if_exists .envrc.local
```

```bash
# .envrc.local (gitignored, secrets)
export ANTHROPIC_API_KEY="sk-ant-..."
```

### Switching between projects

No action needed — direnv automatically loads/unloads when you `cd`. Just set up each project's `.envrc` once.

## Reference: Claude Code env vars

The full, current list of Claude Code environment variables — providers, model,
effort, token limits, config dir — lives in the canonical settings reference.
Don't reproduce it here; it drifts:

> https://code.claude.com/docs/en/settings.md

The examples above cover the common provider / model / flag cases.

## Not for

- Global, single-account setups — set the env vars normally; direnv earns its keep only when config differs *per project*.
- Managing non-Claude-Code environment variables — that's plain `direnv` usage, not this skill.
