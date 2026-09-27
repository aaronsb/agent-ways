def cmd_import(args):
    """adr import scan | apply (ADR-306 §3)."""
    if args.import_command == 'scan':
        return _import_scan(args)
    if args.import_command == 'apply':
        return _import_apply(args)
    print("Usage: adr import scan <paths...> | adr import apply [sheets...] [--partial]", file=sys.stderr)
    return 2

def _scan_inputs(paths: list) -> tuple:
    """(record files, messages). A directory yields its ADR-*.md files,
    leaving out the archive and the sheet directory; an archived record is
    scanned when it is named."""
    files, messages, archived = [], [], 0
    for arg in paths:
        path = Path(arg)
        if path.is_dir():
            for found in sorted(path.rglob('ADR-*.md')):
                parts = found.relative_to(path).parts
                if '.import' in parts:
                    continue
                if 'archive' in parts:
                    archived += 1
                    continue
                files.append(found)
        elif path.is_file():
            files.append(path)
        else:
            messages.append(f"Skipped: {arg}: no such file or directory")
    if archived:
        messages.append(f"Note: {archived} archived record(s) left out; name a file to scan it")
    return files, messages

def _import_scan(args):
    """Write one sheet per record to docs/architecture/.import/ (ADR-306 §1,
    §3). The directory ignores itself, so the repo's .gitignore is untouched.
    A source is only read, never written. A sheet that differs from what a
    fresh scan writes may hold edits, so it is kept unless --force."""
    files, messages = _scan_inputs(args.paths)
    for message in messages:
        print(message)
    out = import_dir()
    written = skipped = 0
    claimed = {}
    for path in files:
        shown = relative_path(path.resolve()) if path.is_absolute() else path
        try:
            sheet = read_record(path)
            text = dump_sheet(sheet)
        except (SheetError, OSError) as e:
            print(f"Skipped: {shown}: {e}")
            skipped += 1
            continue
        name = sheet_filename(sheet['target']['number'])
        dest = out / name
        source = sheet['source']['path']
        if name in claimed:
            print(f"Skipped: {shown}: ADR-{format_number(sheet['target']['number'])} is also {claimed[name]}")
            skipped += 1
            continue
        forced = ''
        if dest.exists() and dest.read_text() != text:
            try:
                previous = (load_sheet(dest).get('source') or {}).get('path')
            except SheetError:
                previous = None
            if previous != source:
                problem = f"{relative_path(dest)} holds a sheet for {previous or 'another source'}"
            else:
                problem = f"{relative_path(dest)} differs from a fresh scan (edited, or the source changed)"
            if not args.force:
                print(f"Skipped: {shown}: {problem}; --force overwrites it and discards its edits")
                skipped += 1
                continue
            forced = '; replaced an edited sheet, its edits are gone'
        out.mkdir(parents=True, exist_ok=True)
        ignore = out / '.gitignore'
        if not ignore.exists():
            ignore.write_text('*\n')
        dest.write_text(text)
        claimed[name] = source
        written += 1
        todo = len(sheet['todo'])
        print(f"Scanned: {shown} -> {relative_path(dest)} ({sheet['source']['format']}, {todo} todo{forced})")
    print(f"Scan: {written} sheet(s) written, {skipped} skipped")
    return 1 if skipped and not written else 0

def _source_file(source: dict) -> Path:
    path = Path(source['path'])
    return path if path.is_absolute() else get_project_root() / path

def _in_tree(path: Path) -> bool:
    arch = get_project_root() / 'docs' / 'architecture'
    try:
        path.resolve().relative_to(arch.resolve())
        return True
    except ValueError:
        return False

def _destination(sheet: dict, source_file: Optional[Path]) -> Path:
    """Where apply writes. A source under docs/architecture is written in
    place and keeps its number and domain: an import never renumbers (§5),
    and moving a record is `adr domain move` (§6). Any other source becomes
    a new file in the domain's folder. The number must sit in the domain's
    range either way. Raises SheetError when none of this holds."""
    target = sheet['target']
    number = format_number(target['number'])
    domain = target.get('domain')
    in_tree = source_file is not None and _in_tree(source_file)
    if in_tree:
        was = filename_number(source_file)
        was_domain = source_domain(source_file, was or number)
        if was != number.lstrip('0') or was_domain != domain:
            raise SheetError(
                f"the source is ADR-{was} in {was_domain}; the sheet says ADR-{number} in {domain}. "
                f"Moving a record to another number or domain is `adr domain move` "
                f"(ADR-306 §6), not built yet; restore target.number and target.domain")
    if not domain:
        raise SheetError('target.domain is empty')
    span = number_range(domain)
    if span is None:
        raise SheetError(f"domain '{domain}' is not in adr.yaml; add it first "
                         f"(creating a domain on apply, ADR-306 §6, is not built yet)")
    if not span[0] <= int(number.split('.')[0]) <= span[1]:
        raise SheetError(f"ADR-{number} is outside the {domain} range {span[0]}-{span[1]}")
    if in_tree:
        return source_file
    for existing in find_adrs(include_archived=True):
        if filename_number(existing) == number.lstrip('0'):
            raise SheetError(f"ADR-{number} already exists: {relative_path(existing)}")
    folders = get_domains()[domain]['folder'] if domain in get_domains() else 'legacy'
    folder = folders[0] if isinstance(folders, list) else folders
    slug = re.sub(r'[^a-z0-9]+', '-', str(target['title']).lower()).strip('-')
    return get_project_root() / 'docs' / 'architecture' / folder / f"ADR-{number}-{slug}.md"

def _imported(source: dict, sheet: dict, raw: bytes) -> dict:
    """`imported` for a record from a non-v1 source (ADR-306 §4): where it
    came from, the source's own status as written, and the source keys with
    no v1 field. The record keeps them whatever happens to the todo items.
    A source outside the repo is named by its file name, so no machine's
    path lands in the record."""
    path = source['path']
    imported = {'from': Path(path).name if Path(path).is_absolute() else path,
                'format': source.get('format')}
    front = split_record(raw.decode('utf-8'))[0]
    data = yaml.safe_load(front or '') or {}
    imported['status'] = data.get('status') if isinstance(data, dict) else None
    if sheet.get('unmapped'):
        imported['unmapped'] = dict(sheet['unmapped'])
    return imported

def _uncommitted(path: Path) -> bool:
    """The file has changes git has not committed. False outside git."""
    root = get_project_root()
    try:
        rel = str(path.resolve().relative_to(root.resolve()))
    except ValueError:
        return False
    return bool((_git(['status', '--porcelain', '--', rel], root) or '').strip())

def _apply_one(sheet: dict, force: bool, undo: Optional[dict] = None) -> tuple:
    """Write one sheet as a v1 record through render_record and return
    (path, whether it changed). A non-v1 source gains `imported: {from,
    format}`, plus `unmapped` when the source had keys with no v1 field, so
    the record keeps them whatever happens to the todo (ADR-306 §1, §4).
    Refused: a source changed since the scan, a source with a body whose
    sheet has none, and a source with uncommitted changes unless --force.
    With `undo`, the destination's prior bytes (None when it did not exist)
    are kept there so a dry run can put them back."""
    source = sheet.get('source')
    source_file = None
    record = dict(sheet['record'])
    if source:
        source_file = _source_file(source)
        if not source_file.is_file():
            raise SheetError(f"source {source['path']} is gone")
        raw = source_file.read_bytes()
        if hashlib.sha256(raw).hexdigest() != source.get('sha256'):
            raise SheetError(f"source {source['path']} changed since the scan; scan it again")
        if source.get('format') != 'v1' and 'imported' not in record:
            record['imported'] = _imported(source, sheet, raw)
        if read_record(source_file)['body'].strip() and not (sheet.get('body') or '').strip():
            raise SheetError('the source has a body and the sheet has none; scan it again')
    dest = _destination(sheet, source_file)
    text = render_record(dict(sheet, record=record))
    if dest.is_file() and _same_record(dest.read_text(), text):
        return dest, False
    if dest.is_file() and not force and _uncommitted(dest):
        raise SheetError(f"{relative_path(dest)} has uncommitted changes; commit them, "
                         f"or --force to overwrite")
    if undo is not None and dest not in undo:
        undo[dest] = dest.read_bytes() if dest.is_file() else None
        made = undo.setdefault('__dirs__', [])
        folder = dest.parent
        while not folder.exists():
            made.append(folder)
            folder = folder.parent
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(text)
    return dest, True

def _same_record(old: str, new: str) -> bool:
    """The same frontmatter data and the same text after it. A v1 record
    applied unedited is left as its author formatted it (ADR-306 §7)."""
    old_front, old_before, old_title, old_after = split_record(old)
    new_front, new_before, new_title, new_after = split_record(new)
    if old_front is None or old_title is None or new_title is None:
        return False
    try:
        same_data = yaml.safe_load(old_front) == yaml.safe_load(new_front)
    except yaml.YAMLError:
        return False
    return (same_data and old_before.strip() == new_before.strip()
            and old_title.group(0) == new_title.group(0) and old_after == new_after)

def _labels(todo: list) -> str:
    """Todo items by label, counted: 'verb, basis, unmapped (2)'."""
    counts = {}
    for item in todo:
        counts[todo_label(item)] = counts.get(todo_label(item), 0) + 1
    return ', '.join(f"{label} ({n})" if n > 1 else label for label, n in counts.items())

def _import_apply(args):
    """Write each finished sheet as a v1 record, then lint what was written
    (ADR-306 §3). A sheet with open todo items is skipped. --partial writes
    it anyway, unless an item is one lint could not find again afterwards
    (BLOCKING_TODO). An applied sheet is removed: the record is what is
    kept. One bad sheet is reported and the rest still apply. --dry-run
    writes the records, lints them inside the corpus, prints each issue,
    then restores every file it wrote and keeps every sheet."""
    if args.sheets:
        paths = [Path(p) for p in args.sheets]
    else:
        paths = sorted(import_dir().glob('ADR-*.yaml')) if import_dir().is_dir() else []
    lines, written = [], []
    undo = {} if args.dry_run else None
    applied = skipped = refused = 0
    counts, issues = {}, {}
    try:
        for path in paths:
            shown = relative_path(path.resolve())
            try:
                sheet = load_sheet(path)
                todo = sheet.get('todo') or []
                held = blocking_todo(todo) if args.partial else todo
                if held:
                    why = "todo item(s) --partial does not write past" if args.partial else 'open todo item(s)'
                    lines.append((f"Skipped: {shown}: {len(held)} {why}: {_labels(held)}", None))
                    skipped += 1
                    continue
                dest, changed = _apply_one(sheet, args.force, undo)
            except (SheetError, OSError, ValueError, TypeError, AttributeError, KeyError) as e:
                lines.append((f"Refused: {shown}: {e}", None))
                refused += 1
                continue
            if not args.dry_run:
                try:
                    path.unlink()
                except OSError as e:
                    lines.append((f"Note: {shown}: applied, but the sheet could not be removed: {e}", None))
            applied += 1
            written.append(dest.resolve())
            partial = (f", {len(todo)} todo left" if todo else '') + ('' if changed else ', unchanged')
            lines.append((f"Applied: {shown} -> {relative_path(dest)}{partial}", dest.resolve()))
        counts, issues = _lint_counts(written)
    finally:
        if undo is not None:
            for failed in _restore(undo):
                lines.append((f"Note: dry run could not restore {failed}", None))
    for text, dest in lines:
        if dest is not None:
            errors, warnings = counts.get(dest, (0, 0))
            text += f" (lint: {errors} errors, {warnings} warnings)"
            if args.dry_run:
                text = text.replace('Applied: ', 'Would apply: ', 1)
                text += ''.join(f"\n    {'error' if i.severity == 'error' else 'warning'}: {i.message}"
                                for i in issues.get(dest, []))
        print(text)
    verb = 'would apply' if args.dry_run else 'applied'
    print(f"Import: {applied} {verb}, {skipped} skipped, {refused} refused"
          + (' (dry run: nothing written)' if args.dry_run else ''))
    lint_errors = sum(e for e, _ in counts.values())
    return 1 if refused or (args.dry_run and lint_errors) else 0

def _restore(undo: dict) -> list:
    """Put back every file a dry run wrote, and remove the directories it
    made that are left empty. Each file is restored on its own, so one
    failure doesn't stop the rest; the paths that failed are returned."""
    failed = []
    for dest, before in undo.items():
        if dest == '__dirs__':
            continue
        try:
            if before is None:
                dest.unlink(missing_ok=True)
            else:
                dest.write_bytes(before)
        except OSError:
            failed.append(relative_path(dest))
    for folder in sorted(undo.get('__dirs__', []), key=lambda d: len(d.parts), reverse=True):
        try:
            folder.rmdir()
        except OSError:
            pass
    return failed

def _lint_counts(paths: list) -> dict:
    """(errors, warnings) per written record, and the issues themselves,
    from the full rule set."""
    if not paths:
        return {}, {}
    corpus = get_all_adrs(include_archived=True)
    ctx = LintContext.from_corpus(corpus)
    targets = [adr for adr in corpus if adr.path.resolve() in set(paths)]
    run_rules(targets, ctx)
    counts = {adr.path.resolve(): (sum(i.severity == 'error' for i in adr.issues),
                                   sum(i.severity != 'error' for i in adr.issues))
              for adr in targets}
    return counts, {adr.path.resolve(): list(adr.issues) for adr in targets}
