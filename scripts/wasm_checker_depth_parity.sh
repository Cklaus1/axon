#!/usr/bin/env bash
# wasm_checker_depth_parity.sh [ax63|ax66] — the wasm32 type checker (axon-run.wasm
# under wasmtime's default 512 KiB stack, both engines) neither traps nor diverges
# from native on flat source that makes it recurse deeply (compilebench AX-63,
# AX-66). Exit 134 is `wasm trap: call stack exhausted`, which I-4 forbids; every
# case names the exit it requires.
#
#   ax63  `let a{i} = [a{i-1}]` repeated N times gives `a{N}` a type N levels deep
#         while every source line is shallow. A type up to the limit
#         (infer.rs `MAX_TYPE_DEPTH`, wasm32 only) runs: N = 100 and 223 print `7`,
#         as native does. Past it (N = 225, 3,000, 5,000, 20,000; the finding's
#         trap was at 3,000 debug / 5,000 release) the program is refused: exit 2
#         with E0301 `the type of `a223` is nested more than <limit> levels deep`
#         at the binding, never a trap. Native keeps no limit (it prints `7` at
#         5,000), so the refused runs are not diffed against it.
#   ax66  chains of declarations, each a flat line, walked once per link:
#         a `@[contained]` fn calling a chain of 1,500 and 20,000 helpers, a
#         `@[total]` chain of 3,000 and 20,000 fns, and a chain of 20,000 and
#         50,000 named refinements all exit 0 and print what native prints
#         (`ok`, `7`). A forbidden `read_file` at the END of a 1,500-helper chain
#         under `@[contained]` is still refused: the same diagnostics as native,
#         not a trap and not silence. A `@[total]` ring of 2,000 fns is still the
#         E1208 mutual-recursion refusal, as on native.
#
# Every case runs on both the debug and the release axon-run.wasm, under both
# engines; each output line names the run as `<profile>/<engine>`.
# AXON_RUN_WASM=<path> runs only that module instead (named `given`).
#
# With no argument (as parity_all.sh runs every harness) it runs both.
# Requires: rustup target wasm32-wasip1 + wasmtime on PATH. Skips if absent.
set -u
. "$(dirname "${BASH_SOURCE[0]}")/lib/harness_skip.sh"

# No argument: each case in its own run, before the build lock below is taken
# (each run takes it). A skip is environmental, the same for every case.
if [ $# -eq 0 ]; then
  rc=0
  for c in ax63 ax66; do
    out="$(bash "${BASH_SOURCE[0]}" "$c" 2>&1)"; r=$?
    printf '%s\n' "$out"
    case "$(printf '%s\n' "$out" | tail -n 1)" in
      *": SKIP"*) [ "$r" -eq 0 ] && exit 0 ;;
    esac
    [ "$r" -eq 0 ] || rc=1
  done
  if [ "$rc" -ne 0 ]; then echo "wasm_checker_depth_parity: FAIL (all)"; exit 1; fi
  echo "wasm_checker_depth_parity: PASS (all)"
  exit 0
fi

# The shared wasm build lock (AUDIT O004), as in the other wasm harnesses.
if command -v flock >/dev/null 2>&1; then exec 9>"${TMPDIR:-/tmp}/axon_wasm_parity.lock" && flock 9; fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. scripts/lib/axon_bin.sh

name=wasm_checker_depth_parity
case="${1:-}"
case "$case" in
  ax63|ax66) ;;
  *) echo "usage: $0 [ax63|ax66]" >&2; exit 2 ;;
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

# The native runner the wasm runs are diffed against: the caller's AXON_RUN
# (cli_run names the one cargo built for the test run), else the one built here.
if [ -n "${AXON_RUN:-}" ]; then
  named_bin AXON_RUN "$name"
else
  if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
    harness_skip "$name" "interp build unavailable"
  fi
  use_built AXON_RUN axon-run
fi
INTERP="$AXON_RUN"
# AXON_RUN_WASM: an already-built axon-run.wasm to measure (nothing built).
# Otherwise the debug and the release module, the release one built as
# vm_wasm_depth.sh builds it (unstripped), so the two share one build.
declare -A WASM_OF
if [ -n "${AXON_RUN_WASM:-}" ]; then
  WASM_OF[given]="$AXON_RUN_WASM"
  PROFILES="given"
else
  echo "$name: building axon-run (wasm32-wasip1, debug + release)…"
  if ! build_err="$(cargo build -q -p axon-core --no-default-features --bin axon-run --target wasm32-wasip1 2>&1)"; then
    echo "$name: FAIL: wasm debug build failed"
    printf '%s\n' "$build_err" | tail -20
    exit 1
  fi
  if ! build_err="$(CARGO_PROFILE_RELEASE_STRIP=none cargo build -q --release -p axon-core \
    --no-default-features --bin axon-run --target wasm32-wasip1 2>&1)"; then
    echo "$name: FAIL: wasm release build failed"
    printf '%s\n' "$build_err" | tail -20
    exit 1
  fi
  WASM_DEBUG=""; use_built WASM_DEBUG axon-run.wasm wasm32-wasip1            # the debug build just made
  WASM_RELEASE=""; use_built WASM_RELEASE axon-run.wasm wasm32-wasip1 release # the release build just made
  WASM_OF[debug]="$WASM_DEBUG"
  WASM_OF[release]="$WASM_RELEASE"
  PROFILES="debug release"
fi
# RUNS: every `<profile>/<engine>` a case runs under.
RUNS=()
for prof in $PROFILES; do RUNS+=("$prof/tree" "$prof/vm"); done

LIMIT="$(sed -n '/^#\[cfg(target_arch = "wasm32")\]$/{n;s/^pub(crate) const MAX_TYPE_DEPTH: Option<usize> = Some(\([0-9_]*\));$/\1/p;}' \
  crates/axon-core/src/infer.rs | tr -d _)"
if ! [[ "$LIMIT" =~ ^[0-9]+$ ]]; then
  echo "$name: FAIL: no wasm32 MAX_TYPE_DEPTH in crates/axon-core/src/infer.rs"
  exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"
fails=0

# wasm <profile>/<engine> <prog> — run on that profile's wasm module under that
# engine; exit code to RC, streams to out/err.
wasm() {
  timeout -k 5 600 "$WASMTIME" run --dir . --env "AXON_ENGINE=${1#*/}" \
    "${WASM_OF[${1%/*}]}" "$2" >out 2>err
  RC=$?
}

fail() {
  echo "  FAIL [$1]: $2"
  sed 's/^/    /' err | cut -c1-300 | head -5
  fails=$((fails + 1))
}

# gen <kind> <n> > file — the finding's repro shapes (see the header).
gen() {
  python3 - "$1" "$2" <<'PY'
import sys
kind, n = sys.argv[1], int(sys.argv[2])
main = 'fn main() -> i64 { println("ok") 0 }'
o = []
if kind == 'deeptype':
    o += ['fn f() -> i64 {', 'let a0 = [1]']
    o += [f'let a{i} = [a{i-1}]' for i in range(1, n)]
    o += ['7 }', 'fn main() -> i64 { println(to_str(f())) 0 }']
elif kind == 'contained':
    o.append('fn f0() -> i64 { 0 }')
    o += [f'fn f{i}() -> i64 {{ f{i-1}() }}' for i in range(1, n)]
    o += [f'@[contained] fn g() -> i64 {{ f{n-1}() }}', main]
elif kind == 'contained_bad':
    o.append('fn f0() -> str { read_file("/etc/passwd") }')
    o += [f'fn f{i}() -> str {{ f{i-1}() }}' for i in range(1, n)]
    o += [f'@[contained(fs: [], net: [], exec: none)] fn g() -> str {{ f{n-1}() }}', main]
elif kind == 'total':
    o.append('@[total] fn f0() -> i64 { 0 }')
    o += [f'@[total] fn f{i}() -> i64 {{ f{i-1}() }}' for i in range(1, n)]
    o.append(main)
elif kind == 'total_ring':
    o += [f'@[total] fn f{i}() -> i64 {{ f{(i+1) % n}() }}' for i in range(n)]
    o.append(main)
elif kind == 'refine':
    o.append('type A0 = i64 where true')
    o += [f'type A{i} = A{i-1} where true' for i in range(1, n)]
    o += [f'fn h(x: A{n-1}) -> i64 {{ 7 }}', 'fn main() -> i64 { println(to_str(h(3))) 0 }']
print('\n'.join(o))
PY
}

# ok_run <case> <kind> <n> <want-stdout> — exit 0 and that stdout, every run;
# native must print the same.
ok_run() {
  gen "$2" "$3" >"$2_$3.ax"
  "$INTERP" run "$2_$3.ax" >native.out 2>native.err
  native_rc=$?
  if [ "$native_rc" != 0 ] || [ "$(cat native.out)" != "$4" ]; then
    echo "  FAIL [$1 native $2 $3]: exit $native_rc, stdout '$(head -c 80 native.out)' (want 0, '$4')"
    fails=$((fails + 1))
    return
  fi
  for e in "${RUNS[@]}"; do
    wasm "$e" "$2_$3.ax"
    if [ "$RC" = 0 ] && [ "$(cat out)" = "$4" ]; then
      echo "  ok  [$1 $e $2 $3] exit 0, '$4' as native"
    else
      fail "$1 $e $2 $3" "exit $RC, stdout '$(head -c 80 out)' (want 0, '$4')"
    fi
  done
}

case "$case" in
ax63)
  for n in 100 223; do ok_run ax63 deeptype "$n" 7; done
  # The first refusal is the binding whose type first passes the limit: a0 = [1]
  # is two levels, so a{limit-1} is `limit + 1`.
  want_name="a$((LIMIT - 1))"
  for n in 225 3000 5000 20000; do
    gen deeptype "$n" >"deeptype_$n.ax"
    for e in "${RUNS[@]}"; do
      wasm "$e" "deeptype_$n.ax"
      first="$(head -1 err)"
      if [ "$RC" = 2 ] && [[ "$first" == *'"code":"E0301"'* ]] \
        && [[ "$first" == *"the type of \`$want_name\` is nested more than $LIMIT levels deep"* ]]; then
        echo "  ok  [ax63 $e deeptype $n] exit 2, E0301 at \`$want_name\`"
      else
        fail "ax63 $e deeptype $n" "exit $RC, first diagnostic '$first' (want exit 2, E0301 for \`$want_name\`)"
      fi
    done
  done
  ;;
ax66)
  for n in 1500 20000; do ok_run ax66 contained "$n" ok; done
  for n in 3000 20000; do ok_run ax66 total "$n" ok; done
  for n in 20000 50000; do ok_run ax66 refine "$n" 7; done
  # Refusals survive the walk's new shape: the same diagnostics as native.
  for kind in contained_bad total_ring; do
    n=1500
    [ "$kind" = total_ring ] && n=2000
    gen "$kind" "$n" >"$kind.ax"
    "$INTERP" run "$kind.ax" >native.out 2>native.err
    native_rc=$?
    native_diag="$(sort native.err | head -3)"
    if [ "$native_rc" != 2 ] || [ -z "$native_diag" ]; then
      echo "  FAIL [ax66 native $kind]: exit $native_rc (want 2 with a diagnostic)"
      fails=$((fails + 1))
      continue
    fi
    for e in "${RUNS[@]}"; do
      wasm "$e" "$kind.ax"
      wasm_diag="$(sort err | head -3)"
      if [ "$RC" = 2 ] && [ "$wasm_diag" = "$native_diag" ]; then
        echo "  ok  [ax66 $e $kind] exit 2, diagnostics identical to native"
      else
        fail "ax66 $e $kind" "exit $RC; wasm '$wasm_diag' != native '$native_diag'"
      fi
    done
  done
  ;;
esac

if [ "$fails" -ne 0 ]; then
  echo "$name: FAIL ($case) — $fails case(s)"
  exit 1
fi
echo "wasm_checker_depth_parity: PASS ($case)"
