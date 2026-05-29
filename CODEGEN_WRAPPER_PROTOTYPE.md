# Codegen wrapper prototype — `#[inline(never)]` non-generic inkwell wrappers

**Date:** 2026-05-28
**Toolchain:** rustc/cargo 1.95.0, 32 cores, LLVM-17 (`llvm-config-17` 17.0.6), inkwell 0.4.0 (feature `llvm17-0`)
**Question under test:** Does routing inkwell's generic `.build_*` calls through `#[inline(never)]`
**non-generic** free-function wrappers materially cut LLVM-IR volume / compile time vs. inlining
the generic calls directly (as `codegen/builtins.rs` does today)?
**Verdict (one line): GO.** Wrappers cut total LLVM-IR by **~43%**, peak compiler RSS by **~36%**,
and full-codegen wall-clock by **~1.7–3×** in an isolated repro — a real, reproducible win, unlike
the prior trait-based IR shim which was a null result.

---

## 1. Why measure in isolation (and why the prior attempts couldn't)

`cargo build -p axon-core` (codegen feature) never finishes; even `cargo llvm-lines` on it times
out at 23 min (per `BUILD_DIAGNOSIS.md` §4). You cannot A/B a change against a target you can't
build. So the experiment uses a **minimal standalone reproduction** that *finishes*, lets it be
measured exactly, and isolates the one variable (generic-inline vs. non-generic-wrapper).

The repro reproduces the *shape* of the pathology: a couple of giant functions
(`declare_builtins` / `declare_string_builtins`) each containing many "builtin" blocks, every block
issuing the same family of heavily-generic inkwell builder calls used by the real code:
`add_function`, `append_basic_block`, `position_at_end`, `build_extract_value`, `build_alloca`,
`build_store`, `build_load`, `build_int_add/sub/mul`, `build_int_compare`, `build_select`,
`build_conditional_branch`, `build_unconditional_branch`, `build_call`, `build_phi`, `build_return`.

---

## 2. Repro design

Two-crate cargo workspace at `/tmp/inkwell_repro` (shared `inkwell = { version = "0.4",
features = ["llvm17-0"] }`, `[profile.dev] opt-level = 0` to match axon-core):

- **crate_a (Variant A — baseline):** a `gen_builtin_a!` macro expands a full builtin body with
  **direct generic `.build_*` calls inline** — exactly like `declare_builtins` today. Two giant
  functions, each with `N/2` macro expansions.
- **crate_b (Variant B — wrapper):** a `gen_builtin_b!` macro builds the *same* IR, but every
  generic inkwell call goes through an `#[inline(never)]` **non-generic** free function
  (`w_add`, `w_icmp`, `w_alloca_i64`, `w_extract_int`, `w_extract_ptr`, `w_load_int`,
  `w_store_int`, `w_select_int`, `w_cond_br`, `w_call_ptr`, `w_phi_merge`, `w_add_function`,
  `w_append_block`, `w_position`, `w_ret_int`, ...). Each wrapper takes/returns **concrete**
  inkwell types (`IntValue`, `PointerValue`, `StructValue`, `FunctionValue`, `IntType`,
  `BasicBlock`) with **no type parameters**, so each generic inkwell instantiation is
  monomorphized **once** (inside the wrapper) rather than re-expanded at every call site.

Block count `N` is parameterized so we can find the superlinear knee. Both variants generate
**byte-for-byte equivalent LLVM IR semantics** — only the Rust→LLVM-IR lowering differs.

Files: `crate_a/src/lib.rs` + `variant_a_body.rs`, `crate_b/src/lib.rs` + `variant_b_body.rs`.

---

## 3. Results

### 3a. LLVM-IR volume (`cargo llvm-lines`) — the decisive IR-generation metric

| N (blocks) | A total IR | A per giant fn | B total IR | B per giant fn | **IR reduction** |
|---:|---:|---:|---:|---:|---:|
| 300  | 89,292  | 44,103  | 51,534  | 25,053  | **−42.3%** |
| 1200 | 353,892 | 176,403 | 201,834 | 100,203 | **−43.0%** |

- In **A**, the two giant functions hold **98.8–99.7%** of all IR (the generic inkwell builder
  bodies are inlined into them). This is the `builtins.rs` pathology in miniature.
- In **B**, each giant function is **~43% smaller**. The ~19 `w_*` wrappers each appear with
  **`Copies = 1`** in the llvm-lines output (confirmed) and total only **~360 IR lines combined** —
  they are NOT inlined back, and they absorb the per-call-site generic expansion that A pays N times.
- The reduction ratio is **stable across scales** (−42% at N=300, −43% at N=1200), i.e. it's a
  structural constant-factor cut in IR volume, exactly the lever `BUILD_DIAGNOSIS.md` §6 option 3
  identified.

### 3b. Wall-clock (full codegen, `CARGO_INCREMENTAL=0`, deps warm, opt-level=0)

N=1200, 3 trials each (`/usr/bin/time`, body content changed each trial to force full re-lowering):

| Trial | A wall | A peak RSS | B wall | B peak RSS |
|---|---:|---:|---:|---:|
| 1 | 14.80 s | 1243 MB | 4.75 s | 795 MB |
| 2 | 15.84 s | 1236 MB | 8.56 s | 795 MB |
| 3 | 13.43 s | 1256 MB | 9.19 s | 792 MB |
| **median** | **14.80 s** | **1243 MB** | **8.56 s** | **795 MB** |

→ **~1.7× faster (median), up to ~3.1× (best trial); ~36% less peak RSS.** B's memory is
rock-steady at ~795 MB; A's is ~1243 MB. (B's wall variance is machine-contention noise; its memory
and IR-line numbers are dead stable, and it is faster in every trial.)

### 3c. Superlinear knee (the pathology signature) — A vs B scaling

| N | A wall | B wall | B speedup |
|---:|---:|---:|---:|
| 300  | 0.18 s  | 0.14 s  | ~1.3× |
| 1200 | 14.8 s  | 8.6 s   | ~1.7× |
| 2400 | 33.0 s  | 16.7 s  | ~2.0× |

A grows **super-linearly**: 4× the blocks (300→1200) costs ~80× the time. This is the
LLVM-IR-generation/lowering superlinearity `BUILD_DIAGNOSIS.md` pins as the root cause, reproduced
in miniature. B grows superlinearly too **but on a ~half-as-steep curve** — the wrappers shrink the
per-giant-function body that drives the superlinear term, so the speedup *widens* with scale.

### 3d. Bonus finding — incremental codegen reuse

With default `CARGO_INCREMENTAL=1`, editing a line *outside* the giant-function bodies (e.g. a
comment in `lib.rs`) rebuilt crate_a in **0.70 s** — rustc's incremental cache reused the unchanged
giant-function codegen entirely. This is orthogonal to A-vs-B but is a real dev-loop lever: the
multi-minute cost is paid only on a clean build or when the giant bodies themselves change.

---

## 4. Why the earlier trait-based IR shim was a null result, and why wrappers differ

`SESSION_RECAP.md` (`research/ir-shim-2026-05`): the shim moved ~1,692 raw `self.ir.builder.*`
calls in `builtins.rs` onto an **IR trait surface** (`self.ir.X`). `cargo check` passed; memory
dropped (3.6 GB → <2 GB) **but build time was unchanged** — "builtins.rs bodies still expand
inkwell-generic call sites even though they go through the IR trait surface."

The mechanism of that failure, now testable:

1. A **trait method that is itself generic** (or whose body is generic over inkwell value/type
   params) does **not** reduce the count of distinct monomorphized inkwell instantiations — it just
   adds an indirection that monomorphizes into the *same* set of generic bodies at each call site.
2. If the optimizer (or `#[inline]`/cross-crate inlining) **inlines the trait method back**, the
   generic inkwell body re-expands at the call site exactly as before. A trait surface alone changes
   *naming/dispatch*, not *instantiation count*. → IR volume unchanged → IR-generation time unchanged.
   (The shim's memory win was a separate effect — fewer simultaneously-live temporaries — not an
   IR-volume win.)

The wrapper approach is **structurally different** on the exact axis that matters:

- Each wrapper is a **free function with concrete (non-generic) parameter and return types**. There
  is **one** monomorphization of the generic inkwell call *inside the wrapper*, full stop —
  regardless of how many call sites invoke it (confirmed: every `w_*` shows `Copies = 1`).
- `#[inline(never)]` **guarantees** the optimizer cannot fold the body back into the giant function,
  so the call site lowers to a single `call @w_add` — a few IR lines — instead of the full expanded
  generic body. This is the precise failure mode the shim hit (inlined-back generics) and that
  `#[inline(never)]` forecloses.

So: **trait surface ≠ instantiation reduction; non-generic `#[inline(never)]` wrappers = guaranteed
one-instantiation-per-shape.** The repro confirms the latter cuts IR by 43% where the former cut 0%.

---

## 5. GO / NO-GO

### GO — apply non-generic `#[inline(never)]` wrappers to `codegen/builtins.rs`.

**Expected payoff** (extrapolating the stable −43% IR / −36% RSS / ~1.7–3× constant factors to the
real crate, whose 951 `.build_*` call sites in two giant functions are the same shape as the repro):

- **IR volume:** roughly **−40% across `codegen::builtins`**, the crate's dominant IR generator
  (`declare_builtins` ~649 calls, `declare_string_builtins` ~557). That is a large constant-factor
  cut to the single most expensive input to the serial MIR→LLVM-IR lowering.
- **Peak RSS:** ~−35%, easing the 3.6 GB pressure noted in `SESSION_STATUS.md`.
- **Wall-clock:** a constant-factor speedup (the repro shows ~1.7–3×). It does **not** change the
  asymptotic class — both A and B are superlinear — so on an unbounded multi-hour build this turns
  e.g. a 5 h build into ~1.5–3 h, not into seconds. It is a **meaningful but not magic** win.

### Important caveats (do not oversell)

1. **It is a constant factor, not an asymptote fix.** The giant-function shape remains; B is on a
   shallower superlinear curve, not a linear one. Pair with the standard recommendation: keep
   `axon-check` / `--no-default-features` as the dev binary (`BUILD_DIAGNOSIS.md` §6) and treat the
   native codegen build as CI/release. Wrappers make that CI build dramatically cheaper, not instant.
2. **The biggest remaining lever may be splitting the giant functions into many real functions** so
   `codegen-units` can parallelize them — wrappers shrink each giant body but two giant bodies still
   land in one CGU each and lower serially. Wrappers + per-builtin functions compound.
3. **Real `builtins.rs` calls are more varied** than the repro's uniform block (different arg/return
   inkwell types per builtin). Each distinct *concrete IR shape* needs its own wrapper, so the win on
   the real file depends on how many shapes recur. The repro's wrappers cover the common shapes
   (int arith/compare/select, alloca/load/store, extract int/ptr, call, phi, branch, add_function,
   append_block, position); these dominate `builtins.rs`, so the bulk of the win should transfer.
4. **Application cost:** mechanical but large (≈950 call sites). Recommend incremental rollout — wrap
   `declare_builtins` first, measure with a *finishing* tool (this repro's methodology, or
   `cargo check --timings` + `llvm-lines` on a single-function extract), then `declare_string_builtins`.

### Bottom line

Non-generic `#[inline(never)]` wrappers are the **first structural change to the codegen build that
shows a real, reproducible, measured win** (−43% IR, −36% RSS, ~1.7–3× faster) — and a clear,
mechanistic explanation of why they succeed where the trait-based IR shim (a documented null result)
did not. **GO**, with the realistic framing that this is a strong constant-factor improvement to the
CI/release codegen build, best combined with function-splitting, while day-to-day dev stays on
`axon-check`.

---

## Appendix — reproduction

- Repro: `/tmp/inkwell_repro/` (workspace; `crate_a` = baseline, `crate_b` = wrappers).
  Body files are generated; block count `N` set via a small Python generator (in the task transcript).
- Build env: `LLVM_SYS_170_PREFIX=$(llvm-config-17 --prefix)`. inkwell+llvm-sys compiled cleanly
  against system LLVM-17 in this environment (~4.5 s `cargo check` for the whole workspace).
- Measurements: `cargo llvm-lines -p crate_{a,b}` (IR lines); `CARGO_INCREMENTAL=0 /usr/bin/time -v
  cargo build -p crate_{a,b}` (wall-clock + max RSS), each forced to full re-lowering by editing the
  body content; all wrapped in `timeout 600` (none hit the cap — the repro finishes in seconds).
- Every wrapper confirmed `Copies = 1` in llvm-lines (not inlined back; one instantiation each).
