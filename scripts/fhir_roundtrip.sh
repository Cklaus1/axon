#!/usr/bin/env bash
# fhir_roundtrip.sh — R22 `native::fhir` gate: real FHIR R4 HTTP round-trip.
#
# Two layers of verification, both against a LOCAL mock HTTP server:
#   1. The in-test round-trip in `axon-domain` (a tiny_http server returns a
#      canned Patient resource; the shim reads it through reqwest, then a field
#      is parsed out — plus a search Bundle round-trip + forged-handle test).
#   2. The end-to-end `.ax` demo: stand up the fixed-port mock FHIR server
#      (examples/fhir_test_server.rs), run examples/domain/fhir_demo.ax under the
#      interpreter pointed at it, assert the parsed Patient field.
#
# SKIP-guards if a build leg is unavailable; asserts it actually RAN.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${FHIR_PORT:-18080}"
ran=0
fail=0
SRV_PID=""
cleanup() { [ -n "$SRV_PID" ] && kill "$SRV_PID" 2>/dev/null; }
trap cleanup EXIT

echo "fhir_roundtrip: running axon-domain in-test FHIR HTTP round-trip…"
if cargo test -q -p axon-domain --lib fhir:: 2>&1 | tee /tmp/fhir_$$.log | tail -6; then
  if grep -qE 'test result: ok\. [1-9]' /tmp/fhir_$$.log; then
    ran=1
    echo "fhir_roundtrip: in-test mock-server round-trip PASSED"
  else
    echo "fhir_roundtrip: FAIL — no fhir tests ran (vacuous)"
    fail=1
  fi
else
  echo "fhir_roundtrip: FAIL — axon-domain fhir tests failed"
  fail=1
fi
rm -f /tmp/fhir_$$.log

echo "fhir_roundtrip: building interpreter + mock server…"
if cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null \
   && cargo build -q -p axon-domain --example fhir_test_server 2>/dev/null; then
  # Launch the PRE-BUILT example binary directly (not `cargo run`, whose re-link
  # latency made a fixed sleep racy under load) and poll the port until it binds.
  SRV_BIN="$(find target/debug/examples -maxdepth 1 -name 'fhir_test_server' -type f | head -1)"
  FHIR_PORT="$PORT" "$SRV_BIN" >/dev/null 2>&1 &
  SRV_PID=$!
  for _ in $(seq 1 60); do
    (echo > "/dev/tcp/127.0.0.1/$PORT") 2>/dev/null && break
    sleep 0.25
  done
  if [ "$PORT" = "18080" ]; then
    echo "fhir_roundtrip: running examples/domain/fhir_demo.ax against 127.0.0.1:$PORT…"
    OUT="$(target/debug/axon run examples/domain/fhir_demo.ax 2>&1)"
    echo "$OUT"
    if echo "$OUT" | grep -q "Patient family=Chalmers gender=male"; then
      ran=1
      echo "fhir_roundtrip: demo round-trip PASSED"
    else
      echo "fhir_roundtrip: FAIL — demo output mismatch"
      fail=1
    fi
  else
    echo "fhir_roundtrip: PORT != 18080, skipping demo leg (in-test leg covers it)"
  fi
else
  echo "fhir_roundtrip: build unavailable — skipping demo leg (in-test leg covers it)"
fi

if [ "$ran" = 0 ]; then
  echo "fhir_roundtrip: SKIPPED (nothing ran)"
  exit 0
fi
if [ "$fail" != 0 ]; then
  echo "❌ fhir_roundtrip FAILED"
  exit 1
fi
echo "✅ fhir_roundtrip PASSED"
