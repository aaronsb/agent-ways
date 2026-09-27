# --- import sheets and the v1 record writer (ADR-306) -------------------------------

SHEET_FORMAT = 'adr-import/v1'

# Frontmatter keys in the order a v1 record is written. Keys a sheet carries
# that are not listed follow in the sheet's own order.
V1_KEY_ORDER = ('contract', 'kind', 'verb', 'capability', 'targets', 'supersedes', 'amends',
                'extends', 'decided_by', 'superseded_by', 'enacted', 'basis', 'agent',
                'considered', 'concern', 'status', 'date', 'deciders', 'related', 'imported')

V1_SUMMARY_SKELETON = '''## Summary

- **Decided:** [what is decided, in plain terms]
- **Trades away:** [what it gives up or forecloses]
- **One-way?** [yes or no, and why]
- **Probes:** *Confident:* [a point you are sure of]. *Not confident:* [a point you are not].
- **Inversion:** [the two ends this sits between; is the answer outside that framing?]
'''

# The body `adr new` writes, under v0 and v1 alike.
BODY_SKELETON = '''## Context

[What is the issue that we're seeing that is motivating this decision or change?]

## Decision

[What is the change that we're proposing and/or doing?]

## Consequences

### Positive

- [What becomes easier?]

### Negative

- [What becomes harder?]

### Neutral

- [What other changes does this enable or require?]

## Alternatives Considered

- [What other options were evaluated?]
- [Why were they rejected?]
'''

# The bracketed prompts in the skeletons. A line still holding one is unfinished.
SKELETON_PROMPTS = tuple(dict.fromkeys(re.findall(r'\[[^\]\n]+\]', V1_SUMMARY_SKELETON + BODY_SKELETON)))

class _RecordDumper(yaml.SafeDumper):
    """Block-style YAML as this corpus writes it: list items indented under
    their key, an empty field as `~`, and a multi-line string on one
    double-quoted line, so no line of it can read as the `---` fence."""
    def increase_indent(self, flow=False, indentless=False):
        return super().increase_indent(flow, False)

def _represent_str(dumper, value):
    style = '"' if '\n' in value else None
    return dumper.represent_scalar('tag:yaml.org,2002:str', value, style=style)

_RecordDumper.add_representer(str, _represent_str)
_RecordDumper.add_representer(type(None),
                              lambda dumper, _: dumper.represent_scalar('tag:yaml.org,2002:null', '~'))

def _as_date(value):
    """A valid YYYY-MM-DD string as a date, so it is written unquoted. An
    invalid one stays a string for lint to report."""
    if isinstance(value, str) and re.fullmatch(r'\d{4}-\d{2}-\d{2}', value):
        try:
            return date.fromisoformat(value)
        except ValueError:
            return value
    return value

def empty_record(kind: str, schema: dict, given: dict, defaults: dict) -> dict:
    """The frontmatter a record of this kind starts with: every field the kind
    requires, from `given` where supplied and empty otherwise. The kind's
    schema decides the fields, not its name (ADR-304 §1, §4)."""
    record = {'contract': V1, 'kind': kind}
    if schema.get('verb') == 'required':
        record['verb'] = given.get('verb')
    empty = {'basis': [], 'targets': []}
    for name in v1_requires(schema):
        if name == 'agent':
            record['agent'] = {'name': given.get('agent'), 'model': given.get('model')}
        else:
            record[name] = given.get(name, empty.get(name))
    lifecycle = v1_lifecycle(schema)
    default_status = str(defaults.get('status') or '').lower()
    record['status'] = default_status if default_status in lifecycle else lifecycle[0]
    return record

def new_sheet(number: int, domain: str, title: str, record: dict,
              summary: Optional[str] = None, body: str = '') -> dict:
    """A sheet for a record that has no source: what `adr new` applies."""
    return {'sheet': SHEET_FORMAT, 'source': None,
            'target': {'number': number, 'domain': domain, 'title': title},
            'record': record, 'summary': summary, 'body': body}

def _ordered(record: dict) -> dict:
    ordered = {k: record[k] for k in V1_KEY_ORDER if k in record}
    ordered.update({k: v for k, v in record.items() if k not in ordered})
    return ordered

def render_record(sheet: dict) -> str:
    """The v1 record a sheet describes. It writes only what the sheet holds:
    the frontmatter, the title, the Summary if the sheet has one, then the
    body verbatim. The frontmatter is read back before it is returned, and a
    mismatch raises: the round trip is checked, not assumed (ADR-306 §7)."""
    target = sheet['target']
    fields = _ordered(sheet['record'])
    if 'date' in fields:
        fields['date'] = _as_date(fields['date'])
    front = yaml.dump(fields, Dumper=_RecordDumper, sort_keys=False, allow_unicode=True,
                      default_flow_style=False, width=1000)
    if yaml.safe_load(front) != fields:
        raise ValueError(f"ADR-{target['number']}: frontmatter does not read back as written")
    title = ' '.join(str(target['title']).split())
    text = f"---\n{front}---\n\n# ADR-{int(target['number']):03d}: {title}\n"
    summary = sheet.get('summary')
    if summary:
        summary = summary if summary.startswith('## Summary') else f"## Summary\n\n{summary}"
        text += '\n' + summary.rstrip('\n') + '\n'
    if sheet.get('body'):
        text += '\n' + sheet['body']
    return text

def placeholder_lines(text: str) -> list:
    """Lines outside fenced code that still hold a skeleton prompt."""
    found, fence = [], None
    for line in text.splitlines():
        marker = re.match(r'\s*(```|~~~)', line)
        if marker:
            fence = None if fence == marker.group(1) else (fence or marker.group(1))
            continue
        if fence is None and any(prompt in line for prompt in SKELETON_PROMPTS):
            found.append(line.strip())
    return found
