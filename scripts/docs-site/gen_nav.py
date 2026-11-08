# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

"""Publish scripts/docs-site/nav.md as the site's SUMMARY.md for mkdocs-literate-nav.

The nav lives beside the other site scripts so docs/ holds only documentation.
"""

from pathlib import Path

import mkdocs_gen_files

with mkdocs_gen_files.open("SUMMARY.md", "w") as f:
    f.write((Path(__file__).parent / "nav.md").read_text(encoding="utf-8"))
