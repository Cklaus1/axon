#!/usr/bin/env python3
"""The re-survey record's own refusals (amendments 107 and 110), on synthetic records: no cargo, no survey.

A record the FREEZE trusts must be for this head AND tree, from a clean tree, recent, made by this gate and
tool, drawn by a salt DERIVED from the commit (never read from the record), at or above the floor, with every
drawn entry either KILLED (backed by a log that names the failing test, the binaries and the exit codes) or
NOT RE-MEASURED with a reason and within the cap, and no survivor. Each case below is an ATTACK (one defect, and
the record must be refused for that reason) or the CONTROL (the good record holds). Round 12 planted none of the
forged shapes: a ground salt, a never-run all-KILLED record, an all-NOT-RE-MEASURED record.

    python3 scripts/test_v022_resurvey.py
"""
import copy
import hashlib
import os
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "scripts"))
import v022_resurvey as rs  # noqa: E402

rc, mut = rs.load("v022_refusal_coverage"), rs.load("v022_g01_mutations")
HEAD = rs.head()
NOW = 1_800_000_000
TREE = "7" * 40
LOGS = tempfile.mkdtemp(prefix="resurvey-test-")


def fake_log(key, n):
    """What `log_block` writes for a re-measured entry: the command, the exit code, the binary and the failing test."""
    failing = f"a_test_for_entry_{n}"
    binary = f"-p axon-x --test t{n}"
    cmd = f"cargo test --no-fail-fast {binary}"
    text = f"$ {cmd}\nexit code: 101\n     Running tests/t{n}.rs\n---- {failing} stdout ----\npanicked\n    `{binary}`\n"
    rel = f"{rs.LOG_DIR}/{HEAD[:12]}/{n:03d}-t.log"
    os.makedirs(os.path.dirname(os.path.join(LOGS, rel)), exist_ok=True)
    open(os.path.join(LOGS, rel), "w").write(text)
    return {"key": key, "result": "KILLED", "failing": [failing], "binaries": [f"tests/t{n}.rs", binary],
            "ran": [{"cmd": cmd, "failing": [failing], "base": [], "rc": 101}],
            "log": rel, "log_sha256": hashlib.sha256(text.encode()).hexdigest()}


def build(salt=None, pct=25, mark=None, with_logs=True):
    """A record as the tool would write it (every drawn entry KILLED), for the stated salt."""
    gate = rs.gate_sha()
    salt = salt or rs.derive_salt(HEAD, gate)
    ents = rs.entries(rc)
    keys = rs.draw(ents, salt, pct)
    entries, fam, n = [], {}, 0
    for k, v in ents.items():
        rows = []
        for key in keys[k]:
            n += 1
            r = fake_log(key, n) if with_logs else {"key": key, "result": "KILLED", "failing": ["x"], "binaries": [],
                                                    "ran": [{"cmd": "c", "failing": ["x"], "base": [], "rc": 101}],
                                                    "log": f"{rs.LOG_DIR}/none/{n}.log", "log_sha256": "0" * 64}
            r["family"] = k
            if mark:
                r = mark(r)
            rows.append(r)
        entries += rows
        fam[k] = {"observed": len(v), "drawn": len(rows),
                  "killed": sum(r["result"] == "KILLED" for r in rows), "survived": [],
                  "not_remeasured": sum(r["result"].startswith("NOT RE-MEASURED") for r in rows)}
    total = sum(len(v) for v in ents.values())
    return {"schema": rs.SCHEMA, "tool_version": rs.TOOL_VERSION, "commit": HEAD, "tree_sha": TREE, "tree_clean": True,
            "gate_sha256": gate, "partial": False,
            "sample": {"pct": pct, "salt": salt, "mode": str(pct), "population": total, "floor": rs.floor_for(total),
                       "n": rs.sample_size(total, pct), "rule": ""},
            "estimate_seconds": 1, "seconds": 1.0, "finished_unix": NOW - 3600, "host": "t",
            "families": fam, "entries": entries}


def problems(doc, head=HEAD):
    return rs.problems(doc, head, rc, mut, now=NOW, logs_root=LOGS, tree_fn=lambda c: TREE)


def attack(name, edit, want, doc=None):
    d = copy.deepcopy(doc if doc is not None else GOOD)
    edit(d)
    got = problems(d)
    if not any(want in g for g in got):
        print(f"ATTACK: {name}: the record was not refused for {want!r}: {got[:3]}")
        return 1
    return 0


def recount(d):
    for k, f in d["families"].items():
        rows = [r for r in d["entries"] if r["family"] == k]
        f["killed"] = sum(r["result"] == "KILLED" for r in rows)
        f["not_remeasured"] = sum(r["result"].startswith("NOT RE-MEASURED") for r in rows)


GOOD = build()


def main():
    bad = 0
    p = problems(GOOD)
    if p:
        print(f"control: the good record must hold: {p[:3]}")
        bad += 1
    pop = sum(len(v) for v in rs.entries(rc).values())
    n = len(GOOD["entries"])
    if n != rs.sample_size(pop, 25) or n < rs.floor_for(pop) or rs.floor_for(pop) < max(rs.FLOOR_ABS, pop // 4):
        print(f"ATTACK: the draw is {n} of {pop}; the floor must be max(20, 25%) and the draw above it")
        bad += 1
    bad += attack("a record for another commit", lambda d: d.update(commit="0" * 40), "commit")
    if not any("commit" in g for g in problems(GOOD, head="1" * 40)):
        print("ATTACK: a record checked at another head was not refused")
        bad += 1
    bad += attack("a tree other than its commit's", lambda d: d.update(tree_sha="8" * 40), "tree hash")
    bad += attack("a dirty tree", lambda d: d.update(tree_clean=False), "clean tree")
    bad += attack("a partial run", lambda d: d.update(partial=True), "PARTIAL")
    bad += attack("an old record", lambda d: d.update(finished_unix=NOW - 40 * 86400), "days old")
    bad += attack("another version of the gate", lambda d: d.update(gate_sha256="0" * 64), "another version of the gate")
    bad += attack("a stale tool version", lambda d: d.update(tool_version=1), "tool version")
    bad += attack("a sample below the floor percentage", lambda d: d["sample"].update(pct=5), "at least")
    bad += attack("a different OBSERVED set", lambda d: d["families"]["value"].update(observed=1), "OBSERVED entries, the gate now has")
    bad += attack("a survivor", lambda d: d["families"]["value"].update(survived=["x"]), "now SURVIVE")
    bad += attack("entries the rule does not draw", lambda d: d["entries"].pop(), "not the ones the rule draws")
    bad += attack("counts that do not match the entries", lambda d: d["families"]["py"].update(killed=999), "counts 999 killed")
    bad += attack("another schema", lambda d: d.update(schema="x"), "not a")

    # ── the forged shapes round 12 planted by hand ────────────────────────────
    # 1. a GROUND salt: the record names a salt of its choosing and draws by it, every entry KILLED, logs and all
    ground = build(salt="grind558")
    bad += attack("a ground salt (the draw follows the salt the record names)", lambda d: None,
                  "sampling salt is not the one derived", ground)
    # the same record with the honest salt written over it is still a different draw
    swapped = copy.deepcopy(ground)
    swapped["sample"]["salt"] = rs.derive_salt(HEAD, swapped["gate_sha256"])
    bad += attack("a ground draw under the honest salt's name", lambda d: None,
                  "not the ones the rule draws", swapped)
    # 2. a NEVER-RUN record: every entry KILLED, no log was ever written
    bad += attack("an all-KILLED record nobody ran (no logs)", lambda d: None, "is missing", build(with_logs=False))
    # 3. every drawn entry NOT RE-MEASURED
    def unmeasured(r):
        return {"key": r["key"], "family": r["family"], "result": "NOT RE-MEASURED (no mutation rule for this value)"}
    allnot = build(mark=unmeasured)
    bad += attack("a record in which NOTHING was re-measured", lambda d: None, "the floor is", allnot)
    bad += attack("... and above the NOT RE-MEASURED cap", lambda d: None, "NOT RE-MEASURED; the cap is", allnot)

    def drop_some(d):
        for r in d["entries"][:15]:
            r["result"] = "NOT RE-MEASURED (no mutation rule for this value)"
        recount(d)
    bad += attack("too many NOT RE-MEASURED", drop_some, "the cap is")
    bad += attack("a NOT RE-MEASURED entry with no reason", lambda d: d["entries"][0].update(
        result="NOT RE-MEASURED"), "no reason a reviewer can read")
    bad += attack("an INCONCLUSIVE entry", lambda d: d["entries"][0].update(result="INCONCLUSIVE (hung)"),
                  "neither KILLED nor NOT RE-MEASURED")
    # 4. logs
    altered = build()
    bad += attack("a log that was altered", lambda d: open(os.path.join(LOGS, d["entries"][0]["log"]), "a").write("x"),
                  "altered", altered)
    fresh = build()
    bad += attack("a log that does not name the failing test", lambda d: d["entries"][0].update(failing=["no_such_test"]),
                  "does not mention the failing test", fresh)
    fresh = build()
    bad += attack("a log that does not show the exit code", lambda d: d["entries"][0]["ran"][0].update(rc=0),
                  "does not show the exit code", fresh)
    bad += attack("a log that does not name the binary", lambda d: d["entries"][0].update(binaries=["tests/other.rs"]),
                  "does not name the test binary", fresh)
    bad += attack("a log outside the log directory", lambda d: d["entries"][0].update(log="../../etc/passwd"),
                  "has no log under", fresh)
    bad += attack("a KILLED entry that names no failing test", lambda d: d["entries"][0].update(failing=[]),
                  "names no failing test", fresh)

    # the sample is a function of (salt, key, pct): the same head draws the same entries, another head another
    a = [k for k, _ in rs.plan(rc, "25", "a" * 40)[1]["value"]]
    b = [k for k, _ in rs.plan(rc, "25", "b" * 40)[1]["value"]]
    if a == b or not a:
        print("ATTACK: the sample did not rotate with the salt, or drew nothing")
        bad += 1
    if rs.derive_salt("a" * 40, "g") == rs.derive_salt("b" * 40, "g") or rs.derive_salt("a" * 40, "g") == rs.derive_salt("a" * 40, "h"):
        print("ATTACK: the salt does not move with the commit AND the gate")
        bad += 1
    if rs.plan(rc, "all", HEAD)[2] != 100 or rs.plan(rc, "auto", HEAD)[2] != 25:
        print("ATTACK: `auto` is not 100% under the budget and 25% over it (estimate %s s)" % rs.plan(rc, "auto", HEAD)[4])
        bad += 1
    # a re-run command per named binary (round 12: a survey ran only the last of two)
    cmds = rs.commands_for("axon-fabric", "fails <binary> -p axon-fabric --test privileged_launcher and "
                           "<binary> -p axon-fabric --lib and <binary> -p axon-fabric --test privileged_launcher")
    want = [("cargo", "test", "--no-fail-fast", "-p", "axon-fabric", "--test", "privileged_launcher", "--", *rs.FLAKY),
            ("cargo", "test", "--no-fail-fast", "-p", "axon-fabric", "--lib", "--", *rs.FLAKY),
            ("cargo", "test", "--no-fail-fast", "-p", "axon-fabric", "--", *rs.FLAKY)]
    if cmds != want:
        print(f"ATTACK: commands_for did not return one command per named binary then the crate suite: {cmds}")
        bad += 1
    # the freeze's refusal: no record at all, and a record for another head, each refuse
    why, _ = rs.freeze_refusal(rc, mut, HEAD, path="governance/status/no-such-resurvey.json")
    if not why or "cannot be read" not in why:
        print(f"ATTACK: a freeze with no re-survey record was not refused: {why!r}")
        bad += 1
    why, _ = rs.freeze_refusal(rc, mut, "1" * 40)
    if not why:
        print("ATTACK: a freeze at a head the (absent or other) record is not for was not refused")
        bad += 1
    print("test_v022_resurvey:", "FAIL" if bad else "PASS")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
