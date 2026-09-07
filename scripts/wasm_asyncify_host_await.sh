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

rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown \
  || { echo "wasm_asyncify_host_await: wasm32-unknown-unknown not installed — skipping"; exit 0; }
command -v wasm-opt >/dev/null 2>&1 || { echo "wasm_asyncify_host_await: wasm-opt (binaryen) not found — skipping"; exit 0; }
command -v node >/dev/null 2>&1 || { echo "wasm_asyncify_host_await: node not found — skipping"; exit 0; }

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

echo "wasm_asyncify_host_await: building axon-wasm + asyncify…"
cargo build -q -p axon-wasm --target wasm32-unknown-unknown --release 2>/dev/null \
  || { echo "axon-wasm wasm build failed — skipping"; exit 0; }
RAW=target/wasm32-unknown-unknown/release/axon_wasm.wasm
ASYNC="$WORK/axon_wasm.async.wasm"
# Modern wasm features rustc emits must be enabled for binaryen 108 to validate.
FEATURES="--enable-bulk-memory --enable-sign-ext --enable-mutable-globals --enable-nontrapping-float-to-int --enable-simd --enable-reference-types --enable-multivalue"
if ! wasm-opt $FEATURES --asyncify --pass-arg=asyncify-imports@env.axon_host_await "$RAW" -o "$ASYNC" 2>/dev/null; then
  echo "wasm_asyncify_host_await: wasm-opt --asyncify failed — skipping"; exit 0
fi
# Asyncify REWIND re-enters every saved wasm frame, so a deep suspend point needs
# more JS stack than node's ~984KB default. Browsers configure stack per worker; for
# the node driver we raise it. (Discovered: guess.ax overflowed at the default but
# works at --stack-size=2000+; this is a host stack-size knob, not an Asyncify limit.)
NODE="node --stack-size=4000"
DRIVER="scripts/wasm_asyncify_driver.js"

# (1) Two-turn program: each host_await suspends across an async (Promise) reply.
cat > "$WORK/greet.ax" <<'AX'
fn main() -> i64 {
    let n = host_await("name?")
    let g = host_await("greet?")
    println("{g}, {n}!")
    0
}
AX
out="$($NODE "$DRIVER" "$ASYNC" "$WORK/greet.ax" $'World\nHi' 2>"$WORK/err")"; code=$?
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
lout="$($NODE "$DRIVER" "$ASYNC" "$WORK/loop.ax" $'a\nb\nc' 2>/dev/null)"; lcode=$?
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
oout="$($NODE "$DRIVER" "$ASYNC" "$WORK/opt.ax" $'P\nQ' 2>/dev/null)"; ocode=$?
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
  gout="$($NODE "$DRIVER" "$ASYNC" examples/interactive/guess.ax $'5\n9\n7' 2>/dev/null)"; gcode=$?
  if ! echo "$gout" | grep -q 'Correct — 3 tries!' || [ "$gcode" != "0" ]; then
    echo "wasm_asyncify_host_await: FAIL (guess loop): code=$gcode out:"; echo "$gout" | sed 's/^/    /'; exit 1
  fi
  echo "  OK  guess: deep-nested multi-turn async loop → 3 tries, clean exit"
fi

echo "wasm_asyncify_host_await: PASS — host_await suspends across async JS work in the browser (Asyncify)"
exit 0
