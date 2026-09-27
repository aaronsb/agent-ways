# ============================================================================
# adr/v1: legibility, enactment, the vocabulary layers, frozen decisions
# ============================================================================
#
# ADR-304 §5 (enactment), §2 (no word shared across layers), §1 and §12 (a
# decision is frozen once it leaves proposed, and opens with a Summary the
# operator can judge alone).

# L3, the derived product state, is fixed by the contract (ADR-304 §2).
V1_DERIVED_STATES = ('active', 'absent', 'present', 'gone', 'living', 'historical')
V1_ENACTING_VERBS = ('cut', 'retire')

_STEM_SUFFIXES = ('ations', 'ation', 'ments', 'ment', 'ions', 'ion', 'ings', 'ing',
                  'ives', 'ive', 'als', 'al', 'ors', 'or', 'ers', 'er', 'ted', 'ed',
                  'ts', 'es', 't', 'e', 's')

def _stem(word: str) -> str:
    """A crude English stem: strip suffixes repeatedly and collapse a doubled
    final consonant, enough to catch cut/cutting, retire/retirement,
    constrain/constraint and propose/proposal."""
    word = word.lower()
    changed = True
    while changed:
        changed = False
        for suffix in _STEM_SUFFIXES:
            if word.endswith(suffix) and len(word) - len(suffix) >= 3:
                word = word[:-len(suffix)]
                changed = True
                break
    if len(word) >= 4 and word[-1] == word[-2] and word[-1] not in 'aeiou':
        word = word[:-1]
    return word

def _collide(a: str, b: str) -> bool:
    """Same word, same stem, or one stem is a prefix of the other (4+ letters)."""
    if a.lower() == b.lower():
        return True
    sa, sb = _stem(a), _stem(b)
    short, long_ = sorted((sa, sb), key=len)
    return sa == sb or (len(short) >= 4 and long_.startswith(short))

def _find_section(adr, name: str) -> Optional[str]:
    for heading in adr.sections:
        if heading.lower() == name.lower() or heading.lower().startswith(name.lower() + ' '):
            return heading
    return None

# --- adr.yaml ------------------------------------------------------------------

@config_rule(contract=V1)
def rule_v1_vocabulary_layers(ctx):
    """No word or stem appears in two layers (ADR-304 §2), so a later contract
    version cannot reintroduce a verb that collides with a state."""
    lifecycle = set()
    for schema in v1_kinds(ctx).values():
        if isinstance(schema, dict):
            lifecycle.update(v1_lifecycle(schema))
    lifecycle = lifecycle or set(V1_LIFECYCLE)
    layers = {
        'lifecycle (L1)': sorted(lifecycle),
        'verbs (L2)': sorted(v1_verbs(ctx)),
        'derived state (L3)': sorted(V1_DERIVED_STATES),
        'basis sources (L4)': sorted(v1_basis_sources(ctx)),
    }
    names = list(layers)
    for i, a in enumerate(names):
        for b in names[i + 1:]:
            for word_a in layers[a]:
                for word_b in layers[b]:
                    if _collide(word_a, word_b):
                        ctx.config_issues.append(Issue(
                            f"'{word_a}' in {a} and '{word_b}' in {b} share a stem; layers share no word", 'error'))

# --- one record ------------------------------------------------------------------

@file_rule(contract=V1)
def rule_v1_required_sections(adr, ctx):
    schema = v1_kind_schema(adr, ctx)
    if not is_v1_record(adr, ctx) or schema is None:
        return
    for name in _str_list(schema.get('sections')) or []:
        if _find_section(adr, name) is None:
            v1_issue(adr, f"a {v1_record_kind(adr)} record opens with a '## {name}' section")

@file_rule(contract=V1)
def rule_v1_summary_legibility(adr, ctx):
    """ADR-304 §12: the Summary carries probes, a mix of points the agent is
    confident on and points it is not, each labelled, and an inversion.
    Guidance for the writer, so a gap warns."""
    if not is_v1_record(adr, ctx) or v1_record_kind(adr) is None:
        return
    heading = _find_section(adr, 'Summary')
    if heading is None or not adr.frontmatter.get('verb'):
        return  # sections rule reports a missing Summary; specs carry no probes
    text = re.sub(r'\s+', ' ', adr.section_text.get(heading, '').lower())
    missing = []
    low = re.search(r'\bnot confident\b|\blow confidence\b', text)
    high = re.search(r'(?<!not )(?<!less )\bconfident\b|\bhigh confidence\b', text)
    if 'probe' not in text:
        missing.append('probes')
    else:
        if low is None:
            missing.append("a probe labelled 'not confident'")
        if high is None:
            missing.append("a probe labelled 'confident'")
    if 'inversion' not in text:
        missing.append('an inversion')
    if missing:
        v1_issue(adr, f"Summary lacks {', '.join(missing)} (ADR-304 §12)", 'warning')

@file_rule(contract=V1)
def rule_v1_enacted(adr, ctx):
    if not is_v1_record(adr, ctx) or 'enacted' not in adr.frontmatter:
        return
    enacted = adr.frontmatter.get('enacted')
    verb = adr.frontmatter.get('verb')
    if verb not in V1_ENACTING_VERBS:
        v1_issue(adr, f"enacted belongs to a cut or retire decision (this one is '{verb}')")
    elif str(adr.status or '').lower() in ('proposed', 'rejected', 'abandoned'):
        v1_issue(adr, f"enacted marks an accepted decision done; this one is {str(adr.status).lower()}")
    if not isinstance(enacted, str):
        v1_issue(adr, f"enacted: quote the commit hash as a string (YAML read {enacted!r})")
    elif not re.fullmatch(r'[0-9a-f]{7,40}', enacted):
        v1_issue(adr, f"enacted: '{enacted}' is not a commit hash")

# --- frozen decisions, read from git history --------------------------------------

def _git(args: list, cwd: Path) -> Optional[str]:
    try:
        result = subprocess.run(['git', '-c', 'core.quotePath=false', *args], cwd=cwd,
                                capture_output=True, encoding='utf-8', errors='replace', timeout=10)
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return None
    return result.stdout if result.returncode == 0 else None

def _frozen_snapshot(adr) -> Optional[tuple]:
    """(frontmatter, body) of the first committed version that was already
    adr/v1 and past proposed, following renames. None outside git, for an
    untracked file, or when no such version exists. A version that was still
    v0 is never the snapshot: migrating an accepted v0 record to v1 adds the
    v1 fields, and that is the migration, not an edit (ADR-304 §7)."""
    root = get_project_root()
    try:
        rel = adr.path.resolve().relative_to(root.resolve())
    except ValueError:
        return None
    # A record freezes where it lands: history is read from the default
    # branch when there is one, so a decision still in review on a feature
    # branch can be revised. Without a remote default, HEAD's history counts.
    ref = (_git(['rev-parse', '--abbrev-ref', 'origin/HEAD'], root) or '').strip() or 'HEAD'
    # --reverse drops pre-rename history under --follow, so read newest first
    # and reverse here. -z keeps names with spaces or non-ASCII intact.
    log = _git(['log', ref, '--follow', '-z', '--format=commit:%H', '--name-only', '--', str(rel)], root)
    if not log:
        return None
    entries, commit = [], None
    for token in log.split('\0'):
        if token.startswith('commit:'):
            commit = token[len('commit:'):]
        elif commit and token.lstrip('\n'):
            entries.append((commit, token.lstrip('\n')))
            commit = None
    for commit, name in reversed(entries):
        text = _git(['show', f'{commit}:{name}'], root)
        if text is None:
            continue
        past = parse_text(text, adr.path)
        if past.contract == V1 and past.status and str(past.status).lower() != 'proposed':
            return past.frontmatter, past.body
    return None

def _same(a, b) -> bool:
    """Equal, treating a date and its quoted string as the same value."""
    if isinstance(a, (str, int, float, date)) and isinstance(b, (str, int, float, date)):
        return str(a) == str(b)
    return a == b

V1_DEFAULT_MUTABLE = ('status', 'enacted', 'superseded_by', 'considered', 'concern')

@file_rule(contract=V1)
def rule_v1_frozen(adr, ctx):
    """Once a decision leaves proposed, only the kind's mutable_after_accept
    fields may change, and the body grows only by appending (ADR-304 §1, §4)."""
    schema = v1_kind_schema(adr, ctx)
    if not is_v1_record(adr, ctx) or schema is None:
        return
    # Proposed records are not frozen, and archived ones carry the archive
    # banner by design; neither needs the history walk.
    if str(adr.status or '').lower() == 'proposed' or is_archived(adr.path):
        return
    mutable = schema.get('mutable_after_accept', list(V1_DEFAULT_MUTABLE))
    if mutable == 'all':
        return
    mutable = set(_str_list(mutable) or V1_DEFAULT_MUTABLE)
    snapshot = _frozen_snapshot(adr)
    if snapshot is None:
        return
    then, body_then = snapshot
    for key in sorted(set(then) | set(adr.frontmatter)):
        if key in mutable:
            continue
        if not _same(then.get(key), adr.frontmatter.get(key)):
            v1_issue(adr, f"'{key}' changed after the decision left proposed; only {', '.join(sorted(mutable)) or 'no fields'} may change")
    if not adr.body.rstrip().startswith(body_then.rstrip()):
        v1_issue(adr, "body edited after the decision left proposed; a decision grows by appending", 'warning')
