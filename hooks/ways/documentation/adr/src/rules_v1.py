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

# Accessors return a safe default when adr.yaml or a record has the wrong
# shape, so a malformed file becomes lint issues rather than a crash.
# rule_v1_config_shape reports the malformed adr.yaml itself.

def _str_list(value) -> Optional[list]:
    """value as a list of strings, or None when it is not one."""
    if isinstance(value, list) and all(isinstance(v, str) for v in value):
        return value
    return None

def _mapping(value) -> dict:
    return value if isinstance(value, dict) else {}

def v1_kinds(ctx) -> dict:
    return _mapping(ctx.config.get('kinds'))

def v1_record_kind(adr) -> Optional[str]:
    kind = adr.frontmatter.get('kind')
    return kind if isinstance(kind, str) else None

def v1_kind_schema(adr, ctx) -> Optional[dict]:
    kind = v1_record_kind(adr)
    schema = v1_kinds(ctx).get(kind) if kind else None
    return schema if isinstance(schema, dict) else None

def v1_verbs(ctx) -> tuple:
    return tuple(_str_list(ctx.config.get('verbs')) or V1_VERBS)

def v1_lifecycle(schema: Optional[dict]) -> tuple:
    statuses = _str_list((schema or {}).get('statuses')) or list(V1_LIFECYCLE)
    return tuple(s.lower() for s in statuses)

def v1_requires(schema: dict) -> list:
    return _str_list(schema.get('requires')) or []

def v1_edges(schema: dict) -> dict:
    """field -> list of kinds it may point at; malformed entries are dropped."""
    edges = {}
    for name, kinds in _mapping(schema.get('edges')).items():
        if isinstance(kinds, str):
            edges[name] = [kinds]
        elif _str_list(kinds) is not None:
            edges[name] = kinds
    return edges

def v1_capabilities(ctx) -> dict:
    return _mapping(ctx.config.get('capabilities'))

def v1_surfaces(ctx) -> dict:
    return _mapping(ctx.config.get('surfaces'))

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

def broader_than(prior, capability: str) -> bool:
    """The prior covers more than this one capability: '*' or a list."""
    scope = capability_scope(prior)
    return '*' in scope or len(scope) > 1

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
    def bad(message):
        ctx.config_issues.append(Issue(message, 'error'))

    config = ctx.config
    if not isinstance(config.get('kinds'), dict) or not config.get('kinds'):
        bad("contract: adr/v1 declares no kinds")
    for name, schema in v1_kinds(ctx).items():
        if not isinstance(schema, dict):
            bad(f"kinds.{name}: expected a mapping")
            continue
        verb = schema.get('verb', 'forbidden')
        if verb not in ('required', 'forbidden'):
            bad(f"kinds.{name}.verb: '{verb}' is not required or forbidden")
        for key in ('requires', 'statuses', 'sections'):
            if key in schema and _str_list(schema[key]) is None:
                bad(f"kinds.{name}.{key}: expected a list of names")
        mutable = schema.get('mutable_after_accept')
        if mutable is not None and mutable != 'all' and _str_list(mutable) is None:
            bad(f"kinds.{name}.mutable_after_accept: expected a list of fields or 'all'")
        if 'edges' in schema:
            edges = schema['edges']
            if not isinstance(edges, dict):
                bad(f"kinds.{name}.edges: expected a mapping of field to kind or kinds")
            else:
                for field_name, kinds in edges.items():
                    if not isinstance(kinds, str) and _str_list(kinds) is None:
                        bad(f"kinds.{name}.edges.{field_name}: expected a kind or a list of kinds")
    if 'verbs' in config and _str_list(config['verbs']) is None:
        bad("verbs: expected a list of names")
    if not isinstance(config.get('capabilities'), dict) or not config.get('capabilities'):
        bad("contract: adr/v1 declares no capabilities")
    if 'surfaces' in config and not isinstance(config['surfaces'], dict):
        bad("surfaces: expected a mapping of namespace to settings")
    if 'baseline' in config:
        baseline = _str_list(config['baseline'])
        if baseline is None:
            bad("baseline: expected a list of capability names")
        elif isinstance(config.get('capabilities'), dict):
            for name in baseline:
                if name not in config['capabilities']:
                    bad(f"baseline: '{name}' is not in the capabilities vocabulary")

@config_rule(contract=V1)
def rule_v1_capabilities_added(ctx):
    """Every capability in the vocabulary has an accepted `add` decision, except
    those in `baseline`: capabilities that were active when the project
    adopted the contract, which no record added. This warns while v0 records
    remain, so a migrating corpus is not failed on every capability, and fails
    once migration is done (ADR-304 §6, amended by ADR-305)."""
    added = set()
    for adr in ctx.corpus:
        if (is_v1_record(adr, ctx)
                and adr.frontmatter.get('verb') == 'add'
                and str(adr.status or '').lower() == 'accepted'):
            added.update(capability_scope(adr))
    # Archived records are never edited, so they never migrate and do not
    # hold the check at warning.
    migrating = any(not is_v1_record(adr, ctx) and not is_archived(adr.path)
                    for adr in ctx.corpus)
    baseline = set(_str_list(ctx.config.get('baseline')) or [])
    for name in v1_capabilities(ctx):
        if name not in added and name not in baseline:
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
    elif not isinstance(kind, str):
        v1_issue(adr, "kind must be a single name")
    elif v1_kind_schema(adr, ctx) is None:
        v1_issue(adr, f"kind '{kind}' is not declared in adr.yaml (declared: {', '.join(sorted(v1_kinds(ctx)))})")

@file_rule(contract=V1)
def rule_v1_status(adr, ctx):
    if not is_v1_record(adr, ctx):
        return
    schema = v1_kind_schema(adr, ctx)
    lifecycle = v1_lifecycle(schema)
    label = f"the {v1_record_kind(adr)} lifecycle" if schema else "the v1 lifecycle"
    if not adr.status:
        v1_issue(adr, "Missing status in frontmatter")
    elif str(adr.status).lower() not in lifecycle:
        v1_issue(adr, f"status '{adr.status}' is not in {label} ({', '.join(lifecycle)})")

@file_rule(contract=V1)
def rule_v1_verb(adr, ctx):
    schema = v1_kind_schema(adr, ctx)
    if not is_v1_record(adr, ctx) or schema is None:
        return
    kind = v1_record_kind(adr)
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
    for name in v1_requires(schema):
        if adr.frontmatter.get(name) in (None, '', [], {}):
            v1_issue(adr, f"a {v1_record_kind(adr)} record requires '{name}'")

@file_rule(contract=V1)
def rule_v1_capability(adr, ctx):
    if not is_v1_record(adr, ctx) or v1_kind_schema(adr, ctx) is None:
        return
    raw = adr.frontmatter.get('capability')
    if raw in (None, '', []):
        return  # reported by rule_v1_required_fields when the kind requires it
    scope = capability_scope(adr)
    if adr.frontmatter.get('verb') != 'constrain':
        if isinstance(raw, list):
            v1_issue(adr, "capability takes one name; only a constrain decision takes a list")
            return
        if '*' in scope:
            v1_issue(adr, "only a constrain decision may be scoped to '*'")
            return
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
    kind = v1_record_kind(adr)
    allowed = v1_edges(schema)
    for field_name in V1_EDGE_FIELDS:
        entries = as_entries(adr.frontmatter.get(field_name))
        if not entries:
            continue
        if field_name not in allowed:
            v1_issue(adr, f"a {kind} record takes no {field_name} edge")
            continue
        target_kinds = allowed[field_name]
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

def decision_order(adr) -> tuple:
    """Records in decision order: by date, then number (ADR-304 §3)."""
    base, _, part = str(adr.number or '0').partition('.')
    return (str(adr.date or ''), int(base or 0), int(part or 0))

def _has_decision_on(capability: str, adr, ctx) -> bool:
    """An earlier v1 decision, other than a constrain, is on this capability."""
    return any(other is not adr and is_v1_record(other, ctx)
               and other.frontmatter.get('kind') == 'decision'
               and other.frontmatter.get('verb') != 'constrain'
               and covers(other, capability)
               and decision_order(other) < decision_order(adr)
               for other in ctx.corpus)

@corpus_rule(contract=V1)
def rule_v1_change_replaces(adr, ctx):
    """A change decision supersedes or amends a prior decision on the same
    capability. When the prior covers more than this capability ('*' or a
    list), the change amends it (ADR-304 §3). The first change on a baseline
    capability has no prior record to name: the baseline stands in for it
    until a decision on that capability exists (ADR-305)."""
    if not is_v1_record(adr, ctx) or adr.frontmatter.get('verb') != 'change':
        return
    baseline = set(_str_list(ctx.config.get('baseline')) or [])
    edges = []
    for field_name in ('supersedes', 'amends'):
        for entry in as_entries(adr.frontmatter.get(field_name)):
            target = ctx.by_number.get(norm_ref(entry)[0])
            if target is None:
                return  # a dangling edge is already reported by the edge rules
            edges.append((field_name, target))
    for capability in [c for c in capability_scope(adr) if c != '*']:
        v0_priors = [t for _, t in edges if not is_v1_record(t, ctx)]
        fits = [(f, t) for f, t in edges if is_v1_record(t, ctx) and covers(t, capability)]
        if any(f == 'amends' or not broader_than(t, capability) for f, t in fits):
            continue
        if fits:
            v1_issue(adr, f"a change on '{capability}' against a broader decision amends it rather than superseding it")
        elif not edges and capability in baseline and not _has_decision_on(capability, adr, ctx):
            continue
        elif v0_priors:
            numbers = ', '.join(f"ADR-{t.number}" for t in v0_priors)
            v1_issue(adr, f"cannot confirm the prior decision on '{capability}': {numbers} is still v0", 'warning')
        else:
            v1_issue(adr, f"a change decision supersedes or amends a prior decision on '{capability}'")
