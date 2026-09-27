def cmd_archive(args):
    """Archive an ADR out of the active set (ADR-303 / issue #438).

    git mv to docs/architecture/archive/<folder>/, rewrite status, prepend a
    banner after the H1. Archiving is not deletion: the file stays tracked,
    linted, and linkable.
    """
    root = get_project_root()
    arch_root = root / 'docs' / 'architecture'
    today = date.today().isoformat()

    # Validate status against config
    archive_status = args.status or 'Superseded'
    valid_statuses = get_statuses()
    if archive_status not in valid_statuses:
        print(f"Error: status '{archive_status}' not in adr.yaml "
              f"(valid: {', '.join(sorted(valid_statuses))})", file=sys.stderr)
        return 1

    # Resolve the ADR
    all_adrs = get_all_adrs(include_archived=True)
    matches = find_by_ref(args.adr, all_adrs)
    if not matches:
        print(f"Error: ADR not found: {args.adr}", file=sys.stderr)
        return 1
    if len(matches) > 1:
        print(f"Multiple ADRs match '{args.adr}':", file=sys.stderr)
        for adr in matches:
            print(f"  ADR-{adr.number}: {adr.title}", file=sys.stderr)
        return 1
    adr = matches[0]

    # Already archived: no-op, not an error
    if is_archived(adr.path):
        print(f"ADR-{adr.number} is already archived: {relative_path(adr.path)}")
        return 0

    # Refuse to archive a partially superseded document (ADR-303 part C):
    # superseded_by declared while the status is still in force means part of
    # it is live — the archive is for documents a reader no longer opens.
    if adr.superseded_by and adr.status not in NON_ACTIVE_STATUSES:
        note = supersession_note(adr)
        print(f"Error: ADR-{adr.number} is {note} while status is "
              f"'{adr.status}' — it is partially superseded, and mostly-live "
              f"documents do not belong in the archive. Either supersede it "
              f"fully (set status accordingly) or leave it active.",
              file=sys.stderr)
        return 1

    # Parse and validate --superseded-by against the corpus
    sup_refs = []
    if args.superseded_by:
        for raw in args.superseded_by.split(','):
            raw = raw.strip()
            if not raw:
                continue
            number, section = norm_ref(raw)
            if not find_by_ref(number, all_adrs):
                print(f"Error: --superseded-by '{raw}' resolves to no known ADR",
                      file=sys.stderr)
                return 1
            sup_refs.append(f"ADR-{number}#{section}" if section else f"ADR-{number}")

    dest_dir = arch_root / 'archive' / adr.path.parent.name
    dest = dest_dir / adr.path.name
    if dest.exists():
        print(f"Error: {relative_path(dest)} already exists — refusing to "
              f"overwrite", file=sys.stderr)
        return 1

    if args.dry_run:
        print(f"Would archive ADR-{adr.number}: {adr.title}")
        print(f"  move:   {relative_path(adr.path)} -> {relative_path(dest)}")
        print(f"  status: {adr.status} -> {archive_status}")
        print(f"  banner: date {today}, reason: {args.reason}"
              + (f", superseded by: {', '.join(sup_refs)}" if sup_refs else ""))
        return 0

    dest_dir.mkdir(parents=True, exist_ok=True)

    # git mv so history follows the file; plain rename outside a work tree
    moved_by_git = False
    try:
        result = subprocess.run(
            ['git', 'mv', str(adr.path), str(dest)],
            cwd=root, capture_output=True, text=True, timeout=10)
        moved_by_git = result.returncode == 0
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass
    if not moved_by_git:
        adr.path.rename(dest)

    # Read the moved file defensively — a broken move must be a named failure
    try:
        content = dest.read_text()
    except Exception as e:
        print(f"Error: archive move left no readable file at "
              f"{relative_path(dest)}: {e}", file=sys.stderr)
        return 1

    lines = content.split('\n')

    # Rewrite frontmatter: status, and merge superseded_by
    fm_end = None
    if lines and lines[0].strip() == '---':
        for i, line in enumerate(lines[1:], 1):
            if line.strip() == '---':
                fm_end = i
                break
    if fm_end:
        new_fm = []
        skipping_sup = False
        status_idx = None
        for line in lines[1:fm_end]:
            if skipping_sup:
                if line.startswith((' ', '\t')) or line.strip().startswith('-'):
                    continue
                skipping_sup = False
            if re.match(r'^superseded_by\s*:', line):
                # existing entries already merged via adr.superseded_by
                skipping_sup = not line.split(':', 1)[1].strip()
                continue
            if re.match(r'^status\s*:', line):
                status_idx = len(new_fm)
                new_fm.append(f'status: {archive_status}')
                continue
            new_fm.append(line)
        if status_idx is None:
            # No status field at all — an archived ADR must carry one
            status_idx = len(new_fm)
            new_fm.append(f'status: {archive_status}')
        merged = []
        for entry in [*adr.superseded_by, *sup_refs]:
            number, section = norm_ref(entry)
            normalized = f"ADR-{number}#{section}" if section else f"ADR-{number}"
            if normalized not in merged:
                merged.append(normalized)
        if merged:
            insert_at = (status_idx + 1) if status_idx is not None else len(new_fm)
            new_fm[insert_at:insert_at] = ['superseded_by:'] + [
                f'  - "{entry}"' for entry in merged]
        lines = ['---'] + new_fm + lines[fm_end:]

    # Banner after the H1 so the title still reads first. Scan only below the
    # frontmatter — a YAML '# comment' line must not be mistaken for the H1.
    body_start = 0
    if lines and lines[0].strip() == '---':
        for i, line in enumerate(lines[1:], 1):
            if line.strip() == '---':
                body_start = i + 1
                break
    h1_idx = next((i for i in range(body_start, len(lines))
                   if lines[i].startswith('# ')), None)
    if h1_idx is None:
        print(f"Error: no H1 found in {relative_path(dest)} — file moved but "
              f"banner not written; fix by hand", file=sys.stderr)
        dest.write_text('\n'.join(lines))
        return 1
    banner = [
        '',
        f'> **ARCHIVED — {today}.** No longer part of the active architecture set. Kept for history',
        '> and so existing references still resolve.',
        '>',
        f'> **Why:** {args.reason}',
    ]
    if sup_refs:
        banner.append(f"> **Superseded by:** {', '.join(sup_refs)}")
    banner += ['>', '> Nothing below this line has been edited.']
    lines[h1_idx + 1:h1_idx + 1] = banner

    dest.write_text('\n'.join(lines))

    print(f"Archived ADR-{adr.number}: {adr.title}")
    print(f"  {relative_path(adr.path)} -> {relative_path(dest)}"
          + ("" if moved_by_git else "  (plain rename — git mv unavailable or failed)"))
    print(f"  status: {archive_status}"
          + (f", superseded by: {', '.join(sup_refs)}" if sup_refs else ""))
    print(f"\nRegenerate the index: {sys.argv[0]} index -y")
    return 0

