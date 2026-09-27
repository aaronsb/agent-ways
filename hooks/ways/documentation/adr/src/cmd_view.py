def cmd_view(args):
    """View an ADR using the configured viewer."""
    import shutil

    # Find the ADR by title number or filename number (archived stay viewable)
    matches = find_by_ref(args.adr, get_all_adrs(include_archived=True))

    if not matches:
        print(f"Error: ADR not found: {args.adr}", file=sys.stderr)
        print(f"Use `adr list` to see available ADRs.", file=sys.stderr)
        return 1

    if len(matches) > 1:
        print(f"Multiple ADRs match '{args.adr}':")
        for adr in matches:
            print(f"  ADR-{adr.number}: {adr.title}")
        return 1

    adr = matches[0]

    # Get viewer command from config
    viewer_cmd = get_config().get('viewer', 'cat {file}')

    # Check if viewer command exists
    viewer_bin = viewer_cmd.split()[0]
    if not shutil.which(viewer_bin):
        print(f"Warning: Viewer '{viewer_bin}' not found, using cat", file=sys.stderr)
        viewer_cmd = 'cat {file}'

    # Build and run command
    cmd = viewer_cmd.replace('{file}', str(adr.path))
    return subprocess.run(cmd, shell=True).returncode


