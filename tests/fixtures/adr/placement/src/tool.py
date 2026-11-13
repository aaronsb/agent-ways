"""Command-line front end.

Module docstrings count as comments: ADR-101 here is in the body.

References:
- ADR-102: the cache the commands read.
"""

import sys


def run():
    """Runs the command.

    References:
    - ADR-101: where results are stored.
    """
    # A comment with no References block: ADR-102.
    print("see ADR-101 for details")
    label = 'ADR-102'
    msg = f"""a triple-quoted string that is not a docstring
names ADR-101"""
    return msg
