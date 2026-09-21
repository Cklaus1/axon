#!/usr/bin/env bash
# gate.sh — the single, atomic build gate for axon-core.
#
# Every code change (mine or a subagent's) must pass THIS exact gate before it
# is committed, so "green" means the same thing everywhere.
#
# WHAT IT RUNS: read the `── gate: …` banners as it executes. They are the
# authoritative list; this header is not. It said four stages while the script
# ran eight — omitting the VISION.md focus check, the serde-json feature builds,
# the runtime-crate clippy and (added 2026-09-17) the clippy-coverage check, plus
# the parity suite under --strict. A duplicated list in a comment drifts from the
# code beside it, which is the defect this gate exists to catch elsewhere; so the
# list is not duplicated any more.
#
# In rough order: VISION.md focus · cargo fmt · the full test suite
# (--no-default-features) · the native codegen build · serde-json feature builds
# · clippy (lib, then every runtime crate, then the coverage check that every
# workspace crate is gated or excused) · and under --strict, --all-targets clippy
# and the whole parity suite (I-2).
#
# Determinism: AXON_SEED + AXON_AI_MOCK are pinned so seeded-RNG / AI-call tests
# never flake. Speed: mold linker + sccache rustc cache are used IF installed
# (purely local — never committed to .cargo/config.toml, so contributors/CI
# without them still build fine). The wasm stack-size config in
# .cargo/config.toml is untouched (this only sets the host target's linker).
#
# Usage:
#   scripts/gate.sh            # standard gate (lib clippy)
#   scripts/gate.sh --strict   # also run --all-targets clippy
#   scripts/gate.sh --nextest  # use cargo-nextest as the test runner
#
# Exit 0 iff all stages pass.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

STRICT=0
USE_NEXTEST=0
for arg in "$@"; do
  case "$arg" in
    --strict) STRICT=1 ;;
    --nextest) USE_NEXTEST=1 ;;
    *) echo "gate: unknown flag $arg" >&2; exit 2 ;;
  esac
done

# Deterministic test environment.
export AXON_SEED="${AXON_SEED:-42}"
export AXON_AI_MOCK="${AXON_AI_MOCK:-1}"

# Optional local speedups — only if present, never required.
if command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
fi
if command -v mold >/dev/null 2>&1; then
  # Host-target-scoped so the committed wasm linker config is untouched.
  export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS:--C link-arg=-fuse-ld=mold}"
fi

fail() { echo ""; echo "❌ gate FAILED at: $1"; exit 1; }

# Run a harness quietly, but SHOW ITS OUTPUT WHEN IT FAILS.
#
# 21 stages were written `./harness.sh >/dev/null 2>&1 || fail "..."`. The
# quiet part is deliberate and worth keeping — a gate log carrying 21
# harnesses' full chatter is unreadable. Discarding the output on FAILURE is
# not: gate12 failed at the eBPF verifier harness and left no diagnostic at
# all, so the only way to learn anything was to re-run the harness by hand,
# where it then passed six times out of six. A failure you cannot see is a
# failure you cannot fix, and a transient one you cannot see is indistinguishable
# from a real one.
run_quiet() {
  local label="$1"; shift
  local out; out="$(mktemp)"
  local code=0
  # Capture the command's status DIRECTLY. Writing `if "$@"; then ... fi` and
  # then reading `$?` reports the status of the `if` CONSTRUCT — which is 0
  # when the condition is false and there is no else — so a harness that
  # exited 3 was reported as "(exit 0)". A diagnostic that misstates the
  # result is the defect it exists to prevent.
  "$@" >"$out" 2>&1 || code=$?
  if [ "$code" -eq 0 ]; then
    rm -f "$out"
    return 0
  fi
  echo "── output of failing stage: $label (exit $code) ──"
  tail -n 40 "$out"
  echo "── end output ──"
  rm -f "$out"
  fail "$label"
}

# SELF-CHECK. run_quiet's entire value is in a branch that almost never runs,
# so nothing would notice it rotting. A subshell, because the failure path
# calls fail(), which exits.
_rq=$( ( run_quiet "selftest" bash -c 'echo DIAGNOSTIC_MARKER; exit 3' ) 2>&1 )
# Both substrings must be present; ORDER IS NOT ASSUMED. The first version of
# this pattern required DIAGNOSTIC_MARKER before "exit 3", but run_quiet prints
# the header (carrying the exit code) BEFORE the captured output. The check
# therefore never matched, fell to the default branch, and printed its own
# diagnostic into the gate log on every healthy run — a self-check that fails
# open is worse than none.
case "$_rq" in
  *"exit 3"*) case "$_rq" in *DIAGNOSTIC_MARKER*) ;; *) _rq_bad=1 ;; esac ;;
  *) _rq_bad=1 ;;
esac
case "${_rq_bad:-0}" in
  0) ;;
  *) echo "$_rq"; fail "run_quiet self-check: a failing stage must print its output and its real exit code" ;;
esac
_rq=$( ( run_quiet "selftest" bash -c 'echo SHOULD_NOT_APPEAR; exit 0' ) 2>&1 )
[ -z "$_rq" ] || { echo "$_rq"; fail "run_quiet self-check: a passing stage must stay quiet"; }
unset _rq

# The harness-skip log (O006b) is APPEND-only across runs, so the coverage notice
# at the end of this script would otherwise report skips from previous runs as if
# they had happened now. Truncate it so the notice describes THIS run only.
SKIPLOG="target/harness-skips.log"
mkdir -p target && : > "$SKIPLOG"

# Fast doc-focus check first (pure text over VISION.md, no build) — keeps the
# north-star doc short/legible the same way the *_parity.sh harnesses keep the
# compiler honest. Cheap enough to run on every gate, codegen or not.
echo "── gate: VISION.md focus ──────────────────────────────────────────"
./scripts/vision_focus.sh || fail "VISION.md focus"

# Artifact admission. Generated build output must never become repository
# source by accident. It twice did, and nothing noticed either time: cac9cdf
# committed two 44MB `axon build` probe binaries (crates/axon-core/{cg,vapp}),
# and the Cortex experiment commits carried 8 __pycache__/*.pyc files. Both
# arrived the same way — a broad `git add` folding unrelated untracked files
# into an otherwise coherent commit — and both survived review, because the
# untracked -> tracked transition is the one state change nothing asserted was
# intended. Cheap (~1s over the index), so it runs on every gate, not --strict.
./scripts/artifact_admission_gate.sh >/dev/null || fail "artifact admission (generated output is tracked as source)"

# The vendored Cortex v0.15 package: integrity in BOTH directions plus the
# honesty invariant. That package proposes 247 product gates, every one marked
# NOT_RUN, and it is scrupulous about saying so — but the NOT_RUN field does not
# travel when an ID is copied into a status table, and this repo already had 7
# of 37 cited gates invoked by nothing. This fails if a gate's result moves off
# NOT_RUN without a registry row naming a script that gate.sh actually invokes.
# 0.15s, no cargo, so it runs on every gate rather than only --strict.
./scripts/cortex_package_gate.sh >/dev/null || fail "cortex v0.15 package (integrity / honesty invariant)"

# Formatting. This is deliberately BEFORE the build: it is pure text, costs
# under a second, and a fmt failure needs no compiler to be true. It is also
# --all, not -p axon-core, because per-crate scoping is exactly how 37 files of
# drift accumulated unseen in the crates nobody was checking (2980206). Adding a
# crate to the workspace? --all picks it up with no edit here.
echo "── gate: cargo fmt --all --check ──────────────────────────────────"
cargo fmt --all -- --check || fail "cargo fmt --all --check (run: cargo fmt --all)"

# Long-running jobs must own their result and their cleanup. This gate exists
# because they did not: gate runs launched with `nohup ... &` reported the
# LAUNCHER's exit 0 as the gate's verdict four separate times, and nine leaked
# `axon test` processes plus dozens of orphaned CPU spinners ran for 15+ hours
# because nothing owned them. Cheap (~10s) and placed early on purpose.
# A package nested inside another package's directory is dead code that still
# ships. `crates/axon-ledger/axon-ledger/` was a PRE-RBAC duplicate of the
# ledger crate inside itself — the shape a contributor or agent revives later,
# carrying the bug just fixed in the real one. Cheap, so it runs early.
echo "── gate: no nested package roots ─────────────────────────────────"
run_quiet "no nested package roots" bash scripts/no_nested_crates.sh

echo "── gate: managed-run supervision (result ownership + cancellation scope) ──"
bash scripts/managed_run_gate.sh || fail "managed-run supervision"

# The Cortex policy boundary. Its proofs existed but nothing ran them: every
# `cargo test` in this gate targeted axon-core, so `axon-cortex`'s conformance
# gates and the adapter's protocol tests were compiled by the clippy line above
# and executed nowhere. A test that is linted but never run is evidence of
# nothing, and this is the boundary MiCode consults before every effectful
# action — the single place where "the check exists" and "the check runs" most
# need to be the same statement.
#
# Found when an omitted --grant-snapshot was discovered to silently disable the
# staleness refusal: the suite that should have caught it was not part of any
# gate, and five of its six boundary cases were passing vacuously besides.
echo "── gate: cortex policy boundary tests ─────────────────────────────"
cargo test -p axon-cortex -p cortex-policy-adapter -p axon-reflex \
  || fail "cortex policy boundary tests"

# The same crate again with `ai` on, because that feature gates the only
# model-backed generator — the one production path where a model contributes to
# a repair episode. A default-feature run neither compiles nor lints it, so the
# module would rot exactly the way lsp.rs did below: present, plausible, and
# verified by nothing. Clippy as well as test, since the default clippy sweep
# further down also passes no features.
cargo clippy -p axon-cortex --all-targets --features ai -- -D warnings \
  || fail "cortex ai-generator lint"
cargo test -p axon-cortex --features ai \
  || fail "cortex ai-generator tests"

# The benchmark prose against the data it cites, in BOTH directions. Twice in
# one session a figure outlived its measurement — the binary's `--help` was
# still advertising a top-1 accuracy the benchmark README had explicitly
# withdrawn. Both times it was found by reading, which is not a method.
#
# Wired here because a checker nothing invokes is the same defect one layer
# up: it existed, it worked, and it would have run only when someone
# remembered it.
python3 crates/axon-cortex/benchmarks/check_docs.py \
  || fail "benchmark docs cite figures the data does not produce"

# The analyser produces every headline number the real-model experiment
# reports, and nothing in the Rust suite touches it. It shipped with a
# load-bearing defect: proposal ownership inferred from ORDER, which is wrong
# whenever a candidate is selected but never reaches the generator. Measured
# with the guard removed, it reported "ranking waste is 100% of inference
# spend" on a fixture where half the proposals went to the true target.
python3 crates/axon-cortex/benchmarks/selftest_analyse.py \
  || fail "analyser attributes proposals to candidates unsoundly"

# The completeness manifest regenerates AXON-COMPLETENESS.md and refuses a row
# claiming a production proof or a mutation it cannot cite. Wired here because
# a manifest nobody runs is a table of assertions, which is the state it was
# written to replace.
python3 scripts/completeness.py \
  || fail "a completeness claim is not backed by evidence"

# A gate function that produced no readable verdict must not be scored as a
# pass. Landed RED and deliberately unwired; wired here now that it is green,
# because a gate nobody runs is the defect one layer up.
bash scripts/gate_verdict_is_read.sh \
  || fail "a safety gate with no readable verdict is being scored as passed"


echo "── gate: native codegen build ─────────────────────────────────────"
cargo build -p axon-core || fail "native build"

# AUDIT T15 (finding P5-34). Nothing ever built the serde-json feature — not
# gate.sh, not CI — so lsp.rs rotted silently as new Type variants landed and
# `cargo check --features serde-json` failed outright. That means `axon lsp` and
# `axon parse --json`, both advertised in CLAUDE.md under "Phase 4 ✅ Complete",
# could not be compiled at all. An advertised command with no build gate is a
# command that will eventually stop existing without anyone noticing.
echo "── gate: serde-json feature builds (axon lsp / axon parse --json) ─"
cargo check --no-default-features --features serde-json -p axon-core \
  || fail "serde-json feature check (axon lsp / axon parse --json)"

echo "── gate: clippy (lib, -D warnings) ────────────────────────────────"
cargo clippy --no-default-features -p axon-core -- -D warnings || fail "lib clippy"

# BUG_HUNT #35: the runtime crates (axon-rt/axon-ai/axon-surface) used to be
# invisible to the clippy gate (scoped to -p axon-core), hiding ~80 lints. They
# are now clippy-clean under --all-targets (the intentional C-ABI ptr-deref
# seams carry a documented crate-level allow), so the gate enforces them. They
# have no codegen feature, so this is cheap and needs no --no-default-features.
# axon-vm/axon-attest joined 2026-07-19 (found while sizing R33.S2a): same class
# of gap, same fix -- 3 pre-existing findings in axon-vm (a too-many-arguments
# and a manual-range-contains in main.rs, a dead-code AxonManifest struct) fixed
# with #[allow(..)]/a mechanical rewrite, no behavior change; axon-attest was
# already clean.
# axon-ledger joined 2026-07-31 (R18 governance audit): same coverage-gap class
# — the crate landed as a workspace member with 63 tests but was never lint-
# gated; 2 mechanical clippy fixes (a ?-operator rewrite, a redundant &) made
# it clean, no behavior change.
echo "── gate: clippy runtime crates (-D warnings) ─────────────────────"
# O-RLM-04: this list is an ALLOWLIST, not the workspace, so a crate absent from
# it is simply unlinted — and a green gate reads as coverage. Six crates were
# outside it (axon-intent, axon-os, axon-web, axon-audit, axon-certcheck,
# axon-signal); axon-os alone carried ~11 warnings including a dead function.
# Third recorded sighting of this class, so the fix is the list AND this note.
# Adding a new crate to the workspace? Add it here in the same commit.
# axon-gfx, axon-guest-init and axon-wasm joined 2026-09-17 (FOURTH sighting of
# this class, which is why the check below now exists rather than another note).
# axon-gfx and axon-guest-init were already clean. axon-wasm needed one fix: its
# generated `tests/oracle.rs` is `include!`d by the browser test and read only on
# wasm32, but living under tests/ cargo ALSO builds it as a host integration test
# with no test fns, where the static is genuinely unused — so the crate could not
# be lint-gated at all.
cargo clippy -p axon-rt -p axon-ai -p axon-surface -p axon-gfx -p axon-gfx-mock \
  -p axon-domain -p axon-vm -p axon-attest -p axon-ledger -p axon-intent \
  -p axon-os -p axon-web -p axon-audit -p axon-certcheck -p axon-signal \
  -p axon-guest-init -p axon-wasm -p axon-cortex -p cortex-policy-adapter \
  -p axon-reflex \
  --all-targets -- -D warnings \
  || fail "runtime-crate clippy"

# COVERAGE CHECK — the note above has been written three times and the list
# drifted anyway, because a comment cannot fail a build. Every workspace member
# must be either lint-gated above (or as axon-core separately) or named here with
# a reason. Adding a crate without doing one of those now turns the gate RED.
echo "── gate: clippy coverage (every crate gated or excused) ──────────"
# axon-guest-kernel: freestanding no_std. On the host target clippy fails with
# "unwinding panics are not supported without std" and a duplicate `panic_impl`
# lang item — artifacts of the wrong target, not lints. It is built and checked
# by scripts/build-guest-image.sh with -Z build-std and its own target spec.
CLIPPY_EXCUSED="axon-guest-kernel"
# tr: `sort -u` is newline-separated, and the `case` glob below matches on
# SPACES. Without it nothing ever matches — the check fails closed (every crate
# "uncovered") rather than open, but it would still have been wrong.
#
# The pattern was `-p axon-[a-z-]+`, which could only ever see crates named
# `axon-*`. `cortex-policy-adapter` — the process MiCode consults for every
# effectful action — was therefore reported uncovered no matter what was added
# to the clippy line above: the detector for drift had the same blind spot as
# the thing it detects. This is the FIFTH sighting of the class, and the first
# where the check itself was the cause, so the pattern now matches any crate
# name rather than one vendor prefix.
_gated="$(grep -oE '\-p [a-z][a-z0-9_-]*' "$0" | awk '{print $2}' | sort -u | tr '\n' ' ')"
_missing=""
for _c in $(ls crates); do
  case " $_gated $CLIPPY_EXCUSED " in
    *" $_c "*) ;;
    *) _missing="$_missing $_c" ;;
  esac
done
if [ -n "$_missing" ]; then
  echo "gate: these crates are neither clippy-gated nor excused:$_missing"
  echo "gate: add them to the clippy line above, or to CLIPPY_EXCUSED with a reason."
  fail "clippy coverage"
fi
echo "  ✓ all $(ls crates | wc -l) workspace crates are lint-gated or excused"

# ORDERING, twice revised, and both revisions were driven by measurement.
#
# This stage takes the better part of an hour, so it was moved BELOW the cheap
# lint/boundary checks: two gate runs had spent ~80 minutes and then failed on
# a one-line lint, invalidating everything the expensive stage established.
#
# But moving it below the NATIVE CODEGEN BUILD broke the parity stages, because
# `cargo test -p axon-core --no-default-features` rebuilds `target/debug/axon`
# WITHOUT codegen, overwriting the codegen-capable binary the later harnesses
# need. Measured: `wasm_browser_io_parity: FAIL — $AXON has no codegen backend
# (E0907)`. The harness refused to call that a skip, which is why it was
# visible at all.
#
# So it sits here: after the cheap checks, before the codegen build that
# restores the binary the parity stages consume. Nothing is weakened — every
# stage still runs and still blocks; only the order changed.
echo "── gate: tests (--no-default-features) ─────────────────────────────"
if [ "$USE_NEXTEST" = 1 ] && command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run -p axon-core --no-default-features || fail "tests (nextest)"
else
  cargo test -p axon-core --no-default-features || fail "tests"
fi

# REBUILD the codegen binary. The stage above rebuilt `target/debug/axon`
# WITHOUT codegen, and every parity harness below consumes that path. Measured
# when this rebuild was absent: `wasm_browser_io_parity: FAIL — $AXON has no
# codegen backend (E0907)`. The harness refused to call it a skip, which is the
# only reason it was visible rather than a silent 14-harness gap.
echo "── gate: restore the codegen binary after the no-default test stage ──"
cargo build -q -p axon-core --bin axon || fail "codegen rebuild after tests"



if [ "$STRICT" = 1 ]; then
  echo "── gate: clippy (--all-targets, -D warnings) ─────────────────────"
  cargo clippy --no-default-features -p axon-core --all-targets -- -D warnings || fail "all-targets clippy"

  # BUG_HUNT #35 follow-on: the codegen feature (axon-core WITH default features)
  # was never clippy-gated — the lib clippy above uses --no-default-features, so
  # the entire IR-emitter path (codegen/*.rs) was invisible. It is now clean
  # (~86 mechanical .into()/let-_/&Path lints fixed, verified native==interp via
  # the parity harnesses), so --strict enforces it going forward. Only under
  # --strict because it links LLVM (slower than the interp-only passes).
  echo "── gate: clippy codegen feature (--all-targets, -D warnings) ─────"
  cargo clippy -p axon-core --all-targets -- -D warnings || fail "codegen-feature clippy"

  # Coverage gap closed: the test stage above runs --no-default-features, so any
  # `#[cfg(feature = "codegen")]` integration test (e.g. the end-to-end runtime
  # `@[verify]` enforcement test) was NEVER executed by the gate — a regression
  # there stayed green. --strict now also runs the codegen-gated integration
  # tests. Scoped to the integration_fixtures target (the only home of codegen-
  # gated tests today); the rest already run under the interp pass / via the
  # CARGO_BIN_EXE harnesses. Under --strict only because it links LLVM.
  echo "── gate: codegen-gated integration tests ────────────────────────"
  cargo test -p axon-core --test integration_fixtures || fail "codegen integration tests"

  # R1d Slice-3 drift kill-gate (governance/specs/R1d-single-source-builtins.md):
  # the builtin_externs drift tests live behind #[cfg(feature = "codegen")], so
  # the standard-gate `cargo test --no-default-features` at the top compiles them
  # out (0 run) and the integration_fixtures line above skips --lib entirely — a
  # BUILTIN_EXTERNS/STR_OUT_EXTERNS table-drift regression passed both gates
  # green (same class as the clippy allowlist gap above). Run them explicitly
  # with default features (codegen on). Cheap: same build as the two stages
  # above, 5 tests, ~0s.
  echo "── gate: builtin-externs drift tests (R1d slice 3) ──────────────"
  cargo test -p axon-core --lib codegen::builtin_externs || fail "builtin-externs drift tests"

  # The two-engine invariant (I-2): native codegen + AOT-wasm must match the
  # interpreter oracle byte-for-byte. ~22 scripts/*_parity.sh harnesses assert
  # this, but were historically run ad hoc — which is how the silent-divergence
  # bugs (#27/#36/#38/#39/parse_*_or) reached main. parity_all.sh runs the whole
  # suite; a real divergence fails the gate, toolchain-absent harnesses skip
  # cleanly. Under --strict only (links LLVM + may run wasmtime; ~2 min).
  echo "── gate: parity suite (interp ↔ codegen / AOT-wasm) ─────────────"
  ./scripts/parity_all.sh --quiet || fail "parity suite"

  # THE BROADEST SWEEP IN THE REPO, and it ran nowhere. `all_examples_parity`
  # compares EVERY example under interp and native rather than a curated list —
  # it is what caught the `llvm_sizeof`-reports-8-bytes Result-payload memory
  # corruption, a bug no per-construct harness had found.
  #
  # Placed HERE, immediately after the parity suite, because placement decides
  # whether it runs at all: this harness SKIPS (exit 0) when `target/debug/axon`
  # cannot codegen, and the smt stage near the end of this file builds
  # --no-default-features, leaving exactly that binary behind. Wired at the
  # bottom it would have skipped on every run while reporting success.
  #
  # So the exit code is not trusted on its own — the PASS line carries a COUNT,
  # and a skip is made loud rather than silent.
  echo "── gate: all-examples parity (previously unwired) ───────────────"
  if ape=$(./scripts/all_examples_parity.sh 2>&1); then
    case "$ape" in
      *"PASS — "*examples*)
        echo "  OK $(printf '%s' "$ape" | grep -o 'PASS — .*' | head -1)" ;;
      *skipping*)
        printf '%s\n' "$ape" | tail -3 | sed 's/^/  /'
        fail "all_examples_parity SKIPPED — it needs a codegen binary, and the \
stage order means it should have had one here" ;;
      *) printf '%s\n' "$ape" | tail -5
         fail "all_examples_parity exited 0 without its PASS line" ;;
    esac
  else
    printf '%s\n' "$ape" | tail -10; fail "all-examples parity"
  fi

  # The per-requirement ACCEPTANCE gates. `r22_acceptance_gate.sh` and
  # `r44_acceptance_gate.sh` assert the §0 checks their specs declare — the
  # intent/approve gateway and the accumulating session respectively. Both are
  # maintained (one was repaired earlier today when a diagnostic string moved)
  # and both pass, and until now NOTHING ran them: they were referenced only by
  # their own specs and governance docs, by no script, test or CI job.
  #
  # That is the same shape as the CI job named "native/interp parity" that had
  # never run the parity suite, and as the 14 fixtures reachable from no test.
  # A gate nobody invokes reports nothing, including when it would have failed.
  echo "── gate: per-requirement acceptance gates (R22, R44) ────────────"
  run_quiet "R22 acceptance gate" ./scripts/r22_acceptance_gate.sh
  run_quiet "R44 acceptance gate" ./scripts/r44_acceptance_gate.sh

  # Two more harnesses that NOTHING invoked — found by the coverage-metric
  # audit, which measured execution instead of counting mentions. Between them
  # they are the entire execution story for 8 bpf/TEE builtins; those builtins'
  # only other coverage was `check_fixture`, which type-checks and never runs.
  #
  # Both pass on this host today: ebpf_verify.sh gets its object ACCEPTED by
  # the in-kernel verifier, and tee_sim_run.sh verifies the baseline and the
  # type rule. Both skip honestly when their capability is genuinely absent
  # (no llvm-objdump, not root, no gramine) rather than reporting success.
  # A harness nobody invokes reports nothing, including when it would fail.
  echo "── gate: previously-unwired harnesses (eBPF verifier, TEE simulation) ──"
  run_quiet "eBPF verifier harness" ./scripts/ebpf_verify.sh
  run_quiet "TEE simulation harness" ./scripts/tee_sim_run.sh

  # THE THREE DOMAIN ROUND-TRIPS. `governance/specs/R22-domain-modules.md`
  # ticks all three as done; nothing ran them. They are the only end-to-end
  # evidence that axon-domain's codecs work through the real CLI — the unit
  # tests exercise the Rust side, these exercise the language boundary.
  #
  # Measured before wiring, because "orphaned" and "would pass" are different
  # claims: all three PASS on this host, in seconds.
  echo "── gate: domain round-trips (FIX, FHIR, Modbus — previously unwired) ──"
  for dh in fix_codec fhir_roundtrip modbus_roundtrip; do
    run_quiet "$dh round-trip" ./scripts/$dh.sh
  done

  # Three MORE acceptance gates nothing invoked, found by re-running the same
  # "which scripts does nothing call?" probe after wiring the first two. Each
  # asserts the §0 checks its spec declares, each passes today, and each was
  # running nowhere. R34's in particular verifies a stamp -> verify -> tamper ->
  # BROKEN chain through the real CLI; that evidence was being produced and
  # discarded on every run that never happened.
  echo "── gate: acceptance gates R33, R34, R39 (previously unwired) ────"
  run_quiet "R33 acceptance gate" ./scripts/r33_acceptance_gate.sh
  run_quiet "R34 acceptance gate" ./scripts/r34_acceptance_gate.sh
  run_quiet "R39 Slice 1 gate" ./scripts/r39_slice1_gate.sh

  # R23 is the one REQUIREMENTS.md cites as the evidence for "Landed 100%",
  # and nothing invoked it. It passes — verified by running it — and it is the
  # only caller of `cargo test -p axon-certcheck --features smt`, so R23's own
  # A5 check (certificate emission byte-identical) ran nowhere either: the
  # gate's smt stage is -p axon-core only.
  run_quiet "R23 acceptance gate" ./scripts/r23_acceptance_gate.sh

  # R26/R27/R28/R29 — cited by REQUIREMENTS.md as the evidence those landed,
  # and reachable from no execution root. They ARE run by axon_safety_gate.sh,
  # which nothing invokes, so the whole subtree hung off nothing.
  #
  # Wired DIRECTLY rather than by wiring their parent. The parent re-runs BUILD
  # and the full unit suite and a flagship demo — all of which this gate has
  # already done by this point — for about 25 minutes of duplication. These four
  # standalone cost 23 SECONDS total (measured: 1s, 7s, 11s, 4s). Minimising the
  # number of WIRINGS and minimising the WORK are different objectives, and here
  # they disagree; this picks the work.
  # THE GUEST-KERNEL SYSCALL GATE'S ONLY LIVE PROOF, previously invoked by
  # nothing — not gate.sh, not CI, not another script.
  #
  # It is a real two-case differential through Firecracker: policy withholds FS
  # -> the openat is DENIED and the guest halts with exit 8; policy grants FS ->
  # the same openat is PERMITTED, with no false violation. The negative case is
  # what makes it worth running, and it is the shape most of this repo's
  # stronger gates share.
  #
  # Measured on this host, where firecracker, /dev/kvm and the freestanding
  # kernel artifact are all present: PASS in 20s, both directions.
  #
  # Output is NOT discarded. This harness exits 0 when its prerequisites are
  # absent, so `>/dev/null 2>&1 || fail` would make a skip byte-indistinguishable
  # from a pass — which is the defect that left 16 harnesses hanging off nothing
  # in the first place. A skip must be legible to whoever reads this log.
  if out=$(./scripts/kernel_enforce_test.sh 2>&1); then
    case "$out" in
      *"PASS — the syscall gate denies/permits by policy"*)
        echo "  OK kernel_enforce_test: syscall gate enforced live, both directions" ;;
      *skipping*)
        echo "  SKIP kernel_enforce_test — prerequisites absent on this host:"
        printf '%s\n' "$out" | sed 's/^/       /' | head -3 ;;
      *)
        echo "$out"; fail "kernel_enforce_test exited 0 without its PASS line" ;;
    esac
  else
    echo "$out"; fail "kernel_enforce_test (guest-kernel syscall enforcement)"
  fi

  run_quiet "R26 acceptance gate" ./scripts/r26_acceptance_gate.sh
  run_quiet "R27 acceptance gate" ./scripts/r27_acceptance_gate.sh
  run_quiet "R28 acceptance gate" ./scripts/r28_acceptance_gate.sh
  run_quiet "R29 acceptance gate" ./scripts/r29_acceptance_gate.sh

  # R31 — its own header has said, since it was written:
  #   "Wire into gate.sh --strict once R28/R29 reach stable artifact paths."
  # R28 and R29 are wired directly above as of this session, so the precondition
  # the author named is now met. The gate does not merely check that its ten
  # normative test names EXIST; it parses a real suite run and requires each to
  # report `... ok`, because a name-grep cannot distinguish a passing test from
  # an #[ignore]d one or from a name that survives only in a comment. Measured:
  # exit 0, "ALL CHECKS PASSED".
  run_quiet "R31 acceptance gate" ./scripts/r31_acceptance_gate.sh

  # R39 slices 3, 4 and 5. REQUIREMENTS.md cites all three as the evidence R39
  # landed, all three pass, and nothing invoked any of them.
  #
  # 41s total for 29 assertions (measured: 35s/11, 4s/8, 2s/10), which is what
  # makes them continuous rather than conditional. Verified TWICE in a worktree
  # that has never run these — once by a subagent and once here — because a gate
  # verified only in a long-lived checkout can pass on ignored leftover state
  # and fail in every fresh clone. That is not hypothetical: r27 and r29 were
  # wired earlier in this session on exactly that mistake.
  # Slice 2 is wired ahead of 3-5 because it carries r39_slice2_validate.sh:
  # that script's only caller is this one, so wiring this makes BOTH reachable.
  # Its check B is the interesting half — a scratch copy of governance/specs/
  # with four deliberately injected bugs, which both validators must find. It
  # proves the port detects bugs rather than proving two validators agree an
  # already-clean tree is clean.
  # R32 — the formal-corrigibility proof artifacts (TLA+ model, TLC config, Coq
  # proof). Measured here: PASS=20 FAIL=0 SKIPPED=0, "all checks ran, none
  # skipped", including coqc_compile — the Coq proof genuinely compiles.
  #
  # Its completeness is HOST-DEPENDENT and its exit code does not say so. With
  # TLC/coqc absent it reports each missing check as SKIPPED and still exits 0.
  # The prose is honest ("SKIPPED is not a substitute for PASS: it is an honest
  # report that the check did not run at all") but gate.sh reads exit codes, not
  # prose — the same gap that made axon_safety_gate's verdict unreadable to a
  # caller. So this is wired to RUN, not to certify: its evidence class is
  # toolchain-conditional, and EVIDENCE_CLASSES.md types it rather than this
  # line pretending it is continuous.
  # R25 — an Axon program running AS a Zephyr application on ARM Cortex-M under
  # QEMU. This could not be wired before today: the gate was UNPASSABLE, because
  # synthesize_freestanding_trap emitted x86 port I/O ("outb", constraints
  # {dx},{al}) for every --freestanding target, so the thumbv7m build died at
  # codegen with `couldn't allocate input reg for constraint '{dx}'`. The trap is
  # now target-aware (x86 keeps outb+hlt; ARM gets bkpt #<marker> + wfi).
  #
  # Verified end to end here, not inferred: exit 0, "PASS: Axon ran on
  # Zephyr/Cortex-M under QEMU — banner + computed 23 + 42", with every skip
  # guard passing (west, cmake, ninja, qemu-system-arm, ZEPHYR_BASE, the
  # arm-zephyr-eabi SDK) rather than the gate skipping past them.
  #
  # On a host without the Zephyr SDK it skips honestly, naming the missing tool.
  # That is the external_hardware class: excused from being EFFECTIVE without
  # the toolchain, never from being wired — unwired it would not run on the host
  # that HAS the hardware either.
  run_quiet "R25 Zephyr/Cortex-M gate" ./scripts/zephyr_qemu_gate.sh
  run_quiet "R32 acceptance gate" ./scripts/r32_acceptance_gate.sh
  run_quiet "R39 Slice 2 gate" ./scripts/r39_slice2_gate.sh
  run_quiet "R39 Slice 3 gate" ./scripts/r39_slice3_gate.sh
  run_quiet "R39 Slice 4 gate" ./scripts/r39_slice4_gate.sh
  run_quiet "R39 Slice 5 gate" ./scripts/r39_slice5_gate.sh

  # A reported diagnostic location must EXIST in the file the diagnostic names.
  # Written RED and left unwired; it stayed red for as long as `Span` was
  # (start, end) with no file identity. Wiring it now that it is green is the
  # point — an unwired gate is a gate nothing can fail, and this one guards a
  # class (one file's offsets rendered against another file's SourceMap) that
  # returns every time a new path merges programs.
  # Output preserved, not discarded: this gate exits 2 for "examined NO located
  # diagnostics" — a broken probe — and 1 for a real failure. `>/dev/null` makes
  # those identical in the log, and a probe that measured nothing is the failure
  # mode this whole class of check exists to expose.
  if dlg=$(./scripts/diagnostic_location_gate.sh 2>&1); then
    echo "  OK $(printf '%s' "$dlg" | tail -1)"
  else
    dlg_rc=$?
    printf '%s\n' "$dlg" | tail -4 | sed 's/^/     /'
    [ "$dlg_rc" = 2 ] \
      && fail "diagnostic location gate examined NO diagnostics (broken probe, not a clean tree)" \
      || fail "diagnostic location (a reported line must exist in the file named)"
  fi

  # Coverage gap closed (the [[coverage-vacuous-pass-guard]] class): the entire
  # `smt` feature — Phase 5 §4's Z3-backed @[verify] + refinement-return prover
  # (smt.rs, 18 unit tests) — is behind `#[cfg(feature = "smt")]` and so was
  # NEVER built or tested by any gate stage. A regression in the prover stayed
  # green. --strict now clippy-gates and tests it. The feature links the system
  # libz3 dynamically; when libz3 isn't installed we SKIP cleanly (like the wasm
  # harnesses) rather than fail, so the gate still works on a Z3-less box.
  if echo 'int main(){return 0;}' | cc -xc - -lz3 -o /dev/null 2>/dev/null; then
    echo "── gate: clippy + tests (smt feature, Z3) ───────────────────────"
    cargo clippy --no-default-features -p axon-core --features smt --all-targets -- -D warnings \
      || fail "smt-feature clippy"
    cargo test --no-default-features -p axon-core --features smt --lib smt \
      || fail "smt unit tests"
  else
    echo "── gate: smt feature SKIPPED (libz3 not found; install to enable) ─"
  fi
fi

echo ""

# AUDIT T36 (finding GATE-03). The default gate's test stage runs
# --no-default-features, so every codegen-dependent parity wrapper in cli_run.rs
# reports `ok` while asserting nothing ("codegen unavailable — parity skipped"),
# and parity_all.sh runs under --strict only. A non-strict run therefore proves
# NOTHING about invariant I-2 while printing "✅ gate PASSED" — the same vacuous-
# pass shape this repo has now hit three times.
#
# Deliberately NOT silently changed: promoting parity_all.sh into the default
# path costs 7m49s on this box (measured 2026-08-04: 44 passed / 5 skipped of
# 49). That is a real decision about gate latency, not one to make as a side
# effect of a bug fix. What this does is refuse to let the vacuity be silent.
# ($SKIPLOG is truncated at the top of this script so this reflects THIS run.)
if [ "$STRICT" != 1 ]; then
  echo "── gate: coverage notice ───────────────────────────────────────────"
  if [ -s "$SKIPLOG" ]; then
    n_skips=$(sort -u "$SKIPLOG" | wc -l | tr -d ' ')
    echo "  $n_skips harness(es) SKIPPED — these gates measured nothing:"
    sort -u "$SKIPLOG" | sed 's/^/    · /'
  fi
  echo "  This run did NOT verify interp↔codegen / AOT-wasm parity (invariant I-2)."
  echo "  The test stage is --no-default-features, so the codegen parity wrappers"
  echo "  cannot assert. To actually check I-2:"
  echo "      ./scripts/gate.sh --strict      # full gate incl. the parity suite"
  echo "      ./scripts/parity_all.sh         # parity suite alone (~8 min)"
  echo "      AXON_HARNESS_STRICT=1 ...       # make any skipped harness FATAL"
fi

echo ""
echo "✅ gate PASSED"
