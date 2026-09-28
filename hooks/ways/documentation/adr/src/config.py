# ============================================================================
# Configuration
# ============================================================================

def get_project_root() -> Path:
    """Find project root via git or by walking up to find docs/architecture/."""
    # Try git root first
    try:
        result = subprocess.run(
            ['git', 'rev-parse', '--show-toplevel'],
            capture_output=True, text=True, timeout=5
        )
        if result.returncode == 0:
            return Path(result.stdout.strip())
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass

    # Walk up from cwd looking for docs/architecture/adr.yaml
    candidate = Path.cwd()
    for _ in range(10):
        if (candidate / 'docs' / 'architecture' / 'adr.yaml').exists():
            return candidate
        if candidate.parent == candidate:
            break
        candidate = candidate.parent

    # Last resort: assume cwd
    return Path.cwd()


def _git(args: list, cwd: Path) -> Optional[str]:
    """git's output, or None when git is missing, fails or times out."""
    try:
        result = subprocess.run(['git', '-c', 'core.quotePath=false', *args], cwd=cwd,
                                capture_output=True, encoding='utf-8', errors='replace', timeout=10)
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return None
    return result.stdout if result.returncode == 0 else None


def get_config_path() -> Path:
    """Get path to adr.yaml config file."""
    return get_project_root() / 'docs' / 'architecture' / 'adr.yaml'

# YAML 1.1 reads a plain key such as `yes`, `on` or `2024` as a boolean, a
# number or a date. Every key in adr.yaml is a name, so a plain key with no
# explicit tag keeps the text it was written with. The key is replaced by a
# new node: a node shared through an anchor keeps its own type elsewhere.
_NAME_KEY_TAGS = {f'tag:yaml.org,2002:{t}' for t in ('bool', 'int', 'float', 'null', 'timestamp')}
_STR_TAG = 'tag:yaml.org,2002:str'

class _ConfigLoader(yaml.SafeLoader):
    def compose_scalar_node(self, anchor):
        event = self.peek_event()
        node = super().compose_scalar_node(anchor)
        node.plain_implicit = event.tag in (None, '!') and event.style is None
        return node

def _name_key(key):
    if isinstance(key, yaml.ScalarNode) and getattr(key, 'plain_implicit', False) \
            and key.tag in _NAME_KEY_TAGS:
        return yaml.ScalarNode(_STR_TAG, key.value, key.start_mark, key.end_mark)
    return key

def _keys_as_names(node, seen=None) -> None:
    seen = set() if seen is None else seen
    if id(node) in seen:
        return
    seen.add(id(node))
    if isinstance(node, yaml.MappingNode):
        node.value = [(_name_key(key), value) for key, value in node.value]
        for _, value in node.value:
            _keys_as_names(value, seen)
    elif isinstance(node, yaml.SequenceNode):
        for item in node.value:
            _keys_as_names(item, seen)

def _read_config(stream):
    """adr.yaml's content, its keys read as names; None for an empty file."""
    loader = _ConfigLoader(stream)
    try:
        node = loader.get_single_node()
        if node is None:
            return None
        _keys_as_names(node)
        return loader.construct_document(node)
    finally:
        loader.dispose()

def load_config() -> dict:
    """Load configuration from adr.yaml. An empty file is an empty config."""
    config_path = get_config_path()

    if not config_path.exists():
        print(f"Error: Config not found: {config_path}", file=sys.stderr)
        print("Run from project root or create docs/architecture/adr.yaml", file=sys.stderr)
        sys.exit(1)

    try:
        with open(config_path) as f:
            config = _read_config(f)
    except yaml.YAMLError as e:
        print(f"Error: Invalid YAML in config: {e}", file=sys.stderr)
        sys.exit(1)
    if config is None:
        config = {}
    if not isinstance(config, dict):
        print("Error: Config is not a mapping of keys to values", file=sys.stderr)
        sys.exit(1)

    # Validate required fields
    if config.get('domains') is None:
        print("Error: Config missing 'domains' section", file=sys.stderr)
        sys.exit(1)
    if not isinstance(config['domains'], dict):
        print("Error: Config 'domains' section is not a mapping of domain names", file=sys.stderr)
        sys.exit(1)

    # Convert range lists to tuples for easier use
    for domain, cfg in config['domains'].items():
        if isinstance(cfg, dict) and isinstance(cfg.get('range'), list):
            cfg['range'] = tuple(cfg['range'])

    legacy = config.get('legacy')
    if isinstance(legacy, dict) and isinstance(legacy.get('range'), list):
        legacy['range'] = tuple(legacy['range'])

    return config

# Global config (loaded on first access)
_config = None

def get_config() -> dict:
    """Get cached config."""
    global _config
    if _config is None:
        _config = load_config()
    return _config

def reload_config() -> dict:
    """Drop the cached config and read adr.yaml again, after a command
    edits it."""
    global _config
    _config = None
    return get_config()

def repo_contract() -> str:
    """The contract adr.yaml declares; adr/v0 when it declares none (ADR-304)."""
    return str(get_config().get('contract') or 'adr/v0')

def contract_rank(contract) -> Optional[int]:
    """A contract's place in KNOWN_CONTRACTS, oldest first; None when the
    tool does not know it. A record with no contract is adr/v0."""
    name = str(contract) if contract else 'adr/v0'
    return KNOWN_CONTRACTS.index(name) if name in KNOWN_CONTRACTS else None

def get_domains() -> dict:
    """Get domain configuration. An entry that is not a mapping reads as an
    empty one; lint's domain-shape rule names it."""
    return {name: cfg if isinstance(cfg, dict) else {}
            for name, cfg in get_config().get('domains', {}).items()}

def domain_range(cfg: dict) -> Optional[tuple]:
    """A domain's (low, high), or None when adr.yaml gives no usable range."""
    r = cfg.get('range')
    if isinstance(r, (list, tuple)) and len(r) == 2 \
            and all(isinstance(n, int) and not isinstance(n, bool) for n in r):
        return tuple(r)
    return None

def domain_folders(cfg: dict) -> list:
    """A domain's folders, first the primary; empty when adr.yaml names none."""
    folders = cfg.get('folder')
    folders = folders if isinstance(folders, list) else [folders]
    return [f for f in folders if isinstance(f, str) and f]

def name_list(value) -> Optional[list]:
    """value as a list of strings, or None when it is not one."""
    if isinstance(value, list) and all(isinstance(v, str) for v in value):
        return value
    return None

def config_section(key: str) -> dict:
    """A top-level mapping in adr.yaml; empty when absent or not a mapping.
    Lint's config-shape rule names a malformed one."""
    value = get_config().get(key)
    return value if isinstance(value, dict) else {}

def get_statuses() -> set:
    """Get valid statuses."""
    statuses = name_list(get_config().get('statuses'))
    if statuses is None:
        statuses = ['Draft', 'Proposed', 'Accepted', 'Superseded', 'Deprecated']
    return set(statuses)

def relative_path(path: Path, base: Path = None) -> Path:
    """Get path relative to base, or return absolute if not possible."""
    if base is None:
        base = Path.cwd()
    try:
        return path.relative_to(base)
    except ValueError:
        return path

def get_defaults() -> dict:
    """Get default values for new ADRs. A deciders or status value of the
    wrong type is left out, so the tool's own default applies."""
    if get_config().get('defaults') is None:
        return {'deciders': [], 'status': 'Draft'}
    defaults = dict(config_section('defaults'))
    if 'deciders' in defaults and name_list(defaults['deciders']) is None:
        del defaults['deciders']
    if 'status' in defaults and not isinstance(defaults['status'], str):
        del defaults['status']
    return defaults

def get_legacy_range() -> tuple:
    """Get legacy ADR number range."""
    return domain_range(config_section('legacy')) or (1, 99)

def get_viewer() -> str:
    """The command `adr view` runs, {file} for the record."""
    viewer = get_config().get('viewer')
    return viewer if isinstance(viewer, str) and viewer.split() else 'cat {file}'

def cite_excludes() -> list:
    """adr.yaml's cite.exclude paths; empty when it lists none."""
    exclude = config_section('cite').get('exclude')
    return [str(p) for p in exclude] if isinstance(exclude, list) else []

def _detect_git_user() -> Optional[str]:
    """Detect current git user (GitHub username or git config name)."""
    # Try GitHub username from gh CLI
    try:
        result = subprocess.run(
            ['gh', 'api', 'user', '--jq', '.login'],
            capture_output=True, text=True, timeout=5)
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.strip()
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    # Fall back to git config user.name
    try:
        result = subprocess.run(
            ['git', 'config', 'user.name'],
            capture_output=True, text=True, timeout=5)
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.strip()
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    return None

