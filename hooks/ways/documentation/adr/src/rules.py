# ============================================================================
# Lint rules
# ============================================================================
#
# Checks are registered functions, run by `adr lint` in registration order, so
# the order of issues in lint output is the order rules appear below. Every rule
# is called as rule(adr, ctx) and appends Issues to adr.issues.
#
# parse_adr reports what it finds while reading: an unreadable file (which stops
# parsing), bad YAML, unclosed or missing frontmatter. Only an unreadable file
# stops the rules. `has_frontmatter` means the file opens with `---`, so after
# bad YAML or an unclosed block the field rules still run and report the
# fields as missing. The goldens depend on that.
#
# A file rule looks at one ADR. A corpus rule also resolves against the whole
# corpus through ctx. A rule registered with contract="adr/v1" runs only when
# adr.yaml declares that contract; an ungated rule always runs.

FILE_RULES = []
CORPUS_RULES = []
CONFIG_RULES = []

def _register(registry, fn=None, *, contract=None):
    def add(f):
        registry.append((f, contract))
        return f
    return add(fn) if fn is not None else add

def file_rule(fn=None, *, contract=None):
    """Register a rule over one ADR: fn(adr, ctx)."""
    return _register(FILE_RULES, fn, contract=contract)

def corpus_rule(fn=None, *, contract=None):
    """Register a rule that resolves against the corpus: fn(adr, ctx)."""
    return _register(CORPUS_RULES, fn, contract=contract)

def config_rule(fn=None, *, contract=None):
    """Register a rule over adr.yaml and the corpus as a whole: fn(ctx).
    Its issues go to ctx.config_issues and print under adr.yaml."""
    return _register(CONFIG_RULES, fn, contract=contract)

@dataclass
class LintContext:
    """What rules resolve against: the corpus index and the loaded config."""
    by_number: dict
    config: dict
    contract: str
    corpus: list = field(default_factory=list)
    config_issues: list = field(default_factory=list)

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
        config = get_config()
        return cls(by_number=by_number, config=config,
                   contract=str(config.get('contract', 'adr/v0')), corpus=corpus)

def _active(registry, ctx):
    return [rule for rule, contract in registry if contract in (None, ctx.contract)]

def run_rules(adrs: list, ctx: 'LintContext') -> None:
    """Config rules, then file rules for every ADR, then corpus rules for
    every ADR."""
    for rule in _active(CONFIG_RULES, ctx):
        rule(ctx)
    file_rules = _active(FILE_RULES, ctx)
    corpus_rules = _active(CORPUS_RULES, ctx)
    for adr in adrs:
        for rule in file_rules:
            rule(adr, ctx)
    for adr in adrs:
        for rule in corpus_rules:
            rule(adr, ctx)

# --- file rules ---------------------------------------------------------------

@file_rule
def rule_title_number(adr, ctx):
    if not adr.number:
        adr.issues.append(Issue("Missing ADR number in title", 'error'))

# Field-level rules skip a file without frontmatter: the root cause is
# already reported by parse_adr.

@file_rule
def rule_status(adr, ctx):
    if not adr.has_frontmatter or is_v1_record(adr, ctx):
        return
    valid_statuses = get_statuses()
    if not adr.status:
        adr.issues.append(Issue("Missing status in frontmatter", 'error'))
    elif adr.status not in valid_statuses:
        adr.issues.append(Issue(f"Invalid status: {adr.status} (valid: {', '.join(sorted(valid_statuses))})", 'warning'))

@file_rule
def rule_date(adr, ctx):
    if adr.has_frontmatter and not adr.date:
        adr.issues.append(Issue("Missing date in frontmatter", 'error'))

@file_rule
def rule_deciders(adr, ctx):
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


# --- adr.yaml -------------------------------------------------------------------

DOMAIN_KEYS = ('range', 'name', 'description', 'folder')

@config_rule
def rule_domain_shape(ctx):
    """Each domain in adr.yaml gives the keys the tool reads from it. Other
    commands read a domain without them as placing no records by range, or
    as named by its key (ADR-311)."""
    def bad(message):
        ctx.config_issues.append(Issue(message, 'error', code='domain-shape'))

    domains = ctx.config.get('domains')
    for name, cfg in (domains.items() if isinstance(domains, dict) else ()):
        if not isinstance(cfg, dict):
            bad(f"domains.{name}: expected a mapping of {', '.join(DOMAIN_KEYS)}")
            continue
        missing = [key for key in DOMAIN_KEYS if key not in cfg]
        if missing:
            bad(f"domains.{name}: missing {', '.join(missing)}")
        if 'range' in cfg and domain_range(cfg) is None:
            bad(f"domains.{name}.range: expected [low, high]")
        if 'folder' in cfg and not domain_folders(cfg):
            bad(f"domains.{name}.folder: expected a folder name or a list of them")

# --- config against records ---------------------------------------------------

@config_rule
def rule_records_ahead_of_config(ctx):
    """Records that declare a newer contract than adr.yaml (#614). Only
    known contracts are compared; a v0 corpus never reaches the warning."""
    declared = contract_rank(ctx.contract)
    if declared is None:
        return
    ahead = {}
    for adr in ctx.corpus:
        rank = contract_rank(adr.contract)
        if rank is not None and rank > declared:
            ahead[adr.contract] = ahead.get(adr.contract, 0) + 1
    if not ahead:
        return
    newest = max(ahead, key=contract_rank)
    count = sum(ahead.values())
    ctx.config_issues.append(Issue(
        f"{count} record(s) declare {newest}, newer than adr.yaml's {ctx.contract}; "
        f"`adr contract --upgrade` brings adr.yaml to {CURRENT_CONTRACT}",
        'warning', code='contract-behind'))
