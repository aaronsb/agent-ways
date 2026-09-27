# ============================================================================
# Data Classes
# ============================================================================

@dataclass
class ADRInfo:
    path: Path
    number: Optional[str] = None
    title: Optional[str] = None
    status: Optional[str] = None
    date: Optional[str] = None
    deciders: list = field(default_factory=list)
    related: list = field(default_factory=list)
    supersedes: list = field(default_factory=list)
    superseded_by: list = field(default_factory=list)
    domain: Optional[str] = None
    has_frontmatter: bool = False
    # adr/v1 (ADR-304): the raw frontmatter, the contract the record declares
    # (None means adr/v0), and its heading texts for section references.
    frontmatter: dict = field(default_factory=dict)
    contract: Optional[str] = None
    sections: list = field(default_factory=list)
    section_text: dict = field(default_factory=dict)  # heading -> its body text
    body: str = ''
    issues: list = field(default_factory=list)

@dataclass
class Issue:
    message: str
    severity: str = 'warning'
    code: Optional[str] = None  # a stable tag for tools that act on issues

