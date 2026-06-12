# Axon UI — GPU-Rendered UI Framework (2D + 3D)

**Spec ID:** `R16-axon-ui` (new requirement row; ties to `REQUIREMENTS.md` R7 platform-vision; depends on `R7c-browser-host.md`, `R13-native-ffi.md`, `R15-resume-runtime.md`)
**Status:** Draft
**Risk class:** Structural
**Author / date:** cklaus, 2026-06-12

> **Naming:** the framework is **Axon UI** (crate `axon-ui`), keeping the one-brand `axon-*`
> convention (`axon-core`/`axon-rt`/`axon-surface`/`axon-web`). 3D ships under the same name
> (`Axon UI 3D`, module `axon-ui::render3d`) as a second consumer of the shared `wgpu` backend —
> one framework, two render modes, **not** two products. (An independent sub-brand is deferred
> until/unless Axon UI is adopted apart from the Axon language — see §12 Q7.)

---

### 1. Motivation

Axon's PRD promises one rendering path — `code → GPU → pixels` — that produces pixel-identical output on
every platform with no DOM, no CSS, no bundled browser (`AI_Language_Plan.md`, "UI — GPU-Rendered, Not
Webview"; "3D — Scene Graph on wgpu"). Today none of it exists: the shipped product UI (`axon-web`) is
HTML/JS over CLI JSON — exactly the webview approach the PRD rejects — and there is no grammar, type, or
runtime for `col{}`/`row{}`/`button(...)`. **Axon UI is the missing top half of a UI framework: the
reactive tree, layout engine, `View`/`Msg`/`update` model, and the `.ax` surface syntax that sits on top of
the already-chosen `wgpu` + `vello` + `winit` + `taffy` stack.** The user-visible win: an Axon developer
writes one declarative `fn app(s) -> View` and gets a native window on Linux/macOS/Windows, a `<canvas>` app
in the browser, and (Phase 3) a `.ipa`/`.apk` — all from one source, ~3.5 MB binaries, no Electron.

### 2. Requirement link

This opens a **new `REQUIREMENTS.md` row R16** (the PRD's UI/3D platform vision was previously folded into
the unscored "full-platform vision ≈ 15%" bucket; it now gets first-class tracking). It also advances **R7**
(cross-platform) by giving the `wasm32-unknown-unknown` browser target (R7c) and the native/mobile targets
(R13/R14) a *reason to render*. Acceptance criterion (full): *the same `fn app(s: &State) -> View` source
renders an interactive, input-handling window natively (winit + wgpu) and in a browser tab (R7c WebGPU
canvas) with observably equivalent layout, and a `Scene3D` renders a lit, animated, physics-stepped scene on
the same `wgpu` backend.* v1 acceptance is the 2D compute-render subset (§9).

> **Hard truth up front (mirrors R7c's framing): a UI is an event loop, and Axon is synchronous.** A
> retained-mode reactive UI *must* suspend on "wait for the next frame / the next click" and resume when the
> host delivers it. Axon executes straight-line and blocking. So Axon UI is **gated on the Phase-6
> `resume`/continuation runtime (`R15-resume-runtime.md`)** exactly as interactive browser apps are — the
> `host_await(event) -> reply` substrate that already landed (worker-thread native, Asyncify browser) is the
> mechanism the event loop is built on. This spec does **not** re-solve suspension; it *consumes* R15. A UI
> with no resume runtime can only render a single static frame — which is, deliberately, the v0 slice (§11).

### 3. Surface (what the user writes)

Axon UI files declare `surface` (per ROADMAP §3 substrate/surface split). The model is **Elm/TEA**
(Model–View–Update): pure `View` tree, `Msg` enum, pure `update`. No mutable widget handles, no callbacks
that close over the world — callbacks emit a `Msg`.

```axon
surface

type State = { users: [User], query: str, filtered: [User] }
type Msg   = DeleteUser { id: i64 } | Filter { text: str }

// View is a pure function of state. col/row/text/button/input are Axon UI builtins
// returning `View`. Modifiers (.font/.pad/.on_click) are postfix builders.
fn app(s: &State) -> View {
  col {
    text("Users: {len(s.users)}").font(24).bold()
    for u in s.users {
      row {
        text(u.name).font(16)
        text(u.email).font(12).color(GRAY)
        spacer()
        button("Delete").on_click(Msg::DeleteUser { id: u.id })
      }.pad(8)
    }
    input("Search...").bind(s.query).on_change(|t| Msg::Filter { text: t })
  }.pad(16)
}

// update is pure: (State, Msg) -> State. The runtime owns the loop.
fn update(s: State, m: Msg) -> State {
  match m {
    Msg::DeleteUser { id } => State { ..s, users: arr_filter(s.users, |u| u.id != id) }
    Msg::Filter { text }   => State { ..s, query: text, filtered: arr_filter(s.users, |u| str_contains(u.name, text)) }
  }
}

// Entry point. @[ui] marks the app root; the runtime supplies init state + drives app/update.
@[ui(title: "Users", size: (800, 600))]
fn main() -> State { State { users: seed_users(), query: "", filtered: [] } }
```

3D shares the surface; a `Scene3D` is the rendered value (Phase 4):

```axon
surface

@[ui3d(title: "World", size: (1280, 720))]
fn scene(s: &World) -> Scene3D {
  scene {
    camera(s.cam.pos, s.cam.target, 60.0)
    dir_light((0.5, -1.0, 0.3), WHITE, 1.0)
    model("char.glb").pos(s.player.pos).rot(s.player.rot).animate("run", s.player.speed)
    for o in s.objects { mesh(o.geom).material(pbr(o.color, 0.5, 0.3)).pos(o.pos).shadow(true) }
    physics { gravity((0.0, -9.81, 0.0)); body(s.player, KINEMATIC, capsule(0.5, 1.8)) }
    post { bloom(0.8, 0.3); tonemap(ACES); fxaa() }
  }
}
```

**Error case (capability):** a `surface` UI file that calls `exec`/`write_file`/`ai_complete` without the
matching effect-row grant is rejected at check (`E16xx`, §6) — a `View` builder is pure-by-construction; side
effects only happen in `update` through declared capabilities. **Error case (window grant):** running a
`@[ui]` program in a headless/sandboxed host with no `Window`/`Gfx` capability fails closed, not by crashing
(I-4): the runtime returns a coded error, exit non-zero.

### 3a. Scope of the 2D surface — demand-driven catalog + named hard deferrals

**The widget/primitive catalog is deliberately NOT defined completely, and should not be.** A complete 2D UI
(SwiftUI/Flutter/Compose-class) is a multi-year catalog that no up-front spec gets right; specifying widgets
before a demo validates them is waste that rots before code matches it. What this spec freezes is the
**architecture and contracts** (the View tree + desugar, the layout model, the reactive loop + R15
integration, the Msg/update type rules, the Gfx capability, the GfxHost seam, the I-2 amendment, the test
approach) — the parts that are expensive to get wrong. The catalog grows **demand-driven**, one demo/customer
at a time.

- **v1 widget set (the `users-list` demo, defined in §3):** `col`, `row`, `text`, `button`, `input`,
  `spacer`; modifiers `.font`/`.bold`/`.color`/`.pad`/`.on_click`/`.on_change`/`.bind`. That's it.
- **Demand-driven later (not specified here):** checkbox/toggle/slider/dropdown/select, grid/stack/overlay,
  sizing & alignment modifiers, background/border/radius/shadow, dialogs/menus/tooltips, animation/transition,
  drag/hover/long-press gestures. The PRD's own UI example already reaches past v1 (`avatar()`,
  `.swipe_to_delete()`) — those land when a demo needs them, each a reviewed addition.

**Hard deferrals — architecturally load-bearing, explicitly deferred (NOT "just more widgets").** These cannot
be cheaply bolted on after the View model freezes, so they are named here as known liabilities with a target
slice, even though v1 ships without them. Treating them as silent omissions would be unsound:

| Deferred capability | Why it's architectural (not a widget) | v1 stopgap | Target |
|---|---|---|---|
| **Text/font shaping** (`cosmic-text`/`swash`) | shaping produces glyph metrics that drive **layout** — it is *upstream* of taffy, not downstream of it; i18n/bidi/emoji/fallback change box sizes | one bundled font, naive single-run shaping | Slice 2 (real shaping), pre-i18n |
| **Accessibility** (`accesskit` tree) | a GPU UI has **no DOM**, so the a11y tree is the framework's job and cannot be inherited — this is the precise cost of "no webview"; retrofitting it after the View model freezes is a rewrite | none (documented gap) | Slice 3 — must be designed against the View tree *before* it's frozen |
| **Scroll + list virtualization** | the headline demo is a *list*; at real sizes you cannot lay out N rows — virtualization is a **layout-engine** concern, not a widget | clip + non-virtualized scroll (small lists only) | Slice 2 |
| **Hi-DPI / scale factor** | winit's scale factor multiplies **every** coordinate vello emits; wrong handling mis-renders the whole frame | read scale factor, no fractional-scaling polish | Slice 1 |

These four are added to §12 as open question Q8 (the one that blocks freezing the View tree is accessibility —
it must be designed against, not after).

### 4. Semantics (what it does)

**Pipeline addition.** `View`/`Scene3D` are new built-in types; `col{}`/`row{}`/`scene{}` are *block-form
builder expressions* desugared at parse time (same machinery as Phase-8 `goal{}`) into nested calls
(`__axon_ui_col([...])`). The reactive loop is a runtime service, not codegen.

**The frame loop (consumes R15).** The runtime owns `main`/`update`; the loop is:

```
state = main()                                  // init
loop {
  view  = app(&state)                           // pure: build the View tree
  frame = layout(view)                           // taffy: assign x/y/w/h to every node
  vello.draw(frame); wgpu.submit()               // GPU: tree → vector draw calls → pixels
  event = host_await(NextEvent)                  // R15 suspend: click / key / resize / close / tick
  match event {
    Close => break
    Input(i) => if let Some(msg) = dispatch(view, i) { state = update(state, msg) }  // hit-test → Msg
    Resize  | Tick => continue                   // re-render
  }
}
```

| Input class | Behavior |
|---|---|
| `app(s)` returns a normal `View` tree | laid out by taffy, rendered by vello→wgpu to the window/canvas; one frame produced |
| `View` tree is empty (`col{}`) | renders an empty container at its padded box; no panic (I-9: empty ≠ error, but a zero-size root is a `W16xx` warn — likely a bug) |
| user clicks a `button` | hit-test resolves the node, its `on_click` `Msg` is fed to `update`, state replaced, next frame rebuilt |
| `update` returns the same state | view rebuilt and re-laid-out (cheap); no infinite loop (frame waits on `host_await`) |
| `input` text change | `.on_change(|t| Msg)` fires per keystroke with the new buffer; `.bind` reflects state→field |
| window resized | layout recomputed against new viewport; content reflows (taffy) |
| window closed | loop breaks, `main` returns its `State` as the program value, exit 0 (I-8) |
| no `Window`/`Gfx` capability (headless/sandbox) | runtime returns coded error, exit non-zero — **never** aborts the host (I-4) |
| long synchronous compute inside `app`/`update` | blocks the frame (documented hazard, same as any TEA app); browser tab freezes — mitigation in §12 Q3 |
| `Scene3D` (Phase 4) | scene graph → renderer → wgpu; camera/lights/meshes/PBR materials/physics-step/post-FX as named, one frame |
| asset missing (`model("x.glb")` not found) | `Result`-typed load; a missing asset renders a placeholder + `W16xx`, does not crash (I-4) |

**Parity caveat (the I-2 tension — see §7).** Rendering is *not* byte-parity-testable the way stdout is.
The interpreter cannot rasterize; the parity oracle moves up a layer: **the `View`/`Scene3D` tree and its
computed layout box-model are the testable artifact** (interp builds + lays out the same tree codegen does),
and pixel output is validated by **golden-image snapshot tests** at the wgpu layer, not by interp↔codegen
byte-equality.

### 5. Type rules (if it touches the type system)

- New built-in types: `View`, `Scene3D` (opaque, like `Uncertain<T>`); `Color`, `Vec2`, `Vec3`, `Quat`,
  `Material`, `Mesh`, `Camera`, `Light` (struct-like value types in the Axon UI prelude).
- `col{}`/`row{}`/`scene{}` block expressions have type `View`/`Scene3D`; their bodies are sequences of
  `View`/scene-node expressions (a new block-body kind the parser must learn, mirroring Phase-8 `goal{}`).
- Modifier chains (`.font(24).pad(8)`) are postfix methods on `View` returning `View` — structural, no new
  trait machinery; resolved like existing builtin method dispatch.
- `@[ui]`/`@[ui3d]` attributes constrain the annotated `fn`'s signature: `main: fn() -> State` and the file
  must define `app: fn(&State) -> View` and `update: fn(State, Msg) -> State` (checker rule, §6 `E16xx`).
- `on_click(Msg)` / `on_change(|arg| Msg)` require the closure/value to be the file's `Msg` type — a new
  unification obligation (the `Msg` type is inferred from `update`'s second param).
- Effect rows: UI rendering introduces a **new effect tag `Gfx`** (window + GPU surface) and reuses `Input`;
  `app` is pure (empty row), the runtime loop carries `{Gfx, Input}`. Composes with Phase-6 effect
  subsumption (E1310) and `@[contained]`.

### 6. Error codes

New block **E16xx / W16xx** (next free range; E13xx=AI-policy, E15xx=goal-strategy, so 16xx is clear).

| Code | Trigger | Message shape |
|---|---|---|
| E1600 | `@[ui]` file missing `app`/`update`/`main` of the required shape | `@[ui] app needs `app(&State)->View`, `update(State,Msg)->State`, `main()->State`; missing: update` |
| E1601 | `on_click`/`on_change` payload is not the file's `Msg` type | `on_click expects Msg `Msg`, found `Foo`` |
| E1602 | side-effecting builtin (`exec`/`write_file`/`ai_complete`) called inside an `app`/`View` builder | `View builders are pure; move the effect into `update` (effect `Net` not grantable in `app`)` |
| E1603 | `Scene3D` node used in a 2D `View` context (or vice versa) | `expected View, found Scene3D node `mesh` — use @[ui3d]` |
| E1604 | running a `@[ui]` program with no `Window`/`Gfx` capability in the active host | `no graphics capability in host `HeadlessHost`; run with `--gfx` or a windowing host` |
| W1610 | `View` root lays out to zero area | `app() root has zero size — likely a missing child or pad` |
| W1611 | asset (`model`/`texture`/`heightmap`) not found at load | `asset `char.glb` not found; rendering placeholder` |
| E0910 (reuse) | `@[ui]`/`Scene3D` built with native LLVM codegen (interp-only v1) | `Axon UI is interpreter + windowing-host only; not supported by `axon build`` |

### 7. Invariants touched

**Preserved:**
- **I-1** (pipeline order): block desugar happens at parse; checker learns `@[ui]` rules; no reordering.
- **I-4** (user code never aborts the host): missing assets, missing GPU, closed window all fail as
  `Result`/coded-error/clean-exit, never a panic-abort. *This is the load-bearing invariant for a UI.*
- **I-8/I-9** (success signal): window-close → exit 0; capability-denied/GPU-init-fail → exit non-zero;
  zero-size root → warn not silent success.
- **I-11** (capability boundary is real and total): `Gfx`/`Window` is a **new capability axis** on the same
  footing as net/fs/exec — a sandboxed/headless host denies it; `@[contained]` and effect rows gate it.
- **I-15** (spec↔behavior no drift): this spec is the contract; shipping behavior updates it.

**Changed / extended (this spec doubles as the invariant-change proposal):**
- **I-2** (interpreter is the reference semantics): **amended for non-textual output.** Byte-equal stdout
  parity is undefined for pixels. The reference oracle is restated as: *the interpreter is authoritative for
  the `View`/`Scene3D` tree and its computed layout box-model; pixel rasterization is validated by
  golden-image snapshot at the wgpu layer.* Codegen is interp-only for UI in v1 (E0910), so interp↔codegen
  pixel divergence cannot arise yet. **This amendment must be ratified per `ARCHITECTURE_INVARIANTS.md`'s
  invariant-change process before Slice 2 lands.**

### 8. Test plan (maps 1:1 to §4)

- [ ] **Unit:** `View` tree construction (`col{}`/`row{}` desugar → expected node tree); modifier chains
      accumulate; `Msg` type-checking of `on_click` payloads.
- [ ] **Unit:** layout engine — taffy box-model assigns expected x/y/w/h for nested col/row/pad/spacer cases
      (this is the deterministic, parity-safe core).
- [ ] **Integration:** full frame on a headless wgpu device → golden-image snapshot (`users-list` example);
      byte-compare PNG against a committed reference with a small SSIM tolerance.
- [ ] **CLI e2e (observable):** `axon run users.ax --gfx=headless --frames=1 --snapshot out.png` exits 0 and
      writes a PNG; `--gfx=none` exits non-zero with E1604.
- [ ] **Adversarial:** missing asset (W1611, placeholder rendered, exit 0); side effect in `app` (E1602);
      wrong `Msg` payload (E1601); `@[ui]` missing `update` (E1600); zero-size root (W1610); GPU-init failure
      injected → I-4 clean error not abort.
- [ ] **Property (invariant):** layout is deterministic — same `View` + viewport ⇒ identical box-model
      (seedless); layout never produces NaN/negative sizes.
- [ ] **Parity (interp↔codegen):** N/A for pixels (I-2 amended); **layout box-model parity** interp vs the
      eventual codegen path IS required once Slice 5 codegen exists. v1: interp-only, E0910 guards codegen.
- [ ] **Journey/red-team:** the `users-list` app — render, click Delete (hit-test → Msg → update → reflow),
      type in search (on_change per keystroke), resize (reflow), close (exit 0) — driven by a scripted host
      that feeds synthetic events through `host_await`.

### 9. Acceptance criteria (the done gate — per slice)

**Slice 0 (v0, static frame):**
- [ ] `axon_ui_renders_static_view_to_golden_png` passes (one `app(&State)->View` → headless wgpu → PNG
      matches reference).
- [ ] `axon_ui_layout_boxmodel_is_deterministic` passes (taffy box-model property test).

**Slice 2 (v1, interactive 2D — the real 2D done gate):**
- [ ] `axon_ui_users_list_journey` passes (the full click/type/resize/close loop through a scripted
      `host_await` host).
- [ ] `axon_ui_capability_denied_exits_nonzero` passes (E1604 in a headless-no-gfx host).
- [ ] `axon_ui_view_builder_is_pure` passes (E1602 on a side effect in `app`).
- [ ] Native (winit window) **and** browser (R7c WebGPU canvas) both render the `users-list` example with
      equivalent layout (golden-image per platform, SSIM ≥ threshold).

**Slice 4 (3D — the 3D done gate):**
- [ ] `axon_ui_3d_lit_animated_scene_golden` passes (camera + dir-light + animated GLB + PBR mesh → reference
      frame).
- [ ] `axon_ui_3d_physics_step_deterministic` passes (fixed-timestep Rapier step is reproducible under seed).

### 10. Performance budget

- Frame budget: **16.6 ms** (60 fps) for the `users-list`-scale example (≤ ~200 nodes) on reference
  hardware; layout ≤ 2 ms, vello encode ≤ 4 ms, GPU submit ≤ 8 ms. Guarded by a `criterion` bench on the
  layout+encode path (the GPU submit is measured but not gated — hardware-variable).
- Binary size: native `users-list` ≤ **6 MB** stripped (PRD claims ~3.5 MB; budget allows headroom for
  Axon runtime). Guarded by a size-check in CI.
- 3D: fixed-timestep physics (Rapier) at 60 Hz must not drift the frame budget; a 1000-particle scene is the
  reference perf target (PRD §3D), benched not gated for v1.

### 11. Rollout & rollback

**Feature-flagged behind `--features ui`** (drags `wgpu`/`vello`/`winit`/`taffy` — heavy deps the
default interpreter build must not carry; keeps `cargo build -p axon-core --no-default-features` sub-second).
Sliced so each commit is independently revertible:

| Slice | Deliverable | Revertible? |
|---|---|---|
| **0 — v0 static** | `View` type + `col/row/text` desugar + taffy layout + headless wgpu → PNG snapshot. No event loop, no resume dep. Proves the render path end-to-end. | yes — pure addition behind the flag |
| **1 — native window** | winit window + live frame; still static (no input handling) | yes |
| **2 — interactive 2D** | `host_await` event loop (consumes R15) + hit-test + `update` + input/resize. **The 2D product gate.** | yes — but depends on Slice 1 |
| **3 — browser 2D** | R7c WebGPU canvas + Asyncify event loop; same examples in a tab | yes |
| **4 — 3D core** | `Scene3D` + scene graph + PBR + camera/lights + GLB load (Three.js-subset) | yes — separate module |
| **5 — 3D physics/post/codegen** | Rapier physics, post-FX (bloom/tonemap/fxaa), and (optional) native codegen lowering past E0910 | yes |

**Blast radius:** confined to the `ui` feature; the default build, the CLI verbs, and every existing
test are untouched (the flag is off by default). A `git revert` of any slice leaves the tree building because
the flag gates all of it.

### 12. Open questions

1. **(§5, blocks Slice 2)** Reactive granularity: full re-`app(&State)` per frame (simple, Elm-pure, fine at
   ~200 nodes) vs. fine-grained reactive diffing (signals, à la Leptos/Xilem — faster but a much larger type
   surface). *Default: full rebuild for v1; revisit if the perf bench fails at scale.*
2. **(§3)** Layout engine: adopt **taffy** (mature flexbox/grid, used by Bevy/Zed) vs. write a minimal
   bespoke one. *Default: taffy — don't rebuild a layout engine.*
3. **(§4, hazard)** Main-thread blocking: a long synchronous `update` freezes the frame (browser: the tab).
   Mitigation — run `app`/`update` on the R15 worker substrate and only touch GPU on the main thread? Or
   document the hazard and offer an explicit `spawn`-to-background `Msg` path? *Open — needs an R15 interaction
   review.*
4. **(§5/§7, blocks Slice 2)** `Msg` type discovery: inferred from `update`'s signature vs. a declared
   `@[ui(msg: Msg)]`. *Default: inferred from `update`; error E1601 if ambiguous.*
5. **(§7, blocks Slice 2 ratification)** I-2 amendment for pixel output must be ratified through the
   `ARCHITECTURE_INVARIANTS.md` change process before any rasterizing slice merges. **This is a hard gate.**
6. **(§3, Phase 4)** 3D scope: how much of the PRD's Three.js/Bevy-scale API (skeletal animation blend trees,
   SSAO, instancing, octree culling, full Rapier joints) is v1 vs. deferred. *Default: a Three.js-*subset* —
   camera/lights/PBR-mesh/GLB/basic-physics — and explicitly defer blend trees, advanced post-FX, and spatial
   acceleration to a later slice. The PRD's full 3D stdlib is a multi-quarter epic on its own.*
7. **(strategic)** Webview tension: `axon-web` (HTML/JS) already satisfies the *current* product-v1 approval
   flow. Axon UI is only justified when a customer needs the native binary-size/performance/offline story.
   *This question gates whether Slices 1–5 are "build next" or "spec parked after Slice 0."*
8. **(§3a, one part BLOCKS freezing the View tree)** Hard deferrals — text/font shaping, accessibility,
   scroll/virtualization, hi-DPI. Three are stopgapped in v1 (§3a table) and grow per slice. **The exception
   is accessibility:** an `accesskit` tree must be *designed against* the View-tree representation *before*
   that representation is frozen in Slice 0, because a GPU UI has no DOM to inherit a11y from and retrofitting
   it post-freeze is a rewrite. *Default: stub shaping/scroll/DPI per the §3a targets; but produce a
   one-page a11y-tree design note against the View model before Slice 0 merges — a soft gate on Slice 0, a
   hard gate on shipping any interactive build.*

---

### 13. Forward-compat: the window/GPU surface is an `AxonHost` provider, not a hardcoded `winit` dependency

**Design constraint (binding, from Slice 0):** Axon UI never calls `winit`, `wgpu` *surface* creation, or the
input event source directly. All three go through the **`AxonHost` seam** (`R7b-axonhost.md`) — the same
indirection that already lets `BrowserHost` (R7c) swap WASI for a WebGPU `<canvas>`. This is what makes the
framework retarget to a **future Axon OS** with zero changes above the seam.

**Why this is the load-bearing decision for "one UI across today's platforms *and* tomorrow's Axon OS":** the
GPU-rendered, no-DOM, no-OS-widget-toolkit design already makes the *top* of the stack portable (§1 stack
diagram — `app`/`View`/`Msg`, taffy layout, vello raster are 100% platform-independent). The *only*
per-platform code is the bottom: who owns the window/surface, which GPU backend `wgpu` drives, and where input
events come from. If those three are a **trait the host implements**, a new platform — including a from-scratch
Axon OS — is a new `impl`, not a fork of the framework. If they are hardcoded `winit` calls, every new
platform is surgery through the whole render path. The seam is cheap now and effectively irreversible to add
later, so it is mandated from the first slice.

**The host-graphics trait (new, extends `AxonHost`):**

```rust
/// Implemented per platform. The ONLY place windowing / GPU-surface / input is platform-specific.
/// Everything above (View tree, layout, vello encode) is identical across all impls.
pub trait GfxHost {
    /// Create a window/drawable surface; return a wgpu Surface + initial (w,h).
    fn create_surface(&mut self, title: &str, size: (u32, u32)) -> Result<GfxSurface, GfxError>;
    /// Block until the next input/lifecycle event. THIS is the R15 `host_await` integration point —
    /// suspends the Axon program, resumes when the host delivers a frame/click/key/resize/close.
    fn next_event(&mut self) -> UiEvent;          // Frame | Pointer | Key | Resize | Close | Lifecycle
    /// Present the encoded frame (vello → wgpu submit already done; this is the swap/flush).
    fn present(&mut self, surface: &GfxSurface);
}
```

**The provider matrix — every platform is one `impl GfxHost`; the framework above is shared:**

| Platform | `GfxHost` impl | Window/surface source | `wgpu` backend | Status / spec |
|---|---|---|---|---|
| Linux/macOS/Windows | `WinitGfxHost` | winit | Vulkan / Metal / DX12 | Slices 1–2 (this spec) |
| Browser | `BrowserGfxHost` | HTML `<canvas>` | WebGPU | Slice 3 (extends `R7c-browser-host.md`) |
| iOS / Android | `MobileGfxHost` | UIKit / Android NDK surface (winit can serve both) | Metal / Vulkan | `R14-mobile-targets.md` |
| **Axon OS (future)** | `AxonOsGfxHost` | the OS's own compositor/surface API | the OS's GPU driver (Vulkan-class, or a native `wgpu` backend) | **out of scope here; this seam is the only hook it needs** |

**What Axon OS would owe — and what it gets for free.** Because the seam exists, porting the *entire* UI and
every Axon UI app to a from-scratch Axon OS is **one `impl GfxHost` plus the substrate `wgpu` already
requires** (a Vulkan-class GPU driver, an allocator, a `libstd`-equivalent). Nothing in `axon-ui` above the
trait changes; no application code changes. **Honest scoping (consistent with ROADMAP §2.3, "the kernel
ambition is killed"):** this section makes the framework *retargetable* to an Axon OS — it does **not** commit
to building one, and it does **not** shrink the real cost, which lives *below* the seam (the GPU driver is one
of the hardest parts of any OS, not the UI). The value delivered here is **optionality at zero extra cost**:
choosing the `AxonHost` seam to serve Linux/macOS/Windows/browser/mobile *today* is the same choice that makes
an Axon OS port a new `impl` rather than a rewrite *tomorrow*.

**Invariant tie-in:** this keeps **I-11** (the capability boundary is real and total) clean across platforms —
the `Gfx`/`Window` capability (§6 E1604) is granted/denied at the `GfxHost` seam, so a headless or sandboxed
host (including a locked-down Axon OS profile) refuses graphics uniformly, by *not* providing a `GfxHost`,
with no per-platform special-casing.

**Acceptance (added to §9, Slice 0):**
- [ ] `axon_ui_gfx_goes_through_host_seam` — a static check / test asserts `axon-ui` makes **no direct `winit`
      or `wgpu::Surface` call**; all windowing/surface/input route through `GfxHost`. (Guards the constraint
      from regressing as later slices land.)
- [ ] `axon_ui_headless_host_has_no_gfxhost_denies_cleanly` — a host with no `GfxHost` yields E1604, exit
      non-zero (I-4/I-11), proving the seam is the sole graphics entry point.
