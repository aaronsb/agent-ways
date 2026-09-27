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


def get_config_path() -> Path:
    """Get path to adr.yaml config file."""
    return get_project_root() / 'docs' / 'architecture' / 'adr.yaml'

def load_config() -> dict:
    """Load configuration from adr.yaml."""
    config_path = get_config_path()

    if not config_path.exists():
        print(f"Error: Config not found: {config_path}", file=sys.stderr)
        print("Run from project root or create docs/architecture/adr.yaml", file=sys.stderr)
        sys.exit(1)

    try:
        with open(config_path) as f:
            config = yaml.safe_load(f)
    except yaml.YAMLError as e:
        print(f"Error: Invalid YAML in config: {e}", file=sys.stderr)
        sys.exit(1)

    # Validate required fields
    if 'domains' not in config:
        print("Error: Config missing 'domains' section", file=sys.stderr)
        sys.exit(1)

    # Convert range lists to tuples for easier use
    for domain, cfg in config.get('domains', {}).items():
        if 'range' in cfg and isinstance(cfg['range'], list):
            cfg['range'] = tuple(cfg['range'])

    if 'legacy' in config and 'range' in config['legacy']:
        if isinstance(config['legacy']['range'], list):
            config['legacy']['range'] = tuple(config['legacy']['range'])

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

def get_domains() -> dict:
    """Get domain configuration."""
    return get_config().get('domains', {})

def get_statuses() -> set:
    """Get valid statuses."""
    return set(get_config().get('statuses', ['Draft', 'Proposed', 'Accepted', 'Superseded', 'Deprecated']))

def relative_path(path: Path, base: Path = None) -> Path:
    """Get path relative to base, or return absolute if not possible."""
    if base is None:
        base = Path.cwd()
    try:
        return path.relative_to(base)
    except ValueError:
        return path

def get_defaults() -> dict:
    """Get default values for new ADRs."""
    return get_config().get('defaults', {'deciders': [], 'status': 'Draft'})

def get_legacy_range() -> tuple:
    """Get legacy ADR number range."""
    legacy = get_config().get('legacy', {})
    return legacy.get('range', (1, 99))

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

