# ============================================================================
# adr/v1: sections, imports, enactment, placeholders, observables
# ============================================================================
#
# Checks of one record's own shape. How records relate over time, and
# whether an accepted one changed, is read from git (ADR-311).

V1_ENACTING_VERBS = ('cut', 'retire')

def _find_section(adr, name: str) -> Optional[str]:
    """The heading that is this section: the name alone, or the name followed
    by punctuation ("Summary: …", "Summary (draft)"). "Summary Nudge" is a
    different section."""
    pattern = re.compile(rf'{re.escape(name)}(\s*[:(].*|\s+[\u2014\u2013-].*)?', re.IGNORECASE)
    for heading in adr.sections:
        if pattern.fullmatch(heading.strip()):
            return heading
    return None

# --- one record ------------------------------------------------------------------

@file_rule(contract=V1)
def rule_v1_required_sections(adr, ctx):
    schema = v1_kind_schema(adr, ctx)
    if not is_v1_record(adr, ctx) or schema is None:
        return
    imported = 'imported' in adr.frontmatter
    for name in _str_list(schema.get('sections')) or []:
        if _find_section(adr, name) is None:
            if imported and name == 'Summary':
                # ADR-306 §4: an imported record may gain its Summary later,
                # once the imported corpus has been read together.
                v1_issue(adr, "imported record has no '## Summary' yet (ADR-306 §4)", 'warning')
            else:
                v1_issue(adr, f"a {v1_record_kind(adr)} record opens with a '## {name}' section")

@file_rule(contract=V1)
def rule_v1_imported(adr, ctx):
    """`imported` records where the record came from: {from, format}, and
    optionally `status`, the source status as written, and `unmapped`, the
    source keys with no v1 field (ADR-306 §1, §4, §7)."""
    if not is_v1_record(adr, ctx) or 'imported' not in adr.frontmatter:
        return
    imported = adr.frontmatter.get('imported')
    if not isinstance(imported, dict) or not all(
            isinstance(imported.get(k), str) and imported.get(k).strip() for k in ('from', 'format')):
        v1_issue(adr, "imported: expected {from: <source path>, format: <reader>}")
    elif 'unmapped' in imported and not isinstance(imported['unmapped'], dict):
        v1_issue(adr, "imported.unmapped: expected a mapping of source fields")
    elif isinstance(imported.get('status'), (dict, list)):
        v1_issue(adr, "imported.status: expected the source's status as written")

@file_rule(contract=V1)
def rule_v1_enacted(adr, ctx):
    if not is_v1_record(adr, ctx) or 'enacted' not in adr.frontmatter:
        return
    enacted = adr.frontmatter.get('enacted')
    verb = adr.frontmatter.get('verb')
    if verb not in V1_ENACTING_VERBS:
        v1_issue(adr, f"enacted belongs to a cut or retire decision (this one is '{verb}')")
    elif str(adr.status or '').lower() in ('proposed', 'rejected', 'abandoned'):
        v1_issue(adr, f"enacted marks an accepted decision done; this one is {str(adr.status).lower()}")
    if not isinstance(enacted, str):
        v1_issue(adr, f"enacted: quote the commit hash as a string (YAML read {enacted!r})")
    elif not re.fullmatch(r'[0-9a-f]{7,40}', enacted):
        v1_issue(adr, f"enacted: '{enacted}' is not a commit hash")

@file_rule(contract=V1)
def rule_v1_no_placeholders(adr, ctx):
    """A record still holding a prompt from `adr new`'s skeleton is unfinished:
    a warning while proposed, an error once it has left proposed. The prompts
    live in sheet.py, which assembles after this module."""
    if not is_v1_record(adr, ctx):
        return
    left = placeholder_lines(adr.body)
    if not left:
        return
    level = 'warning' if str(adr.status or '').lower() == 'proposed' else 'error'
    v1_issue(adr, f"{len(left)} placeholder line(s) from `adr new` still to fill, first: {left[0]}", level)

@file_rule(contract=V1)
def rule_v1_observable(adr, ctx):
    """What should be observable when a decision holds (ADR-307): optional, a
    list whose entries are plain words or mappings with keys the author chooses.
    The check applies to any v1 kind that carries the field."""
    if not is_v1_record(adr, ctx) or 'observable' not in adr.frontmatter:
        return
    entries = adr.frontmatter.get('observable')
    if entries is None or entries == []:
        v1_issue(adr, "observable: empty; remove the key or add an entry")
        return
    if not isinstance(entries, list):
        v1_issue(adr, "observable: expected a list of entries, each a line of words or a mapping")
        return
    for i, entry in enumerate(entries, 1):
        if isinstance(entry, str) and entry.strip():
            continue
        if isinstance(entry, dict) and entry:
            continue
        v1_issue(adr, f"observable entry {i}: expected a line of words or a non-empty mapping")
