# ============================================================================
# Supersession references (ADR-303 / issue #438)
# ============================================================================

def norm_ref(ref) -> tuple[str, Optional[str]]:
    """Normalize a supersession reference to (number, section).

    Accepts 167, "167", "ADR-167", "ADR-167#4" — returns ("167", None) or
    ("167", "4"). The number keeps sub-numbers ("51.2") but drops zero padding.
    """
    text = str(ref).strip()
    number, _, section = text.partition('#')  # before upper(): sections keep their case
    number = number.upper().replace('ADR-', '').lstrip('0') or '0'
    return number, (section or None)

def filename_number(path: Path) -> Optional[str]:
    """Extract the ADR number from a filename (ADR-051.2-foo.md -> 51.2)."""
    match = re.match(r'ADR-(\d+(?:\.\d+)?)', path.name, re.IGNORECASE)
    return match.group(1).lstrip('0') if match else None

def find_by_ref(ref: str, adrs: list) -> list:
    """Find ADRs matching a reference by title number or filename number."""
    number, _ = norm_ref(ref)
    return [a for a in adrs if
            (a.number and a.number.lstrip('0') == number) or
            filename_number(a.path) == number]

def supersession_note(adr) -> str:
    """Human-readable supersession annotation for list/index, or ''."""
    if not adr.superseded_by:
        return ''
    parts = []
    for entry in adr.superseded_by:
        number, section = norm_ref(entry)
        parts.append(f"ADR-{number} §{section}" if section else f"ADR-{number}")
    in_force = adr.status not in NON_ACTIVE_STATUSES
    label = 'partially superseded by' if in_force else 'superseded by'
    return f"{label} {', '.join(parts)}"

