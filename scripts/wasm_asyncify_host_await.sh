#!/usr/bin/env bash
# wasm_asyncify_host_await.sh — the BROWSER-ASYNC binding (R15 §13 B3): an Axon
# program suspends ACROSS an async JS operation at host_await and resumes, via
# Asyncify. This is the critical-path capability for all interactive browser
# targets (input box / fetch / requestAnimationFrame): the reply arrives from a
# Promise, the wasm module is suspended in between.
#
# Pipeline: build axon-wasm (wasm32-unknown-unknown) → `wasm-opt --asyncify`
# (only env.axon_host_await suspends) → drive under Node with an ASYNC host
# (replies delivered via setTimeout Promises), asserting the round-trip.
#
# Requires: rustup wasm32-unknown-unknown + wasm-opt (binaryen) + node. Skips if absent.
set -u

# AUDIT O004: take the SHARED wasm build lock. Several of these harnesses build
# for wasm32 concurrently under cargo's parallel test threads and clobber each
# other's intermediates, which surfaces as examples silently failing to link.
# Nine harnesses already took this lock; this one did not, so it raced against
# them. No-op without flock.
if command -v flock >/dev/null 2>&1; then exec 9>"${TMPDIR:-/tmp}/axon_wasm_parity.lock" && flock 9; fi


ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

. "$ROOT/scripts/lib/harness_skip.sh"

# ABSENT tools are a legitimate SKIP. A tool that is PRESENT and FAILS is a
# result: the build and wasm-opt steps below used to print "skipping" and exit 0
# on failure, so a broken toolchain or a bad instrumentation pass read as
# "not applicable" — measured, a wasm-opt that exits 1 produced exit 0 and a
# skip line. Failure may never synthesize success.
rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown \
  || harness_skip wasm_asyncify_host_await "wasm32-unknown-unknown target not installed"
command -v wasm-opt >/dev/null 2>&1 || harness_skip wasm_asyncify_host_await "wasm-opt (binaryen) not found"
command -v node >/dev/null 2>&1 || harness_skip wasm_asyncify_host_await "node not found"

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# MEMORY CLASS: HEAVY. Measured under a cgroup ceiling, the four programs below
# peak at ~8.4 GB of V8 memory together (each 6-8 GB), all of it native V8
# structures created while executing Asyncify-instrumented code — the wasm
# linear memory stays at 64 MB throughout. That is after asyncify-ignore-indirect
# cut it from 18-31 GB. It is recorded here so a scheduler (or a person) can
# see this is not an ordinary test: running it beside other memory-heavy
# workloads is what got a strict gate cancelled at 2-3 GiB free.

echo "wasm_asyncify_host_await: building axon-wasm + asyncify…"
if ! berr="$(cargo build -q -p axon-wasm --target wasm32-unknown-unknown --release 2>&1)"; then
  echo "wasm_asyncify_host_await: FAIL — axon-wasm wasm build FAILED (a build error is not a skip):"
  printf '%s\n' "$berr" | tail -8 | sed 's/^/    /'
  exit 1
fi
. scripts/lib/axon_bin.sh
RAW=""; use_built RAW axon_wasm.wasm wasm32-unknown-unknown release  # the build just made
[ -s "$RAW" ] || { echo "wasm_asyncify_host_await: FAIL — build reported success but $RAW is missing or empty"; exit 1; }
ASYNC="$WORK/axon_wasm.async.wasm"
# Modern wasm features rustc emits must be enabled explicitly for binaryen to
# validate the module (needed on every version tested: 108 in R15, 120, 127).
FEATURES="--enable-bulk-memory --enable-sign-ext --enable-mutable-globals --enable-nontrapping-float-to-int --enable-simd --enable-reference-types --enable-multivalue"
# Two independent requirements, both measured, neither optional:
#  * -O2: the UNOPTIMIZED asyncify output runs away — `axon_eval` never returns
#    and linear memory grows until the host is exhausted, even for a program with
#    no `host_await` at all (wasm-opt 120 and 127). Do not drop the -O level.
#  * asyncify-ignore-indirect: without it binaryen instruments 1418 of 1423
#    functions and this harness's four programs peaked at 18-31 GB; with it 6-8 GB.
#    It is only sound when no suspend-reaching function is ever called indirectly,
#    so the soundness condition is CHECKED, not assumed: the guard proves no
#    suspend-reaching function is address-taken.
if ! python3 scripts/asyncify_indirect_guard.py "$RAW" env axon_host_await; then
  echo "wasm_asyncify_host_await: FAIL — asyncify-ignore-indirect is not provably safe for this module"; exit 1
fi
if ! oerr="$(wasm-opt $FEATURES -O2 --asyncify --pass-arg=asyncify-imports@env.axon_host_await \
      --pass-arg=asyncify-ignore-indirect "$RAW" -o "$ASYNC" 2>&1)"; then
  echo "wasm_asyncify_host_await: FAIL — wasm-opt --asyncify FAILED (an instrumentation error is not a skip):"
  printf '%s\n' "$oerr" | tail -8 | sed 's/^/    /'
  exit 1
fi
# The instrumented module must actually be instrumented: without the asyncify
# exports the driver cannot suspend, and a silently-uninstrumented artifact is
# exactly the "stale/corrupt artifact" shape this incident taught us to refuse.
if ! grep -qa asyncify_start_unwind "$ASYNC"; then
  echo "wasm_asyncify_host_await: FAIL — $ASYNC is missing the asyncify exports (not instrumented)"; exit 1
fi
# Asyncify REWIND re-enters every saved wasm frame, so a deep suspend point needs
# more JS stack than node's ~984KB default. Browsers configure stack per worker; for
# the node driver we raise it. (Discovered: guess.ax overflowed at the default but
# works at --stack-size=2000+; this is a host stack-size knob, not an Asyncify limit.)
NODE="node --stack-size=4000"
DRIVER="scripts/wasm_asyncify_driver.js"

# HOST SAFETY (R15 §13 B3). Every driver run below goes through `drive`, which
# bounds it with an OS-enforced memory ceiling AND a wall-clock deadline. This is
# not belt-and-braces: a source-ownership defect in the rewind loop once grew this
# exact driver's memory at ~0.5 GiB/s until the WSL VM died. A runtime bug must
# fail its test, never the machine.
#
# The V8 heap flag is NOT the protection — `--max-old-space-size` does not bound
# Wasm linear memory (measured: a node process capped at 512MB old-space reached
# 24.8 GiB RSS). The cgroup ceiling is what bounds peak memory; the deadline is
# what bounds a program that allocates gracefully. We set a V8 cap too, as a
# secondary layer that fails earlier and more cheaply when the bug IS in the heap.
. "$ROOT/scripts/lib_bounded_run.sh"
ASYNCIFY_MEM_MAX="${ASYNCIFY_MEM_MAX:-1G}"   # per-run memory ceiling
ASYNCIFY_DEADLINE="${ASYNCIFY_DEADLINE:-60}" # per-run wall-clock seconds, in s

# drive <stdout-file> <stderr-file> <ax-file> <replies> -> sets DRIVE_CODE,
# DRIVE_CLASS. DRIVE_CODE is the driver's own exit status for a normal run, or
# 137/124 when a safeguard fired. A safeguard firing is ALWAYS a nonzero status
# and is classified RESOURCE_EXHAUSTED / TIMEOUT — never silently a pass.
drive() {
  local so="$1" se="$2" ax="$3" replies="$4"
  bounded_run "$ASYNCIFY_MEM_MAX" "$ASYNCIFY_DEADLINE" \
    node --stack-size=4000 --max-old-space-size=512 "$DRIVER" "$ASYNC" "$ax" "$replies" \
    >"$so" 2>"$se"
  DRIVE_CODE=$?
  DRIVE_CLASS="$BOUNDED_RUN_CLASS"
  if [ "$DRIVE_CLASS" = "RESOURCE_EXHAUSTED" ] || [ "$DRIVE_CLASS" = "TIMEOUT" ]; then
    echo "wasm_asyncify_host_await: FAIL ($(basename "$ax")): safeguard fired — $DRIVE_CLASS" >&2
    echo "  peak=$(( ${BOUNDED_RUN_PEAK:-0} / 1048576 ))MB ceiling=$ASYNCIFY_MEM_MAX deadline=${ASYNCIFY_DEADLINE}s" >&2
    echo "  This means the driver consumed unbounded memory or never terminated." >&2
    echo "  stderr tail: $(tail -3 "$se" 2>/dev/null | tr '\n' ' ')" >&2
  fi
  return $DRIVE_CODE
}

# (1) Two-turn program: each host_await suspends across an async (Promise) reply.
cat > "$WORK/greet.ax" <<'AX'
fn main() -> i64 {
    let n = host_await("name?")
    let g = host_await("greet?")
    println("{g}, {n}!")
    0
}
AX
drive "$WORK/out" "$WORK/err" "$WORK/greet.ax" $'World\nHi'; code=$?
out="$(cat "$WORK/out")"
reqs="$(grep '^REQ:' "$WORK/err" | sed 's/^REQ://' | tr '\n' '|')"
if [ "$out" != "Hi, World!" ] || [ "$code" != "0" ] || [ "$reqs" != "name?|greet?|" ]; then
  echo "wasm_asyncify_host_await: FAIL (greet): out='$out' code=$code reqs='$reqs'; err: $(cat "$WORK/err")"; exit 1
fi
echo "  OK  greet: async suspend x2 → '$out' (requests: $reqs)"

# (2) MULTI-TURN LOOP: repeated async suspend/resume across iterations of a
# while-loop — the frame-loop / REPL shape. Each iteration suspends for a fresh
# async reply.
cat > "$WORK/loop.ax" <<'AX'
fn main() -> i64 {
    let i = 0
    while i < 3 {
        let r = host_await("> ")
        println("r={r}")
        i = i + 1
    }
    0
}
AX
drive "$WORK/lout" "$WORK/lerr" "$WORK/loop.ax" $'a\nb\nc'; lcode=$?
lout="$(cat "$WORK/lout")"
if [ "$lout" != $'r=a\nr=b\nr=c' ] || [ "$lcode" != "0" ]; then
  echo "wasm_asyncify_host_await: FAIL (loop): code=$lcode out:"; echo "$lout" | sed 's/^/    /'; exit 1
fi
echo "  OK  loop: 3-iteration async while-loop → r=a/r=b/r=c"

# (3) host_await_opt across an async reply → Some(...), and None at EOF.
cat > "$WORK/opt.ax" <<'AX'
fn main() -> i64 {
    match host_await_opt("a?") { Some(x) => println("got {x}")  None => println("none") }
    match host_await_opt("b?") { Some(y) => println("got {y}")  None => println("none") }
    0
}
AX
drive "$WORK/oout" "$WORK/oerr" "$WORK/opt.ax" $'P\nQ'; ocode=$?
oout="$(cat "$WORK/oout")"
if [ "$oout" != $'got P\ngot Q' ] || [ "$ocode" != "0" ]; then
  echo "wasm_asyncify_host_await: FAIL (opt): code=$ocode out:"; echo "$oout" | sed 's/^/    /'; exit 1
fi
echo "  OK  opt: host_await_opt async → Some(P)/Some(Q)"

# (4) DEEPLY-NESTED suspend point: the guessing game (host_await_opt inside
# while→match→match→if). The rewind re-enters every saved wasm frame, so this needs
# the raised JS stack ($NODE) — with it, the deep-nest case round-trips just like the
# rest. guesses 5/9/7 → Higher/Lower/Correct in 3 tries, printed, exit 0.
#
# The try count is read from STDOUT, not the exit status: the demo used to return
# it, which put an answer in the channel where 3 means "@[verify] failed" to
# anything reading the run (governance/EXIT_CODES.md).
if [ -f examples/interactive/guess.ax ]; then
  drive "$WORK/gout" "$WORK/gerr" examples/interactive/guess.ax $'5\n9\n7'; gcode=$?
  gout="$(cat "$WORK/gout")"
  if ! echo "$gout" | grep -q 'Correct — 3 tries!' || [ "$gcode" != "0" ]; then
    echo "wasm_asyncify_host_await: FAIL (guess loop): code=$gcode out:"; echo "$gout" | sed 's/^/    /'; exit 1
  fi
  echo "  OK  guess: deep-nested multi-turn async loop → 3 tries, clean exit"
fi

# (5) The BROWSER must refuse an artifact it cannot vouch for. The page's module is
# generated and gitignored, so it is whatever the last local build left; a stale
# -O0 one exhausts host memory (incident 2026-09-24). Same loader the page imports.
stamp() { printf '{"schema":"axon-asyncify-artifact/1","opt":"%s","sha256":"%s","bytes":%s}\n' \
  "$2" "$(sha256sum "$1" | cut -d' ' -f1)" "$(wc -c < "$1")"; }
stamp "$ASYNC" O2 > "$WORK/good.stamp"
wasm-opt $FEATURES --asyncify --pass-arg=asyncify-imports@env.axon_host_await "$RAW" -o "$WORK/o0.wasm" 2>/dev/null \
  || { echo "wasm_asyncify_host_await: FAIL — could not build the -O0 refusal fixture"; exit 1; }
stamp "$WORK/o0.wasm" O0 > "$WORK/o0.stamp"
for c in "$ASYNC|$WORK/good.stamp|accept|fresh -O2 + its stamp" \
         "$ASYNC|-|refuse|no stamp" \
         "$WORK/o0.wasm|$WORK/good.stamp|refuse|stale -O0 module, current stamp" \
         "$WORK/o0.wasm|$WORK/o0.stamp|refuse|-O0 module, honest stamp"; do
  IFS='|' read -r w st ex label <<<"$c"
  bounded_run "$ASYNCIFY_MEM_MAX" "$ASYNCIFY_DEADLINE" node scripts/browser_artifact_guard.mjs "$w" "$st" "$ex" >"$WORK/g" 2>&1 \
    || { echo "wasm_asyncify_host_await: FAIL (artifact guard: $label): $(cat "$WORK/g") [$BOUNDED_RUN_CLASS]"; exit 1; }
done
echo "  OK  artifact guard: page runs a fresh stamped -O2 build; refuses unstamped / stale / -O0"

echo "wasm_asyncify_host_await: PASS — host_await suspends across async JS work in the browser (Asyncify)"
exit 0
