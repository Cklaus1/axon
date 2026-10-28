#!/usr/bin/env python3
"""Self-test of build.py's exit status and arguments: no axon or cortex runs.

    python3 -B selftest_check.py

Both binaries are `/bin/false`, so every spec that reaches a binary fails,
and nothing here depends on a build:
  (a) `--check` on a spec whose edit does not occur exits 1 (it fails in
      `validate` before any binary runs);
  (b) `--check` with no FILE checks every `defects/*.json`: one FAIL line
      per spec in those files, exit 1;
  (c) `--help` exits 0 and `--bogus` exits 2, and neither enters build mode
      (which re-mines and rewrites the sets).

Exit 0 when every check holds; otherwise the failures are listed and the exit
code is 1.
"""
import glob
import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
BUILD = os.path.join(HERE, "build.py")
BUILD_MODE = "is not a green snapshot"
ENV = dict(os.environ, AXON_BIN="/bin/false", CORTEX_BIN="/bin/false")

FAILS = []


def check(cond, what):
    if not cond:
        FAILS.append(what)


def build(*args):
    r = subprocess.run([sys.executable, "-B", BUILD, *args], env=ENV,
                       capture_output=True, text=True, timeout=300)
    return r.returncode, r.stdout + r.stderr


def main():
    specs = sorted(glob.glob(os.path.join(HERE, "defects", "*.json")))
    with tempfile.TemporaryDirectory(prefix="hardset_selftest_") as tmp:
        broken = json.load(open(specs[0]))["defects"][0]
        broken["edits"] = [dict(broken["edits"][0], old="THIS TEXT DOES NOT OCCUR")]
        path = os.path.join(tmp, "broken.json")
        json.dump({"defects": [broken]}, open(path, "w"))
        rc, out = build("--check", path)
        check(rc == 1, f"(a) --check on a failing spec: rc {rc}, want 1")
        check("FAIL broken.json#0" in out and "occurs 0 times" in out,
              f"(a) --check on a failing spec: no FAIL line for it:\n{out}")

    want = sum(len(json.load(open(p))["defects"]) for p in specs)
    rc, out = build("--check")
    fails = [ln for ln in out.splitlines() if ln.startswith("FAIL ")]
    check(rc == 1, f"(b) --check with no FILE: rc {rc}, want 1")
    check(len(fails) == want,
          f"(b) --check with no FILE: {len(fails)} FAIL lines, want one per spec ({want})")

    for args, want_rc in ((["--help"], 0), (["-h"], 0), (["--bogus"], 2),
                          (["--check", "--bogus"], 2)):
        rc, out = build(*args)
        check(rc == want_rc, f"(c) build.py {' '.join(args)}: rc {rc}, want {want_rc}")
        check(BUILD_MODE not in out, f"(c) build.py {' '.join(args)} entered build mode")

    for f in FAILS:
        print(f"FAIL {f}", file=sys.stderr)
    print(f"hardset/selftest_check.py: {'FAILED' if FAILS else 'ok'}", file=sys.stderr)
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
