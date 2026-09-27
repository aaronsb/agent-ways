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
        }.get(str(status).capitalize() if status else status, '❓')

    def print_adr(adr):
        note = supersession_note(adr)
        suffix = f"  ({note})" if note else ''
        marker = ' [archived]' if is_archived(adr.path) and getattr(args, 'all', False) else ''
        print(f"  {status_icon(adr.status)} ADR-{adr.number or '???':8} {adr.title or '(no title)'}{suffix}{marker}")

    if args.group:
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


