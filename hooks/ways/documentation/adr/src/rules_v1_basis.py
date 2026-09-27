# ============================================================================
# adr/v1: basis, agent, consideration and concerns (ADR-304 §11, §12)
# ============================================================================
#
# A decision's basis names what it rests on. precedent points at another
# record in the corpus; every other source is external. Following precedent
# must reach an external source: a corpus that justifies itself only by citing
# itself can drift anywhere and still look consistent.
#
# The operator basis and `considered` are an audit trail, not a credential.
# Lint checks that `said` and `via` are present. It cannot check that they are
# genuine (ADR-304 §11, "Fabrication risk").

V1_BASIS_SOURCES = ('operator', 'evidence', 'standard', 'upstream', 'precedent')
V1_OPERATOR_LEVELS = ('authored', 'directed', 'guided')
V1_OPERATOR_KEYS = ('level', 'said', 'via', 'paraphrase')
V1_CONSIDERED_KEYS = ('operator', 'said', 'via', 'paraphrase', 'covers', 'canary')
V1_CONCERN_KEYS = ('said', 'resolve', 'answer', 'withdrawn', 'raised')
V1_ANSWER_KEYS = ('operator', 'said', 'via', 'paraphrase')
V1_CANARY = ('caught', 'missed')
# A precedent is "another accepted decision" (§11). These statuses ground;
# proposed warns; anything else (rejected, abandoned) does not ground.
V1_GROUNDING_STATUSES = ('accepted', 'superseded', 'archived')

def v1_basis_sources(ctx) -> tuple:
    """adr.yaml may rename or extend the sources. precedent is the one
    internal source; every other declared source is external."""
    return tuple(_str_list(ctx.config.get('basis_sources')) or V1_BASIS_SOURCES)

def v1_external_sources(ctx) -> tuple:
    return tuple(s for s in v1_basis_sources(ctx) if s != 'precedent')

def _text(value) -> bool:
    return isinstance(value, str) and value.strip() != ''

def _first_line(value) -> str:
    lines = value.strip().splitlines() if _text(value) else []
    return lines[0][:70] if lines else '(no said)'

def basis_entries(adr, ctx) -> list:
    """(source, value, entry) for each well-formed basis entry."""
    out = []
    sources_known = v1_basis_sources(ctx)
    basis = adr.frontmatter.get('basis')
    for entry in basis if isinstance(basis, list) else []:
        if isinstance(entry, dict):
            sources = [k for k in entry if k in sources_known]
            if len(sources) == 1:
                out.append((sources[0], entry[sources[0]], entry))
    return out

def has_operator_basis(adr, ctx) -> bool:
    return any(source == 'operator' for source, _, _ in basis_entries(adr, ctx))

def _unknown_keys(entry: dict, allowed: tuple) -> list:
    return [k for k in entry if k not in allowed]

# --- adr.yaml ------------------------------------------------------------------

@config_rule(contract=V1)
def rule_v1_basis_config(ctx):
    raw = ctx.config.get('basis_sources')
    if raw is None:
        return
    sources = _str_list(raw)
    if sources is None:
        ctx.config_issues.append(Issue("basis_sources: expected a list of names", 'error'))
    elif not any(s != 'precedent' for s in sources):
        ctx.config_issues.append(Issue("basis_sources: declares no external source, so no chain can leave the corpus", 'error'))

# --- one record ----------------------------------------------------------------

@file_rule(contract=V1)
def rule_v1_basis_shape(adr, ctx):
    if not is_v1_record(adr, ctx) or 'basis' not in adr.frontmatter:
        return
    basis = adr.frontmatter.get('basis')
    schema = v1_kind_schema(adr, ctx)
    if basis == [] and schema is not None and 'basis' in v1_requires(schema):
        return  # empty where the kind requires a basis: the requires rule reports it
    if not isinstance(basis, list):
        v1_issue(adr, "basis: expected a list of entries, each naming one source")
        return
    allowed = v1_basis_sources(ctx)
    for i, entry in enumerate(basis, 1):
        where = f"basis entry {i}"
        if not isinstance(entry, dict):
            v1_issue(adr, f"{where}: expected a mapping such as '- evidence: ...'")
            continue
        sources = [k for k in entry if k in allowed]
        unknown = _unknown_keys(entry, tuple(allowed) + V1_OPERATOR_KEYS)
        if unknown:
            v1_issue(adr, f"{where}: unknown key {', '.join(repr(k) for k in unknown)} (sources: {', '.join(allowed)})")
            continue
        if len(sources) != 1:
            v1_issue(adr, f"{where}: names {len(sources)} sources; each entry names exactly one")
            continue
        source = sources[0]
        value = entry[source]
        if source != 'operator':
            extra = [k for k in entry if k in V1_OPERATOR_KEYS]
            if extra:
                v1_issue(adr, f"{where}: {', '.join(extra)} {'belongs' if len(extra) == 1 else 'belong'} to an operator entry")
            if source == 'precedent' and (not isinstance(value, (str, int)) or isinstance(value, bool)):
                v1_issue(adr, f"{where}: precedent takes one reference, such as ADR-101")
            elif not _text(value) and not (isinstance(value, int) and not isinstance(value, bool)):
                v1_issue(adr, f"{where}: {source} needs a reference")
            continue
        # operator: who, the level, what was said and via which channel
        if not _text(value):
            v1_issue(adr, f"{where}: operator names who")
        if entry.get('level') not in V1_OPERATOR_LEVELS:
            v1_issue(adr, f"{where}: level is one of {', '.join(V1_OPERATOR_LEVELS)}")
        for key in ('said', 'via'):
            if not _text(entry.get(key)):
                v1_issue(adr, f"{where}: an operator basis records '{key}'")
        if 'paraphrase' in entry and not isinstance(entry['paraphrase'], bool):
            v1_issue(adr, f"{where}: paraphrase is true or false")

@file_rule(contract=V1)
def rule_v1_agent(adr, ctx):
    if not is_v1_record(adr, ctx) or 'agent' not in adr.frontmatter:
        return
    agent = adr.frontmatter.get('agent')
    if not isinstance(agent, dict):
        v1_issue(adr, "agent: expected {name, model}")
        return
    for key in ('name', 'model'):
        if not _text(agent.get(key)):
            v1_issue(adr, f"agent: records '{key}'")

@file_rule(contract=V1)
def rule_v1_considered(adr, ctx):
    if not is_v1_record(adr, ctx):
        return
    considered = adr.frontmatter.get('considered')
    if considered is not None:
        if not isinstance(considered, list) or not considered:
            v1_issue(adr, "considered: expected a list of entries")
            considered = []
        for i, entry in enumerate(considered, 1):
            where = f"considered entry {i}"
            if not isinstance(entry, dict):
                v1_issue(adr, f"{where}: expected a mapping with operator, said and via")
                continue
            unknown = _unknown_keys(entry, V1_CONSIDERED_KEYS)
            if unknown:
                v1_issue(adr, f"{where}: unknown key {', '.join(repr(k) for k in unknown)} (keys: {', '.join(V1_CONSIDERED_KEYS)})")
            for key in ('operator', 'said', 'via'):
                if not _text(entry.get(key)):
                    v1_issue(adr, f"{where}: records '{key}'")
            covers_list = entry.get('covers')
            if covers_list is not None and _str_list(covers_list) is None:
                v1_issue(adr, f"{where}: covers is a list of probe names")
            if 'canary' in entry and entry['canary'] not in V1_CANARY:
                v1_issue(adr, f"{where}: canary is caught or missed")
    # ADR-304 §12: a decision the operator started waits for their consideration
    if (has_operator_basis(adr, ctx) and str(adr.status or '').lower() == 'accepted'
            and not adr.frontmatter.get('considered')):
        v1_issue(adr, "accepted with an operator basis but no considered entry; the operator considers what they started")

@file_rule(contract=V1)
def rule_v1_concern(adr, ctx):
    """Concerns are few, actionable and never silent (ADR-304 §12). An open
    concern is shown at lint time and does not fail it."""
    if not is_v1_record(adr, ctx) or 'concern' not in adr.frontmatter:
        return
    concerns = adr.frontmatter.get('concern')
    if not isinstance(concerns, list) or not concerns:
        v1_issue(adr, "concern: expected a list of entries")
        return
    for i, entry in enumerate(concerns, 1):
        where = f"concern {i}"
        if not isinstance(entry, dict):
            v1_issue(adr, f"{where}: expected a mapping with said and resolve")
            continue
        unknown = _unknown_keys(entry, V1_CONCERN_KEYS)
        if unknown:
            v1_issue(adr, f"{where}: unknown key {', '.join(repr(k) for k in unknown)} (keys: {', '.join(V1_CONCERN_KEYS)})")
        if not _text(entry.get('said')):
            v1_issue(adr, f"{where}: records what the concern is ('said')")
        if not _text(entry.get('resolve')):
            v1_issue(adr, f"{where}: names what would resolve it ('resolve')")
        answer, withdrawn = entry.get('answer'), entry.get('withdrawn')
        if answer is not None and withdrawn is not None:
            v1_issue(adr, f"{where}: is either answered or withdrawn, not both")
        elif withdrawn is not None:
            if not _text(withdrawn):
                v1_issue(adr, f"{where}: a withdrawn concern states why")
        elif answer is not None:
            if (not isinstance(answer, dict) or _unknown_keys(answer, V1_ANSWER_KEYS)
                    or not _text(answer.get('said')) or not _text(answer.get('via'))):
                v1_issue(adr, f"{where}: an answer records 'said' and 'via' (keys: {', '.join(V1_ANSWER_KEYS)})")
        else:
            adr.issues.append(Issue(f"open concern: {_first_line(entry.get('said'))}", 'warning', 'open-concern'))

# --- against the corpus: grounding -------------------------------------------------
#
# Grounding is a least fixed point, so the verdict does not depend on the order
# of basis entries and each record is visited a bounded number of times:
#
#   grounded(r) = r has an external source
#              or some precedent of r points at a grounding-status record t
#                 with grounded(t)
#
# A record with no basis but a decided_by (a spec) is grounded through the
# records that decided it. A non-archived v0 record grounds provisionally: the
# chain passes with a warning until the record is migrated. An archived v0
# record grounds outright, since archived records never migrate.

def _is_v0_ground(target, ctx) -> Optional[str]:
    """'final' for an archived v0 record, 'provisional' for a live one."""
    if is_v1_record(target, ctx):
        return None
    return 'final' if is_archived(target.path) else 'provisional'

def _grounding_map(ctx) -> dict:
    """path -> 'final' | 'provisional' for every grounded v1 record."""
    cache = getattr(ctx, '_grounding', None)
    if cache is not None:
        return cache
    records = [a for a in ctx.corpus if is_v1_record(a, ctx)]
    external = v1_external_sources(ctx)
    grounded = {}
    for adr in records:
        if any(source in external for source, _, _ in basis_entries(adr, ctx)):
            grounded[adr.path] = 'final'

    def level_of(target) -> Optional[str]:
        v0 = _is_v0_ground(target, ctx)
        if v0:
            return v0
        return grounded.get(target.path)

    changed = True
    while changed:
        changed = False
        for adr in records:
            best = grounded.get(adr.path)
            if best == 'final':
                continue
            candidates = []
            entries = basis_entries(adr, ctx)
            if entries:
                for value in _precedent_values(adr, ctx):
                    target = ctx.by_number.get(norm_ref(value)[0])
                    if target is None or str(target.status or '').lower() not in V1_GROUNDING_STATUSES + ('proposed',):
                        continue
                    candidates.append(level_of(target))
            elif 'basis' not in adr.frontmatter:
                for ref in as_entries(adr.frontmatter.get('decided_by')):
                    target = ctx.by_number.get(norm_ref(ref)[0])
                    if target is not None:
                        candidates.append(level_of(target))
            new = 'final' if 'final' in candidates else ('provisional' if 'provisional' in candidates else None)
            if new and new != best:
                grounded[adr.path] = new
                changed = True
    ctx._grounding = grounded
    return grounded

def _precedent_values(adr, ctx) -> list:
    """Precedent references of a usable shape; the shape rule reports others."""
    return [value for source, value, _ in basis_entries(adr, ctx)
            if source == 'precedent' and isinstance(value, (str, int)) and not isinstance(value, bool)]

def _precedent_targets(adr, ctx) -> list:
    return [ctx.by_number.get(norm_ref(value)[0]) for value in _precedent_values(adr, ctx)]

def _cycle_through(adr, ctx) -> Optional[list]:
    """The numbers on a precedent cycle that returns to adr, if any."""
    stack = [(adr, [adr])]
    visited = set()
    while stack:
        node, path = stack.pop()
        for target in _precedent_targets(node, ctx):
            if target is None or not is_v1_record(target, ctx):
                continue
            if target.path == adr.path:
                return [n.number or n.path.name for n in path]
            if target.path not in visited:
                visited.add(target.path)
                stack.append((target, path + [target]))
    return None

@corpus_rule(contract=V1)
def rule_v1_basis_chain(adr, ctx):
    """Every precedent resolves to an allowed, accepted record, and following
    precedent reaches an external source (ADR-304 §11)."""
    if not is_v1_record(adr, ctx):
        return
    entries = basis_entries(adr, ctx)
    precedents = [(value, ctx.by_number.get(norm_ref(value)[0]))
                  for value in _precedent_values(adr, ctx)]
    schema = v1_kind_schema(adr, ctx) or {}
    allowed_kinds = v1_edges(schema).get('basis')
    for value, target in precedents:
        if target is None:
            v1_issue(adr, f"basis: precedent '{value}' resolves to no known ADR")
            continue
        status = str(target.status or '').lower()
        if is_v1_record(target, ctx):
            kind = v1_record_kind(target)
            if allowed_kinds is not None and kind not in allowed_kinds:
                v1_issue(adr, f"basis: precedent ADR-{target.number} is a {kind}, expected {' or '.join(allowed_kinds)}")
            if status == 'proposed':
                adr.issues.append(Issue(f"basis: precedent ADR-{target.number} is still proposed", 'warning', 'precedent-proposed'))
            elif status not in V1_GROUNDING_STATUSES:
                v1_issue(adr, f"basis: precedent ADR-{target.number} is {status or 'without a status'}, so it grounds nothing")
            elif 'basis' not in target.frontmatter and not target.frontmatter.get('decided_by'):
                v1_issue(adr, f"basis: precedent ADR-{target.number} has neither a basis nor decided_by")
    if not entries:
        return
    level = _grounding_map(ctx).get(adr.path)
    if level == 'final':
        return
    if level == 'provisional':
        v0s = sorted({t.number for _, t in precedents if t is not None and _is_v0_ground(t, ctx) == 'provisional'})
        via = f" (ADR-{', ADR-'.join(v0s)})" if v0s else ''
        v1_issue(adr, f"basis: grounded only through records still on v0{via}; migrate them to confirm the chain", 'warning')
        return
    if not precedents:
        return  # no source at all: the shape rule reports the entries
    cycle = _cycle_through(adr, ctx)
    if cycle:
        v1_issue(adr, f"basis: precedent loops back through ADR-{' -> ADR-'.join(str(n) for n in cycle)} without reaching an external source")
    else:
        v1_issue(adr, f"basis: following precedent never reaches an external source ({', '.join(v1_external_sources(ctx))})")
