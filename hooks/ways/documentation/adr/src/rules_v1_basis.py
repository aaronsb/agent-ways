# ============================================================================
# adr/v1: basis, agent, consideration and concerns (ADR-304 §11, §12)
# ============================================================================
#
# A decision's basis names what it rests on. operator, evidence, standard and
# upstream are external. precedent points at another record, and following
# precedent must reach an external basis: a corpus that justifies itself only
# by citing itself can drift anywhere and still look consistent.
#
# The operator basis and `considered` are an audit trail, not a credential.
# Lint checks that `said` and `via` are present. It cannot check that they are
# genuine (ADR-304 §11, "Fabrication risk").

V1_BASIS_SOURCES = ('operator', 'evidence', 'standard', 'upstream', 'precedent')
V1_EXTERNAL_SOURCES = ('operator', 'evidence', 'standard', 'upstream')
V1_OPERATOR_LEVELS = ('authored', 'directed', 'guided')
V1_OPERATOR_KEYS = ('level', 'said', 'via', 'paraphrase')
V1_CANARY = ('caught', 'missed')

def v1_basis_sources(ctx) -> tuple:
    return tuple(_str_list(ctx.config.get('basis_sources')) or V1_BASIS_SOURCES)

def _text(value) -> bool:
    return isinstance(value, str) and value.strip() != ''

def basis_entries(adr) -> list:
    """(source, value, entry) for each well-formed basis entry."""
    out = []
    basis = adr.frontmatter.get('basis')
    for entry in basis if isinstance(basis, list) else []:
        if isinstance(entry, dict):
            sources = [k for k in entry if k in V1_BASIS_SOURCES]
            if len(sources) == 1:
                out.append((sources[0], entry[sources[0]], entry))
    return out

def has_operator_basis(adr) -> bool:
    return any(source == 'operator' for source, _, _ in basis_entries(adr))

# --- one record ----------------------------------------------------------------

@file_rule(contract=V1)
def rule_v1_basis_shape(adr, ctx):
    if not is_v1_record(adr, ctx) or 'basis' not in adr.frontmatter:
        return
    basis = adr.frontmatter.get('basis')
    if not isinstance(basis, list) or not basis:
        v1_issue(adr, "basis: expected a list of entries, each naming one source")
        return
    allowed = v1_basis_sources(ctx)
    for i, entry in enumerate(basis, 1):
        where = f"basis entry {i}"
        if not isinstance(entry, dict):
            v1_issue(adr, f"{where}: expected a mapping such as '- evidence: ...'")
            continue
        sources = [k for k in entry if k in V1_BASIS_SOURCES]
        unknown = [k for k in entry if k not in V1_BASIS_SOURCES and k not in V1_OPERATOR_KEYS]
        if unknown:
            v1_issue(adr, f"{where}: unknown key {', '.join(repr(k) for k in unknown)} (sources: {', '.join(allowed)})")
            continue
        if len(sources) != 1:
            v1_issue(adr, f"{where}: names {len(sources)} sources; each entry names exactly one")
            continue
        source = sources[0]
        if source not in allowed:
            v1_issue(adr, f"{where}: source '{source}' is not in adr.yaml basis_sources")
            continue
        if source != 'operator':
            extra = [k for k in entry if k in V1_OPERATOR_KEYS]
            if extra:
                v1_issue(adr, f"{where}: {', '.join(extra)} belong to an operator entry")
            if not _text(entry[source]) and not isinstance(entry[source], int):
                v1_issue(adr, f"{where}: {source} needs a reference")
            continue
        # operator: who, the level, what was said and via which channel
        if not _text(entry['operator']):
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
            for key in ('operator', 'said', 'via'):
                if not _text(entry.get(key)):
                    v1_issue(adr, f"{where}: records '{key}'")
            covers_list = entry.get('covers', [])
            if _str_list(covers_list) is None:
                v1_issue(adr, f"{where}: covers is a list of probe names")
            if 'canary' in entry and entry['canary'] not in V1_CANARY:
                v1_issue(adr, f"{where}: canary is caught or missed")
    # ADR-304 §12: a decision the operator started waits for their consideration
    if (has_operator_basis(adr) and str(adr.status or '').lower() == 'accepted'
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
            if not isinstance(answer, dict) or not _text(answer.get('said')) or not _text(answer.get('via')):
                v1_issue(adr, f"{where}: an answer records 'said' and 'via'")
        else:
            summary = str(entry.get('said') or '').strip().splitlines()[0][:70] if entry.get('said') else ''
            v1_issue(adr, f"open concern: {summary}", 'warning')

# --- against the corpus ------------------------------------------------------------

def _grounding(adr, ctx, seen: tuple) -> tuple:
    """Follow precedent from adr. Returns (outcome, detail): 'external',
    'v0' (a v0 record in the chain), 'loop', 'dangling' or 'internal'."""
    entries = basis_entries(adr)
    if any(source in V1_EXTERNAL_SOURCES for source, _, _ in entries):
        return 'external', None
    outcome = ('internal', None)
    for source, value, _ in entries:
        if source != 'precedent':
            continue
        target = ctx.by_number.get(norm_ref(value)[0])
        if target is None:
            return 'dangling', str(value)
        if target.number in seen:
            return 'loop', target.number
        if not is_v1_record(target, ctx):
            outcome = ('v0', target.number)
            continue
        if v1_record_kind(target) == 'spec':
            # A spec is grounded through the decision that decided it.
            deciders = [ctx.by_number.get(norm_ref(r)[0])
                        for r in as_entries(target.frontmatter.get('decided_by'))]
            results = [_grounding(d, ctx, seen + (target.number,)) for d in deciders if d]
        else:
            results = [_grounding(target, ctx, seen + (target.number,))]
        for result in results:
            if result[0] == 'external':
                return result
            if result[0] in ('loop', 'dangling'):
                return result
            if result[0] == 'v0':
                outcome = result
    return outcome

@corpus_rule(contract=V1)
def rule_v1_basis_chain(adr, ctx):
    """Following precedent must leave the corpus (ADR-304 §11)."""
    if not is_v1_record(adr, ctx) or not basis_entries(adr):
        return
    outcome, detail = _grounding(adr, ctx, (adr.number,))
    if outcome == 'dangling':
        v1_issue(adr, f"basis: precedent '{detail}' resolves to no known ADR")
    elif outcome == 'loop':
        v1_issue(adr, f"basis: precedent loops back through ADR-{detail}")
    elif outcome == 'internal':
        v1_issue(adr, "basis: following precedent never reaches an external source (operator, evidence, standard or upstream)")
    elif outcome == 'v0':
        v1_issue(adr, f"basis: precedent reaches ADR-{detail}, still v0; treated as external until it is migrated", 'warning')
