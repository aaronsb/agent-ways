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
  - operator: aaronsb
    level: authored
    said: "if someone hosts it and sells it, that seems like something I'd want a say in"
    via: chat, session 02e97f86, 2026-10-01, asked whether hosted use was part of the concern
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "I don't want dual licensing - I just want contributions back"
    via: chat, session 02e97f86, 2026-10-01, asked whether a say over commercial hosting should mean dual licensing
    covers: [veto]
status: accepted
date: 2026-10-01
deciders:
  - aaronsb
related: [ADR-177]
---

# ADR-202: License agent-ways under AGPL-3.0-or-later

## Summary

- **Decided:** agent-ways is licensed under the GNU Affero General Public License, version 3 or (at the recipient's option) any later version, from 2026-10-01. The root `LICENSE` carries the AGPL-3.0 text, every crate in `tools/` declares `AGPL-3.0-or-later`, and the README states the license and the cutover.
- **Trades away:** The permissive terms. Anyone who distributes agent-ways, or offers a modified version to others over a network, must offer the corresponding source under the same terms, which some adopters will not accept.
- **One-way?** For what is released from now on, yes in practice: moving back to a permissive license needs every copyright holder's agreement again. Everything released before the cutover stays available under MIT, which cannot be withdrawn.
- **Probes:** *Confident (hosted):* a modified agent-ways run as a paid hosted service should have to publish its changes. *Not confident (veto):* source publication is enough of a say, rather than the power to refuse or charge for commercial hosting, which needs dual licensing on top of this.
- **Inversion:** Between a permissive license any adopter can fold into closed work or a closed service, and a copyleft one that keeps both open. The decision takes network copyleft.

## Context

The repository was licensed under MIT, and its crates declared `MIT` or `MIT OR Apache-2.0`. A permissive license lets anyone take the work, extend it, and ship the result as a closed product or a closed hosted service with no obligation to share what they changed. The contributors want the opposite: improvements to agent-ways that reach other people should come back as source. The operator, with the agreement of the other contributors, chose a copyleft license, and chose the network variant once hosted resale was named as part of the concern.

What copyleft does and does not cover shaped the choice:

- The GPL binds distribution. A modified agent-ways, or a product that bundles it, can only be distributed with its source under the same license.
- The GPL does not treat a hosted service as distribution, so a modified copy run on someone's servers and offered over a network owes no source. The AGPL closes that gap: its section 13 requires offering the source to every user who interacts with a modified version over a network.
- Neither binds use. Commercial use, internal use, paid support and selling a hosted service are all allowed, as long as the source obligations are met. Refusing or licensing commercial hosting takes dual licensing, which in turn needs the right to relicense every contribution.
- Copyright covers the code and the text of the ways, not the method. A reimplementation from scratch owes nothing.
- Releases made under MIT stay under MIT.

## Decision

1. **The license.** AGPL-3.0-or-later (SPDX `AGPL-3.0-or-later`) for the whole repository: code, ways, skills, agents, scripts and docs.
2. **Where it is stated.** The root `LICENSE` holds the AGPL-3.0 text as published by the FSF. Every crate manifest under `tools/` declares `license = "AGPL-3.0-or-later"`. The README's License section names the license, the network-use obligation, the copyright holders and the cutover date.
3. **The cutover.** The license applies from 2026-10-01. Releases and commits before it were published under MIT and remain available under MIT.
4. **Third-party material keeps its license.** The fonts under `tools/agent-fmt/fonts/`, the `llama.cpp` submodule, and the Cypress bound-hook adapted in `hooks/ways/check-bash-bound.py` are MIT and keep their notices. The Rust dependency tree is under permissive licenses the AGPL-3.0 can include (MIT, Apache-2.0, ISC, BSD, Zlib, Unicode-3.0, CDLA-Permissive-2.0, BSL-1.0, CC0).

## Consequences

### Positive

- Derived works that are distributed, or offered as a service, stay open under the same terms.

### Negative

- Adopters who embed agent-ways in proprietary distributed work or a closed hosted service can no longer do so on the new releases. Some organizations bar AGPL software outright.
- The tools ADR-177 has adopting repositories copy in (`adr-tool`, `doc-tool`, `chart-tool`) carry the AGPL into those repositories, with no permissive exception: the point is that improvements come back, and that holds for the copied tools as well.
- A new dependency must be checked for AGPL-3.0 compatibility before it is added; a GPL-2.0-only dependency, for one, could not be combined.

### Neutral

- Each component's next release carries the new license; the binaries already published carry MIT.

## Alternatives Considered

- **Keep MIT.** It allows the closed redistribution and closed hosting the contributors want to rule out.
- **Apache-2.0.** It adds a patent grant, but is permissive like MIT.
- **GPL-3.0-or-later.** Binds distribution only; a modified copy sold as a hosted service would owe nothing. Rejected once hosted resale was named as part of the concern.
- **A source-available license (BSL, PolyForm).** Would give a veto over commercial hosting directly, but is not open source and would turn away contributors and adopters. Not taken in the session that made this decision; dual licensing on top of the AGPL would reach the same control while staying open source.
