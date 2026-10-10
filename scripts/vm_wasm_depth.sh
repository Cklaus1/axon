#!/usr/bin/env bash
# vm_wasm_depth.sh — R50 §4 Execution (Recursion) gate: how deep the wasm32
# interpreter recurses under both engines before its stack runs out.
#
# On wasm32 a stack overflow traps the module, which I-4 forbids, so the
# recursion guard (128 on wasm32, AX-56) must fire before either stack is
# exhausted:
#   - the 64 MiB linear-memory stack (`.cargo/config.toml` -zstack-size), and
#   - the wasm engine's own native stack (wasmtime's `max-wasm-stack`, 512 KiB
#     by default), which is reached first.
#
# The script builds `axon-run` for wasm32-wasip1 in BOTH profiles (debug and
# release) from the working tree under test, and runs the four chain programs in
# crates/axon-core/tests/fixtures/vm_depth/ (plain, closure, mut, with). Each
# fixture's FIRST line is `let DEPTH = 100`; every probe rewrites it in a
# temporary copy. Every chain runs under both engines of the same build
# (`--env AXON_ENGINE=tree|vm`) in three configurations:
#
#   linear   `-W max-wasm-stack=4294967295 --env AXON_MAX_DEPTH=1000000`: native
#            stack and guard lifted, so the linear-memory stack is the bound.
#            Bisects the deepest completed depth. REQUIRED: >= 1.3 x the guard
#            for every chain, engine and profile.
#   default  wasmtime's default max-wasm-stack, `--env AXON_MAX_DEPTH=1000000`
#            (guard lifted so the number is the native stack's own bound).
#            Bisects the deepest completed depth. REQUIRED: >= 1.2 x the guard
#            for every chain, engine and profile. With --require-default-stack
#            the VM depth must also be >= the tree's for every (profile, chain)
#            — the R50 S6 gate (R50 §4, §12 Q4).
#   guard    wasmtime's default max-wasm-stack, AXON_MAX_DEPTH unset:
#            DEPTH=<guard> must exit 101 with `recursion limit exceeded
#            (<guard>)` on stderr, not trap.
#   nest     wasmtime's default max-wasm-stack, AXON_MAX_DEPTH unset, for the
#            nesting probes in crates/axon-core/tests/fixtures/vm_depth/nest/
#            (a call level nested in expressions, `with` bodies, callbacks,
#            lambdas, ...; same first-line convention): DEPTH=100000 must exit
#            101 with `recursion limit exceeded (<guard>)`, not trap. These
#            shapes take more stack per level than the chains, so the wasm32
#            stack budget (interp.rs `nest_cost`) is what stops them.
#            A probe whose SECOND line is `// vm_wasm_depth: <kind>` is held
#            to that kind's rule instead (never a trap, exit 134, in any):
#              exit0  DEPTH=100000 must exit 0 (AX-59: dropping a value
#                     100,000 levels deep at the bottom of a recursion).
#              sweep  every DEPTH from 1 to <guard> + 2 must exit 101 with
#                     `no host driver` or `recursion limit exceeded
#                     (<guard>)` (AX-61: `host_await_val` of a closure).
#              front  a template: each `⟪open¦leaf¦close⟫` is expanded to
#                     open×DEPTH leaf close×DEPTH. At DEPTH = the wasm32
#                     nesting limit (parser.rs `MAX_EXPR_DEPTH`) it must
#                     exit 0 or 2 with `expression nesting too deep
#                     (limit <limit>)`, at 20× the limit exit 2 with that
#                     message (AX-60: the front end on deep source).
#   reach    the nest configuration, bisecting the deepest depth that exits 0
#            (at most <guard> - 1) of each chain and nesting probe (for a
#            `front` probe, the deepest nesting the front end accepts; none
#            for `exit0` and `sweep`). The budget
#            charges the two engines differently, so with
#            --require-default-stack the VM depth must be >= the tree's for
#            every (profile, chain or probe): the default engine must not
#            panic on a program the tree completes.
#
# Before probing, the script checks the stack budget is sound on both builds
# (scripts/wasm_stack_budget.py): every recursion through `Interp::eval`
# passes a function carrying `nest_guard!`, no recursion runs through an
# indirect call, and each guarded function's `nest_cost` constant covers its
# native frame plus the deepest unguarded chain it can call before the next
# guard; the drop-glue recursion over a value's depth passes `bounded_drop`
# and `DROP_INLINE_DEPTH` levels of it fit the headroom the budget leaves;
# and the parser's worst frame chain per `MAX_EXPR_DEPTH` unit, times the
# wasm32 limit, fits the budget. It compiles both builds with `wasmtime
# compile`, disassembles them
# (`objdump -d` | `c++filt`, `wasm-objdump`) and fails on any violation: a
# frame or tail that grew would let a nesting shape trap under the guard. The
# constants are Cranelift x86-64 frames, so this check needs an x86-64 host
# with binutils and wabt; anything else is a setup error.
#
# "Completed" means exit 0 with the chain's result (DEPTH) on stdout; for the
# nesting probes (reach), exit 0.
# Output: one line per (profile, engine, chain, configuration):
#   vm_wasm_depth: <profile> <engine> <chain> <config> <result> <verdict>
# then a summary. Exit 0 when every required check holds, 1 otherwise, 2 on a
# setup error (no wasmtime / objdump / c++filt / wasm-objdump, not x86-64,
# build failure).
#
# Usage:
#   scripts/vm_wasm_depth.sh [--require-default-stack]
# Env:
#   VM_DEPTH_JOBS   parallel bisections (default: nproc)
#   WASMTIME        wasmtime binary (default: from PATH)
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

REQUIRE_DEFAULT=0
for arg in "$@"; do
  case "$arg" in
    --require-default-stack) REQUIRE_DEFAULT=1 ;;
    -h|--help) sed -n '2,/^set -uo pipefail$/p' "$0" | sed '$d'; exit 0 ;;
    *) echo "vm_wasm_depth: unknown flag $arg" >&2; exit 2 ;;
  esac
done

WASMTIME="${WASMTIME:-$(command -v wasmtime || true)}"
if [ -z "$WASMTIME" ] || [ ! -x "$WASMTIME" ]; then
  echo "vm_wasm_depth: wasmtime not found — cannot measure (this is a failure, not a skip)" >&2
  exit 2
fi
for tool in objdump c++filt wasm-objdump python3; do
  if ! command -v "$tool" >/dev/null; then
    echo "vm_wasm_depth: $tool not found — cannot check the nest_cost frames (this is a failure, not a skip)" >&2
    exit 2
  fi
done
if [ "$(uname -m)" != x86_64 ]; then
  echo "vm_wasm_depth: nest_cost constants are x86-64 frames; run on an x86-64 host (got $(uname -m))" >&2
  exit 2
fi

GUARD=128                          # the wasm32 RECURSION_LIMIT (interp.rs)
REQUIRED_LINEAR=$((GUARD * 13 / 10 + 1))    # > 1.3 x GUARD
REQUIRED_DEFAULT=$((GUARD * 12 / 10 + 1))   # > 1.2 x GUARD
CHAINS=(plain closure mut with)
ENGINES=(tree vm)
PROFILES=(debug release)
FIX="$ROOT/crates/axon-core/tests/fixtures/vm_depth"
NESTS=()
for f in "$FIX"/nest/*.ax; do NESTS+=("nest/$(basename "$f" .ax)"); done
for c in "${CHAINS[@]}" "${NESTS[@]}"; do
  if [ "$(head -1 "$FIX/$c.ax")" != "let DEPTH = 100" ]; then
    echo "vm_wasm_depth: $FIX/$c.ax must start with 'let DEPTH = 100'" >&2
    exit 2
  fi
done
if [ "${#NESTS[@]}" -eq 0 ]; then
  echo "vm_wasm_depth: no nesting probes in $FIX/nest" >&2
  exit 2
fi
# kind <probe> — the rule a nesting probe is held to: its second line's
# `// vm_wasm_depth: <kind>`, else `guard` (the recursion-limit panic).
kind() {
  local k
  k="$(sed -n '2s|^// vm_wasm_depth: \([a-z0-9]*\)$|\1|p' "$FIX/$1.ax")"
  echo "${k:-guard}"
}
# bad <rc> <dir> — a failed probe's result line. Exit 2 from a guard, exit0
# or sweep probe is the front end refusing its source (AX-60), so the probe
# measures nothing: it must be regenerated under the wasm32 nesting limit.
bad() {
  local why=""
  [ "$1" = 2 ] && why=" (front end refused the probe: regenerate it under the wasm32 limit $FRONT_LIMIT)"
  echo "exit=$1 bad$why: $(head -c 200 "$2/err" | tr '\n' ' ')"
}
for n in "${NESTS[@]}"; do
  case "$(kind "$n")" in
    guard|exit0|sweep|front) ;;
    *) echo "vm_wasm_depth: $FIX/$n.ax: unknown probe kind '$(kind "$n")'" >&2; exit 2 ;;
  esac
done
# The wasm32 nesting limit (parser.rs `MAX_EXPR_DEPTH`, AX-60).
FRONT_LIMIT="$(sed -n '/^#\[cfg(target_arch = "wasm32")\]$/{n;s/^const MAX_EXPR_DEPTH: usize = \([0-9_]*\);$/\1/p;}' \
  "$ROOT/crates/axon-core/src/parser.rs" | tr -d _)"
if ! [[ "$FRONT_LIMIT" =~ ^[0-9]+$ ]]; then
  echo "vm_wasm_depth: no wasm32 MAX_EXPR_DEPTH in parser.rs" >&2
  exit 2
fi

echo "vm_wasm_depth: building axon-run (wasm32-wasip1, debug + release)…"
cargo build -q -p axon-core --no-default-features --bin axon-run --target wasm32-wasip1 \
  || { echo "vm_wasm_depth: debug build failed" >&2; exit 2; }
# Unstripped, so the frame check can name functions; strip removes only the
# name section, so the code the probes run is the same.
CARGO_PROFILE_RELEASE_STRIP=none \
  cargo build -q --release -p axon-core --no-default-features --bin axon-run --target wasm32-wasip1 \
  || { echo "vm_wasm_depth: release build failed" >&2; exit 2; }
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$ROOT/$TARGET_DIR" ;; esac

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# frames <profile> — check the stack budget of the build of <profile> with
# scripts/wasm_stack_budget.py. Prints one line per constant; returns 1 when
# the budget is unsound, 2 on a setup error.
frames() {
  local profile="$1"
  local wasm="$TARGET_DIR/wasm32-wasip1/$profile/axon-run.wasm"
  local out="$WORK/$profile"
  "$WASMTIME" compile "$wasm" -o "$out.cwasm" \
    || { echo "vm_wasm_depth: wasmtime compile ($profile) failed" >&2; return 2; }
  objdump -d --no-show-raw-insn "$out.cwasm" | c++filt >"$out.dis" \
    && wasm-objdump -x "$wasm" >"$out.wx" \
    && wasm-objdump -d "$wasm" >"$out.wd" \
    || { echo "vm_wasm_depth: disassembly ($profile) failed" >&2; return 2; }
  python3 "$ROOT/scripts/wasm_stack_budget.py" "$ROOT/crates/axon-core/src/interp.rs" \
    "$profile" "$out.wx" "$out.wd" "$out.dis"
}
FRAME_FAILS=0
for profile in "${PROFILES[@]}"; do
  frames "$profile"
  case $? in 0) ;; 1) FRAME_FAILS=$((FRAME_FAILS + 1)) ;; *) exit 2 ;; esac
done

# probe <wasm> <engine> <chain> <config> <depth> <dir> — run one probe.
# Prints the exit code; leaves stdout/stderr in <dir>/out and <dir>/err.
probe() {
  local wasm="$1" engine="$2" chain="$3" config="$4" depth="$5" dir="$6"
  local -a cfg
  case "$config" in
    linear)  cfg=(-W max-wasm-stack=4294967295 --env AXON_MAX_DEPTH=1000000) ;;
    default) cfg=(--env AXON_MAX_DEPTH=1000000) ;;
    guard|nest|reach) cfg=() ;;
  esac
  { echo "let DEPTH = $depth"; tail -n +2 "$FIX/$chain.ax"; } >"$dir/p.ax"
  if [ "$(kind "$chain")" = front ]; then
    python3 -c 'import re, sys
n = int(sys.argv[2]); p = sys.argv[1]; s = open(p, encoding="utf-8").read()
s = re.sub("⟪(.*?)¦(.*?)¦(.*?)⟫", lambda m: m[1] * n + m[2] + m[3] * n, s)
open(p, "w", encoding="utf-8").write(s)' "$dir/p.ax" "$depth"
  fi
  timeout -k 5 300 "$WASMTIME" run "${cfg[@]}" --env "AXON_ENGINE=$engine" \
    --dir "$dir" "$wasm" "$dir/p.ax" </dev/null >"$dir/out" 2>"$dir/err"
  echo $?
}

completes() { # same args as probe
  local rc
  rc="$(probe "$@")"
  [ "$rc" = 0 ] && { [ "$4" = reach ] || [ "$(cat "$6/out")" = "$5" ]; }
}

# bisect <wasm> <engine> <chain> <config> <dir> — deepest completed depth.
# Exponential search for the first failing depth, then binary search. Assumes
# completion is monotone in depth. Prints the depth (0 if depth 1 fails).
bisect() {
  local wasm="$1" engine="$2" chain="$3" config="$4" dir="$5"
  local lo=0 hi=1 cap=$((1 << 20)) mid
  while [ "$hi" -le "$cap" ] && completes "$wasm" "$engine" "$chain" "$config" "$hi" "$dir"; do
    lo=$hi; hi=$((hi * 2))
  done
  if [ "$hi" -gt "$cap" ]; then echo "$lo"; return; fi
  while [ $((hi - lo)) -gt 1 ]; do
    mid=$(((lo + hi) / 2))
    if completes "$wasm" "$engine" "$chain" "$config" "$mid" "$dir"; then lo=$mid; else hi=$mid; fi
  done
  echo "$lo"
}

# job <profile> <engine> <chain> <config> — one measurement, result to a file.
job() {
  local profile="$1" engine="$2" chain="$3" config="$4"
  local wasm="$TARGET_DIR/wasm32-wasip1/$profile/axon-run.wasm"
  local dir="$WORK/$profile.$engine.${chain//\//_}.$config"
  mkdir -p "$dir"
  if [ "$config" = guard ] || [ "$config" = nest ]; then
    local rc depth="$GUARD" deep k
    k="$(kind "$chain")"
    [ "$config" = guard ] && k=guard
    local refusal="expression nesting too deep (limit $FRONT_LIMIT)"
    case "$k" in
      guard)
        [ "$config" = nest ] && depth=100000
        rc="$(probe "$wasm" "$engine" "$chain" guard "$depth" "$dir")"
        if [ "$rc" = 101 ] && grep -q "recursion limit exceeded ($GUARD)" "$dir/err"; then
          echo "exit=101 ok" >"$dir/result"
        else
          bad "$rc" "$dir" >"$dir/result"
        fi ;;
      exit0)
        rc="$(probe "$wasm" "$engine" "$chain" guard 100000 "$dir")"
        if [ "$rc" = 0 ]; then
          echo "ok exit=0 (DEPTH=100000)" >"$dir/result"
        else
          bad "$rc" "$dir" >"$dir/result"
        fi ;;
      sweep)
        echo "ok exit=101 at DEPTH 1..$((GUARD + 2))" >"$dir/result"
        for ((depth = 1; depth <= GUARD + 2; depth++)); do
          rc="$(probe "$wasm" "$engine" "$chain" guard "$depth" "$dir")"
          if ! { [ "$rc" = 101 ] && grep -qE "no host driver|recursion limit exceeded \($GUARD\)" "$dir/err"; }; then
            echo "DEPTH=$depth $(bad "$rc" "$dir")" >"$dir/result"
            break
          fi
        done ;;
      front)
        deep=$((FRONT_LIMIT * 20))
        rc="$(probe "$wasm" "$engine" "$chain" guard "$FRONT_LIMIT" "$dir")"
        if ! { [ "$rc" = 0 ] || { [ "$rc" = 2 ] && grep -qF "$refusal" "$dir/err"; }; }; then
          echo "DEPTH=$FRONT_LIMIT exit=$rc bad: $(head -c 200 "$dir/err" | tr '\n' ' ')" >"$dir/result"
          return
        fi
        local first="$rc"
        rc="$(probe "$wasm" "$engine" "$chain" guard "$deep" "$dir")"
        if [ "$rc" = 2 ] && grep -qF "$refusal" "$dir/err"; then
          echo "ok exit=$first at $FRONT_LIMIT, E0000 at $deep" >"$dir/result"
        else
          echo "DEPTH=$deep exit=$rc bad: $(head -c 200 "$dir/err" | tr '\n' ' ')" >"$dir/result"
        fi ;;
    esac
  else
    bisect "$wasm" "$engine" "$chain" "$config" "$dir" >"$dir/result"
  fi
}
export -f probe completes bisect job kind bad
export WASMTIME FIX WORK TARGET_DIR GUARD FRONT_LIMIT

JOBS="${VM_DEPTH_JOBS:-$(nproc)}"
for profile in "${PROFILES[@]}"; do
  for engine in "${ENGINES[@]}"; do
    for chain in "${CHAINS[@]}"; do
      for config in linear default guard reach; do
        printf '%s %s %s %s\n' "$profile" "$engine" "$chain" "$config"
      done
    done
    for n in "${NESTS[@]}"; do
      printf '%s %s %s nest\n' "$profile" "$engine" "$n"
      case "$(kind "$n")" in guard|front) printf '%s %s %s reach\n' "$profile" "$engine" "$n" ;; esac
    done
  done
done | xargs -P "$JOBS" -L 1 bash -c 'job "$@"' _

fails=$FRAME_FAILS
declare -A depth
for profile in "${PROFILES[@]}"; do
  for engine in "${ENGINES[@]}"; do
    for chain in "${CHAINS[@]}"; do
      for config in linear default guard; do
        key="$profile.$engine.$chain.$config"
        res="$(cat "$WORK/$key/result" 2>/dev/null || echo "missing")"
        verdict=""
        case "$config" in
          linear)
            depth[$key]="$res"
            if [[ "$res" =~ ^[0-9]+$ ]] && [ "$res" -ge "$REQUIRED_LINEAR" ]; then
              verdict="ok (>= $REQUIRED_LINEAR)"
            else
              verdict="FAIL (< $REQUIRED_LINEAR)"; fails=$((fails + 1))
            fi
            res="deepest=$res" ;;
          default)
            depth[$key]="$res"
            if [[ "$res" =~ ^[0-9]+$ ]] && [ "$res" -ge "$REQUIRED_DEFAULT" ]; then
              verdict="ok (>= $REQUIRED_DEFAULT)"
            else
              verdict="FAIL (< $REQUIRED_DEFAULT)"; fails=$((fails + 1))
            fi
            res="deepest=$res" ;;
          guard)
            case "$res" in
              "exit=101 ok") verdict="ok (depth panic at $GUARD)"; res="exit=101" ;;
              *) verdict="FAIL (expected exit 101 'recursion limit exceeded ($GUARD)')"; fails=$((fails + 1)) ;;
            esac ;;
        esac
        printf 'vm_wasm_depth: %-7s %-4s %-7s %-7s %-14s %s\n' "$profile" "$engine" "$chain" "$config" "$res" "$verdict"
      done
      depth[$profile.$engine.$chain.reach]="$(cat "$WORK/$profile.$engine.$chain.reach/result" 2>/dev/null || echo "missing")"
    done
    for n in "${NESTS[@]}"; do
      res="$(cat "$WORK/$profile.$engine.${n//\//_}.nest/result" 2>/dev/null || echo "missing")"
      k="$(kind "$n")"
      case "$k:$res" in
        "guard:exit=101 ok") verdict="ok (recursion-limit panic, no trap)"; res="exit=101" ;;
        guard:*) verdict="FAIL (expected exit 101 'recursion limit exceeded ($GUARD)')"; fails=$((fails + 1)) ;;
        *:"ok "*) verdict="ok ($k: ${res#ok }, no trap)"; res="$k" ;;
        *) verdict="FAIL ($k rule)"; fails=$((fails + 1)) ;;
      esac
      printf 'vm_wasm_depth: %-7s %-4s %-14s nest    %-14s %s\n' "$profile" "$engine" "$n" "$res" "$verdict"
      depth[$profile.$engine.$n.reach]="$(cat "$WORK/$profile.$engine.${n//\//_}.reach/result" 2>/dev/null || echo "missing")"
    done
  done
done

# Default-stack comparison: VM vs tree per (profile, chain).
for profile in "${PROFILES[@]}"; do
  for chain in "${CHAINS[@]}"; do
    t="${depth[$profile.tree.$chain.default]}"; v="${depth[$profile.vm.$chain.default]}"
    [[ "$t" =~ ^[0-9]+$ && "$v" =~ ^[0-9]+$ ]] || continue
    if [ "$v" -ge "$t" ]; then
      cmp="vm >= tree"
    elif [ "$REQUIRE_DEFAULT" = 1 ]; then
      cmp="FAIL vm < tree (--require-default-stack)"; fails=$((fails + 1))
    else
      cmp="vm < tree (reported; required only with --require-default-stack)"
    fi
    printf 'vm_wasm_depth: %-7s default-stack %-7s tree=%s vm=%s %s\n' "$profile" "$chain" "$t" "$v" "$cmp"
  done
done

# Comparison under the guard: VM vs tree per (profile, chain or probe).
for profile in "${PROFILES[@]}"; do
  for n in "${CHAINS[@]}" "${NESTS[@]}"; do
    case "$n" in nest/*) case "$(kind "$n")" in exit0|sweep) continue ;; esac ;; esac
    t="${depth[$profile.tree.$n.reach]}"; v="${depth[$profile.vm.$n.reach]}"
    if ! [[ "$t" =~ ^[0-9]+$ && "$v" =~ ^[0-9]+$ ]]; then
      cmp="FAIL (no depth measured)"; fails=$((fails + 1))
    elif [ "$v" -ge "$t" ]; then
      cmp="vm >= tree"
    elif [ "$REQUIRE_DEFAULT" = 1 ]; then
      cmp="FAIL vm < tree (--require-default-stack)"; fails=$((fails + 1))
    else
      cmp="vm < tree (reported; required only with --require-default-stack)"
    fi
    printf 'vm_wasm_depth: %-7s reach %-14s tree=%s vm=%s %s\n' "$profile" "$n" "$t" "$v" "$cmp"
  done
done

if [ "$fails" -ne 0 ]; then
  echo "vm_wasm_depth: FAILED — $fails check(s)"
  exit 1
fi
echo "vm_wasm_depth: PASS — the wasm32 stack budget is sound (every recursion through eval is guarded, each nest_cost covers its frame and unguarded tail, value drops are bounded, the parser's limit fits); every chain completes $REQUIRED_LINEAR in the linear stack and $REQUIRED_DEFAULT under the default stack, and panics at $GUARD; every nesting probe panics instead of trapping, every drop, host_await and front-end probe meets its rule (limit $FRONT_LIMIT) without a trap; under both engines and profiles$([ "$REQUIRE_DEFAULT" = 1 ] && echo "; the VM reaches at least the tree's depth on every chain and probe")"
exit 0
