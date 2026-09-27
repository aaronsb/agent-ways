#!/usr/bin/env python3
"""
ADR - Architecture Decision Record CLI Tool

A librarian for managing Architecture Decision Records.

Usage:
    adr list [--domain DOMAIN] [--status STATUS] [--group] [--archived|--all]
    adr view <number>          # View an ADR (aliases: v, show)
    adr new <domain> <title>
    adr rename <number> [new-title] [--slug SLUG]
    adr archive <number> --reason "..." [--superseded-by ADR-N[,ADR-M]] [--status S] [--dry-run]
    adr lint [--check] [paths...]
    adr index [-y]
    adr domains
    adr config

Configuration is loaded from docs/architecture/adr.yaml
"""

import argparse
import re
import subprocess
import sys
from datetime import date
from pathlib import Path
from dataclasses import dataclass, field
from typing import Optional

# Check for PyYAML
try:
    import yaml
except ImportError:
    print("Error: PyYAML is required. Install with: pip install pyyaml", file=sys.stderr)
    print("       Or system-wide: sudo apt install python3-yaml", file=sys.stderr)
    sys.exit(1)

# Vendored-tool version (ADR-177). Bump when this tool changes — way macros
# compare it against the installed template to tell stale from customized.
TOOL_VERSION = "1.1.0"

# Statuses that mean "no longer in force" — used by archive and the
# partial-supersession convention (ADR-303 / issue #438 option C2).
NON_ACTIVE_STATUSES = {'Superseded', 'Deprecated', 'Rejected'}

