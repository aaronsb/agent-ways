# ============================================================================
# Lint rules
# ============================================================================
#
# Checks are registered functions, run in registration order, so the order of
# issues in lint output is the order rules appear below. Parse failures (an
# unreadable file, bad YAML, missing or unclosed frontmatter) are not rules:
# they stop parsing, so parse_adr reports them itself.
#
# A file rule sees one parsed ADR. A corpus rule also sees the whole corpus
# through a LintContext. Each rule appends Issues to adr.issues.

FILE_RULES = []
CORPUS_RULES = []

def file_rule(fn):
    """Register a rule over one parsed ADR: fn(adr)."""
    FILE_RULES.append(fn)
    return fn

def corpus_rule(fn):
    """Register a rule that resolves against the corpus: fn(adr, ctx)."""
    CORPUS_RULES.append(fn)
    return fn

@dataclass
class LintContext:
    """What corpus rules resolve against."""
    by_number: dict

    @classmethod
    def from_corpus(cls, corpus: list) -> 'LintContext':
        by_number = {}
        for adr in corpus:
            if adr.number:
                by_number.setdefault(adr.number.lstrip('0') or '0', adr)
            # Filename fallback: a target with a malformed H1 should still resolve
            fn_number = filename_number(adr.path)
            if fn_number:
                by_number.setdefault(fn_number, adr)
        return cls(by_number=by_number)

def run_file_rules(adr) -> None:
    for rule in FILE_RULES:
        rule(adr)

def run_corpus_rules(adrs: list, ctx: 'LintContext') -> None:
    for adr in adrs:
        for rule in CORPUS_RULES:
            rule(adr, ctx)

# --- file rules ---------------------------------------------------------------

@file_rule
def rule_title_number(adr):
    if not adr.number:
        adr.issues.append(Issue("Missing ADR number in title", 'error'))

# Field-level rules skip a file without frontmatter: the root cause is
# already reported by parse_adr.

@file_rule
def rule_status(adr):
    if not adr.has_frontmatter:
        return
    valid_statuses = get_statuses()
    if not adr.status:
        adr.issues.append(Issue("Missing status in frontmatter", 'error'))
    elif adr.status not in valid_statuses:
        adr.issues.append(Issue(f"Invalid status: {adr.status} (valid: {', '.join(sorted(valid_statuses))})", 'warning'))

@file_rule
def rule_date(adr):
    if adr.has_frontmatter and not adr.date:
        adr.issues.append(Issue("Missing date in frontmatter", 'error'))

@file_rule
def rule_deciders(adr):
    if adr.has_frontmatter and not adr.deciders:
        adr.issues.append(Issue("Missing deciders in frontmatter", 'warning'))

# --- corpus rules -------------------------------------------------------------

@corpus_rule
def rule_supersession_links(adr, ctx):
    """Each supersession entry resolves, and its target declares the reverse."""
    for field_name in ('supersedes', 'superseded_by'):
        for entry in getattr(adr, field_name):
            number, _ = norm_ref(entry)
            target = ctx.by_number.get(number)
            if target is None:
                adr.issues.append(Issue(
                    f"{field_name}: '{entry}' resolves to no known ADR", 'error'))
                continue
            # Reciprocity: A superseded_by B ⟺ B supersedes A (sections ignored)
            if adr.number:
                own = adr.number.lstrip('0') or '0'
                reverse = 'supersedes' if field_name == 'superseded_by' else 'superseded_by'
                reverse_numbers = {norm_ref(e)[0] for e in getattr(target, reverse)}
                if own not in reverse_numbers:
                    adr.issues.append(Issue(
                        f"{field_name}: ADR-{number} does not declare the reciprocal "
                        f"{reverse}: ADR-{own} — one-directional links rot", 'warning'))

