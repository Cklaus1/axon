#!/usr/bin/env python3
"""compat_sweep.py - what a release's interpreter refuses in the tracked corpus (release-contract RC-5).

Usage: python3 scripts/compat_sweep.py --bin AXON --out TSV [--jobs N]
       python3 scripts/compat_sweep.py --diff OLD NEW --section-has-behaviour-change {0,1}

Sweep: for every `git ls-files '*.ax'` of this repository, `AXON check --json <basename>` from
the file's directory, with every AXON_* variable removed, HOME set to an empty temp dir and the
binary copied into a temp dir with no ../lib/axon (E0901's module search reads $HOME/.axon/lib
and <bin>/../lib/axon, so the host must not decide an exit code), under a 60 s timeout. One row
per file, sorted: `path<TAB>rc<TAB>first error code or ->` (a timeout is rc 124, code TIMEOUT).
Diagnostic text is never recorded: E0901's search list embeds the binary's directory
(spec/stability.md section 7).

Diff: files in both sweeps whose rc went 0 -> nonzero (NEWLY REFUSED) or whose first code
changed (CODE CHANGED), and files whose rc went nonzero -> 0 (NEWLY ACCEPTED, informational).
Exit 1 when a file is newly refused and --section-has-behaviour-change is 0 (the release's
CHANGELOG section records no behaviour change: `release_ledger.py
--section-has-behaviour-change vX.Y.Z`); otherwise exit 0.
"""

import argparse
import concurrent.futures
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TIMEOUT_S = 60


def first_error(text):
    for line in text.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if isinstance(d, dict) and d.get("severity") == "error" and d.get("code"):
            return d["code"]
    return "-"


def check_one(axon, env, rel):
    p = ROOT / rel
    try:
        o = subprocess.run(
            [axon, "check", "--json", p.name],
            cwd=p.parent,
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            errors="replace",
            timeout=TIMEOUT_S,
        )
    except subprocess.TimeoutExpired:
        return (rel, 124, "TIMEOUT")
    code = first_error(o.stdout + "\n" + o.stderr) if o.returncode != 0 else "-"
    return (rel, o.returncode, code)


def sweep(binary, out, jobs):
    files = subprocess.run(
        ["git", "ls-files", "-z", "*.ax"], cwd=ROOT, check=True, capture_output=True
    ).stdout.decode().split("\0")
    files = [f for f in files if f]
    with tempfile.TemporaryDirectory(prefix="axon-compat-") as tmp:
        home = Path(tmp) / "home"
        home.mkdir()
        bindir = Path(tmp) / "bin"
        bindir.mkdir()
        axon = str(bindir / "axon")
        shutil.copy2(binary, axon)
        env = {k: v for k, v in os.environ.items() if not k.startswith("AXON_") and k != "AXON"}
        env["HOME"] = str(home)
        with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as ex:
            rows = sorted(ex.map(lambda f: check_one(axon, env, f), files))
    with open(out, "w") as fh:
        for rel, rc, code in rows:
            fh.write("%s\t%d\t%s\n" % (rel, rc, code))
    clean = sum(1 for r in rows if r[1] == 0)
    print("compat_sweep: %d files, %d rc 0, %d refused -> %s" % (len(rows), clean, len(rows) - clean, out))
    return 0


def load(path):
    rows = {}
    for n, line in enumerate(Path(path).read_text().splitlines(), 1):
        parts = line.split("\t")
        if len(parts) != 3:
            raise SystemExit("compat_sweep: %s:%d: not `path<TAB>rc<TAB>code`: %r" % (path, n, line))
        rows[parts[0]] = (parts[1], parts[2])
    return rows


def diff(old_path, new_path, behaviour_change):
    old, new = load(old_path), load(new_path)
    refused = accepted = changed = 0
    for path in sorted(old.keys() & new.keys()):
        (o_rc, o_code), (n_rc, n_code) = old[path], new[path]
        if o_rc == "0" and n_rc != "0":
            refused += 1
            print("NEWLY REFUSED\t%s\trc %s -> %s %s" % (path, o_rc, n_rc, n_code))
        elif o_rc != "0" and n_rc == "0":
            accepted += 1
            print("NEWLY ACCEPTED\t%s\trc %s %s -> 0" % (path, o_rc, o_code))
        elif o_code != n_code:
            changed += 1
            print("CODE CHANGED\t%s\trc %s %s -> %s %s" % (path, o_rc, o_code, n_rc, n_code))
    print(
        "compat_sweep: %d newly refused, %d newly accepted, %d first code changed (%d files in both)"
        % (refused, accepted, changed, len(old.keys() & new.keys())),
        file=sys.stderr,
    )
    if refused and not behaviour_change:
        print(
            "compat_sweep: FAIL: %d tracked program(s) newly refused, and the release's CHANGELOG section records "
            "no `**Behaviour change`" % refused,
            file=sys.stderr,
        )
        return 1
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--bin")
    ap.add_argument("--out")
    ap.add_argument("--jobs", type=int, default=min(12, os.cpu_count() or 1))
    ap.add_argument("--diff", nargs=2, metavar=("OLD", "NEW"))
    ap.add_argument("--section-has-behaviour-change", dest="flag", choices=["0", "1"])
    a = ap.parse_args()
    if a.diff:
        if a.flag is None:
            ap.error("--diff needs --section-has-behaviour-change {0,1}")
        return diff(a.diff[0], a.diff[1], a.flag == "1")
    if not (a.bin and a.out):
        ap.error("give --bin AXON --out TSV, or --diff OLD NEW --section-has-behaviour-change {0,1}")
    return sweep(os.path.abspath(a.bin), a.out, a.jobs)


if __name__ == "__main__":
    sys.exit(main())
