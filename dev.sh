#!/usr/bin/env bash
# Axon dev helper — quick commands for compiler development
set -euo pipefail

CARGO=/root/.cargo/bin/cargo
AXON=./target/debug/axon

cmd=${1:-help}

case "$cmd" in
  build)
    $CARGO build 2>&1
    ;;

  test)
    # Unit + integration tests without codegen — same flags as CI
    $CARGO test --no-default-features -p axon-core 2>&1
    ;;

  test-full)
    # Full test suite with all features (requires LLVM 17)
    $CARGO test 2>&1
    ;;

  check)
    # Fast type-check, no LLVM required — mirrors what CI runs
    $CARGO check --no-default-features -p axon-core 2>&1
    ;;

  check-full)
    # Type-check with all features (requires LLVM 17 installed locally)
    $CARGO check 2>&1
    ;;

  run)
    # Usage: ./dev.sh run examples/hello.ax
    $AXON run "${2:-examples/hello.ax}"
    ;;

  all-examples)
    # Run every runnable example and show pass/fail
    $CARGO build -q 2>/dev/null
    pass=0; fail=0
    for f in examples/*.ax; do
      # skip test-only files (no main or test runner handles them)
      # Axon uses @[test] / @[test(should_fail)] attribute syntax
      if grep -qE "^@\[test" "$f" && ! grep -q "^fn main" "$f"; then
        continue
      fi
      if [[ "$f" == *"should_fail"* ]] || [[ "$f" == *"stdlib_tests"* ]]; then
        continue
      fi
      output=$($AXON run "$f" 2>&1) && {
        echo "✓ $f"
        ((pass++))
      } || {
        echo "✗ $f"
        echo "  $output" | head -3
        ((fail++))
      }
    done
    echo ""
    echo "examples: $pass passed, $fail failed"
    ;;

  all-tests)
    # Run axon test on all test files
    $CARGO build -q 2>/dev/null
    for f in examples/tests.ax examples/stdlib_tests.ax examples/should_fail_test.ax; do
      echo "=== $f ==="
      $AXON test "$f" 2>&1
    done
    ;;

  full)
    # Full CI: build + unit tests + examples + axon tests
    echo "=== cargo build ==="
    $CARGO build 2>&1
    echo ""
    echo "=== cargo test ==="
    $CARGO test 2>&1
    echo ""
    echo "=== examples ==="
    bash "$0" all-examples
    echo ""
    echo "=== axon tests ==="
    bash "$0" all-tests
    ;;

  fmt)
    # Format the axon-core crate in-place
    $CARGO fmt -p axon-core 2>&1
    ;;

  fmt-check)
    # Check formatting without modifying files (mirrors CI)
    $CARGO fmt -p axon-core -- --check 2>&1
    ;;

  clippy)
    # Lint with clippy, no codegen feature (mirrors CI)
    $CARGO clippy --no-default-features -p axon-core -- -D warnings 2>&1
    ;;

  ci)
    # Run exactly what CI runs: check → test → fmt-check → clippy
    echo "=== cargo check (no codegen) ==="
    bash "$0" check
    echo ""
    echo "=== cargo test (no codegen) ==="
    bash "$0" test
    echo ""
    echo "=== cargo fmt --check ==="
    bash "$0" fmt-check
    echo ""
    echo "=== cargo clippy (no codegen) ==="
    bash "$0" clippy
    ;;

  watch)
    # Rebuild on source change (requires cargo-watch: cargo install cargo-watch)
    $CARGO watch -x build 2>&1
    ;;

  ast)
    # Print AST as JSON for a file
    $CARGO build -q 2>/dev/null
    $AXON parse "${2:-examples/hello.ax}"
    ;;

  *)
    echo "Usage: ./dev.sh <command> [args]"
    echo ""
    echo "CI commands (no LLVM required):"
    echo "  ci             check + test + fmt-check + clippy  (same as GitHub Actions)"
    echo "  check          cargo check --no-default-features  (fast, no LLVM)"
    echo "  test           cargo test  --no-default-features  (unit + integration)"
    echo "  fmt-check      cargo fmt --check                  (formatting gate)"
    echo "  clippy         cargo clippy -D warnings            (lint gate)"
    echo ""
    echo "Local dev (requires LLVM 17):"
    echo "  build          cargo build  (full, with codegen)"
    echo "  check-full     cargo check  (full, with codegen)"
    echo "  test-full      cargo test   (full, with codegen)"
    echo "  fmt            cargo fmt    (format in-place)"
    echo "  run [file]     axon run <file>"
    echo "  all-examples   run every example, show pass/fail"
    echo "  all-tests      run axon test on test files"
    echo "  full           build + unit tests + examples + axon tests"
    echo "  ast [file]     print AST as JSON"
    echo "  watch          rebuild on file change (needs cargo-watch)"
    ;;
esac
