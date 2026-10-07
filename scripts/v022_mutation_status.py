#!/usr/bin/env python3
"""What a freeze requires of the MUTATION-RUN status file (amendment 87).

The freeze bound the registry's digest and counts and nothing else about the
evidence that the active rows are KILLED: a freeze could be cut with no merged
mutation run at HEAD, and `all_killed` was never judged. `problems()` is what
the freeze asks about `governance/status/v022-psv-mutation-run.json`, the file
`v022_g01_mutations.py --merge OUT SHARD...` writes over `--scope=all`:

* produced by `--merge` (`merged_from`, one entry per shard, indices 0..N-1,
  no `shard`, no `only`), schema axon-v022-mutation-run/3, scope `all`, a clean
  tree, this tree's registry/marker blobs;
* its commit is the freeze commit, or an ancestor with only `governance/status/`
  changed since (a status file is committed AFTER the run that made it);
* every shard ran on ONE toolchain, interpreter digest and uid, each naming its
  host (the same refusal `--merge` makes, through the same helper);
* every ACTIVE row (not retired, not sibling-only) is present exactly once with
  the edit digests this registry holds, and is GOOD by the run's own predicate
  (`row_good`: baseline passed, result `killed`, interpreter restored) recomputed
  from the row's recorded results, never from the label: a `refused_elsewhere`,
  `survived`, `not_applicable` or stale row is named with its id;
* LIBRARY_PRIMITIVE rows are present and flagged as such in the file exactly as
  the registry classifies them, and are reported SEPARATELY (`counts()`), never
  added to the killed count;
* `all_killed` is true and equal to the recomputation.

What this is not: the file is operator-attested harness output. The harness is
run by the operator's own (freeze) account and nothing binds a file to a run
cryptographically, so this detects staleness, partial or mixed shards and
internal inconsistency, NOT a file forged by the account that runs the freeze
(amendment 87).
"""
import json
import sys

STATUS_PATH = "governance/status/v022-psv-mutation-run.json"
SCHEMA = "axon-v022-mutation-run/3"


def evidence_commit_problems(commit, head, mut):
    """Why a status file's `commit` is not the freeze commit (empty: it is, or
    differs only by evidence files). One rule for every status file."""
    if commit == head:
        return []
    if not isinstance(commit, str) or mut.sh(f"git cat-file -e {commit}^{{commit}}").returncode != 0:
        return [f"its commit {commit!r} is not in this repository"]
    if mut.sh(f"git merge-base --is-ancestor {commit} {head}").returncode != 0:
        return [f"its commit {commit[:8]} is not an ancestor of the freeze commit {head[:8]}"]
    changed = [f for f in mut.sh(f"git diff --name-only {commit} {head}").stdout.split()
               if not f.startswith("governance/status/")]
    if changed:
        return [f"its commit {commit[:8]} is not the freeze commit {head[:8]}: {len(changed)} "
                f"file(s) other than governance/status/ changed since (first: {changed[0]})"]
    return []


def active_ids(mut):
    return [m[0] for m in mut.MUTATIONS if mut.in_scope(m[0], "all")]


def problems(doc, head, mut):
    """Every defect that stops `doc` from being the merged mutation run for the
    freeze commit `head` (empty: none)."""
    out = []
    if not isinstance(doc, dict) or not isinstance(doc.get("mutations"), list):
        return ["not a mutation-run status file (no `mutations` list)"]
    if doc.get("schema") != SCHEMA:
        out.append(f"schema is {doc.get('schema')!r}, not {SCHEMA}")
    merged = doc.get("merged_from")
    if (not isinstance(merged, list) or not merged or "shard" in doc or doc.get("only") is not None):
        out.append("not produced by `--merge` (a merged run carries `merged_from` and no `shard` "
                   "or `only`): a single shard or a sample is not the whole run")
    else:
        idx = sorted((m.get("shard") or {}).get("index", -1) for m in merged)
        if idx != list(range(len(merged))):
            out.append(f"merged shards are {idx}, not 0..{len(merged) - 1}")
    if doc.get("scope") != "all":
        out.append(f"scope is {doc.get('scope')!r}: a freeze needs the run over `--scope=all`")
    out.extend(evidence_commit_problems(doc.get("commit"), head, mut))
    if doc.get("tree_clean") is not True:
        out.append("it does not record a clean tree")
    if doc.get("registry_blobs") != mut.registry_blobs():
        out.append("it was made from registry/marker blobs other than this tree's")
    envs, tcs = doc.get("environment"), doc.get("toolchain")
    if not isinstance(envs, list) or not isinstance(tcs, list) or len(envs) != len(tcs) or not envs:
        out.append("it does not record each shard's environment and toolchain")
    else:
        why = mut.shard_toolchain_problem([
            (f"shard {i}", (e or {}).get("host"),
             {"axon_bin_sha256": (t or {}).get("axon_bin_sha256", "unrecorded"),
              "rustc": (t or {}).get("rustc"), "cargo": (t or {}).get("cargo"),
              "euid": (e or {}).get("euid"), "etc_axon_present": (e or {}).get("etc_axon_present")})
            for i, (e, t) in enumerate(zip(envs, tcs))])
        if why:
            out.append(why)
    rows = [r for r in doc["mutations"] if isinstance(r, dict)]
    want = active_ids(mut)
    seen = [r.get("id") for r in rows]
    missing = [i for i in want if i not in seen]
    if missing:
        out.append(f"{len(missing)} of {len(want)} active rows are missing (first: {', '.join(missing[:5])})")
    extra = sorted({i for i in seen if i not in want}, key=str)
    if extra:
        out.append(f"rows that are not active here: {extra[:5]}")
    dup = sorted({i for i in seen if seen.count(i) > 1}, key=str)
    if dup:
        out.append(f"duplicate rows: {dup[:5]}")
    by_id = {m[0]: m for m in mut.MUTATIONS}
    bad = {"refused_elsewhere": [], "survived": [], "not_applicable": [], "baseline": [],
           "interpreter": [], "other": []}
    for r in rows:
        rid = r.get("id")
        row = by_id.get(rid)
        if row is None or rid not in want:
            continue
        digest = {k: r.get(k) for k in ("old_sha256", "new_sha256")}
        if digest != mut.row_digest(row):
            out.append(f"{rid}: it ran an edit that is not this registry's row")
        lib = rid in mut.LIBRARY_PRIMITIVE
        if (r.get("status") == "LIBRARY_PRIMITIVE") != lib:
            out.append(f"{rid}: {'is' if lib else 'is not'} a LIBRARY_PRIMITIVE row here and the file "
                       f"{'does not say so' if lib else 'says it is'}")
        if mut.row_good(r.get("baseline"), r.get("result"), r.get("interpreter_not_restored")):
            continue
        res = str(r.get("result"))
        if r.get("baseline") != "passed":
            bad["baseline"].append(rid)
        elif r.get("interpreter_not_restored"):
            bad["interpreter"].append(rid)
        elif res == "refused_elsewhere":
            bad["refused_elsewhere"].append(rid)
        elif res.startswith("survived"):
            bad["survived"].append(rid)
        elif res.startswith("not_applicable"):
            bad["not_applicable"].append(rid)
        else:
            bad["other"].append(rid)
    for k, label in (("refused_elsewhere", "REFUSED_ELSEWHERE (never a kill)"),
                     ("survived", "SURVIVED"), ("not_applicable", "stale or unapplied"),
                     ("baseline", "baseline did not pass"),
                     ("interpreter", "interpreter not restored"), ("other", "not killed")):
        if bad[k]:
            out.append(f"{len(bad[k])} row(s) {label}: {', '.join(bad[k][:8])}")
    recomputed = not any(bad.values())
    if doc.get("all_killed") is not True:
        out.append("all_killed is not true")
    elif not recomputed:
        out.append("all_killed is true over rows that are not all killed")
    return out


def counts(doc, mut):
    """(killed active rows, killed LIBRARY_PRIMITIVE rows) of a file that
    `problems()` accepted: the two are never added."""
    lib = sum(1 for r in doc["mutations"] if r["id"] in mut.LIBRARY_PRIMITIVE)
    return len(doc["mutations"]) - lib, lib


if __name__ == "__main__":
    sys.exit("run as: v022_g01_mutations.py --check-status PATH")
