#!/usr/bin/env python3
"""Run ONE package's whole test suite as concurrent PROCESSES, reported as
`cargo test` reports it (governance/notes/v022-fabric-suite-time.md).

    python3 scripts/cargo_test_shards.py [--jobs=N] [--shard-tests=K] \\
        <cargo test args, one -p> [-- <libtest args>]

`cargo test -p P -- --test-threads=1` runs P's test binaries one after
another and each binary's tests one after another: the axon-fabric suite took
~21 min idle that way. Here every test binary is listed (`--list`), its tests
are split into shards of at most K, and each shard is one PROCESS of the
ALREADY-BUILT test binary, `<exe> <libtest args> --exact <its tests>`; up to N
shards run at once. Cargo runs ONCE, up front, to build (`--no-run`) and to
record, per test binary, the environment and working directory cargo itself
gives a test process (a stub runner dumps them; nothing is guessed), so no
shard can re-invoke cargo and RELINK a binary its siblings are running
(C9 shardflake, amendment 77: a touched `.git/index` made a later shard's
cargo replace the file under running shards, and tests read `... (deleted)`). Inside a process nothing changes (the libtest args, --test-threads=1
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
import shutil
import signal
import subprocess
import sys
import tempfile
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


STUB = "env -0 > $1/${2##*/}.env; pwd > $1/${2##*/}.cwd"
SELECTORS = {"--lib", "--bin", "--bins", "--test", "--tests", "--bench", "--benches",
             "--example", "--examples", "--all-targets", "--doc"}


def captured_environments(cargo, tgts, outdir):
    """{executable: (env dict, cwd)}: what `cargo test` gives each test binary
    at RUNTIME (CARGO_MANIFEST_DIR, CARGO_PKG_*, CARGO_PRIMARY_PACKAGE,
    CARGO_CRATE_NAME, CARGO, library paths, ...), read from cargo itself by
    running the same cargo test with a runner that records its environment
    instead of running the binary. Nothing is rebuilt (it was just built). A
    binary whose environment was not captured is a failure of this run."""
    sel = cargo if any(a.split("=", 1)[0] in SELECTORS for a in cargo) \
        else [*cargo, "--lib", "--bins", "--tests"]
    runner = json.dumps(["sh", "-c", STUB, "x", outdir])
    r = subprocess.run(["cargo", "test", *sel, "--no-fail-fast", "--config",
                        f"target.'cfg(all())'.runner={runner}"],
                       capture_output=True, text=True)
    if r.returncode != 0:
        sys.stdout.write(r.stdout)
        sys.stdout.write(r.stderr)
        sys.exit(r.returncode or 101)
    got = {}
    for _, exe, _ in tgts:
        base = os.path.join(outdir, os.path.basename(exe))
        try:
            raw = open(base + ".env", "rb").read()
            cwd = open(base + ".cwd").read().strip()
        except OSError:
            sys.exit(f"cargo_test_shards: no cargo environment captured for {exe}: "
                     "a binary run without cargo's environment is not the same test run")
        env = {}
        for kv in raw.split(b"\0"):
            if b"=" in kv:
                k, v = kv.split(b"=", 1)
                env[os.fsdecode(k)] = os.fsdecode(v)
        # The stub is a shell started in the package root: it rewrote PWD and
        # `_`. Cargo leaves both as its own parent had them.
        for k in ("PWD", "_", "OLDPWD", "SHLVL"):
            if k in os.environ:
                env[k] = os.environ[k]
            else:
                env.pop(k, None)
        got[exe] = (env, cwd)
    return got


def listed(exe, cwd, env=None):
    """The test names a libtest binary lists, or None if it cannot list."""
    r = subprocess.run([exe, "--list", "--format", "terse"], cwd=cwd, env=env, capture_output=True, text=True)
    if r.returncode != 0:
        return None
    return [ln[: -len(": test")] for ln in r.stdout.splitlines() if ln.endswith(": test")]


def plan(cargo, libtest, shard, outdir):
    """Units: (label, argv, expected test count or None, env, cwd). The test
    units exec the built binary; only the doc-test unit goes through cargo."""
    tgts, has_lib = targets(cargo)
    envs = captured_environments(cargo, tgts, outdir)
    # What cargo itself forwards to libtest: `-q` makes the test binary quiet.
    quiet = ["-q"] if any(a in ("-q", "--quiet") for a in cargo) else []
    units = []
    for sel, exe, _ in tgts:
        env, cwd = envs[exe]
        names = listed(exe, cwd, env)
        base = [exe, *quiet, *libtest]
        label = " ".join(sel)
        if not names:
            # Nothing listed (or no libtest list): one unit, as cargo runs it.
            units.append((label, base, None if names is None else 0, env, cwd))
            continue
        k = math.ceil(len(names) / shard)
        per = math.ceil(len(names) / k)
        for i in range(k):
            part = names[i * per:(i + 1) * per]
            units.append((f"{label} [{i + 1}/{k}]", base + ["--exact", *part], len(part), env, cwd))
    if has_lib:
        units.append(("--doc", ["cargo", "test", *cargo, "--doc", "--", *libtest], None, None, None))
    return units


RUNNING = re.compile(r"^running (\d+) tests?$", re.M)


def main():
    jobs, shard, cargo, libtest = split_args(sys.argv[1:])
    outdir = tempfile.mkdtemp(prefix="cargo-test-shards-env-")
    try:
        units = plan(cargo, libtest, shard, outdir)
    finally:
        shutil.rmtree(outdir, ignore_errors=True)
    results = [None] * len(units)
    live = {}
    lock = threading.Lock()
    cut = threading.Event()

    def run(i):
        if cut.is_set():
            return
        label, argv, _, env, cwd = units[i]
        t0 = time.monotonic()
        p = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             text=True, errors="replace", start_new_session=True,
                             env=env, cwd=cwd)
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
    for i, (label, argv, want, _env, _cwd) in enumerate(units):
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
