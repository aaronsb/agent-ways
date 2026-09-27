# --- adr import: readers and the sheet file (ADR-306) ---------------------------------
#
# A reader turns one source record into one import sheet (ADR-306 §1, §2).
# It fills what the source states and lists the rest in `todo`. It never
# writes an operator basis, `considered` or `concern`: `deciders` names who
# signed the record, not what they said (ADR-304 §7, §11).

IMPORT_DIR = ('docs', 'architecture', '.import')

SHEET_HEADER = ('# adr import sheet (ADR-306). Fill or clear each todo item, then run\n'
                '# `adr import apply`. Sheets are working files; only records are committed.\n')

# ADR-304 §7: the v0 status table. Deprecated depends on the record.
V0_STATUS_MAP = {'draft': 'proposed', 'proposed': 'proposed', 'accepted': 'accepted',
                 'superseded': 'superseded', 'rejected': 'rejected'}

# v0 keys that carry over under the same name and meaning.
V0_CARRIED = ('date', 'deciders', 'related', 'supersedes', 'superseded_by', 'amends')

# Todo items that lint cannot detect once the record is written. They block
# --partial; every other item is advisory (ADR-306 §3).
BLOCKING_TODO = ('status', 'status note', 'target.number', 'target.domain')

# The v1 default decision (ADR-304 §1), for a project whose adr.yaml declares no kinds.
IMPORT_DECISION_SCHEMA = {'verb': 'required', 'requires': ['capability', 'basis', 'agent'],
                          'sections': ['Summary']}

_STOPWORDS = frozenset('''
    the and for with that this from into over under onto are was were been being has have
    had not but its it's our their them they these those which what when where who whom
    why how all any each every one two per via than then there here also only more most
    such can could should would will may might must shall does did doing done use used
    using uses make makes made new adr record records decision decisions context
'''.split())

class SheetError(Exception):
    """A source a reader cannot turn into a sheet, or a sheet apply cannot use."""

def import_dir() -> Path:
    return get_project_root().joinpath(*IMPORT_DIR)

def sheet_filename(number) -> str:
    return f"ADR-{format_number(number)}.yaml"

def _number_value(text: str) -> str:
    """A record number as the sheet holds it: a string, '42' or '101.1', so
    YAML never reads a zero-padded number as octal."""
    base, _, part = text.partition('.')
    return (base.lstrip('0') or '0') + (f".{part}" if part else '')

# --- splitting a record into frontmatter, title, Summary and body -------------------

def split_record(text: str) -> tuple:
    """(frontmatter text or None, text before the H1, H1 match or None, text
    after the H1 line). Joined back with the H1 line, the parts are the file."""
    lines = text.split('\n')
    front, start = None, 0
    if lines and lines[0].strip() == '---':
        for i in range(1, len(lines)):
            if lines[i].strip() == '---':
                front, start = '\n'.join(lines[1:i]), i + 1
                break
    for j in range(start, len(lines)):
        match = TITLE_PATTERN.match(lines[j])
        if match:
            return front, '\n'.join(lines[start:j]), match, '\n'.join(lines[j + 1:])
    return front, '\n'.join(lines[start:]), None, ''

def _render_tail(summary: Optional[str], body: str) -> str:
    """What render_record writes after the H1 line for this summary and body."""
    tail = ''
    if summary:
        summary = summary if summary.startswith('## Summary') else f"## Summary\n\n{summary}"
        tail += '\n' + summary.rstrip('\n') + '\n'
    if body:
        tail += '\n' + body
    return tail

def split_summary(after: str) -> tuple:
    """(summary, body, note) for the text after the H1. A `## Summary` that
    opens the body becomes the summary; the rest is the body, verbatim. The
    split is kept only when render_record writes the same text back, so an
    unedited sheet keeps the body byte-identical (ADR-306 §7). Otherwise the
    Summary stays in the body and note says why."""
    body = after[1:] if after.startswith('\n') else after
    lines = body.split('\n')
    if lines[0].strip() != '## Summary':
        note = 'the Summary is not the first section; kept in the body' if re.search(
            r'(?m)^## Summary\s*$', body) else None
        return None, body, note
    fence, end = None, len(lines)
    for k in range(1, len(lines)):
        marker = re.match(r'\s*(```|~~~)', lines[k])
        if marker:
            fence = None if fence == marker.group(1) else (fence or marker.group(1))
        elif fence is None and lines[k].startswith('## '):
            end = k
            break
    section = '\n'.join(lines[:end])
    rest = '\n'.join(lines[end:])
    summary = section[len('## Summary\n\n'):] if section.startswith('## Summary\n\n') else section
    if summary.strip() and _render_tail(summary, rest) == after:
        return summary, rest, None
    return None, body, 'the Summary does not split cleanly from the body; kept in the body'

# --- what a reader decides ------------------------------------------------------------

def _empty(value) -> bool:
    return value in (None, '', [], {})

def open_fields(record: dict, schema: dict) -> list:
    """The fields the kind requires that the record leaves empty, in the
    order a record is written: the reader's part of `todo` (ADR-306 §1)."""
    wanted = (['verb'] if schema.get('verb') == 'required' else []) + v1_requires(schema)
    found = []
    for name in wanted:
        value = record.get(name)
        if name == 'agent' and isinstance(value, dict):
            found += [f"agent.{k}" for k in ('name', 'model') if _empty(value.get(k))]
        elif _empty(value):
            found.append(name)
    return found

def _words(text: str) -> set:
    return {_stem(w) for w in re.findall(r"[a-z][a-z0-9']+", text.lower())
            if len(w) >= 3 and w not in _STOPWORDS}

def capability_candidates(text: str, vocabulary: dict, top: int = 3) -> list:
    """Vocabulary names ranked by how many words the record's title and
    Context share with each capability's name and description (ADR-306 §1).
    A word counts less the more descriptions carry it, so a word every
    capability mentions does not decide the ranking."""
    words = _words(text)
    described = [(str(name), _words(f"{name} {description or ''}"))
                 for name, description in vocabulary.items()]
    spread = {}
    for _, found in described:
        for word in found:
            spread[word] = spread.get(word, 0) + 1
    scored = []
    for i, (name, found) in enumerate(described):
        overlap = round(sum(1 / spread[w] for w in words & found), 6)
        if overlap:
            scored.append((-overlap, i, name))
    return [name for _, _, name in sorted(scored)[:top]]

def _folder_domain(path: Path) -> Optional[str]:
    """The domain whose folder holds the file, or 'legacy' for the legacy folder."""
    name = path.parent.name
    for domain, config in get_domains().items():
        folders = config.get('folder')
        if name in (folders if isinstance(folders, list) else [folders]):
            return domain
    if name == 'legacy' and 'legacy' in get_config():
        return 'legacy'
    return None

def number_range(domain: str) -> Optional[tuple]:
    """(low, high) for a domain in adr.yaml, or the legacy range."""
    if domain in get_domains():
        return tuple(get_domains()[domain].get('range', (0, -1)))
    if domain == 'legacy' and 'legacy' in get_config():
        return tuple(get_legacy_range())
    return None

def source_domain(path: Path, number) -> Optional[str]:
    """The domain a record file sits in: its folder, else its number range."""
    return _folder_domain(path) or _range_domain(number)

def todo_label(item) -> str:
    return str(item).split(':')[0]

def blocking_todo(todo: list) -> list:
    """The todo items --partial cannot write past (ADR-306 §3)."""
    return [t for t in todo if todo_label(t) in BLOCKING_TODO]

def _range_domain(number) -> Optional[str]:
    base = int(str(number).split('.')[0])
    for domain, config in get_domains().items():
        low, high = config.get('range', (0, -1))
        if low <= base <= high:
            return domain
    low, high = get_legacy_range()
    return 'legacy' if 'legacy' in get_config() and low <= base <= high else None

def _kind_schema(kind: str) -> dict:
    kinds = _mapping(get_config().get('kinds'))
    schema = kinds.get(kind)
    return schema if isinstance(schema, dict) else IMPORT_DECISION_SCHEMA

def _source_path(path: Path) -> str:
    """The path a sheet records: repo-relative inside the repo, else absolute."""
    resolved = path.resolve()
    try:
        return str(resolved.relative_to(get_project_root().resolve()))
    except ValueError:
        return str(resolved)

def read_record(path: Path) -> dict:
    """The sheet for one record file: the v0 reader, or a v1 record read as
    itself (ADR-306 §2, §7). Raises SheetError for a source it cannot read."""
    raw = path.read_bytes()
    try:
        text = raw.decode('utf-8')
    except UnicodeDecodeError:
        raise SheetError('not UTF-8 text')
    if text.startswith('\ufeff'):
        raise SheetError('the file starts with a UTF-8 byte order mark; remove it before scanning')
    if '\r' in text:
        raise SheetError('CRLF line endings; convert the file to LF before scanning')
    front, before, title, after = split_record(text)
    if front is None:
        raise SheetError('no YAML frontmatter; import reads structured records (ADR-306 §2)')
    try:
        data = yaml.safe_load(front) or {}
    except yaml.YAMLError as e:
        raise SheetError(f"frontmatter is not valid YAML: {e}")
    if not isinstance(data, dict):
        raise SheetError('frontmatter is not a mapping of fields')
    if title is None:
        raise SheetError("no '# ADR-NNN: title' heading")
    contract = data.get('contract')
    if contract and contract != V1:
        raise SheetError(f"contract '{contract}' has no reader")
    fmt = 'v1' if contract == V1 else 'v0'

    todo, provenance = [], {}
    from_file = filename_number(path)
    number = _number_value(from_file or title.group(1))
    provenance['target.number'] = 'file name' if from_file else 'H1'
    if from_file and (title.group(1).lstrip('0') or '0') != from_file:
        todo.append(f"target.number: the file name says ADR-{from_file}, the H1 says ADR-{title.group(1)}")
    domain = source_domain(path, number)
    if _folder_domain(path):
        provenance['target.domain'] = f"folder {path.parent.name}"
    elif domain:
        provenance['target.domain'] = 'number range'
    else:
        todo.append('target.domain: neither the folder nor the number names a domain')
    # Under adr/v1 a record in the tree keeps its number wherever its folder
    # puts it (ADR-306 §6); the range only allocates new numbers.
    moved_in_tree = repo_contract() == V1 and _in_tree(path)
    span = number_range(domain) if domain and not moved_in_tree else None
    if span and not span[0] <= int(str(number).split('.')[0]) <= span[1]:
        todo.append(f"target.number: ADR-{number} is outside the {domain} range {span[0]}-{span[1]}")
    provenance['target.title'] = 'H1'

    preamble = before.strip('\n')
    if preamble.strip():
        # A record is written H1 first, so text above the H1 moves to just
        # below it. The body keeps it; nothing is dropped (ADR-306 §1).
        summary, body = None, preamble + '\n' + after
        provenance['body'] = 'text above the H1, then the text after it'
        todo.append('preamble: the text between the frontmatter and the H1 moved into the body, '
                    'right after the H1; check where it belongs')
    else:
        summary, body, note = split_summary(after)
        if summary is not None:
            provenance['summary'] = '## Summary section'
        elif note:
            provenance['summary'] = note

    unmapped = {}
    if fmt == 'v1':
        record = dict(data)
        provenance['record'] = 'frontmatter (adr/v1)'
        kind = record.get('kind')
        schema = _kind_schema(kind) if isinstance(kind, str) else IMPORT_DECISION_SCHEMA
    else:
        schema = _kind_schema('decision')
        record = empty_record('decision', schema, {'model': 'unrecorded'}, {})
        provenance['record.contract'] = 'v0 reader'
        provenance['record.kind'] = 'v0 reader: every v0 record is a decision'
        provenance['record.agent.model'] = 'v0 reader: the model was not recorded'
        record['status'] = _v0_status(data, todo, provenance)
        for key in V0_CARRIED:
            if key in data:
                record[key] = data[key]
                provenance[f"record.{key}"] = f"frontmatter {key}"
        if 'deciders' in data:
            provenance['record.deciders'] = 'frontmatter deciders (never an operator basis, ADR-304 §7)'
        for key, value in data.items():
            if key != 'status' and key not in V0_CARRIED:
                unmapped[key] = value
    todo[:0] = open_fields(record, schema)
    todo += [f"unmapped: {key} has no v1 field; apply keeps it under imported.unmapped. "
             f"Move it into the record, or leave it there" for key in unmapped]

    candidates = {}
    if 'capability' in todo:
        context = parse_text(text, path).section_text.get('Context', '')
        candidates['capability'] = capability_candidates(
            f"{title.group(2)}\n{context}", _mapping(get_config().get('capabilities')))
    return {
        'sheet': SHEET_FORMAT,
        'source': {'path': _source_path(path), 'format': fmt,
                   'sha256': hashlib.sha256(raw).hexdigest()},
        'target': {'number': number, 'domain': domain, 'title': ' '.join(title.group(2).split())},
        'record': _ordered(record),
        'summary': summary,
        'todo': todo,
        'candidates': candidates,
        'provenance': provenance,
        'unmapped': unmapped,
        'body': body,
    }

def _v0_status(data: dict, todo: list, provenance: dict) -> Optional[str]:
    """ADR-304 §7's table. Deprecated is superseded when something replaced
    it, and otherwise accepted with a note, which needs judgement."""
    raw = data.get('status')
    key = str(raw or '').strip().lower()
    if key == 'deprecated':
        if data.get('superseded_by'):
            provenance['record.status'] = f"frontmatter status: {raw}, with superseded_by (ADR-304 §7)"
            return 'superseded'
        provenance['record.status'] = f"frontmatter status: {raw}, with no successor (ADR-304 §7)"
        todo.append('status note: Deprecated with no successor maps to accepted with a note that '
                    'the decision is historical (ADR-304 §7); add the note to the body')
        return 'accepted'
    if key in V0_STATUS_MAP:
        provenance['record.status'] = f"frontmatter status: {raw}"
        return V0_STATUS_MAP[key]
    if 'status' not in data:
        todo.append('status: the source has no status; set one (ADR-304 §7)')
    else:
        todo.append(f"status: '{raw}' has no v1 mapping; set one (ADR-304 §7)")
    return None

# --- the sheet file ----------------------------------------------------------------

class _SheetDumper(_RecordDumper):
    """A sheet is edited by hand, so multi-line text is a literal block."""

def _represent_sheet_str(dumper, value):
    style = '|' if '\n' in value else None
    return dumper.represent_scalar('tag:yaml.org,2002:str', value, style=style)

_SheetDumper.add_representer(str, _represent_sheet_str)

def dump_sheet(sheet: dict) -> str:
    """The sheet as YAML, read back before it is returned (ADR-306 §7)."""
    text = yaml.dump(sheet, Dumper=_SheetDumper, sort_keys=False, allow_unicode=True,
                     default_flow_style=False, width=1000)
    if yaml.safe_load(text) != sheet:
        raise SheetError('the sheet does not read back as written')
    return SHEET_HEADER + text

def _number_token(path: Path) -> Optional[str]:
    """target.number as written, when YAML reads it as an unquoted int, so
    042 (octal), 0x2A or 1_0 can be refused rather than read as another number."""
    try:
        node = yaml.compose(path.read_text(), Loader=yaml.SafeLoader)
    except (OSError, UnicodeDecodeError, yaml.YAMLError):
        return None
    for mapping, name in ((node, 'target'), (None, 'number')):
        if mapping is None:
            mapping = node
        if not isinstance(mapping, yaml.MappingNode):
            return None
        node = next((v for k, v in mapping.value if getattr(k, 'value', None) == name), None)
    if isinstance(node, yaml.ScalarNode) and node.tag == 'tag:yaml.org,2002:int' and not node.style:
        return node.value
    return None

def load_sheet(path: Path) -> dict:
    """A sheet file, checked for the fields apply needs."""
    try:
        sheet = yaml.safe_load(path.read_text())
    except yaml.MarkedYAMLError as e:
        where = f" at line {e.problem_mark.line + 1}" if e.problem_mark else ''
        raise SheetError(f"the sheet is not valid YAML: {e.problem}{where}")
    except (OSError, UnicodeDecodeError, yaml.YAMLError) as e:
        raise SheetError(f"cannot read the sheet: {e}")
    if not isinstance(sheet, dict) or sheet.get('sheet') != SHEET_FORMAT:
        raise SheetError(f"not an {SHEET_FORMAT} sheet")
    target = sheet.get('target')
    if not isinstance(target, dict) or any(_empty(target.get(k)) for k in ('number', 'title')):
        raise SheetError('target needs a number and a title')
    number = target['number']
    token = _number_token(path)
    if token is not None and not re.fullmatch(r'0|[1-9][0-9]*', token):
        raise SheetError(f"target.number {token} is not a plain decimal, and YAML reads it as "
                         f"{number}; quote it, as in number: '{token}'")
    if isinstance(number, float):
        raise SheetError(f"target.number {number} reads as a decimal; quote a sub-part number, "
                         f"as in number: '101.10'")
    if isinstance(number, bool) or not re.fullmatch(r'\d+(\.\d+)?', str(number)):
        raise SheetError(f"target.number '{number}' is not a record number")
    if not isinstance(sheet.get('record'), dict):
        raise SheetError('record is not a mapping of fields')
    for key in ('summary', 'body'):
        if sheet.get(key) is not None and not isinstance(sheet[key], str):
            raise SheetError(f"{key} is not text")
    todo = sheet.get('todo')
    if todo is not None and not (isinstance(todo, list) and all(isinstance(t, str) for t in todo)):
        raise SheetError('todo is not a list of items')
    if sheet.get('unmapped') is not None and not isinstance(sheet['unmapped'], dict):
        raise SheetError('unmapped is not a mapping of fields')
    source = sheet.get('source')
    if source is not None and not (isinstance(source, dict) and isinstance(source.get('path'), str)
                                   and source['path']):
        raise SheetError('source needs a path')
    return sheet
