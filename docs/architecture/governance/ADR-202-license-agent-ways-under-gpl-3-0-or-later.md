---
contract: adr/v1
kind: decision
verb: change
capability: governance
basis:
  - operator: aaronsb
    level: authored
    said: "I've decided to make this a gpl 3 or later license for this the other contributors clayton and jonathan agree"
    via: chat, session 02e97f86, 2026-10-01
agent:
  name: Claude
  model: claude-opus-5-5
status: accepted
date: 2026-10-01
deciders:
  - aaronsb
related: []
---

# ADR-202: License agent-ways under GPL-3.0-or-later

## Summary

- **Decided:** agent-ways is licensed under the GNU General Public License, version 3 or (at the recipient's option) any later version, from 2026-10-01. The root `LICENSE` carries the GPL-3.0 text, every crate in `tools/` declares `GPL-3.0-or-later`, and the README states the license and the cutover.
- **Trades away:** The permissive terms. Anyone distributing agent-ways, or a work built on it, must offer the corresponding source under the same terms, which some adopters will not accept.
- **One-way?** For what is released from now on, yes in practice: moving back to a permissive license needs every copyright holder's agreement again. Everything released before the cutover stays available under MIT, which cannot be withdrawn.
- **Probes:** *Confident (scope):* the license covers the whole repository, the ways corpus and docs as well as the code. *Not confident (later):* you want the "or later" option, which lets a future GPL version from the FSF apply.
- **Inversion:** Between a permissive license any adopter can fold into closed work and a copyleft one that keeps derived work open. The decision takes copyleft.

## Context

The repository was licensed under MIT, and its crates declared `MIT` or `MIT OR Apache-2.0`. The operator, with the agreement of the other contributors, chose to license it under the GPL, version 3 or later.

## Decision

1. **The license.** GPL-3.0-or-later (SPDX `GPL-3.0-or-later`) for the whole repository: code, ways, skills, agents, scripts and docs.
2. **Where it is stated.** The root `LICENSE` holds the GPL-3.0 text as published by the FSF. Every crate manifest under `tools/` declares `license = "GPL-3.0-or-later"`. The README's License section names the license, the copyright holders and the cutover date.
3. **The cutover.** The license applies from 2026-10-01. Releases and commits before it were published under MIT and remain available under MIT.
4. **Third-party material keeps its license.** The fonts under `tools/agent-fmt/fonts/` and the vendored `llama.cpp` keep their own licenses. The Rust dependency tree is under permissive licenses the GPL-3.0 can include (MIT, Apache-2.0, ISC, BSD, Zlib, Unicode-3.0, CDLA-Permissive-2.0, BSL-1.0, CC0).

## Consequences

### Positive

- Derived works that are distributed stay open under the same terms.

### Negative

- Adopters who embed agent-ways in proprietary distributed work can no longer do so on the new releases.
- A new dependency must be checked for GPL-3.0 compatibility before it is added; a GPL-2.0-only dependency, for one, could not be combined.

### Neutral

- Each component's next release carries the new license; the binaries already published carry MIT.

## Alternatives Considered

None were weighed in this record's session: the operator chose the license with the other contributors.
