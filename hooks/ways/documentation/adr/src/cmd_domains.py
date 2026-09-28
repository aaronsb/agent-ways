def cmd_domains(args):
    """List available domains."""
    domains = get_domains()

    project = get_config().get('project_name', 'Project')
    print(f"\n{project} — ADR Domain Number Series")
    print("=" * 60)
    print(f"(from {relative_path(get_config_path())})")

    for domain, config in domains.items():
        r = config['range']
        folders = config['folder']
        if isinstance(folders, list):
            folder_str = ', '.join(f"{f}/" for f in folders)
        else:
            folder_str = f"{folders}/"
        print(f"\n  {domain:8} ({r[0]:3}-{r[1]:3})  {config['name']}")
        print(f"           {config['description']}")
        print(f"           Folder: {folder_str}")

    # Show legacy range
    legacy = get_config().get('legacy', {})
    if legacy:
        r = legacy.get('range', (1, 99))
        print(f"\n  {'legacy':8} ({r[0]:3}-{r[1]:3})  {legacy.get('label', 'Legacy')}")

    print()
    seeds = _pending_seeds(get_config())
    for finding in shape_findings(vocabulary_shape(get_all_adrs()), get_config()):
        print(f"Notice: {shape_notice(finding, seeds=seeds)}\n")
    return 0

