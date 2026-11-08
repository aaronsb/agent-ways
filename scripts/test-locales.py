#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

"""Validate locale files: no gaps, no duplicates, no inactive entries."""
import json, glob, sys

with open("tools/ways-cli/languages.json") as f:
    active = {
        k for k, v in json.load(f)["languages"].items()
        if v.get("active") and k != "en"
    }

errors = 0
file_count = 0

for fp in sorted(glob.glob("hooks/ways/**/*.locales.jsonl", recursive=True)):
    file_count += 1
    seen = {}
    with open(fp) as f:
        for i, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            obj = json.loads(line)
            lang = obj.get("lang", "")
            if lang in seen:
                print(f"  DUPLICATE: {fp}:{i} lang={lang} (first at line {seen[lang]})")
                errors += 1
            seen[lang] = i

    missing = active - set(seen.keys())
    if missing:
        rel = fp.replace("hooks/ways/", "")
        print(f"  GAP: {rel} missing: {', '.join(sorted(missing))}")
        errors += 1

    inactive = set(seen.keys()) - active
    if inactive:
        rel = fp.replace("hooks/ways/", "")
        print(f"  INACTIVE: {rel} has entries for: {', '.join(sorted(inactive))}")
        errors += 1

print(f"  Checked {file_count} files, {len(active)} active languages")
if errors:
    print(f"  {errors} issues found")
    sys.exit(1)
print("  Locale coverage: PASS")
