# Axon UI — GPU-Rendered UI Framework (2D + 3D)

**Spec ID:** `R16-axon-ui` (new requirement row; ties to `REQUIREMENTS.md` R7 platform-vision; depends on `R7c-browser-host.md`, `R13-native-ffi.md`, `R15-resume-runtime.md`; **interacts with `R24-defended-approval-boundary.md`, `R28-capability-audit-ledger.md`, `R31-extended-tcb-attestation.md`** — see §7.1, added 2026-07-31)
**Status:** Draft
**Risk class:** Structural
**Author / date:** cklaus, 2026-06-12
**Reserves:** the **E21xx / W21xx** block (E2100–E2108, W2110–W2112) — *renumbered 2026-07-31; extended
2026-07-31 with E2105–E2108 / W2112 by the ASI-trajectory review, §6* — from the
draft's E16xx claim, which was wrong: `E1604` is already live as the R12b kernel-goal budget-exhausted code
(dedicated exit 7, asserted by supervisors and `cli_run.rs` tests), and `error.rs` allocates the E16xx band
to R20 kernel TCB obligations (E1610/E1611). E21xx confirmed free at claim time (grepped `error.rs` + all
`governance/specs/` reserve lines; E20xx is R41's, E2300–E2302 are eBPF's). When the first R16 code lands,
record the band in `error.rs` with a reserving comment, R14/R17-style.

> **Naming:** the framework is **Axon UI** (crate `axon-ui`), keeping the one-brand `axon-*`
> convention (`axon-core`/`axon-rt`/`axon-surface`/`axon-web`). 3D ships under the same name
> (`Axon UI 3D`, module `axon-ui::render3d`) as a second consumer of the shared `wgpu` backend —
> one framework, two render modes, **not** two products. (An independent sub-brand is deferred
> until/unless Axon UI is adopted apart from the Axon language — see §12 Q7.)

---

### 1. Motivation

Axon's PRD promises one rendering path — `code → GPU → pixels` — that produces pixel-identical output on
every platform with no DOM, no CSS, no bundled browser (`AI_Language_Plan.md`, "UI — GPU-Rendered, Not
Webview"; "3D — Scene Graph on wgpu"). Today the framework **top half** of it does not exist *(scoped
2026-07-31 — the original "none of it exists" overstated: R13 already shipped the bottom-half GPU bring-up,
see §1a)*: the shipped product UI (`axon-web`) is HTML/JS over CLI JSON — exactly the webview approach the
PRD rejects — and there is no grammar, type, or runtime for `col{}`/`row{}`/`button(...)`. **Axon UI is the missing top half of a UI framework: the
reactive tree, layout engine, `View`/`Msg`/`update` model, and the `.ax` surface syntax that sits on top of
the already-chosen `wgpu` + `vello` + `winit` + `taffy` stack.**

**The primary justification is checkability, not developer ergonomics** *(reframed 2026-07-31 — the draft led
with "an Axon developer writes one declarative `fn app(s) -> View` … no Electron", a human-productivity
argument that sits badly with ROADMAP §2.1, which states the typed language is "an **IR**, not a
human-authored surface … optimize for unambiguous audit, machine generation, and stable spec — not human
ergonomics", and §2.4, which makes the typed AST the legal artifact)*: a UI expressed as a typed `View` tree
is **visible to the compiler** — effect rows, the R6 taint analysis, refinement types, SMT discharge, and the
R28 audit ledger can all inspect it — whereas the incumbent `axon-web` UI is generated HTML/JS/CSS that *no
Axon check can see at all*. When the program under a UI is machine-generated, an unverifiable presentation
layer is a growing liability, not a neutral implementation detail. This spec only earns that claim if it
actually specifies the checks; §5/§6/§8 are where it does (and the review-driven additions of 2026-07-31 —
closed-purity E2102, the `Input` taint source E2105, and the structural `View`-tree oracle §8 — are precisely
the ones that cash it).

**Secondary (still real) win:** an Axon developer writes one declarative `fn app(s) -> View` and gets a native
window on Linux/macOS/Windows, a `<canvas>` app in the browser, and (Phase 3) a `.ipa`/`.apk` — all from one
source, no Electron. *(Scoped 2026-07-31: the
PRD's "~3.5 MB binaries" win is a **Slice-5 outcome** — it requires the optional native codegen lowering
(§11); v1 is interp-only (§6 E0910), so shipping a v1 UI app means shipping the Axon interpreter CLI built
with `--features ui`, not a small per-app binary. See the matching §10 scoping.)*

### 1a. Prior art in-tree — reconciliation with the R13 `gfx` capability + `crates/axon-gfx` (added 2026-07-31)

The draft specified the graphics stack as greenfield. It is not; R13 (a declared dependency of this spec)
already landed two things this spec MUST build on, not duplicate:

1. **The `gfx` capability grant axis already exists.** `@[contained(gfx: any)]` is the E1004-enforced grant
   key for `use native::gfx` (`R13-native-ffi.md` §"gfx", wired through checker + interp); an ungranted
   `native::platform`/`native::gfx` call is **E1004** today (`error.rs`'s own R14 comment: "Mobile reuses
   E1004"; E1708–E1709 are an *explicitly unclaimed gap* in the E17xx band, reserved by no spec — an earlier
   2026-07-31 edit here wrongly attributed them to R14; corrected same day). The §5 `Gfx` **effect tag** is
   the *effect-row face of that same axis*, not a second, parallel capability: granting
   `@[contained(gfx: ...)]` and carrying `Gfx` in the effect row must resolve to the same underlying
   permission (one axis, two surface syntaxes). **The bridge precedent, stated precisely (corrected
   2026-07-31):** `contained_effect_row` (`effects.rs`) maps `fs:`/`exec:` → `IO` and `net:` → `Net` — there
   is no `FS` effect row, and it has **no gfx arm at all** today; the only current gfx→row bridge is the R13
   `native::gfx` *module's* declared `IO` row on its calls. Adding the `gfx:` → `Gfx` arm to
   `contained_effect_row` is therefore **R16 to-build work**, not existing behavior this spec merely mirrors.
   An implementation that creates a distinct second graphics permission is wrong.
2. **A real wgpu bring-up already exists.** `crates/axon-gfx` is a headless wgpu offscreen renderer with
   pixel read-back, gate-verified on lavapipe behind the off-by-default `gfx-wgpu` feature (plus
   `crates/axon-gfx-mock`). **Slice 0 builds on `crates/axon-gfx`** — its device/adapter/offscreen-target
   plumbing is the render-path substrate the PNG-snapshot gate runs on; Slice 0 adds the vello encode +
   taffy layout above it, it does not re-do wgpu init. The proposed `--features ui` flag is a superset
   feature that *implies* (depends on) `gfx-wgpu`; it does not introduce a second wgpu feature gate.

What §1 correctly calls missing is everything **above** that substrate: the `View` tree, `Msg`/`update`
model, `.ax` surface syntax, taffy layout integration, vello raster, and the R15 event loop.

### 1b. Prior art out-of-tree — what we adopt, and what we refuse to build *(added 2026-08-01, founder decision)*

**Decision: Axon UI does not build a renderer.** It does not build a text engine, a layout
engine, or a platform/windowing layer either. It builds exactly one thing — **the typed,
inspectable `View` tree and the `.ax` surface syntax that produces it** — and sits that on a
thin, replaceable backend seam.

The rationale follows §1's own framing. The differentiator is *checkability*: effect rows, R6
taint, refinement types, SMT discharge and the R28 ledger can all read a typed `View` tree. None
of that value lives in rasterization, glyph shaping, or event loops. Those are years of work that
several teams have already done, and doing them again buys Axon nothing strategically while
adding a large, permanently-maintained surface *below* the TCB boundary that §7.1 is trying to
keep small.

Stated as an invariant for this spec: **every pixel-producing component is a dependency, and must
remain replaceable.** If a decision here makes a renderer hard to swap, that decision is wrong.

#### The survey

| Project | What it is | What Axon takes | What Axon refuses |
|---|---|---|---|
| **Vello** | "A 2D graphics rendering engine written in Rust, with a focus on GPU compute" (Linebender) — same role as Skia/Cairo. Uses prefix-sum algorithms to parallelize normally-sequential work onto the GPU; runs on `wgpu`, targets anything supporting WebGPU default limits (desktop, web via Chrome, Android). ***Verified 2026-08-01: IN AN ALPHA STATE.*** Acknowledged open work: blur/filter effects, conflation artifacts, GPU memory allocation strategy, glyph caching. **Hard requirement: a GPU with compute-shader support** — no software fallback. | The renderer. Already chosen; this section ratifies it **as an alpha dependency** — see §1c. | Writing our own rasterizer. |
| **Parley** | "An API for implementing rich text layout" (Linebender). ***Verified 2026-08-01 against the repo.*** Sits on the "Parley text stack": **Fontique** (font enumeration + fallback), **HarfRust** (a Rust port of HarfBuzz — shaping), **Skrifa** (TrueType/OpenType reading, glyph metrics), **ICU4X** (bidi, segmentation, Unicode). Parley itself computes layout: glyph coordinates, line breaking, bidi resolution. MSRV Rust 1.88+, Apache-2.0/MIT, actively maintained. | Text layout, wholesale. **Currently absent from this spec — that is a hole.** | Shaping/bidi/font fallback. This is the classic thing hand-rolled UI frameworks underestimate and never finish. |
| **Taffy** | "A flexible, high-performance, cross-platform UI layout library." ***Verified 2026-08-01.*** Implements **CSS Block, Flexbox and CSS Grid**. Used by **Servo, Blitz, Bevy, Slint, Lapce and Zed** — by some distance the best-attested dependency in this table. | The layout engine. Ratifies §12 Q2. | A bespoke layout engine. |
| **Masonry** | ***Verified 2026-08-01:*** "a toolkit for building UI frameworks (including Xilem)" — retained widget tree, event handling, update passes. Explicitly **experimental**. Note its self-description targets *framework builders*, which is exactly our position. | The widget/paint seam — still the concrete candidate for our backend boundary, now known to be experimental. | Owning widget internals. |
| **Xilem** | Reactive UI framework over Masonry, "inspired by React, SwiftUI and Elm"; lightweight view tree, re-renders from tree changes. ***Verified 2026-08-01: explicitly EXPERIMENTAL.*** | The *architecture study* for §12 Q1 — and its experimental status is itself evidence for Q1's resolution (fine-grained reactivity in Rust is not settled). | Adopting its type machinery wholesale. |
| **gpui** | ***Verified 2026-08-01 — the draft was WRONG about reusability.*** README: "a hybrid immediate and retained mode, GPU accelerated, UI framework for Rust, designed to support a wide variety of applications", with setup instructions for standalone apps on macOS/Linux/FreeBSD/Windows. So it IS intended for use outside Zed. But: "still in active development as we work on the Zed code editor, and is still pre-1.0. There will often be breaking changes between versions." | Evidence on what product quality costs, and a live standalone option worth re-evaluating — not the dismissal the draft implied. | Its **hybrid immediate/retained** model: an immediate-mode component undercuts §1's retained-tree checkability argument (see the egui row). |
| **Slint** | ***Verified 2026-08-01 — analogue claim CONFIRMED:*** ".slint files are compiled ahead of time. The expressions in the .slint are pure functions that the compiler can optimize" — lex/parse/optimize/codegen into Rust/C++/JS/Python. Backends: femtovg (GLES2), Skia, **software (CPU, no deps)**, Qt. **Stable 1.x API** — the only mature option in this table. **Licensing: royalty-free / GPLv3 / commercial — NOT permissive.** | The compiler/runtime split, and their AOT-purity framing, which matches §5's `app` purity obligation almost exactly. Their **software renderer** is also the answer to Vello's compute-shader hard requirement, if that becomes a constraint. | Adopting it: the licence is incompatible with vendoring into a permissively-licensed Axon, and we already have a language. Study only. |
| **Iced** | "Cross-platform GUI library for Rust focused on simplicity and type-safety. Inspired by Elm" — State / Messages / view / update, exactly §3's model. Renderers: `iced_wgpu` (Vulkan/Metal/DX12) and `iced_tiny_skia` (software). ***Verified 2026-08-01: README says "currently experimental software"*** despite wide adoption. | The reference implementation of our `update` model. | Its renderer abstraction. |
| **Floem** | Fine-grained reactive (Lapce) | The counter-data-point for §12 Q1 — what signals cost in practice. | — |
| **Blitz** | "A radically modular HTML/CSS rendering engine." ***Verified 2026-08-01 — and it is the single most useful row here:*** it composes **Stylo + Taffy + Parley + Vello + Winit + html5ever + AccessKit**. That is our entire chosen stack *including both §3b seams*, wired together and rendering real layout. Status: **pre-alpha** — "already has a very capable renderer, but there are also still many bugs and missing features." | Existence proof that the stack composes, and a reference for HOW to wire Parley and AccessKit into a Vello/Taffy tree — the two seams §3b just added. | CSS. |
| **Makepad** | Shader-based DSL with live reload | Only if shader-level control becomes a requirement. | Currently out of scope. |
| **egui / Dear ImGui** | Immediate mode | Studied as the **contrast case**: immediate mode is simple and fast to build, and it is *structurally wrong here* — there is no retained tree to inspect, so the entire §1 checkability argument evaporates. | The paradigm. |
| **AccessKit** | "Accessibility infrastructure for UI toolkits" — a cross-platform, cross-language abstraction so a toolkit implements a11y once. ***Verified 2026-08-01 against the repo.*** Tree-based schema: nodes with **stable integer IDs**, **roles** (button/label/text input), attributes, and an **action system** (focus, invoke, text selection). Push-a-full-tree-then-incremental-updates model, explicitly after Chromium's design; adapters keep the full tree in memory. Released adapters: **Windows (UI Automation), macOS (NSAccessibility), Linux/Unix (AT-SPI D-Bus), Android, iOS**. C and Python bindings. Adapters at "rough feature parity"; single- and multi-line text inputs supported, **rich text / hypertext not yet**. | The accessibility seam. **Also absent from this spec — a second hole.** | Retrofitting a11y later; it is far more expensive after the tree design is frozen. |

#### Consequences for this spec

1. **§12 Q1 (reactive granularity) — RESOLVED: full rebuild for v1.** The default stands, and the
   survey strengthens it rather than merely deferring: Xilem and Floem are both still actively
   working out the ergonomics of fine-grained reactivity in Rust, and their type surfaces are
   large. A coarse rebuild also keeps the `View` tree trivially serializable, which §7.1 depends
   on for the canonical hash. Revisit only if §10's bench fails — and if it does, adopt Masonry's
   damage/dirty-region model before reaching for signals.
2. **§12 Q2 (layout engine) — RESOLVED: taffy.** Ratified by the "no pixel-producing component is
   ours" rule; Blitz and Bevy are the scale evidence.
3. **Two gaps this spec must close before Slice 1.** Text layout (**Parley**) and accessibility
   (**AccessKit**) appear nowhere in §3/§4/§9. Both are seams that must exist in the `View` type
   from the start — a11y in particular cannot be bolted on after the tree design freezes, and a
   `View` tree that cannot describe its own accessible structure is also one an auditor cannot
   fully read, which is the same defect §1 objects to in `axon-web`.
4. **The backend seam is the deliverable.** §13 already argues the window/GPU surface is an
   `AxonHost` provider rather than a hardcoded `winit` dependency. This section extends that to
   the renderer, text engine and layout engine: all four sit behind provider traits, and the
   acceptance criteria in §9 should include *swapping one out* as an exercised test, not an
   aspiration.

#### 1c. The finding the verification produced: the whole stack is pre-1.0

Verifying the rows changed the picture more than any individual row did.

| dependency | status as verified 2026-08-01 |
|---|---|
| Vello (renderer) | **alpha**; needs compute shaders; glyph caching + GPU memory strategy still open |
| Masonry (widget seam) | **experimental** |
| Xilem (architecture reference) | **experimental** |
| Blitz (composition reference) | **pre-alpha** |
| Iced (model reference) | "currently experimental software" |
| gpui | **pre-1.0**, "often breaking changes between versions" |
| Taffy (layout) | mature and widely attested — Servo, Bevy, Slint, Lapce, Zed |
| Slint | **stable 1.x** — and licensed royalty-free / GPLv3 / commercial |

**Every pixel-producing dependency Axon has chosen is pre-1.0, and the only stable
alternative is not permissively licensed.** §1b's "we do not build the renderer" decision is
still right — the alternative is building an alpha renderer ourselves, which is strictly worse.
But "ratifies" was too settled a word for what these actually are, and the spec should not read
as though the stack were a solved problem.

Three consequences:

1. **The replaceability invariant in §1b is now load-bearing, not stylistic.** It was written as
   good hygiene. Given an alpha renderer and an experimental widget layer, it is the actual risk
   control: if Vello's alpha gaps (blur/filters, conflation artifacts) block a slice, the seam is
   what makes swapping possible rather than catastrophic. §9 must exercise a backend swap.
2. **Vello's compute-shader requirement is a platform-reach constraint, and it is not recorded
   anywhere in §2/§10.** See §1d — this is now a decision, not an open question.
3. **Blitz is the reference implementation to read first.** It already composes Stylo + Taffy +
   Parley + Vello + Winit + AccessKit — our exact stack plus both §3b seams — so the integration
   questions §3b raises have a worked answer in-tree somewhere. Read it before designing the
   seams from first principles.

Also worth recording as a correction: the draft dismissed **gpui** as "coupled to Zed's needs and
not designed as a reusable dependency". That is wrong — it ships standalone setup instructions for
four platforms. The real objection is different and narrower: gpui is a *hybrid immediate and
retained* mode framework, and an immediate-mode component has no persistent tree to inspect,
which is what §1's checkability argument needs. Right conclusion, wrong reason — and the wrong
reason would have led us to stop looking at it, which the corrected one does not.

#### 1d. Reach — the renderer seam MUST admit a CPU path *(decision, 2026-08-01)*

**Decision: the renderer seam is required to support a software (CPU) backend. Vello is the
default renderer, not the only one.**

Vello requires a GPU with compute-shader support and ships no software fallback (§1b, verified).
Taken alone that is a hardware-support footnote. It is not, for three reasons, and the third is
the one that decides it.

**(a) Reach.** Any target without compute shaders cannot run Axon UI *at all* — older hardware,
much embedded, constrained or GPU-less VMs, remote/headless sessions, and locked-down enterprise
environments where GPU access is restricted. That is not a long tail for a system whose pitch is
running agents under supervision on infrastructure you do not necessarily control.

**(b) It contradicts a stated target.** R17's bare-metal Axon OS track and R14's mobile targets
both reach hardware where a compute-shader GPU driver is the hardest part of the port. ROADMAP
§2.3 already names "the GPU driver + HAL below the language" as the hard 90%. A UI framework that
*cannot render without that driver* makes the hard 90% a prerequisite for the UI rather than a
later slice.

**(c) The decisive one: a GPU-only renderer cannot be gate-tested.** This project's entire
governance discipline is executed gates — and §7.2 documents what happens to controls that are
not executed: the R28 ledger was never written, the kill switch polled nothing, verdicts sealed
faults as success, all silently, all for months. CI has no GPU. A renderer with no CPU path means
**every visual and layout acceptance criterion in §9 is either skipped in CI or asserted without
running** — which is precisely the vacuous-gate failure this repo has spent an audit cycle
removing (see the harness skip census, `tasks/opportunities.md` O006). A software backend is not
primarily a reach feature; it is the thing that makes Axon UI *testable in the same way as the
rest of the system*.

**Existence proofs** (verified §1b): Slint ships a `software` renderer with no dependencies, and
Iced ships `iced_tiny_skia`. A CPU path for 2D vector UI is well-trodden; this is not research.

**Obligations:**

1. The renderer provider trait (§13) is defined so that a CPU backend is a *conforming
   implementation*, not a special case. If any part of the `View`→pixels contract assumes GPU
   semantics, that is a defect in the seam.
2. **§9 acceptance criteria render on the CPU backend in CI**, and the GPU backend is verified
   separately where hardware exists. Golden-image comparison runs headless. A criterion that can
   only pass on a developer's machine is, by this repo's own standard, not a gate.
3. Any output difference between backends is either bounded and documented (antialiasing
   tolerance) or a bug. "GPU and CPU render differently" must not be discovered late — it is the
   same class as the native↔interp parity invariant (I-2) the language already holds itself to.
4. 3D (`render3d`) is explicitly **out of scope** for the CPU path. Compute-shader hardware is a
   reasonable requirement for 3D; it is not one for a button.

**Non-goal:** the CPU backend need not match GPU performance. §10's budget applies to the GPU
path. The CPU path's bar is *correctness and testability*, not frame rate.

#### Verification status of this section

**Parley and AccessKit rows were verified against their upstream repositories on 2026-08-01**
(see the inline notes). The verification found a real error in the draft: it described Parley as
sitting on **swash**, which is wrong — the current stack is Fontique + HarfRust + Skrifa + ICU4X.
Swash is not in it. That is precisely the failure mode this note warns about, caught on the first
two rows checked.

**Verified 2026-08-01:** Parley, AccessKit, Vello, Taffy, Masonry, Xilem, gpui, Slint, Iced,
Blitz. Three corrections came out of it (Parley's stack, gpui's reusability, and the maturity
picture in §1c) — a 3-in-10 error rate on rows written from working knowledge, which is the
argument for this note existing.

**Still unverified:** Floem, Makepad, egui/Dear ImGui. All three are minor rows (a counter-data
point and two contrast cases), none of which currently binds a decision. Every
"what Axon takes" row is a hypothesis about a dependency, and this project has a documented,
repeatedly-measured habit of code-read confidence being wrong. Before any of this binds a slice:
clone each candidate, build the hello-world, and record the actual API shape and activity level.
Treat this table as a research plan, not a finding.

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

**Error case (capability):** a `View` builder is pure **by enforcement, not by convention** — see §6 E2102,
which is a *closed* transitive obligation (empty effect row under the `infer_effects` fixpoint **and**
`@[pure]`-checkable under the P05 callee-whitelist), not a denylist of named builtins. Side effects happen
only in `update`, through declared capabilities. *(Contradiction resolved 2026-07-31: the draft read "calls
`exec`/`write_file`/`ai_complete` **without the matching effect-row grant** is rejected", which implies a
grant makes an effect legal *inside `app`*. It does not. **No effect-row grant, `@[contained]` grant, or
capability of any kind makes an effect legal inside `app` or a `View` builder.** The grant surface applies to
`update` only.)* **Error case (window grant):** running a
`@[ui]` program in a headless/sandboxed host with no `Window`/`Gfx` capability fails closed, not by crashing
(I-4): the runtime returns a coded error, exit non-zero.

### 3b. Two seams that belong in the `View` type, not the backend *(added 2026-08-01)*

§1b flagged text layout and accessibility as absent. Both are commonly treated as backend
concerns and added later. In this spec they cannot be, for the same reason: **each one changes
what the `View` tree *means*, and §7.1 hashes that tree.** They are type-level obligations.

#### (a) The text seam — `Text` is a shaped result, not a string

`text("Users: {n}").font(24)` currently implies the node holds a string plus style. That is
insufficient, and not merely for layout:

> **The rendered glyphs are not a function of the `View` tree alone.** Shaping depends on the
> fonts actually resolved at render time — bidi reordering, cluster breaking, and font fallback
> can change what a human sees without changing one byte of the source string.

So a canonical hash taken over `text: "…"` does **not** bind what was displayed. §7.1's claim
("what you see is what was authorized") is false at the glyph level unless the shaped result is
part of the hashed artifact. This is a small hole with the same shape as the audit failures in
§7.2: a control that appears to cover something it does not.

Type rule: a `text(...)` node carries a **`TextLayout`** — an opaque handle produced by the text
provider (Parley) from `(string, style, available width, font set)` — and the canonical
serialization of a `View` includes a **digest of the resolved shaping inputs**: the font
identities and versions actually selected, not the requested family. Two runs that resolve
different fonts must produce different hashes, because they produced different pixels.

Consequences: `TextLayout` is opaque to `.ax` (no glyph-level API on the surface); measurement is
a provider call, so layout (taffy) and paint (vello) consume one shaped result rather than each
shaping independently; and font resolution becomes a **capability-visible input** — a program
whose rendering changes with ambient system fonts is not reproducible, which R15/§9.5 replay
already treats as a defect elsewhere.

#### (b) The accessibility seam — the a11y tree IS the audit tree

Every `View` node must be able to produce an accessibility node: **role, name, value, state,
relations**. The framing that makes this cheap rather than an add-on:

> An auditor asking "what does this screen assert, and what can it do?" and a screen reader
> asking the same question want **the same tree**. AccessKit's node model is, structurally, the
> machine-readable description of the interface that §1's checkability argument already requires.

Building a11y is therefore not a separate cost centre — it is the export format for the property
this spec exists to provide. A `View` tree that cannot describe its own accessible structure is
one an auditor cannot fully read, and that is the exact defect §1 levels at `axon-web`.

Type rules:

- Every builtin `View` constructor has a defined default role (`button` → button, `text` → static
  text, `input` → text field, `col`/`row` → generic container).
- **Name is mandatory for interactive nodes.** A `button` whose accessible name cannot be derived
  from its content is a **compile error**, not a lint. Rationale: an unnamed interactive control
  is exactly an action a human cannot audit — the same class as an approval banner that does not
  say what it approves.
- Decorative nodes must be marked explicitly (`.a11y_hidden()`), never defaulted. Fail-closed:
  an unmarked node is exposed, so forgetting the annotation produces noise rather than an
  invisible control.
- The accessible tree is derived from the `View` tree by construction — never assembled
  separately — so the two cannot drift. This is a structural guarantee, not a test obligation.
- **Constraint discovered on verification (2026-08-01):** AccessKit adapters support single- and
  multi-line text inputs but **not rich text / hypertext**. §3a's deferral list must therefore
  name rich text as a11y-blocked, not merely unscoped — shipping styled/linked text before
  upstream support exists would create a surface that is renderable but not describable, which
  §3b(b) forbids by design.

#### Acceptance obligations (feed §9)

1. A `text` node whose resolved font differs between two runs produces two different canonical
   hashes. *Executed*, not asserted from the code path (§7.2 obligation 1).
2. A `button` with no derivable accessible name fails to compile with an `E21xx` code.
3. The AccessKit tree exported for the §3 headline example round-trips: every interactive node in
   the `View` tree appears with a role and a non-empty name.
4. Layout and paint consume the **same** `TextLayout` instance — asserted by provider-call count,
   so a regression that re-shapes independently is caught rather than merely being slow.

### 3c. How Blitz wires Parley and AccessKit — a read of the reference *(added 2026-08-01)*

§1c named Blitz as the one project already composing our exact stack plus both §3b seams. Read at
`packages/blitz-dom/src`. Four things transfer directly, and one gap is more instructive than the
things that work.

**Module shape.** Accessibility and text are *peer consumers of the node tree*, not layers inside
it: `accessibility.rs` walks the DOM independently, and `stylo_to_parley.rs` converts resolved
style into text-layout input, with `font_metrics.rs` handling measurement. Text layout is
positioned as a specialised **style consumer** rather than a layout stage — which is why it can
be swapped without touching the tree.

**What transfers to the `View` type:**

1. **The accessible tree is built by walking the node tree** — `build_accessibility_tree(&self)
   -> TreeUpdate`, visiting every node and building parent/child links as it goes. This is the
   direct validation of §3b(b)'s "derived by construction, never assembled separately". It is not
   a theoretical property; it is how the only working reference does it.
2. **Accessibility node IDs are the node's own IDs** — `NodeId(node.id.as_u64())`, with the window
   root at `NodeId(u64::MAX)`. **This imposes a requirement §3b did not state: `View` nodes need
   stable identity.** A tree rebuilt each frame with fresh identities cannot produce a stable
   accessible tree, and assistive technology tracks nodes across updates. Identity must be
   derivable from tree position or an explicit key — this is now a §5 type obligation, not an
   implementation detail.
3. **Roles come from a static element→role table** (W3C HTML-AAM: `"button" => Role::Button`,
   `<input>` role varying by `type`). Exactly §3b(b)'s constructor→role mapping. Confirms the
   approach is a lookup table, not inference.
4. **The tree is FULLY REBUILT on each call** — fresh map, single traversal, converted into one
   `TreeUpdate`. Independent convergence with §12 Q1's resolution (full rebuild for v1): the
   reference implementation of our stack made the same choice for its accessibility tree.

**The instructive gap: accessible *name* computation is not visible in that module.** Text
content is read via `node.text_content()`, but deriving a name from `aria-label`, `alt`, labelled-by
relations and so on is not there. That is the genuinely hard part of accessibility, and the
reference punts on it.

This *strengthens* §3b(b) rather than undermining it. Blitz inherits HTML's problem: a name can
come from six places with a precedence order, and must be computed at runtime from a document it
does not control. **Axon does not have that problem** — `button("Delete")` has its name in the
constructor. Making a derivable name a *compile-time obligation* (E21xx, §3b) is available to us
precisely because we control the surface syntax, and it converts the hardest part of a11y from a
runtime resolution algorithm into a type rule. That is a real advantage of the compiled-DSL
approach, and §3b should be read as claiming it deliberately.

**Recommended before Slice 1:** build Blitz, run its a11y output against a screen reader, and
diff its `TreeUpdate` against what our `View` tree would produce for the §3 headline example. The
seam design questions are answered there; re-deriving them from first principles would be the
same mistake §1b's verification note warns about.

### 3d. ASI view semantics — rendering values that are not yet values *(added 2026-08-01)*

Everything in §3–§3c is table stakes: a competent 2026 Rust GUI needs a renderer, layout, text,
a11y and stable identity. **None of it is specific to this system.** This section is the part that
is, and it exists because a UI expert review found the spec had no answer for the four things an
ASI interface actually does.

**The unifying observation.** Four apparent gaps — uncertainty, pending operations, streaming
output, and agent activity — are one problem: *a `View` must be able to render a value that is not
(yet, or not fully) a value.* They differ only in which axis is incomplete:

| axis | incomplete how | Axon type that already exists |
|---|---|---|
| **epistemic** | known, but not confidently | `Uncertain<T>` |
| **temporal — pending** | does not exist yet; an effect is in flight | *(none — see (b))* |
| **temporal — streaming** | arriving, append-only | *(none — see (c))* |
| **provenance** | exists, but its origin is unverified | R28 ledger record (§7.2, §9E) |

One rule covers all four, and it is §1's checkability thesis applied to values rather than to
structure:

> **A `View` may not silently render an incomplete value as though it were complete.** Collapsing
> an incomplete value to a plain one is permitted, but it must be an **explicit, visible act in
> the source** — and therefore an act the compiler, the auditor and the reviewer can all see.

That is the whole design. The rest is what it implies.

#### (a) Uncertainty is not silently discardable

`text(u)` where `u: Uncertain<f64>` is a **compile error** (`E21xx`). The author must write one of:

- `text(u)` → *(with a `View`-level uncertainty renderer configured)* renders the value **and its
  confidence** — the default, chosen so the honest thing is the short thing;
- `text(u.point_estimate())` → renders the value alone, discarding confidence. Legal, greppable,
  and reviewable. It is the UI counterpart of an `unwrap()`.

This is available to Axon and to essentially no one else: the uncertainty is *in the type*, so the
framework can refuse the lossy path at compile time rather than hoping a designer remembers. It is
also the first place the language's ASI types reach the surface — today `Uncertain<T>` is
enforced through the interpreter and invisible to the product layer.

Same treatment for `Result<T,E>` in a `View` position: no implicit "render the Ok arm and blank on
error". A UI that blanks on error is how an operator ends up looking at a screen that is confidently
wrong — §7.2's failure mode, in the surface.

#### (b) Pending is a runtime state, not an app-state field

Every MVU app eventually grows `is_loading: bool` fields, and they drift from reality. They should
not exist here, because **the runtime already knows**: it issued the effect. An `ai_complete`, a
`goal_run`, a `host_await` — the frame loop knows which are outstanding.

Proposal: `Pending<T>` is a runtime-provided value, not something `update` maintains. A `View` node
bound to a pending value renders a defined pending representation, and — critically — **carries the
effect row of the operation it is waiting on**, so the interface can say *what* it is waiting for
(a model call, the filesystem, the network) rather than showing an undifferentiated spinner.

Consequences: `update` stays pure and synchronous (§5 preserved); the §12 Q3 main-thread question
narrows, because long work is *by construction* not in `update`; and "what is this app waiting on"
becomes inspectable in the `View` tree, which is exactly what §1 wants of everything else.

#### (c) Streaming is append-only, and that is a type-level fact

LLM output arrives token by token. Under §12 Q1's full-rebuild-per-frame, naive streaming reshapes
the whole string every frame — quadratic, and the first thing that will blow §10's budget in a
real ASI app. Q1's resolution did not consider it.

`Stream<str>` is an **append-only** text value. Because appendedness is in the type rather than
inferred by diffing, the text provider may shape only the new run and append glyphs — which is
what Parley's incremental layout supports (§3b(a)), and what §12 Q1 would otherwise have to be
reopened for. **This is the case that would have forced fine-grained reactivity**; typing the
append instead of detecting it avoids that, and keeps Q1 resolved.

Constraint: a `Stream` node's identity (§5) is stable across appends by definition — the node is
not replaced, it grows. Otherwise a screen reader re-announces the whole message on every token,
which is the a11y failure that makes streaming interfaces unusable.

#### (d) Agency is ambient, and rendering it is mandatory

For an approval-boundary product the first UI question is not "what does the data look like" but
**what is the agent doing right now, what did it just do, and how do I stop it.** R27's kill switch
exists at the CLI (§7.2 — and was silently inert until today). It has no `View` representation.

Agency is not app state; it is ambient runtime state, like focus. The runtime exposes an
`Agency` value — the acting **principal**, the **effect row** currently being exercised, **budget
remaining**, and the **stop affordance** bound to R27.

Rules, deliberately fail-closed in the same shape as `.a11y_hidden()`:

- When agency is non-idle, an app that renders **no** agency surface is a **compile error**. An
  interface that hides a running agent is the one failure this product cannot ship.
- The stop affordance is not the app's to implement. It is provided, always reachable, and — per
  §1d — must render on the **CPU path** too, because "the operator can always stop it" cannot
  depend on a working GPU driver.
- The agency surface is part of the a11y tree with a mandatory name (§3b(b)). A stop control a
  screen-reader user cannot find is not a stop control.

#### Deliberately NOT specified here — adopt, do not invent

The review also found input and presentation gaps. They are real and they are **ordinary**, and
this spec is better served by naming a reference than by inventing:

- **Focus, tab order, pointer capture, IME** — adopt Masonry's model. Note focus order and the
  a11y traversal are *the same traversal* (§3b(b)); specifying them separately would guarantee
  they drift.
- **Animation / transitions** — the known-hard case for MVU. Defer to Slice 3; do not design it
  before (b) and (c) land, since pending and streaming are where transitions are actually needed.
- **Theming, dark mode, localization** — Slice 3+, unremarkable.

#### Acceptance obligations (extend §9.0)

- [ ] `axon_ui_uncertain_in_view_position_is_a_compile_error` — `text(u: Uncertain<f64>)` fails with
      `E21xx`; `text(u.point_estimate())` compiles.
- [ ] `axon_ui_pending_node_reports_its_effect_row` — a node awaiting `ai_complete` renders a
      pending state naming the **AI/Net** row, not a generic spinner.
- [ ] `axon_ui_stream_append_does_not_reshape_prefix` — asserted by **text-provider call count**
      across N appends (same technique as §9.0(B)); a quadratic regression fails the gate.
- [ ] `axon_ui_stream_node_identity_is_stable_across_appends` — id unchanged; no re-announcement.
- [ ] `axon_ui_non_idle_agency_without_surface_is_a_compile_error`.
- [ ] `axon_ui_stop_affordance_renders_on_cpu_backend` — §1d(A) + §3d(d), executed headless.

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
| **Text/font shaping** (**Parley** — *corrected 2026-08-01; the draft said `cosmic-text`/`swash`, and Parley's stack is Fontique + HarfRust + Skrifa + ICU4X, §1b*) | shaping produces glyph metrics that drive **layout** — it is *upstream* of taffy, not downstream of it; i18n/bidi/emoji/fallback change box sizes | one bundled font, naive single-run shaping | Slice 2 (real shaping), pre-i18n |
| **Accessibility** (`accesskit` tree) | a GPU UI has **no DOM**, so the a11y tree is the framework's job and cannot be inherited — this is the precise cost of "no webview"; retrofitting it after the View model freezes is a rewrite | none (documented gap) | **Split 2026-08-01:** the *seam* (roles, mandatory names, stable ids, `.a11y_hidden()`) is now a §3b/§5 **type obligation in Slice 1** — it constrains the View type and so cannot wait. Platform *adapter* wiring stays Slice 3. |
| **Rich text / hypertext** (styled runs, inline links) | **a11y-BLOCKED, not merely unscoped** *(added 2026-08-01, discovered by verifying AccessKit against upstream — §1b)*. AccessKit adapters support single- and multi-line text inputs but **do not yet support rich text or hypertext**. §3b(b) requires every `View` node to be describable in the accessible tree, so shipping styled or linked text would create a surface that **renders but cannot be described** — which §3b forbids by construction, not by preference. | plain text runs only; a whole-node style (`.font()`, `.color()`) is fine, *inline* style spans and inline links are not | **Blocked on upstream AccessKit**, not on our slice order. Re-check before scheduling; do not schedule against our own roadmap. |
| **Scroll + list virtualization** | the headline demo is a *list*; at real sizes you cannot lay out N rows — virtualization is a **layout-engine** concern, not a widget | clip + non-virtualized scroll (small lists only) | Slice 2 |
| **Hi-DPI / scale factor** | winit's scale factor multiplies **every** coordinate vello emits; wrong handling mis-renders the whole frame | read scale factor, no fractional-scaling polish | Slice 1 |

These four are added to §12 as open question Q8 (the one that blocks freezing the View tree is accessibility —
it must be designed against, not after).

**Stated limit — the demand-driven model assumes human authorship is the bottleneck and human review scales
with it** *(recorded 2026-07-31 as an explicitly expiring assumption, not a hidden one)*. The reasoning above
is a cost argument about *writing*: a multi-year catalog is expensive, so grow it one validated demo at a
time, "each a reviewed addition". That holds only while writing a widget is expensive and reviewing one is
cheap, and while code arrives slowly enough that the spec can lead it. **If widget code becomes cheap to
generate, the bottleneck inverts: per-item human review becomes the throughput limit, and I-15 (spec↔behavior
no drift) turns from a documentation discipline into a rate limit.** This spec does not assume that inversion
has happened; it states the dependency so the model can be retired deliberately rather than silently
overrun. Nothing in the deferral list above is unsound today.

> **One deferral is now externally gated** *(2026-08-01)*. Rich text is blocked on an upstream
> dependency rather than on our sequencing — the first item in this table we cannot unblock by
> deciding to. It is listed with the others because it is a scope boundary, but it must not be
> planned like them: no slice may assume it, and the gate is "AccessKit ships rich-text support",
> checked upstream. Recorded because a deferral that *looks* like a scheduling choice but is
> actually an external dependency is the kind of thing a roadmap silently absorbs and then misses.

**Mechanical widget-conformance contract (obligation, from the first post-v1 widget).** So that catalog growth
is gated by a suite rather than by review bandwidth, any new builder added to the catalog MUST ship with all
of:

1. a declared effect row that satisfies the `builtin_effect_row_agrees_with_impurity` lockstep test — with a
   *named carrier*, since §5 already records that a tag no user-callable builtin carries passes that test
   vacuously;
2. a layout property test: no NaN, no negative sizes, deterministic box-model (§8);
3. a structural golden `View`-tree test (§8's primary oracle), not only a golden PNG;
4. an a11y-node mapping, once the Q8 a11y design exists — a widget with no a11y node is not mergeable after
   that design lands;
5. a purity obligation: the builder satisfies the §6 E2102 closed obligation.

A generated widget change is therefore machine-gated first and human-reviewed second, which is the ordering
that survives volume. Human review remains required; it is no longer the *only* gate.

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
| `View` tree is empty (`col{}`) | renders an empty container at its padded box; no panic (I-9: empty ≠ error, but a zero-size root is a `W21xx` warn — likely a bug) |
| user clicks a `button` | hit-test resolves the node against **the same laid-out tree that was presented** (no re-`app()` between present and hit-test), its `on_click` `Msg` is fed to `update` carrying the content hash of the producing node subtree (§7.1), state replaced, next frame rebuilt. A node fully occluded by a later-painted sibling (z-order, zero-alpha overlay) does **not** receive the click *(occlusion + present/dispatch coherence added 2026-07-31 — UI-redress class, §7.1)* |
| `update` returns the same state | view rebuilt and re-laid-out (cheap); no infinite loop (frame waits on `host_await`) |
| `input` text change | `.on_change(\|t\| Msg)` fires per keystroke with the new buffer; `.bind` reflects state→field. **The buffer is an `Input`-tainted value** (§5) — forwarding it to an exfiltration sink inside `update` is E2105 without an explicit declassification *(added 2026-07-31)* |
| `input` field marked `.secret()` | value is excluded from every frame/`View`-tree record and audit artifact, and enters the info-flow lattice at a raised confidentiality level (§5) — so the §7.1 presented-frame record cannot itself become the leak *(added 2026-07-31)* |
| window resized | layout recomputed against new viewport; content reflows (taffy) |
| window closed | loop breaks; the runtime exits 0; the final `State` is the *runtime's* terminal value (`main` returned once at init — it is not re-entered at close). The process exit value follows the I-8 i64 convention: a clean close is exit 0; the `State` itself is an audit/trace artifact, never the exit code |
| no `Window`/`Gfx` capability (headless/sandbox) | runtime returns coded error, exit non-zero — **never** aborts the host (I-4) |
| long synchronous compute inside `app`/`update` | **bounded, not merely documented** *(hardened 2026-07-31)*: the frame loop arms a watchdog before entering `app`/`update`; exceeding the deadline aborts the in-flight call and surfaces E2107, leaving the host responsive. The draft's "documented hazard, same as any TEA app" was a human-authorship assumption — it holds when a person wrote `update` and will notice the freeze, and fails for machine-generated `app`/`update` at volume. A hang is an I-4 violation regardless of who wrote the code (see §7) |
| `app` produces an unbounded `View` tree (e.g. `for` over an unbounded array) | node-count / depth ceiling: W2112 warn at the soft threshold, **E2106 hard refusal** past the ceiling — fails cleanly instead of exhausting memory. *(Added 2026-07-31: §3a's stopgap for scroll is "clip + non-virtualized scroll (small lists only)" while the headline demo iterates `s.users` unbounded; §10's ~200-node figure is a bench scale, not an enforced limit.)* |
| `Scene3D` (Phase 4) | scene graph → renderer → wgpu; camera/lights/meshes/PBR materials/physics-step/post-FX as named, one frame |
| asset missing (`model("x.glb")` not found) | `Result`-typed load; a missing asset renders a placeholder + `W21xx`, does not crash (I-4) |

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
- **`View` nodes carry STABLE IDENTITY** *(added 2026-08-01; obligation discovered by reading
  Blitz, §3c)*. AccessKit keys its tree on node IDs and assistive technology tracks nodes across
  updates — Blitz uses the DOM node's own id (`NodeId(node.id.as_u64())`). With §12 Q1 resolved as
  *full rebuild per frame*, identity cannot come from allocation: a fresh tree each frame would
  produce a fresh accessible tree each frame, and a screen reader would lose focus and cursor
  position on every repaint. Identity is therefore **derived from structural position** (the path
  of child indices from the root), with an **explicit `.key(k)`** escape for nodes whose position
  moves between frames — the `for u in s.users` case in §3, where reordering or deleting a row
  must not renumber its siblings.
  - This is a type rule, not an implementation detail: a `View` constructor that cannot produce a
    stable id is ill-formed.
  - It also serves §7.1: a canonical hash over a tree whose node identities churn per frame is
    not a stable artifact to sign.
  - Keys are required, not optional, inside a `for` body that can reorder — checked, `E21xx`.
    Absent-key reordering is the classic reconciliation bug, and here it is also an a11y bug.
- Modifier chains (`.font(24).pad(8)`) are postfix methods on `View` returning `View` — structural, no new
  trait machinery; resolved like existing builtin method dispatch.
- `@[ui]`/`@[ui3d]` attributes constrain the annotated `fn`'s signature: `main: fn() -> State` and the file
  must define `app: fn(&State) -> View` and `update: fn(State, Msg) -> State` (checker rule, §6 `E21xx`).
- `on_click(Msg)` / `on_change(|arg| Msg)` require the closure/value to be the file's `Msg` type — a new
  unification obligation (the `Msg` type is inferred from `update`'s second param).
- **`app` purity is a closed, mandatory obligation — not an opt-in effect annotation** *(tightened
  2026-07-31)*. `app` and **every fn transitively reachable from it** must (a) satisfy the empty effect row
  under the landed `infer_effects` transitive fixpoint (`effects.rs`, E1310 machinery — the mechanism
  documented as closing exactly the launder-through-a-helper hole), **and** (b) be `@[pure]`-checkable under
  the existing P05 callee-whitelist (`checker.rs`, E1207 — a `@[pure]` fn may call only annotated-`@[pure]`
  fns, so an *unannotated or unknown* callee is a refusal, not a pass). This obligation applies to a `@[ui]`
  file **whether or not the author wrote any effect row**: §5's own migration note ("un-annotated callers …
  are unaffected — effect checking is opt-in", matching `effects.rs`'s `f.effect_row.is_none() ⇒
  Allowed::Open`) describes the general Phase-6 rule and **does not apply to `app`**, which the checker
  already identifies via `@[ui]`. An un-annotated `app` is checked, not exempted. See §6 E2102.
- **Layout box-model fields carry refinement contracts, not just property tests** *(added 2026-07-31)*: the
  computed `LayoutBox` declares `w: f64 where _ >= 0.0`, `h: f64 where _ >= 0.0`, and non-NaN, so every
  construction site — including one in a generated widget — carries the obligation, and the default-pipeline
  SMT discharge (Phase 5, commit 514e059) statically elides what it can prove; the §8 property test remains
  as the residual runtime check. This is the Z3-free-fallback pattern Phase 5 already blesses, applied to a
  spec that previously stated the same invariant only as a sampling test.
- **`Input` is a taint SOURCE, not only a scheduling/purity tag** *(added 2026-07-31)*. The §3 containment
  split ("side effects only happen in `update` through declared capabilities") plus E2102's `app`-only scope
  means a `@[ui]` app whose `update` carries `| {Net}` or `{AI}` may lawfully forward **every keystroke** —
  passwords, API keys, the text of an approval dialog — to `ai_complete` or an allowlisted host, with every
  check in the draft passing. Therefore: any `Msg` payload produced by `on_change` / `on_click` /
  `host_await*` enters the landed R6 transitive taint analysis as `@[sensitive]`-equivalent, so reaching an
  exfiltration sink (`ai_complete`, `write_file`, net) inside `update` is **E2105** unless the value passes an
  explicit declassification (`trust_validate` / `secret_declassify`, per `examples/stdlib/tainted.ax`, whose
  headline test is exactly "confidential data cannot exfiltrate to an AI/Net sink"). Under a careless author
  this is a latent bug; under a generator optimizing against a gate that never inspects it, it is a free
  channel. An `input()` carrying secrets uses `.secret()` (§4) to raise its lattice level and suppress it from
  frame/audit records.
- Effect rows: UI rendering introduces **two new effect tags — `Gfx`** (window + GPU surface) **and
  `Input`** (event source). *(Corrected 2026-07-31, twice: the draft said `Input` was "reused", but the
  Phase-6 catalog — `builtin_effect_row`, `builtins.rs` — contains only the
  AI/Net/IO/Time/Random/Chan/Hal/Bpf/Tee + goal rows; no `Input` tag exists, and `host_await*` carries no
  Input row. Both tags are new. The first correction mis-enumerated the catalog as "AI/Net/IO/Time/Random +
  goal rows only", dropping Chan/Hal/Bpf/Tee; the load-bearing conclusion — no `Input` tag — stands.)*
  **Each new tag needs a named carrier or it is dead metadata (added 2026-07-31):** the
  `builtin_effect_row_agrees_with_impurity` lockstep test iterates only the `BUILTINS` table, so a tag no
  user-callable builtin carries passes it *vacuously* and no checker path ever sees it. The carriers are:
  - **`Gfx`**: the new `__axon_ui_*` window/surface builtins Slices 1–2 add (window open, present).
  - **`Input`**: `host_await` / `host_await_opt` / `host_await_val` / `host_await_val_opt` are **retagged**
    `is_impure_builtin` = true + row `{Input}`. Today they appear in *neither* table (fall to `_ => &[]`) —
    i.e. the event source this spec's entire §4/§13 loop routes through is currently treated as pure and
    callable from `@[pure]` code with an empty row, a hole this retag closes.
  - **Listed breaking change (the retag's migration cost):** existing R15 interactive programs break in two
    ways — any *annotated* caller of `host_await*` now needs `Input` in its row (E1310 subsumption), and any
    `@[pure]` caller becomes E1207. Migration: update all in-tree `host_await` examples/tests in the same
    commit as the retag, with a dedicated test asserting `builtin_effect_row("host_await") == ["Input"]` and
    the impurity flag. Un-annotated callers (the common case) are unaffected — effect checking is opt-in.

  Each new tag must satisfy the lockstep test (`is_impure_builtin` must agree). `app` is pure (empty row) —
  **by the closed obligation stated above, which is what MAKES it so**; the draft asserted this as a fact with
  no rule behind it *(corrected 2026-07-31)*. The runtime loop carries `{Gfx, Input}`. Composes with Phase-6 effect subsumption (E1310) and
  `@[contained]`; the `Gfx` tag is the effect-row face of the existing R13 `@[contained(gfx: ...)]` grant
  axis (§1a) — one axis, not two (the `gfx:` arm of `contained_effect_row` is R16 work, §1a).

### 6. Error codes

New block **E21xx / W21xx**. *(Renumbered 2026-07-31 — the draft claimed "E16xx, next free range", which was
false: `E1604` is already shipped as the R12b kernel-goal budget-exhausted code with dedicated exit 7
(supervisors branch on it; `cli_run.rs` asserts it), and `error.rs` allocates the E16xx band to R20 kernel
TCB obligations (E1610/E1611) — the draft's W1610/W1611 shadowed those exactly. Same failure class as the
R13 E170x→E18xx move after the R17 collision. E21xx confirmed free — see the spec-meta Reserves line.)*

| Code | Trigger | Message shape |
|---|---|---|
| E2100 | `@[ui]` file missing `app`/`update`/`main` of the required shape | `@[ui] app needs `app(&State)->View`, `update(State,Msg)->State`, `main()->State`; missing: update` |
| E2101 | `on_click`/`on_change` payload is not the file's `Msg` type | `on_click expects Msg `Msg`, found `Foo`` |
| E2102 | **closed obligation** *(respecified 2026-07-31)*: `app`, or any fn transitively reachable from it, fails the empty-effect-row check under the `infer_effects` fixpoint **or** the `@[pure]` P05 callee-whitelist (an unannotated/unknown callee is a refusal). Mandatory for `@[ui]` files regardless of whether the author declared an effect row. `exec`/`write_file`/`ai_complete` are named **only as diagnostic sugar in the message, never as the trigger** — the draft's enumerated-denylist-at-the-immediate-call-site form was the exact laundering class this repo has already fixed three times (R6 taint, `@[contained]`, R4 agent log) and would be defeated by an unlisted builtin (`http_get`/`read_file`/`env_var`/`now_ms`/`random_i64`/`spawn`/`native::*`), by one hop behind a helper, or by simply not annotating | `View builders are pure; move the effect into `update` (effect `Net` reaches `app` via `fetch_rows` → `http_get`; no grant makes an effect legal in `app`)` |
| E2105 | an `Input`-tainted value (`on_change`/`on_click`/`host_await*` payload, §5) reaches an exfiltration sink inside `update` with no explicit declassification | `keystroke buffer flows to `ai_complete` without `trust_validate`; declassify explicitly or mark the field `.secret()`` |
| E2106 | `View` tree exceeds the node-count / depth ceiling (§4) | `app() produced 10485760 nodes (ceiling 65536) — needs virtualization (§3a)` |
| E2107 | frame watchdog: `app`/`update` exceeded the frame deadline and was aborted (I-4 "never hang", §7) | `app() exceeded the 500 ms frame deadline and was aborted; host remains responsive` |
| E2108 | *(reserved, §7.1)* a `@[ui]` app is the host of an R22/R24 approval or capability-grant flow while the trusted-render obligations are unmet | `Axon UI may not render an approval surface (R24) until §7.1 trusted-render obligations land` |
| W2112 | `View` tree exceeds the soft node-count threshold (below the E2106 ceiling) | `app() root has 4200 nodes — consider virtualization before the 65536 ceiling` |
| E2103 | `Scene3D` node used in a 2D `View` context (or vice versa) | `expected View, found Scene3D node `mesh` — use @[ui3d]` |
| E2104 | running a `@[ui]` program when the active host provides **no `GfxHost`** (`--gfx=none`; §13 — a `HeadlessGfxHost` IS a provider and renders offscreen) | `no GfxHost provided (--gfx=none); run with --gfx=headless or a windowing host` |
| W2110 | `View` root lays out to zero area | `app() root has zero size — likely a missing child or pad` |
| W2111 | asset (`model`/`texture`/`heightmap`) not found at load | `asset `char.glb` not found; rendering placeholder` |
| E0910 (reuse) | `@[ui]`/`Scene3D` built with native LLVM codegen (interp-only v1) | `Axon UI is interpreter + windowing-host only; not supported by `axon build`` |

### 7. Invariants touched

**Preserved:**
- **I-1** (pipeline order): block desugar happens at parse; checker learns `@[ui]` rules; no reordering.
- **I-4** (user code never aborts the host): missing assets, missing GPU, closed window all fail as
  `Result`/coded-error/clean-exit, never a panic-abort. *This is the load-bearing invariant for a UI.*
  **Preserved only because of the 2026-07-31 hardening, not as originally drafted:** I-4 reads "never SIGABRT,
  never stack-overflow, **never hang**", while §4 documented an unbounded hang ("blocks the frame (documented
  hazard, same as any TEA app)") as an accepted condition with the mitigation parked in §12 Q3 as an
  *ergonomics* question. Nothing bounded it — §10's 16.6 ms budget is a `criterion` bench on the *framework's*
  layout+encode path, never on user `app`/`update`; §3a's scroll stopgap is "small lists only" against an
  unbounded headline demo; the ~200-node figure is a bench scale. Purity does not imply termination (the
  project has a separate `@[total]` checker, E1208, unused here). The frame loop is also the eventual
  corrigibility surface (§7.1): an app that hangs the loop is an app the operator cannot click Stop in. So
  I-4 is now enforced by the frame watchdog (E2107) and the node/depth ceiling (E2106/W2112), §4 — and §12 Q3
  is reclassified from an ergonomics question to an I-4 obligation blocking Slice 2.
- **I-8/I-9** (success signal): window-close → exit 0; capability-denied/GPU-init-fail → exit non-zero;
  zero-size root → warn not silent success.
- **I-11** (capability boundary is real and total): `Gfx`/`Window` is a capability axis on the same
  footing as net/fs/exec — a sandboxed/headless host denies it; `@[contained]` and effect rows gate it.
  *(Corrected 2026-07-31: not a "new" axis — the R13 `@[contained(gfx: ...)]` grant already exists; this
  spec extends that same axis with the `Gfx` effect-row tag, §1a/§5.)*
- **I-15** (spec↔behavior no drift): this spec is the contract; shipping behavior updates it.

**Changed / extended (this spec doubles as the invariant-change proposal):**
- **I-2** (interpreter is the reference semantics): **amended for non-textual output.** Byte-equal stdout
  parity is undefined for pixels. The reference oracle is restated as: *the interpreter is authoritative for
  the `View`/`Scene3D` tree and its computed layout box-model; pixel rasterization is validated by
  golden-image snapshot at the wgpu layer.* Codegen is interp-only for UI in v1 (E0910), so interp↔codegen
  pixel divergence cannot arise yet. **This amendment is formally proposed + ratified-as-standing-decision in
  `R16a-i2-pixel-parity-amendment.md`** (steps 1–2 of the `ARCHITECTURE_INVARIANTS.md` process: proposal +
  blast-radius enumeration, existing-test impact shown to be zero). Per step 3, the actual edit to the I-2
  line lands **in the same commit as Slice-2 rendering code** — not before.

### 7.1. Threat model — trusted rendering and the approval boundary *(added 2026-07-31)*

`R24-defended-approval-boundary.md` opens with the project's sharpest threat statement: *"Against a
capability-proven box (R20/R21), a superhuman AI's highest-probability escape is not technical — it's
persuading the human approver to grant a capability."* R24 hardens the approval **act** (multi-sig, legible
bounds, tamper-evident provenance) but assumes the display channel is faithful — it contains no treatment of
rendering or spoofing. **R16 owns that channel**, and the draft had no threat model for it.

The concrete exposure:

- §4's dispatch was "hit-test resolves the node, its `on_click` `Msg` is fed to `update`", and §6 E2101 checks
  only that the payload has the file's `Msg` *type*. **Nothing bound a rendered label to the `Msg` it emits.**
- Overlay/z-order and zero-alpha nodes could cover a visible control; §3a defers scroll/clipping and hi-DPI —
  the classic UI-redress vectors.
- §3a states, for the a11y deferral, that "a GPU UI has **no DOM**, so the a11y tree is the framework's job
  and cannot be inherited." The same fact means **there is no independently inspectable record of what was
  displayed to an approver**: in `axon-web` (HTML/JS) an auditor can scrape the DOM; in Axon UI the only
  artifact is pixels, and §8 records pixels into a committed golden, never into the audit chain.
- §12 Q7 parked all of this on the premise that "`axon-web` already satisfies the *current* product-v1
  approval flow" — but **§13 of this same spec argues the Axon OS substrate is real and in-flight** (R17 ~90%
  landed; R21/R36/R37 drafted), **and an Axon OS has no browser to run `axon-web` in.** The premise that keeps
  the approval UI out of scope expires on the same roadmap this spec cites to justify its seam.

The spec already accepts exactly this shape of argument for accessibility (§3a / Q8: design it against the
View tree *before* the tree is frozen, because retrofitting post-freeze is a rewrite). A trustworthy render
record is the same decision, at the same moment, and is now made:

1. **v1 NON-GOAL, enforced.** Axon UI does **not** render any R22/R24 approval or capability-grant surface.
   A `@[ui]` app that hosts an `ApprovalToken` flow is a checked refusal (**E2108**) until obligations 2–4
   land. This is a refusal, not a warning.
2. **Content-hashed `View` tree.** The `View` tree is canonically serializable and content-hashable, and every
   `Msg` dispatched by `dispatch(view, i)` carries the hash of the node subtree that produced it — so an audit
   record can prove *what was on screen at the node the human clicked*, with no pixels involved. (This is the
   same canonical serialization §8's structural oracle and §8's session replay use — one artifact, three
   consumers.)
3. **Present/dispatch coherence + occlusion.** `dispatch` resolves against the same laid-out tree that was
   presented — no re-`app()` between present and hit-test — and a fully occluded control cannot receive the
   click (§4).
4. **Slice-0 design obligation, parallel to the Q8 a11y note.** A one-pager on how the presented-frame record
   chains into the R28 capability audit ledger and the R31 extended-TCB measurement, produced before the View
   representation is frozen. **`axon-ui` becomes TCB the moment it renders an approval**, so this cannot be a
   post-hoc addition. Tracked as §12 Q9.

Note the interaction with §5's `Input` taint rule: the presented-frame record must not itself become the
exfiltration channel — fields marked `.secret()` are excluded from it (§4).

### 7.2. Empirical status of the mechanisms §7.1 depends on *(added 2026-08-01, from the audit of 2026-07-31)*

§7.1 makes Axon UI's central claim conditional on machinery it does not own: the R24 approval
boundary, the R28 capability-audit ledger, and R31 extended-TCB attestation. "What you see is
what was authorized" is only a property if those work. A governance audit executed those paths
rather than reading them, and **most of them did not work.** Recorded here because a threat model
that cites a control which silently no-ops is worse than one that cites nothing — it converts an
absent guarantee into a stated one.

Found broken and **fixed** (see `tasks/todo.md` for commits):

| mechanism §7.1 relies on | observed behaviour | status |
|---|---|---|
| R28 capability-audit ledger | **never written** for any job run under the supervisor — `env_clear` dropped `AXON_AUDIT_LEDGER`. Silent: operator sets it, run succeeds, no ledger. | fixed (T23) |
| sealed verdict record | a program that **failed to parse** sealed as `{"kind":"Completed","value":2}`; an AI-policy refusal (exit 5, a *carved* fault code) sealed as `Completed{value:5}` | fixed (T24) |
| R27 operator kill switch | `--killable --monitor` created no latch that anything polled; `axon-os kill` printed "🛑 kill tripped" and exited 0 against a dead path | fixed (T26) |
| R29 compliance monitor | returned `CleanExit` on the stop flag with **no final ledger read** — a violation committed in the last ~100 ms window was reported as a clean run | fixed (T27) |
| supervised runs generally | any job emitting more than a pipe buffer deadlocked, and the record blamed the job (`Denied{axis:"time"}`) for a program that runs in 0.09 s | fixed (T25) |

Found and **still open** — these bound what §7.1 may currently claim:

- **Carved fault codes other than AI-policy are still indistinguishable from a program's return
  value** in the sealed verdict, because `axon run` propagates `main`'s integer return. Blocked on
  the exit-code semantics decision (needs-human queue, group A). Until resolved, a verdict of
  `Completed{value:N}` for N in 3/4/6/7/8/12 is **not** evidence the run succeeded.
- **`principal_root` is ungated** (O003) — principal attenuation assumes root authority is hard to
  obtain, and it is not.
- **`--killable --monitor` still splits the latch** (O020); only the `kill` side was repaired.
  Whether the two flags should share one latch is a semantics call.

**Consequence for this spec.** §7.1's canonical-hash and approval-binding arguments should not be
written as if the ledger and verdict record are trustworthy substrates — they were not, three
weeks before this spec's threat model was drafted, and nothing in the spec would have detected
that. Two obligations follow:

1. Any §9 acceptance criterion that says "the ledger records X" must **execute** and assert the
   artifact exists and contains X. Not "the code path calls the logger."
2. Axon UI should treat a *missing or unverifiable* provenance record as a **rendering-visible
   state**, not a silent absence. A pane that shows an unverified claim identically to a verified
   one reproduces, in the product surface, exactly the failure this table documents in the
   substrate — and it is the failure mode with the worst blast radius, because it manufactures
   confidence rather than merely losing it.

### 8. Test plan (maps 1:1 to §4)

- [ ] **Unit:** `View` tree construction (`col{}`/`row{}` desugar → expected node tree); modifier chains
      accumulate; `Msg` type-checking of `on_click` payloads.
- [ ] **Unit:** layout engine — taffy box-model assigns expected x/y/w/h for nested col/row/pad/spacer cases
      (this is the deterministic, parity-safe core).
- [ ] **PRIMARY oracle — structural `View`-tree + box-model golden** *(inverted 2026-07-31)*: assert exact
      node kinds, labels, `Msg` bindings, and computed boxes against a committed canonical serialization of
      the tree. §4's parity caveat already names this as "the testable artifact" and §8's property test
      already establishes it is deterministic and seedless — the draft nevertheless gated every slice on the
      *weak* oracle. This one is machine-readable, diffable, reviewable, hashable (§7.1), and legible to every
      other tool the project owns; a pixel is legible to none of them.
- [ ] **SECONDARY oracle — rasterization regression:** full frame on a headless wgpu device → golden-image
      snapshot (`users-list` example); PNG compared against a committed reference with a **stated numeric SSIM
      threshold plus a pixel-difference-region check**, so a localized change (a changed button label, an
      inverted confirm/cancel order, a control moved under the cursor) fails even when global SSIM passes.
      *(Hardened 2026-07-31: "a small SSIM tolerance" is slack, and slack on a mostly-text frame comfortably
      hides exactly the differences that matter for §7.1.)*
- [ ] **Golden provenance (anti-vacuity):** golden references are content-addressed and record who/what
      generated them (the R10 `proposed_by` precedent), and **a golden update is a separate reviewed change
      from the code change that motivates it** — so the same author/generator cannot move code and oracle
      together. *(Added 2026-07-31: this repo has hit the vacuous-gate class twice — the "glob-sweep tests
      must assert passed>0" lesson, and the golden-IR test that checked only a struct's type declaration and
      let a real memory-corruption bug ship for a month.)*
- [ ] **CLI e2e (observable):** `axon run users.ax --gfx=headless --frames=1 --snapshot out.png` exits 0 and
      writes a PNG; `--gfx=none` exits non-zero with E2104.
- [ ] **Adversarial:** missing asset (W2111, placeholder rendered, exit 0); wrong `Msg` payload (E2101);
      `@[ui]` missing `update` (E2100); zero-size root (W2110); GPU-init failure injected → I-4 clean error
      not abort.
- [ ] **Adversarial — purity, a suite not a single case** *(expanded 2026-07-31; the draft's one positive
      test proved only that the denylist fires on the one case it enumerated, and nothing about the
      complement)*: effect one hop behind an un-annotated helper; effect via method call; effect inside a
      `for` / `with` / `spawn` body within `app`; effect via string interpolation; an effectful builtin **not**
      in the `exec`/`write_file`/`ai_complete` trio (`http_get`, `read_file`, `env_var`, `now_ms`,
      `random_i64`, `native::*`); and an **un-annotated `app`** — which must NOT silently pass. All E2102.
- [ ] **Adversarial — info-flow:** a `@[ui]` app whose `update` forwards an `on_change` keystroke buffer to
      `ai_complete` is refused (E2105); the same value declassified via `trust_validate` is accepted; a
      `.secret()` field never appears in the frame/`View`-tree record or any audit artifact.
- [ ] **Adversarial — I-4 bounds:** an `app` that never returns → clean E2107 within the deadline, host still
      responsive (not a hang); an `app` producing 10^7 nodes → clean E2106 (not OOM).
- [ ] **Adversarial — UI redress (§7.1):** a zero-alpha overlay covering a `button` does not steal its click;
      a `Msg` dispatched after a mutated re-`app()` is rejected (present/dispatch coherence); a `@[ui]` app
      hosting an approval flow is refused (E2108).
- [ ] **Property (invariant):** layout is deterministic — same `View` + viewport ⇒ identical box-model
      (seedless), **stated over the canonical serialized tree** so it is the same hash §7.1 and the session
      replay above consume *(2026-07-31)*. Non-NaN / non-negative sizes remain here as the **residual**
      runtime check; the primary enforcement is the §5 refinement contract on `LayoutBox`, which binds every
      construction site (including a generated widget's) rather than sampling.
- [ ] **Parity (interp↔codegen):** N/A for pixels (I-2 amended); **layout box-model parity** interp vs the
      eventual codegen path IS required once Slice 5 codegen exists. v1: interp-only, E0910 guards codegen.
- [ ] **Journey/red-team:** the `users-list` app — render, click Delete (hit-test → Msg → update → reflow),
      type in search (on_change per keystroke), resize (reflow), close (exit 0) — driven by a scripted host
      that feeds synthetic events through `host_await`. **This test IS a recorded `UiEvent` stream** (below),
      not bespoke scaffolding.
- [ ] **Session record/replay** *(added 2026-07-31 — a scoped Slice-2 addition, reusing the seam that already
      exists rather than adding a mechanism)*: `GfxHost::next_event` (§13) is the single funnel for every
      input, so a UI session is structurally a recordable stream. The counterpart infrastructure has already
      landed — every `axon run` stamps a `run_start` provenance event with run-id + seed, `axon trace --replay
      <run-id>` re-executes deterministically, and `AXON_AI_REPLAY` memoizes model calls — and the draft
      referenced none of it, leaving **the UI the one execution mode in the system that leaves no reproducible
      record**, which is backwards for the surface that will host a human decision (§7.1) and for debugging
      generated apps whose failures are input-order-dependent. Specify: a recorded `UiEvent` stream as a
      first-class artifact of any `@[ui]` run (`--record events.json` / `--replay events.json`), tied into the
      Phase-9 run-id/provenance record. **Contract:** replaying the same `(State₀, seed, event stream)`
      reproduces an identical `View`-tree/box-model sequence — which the §8 determinism property already
      guarantees is well-defined. Acceptance criterion on Slice 2 (§9).

### 9. Acceptance criteria (the done gate — per slice)

#### 9.0 Cross-slice obligations *(added 2026-08-01 — from §1d, §3b, §3c, §5, §7.2)*

These bind every slice and are stated once. **Each must EXECUTE and assert an artifact** — not
assert that a code path was called. That distinction is not pedantry here: §7.2 records five
controls in this repo that called their logger, reported success, and produced nothing, silently,
for months. A criterion that cannot fail is not a gate.

**(A) Renderer backend — from §1d**
- [ ] `axon_ui_renders_on_cpu_backend` — every visual/layout criterion below runs on the **software
      backend** in CI, headless, no GPU. This is the primary execution path for §9 on the gate host.
- [ ] `axon_ui_backend_swap_is_exercised` — the same `View` tree renders through **two** backends
      (CPU and GPU) in one test run. §1b's replaceability invariant is otherwise unfalsifiable, and
      given an alpha renderer (§1c) it is the actual risk control.
- [ ] `axon_ui_backend_output_difference_is_bounded` — CPU vs GPU output differs only within a
      documented antialiasing tolerance. Same class as the I-2 native↔interp parity invariant; an
      undocumented divergence is a bug, not a rounding detail.
- [ ] **No visual criterion may be satisfied by a SKIP.** If the GPU path is unavailable on the
      host, the CPU path still runs. (`tasks/opportunities.md` O006: 8 of this repo's harnesses were
      silently skipping and reporting green.)

**(B) Text — from §3b(a)**
- [ ] `axon_ui_text_hash_covers_resolved_fonts` — two runs that resolve **different fonts** for the
      same source string produce **different canonical hashes**. Executed by forcing a font set, not
      argued from the serializer's code.
- [ ] `axon_ui_layout_and_paint_share_one_text_layout` — asserted by **provider-call count**, so a
      regression that re-shapes independently is caught rather than merely being slow.

**(C) Accessibility — from §3b(b), §3c**
- [ ] `axon_ui_unnamed_interactive_node_is_a_compile_error` — a `button` with no derivable
      accessible name fails to compile with the reserved `E21xx` code.
- [ ] `axon_ui_accesskit_tree_round_trips` — for the §3 headline example, every interactive node in
      the `View` tree appears in the exported AccessKit tree with a role and a **non-empty name**.
- [ ] `axon_ui_a11y_tree_is_derived_not_assembled` — the exported tree is produced by walking the
      `View` tree (§3c: Blitz's `build_accessibility_tree`), so the two cannot drift. Asserted
      structurally: no second source of nodes exists.

**(D) Identity — from §5**
- [ ] `axon_ui_node_identity_is_stable_across_rebuild` — a full rebuild with unchanged state
      produces **identical** node ids. With §12 Q1's full-rebuild-per-frame, unstable ids would make
      a screen reader lose focus every repaint, and would make §7.1's canonical hash unsignable.
- [ ] `axon_ui_reordered_list_preserves_keyed_identity` — deleting or reordering a row in the §3
      `for u in s.users` example does **not** renumber its siblings; a missing key in a reorderable
      `for` is `E21xx`.

**(E) Provenance surfacing — from §7.2**
- [ ] `axon_ui_unverified_claim_is_visually_distinct` — a pane rendering a claim whose provenance
      record is **missing or unverifiable** must not render identically to a verified one. §7.2's
      second obligation, and the failure mode with the worst blast radius: it manufactures
      confidence rather than losing it.
- [ ] Any criterion asserting "the ledger records X" opens the artifact and checks it contains X.

**Slice 0 (v0, static frame):**
- [ ] `axon_ui_view_tree_structural_golden` passes — **the primary oracle** (§8): canonical `View`-tree +
      box-model serialization matches a committed, provenance-recorded reference *(added 2026-07-31; a
      golden PNG alone is not sufficient to pass Slice 0)*.
- [ ] `axon_ui_renders_static_view_to_golden_png` passes (one `app(&State)->View` → headless wgpu → PNG
      matches reference; secondary rasterization-regression oracle).
- [ ] `axon_ui_layout_boxmodel_is_deterministic` passes (taffy box-model property test).
- [ ] **Design note (soft gate, parallel to the Q8 a11y note):** the §7.1 one-pager on chaining the
      presented-frame record into R28 (capability audit ledger) and R31 (extended-TCB measurement), produced
      *before* the `View` representation is frozen *(added 2026-07-31 — §12 Q9)*.

**Slice 1 (native window):** *(gate added 2026-07-31 — the per-slice reorganization left Slice 1 with no
criterion at all)*
- [ ] `axon_ui_native_window_presents_one_frame` — a winit window opens and one frame is presented. **CI
      execution story (this repo's gate host has no display server — ENVIRONMENTS.md provisions lavapipe
      offscreen + headless Chrome only):** either run under a provisioned virtual display
      (Xvfb/weston-headless, added to `scripts/setup-environments.sh` in the same slice), or capture via
      `GfxHost::read_pixels` on the surface/swapchain texture — a criterion with no runnable oracle on the
      gate host would land as a permanent SKIP (the qemu_boot stale-skip class) and is not acceptable.

**Slice 2 (v1, interactive 2D — the native 2D done gate):**
- [ ] `axon_ui_users_list_journey` passes (the full click/type/resize/close loop through a scripted
      `host_await` host).
- [ ] `axon_ui_capability_denied_exits_nonzero` passes (E2104 in a headless-no-gfx host).
- [ ] `axon_ui_view_builder_is_pure` passes — **the full §8 adversarial purity suite**, not a single positive
      case: helper-hop, method call, `for`/`with`/`spawn` body, interpolation, unlisted effectful builtin, and
      un-annotated `app`. All E2102 *(expanded 2026-07-31)*.
- [ ] `axon_ui_input_taint_cannot_exfiltrate` passes (E2105: `on_change` buffer → `ai_complete` refused;
      declassified value accepted; `.secret()` field absent from frame/audit records).
- [ ] `axon_ui_frame_is_bounded` passes (E2107 non-returning `app`; E2106 unbounded tree) — the I-4
      obligation, §7. **Blocks Slice 2** (this is §12 Q3, reclassified from ergonomics).
- [ ] `axon_ui_session_record_replay_is_identical` passes (`--record`/`--replay`: same `(State₀, seed, event
      stream)` ⇒ identical `View`-tree/box-model sequence, §8).
- [ ] `axon_ui_refuses_approval_surface` passes (E2108, §7.1).
- [ ] Native (winit window) renders the `users-list` example (golden-image, SSIM ≥ threshold). **Capture
      in CI is texture read-back** (`GfxHost::read_pixels` on the same swapchain/surface texture that was
      presented, §13), *not* a screen-scrape — this removes the display-server dependency so the gate runs
      on the repo's headless gate host *(capture story added 2026-07-31)*.

**Slice 3 (browser 2D — the cross-platform 2D done gate):**
- [ ] Native (winit window) **and** browser (R7c WebGPU canvas) both render the `users-list` example with
      equivalent layout (golden-image per platform, SSIM ≥ threshold). *(Moved here from the Slice-2 gate
      2026-07-31: the draft required the browser render inside Slice 2's gate while §11 delivers the browser
      path in Slice 3 — Slice 2 could never pass its own gate. Slice 2 now gates native-only; the
      native↔browser parity criterion IS still a hard gate, just at the slice that ships the browser path.)*

**Slice 4 (3D core — the 3D done gate):**
- [ ] `axon_ui_3d_lit_animated_scene_golden` passes (camera + dir-light + animated GLB + PBR mesh → reference
      frame).

**Slice 5 (3D physics):**
- [ ] `axon_ui_3d_physics_step_deterministic` passes (fixed-timestep Rapier step is reproducible under seed).
      *(Moved here from the Slice-4 gate 2026-07-31: §11 delivers Rapier in Slice 5; same gate/slice
      mismatch class as the Slice-2/3 fix above.)*

### 10. Performance budget

- Frame budget: **16.6 ms** (60 fps) for the `users-list`-scale example (≤ ~200 nodes) on reference
  hardware; layout ≤ 2 ms, vello encode ≤ 4 ms, GPU submit ≤ 8 ms. Guarded by a `criterion` bench on the
  layout+encode path (the GPU submit is measured but not gated — hardware-variable).
- Binary size: native `users-list` ≤ **6 MB** stripped (PRD claims ~3.5 MB; budget allows headroom for
  Axon runtime). **Slice-5-scoped (corrected 2026-07-31):** this budget is measurable only once the
  optional Slice-5 codegen lowering exists — in interp-only v1 (§6 E0910) there is *no per-app native
  binary to size*; the only runnable artifact is the interpreter CLI with the `ui` feature (wgpu/vello/winit
  compiled in), which is neither a 6 MB artifact nor the user's app. The CI size-check lands **with Slice
  5** and is NOT a v1 gate (an earlier unconditional "guarded in CI" phrasing had no measurable artifact).
- 3D: fixed-timestep physics (Rapier) at 60 Hz must not drift the frame budget; a 1000-particle scene is the
  reference perf target (PRD §3D), benched not gated for v1.

### 11. Rollout & rollback

**Feature-flagged behind `--features ui`** (a superset of, and depending on, the existing R13 `gfx-wgpu`
feature — one wgpu gate, not two; §1a) (drags `wgpu`/`vello`/`winit`/`taffy` — heavy deps the
default interpreter build must not carry; keeps `cargo build -p axon-core --no-default-features` sub-second).
Sliced so each commit is independently revertible:

| Slice | Deliverable | Revertible? |
|---|---|---|
| **0 — v0 static** | `View` type + `col/row/text` desugar + taffy layout + headless wgpu → PNG snapshot (builds on `crates/axon-gfx`, §1a). No event loop, no resume dep. Proves the render path end-to-end. | yes — pure addition behind the flag |
| **1 — native window** | winit window + live frame; still static (no input handling) | yes |
| **2 — interactive 2D** | `host_await` event loop (consumes R15) + hit-test + `update` + input/resize; **plus the 2026-07-31 review additions that are Slice-2-scoped**: frame watchdog + node ceiling (E2107/E2106, the I-4 obligation), `Input` taint enforcement (E2105), `UiEvent` record/replay, and E2108 approval-surface refusal. **The 2D product gate.** | yes — but depends on Slice 1 |
| **3 — browser 2D** | R7c WebGPU canvas + Asyncify event loop; same examples in a tab | yes |
| **4 — 3D core** | `Scene3D` + scene graph + PBR + camera/lights + GLB load (Three.js-subset) | yes — separate module |
| **5 — 3D physics/post/codegen** | Rapier physics, post-FX (bloom/tonemap/fxaa), and (optional) native codegen lowering past E0910 | yes |

**Blast radius:** confined to the `ui` feature; the default build, the CLI verbs, and every existing
test are untouched (the flag is off by default). A `git revert` of any slice leaves the tree building because
the flag gates all of it.

### 12. Open questions

1. **(§5, blocks Slice 2)** Reactive granularity: full re-`app(&State)` per frame (simple, Elm-pure, fine at
   ~200 nodes) vs. fine-grained reactive diffing (signals, à la Leptos/Xilem — faster but a much larger type
   surface). ***RESOLVED 2026-08-01 (§1b): full rebuild for v1.*** Coarse rebuild also keeps the
   `View` tree trivially serializable for §7.1's canonical hash. If §10's bench fails, adopt a
   damage/dirty-region model (Masonry) before reaching for signals.
2. **(§3)** Layout engine: adopt **taffy** (mature flexbox/grid, used by Bevy/Zed) vs. write a minimal
   bespoke one. ***RESOLVED 2026-08-01 (§1b): taffy.*** Ratified by the "no pixel-producing
   component is ours" rule; Blitz and Bevy are the scale evidence.
3. **(§4/§7 — I-4 OBLIGATION, blocks Slice 2; reclassified 2026-07-31 from "hazard/ergonomics")** Main-thread
   blocking: a long synchronous `update` freezes the frame (browser: the tab). **"Document the hazard" is no
   longer an admissible answer** — I-4 says "never hang", and the frame loop is the operator's Stop button
   (§7.1). The bound is settled (watchdog E2107 + node ceiling E2106, §4). What remains open is the
   *ergonomic* half only: run `app`/`update` on the R15 worker substrate and touch GPU only on the main
   thread, vs. an explicit `spawn`-to-background `Msg` path. *Open — needs an R15 interaction review. Either
   answer must sit on top of the watchdog, not replace it.*
4. **(§5/§7, blocks Slice 2)** `Msg` type discovery: inferred from `update`'s signature vs. a declared
   `@[ui(msg: Msg)]`. *Default: inferred from `update`; error E2101 if ambiguous.*
5. **(§7 — RESOLVED as standing decision; APPLICATION gated to Slice 2)** I-2 amendment for pixel output:
   formally proposed + blast-radius-enumerated in `R16a-i2-pixel-parity-amendment.md` (existing-test impact =
   zero; the amendment is additive). The standing decision is ratified; per process step 3 the I-2 line edit
   lands *in the Slice-2 commit*, linking R16a. **Hard gate on Slice 2 merging** = that commit must carry the
   invariant edit; nothing earlier is blocked.
6. **(§3, Phase 4)** 3D scope: how much of the PRD's Three.js/Bevy-scale API (skeletal animation blend trees,
   SSAO, instancing, octree culling, full Rapier joints) is v1 vs. deferred. *Default: a Three.js-*subset* —
   camera/lights/PBR-mesh/GLB/basic-physics — and explicitly defer blend trees, advanced post-FX, and spatial
   acceleration to a later slice. The PRD's full 3D stdlib is a multi-quarter epic on its own.*
7. **(strategic — decision criterion restated 2026-07-31, question still OPEN)** Webview tension: `axon-web`
   (HTML/JS) satisfies the *current* product-v1 approval flow. The draft's criterion — "Axon UI is only
   justified when a customer needs the native binary-size/performance/offline story" — is a
   human-developer-productivity framing that ROADMAP §2.1 does not support and that nothing on the forward
   roadmap strengthens. **Restated trigger for Slices 1–5:** (a) any requirement to render on a substrate with
   **no webview** (the R17/R21/R36 Axon OS track this spec's own §13 cites as real and in-flight), or (b) any
   requirement that **the UI of generated code be subject to the same containment checks as its logic** (§1) —
   an unverifiable HTML/JS surface is a growing liability as generated UI becomes cheap, and no Axon check can
   see it. Binary size stays as a third, secondary trigger. *Still open: which of (a)/(b) fires first, and
   whether Slice 0 alone is the right parking point. Not answered here.*
8. **(§3a, one part BLOCKS freezing the View tree)** Hard deferrals — text/font shaping, accessibility,
   scroll/virtualization, hi-DPI. Three are stopgapped in v1 (§3a table) and grow per slice. **The exception
   is accessibility:** an `accesskit` tree must be *designed against* the View-tree representation *before*
   that representation is frozen in Slice 0, because a GPU UI has no DOM to inherit a11y from and retrofitting
   it post-freeze is a rewrite. *Default: stub shaping/scroll/DPI per the §3a targets; but produce a
   one-page a11y-tree design note against the View model before Slice 0 merges — a soft gate on Slice 0, a
   hard gate on shipping any interactive build.*
9. **(§7.1, one part BLOCKS freezing the View tree; added 2026-07-31)** Trusted rendering and the approval
   boundary. Settled in §7.1: the v1 non-goal (E2108), content-hashed `View` tree with `Msg`↔subtree binding,
   and present/dispatch coherence + occlusion. **Genuinely open:** (a) what exactly the presented-frame record
   contains and how it chains into the R28 capability audit ledger and the R31 extended-TCB measurement —
   `axon-ui` becomes TCB the moment it renders an approval, and the R31 measurement surface would have to grow
   to cover it; (b) whether a hash-of-`View`-subtree is a *sufficient* legibility record for a human approver,
   or whether R24's "legible bounds" requirement demands something stronger than a tree the approver never
   sees; (c) at what point (b) becomes a reason to render approvals in Axon UI *instead of* `axon-web`, since
   the DOM an auditor can scrape today is also a surface the approver cannot verify. *Default: produce the
   §7.1 one-pager before Slice 0 merges (soft gate on Slice 0, per Q8's precedent); E2108 stands until (a)
   and (b) are answered. Do not lift E2108 as a side effect of shipping widgets.*
10. **(§3a, decision-process; added 2026-07-31)** When does the demand-driven catalog model expire? §3a now
   states its dependency explicitly — it assumes writing a widget is expensive and reviewing one is cheap. The
   mechanical conformance contract (§3a) buys headroom but does not answer the question. *Open: what signal
   retires "each a reviewed addition" as the merge gate, and what replaces it. No answer invented here.*

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

    /// Offscreen path (added 2026-07-31 — the Slice-0 gate runs on THIS, not on a window):
    /// create an offscreen render target instead of a window, and read pixels back for the
    /// PNG-snapshot gate. `HeadlessGfxHost` implements these by wrapping `crates/axon-gfx`'s
    /// device/adapter/offscreen-target plumbing (§1a); windowed hosts may also use read-back
    /// of the surface texture for CI golden-image capture (§9).
    fn create_offscreen(&mut self, size: (u32, u32)) -> Result<GfxSurface, GfxError>;
    fn read_pixels(&mut self, surface: &GfxSurface) -> Result<Vec<u8>, GfxError>; // RGBA8
}
```

*(Trait sketch corrected 2026-07-31: the draft's windowed-only shape — `create_surface`/`next_event`/
`present` — could not serve the Slice-0 gate it is binding on from Slice 0: Slice 0 is a headless offscreen
render with pixel read-back and no window/events/present, so a Slice-0 builder would have had to either
invent an off-trait read-back path or violate the no-direct-wgpu constraint. The offscreen methods close
that; `next_event` on a headless host returns `Close` after the requested frame count.)*

**The provider matrix — every platform is one `impl GfxHost`; the framework above is shared:**

| Platform | `GfxHost` impl | Window/surface source | `wgpu` backend | Status / spec |
|---|---|---|---|---|
| Headless / CI (`--gfx=headless`) | `HeadlessGfxHost` | none — offscreen wgpu target with pixel read-back (wraps `crates/axon-gfx`, §1a) | Vulkan (lavapipe on the gate host) | **Slice 0** (this spec) — the host every §8/§9 offscreen/golden test runs on *(row added 2026-07-31; the matrix had no headless row despite Slice 0 running entirely on it)* |
| Linux/macOS/Windows | `WinitGfxHost` | winit | Vulkan / Metal / DX12 | Slices 1–2 (this spec) |
| Browser | `BrowserGfxHost` | HTML `<canvas>` | WebGPU | Slice 3 (extends `R7c-browser-host.md`) |
| iOS / Android | `MobileGfxHost` | UIKit / Android NDK surface (winit can serve both) | Metal / Vulkan | `R14-mobile-targets.md` |
| **Axon OS (in-flight substrate track)** | `AxonOsGfxHost` | the OS's own compositor/surface API | the OS's GPU driver (Vulkan-class, or a native `wgpu` backend) | **out of scope here; this seam is the only hook it needs** — the actual substrate is the R17 bare-metal track (+ R21 axon-os-supervisor, R36 full-asi-os, R37 nano/micro-kernel specs) |

**What Axon OS would owe — and what it gets for free.** Because the seam exists, porting the *entire* UI and
every Axon UI app to a from-scratch Axon OS is **one `impl GfxHost` plus the substrate `wgpu` already
requires** (a Vulkan-class GPU driver, an allocator, a `libstd`-equivalent). Nothing in `axon-ui` above the
trait changes; no application code changes. **Honest scoping (premise corrected 2026-07-31):** the draft
cited ROADMAP §2.3 "the kernel ambition is killed" as the reason an Axon OS was pure future optionality —
that decision was **REVERSED 2026-06-19 (founder decision, ROADMAP.md §2.3)** for the R17 bare-metal track,
which is now ~90% landed (QEMU boot, inline asm, SMP atomics, timer interrupt), with R21
(axon-os-supervisor), R36 (full-asi-os), and R37 (nano/micro kernel) specs drafted in `governance/specs/`.
The `AxonOsGfxHost` row above therefore targets a real in-flight substrate, not a hypothetical — which
*strengthens* the seam argument. The scoping discipline stands unchanged: this spec still does **not**
commit R16 to building the OS, and it does **not** shrink the real cost, which lives *below* the seam (the
GPU driver is one of the hardest parts of any OS, not the UI — none of R17/R21/R36/R37 has one yet). The
value delivered here is **optionality at zero extra cost**: choosing the `AxonHost` seam to serve
Linux/macOS/Windows/browser/mobile *today* is the same choice that makes an Axon OS port a new `impl`
rather than a rewrite *tomorrow*.

**Invariant tie-in:** this keeps **I-11** (the capability boundary is real and total) clean across platforms —
the `Gfx`/`Window` capability (§6 E2104) is granted/denied at the `GfxHost` seam, so a headless or sandboxed
host (including a locked-down Axon OS profile) refuses graphics uniformly, by *not* providing a `GfxHost`,
with no per-platform special-casing.

**Acceptance (added to §9, Slice 0):**
- [ ] `axon_ui_gfx_goes_through_host_seam` — a static check / test asserts `axon-ui` makes **no direct `winit`
      or `wgpu::Surface` call**; all windowing/surface/input route through `GfxHost`. (Guards the constraint
      from regressing as later slices land.)
- [ ] `axon_ui_no_gfxhost_denies_cleanly` — a host that provides **no `GfxHost` at all** (`--gfx=none`)
      yields E2104, exit non-zero (I-4/I-11), proving the seam is the sole graphics entry point.
      *(Renamed + clarified 2026-07-31: the old name `axon_ui_headless_host_has_no_gfxhost_denies_cleanly`
      conflated headless with denial while §8 requires `--gfx=headless` to render a PNG and exit 0. The
      distinction is now explicit in the seam design, not just CLI flags: **E2104 fires when no `GfxHost` is
      provided**; a `HeadlessGfxHost` (`--gfx=headless`) is a real provider and renders offscreen.)*
