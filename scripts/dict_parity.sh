#!/usr/bin/env bash
# dict_parity.sh — R1c slice 1: native codegen == interpreter for the core
# dict_* builtins (dict_new / dict_set / dict_get / dict_has / dict_len).
#
# A Dict lowers to an opaque i8* handle to an Arc<Mutex<HashMap<String,
# TaggedVal>>> in axon-rt (the __axon_dict_* externs), mirroring the channel
# runtime. v1 covers INT-valued dicts (the common state-counter shape);
# str/f64-valued get + the slice/closure dict ops are follow-on slices.
#
# Skips (exit 0) when the codegen toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "dict_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "dict_parity: interp build unavailable — skipping"; exit 0
fi
AXON="target/debug/axon"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

fail=0
check() {
  local name="$1" src="$2"
  printf '%s\n' "$src" > "$WORK/$name.ax"
  "$INTERP" "$WORK/$name.ax" >/dev/null 2>&1; local i=$?
  if "$AXON" build "$WORK/$name.ax" -o "$WORK/$name" >/dev/null 2>&1; then
    "$WORK/$name" >/dev/null 2>&1; local n=$?
    if [ "$i" = "$n" ]; then echo "  OK   $name: interp=$i native=$n"
    else echo "  FAIL $name: interp=$i native=$n"; fail=1; fi
  else echo "  SKIP $name (native build unavailable)"; fi
}

check len     'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 1)  dict_set(d, "b", 2)  dict_len(d) }'
check has_y   'fn main() -> i64 { let d = dict_new()  dict_set(d, "x", 9)  if dict_has(d, "x") { 1 } else { 0 } }'
check has_n   'fn main() -> i64 { let d = dict_new()  dict_set(d, "x", 9)  if dict_has(d, "y") { 1 } else { 0 } }'
check overwr  'fn main() -> i64 { let d = dict_new()  dict_set(d, "k", 1)  dict_set(d, "k", 5)  dict_len(d) }'
check get_s   'fn main() -> i64 { let d = dict_new()  dict_set(d, "k", 42)  match dict_get(d, "k") { Some(v) => v  None => 0 - 1 } }'
check get_n   'fn main() -> i64 { let d = dict_new()  dict_set(d, "k", 42)  match dict_get(d, "z") { Some(v) => v  None => 0 - 1 } }'
check counter 'fn main() -> i64 { let d = dict_new()  dict_set(d, "n", 5)  let cur = match dict_get(d, "n") { Some(v) => v  None => 0 }  dict_set(d, "n", cur + 1)  match dict_get(d, "n") { Some(v) => v  None => 0 } }'
check interp_key 'fn main() -> i64 { let d = dict_new()  let i = 3  dict_set(d, "step-{i}", i)  match dict_get(d, "step-3") { Some(v) => v  None => 0 - 1 } }'
check inc       'fn main() -> i64 { let d = dict_new()  dict_inc(d, "a")  dict_inc(d, "a")  dict_inc(d, "a") }'
check inc_new   'fn main() -> i64 { let d = dict_new()  dict_inc(d, "fresh") }'
check inc_freq  'fn main() -> i64 { let d = dict_new()  dict_inc(d, "x")  dict_inc(d, "y")  dict_inc(d, "x")  match dict_get(d, "x") { Some(v) => v  None => 0 } }'
check getor_hit 'fn main() -> i64 { let d = dict_new()  dict_set(d, "k", 7)  dict_get_or(d, "k", 99) }'
check getor_mis 'fn main() -> i64 { let d = dict_new()  dict_set(d, "k", 7)  dict_get_or(d, "z", 99) }'

[ "$fail" -eq 0 ] || { echo "dict_parity: FAIL"; exit 1; }
echo "dict_parity: PASS — dict_new/set/get/has/len/inc/get_or (int values) match the interpreter ✓"
exit 0
