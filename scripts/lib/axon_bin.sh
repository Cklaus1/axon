# axon_bin.sh — which binary a harness executes. Sourced, never run.
#
# A harness execs EXACTLY ONE of:
#
#   * the binary its CALLER names, in an environment variable (`named_bin`);
#   * a binary it BUILT ITSELF, at the path cargo built it to (`built_bin`,
#     called after the harness's own `cargo build`).
#
# Never a binary that merely sits under `target/` or on PATH. That file was
# built from SOME tree at SOME time: the workspace's `target/debug/axon` is
# whatever the last build in that directory left (a mutated tree's, in a
# paired-disable cell), and an `axon` on PATH is anyone's. C9 round 4
# (EQUIVALENCE, blocker 6): clock_parity.sh ran a planted
# `$REPO/target/debug/axon` and reported on it, and the R17 IR/QEMU harnesses
# ran a planted `axon` from PATH and PASSED.
#
# The rule is enforced at this one primitive; `crates/axon-core/tests/
# harness_binaries.rs` fails the build if any script picks a binary another way.
#
# A harness whose caller named its binaries builds NOTHING (AX-51): under
# `cargo test` the caller is `cli_run.rs`, which names the binaries cargo built
# for that test run (`AXON`, `AXON_RUN`), with the run's own features. A nested
# `cargo build` there measured a different compiler from the one under test,
# and concurrent harnesses rebuilt the shared `target/debug/axon` with other
# feature sets while siblings executed it (an `axon-run` gone mid-run, exit
# 127). So a building harness builds only when its variable is unset:
#
#     if [ -n "${AXON:-}" ]; then named_bin AXON <harness>; else
#       cargo build -q -p axon-core --bin axon || <skip>; use_built AXON axon; fi

# cargo_target_dir — the absolute directory cargo builds into from $PWD, as
# CARGO resolves it (CARGO_TARGET_DIR, then `build.target-dir` in any config
# cargo reads, then the workspace's target/). Not `${CARGO_TARGET_DIR:-target}`:
# that guess is wrong whenever a config file names the directory.
cargo_target_dir() {
  local d
  d="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
       | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)"
  if [ -z "$d" ]; then
    echo "axon_bin: cannot resolve the directory cargo builds into from $PWD" >&2
    return 1
  fi
  printf '%s\n' "$d"
}

# interp_target_dir — the directory a harness builds its private
# interpreter-only `axon` into: `interp-only/` under whatever cargo_target_dir
# resolves. A relative `CARGO_TARGET_DIR="target/interp-only"` resolved
# against the harness's `cd "$ROOT"` and overrode a caller's outside target
# dir, so a lane building elsewhere got a second full axon-core build inside
# its worktree. Stable path so cargo can reuse. Use it as
#     ITD="$(interp_target_dir)" || exit 2
#     … env CARGO_TARGET_DIR="$ITD" cargo build … ; NATIVE="$ITD/debug/axon"
# (the shape crates/axon-core/tests/harness_binaries.rs accepts).
interp_target_dir() {
  local t
  t="$(cargo_target_dir)" || return 1
  printf '%s/interp-only\n' "$t"
}

# built_bin <name> [<triple>] [<profile>] — print the path cargo built <name>
# at. <profile> is `debug` (default) or `release`; <triple> is empty for the
# host. Prefer `use_built`, which cannot lose a failure in a `$(…)`.
built_bin() {
  local name="$1" triple="${2:-}" profile="${3:-debug}" t
  t="$(cargo_target_dir)" || return 1
  if [ -n "$triple" ]; then
    printf '%s/%s/%s/%s\n' "$t" "$triple" "$profile" "$name"
  else
    printf '%s/%s/%s\n' "$t" "$profile" "$name"
  fi
}

# use_built <VAR> <name> [<triple>] [<profile>] — for a harness that has JUST
# run its own `cargo build` of <name>: unless the caller named a binary in
# $VAR, set $VAR to the path cargo built <name> at. Runs in the caller's shell
# (never in a `$(…)`), so a failure to resolve the path exits the harness (2).
use_built() {
  local var="$1" p
  if [ -n "${!var:-}" ]; then
    return 0
  fi
  p="$(built_bin "$2" "${3:-}" "${4:-}")" || {
    echo "axon_bin: FAIL — cannot tell where cargo built $2" >&2
    exit 2
  }
  printf -v "$var" '%s' "$p"
}

# named_bin <VAR> <harness> — require that the caller named the binary in
# $VAR. A harness that builds nothing has no other binary it may run: with
# $VAR unset or empty it REFUSES (exit 2) rather than guess, and a named path
# that is not an executable file is refused too. Runs in the caller's shell, so
# the refusal ends the harness; on success $VAR is made absolute. A refusal is a failure, never a skip: a skip
# says the environment cannot run the check; here the caller did not say which
# build to judge.
named_bin() {
  local var="$1" harness="$2" v
  v="${!var:-}"
  if [ -z "$v" ]; then
    echo "$harness: REFUSED — \$$var names no binary. This harness builds nothing and runs only" >&2
    echo "$harness:   the binary its caller names (it never falls back to target/ or PATH)." >&2
    echo "$harness: FAIL — no binary named" >&2
    exit 2
  fi
  if [ ! -f "$v" ] || [ ! -x "$v" ]; then
    echo "$harness: FAIL — \$$var=$v is not an executable file" >&2
    exit 2
  fi
  # Absolute, so a harness that cd's elsewhere runs the same file. A bare name
  # is a path relative to here, never a PATH lookup.
  printf -v "$var" '%s/%s' "$(cd "$(dirname "$v")" && pwd)" "$(basename "$v")"
}

# require_codegen_axon <harness> — ask $AXON (already chosen by named_bin or
# use_built) whether it can emit native code: a trivial `axon build` must
# succeed. A caller can legitimately name an interpreter-only binary (`cargo
# test --no-default-features` builds one), and interp<->native parity is not
# measurable with it: a SKIP (fatal under AXON_HARNESS_STRICT=1). No linker
# etc. is a SKIP too (native_build_unavailable); any other failure is a broken
# compiler: FAIL, never a skip (AX-51).
require_codegen_axon() {
  local d err
  declare -F harness_skip >/dev/null \
    || . "$(dirname "${BASH_SOURCE[0]}")/harness_skip.sh"
  d="$(mktemp -d)"
  printf 'fn main() -> i64 { 0 }\n' > "$d/probe.ax"
  if err="$("$AXON" build "$d/probe.ax" -o "$d/probe.bin" --no-cache 2>&1)"; then
    rm -rf "$d"
    return 0
  fi
  rm -rf "$d"
  case "$err" in
    *'requires building axon with the `codegen` feature'*)
      harness_skip "$1" \
        "\$AXON cannot produce native binaries, so interp<->native parity is not measurable with it." \
        "\$AXON ($AXON) is an interpreter-only build — native codegen is not under test"
      ;;
  esac
  if native_build_unavailable "$err"; then
    harness_skip "$1" "$(printf '%s' "$err" | head -2)" \
      "native codegen unavailable in this environment"
  fi
  echo "$1: FAIL — \$AXON ($AXON) cannot build a trivial program (a build error is not a skip):"
  printf '%s\n' "$err" | head -5 | sed 's/^/        /'
  exit 1
}
