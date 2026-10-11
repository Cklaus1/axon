#!/usr/bin/env python3
"""s7stack-perlevel.py label=bin ... : native stack bytes per recursion level, by shape.

bytes/level = (maxRSS(D2) - maxRSS(D1)) / (D2 - D1), AXON_MAX_DEPTH raised so no limit trips.
"""
import os, re, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

PD = "/var/tmp/r51-tmp/s7stack-probes"
os.makedirs(PD, exist_ok=True)
P = "(" * 100
C = " + 0)" * 100
SHAPES = {
    "plain": "fn down(n: i64) -> i64 {\n    if n == 0 { 0 } else { 1 + down(n - 1) }\n}\nfn main() { println(to_str(down(D))) }\n",
    "paren100": "fn down(n: i64) -> i64 {\n    if n == 0 { 0 } else { " + P + "down(n - 1)" + C + " + 1 }\n}\nfn main() { println(to_str(down(D))) }\n",
    "block20": "fn down(n: i64) -> i64 {\n    if n == 0 { 0 } else { " + "{ " * 20 + "down(n - 1) + 1" + " }" * 20 + " }\n}\nfn main() { println(to_str(down(D))) }\n",
    "let": "fn down(n: i64) -> i64 {\n    if n == 0 { return 0 }\n    let a = down(n - 1)\n    a + 1\n}\nfn main() { println(to_str(down(D))) }\n",
    "mutual": "fn ev(n: i64) -> bool {\n    if n == 0 { true } else { od(n - 1) }\n}\nfn od(n: i64) -> bool {\n    if n == 0 { false } else { ev(n - 1) }\n}\nfn main() { println(to_str(ev(D))) }\n",
    "closure": "fn apply(f: fn(i64) -> i64, n: i64) -> i64 {\n    f(n)\n}\nfn go(n: i64) -> i64 {\n    if n == 0 { 0 } else { apply(|k| go(k - 1) + 1, n) }\n}\nfn main() { println(to_str(go(D))) }\n",
    "question": "fn down(n: i64) -> Result<i64, str> {\n    if n == 0 { return Ok(0) }\n    let v = down(n - 1)?\n    Ok(v + 1)\n}\nfn main() {\n    match down(D) {\n        Ok(v) => println(to_str(v))\n        Err(e) => println(e)\n    }\n}\n",
    "mutref": "fn walk(xs: &mut [i64], n: i64) {\n    if n > 0 {\n        xs[0] = xs[0] + 1\n        walk(&mut xs, n - 1)\n    }\n}\nfn main() {\n    let a = [0]\n    walk(&mut a, D)\n    println(to_str(a[0]))\n}\n",

    "match": "fn down(n: i64) -> i64 {\n    match n {\n        0 => 0\n        _ => down(n - 1) + 1\n    }\n}\nfn main() { println(to_str(down(D))) }\n",
}
D1, D2 = int(os.environ.get("PL_D1", "1000")), int(os.environ.get("PL_D2", "21000"))


def rss(binp, eng, shape, d):
    p = f"{PD}/pl_{shape}_{d}.ax"
    with open(p, "w") as f:
        f.write(SHAPES[shape].replace("D", str(d)).replace("Dict", "Dict"))
    env = dict(os.environ, AXON_ENGINE=eng, AXON_MAX_DEPTH="1000000")
    r = subprocess.run(["/usr/bin/time", "-f", "MAXRSS %M", binp, "run", p], env=env,
                       capture_output=True, text=True, timeout=900)
    m = re.search(r"MAXRSS (\d+)", r.stderr)
    ok = r.returncode == 0
    return int(m.group(1)) if m else -1, ok, r.stderr[-200:] if not ok else ""


def cell(a):
    label, binp, eng, shape = a
    r1, ok1, e1 = rss(binp, eng, shape, D1)
    r2, ok2, e2 = rss(binp, eng, shape, D2)
    per = (r2 - r1) * 1024 // (D2 - D1)
    return label, eng, shape, per, ok1 and ok2, e1 + e2


def main():
    bins = [a.split("=", 1) for a in sys.argv[1:]]
    engs = os.environ.get("PL_E", "tree").split(",")
    cells = [(l, b, e, s) for l, b in bins for e in engs for s in SHAPES]
    with ThreadPoolExecutor(int(os.environ.get("PL_J", "4"))) as ex:
        res = list(ex.map(cell, cells))
    for e in engs:
        print(f"\n[{e}] bytes/level, D {D1}->{D2}")
        print(f"{'shape':<10}" + "".join(f"{l:>12}" for l, _ in bins))
        for s in SHAPES:
            row = {r[0]: r for r in res if r[1] == e and r[2] == s}
            print(f"{s:<10}" + "".join(f"{row[l][3]:>11}{'' if row[l][4] else '!'}" for l, _ in bins))
    for r in res:
        if not r[4]:
            print("FAILED", r[:3], r[5])


main()
