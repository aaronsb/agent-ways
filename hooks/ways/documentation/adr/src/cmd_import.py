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
    A source is only read, never written."""
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
        if dest.exists():
            try:
                previous = (load_sheet(dest).get('source') or {}).get('path')
            except SheetError:
                previous = None
            if previous != source:
                print(f"Skipped: {shown}: {relative_path(dest)} holds a sheet for {previous or 'another source'}")
                skipped += 1
                continue
        try:
            text = dump_sheet(sheet)
        except SheetError as e:
            print(f"Skipped: {shown}: {e}")
            skipped += 1
            continue
        out.mkdir(parents=True, exist_ok=True)
        ignore = out / '.gitignore'
        if not ignore.exists():
            ignore.write_text('*\n')
        dest.write_text(text)
        claimed[name] = source
        written += 1
        todo = len(sheet['todo'])
        print(f"Scanned: {shown} -> {relative_path(dest)} ({sheet['source']['format']}, {todo} todo)")
    print(f"Scan: {written} sheet(s) written, {skipped} skipped")
    return 1 if skipped and not written else 0

def _source_file(source: dict) -> Path:
    path = Path(source['path'])
    return path if path.is_absolute() else get_project_root() / path

def _destination(sheet: dict, source_file: Optional[Path]) -> Path:
    """Where apply writes: over the source when it sits in the repo at the
    path the target implies, otherwise a new file in the domain's folder
    (ADR-306 §3, §5). Raises SheetError when neither works."""
    target = sheet['target']
    number = format_number(target['number'])
    domain = target.get('domain')
    arch = get_project_root() / 'docs' / 'architecture'
    if source_file is not None:
        try:
            source_file.resolve().relative_to(arch.resolve())
            inside = True
        except ValueError:
            inside = False
        if (inside and filename_number(source_file) == number.lstrip('0')
                and _folder_domain(source_file) == domain):
            return source_file
    if not domain:
        raise SheetError('target.domain is empty')
    if domain in get_domains():
        folders = get_domains()[domain]['folder']
        folder = folders[0] if isinstance(folders, list) else folders
    elif domain == 'legacy' and 'legacy' in get_config():
        folder = 'legacy'
    else:
        raise SheetError(f"domain '{domain}' is not in adr.yaml; add it first "
                         f"(creating a domain on apply, ADR-306 §6, is not built yet)")
    for existing in find_adrs(include_archived=True):
        if filename_number(existing) == number.lstrip('0') and (
                source_file is None or existing.resolve() != source_file.resolve()):
            raise SheetError(f"ADR-{number} already exists: {relative_path(existing)}")
    slug = re.sub(r'[^a-z0-9]+', '-', str(target['title']).lower()).strip('-')
    return arch / folder / f"ADR-{number}-{slug}.md"

def _apply_one(sheet: dict) -> tuple:
    """Write one sheet as a v1 record through render_record and return
    (path, whether it changed). A non-v1 source gains `imported: {from,
    format}` (ADR-306 §4). A source changed since the scan is refused."""
    source = sheet.get('source')
    source_file = None
    record = dict(sheet['record'])
    if source:
        source_file = _source_file(source)
        if not source_file.is_file():
            raise SheetError(f"source {source['path']} is gone")
        if hashlib.sha256(source_file.read_bytes()).hexdigest() != source.get('sha256'):
            raise SheetError(f"source {source['path']} changed since the scan; scan it again")
        if source.get('format') != 'v1':
            record.setdefault('imported', {'from': source['path'], 'format': source.get('format')})
    dest = _destination(sheet, source_file)
    try:
        text = render_record(dict(sheet, record=record))
    except (ValueError, TypeError) as e:
        raise SheetError(str(e))
    if dest.is_file() and _same_record(dest.read_text(), text):
        return dest, False
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

def _import_apply(args):
    """Write each finished sheet as a v1 record, then lint what was written
    (ADR-306 §3). A sheet with open todo items is skipped unless --partial.
    An applied sheet is removed: the record is what is kept."""
    if args.sheets:
        paths = [Path(p) for p in args.sheets]
    else:
        paths = sorted(import_dir().glob('ADR-*.yaml')) if import_dir().is_dir() else []
    lines, written = [], []
    applied = skipped = refused = 0
    for path in paths:
        shown = relative_path(path.resolve())
        try:
            sheet = load_sheet(path)
            todo = sheet.get('todo') or []
            if todo and not args.partial:
                lines.append((f"Skipped: {shown}: {len(todo)} open todo item(s): "
                              f"{', '.join(str(t).split(':')[0] for t in todo)}", None))
                skipped += 1
                continue
            dest, changed = _apply_one(sheet)
        except SheetError as e:
            lines.append((f"Refused: {shown}: {e}", None))
            refused += 1
            continue
        path.unlink()
        applied += 1
        written.append(dest.resolve())
        partial = (f", {len(todo)} todo left" if todo else '') + ('' if changed else ', unchanged')
        lines.append((f"Applied: {shown} -> {relative_path(dest)}{partial}", dest.resolve()))
    counts = _lint_counts(written)
    for text, dest in lines:
        if dest is not None:
            errors, warnings = counts.get(dest, (0, 0))
            text += f" (lint: {errors} errors, {warnings} warnings)"
        print(text)
    print(f"Import: {applied} applied, {skipped} skipped, {refused} refused")
    return 1 if refused else 0

def _lint_counts(paths: list) -> dict:
    """(errors, warnings) per written record, from the full rule set."""
    if not paths:
        return {}
    corpus = get_all_adrs(include_archived=True)
    ctx = LintContext.from_corpus(corpus)
    targets = [adr for adr in corpus if adr.path.resolve() in set(paths)]
    run_rules(targets, ctx)
    return {adr.path.resolve(): (sum(i.severity == 'error' for i in adr.issues),
                                 sum(i.severity != 'error' for i in adr.issues))
            for adr in targets}
