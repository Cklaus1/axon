#!/usr/bin/env bash
# vm_wasm_depth.sh — R50 §4 Execution (Recursion) gate: how deep the wasm32
# interpreter recurses under both engines before its stack runs out.
#
# On wasm32 a stack overflow traps the module, which I-4 forbids, so the
# recursion guard (450 on wasm32) must fire before either stack is exhausted:
#   - the 64 MiB linear-memory stack (`.cargo/config.toml` -zstack-size), and
#   - the wasm engine's own native stack (wasmtime's `max-wasm-stack`).
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
#            Bisects the deepest completed depth. REQUIRED: >= 585 (1.3 x 450)
#            for every chain, engine and profile.
#   default  wasmtime's default max-wasm-stack, `--env AXON_MAX_DEPTH=1000000`
#            (guard lifted so the number is the native stack's own bound).
#            Bisects the deepest completed depth. REPORTED only, unless
#            --require-default-stack: then the VM depth must be >= the tree's
#            for every (profile, chain) — the S6 gate (§4, §12 Q4).
#   guard    `-W max-wasm-stack=4294967295`, AXON_MAX_DEPTH unset: DEPTH=450
#            must exit 101 with `recursion limit exceeded (450)` on stderr.
#
# "Completed" means exit 0 with the chain's result (DEPTH) on stdout.
# Output: one line per (profile, engine, chain, configuration):
#   vm_wasm_depth: <profile> <engine> <chain> <config> <result> <verdict>
# then a summary. Exit 0 when every required check holds, 1 otherwise, 2 on a
# setup error (no wasmtime, build failure).
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
    -h|--help) sed -n '2,40p' "$0"; exit 0 ;;
    *) echo "vm_wasm_depth: unknown flag $arg" >&2; exit 2 ;;
  esac
done

WASMTIME="${WASMTIME:-$(command -v wasmtime || true)}"
if [ -z "$WASMTIME" ] || [ ! -x "$WASMTIME" ]; then
  echo "vm_wasm_depth: wasmtime not found — cannot measure (this is a failure, not a skip)" >&2
  exit 2
fi

REQUIRED_LINEAR=585   # 1.3 x the wasm32 RECURSION_LIMIT (450)
GUARD=450
CHAINS=(plain closure mut with)
ENGINES=(tree vm)
PROFILES=(debug release)
FIX="$ROOT/crates/axon-core/tests/fixtures/vm_depth"
for c in "${CHAINS[@]}"; do
  if [ "$(head -1 "$FIX/$c.ax")" != "let DEPTH = 100" ]; then
    echo "vm_wasm_depth: $FIX/$c.ax must start with 'let DEPTH = 100'" >&2
    exit 2
  fi
done

echo "vm_wasm_depth: building axon-run (wasm32-wasip1, debug + release)…"
cargo build -q -p axon-core --no-default-features --bin axon-run --target wasm32-wasip1 \
  || { echo "vm_wasm_depth: debug build failed" >&2; exit 2; }
cargo build -q --release -p axon-core --no-default-features --bin axon-run --target wasm32-wasip1 \
  || { echo "vm_wasm_depth: release build failed" >&2; exit 2; }
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$ROOT/$TARGET_DIR" ;; esac

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# probe <wasm> <engine> <chain> <config> <depth> <dir> — run one probe.
# Prints the exit code; leaves stdout/stderr in <dir>/out and <dir>/err.
probe() {
  local wasm="$1" engine="$2" chain="$3" config="$4" depth="$5" dir="$6"
  local -a cfg
  case "$config" in
    linear)  cfg=(-W max-wasm-stack=4294967295 --env AXON_MAX_DEPTH=1000000) ;;
    default) cfg=(--env AXON_MAX_DEPTH=1000000) ;;
    guard)   cfg=(-W max-wasm-stack=4294967295) ;;
  esac
  { echo "let DEPTH = $depth"; tail -n +2 "$FIX/$chain.ax"; } >"$dir/p.ax"
  timeout -k 5 300 "$WASMTIME" run "${cfg[@]}" --env "AXON_ENGINE=$engine" \
    --dir "$dir" "$wasm" "$dir/p.ax" </dev/null >"$dir/out" 2>"$dir/err"
  echo $?
}

completes() { # same args as probe
  local rc
  rc="$(probe "$@")"
  [ "$rc" = 0 ] && [ "$(cat "$6/out")" = "$5" ]
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
  local dir="$WORK/$profile.$engine.$chain.$config"
  mkdir -p "$dir"
  if [ "$config" = guard ]; then
    local rc
    rc="$(probe "$wasm" "$engine" "$chain" guard "$GUARD" "$dir")"
    if [ "$rc" = 101 ] && grep -q "recursion limit exceeded ($GUARD)" "$dir/err"; then
      echo "exit=101 ok" >"$dir/result"
    else
      echo "exit=$rc bad: $(head -c 200 "$dir/err" | tr '\n' ' ')" >"$dir/result"
    fi
  else
    bisect "$wasm" "$engine" "$chain" "$config" "$dir" >"$dir/result"
  fi
}
export -f probe completes bisect job
export WASMTIME FIX WORK TARGET_DIR GUARD

JOBS="${VM_DEPTH_JOBS:-$(nproc)}"
for profile in "${PROFILES[@]}"; do
  for engine in "${ENGINES[@]}"; do
    for chain in "${CHAINS[@]}"; do
      for config in linear default guard; do
        printf '%s %s %s %s\n' "$profile" "$engine" "$chain" "$config"
      done
    done
  done
done | xargs -P "$JOBS" -L 1 bash -c 'job "$@"' _

fails=0
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
            if [[ ! "$res" =~ ^[0-9]+$ ]]; then
              verdict="FAIL (no measurement)"; fails=$((fails + 1))
            else
              verdict="reported"
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

if [ "$fails" -ne 0 ]; then
  echo "vm_wasm_depth: FAILED — $fails check(s)"
  exit 1
fi
echo "vm_wasm_depth: PASS — every chain completes $REQUIRED_LINEAR in the linear stack and panics at $GUARD under both engines and profiles"
exit 0
