#!/usr/bin/env python3
"""ae_weekly.py - the weekly CERE screen of the authoring-eval (tasks/authoring-eval-spec.md AE-7).

Usage:
  python3 -B scripts/ae_weekly.py run [--date YYYY-MM-DD] [--no-window] [--repo DIR] [--ref REF] [--no-fetch]
      [--set-dir DIR] [--seeds 0,1,2] [--parallel 3] [--history FILE] [--prereg FILE] [--genviab DIR]
  python3 -B scripts/ae_weekly.py cache-bins BIN_DIR

`run` compares the last labelled release (the newest `design: paired` row of history.jsonl whose label is not
`weekly-*`, on PREREG-ae.md's set version) with `main` HEAD: ae.py run --label weekly-<date> --advisory
--parallel 3 --until 20:00 on CERE (`CERE_TOKEN_AE`, NH-6). It is started Saturday 12:00 by ae-weekly.timer and
refuses to start outside Saturday 12:00-20:00 (--no-window: a hand run, still never 01:30-08:00, which ae.py
enforces). ae.py stops dispatching at 20:00 and then writes no trend row (INCOMPLETE).

Binaries are cached by commit under $AE_PRIVATE/bins/<sha8>/ (sha8 = the commit `axon --version` prints). The
base is never rebuilt: its cached `axon` must hash to the release row's `cand.axon_sha256` (the integrator
caches a release's binaries with `cache-bins`). A candidate commit not cached yet is built once from a clean
worktree ($AE_PRIVATE/bins/.build/wt) into a dedicated CARGO_TARGET_DIR ($AE_PRIVATE/bins/.build/target).
When `main` HEAD is the release commit (no merged change) both sides use the same binaries: with the reply
cache (AE-1) the run makes no cache-miss call apart from the replay sample, and shows NO CHANGE.

It never starts a confirm run. The rerun trigger (spec §3: the CI excludes 0, |Δ| >= 2.5 x PREREG's A/A null
SD, or an M2s instance whose (family, panic kind) never appeared in a history.jsonl run of this set version)
goes into the digest $AE_PRIVATE/weekly/<date>.md for the integrator.

Exit: 0 trend row written; 1 no trend row (INCOMPLETE, INVALID, canary); 2 refused (window, no release,
base binaries missing or changed, build failed, another weekly running).
"""
import argparse
import datetime
import fcntl
import json
import os
import shutil
import subprocess
import sys

SCRIPTS = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(SCRIPTS)
GENVIAB = os.path.join(REPO, "crates", "axon-intent", "benchmarks", "genviab")
PRIVATE = os.environ.get("AE_PRIVATE", "/root/axon-ae")
CARGO = os.environ.get("AE_WEEKLY_CARGO", "cargo")
WINDOW = (datetime.time(12, 0), datetime.time(20, 0))
SATURDAY = 5
PARALLEL = 3
RERUN_SD = 2.5


class Refused(Exception):
    pass


def git(repo, *args):
    """git as root on a checkout another user owns (the timer runs as root)."""
    r = subprocess.run(["git", "-c", "safe.directory=*", "-C", repo, *args], capture_output=True, text=True)
    if r.returncode != 0:
        raise Refused(f"git {' '.join(args)} failed in {repo}: {r.stderr.strip()[:300]}")
    return r.stdout.strip()


def check_window(now):
    if now.weekday() != SATURDAY or not WINDOW[0] <= now.time() < WINDOW[1]:
        raise Refused(f"the weekly screen runs Saturday {WINDOW[0]:%H:%M}-{WINDOW[1]:%H:%M} only "
                      f"(now {now:%a %H:%M}); --no-window for a hand run")


def last_release(history, set_version):
    rel = [h for h in history if h.get("design") == "paired" and (h.get("set") or {}).get("version") == set_version
           and not h.get("label", "").startswith("weekly-")]
    if not rel:
        raise Refused(f"no labelled release run of set {set_version} in history.jsonl: the integrator's two hand "
                      "release runs come first (spec §7)")
    return rel[-1]


def bins_dir(sha8):
    return os.path.join(PRIVATE, "bins", sha8)


def version_sha8(axon):
    r = subprocess.run([axon, "--version"], capture_output=True, text=True, timeout=30)
    v = r.stdout.strip()
    return v[v.rfind("(") + 1:v.rfind(")")] if "(" in v else None


def cache_bins(src):
    """Copy a release's binaries into the cache under the commit `axon --version` names."""
    import hashlib
    for n in ("axon", "axon-intent"):
        if not os.access(os.path.join(src, n), os.X_OK):
            raise Refused(f"{src}/{n} is not an executable")
    sha8 = version_sha8(os.path.join(src, "axon"))
    if not sha8:
        raise Refused(f"{src}/axon --version names no commit")
    dst = bins_dir(sha8)
    if os.path.exists(dst):
        raise Refused(f"{dst} exists: cached binaries are never replaced")
    tmp = dst + ".tmp"
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    for n in ("axon", "axon-intent"):
        shutil.copy2(os.path.join(src, n), os.path.join(tmp, n))
    os.rename(tmp, dst)
    print(f"ae_weekly: cached {sha8} axon "
          f"{hashlib.sha256(open(os.path.join(dst, 'axon'), 'rb').read()).hexdigest()[:12]}")
    return dst


def base_bins(rel, ae):
    sha8 = rel["cand"]["git_head"]
    d = bins_dir(sha8 or "?")
    if not sha8 or not os.path.isfile(os.path.join(d, "axon")):
        raise Refused(f"the release {rel['label']}'s binaries are not cached at {d} (the base is never "
                      "rebuilt: `ae_weekly.py cache-bins <release bin dir>`)")
    got = ae.gv.sha256_file(os.path.join(d, "axon"))
    if got != rel["cand"]["axon_sha256"]:
        raise Refused(f"{d}/axon is {got[:12]}, not the release row's {rel['cand']['axon_sha256'][:12]}")
    return d


def build_bins(repo, sha):
    """Build `sha` from a clean worktree into the dedicated target dir; cache under sha8."""
    sha8 = sha[:8]
    dst = bins_dir(sha8)
    if os.path.isfile(os.path.join(dst, "axon")) and os.path.isfile(os.path.join(dst, "axon-intent")):
        return dst, False
    build = os.path.join(PRIVATE, "bins", ".build")
    wt, target = os.path.join(build, "wt"), os.path.join(build, "target")
    os.makedirs(build, exist_ok=True)
    if os.path.exists(wt):
        subprocess.run(["git", "-c", "safe.directory=*", "-C", repo, "worktree", "remove", "--force", wt],
                       capture_output=True)
        shutil.rmtree(wt, ignore_errors=True)
    git(repo, "worktree", "prune")
    git(repo, "worktree", "add", "-q", "--detach", "--force", wt, sha)
    env = dict(os.environ, CARGO_TARGET_DIR=target)
    r = subprocess.run([CARGO, "build", "-q", "--locked", "-p", "axon-core", "--bin", "axon",
                        "-p", "axon-intent", "--bin", "axon-intent"], cwd=wt, env=env,
                       capture_output=True, text=True)
    if r.returncode != 0:
        raise Refused(f"building {sha8} failed (rc {r.returncode}): {(r.stdout + r.stderr)[-600:]}")
    tmp = dst + ".tmp"
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    for n in ("axon", "axon-intent"):
        shutil.copy2(os.path.join(target, "debug", n), os.path.join(tmp, n))
    got = version_sha8(os.path.join(tmp, "axon"))
    if got != sha8:
        shutil.rmtree(tmp)
        raise Refused(f"the build of {sha8} reports {got!r} in --version (not a clean build of that commit)")
    os.rename(tmp, dst)
    return dst, True


def aa_null_sd(prereg_path):
    import re
    text = open(prereg_path).read() if os.path.exists(prereg_path) else ""
    m = re.search(r"^- A/A null SD: `([0-9.]+)`", text, re.M)
    return float(m.group(1)) if m else None


def m2s_pairs(ae, runs, run_id):
    """(family, panic kind) of every M2s instance (type-class panic of a check-clean candidate) in one run."""
    out = set()
    for b in ("base", "cand"):
        for r in ae.read_rows(ae.run_paths(runs, run_id)["rows"][b]):
            for g in (r.get("b_grade") or {}).get("runs") or []:
                if g.get("ended_class") == "panic" and g.get("panic_kind") == "type-class":
                    out.add((r.get("family") or r["id"], g["panic_kind"]))
    return out


def rerun_triggers(row, sd, new_m2s):
    """Spec §3: why this screen asks the integrator for a confirm-style rerun ([] = none)."""
    why = []
    p = row.get("paired")
    if p:
        lo, hi = p["ci95"]
        if lo > 0 or hi < 0:
            why.append(f"the CI [{lo:+.3f}, {hi:+.3f}] excludes 0")
        if sd and abs(p["delta_ft_pass"]) >= RERUN_SD * sd:
            why.append(f"|Δ| {abs(p['delta_ft_pass']):.3f} >= {RERUN_SD} x the A/A null SD {sd}")
    if new_m2s:
        why.append(f"{len(new_m2s)} M2s instance(s) with a (family, panic kind) never seen on this set version "
                   "(listed by `ae.py classes`)")
    return why


def cmd_run(a):
    sys.path.insert(0, a.genviab)
    import ae
    now = datetime.datetime.now()
    if not a.no_window:
        check_window(now)
    date = a.date or now.date().isoformat()
    history_path = a.history or ae.HISTORY
    prereg = a.prereg or ae.PREREG_AE
    pins = ae.prereg_pins(prereg)
    if not pins["frozen"]:
        raise Refused(f"{prereg} is not FROZEN: the weekly screen runs on the frozen set only")
    history = ae.read_history(history_path)
    rel = last_release(history, pins["set_version"])
    base = base_bins(rel, ae)
    if not a.no_fetch:
        git(a.repo, "fetch", "-q", "origin", "main")
    head = git(a.repo, "rev-parse", "--verify", a.ref + "^{commit}")
    merged = head[:8] != rel["cand"]["git_head"]
    built = False
    if merged:
        cand, built = build_bins(a.repo, head)
    else:
        cand = base
    runs = os.path.join(PRIVATE, "runs")
    before = set(os.listdir(runs)) if os.path.isdir(runs) else set()
    cmd = [sys.executable, "-B", os.path.join(a.genviab, "ae.py"), "run", "--set-dir", a.set_dir,
           "--label", f"weekly-{date}", "--advisory", "--base-bin", base, "--cand-bin", cand,
           "--seeds", a.seeds, "--parallel", str(a.parallel), "--history", history_path, "--prereg", prereg,
           "--runs-dir", runs]
    if not a.no_window:
        cmd += ["--until", f"{WINDOW[1]:%H:%M}"]
    r = subprocess.run(cmd, capture_output=True, text=True)
    new = sorted(set(os.listdir(runs)) - before) if os.path.isdir(runs) else []
    run_id = next((x for x in new if x.endswith(f"weekly-{date}")), None)
    row = None
    if run_id and os.path.exists(ae.run_paths(runs, run_id)["row"]):
        row = json.load(open(ae.run_paths(runs, run_id)["row"]))
    written = r.returncode == 0 and row is not None and any(
        h.get("run_id") == run_id for h in ae.read_history(history_path))
    commits = git(a.repo, "rev-list", "--count", f"{rel['cand']['git_head']}..{head}") if merged else "0"
    lines = [f"# ae weekly screen {date}", "",
             f"- base: release `{rel['label']}` ({rel['cand']['git_head']}); candidate: `{a.ref}` {head[:8]} "
             f"({commits} commit(s) since the release{'; built now' if built else ''})",
             f"- run: {run_id or '(none)'}; ae.py exit {r.returncode}; "
             + ("trend row written (advisory)" if written else "NO trend row")]
    if row:
        c = row["cache"]
        lines.append(f"- flag (advisory, never blocks): {row['flag']}; cache-miss calls {c['misses']} "
                     f"(replay sample {c['replayed']}, hits {c['hits']}); calls {row['cost']['calls']}")
        if not merged and c["misses"] > c["replayed"]:
            lines.append("- WARNING: no merged change, yet calls missed the cache beyond the replay sample "
                         "(AE-1 cache key drift?)")
        if row.get("notes"):
            lines.append(f"- notes: {row['notes']}")
        seen = set()
        for h in history:
            if (h.get("set") or {}).get("version") == pins["set_version"] and h.get("run_id") != run_id:
                seen |= m2s_pairs(ae, runs, h["run_id"])
        why = rerun_triggers(row, aa_null_sd(prereg), m2s_pairs(ae, runs, run_id) - seen)
        lines.append("- rerun trigger: " + ("; ".join(why) + " -> the integrator decides on a confirm run "
                                            "(`ae.py run --confirm " + run_id + "`); none was started"
                                            if why else "none"))
    lines += ["", "## ae.py output", "", "```", (r.stdout + r.stderr).strip()[-6000:], "```", ""]
    digest = os.path.join(PRIVATE, "weekly", f"{date}.md")
    os.makedirs(os.path.dirname(digest), exist_ok=True)
    with open(digest, "w") as fh:
        fh.write("\n".join(lines))
    print("\n".join(lines[:8]))
    print(f"ae_weekly: digest {digest}")
    return 0 if written else 1


def main(argv=None):
    ap = argparse.ArgumentParser(usage=__doc__.split("\n\n")[1])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--date")
    r.add_argument("--no-window", action="store_true")
    r.add_argument("--repo", default=REPO)
    r.add_argument("--ref", default="origin/main")
    r.add_argument("--no-fetch", action="store_true")
    r.add_argument("--set-dir", default=os.path.join(PRIVATE, "ae-1"))
    r.add_argument("--seeds", default="0,1,2")
    r.add_argument("--parallel", type=int, default=PARALLEL)
    r.add_argument("--history")
    r.add_argument("--prereg")
    r.add_argument("--genviab", default=GENVIAB)
    c = sub.add_parser("cache-bins")
    c.add_argument("bin_dir")
    try:
        a = ap.parse_args(argv)
    except SystemExit as e:
        return 0 if e.code == 0 else 2
    try:
        os.makedirs(os.path.join(PRIVATE, "bins"), exist_ok=True)
        with open(os.path.join(PRIVATE, "bins", ".weekly.lock"), "a") as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError:
                raise Refused("another ae_weekly is running")
            if a.cmd == "cache-bins":
                cache_bins(a.bin_dir)
                return 0
            return cmd_run(a)
    except Refused as e:
        print(f"ae_weekly: REFUSED: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
