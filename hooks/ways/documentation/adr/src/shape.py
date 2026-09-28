# ============================================================================
# Vocabulary shape
# ============================================================================
#
# Counts active records per domain and, for adr/v1 records, per capability,
# and names a corpus that is large while its vocabulary is small: many
# records under few domains or capabilities, or most of them under one.
# `adr contract`, `adr domains` and, under adr/v1, `adr lint` print it as a
# notice. It is a count and never fails a command. The way macro prints the
# notices from `adr domains --shape`, run with the installed agent-ways tool.

SHAPE_MIN_RECORDS = 40        # below this many records, no notice
SHAPE_FEW_DOMAINS = 2         # this many domains in use, or fewer
SHAPE_DOMAIN_SHARE = 0.60     # or one domain holding this share of the records
SHAPE_FEW_CAPABILITIES = 2    # this many capabilities in use, or fewer
SHAPE_CAPABILITY_SHARE = 0.50  # or one capability on this share of the records


@dataclass
class ShapeFinding:
    axis: str        # 'domain' or 'capability'
    records: int     # records counted on this axis
    in_use: int      # distinct domains or capabilities among them
    top: str         # the one holding the most records (ties: first by name)
    top_count: int


def _thin(axis: str, counts: dict, records: int, few: int, share: float):
    if records < SHAPE_MIN_RECORDS or not counts:
        return None
    top = sorted(counts, key=lambda name: (-counts[name], name))[0]
    if len(counts) <= few or counts[top] >= share * records:
        return ShapeFinding(axis, records, len(counts), top, counts[top])
    return None


def vocabulary_shape(corpus: list) -> dict:
    """The thin axes of the active corpus: {'domain': finding,
    'capability': finding}, each None when that axis is not thin. The
    capability axis counts only adr/v1 records; a record listing several
    capabilities counts once for each."""
    active = [adr for adr in corpus if not is_archived(adr.path)]
    domains = {}
    for adr in active:
        if adr.domain:
            domains[adr.domain] = domains.get(adr.domain, 0) + 1
    capabilities, v1_records = {}, 0  # v1 records naming a capability
    for adr in active:
        if adr.contract != V1:
            continue
        names = sorted({name for name in capability_scope(adr) if name != '*'})
        if names:
            v1_records += 1
        for name in names:
            capabilities[name] = capabilities.get(name, 0) + 1
    return {
        'domain': _thin('domain', domains, sum(domains.values()),
                        SHAPE_FEW_DOMAINS, SHAPE_DOMAIN_SHARE),
        'capability': _thin('capability', capabilities, v1_records,
                            SHAPE_FEW_CAPABILITIES, SHAPE_CAPABILITY_SHARE),
    }


def _plural(count: int, one: str, many: str) -> str:
    return f"{count} {one if count == 1 else many}"


def shape_notice(finding: ShapeFinding, seeds: Optional[list] = None) -> str:
    """One notice line for a thin axis. seeds: the capabilities seeded, or
    to be seeded, from the domains; None when no capabilities are seeded."""
    pct = round(100 * finding.top_count / finding.records)
    if finding.axis == 'domain':
        text = f"{_plural(finding.records, 'record', 'records')} in {_plural(finding.in_use, 'domain', 'domains')}"
        text += f" ({finding.top})." if finding.in_use == 1 else \
            f"; {finding.top} holds {finding.top_count} ({pct}%)."
        if seeds and finding.top in seeds:
            text += (f" Of the {_plural(len(seeds), 'capability', 'capabilities')} seeded from the "
                     f"domains, {finding.top} would carry {finding.top_count} of the "
                     f"{finding.records} records.")
        return (text + " Read the records and propose domains or capabilities (`adr list --json` "
                "gives titles and fields), then apply with `adr domain move --plan` or by editing "
                "`capabilities`, previewing with --whatif.")
    text = f"{_plural(finding.records, 'adr/v1 record', 'adr/v1 records')} use " \
           f"{_plural(finding.in_use, 'capability', 'capabilities')}"
    text += f" ({finding.top})." if finding.in_use == 1 else \
        f"; {finding.top} is on {finding.top_count} ({pct}%)."
    return (text + " Read the records and propose a finer capability list (`adr list --json` "
            "gives titles and fields), then edit `capabilities` in adr.yaml and each record's "
            "`capability` with `adr set`, previewing with --whatif.")


def shape_findings(shape: dict, config: dict) -> list:
    """The findings to report. Under adr/v1 with a capabilities vocabulary,
    capabilities carry what a record is about and a domain only allocates
    numbers, so only the capability axis is reported. Otherwise the domain
    axis is reported too, since the capabilities are still to be seeded
    from it."""
    if str(config.get('contract')) == V1 and config.get('capabilities'):
        return [shape['capability']] if shape['capability'] else []
    return [shape[axis] for axis in ('domain', 'capability') if shape[axis]]


@config_rule(contract=V1)
def rule_vocabulary_thin(ctx):
    finding = vocabulary_shape(ctx.corpus)['capability']
    if finding:
        ctx.config_issues.append(Issue(shape_notice(finding), 'warning', code='vocabulary-thin'))
