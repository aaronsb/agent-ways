def cmd_cite(args):
    """Check ADR citations in the code against the records they cite.

    ADR-304 §6 gives code citations a maintenance loop:

    - an ADR-N that resolves to no record is an error
    - a citation whose records are all superseded warns and names successors
    - a citation of a proposed record prompts acceptance
    - under adr/v1, a citation of a capability that a cut decision removes
      warns until the cut is enacted and fails after (§5)
    - under adr/v1, a retire decision's targets are checked against the
      surface inventory commands in adr.yaml, when a surface has one

    A bare ADR-N resolves to its family {N, N.1, ...}. Files come from
    `git ls-files`, so ignored paths are never read. docs/architecture and the
    paths in adr.yaml cite.exclude are skipped, and a line carrying
    `adr-cite-ignore` (or doclint's `doclint-allow-retired`) is skipped.
    """
    import fnmatch

    root = get_project_root()
    config = get_config()
    cite_config = config.get('cite') if isinstance(config.get('cite'), dict) else {}
    excludes = ['docs/architecture/*'] + [str(p) for p in (cite_config.get('exclude') or [])]
    records = get_all_adrs(include_archived=True)
    ctx = LintContext.from_corpus(records)
    v1 = ctx.contract == V1

    def family(number: str) -> list:
        base = number.split('.')[0]
        if '.' in number:
            return [a for a in records if a.number and a.number.lstrip('0') == number]
        return [a for a in records if a.number and
                (a.number.lstrip('0') == base or a.number.lstrip('0').startswith(base + '.'))]

    def successors(adr) -> str:
        named = ', '.join(f"ADR-{norm_ref(e)[0]}" for e in adr.superseded_by)
        return f"see {named}" if named else 'no successor named'

    def is_proposed(adr) -> bool:
        return str(adr.status or '').lower() in ('proposed', 'draft')

    # Capabilities removed by an accepted cut: capability -> (cut record, enacted?)
    cuts = {}
    if v1:
        for adr in records:
            if (is_v1_record(adr, ctx) and adr.frontmatter.get('verb') == 'cut'
                    and str(adr.status or '').lower() == 'accepted'):
                for capability in capability_scope(adr):
                    cuts[capability] = (adr, bool(adr.frontmatter.get('enacted')))

    findings = []  # (severity, location, message)

    def scan_files():
        out = _git(['ls-files', '--cached', '--others', '--exclude-standard', '-z'], root)
        names = out.split('\0') if out is not None else [
            str(p.relative_to(root)) for p in root.rglob('*') if p.is_file()]
        for name in names:
            if not name or any(fnmatch.fnmatch(name, pattern) for pattern in excludes):
                continue
            path = root / name
            if path.suffix not in CITE_EXTS or path.name in ('adr.yaml', 'INDEX.md'):
                continue
            yield name, path

    for name, path in scan_files():
        if args.paths and not any(name == p or name.startswith(p.rstrip('/') + '/') for p in args.paths):
            continue
        try:
            text = path.read_text()
        except (OSError, UnicodeDecodeError):
            continue
        if 'ADR-' not in text:
            continue
        for ln, line in enumerate(text.split('\n'), 1):
            if 'adr-cite-ignore' in line or 'doclint-allow-retired' in line:
                continue
            for match in CITE_RE.finditer(line):
                number = match.group(1).lstrip('0') + (match.group(2) or '')
                cited = match.group(0)  # as written, e.g. ADR-005
                where = f"{name}:{ln}"
                members = family(number)
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

    # Retire targets against the surface inventories (§5)
    if v1:
        surfaces = v1_surfaces(ctx)
        for adr in records:
            if not is_v1_record(adr, ctx) or adr.frontmatter.get('verb') != 'retire':
                continue
            if str(adr.status or '').lower() != 'accepted':
                continue
            enacted = bool(adr.frontmatter.get('enacted'))
            for target in as_entries(adr.frontmatter.get('targets')):
                namespace, _, target_name = target.partition(':')
                command = _mapping(surfaces.get(namespace)).get('inventory')
                if not command:
                    continue
                listed = _inventory(command, root)
                if listed is None:
                    findings.append(('warning', relative_path(adr.path), f"inventory for '{namespace}' did not run: {command}"))
                elif target_name in listed:
                    if enacted:
                        findings.append(('error', relative_path(adr.path), f"{target} is still present after ADR-{adr.number} was enacted"))
                    else:
                        findings.append(('warning', relative_path(adr.path), f"{target} is still present; ADR-{adr.number} retires it"))
                elif not enacted and target_name not in listed:
                    findings.append(('warning', relative_path(adr.path), f"{target} is not in the '{namespace}' inventory; check the name"))

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

def _inventory(command: str, root: Path) -> Optional[set]:
    """Run a surface inventory command from adr.yaml; one name per line."""
    try:
        result = subprocess.run(command, shell=True, cwd=root, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired:
        return None
    if result.returncode != 0:
        return None
    return {line.strip() for line in result.stdout.splitlines() if line.strip()}

