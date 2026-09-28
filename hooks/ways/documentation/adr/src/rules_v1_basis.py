# ============================================================================
# adr/v1: basis, agent, consideration and concerns (ADR-304 §11, §12)
# ============================================================================
#
# A decision's basis names what it rests on. precedent points at another
# record in the corpus, and must resolve to one; every other source is
# external. Where a chain of precedent leads is for a reader to judge, not
# lint (ADR-311).
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

def v1_basis_sources(ctx) -> tuple:
    """adr.yaml may rename or extend the sources. precedent is the one
    internal source; every other declared source is external."""
    return tuple(_str_list(ctx.config.get('basis_sources')) or V1_BASIS_SOURCES)

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

def _unknown_keys(entry: dict, allowed: tuple) -> list:
    return [k for k in entry if k not in allowed]

# --- adr.yaml ------------------------------------------------------------------

@config_rule(contract=V1)
def rule_v1_basis_config(ctx):
    raw = ctx.config.get('basis_sources')
    if raw is None:
        return
    if _str_list(raw) is None:
        ctx.config_issues.append(Issue("basis_sources: expected a list of names", 'error'))

# --- one record ----------------------------------------------------------------

def _check_evidence_record(adr, ctx, where: str, ref: str) -> None:
    """A basis `evidence: ADR-N` that names a record must resolve to one, and
    under v1 to a kind the decision's basis edge accepts other than decision
    (ADR-309 §2): a record cited as evidence is evidence or a spec."""
    target = ctx.by_number.get(norm_ref(ref)[0])
    if target is None:
        v1_issue(adr, f"{where}: evidence {ref} resolves to no record")
        return
    if not is_v1_record(target, ctx):
        return
    schema = v1_kind_schema(adr, ctx) or {}
    allowed = [k for k in (v1_edges(schema).get('basis') or []) if k != 'decision']
    kind = target.frontmatter.get('kind')
    if allowed and kind not in allowed:
        v1_issue(adr, f"{where}: evidence {ref} is a {kind}; evidence cites a {' or '.join(allowed)} record")

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
            elif source == 'evidence' and re.fullmatch(r'ADR-\d+(\.\d+)?', str(value).strip()):
                _check_evidence_record(adr, ctx, where, str(value).strip())
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

# --- against the corpus ------------------------------------------------------------

def _precedent_values(adr, ctx) -> list:
    """Precedent references of a usable shape; the shape rule reports others."""
    return [value for source, value, _ in basis_entries(adr, ctx)
            if source == 'precedent' and isinstance(value, (str, int)) and not isinstance(value, bool)]

@corpus_rule(contract=V1)
def rule_v1_precedent(adr, ctx):
    """Each precedent resolves to a record of a kind the basis edge accepts."""
    if not is_v1_record(adr, ctx):
        return
    allowed_kinds = v1_edges(v1_kind_schema(adr, ctx) or {}).get('basis')
    for value in _precedent_values(adr, ctx):
        target = ctx.by_number.get(norm_ref(value)[0])
        if target is None:
            v1_issue(adr, f"basis: precedent '{value}' resolves to no known ADR")
        elif is_v1_record(target, ctx) and allowed_kinds is not None \
                and v1_record_kind(target) not in allowed_kinds:
            v1_issue(adr, f"basis: precedent ADR-{target.number} is a {v1_record_kind(target)}, "
                          f"expected {' or '.join(allowed_kinds)}")
