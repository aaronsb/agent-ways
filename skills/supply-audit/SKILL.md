---
name: supply-audit
description: Run a read-only, tiered trust audit of an unfamiliar or untrusted repository — repo smell, secrets in git history, dangerous source patterns, and dependency vulnerabilities — ending in a go/no-go verdict. Use when vetting a repo before running or installing it, auditing a dependency or fork, or asked "is this repo safe", "audit this repo", "is this package trustworthy". Not for rewriting git history, setting up CI scanning, or fixing findings — it reads and reports only.
allowed-tools: Bash, Read, Grep, Glob
---

# Supply-Chain Trust Audit

A read-only, tiered audit of a repository you don't yet trust — a dependency, a
fork, a package, a clone you're about to run. It surfaces the evidence; **you and
the operator decide.** It never modifies the repo: no fixes, no history rewriting,
no CI setup. This is the deep, on-demand companion to the `supplychain` way's
inline pre-install check.

Run from the repo root (or pass a path). Work top-down; stop early and surface if
a tier turns up something disqualifying.

## Tier 1 — Repo smell

Cheap structural signals that something is hidden:

```bash
du -sh .git && du -sh . --exclude=.git    # .git >> tree → blobs buried in history
git rev-list --objects --all \
  | git cat-file --batch-check='%(objecttype) %(objectsize) %(rest)' \
  | awk '/^blob/ && $2 > 1048576 {print $2, $3}' | sort -rn | head   # large blobs
git ls-files -i --exclude-standard         # committed-then-gitignored (classic leak)
```

## Tier 2 — Secrets in history

History keeps credentials even after they're deleted from the tree:

```bash
git log -p --all | grep -E '(AKIA[A-Z0-9]{16}|sk-[a-zA-Z0-9]{20,}|ghp_[a-zA-Z0-9]{36}|glpat-[a-zA-Z0-9-]{20}|xox[bpas]-|-----BEGIN.*(PRIVATE|RSA))' | head -30
```

Found one? **Mask the value** in your report; if it's someone else's repo, note
responsible disclosure.

## Tier 3 — Dangerous source patterns

Language-agnostic red flags, weighted by *where* they appear (install/import-time
is worst):

| Pattern | Risk | Grep |
|---|---|---|
| `eval` / `exec` | arbitrary execution | `grep -rn 'eval\|exec'` |
| `base64` / `atob` decode | obfuscated payload | `grep -rn 'b64decode\|atob\|base64'` |
| `pickle` / `marshal` / `yaml.load` | deserialization RCE | `grep -rn 'pickle\|marshal\|yaml.load'` |
| `shell=True` / `os.system` | shell injection | `grep -rn 'shell=True\|os\.system\|os\.popen'` |
| network in setup / postinstall | exfiltration | `grep -rn 'requests\.\|urllib\|fetch(' setup.py __init__.py package.json` |

Context matters: `eval` in a template engine is normal; `eval` in `setup.py`, an
npm `postinstall`, or code that *decodes-then-executes* is the smell. Flag
install-time and import-time execution especially.

## Tier 4 — Dependency vulnerabilities

Scan the **lockfile** *before* installing — the install step itself runs code.
Detect the ecosystem and use the right tool; if it's missing, say so and suggest
the install rather than installing it yourself:

| Ecosystem | Tool |
|---|---|
| Any / multi | `osv-scanner --lockfile=<file>` |
| Python | `pip-audit -r requirements.txt` |
| Node | `npm audit` (or `osv-scanner --lockfile=package-lock.json`) |
| Rust | `cargo audit` |
| Go | `govulncheck ./...` |

Triage each hit: is the vulnerable path actually reached? dev vs. runtime dep?
patched version available? severity?

## Report

End with a structured verdict:

- **Findings by tier** — each with severity and the evidence (the command and what
  it showed). Mark each finding PROVEN (reproduced or directly observed) or
  INFERRED (reasoned from configuration or source). Give each one disposition:
  *confirmed*, *overstated or already mitigated*, or *could not reach*. Every
  candidate gets one of the three.
- **Controls that held.** A guard the audit exercised and found sound is a finding
  too. A report of only what broke hides the difference between an untested
  surface and a sound one.
- **Trust verdict** — one of: *clear* (no blocking findings), *caution* (things to
  understand before running), *do-not-run* (disqualifying: live secrets,
  install-time execution, critical unpatched vuln).
- **Could not test.** Every tier skipped (tool missing, scan declined) and every
  surface the audit never exercised (an unreadable blob, an install hook left
  untraced, a lockfile absent), so gaps read as gaps, never as a clean bill.

## Not for

- Rewriting git history to strip secrets or severing forks — that's the
  destructive `historysever` way, a separate deliberate act.
- Standing up CI scanning (Actions, Dependabot, CodeQL) — that's the `automation`
  way (setup, not audit).
- Fixing findings or upgrading dependencies — this reads and reports; remediation
  is a separate decision.

## See also

- the `supplychain` way — repository trust assessment and the inline pre-install check
- the `repoaudit` / `sourceaudit` / `depscan` ways — the rationale behind each tier
