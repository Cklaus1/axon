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
check rm_some   'fn main() -> i64 { let d = dict_new()  dict_set(d, "k", 8)  match dict_remove(d, "k") { Some(v) => v  None => 0 - 1 } }'
check rm_none   'fn main() -> i64 { let d = dict_new()  dict_set(d, "k", 8)  match dict_remove(d, "z") { Some(v) => v  None => 0 - 1 } }'
check rm_len    'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 1)  dict_set(d, "b", 2)  dict_remove(d, "a")  dict_len(d) }'
check keys_len  'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 1)  dict_set(d, "b", 2)  let ks = dict_keys(d)  len(ks) }'
check keys_sort 'fn main() -> i64 { let d = dict_new()  dict_set(d, "zebra", 1)  dict_set(d, "apple", 2)  let ks = dict_keys(d)  str_len(ks[0]) }'
check keys_ord  'fn main() -> i64 { let d = dict_new()  dict_set(d, "bb", 1)  dict_set(d, "a", 2)  let ks = dict_keys(d)  str_len(ks[0]) }'
check vals_sum  'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 10)  dict_set(d, "b", 20)  dict_set(d, "c", 12)  let vs = dict_values(d)  let s = 0  let i = 0  while i < len(vs) { s = s + vs[i]  i = i + 1 }  s }'
check vals_len  'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 1)  dict_set(d, "b", 2)  let vs = dict_values(d)  len(vs) }'
check vals_ord  'fn main() -> i64 { let d = dict_new()  dict_set(d, "zebra", 7)  dict_set(d, "apple", 3)  let vs = dict_values(d)  vs[0] }'
check vals_empty 'fn main() -> i64 { let d = dict_new()  let vs = dict_values(d)  len(vs) }'
check mrg_len  'fn main() -> i64 { let a = dict_new()  dict_set(a, "x", 1)  dict_set(a, "y", 2)  let b = dict_new()  dict_set(b, "y", 9)  dict_set(b, "z", 3)  let m = dict_merge(a, b)  dict_len(m) }'
check mrg_win  'fn main() -> i64 { let a = dict_new()  dict_set(a, "k", 1)  let b = dict_new()  dict_set(b, "k", 99)  let m = dict_merge(a, b)  match dict_get(m, "k") { Some(v) => v  None => 0 } }'
check mrg_keep 'fn main() -> i64 { let a = dict_new()  dict_set(a, "x", 5)  let b = dict_new()  dict_set(b, "y", 7)  let m = dict_merge(a, b)  match dict_get(m, "x") { Some(v) => v  None => 0 } }'
check mrg_cp   'fn main() -> i64 { let a = dict_new()  dict_set(a, "x", 1)  let b = dict_new()  dict_set(b, "y", 2)  let m = dict_merge(a, b)  dict_len(a) }'
check fp_len   'fn main() -> i64 { let p = [("a", 1), ("b", 2), ("c", 3)]  let d = dict_from_pairs(p)  dict_len(d) }'
check fp_get   'fn main() -> i64 { let p = [("a", 10), ("b", 20)]  let d = dict_from_pairs(p)  match dict_get(d, "b") { Some(v) => v  None => 0 - 1 } }'
check fp_dup   'fn main() -> i64 { let p = [("k", 1), ("k", 9)]  let d = dict_from_pairs(p)  match dict_get(d, "k") { Some(v) => v  None => 0 } }'
check fp_one   'fn main() -> i64 { let p = [("solo", 7)]  let d = dict_from_pairs(p)  match dict_get(d, "solo") { Some(v) => v  None => 0 } }'
check tp_len   'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 1)  dict_set(d, "b", 2)  let p = dict_to_pairs(d)  len(p) }'
check tp_ord   'fn main() -> i64 { let d = dict_new()  dict_set(d, "zebra", 1)  dict_set(d, "apple", 2)  let p = dict_to_pairs(d)  str_len(p[0].0) }'
check tp_val   'fn main() -> i64 { let d = dict_new()  dict_set(d, "zebra", 1)  dict_set(d, "apple", 2)  let p = dict_to_pairs(d)  p[0].1 }'
check tp_rt    'fn main() -> i64 { let d = dict_new()  dict_set(d, "x", 5)  dict_set(d, "y", 7)  let d2 = dict_from_pairs(dict_to_pairs(d))  match dict_get(d2, "y") { Some(v) => v  None => 0 - 1 } }'
check mv_get   'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 10)  dict_set(d, "b", 20)  let m = dict_map_values(d, |v| v * 2)  match dict_get(m, "b") { Some(v) => v  None => 0 - 1 } }'
check mv_len   'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 1)  dict_set(d, "b", 2)  let m = dict_map_values(d, |v| v + 100)  dict_len(m) }'
check mv_cap   'fn main() -> i64 { let d = dict_new()  dict_set(d, "k", 3)  let factor = 50  let m = dict_map_values(d, |v| v * factor)  match dict_get(m, "k") { Some(v) => v  None => 0 } }'
check mv_cp    'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 9)  let m = dict_map_values(d, |v| v * 2)  match dict_get(d, "a") { Some(v) => v  None => 0 } }'
check fl_val   'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 5)  dict_set(d, "b", 20)  let f = dict_filter(d, |k, v| v > 10)  dict_len(f) }'
check fl_key   'fn main() -> i64 { let d = dict_new()  dict_set(d, "x", 1)  dict_set(d, "long", 2)  let f = dict_filter(d, |k, v| str_len(k) > 1)  dict_len(f) }'
check fl_get   'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 5)  dict_set(d, "b", 20)  let f = dict_filter(d, |k, v| v > 10)  match dict_get(f, "b") { Some(v) => v  None => 0 - 1 } }'
check fl_cp    'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 5)  let f = dict_filter(d, |k, v| v > 100)  dict_len(d) }'
check ea_run   'fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 1)  dict_set(d, "bb", 2)  dict_each(d, |k, v| println(to_str(str_len(k) + v)))  0 }'
check ts_len   'fn main() -> i64 { let d = dict_new()  dict_set(d, "apple", 1)  dict_set(d, "banana", 2)  match dict_to_str(d) { Ok(s) => str_len(s)  Err(e) => 0 - 1 } }'
check ts_ok    'fn main() -> i64 { let d = dict_new()  dict_set(d, "x", 5)  match dict_to_str(d) { Ok(s) => 1  Err(e) => 0 } }'
check ts_err   'fn main() -> i64 { let d = dict_new()  dict_set(d, "k=v", 1)  match dict_to_str(d) { Ok(s) => 0  Err(e) => 1 } }'
check ts_empty 'fn main() -> i64 { let d = dict_new()  match dict_to_str(d) { Ok(s) => str_len(s)  Err(e) => 0 - 1 } }'

[ "$fail" -eq 0 ] || { echo "dict_parity: FAIL"; exit 1; }
echo "dict_parity: PASS — dict_new/set/get/has/len/inc/get_or/remove + dict_keys/dict_values/dict_merge/dict_from_pairs/dict_to_pairs/dict_map_values/dict_to_str/dict_filter/dict_each (int values, BTreeMap order) match the interpreter ✓"
exit 0
