# ============================================================================
# adr domain: add, rename, move (ADR-306 §6)
# ============================================================================
#
# The domain layout changes as a corpus grows. A record's number is its
# permanent identity: it is never renumbered and never reused. Under adr/v1 a
# record's folder decides its domain, and a domain's range only allocates the
# numbers of new records. So moving a record moves its file and rewrites every
# path to it, and ADR-N citations stay as they are.
#
# adr.yaml is edited as text, so its comments and layout survive.

DOMAIN_NAME_RE = re.compile(r'[a-z][a-z0-9_-]*')
FOLDER_NAME_RE = re.compile(r'[A-Za-z0-9][\w.-]*')


def cmd_domain(args):
    handlers = {'add': _domain_add, 'rename': _domain_rename, 'move': _domain_move}
    if args.domain_command not in handlers:
        print("usage: adr domain {add,rename,move} ...", file=sys.stderr)
        return 2
    return handlers[args.domain_command](args)


# --- the files a rewrite reaches ----------------------------------------------------

def _relocation_scope(root: Path) -> tuple:
    """(names in scope, every tracked path and directory). Scope is every file
    git tracks except fixtures, import sheets, the archive (archived records
    are never edited), adr.yaml's cite.exclude list, and INDEX.md."""
    import fnmatch
    tracked, known = tracked_paths(root)
    cite_config = get_config().get('cite')
    patterns = ['tests/fixtures', 'docs/architecture/archive'] + [
        str(p) for p in ((cite_config.get('exclude') if isinstance(cite_config, dict) else None) or [])]

    def excluded(name: str) -> bool:
        if '.import' in name.split('/'):
            return True
        for pattern in patterns:
            if any(c in pattern for c in '*?['):
                if fnmatch.fnmatch(name, pattern):
                    return True
            elif name == pattern.rstrip('/') or name.startswith(pattern.rstrip('/') + '/'):
                return True
        return False
    # INDEX.md is regenerated after the move, so it is not rewritten. A
    # symlink is skipped: writing through it would edit its target, which is
    # in scope (or excluded) in its own right. docs/scripts/adr is one.
    return [n for n in tracked if not excluded(n) and n != 'docs/architecture/INDEX.md'
            and not (root / n).is_symlink()], known


def _plan_rewrites(relocation: Relocation, root: Path, names: list) -> list:
    """(name, count, new bytes, [(line number, before, after)]) for each file
    whose text changes. Binary files and files that are not UTF-8 are never
    touched."""
    changes = []
    for name in names:
        path = root / name
        try:
            raw = path.read_bytes()
        except OSError:
            continue
        if b'\0' in raw[:8192]:
            continue
        try:
            text = raw.decode('utf-8')
        except UnicodeDecodeError:
            continue
        new, count = relocation.text(text, name)
        if count and new != text:
            changes.append((name, count, new.encode('utf-8'), _changed_lines(text, new)))
    return changes


def _changed_lines(before: str, after: str) -> list:
    """[(line number, before, after)] for each line the rewrite changed. A
    rewrite changes text within lines, so the lines pair up."""
    return [(i, a, b) for i, (a, b) in enumerate(zip(before.split('\n'), after.split('\n')), 1) if a != b]


def _print_rewrites(changes: list, verb: str, lines: bool = False) -> None:
    """The count of rewritten paths per file, and with lines=True each line
    the rewrite changes, before and after."""
    total = sum(c[1] for c in changes)
    print(f"{verb} {total} path{'s' if total != 1 else ''} in {len(changes)} file{'s' if len(changes) != 1 else ''}")
    for name, count, _, changed in changes:
        print(f"  {name}: {count}")
        if lines:
            for number, before, after in changed:
                print(f"    {name}:{number}")
                print(f"        {before.rstrip(chr(13))}")
                print(f"      → {after.rstrip(chr(13))}")


def _git_mv(root: Path, old: str, new: str) -> None:
    (root / new).parent.mkdir(parents=True, exist_ok=True)
    try:
        result = subprocess.run(['git', 'mv', old, new], cwd=root,
                                capture_output=True, text=True, timeout=30)
        if result.returncode == 0:
            return
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    (root / old).rename(root / new)


def _write_rewrites(relocation: Relocation, root: Path, changes: list) -> None:
    for name, _, data, _ in changes:
        (root / (relocation.target(name) or name)).write_bytes(data)


def _refresh_index(root: Path) -> None:
    """Regenerate INDEX.md when the project keeps one."""
    if (root / 'docs' / 'architecture' / 'INDEX.md').exists():
        cmd_index(argparse.Namespace(yes=True))


# --- adr.yaml as text ---------------------------------------------------------------

def _yaml_scalar(value: str) -> str:
    if value and re.fullmatch(r'[A-Za-z0-9(][\w ,.()/+-]*', value) and value.strip() == value \
            and value.lower() not in ('yes', 'no', 'true', 'false', 'on', 'off', 'null', '~'):
        return value
    return '"' + value.replace('\\', '\\\\').replace('"', '\\"') + '"'


def _domains_block(lines: list) -> Optional[tuple]:
    """(start, end) of the domains block: the `domains:` line and the index
    just past its last indented line. None when there is no block form."""
    start = next((i for i, line in enumerate(lines)
                  if re.fullmatch(r'domains:\s*(#.*)?', line.rstrip('\r\n'))), None)
    if start is None:
        return None
    end = start + 1
    while end < len(lines) and (not lines[end].strip() or lines[end][0] in ' \t'):
        end += 1
    return start, end


def _entry_indent(lines: list, start: int, end: int) -> tuple:
    """(entry indent, field indent) used in the domains block."""
    entry = field_ = None
    for line in lines[start + 1:end]:
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        indent = line[:len(line) - len(line.lstrip())]
        if entry is None:
            entry = indent
        elif len(indent) > len(entry):
            field_ = indent
            break
    entry = entry if entry is not None else '  '
    return entry, field_ if field_ is not None else entry * 2


def _save_config(text: str) -> bool:
    """Write adr.yaml if the edited text still reads as YAML."""
    try:
        data = yaml.safe_load(text)
    except yaml.YAMLError as e:
        print(f"Error: the edited adr.yaml does not parse ({e}); nothing written", file=sys.stderr)
        return False
    if not isinstance(data, dict) or not isinstance(data.get('domains'), dict):
        print("Error: the edited adr.yaml lost its domains; nothing written", file=sys.stderr)
        return False
    get_config_path().write_text(text)
    reload_config()
    return True


def _folders(config: dict) -> list:
    folders = config.get('folder')
    return list(folders) if isinstance(folders, list) else [folders]


# --- add ----------------------------------------------------------------------------

def _domain_add(args):
    name, folder = args.name, args.folder
    domains = get_domains()
    problems = []
    if not DOMAIN_NAME_RE.fullmatch(name) or name in ('legacy', 'archive'):
        problems.append(f"'{name}' is not a usable domain name (lowercase letters, digits, - and _; not legacy or archive)")
    elif name in domains:
        problems.append(f"domain '{name}' already exists")
    m = re.fullmatch(r'\s*(\d+)\s*-\s*(\d+)\s*', args.range or '')
    low = high = None
    if not m:
        problems.append(f"--range '{args.range}' is not A-B")
    else:
        low, high = int(m.group(1)), int(m.group(2))
        if low > high:
            problems.append(f"--range {low}-{high} runs backwards")
    if not FOLDER_NAME_RE.fullmatch(folder) or folder == 'archive':
        problems.append(f"--folder '{folder}' is not a folder name under docs/architecture (and not archive)")
    for other, config in domains.items():
        if folder in _folders(config):
            problems.append(f"folder '{folder}' already belongs to {other}")
    if low is not None and low <= high:
        spans = [(other, config['range']) for other, config in domains.items()]
        if 'legacy' in get_config():
            spans.append(('legacy', get_legacy_range()))
        for other, (a, b) in spans:
            if low <= b and a <= high:
                problems.append(f"range {low}-{high} overlaps {other} ({a}-{b})")
    if problems:
        for problem in problems:
            print(f"Error: {problem}", file=sys.stderr)
        return 1

    text = get_config_path().read_text()
    lines = text.splitlines(keepends=True)
    block = _domains_block(lines)
    if block is None:
        print("Error: adr.yaml has no block-style domains: section to add to; edit it by hand", file=sys.stderr)
        return 1
    start, end = block
    entry, field_ = _entry_indent(lines, start, end)
    last = max((i for i in range(start, end) if lines[i].strip()), default=start)
    if not lines[last].endswith('\n'):
        lines[last] += '\n'
    label = args.label or name.capitalize()
    new = ['\n', f"{entry}{name}:\n",
           f"{field_}range: [{low}, {high}]\n",
           f"{field_}name: {_yaml_scalar(label)}\n",
           f"{field_}description: {_yaml_scalar(args.description or '')}\n",
           f"{field_}folder: {_yaml_scalar(folder)}\n"]
    if last == start:
        new = new[1:]
    lines[last + 1:last + 1] = new
    if not _save_config(''.join(lines)):
        return 1
    print(f"Added domain: {name} ({low}-{high})")
    print(f"  Name: {label}")
    print(f"  Folder: docs/architecture/{folder}/")
    return 0


# --- rename -------------------------------------------------------------------------

def _domain_rename(args):
    old, new = args.old, args.new
    domains = get_domains()
    if old not in domains:
        print(f"Error: Unknown domain '{old}'", file=sys.stderr)
        print(f"Valid domains: {', '.join(domains.keys())}", file=sys.stderr)
        return 1
    config = domains[old]
    folders = _folders(config)
    problems = []
    if new != old:
        if not DOMAIN_NAME_RE.fullmatch(new) or new in ('legacy', 'archive'):
            problems.append(f"'{new}' is not a usable domain name (lowercase letters, digits, - and _; not legacy or archive)")
        elif new in domains:
            problems.append(f"domain '{new}' already exists")
    if args.folder and len(folders) > 1:
        problems.append(f"{old} spans several folders ({', '.join(folders)}); rename them by hand")
    # The folder follows the name when it matched the name; otherwise it
    # stays, unless --folder names one.
    folder_old = folders[0]
    folder_new = args.folder or (new if len(folders) == 1 and folder_old == old else folder_old)
    root = get_project_root()
    arch = root / 'docs' / 'architecture'
    if folder_new != folder_old:
        if not FOLDER_NAME_RE.fullmatch(folder_new) or folder_new == 'archive':
            problems.append(f"folder '{folder_new}' is not a folder name under docs/architecture (and not archive)")
        for other, other_config in domains.items():
            if other != old and folder_new in _folders(other_config):
                problems.append(f"folder '{folder_new}' already belongs to {other}")
        if (arch / folder_new).exists():
            problems.append(f"docs/architecture/{folder_new} already exists")
    if new == old and folder_new == folder_old:
        problems.append("nothing to rename: give a new name or --folder")
    if problems:
        for problem in problems:
            print(f"Error: {problem}", file=sys.stderr)
        return 1

    text = get_config_path().read_text()
    lines = text.splitlines(keepends=True)
    block = _domains_block(lines)
    key_at = None
    if block:
        start, end = block
        entry, _ = _entry_indent(lines, start, end)
        key_re = re.compile(rf'{re.escape(entry)}{re.escape(old)}:(\s*(?:#.*)?)')
        key_at = next((i for i in range(start + 1, end) if key_re.fullmatch(lines[i].rstrip('\r\n'))), None)
    if key_at is None:
        print(f"Error: cannot find the '{old}:' entry in adr.yaml's domains block; edit it by hand", file=sys.stderr)
        return 1
    ending = lines[key_at][len(lines[key_at].rstrip('\r\n')):]
    lines[key_at] = f"{entry}{new}:" + key_re.fullmatch(lines[key_at].rstrip('\r\n')).group(1) + ending
    if folder_new != folder_old:
        stop = next((i for i in range(key_at + 1, end)
                     if lines[i].strip() and len(lines[i]) - len(lines[i].lstrip()) <= len(entry)), end)
        folder_re = re.compile(r'(\s+folder:\s*)(\S+?)(\s*(?:#.*)?)')
        for i in range(key_at + 1, stop):
            fm = folder_re.fullmatch(lines[i].rstrip('\r\n'))
            if fm:
                ending_i = lines[i][len(lines[i].rstrip('\r\n')):]
                lines[i] = fm.group(1) + _yaml_scalar(folder_new) + fm.group(3) + ending_i
                break
        else:
            print(f"Error: cannot find {old}'s folder: line in adr.yaml; edit it by hand", file=sys.stderr)
            return 1

    names, known = _relocation_scope(root)
    old_dir = f"docs/architecture/{folder_old}"
    new_dir = f"docs/architecture/{folder_new}"
    relocation = Relocation(dirs={old_dir: new_dir} if folder_new != folder_old else {},
                            known=known, domain=(old, new) if new != old else None,
                            repos=origin_repos(root))
    changes = _plan_rewrites(relocation, root, names)
    # adr.yaml is written from its edited text, with its own paths rewritten.
    config_rel = 'docs/architecture/adr.yaml'
    config_before = ''.join(lines)
    config_text, config_count = relocation.text(config_before, config_rel)
    changes = [c for c in changes if c[0] != config_rel]
    if config_count:
        changes = sorted(changes + [(config_rel, config_count, config_text.encode('utf-8'),
                                     _changed_lines(config_before, config_text))])

    verb = 'Would rename' if args.dry_run else 'Renamed'
    print(f"{verb} domain: {old} → {new}")
    if folder_new != folder_old:
        print(f"  Folder: {old_dir}/ → {new_dir}/")
    _print_rewrites(changes, 'Would rewrite' if args.dry_run else 'Rewrote', lines=args.dry_run)
    if args.dry_run:
        print("Dry run: nothing written.")
        return 0
    if not _save_config(config_text):
        return 1
    if folder_new != folder_old and (root / old_dir).exists():
        _git_mv(root, old_dir, new_dir)
    _write_rewrites(relocation, root, [c for c in changes if c[0] != config_rel])
    _refresh_index(root)
    return 0


# --- move ---------------------------------------------------------------------------

def _load_plan(path: str) -> Optional[list]:
    """The moves a plan file lists: [{record, domain}, ...]."""
    try:
        data = yaml.safe_load(Path(path).read_text())
    except (OSError, yaml.YAMLError) as e:
        print(f"Error: cannot read plan {path}: {e}", file=sys.stderr)
        return None
    if isinstance(data, dict) and 'moves' in data:
        data = data['moves']
    if not isinstance(data, list) or not data:
        print(f"Error: plan {path}: expected a list of {{record, domain}} entries", file=sys.stderr)
        return None
    moves = []
    for i, entry in enumerate(data, 1):
        if not isinstance(entry, dict) or 'record' not in entry or 'domain' not in entry:
            print(f"Error: plan entry {i}: expected {{record, domain}}", file=sys.stderr)
            return None
        extra = sorted(set(entry) - {'record', 'domain'})
        if extra:
            hint = '; a record keeps its number' if 'number' in extra else ''
            print(f"Error: plan entry {i}: unknown key{'s' if len(extra) > 1 else ''} {', '.join(extra)}{hint}", file=sys.stderr)
            return None
        moves.append((str(entry['record']), str(entry['domain'])))
    return moves


def _domain_move(args):
    if repo_contract() != 'adr/v1':
        print("Error: under adr/v0 a record's number decides its domain, so a record cannot "
              "change domain by folder; declare contract: adr/v1 in adr.yaml first", file=sys.stderr)
        return 1
    if args.plan:
        if args.record or args.domain:
            print("Error: give either <record> <domain> or --plan, not both", file=sys.stderr)
            return 2
        moves = _load_plan(args.plan)
        if moves is None:
            return 1
    else:
        if not args.record or not args.domain:
            print("Error: adr domain move <record> <domain>, or --plan <file.yaml>", file=sys.stderr)
            return 2
        moves = [(args.record, args.domain)]

    root = get_project_root()
    domains = get_domains()
    records = get_all_adrs(include_archived=True)
    problems, files, lines_out, seen = [], {}, [], set()
    for ref, domain in moves:
        matches = find_by_ref(ref, records)
        if not matches:
            problems.append(f"ADR not found: {ref}")
            continue
        if len(matches) > 1:
            problems.append(f"{ref} matches several records: " + ', '.join(str(relative_path(a.path, root)) for a in matches))
            continue
        adr = matches[0]
        rel = relative_path(adr.path, root).as_posix()
        if rel in seen:
            problems.append(f"ADR-{adr.number} is listed twice")
            continue
        seen.add(rel)
        if is_archived(adr.path):
            problems.append(f"ADR-{adr.number} is archived ({rel}); archived records are kept as they are")
            continue
        if domain not in domains:
            problems.append(f"unknown domain '{domain}' (valid: {', '.join(domains.keys())})")
            continue
        folders = _folders(domains[domain])
        if adr.domain == domain and adr.path.parent.name in folders:
            problems.append(f"ADR-{adr.number} is already in {domain} ({rel})")
            continue
        new_rel = f"docs/architecture/{folders[0]}/{adr.path.name}"
        if (root / new_rel).exists():
            problems.append(f"{new_rel} already exists")
            continue
        files[rel] = new_rel
        lines_out.append((adr, adr.domain or 'legacy', domain, rel, new_rel))
    if problems:
        for problem in problems:
            print(f"Error: {problem}", file=sys.stderr)
        return 1

    names, known = _relocation_scope(root)
    relocation = Relocation(files=files, known=known, repos=origin_repos(root))
    changes = _plan_rewrites(relocation, root, names)

    verb = 'Would move' if args.dry_run else 'Moved'
    for adr, was, domain, rel, new_rel in lines_out:
        print(f"{verb}: ADR-{adr.number} {was} → {domain}")
        print(f"  {rel} → {new_rel}")
    _print_rewrites(changes, 'Would rewrite' if args.dry_run else 'Rewrote', lines=args.dry_run)
    if args.dry_run:
        print("Dry run: nothing written.")
        return 0
    for rel, new_rel in files.items():
        _git_mv(root, rel, new_rel)
    _write_rewrites(relocation, root, changes)
    _refresh_index(root)
    return 0
