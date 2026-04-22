---
description: Makefile as the standard project task runner — build, test, lint, format, docs, release, and custom project commands
vocabulary: makefile make target build lint linter test format clean install publish release dist docs help phony check adr npm cargo pip docker repo artifacts dependencies quality ci runner
files: Makefile$|makefile$|GNUmakefile$|\.mk$
commands: make
refire: 0.1
scope: agent, subagent
macro: append
requires: ["Bash(awk:*)", "Bash(make:*)", "Bash(sort:*)", "Bash(tr:*)"]
---
<!-- epistemic: convention -->
# Makefile Way

A Makefile is the project's task runner. It's the answer to "how do I build/test/lint this repo?" regardless of language or toolchain.

## Why Make

- **Zero dependencies** — installed on every Unix system, available on Windows via WSL/MSYS
- **Discoverable** — `make help` lists what's available; reading the Makefile shows how
- **Composable** — targets chain together; CI and humans run the same commands
- **Language-agnostic** — wraps npm, cargo, pip, go, or anything else

## When to Use

- **New repo**: Scaffold a Makefile early — it's the project's control panel
- **Existing repo with no Makefile**: Propose one when you see scattered build/test commands
- **Existing Makefile**: Use it. Run `make help` or read targets before inventing ad-hoc commands

## Standard Targets

Prefer these conventional names. Not every project needs all of them.

| Target | Purpose | Example |
|--------|---------|---------|
| `help` | List available targets with descriptions | (self-documenting, see below) |
| `install` | Install dependencies | `npm ci`, `pip install -r requirements.txt` |
| `build` | Compile / bundle | `cargo build`, `npm run build` |
| `test` | Run test suite | `pytest`, `npm test`, `go test ./...` |
| `lint` | Run linters | `eslint .`, `ruff check .`, `golangci-lint run` |
| `format` | Auto-format code | `prettier --write .`, `ruff format .` |
| `clean` | Remove build artifacts | `rm -rf dist/ build/ node_modules/` |
| `docs` | Generate documentation | `mkdocs build`, `typedoc` |
| `release` | Tag + publish a release | `npm publish`, `cargo publish` |
| `dist` | Create distributable artifacts | `tar`, `docker build` |
| `adr` | ADR management (if project uses ADRs) | `docs/scripts/adr $(CMD)` |
| `check` | Run all quality gates (lint + test) | Combines lint and test targets |

## Self-Documenting Help

Use this pattern — targets document themselves via `##` comments:

```makefile
.DEFAULT_GOAL := help

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'
```

Then every target gets a description:

```makefile
test: ## Run test suite
	pytest -x --tb=short

lint: ## Run linters
	ruff check . && mypy src/
```

Running `make help` produces:

```
  help             Show this help
  test             Run test suite
  lint             Run linters
```

## Writing Targets

```makefile
.PHONY: test lint format check clean

# Use .PHONY for targets that don't produce files
# Use tabs (not spaces) for recipe indentation
# Use @ prefix to suppress command echo when the output is self-evident

test: ## Run test suite
	pytest -x --tb=short

lint: ## Run linters
	ruff check .

format: ## Auto-format code
	ruff format .

check: lint test ## Run all quality gates

clean: ## Remove build artifacts
	rm -rf dist/ build/ *.egg-info
```

## Composing with Project Tools

Make wraps the project's actual tools — it doesn't replace them:

```makefile
# ADR integration
adr: ## ADR management (usage: make adr CMD="new core title")
	docs/scripts/adr $(CMD)

# Docker
up: ## Start services
	docker compose up -d

down: ## Stop services
	docker compose down

# Multi-language: each target calls the right tool
test-backend: ## Run backend tests
	cd backend && cargo test

test-frontend: ## Run frontend tests
	cd frontend && npm test

test: test-backend test-frontend ## Run all tests
```

## Guidelines

- **Read before writing**: If a Makefile exists, run `make help` or read it before adding targets
- **Don't duplicate**: If `npm test` works, `make test` should call `npm test`, not reimplement it
- **Keep recipes short**: A target should be 1-3 commands. Complex logic belongs in a script that Make calls
- **Use variables for tunables**: Versions, paths, flags — put them at the top of the Makefile
- **CI parity**: CI should run `make check` (or `make lint && make test`), not its own bespoke commands
