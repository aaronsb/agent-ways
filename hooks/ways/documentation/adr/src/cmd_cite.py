def cmd_cite(args):
    """Check ADR citations in the code against the records they cite.

    ADR-304 §6 gives code citations a maintenance loop:

    - an ADR-N that resolves to no record is an error
    - a citation whose records are all out of force (superseded, deprecated,
      rejected, abandoned, archived) warns and names each status and successor
    - a citation of a proposed or draft record prompts acceptance
    - under adr/v1, a citation of a record on a capability whose latest
      accepted add-or-cut is a cut warns until the cut is enacted and fails
      after (§5)
    - under adr/v1, an accepted retire's targets are checked against the
      surface inventory commands in adr.yaml (§5). Inventory commands are
      shell commands from adr.yaml; they run only on a whole-repo scan and
      never with --no-inventory.

    A bare ADR-N resolves to its family {N, N.1, ...}. Files come from
    `git ls-files`, so ignored paths are never read. docs/architecture and the
    paths in adr.yaml cite.exclude are skipped (a glob, or a directory prefix
    when the pattern has no glob character), and a line carrying
    `adr-cite-ignore` (or doclint's `doclint-allow-retired`) is skipped.
    """
    import fnmatch

    root = get_project_root().resolve()
    config = get_config()
    cite_config = config.get('cite') if isinstance(config.get('cite'), dict) else {}
    excludes = ['docs/architecture/'] + [str(p) for p in (cite_config.get('exclude') or [])]

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
    ctx = LintContext.from_corpus(records)
    v1 = ctx.contract == V1

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

    # The state of each capability is its latest accepted add or cut, by date
    # then number (ADR-304 §3). Only a capability whose latest is a cut counts.
    cuts = {}
    if v1:
        latest = {}
        for adr in records:
            verb = adr.frontmatter.get('verb')
            if (is_v1_record(adr, ctx) and verb in ('add', 'cut')
                    and str(adr.status or '').lower() == 'accepted'):
                order = decision_order(adr)
                for capability in capability_scope(adr):
                    if capability not in latest or order >= latest[capability][0]:
                        latest[capability] = (order, adr)
        for capability, (_, adr) in latest.items():
            if adr.frontmatter.get('verb') == 'cut':
                cuts[capability] = (adr, bool(adr.frontmatter.get('enacted')))

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
                for member in members:
                    if not is_v1_record(member, ctx):
                        continue
                    for capability in capability_scope(member):
                        if capability in cuts and cuts[capability][0] is not member:
                            cut, enacted = cuts[capability]
                            if enacted:
                                findings.append(('error', where, f"{cited} is on '{capability}', cut and enacted by ADR-{cut.number}; remove the citation"))
                            else:
                                findings.append(('warning', where, f"{cited} is on '{capability}', which ADR-{cut.number} cuts; remove before enacting"))

    # Retire targets against the surface inventories (§5). Inventories run
    # shell commands from adr.yaml, so only on a whole-repo scan.
    if v1 and not scopes and not args.no_inventory:
        surfaces = v1_surfaces(ctx)
        listed_by, failed = {}, set()
        for adr in records:
            if (not is_v1_record(adr, ctx) or adr.frontmatter.get('verb') != 'retire'
                    or str(adr.status or '').lower() != 'accepted'):
                continue
            enacted = bool(adr.frontmatter.get('enacted'))
            for target in as_entries(adr.frontmatter.get('targets')):
                namespace, _, target_name = target.partition(':')
                command = _mapping(surfaces.get(namespace)).get('inventory')
                if not command:
                    continue
                if namespace not in listed_by:
                    listed_by[namespace] = _inventory(str(command), root)
                listed, failure = listed_by[namespace]
                where = str(relative_path(adr.path))
                if listed is None:
                    if namespace not in failed:  # once per namespace, not per target
                        failed.add(namespace)
                        findings.append(('warning', where, f"inventory for '{namespace}' did not run ({failure}): {command}"))
                elif target_name in listed:
                    if enacted:
                        findings.append(('error', where, f"{target} is still present after ADR-{adr.number} was enacted"))
                    else:
                        findings.append(('warning', where, f"{target} is still present; ADR-{adr.number} retires it"))
                elif not enacted:
                    findings.append(('warning', where, f"{target} is not in the '{namespace}' inventory; check the name"))

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

def _inventory(command: str, root: Path) -> tuple:
    """Run a surface inventory command from adr.yaml. Returns (names, None),
    or (None, reason) when it failed. One name per line of output."""
    import signal
    try:
        proc = subprocess.Popen(command, shell=True, cwd=root, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                text=True, start_new_session=True)
    except OSError as e:
        return None, str(e)
    try:
        stdout, stderr = proc.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)  # the shell and anything it started
        proc.communicate()
        return None, 'timed out after 60s'
    if proc.returncode != 0:
        detail = (stderr.strip().splitlines() or [''])[-1][:120]
        return None, f"exit {proc.returncode}" + (f": {detail}" if detail else '')
    return {line.strip() for line in stdout.splitlines() if line.strip()}, None

