def cmd_new(args):
    """Create a new ADR."""
    domain = args.domain.lower()
    domains = get_domains()
    defaults = get_defaults()

    if domain not in domains:
        print(f"Error: Unknown domain '{domain}'", file=sys.stderr)
        print(f"Valid domains: {', '.join(domains.keys())}", file=sys.stderr)
        return 1

    config = domains[domain]
    span, folders = domain_range(config), domain_folders(config)
    if span is None or not folders:
        missing = 'range' if span is None else 'folder'
        print(f"Error: domain '{domain}' has no {missing} in adr.yaml; `adr lint` names what it is missing", file=sys.stderr)
        return 1

    # Find next available number in range — archived ADRs keep their numbers
    adrs = get_all_adrs(include_archived=True)
    used_numbers = set()
    for adr in adrs:
        if adr.number:
            try:
                used_numbers.add(int(adr.number.split('.')[0]))
            except ValueError:
                pass

    next_num = None
    for n in range(span[0], span[1] + 1):
        if n not in used_numbers:
            next_num = n
            break

    if next_num is None:
        print(f"Error: No available numbers in {domain} range ({span[0]}-{span[1]})", file=sys.stderr)
        return 1

    # Generate slug from title
    slug = re.sub(r'[^a-z0-9]+', '-', args.title.lower()).strip('-')

    # Create file path (use first folder if multiple)
    primary_folder = folders[0]
    folder = get_project_root() / 'docs' / 'architecture' / primary_folder
    filename = f"ADR-{next_num:03d}-{slug}.md"
    filepath = folder / filename

    if filepath.exists():
        print(f"Error: File already exists: {filepath}", file=sys.stderr)
        return 1

    today = date.today().isoformat()
    if get_config().get('contract') == V1:
        content = _v1_content(args, next_num, domain, today, defaults)
        if content is None:
            return 1
        folder.mkdir(parents=True, exist_ok=True)
        filepath.write_text(content)
        print(f"Created: {relative_path(filepath)}")
        print(f"  Domain: {config.get('name') or domain} ({domain})")
        print(f"  Number: ADR-{next_num:03d}")
        print("  Contract: adr/v1 (fill the empty fields; `adr lint` lists them)")
        return 0

    # Generate content
    default_status = defaults.get('status', 'Draft')
    default_deciders = defaults.get('deciders', [])

    # Auto-detect current user if no deciders configured
    if not default_deciders:
        git_user = _detect_git_user()
        if git_user:
            default_deciders = [git_user]

    deciders_yaml = '\n'.join(f'  - {d}' for d in default_deciders) if default_deciders else '  - # add deciders'

    content = f'''---
status: {default_status}
date: {today}
deciders:
{deciders_yaml}
related: []
---

# ADR-{next_num:03d}: {args.title}

'''  + BODY_SKELETON

    # Write file
    folder.mkdir(parents=True, exist_ok=True)
    filepath.write_text(content)

    print(f"Created: {relative_path(filepath)}")
    print(f"  Domain: {config.get('name') or domain} ({domain})")
    print(f"  Number: ADR-{next_num:03d}")
    return 0


def _v1_content(args, number: int, domain: str, today: str, defaults: dict) -> Optional[str]:
    """A v1 record from an empty sheet (ADR-306 §3). The kind's schema decides
    which fields the record carries; fields the arguments do not give stay
    empty, and lint names each one. Arguments the kind cannot take are refused."""
    config = get_config()
    kinds = {k: v for k, v in _mapping(config.get('kinds')).items() if isinstance(v, dict)}
    if not kinds:
        print("Error: adr.yaml declares contract adr/v1 but no kinds; `adr lint` reports the config", file=sys.stderr)
        return None
    wanted = (args.kind or 'decision').lower()
    kind = next((k for k in kinds if str(k).lower() == wanted), None)
    if kind is None:
        print(f"Error: Unknown kind '{args.kind or 'decision'}'. Kinds: {', '.join(map(str, kinds))}", file=sys.stderr)
        return None
    schema = kinds[kind]
    problems = []
    takes_verb = schema.get('verb') == 'required'
    if args.verb and not takes_verb:
        problems.append(f"a {kind} record takes no verb")
    verbs = _str_list(config.get('verbs')) or list(V1_VERBS)
    if args.verb and takes_verb and args.verb not in verbs:
        problems.append(f"verb '{args.verb}' is not one of: {', '.join(verbs)}")
    vocabulary = _mapping(config.get('capabilities'))
    if args.capability and args.capability not in vocabulary:
        problems.append(f"capability '{args.capability}' is not in the adr.yaml vocabulary")
    if (args.agent or args.model) and 'agent' not in v1_requires(schema):
        problems.append(f"a {kind} record carries no agent")
    for problem in problems:
        print(f"Error: {problem}", file=sys.stderr)
    if problems:
        return None
    deciders = list(defaults.get('deciders') or [])
    if not deciders:
        git_user = _detect_git_user()
        if git_user:
            deciders = [git_user]
    given = {'verb': args.verb, 'capability': args.capability, 'agent': args.agent, 'model': args.model}
    record = empty_record(kind, schema, given, defaults)
    record.update({'date': today, 'deciders': deciders, 'related': []})
    sections = _str_list(schema.get('sections')) or []
    summary = V1_SUMMARY_SKELETON if 'Summary' in sections else None
    sheet = new_sheet(number, domain, args.title, record, summary=summary,
                      body=BODY_SKELETON if takes_verb else '')
    return render_record(sheet)
