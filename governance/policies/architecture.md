# Architecture Ways

Guidance for system design, API design, dependency management, database migrations, and architectural decision records.

## Design

**Triggers**: Semantic match on architecture/patterns/system design concepts

Provides a framework for design discussions rather than prescribing specific architectures. The five-step framework:

1. **Context** - What problem are we solving?
2. **Constraints** - What limits our options?
3. **Options** - What approaches could work?
4. **Trade-offs** - What does each option cost/gain?
5. **Decision** - What do we choose and why?

This structure prevents premature solutioning. Teams (and Claude) tend to jump to "let's use X" before understanding constraints. The framework forces the problem space open before narrowing to a solution.

The way includes a pattern reference table (Factory, Strategy, Observer, Repository, Adapter) with "when to use" and "when NOT to use" columns. The negative guidance matters more than the positive - knowing when a pattern is overkill prevents over-engineering.

When design discussions surface architectural trade-offs worth preserving, the way points to the ADR process for documentation.

## ADR (Agent Decision Records)

**Triggers**: Prompt mentions "ADR", "architect", "decision", "design pattern", "technical choice", "tradeoff"; editing files under `docs/architecture/`

**Macro**: Detects the project's ADR tooling (declined, installed, available) and, when installed, the contract `adr.yaml` declares. It prints the commands, record format and lifecycle for that contract.

ADRs record the "why" behind decisions. Under the adr/v1 contract (ADR-304) a record has a kind: a decision, a spec kept current, or evidence (ADR-309). A decision carries a verb, a capability, a basis naming where it came from, and opens with a Summary. The way provides:

- **When to write one**: any decision that's hard to reverse, affects multiple components, or will confuse future readers if unexplained
- **Lifecycle**: create with `adr new`, ask the operator the decision's probes and record the answer with `adr consider`, then `adr accept`; `adr lint` checks the contract throughout
- **Legacy contract**: a project whose `adr.yaml` declares no contract keeps the adr/v0 format (Status, Context, Decision, Consequences) and the debate → draft → PR → merge workflow

If the project has explicitly opted out (`.claude/no-adr-tooling`), the macro respects that choice and stops suggesting setup.

## API

**Triggers**: Semantic match on API design, REST endpoints, request handling

Covers REST API conventions that prevent common problems:

- **Pagination** - always paginate list endpoints, even if the current dataset is small
- **Error shapes** - consistent error response format across all endpoints
- **Input validation** - validate at the boundary, return clear 400 errors
- **Nested resource 404s** - distinguish "parent not found" from "child not found"
- **Versioning** - default approach and when to introduce it

The way uses semantic matching because API design surfaces in many phrasings ("add an endpoint", "build a REST service", "expose this data") that don't share keywords.

## Dependencies

**Triggers**: Prompt mentions "dependency", "package", "library", "npm install", "upgrade version"; running `npm install`, `yarn add`, `pip install`, `cargo add`, etc.

Enforces a pre-addition checklist:

1. **Necessity** - can this be done without a dependency?
2. **Maintenance** - is it actively maintained? When was the last release?
3. **Size** - what's the install footprint? (matters for frontend bundles, Lambda packages)
4. **License** - is it compatible with the project's license?

For updates, the way requires reading changelogs before bumping. Breaking changes in dependencies are a leading cause of production incidents, and the fix is simply reading the release notes.

Security audits (`npm audit`, `pip audit`, etc.) are prescribed as routine, not reactive.

## Migrations

**Triggers**: Prompt mentions "migration", "schema", "database change", "alter table", and names of common migration tools (Alembic, Prisma, Knex, Flyway, Liquibase)

Key positions:

- **Both directions** - every migration must have both up and down. If down is impossible (dropping a column with data), document why and mark it irreversible.
- **One logical change** - each migration does one thing. "Add users table" and "add index on email" are separate migrations even if they're related.
- **Tool detection** - detect the project's migration framework and follow its conventions for file naming and placement.
- **Large table warnings** - ALTER TABLE on large tables can lock the table. The way flags this risk for operations that modify existing columns.
