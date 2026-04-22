---
description: Python dependency security, pip-audit, setup.py risks, PyPI typosquatting
vocabulary: pip-audit setup.py pyproject.toml requirements.txt wheel sdist PyPI typosquat safety pip install python package
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Python Dependency Security

## Scanning

```bash
# Preferred: scan without installing
pip-audit -r requirements.txt
osv-scanner --lockfile=requirements.txt

# If already installed
pip-audit
```

## Python-Specific Risks

- **`setup.py` runs on install.** `pip install .` executes `setup.py` — any code in there runs with your permissions. Read it first for unfamiliar packages.
- **`pyproject.toml` is safer** but can still specify build backends that execute code.
- **Pickle files are code.** `pickle.load()` can execute arbitrary Python. Never unpickle data from untrusted sources. Same for `marshal`, `shelve`, `yaml.load()` (use `yaml.safe_load()`).
- **Typosquatting on PyPI is real.** `reqeusts` instead of `requests`. Check package names carefully, especially in copied requirements files.
