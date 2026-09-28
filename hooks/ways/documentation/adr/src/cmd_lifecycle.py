def _lifecycle_target(args):
    """The v1 record the command acts on, or an exit code after an error."""
    matches = find_by_ref(args.adr, get_all_adrs())
    if not matches:
        print(f"Error: ADR not found: {args.adr}", file=sys.stderr)
        return None, 1
    if len(matches) > 1:
        print(f"Error: '{args.adr}' matches several records; resolve the duplicate number first:", file=sys.stderr)
        for match in matches:
            print(f"  {relative_path(match.path)}", file=sys.stderr)
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

def _set_status(raw: bytes, status: str) -> bytes:
    """Rewrite the one status line in the frontmatter, keeping the line's
    ending (LF or CRLF) and any trailing comment. Refuses a file with no status
    line or with several."""
    lines = raw.split(b'\n')
    status_lines = []
    for i, line in enumerate(lines[1:], 1):
        if line.strip() == b'---':
            break
        if re.match(rb'^status\s*:', line):
            status_lines.append(i)
    if len(status_lines) != 1:
        raise ValueError(f"expected one status line in the frontmatter, found {len(status_lines)}")
    i = status_lines[0]
    line = lines[i]
    ending = b'\r' if line.endswith(b'\r') else b''
    comment = re.search(rb'\s+#.*$', line.rstrip(b'\r'))
    lines[i] = b'status: ' + status.encode() + (comment.group(0) if comment else b'') + ending
    return b'\n'.join(lines)

def _trial(adr, status: str):
    """The record with its status changed, and its own errors as (where,
    message): the file rules only, over this one record. The rest of the
    corpus is not re-linted (ADR-311 §3)."""
    corpus = get_all_adrs(include_archived=True)
    ctx = LintContext.from_corpus(corpus)
    trial = next(a for a in corpus if a.path == adr.path)
    trial.status = status
    trial.frontmatter = dict(trial.frontmatter, status=status)
    # The record was parsed clean enough to reach here (_lifecycle_target
    # refused anything else), so starting its issues fresh loses nothing.
    trial.issues = []
    for rule in _active(FILE_RULES, ctx):
        rule(trial, ctx)
    where = relative_path(trial.path)
    return trial, [(where, i.message) for i in trial.issues if i.severity == 'error']

def _write_status(adr, status: str, suffix: str = '') -> Optional[str]:
    """Write the new status (and an appended suffix), then re-read the file to
    confirm the status took. Returns an error message or None."""
    raw = adr.path.read_bytes()
    try:
        updated = _set_status(raw, status)
    except ValueError as e:
        return str(e)
    if suffix:
        newline = b'\r\n' if b'\r\n' in raw else b'\n'
        updated = updated.rstrip(b'\r\n') + newline + suffix.replace('\n', newline.decode()).encode()
    check = parse_text(updated.decode('utf-8', 'replace'), adr.path)
    if str(check.status or '').lower() != status:
        return f"the status did not take (the file reads '{check.status}')"
    adr.path.write_bytes(updated)
    return None

def _refuse(adr, verb: str, new: list) -> int:
    print(f"Refused: {verb} ADR-{adr.number} would leave it with errors:", file=sys.stderr)
    for where, message in new:
        print(f"  ❌ {where}: {message}", file=sys.stderr)
    return 1

def cmd_accept(args):
    """Accept a proposed adr/v1 record (ADR-304 §2, ADR-311 §3).

    Runs the record's own file rules as if it were accepted and refuses if
    any reports an error. The rest of the corpus is not re-linted. The
    record's warnings are printed, with open concerns first: shown at
    acceptance, not blocking.
    """
    adr, code = _lifecycle_target(args)
    if adr is None:
        return code
    trial, new = _trial(adr, 'accepted')
    if new:
        return _refuse(adr, 'accepting', new)
    concerns = [i for i in trial.issues if i.code == 'open-concern']
    others = [i for i in trial.issues if i.severity == 'warning' and i.code != 'open-concern']
    if concerns:
        print(f"Open concerns on ADR-{adr.number} at acceptance (shown, not blocking):")
        for issue in concerns:
            print(f"  ⚠️ {issue.message[len('open concern: '):]}")
    for issue in others:
        print(f"  ⚠️ {issue.message}")
    if args.dry_run:
        print(f"Would accept ADR-{adr.number}: {adr.title}")
        return 0
    error = _write_status(adr, 'accepted')
    if error:
        print(f"Error: ADR-{adr.number} not changed: {error}", file=sys.stderr)
        return 1
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
        article = 'an' if status[0] in 'aeiou' else 'a'
        print(f"Error: --reason is required; {article} {status} record states why.", file=sys.stderr)
        return 1
    _, new = _trial(adr, status)
    if new:
        return _refuse(adr, 'marking' if status == 'abandoned' else 'rejecting', new)
    if args.dry_run:
        print(f"Would mark ADR-{adr.number} {status}: {reason}")
        return 0
    # Appended, never inserted: a closed record stays append-only.
    closure = f"\n## Closure\n\n{status.capitalize()} {date.today().isoformat()}: {reason}\n"
    error = _write_status(adr, status, closure)
    if error:
        print(f"Error: ADR-{adr.number} not changed: {error}", file=sys.stderr)
        return 1
    print(f"Marked ADR-{adr.number} {status}: {adr.title}")
    print(f"Run `adr index -y` to refresh INDEX.md.")
    return 0

def cmd_reject(args):
    return _close(args, 'rejected')

def cmd_abandon(args):
    return _close(args, 'abandoned')

