#!/usr/bin/env python3
"""Cheapest-kill survey of VALUE sites (amendment 103).

For every value site (scripts/v022_refusal_coverage.py `value_sites`) of the given
crate that no row credits and no VALUE_EXEMPT entry names, make the cheapest edit
that CHANGES that one value (a flag spelled differently, "1" -> "0", a mode widened
or a mask emptied, a uid argument replaced by 0, `Some(h.owner)` -> `None`, a
boolean flipped), run the crate's tests, and restore the file. A failing test is a
KILL (the value is observed: the survey records the test names, which is an
OBSERVED entry, not a row). A green suite is a SURVIVOR: it needs a test and a row.
A build that breaks, or a value the survey cannot edit by rule, is reported MANUAL.

    python3 scripts/v022_value_survey.py OUT.json PKG [--shard I/N] [--only SUBSTR] -- cargo-test-args

Run from a clean clone; edits files in place and restores them.
"""
import json
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import v022_refusal_coverage as rc  # noqa: E402

STR = re.compile(r'"((?:[^"\\]|\\.)*)"')


def mutate(label, frag, before):
    """The replacement text for the value `frag`, or None when no rule applies."""
    if label == "val_stdio":
        if "Stdio::piped()" in frag:
            return frag.replace("Stdio::piped()", "Stdio::null()", 1)
        if "Stdio::null()" in frag:
            return frag.replace("Stdio::null()", "Stdio::inherit()", 1)
        return None
    if label in ("val_env", "val_arg", "val_cwd"):
        m = STR.search(frag)
        if not m:
            return None
        c = m.group(1)
        n = {"1": "0", "0": "1"}.get(c, c + "x")
        if label == "val_cwd" and c == "/":
            n = "/tmp"
        return frag[:m.start(1)] + n + frag[m.end(1):]
    if label == "val_mode":
        m = re.fullmatch(r"0o([0-7_]+)", frag)
        if not m:
            return None
        masked = before.rstrip().endswith("&")
        if masked and frag == "0o7777":
            return "0o777"
        if masked:
            return "0o0"
        d = m.group(1)
        if len(d) >= 5:   # a full st_mode constant being compared (0o40000, 0o100644)
            return "0o" + d[:-1] + str((int(d[-1]) + 1) % 8)
        return "0o777" if d != "777" else "0o700"
    if label == "val_priv":
        if frag.strip() == "0":
            return "1"
        return "0"
    if label == "val_owner":
        return "None"
    if label == "val_field":
        if frag == "true":
            return "false"
        if frag == "false":
            return "true"
        m = STR.fullmatch(frag)
        if m:
            return '"' + m.group(1) + 'x"'
        if re.fullmatch(r"\d+", frag):
            return str(int(frag) + 1)
        if frag.endswith(".into()"):
            return '"x".into()'
        if re.fullmatch(r"[A-Z][A-Z0-9_]+", frag):
            return f"({frag} + 1)"
        return None
    return None


def candidates(pkg, only):
    rows = rc.load_rows()
    out = []
    for f in rc.in_scope_files():
        if not f.startswith(f"crates/{pkg}/") or f in rc.OUT_OF_SCOPE or (only and only not in f):
            continue
        text = open(os.path.join(rc.ROOT, f)).read()
        rs = [(r[0], rc.edit_ranges(text, r[3], r[4])) for r in rows if r[2] == f and text.count(r[3]) == 1]
        ex = {(e[1], e[2]) for e in rc.VALUE_EXEMPT if e[0] == f}
        for a, b, label, fn, n in rc.value_sites(text, rc.scope_regions(f, rc.code_lines(text), text, [])):
            if any(rc._ranges_hit(rg, a, b) for _, rg in rs) or (fn, n) in ex:
                continue
            out.append((f, a, b, label, fn, n))
    return out


def main():
    argv = sys.argv[1:]
    out, pkg = argv[0], argv[1]
    split = argv.index("--")
    opts, cargo = argv[2:split], argv[split + 1:]
    shard, only = (0, 1), None
    for i, o in enumerate(opts):
        if o == "--shard":
            k, n = opts[i + 1].split("/")
            shard = (int(k), int(n))
        if o == "--only":
            only = opts[i + 1]
    results = []
    for idx, (f, a, b, label, fn, n) in enumerate(candidates(pkg, only)):
        if idx % shard[1] != shard[0]:
            continue
        path = os.path.join(rc.ROOT, f)
        text = open(path).read()
        frag = text[a:b]
        new = mutate(label, frag, text[max(0, a - 12):a])
        rec = {"file": f, "line": rc.line_of(text, a) + 1, "label": label, "fn": fn, "n": n, "value": frag[:80]}
        if new is None or new == frag:
            rec["result"] = "MANUAL (no mutation rule)"
            results.append(rec)
            print(rec["result"], f, rec["line"], frag[:40], flush=True)
            continue
        rec["edit"] = new[:80]
        open(path, "w").write(text[:a] + new + text[b:])
        try:
            try:
                r = subprocess.run(["cargo", "test", "-p", pkg, "--no-fail-fast", *cargo], capture_output=True,
                                   text=True, timeout=2400)
            except subprocess.TimeoutExpired:
                r = subprocess.CompletedProcess([], 124, "", "hung")
        finally:
            open(path, "w").write(text)
        tail = r.stdout + r.stderr
        failing = sorted(set(re.findall(r"^---- (\S+) stdout ----$", tail, re.M)))
        # `error[E....]` is the AXON interpreter's own diagnostic, printed by a test that
        # runs `axon test`: only cargo's "could not compile" is a build failure.
        built = "could not compile" not in tail
        if r.returncode == 124:
            rec["result"] = "INCONCLUSIVE (hung)"
        elif not built:
            rec["result"] = "BUILD BROKE"
        elif r.returncode != 0:
            rec["result"] = "KILLED"
            rec["failing"] = failing[:6]
        else:
            rec["result"] = "SURVIVED"
        results.append(rec)
        print(rec["result"], f, rec["line"], label, frag[:40], "->", new[:40], rec.get("failing", ""), flush=True)
        json.dump(results, open(out, "w"), indent=1)
    json.dump(results, open(out, "w"), indent=1)


if __name__ == "__main__":
    main()
