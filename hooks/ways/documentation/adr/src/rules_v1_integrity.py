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

def _stem(word: str) -> str:
    """A crude English stem, enough to catch retire/retired or cut/cutting."""
    word = word.lower()
    for suffix in ('ments', 'ment', 'ions', 'ion', 'ings', 'ing', 'ed', 'es', 'e', 's'):
        if word.endswith(suffix) and len(word) - len(suffix) >= 3:
            return word[:-len(suffix)]
    return word

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
                    if word_a.lower() == word_b.lower() or _stem(word_a) == _stem(word_b):
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
    text = adr.section_text.get(heading, '').lower()
    missing = []
    if 'probe' not in text:
        missing.append('probes')
    else:
        if 'not confident' not in text:
            missing.append("a probe labelled 'not confident'")
        if re.search(r'(?<!not )confident', text) is None:
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
    elif str(adr.status or '').lower() != 'accepted':
        v1_issue(adr, "enacted marks an accepted decision done; this one is not accepted")
    if not re.fullmatch(r'[0-9a-f]{7,40}', str(enacted or '')):
        v1_issue(adr, f"enacted: '{enacted}' is not a commit hash")

# --- frozen decisions, read from git history --------------------------------------

def _git(args: list, cwd: Path) -> Optional[str]:
    try:
        result = subprocess.run(['git', *args], cwd=cwd, capture_output=True, text=True, timeout=10)
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return None
    return result.stdout if result.returncode == 0 else None

def _frozen_snapshot(adr) -> Optional[tuple]:
    """(frontmatter, body) as first committed with a status past proposed,
    following renames. None outside git, untracked, or never frozen."""
    root = get_project_root()
    try:
        rel = adr.path.resolve().relative_to(root.resolve())
    except ValueError:
        return None
    log = _git(['log', '--reverse', '--follow', '--format=%H', '--name-only', '--', str(rel)], root)
    if not log:
        return None
    entries, commit = [], None
    for line in log.splitlines():
        line = line.strip()
        if not line:
            continue
        if re.fullmatch(r'[0-9a-f]{40}', line):
            commit = line
        elif commit:
            entries.append((commit, line))
            commit = None
    for commit, name in entries:
        text = _git(['show', f'{commit}:{name}'], root)
        if text is None:
            continue
        past = parse_text(text, adr.path)
        if past.status and str(past.status).lower() != 'proposed':
            return past.frontmatter, past.body
    return None

@file_rule(contract=V1)
def rule_v1_frozen(adr, ctx):
    """Once a decision leaves proposed, only the kind's mutable_after_accept
    fields may change, and the body grows only by appending (ADR-304 §1, §4)."""
    schema = v1_kind_schema(adr, ctx)
    if not is_v1_record(adr, ctx) or schema is None:
        return
    mutable = schema.get('mutable_after_accept', [])
    if mutable == 'all':
        return
    mutable = set(_str_list(mutable) or [])
    snapshot = _frozen_snapshot(adr)
    if snapshot is None:
        return
    then, body_then = snapshot
    for key in sorted(set(then) | set(adr.frontmatter)):
        if key in mutable:
            continue
        if then.get(key) != adr.frontmatter.get(key):
            v1_issue(adr, f"'{key}' changed after the decision left proposed; only {', '.join(sorted(mutable)) or 'no fields'} may change")
    if not adr.body.rstrip().startswith(body_then.rstrip()):
        v1_issue(adr, "body edited after the decision left proposed; a decision grows by appending", 'warning')
