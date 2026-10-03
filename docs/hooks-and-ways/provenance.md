# Adding a compliance claim to a way

A way can carry a **claim** that its guidance is *designed* to address a specific control (NIST 800-53, OWASP, ISO 27001, SOC 2, CIS, IEEE). A claim is a design assertion, not evidence that the control operates. This page shows how to write one and check it. [governance.md](../governance.md) is the reference for the fields and every `ways-audit` command, and [ADR-200](../architecture/governance/ADR-200-compliance-claims-and-session-derived-findings.md) explains the model.

## Where a claim lives

A claim is a `provenance.yaml` file in the way's own directory, beside `{name}.md` (ADR-110). The runtime never reads it, so a claim costs the agent's context nothing. It exists for the compliance tooling and for people.

```
softwaredev/delivery/commits/
├── commits.md            # the way
├── commits.check.md      # optional check
└── provenance.yaml       # the claim  ← add this
```

Claims are optional. A way without one runs identically. Operational ways such as `meta/todos` and `meta/memory` are not derived from a policy and should not carry one; a claim that cannot be defended is worse than none.

## Write the sidecar

```yaml
policy:
  - uri: governance/policies/code-lifecycle.md
    type: governance-doc
controls:
  - id: NIST SP 800-53 CM-3 (Configuration Change Control)
    justifications:
      - Conventional commit types (feat/fix/refactor) classify changes by nature
      - Atomic single-concern commits make each change independently reviewable
    satisfied_when: >
      Commits created in the session carry a conventional-commit type prefix,
      each covers a single logical change, and the message body states the
      rationale for the change.
  - id: SOC 2 CC8.1 (Change Management)
    justifications:
      - Type prefix and scope create structured change records
verified: 2026-02-05
rationale: >
  Conventional commits create structured change records with type
  classification and justification.
```

1. Under `policy`, point `uri` at the policy document the way derives from: a relative path, or `github://org/repo/path` for a policy kept in another repository.
2. Under `controls`, name each control by an ID you can defend, and check that the control means what you think it means. An unchecked citation is itself a claim.
3. For each control, write `justifications`: how the guidance is designed to address it.
4. Add `satisfied_when`: the session behavior that would show the control was met. Without it the control can never be assessed on outcome.
5. Set `verified` to today's date and write a one-paragraph `rationale`.

## Check it

`ways-audit` reads the project's `.claude/ways/` when you run it inside a project, and the shipped ways with `--global`.

```bash
ways-audit lint                                 # fields present, policy URIs resolve
ways-audit trace softwaredev/delivery/commits   # the chain for one way
ways-audit report                               # which ways carry a claim
ways-audit assemble --json                      # the finding records for these claims
```

## Keep it honest

- A `justification` says how the guidance is designed to address the control, not that it did. Showing that it did takes a session-derived **finding**. `ways-audit assemble` builds the finding record, the `satisfied_when` criterion beside the firing evidence, and leaves the determination empty for a separate assessor ([ADR-201](../architecture/governance/ADR-201-findings-assembled-as-classifier-ready-assessment-records.md)).
- Read `ways-audit report` coverage as *claims made*, not *conformance achieved*.
