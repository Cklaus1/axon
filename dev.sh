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
    $CARGO test 2>&1
    ;;

  check)
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
    echo "Commands:"
    echo "  build          cargo build"
    echo "  test           cargo test (unit tests)"
    echo "  check          cargo check (fast type check)"
    echo "  run [file]     axon run <file>"
    echo "  all-examples   run every example, show pass/fail"
    echo "  all-tests      run axon test on test files"
    echo "  full           build + unit tests + examples + axon tests"
    echo "  ast [file]     print AST as JSON"
    echo "  watch          rebuild on file change"
    ;;
esac
