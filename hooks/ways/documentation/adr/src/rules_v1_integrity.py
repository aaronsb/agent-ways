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
    """The heading that is this section: the name alone, or the name followed
    by punctuation ("Summary: …", "Summary (draft)"). "Summary Nudge" is a
    different section."""
    pattern = re.compile(rf'{re.escape(name)}(\s*[:(].*|\s+[\u2014\u2013-].*)?', re.IGNORECASE)
    for heading in adr.sections:
        if pattern.fullmatch(heading.strip()):
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
    imported = 'imported' in adr.frontmatter
    for name in _str_list(schema.get('sections')) or []:
        if _find_section(adr, name) is None:
            if imported and name == 'Summary':
                # ADR-306 §4: an imported record may gain its Summary later,
                # once the imported corpus has been read together.
                v1_issue(adr, "imported record has no '## Summary' yet (ADR-306 §4)", 'warning')
            else:
                v1_issue(adr, f"a {v1_record_kind(adr)} record opens with a '## {name}' section")

@file_rule(contract=V1)
def rule_v1_imported(adr, ctx):
    """`imported` records where the record came from: {from, format}, and
    optionally `status`, the source status as written, and `unmapped`, the
    source keys with no v1 field (ADR-306 §1, §4, §7)."""
    if not is_v1_record(adr, ctx) or 'imported' not in adr.frontmatter:
        return
    imported = adr.frontmatter.get('imported')
    if not isinstance(imported, dict) or not all(
            isinstance(imported.get(k), str) and imported.get(k).strip() for k in ('from', 'format')):
        v1_issue(adr, "imported: expected {from: <source path>, format: <reader>}")
    elif 'unmapped' in imported and not isinstance(imported['unmapped'], dict):
        v1_issue(adr, "imported.unmapped: expected a mapping of source fields")
    elif isinstance(imported.get('status'), (dict, list)):
        v1_issue(adr, "imported.status: expected the source's status as written")

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

def _file_history(ref: str, rel: str, root: Path) -> list:
    """[(commit, path)] for each commit on ref's first-parent line that
    changed the file at rel, newest first, following renames. git's --follow
    also follows copies, so a new file copied from a record would inherit
    that record's history; the renames are followed here instead, one
    `git diff -M` at each commit that added the file under its later name."""
    entries, seen = [], set()
    while (ref, rel) not in seen:
        seen.add((ref, rel))
        # -m lists a merge's files against its first parent on older git.
        # -z keeps names with spaces or non-ASCII intact.
        log = _git(['log', ref, '--first-parent', '-m', '--no-renames', '-z',
                    '--format=%x01%H', '--name-status', '--', rel], root)
        added = None
        for chunk in (log or '').split('\x01'):
            tokens = [t.lstrip('\n') for t in chunk.split('\0')]
            if not tokens[0]:
                continue
            commit, status = tokens[0], tokens[1] if len(tokens) > 1 else ''
            if status.startswith('D'):
                continue  # deleted here; an earlier commit holds the file
            entries.append((commit, rel))
            if status.startswith('A'):
                added = commit
                break
        if added is None:
            break
        renamed = _git(['diff', '-M', '--diff-filter=R', '--name-status', '-z',
                        f'{added}^', added], root)
        old = next((a for a, b in _renames(renamed) if b == rel), None)
        if old is None:
            break
        ref, rel = f'{added}^', old
    return entries

def _frozen_versions(adr, every: bool = False) -> list:
    """(frontmatter, body, path) of the first committed version that was
    already adr/v1 and past proposed, following renames, and with every=True
    each committed version after it, oldest first. path is where the file was
    in that version. Empty outside git, for an untracked file, or when no
    such version exists. A version that was still v0 is never the snapshot:
    migrating an accepted v0 record to v1 adds the v1 fields, and that is the
    migration, not an edit (ADR-304 §7)."""
    root = get_project_root()
    try:
        rel = adr.path.resolve().relative_to(root.resolve())
    except ValueError:
        return None
    # A record freezes where it lands: history is read from the default
    # branch when there is one, so a decision still in review on a feature
    # branch can be revised. Without a remote default, HEAD's history counts.
    # --first-parent keeps to the branch's own line, so a pull request merged
    # with a merge commit lands at the merge, not at the first commit on its
    # branch; -m lists the merge's files against that parent on older git.
    ref = (_git(['rev-parse', '--abbrev-ref', 'origin/HEAD'], root) or '').strip() or 'HEAD'
    versions = []
    for commit, name in reversed(_file_history(ref, rel.as_posix(), root)):
        text = _git(['show', f'{commit}:{name}'], root)
        if text is None:
            continue
        past = parse_text(text, adr.path)
        if versions or (past.contract == V1 and past.status and str(past.status).lower() != 'proposed'):
            versions.append((past.frontmatter, past.body, name))
            if not every:
                break
    return versions

def _unfilled(value) -> bool:
    return value in (None, '', [], {})

def _completes(then, now) -> bool:
    """now fills what was empty in then and changes nothing else. On an
    imported record that is finishing the import, not an edit: a sheet applied
    with --partial is committed with empty fields (ADR-306 §3, §4)."""
    if _unfilled(then):
        return True
    if isinstance(then, dict) and isinstance(now, dict):
        return all(_unfilled(then.get(k)) or _same(then.get(k), now.get(k))
                   for k in set(then) | set(now))
    return False

def _without_opening_summary(body: str, body_then: str) -> str:
    """body with a ## Summary inserted right after the H1 taken out, when
    the rest of it still starts with what followed the H1 in body_then.
    Otherwise body unchanged."""
    title = re.match(r'\s*# ADR-[^\n]*\n', body)
    title_then = re.match(r'\s*# ADR-[^\n]*\n', body_then)
    if not title or not title_then:
        return body
    after, after_then = body[title.end():], body_then[title_then.end():].strip('\n')
    if not re.match(r'\n*## Summary[ \t]*\n', after):
        return body
    at = after.find(after_then) if after_then else len(after)
    inserted = after[:at]
    if at <= 0 or re.search(r'(?m)^## (?!Summary[ \t]*$)', inserted):
        return body
    return body[:title.end()] + '\n' + after[at:]

def _same(a, b) -> bool:
    """Equal, treating a date and its quoted string as the same value."""
    if isinstance(a, (str, int, float, date)) and isinstance(b, (str, int, float, date)):
        return str(a) == str(b)
    return a == b

V1_DEFAULT_MUTABLE = ('status', 'enacted', 'superseded_by', 'considered', 'concern', 'observable')

@file_rule(contract=V1)
def rule_v1_frozen(adr, ctx):
    """Once a decision leaves proposed, only the kind's mutable_after_accept
    fields may change, and the body grows only by appending (ADR-304 §1, §4).

    An imported record may fill each field its import wrote empty, once: the
    first committed value is then frozen like any other. A field absent at
    import gets no allowance. It may also gain an opening Summary when the
    import had none (ADR-306 §4), and that Summary stays editable: the
    operator expects Summaries to change once the whole corpus is read.
    `imported` is self-declared, so a record that adds it by hand gets the
    same allowance; git history still shows who added it. A path rewritten
    from where a record was to where it is now is not an edit."""
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
    versions = _frozen_versions(adr, every=True)
    if not versions:
        return
    then, body_then, then_path = versions[0]
    imported = 'imported' in then and 'imported' in adr.frontmatter
    # A record, or a record it names by path, may have moved folder since
    # it was accepted, and its number did not change (ADR-306 §6). The
    # accepted version with those paths rewritten, as `adr domain` rewrites
    # them, stands for it too; any other difference is an edit.
    relocation = history_relocation(get_project_root())
    for key in sorted(set(then) | set(adr.frontmatter)):
        if key in mutable:
            continue
        frozen = then.get(key)
        fillable = imported and key in then
        if fillable:
            # Fill once: each committed fill of an empty part becomes frozen.
            for later, _, _ in versions[1:]:
                if _completes(frozen, later.get(key)):
                    frozen = later.get(key)
        now = adr.frontmatter.get(key)
        if _same(frozen, now) or (fillable and _completes(frozen, now)):
            continue
        if key not in RELOCATE_HISTORY_KEYS and _same(relocation.value(frozen, then_path), now):
            continue
        hint = (f" (to allow it, add observable to kinds.{v1_record_kind(adr)}.mutable_after_accept; ADR-307 §2)"
                if key == 'observable' else '')
        v1_issue(adr, f"'{key}' changed after the decision left proposed; only {', '.join(sorted(mutable)) or 'no fields'} may change{hint}")
    relocated = relocation.text(body_then, then_path)[0]
    if not any(_grows_from(adr.body, body, imported) for body in (body_then, relocated)):
        v1_issue(adr, "body edited after the decision left proposed; a decision grows by appending", 'warning')

def _grows_from(body: str, body_then: str, imported: bool) -> bool:
    """body is body_then with text appended. An imported record accepted
    without a Summary may also have gained an opening one."""
    if imported and not re.search(r'(?m)^## Summary[ \t]*$', body_then):
        body = _without_opening_summary(body, body_then)
    return body.rstrip().startswith(body_then.rstrip())

@file_rule(contract=V1)
def rule_v1_no_placeholders(adr, ctx):
    """A record still holding a prompt from `adr new`'s skeleton is unfinished:
    a warning while proposed, an error once it has left proposed. The prompts
    live in sheet.py, which assembles after this module."""
    if not is_v1_record(adr, ctx):
        return
    left = placeholder_lines(adr.body)
    if not left:
        return
    level = 'warning' if str(adr.status or '').lower() == 'proposed' else 'error'
    v1_issue(adr, f"{len(left)} placeholder line(s) from `adr new` still to fill, first: {left[0]}", level)

@file_rule(contract=V1)
def rule_v1_observable(adr, ctx):
    """What should be observable when a decision holds (ADR-307): optional, a
    list whose entries are plain words or mappings with keys the author chooses.
    The check applies to any v1 kind that carries the field."""
    if not is_v1_record(adr, ctx) or 'observable' not in adr.frontmatter:
        return
    entries = adr.frontmatter.get('observable')
    if entries is None or entries == []:
        v1_issue(adr, "observable: empty; remove the key or add an entry")
        return
    if not isinstance(entries, list):
        v1_issue(adr, "observable: expected a list of entries, each a line of words or a mapping")
        return
    for i, entry in enumerate(entries, 1):
        if isinstance(entry, str) and entry.strip():
            continue
        if isinstance(entry, dict) and entry:
            continue
        v1_issue(adr, f"observable entry {i}: expected a line of words or a non-empty mapping")
