def cmd_lint(args):
    """Lint ADR files for issues.

    Deliberately covers the archive (ADR-303): a malformed ADR must not be
    able to hide by being moved.
    """
    if args.paths:
        paths = [Path(p) for p in args.paths]
        adrs = [parse_adr(p) for p in paths]
    else:
        adrs = get_all_adrs(include_archived=True)

    # Cross-file supersession checks resolve against the full corpus, even
    # when linting an explicit subset of paths.
    corpus = adrs if not args.paths else get_all_adrs(include_archived=True)
    by_number = {}
    for adr in corpus:
        if adr.number:
            by_number.setdefault(adr.number.lstrip('0') or '0', adr)
        # Filename fallback: a target with a malformed H1 should still resolve
        fn_number = filename_number(adr.path)
        if fn_number:
            by_number.setdefault(fn_number, adr)

    for adr in adrs:
        for field_name in ('supersedes', 'superseded_by'):
            for entry in getattr(adr, field_name):
                number, _ = norm_ref(entry)
                target = by_number.get(number)
                if target is None:
                    adr.issues.append(Issue(
                        f"{field_name}: '{entry}' resolves to no known ADR", 'error'))
                    continue
                # Reciprocity: A superseded_by B ⟺ B supersedes A (sections ignored)
                if adr.number:
                    own = adr.number.lstrip('0') or '0'
                    reverse = 'supersedes' if field_name == 'superseded_by' else 'superseded_by'
                    reverse_numbers = {norm_ref(e)[0] for e in getattr(target, reverse)}
                    if own not in reverse_numbers:
                        adr.issues.append(Issue(
                            f"{field_name}: ADR-{number} does not declare the reciprocal "
                            f"{reverse}: ADR-{own} — one-directional links rot", 'warning'))

    total_errors = 0
    total_warnings = 0

    # Status summary
    status_counts = {}
    for adr in adrs:
        status = adr.status or 'Unknown'
        status_counts[status] = status_counts.get(status, 0) + 1

    print(f"\nScanned: {len(adrs)} ADRs")
    print(f"\nStatus distribution:")
    for status, count in sorted(status_counts.items()):
        print(f"  {status}: {count}")

    # Issues
    files_with_issues = [adr for adr in adrs if adr.issues]

    if files_with_issues:
        print(f"\n{'─'*60}")
        print(f"Issues found in {len(files_with_issues)} files:")
        print(f"{'─'*60}")

        for adr in files_with_issues:
            print(f"\n{relative_path(adr.path)}")

            for issue in adr.issues:
                icon = '❌' if issue.severity == 'error' else '⚠️'
                print(f"  {icon} {issue.message}")

                if issue.severity == 'error':
                    total_errors += 1
                else:
                    total_warnings += 1

    print(f"\n{'═'*60}")
    print(f"Summary: {total_errors} errors, {total_warnings} warnings")
    print(f"{'═'*60}\n")

    if args.check and total_errors > 0:
        return 1
    return 0

