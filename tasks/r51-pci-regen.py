#!/usr/bin/env python3
"""Regenerate the mechanical block of governance/notes/v022-pci-delta.md at HEAD.

Run after classifying every new crates/axon-core/src commit in
governance/notes/pci-themes/<hash8>.json (a merge needs a file only when it is not
a pure join: see scripts/pci_delta.py PURE_JOIN), then
`python3 -B scripts/pci_delta.py --check`.

Every run (and `--setup` alone) also installs the note's merge driver
(`.gitattributes`: `governance/notes/v022-pci-delta.md merge=pci-note`) in the
repository's git config. That config is the common one, shared by every worktree.
"""
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.join(ROOT, "scripts"))
from pci_delta import BEGIN, END, NOTE, NOTE_DRIVER  # noqa: E402

DRIVER = {"merge.pci-note.name": "PCI delta note: ours' mechanical block, the rest three-way (regenerate after the merge)",
          "merge.pci-note.driver": NOTE_DRIVER}


def setup():
    for key, value in DRIVER.items():
        have = subprocess.run(["git", "config", "--get", key], cwd=ROOT, capture_output=True, text=True).stdout.rstrip("\n")
        if have != value:
            subprocess.run(["git", "config", key, value], cwd=ROOT, check=True)
            print(f"git config {key} set")


setup()
if sys.argv[1:] == ["--setup"]:
    sys.exit(0)
if sys.argv[1:]:
    sys.exit("usage: tasks/r51-pci-regen.py [--setup]")
r = subprocess.run([sys.executable, "-B", "scripts/pci_delta.py", "--emit", "HEAD"],
                   cwd=ROOT, capture_output=True, text=True)
if r.returncode != 0:
    sys.exit(r.stderr.strip() or "pci_delta --emit failed")
text = open(NOTE).read()
i, j = text.index(BEGIN), text.index(END) + len(END)
open(NOTE, "w").write(text[:i] + r.stdout.rstrip("\n") + text[j:])
print("note regenerated at HEAD")
