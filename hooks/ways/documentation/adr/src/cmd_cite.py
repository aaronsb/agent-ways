def cmd_cite(args):
    """Check ADR citations in the code against the records they cite.

    ADR-304 §6 gives code citations a maintenance loop:

    - an ADR-N that resolves to no record is an error
    - a citation whose records are all out of force (superseded, deprecated,
      rejected, abandoned, archived) warns and names each status and successor
    - a citation of a proposed or draft record prompts acceptance

    A bare ADR-N resolves to its family {N, N.1, ...}. Files come from
    `git ls-files`, so ignored paths are never read. docs/architecture and the
    paths in adr.yaml cite.exclude are skipped (a glob, or a directory prefix
    when the pattern has no glob character), and a line carrying
    `adr-cite-ignore` (or doclint's `doclint-allow-retired`) is skipped.
    """
    import fnmatch

    root = get_project_root().resolve()
    excludes = ['docs/architecture/'] + cite_excludes()

    # Path arguments are resolved against the working directory and must sit
    # inside the repository.
    scopes = []
    for raw in args.paths:
        target = (Path.cwd() / raw).resolve()
        if not target.exists():
            print(f"Error: {raw} does not exist", file=sys.stderr)
            return 2
        try:
            scopes.append(target.relative_to(root).as_posix())
        except ValueError:
            print(f"Error: {raw} is outside the repository ({root})", file=sys.stderr)
            return 2

    records = get_all_adrs(include_archived=True)

    # Lookup tables, built once: full number -> records, base -> family
    by_full, by_base = {}, {}
    for adr in records:
        if not adr.number:
            continue
        key = _cite_key(adr.number)
        by_full.setdefault(key, []).append(adr)
        by_base.setdefault(key.split('.')[0], []).append(adr)

    def family(key: str) -> list:
        return by_full.get(key, []) if '.' in key else by_base.get(key, [])

    def successors(adr) -> str:
        named = ', '.join(f"ADR-{norm_ref(e)[0]}" for e in adr.superseded_by)
        return f"see {named}" if named else 'no successor named'

    def is_proposed(adr) -> bool:
        return str(adr.status or '').lower() in ('proposed', 'draft')

    def excluded(name: str) -> bool:
        for pattern in excludes:
            if any(c in pattern for c in '*?['):
                if fnmatch.fnmatch(name, pattern):
                    return True
            elif name == pattern.rstrip('/') or name.startswith(pattern.rstrip('/') + '/'):
                return True
        return False

    def in_scope(name: str) -> bool:
        return not scopes or any(s in ('', '.') or name == s or name.startswith(s + '/') for s in scopes)

    findings = []  # (severity, location, message)
    for name in _cite_files(root):
        if excluded(name) or not in_scope(name):
            continue
        path = root / name
        if path.suffix not in CITE_EXTS or path.name in ('adr.yaml', 'INDEX.md'):
            continue
        try:
            text = path.read_text(encoding='utf-8', errors='replace')
        except OSError:
            continue
        if 'ADR-' not in text:
            continue
        for ln, line in enumerate(text.split('\n'), 1):
            if 'adr-cite-ignore' in line or 'doclint-allow-retired' in line:
                continue
            for match in CITE_RE.finditer(line):
                key = _cite_key(match.group(1) + (match.group(2) or ''))
                cited = match.group(0)  # as written, e.g. ADR-005
                where = f"{name}:{ln}"
                members = family(key)
                if not members:
                    findings.append(('error', where, f"{cited} resolves to no record"))
                    continue
                if all(is_non_active(m.status) for m in members):
                    named = '; '.join(f"ADR-{m.number} is {str(m.status).lower()}, {successors(m)}" for m in members)
                    findings.append(('warning', where, f"{cited} is not in force ({named})"))
                elif any(is_proposed(m) for m in members):
                    findings.append(('warning', where, f"{cited} is still proposed; accept it or cite the record in force"))

    errors = sum(1 for f in findings if f[0] == 'error')
    warnings = len(findings) - errors
    for severity, where, message in findings:
        icon = '❌' if severity == 'error' else '⚠️'
        print(f"  {icon} {where}  {message}")
    print(f"\n{'═'*60}")
    print(f"Citations: {errors} errors, {warnings} warnings")
    print(f"{'═'*60}")
    if args.check and errors:
        return 1
    return 0

# Files and patterns for adr cite (the extensions doclint's retired scan reads)
CITE_EXTS = {".md", ".py", ".ts", ".tsx", ".js", ".mjs", ".rs", ".sh", ".yml", ".yaml", ".json", ".go", ".sql"}
CITE_RE = re.compile(r"\bADR-0*(\d+)(\.\d+)?\b")
CITE_SKIP_DIRS = {'.git', 'node_modules', '.venv', 'venv', 'dist', 'build', 'target', '__pycache__'}

def _cite_key(number: str) -> str:
    """ADR-005 -> '5', ADR-101.01 -> '101.1'."""
    base, _, part = str(number).partition('.')
    base = base.lstrip('0') or '0'
    return f"{base}.{part.lstrip('0') or '0'}" if part else base

def _cite_files(root: Path) -> list:
    """Repository files, sorted: git's view when available (tracked and
    untracked but not ignored), else a walk that prunes dependency and build
    directories."""
    out = _git(['ls-files', '--cached', '--others', '--exclude-standard', '-z'], root)
    if out is not None:
        return sorted(n for n in out.split('\0') if n)
    names = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in CITE_SKIP_DIRS]
        for filename in filenames:
            names.append((Path(dirpath) / filename).relative_to(root).as_posix())
    return sorted(names)
