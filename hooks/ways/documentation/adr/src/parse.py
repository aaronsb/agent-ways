# ============================================================================
# Parsing
# ============================================================================

TITLE_PATTERN = re.compile(r'^# ADR-(\d+(?:\.\d+)?): (.+)$')

def parse_adr(path: Path) -> ADRInfo:
    """Parse an ADR file and extract metadata."""
    try:
        content = path.read_text()
    except Exception as e:
        info = ADRInfo(path=path)
        info.issues.append(Issue(f"Cannot read: {e}", 'error'))
        return info
    return parse_text(content, path)

def parse_text(content: str, path: Path) -> ADRInfo:
    """Parse ADR text; path places it (domain by folder) and names it."""
    info = ADRInfo(path=path)
    lines = content.split('\n')

    # Parse YAML frontmatter
    has_frontmatter = lines and lines[0].strip() == '---'
    info.has_frontmatter = bool(has_frontmatter)
    if has_frontmatter:
        end_idx = None
        for i, line in enumerate(lines[1:], 1):
            if line.strip() == '---':
                end_idx = i
                break

        if end_idx:
            yaml_content = '\n'.join(lines[1:end_idx])
            try:
                data = yaml.safe_load(yaml_content) or {}
                if not isinstance(data, dict):
                    info.issues.append(Issue(
                        f"Frontmatter is a YAML {type(data).__name__}, not a mapping of fields", 'error'))
                    data = {}
                info.frontmatter = data
                contract = data.get('contract')
                info.contract = str(contract) if contract else None
                info.status = data.get('status')
                info.date = str(data.get('date', '')) if data.get('date') else None
                deciders = data.get('deciders', [])
                info.deciders = deciders if isinstance(deciders, list) else [deciders]
                info.related = data.get('related', [])

                def as_list(value):
                    if value is None:
                        return []
                    return value if isinstance(value, list) else [value]
                info.supersedes = [str(v) for v in as_list(data.get('supersedes'))]
                info.superseded_by = [str(v) for v in as_list(data.get('superseded_by'))]
            except yaml.YAMLError as e:
                info.issues.append(Issue(f"YAML error: {e}", 'error'))
        else:
            info.issues.append(Issue("Opening --- found but no closing --- for frontmatter", 'error'))
    else:
        # Check for inline metadata (pre-YAML pattern)
        inline_meta = any(re.match(r'^(Status|Date|Deciders):\s', line) for line in lines[:15])
        if inline_meta:
            info.issues.append(Issue(
                "No YAML frontmatter — found inline metadata (Status:/Date:/Deciders:). "
                "Convert to YAML frontmatter: wrap in --- delimiters, use lowercase keys",
                'error'))
        else:
            info.issues.append(Issue("No YAML frontmatter found", 'error'))

    # Heading texts in the body, for section references (ADR-104#2). The
    # frontmatter and fenced code blocks are skipped: a heading inside a code
    # sample is not a section.
    body_start = 0
    if has_frontmatter:
        for i, line in enumerate(lines[1:], 1):
            if line.strip() == '---':
                body_start = i + 1
                break
    # A ### heading is a section of its own, and its text also belongs to the
    # enclosing ## section, so a Summary with ### Probes still reads whole.
    in_fence = False
    current = parent = None
    info.body = '\n'.join(lines[body_start:])
    for line in lines[body_start:]:
        if line.lstrip().startswith(('```', '~~~')):
            in_fence = not in_fence
        elif not in_fence and (line.startswith('## ') or line.startswith('### ')):
            current = line.lstrip('#').strip()
            info.sections.append(current)
            info.section_text.setdefault(current, '')
            if line.startswith('## '):
                parent = current
            elif parent is not None:
                info.section_text[parent] += line + '\n'
            continue
        if current is not None:
            info.section_text[current] += line + '\n'
        if parent is not None and parent != current:
            info.section_text[parent] += line + '\n'

    # Find title
    for line in lines:
        match = TITLE_PATTERN.match(line)
        if match:
            info.number = match.group(1)
            info.title = match.group(2)
            break

    # Determine the domain. Under adr/v0 the number range is authoritative and
    # the folder is the fallback: a number definitively places a record, even
    # in another domain's folder. Under adr/v1 a number is only an identity,
    # never renumbered (ADR-306 §6): the folder decides, and the range, which
    # allocates new numbers, is the fallback for a folder no domain names.
    def domain_by_range():
        if info.number:
            try:
                base_num = int(info.number.split('.')[0])
            except ValueError:
                return None
            for domain, config in get_domains().items():
                if config['range'][0] <= base_num <= config['range'][1]:
                    return domain
        return None

    def domain_by_folder():
        folder_name = path.parent.name
        for domain, config in get_domains().items():
            folders = config['folder']
            if isinstance(folders, str):
                folders = [folders]
            if folder_name in folders:
                return domain
        return None

    if repo_contract() == 'adr/v1':
        info.domain = domain_by_folder() or domain_by_range()
    else:
        info.domain = domain_by_range() or domain_by_folder()

    return info

def is_archived(path: Path) -> bool:
    """True if the path sits under docs/architecture/archive/.

    Only segments below the 'architecture' directory count, so a repo that
    itself lives under a directory named 'archive' is unaffected. The
    archive/ name is reserved: a domain must not use it as its folder.
    """
    parts = path.parts
    for i in range(len(parts) - 1, -1, -1):
        if parts[i] == 'architecture':
            return 'archive' in parts[i + 1:]
    return 'archive' in parts

def find_adrs(include_archived: bool = False) -> list[Path]:
    """Find ADR files — the active set by default (ADR-303)."""
    arch_dir = get_project_root() / 'docs' / 'architecture'
    paths = sorted(arch_dir.rglob("ADR-*.md"))
    if include_archived:
        return paths
    return [p for p in paths if not is_archived(p)]

def get_all_adrs(include_archived: bool = False) -> list[ADRInfo]:
    """Parse ADR files — the active set by default."""
    return [parse_adr(p) for p in find_adrs(include_archived=include_archived)]

