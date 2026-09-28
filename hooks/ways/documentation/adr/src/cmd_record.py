
# --- record edits: consider, set, supersede, enact -----------------------------------
#
# Each command edits frontmatter through FrontmatterEdit, so only the lines of
# the fields it touches change. It refuses what the contract refuses before
# writing, then lints the records it wrote and prints their issues.

def _record_target(ref: str, command: str, v1_only: bool = True):
    """The record ref names, or (None, exit code) after an error."""
    matches = find_by_ref(ref, get_all_adrs(include_archived=True))
    if not matches:
        print(f"Error: ADR not found: {ref}", file=sys.stderr)
        return None, 1
    if len(matches) > 1:
        print(f"Error: '{ref}' matches several records; resolve the duplicate number first:", file=sys.stderr)
        for match in matches:
            print(f"  {relative_path(match.path)}", file=sys.stderr)
        return None, 1
    adr = matches[0]
    if v1_only and (repo_contract() != V1 or adr.contract != V1):
        print(f"Error: ADR-{adr.number} is not an adr/v1 record; `adr {command}` edits v1 fields "
              f"(ADR-304 §7).", file=sys.stderr)
        return None, 1
    return adr, 0

def _open_edit(adr):
    try:
        return FrontmatterEdit(adr.path.read_bytes()), None
    except (ValueError, UnicodeDecodeError, yaml.YAMLError) as e:
        return None, f"ADR-{adr.number}: {e}"

def _record_schema(adr) -> Optional[dict]:
    kinds = get_config().get('kinds')
    schema = kinds.get(adr.frontmatter.get('kind')) if isinstance(kinds, dict) else None
    return schema if isinstance(schema, dict) else None

def _frozen(adr) -> Optional[set]:
    """The fields that may still change on this record, or None when every
    field may: a v0 or proposed record, an archived one, or a kind whose
    mutable_after_accept is 'all'. Mirrors rule_v1_frozen."""
    if adr.contract != V1 or str(adr.status or '').lower() == 'proposed' or is_archived(adr.path):
        return None
    mutable = (_record_schema(adr) or {}).get('mutable_after_accept', list(V1_DEFAULT_MUTABLE))
    if mutable == 'all':
        return None
    return set(_str_list(mutable) or V1_DEFAULT_MUTABLE)

def _frozen_message(key: str, mutable: set) -> str:
    """The message rule_v1_frozen gives for the same field."""
    return f"'{key}' changed after the decision left proposed; only {', '.join(sorted(mutable)) or 'no fields'} may change"

def _frozen_hint(adr) -> str:
    return (f"ADR-{adr.number} is {str(adr.status).lower()}. Record the change as a new decision "
            f"that supersedes or amends it, or pass --force for migration cleanup.")

def _force_warning(adr, keys: list) -> None:
    print(f"Warning: --force wrote frozen {'field' if len(keys) == 1 else 'fields'} "
          f"{', '.join(repr(k) for k in keys)} on ADR-{adr.number}. Lint's frozen check still applies: "
          f"it compares against the record as first accepted on the default branch's history.",
          file=sys.stderr)

def _show_diff(adr, before: bytes, after: bytes) -> None:
    import difflib
    name = str(relative_path(adr.path))
    diff = difflib.unified_diff(before.decode('utf-8').splitlines(True), after.decode('utf-8').splitlines(True),
                                fromfile=f"a/{name}", tofile=f"b/{name}")
    sys.stdout.writelines(line if line.endswith('\n') else line + '\n' for line in diff)

def _write(edits: list, dry_run: bool) -> None:
    """Write each (adr, edit) whose bytes changed, or show the diff."""
    for adr, edit in edits:
        before, after = adr.path.read_bytes(), edit.bytes()
        if before == after:
            continue
        if dry_run:
            _show_diff(adr, before, after)
        else:
            adr.path.write_bytes(after)

def _lint_records(paths: list) -> None:
    """Lint the records just written against the whole corpus and print
    their issues, as `adr lint <path>` would list them."""
    corpus = get_all_adrs(include_archived=True)
    ctx = LintContext.from_corpus(corpus)
    records = [a for a in corpus if a.path in paths]
    run_rules(records, ctx)
    for record in records:
        where = relative_path(record.path)
        if not record.issues:
            print(f"Lint {where}: clean")
            continue
        print(f"Lint {where}:")
        for issue in record.issues:
            print(f"  {'❌' if issue.severity == 'error' else '⚠️'} {issue.message}")

def _finish(edits: list, dry_run: bool, done: str) -> int:
    _write(edits, dry_run)
    if dry_run:
        print(f"Dry run, nothing written: {done}")
        return 0
    print(done)
    _lint_records([adr.path for adr, _ in edits])
    return 0

# --- consider -----------------------------------------------------------------------

_PROBE_NAME = re.compile(r'\*\s*(?:not\s+)?confident\s*\(([^)\n]+)\)\s*:?\s*\*', re.IGNORECASE)

def probe_names(adr) -> list:
    """The named probes in a record's Summary, written *Confident (name):* or
    *Not confident (name):*, then 'inversion' when the Summary has one: a
    considered entry may cover the inversion too (ADR-304 §12)."""
    heading = _find_section(adr, 'Summary')
    text = adr.section_text.get(heading, '') if heading else ''
    names = list(dict.fromkeys(m.strip() for m in _PROBE_NAME.findall(text)))
    if re.search(r'\binversion\b', text, re.IGNORECASE):
        names.append('inversion')
    return names

def cmd_consider(args):
    """Append one considered entry: the operator's answer to the Summary
    (ADR-304 §12). considered is mutable after acceptance, so any status."""
    adr, code = _record_target(args.adr, 'consider')
    if adr is None:
        return code
    for flag, value, what in (('--said', args.said, 'what the operator said, verbatim'),
                              ('--via', args.via, 'the channel it was said in, such as a PR or a session')):
        if value is None or not value.strip():
            print(f"Error: {flag} is required: {what}.", file=sys.stderr)
            return 1
    operator = args.operator or _detect_git_user()
    if not operator:
        print("Error: no operator given and none detected from gh or git; pass --operator NAME.", file=sys.stderr)
        return 1
    if args.covers is not None:
        valid = probe_names(adr)
        unknown = [c for c in args.covers if c not in valid]
        if unknown:
            names = ', '.join(repr(u) for u in unknown)
            if any(v != 'inversion' for v in valid):
                print(f"Error: ADR-{adr.number} has no probe {names}. Names it can cover: {', '.join(valid)}.",
                      file=sys.stderr)
            else:
                also = f" It can cover: {', '.join(valid)}." if valid else ''
                print(f"Error: ADR-{adr.number} has no probe {names}; its Summary names no probes. "
                      f"A probe is named as *Confident (name):* or *Not confident (name):*.{also}",
                      file=sys.stderr)
            return 1
    edit, error = _open_edit(adr)
    if error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    entry = {'operator': operator, 'said': _Quoted(args.said), 'via': _prefer_double(args.via)}
    if args.paraphrase:
        entry['paraphrase'] = True
    if args.covers is not None:
        entry['covers'] = _FlowList(args.covers)
    if args.canary:
        entry['canary'] = args.canary
    try:
        edit.append('considered', [entry])
    except ValueError as e:
        print(f"Error: ADR-{adr.number} not changed: {e}", file=sys.stderr)
        return 1
    count = len(edit.fields.get('considered') or [])
    return _finish([(adr, edit)], args.dry_run, f"Added considered entry {count} to ADR-{adr.number}: {adr.title}")

# --- set ----------------------------------------------------------------------------

_ASSIGNMENT = re.compile(r'^([A-Za-z_][A-Za-z0-9_]*(?:-[A-Za-z0-9_]+)*)(\+=|-=|=)(.*)$', re.DOTALL)
# Statuses with a command of their own, which checks the corpus and records why.
_STATUS_COMMANDS = {'accepted': 'accept', 'rejected': 'reject', 'abandoned': 'abandon',
                    'superseded': 'supersede', 'archived': 'archive'}

def _parse_assignment(text: str):
    match = _ASSIGNMENT.match(text)
    if not match:
        raise ValueError(f"'{text}' is not key=value, key+=value or key-=value")
    key, op, raw = match.groups()
    try:
        value = yaml.safe_load(raw) if raw.strip() else None
    except yaml.YAMLError as e:
        raise ValueError(f"{key}: the value is not YAML ({str(e).splitlines()[0]})")
    return key, op, value

def cmd_set(args):
    """Set, append to or remove from frontmatter fields, refusing a frozen
    field of a record that has left proposed unless --force."""
    adr, code = _record_target(args.adr, 'set', v1_only=False)
    if adr is None:
        return code
    try:
        changes = [_parse_assignment(a) for a in args.assignments]
    except ValueError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1
    mutable = _frozen(adr)
    frozen = list(dict.fromkeys(k for k, _, _ in changes if mutable is not None and k not in mutable))
    if frozen and not args.force:
        for key in frozen:
            print(f"Refused: ADR-{adr.number}: {_frozen_message(key, mutable)}", file=sys.stderr)
        print(_frozen_hint(adr), file=sys.stderr)
        return 1
    if adr.contract == V1 and not args.force:
        for key, op, value in changes:
            if key != 'status':
                continue
            command = _STATUS_COMMANDS.get(str(value).lower()) if op == '=' else None
            if command:
                print(f"Refused: use `adr {command} {adr.number}`; it checks the corpus and records why. "
                      f"--force sets the status anyway.", file=sys.stderr)
            else:
                print(f"Refused: a record's status changes only through accept, reject, abandon, "
                      f"supersede and archive. --force sets the status anyway.", file=sys.stderr)
            return 1
    # A key the record lacks and no v1 record carries is most likely a typo.
    unknown = [k for k, _, _ in changes
               if adr.contract == V1 and k not in adr.frontmatter and k not in V1_KEY_ORDER]
    if unknown and not args.force:
        print(f"Refused: ADR-{adr.number} has no field {', '.join(repr(k) for k in unknown)}, and it is not "
              f"a v1 field ({', '.join(V1_KEY_ORDER)}). --force adds it anyway.", file=sys.stderr)
        return 1
    edit, error = _open_edit(adr)
    if error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    try:
        for key, op, value in changes:
            items = value if isinstance(value, list) else [value]
            if op == '=':
                edit.set(key, value)
            elif op == '+=':
                edit.append(key, items)
            else:
                missing = edit.remove(key, items)
                if missing:
                    print(f"Error: ADR-{adr.number} not changed: {key} does not list "
                          f"{', '.join(repr(str(m)) for m in missing)}.", file=sys.stderr)
                    return 1
    except ValueError as e:
        print(f"Error: ADR-{adr.number} not changed: {e}", file=sys.stderr)
        return 1
    if frozen and not args.dry_run:
        _force_warning(adr, frozen)
    keys = ', '.join(dict.fromkeys(k for k, _, _ in changes))
    return _finish([(adr, edit)], args.dry_run, f"Set {keys} on ADR-{adr.number}: {adr.title}")

# --- supersede ----------------------------------------------------------------------

def _ref(adr, section: Optional[str] = None) -> str:
    return f"ADR-{adr.number}" + (f"#{section}" if section else '')

def _lists(adr, key: str, ref: str) -> bool:
    number, section = norm_ref(ref)
    return any(norm_ref(e) == (number, section) for e in as_entries(adr.frontmatter.get(key)))

def cmd_supersede(args):
    """Write both sides of a supersession, or with --amends a partial
    replacement on the new record only (ADR-304 §3)."""
    old, code = _record_target(args.adr, 'supersede')
    if old is None:
        return code
    new, code = _record_target(args.by, 'supersede')
    if new is None:
        return code
    if old.path == new.path:
        print(f"Error: ADR-{old.number} cannot supersede itself.", file=sys.stderr)
        return 1
    new_status = str(new.status or '').lower()
    if new_status not in ('accepted', 'proposed'):
        print(f"Error: ADR-{new.number} is {new_status or 'without a status'}; only an accepted or proposed "
              f"record supersedes another.", file=sys.stderr)
        return 1
    old_status = str(old.status or '').lower()
    if old_status not in ('accepted', 'superseded'):
        print(f"Error: ADR-{old.number} is {old_status or 'without a status'}; only an accepted record is "
              f"superseded (a proposed one is rejected or abandoned).", file=sys.stderr)
        return 1
    field_name = 'amends' if args.amends else 'supersedes'
    new_kind, old_kind = new.frontmatter.get('kind'), old.frontmatter.get('kind')
    edges = v1_edges(_record_schema(new) or {})
    if old_kind not in edges.get(field_name, []):
        allowed = edges.get(field_name)
        target = f"points at {' or '.join(allowed)} records" if allowed else "is not an edge it takes"
        print(f"Error: a {new_kind} record cannot {'amend' if args.amends else 'supersede'} a {old_kind}: "
              f"for a {new_kind}, {field_name} {target} (adr.yaml kinds.{new_kind}.edges).", file=sys.stderr)
        return 1
    section = args.amends
    if section and not section_exists(old, section):
        print(f"Error: ADR-{old.number} has no section '{section}'. Its sections: {', '.join(old.sections)}.",
              file=sys.stderr)
        return 1
    if not section and 'superseded' not in v1_lifecycle(_record_schema(old)):
        print(f"Error: the {old_kind} lifecycle has no superseded status.", file=sys.stderr)
        return 1
    ref = _ref(old, section)
    edits = []
    mutable = _frozen(new)
    if not _lists(new, field_name, ref):
        if mutable is not None and field_name not in mutable and not args.force:
            print(f"Refused: ADR-{new.number}: {_frozen_message(field_name, mutable)}", file=sys.stderr)
            print(_frozen_hint(new), file=sys.stderr)
            return 1
        edit, error = _open_edit(new)
        if error:
            print(f"Error: {error}", file=sys.stderr)
            return 1
        try:
            edit.append(field_name, [ref])
        except ValueError as e:
            print(f"Error: ADR-{new.number} not changed: {e}", file=sys.stderr)
            return 1
        edits.append((new, edit))
        if mutable is not None and field_name not in mutable and not args.dry_run:
            _force_warning(new, [field_name])
    if not section:
        edit, error = _open_edit(old)
        if error:
            print(f"Error: {error}", file=sys.stderr)
            return 1
        try:
            if not _lists(old, 'superseded_by', _ref(new)):
                edit.append('superseded_by', [_ref(new)])
            if old_status != 'superseded':
                edit.set('status', 'superseded')
        except ValueError as e:
            print(f"Error: ADR-{old.number} not changed: {e}", file=sys.stderr)
            return 1
        edits.append((old, edit))
    if section:
        done = f"ADR-{new.number} amends ADR-{old.number} §{section}"
    else:
        done = f"ADR-{new.number} supersedes ADR-{old.number}; ADR-{old.number} is superseded"
    code = _finish(edits, args.dry_run, done)
    if not args.dry_run and not section:
        print("Run `adr index -y` to refresh INDEX.md.")
    return code

# --- enact --------------------------------------------------------------------------

def cmd_enact(args):
    """Mark an accepted cut or retire decision done at a commit (ADR-304 §5)."""
    adr, code = _record_target(args.adr, 'enact')
    if adr is None:
        return code
    verb = adr.frontmatter.get('verb')
    if verb not in V1_ENACTING_VERBS:
        print(f"Error: ADR-{adr.number} is a{'n' if str(verb)[:1] in 'aeiou' else ''} {verb or 'verbless'} "
              f"record; enacted belongs to a cut or retire decision (ADR-304 §5).", file=sys.stderr)
        return 1
    status = str(adr.status or '').lower()
    if status != 'accepted':
        print(f"Error: ADR-{adr.number} is {status}; enacted marks an accepted decision done.", file=sys.stderr)
        return 1
    commit = args.commit.strip().lower()
    if not re.fullmatch(r'[0-9a-f]{7,40}', commit):
        print(f"Error: '{args.commit}' is not a commit hash (7 to 40 hex digits).", file=sys.stderr)
        return 1
    edit, error = _open_edit(adr)
    if error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    previous = adr.frontmatter.get('enacted')
    try:
        edit.set('enacted', _Quoted(commit))
    except ValueError as e:
        print(f"Error: ADR-{adr.number} not changed: {e}", file=sys.stderr)
        return 1
    note = f" (was {previous})" if previous and str(previous) != commit else ''
    return _finish([(adr, edit)], args.dry_run, f"Enacted ADR-{adr.number} at {commit}{note}: {adr.title}")
