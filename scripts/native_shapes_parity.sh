#!/usr/bin/env bash
# native_shapes_parity.sh — native↔interp parity on the program shapes the
# native-parity batch fixed (tasks/native-parity-spec.md §3, NP-1..NP-6).
#
# The interpreter is the reference. For each whole program below we run
# `axon run` and the `axon build` binary and require the SAME stdout, the SAME
# exit code and the SAME stderr:
#   np1_*  a tuple pattern tests every component; an int literal is compared
#          with the subject widened by its signedness; `Ok(lit)` reads the
#          payload at its type (NP-1; `u01` looped forever natively).
#   np2_*  a match no arm matches panics `no match arm matched`, exit 101 (NP-2).
#   np3_*  a generic fn at two types / called from an impl method runs every
#          call natively (NP-3).
#   np4_*  `break`/`continue` reaching a fn, method or closure edge panics with
#          the interpreter's text, exit 101 (NP-4).
#   np5_*  `arr_range`/`arr_repeat` spans and refused allocations panic like
#          the interpreter (NP-5). `arr_range(0, 2^40)` is left out: whether
#          8 TiB can be reserved depends on the host's overcommit (the cargo
#          killer runs it under `ulimit -v`).
#   np6_*  `min_i32`/`max_i32` truncate their arguments like `abs_i32` (NP-6).
# The cargo killers in `integration_fixtures` hold the g01 rows; this harness
# is the coverage sweep `parity_all.sh` runs.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. "$ROOT/scripts/lib/harness_skip.sh"
. "$ROOT/scripts/lib/axon_bin.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ -n "${AXON:-}" ]; then
  named_bin AXON native_shapes_parity
else
  echo "native_shapes_parity: building codegen axon binary…"
  cargo_build_or_skip native_shapes_parity llvm cargo build -q -p axon-core --bin axon
  cargo_build_or_skip native_shapes_parity interp cargo build -q -p axon-rt
  use_built AXON axon
fi
require_codegen_axon native_shapes_parity

# (label, whole program).
declare -A CASES

# ── NP-1: tuple patterns, literal widening, Ok/Err payload literals ─────────
CASES[np1_v22]='fn f(p: (i64, i64)) -> i64 {
  match p {
    (0, 0) => 0,
    (0, y) => y,
    (x, _) => x,
  }
}
fn main() -> i64 {
  println(to_str(f((3, 4))))
  0
}'
CASES[np1_v25]='fn f(p: (bool, bool)) -> i64 {
  match p {
    (true, true) => 3,
    (true, false) => 2,
    (false, true) => 1,
    (false, false) => 0,
  }
}
fn main() -> i64 {
  println(to_str(f((true, false))))
  0
}'
CASES[np1_w08]='enum C { R, G }
fn f(p: (C, C)) -> i64 {
  match p {
    (C::R, C::R) => 0,
    (C::R, C::G) => 1,
    (C::G, _) => 2,
  }
}
fn main() -> i64 {
  println(to_str(f((C::G, C::R))))
  0
}'
CASES[np1_w09]='fn f(p: ((i64, bool), str)) -> i64 {
  match p {
    ((0, true), _) => 0,
    ((n, _), s) => n + str_len(s),
  }
}
fn main() -> i64 {
  println(to_str(f(((2, false), "ab"))))
  0
}'
CASES[np1_t06]='fn f(p: (str, i64)) -> i64 {
  match p {
    ("a", n) => n,
    ("b", n) => n * 2,
    _ => 0,
  }
}
fn main() -> i64 {
  println(to_str(f(("b", 5))))
  println(to_str(f(("z", 5))))
  0
}'
CASES[np1_t09]='enum C { R, G, B }
fn f(p: (C, i64)) -> i64 {
  match p {
    (C::R, n) => n,
    (C::G, n) => n * 2,
    (C::B, n) => n * 3,
  }
}
fn main() -> i64 {
  println(to_str(f((C::B, 5))))
  0
}'
CASES[np1_t14]='fn f(o: Option<(i64, i64)>) -> i64 {
  match o {
    Some((0, y)) => y,
    Some((x, _)) => x * 100,
    None => -1,
  }
}
fn main() -> i64 {
  println(to_str(f(Some((3, 4)))))
  0
}'
CASES[np1_t17]='fn f(p: (i64, bool)) {
  match p {
    (0, true) => println("zero-true"),
    (_, false) => println("false"),
    (_, true) => println("true"),
  }
}
fn main() -> i64 {
  f((5, true))
  0
}'
CASES[np1_t21]='fn f(p: (i64, i64)) -> i64 {
  match p {
    (_, 0) => 100,
    (x, _) => x,
  }
}
fn main() -> i64 {
  println(to_str(f((3, 4))))
  0
}'
CASES[np1_t27]='fn f(p: (i64, i64, i64)) -> i64 {
  match p {
    (1, 1, 1) => 0,
    (_, _, z) => z,
  }
}
fn main() -> i64 {
  println(to_str(f((2, 2, 9))))
  0
}'
CASES[np1_u01]='fn step(n: i64) -> (bool, i64) { (n < 3, n + 1) }
fn main() -> i64 {
  let n = 0
  let c = 0
  while let (true, m) = step(n) {
    n = m
    c = c + 1
  }
  println(to_str(c))
  0
}'
CASES[np1_u05]='fn f(n: i32) -> i64 {
  match n {
    0 => 100,
    _ => 7,
  }
}
fn main() -> i64 {
  println(to_str(f(0)))
  0
}'
CASES[np1_a1]='fn f(x: u8) -> i64 {
  match x {
    256 => 1,
    _ => 2,
  }
}
fn main() -> i64 {
  println(to_str(f(0)))
  0
}'
CASES[np1_a2]='fn f(x: i32) -> i64 {
  match x {
    4294967297 => 1,
    _ => 2,
  }
}
fn main() -> i64 {
  println(to_str(f(1)))
  0
}'
CASES[np1_a3]='fn f(t: (i32, i64)) -> i64 {
  match t {
    (4294967297, _) => 1,
    _ => 2,
  }
}
fn main() -> i64 {
  let y: i32 = 1
  println(to_str(f((y, 0))))
  0
}'
CASES[np1_a4]='fn f(x: u8) -> i64 {
  match x {
    -1 => 1,
    _ => 2,
  }
}
fn main() -> i64 {
  println(to_str(f(255)))
  0
}'
CASES[np1_okl]='fn f(r: Result<i64, str>) -> i64 {
  match r {
    Ok(0) => 0,
    Ok(n) => n,
    Err(_) => 1,
  }
}
fn main() -> i64 {
  println(to_str(f(Ok(5))))
  0
}'
# Controls: identical on both engines before NP-1.
CASES[np1_ctl_t04]='fn main() -> i64 {
  let r = match (1, 2) {
    (_, _) => 9,
  }
  println(to_str(r))
  0
}'
CASES[np1_ctl_t05]='fn main() -> i64 {
  let r = match (3, 4) {
    (x, y) => x * 10 + y,
  }
  println(to_str(r))
  0
}'
CASES[np1_ctl_t11]='fn f(p: (i64, i64)) -> i64 {
  match p {
    (x, y) if x > y => x,
    (_, y) => y,
  }
}
fn main() -> i64 {
  println(to_str(f((3, 9))))
  println(to_str(f((9, 3))))
  0
}'
CASES[np1_ctl_t12]='fn main() -> i64 {
  let p = (3, 4)
  let (a, b) = p
  println(to_str(a * 10 + b))
  0
}'
CASES[np1_ctl_u06]='fn f(o: Option<i64>) -> i64 {
  match o {
    Some(0) => 100,
    Some(n) => n,
    None => 0 - 1,
  }
}
fn main() -> i64 {
  println(to_str(f(Some(0))))
  println(to_str(f(Some(4))))
  0
}'
CASES[np1_ctl_v26]='fn f(p: (Option<i64>, bool)) -> i64 {
  match p {
    (Some(x), true) => x,
    (Some(_), false) => 1,
    (None, _) => 0,
  }
}
fn main() -> i64 {
  println(to_str(f((Some(5), true))))
  0
}'

# ── NP-2: a match no arm matches panics (exit 101) ──────────────────────────
CASES[np2_h31]='fn f(o: Option<i64>) -> i64 {
  match o {
    Some(0) => 0,
    None => 1,
  }
}
fn main() -> i64 {
  println(to_str(f(Some(5))))
  0
}'
CASES[np2_u02]='fn f(o: Option<i64>) {
  match o {
    Some(0) => println("zero"),
    None => println("none"),
  }
}
fn main() -> i64 {
  f(Some(5))
  println("after")
  0
}'

# ── NP-3: generic fns at several types / from impl methods ──────────────────
CASES[np3_d12]='fn put<V>(d: Dict<str, V>, k: str, v: V) { dict_set(d, k, v) }
fn main() -> i64 {
  let a = dict_new()
  put(a, "x", 1)
  println(to_str(dict_len(a)))
  let b = dict_new()
  put(b, "y", "s")
  println(to_str(dict_len(b)))
  0
}'
CASES[np3_d13]='fn put<V>(d: Dict<str, V>, k: str, v: V) { dict_set(d, k, v) }
fn main() -> i64 {
  let b = dict_new()
  put(b, "y", "s")
  let a = dict_new()
  put(a, "x", 1)
  println(to_str(dict_len(a)))
  println(to_str(dict_len(b)))
  0
}'
CASES[np3_d14]='fn show<T>(x: T) -> T { x }
fn put<V>(d: Dict<str, V>, k: str, v: V) { println("put " + k)
  dict_set(d, k, v) }
fn main() -> i64 {
  let a = dict_new()
  put(a, "x", 1)
  let b = dict_new()
  put(b, "y", true)
  println(to_str(dict_len(a)))
  println(to_str(dict_len(b)))
  0
}'
CASES[np3_g02]='fn log<T>(x: T, msg: str) { println(msg) }
fn main() -> i64 {
  log(1, "one")
  log("a", "two")
  println("end")
  0
}'
CASES[np3_g06]='fn log<T>(x: T, msg: str) { println(msg) }
fn main() -> i64 {
  if true { log(1, "one") }
  let k = 0
  while k < 2 { log("a", "loop")
    k = k + 1 }
  0
}'
CASES[np3_g08]='fn tell<T>(x: T) -> i64 { println("told")
  1 }
fn main() -> i64 {
  tell(1)
  tell("a")
  0
}'
CASES[np3_w31]='fn put<V>(d: Dict<V>, k: str, v: V) { dict_set(d, k, v) }
fn main() -> i64 {
  let a = dict_new()
  put(a, "x", 1)
  let b = dict_new()
  put(b, "y", "s")
  println(to_str(dict_len(a) + dict_len(b)))
  0
}'
CASES[np3_i1]='fn tell<T>(x: T, m: str) -> i64 { println(m)
  7 }
type S = { k: i64 }
trait G {
  fn go(self) -> i64
}
impl G for S {
  fn go(self: S) -> i64 { tell(self.k, "in-method") }
}
fn main() -> i64 {
  let s = S { k: 1 }
  println(to_str(s.go()))
  0
}'
CASES[np3_i3]='fn tell<T>(x: T, m: str) { println(m) }
type S = { k: i64 }
trait G {
  fn go(self) -> i64
}
impl G for S {
  fn go(self: S) -> i64 {
    tell(self.k, "in-method")
    5
  }
}
fn main() -> i64 {
  let s = S { k: 1 }
  println(to_str(s.go()))
  0
}'
CASES[np3_ctl_g03]='fn log<T>(x: T, msg: str) { println(msg) }
fn main() -> i64 {
  log(1, "one")
  log(2, "two")
  println("end")
  0
}'
CASES[np3_ctl_g12]='fn note<T>(xs: [T]) { println("n=" + to_str(len(xs))) }
fn main() -> i64 {
  note([1, 2])
  note(["a"])
  0
}'

# ── NP-4: break/continue never leave their frame ────────────────────────────
CASES[np4_b01]='fn main() {
    for i in 0..4 {
        let g = |n: i64| {
            if n > 1 { break }
            n
        }
        println(to_str(g(i)))
    }
    println("after")
}'
CASES[np4_b02]='fn stop(n: i64) -> i64 {
    if n > 1 { continue }
    n
}
fn main() {
    let i = 0
    while i < 4 {
        i = i + 1
        println(to_str(stop(i)))
    }
    println("after")
}'
CASES[np4_bm]='type S = { k: i64 }
trait G {
  fn go(self) -> i64
}
impl G for S {
  fn go(self: S) -> i64 {
    if self.k > 1 { break }
    self.k
  }
}
fn main() -> i64 {
  let i = 0
  while i < 4 {
    let s = S { k: i }
    println(to_str(s.go()))
    i = i + 1
  }
  0
}'
CASES[np4_escape_fast_local_int]='// R51 S0b: a callee'\''s `break` does not end the caller'\''s loop.
fn stop(n: i64) -> i64 {
    if n > 3 { break }
    n
}

fn main() {
    let i = 0
    while i < 6 {
        println(to_str(stop(i + 1)))
        i = i + 1
    }
    println("unreachable: the loop ended silently")
}'
CASES[np4_escape_closure]='// R51 S0b: a closure'\''s `break` stays in the closure: an error at its edge.
fn main() {
    let g = |n: i64| {
        if n > 1 { break }
        n
    }
    for i in 0..4 {
        println(to_str(g(i)))
    }
    println("unreachable: the loop ended silently")
}'
CASES[np4_escape_fast_inline]='// R51 S0b: a callee'\''s `continue` does not skip the caller'\''s iteration.
fn skip(n: i64) -> i64 {
    if n > 2 { continue }
    n * 10
}

fn main() {
    for i in 0..6 {
        println(to_str(skip(i)))
    }
    println("unreachable: the loop skipped silently")
}'

# ── NP-5: arr_range/arr_repeat spans and refused allocations ────────────────
CASES[np5_h11]='fn main() -> i64 {
  let xs = arr_range(-9223372036854775807, 9223372036854775807)
  println(to_str(len(xs)))
  0
}'
CASES[np5_r01]='fn main() -> i64 {
  let xs = arr_range(0, 9223372036854775807)
  println(to_str(len(xs)))
  0
}'
CASES[np5_r02]='fn main() -> i64 {
  let xs = arr_range(0, 4611686018427387904)
  println(to_str(len(xs)))
  0
}'
CASES[np5_r03]='fn main() -> i64 {
  let xs = arr_repeat(1, 9223372036854775807)
  println(to_str(len(xs)))
  0
}'
CASES[np5_r06]='fn main() -> i64 {
  let xs = arr_repeat(true, 9223372036854775807)
  println(to_str(len(xs)))
  0
}'
CASES[np5_ctl_r04]='fn main() -> i64 {
  let xs = arr_range(-3, 3)
  println(to_str(len(xs)))
  0
}'

# ── NP-6: min_i32/max_i32 truncate like abs_i32 ─────────────────────────────
CASES[np6_m01]='fn main() -> i64 {
  println(to_str(min_i32(3000000000, 1)))
  println(to_str(max_i32(3000000000, 1)))
  println(to_str(min_i32(5000000000, 6000000000)))
  println(to_str(max_i32(0 - 3000000000, 0)))
  println(to_str(abs_i32(5000000000)))
  println(to_str(abs_i32(0 - 7)))
  println(to_str(min_i32(2, 9)))
  println(to_str(max_i32(2, 9)))
  0
}'
CASES[np6_m03]='fn main() -> i64 {
  let x: i32 = min_i32(3000000000, 1)
  println(to_str(x))
  let y: i32 = x + 1
  println(to_str(y))
  0
}'
CASES[np6_ctl_m02]='fn main() -> i64 {
  println(to_str(min_i32(0 - 2147483648, 2147483647)))
  println(to_str(max_i32(0 - 2147483648, 2147483647)))
  0
}'

fail=0
n=0
labels="$(printf '%s\n' "${!CASES[@]}" | LC_ALL=C sort)"
for label in $labels; do
  n=$((n + 1))
  PROG="$WORK/$label.ax"
  printf '%s\n' "${CASES[$label]}" > "$PROG"

  # Interpreter (oracle). stdout and stderr are compared separately.
  "$AXON" run "$PROG" > "$WORK/$label.iout" 2> "$WORK/$label.ierr"
  iexit=$?

  # Native. `timeout` turns a native hang (np1_u01 used to loop forever) into
  # a divergence instead of a wedged harness.
  BIN="$WORK/${label}_bin"
  if ! berr="$("$AXON" build "$PROG" -o "$BIN" --no-cache 2>&1)"; then
    native_build_failed native_shapes_parity "$label" "$berr" || { fail=1; continue; }
  fi
  timeout 60 "$BIN" > "$WORK/$label.nout" 2> "$WORK/$label.nerr"
  nexit=$?

  if [ "$iexit" -ne "$nexit" ] \
    || ! cmp -s "$WORK/$label.iout" "$WORK/$label.nout" \
    || ! cmp -s "$WORK/$label.ierr" "$WORK/$label.nerr"; then
    echo "native_shapes_parity: FAIL — $label diverged"
    echo "  interp: exit=$iexit stdout=[$(cat "$WORK/$label.iout")] stderr=[$(cat "$WORK/$label.ierr")]"
    echo "  native: exit=$nexit stdout=[$(cat "$WORK/$label.nout")] stderr=[$(cat "$WORK/$label.nerr")]"
    fail=1
  else
    echo "  $label: interp==native (exit $iexit) ✓"
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "native_shapes_parity: FAILED"
  exit 1
fi
echo "native_shapes_parity: OK — $n native-parity shapes match the interpreter (stdout, stderr, exit)"
exit 0
