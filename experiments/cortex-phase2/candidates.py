"""Phase-2 candidate tasks: harder than Phase 1, still objectively verifiable.

Difficulty is NOT asserted here — `calibrate.py` measures each one's control
pass-rate and keeps only the band that can show movement in either direction.
"""
CANDIDATES = [
    # --- helper/caller split: the symptom is not where the defect is ---
    ("helper_bug_distant",
     "fn scale(x: i64) -> i64 {\n    x * 3\n}\n\nfn total(a: i64, b: i64) -> i64 {\n    scale(a) + scale(b)\n}\n",
     "fn scale(x: i64) -> i64 {\n    x * 2\n}\n\nfn total(a: i64, b: i64) -> i64 {\n    scale(a) + scale(b)\n}\n",
     "@[test]\nfn t_visible() { assert_eq(total(1, 1), 4) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(total(2, 3), 10)\n    assert_eq(total(0, 0), 0)\n    assert_eq(scale(5), 10)\n}\n",
     "`scale` doubles its argument; `total` sums the scaled inputs."),
    # --- nested loop, inner bound wrong ---
    ("nested_inner_bound",
     "fn grid_count(n: i64) -> i64 {\n    let mut c = 0\n    let mut i = 0\n    while i < n {\n        let mut j = 0\n        while j < i {\n            c = c + 1\n            j = j + 1\n        }\n        i = i + 1\n    }\n    c\n}\n",
     "fn grid_count(n: i64) -> i64 {\n    let mut c = 0\n    let mut i = 0\n    while i < n {\n        let mut j = 0\n        while j < n {\n            c = c + 1\n            j = j + 1\n        }\n        i = i + 1\n    }\n    c\n}\n",
     "@[test]\nfn t_visible() { assert_eq(grid_count(3), 9) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(grid_count(1), 1)\n    assert_eq(grid_count(4), 16)\n    assert_eq(grid_count(0), 0)\n}\n",
     "`grid_count(n)` counts all n*n cells of an n-by-n grid."),
    # --- accumulator seeded wrong for a product ---
    ("product_seeded_zero",
     "fn product(n: i64) -> i64 {\n    let mut p = 0\n    let mut i = 1\n    while i <= n {\n        p = p * i\n        i = i + 1\n    }\n    p\n}\n",
     "fn product(n: i64) -> i64 {\n    let mut p = 1\n    let mut i = 1\n    while i <= n {\n        p = p * i\n        i = i + 1\n    }\n    p\n}\n",
     "@[test]\nfn t_visible() { assert_eq(product(4), 24) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(product(1), 1)\n    assert_eq(product(5), 120)\n    assert_eq(product(0), 1)\n}\n",
     "`product(n)` returns n! (empty product is 1)."),
    # --- two defects, one visible ---
    ("two_defects",
     "fn norm(v: i64, lo: i64, hi: i64) -> i64 {\n    if v < lo { lo } else { if v > hi { lo } else { v } }\n}\n",
     "fn norm(v: i64, lo: i64, hi: i64) -> i64 {\n    if v < lo { lo } else { if v > hi { hi } else { v } }\n}\n",
     "@[test]\nfn t_visible() { assert_eq(norm(50, 0, 10), 10) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(norm(0 - 9, 0, 10), 0)\n    assert_eq(norm(5, 0, 10), 5)\n    assert_eq(norm(11, 2, 8), 8)\n}\n",
     "`norm` clamps v into [lo, hi]."),
    # --- recursion: wrong combining operation ---
    ("fib_wrong_combine",
     "fn fib(n: i64) -> i64 {\n    if n < 2 { n } else { fib(n - 1) * fib(n - 2) }\n}\n",
     "fn fib(n: i64) -> i64 {\n    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }\n}\n",
     "@[test]\nfn t_visible() { assert_eq(fib(6), 8) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(fib(0), 0)\n    assert_eq(fib(1), 1)\n    assert_eq(fib(10), 55)\n}\n",
     "`fib` is the Fibonacci sequence: fib(n) = fib(n-1) + fib(n-2)."),
    # --- array walk skips the last element ---
    ("array_skips_last",
     "fn sum_arr(xs: &[i64]) -> i64 {\n    let mut t = 0\n    let mut i = 0\n    while i < len(xs) - 1 {\n        t = t + xs[i]\n        i = i + 1\n    }\n    t\n}\n",
     "fn sum_arr(xs: &[i64]) -> i64 {\n    let mut t = 0\n    let mut i = 0\n    while i < len(xs) {\n        t = t + xs[i]\n        i = i + 1\n    }\n    t\n}\n",
     "@[test]\nfn t_visible() {\n    let a = [1, 2, 3]\n    assert_eq(sum_arr(&a), 6)\n}\n",
     "@[test]\nfn t_hidden() {\n    let a = [5]\n    assert_eq(sum_arr(&a), 5)\n    let b = [1, 1, 1, 1]\n    assert_eq(sum_arr(&b), 4)\n}\n",
     "`sum_arr` sums every element of the slice."),
    # --- struct fields swapped at construction ---
    ("struct_fields_swapped",
     "type Pt = { x: i64, y: i64 }\n\nfn make(a: i64, b: i64) -> Pt {\n    Pt { x: b, y: a }\n}\n\nfn dx(p: Pt) -> i64 { p.x }\n",
     "type Pt = { x: i64, y: i64 }\n\nfn make(a: i64, b: i64) -> Pt {\n    Pt { x: a, y: b }\n}\n\nfn dx(p: Pt) -> i64 { p.x }\n",
     "@[test]\nfn t_visible() { assert_eq(dx(make(7, 9)), 7) }\n",
     "@[test]\nfn t_hidden() {\n    let p = make(1, 2)\n    assert_eq(p.x, 1)\n    assert_eq(p.y, 2)\n}\n",
     "`make(a, b)` builds a point with x=a and y=b."),
    # --- early return short-circuits the search ---
    ("search_returns_early",
     "fn find_first_even(xs: &[i64]) -> i64 {\n    let mut i = 0\n    while i < len(xs) {\n        if xs[i] % 2 == 0 { return i }\n        return 0 - 1\n    }\n    0 - 1\n}\n",
     "fn find_first_even(xs: &[i64]) -> i64 {\n    let mut i = 0\n    while i < len(xs) {\n        if xs[i] % 2 == 0 { return i }\n        i = i + 1\n    }\n    0 - 1\n}\n",
     "@[test]\nfn t_visible() {\n    let a = [2, 4]\n    assert_eq(find_first_even(&a), 0)\n}\n",
     "@[test]\nfn t_hidden() {\n    let a = [1, 3, 6]\n    assert_eq(find_first_even(&a), 2)\n    let b = [1, 3]\n    assert_eq(find_first_even(&b), 0 - 1)\n}\n",
     "`find_first_even` returns the index of the first even element, or -1."),
    # --- integer division truncation used where it must not be ---
    ("avg_truncates_early",
     "fn avg3(a: i64, b: i64, c: i64) -> i64 {\n    a / 3 + b / 3 + c / 3\n}\n",
     "fn avg3(a: i64, b: i64, c: i64) -> i64 {\n    (a + b + c) / 3\n}\n",
     "@[test]\nfn t_visible() { assert_eq(avg3(1, 1, 1), 1) }\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(avg3(1, 2, 3), 2)\n    assert_eq(avg3(2, 2, 5), 3)\n    assert_eq(avg3(0, 0, 0), 0)\n}\n",
     "`avg3` is the integer mean of three values: (a+b+c)/3."),
    # --- comparison inverted inside a swap-free selection ---
    ("select_max_inverted",
     "fn max_of(xs: &[i64]) -> i64 {\n    let mut m = xs[0]\n    let mut i = 1\n    while i < len(xs) {\n        if xs[i] < m { m = xs[i] }\n        i = i + 1\n    }\n    m\n}\n",
     "fn max_of(xs: &[i64]) -> i64 {\n    let mut m = xs[0]\n    let mut i = 1\n    while i < len(xs) {\n        if xs[i] > m { m = xs[i] }\n        i = i + 1\n    }\n    m\n}\n",
     "@[test]\nfn t_visible() {\n    let a = [1, 9, 3]\n    assert_eq(max_of(&a), 9)\n}\n",
     "@[test]\nfn t_hidden() {\n    let a = [5]\n    assert_eq(max_of(&a), 5)\n    let b = [0 - 3, 0 - 1, 0 - 7]\n    assert_eq(max_of(&b), 0 - 1)\n}\n",
     "`max_of` returns the largest element of a non-empty slice."),
    # --- boolean logic: wrong connective ---
    ("range_check_or",
     "fn in_range(v: i64, lo: i64, hi: i64) -> bool {\n    v >= lo || v <= hi\n}\n",
     "fn in_range(v: i64, lo: i64, hi: i64) -> bool {\n    v >= lo && v <= hi\n}\n",
     "@[test]\nfn t_visible() { assert(!in_range(20, 0, 10)) }\n",
     "@[test]\nfn t_hidden() {\n    assert(in_range(5, 0, 10))\n    assert(!in_range(0 - 1, 0, 10))\n    assert(in_range(0, 0, 10))\n}\n",
     "`in_range` is true exactly when lo <= v <= hi."),
    # --- accumulate into the wrong variable ---
    ("accumulates_wrong_var",
     "fn count_pos(xs: &[i64]) -> i64 {\n    let mut c = 0\n    let mut i = 0\n    while i < len(xs) {\n        if xs[i] > 0 { i = i + 1 }\n        i = i + 1\n    }\n    c\n}\n",
     "fn count_pos(xs: &[i64]) -> i64 {\n    let mut c = 0\n    let mut i = 0\n    while i < len(xs) {\n        if xs[i] > 0 { c = c + 1 }\n        i = i + 1\n    }\n    c\n}\n",
     "@[test]\nfn t_visible() {\n    let a = [1, 2]\n    assert_eq(count_pos(&a), 2)\n}\n",
     "@[test]\nfn t_hidden() {\n    let a = [0 - 1, 5, 0, 7]\n    assert_eq(count_pos(&a), 2)\n    let b = [0 - 1]\n    assert_eq(count_pos(&b), 0)\n}\n",
     "`count_pos` counts elements strictly greater than zero."),
]
