# ============================================================================
# Contract (#614)
# ============================================================================
#
# `adr contract` names the contract adr.yaml declares and the one this tool
# writes. `--upgrade` brings adr.yaml to the current contract by editing
# lines: the contract line is changed or appended, and each top-level key the
# contract needs and the file lacks is appended as adr.yaml.template writes
# it. A config already on the current contract gets only its missing blocks.
# Comments and every other line are left as they were. A capabilities block
# written into a config with domains holds one capability per domain in
# place of the placeholder (ADR-312).

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
# `core` placeholder with the project's own list. `process` and `adr` apply
# to every project that keeps these records.
capabilities:
  core: The project's core behaviour (placeholder; replace)
  process: Conventions, documentation and the development process that belong to no product capability
  adr: Decision records, their contract and the adr tool
'''),
    ),
}

# The capabilities every project has (ADR-312). The template ships them after
# the placeholder, and --upgrade writes them after the capabilities it seeds
# from domains.
DEFAULT_CAPABILITIES = ('process', 'adr')

# The comment above a capabilities block seeded from domains (ADR-312).
SEEDED_CAPABILITIES_COMMENT = '''# What the project does, one line each. A record's `capability` names one of
# these, and a decision adds, cuts, changes or constrains them.
# `adr contract --upgrade` seeded the entries before `process` from the
# domains in this file. Each may be kept, split, renamed or deleted.
'''

# A top-level contract line: key, optional quotes, value, optional comment.
CONTRACT_LINE_RE = re.compile(
    r'^(contract:[ \t]*)(["\']?)([A-Za-z0-9/._-]*)\2([ \t]*(?:#.*)?)$')


def cmd_contract(args):
    """adr contract [--current | --upgrade [--dry-run]]."""
    dry_run = getattr(args, 'dry_run', False)
    if dry_run and not args.upgrade:
        print("Error: --dry-run applies to `adr contract --upgrade`.", file=sys.stderr)
        return 1
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
        missing = _missing_blocks(get_config())
        if declared == CURRENT_CONTRACT and missing:
            print(f"{config_path} declares {CURRENT_CONTRACT} but lacks {' and '.join(missing)}. "
                  f"`adr contract --upgrade` adds {'it' if len(missing) == 1 else 'them'}.")
        elif declared == CURRENT_CONTRACT:
            print(f"{config_path} is current.")
        else:
            print(f"{config_path} is behind this tool. "
                  f"`adr contract --upgrade` brings it to {CURRENT_CONTRACT}.")
        for finding in shape_findings(vocabulary_shape(get_all_adrs()), get_config()):
            print(f"Notice: {shape_notice(finding, seeds='capabilities' in missing)}")
        return 0
    return _contract_upgrade(declared, dry_run)


def _missing_blocks(config: dict) -> list:
    """The top-level keys the current contract needs that config lacks,
    other than the contract line itself."""
    return [key for key, _ in CONTRACT_BLOCKS.get(CURRENT_CONTRACT, ())
            if key != 'contract' and key not in config]


def _yaml_scalar(text, as_key: bool = False) -> str:
    """text as a one-line YAML scalar: plain when it reads back unchanged,
    double-quoted otherwise."""
    text = ' '.join(str(text).split())
    probe, want = (f"{text}: x", {text: 'x'}) if as_key else (f"k: {text}", {'k': text})
    try:
        if text and yaml.safe_load(probe) == want:
            return text
    except yaml.YAMLError:
        pass
    return json.dumps(text, ensure_ascii=False)


def _seeded_capabilities(config: dict):
    """The capabilities block --upgrade writes (ADR-312): one entry per domain,
    then the defaults. Returns the block, the domains seeded, and the domains
    not seeded because a default capability has their name. With no domain
    to seed, the block is the template's."""
    template_block = dict(CONTRACT_BLOCKS[CURRENT_CONTRACT])['capabilities']
    domains = config.get('domains')
    seeds, taken = [], []
    for key, cfg in (domains.items() if isinstance(domains, dict) else ()):
        name = str(key)
        if name in DEFAULT_CAPABILITIES:
            taken.append(name)
            continue
        cfg = cfg if isinstance(cfg, dict) else {}
        seeds.append((name, cfg.get('description') or cfg.get('name') or name))
    if not seeds:
        return template_block, [], taken
    defaults = [line for line in template_block.splitlines(keepends=True)
                if any(line.startswith(f"  {name}: ") for name in DEFAULT_CAPABILITIES)]
    body = ''.join(f"  {_yaml_scalar(name, as_key=True)}: {_yaml_scalar(text)}\n"
                   for name, text in seeds)
    block = SEEDED_CAPABILITIES_COMMENT + 'capabilities:\n' + body + ''.join(defaults)
    return block, [name for name, _ in seeds], taken


def _contract_upgrade(declared: str, dry_run: bool = False) -> int:
    path = get_config_path()
    shown = relative_path(path)
    if declared == CURRENT_CONTRACT and not _missing_blocks(get_config()):
        print(f"{shown} already declares {CURRENT_CONTRACT} and has the blocks it needs; nothing to do.")
        return 0
    original = path.read_bytes()
    text = original.decode('utf-8')
    newline = '\r\n' if '\r\n' in text else '\n'
    lines = text.splitlines(keepends=True)
    config = get_config()
    added = []
    replaced = None
    for i, line in enumerate(lines):
        if declared == CURRENT_CONTRACT:
            break
        match = CONTRACT_LINE_RE.match(line.rstrip('\r\n'))
        if match:
            ending = line[len(line.rstrip('\r\n')):]
            prefix, quote, _, suffix = match.groups()
            if prefix == 'contract:':
                prefix += ' '  # an empty `contract:` line has no space after the colon
            lines[i] = f"{prefix}{quote}{CURRENT_CONTRACT}{quote}{suffix}{ending}"
            replaced = i
            break
    if 'contract' in config and replaced is None and declared != CURRENT_CONTRACT:
        print(f"Error: {shown} sets contract in a form this command does not edit; "
              f"change it to `contract: {CURRENT_CONTRACT}` by hand.", file=sys.stderr)
        return 1
    blocks = []
    seeded, taken = [], []
    for key, block in CONTRACT_BLOCKS[CURRENT_CONTRACT]:
        if key == 'contract':
            if replaced is not None:
                added.append(f"changed: contract: {declared} -> {CURRENT_CONTRACT}")
                continue
            if declared == CURRENT_CONTRACT:
                continue
        elif key in config:
            continue
        elif key == 'capabilities':
            block, seeded, taken = _seeded_capabilities(config)
        blocks.append(block.replace('\n', newline))
        added.append(f"added: contract: {CURRENT_CONTRACT}" if key == 'contract' else f"added: {key}")
    if lines and not lines[-1].endswith('\n'):
        lines[-1] += newline
    body = ''.join(lines)
    for block in blocks:
        body += newline + block
    # The domain shape, read before the upgrade, says whether the seeds are thin.
    thin = vocabulary_shape(get_all_adrs())['domain'] if 'added: capabilities' in added else None
    if dry_run:
        return _contract_upgrade_preview(shown, declared, body, lines, replaced, blocks,
                                         added, seeded, taken, thin)
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
    if declared == CURRENT_CONTRACT:
        print(f"{shown}: {CURRENT_CONTRACT}, completed")
    else:
        print(f"{shown}: {declared} -> {CURRENT_CONTRACT}")
    for line in added:
        print(f"  {line}")
    _print_capability_notes(added, seeded, taken, thin)
    print("Run `adr lint` to check the records against the contract.")
    return 0


def _print_capability_notes(added: list, seeded: list, taken: list, thin=None):
    defaults = ' and '.join(DEFAULT_CAPABILITIES)
    if seeded:
        print(f"Capabilities seeded from domains: {', '.join(seeded)}. They are a starting point; "
              "the operator or the agent may keep, split, rename or delete each one.")
        print(f"Capabilities from the template: {defaults}.")
    elif 'added: capabilities' in added:
        print(f"The capabilities list holds the placeholder core, with {defaults}. "
              "Replace core with the project's own list.")
    for name in taken:
        print(f"Domain {name} is not seeded: the capability {name} keeps the template's text.")
    if thin:
        print(f"Notice: {shape_notice(thin, seeds=True)}")


def _contract_upgrade_preview(shown, declared, body, lines, replaced, blocks,
                              added, seeded, taken, thin) -> int:
    """--upgrade --dry-run: print the lines the upgrade would write, and
    write nothing."""
    try:
        loaded = yaml.safe_load(body)
    except yaml.YAMLError:
        loaded = None
    if not isinstance(loaded, dict) or str(loaded.get('contract')) != CURRENT_CONTRACT:
        print(f"Error: the upgraded {shown} would not read as {CURRENT_CONTRACT}; "
              "the upgrade would not be written.", file=sys.stderr)
        return 1
    if declared == CURRENT_CONTRACT:
        print(f"{shown}: {CURRENT_CONTRACT}, would be completed (dry run; nothing written)")
    else:
        print(f"{shown}: {declared} -> {CURRENT_CONTRACT} (dry run; nothing written)")
    for line in added:
        print(f"  would be {line}")
    if replaced is not None:
        print(f"\nLine {replaced + 1} would read:\n")
        print(lines[replaced].rstrip('\r\n'))
    if blocks:
        print("\nAppended to the end of the file:")
        for block in blocks:
            print()
            print(block.rstrip('\r\n').replace('\r\n', '\n'))
        print()
    _print_capability_notes(added, seeded, taken, thin)
    print("Run `adr contract --upgrade` without --dry-run to write it.")
    return 0
