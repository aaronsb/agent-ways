def cmd_cite(args):
    """Check ADR citations in the code: that each resolves to a record in
    force, and that each sits where the placement rule puts it.

    Resolution:

    - an ADR-N that resolves to no record is an error
    - a citation whose records are all out of force (superseded, deprecated,
      rejected, abandoned, archived) warns and names each status and successor
    - a citation of a proposed or draft record prompts acceptance

    A bare ADR-N resolves to its family {N, N.1, ...}.

    Placement: a comment or page cites in a references block at its end, and
    text that ships with the built program cites nothing (see the citation
    placement section).
    adr.yaml's cite.placement sets the strength: error, warn (the default) or
    off. With --since, placement findings cover only the lines added or
    changed since the merge base of the ref and HEAD, as errors. A ref git
    cannot resolve, or one with no merge base, is a usage error (exit 2):
    the check never falls back to the whole tree on its own.

    Files come from `git ls-files`, so ignored paths are never read, and a
    symlink is read only at its target. A file with no extension is read when
    its shebang names a shell or Python.
    docs/architecture and the paths in adr.yaml cite.exclude are skipped (a
    glob, or a directory prefix when the pattern has no glob character), and
    a line carrying `adr-cite-ignore` (or doclint's `doclint-allow-retired`)
    is skipped.
    """
    root = get_project_root().resolve()
    excludes = ['docs/architecture/'] + cite_excludes()
    shipped_paths = cite_shipped()
    placement = cite_placement()

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

    changed = None
    if args.since:
        changed, problem = _cite_changed_lines(root, args.since)
        if problem:
            print(f"Error: {problem}", file=sys.stderr)
            return 2
        if placement == 'off':
            changed = None
    place_severity = 'error' if placement == 'error' or changed is not None else 'warning'

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

    def resolution(cited: str):
        """(severity, message) for a citation that does not resolve to a
        record in force, else None."""
        members = family(_cite_key(cited[len('ADR-'):]))
        if not members:
            return 'error', f"{cited} resolves to no record"
        if all(is_non_active(m.status) for m in members):
            named = '; '.join(f"ADR-{m.number} is {str(m.status).lower()}, {successors(m)}" for m in members)
            return 'warning', f"{cited} is not in force ({named})"
        if any(is_proposed(m) for m in members):
            return 'warning', f"{cited} is still proposed; accept it or cite the record in force"
        return None

    def in_scope(name: str) -> bool:
        return not scopes or any(s in ('', '.') or name == s or name.startswith(s + '/') for s in scopes)

    found = []  # (severity, location, message)
    placed = []
    for name in _cite_files(root):
        if path_matches(name, excludes) or not in_scope(name):
            continue
        path = root / name
        if (path.suffix and path.suffix not in CITE_EXTS) or path.name in ('adr.yaml', 'INDEX.md'):
            continue
        # A symlink is read at its target, which the tree lists on its own
        # when it sits inside the repository.
        if path.is_symlink():
            continue
        try:
            text = path.read_text(encoding='utf-8', errors='replace')
        except OSError:
            continue
        if 'ADR-' not in text:
            continue
        file_kind = _cite_file_kind(name, text)
        if file_kind is None:
            continue
        lines = text.split('\n')
        ignored = {ln for ln, line in enumerate(lines, 1)
                   if 'adr-cite-ignore' in line or 'doclint-allow-retired' in line}
        # Each finding sorts by line and column; for one citation the
        # resolution finding comes before the placement finding.
        file_findings = []
        for ln, match in _cite_cites(lines, ignored, 1, len(lines)):
            result = resolution(match.group(0))
            if result:
                file_findings.append((ln, match.start(), 0, 'resolve', *result))
        if placement != 'off':
            changed_here = changed.get(name) if changed is not None else True
            for ln, col, message in _cite_placement(file_kind, name, text, lines, ignored,
                                                    path_matches(name, shipped_paths)):
                if changed_here is True or (changed_here and ln in changed_here):
                    file_findings.append((ln, col, 1, 'place', place_severity, message))
        for ln, _col, _order, kind, severity, message in sorted(file_findings):
            (found if kind == 'resolve' else placed).append((severity, f"{name}:{ln}", message))
            print(f"  {'❌' if severity == 'error' else '⚠️'} {name}:{ln}  {message}")

    def counts(findings: list) -> str:
        errors = sum(1 for f in findings if f[0] == 'error')
        return f"{errors} errors, {len(findings) - errors} warnings"

    if placement == 'off':
        place_line = "off"
    else:
        place_line = counts(placed)
        if changed is not None:
            place_line += f" (lines changed since {args.since})"
    print(f"\n{'═'*60}")
    print(f"Citations: {counts(found)}")
    print(f"Placement: {place_line}")
    print(f"{'═'*60}")
    if args.check and any(f[0] == 'error' for f in found + placed):
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
