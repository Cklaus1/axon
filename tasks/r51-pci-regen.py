#!/usr/bin/env python3
"""Regenerate the mechanical block of governance/notes/v022-pci-delta.md at HEAD.

Run after classifying every new crates/axon-core/src commit (including merges) in
scripts/pci_delta.py THEMES, then `python3 -B scripts/pci_delta.py --check`.
"""
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NOTE = os.path.join(ROOT, "governance/notes/v022-pci-delta.md")
BEGIN = "<!-- BEGIN MECHANICAL (scripts/pci_delta.py) -->"
END = "<!-- END MECHANICAL -->"

r = subprocess.run([sys.executable, "-B", "scripts/pci_delta.py", "--emit", "HEAD"],
                   cwd=ROOT, capture_output=True, text=True)
if r.returncode != 0:
    sys.exit(r.stderr.strip() or "pci_delta --emit failed")
text = open(NOTE).read()
i, j = text.index(BEGIN), text.index(END) + len(END)
open(NOTE, "w").write(text[:i] + r.stdout.rstrip("\n") + text[j:])
print("note regenerated at HEAD")
