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

    # Cross-file rules resolve against the full corpus, even when linting an
    # explicit subset of paths.
    corpus = adrs if not args.paths else get_all_adrs(include_archived=True)
    ctx = LintContext.from_corpus(corpus)
    run_rules(adrs, ctx)

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

    # Under adr/v1, the records not yet migrated (ADR-304 §7)
    if ctx.contract == V1:
        v0_count = sum(1 for adr in adrs if not is_v1_record(adr, ctx))
        print(f"\nContract: {V1} ({v0_count} v0 records remain)")

    # Issues. adr.yaml's own issues come first, under its path.
    files_with_issues = [adr for adr in adrs if adr.issues]
    if ctx.config_issues:
        files_with_issues.insert(0, ADRInfo(path=get_config_path(), issues=ctx.config_issues))

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

