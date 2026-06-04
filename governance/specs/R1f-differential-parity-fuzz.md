# Tech Spec — R1f: Differential parity fuzzing (auto-find interp↔codegen divergence)

**Status:** 📋 Draft (2026-06-04) — fork-first. Turns I-2 from "22 hand-written
fixed-case harnesses, audited periodically" into "random typed inputs, compared
on every change." Prerequisite for safely collapsing the double-impl (R1f-2).

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

## Out of scope (named, not faked)

- Collection/higher-order builtins (`arr_map`, `dict_filter`, …) and ASI/host
  builtins (`ai_complete`, `read_file`) — need a `Value`-level generator and/or
  can't be flat-literal compared. v2.
- **R1f-2 — interp calls axon-rt** (collapse the scalar double-impl to one Rust
  fn). The deep endgame; this fuzzer is its safety net, specced separately when
  R1f-2 is picked up.
