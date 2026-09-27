# ============================================================================
# adr/v1 contract rules (ADR-304)
# ============================================================================
#
# These rules run only when adr.yaml declares `contract: adr/v1`, and then
# only on records that declare it too. A record without `contract:` is adr/v0
# and keeps the v0 checks until someone migrates it (ADR-304 §7).
#
# Kinds are data: adr.yaml's `kinds:` declares each kind's verb rule, required
# fields and edge fields, and these rules read that declaration rather than
# hard-coding decision and spec (ADR-304 §1, §4).

V1 = 'adr/v1'

# ADR-304 §2: the record lifecycle (L1) and the decision verbs (L2). adr.yaml
# may restate them; these are the v1 defaults.
V1_LIFECYCLE = ('proposed', 'accepted', 'rejected', 'abandoned', 'superseded', 'archived')
V1_VERBS = ('add', 'cut', 'change', 'retire', 'constrain')

# Frontmatter fields that point at other records. superseded_by is the reverse
# of supersedes and stays under the v0 reciprocity rule.
V1_EDGE_FIELDS = ('supersedes', 'amends', 'extends', 'decided_by')

def is_v1_record(adr, ctx) -> bool:
    return ctx.contract == V1 and adr.contract == V1

def v1_kinds(ctx) -> dict:
    kinds = ctx.config.get('kinds')
    return kinds if isinstance(kinds, dict) else {}

def v1_kind_schema(adr, ctx) -> Optional[dict]:
    schema = v1_kinds(ctx).get(adr.frontmatter.get('kind'))
    return schema if isinstance(schema, dict) else None

def v1_verbs(ctx) -> tuple:
    return tuple(ctx.config.get('verbs') or V1_VERBS)

def v1_lifecycle(schema: dict) -> tuple:
    return tuple(s.lower() for s in (schema.get('statuses') or V1_LIFECYCLE))

def v1_capabilities(ctx) -> dict:
    caps = ctx.config.get('capabilities')
    return caps if isinstance(caps, dict) else {}

def v1_surfaces(ctx) -> dict:
    surfaces = ctx.config.get('surfaces')
    return surfaces if isinstance(surfaces, dict) else {}

def as_entries(value) -> list:
    if value is None:
        return []
    return [str(v) for v in value] if isinstance(value, list) else [str(value)]

def capability_scope(adr) -> list:
    """The capabilities a record covers: one name, a list, or ['*']."""
    return as_entries(adr.frontmatter.get('capability'))

def covers(prior, capability: str) -> bool:
    """A prior decision is on this capability when it names it, lists it,
    or is scoped to '*' (ADR-304 §3)."""
    scope = capability_scope(prior)
    return '*' in scope or capability in scope

def section_exists(target, section: str) -> bool:
    """A section reference matches a heading that is numbered with it ('2.',
    '2 ') or whose slug equals it ('priority-bands')."""
    for heading in target.sections:
        if heading == section or heading.startswith(f"{section}.") or heading.startswith(f"{section} "):
            return True
        if re.sub(r'[^a-z0-9]+', '-', heading.lower()).strip('-') == section.lower():
            return True
    return False

def v1_issue(adr, message, severity='error'):
    adr.issues.append(Issue(message, severity))

# --- adr.yaml ------------------------------------------------------------------

@config_rule(contract=V1)
def rule_v1_config_shape(ctx):
    if not v1_kinds(ctx):
        ctx.config_issues.append(Issue("contract: adr/v1 declares no kinds", 'error'))
    for name, schema in v1_kinds(ctx).items():
        if not isinstance(schema, dict):
            ctx.config_issues.append(Issue(f"kinds.{name}: expected a mapping", 'error'))
            continue
        verb = schema.get('verb', 'forbidden')
        if verb not in ('required', 'forbidden'):
            ctx.config_issues.append(Issue(
                f"kinds.{name}.verb: '{verb}' is not required or forbidden", 'error'))
    if not v1_capabilities(ctx):
        ctx.config_issues.append(Issue("contract: adr/v1 declares no capabilities", 'error'))

@config_rule(contract=V1)
def rule_v1_capabilities_added(ctx):
    """Every capability in the vocabulary has an accepted `add` decision. This
    warns while v0 records remain, so a migrating corpus is not failed on every
    capability, and fails once migration is done (ADR-304 §6)."""
    added = set()
    for adr in ctx.corpus:
        if (is_v1_record(adr, ctx)
                and adr.frontmatter.get('verb') == 'add'
                and str(adr.status or '').lower() == 'accepted'):
            added.update(capability_scope(adr))
    migrating = any(not is_v1_record(adr, ctx) for adr in ctx.corpus)
    for name in v1_capabilities(ctx):
        if name not in added:
            ctx.config_issues.append(Issue(
                f"capability '{name}' has no accepted add decision",
                'warning' if migrating else 'error'))

# --- one record ------------------------------------------------------------------

@file_rule(contract=V1)
def rule_v1_kind(adr, ctx):
    if not is_v1_record(adr, ctx):
        return
    kind = adr.frontmatter.get('kind')
    if not kind:
        v1_issue(adr, "adr/v1 record declares no kind")
    elif v1_kind_schema(adr, ctx) is None:
        v1_issue(adr, f"kind '{kind}' is not declared in adr.yaml (declared: {', '.join(sorted(v1_kinds(ctx)))})")

@file_rule(contract=V1)
def rule_v1_status(adr, ctx):
    schema = v1_kind_schema(adr, ctx)
    if not is_v1_record(adr, ctx) or schema is None:
        return
    lifecycle = v1_lifecycle(schema)
    if not adr.status:
        v1_issue(adr, "Missing status in frontmatter")
    elif str(adr.status).lower() not in lifecycle:
        v1_issue(adr, f"status '{adr.status}' is not in the {adr.frontmatter.get('kind')} lifecycle ({', '.join(lifecycle)})")

@file_rule(contract=V1)
def rule_v1_verb(adr, ctx):
    schema = v1_kind_schema(adr, ctx)
    if not is_v1_record(adr, ctx) or schema is None:
        return
    kind = adr.frontmatter.get('kind')
    verb = adr.frontmatter.get('verb')
    if schema.get('verb', 'forbidden') == 'required':
        if not verb:
            v1_issue(adr, f"a {kind} record requires a verb ({', '.join(v1_verbs(ctx))})")
        elif verb not in v1_verbs(ctx):
            v1_issue(adr, f"verb '{verb}' is not a decision verb ({', '.join(v1_verbs(ctx))})")
    elif verb:
        v1_issue(adr, f"a {kind} record takes no verb (found '{verb}')")

@file_rule(contract=V1)
def rule_v1_required_fields(adr, ctx):
    schema = v1_kind_schema(adr, ctx)
    if not is_v1_record(adr, ctx) or schema is None:
        return
    for name in schema.get('requires') or []:
        if adr.frontmatter.get(name) in (None, '', []):
            v1_issue(adr, f"a {adr.frontmatter.get('kind')} record requires '{name}'")

@file_rule(contract=V1)
def rule_v1_capability(adr, ctx):
    if not is_v1_record(adr, ctx) or v1_kind_schema(adr, ctx) is None:
        return
    raw = adr.frontmatter.get('capability')
    if raw is None:
        return  # reported by rule_v1_required_fields when the kind requires it
    scope = capability_scope(adr)
    verb = adr.frontmatter.get('verb')
    if (isinstance(raw, list) or '*' in scope) and verb != 'constrain':
        v1_issue(adr, "only a constrain decision may name several capabilities or '*'")
    vocabulary = v1_capabilities(ctx)
    for name in scope:
        if name != '*' and name not in vocabulary:
            v1_issue(adr, f"capability '{name}' is not in the adr.yaml vocabulary")

@file_rule(contract=V1)
def rule_v1_retire_targets(adr, ctx):
    if not is_v1_record(adr, ctx) or adr.frontmatter.get('verb') != 'retire':
        return
    targets = as_entries(adr.frontmatter.get('targets'))
    if not targets:
        v1_issue(adr, "a retire decision names its targets (targets: [cli:..., route:...])")
    surfaces = v1_surfaces(ctx)
    for target in targets:
        namespace, sep, name = target.partition(':')
        if not sep or not name:
            v1_issue(adr, f"target '{target}' is not namespace:name")
        elif namespace not in surfaces:
            v1_issue(adr, f"target '{target}': surface '{namespace}' is not declared in adr.yaml")

# --- against the corpus ------------------------------------------------------------

@corpus_rule(contract=V1)
def rule_v1_edges(adr, ctx):
    """Each edge field is allowed for the kind, resolves, points at an allowed
    kind, and a section reference names a heading that exists."""
    schema = v1_kind_schema(adr, ctx)
    if not is_v1_record(adr, ctx) or schema is None:
        return
    kind = adr.frontmatter.get('kind')
    allowed = schema.get('edges') or {}
    for field_name in V1_EDGE_FIELDS:
        entries = as_entries(adr.frontmatter.get(field_name))
        if not entries:
            continue
        if field_name not in allowed:
            v1_issue(adr, f"a {kind} record takes no {field_name} edge")
            continue
        target_kinds = allowed[field_name]
        target_kinds = target_kinds if isinstance(target_kinds, list) else [target_kinds]
        for entry in entries:
            number, section = norm_ref(entry)
            target = ctx.by_number.get(number)
            if target is None:
                if field_name != 'supersedes':  # v0 rule already reports supersedes
                    v1_issue(adr, f"{field_name}: '{entry}' resolves to no known ADR")
                continue
            if is_v1_record(target, ctx):
                target_kind = target.frontmatter.get('kind')
                if target_kind not in target_kinds:
                    v1_issue(adr, f"{field_name}: ADR-{number} is a {target_kind}, expected {' or '.join(target_kinds)}")
            if section and not section_exists(target, section):
                v1_issue(adr, f"{field_name}: ADR-{number} has no section '{section}'")

@corpus_rule(contract=V1)
def rule_v1_change_replaces(adr, ctx):
    """A change decision supersedes or amends a prior decision on the same
    capability (ADR-304 §3)."""
    if not is_v1_record(adr, ctx) or adr.frontmatter.get('verb') != 'change':
        return
    scope = [c for c in capability_scope(adr) if c != '*']
    refs = as_entries(adr.frontmatter.get('supersedes')) + as_entries(adr.frontmatter.get('amends'))
    priors = [ctx.by_number.get(norm_ref(r)[0]) for r in refs]
    priors = [p for p in priors if p is not None]
    for capability in scope:
        if not any(covers(prior, capability) for prior in priors):
            v1_issue(adr, f"a change decision supersedes or amends a prior decision on '{capability}'")
