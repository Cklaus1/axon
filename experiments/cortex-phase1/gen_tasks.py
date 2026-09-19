#!/usr/bin/env python3
"""Build the Phase-1 repair task set, and PROVE each task is usable.

A task is admitted only if, measured against the real `axon` binary:
  * the BROKEN program fails its visible check  (there is something to repair)
  * the FIXED program passes visible AND hidden (the repair is reachable)
  * the broken program fails the HIDDEN check too (so a repair that games the
    visible test alone cannot score)

A task that does not satisfy all three is dropped with its reason recorded.
An experiment whose task set was never validated measures the task set.
"""
import json, os, subprocess, sys, tempfile, pathlib

AXON = "/home/cklaus/projects/axon/target/debug/axon"

# (name, broken_body, fixed_body, visible_test, hidden_test, intent)
TASKS = [
    ("double_adds",
     "fn double(n: i64) -> i64 {\n    n + 2\n}\n",
     "fn double(n: i64) -> i64 {\n    n * 2\n}\n",
     "@[test]\nfn t_visible() { assert_eq(double(5), 10) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(double(7), 14)\n    assert_eq(double(0), 0)\n    assert_eq(double(0 - 3), 0 - 6)\n}\n",
     "`double` must return twice its argument."),
    ("sum_off_by_one",
     "fn sum_to(n: i64) -> i64 {\n    let mut t = 0\n    let mut i = 1\n    while i < n {\n        t = t + i\n        i = i + 1\n    }\n    t\n}\n",
     "fn sum_to(n: i64) -> i64 {\n    let mut t = 0\n    let mut i = 1\n    while i <= n {\n        t = t + i\n        i = i + 1\n    }\n    t\n}\n",
     "@[test]\nfn t_visible() { assert_eq(sum_to(5), 15) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(sum_to(1), 1)\n    assert_eq(sum_to(10), 55)\n    assert_eq(sum_to(0), 0)\n}\n",
     "`sum_to(n)` must return 1+2+...+n inclusive."),
    ("max_returns_min",
     "fn larger(a: i64, b: i64) -> i64 {\n    if a > b { b } else { a }\n}\n",
     "fn larger(a: i64, b: i64) -> i64 {\n    if a > b { a } else { b }\n}\n",
     "@[test]\nfn t_visible() { assert_eq(larger(3, 9), 9) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(larger(9, 3), 9)\n    assert_eq(larger(4, 4), 4)\n    assert_eq(larger(0 - 5, 0 - 2), 0 - 2)\n}\n",
     "`larger` must return the greater of its two arguments."),
    ("abs_wrong_sign",
     "fn my_abs(n: i64) -> i64 {\n    if n < 0 { n } else { n }\n}\n",
     "fn my_abs(n: i64) -> i64 {\n    if n < 0 { 0 - n } else { n }\n}\n",
     "@[test]\nfn t_visible() { assert_eq(my_abs(0 - 4), 4) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(my_abs(7), 7)\n    assert_eq(my_abs(0), 0)\n    assert_eq(my_abs(0 - 1), 1)\n}\n",
     "`my_abs` must return the absolute value."),
    ("count_skips_last",
     "fn count_up(n: i64) -> i64 {\n    let mut c = 0\n    let mut i = 0\n    while i < n {\n        c = c + 1\n        i = i + 2\n    }\n    c\n}\n",
     "fn count_up(n: i64) -> i64 {\n    let mut c = 0\n    let mut i = 0\n    while i < n {\n        c = c + 1\n        i = i + 1\n    }\n    c\n}\n",
     "@[test]\nfn t_visible() { assert_eq(count_up(5), 5) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(count_up(1), 1)\n    assert_eq(count_up(0), 0)\n    assert_eq(count_up(10), 10)\n}\n",
     "`count_up(n)` must return n."),
    ("factorial_zero",
     "fn fact(n: i64) -> i64 {\n    if n <= 1 { 0 } else { n * fact(n - 1) }\n}\n",
     "fn fact(n: i64) -> i64 {\n    if n <= 1 { 1 } else { n * fact(n - 1) }\n}\n",
     "@[test]\nfn t_visible() { assert_eq(fact(4), 24) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(fact(0), 1)\n    assert_eq(fact(1), 1)\n    assert_eq(fact(6), 720)\n}\n",
     "`fact(n)` must return n! with fact(0)=1."),
    ("min_of_three",
     "fn min3(a: i64, b: i64, c: i64) -> i64 {\n    let m = min_i64(a, b)\n    m\n}\n",
     "fn min3(a: i64, b: i64, c: i64) -> i64 {\n    let m = min_i64(a, b)\n    min_i64(m, c)\n}\n",
     "@[test]\nfn t_visible() { assert_eq(min3(5, 3, 1), 1) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(min3(3, 2, 1), 1)\n    assert_eq(min3(5, 9, 2), 2)\n    assert_eq(min3(2, 2, 2), 2)\n}\n",
     "`min3` must return the smallest of three arguments."),
    ("is_even_inverted",
     "fn is_even(n: i64) -> bool {\n    n % 2 == 1\n}\n",
     "fn is_even(n: i64) -> bool {\n    n % 2 == 0\n}\n",
     "@[test]\nfn t_visible() { assert(is_even(4)) }\n",
     "@[test]\nfn t_hidden() {\n    assert(is_even(0))\n    assert(!is_even(3))\n    assert(is_even(10))\n}\n",
     "`is_even` must be true exactly for even numbers."),
    ("clamp_ignores_hi",
     "fn clamp(v: i64, lo: i64, hi: i64) -> i64 {\n    if v < lo { lo } else { v }\n}\n",
     "fn clamp(v: i64, lo: i64, hi: i64) -> i64 {\n    if v < lo { lo } else { if v > hi { hi } else { v } }\n}\n",
     "@[test]\nfn t_visible() { assert_eq(clamp(15, 0, 10), 10) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(clamp(100, 0, 10), 10)\n    assert_eq(clamp(0 - 5, 0, 10), 0)\n    assert_eq(clamp(5, 0, 10), 5)\n}\n",
     "`clamp` must bound v to [lo, hi]."),
    ("sign_missing_zero",
     "fn my_sign(n: i64) -> i64 {\n    if n > 0 { 1 } else { 0 - 1 }\n}\n",
     "fn my_sign(n: i64) -> i64 {\n    if n > 0 { 1 } else { if n < 0 { 0 - 1 } else { 0 } }\n}\n",
     "@[test]\nfn t_visible() { assert_eq(my_sign(0), 0) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(my_sign(5), 1)\n    assert_eq(my_sign(0 - 5), 0 - 1)\n    assert_eq(my_sign(0), 0)\n}\n",
     "`my_sign` must return 1, -1, or 0 for zero."),
]

def run_tests(body, tests, tag):
    """Run `axon test` on body+tests; return (passed, output)."""
    with tempfile.TemporaryDirectory() as d:
        p = pathlib.Path(d, f"{tag}.ax")
        p.write_text(body + "\n" + tests)
        r = subprocess.run([AXON, "test", str(p)], capture_output=True, text=True, timeout=60)
        return r.returncode == 0, (r.stdout + r.stderr)

def main():
    out_dir = pathlib.Path(__file__).parent / "tasks"
    out_dir.mkdir(exist_ok=True)
    admitted, dropped = [], []
    for name, broken, fixed, vis, hid, intent in TASKS:
        checks = {}
        checks["broken_fails_visible"] = not run_tests(broken, vis, name)[0]
        checks["broken_fails_hidden"] = not run_tests(broken, hid, name)[0]
        checks["fixed_passes_visible"] = run_tests(fixed, vis, name)[0]
        checks["fixed_passes_hidden"] = run_tests(fixed, hid, name)[0]
        if all(checks.values()):
            (out_dir / f"{name}.json").write_text(json.dumps({
                "name": name, "intent": intent, "broken": broken,
                "reference_fix": fixed, "visible_test": vis, "hidden_test": hid,
            }, indent=2))
            admitted.append(name)
        else:
            dropped.append((name, {k: v for k, v in checks.items() if not v}))
    print(json.dumps({"admitted": len(admitted), "dropped": dropped}, indent=2))

if __name__ == "__main__":
    main()
