# ============================================================================
# Commands
# ============================================================================

def cmd_list(args):
    """List ADRs — the active set by default (ADR-303)."""
    if getattr(args, 'all', False):
        adrs = get_all_adrs(include_archived=True)
    elif getattr(args, 'archived', False):
        adrs = [a for a in get_all_adrs(include_archived=True) if is_archived(a.path)]
    else:
        adrs = get_all_adrs()
    domains = get_domains()

    # Filter by domain
    if args.domain:
        adrs = [a for a in adrs if a.domain == args.domain]

    # Filter by status
    if args.status:
        adrs = [a for a in adrs if a.status and a.status.lower() == args.status.lower()]

    # Sort by number
    def sort_key(adr):
        if not adr.number:
            return (9999, 0)
        parts = adr.number.split('.')
        return (int(parts[0]), int(parts[1]) if len(parts) > 1 else 0)

    adrs.sort(key=sort_key)

    # Frontmatter filters: --field, and --kind, --verb, --capability as its
    # shorthands. A list field matches when it lists the value.
    filters = list(getattr(args, 'field', None) or [])
    for name in ('kind', 'verb', 'capability'):
        if getattr(args, name, None):
            filters.append(f"{name}={getattr(args, name)}")
    for spec in filters:
        key, has_value, value = spec.partition('=')
        adrs = [a for a in adrs if _field_matches(a, key.strip(), value if has_value else None)]

    if getattr(args, 'json', False):
        return _list_json(adrs, getattr(args, 'group_by', None))

    project = get_config().get('project_name', 'Project')
    print(f"\n{project} — Architecture Decision Records ({len(adrs)} total)")
    print("=" * 55)

    def status_icon(status):
        return {
            'Draft': '📝',
            'Proposed': '💡',
            'Accepted': '✅',
            'Superseded': '📦',
            'Deprecated': '🗑️'
        }.get(str(status).capitalize() if status and repo_contract() == 'adr/v1' else status, '❓')

    def print_adr(adr):
        note = supersession_note(adr)
        suffix = f"  ({note})" if note else ''
        marker = ' [archived]' if is_archived(adr.path) and getattr(args, 'all', False) else ''
        print(f"  {status_icon(adr.status)} ADR-{adr.number or '???':8} {adr.title or '(no title)'}{suffix}{marker}")

    if getattr(args, 'group_by', None):
        for value, members in _groups(adrs, args.group_by):
            print(f"\n## {args.group_by}: {value} ({len(members)})")
            print("-" * 50)
            for adr in members:
                print_adr(adr)
    elif args.group:
        # Group by domain - show domains in config order, then legacy
        for domain_key, domain_info in domains.items():
            domain_adrs = [a for a in adrs if a.domain == domain_key]
            if not domain_adrs:
                continue
            print(f"\n## {domain_info.get('name', domain_key)} ({domain_key})")
            print("-" * 50)
            for adr in domain_adrs:
                print_adr(adr)

        # Legacy (no domain)
        legacy_adrs = [a for a in adrs if not a.domain]
        if legacy_adrs:
            legacy_label = get_config().get('legacy', {}).get('label', 'Legacy')
            print(f"\n## {legacy_label}")
            print("-" * 50)
            for adr in legacy_adrs:
                print_adr(adr)
    else:
        # Flat list, sorted by number
        for adr in adrs:
            print_adr(adr)

    print(f"\nTotal: {len(adrs)} ADRs")
    return 0



# --- frontmatter queries for list ----------------------------------------------------

_NO_VALUE = '(none)'

def _field_values(adr, key: str) -> list:
    """A field's values as strings: each item of a list, a scalar alone, and
    nothing for an absent or empty field. status and date come from the
    parsed record, so a v0 record reads the same way."""
    value = adr.frontmatter.get(key)
    if value in (None, '', [], {}):
        return []
    if isinstance(value, list):
        return [json.dumps(v, default=str, ensure_ascii=False) if isinstance(v, (dict, list)) else str(v)
                for v in value]
    if isinstance(value, dict):
        return [json.dumps(value, default=str, ensure_ascii=False)]
    return [str(value)]

def _field_matches(adr, key: str, value: Optional[str]) -> bool:
    values = _field_values(adr, key)
    if value is None:
        return bool(values)
    if key == 'status':
        return value.lower() in (v.lower() for v in values)
    if key in V1_EDGE_FIELDS + ('superseded_by', 'related'):
        # A reference without a section matches every section of that record.
        number, section = norm_ref(value)
        return any(norm_ref(v)[0] == number and (section is None or norm_ref(v)[1] == section)
                   for v in values)
    return value in values

def _groups(adrs: list, key: str) -> list:
    """(value, records) sorted by value, records without the field last.
    A record listing several values is in each."""
    groups = {}
    for adr in adrs:
        for value in _field_values(adr, key) or [_NO_VALUE]:
            groups.setdefault(value, [])
            if adr not in groups[value]:
                groups[value].append(adr)
    ordered = sorted(((v, m) for v, m in groups.items() if v != _NO_VALUE), key=lambda g: g[0].lower())
    if _NO_VALUE in groups:
        ordered.append((_NO_VALUE, groups[_NO_VALUE]))
    return ordered

def _record_json(adr) -> dict:
    return {'number': adr.number, 'title': adr.title, 'path': str(relative_path(adr.path)),
            'status': adr.status, 'frontmatter': adr.frontmatter}

def _list_json(adrs: list, group_by: Optional[str]) -> int:
    if group_by:
        data = {value: [_record_json(a) for a in members] for value, members in _groups(adrs, group_by)}
    else:
        data = [_record_json(a) for a in adrs]
    print(json.dumps(data, indent=2, default=str, ensure_ascii=False))
    return 0
