#!/usr/bin/env python3
"""require_idle.py — refuse to start a timing- or starvation-sensitive run on a busy machine.

    python3 scripts/lib/require_idle.py --label NAME [--max-busy-cores F] [--sample-seconds S]

Samples utime+stime of every process in /proc over the window (two reads of
/proc/<pid>/stat, delta / SC_CLK_TCK / elapsed = cores). FOREIGN load is every
process except the caller (this helper's parent), the caller's ancestor chain,
the caller's descendants, and this helper itself. A burner the caller started
is the caller's own work; a burner anyone else started is not.

Exit 0: foreign load <= --max-busy-cores (default 2.0).
Exit 75 (EX_TEMPFAIL): busy; the top foreign consumers are printed as
        pid comm cores. Anything unreadable is a usage error (exit 2), never a pass.
A process that starts and exits entirely inside the window is not seen;
sustained load (what starves a SCHED_IDLE test) is.
"""
import argparse
import os
import sys
import time

EX_BUSY = 75


def read_stat(pid):
    """(ppid, comm, start, cpu_ticks) or None when the process is gone."""
    try:
        with open(f"/proc/{pid}/stat", "rb") as f:
            raw = f.read().decode("utf-8", "replace")
    except OSError:
        return None
    lp, rp = raw.find("("), raw.rfind(")")
    if lp < 0 or rp < 0:
        return None
    f = raw[rp + 2:].split()
    try:
        return int(f[1]), raw[lp + 1:rp], f[19], int(f[11]) + int(f[12])
    except (IndexError, ValueError):
        return None


def snapshot():
    snap = {}
    for name in os.listdir("/proc"):
        if name.isdigit():
            s = read_stat(int(name))
            if s:
                snap[int(name)] = s
    return snap


def excluded(snap, me, caller):
    """Pids that are not foreign: me, the caller's ancestor chain, the caller's descendants."""
    ex = {me}
    p = caller
    while p and p not in ex:
        ex.add(p)
        s = snap.get(p)
        p = s[0] if s else 0
    children = {}
    for pid, s in snap.items():
        children.setdefault(s[0], []).append(pid)
    stack, seen = [caller], {caller}
    while stack:
        for c in children.get(stack.pop(), []):
            if c not in seen:
                seen.add(c)
                ex.add(c)
                stack.append(c)
    return ex


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--label", required=True)
    ap.add_argument("--max-busy-cores", type=float, default=2.0)
    ap.add_argument("--sample-seconds", type=float, default=3.0)
    a = ap.parse_args()
    if a.sample_seconds <= 0:
        sys.exit("require_idle: --sample-seconds must be positive")
    hz = os.sysconf("SC_CLK_TCK")
    me, caller = os.getpid(), os.getppid()
    t0, before = time.monotonic(), snapshot()
    time.sleep(a.sample_seconds)
    after, elapsed = snapshot(), time.monotonic() - t0
    ex = excluded(after, me, caller) | excluded(before, me, caller)
    rows, own = [], 0.0
    for pid, s in after.items():
        b = before.get(pid)
        # same pid AND same start time, else the pid was reused: count from 0
        base = b[3] if b and b[2] == s[2] else 0
        cores = max(0, s[3] - base) / hz / elapsed
        if pid in ex:
            own += cores
        elif cores > 0:
            rows.append((cores, pid, s[1]))
    rows.sort(reverse=True)
    total = sum(r[0] for r in rows)
    verdict = "BUSY" if total > a.max_busy_cores else "idle"
    print(f"require_idle[{a.label}]: foreign {total:.2f} cores over {elapsed:.1f}s "
          f"(threshold {a.max_busy_cores:g}; caller tree excluded: {own:.2f} cores in {len(ex)} procs) -> {verdict}")
    if total > a.max_busy_cores:
        for cores, pid, comm in rows[:8]:
            print(f"  pid {pid} {comm} {cores:.2f} cores")
        print(f"require_idle[{a.label}]: refusing; wait for the machine to go idle")
        sys.exit(EX_BUSY)


if __name__ == "__main__":
    main()
