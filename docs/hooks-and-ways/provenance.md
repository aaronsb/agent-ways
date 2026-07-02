# Adding a compliance claim to a way

A way can carry a **claim** that its guidance is *designed* to address a specific control
(NIST 800-53, OWASP, ISO 27001, SOC 2, CIS, IEEE). A claim is a control-*design*
assertion — not evidence the control operates. For the model and its honest scope, see
[governance.md](../governance.md) and
[ADR-200](../architecture/governance/ADR-200-compliance-claims-and-session-derived-findings.md).
This page is the how-to: author a claim and check it.

## Where a claim lives

A claim is a **`provenance.yaml` sidecar** in the way's own directory, beside `{name}.md`
(ADR-110). The runtime never reads it — way frontmatter is stripped before injection, so
a claim reaches the agent's context **never**: zero tokens, zero latency. It exists for
the compliance tooling and for humans.

```
hooks/ways/softwaredev/delivery/commits/
├── commits.md            # the way (guidance + matching frontmatter)
├── commits.check.md      # optional re-fire check
└── provenance.yaml       # the compliance claim  ← add this
```

## Write the sidecar

```yaml
# hooks/ways/softwaredev/delivery/commits/provenance.yaml
policy:
  - uri: governance/policies/code-lifecycle.md
    type: governance-doc
controls:
  - id: NIST SP 800-53 CM-3 (Configuration Change Control)
    justifications:
      - Conventional commit types classify changes by nature
      - Atomic commits make each change independently reviewable
  - id: SOC 2 CC8.1 (Change Management)
    justifications:
      - Type prefix and scope create structured change records
verified: 2026-02-05
rationale: >
  Conventional commits create structured change records with type classification,
  implementing auditable change control.
```

### Fields

| Field | Purpose |
|-------|---------|
| `policy[].uri` | Source policy document — relative path (same repo) or `github://org/repo/path` (cross-repo) |
| `policy[].type` | Classification: `adr`, `governance-doc`, `regulatory-framework`, `control-spec` |
| `controls[].id` | The control this way *claims* to be designed for |
| `controls[].justifications[]` | How the guidance is meant to address the control — assertions, not evidence |
| `verified` | Date the claim's authoring was last reviewed (not an assessment date) |
| `rationale` | Summary of how the way's guidance compiles the cited controls into practice |

A way without a `provenance.yaml` runs identically at runtime — claims are optional and
additive. Operational ways (`meta/todos`, `meta/memory`) aren't policy-derived and
shouldn't carry one; forcing a claim where none is honest is the anti-pattern.

## Check it

`ways governance` reads the sidecars directly and builds its view **in memory** — there
is no persisted manifest and no separate scripts (the former `governance.sh` /
`provenance-scan.py` were consolidated into the `ways` binary, ADR-111):

```bash
ways governance trace softwaredev/delivery/commits   # the chain for one way
ways governance report                               # which ways carry a claim
ways governance lint                                 # validate: URIs resolve, fields present
ways governance report --json                        # machine-readable
```

## Keep it honest

- A `justification` is an **assertion** about how the guidance is *designed* to address
  the control — not proof it did. Substantiating a claim needs a session-derived
  **finding** (SOC 2 Type II), which is the `ways-audit` direction
  ([ADR-151](../architecture/system/ADR-151-extract-ways-core-crate-and-ways-audit-sibling-binary.md)),
  not yet built.
- Read `ways governance report` coverage as *claims made*, not *conformance achieved*.
- Point a claim at a control you can defend by ID — and verify the control means what you
  think, because an unchecked citation is itself a claim.

## Cross-repo

Policy documents and ways often live in separate repositories:

```
compliance-repo/              your-claude-config/
├── docs/architecture/        └── hooks/ways/softwaredev/delivery/commits/
│   ├── ADR-150.md               ├── commits.md
│   └── ADR-200.md               └── provenance.yaml   (policy uri → ADR-150)
└── controls-catalog.md
```

A claim references its policy by `uri`; `ways governance` resolves those URIs and builds
the cross-repo view at query time. Nothing is persisted between the repos, so the two
sides stay decoupled.

---

*Informally:* the way is compiled guidance and the sidecar is the note saying which
standard it was compiled to address. The note is an assertion — only a finding turns it
into evidence.
