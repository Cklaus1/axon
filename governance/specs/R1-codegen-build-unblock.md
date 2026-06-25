# Tech Spec — R1 Unblock: Move Inline-IR Builtins into the `axon-rt` Staticlib

> **✅ RESOLVED (2026-06-25, founder decision).** R1's goal — a native build that finishes and
> produces a working compiler at interp parity — is **met**, by a different (cheaper) route than
> this spec's planned inline-IR→`axon-rt` migration. `cargo build -p axon-core --bin axon`
> (default `codegen`) finishes in **~1.5–2.6s** (verified by touching `codegen/mod.rs` +
> `codegen/builtins.rs` to force a full codegen recompile), ~725 MB peak RSS, producing a working
> 144 MB native compiler; `axon build hello.ax` → LLVM → runnable ELF. Native↔interp parity
> confirmed: `scripts/all_examples_parity.sh` is 34/38 byte-identical with **0 divergences** (4
> non-builds are deliberate E0910 refusals of interp-only net builtins), every non-wasm
> `scripts/*_parity.sh` passes, and `fuzz_parity.sh` / the wasm parity suite are green. The
> `serde-json`-drop (`BUILD_RESOLVED.md`) + non-generic `build_wrappers.rs` closed the build-time
> gap. **The inline-IR→`axon-rt` migration described below is DE-SCOPED as a build-time fix** (no
> longer needed for R1). Any residual value it carries — single-source builtin authoring,
> IR-volume reduction, wasm extern-freedom — lives on in **R1b/R1c/R1d** as ordinary refactors,
> not as a build-unblock emergency. Everything below is retained as the historical Draft.

**Status:** ✅ RESOLVED (2026-06-25) — *build unblocked by the serde-drop + build_wrappers route; the migration below is de-scoped (folded into R1b/R1c/R1d). Original Draft preserved below.*
**Requirement:** `../REQUIREMENTS.md` R1 — *Native compiler pipeline (parse→typecheck→borrow→LLVM→native).* Currently 40%, ⚠️ Partial: *"LLVM build does not finish (BUILD_DIAGNOSIS.md); native binary not routinely producible."*
**The blocker, in one line (from `BUILD_DIAGNOSIS.md` §5):** rustc's serial, single-threaded MIR→LLVM-IR lowering of ~158 **hand-emitted inkwell IR function bodies** in `codegen/builtins.rs` is superlinear in IR volume, lands one-giant-function-per-CGU, and never finishes.

> **Why this spec exists and why it is NOT a loop tick.** Every cheap fix is already tried *and falsified* in `BUILD_DIAGNOSIS.md` / `CODEGEN_WRAPPER_PROTOTYPE.md`: more codegen-units, opt-level=0, splitting `declare_builtins` into helpers, the trait "IR shim" (0% win), cranelift (also stalled), nightly parallel-frontend (1/8 threads). The `#[inline(never)]` wrappers from the prototype **were shipped** (951 call sites in `builtins.rs`) and bought a real ~1.7–3× constant factor — **but the build still doesn't finish**, exactly as that prototype's own caveats predicted ("constant factor, not an asymptote fix"). This spec is the one structural lever left that attacks **IR volume itself**, not a constant factor. It requires human-driven, measure-on-a-finishing-build execution (the feedback signal takes hours), so it is drafted here as a checklist, not handed to the autonomous loop.

---

## 1. Motivation

The native build's entire cost is IR *generation* for `codegen::builtins` (`BUILD_DIAGNOSIS.md` §2: `cargo check` with inkwell present finishes in 4.5 s; `cargo build` is unbounded; `cargo llvm-lines`, which only *generates* IR, also times out). The IR comes from **158 inline function bodies** — every `append_basic_block` in `builtins.rs` is a hand-emitted LLVM body, 951 `build_wrappers::w_*` calls building them, spread evenly (~120 per 500 lines) across all 3,961 lines. There is no hot spot to split out; the *aggregate volume* is the tax, which is precisely why function-splitting failed.

**The architecture already contains the fix, proven on 38 builtins.** `axon-rt` is a real Rust staticlib (`libaxon_rt.a`), **already built and linked into every native binary** (`codegen/link.rs:121` `build_axon_rt`). Its 38 `extern "C"` functions (`__axon_sqrt`, `__axon_read_file`, `__axon_spawn`, channels, provenance…) cost **~zero IR** in `axon-core` — codegen merely *declares* them (`add_function(name, None)`) and the linker resolves them against the precompiled, fast-to-build, parallel, cached `axon-rt`.

The 158 inline builtins are the same *kind* of thing — pure or near-pure compute (`abs_i64`, `str_contains`, `str_to_upper`, `to_str`, the string family) — but they pay the full IR-generation tax because they are emitted as LLVM IR by hand instead of written as Rust and linked. **Moving them from inline-IR to `axon-rt` Rust functions deletes their IR-generation cost** from the unbounded serial phase and relocates it into `axon-rt`'s ordinary, parallelizable, incrementally-cached Rust compile. This is an *asymptote* fix (IR volume → ~0 for moved builtins), composing with the wrappers already shipped.

This is design-only; the actual ports are the human work this spec scopes. No implementation lands here.

---

## 2. Requirement link

`../REQUIREMENTS.md` **R1** (40%). Quoted acceptance:

> *A native binary of `examples/*.ax` runs and matches interpreter output + a perf-tier benchmark.*

This spec targets the **first half** — *a native binary that builds and matches the interpreter* — by making the codegen build *finish*. The perf-tier benchmark is downstream of a finishing build. Acceptance (§9) is framed as "the build completes under a time budget AND native output matches the interpreter on the corpus," reusing the R10 interpreter-oracle equivalence idea.

Dependencies / leverage: **I-2** (interpreter is the equivalence oracle — every moved builtin is validated against it), **I-8/I-9** (the C-ABI contract must preserve exact observable behavior, incl. error/Result shapes), the existing `axon-rt` link path (`link.rs`), and the shipped `build_wrappers` (composes).

---

## 3. Surface (what changes — internal, no language surface)

No `.ax` language change. The change is **where a builtin's body lives**:

```rust
// BEFORE (codegen/builtins.rs): ~30 lines of hand-emitted inkwell IR per builtin.
let fn_val = self.ir.module.add_function("abs_i64", fn_ty, None);
let entry = self.ir.context.append_basic_block(fn_val, "entry");
self.ir.builder.position_at_end(entry);
// ... build_wrappers::w_int_* calls building abs by hand ...

// AFTER: a declaration only (zero IR body), pointing at the axon-rt symbol.
let fn_val = self.ir.module.add_function("__axon_abs_i64", fn_ty, None);  // extern, no body
self.functions.insert("abs_i64".to_string(), fn_val);                     // dispatch unchanged
```

```rust
// axon-rt/src/builtins_compute.rs (NEW): the body, as ordinary fast-compiling Rust.
#[no_mangle]
pub extern "C" fn __axon_abs_i64(n: i64) -> i64 { n.wrapping_abs() }
```

CLI/observable surface is unchanged — `emit_call` already dispatches by name via `self.functions.get(name)` (`expr.rs:55`), so a declared extern is called identically to an inline-emitted one.

---

## 4. Semantics

### 4.1 The migration mechanic (per builtin)

For each inline builtin moved:
1. **Write the body as `#[no_mangle] pub extern "C"` Rust in `axon-rt`**, honoring the exact C ABI codegen uses (§4.2).
2. **Replace its inline-IR block in `builtins.rs` with a bare `add_function(symbol, None)` declaration** + the existing `self.functions.insert(name, fn_val)` so dispatch is unchanged.
3. **Parity-test** the builtin's `.ax`-observable behavior interpreter-vs-(future)native; in the interim, test the `axon-rt` Rust fn directly against the interpreter's implementation of the same builtin (both are reference-checkable now without the slow build).

### 4.2 The ABI contracts (must preserve exactly — I-8/I-9)

From the existing axon-rt externs and the codegen str layout:

| Axon type | C ABI | Source of truth |
|---|---|---|
| `i64` | `i64` | direct |
| `f64` | `f64` | direct |
| `bool` | `i1`/`i8` | direct |
| `str` (arg) | `(ptr: *const u8, len: i64)` flattened, OR a `{i64 len, ptr}` struct by value | `__axon_read_file(path_ptr, path_len, …)` pattern (`axon-rt/src/lib.rs:245`) |
| `str` (return) | out-params `(*mut i64 len, *mut *mut u8 ptr)`; **negative len = error** (Result-Err) | `__axon_read_file` / `__axon_read_line` convention (`lib.rs:218,243`) |
| `Result<T,str>` | follow the out-param + sign convention OR the canonical `{i1 tag, payload}` per `CLAUDE.md` invariants | `__axon_write_file` error out-params (`lib.rs:283`) |

**Decision:** moved builtins reuse the **existing axon-rt str/Result conventions verbatim** (negative-len-is-error for str returns; out-params for multi-value). No new ABI is invented — this is why the 38 existing externs already interoperate with codegen. Memory ownership: `axon-rt` `malloc`s returned buffers exactly as the inline IR does today (the inline code calls `malloc` 6+ times for str returns; the Rust side uses the same allocator via `libc`/`malloc` so free-side semantics are unchanged).

### 4.3 Behavior table — per migrated builtin

| Input class | Behavior |
|---|---|
| Pure-compute builtin (`abs_i64`, `min_i64`, `sign_i64`, `clamp_i64`) | Trivial Rust; identical i64→i64. Zero IR in axon-core. **Batch 1.** |
| Str→str / str→bool (`str_to_upper`, `str_contains`, `str_index_of`, `str_repeat`, `str_slice`, `str_replace`) | Rust string ops via the len/ptr ABI; the **largest IR consumers** (`declare_string_builtins` ≈ 557 of 951 wrapper calls). **Batch 2 — the big win.** |
| Conversion (`to_str`, `to_str_f64`, `to_str_bool`, `parse_float`) | Rust `format!`/`parse` via str-return ABI. **Batch 3.** |
| Builtins that touch codegen-internal state (control flow, channels, spawn, select) | **Do NOT move** — already extern (channels/spawn) or genuinely need IR-level control flow. Stay inline. |
| A moved builtin whose Rust output ≠ the interpreter's for any corpus input | **Parity fail → blocks that builtin's migration** (E1601, §6). |

### 4.4 Why this finishes the build where wrappers didn't

Wrappers cut each giant IR body ~43% (a constant factor on a superlinear curve). **Moving a builtin deletes its body** — its contribution to `axon-core` IR volume goes to a single `declare` line (~0). Migrate the string family (≈557/951 calls, the bulk) and `declare_string_builtins` shrinks from ~1,178 lines of IR-emitting code to a list of declarations. The superlinear term is driven by per-giant-function body size; emptying the bodies attacks that term directly. Combined with the existing wrappers and `codegen-units` (now able to parallelize the *remaining* smaller functions), the build should cross from "unbounded" to "finishes."

### 4.5 Determinism / correctness preservation

Each moved builtin must be **observably identical** to its inline-IR version. The oracle is the interpreter (`interp.rs`'s implementation of the same builtin, I-2): for every corpus `.ax` using the builtin, `interp` output is the spec. Because the interpreter already has Rust implementations of all these builtins (e.g. `interp.rs` `to_str`, `str_contains`), the axon-rt Rust port can frequently **share code with or mirror the interpreter's exact logic** — making parity near-automatic and eliminating the inline-IR/interpreter drift that parity findings #33/#36/#37 document.

---

## 5. Type rules

N/A. Builtin *signatures* (`builtin_sigs`, `fn_return_types`) are unchanged — only the body's location moves. `infer.rs`/`checker.rs` see no difference (they already treat these as opaque builtins). This is purely a codegen/runtime refactor beneath the type system.

## 6. Error codes

New **E16xx** band (codegen-build / runtime-migration — follows E15xx), per I-14. These are *developer/CI* diagnostics for the migration, not user-facing language errors.

| Code | Trigger | Message shape |
|---|---|---|
| **E1601** | Parity: a migrated `axon-rt` builtin's output ≠ the interpreter oracle on a corpus input | `` migrated builtin `{name}` diverges from the interpreter on `{input}` — ABI or logic mismatch, migration blocked `` |
| **E1602** | A declared extern has no matching `__axon_{name}` symbol in `libaxon_rt.a` (link-time) | `` `{name}` is declared extern but unresolved in axon-rt — add the `#[no_mangle]` fn or revert the declaration `` |
| **E1603** | ABI mismatch: declared `fn_type` ≠ the axon-rt signature (arg/return shape) | `` ABI mismatch for `{name}`: codegen declares {sig1}, axon-rt exports {sig2} `` |
| **W1610** | A builtin that *could* move (pure-compute, no IR control flow) is still inline-emitted | `` `{name}` is inline-IR but movable — migrating it would cut codegen IR volume `` |

## 7. Invariants touched

- **I-2 (interpreter is reference):** the migration's correctness gate *is* the interpreter oracle — every moved builtin is validated against `interp.rs`. This spec also **reduces** I-2 parity risk: today inline-IR and interpreter are two separate implementations that can drift (#33 `random_i64`, #36, #37 `parse_int` all document codegen/interp divergence); moving builtins to axon-rt Rust lets them *share logic* with the interpreter, collapsing two implementations toward one. **Preserved + strengthened.**
- **I-8/I-9 (success signal):** the ABI contracts (§4.2) preserve exact Result/error shapes (negative-len-is-error); a migrated builtin that changed an error into a wrong value fails parity (E1601). **Preserved.**
- **I-11 (capability boundary):** builtins that perform I/O (`read_file`/`write_file`) are *already* extern in axon-rt and already capability-checked at the `@[contained]` boundary in the frontend; moving more compute builtins doesn't touch the boundary (they're pure). **Preserved.**
- **I-14 (stable codes):** E16xx band defined here. **Preserved.**
- **No invariant changed.** This is a build/runtime refactor; semantics are held constant by construction (parity gate).

## 8. Test plan (maps 1:1 to §4.3)

Red test that must fail first: **`migrated_abs_i64_matches_interpreter`** — port `abs_i64` to `axon-rt`, declare it extern, and assert (a) `axon-rt`'s `__axon_abs_i64` returns the same value as the interpreter's `abs_i64` across a value sweep incl. `i64::MIN`, and (b) the symbol resolves at link. Fails today: there is no `__axon_abs_i64` in axon-rt; `abs_i64` is inline IR.

- [ ] **Unit (axon-rt):** each migrated builtin's Rust fn tested directly against a value/string sweep (incl. boundaries: empty str, `i64::MIN`, multibyte UTF-8, out-of-range slice).
- [ ] **Differential (the core):** for each migrated builtin, a property test asserts `axon_rt_fn(x) == interp_builtin(x)` over generated inputs — the interpreter is the oracle (I-2). This runs **now**, no slow build needed.
- [ ] **ABI (link):** a `cargo build -p axon-rt` produces `__axon_{name}` symbols; a check asserts every codegen `add_function(extern)` has a matching exported symbol (E1602/E1603 guard).
- [ ] **Build-finishes (the actual R1 gate, R1-measured):** after Batch 2, `cargo build -p axon-core` (codegen) **completes within a time budget** (target: < 30 min on the reference machine; the metric that defines "unblocked"). This is the one test that needs the slow build — run it on CI/a beefy box per `BUILD_DIAGNOSIS.md` §6, not the inner loop.
- [ ] **Native-parity (post-build):** once the build finishes, a native binary of each corpus `.ax` matches interpreter output (this is the existing `#[ignore]`d parity test, un-ignored).
- [ ] **Regression:** the existing 532-test suite (interpreter path) is untouched and stays green throughout — migration must not change interpreter behavior at all.

## 9. Acceptance criteria (the done gate)

The migration advances R1 in measurable slices:

**Slice 1 — pure-compute (provable now, no slow build):**
- [ ] `migrated_abs_i64_matches_interpreter` + the same for `min_i64`/`max_i64`/`sign_i64`/`clamp_i64` pass (differential vs interpreter).
- [ ] All moved symbols resolve in `libaxon_rt.a` (E1602 clean).

**Slice 2 — string family (the IR-volume win):**
- [ ] `str_*` builtins (`str_contains`/`str_index_of`/`str_to_upper`/`str_to_lower`/`str_repeat`/`str_slice`/`str_replace`) pass differential parity.
- [ ] `declare_string_builtins` reduced to declarations (measured: its `build_wrappers::` call count drops from ~557 toward ~0).

**Slice 3 — the R1 acceptance (R1-machine-gated):**
- [ ] `cargo build -p axon-core` **finishes** under the time budget (< 30 min reference) — *the definition of "past R1."*
- [ ] `native_matches_interpreter_on_corpus` passes (the requirement's "native binary runs + matches interpreter").

R1 rises 40% → ~70% when the build finishes and native matches the interpreter; the perf-tier benchmark (the last requirement clause) follows.

## 10. Performance budget

The *target* is the build itself: move from unbounded to < 30 min (reference machine). The `BUILD_DIAGNOSIS.md` measurement is the baseline; the `CODEGEN_WRAPPER_PROTOTYPE.md` −43%-IR result (already shipped) plus this spec's body-deletion is the lever. **Runtime perf of migrated builtins is neutral-to-better** — a Rust `n.wrapping_abs()` compiled by rustc with normal optimization is at least as good as hand-emitted opt0 IR, and it's now in a separately-optimizable staticlib.

## 11. Rollout & rollback

- **Incremental, per-builtin, parity-gated — the lowest-risk possible shape.** Each builtin moves in its own commit: write the axon-rt fn, swap the declaration, parity test green, commit. If a build measurement regresses or parity fails, `git revert` that one builtin — the inline IR is restored, nothing else touched.
- **Order (by IR-cost-per-effort):** Batch 1 pure-compute (trivial, proves the pipeline) → Batch 2 string family (≈557 calls, the structural win) → Batch 3 conversions. Stop measuring after Batch 2 — if the build finishes, Batch 3 is optional polish.
- **Blast radius:** zero to the interpreter (untouched; 532 tests stay green throughout). Native-only risk, gated by the differential parity oracle. The 38 already-extern builtins are the existence proof that the link path is sound.
- **Measurement discipline (the human part):** per `CODEGEN_WRAPPER_PROTOTYPE.md`'s methodology, **measure on a finishing build** — after each batch, run the timed `cargo build -p axon-core` on the reference machine. Do not declare victory from the interpreter-side parity alone; the build-finishes test (§8) is the real gate and only it can confirm the unblock.

## 12. Open questions

Blocking the final measurement (the human/hardware part):
- **Q1 (the actual finish threshold):** how many builtins must move before the build crosses from unbounded to < 30 min is **empirical** — `BUILD_DIAGNOSIS.md` shows splitting alone didn't help, but body-*deletion* is a different lever than splitting; the knee is unknown until measured. Plan: migrate Batch 1+2, measure; if still unbounded, the remaining `declare_builtins` giants (the ~649-call function) are the next target. **Only a finishing build answers this** — hence human-driven.
- **Q2 (reference machine):** `BUILD_DIAGNOSIS.md` and the maintainers both flag "faster-machine validation." The finish-threshold measurement should run on a high-core/high-RAM box, not a laptop. Procuring/configuring that is outside the codebase.

Non-blocking:
- **Q3 (`declare_builtins` itself):** the ~649-call `declare_builtins` (not the string file) is the other giant. Many of its builtins (arithmetic, comparison) are pure-compute and equally movable; it's Batch 1's natural extension. Sequenced after the string win is measured.
- **Q4 (shared interp/axon-rt source):** §4.5 notes migrated builtins can share logic with the interpreter. A future cleanup factors the common implementations into one crate both depend on, permanently killing inline-IR/interp drift (#33/#36/#37). Deferred — get the build finishing first.
- **Q5 (does this fully replace inline IR?):** some builtins genuinely need IR-level control flow (the ones touching codegen state, §4.3). The build doesn't need *zero* inline IR — just *enough* removed to cross the finish threshold (Q1). Total elimination is not a goal.
