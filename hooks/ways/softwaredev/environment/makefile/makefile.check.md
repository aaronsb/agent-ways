---
description: intercept raw build, test, lint, publish commands when a Makefile exists
vocabulary: npm run npm test npx pytest cargo build cargo test go build go test pip install docker build docker compose eslint prettier ruff tsc webpack vite jest mocha cargo publish npm publish twine
commands: ^(npm\s+(run|test|ci|install|publish|build)\b|npx\s+(eslint|prettier|tsc|jest|mocha|vite|webpack)\b|pytest\b|cargo\s+(build|test|clippy|publish|run|fmt|check)\b|go\s+(build|test|vet|run|install)\b|pip\s+install\b|docker\s+(build|compose)\b|eslint\b|prettier\b|ruff\s+(check|format)\b|tsc\b|webpack\b|vite\s+build\b|jest\b|mocha\b|twine\s+upload\b)
scope: agent, subagent
when:
  file_exists: Makefile
---
## anchor
This project has a Makefile. Use `make <target>` instead of running raw commands.

## check
Before running this command directly:
- Does `make help` list a target that does the same thing?
- The Makefile wraps these commands with the project's conventions (flags, env, ordering)
- Running raw commands bypasses any project-specific setup the Makefile provides

Run `make help` if you haven't already, then use the appropriate target.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I'll just run it directly this once" | The Makefile exists because direct commands have gotchas. Use it. |
| "The Makefile probably just calls the same thing" | It often adds flags, env vars, or ordering you'll miss. |
| "I need to pass custom args" | Many Makefile targets accept variables: `make test ARGS="--verbose"` |
