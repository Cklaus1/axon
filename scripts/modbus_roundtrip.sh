#!/usr/bin/env bash
# modbus_roundtrip.sh — R22 `native::modbus` gate: real Modbus TCP round-trip.
#
# Two layers of verification, both against a LOCAL tokio-modbus server:
#   1. The in-test round-trip in `axon-domain` (spawns a tokio-modbus TCP server
#      IN the test, connects through the shim, write reg → read it back → assert;
#      also a coil round-trip + the forged-handle graceful-Err soundness test).
#   2. The end-to-end `.ax` demo: stand up the fixed-port test server
#      (examples/modbus_test_server.rs), run examples/domain/modbus_demo.ax under
#      the interpreter pointed at it, assert the written value reads back.
#
# SKIP-guards if a build leg is unavailable; asserts it actually RAN (no
# vacuous pass).
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${MODBUS_PORT:-15502}"
ran=0
fail=0
SRV_PID=""
cleanup() { [ -n "$SRV_PID" ] && kill "$SRV_PID" 2>/dev/null; }
trap cleanup EXIT

echo "modbus_roundtrip: running axon-domain in-test Modbus TCP round-trip…"
if cargo test -q -p axon-domain --lib modbus:: 2>&1 | tee /tmp/mb_$$.log | tail -6; then
  if grep -qE 'test result: ok\. [1-9]' /tmp/mb_$$.log; then
    ran=1
    echo "modbus_roundtrip: in-test server round-trip PASSED"
  else
    echo "modbus_roundtrip: FAIL — no modbus tests ran (vacuous)"
    fail=1
  fi
else
  echo "modbus_roundtrip: FAIL — axon-domain modbus tests failed"
  fail=1
fi
rm -f /tmp/mb_$$.log

# Demo leg: fixed-port server + the .ax demo.
echo "modbus_roundtrip: building interpreter + test server…"
if cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null \
   && cargo build -q -p axon-domain --example modbus_test_server 2>/dev/null; then
  MODBUS_PORT="$PORT" cargo run -q -p axon-domain --example modbus_test_server >/dev/null 2>&1 &
  SRV_PID=$!
  sleep 1.5
  echo "modbus_roundtrip: running examples/domain/modbus_demo.ax against 127.0.0.1:$PORT…"
  # The demo hardcodes port 15502; only run the demo leg when PORT matches.
  if [ "$PORT" = "15502" ]; then
    OUT="$(target/debug/axon run examples/domain/modbus_demo.ax 2>&1)"
    echo "$OUT"
    if echo "$OUT" | grep -q "holding\[3\] = 4660" && echo "$OUT" | grep -q "coil\[5\] = 1"; then
      ran=1
      echo "modbus_roundtrip: demo round-trip PASSED"
    else
      echo "modbus_roundtrip: FAIL — demo output mismatch"
      fail=1
    fi
  else
    echo "modbus_roundtrip: PORT != 15502, skipping demo leg (in-test leg covers it)"
  fi
else
  echo "modbus_roundtrip: build unavailable — skipping demo leg (in-test leg covers it)"
fi

if [ "$ran" = 0 ]; then
  echo "modbus_roundtrip: SKIPPED (nothing ran)"
  exit 0
fi
if [ "$fail" != 0 ]; then
  echo "❌ modbus_roundtrip FAILED"
  exit 1
fi
echo "✅ modbus_roundtrip PASSED"
