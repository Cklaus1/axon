#!/usr/bin/env python3
"""Protected task sets: their digest pins and the spent ledger.

A protected set is one whose every paid use leaks a little of it, so every
paid use is recorded. `PROTECTED_SETS` maps each to the sha256 of its corpus
file (None until the set is frozen). A corpus is protected if its sha256 is a
pin (a copy elsewhere is still the set) or its path is the set's registered
path (`SET_PATHS`); a registered path whose bytes no longer match the pin is
refused, never read as unprotected.

`heldout-spent.jsonl` (here) is the ledger: one JSON object per paid run on a
protected set, appended by `ab/run_ab.py --spend` BEFORE the first model call,
each naming the sha256 of the row before it (`row_sha`, canonical JSON), so an
edited, deleted or reordered row breaks the chain. `scorecard.py add` on a
protected set requires the rows to name their ledger row. The gate (`--check`)
verifies the pins, the chain, and that the working ledger begins with the
ledger committed at HEAD (no committed row was rewritten or dropped).

Honest limits: the ledger guards a run made through run_ab.py on this
checkout. A copy of the corpus edited by one byte, or a run against another
ledger file, is not caught here; a rewrite of the LAST row before it is
committed is caught only through the rows that name it.

    python3 -B protected.py --check     (exit 0 ok, 1 refused)
"""
import fcntl
import hashlib
import json
import os
import subprocess
import sys

BENCH = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(BENCH)))
LEDGER = os.path.join(BENCH, "heldout-spent.jsonl")

# set -> sha256 of its corpus file; None: not frozen yet (hx-test is pinned
# when it is built and frozen).
PROTECTED_SETS = {
    "hx-test": None,
    "hard-once": "3ebb8d12211168d1fe11809badad7345532e7ba01a825b3c1a9131889ff8c8d6",
}
# set -> corpus path relative to this directory.
SET_PATHS = {
    "hx-dev": "headroom/hx-dev.json",
    "hx-test": "headroom/hx-test.json",
    "hard-once": "hardset/hard-once.json",
}
# Every field of a ledger row; `rerun_of` and `reason` only on a rerun.
FIELDS = ("ts", "set", "corpus_sha256", "purpose", "repair_digest", "patch_sha256",
          "cortex_sha256", "axon_sha256", "grade_axon_sha256", "generator", "model", "k",
          "out", "git_head", "arm", "prev_row_sha256")
RERUN_FIELDS = ("rerun_of", "reason")
# One spend per key; a second needs --rerun-of the latest row with that key.
SPEND_KEY = ("set", "repair_digest", "patch_sha256", "model", "purpose")
# What a resumed run must share with the row it resumes.
RESUME_SAME = ("set", "corpus_sha256", "purpose", "repair_digest", "patch_sha256",
               "cortex_sha256", "axon_sha256", "grade_axon_sha256", "generator", "model", "k",
               "out", "arm")


class ProtectedError(Exception):
    pass


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for b in iter(lambda: fh.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def check_pin(name, path):
    """Refuse a protected set's corpus whose bytes are not its pin."""
    pin = PROTECTED_SETS.get(name)
    if pin is not None and sha256_file(path) != pin:
        raise ProtectedError(f"{path} is the {name} set's corpus but its sha256 "
                             f"{sha256_file(path)} is not the pin {pin} (protected.py): a "
                             "protected set is never rebuilt in place")


def set_of(path, bench=BENCH):
    """The registered set a corpus file is, by path or by pinned digest; None."""
    real = os.path.realpath(path)
    for name, rel in SET_PATHS.items():
        if os.path.realpath(os.path.join(bench, rel)) == real:
            return name
    sha = sha256_file(path)
    for name, pin in PROTECTED_SETS.items():
        if pin is not None and pin == sha:
            return name
    return None


def protected_set(path, bench=BENCH):
    """The protected set `path` is (pin-checked), else None."""
    name = set_of(path, bench)
    if name not in PROTECTED_SETS:
        return None
    check_pin(name, path)
    return name


def row_sha(row):
    return hashlib.sha256(json.dumps(row, sort_keys=True, separators=(",", ":"))
                          .encode()).hexdigest()


def read_ledger(path):
    """Every row, after verifying each names the sha256 of the one before."""
    if not os.path.exists(path):
        raise ProtectedError(f"ledger {path} is missing: the committed ledger is the chain's start")
    rows, prev = [], None
    with open(path) as fh:
        for n, line in enumerate(fh, 1):
            if not line.strip():
                continue
            try:
                r = json.loads(line)
            except ValueError:
                raise ProtectedError(f"ledger {path} line {n}: not JSON") from None
            if not isinstance(r, dict):
                raise ProtectedError(f"ledger {path} line {n}: not an object")
            missing = [k for k in FIELDS if k not in r]
            extra = sorted(set(r) - set(FIELDS) - set(RERUN_FIELDS))
            if missing or extra:
                raise ProtectedError(f"ledger {path} line {n}: missing {missing}, unknown {extra}")
            if r["prev_row_sha256"] != prev:
                raise ProtectedError(f"ledger {path} line {n}: prev_row_sha256 "
                                     f"{r['prev_row_sha256']} is not the sha256 of the row "
                                     f"before it ({prev}): a row was edited, dropped or moved")
            rows.append(r)
            prev = row_sha(r)
    return rows


def find(rows, sha):
    for r in rows:
        if row_sha(r) == sha:
            return r
    return None


def spend(path, row, rerun_of=None, reason=None):
    """Append `row` (FIELDS but ts/prev filled here) under an exclusive lock;
    return its row sha. Refuses a second spend of one SPEND_KEY unless it is a
    `rerun_of` the latest row with that key, with a reason."""
    import datetime
    with open(path, "a+") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        rows = read_ledger(path)
        key = tuple(row.get(k) for k in SPEND_KEY)
        same = [r for r in rows if tuple(r[k] for k in SPEND_KEY) == key]
        row = dict(row)
        if rerun_of is None:
            if same:
                raise ProtectedError(
                    f"already spent: {dict(zip(SPEND_KEY, key))} is ledger row "
                    f"{row_sha(same[-1])} ({same[-1]['ts']}, {same[-1]['out']}). Continue it with "
                    "--resume ROW_SHA, or after an infrastructure fault --rerun-of ROW_SHA REASON")
        else:
            orig = find(rows, rerun_of)
            if orig is None:
                raise ProtectedError(f"--rerun-of {rerun_of}: no ledger row has that sha256")
            if not same or row_sha(same[-1]) != rerun_of:
                raise ProtectedError(f"--rerun-of {rerun_of}: not the latest ledger row for "
                                     f"{dict(zip(SPEND_KEY, key))}"
                                     + (f" (that is {row_sha(same[-1])})" if same else ""))
            if not reason or not reason.strip():
                raise ProtectedError("--rerun-of needs a reason")
            row.update(rerun_of=rerun_of, reason=reason.strip())
        if any(r["out"] == row.get("out") for r in rows):
            raise ProtectedError(f"out {row.get('out')} already belongs to a ledger row")
        row["ts"] = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
        row["prev_row_sha256"] = row_sha(rows[-1]) if rows else None
        missing = [k for k in FIELDS if k not in row]
        if missing:
            raise ProtectedError(f"ledger row lacks {missing}")
        with open(path, "a") as fh:
            fh.write(json.dumps(row, sort_keys=True) + "\n")
            fh.flush()
            os.fsync(fh.fileno())
        return row_sha(row)


def resume(path, sha, ident, done, total):
    """The ledger row `sha`, if this run (`ident`) is the same run and its
    out file holds fewer than n*k rows (`done` < `total`)."""
    r = find(read_ledger(path), sha)
    if r is None:
        raise ProtectedError(f"--resume {sha}: no ledger row has that sha256")
    for k in RESUME_SAME:
        if k in ident and ident[k] != r[k]:
            raise ProtectedError(f"--resume {sha}: this run differs in {k}: ledger {r[k]!r}, "
                                 f"this run {ident[k]!r}")
    if done >= total:
        raise ProtectedError(f"--resume {sha}: the run is complete ({done} of {total} rows)")
    return r


def repair_digest_at(commit):
    """The scorecard's repair digest at `commit` (git)."""
    sys.path.insert(0, os.path.join(BENCH, "scorecard"))
    import scorecard
    try:
        return scorecard.repair_digest(ROOT, commit)
    except scorecard.Refused as e:
        raise ProtectedError(str(e)) from None


def check(bench=BENCH, ledger=LEDGER, root=ROOT):
    """The gate: pins, the ledger chain, the committed ledger as a prefix,
    and every registered corpus present loads. Returns report lines."""
    out = []
    for name, pin in PROTECTED_SETS.items():
        p = os.path.join(bench, SET_PATHS[name])
        if pin is None:
            out.append(f"{name}: not frozen (no pin)"
                       + (f"; {SET_PATHS[name]} sha256 {sha256_file(p)}" if os.path.exists(p) else ""))
            continue
        if not os.path.exists(p):
            raise ProtectedError(f"{name}: pinned corpus {p} is missing")
        check_pin(name, p)
        out.append(f"{name}: pin {pin[:16]} matches")
    rows = read_ledger(ledger)
    out.append(f"ledger {os.path.basename(ledger)}: {len(rows)} row(s), chain verified")
    if root:
        rel = os.path.relpath(os.path.realpath(ledger), os.path.realpath(root))
        g = subprocess.run(["git", "-C", root, "show", f"HEAD:{rel}"], capture_output=True)
        if g.returncode == 0:
            with open(ledger, "rb") as fh:
                if not fh.read().startswith(g.stdout):
                    raise ProtectedError(f"ledger {rel}: does not begin with the ledger committed "
                                         "at HEAD: a committed row was rewritten or dropped")
            committed = g.stdout.count(b"\n")
            out.append(f"ledger {rel}: begins with HEAD's ({committed} committed row(s))")
        else:
            out.append(f"ledger {rel}: not in HEAD yet (no committed prefix to check)")
    sys.path.insert(0, os.path.join(bench, "ab"))
    sys.path.insert(0, bench)
    import corpus as corpus_mod
    for name, rel in SET_PATHS.items():
        p = os.path.join(bench, rel)
        if os.path.exists(p):
            n = len(corpus_mod.load(p)["tasks"])
            out.append(f"{name}: {rel} loads ({n} tasks)")
    return out


def main():
    if sys.argv[1:] != ["--check"]:
        print(__doc__.strip().splitlines()[-1].strip(), file=sys.stderr)
        return 2
    try:
        for line in check():
            print(line)
    except ProtectedError as e:
        print(f"protected.py --check: REFUSED: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
