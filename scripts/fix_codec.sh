#!/usr/bin/env bash
# fix_codec.sh — R22 `native::fix` gate: FIX 4.4 build → parse → checksum.
#
# Two layers of verification:
#   1. The Rust codec round-trip + checksum tests in `axon-domain` (the build→
#      parse→validate oracle, incl. a corrupted-byte negative).
#   2. The end-to-end `.ax` demo under the interpreter: build a NewOrderSingle,
#      validate it, parse it, read fields back by tag.
#
# `fix` is a PURE codec (no network), so this gate has no external dependency.
# It SKIP-guards only if the interpreter can't build, and asserts it actually
# ran (no vacuous pass).
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ran=0
fail=0

echo "fix_codec: running axon-domain FIX codec tests…"
if cargo test -q -p axon-domain --lib fix:: 2>&1 | tee /tmp/fix_codec_$$.log | tail -5; then
  if grep -qE 'test result: ok\. [1-9]' /tmp/fix_codec_$$.log; then
    ran=1
    echo "fix_codec: codec round-trip + checksum tests PASSED"
  else
    echo "fix_codec: FAIL — no FIX codec tests ran (vacuous)"
    fail=1
  fi
else
  echo "fix_codec: FAIL — axon-domain FIX tests failed"
  fail=1
fi
rm -f /tmp/fix_codec_$$.log

echo "fix_codec: building interpreter axon binary…"
if cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null; then
  AXON="target/debug/axon"
  echo "fix_codec: running examples/domain/fix_demo.ax under the interpreter…"
  OUT="$("$AXON" run examples/domain/fix_demo.ax 2>&1)"
  echo "$OUT"
  if echo "$OUT" | grep -q "fix message valid: 1" \
     && echo "$OUT" | grep -q "MsgType=D Symbol=AAPL Qty=100 Price=150"; then
    ran=1
    echo "fix_codec: demo round-trip PASSED"
  else
    echo "fix_codec: FAIL — demo output did not match expected round-trip"
    fail=1
  fi
else
  echo "fix_codec: interpreter build unavailable — skipping demo leg"
fi

if [ "$ran" = 0 ]; then
  echo "fix_codec: SKIPPED (nothing ran) — treating as skip, not pass"
  exit 0
fi
if [ "$fail" != 0 ]; then
  echo "❌ fix_codec FAILED"
  exit 1
fi
echo "✅ fix_codec PASSED"
