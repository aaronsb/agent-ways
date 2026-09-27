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
    issues: list = field(default_factory=list)

@dataclass
class Issue:
    message: str
    severity: str = 'warning'

