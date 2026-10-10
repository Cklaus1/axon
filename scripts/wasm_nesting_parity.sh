#!/usr/bin/env bash
# wasm_nesting_parity.sh <ax59|ax60|ax61> — the wasm32 interpreter
# (axon-run.wasm under wasmtime's default 512 KiB stack, both engines) neither
# traps nor diverges from native on deep values and deep source
# (compilebench AX-59, AX-60, AX-61). Exit 134 is `wasm trap: call stack
# exhausted`, which I-4 forbids; every case names the exit it requires.
#
#   ax59  dropping a value 100,000 levels deep (an enum list, a dict of
#         dicts, a closure chain, and a list dropped under 100 levels of
#         recursion) exits 0 with the program's output.
#   ax60  source nested past the wasm32 limit (parser.rs `MAX_EXPR_DEPTH`):
#         parentheses 300 and 5,000 deep, 1,000-term `+` and string chains,
#         a nested `Some` pattern, an `Option<` type, nested calls and
#         blocks — each exits 2 with E0000 `expression nesting too deep
#         (limit N)` at line 2, col 5, where its root expression starts.
#   ax61  `host_await_val` without a host driver: a closure whose body is a
#         long chain, passed from depths around the finding's (1-130) under
#         `with` frames, and a 100,000-node list payload exit 101 (`no host
#         driver` or the recursion-limit panic); a payload holding a `Chan`
#         is refused with the same message (and path) as native; a `str`
#         payload still round-trips through stdin, byte-identical to native.
#
# Requires: rustup target wasm32-wasip1 + wasmtime on PATH. Skips if absent.
set -u
. "$(dirname "${BASH_SOURCE[0]}")/lib/harness_skip.sh"

# The shared wasm build lock (AUDIT O004), as in the other wasm harnesses.
if command -v flock >/dev/null 2>&1; then exec 9>"${TMPDIR:-/tmp}/axon_wasm_parity.lock" && flock 9; fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. scripts/lib/axon_bin.sh

name=wasm_nesting_parity
case="${1:-}"
case "$case" in
  ax59|ax60|ax61) ;;
  *) echo "usage: $0 <ax59|ax60|ax61>" >&2; exit 2 ;;
esac

rust_target_installed wasm32-wasip1; _rt=$?
if [ "$_rt" -eq 2 ]; then
  echo "$name: cannot determine whether wasm32-wasip1 is installed — refusing to call that a skip" >&2
  exit 1
elif [ "$_rt" -ne 0 ]; then
  harness_skip "$name" "wasm32-wasip1 target not installed"
fi
WASMTIME=""
for c in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do command -v "$c" >/dev/null 2>&1 && WASMTIME="$c" && break; done
[ -n "$WASMTIME" ] || harness_skip "$name" "wasmtime not found"

need_axon_run "$name"
# AXON_RUN_WASM: an already-built axon-run.wasm to measure (nothing built).
if [ -n "${AXON_RUN_WASM:-}" ]; then
  WASM="$AXON_RUN_WASM"
else
  echo "$name: building axon-run (wasm32-wasip1)…"
  if ! build_err="$(cargo build -q -p axon-core --no-default-features --bin axon-run --target wasm32-wasip1 2>&1)"; then
    echo "$name: FAIL: wasm build failed"
    printf '%s\n' "$build_err" | tail -20
    exit 1
  fi
  WASM="$(_axon_target_dir)/wasm32-wasip1/debug/axon-run.wasm"
fi

LIMIT="$(sed -n '/^#\[cfg(target_arch = "wasm32")\]$/{n;s/^const MAX_EXPR_DEPTH: usize = \([0-9_]*\);$/\1/p;}' \
  crates/axon-core/src/parser.rs | tr -d _)"
if ! [[ "$LIMIT" =~ ^[0-9]+$ ]]; then
  echo "$name: FAIL: no wasm32 MAX_EXPR_DEPTH in crates/axon-core/src/parser.rs"
  exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"
fails=0

# rep <s> <n> — <s> repeated <n> times.
rep() {
  local pad
  printf -v pad '%*s' "$2" ''
  printf '%s' "${pad// /$1}"
}

# wasm <engine> <prog> [stdin] — run on wasm; exit code to RC, streams to out/err.
wasm() {
  printf '%b' "${3:-}" | timeout -k 5 300 "$WASMTIME" run --dir . --env "AXON_ENGINE=$1" "$WASM" "$2" >out 2>err
  RC=$?
}

fail() {
  echo "  FAIL [$1]: $2"
  sed 's/^/    /' err | cut -c1-300 | head -5
  fails=$((fails + 1))
}

case "$case" in
ax59)
  cat >list.ax <<'EOF'
enum L { Nil, Cons { h: i64, t: L } }
fn main() -> i64 {
    let l = L::Nil
    for i in 0..100000 { l = L::Cons { h: i, t: l } }
    println("built")
    0
}
EOF
  cat >dict.ax <<'EOF'
fn main() -> i64 {
    let d = dict_new()
    for i in 0..100000 {
        let e = dict_new()
        dict_set(e, "k", d)
        d = e
    }
    println("built")
    0
}
EOF
  cat >closure.ax <<'EOF'
fn main() -> i64 {
    let f = |x: i64| x
    for i in 0..100000 {
        let g = f
        f = |x: i64| g(x) + 1
    }
    println("built")
    0
}
EOF
  cat >deep_list.ax <<'EOF'
enum L { Nil, Cons { h: i64, t: L } }
fn build(n: i64) -> i64 {
    let l = L::Nil
    for i in 0..n { l = L::Cons { h: i, t: l } }
    n
}
fn r(k: i64, n: i64) -> i64 {
    if k == 0 { build(n) } else { r(k - 1, n) }
}
fn main() -> i64 {
    println(to_str(r(100, 100000)))
    0
}
EOF
  for e in tree vm; do
    for p in list dict closure deep_list; do
      want=built; [ "$p" = deep_list ] && want=100000
      wasm "$e" "$p.ax"
      if [ "$RC" = 0 ] && [ "$(cat out)" = "$want" ]; then
        echo "  ok  [ax59 $e $p] exit 0"
      else
        fail "ax59 $e $p" "exit $RC, stdout '$(head -c 80 out)' (want exit 0, '$want')"
      fi
    done
  done
  ;;
ax60)
  msg="expression nesting too deep (limit $LIMIT)"
  src() { printf 'fn main() -> i64 {\n    %s\n    0\n}\n' "$2" >"$1.ax"; }
  src paren300 "let x = $(rep '(' 300)1$(rep ')' 300)"
  src paren5000 "let x = $(rep '(' 5000)1$(rep ')' 5000)"
  src chain "let x = 1$(rep ' + 1' 999)"
  src strchain "let x = \"a\"$(rep ' + "a"' 999)"
  src some_pattern "let x = match 1 { $(rep 'Some(' 300)y$(rep ')' 300) => 1, _ => 0 }"
  src option_type "let x: $(rep 'Option<' 300)i64$(rep '>' 300) = None"
  src calls "let x = $(rep 'f(' 300)1$(rep ')' 300)"
  src blocks "let x = $(rep '{ ' 300)1$(rep ' }' 300)"
  for e in tree vm; do
    for p in paren300 paren5000 chain strchain some_pattern option_type calls blocks; do
      wasm "$e" "$p.ax"
      if [ "$RC" = 2 ] && grep -qF "\"code\":\"E0000\",\"file\":\"$p.ax\",\"line\":2,\"col\":5,\"message\":\"$msg\"" err; then
        echo "  ok  [ax60 $e $p] exit 2, E0000 at 2:5"
      else
        fail "ax60 $e $p" "exit $RC (want exit 2, E0000 '$msg' at line 2, col 5)"
      fi
    done
  done
  ;;
ax61)
  # The finding's shape: each level a `with` around a call in nine
  # parentheses; at the bottom, a closure whose body is a long chain passed
  # to `host_await_val`. With the fixture's chain (under the limit) the run
  # reaches the builtin: exit 101. With the finding's own 700-term chain the
  # front end may refuse it (exit 2, E0000); neither may trap.
  probe="$ROOT/crates/axon-core/tests/fixtures/vm_depth/nest/host_await_closure.ax"
  for e in tree vm; do
    for d in 1 60 67 74 77 120 130; do
      { echo "let DEPTH = $d"; tail -n +2 "$probe"; } >closure.ax
      wasm "$e" closure.ax
      if [ "$RC" = 101 ] && grep -qE "no host driver|recursion limit exceeded" err; then
        echo "  ok  [ax61 $e closure depth $d] exit 101"
      else
        fail "ax61 $e closure depth $d" "exit $RC (want 101: no host driver / recursion limit exceeded)"
      fi
      sed "s/|x: i64| x\( + 1\)*$/|x: i64| x$(rep ' + 1' 700)/" closure.ax >closure700.ax
      wasm "$e" closure700.ax
      if { [ "$RC" = 101 ] && grep -qE "no host driver|recursion limit exceeded" err; } \
        || { [ "$RC" = 2 ] && grep -qF "expression nesting too deep (limit $LIMIT)" err; }; then
        echo "  ok  [ax61 $e 700-term closure depth $d] exit $RC"
      else
        fail "ax61 $e 700-term closure depth $d" "exit $RC (want 101, or 2 with the nesting refusal)"
      fi
    done
  done
  cat >list.ax <<'EOF'
enum L { Nil, Cons { h: i64, t: L } }
fn main() -> i64 {
    let l = L::Nil
    for i in 0..100000 { l = L::Cons { h: i, t: l } }
    let _r = host_await_val(l)
    0
}
EOF
  cat >capture.ax <<'EOF'
enum L { Nil, Cons { h: i64, t: L } }
fn main() -> i64 {
    let l = L::Nil
    for i in 0..100000 { l = L::Cons { h: i, t: l } }
    let f = |x: i64| match l { L::Nil => x, _ => x + 1 }
    let _r = host_await_val(f)
    0
}
EOF
  cat >chan.ax <<'EOF'
type P = { a: i64, q: [Chan<i64>] }
fn main() -> i64 {
    let c = chan<i64>()
    let p = P { a: 2, q: [c] }
    let f = |x: i64| x + p.a
    let _r = host_await_val((1, [f]))
    0
}
EOF
  cat >str.ax <<'EOF'
fn main() -> i64 {
    let r: str = host_await_val("name? ")
    println("got " + r)
    let o = host_await_val_opt("again? ")
    println(to_str(o))
    0
}
EOF
  "$INTERP" run chan.ax </dev/null >/dev/null 2>native.err
  native_chan="$(grep -o 'host_await_val: payload cannot cross.*' native.err)"
  printf 'ada\n' | "$INTERP" run str.ax >native.out 2>/dev/null
  native_str_rc=$?
  for e in tree vm; do
    for p in list capture; do
      wasm "$e" "$p.ax"
      if [ "$RC" = 101 ] && grep -q "no host driver" err; then
        echo "  ok  [ax61 $e $p payload] exit 101"
      else
        fail "ax61 $e $p payload" "exit $RC (want 101: no host driver)"
      fi
    done
    wasm "$e" chan.ax
    wasm_chan="$(grep -o 'host_await_val: payload cannot cross.*' err)"
    if [ "$RC" = 101 ] && [ -n "$native_chan" ] && [ "$wasm_chan" = "$native_chan" ]; then
      echo "  ok  [ax61 $e chan payload] exit 101, refusal identical to native"
    else
      fail "ax61 $e chan payload" "exit $RC; wasm '$wasm_chan' != native '$native_chan'"
    fi
    wasm "$e" str.ax 'ada\n'
    if [ "$RC" = "$native_str_rc" ] && cmp -s out native.out; then
      echo "  ok  [ax61 $e str payload] round-trips, identical to native"
    else
      fail "ax61 $e str payload" "wasm[$RC] '$(head -c 80 out)' != native[$native_str_rc] '$(head -c 80 native.out)'"
    fi
  done
  ;;
esac

if [ "$fails" -ne 0 ]; then
  echo "$name: FAIL ($case) — $fails case(s)"
  exit 1
fi
echo "wasm_nesting_parity: PASS ($case)"
