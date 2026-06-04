# Tech Spec — R1f: Differential parity fuzzing (auto-find interp↔codegen divergence)

**Status:** 🟡 Slices 1+2 LANDED (2026-06-04). `scripts/fuzz_parity.sh`
implements fork (b): per builtin it generates edge + `FUZZ_N` (default 40)
seeded-random inputs, emits ONE `.ax` exercising all of them, builds once, diffs
interp vs native stdout + exit code. **Slice 2** widened the table to **30
descriptors across 4 domains** (`i64`/`pos`/`f64`/`str`): i64 scalars
(abs/sign/min/max/clamp, i32 variants) + binops (`+`/`-`/`<`/`==`) + bitwise
(and/or/xor/not); `pos`-domain pow/shl/shr/mod (exponent/shift counts kept in
range); the f64 math family (abs/floor/ceil/min/max/f64_to_i64); str-scalar
predicates (len/contains/starts/ends/index over a fixed corpus). The widening
**immediately caught two real float-formatting divergences** (the
`fmt_g`-vs-`snprintf` drift the review flagged — see Findings): `-0.0` → FIXED
(codegen `to_str_f64` normalizes `-0.0 → +0.0`); scientific-notation `%g` →
DOCUMENTED RESIDUAL with its own follow-up. i64 inputs bounded to ±1e9 and f64 to
|x|≤1e5 on purpose — the overflow boundary (verified `abs_i64(i64::MIN)`: interp
exit 101 vs native 134) and the sci-notation range are known divergences with
explicit descriptors deferred to slice 2b. Gated two ways: the
`codegen_fuzz_parity_finds_no_divergence` cli_run test AND `parity_all.sh`
(filename auto-matches `*_parity.sh`). Skips cleanly when LLVM absent.
**Remaining: slice 2b** — `{compare: ExitCode|F64Bits}` descriptors for the
overflow boundary + NaN/inf, and converge the sci-notation residual. Turns I-2
from "fixed cases, audited periodically" into "random inputs, compared on every
change." Prerequisite for collapsing the double-impl (R1f-2).

**Requirement:** R1 (native pipeline) / I-2 (interpreter is the oracle).

---

## The problem (measured 2026-06-04)

- A builtin's *semantics* live in **two** places with no shared definition:
  `interp.rs`'s `call_builtin` arm (the oracle) and either an `axon-rt`
  `extern "C"` fn or hand-written inline LLVM IR in `codegen/`. Drift is always
  representable.
- The I-2 safety net is **22 `scripts/*_parity.sh` harnesses with hand-written
  fixed inputs**. They are now gate-wired (`parity_all.sh`, 2026-06-04,
  `fd1e017`) — but coverage is partial: **~95 of 183 builtins (52%) appear in
  NO parity harness**, and fixed cases miss the *inputs that trigger
  divergence* (overflow edges, empty slices, UTF-8 boundaries, ±0.0, i64::MIN).
- The history is a string of inputs the fixed cases didn't have: #27
  (random_i64 inverted), #36 (SIGFPE on hi==lo), #38/#39 (UTF-8 boundary),
  `arr_sum` saturating-vs-wrapping (a *documented, still-live* overflow-only
  divergence — `codegen/expr.rs:3931`).

A generator that *searches* the input space finds these; a human writing
`check(5, 3)` does not.

---

## Decisive fork

*What is the comparison oracle's execution substrate for the codegen side —
per-input AOT compile, JIT, or a batch harness program?*

- **(a) Per-input AOT compile+link+run.** Faithful to the shipped pipeline, but
  ~700ms/input link → far too slow for thousands of fuzz iterations.
- **(b) One generated program per builtin that loops over many embedded
  inputs**, compiled once, diffed against the interpreter running the same
  inputs. Amortizes the link over N inputs. The generator emits an `.ax` that
  reads a table of literals and prints `f(input)` for each.
- **(c) inkwell JIT** (`ExecutionEngine`) — compile each builtin to a function
  pointer once, call it per input in-process. Fastest, but the JIT path is not
  the AOT path (different relocation/ABI corners — e.g. the wasm str-ABI bugs
  were ABI-specific), so it could miss link-time divergences.

**→ Lean (b).** It reuses the *exact* AOT pipeline the harnesses already trust
(so it catches the ABI-class bugs (c) would miss), while amortizing the link
cost that kills (a). The generator is a Rust `#[test]` that, per builtin: picks a
domain-aware generator, emits one `.ax` exercising K inputs, builds it once,
runs both engines, diffs stdout line-by-line. proptest/quickcheck supplies the
input generators; the failing input *is* the shrink target.

The secondary fork the spec must settle: **per-builtin input-domain
declarations.** Equality can't be asserted blindly — `parse_int("xyz")` panics
in both engines (compare *exit codes*, not stdout); `random_i64` is seeded
(pin `AXON_SEED`); `sqrt(-1.0)` is NaN (compare bit patterns). The spec defines
a small per-builtin descriptor: `{ domain: Gen, compare: Stdout | ExitCode |
F64Bits }`. ~30-40 pure scalar/str/math builtins get descriptors first; the
collection/higher-order/host/ASI builtins are out of scope for v1 (they need a
`Value`-level generator, not flat literals).

---

## Why this is high value

- Attacks the **52% coverage gap** and the **edge-input gap** with one
  generator instead of 95 more hand-written harnesses.
- Makes I-2 **continuously enforced**, closing the "found only by audit" loop.
- Is the **prerequisite that makes R1f-2 (interp calls axon-rt — collapse the
  double-impl, the endgame R1d §5 named) provably behavior-preserving**: you
  can't safely delete one of two implementations until a fuzzer proves they
  agree on random inputs.

## Slices

1. **Harness skeleton + 3 builtins** (`abs_i64`, `min_i64`, `add`): the
   generator-emit-build-diff loop, fork (b), with the `{domain, compare}`
   descriptor for three cases. Gate: runs under `--strict` (links LLVM), skips
   cleanly when LLVM absent (same pattern as the 22 harnesses).
2. **Scalar math + str-scalar descriptors** (~30-40 builtins): widen the
   descriptor table. Expect it to FIND the documented `arr_sum` overflow
   divergence and any latent ones — each becomes a fix or a documented,
   asserted residual.
3. **Wire into `parity_all.sh`** as one more harness so it runs with the suite.

## Risk / cost

- Per-input link is slow; fork (b) amortizes but the test still links LLVM →
  `--strict` only, skip when absent. Bound K and iteration count for gate speed;
  a longer nightly mode can crank them up.
- Domain descriptors are per-builtin authoring cost (modest, ~1 line each).
- Will surface real divergences (that's the point) — each is triaged as fix vs
  documented residual (like the `arr_sum` overflow note), not a blocker.
- Does NOT remove the dual implementation; it makes the dual impl *safe to keep
  and cheap to converge*, and unblocks R1f-2.

## Findings (what the fuzzer caught)

The widening (slice 2) immediately earned its keep — on its first run it found
two real interp↔codegen float-formatting divergences (the `fmt_g`-vs-`snprintf`
drift surface the architecture review flagged):

1. **`-0.0` formatting — FIXED** (slice 2, same commit). `ceil(-0.5)` is `-0.0`
   in IEEE-754; interp's `fmt_g` returns `"0"` (since `-0.0 == 0.0`) but native's
   `snprintf("%.6g", -0.0)` prints `"-0"`. Per I-2 codegen must match the oracle
   → `to_str_f64` now normalizes `-0.0 → +0.0` before snprintf
   (`(n == 0.0) ? 0.0 : n`, an OEQ-select). Verified native==interp.

2. **Scientific-notation format — CONVERGED** (slice 2b). For floats large/small
   enough to render in scientific notation, interp's hand-rolled `fmt_g` and C's
   `%.6g` disagreed on BOTH trailing zeros and exponent style: `1000000.0` →
   interp `1.00000e6` vs native `1e+06`; `0.0000001` → `1.00000e-7` vs `1e-07`.
   **Resolution: make interp the one that moves** — C's `%.6g` is the
   well-specified standard, so `interp.rs::fmt_g`'s exponential branch now trims
   the mantissa's trailing zeros and emits a signed two-digit exponent
   (`1e+06` / `1.23457e+06` / `1e-07` / `1.5e+15`), matching C byte-for-byte. (A
   behavior change to the I-2 oracle, but toward the standard, and verified
   against `printf '%.6g'` across the range; no shipped example pinned the old
   form — `all_examples_parity` stayed 32/32.) The fuzzer's `f64` domain was
   then **widened to span the sci-notation range** (edges at ±1e6/±1e-7/±1e15/
   ±1e-12; random scaled across 10^[-9,9]) so it proves the convergence — 56
   inputs/descriptor, green. Pinned by the `fmt_g_matches_c_printf_six_g` unit
   test. (Surfaced + fixed a pre-existing parallel-test env-var flake in
   `ai_routing` as collateral — the gate's parity stage exposed it.)

## Out of scope (named, not faked)

- Collection/higher-order builtins (`arr_map`, `dict_filter`, …) and ASI/host
  builtins (`ai_complete`, `read_file`) — need a `Value`-level generator and/or
  can't be flat-literal compared. v2.
- **R1f-2 — interp calls axon-rt** (collapse the scalar double-impl to one Rust
  fn). The deep endgame; this fuzzer is its safety net, specced separately when
  R1f-2 is picked up.
