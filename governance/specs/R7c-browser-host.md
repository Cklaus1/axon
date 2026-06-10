# Browser Host — `wasm32-unknown-unknown` + WebGPU canvas

**Spec ID:** `R7c-browser-host` (extends `R7-targets.md` / `R7b-axonhost.md`; ties to `REQUIREMENTS.md` R7)
**Status:** Draft
**Risk class:** Structural
**Author / date:** cklaus, 2026-06-07

---

### 1. Motivation

R7 already runs Axon two ways on wasm: the **interpreter** compiled to `wasm32-wasip1` (Slice A, 28/28
parity) and **AOT codegen** to wasm that runs int/float/struct/array/string value-identically under
`wasmtime` (Slice B, the i64→i32 ABI bridge landed 2026-06-03). But **neither runs in a browser.** Both
target WASI/`wasmtime`, which is a headless server runtime — it has `std::fs`, `--env`, and `_start`, none of
which exist in a browser. The PRD's "Web apps → wasm (WebGPU)" target (`AI_Language_Plan.md:375`) needs the
*other* wasm triple, `wasm32-unknown-unknown`, with a **`BrowserHost`** supplying virtual fs/env/time over JS
and — the whole point — a **WebGPU surface** so the GPU UI stack (`wgpu`, which has a WebGPU backend) renders
to an HTML `<canvas>`. R7b built the `AxonHost` seam and explicitly deferred this: *"a future `BrowserHost`
(fetch FS, env map, `performance.now`, no-op sleep) plugs in via `set_host`"* (`R7b-axonhost.md:76`). This
spec is that BrowserHost, plus the canvas/WebGPU bridge that makes it a UI target rather than just a compute
target.

**The hard truth up front: the browser is asynchronous and single-threaded, and Axon is neither.** Every
real browser I/O path is async — `fetch` (network *and* the modern file API), WebGPU adapter acquisition
(`requestAdapter`), even reading a file — and a long *synchronous* computation on the main thread **freezes
the tab**. Axon today executes synchronously and blocking. So this spec delivers, honestly, **two things at
different maturities**: (1) a *compute-only* browser target — pure `.ax` runs to completion and returns a
value, a real and useful playground — which ships now; and (2) *interactive/async-I/O* browser apps, which
need Axon to suspend and resume on external events and are therefore **gated on the same Phase-6
`resume`/continuation runtime** as the native event loop (R13 Q3). v1 is (1); (2) is scoped, not faked. See
§12 Q4 and the Dependencies note in §4.

### 2. Requirement link

`REQUIREMENTS.md` **R7** (cross-platform, currently ~66–70%, "R7 cross-platform at 66 is the laggard",
`REQUIREMENTS.md:41`). Acceptance criterion: *the same `.ax` source that runs on native and `wasm32-wasip1`
also runs in a browser tab, with host-interface touchpoints (fs/env/time) served by JS and graphics served
by WebGPU — observable result identical to the native run for compute, and a rendered frame for UI.* This
spec also unblocks the `native::gfx` module (R13) on the web target: `wgpu` is one crate with both a native
(Vulkan/Metal/DX12) and a WebGPU backend, so the **same shim** compiles to both once the host seam exists.

### 3. Surface (what the user writes)

**Nothing new in `.ax`.** This is the design constraint (R7's whole thesis: "same source runs everywhere").
The user writes the same program they'd run natively; the *build target* and the *host shim* differ. The
surface is the **build CLI** + the **JS embedding API**, not the language. `--host` is a **new** flag added by
this spec (today's surface is only `--engine {interp|codegen} --target {triple}`); it selects the host shim.

```bash
# Build the interpreter-in-browser bundle (Slice A path — runs .ax source in the tab).
# (--host browser is new in this spec; --engine defaults to interp)
axon target build --target wasm32-unknown-unknown --host browser examples/hello.ax
#   → out/hello.wasm  +  out/hello.js (wasm-bindgen glue)  +  out/hello.html (loader)

# Build an AOT-compiled program for the browser (Slice B path — codegen → wasm, no interpreter in the bundle).
axon target build --target wasm32-unknown-unknown --host browser --engine codegen examples/math.ax
```

```html
<!-- JS embedding API (generated glue; the user includes it) -->
<canvas id="axon"></canvas>
<script type="module">
  import init, { run, mount } from "./hello.js";
  await init();                         // instantiate the wasm module
  run();                               // headless compute: returns the i64 the program returned
  // — or, for a UI program —
  mount(document.getElementById("axon"), { env: { THEME: "dark" } });  // attach to canvas + provide env map
</script>
```

```axon
// A UI program — same source as the desktop wgpu target (R13). No web-specific code.
use native::gfx
@[contained(gfx: any)]
fn view(s: ref AppState) -> Handle { /* build a View tree → gfx draws it to the surface */ ... }
```

### 4. Semantics (what it does)

Two engines × the browser host. The host is the only new code; the engines are unchanged (I-2).

| Input class | Behavior |
|---|---|
| pure-compute `.ax`, interp→browser | Interpreter (already wasm-clean) runs in the tab; `run()` returns the program's `i64`. No host touchpoints hit. Identical to the wasip1/native result by construction. |
| pure-compute `.ax`, codegen→browser | AOT wasm (the landed i64→i32 + size_t bridge) instantiated via wasm-bindgen; `run()` returns the same `i64`. The `--no-entry --export=main` reactor link (already used for wasmtime) exports `main` for JS to invoke instead of `wasmtime --invoke`. |
| `read_file`/`write_file` | Routed through `AxonHost` to `BrowserHost`: a virtual FS backed by an in-memory map (seeded from a `fetch`-ed manifest or `FileSystemAccess` if granted). Ungranted path → `Err` (same Result shape as native, R7 §4.1), never a trap. |
| `env_var(k)` | `BrowserHost` returns from the JS-provided env map (`mount({env:{...}})`); absent key → `None`. |
| `now_ms` | `performance.now()` + the page's time origin → epoch ms (i64). |
| `sleep_ms` | The browser cannot block the main thread. **Synchronous `sleep_ms` is refused** on the browser host (graceful `Err`/no-op with a `W` diagnostic) — pure-compute programs never call it; UI programs use the frame loop, not sleep. (Async sleep is a future `await` surface, out of scope.) |
| `exec` | Always denied (the default `AxonHost::exec` refusal — no process model in a browser). I-11 strengthened: the browser sandbox is a second layer (R7 §"wasm's sandbox is a second enforcement layer"). |
| a UI program (`use native::gfx`) | `gfx`'s WebGPU backend (`wgpu` configured for `webgpu`) acquires a `GPUCanvasContext` from the `<canvas>` passed to `mount()`. `window_open` maps to "attach to the provided canvas" (a browser has no OS windows); `present` submits a WebGPU frame. The event loop is `requestAnimationFrame`, bridged to Axon's `update`/`view` via the callback ABI (R13 Q3). |
| WebGPU unavailable (browser/flag off) | `gfx` init returns a graceful `Err` surfaced as a clean diagnostic in `view`'s `Result`, and `mount` rejects its promise with a readable message — **never** a wasm trap (I-4). |
| `@[contained(net: [...])]` program | A net capability maps to `fetch` restricted to the allowlisted hosts; the browser's CORS/sandbox is the runtime backstop beneath the compile-time E1001–E1004 check. **`fetch` is async** — so net actually working in the browser is part of the async-gated tier (Q4), not v1. |
| `spawn` / `select` / channels (Axon's cooperative concurrency) | `wasm32-unknown-unknown` has **no threads** without the wasm-threads proposal + `SharedArrayBuffer` + cross-origin isolation. v1 runs Axon's cooperative scheduler on the **single browser thread** — the interpreter's spawns already run cooperatively in one OS thread (see `axon-cooperative-concurrency` memory), so fan-out/collect within a single synchronous `run()` works; what does **not** work is any spawn that must *block on external async* (a `fetch`, a timer) — that re-enters the async-gated tier (Q4). A spawn that needs true parallelism is **W**-diagnosed and runs serially. |

**Why a separate triple from wasip1.** `wasm32-wasip1` links against a WASI libc (`_start`, fd-based I/O) and
is run by `wasmtime`. `wasm32-unknown-unknown` has **no libc, no WASI** — all host calls must be JS imports
provided by wasm-bindgen. The AOT codegen path therefore needs a third link mode (`--no-entry`, exports for
JS, no wasi libc, no crt1) distinct from both native cc-link and the wasip1 `rust-lld` link. The interpreter
path needs the interpreter crate to build for `unknown-unknown` (no `std::fs`/`std::thread::scope` —
already the deferred half of R7b: `on_deep_stack` and the five host builtins must all route through
`AxonHost`, with no residual direct `std::*` call on this target).

#### Dependencies — compute now, interactive after Phase-6

| Target | Needs |
|---|---|
| **Browser compute** (`run()` returns a value; in-memory fs/env/time; one static WebGPU frame) | This spec (BrowserHost + the new link mode). **No Phase-6.** Ships in v1. |
| **Interactive / async-I/O browser app** (fetch, real file API, frame loop, net) | This spec **+ the Phase-6 `resume`/continuation runtime** (unbuilt) **+ R-UI**. |

Same two-tier reality as R13 and R14: the static-frame target is reachable now; everything interactive
converges on the one unbuilt runtime capability (suspend/resume), so the platform critical path is
**R7c (compute) → Phase-6 resume → R-UI (interactive)**.

### 5. Type rules

N/A — no language-surface or type-system change. This is a build-target + host-impl concern, exactly as R7
and R7b are. (`@[target(...)]` conditional compilation remains deferred — R7 §12 Q3 — and is not needed:
the same source compiles unchanged.)

### 6. Error codes

| Code | Trigger | Message shape |
|---|---|---|
| E0911 | `--target wasm32-unknown-unknown` build where the interpreter crate has a residual direct `std::fs`/`std::thread` call (won't link) | `target wasm32-unknown-unknown requires all host access via AxonHost; direct std::<x> call at <site>` |
| E0912 | browser AOT link failed (wasm-bindgen / export step) | `browser wasm link failed: <lld/wasm-bindgen detail>` |
| W0913 | `sleep_ms` called under `BrowserHost` | `sleep_ms is a no-op on the browser host (main thread cannot block); use the frame loop` |
| (reuse) E0907/E0908/E0909 | wasm codegen unsupported construct / link failed / axon-rt unavailable (existing wasm bands) | (existing) |

(Confirm E0911/E0912/W0913 are free in the E09xx wasm band against `error.rs` before reserving — I-14.)

### 7. Invariants touched

- **I-2 (parity)** — *preserved*: both engines are unchanged; only the host shim is new. A browser run of a
  pure-compute program is provably the wasip1/native result (same interpreter / same AOT object). The browser
  result joins `wasm_parity.sh` as a third comparison column where a headless browser runner (wasm-bindgen
  test / `wasmtime` cannot help here — needs `wasm-bindgen-test` under headless Chrome) is available.
- **I-11 (capability boundary)** — *preserved and strengthened*: the browser sandbox is a second runtime
  layer beneath `@[contained]`; fs/net/exec the host shim doesn't expose are unreachable even if the
  compile-time check were bypassed.
- **I-4 (never abort the host)** — *preserved*: every browser-host failure (missing WebGPU, denied fs,
  blocked fetch) is a graceful `Err`/rejected promise, never a wasm trap that takes down the tab.
- **I-8 (exit codes)** — adapted: a browser has no process exit; `run()` returns the program's `i64` and a
  nonzero/panic maps to a thrown JS error with the exit code attached. The semantics (success vs failure) are
  preserved; the *delivery* is a return value/exception, not a process code.

### 8. Test plan

- [ ] Unit: `BrowserHost` impls (virtual FS map, env map, `performance.now` shim) tested in Rust against the
      `AxonHost` contract (mirrors R7b's `TestHost` tests).
- [ ] Integration: interpreter crate compiles for `wasm32-unknown-unknown` with **zero** residual `std::*`
      host calls (a `cargo check --target wasm32-unknown-unknown` gate; E0911 is the failure).
- [ ] CLI e2e: `axon target build --target wasm32-unknown-unknown --host browser hello.ax` emits
      `.wasm + .js + .html`; the wasm is magic-verified and imports only the expected wasm-bindgen host fns.
- [ ] Parity (interp/codegen↔browser): `wasm-bindgen-test` under headless Chrome runs `examples/*.ax`
      pure-compute set and asserts `run()` returns the same `i64` as the interpreter oracle. Gated behind a
      `--features browser-test` + headless-Chrome-available guard (manual tier until CI has it, like the GPU
      journey tests).
- [ ] Adversarial: ungranted `read_file` in browser → `Err` not trap; `sleep_ms` → W0913 + no hang;
      `exec` → denied; WebGPU-off → graceful `Err`, tab survives.
- [ ] Journey (UI): `examples/gfx/window_clear.ax` built for browser, `mount()`-ed to a canvas, renders one
      cleared frame (manual/`#[ignore]` tier — needs a WebGPU-capable headless browser).

### 9. Acceptance criteria (the done gate)

- [ ] Test `interp_builds_for_unknown_unknown` passes (no residual std host call; E0911 guards regressions).
- [ ] Test `browser_pure_compute_parity` passes: `examples/hello.ax` returns `42` via `run()` in headless
      Chrome, == the interpreter oracle.
- [ ] Test `browser_host_fs_returns_err_not_trap` passes (graceful degradation).
- [ ] `axon target build --host browser` emits a loadable bundle, asserted by a CLI e2e test.
- [ ] (Stretch / manual tier) `browser_window_clear_renders_frame` — one WebGPU frame on a canvas, behind a
      guarded journey test.

### 10. Performance budget

Bundle size is the web-facing budget. The PRD claims ~3.5 MB for the wgpu+vello stack
(`AI_Language_Plan.md:288`). Targets: **interp-in-browser bundle ≤ 2 MB gzipped** for the runtime (the
interpreter is small Rust); **AOT-per-program bundle ≤ 200 KB gzipped** for a non-trivial compute program
(AOT carries no interpreter). The UI bundle (wgpu/vello) is dominated by the gfx shim and tracked against the
3.5 MB PRD figure. Guard with a `wasm-opt -Oz` size check in the build e2e test.

### 11. Rollout & rollback

- Behind a **new** `--host {default|browser}` flag on `axon target build` (the current surface is
  `axon target build --engine {interp|codegen} --target {triple}`, `main.rs:1639` — there is no `--host` flag
  today; R7b's `AxonHost` is a Rust-internal trait, not a CLI surface, so this flag is the CLI's first
  exposure of host selection) + a `--features browser` build feature. Inert without it; wasip1/native
  untouched.
- Slices, each revertible:
  1. **Interpreter `unknown-unknown` cleanliness** — route the last `std::*` host calls (incl.
     `on_deep_stack`) through `AxonHost`; add the E0911 link gate. (Finishes R7b's deferred half.) Revertible.
  2. **`BrowserHost` + wasm-bindgen glue + `--host browser` CLI** — virtual fs/env/time, `run()` API,
     pure-compute parity in headless Chrome. The load-bearing slice.
  3. **AOT browser link mode** — `--engine codegen` for `unknown-unknown` (export `main`, no wasi libc).
  4. **Canvas/WebGPU bridge** (`mount()`, `requestAnimationFrame` loop) — depends on R13 `native::gfx` and
     wgpu's WebGPU backend. The UI slice; manual journey tier.
- Blast radius: confined to browser builds. A bug cannot affect native or wasip1 (separate target + feature).
  `git revert` of any slice leaves a clean tree; slices 1–2 are independently useful (headless compute in the
  browser) even if 3–4 never land.

### 12. Open questions

- **Q1 (the decisive fork): does the AOT browser path reuse the wasip1 object or need its own?** Resolved:
  **its own link mode, same object-emission.** The codegen IR→wasm object is target-agnostic (it's the
  *link* that differs: wasip1 links wasi libc + reactor entry; browser links nothing and exports for JS via
  wasm-bindgen). The i64→i32 + size_t bridges already in axon-rt apply to both (they key on
  `target_arch="wasm32"`, which covers `unknown-unknown` too). So this is a new *link mode*, not a new
  codegen path. **Verify** axon-rt builds for `wasm32-unknown-unknown` (it currently builds for wasip1; the
  unknown-unknown build must have no wasi-libc dependency — the str/size_t bridges are libc-free, but confirm
  no `__wasi_*` import sneaks in).
- **Q2 (interp vs AOT as the primary browser path).** Lean **interp-first** (Slice A): it's the shipped,
  parity-by-construction path and lets a page run *arbitrary* `.ax` (a playground). AOT (Slice 3) is the
  size/perf path for a *fixed* shipped app. Both ship; interp is the default.
- **Q3 (headless browser in CI).** The parity + journey tests need headless Chrome with WebGPU. Until CI has
  it, these are manual/`#[ignore]` tier (same posture as the native GPU journey test in R13). The
  *pure-compute* browser parity can run under `wasm-bindgen-test` + headless Chrome without WebGPU.
- **Q4 (async — the gating dependency, shared with R13 Q3 and R14 Q3).** Real browser I/O is async:
  `fetch`-backed fs, `@[contained(net)]`→`fetch`, WebGPU `requestAdapter`, and *any* interactive frame loop
  all require Axon to **suspend and resume** on an external event without blocking the main thread. Axon's
  sync builtins can't await yet. The principled mechanism is the **Phase-6 `resume`/shallow-continuation
  runtime** (`CLAUDE.md`: unbuilt; "handlers erase to their body today") — the *same* dependency the native
  event loop (R13 Q3) and the mobile lifecycle (R14 Q3) need. So the honest tiering is: **v1 = compute-only**
  (in-memory synchronous virtual fs + env + time + a *single static* WebGPU frame); **interactive/async-I/O =
  gated on Phase-6 resume**, then R-UI. This is not an `await`-surface bikeshed deferred for taste; it is the
  one runtime capability the entire interactive platform vision sits on. See the Dependencies note in §4.
- **Q5 (the js *backend* vs js *target*).** Still deferred (R7 §12 Q4). This spec delivers the *web target*
  via wasm; a `.ax`→JS transpiler is a separate, large backend and is not this.
