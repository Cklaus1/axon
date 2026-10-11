#!/usr/bin/env python3
"""s7stack-sweep.py out.json bin_label=path ... : call depth x paren depth sweep, tree+vm.

Program: fn down(n) { if n == 0 { 0 } else { (((down(n - 1) + 0) + 0) ...) + 1 } } with k
parens, main prints down(D). Records the rc of each (bin, engine, D, k) cell.
"""
import json, os, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

DS = [int(x) for x in os.environ.get("SW_D", "500,1000,2000,3000,4000,5000,5990,6500").split(",")]
KS = [int(x) for x in os.environ.get("SW_K", "0,100,200,250,260,270,280,285,290,300,350,400,600,1000").split(",")]
ENGS = os.environ.get("SW_E", "tree,vm").split(",")
PD = "/var/tmp/r51-tmp/s7stack-probes"
os.makedirs(PD, exist_ok=True)


def prog(d, k):
    p = f"{PD}/d{d}_k{k}.ax"
    if not os.path.exists(p):
        inner = "down(n - 1)"
        body = "(" * k + inner + " + 0)" * k
        with open(p, "w") as f:
            f.write(f"fn down(n: i64) -> i64 {{\n    if n == 0 {{ 0 }} else {{ {body} + 1 }}\n}}\n\n"
                    f"fn main() {{\n    println(to_str(down({d})))\n}}\n")
    return p


def run(cell):
    label, binp, eng, d, k = cell
    env = dict(os.environ, AXON_ENGINE=eng)
    env.pop("AXON_MAX_DEPTH", None)
    try:
        r = subprocess.run([binp, "run", prog(d, k)], env=env, capture_output=True, text=True, timeout=600)
        rc = r.returncode
        if rc < 0:
            rc = 128 - rc
        err = r.stderr
    except subprocess.TimeoutExpired:
        rc, err = 124, ""
    tag = "ok" if rc == 0 else ("limit" if rc == 101 and "recursion limit" in err else
                                ("ABORT" if rc == 134 and "overflow" in err else f"rc{rc}"))
    return (label, eng, d, k, rc, tag)


def main():
    out = sys.argv[1]
    bins = [a.split("=", 1) for a in sys.argv[2:]]
    cells = [(l, b, e, d, k) for l, b in bins for e in ENGS for d in DS for k in KS]
    for c in cells:
        prog(c[3], c[4])
    with ThreadPoolExecutor(int(os.environ.get("SW_J", "6"))) as ex:
        res = list(ex.map(run, cells))
    json.dump(res, open(out, "w"))
    for l, _ in bins:
        for e in ENGS:
            print(f"\n{l} {e}: rows D, cols k")
            print("D\\k   " + " ".join(f"{k:>6}" for k in KS))
            for d in DS:
                row = {r[3]: r[5] for r in res if r[0] == l and r[1] == e and r[2] == d}
                print(f"{d:<6}" + " ".join(f"{row[k]:>6}" for k in KS))


main()
