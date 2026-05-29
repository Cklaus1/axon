#!/usr/bin/env bash
# Axon dev helper — quick commands for compiler development.
# Interpreter-first: `build`/`run`/examples use the codegen-free `axon` binary
# (no LLVM). The native codegen build is slow (see BUILD_DIAGNOSIS.md) and
# isolated under `build-native`.
set -euo pipefail

CARGO=/root/.cargo/bin/cargo
AXON=./target/debug/axon

# Build the codegen-free interpreter CLI (fast, no LLVM). Used by run/examples.
build_interp() { $CARGO build -q -p axon-core --no-default-features --bin axon; }

cmd=${1:-help}

case "$cmd" in
  build)
    build_interp
    echo "built $AXON (interpreter CLI, no codegen)"
    ;;

  build-native)
    echo "WARNING: native codegen build is pathologically slow and may not finish."
    echo "         See BUILD_DIAGNOSIS.md. Use 'build' (interpreter) for dev."
    $CARGO build -p axon-core 2>&1
    ;;

  test)
    # Unit + integration tests without codegen — same flags as CI
    $CARGO test --no-default-features -p axon-core 2>&1
    ;;

  test-full)
    $CARGO test 2>&1
    ;;

  check)
    $CARGO check --no-default-features -p axon-core 2>&1
    ;;

  check-full)
    $CARGO check 2>&1
    ;;

  run)
    # Usage: ./dev.sh run examples/hello.ax
    build_interp
    $AXON run "${2:-examples/hello.ax}"
    ;;

  goal)
    # Usage: ./dev.sh goal examples/goals/optimize-goal.md  (mock LLM by default)
    build_interp
    AXON_AI_MOCK=1 $AXON goal "${2:-examples/goals/optimize-goal.md}"
    ;;

  all-examples)
    # Run every runnable top-level example, show pass/fail
    build_interp
    pass=0; fail=0
    for f in examples/*.ax; do
      if grep -qE "^@\[test" "$f" && ! grep -q "^fn main" "$f"; then continue; fi
      case "$f" in *should_fail*|*stdlib_tests*) continue;; esac
      if $AXON run "$f" >/dev/null 2>&1; then
        echo "✓ $f"; pass=$((pass+1))
      else
        echo "✗ $f"; fail=$((fail+1))
      fi
    done
    echo ""
    echo "examples: $pass passed, $fail failed"
    ;;

  stdlib)
    # Run the Tier-1 stdlib module tests
    build_interp
    for f in examples/stdlib/*.ax; do
      echo "=== $f ==="
      $AXON test "$f" 2>&1 | tail -1 || true
    done
    ;;

  goals)
    # Run the key-free goal demos and show each outcome
    build_interp
    echo "goal demos (exit 0=deploy, 101=verify-block, 1=redteam-block):"
    for g in optimize verified redteam compose; do
      rc=0
      $AXON goal "examples/goals/$g-goal.md" >/dev/null 2>&1 || rc=$?
      echo "  $g-goal -> exit $rc"
    done
    ;;

  all-tests)
    build_interp
    for f in examples/tests.ax examples/stdlib_tests.ax examples/should_fail_test.ax examples/stdlib/*.ax; do
      echo "=== $f ==="
      $AXON test "$f" 2>&1 | tail -1 || true
    done
    ;;

  full|verify)
    # Interpreter-first quality gate: build + tests + examples + stdlib + goals
    echo "=== build interpreter ==="; build_interp
    echo "=== cargo test (no codegen) ==="; $CARGO test --no-default-features -p axon-core 2>&1 | grep -E "test result" || true
    echo "=== examples ==="; bash "$0" all-examples
    echo "=== stdlib ==="; bash "$0" stdlib
    echo "=== goals ==="; bash "$0" goals
    ;;

  fmt)
    $CARGO fmt -p axon-core 2>&1
    ;;

  fmt-check)
    $CARGO fmt -p axon-core -- --check 2>&1
    ;;

  clippy)
    $CARGO clippy --no-default-features -p axon-core -- -D warnings 2>&1
    ;;

  ci)
    # Run exactly what CI runs: check → test → fmt-check → clippy
    echo "=== cargo check (no codegen) ==="; bash "$0" check
    echo ""; echo "=== cargo test (no codegen) ==="; bash "$0" test
    echo ""; echo "=== cargo fmt --check ==="; bash "$0" fmt-check
    echo ""; echo "=== cargo clippy (no codegen) ==="; bash "$0" clippy
    ;;

  watch)
    $CARGO watch -x "build -p axon-core --no-default-features --bin axon" 2>&1
    ;;

  ast)
    build_interp
    $AXON parse "${2:-examples/hello.ax}"
    ;;

  *)
    echo "Usage: ./dev.sh <command> [args]"
    echo ""
    echo "CI (no LLVM):"
    echo "  ci             check + test + fmt-check + clippy  (same as GitHub Actions)"
    echo "  check / test   cargo check / test --no-default-features"
    echo "  fmt-check / clippy   formatting + lint gates"
    echo ""
    echo "Interpreter dev (no LLVM):"
    echo "  build          build the codegen-free axon CLI (fast)"
    echo "  run [file]     axon run <file>"
    echo "  goal [file.md] axon goal <file> (mock LLM)"
    echo "  all-examples   run every top-level example, show pass/fail"
    echo "  stdlib         run the Tier-1 stdlib module tests"
    echo "  goals          run the key-free goal demos, show outcomes"
    echo "  all-tests      axon test on all test/stdlib files"
    echo "  verify | full  build + tests + examples + stdlib + goals"
    echo "  ast [file]     print AST as JSON"
    echo ""
    echo "Native codegen (SLOW — see BUILD_DIAGNOSIS.md):"
    echo "  build-native / check-full / test-full"
    ;;
esac
