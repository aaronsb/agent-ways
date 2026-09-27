def _lifecycle_target(args):
    """The v1 record the command acts on, or an exit code after an error."""
    matches = find_by_ref(args.adr, get_all_adrs())
    if not matches:
        print(f"Error: ADR not found: {args.adr}", file=sys.stderr)
        return None, 1
    if len(matches) > 1:
        print(f"Error: '{args.adr}' matches several ADRs; name the sub-part, e.g. ADR-101.1", file=sys.stderr)
        return None, 1
    adr = matches[0]
    if repo_contract() != 'adr/v1' or adr.contract != 'adr/v1':
        print(f"Error: ADR-{adr.number} is not an adr/v1 record. A v0 record's status is edited by hand, "
              f"as before (ADR-304 §7).", file=sys.stderr)
        return None, 1
    if str(adr.status or '').lower() != 'proposed':
        print(f"Error: ADR-{adr.number} is {adr.status}; only a proposed record can be "
              f"accepted, rejected or abandoned.", file=sys.stderr)
        return None, 1
    return adr, 0

def _set_status(text: str, status: str) -> str:
    """Rewrite the status line inside the frontmatter only."""
    lines = text.split('\n')
    for i, line in enumerate(lines[1:], 1):
        if line.strip() == '---':
            break
        if re.match(r'^status\s*:', line):
            lines[i] = f"status: {status}"
            return '\n'.join(lines)
    raise ValueError("no status line in the frontmatter")

def cmd_accept(args):
    """Accept a proposed adr/v1 record (ADR-304 §2, §11, §12).

    Runs every v1 rule as if the record were accepted and refuses on any
    error, so a decision whose basis does not meet the rules, or one the
    operator started without a considered entry, stays proposed. Open
    concerns are listed: shown at acceptance, not blocking.
    """
    adr, code = _lifecycle_target(args)
    if adr is None:
        return code
    corpus = get_all_adrs(include_archived=True)
    trial = next(a for a in corpus if a.path == adr.path)
    trial.status = 'accepted'
    trial.frontmatter = dict(trial.frontmatter, status='accepted')
    trial.issues = []
    ctx = LintContext.from_corpus(corpus)
    for rule, contract in FILE_RULES:
        if contract in (None, ctx.contract):
            rule(trial, ctx)
    for rule, contract in CORPUS_RULES:
        if contract in (None, ctx.contract):
            rule(trial, ctx)
    errors = [i for i in trial.issues if i.severity == 'error']
    concerns = [i for i in trial.issues if i.message.startswith('open concern:')]
    if errors:
        print(f"Refused: ADR-{adr.number} would not lint clean as accepted:", file=sys.stderr)
        for issue in errors:
            print(f"  ❌ {issue.message}", file=sys.stderr)
        return 1
    if concerns:
        print(f"Open concerns on ADR-{adr.number} at acceptance (shown, not blocking):")
        for issue in concerns:
            print(f"  ⚠️ {issue.message[len('open concern: '):]}")
    if args.dry_run:
        print(f"Would accept ADR-{adr.number}: {adr.title}")
        return 0
    adr.path.write_text(_set_status(adr.path.read_text(), 'accepted'))
    print(f"Accepted ADR-{adr.number}: {adr.title}")
    print(f"Run `adr index -y` to refresh INDEX.md.")
    return 0

def _close(args, status: str):
    """Reject or abandon a proposed record, recording why (ADR-304 §2)."""
    adr, code = _lifecycle_target(args)
    if adr is None:
        return code
    reason = (args.reason or '').strip()
    if not reason:
        print(f"Error: --reason is required; {'an' if status[0] in 'aeiou' else 'a'} {status} record states why.", file=sys.stderr)
        return 1
    text = _set_status(adr.path.read_text(), status)
    # Appended, never inserted: a closed record stays append-only.
    closure = f"\n## Closure\n\n{status.capitalize()} {date.today().isoformat()}: {reason}\n"
    text = text.rstrip('\n') + '\n' + closure
    if args.dry_run:
        print(f"Would mark ADR-{adr.number} {status}: {reason}")
        return 0
    adr.path.write_text(text)
    print(f"Marked ADR-{adr.number} {status}: {adr.title}")
    print(f"Run `adr index -y` to refresh INDEX.md.")
    return 0

def cmd_reject(args):
    return _close(args, 'rejected')

def cmd_abandon(args):
    return _close(args, 'abandoned')

