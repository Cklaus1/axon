#!/usr/bin/env python3
"""Run ONE package's whole test suite as concurrent PROCESSES, reported as
`cargo test` reports it (governance/notes/v022-fabric-suite-time.md).

    python3 scripts/cargo_test_shards.py [--jobs=N] [--shard-tests=K] \\
        <cargo test args, one -p> [-- <libtest args>]

`cargo test -p P -- --test-threads=1` runs P's test binaries one after
another and each binary's tests one after another: the axon-fabric suite took
~21 min idle that way. Here every test binary is listed (`--list`), its tests
are split into shards of at most K, and each shard is one `cargo test -p P
<target> -- <libtest args> --exact <its tests>` PROCESS; up to N shards run at
once. Inside a process nothing changes (the libtest args, --test-threads=1
included, are passed through), so what a test shares with a sibling in its
own process is exactly what it shared before. What changes is that tests in
DIFFERENT processes now overlap in time: a package is sharded only if its
tests share no state across processes (the audit is in the note above; see
SHARDED_PACKAGES in v022_paired_disable.py).

Evidence is kept, not summarised:
* every listed test is run exactly once, by name; a shard whose libtest did
  not report `running <its count> tests` and a `test result:` line is a
  FAILURE of this run (a test lost is never a pass);
* every shard's whole output is printed, in cargo's target order, so
  `---- name stdout ----` blocks, `name ... FAILED` lines and `test result:`
  lines read exactly as in a serial run;
* doc tests run as their own unit, as cargo would run them;
* exit status is 0 only if every unit exited 0 and every count held, 101
  otherwise (cargo's own failure status); every unit is run even after one
  fails (a serial `cargo test` stops at the first failing binary, so this
  reports at least as much).
On SIGTERM (the caller's wall-clock bound) every running shard's process
group is killed, the finished shards are printed, and each unfinished one is
named as cut; the caller's own status says the bound was hit.
"""
import concurrent.futures
import json
import math
import os
import re
import signal
import subprocess
import sys
import threading
import time

LIB_KINDS = {"lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"}


def split_args(argv):
    jobs, shard = None, 8
    rest = []
    for a in argv:
        if a.startswith("--jobs="):
            jobs = int(a.split("=", 1)[1])
        elif a.startswith("--shard-tests="):
            shard = int(a.split("=", 1)[1])
        else:
            rest.append(a)
    if "--" in rest:
        i = rest.index("--")
        cargo, libtest = rest[:i], rest[i + 1:]
    else:
        cargo, libtest = rest, []
    if cargo.count("-p") + sum(1 for a in cargo if a.startswith("--package")) != 1:
        sys.exit("cargo_test_shards: exactly one -p PACKAGE")
    if jobs is None:
        jobs = int(os.environ.get("AXON_TEST_SHARD_JOBS") or max(2, min(8, (os.cpu_count() or 4) // 4)))
    return max(1, jobs), max(1, shard), cargo, libtest


def targets(cargo):
    """The test executables `cargo test <cargo>` would run, in its order:
    [(selector, executable, cwd)]. Exits with cargo's status if the build
    fails (its output printed)."""
    r = subprocess.run(["cargo", "test", *cargo, "--no-run", "--message-format=json"],
                       capture_output=True, text=True)
    if r.returncode != 0:
        sys.stdout.write(r.stdout)
        sys.stdout.write(r.stderr)
        sys.exit(r.returncode)
    out, has_lib = [], False
    for line in r.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("reason") != "compiler-artifact" or not m.get("executable"):
            continue
        if not m.get("profile", {}).get("test"):
            continue
        t = m["target"]
        kinds = set(t["kind"])
        if kinds & LIB_KINDS:
            sel = ["--lib"]
            has_lib = has_lib or bool(t.get("doctest"))
        elif "bin" in kinds:
            sel = ["--bin", t["name"]]
        elif "test" in kinds:
            sel = ["--test", t["name"]]
        elif "bench" in kinds:
            sel = ["--bench", t["name"]]
        elif "example" in kinds:
            sel = ["--example", t["name"]]
        else:
            sys.exit(f"cargo_test_shards: unknown target kind {t['kind']} ({t['name']})")
        out.append((sel, m["executable"], os.path.dirname(m["manifest_path"])))
    return out, has_lib


def listed(exe, cwd):
    """The test names a libtest binary lists, or None if it cannot list."""
    r = subprocess.run([exe, "--list", "--format", "terse"], cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        return None
    return [ln[: -len(": test")] for ln in r.stdout.splitlines() if ln.endswith(": test")]


def plan(cargo, libtest, shard):
    """Units: (label, argv, expected test count or None)."""
    tgts, has_lib = targets(cargo)
    units = []
    for sel, exe, cwd in tgts:
        names = listed(exe, cwd)
        base = ["cargo", "test", *cargo, *sel, "--", *libtest]
        label = " ".join(sel)
        if not names:
            # Nothing listed (or no libtest list): one unit, as cargo runs it.
            units.append((label, base, None if names is None else 0))
            continue
        k = math.ceil(len(names) / shard)
        per = math.ceil(len(names) / k)
        for i in range(k):
            part = names[i * per:(i + 1) * per]
            units.append((f"{label} [{i + 1}/{k}]", base + ["--exact", *part], len(part)))
    if has_lib:
        units.append(("--doc", ["cargo", "test", *cargo, "--doc", "--", *libtest], None))
    return units


RUNNING = re.compile(r"^running (\d+) tests?$", re.M)


def main():
    jobs, shard, cargo, libtest = split_args(sys.argv[1:])
    units = plan(cargo, libtest, shard)
    results = [None] * len(units)
    live = {}
    lock = threading.Lock()
    cut = threading.Event()

    def run(i):
        if cut.is_set():
            return
        label, argv, _ = units[i]
        t0 = time.monotonic()
        p = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             text=True, errors="replace", start_new_session=True)
        with lock:
            live[i] = p
        out, _ = p.communicate()
        with lock:
            live.pop(i, None)
        results[i] = (p.returncode, out, time.monotonic() - t0)

    def on_term(signum, _frame):
        cut.set()
        with lock:
            procs = list(live.values())
        for p in procs:
            try:
                os.killpg(p.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass

    signal.signal(signal.SIGTERM, on_term)
    signal.signal(signal.SIGINT, on_term)
    # Largest targets first, so the long shards start early.
    order = sorted(range(len(units)), key=lambda i: -(units[i][2] or 0))
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as ex:
        list(ex.map(run, order))

    ok = not cut.is_set()
    for i, (label, argv, want) in enumerate(units):
        res = results[i]
        if res is None:
            sys.stdout.write(f"\n[cargo_test_shards] {label}: CUT before it finished (not run or killed)\n")
            ok = False
            continue
        rc, out, secs = res
        sys.stdout.write(out)
        if not out.endswith("\n"):
            sys.stdout.write("\n")
        sys.stdout.write(f"[cargo_test_shards] {label}: rc={rc} {secs:.1f}s\n")
        # libtest's own count comes first; a test's captured output (shown
        # under --show-output) may echo a nested run's, later.
        ran = [int(n) for n in RUNNING.findall(out)][:1]
        if rc != 0:
            ok = False
        if want is not None and (ran != [want] or "test result:" not in out):
            sys.stdout.write(f"[cargo_test_shards] {label}: expected `running {want} tests` and a "
                             f"result, saw running {ran}: a test lost is never a pass\n")
            ok = False
    total = sum(u[2] or 0 for u in units)
    sys.stdout.write(f"[cargo_test_shards] {len(units)} units, {total} listed tests, "
                     f"jobs={jobs}: {'ok' if ok else 'FAILED'}\n")
    sys.stdout.flush()
    sys.exit(0 if ok else 101)


if __name__ == "__main__":
    main()
