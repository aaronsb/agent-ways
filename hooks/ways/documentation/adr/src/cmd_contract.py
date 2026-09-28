# ============================================================================
# Contract (#614)
# ============================================================================
#
# `adr contract` names the contract adr.yaml declares and the one this tool
# writes. `--upgrade` brings adr.yaml to the current contract by editing
# lines: the contract line is changed or appended, and each top-level key the
# contract needs and the file lacks is appended as adr.yaml.template writes
# it. Comments and every other line are left as they were.

# What adr.yaml needs for each contract past adr/v0, in the order it is
# appended: (top-level key, block). The text matches adr.yaml.template, and
# tests/adr-template-test.sh checks that it still does.
CONTRACT_BLOCKS = {
    'adr/v1': (
        ('contract', '''# The record contract (ADR-304). With `contract: adr/v1`, records are typed:
# each declares a kind, and a decision names a verb, a capability and its
# basis. Delete this line and the v1 blocks below to stay on adr/v0.
contract: adr/v1
'''),
        ('kinds', '''kinds:
  decision:
    verb: required
    requires: [capability, basis, agent]
    sections: [Summary]
    edges: { supersedes: decision, amends: decision, extends: decision, basis: [decision, spec, evidence] }
  spec:
    verb: forbidden
    requires: [capability]
    edges: { supersedes: spec, decided_by: decision }
  evidence:
    verb: forbidden
    requires: [capability]
    edges: { supersedes: evidence }
'''),
        ('capabilities', '''# What the project does, one line each. A record's `capability` names one of
# these, and a decision adds, cuts, changes or constrains them. Replace the
# placeholder with the project's own list.
capabilities:
  core: The project's core behaviour (placeholder; replace)
'''),
    ),
}

# A top-level contract line: key, optional quotes, value, optional comment.
CONTRACT_LINE_RE = re.compile(
    r'^(contract:[ \t]*)(["\']?)([A-Za-z0-9/._-]*)\2([ \t]*(?:#.*)?)$')


def cmd_contract(args):
    """adr contract [--current | --upgrade]."""
    if args.current:
        print(CURRENT_CONTRACT)
        return 0
    declared = repo_contract()
    config_path = relative_path(get_config_path())
    if contract_rank(declared) is None:
        print(f"Error: {config_path} declares contract: {declared}, which this tool does not know "
              f"(it knows {', '.join(KNOWN_CONTRACTS)}). Check the value.", file=sys.stderr)
        return 1
    if not args.upgrade:
        note = '' if get_config().get('contract') else ' (no contract line)'
        print(f"Declared: {declared}{note}")
        print(f"Current:  {CURRENT_CONTRACT} (adr-tool {TOOL_VERSION})")
        if declared == CURRENT_CONTRACT:
            print(f"{config_path} is current.")
        else:
            print(f"{config_path} is behind this tool. "
                  f"`adr contract --upgrade` brings it to {CURRENT_CONTRACT}.")
        return 0
    return _contract_upgrade(declared)


def _contract_upgrade(declared: str) -> int:
    path = get_config_path()
    shown = relative_path(path)
    if declared == CURRENT_CONTRACT:
        print(f"{shown} already declares {CURRENT_CONTRACT}; nothing to do.")
        return 0
    original = path.read_bytes()
    text = original.decode('utf-8')
    newline = '\r\n' if '\r\n' in text else '\n'
    lines = text.splitlines(keepends=True)
    config = get_config()
    added = []
    replaced = False
    for i, line in enumerate(lines):
        match = CONTRACT_LINE_RE.match(line.rstrip('\r\n'))
        if match:
            ending = line[len(line.rstrip('\r\n')):]
            prefix, quote, _, suffix = match.groups()
            lines[i] = f"{prefix}{quote}{CURRENT_CONTRACT}{quote}{suffix}{ending}"
            replaced = True
            break
    if 'contract' in config and not replaced:
        print(f"Error: {shown} sets contract in a form this command does not edit; "
              f"change it to `contract: {CURRENT_CONTRACT}` by hand.", file=sys.stderr)
        return 1
    blocks = []
    for key, block in CONTRACT_BLOCKS[CURRENT_CONTRACT]:
        if key == 'contract':
            if replaced:
                added.append(f"changed: contract: {declared} -> {CURRENT_CONTRACT}")
                continue
        elif key in config:
            continue
        blocks.append(block.replace('\n', newline))
        added.append(f"added: contract: {CURRENT_CONTRACT}" if key == 'contract' else f"added: {key}")
    if lines and not lines[-1].endswith('\n'):
        lines[-1] += newline
    body = ''.join(lines)
    for block in blocks:
        body += newline + block
    try:
        path.write_bytes(body.encode('utf-8'))
        reloaded = reload_config()
    except SystemExit:
        path.write_bytes(original)
        print(f"Error: the upgraded {shown} did not load; it is restored unchanged.", file=sys.stderr)
        return 1
    if str(reloaded.get('contract')) != CURRENT_CONTRACT:
        path.write_bytes(original)
        print(f"Error: the upgraded {shown} does not read as {CURRENT_CONTRACT}; "
              "it is restored unchanged.", file=sys.stderr)
        return 1
    print(f"{shown}: {declared} -> {CURRENT_CONTRACT}")
    for line in added:
        print(f"  {line}")
    if 'added: capabilities' in added:
        print("The capabilities list holds one placeholder. Replace it with the project's own list.")
    print("Run `adr lint` to check the records against the new contract.")
    return 0
