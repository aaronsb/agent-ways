---
description: Node.js dependency security, npm audit, postinstall scripts, typosquatting
vocabulary: npm audit package-lock.json node_modules postinstall preinstall yarn pnpm npx typosquat javascript typescript
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Node.js Dependency Security

## Scanning

```bash
# Built-in, zero install
npm audit

# Or with osv-scanner
osv-scanner --lockfile=package-lock.json
```

## Node-Specific Risks

- **`postinstall` scripts run on `npm install`.** Check `package.json` scripts section for unfamiliar packages. `npm install --ignore-scripts` skips them but may break legitimate packages.
- **`npx` runs packages without installing** — convenient but downloads and executes in one step. Know what you're running.
- **Transitive dependencies are the real surface.** A project with 20 direct deps can have 800+ transitive. `npm audit` covers them all.
- **Typosquatting is rampant on npm.** `lodahs`, `crossenv`, `event-stream` (compromised maintainer). Verify package names and check download counts on npmjs.com.
