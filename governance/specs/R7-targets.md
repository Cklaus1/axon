# Tech Spec — R7: Cross-Platform Targets

**Status:** ✅ Reviewed (2026-06-01) — *Slice A landed (interp→wasm, 15/15 parity); Slice B object-emit landed; **Slice B AOT-wasm now RUNS end-to-end (2026-06-03): pure-int programs link+run (prune + reactor-mode link), and the str/array ABI gap is CLOSED — see §12 Q6 resolution. The fix was cheaper than the predicted multi-slice codegen retarget: rather than re-lower codegen's pointer width, axon-rt's wasm build (`#[cfg(target_arch="wasm32")]`) declares the SCALAR-EXPANDED form LLVM already produces from a by-value `AxonStr {i64,ptr}` arg, so the codegen object and the runtime agree at link. A 7-str-builtin program now yields the SAME value on interp, native, and AOT-wasm (`scripts/wasm_str_abi_parity.sh`).** Slice A remains the shipped R7-acceptance path; Slice B AOT-wasm is now runnable for int+str programs.*
**Requirement:** `../REQUIREMENTS.md` R7 — *Cross-platform targets: native / wasm / js / mobile from one source.*
**Decisive fork (from `README.md`):** *Does wasm reuse the LLVM backend or need a separate codegen?* (a) native+wasm share the inkwell path (blocked on R1's stalled build), or (b) wasm gets its own lean backend. *"The native build must be unstalled (R1) before any multi-target work is real."* **→ Resolved below (R1 resolved 2026-06-01; with a third option the fork omitted).**

---

## 1. Motivation

R7 is 10% (❌ Thin): native only. **R1 resolved (2026-06-01): the native build now finishes in ~4s** (`BUILD_RESOLVED.md`: the stall was a serde×codegen feature collision, not LLVM). The codegen path is unblocked — the native+codegen build just needs `--no-default-features --features codegen` instead of the default-feature collision. wasm/js/mobile targets are still 0% built.

The fork as written is a **false binary**. It assumes wasm must come from a *codegen* backend (LLVM-IR→wasm or a hand-written wasm emitter). But Axon has a **second, already-shipped execution engine that is pure Rust with no LLVM**: the tree-walking interpreter (`interp.rs`, the `--no-default-features` build, `Cargo.toml` `[[bin]] axon-run`). Compiling *the interpreter itself* to `wasm32-unknown-unknown` yields an in-browser Axon runtime that executes `.ax` source directly — and this path **does not touch the LLVM backend at all** (I-2: the interpreter is already the reference semantics).

So there are three options, not two, and they are not mutually exclusive — they're a sequence:

- **(A) Interpreter→wasm** — compile `interp.rs` to wasm32, run `.ax` in the browser. **Not R1-blocked.** Lands first.
- **(B) LLVM→wasm codegen** — reuse the inkwell pipeline targeting `wasm32-wasi`. **No longer R1-blocked** (R1 resolved 2026-06-01; `BUILD_RESOLVED.md`). The existing cross-compile surface (`codegen/link.rs` `target_triple`, `cross.toml`, E0904/E0905) is the seam. Still deferred — Slice A delivers the R7 acceptance criterion "identical observable results" by construction.
- **(C) Lean wasm backend** — a dedicated AST→wasm emitter bypassing LLVM. Large; only justified if (B) stays blocked long-term.

This spec **resolves the fork by sequencing**: ship (A) now for the R7 acceptance ("same `.ax` runs on native and wasm with identical observable results" — satisfiable interpreter-side), keep (B) as the perf path gated on R1, and hold (C) as the fallback. It specifies (A) concretely and scopes (B)/(C) honestly as R1-dependent.

Design-only; no code until **Reviewed** (Gate 1).

---

## 2. Requirement link

`../REQUIREMENTS.md` **R7** (10%, ❌ Thin). Quoted acceptance:

> *Same `.ax` compiles+runs on native and wasm with identical observable results.*

Critically, "**runs**… identical observable results" is satisfiable by **option (A)** — the interpreter compiled to wasm runs the same `.ax` and, being the *same code* as the native interpreter, produces identical results **by construction** (I-2). "**compiles**" in the AOT sense (a standalone `.wasm` *binary* of a program, not the interpreter) is option (B), R1-resolved but deferred. This spec splits the acceptance accordingly (§9).

Dependencies: **R1** (native build — resolved 2026-06-01, unblocks B; A is still preferred first), **I-2** (interpreter is reference — the lever that unblocks A), **I-11** (capability boundary — wasm's sandbox interacts with `@[contained]`, §4.4).

---

## 3. Surface (what the user writes / runs)

### 3.1 Option A — interpreter in the browser (the shipped path)

No language surface changes. A JS shim instantiates the wasm interpreter and feeds it `.ax` source:

```js
import init, { axon_run_source } from "./axon_wasm.js";
await init();
const exit = axon_run_source("fn main() -> i64 { 21 + 21 }");   // → 42
```

```
# Build the wasm interpreter (no LLVM):
cargo build -p axon-core --no-default-features --target wasm32-unknown-unknown --bin axon-run
# (wrapped by) axon target build --engine interp --target wasm32   # NEW CLI
axon target list                                                   # NEW: show buildable targets + engine
```

### 3.2 Option B — AOT wasm binary (object emits; link reframed 2026-06-03)

```
axon target build --engine codegen --target wasm32 prog.ax   # emits a wasm OBJECT today
axon build prog.ax --target wasm32-wasi                       # the runnable-.wasm goal (link, §12 Q6)
```

The IR→wasm **object** half is shipped (`compile_to_wasm_object`, magic-verified `\0asm`). The
remaining gap is the **link** into a runnable module — and the 2026-06-03 empirical test (§12 Q6)
**corrects the prior diagnosis**: it is *not* a 64→32-bit pointer-width ABI retarget. `rust-lld
-flavor wasm` accepts the object structurally; it fails on **undefined libc/runtime symbols**
(`printf`, `puts`, `malloc`, `exit`, `write`, `snprintf`, `strtoll`, `__axon_parse_int_radix`,
`__axon_parse_int_err`). Even `fn main() -> i64 { 21+21 }` pulls these because codegen declares libc
externs and axon-rt helpers call them. The real link slice is therefore **providing a wasm libc +
a wasm build of axon-rt to link against** — a tractable supply-the-symbols problem, not an IR
retarget. A wasi `libc.a` is already on disk (`…/rustlib/wasm32-wasip1/lib/self-contained/libc.a`).

### 3.3 The host-capability boundary (option A's real work)

The interpreter calls `std::fs`, `std::env`, `std::thread` (`interp.rs`: `read_file`/`write_file` ~2340, env ~3642, `sleep` ~3654, `on_deep_stack` thread-scope ~279). wasm32-unknown-unknown has none of these. The surface is a **host interface trait** the program's I/O builtins route through:

```rust
// Design sketch (not built here): the interpreter takes a `Host` impl.
trait AxonHost {
    fn read_file(&self, path: &str) -> Result<String, String>;   // browser: virtual FS / fetch
    fn write_file(&self, path: &str, data: &str) -> Result<(), String>;
    fn env(&self, key: &str) -> Option<String>;                  // browser: a provided map
    fn now_ms(&self) -> i64;                                     // browser: performance.now
    // no sleep, no threads on wasm — see §4.3
}
```

---

## 4. Semantics

### 4.1 Behavior table — `axon target build --engine interp --target wasm32`

| Input class | Behavior |
|---|---|
| Pure-compute `.ax` (no I/O builtins) | Compiles interpreter to wasm; `axon_run_source` returns the same exit code / output as native. **Identical by construction** (same `interp.rs`). |
| `.ax` using `read_file`/`write_file` | Routes through the `AxonHost` trait; browser host supplies a virtual FS. Without a host impl → the builtin returns `Err` (not a panic), same Result shape as native. |
| `.ax` using `random_*` with `AXON_SEED` | Seeded RNG is pure (`interp.rs` xorshift); reproducible on wasm identically to native. |
| `.ax` using threads / `spawn` / channels | wasm32-unknown-unknown is single-threaded; the cooperative interpreter scheduler already runs spawns eagerly on one thread (it is not OS-threaded), so this works — but `on_deep_stack`'s `std::thread::scope` (`interp.rs:283`) must be replaced (§4.3). |
| `.ax` using `sleep_ms` | No-op or host-provided async on wasm; spec'd as host-delegated (§4.3). |
| AOT `axon build --target wasm32-wasi` (option B) | **E0907** today (unblocked but deferred — R1 landed 2026-06-01; the reason to defer B is priority, not blocker). |

### 4.2 Why option A is "identical results by construction"

The wasm and native interpreters are *the same Rust source* (`interp.rs`) compiled to two targets. There is no second implementation to drift (contrast option C, which would *be* a second implementation and reintroduce an I-2 parity burden). The only divergence surface is the **host interface** (I/O, time, threads) — explicitly enumerated in §4.3 — and each divergence is a capability the program already routes through a builtin, so it is testable as a Result, not a silent difference.

### 4.3 The host-interface divergences (the enumerated, bounded gap)

`wasm32-unknown-unknown` lacks: OS threads, blocking sleep, `std::fs`, `std::env`, process spawn. Mapping:

- **`on_deep_stack` (`interp.rs:279`, `std::thread::scope` at L283):** on wasm, run on the single main stack with the existing `AXON_MAX_DEPTH` guard (BUG_HUNT #28) sized to wasm's default stack; no OS thread. The recursion-limit *graceful panic* still fires before a wasm stack overflow — the #28 stack-coupling logic ports directly.
- **`std::fs` / `std::env`:** behind `AxonHost`; browser supplies virtual implementations or returns `Err`.
- **`sleep_ms`:** host-delegated (browser: async); pure-compute programs never hit it.
- **Provenance log (`append_provenance_jsonl`, interp.rs:4663, fs-backed):** on wasm, writes to an in-memory host buffer the JS side can read — provenance is preserved (R4), just not on a real filesystem.

These are the *complete* set of host touchpoints (grep-enumerable: `std::thread|std::process|std::fs|std::net|std::env` in `interp.rs`). The spec's claim is bounded: **port these N touchpoints behind `AxonHost`, and the wasm interpreter is byte-identical in behavior for everything else.**

### 4.4 Capability boundary on wasm (I-11)

wasm's sandbox is a *second* enforcement layer beneath `@[contained]`: the browser cannot grant fs/net/exec the host shim doesn't expose. `@[contained]` remains the *declared* boundary (compile-time, E1001–E1004); the wasm sandbox is the *runtime* backstop. They compose — a program that passes `@[contained]` and runs on a host shim with no fs simply gets `Err` from `read_file`, never an escape. **I-11 preserved and strengthened** (two layers).

---

## 5. Type rules

N/A. R7 is a build-target and runtime-host concern. It introduces no types, no inference changes. The `AxonHost` trait is Rust-internal (host embedding), not Axon-surface. (A future `@[target(...)]` attribute to conditionally compile per-target is noted in §12 Q3, not specified here.)

---

## 6. Error codes

**Decision:** extend the existing **E09xx target band** (which already holds E0904 "`--target` triple not supported" and E0905 "cross-compile needs sysroot"), rather than open a new band — R7's diagnostics are the same family. Codes E0907/E0908/W0910, invented here per I-14.

| Code | Trigger | Message shape |
|---|---|---|
| **E0907** | `axon build --target wasm32-wasi` (AOT option B) while codegen is unavailable (rare now; R1 resolved 2026-06-01) | `` AOT wasm build needs the native codegen backend, which is not available; use `axon target build --engine interp --target wasm32` to run via the interpreter `` |
| **E0908** | `--target <triple>` names a triple no engine supports | `` no Axon engine targets `{triple}` — interpreter targets wasm32-unknown-unknown; codegen targets are gated on R1 `` |
| **W0910** | A program uses an I/O builtin with no `AxonHost` provided on wasm | `` `{builtin}` has no host implementation on this wasm build — it will return Err; provide an AxonHost to enable it `` |

## 7. Invariants touched

- **I-2 (interpreter is reference):** option A is *literally the reference interpreter* on a new target — the strongest possible parity story (no second implementation). Option C would violate the spirit by creating a divergent engine; it's held as last resort precisely for this reason. **Preserved (A), at-risk (C, hence deprioritized).**
- **I-11 (capability boundary total):** wasm sandbox composes *beneath* `@[contained]` as a runtime backstop (§4.4). **Preserved + strengthened.**
- **I-8/I-9 (success signal):** host-missing I/O returns `Err` (same Result shape as native), never a silent wrong value or a panic that looks like success. **Preserved.**
- **I-14 (stable codes):** E0907/E0908/W0910 defined here. **Preserved.**
- **No invariant *changed*.** R7 adds targets, it doesn't alter semantics — the whole point of routing through the existing interpreter.

## 8. Test plan (maps 1:1 to §4.1)

Red test that must fail first: **`wasm_interp_matches_native_on_pure_compute`** — a harness that runs a set of pure-compute `.ax` files through (i) the native interpreter and (ii) the wasm interpreter (via a headless wasm runtime, e.g. `wasmtime` for `wasm32-wasi` or a node harness for `unknown-unknown`), asserting identical exit codes + stdout. Fails today: there is no wasm build target wired, so the wasm side cannot run.

- [ ] **Unit:** `AxonHost` trait default impl returns `Err` for fs/env on a no-host build; `now_ms`/RNG are pure and target-independent.
- [ ] **Integration:** `cargo build --target wasm32-unknown-unknown --no-default-features --bin axon-run` succeeds (the build itself is the first gate — it fails today when `std::thread::scope` hits).
- [ ] **CLI e2e:** `axon target list` shows `wasm32 (interp)`; `axon build --target wasm32-wasi` → E0907 (honest block, not a hang).
- [ ] **Adversarial:** a deep-recursion `.ax` on wasm hits the `AXON_MAX_DEPTH` graceful panic *before* a wasm stack overflow (ports #28); an I/O program with no host → `Err`, not a trap.
- [ ] **Property (the R7 acceptance):** for a corpus of pure-compute `.ax`, native-exit == wasm-exit for every file (`wasm_interp_matches_native_on_pure_compute`).
- [ ] **Parity (interp↔codegen):** N/A for option A (it *is* the interpreter). Option B's native-vs-wasm-codegen parity is deferred and out of scope.
- [ ] **Journey:** a browser demo runs `examples/hello.ax` and prints `42` — the "Axon in the browser" proof.

## 9. Acceptance criteria (the done gate)

R7 splits into two slices:

**Slice A (interpreter→wasm — deliverable now):**
- [x] `cargo build --target wasm32-wasip1 --no-default-features --bin axon-run` succeeds (host touchpoints handled). **DONE.** *Implementation note: targeted **wasm32-wasip1** (WASI) rather than `unknown-unknown` — WASI provides `std::fs`/`std::env`/exit codes natively and is runnable headless by `wasmtime`, so the only host touchpoint that actually needed changing was `on_deep_stack` (the `std::thread::scope` stack-sizing, which traps on wasm). The full `AxonHost` trait abstraction (for bare `unknown-unknown` / a browser virtual FS) is deferred — WASI covers the I-2 parity acceptance without it.*
- [x] `wasm_interp_matches_native_on_pure_compute` passes over the examples corpus. **DONE** — `scripts/wasm_parity.sh` + cli_run test: 15/15 pure-compute examples produce identical exit code AND stdout on native and wasm (`wasmtime`). Identical by construction (same `interp.rs`, two targets).
- [x] `deep_recursion_graceful_on_wasm` passes (#28 stack guard ports). **DONE** — `on_deep_stack` runs on the single wasm stack (`#[cfg(target_arch="wasm32")]`); `.cargo/config.toml` sets a 64 MiB wasm stack and `RECURSION_LIMIT` is 450 on wasm (empirical overflow boundary ~700), so deep recursion fires the same graceful "recursion limit exceeded" panic as native — verified through `wasmtime`, no trap. *Bounded divergence (R7 §4.3): same failure kind, lower max depth on wasm.*
- [x] `axon target list` / `axon target build --target wasm32 → object|E0907` emit stable output. **DONE** (cli_run: target_list_shows_engines, target_build_aot_wasm_object_or_e0907).

**Slice B (AOT wasm codegen — object half DONE 2026-06-02):**
- [x] **AOT wasm OBJECT emission DONE.** `axon target build --engine codegen --target wasm32 <file>` now emits a real WebAssembly object via the inkwell `wasm32-unknown-unknown` backend (`Codegen::compile_to_wasm_object` → `link::emit_wasm_object`), bypassing the native cc link. The emitted file is magic-verified (`\0asm`); manually confirmed a 22 KB `file`-recognized "WebAssembly (wasm) binary module version 0x1 (MVP)" from `examples/math.ax`. This is the real IR→wasm codegen step §3.2 deferred behind E0907 — no longer a stub. cli_run `target_build_aot_wasm_object_or_e0907` asserts object+magic under codegen, honest E0907 without.
- [ ] **Remaining (the link half):** turning the object into a *runnable* `.wasm` needs a wasm libc sysroot + `wasm-ld` (and, for WASI, the wasi-sdk). Environment-fragile and not wired; documented as the gap. The object-emit half proves the backend works; the link is a packaging step.

R7's REQUIREMENTS row rises from 50% → ~62%: Slice A (wasm-via-interp parity, 15/15) + Slice B object emission (real IR→wasm codegen). The `AxonHost` browser host, js/mobile targets, and the wasm *link* step remain open.

## 10. Performance budget

Option A is the *interpreter* — no Tier-1 perf claim (that's R1/option B's job, and R1 is now resolved). The honest framing: A delivers **portability and reach** (run anywhere wasm runs), not native speed. A perf budget applies only to option B (AOT), which is R1-landed and deferred by priority. Stated so A is not oversold as a performance target.

## 11. Rollout & rollback

- **Slice A is decomposed:** (1) `AxonHost` trait + route the ~6 host touchpoints through it (interp refactor, no behavior change on native — a `DefaultHost` preserves today's `std::fs`/`std::env`); (2) wasm build wiring + `axon target` CLI; (3) the wasm test harness. Each reverts to a green native tree; (1) is a pure refactor guarded by the existing 532-test suite.
- **Blast radius:** the host-trait refactor touches every I/O builtin — highest-risk step. Mitigation: `DefaultHost` is the existing code verbatim; the parity is the *current* test suite passing unchanged (no new native behavior).
- **Slice B/C:** not rolled out — specified and deferred. R1 landed (2026-06-01), so Slice B is no longer blocked by a blocker; it is deferred because Slice A already satisfies the R7 acceptance criterion. Option C (lean backend) remains a last resort — it would carry a full I-2 parity-test burden (a second engine) and is explicitly the least-preferred.

## 12. Open questions

Blocking Slice B/C (engineering decisions, not R1):
- **Q1 (R1 native build):** R1 landed (2026-06-01): `cargo build -p axon-core --no-default-features --features codegen --bin axon` finishes in ~4s (`BUILD_RESOLVED.md`: the stall was a serde×codegen feature collision, not LLVM). **Slice B is now unblocked** — `axon build --target wasm32-wasi` can produce AOT wasm binaries. Slice B remains deferred (not built) because Slice A already satisfies the R7 acceptance criterion "identical observable results" by construction (same `interp.rs` compiled to native + wasm). The gate that was blocking B is gone; the reason to defer B is now *priority*, not *blocker*.

Blocking Slice A (must resolve before building A):
- **Q2 (wasm32-unknown-unknown vs wasm32-wasi for the interp build):** `-wasi` gives `std::fs`/`std::env` "for free" via WASI (less host-trait work) but targets server/CLI wasm, not browsers; `-unknown-unknown` needs the full `AxonHost` but runs in a browser. **Recommendation:** target `-unknown-unknown` with `AxonHost` (the browser is product-v1 per ROADMAP §2.5), and get `-wasi` as a near-free bonus by having `DefaultHost` use real `std::fs` under WASI. Confirm before building.

Blocking Slice B link (diagnosis 2026-06-03, **revised twice — now fully tested end-to-end**):
- **Q6 (what actually blocks the runnable-.wasm link?):** **RESOLVED — it is BOTH a libc symbol set (solvable) AND a real i64↔i32 pointer-width ABI mismatch at the axon-rt FFI boundary (the actual hard blocker).** The diagnosis went through three stages, each empirically tested:
  1. *First (stale, in BUG_HUNT/REQUIREMENTS):* "wrong-width-ABI retarget." Unverified.
  2. *Second (this spec's earlier Q6):* "just libc symbols, NOT pointer width." Tested `21+21` → object → `rust-lld -flavor wasm` against the bare object: failed only on undefined `printf`/`malloc`/`exit`/… → concluded libc-only. **This was incomplete** — it never linked against a wasm axon-rt or *ran* the result.
  3. *Third (definitive, 2026-06-03):* built `cargo build -p axon-rt --target wasm32-wasip1` (succeeds, 11 MB staticlib), emitted the object for `wasm32-wasip1`, and linked object + wasi `libc.a` + `crt1-command.o` + `libaxon_rt.a(wasm)`. The link **succeeds with warnings and the wasm TRAPS at runtime under `wasmtime`**. `rust-lld` reports `function signature mismatch` on `__axon_str_reverse`, `__axon_str_slice`, `__axon_parse_int_err`, **and even `memcmp`/`write`/`strlen`/`snprintf`** — our codegen object declares them with **i64** pointer/length params (the AxonStr `{i64 len, i64 ptr}` ABI baked into the IR) while wasm32 libc + the wasm axon-rt use **i32** pointers. Even `fn main()->i64{21+21}` carries all 19 `__axon_*` extern declarations (codegen's `declare_builtins` emits the whole table unconditionally), so a pointerless program still drags the clashing i64 signatures and `main` returns i64 where wasi's `_start` expects an i32-returning `main`. **It links by luck and traps — the exact "computes wrong / crashes" outcome we must not ship.**
- **So the real Slice B work (corrected) is a wasm32 codegen ABI retarget, not a link step:**
  - **(a) pointer width.** On the wasm32 target, codegen must lower Axon `str`/array/ptr as `{i32 len, i32 ptr}` (or use LLVM's target-pointer-size type) instead of the hardcoded i64 the native path uses. This is the substantial piece — it touches every place codegen builds the str/array struct type and every builtin call ABI.
  - **(b) entry point.** Emit a wasi-compatible entry: either a C-ABI `int main(void)` (i32 return) that `crt1-command.o`'s `_start` calls, or export `_start` directly.
  - **(c) prune unused externs.** `declare_builtins` should declare only the `__axon_*` the program references (or make them weak), so a pointerless program links clean.
  - **(d) wasm axon-rt + link, then `wasmtime`-verify exit+stdout == the interp oracle** (I-2; reuse the `wasm_parity.sh` env). New bands E0908 (wasm link failed) / E0909 (wasm axon-rt unavailable).
- **Honest status:** the libc/sysroot half is *solved* (wasi `libc.a` + a building wasm axon-rt are on disk and link). The blocker is the i64→i32 IR retarget (a). It is NOT a one-iteration slice — it is multi-slice codegen work and is scoped here rather than faked. **Slice A (interp→wasm, 15/15 parity) remains the shipped path that satisfies the R7 "runs identically" acceptance by construction; Slice B AOT-wasm is genuinely gated on (a).**

- **✅ RESOLUTION (2026-06-03) — Slice B AOT-wasm now RUNS for int + str programs; the predicted multi-slice retarget was NOT needed.** The corrected diagnosis above was *itself* one level too pessimistic. The key realization: codegen does NOT actually emit `{i32,i32}` vs `{i64,i64}` — it passes `AxonStr` **by value**, and **LLVM expands a by-value struct argument into its scalar fields** at the call boundary, producing `__axon_str_reverse(i64 len, i32 ptr, …)` on wasm32 (the `ptr` field is i32 because wasm32 pointers are 32-bit; the `len` field stays i64). The `function signature mismatch` was therefore NOT a pointer-width retarget problem in *codegen* — it was that **rustc**, compiling axon-rt for wasm32, passes `#[repr(C)] AxonStr` *indirectly* (a single i32 pointer-to-struct), which disagrees with codegen's scalar expansion. The fix lives entirely in **axon-rt**, native untouched:
  - **(a′) — DONE, replaces (a).** For all 13 str/array-taking externs, axon-rt declares, under `#[cfg(target_arch = "wasm32")]`, the **scalar-expanded** signature LLVM produces (`fn(s_len: i64, s_ptr: *const u8, …)`) and rebuilds `AxonStr { len, ptr }` inside. The native (`#[cfg(not(wasm32))]`) by-value form is unchanged — x86-64 agrees on by-value there, so native parity (213/213 cli_run) is intact. No codegen pointer-width retarget; the str/array IR keeps its existing shape.
  - **(b) — DONE.** Reactor-mode entry: `rust-lld --no-entry --export=main`, run via `wasmtime --invoke main`. No crt1, no `_start` ABI clash.
  - **(c) — DONE.** `prune_dead_functions` (wasm path only) drops unreferenced `__axon_*` so pure-int programs carry zero externs.
  - **(d) — DONE.** `try_link_wasm` now also links the wasm `libaxon_rt.a` (located via `$AXON_WASM_RT` or `target/wasm32-wasip1/{debug,release}/`); `scripts/wasm_str_abi_parity.sh` (cli_run `wasm_str_abi_bridge_runs_str_builtins`) asserts a 7-str-builtin program yields the **same i64 on interp, native, and AOT-wasm** (=21). `wasm_aot_run_parity.sh` covers pure-int (fib(10)=55).
  - **Remaining wasm gap (honest, narrowed empirically):** `println` round-trips (verified: prints to the wasi fd, exit 0 == interp), float compute (`f64_to_i64`, `%.6g`), structs, **array literals**, and **`to_str`** all now run AOT-wasm value-identical to interp. The size_t-width follow-on (below) closed the array/to_str cases. Not yet exercised end-to-end: large heap-growth programs, array-of-struct with nested allocations, and the codegen-side `memcpy`/`memset`/`realloc` paths whose size args still carry i64 (str-builtin memcpy goes through the *runtime* bridge, so it's covered; the remaining codegen memcpy sites are in `str_slice`/`str_repeat`/`splice` fast-paths that currently delegate to axon-rt). **What IS shipped: int, float, struct, array, and string compute on AOT-wasm, value-identical to the interpreter oracle.**

- **✅ FOLLOW-ON (2026-06-03) — libc `size_t` width bridge; array literals + `to_str` now RUN on AOT-wasm.** After the str-ABI bridge, an array literal still trapped the wasm verifier (`type mismatch: expected i32, found i64`): wasm32 is ILP32, so libc `malloc`/`snprintf` take an **i32** `size_t`, but codegen baked i64 (the native LP64 width) into both the declaration AND every size argument. Root structure found via disassembly: `malloc` is declared 9 ways + called 8 ways across codegen, and **first-declaration-wins** per module — `to_str` declared the i64 `malloc` before the array site, so a local truncation couldn't help. Fix:
  - `Codegen.target_is_wasm` (set by `set_target_is_wasm` before `emit_program`) + `size_ty()` (i32 on wasm32, i64 native) + `emit_malloc`/`msize` helpers (mod.rs). Native path unchanged (`target_is_wasm=false` → i64, by-value, LP64).
  - `declare_builtins` now declares `malloc` and `snprintf` ONCE up front with `size_ty()` width; the 7 scattered `unwrap_or_else` fallbacks are now dead (all reuse the canonical decl). Every malloc/snprintf size argument flows through `msize()` (truncates i64→i32 on wasm, identity on native).
  - Verified: `scripts/wasm_malloc_abi_parity.sh` (cli_run `wasm_malloc_abi_bridge_runs_array_and_to_str`) — `[10,20,12]` sum, `to_str(i64)`, and a combined array+int-str+float-str program all yield the same value (42 / 5 / 17) on interp, native, AOT-wasm. Native parity 215/215; all six wasm harnesses green. **NEXT size_t sub-slice (when needed): the codegen-side `memcpy`/`memset`/`realloc` size args (same `msize` treatment) for programs that don't route those through the axon-rt bridge.**

Non-blocking:
- **Q3 (`@[target(...)]` conditional compilation):** per-target code selection in `.ax` source — deferred; not needed for "same source runs everywhere."
- **Q4 (js backend):** the PRD lists js as a separate backend (not LLVM). With option A, "js" could mean "the wasm interpreter behind a JS API" (already covered) vs "transpile `.ax`→JS" (a real separate backend, large). Deferred; A makes the js *target* reachable without a js *backend*.
- **Q5 (mobile):** iOS/Android = wasm-in-webview or native-via-R1; both downstream of A or R1. Deferred.
