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

V1_BODY_SKELETON = '''## Context

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

class _RecordDumper(yaml.SafeDumper):
    """Block-style YAML that indents list items under their key, as the
    records in this corpus are written."""
    def increase_indent(self, flow=False, indentless=False):
        return super().increase_indent(flow, False)

def _as_date(value):
    """A YYYY-MM-DD string as a date, so it is written unquoted."""
    if isinstance(value, str) and re.fullmatch(r'\d{4}-\d{2}-\d{2}', value):
        return date.fromisoformat(value)
    return value

def placeholder_lines(text: str) -> list:
    """Lines still holding a skeleton placeholder from `adr new`."""
    marks = [line.strip() for line in (V1_SUMMARY_SKELETON + V1_BODY_SKELETON).splitlines()
             if '[' in line and ']' in line]
    return [line.strip() for line in text.splitlines() if line.strip() in marks]

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
    """The v1 record a sheet describes: frontmatter, title, Summary, body.
    A sheet with no summary gets the skeleton when the record is a decision."""
    target = sheet['target']
    record = sheet['record']
    fields = _ordered(record)
    if 'date' in fields:
        fields['date'] = _as_date(fields['date'])
    front = yaml.dump(fields, Dumper=_RecordDumper, sort_keys=False, allow_unicode=True,
                      default_flow_style=False, width=1000)
    parts = [f"---\n{front}---\n", f"# ADR-{int(target['number']):03d}: {target['title']}\n"]
    summary = sheet.get('summary')
    if summary:
        parts.append(summary if summary.startswith('## Summary') else f"## Summary\n\n{summary}")
    elif record.get('kind') == 'decision':
        parts.append(V1_SUMMARY_SKELETON)
    if sheet.get('body'):
        parts.append(sheet['body'])
    return '\n'.join(p.rstrip('\n') + '\n' for p in parts)

@file_rule(contract=V1)
def rule_v1_no_placeholders(adr, ctx):
    """A record still holding `adr new`'s placeholder text is unfinished: a
    warning while proposed, an error once it has left proposed."""
    if not is_v1_record(adr, ctx):
        return
    left = placeholder_lines(adr.body)
    if not left:
        return
    level = 'warning' if str(adr.status or '').lower() == 'proposed' else 'error'
    v1_issue(adr, f"{len(left)} placeholder line(s) from `adr new` still to fill, first: {left[0]}", level)
