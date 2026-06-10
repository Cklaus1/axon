#!/usr/bin/env bash
# wasm_browser_host_await.sh — host_await works in the BROWSER substrate (R15 §13
# B1): a suspending program, run by the axon-wasm interpreter under Node, gets its
# replies from an imported `axon_host_await` (JS), with requests handed to the host
# (not stdout). Proves the full request-out / reply-in round-trip — the synchronous
# precursor to the Asyncify async binding (B3).
#
# Requires: rustup target wasm32-unknown-unknown + node. Skips (exit 0) if absent.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
  echo "wasm_browser_host_await: wasm32-unknown-unknown not installed — skipping"; exit 0
fi
command -v node >/dev/null 2>&1 || { echo "wasm_browser_host_await: node not found — skipping"; exit 0; }

echo "wasm_browser_host_await: building axon-wasm (wasm32-unknown-unknown)…"
cargo build -q -p axon-wasm --target wasm32-unknown-unknown --release 2>/dev/null \
  || { echo "axon-wasm wasm build failed — skipping"; exit 0; }
WASM="target/wasm32-unknown-unknown/release/axon_wasm.wasm"
DRIVER="scripts/wasm_browser_host_driver.js"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# A two-turn host_await program: request the name + a greeting word, then combine.
# The requests ("name?" / "greet?") go to the HOST; only the println is stdout.
cat > "$WORK/greet.ax" <<'AX'
fn main() -> i64 {
    let n = host_await("name?")
    let g = host_await("greet?")
    println("{g}, {n}!")
    0
}
AX

out="$(node "$DRIVER" "$WASM" "$WORK/greet.ax" $'World\nHi' 2>"$WORK/err")"; code=$?
reqs="$(grep '^REQ:' "$WORK/err" | sed 's/^REQ://' | tr '\n' '|')"

if [ "$out" != "Hi, World!" ]; then
  echo "wasm_browser_host_await: FAIL — program output '$out' (expected 'Hi, World!'); err: $(cat "$WORK/err")"; exit 1
fi
if [ "$code" != "0" ]; then
  echo "wasm_browser_host_await: FAIL — exit $code (expected 0)"; exit 1
fi
if [ "$reqs" != "name?|greet?|" ]; then
  echo "wasm_browser_host_await: FAIL — host saw requests '$reqs' (expected 'name?|greet?|')"; exit 1
fi

# EOF: with NO replies, host_await returns "" (the non-opt form collapses EOF).
cat > "$WORK/eof.ax" <<'AX'
fn main() -> i64 {
    let r = host_await("q?")
    println("got[{r}]")
    0
}
AX
out2="$(node "$DRIVER" "$WASM" "$WORK/eof.ax" '' 2>/dev/null)"; code2=$?
if [ "$out2" != "got[]" ] || [ "$code2" != "0" ]; then
  echo "wasm_browser_host_await: FAIL — EOF case: out '$out2' code $code2 (expected 'got[]' / 0)"; exit 1
fi

echo "wasm_browser_host_await: PASS — host_await round-trips through the browser JS-import host"
echo "  greet: out='$out' requests=[$reqs]"
exit 0
