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
    for n in range(config['range'][0], config['range'][1] + 1):
        if n not in used_numbers:
            next_num = n
            break

    if next_num is None:
        print(f"Error: No available numbers in {domain} range ({config['range'][0]}-{config['range'][1]})", file=sys.stderr)
        return 1

    # Generate slug from title
    slug = re.sub(r'[^a-z0-9]+', '-', args.title.lower()).strip('-')

    # Create file path (use first folder if multiple)
    folders = config['folder']
    primary_folder = folders[0] if isinstance(folders, list) else folders
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
        print(f"  Domain: {config['name']} ({domain})")
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

## Context

[What is the issue that we're seeing that is motivating this decision or change?]

## Decision

[What is the change that we're proposing and/or doing?]

## Consequences

### Positive

- [What becomes easier?]

### Negative

- [What becomes harder?]

### Neutral

- [What other changes does this enable or require?]

## Alternatives Considered

- [What other options were evaluated?]
- [Why were they rejected?]
'''

    # Write file
    folder.mkdir(parents=True, exist_ok=True)
    filepath.write_text(content)

    print(f"Created: {relative_path(filepath)}")
    print(f"  Domain: {config['name']} ({domain})")
    print(f"  Number: ADR-{next_num:03d}")
    return 0


def _v1_content(args, number: int, domain: str, today: str, defaults: dict) -> Optional[str]:
    """A v1 record from an empty sheet (ADR-306 §3). Fields the arguments do
    not give are left empty for the author to fill; lint names each one."""
    kinds = {k: v for k, v in _mapping(get_config().get('kinds')).items() if isinstance(v, dict)}
    kind = (args.kind or 'decision').lower()
    if kind not in kinds:
        print(f"Error: Unknown kind '{kind}'. Kinds: {', '.join(kinds)}", file=sys.stderr)
        return None
    deciders = list(defaults.get('deciders') or [])
    if not deciders:
        git_user = _detect_git_user()
        if git_user:
            deciders = [git_user]
    record = {'contract': V1, 'kind': kind}
    is_decision = kinds[kind].get('verb') == 'required'
    if is_decision:
        record['verb'] = args.verb
    record['capability'] = args.capability
    if is_decision:
        record['basis'] = []
        record['agent'] = {'name': args.agent, 'model': args.model}
    record.update({'status': 'proposed', 'date': today, 'deciders': deciders, 'related': []})
    sheet = new_sheet(number, domain, args.title, record, body=V1_BODY_SKELETON if is_decision else '')
    return render_record(sheet)
