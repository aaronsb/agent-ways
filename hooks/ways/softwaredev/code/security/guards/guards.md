---
description: designing a restrictive rule such as an allowlist, denylist, filter, quota, rate limit, or validator so it catches what it must and keeps the ordinary path working; fallback posture fail-open or fail-closed; never widen a control to clear a symptom
vocabulary: allowlist denylist blocklist filter guard validator quota rate limit fallback fail-open fail-closed widen loosen relax the rule
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Guards Way

A restrictive rule is written from the case that motivated it and evaluated by every case that did not. The rule serves its target path as designed while an ordinary path it never considered degrades or stops. The second failure is quieter: the rule breaks something, and the fix widens the rule until the symptom clears, taking the protection with it.

## Design Against Two Sets

Every allowlist, denylist, filter, quota, rate limit, and validator has two sets to satisfy: what it must catch, and what must keep working. Before it ships:

1. Enumerate the traffic, callers, or inputs it will now deny.
2. Confirm each denial is intended. Check the common cases as well as the case the rule was written for.
3. Where the rule is positional (an ordered chain, a first-match table, a priority queue), include what its placement displaces.
4. State the invariant the rule may not violate, in the same change that introduces it. A written baseline can be tested; one in the author's head is rediscovered by whoever it breaks.

## Decide the Failure

A guard that cannot block is decoration. A guard whose own internal failure blocks everything is a wedge. Each guard takes one of these failures, and the choice is written down next to the guard.

| Posture | When the guard itself fails | Fits when |
|---|---|---|
| Fail-closed | Deny, and emit a signal | The guard protects data, money, or identity |
| Fail-open | Allow, and emit a signal | The guard refines an optional path and denial would strand required work |

A silent fallback is a fail-open nobody decided. A degraded path is never a less-filtered path: when the guarded component is unavailable, fail visibly. A protective transform that errors internally drops or masks the value. A library default counts as no decision until someone writes it down.

## Never Weaken a Control to Clear a Symptom

Widening an allowlist, granting the privilege, lengthening a lifetime, or disabling a dependent control makes the error disappear along with the protection. Diagnose first. Prove from evidence that the control causes the fault, then fix the controllable cause: the transport, the file ownership, the missing configuration.

- Repair a fail-open primary control before layering a compensating one over it.
- A guard made redundant by an earlier one stays as defense in depth.
- Where a weakening has been tried once already, pin the temptation with a contract test.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "It works for the case I tested" | The rule applies to everything. Enumerate what else it now denies. |
| "Widening the allowlist fixed the error" | The error is gone. Is the fault? Diagnose before touching the control. |
| "The library falls back to a safe default" | Nobody chose it. Write the posture down and emit a signal on degrade. |
| "The check is redundant now" | Redundant guards are defense in depth. Delete only with the reason recorded. |
| "Nothing visibly broke when it degraded" | Damage from a silent fallback is invisible by construction. Emit a signal. |

## See Also

- code/security(softwaredev) — least privilege and validation at boundaries
- code/security/injection(softwaredev) — validators at the input boundary
- code/security/auth(softwaredev) — an authorization check is a guard with a decided failure
- architecture/threat-modeling(softwaredev) — the blast radius a guard bounds
