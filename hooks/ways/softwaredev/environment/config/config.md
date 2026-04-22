---
description: configuration, environment variables, dotenv files, connection settings
vocabulary: dotenv environment configuration envvar config.json config.yaml connection port host url setting variable string
files: \.env|config\.(json|yaml|yml|toml)$
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Configuration Way

## Hierarchy

1. Environment variables (highest priority)
2. Config files
3. Default values (lowest priority)

## When Creating Config

- Fail fast if required config is missing — check at startup, not at first use
- Provide sensible defaults where safe (timeouts, ports, log levels)
- For secrets handling: see Security Way

## .env Files

When creating a `.env` file:
1. Also create `.env.example` with placeholder values and comments
2. Verify `.env` is in `.gitignore`

```bash
# .env.example
DATABASE_URL=postgresql://user:pass@localhost:5432/mydb
API_KEY=your-api-key-here
LOG_LEVEL=info  # debug, info, warn, error
```

## Validation Pattern

```javascript
// Check required config at startup
const required = ['DATABASE_URL', 'API_KEY'];
for (const key of required) {
  if (!process.env[key]) throw new Error(`Missing required env var: ${key}`);
}
```

## See Also

- code/security/secrets(softwaredev) — secrets belong in config, not code
