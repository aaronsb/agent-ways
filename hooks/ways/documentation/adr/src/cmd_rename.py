def cmd_rename(args):
    """Rename an ADR's title and/or file slug. Number and domain are unchanged.

    Updates the `# ADR-NNN: <title>` heading (when a new title is given) and the
    filename slug, preserving the number prefix and folder. Uses `git mv` when the
    file is tracked so history follows the rename; falls back to a plain move.
    """
    # Normalize reference (accept "38", "038", "ADR-038", "51.2")
    ref = args.adr.upper().replace('ADR-', '').lstrip('0') or '0'

    def filename_number(path):
        m = re.match(r'ADR-(\d+(?:\.\d+)?)', path.name, re.IGNORECASE)
        return m.group(1).lstrip('0') if m else None

    adrs = get_all_adrs(include_archived=True)
    matches = [a for a in adrs if
               (a.number and a.number.lstrip('0') == ref) or
               filename_number(a.path) == ref]

    if not matches:
        print(f"Error: ADR not found: {args.adr}", file=sys.stderr)
        print("Use `adr list` to see available ADRs.", file=sys.stderr)
        return 1
    if len(matches) > 1:
        print(f"Multiple ADRs match '{args.adr}':", file=sys.stderr)
        for adr in matches:
            print(f"  ADR-{adr.number}: {adr.title}", file=sys.stderr)
        return 1
    if is_archived(matches[0].path):
        print(f"Error: ADR-{matches[0].number} is archived "
              f"({relative_path(matches[0].path)}) — archived ADRs are kept "
              f"as-is; not renaming", file=sys.stderr)
        return 1

    if not args.title and not args.slug:
        print("Error: provide a new <title> and/or --slug", file=sys.stderr)
        return 1

    adr = matches[0]
    old_path = adr.path

    # Preserve the exact number prefix from the filename (keeps zero-padding)
    fn_match = re.match(r'(ADR-\d+(?:\.\d+)?)-', old_path.name, re.IGNORECASE)
    if not fn_match:
        print(f"Error: cannot parse ADR number from filename: {old_path.name}", file=sys.stderr)
        return 1
    num_prefix = fn_match.group(1)

    # Update the heading title in-file when a new title is supplied
    original = old_path.read_text()
    content = original
    if args.title:
        content, n = re.subn(
            r'^(# ADR-\d+(?:\.\d+)?: ).+$',
            lambda m: m.group(1) + args.title,
            content, count=1, flags=re.MULTILINE)
        if n == 0:
            print("Warning: no '# ADR-NNN: title' heading found to update", file=sys.stderr)

    # Compute the new filename slug (explicit --slug wins over title-derived)
    slug_source = args.slug if args.slug else args.title
    slug = re.sub(r'[^a-z0-9]+', '-', slug_source.lower()).strip('-')
    if not slug:
        print(f"Error: could not derive a slug from '{slug_source}' "
              "(no alphanumerics); pass --slug explicitly", file=sys.stderr)
        return 1
    new_path = old_path.parent / f"{num_prefix}-{slug}.md"

    if new_path == old_path and content == original:
        print("Nothing to change.")
        return 0
    if new_path != old_path and new_path.exists():
        print(f"Error: target already exists: {relative_path(new_path)}", file=sys.stderr)
        return 1

    # Write any heading change first, then move the file
    if content != original:
        old_path.write_text(content)

    if new_path != old_path:
        moved = False
        try:
            result = subprocess.run(['git', 'mv', str(old_path), str(new_path)],
                                    capture_output=True, text=True, timeout=10)
            moved = result.returncode == 0
        except (FileNotFoundError, subprocess.TimeoutExpired):
            moved = False
        if not moved:
            old_path.rename(new_path)

    print(f"Renamed: ADR-{adr.number}")
    if args.title:
        print(f"  Title: {args.title}")
    if new_path != old_path:
        print(f"  File:  {relative_path(old_path)} → {relative_path(new_path)}")
    print("Run `adr index -y` to refresh INDEX.md.")
    return 0

