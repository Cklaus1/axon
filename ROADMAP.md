# Axon Roadmap

**Last updated**: 2026-05-28
**Companion docs**: `STATUS.md` (shipped state), `CLAUDE.md` (project conventions),
`spec/compiler-phaseN.md` (per-phase specs).

---

## 0. The Three Load-Bearing Pillars

Every later decision in this roadmap reduces to one of these:

1. **Proof** — refinement types + SMT, with runtime fallback. Earliest enforcement wins.
2. **Containment** — row-polymorphic effects + capability-bound principals. Pure by default.
3. **Goal-directedness** — search as control flow. `for! maximize` is not sugar; it's the irreducible Axon move.

If a feature does not advance one of these, it is not in scope.

---

## 1. Current State (Shipped)

| Layer | Phase | Status |
|---|---|---|
| Compiler | 1–4 | Lexer → Parser → Resolver → Infer (HM) → Checker → Borrow → Mono → Codegen → LLVM, plus LSP / fmt / doc / cache / cross-compile |
| ASI Layer 1 | shipped | `Uncertain<T>`, `Temporal<T>` types, runtime + builtins |
| ASI Layer 2 | shipped | Uncertain propagation through binary operators |
| ASI Layer 3 | shipped | `@[verify]` runtime check via `__axon_verify_panic`; `ai_complete`, `ai_extract::<T>`, `ai_extract_uncertain_{i64,f64}` |
| ASI Layer 3.5 | shipped | Runtime lattice value for AI-sourced predicates |
| ASI Layer 3.6 | shipped | `uncertain_dyn_{i64,f64}` runtime-classified constructors |
| `@[adaptive]` / `@[goal]` | shipped | Provenance log + hill-climb `goal_run` |
| `@[contained]` | shipped | Capability permissions (E1001–E1004) |

Detail: `STATUS.md`.

---

## 2. Strategic Reframe (decisions locked this cycle)

These are the commitments derived from the design-feedback rounds in 2026-Q2. They override
any earlier framing in spec or status docs.

### 2.1 Two-track product, single codebase

```
Axon ≠ "a language"
Axon = (Axon Language) + (Axon Userland OS) + (Axon Surface UX)
```

- **Axon Language** is the typed substrate. Going forward, treat it as an **IR**, not a
  human-authored surface. Optimize for unambiguous audit, machine generation, and stable
  spec — not human ergonomics.
- **Axon Userland OS** is a daemon + library that runs on Linux/macOS. No kernel-level
  ambition. The OS-ness comes from its abstractions (agent / goal / principal / effect /
  store / proof), not its privilege level.
- **Axon Surface UX** is the user-facing layer. Structured-prose forms compiled into typed
  Axon AST by an LLM-driven compiler, with mandatory user approval of the resolved AST.

### 2.2 Co-evolution is mandatory

Language, runtime, and surface evolve together. There is no "language v1, then runtime v1."
Every Phase below ships *all three layers* of its scope.

### 2.3 The kernel ambition is killed

No syscall replacement, no process-model replacement, no Linux fork. Userland OS forever
(or until forced otherwise by a real customer requirement).

### 2.4 The typed AST — not the English — is the legal/audit artifact

Users author intent; Axon's LLM-compiler proposes a typed AST; users **must approve the
AST** before it runs. The English is a comment; the AST is the contract. Bug reports,
audits, and rollbacks reference the AST.

### 2.5 CLI is engineering v1; web UI is product v1

CLI ships first because it forces the data model honest, gives replay/audit/scripting
free, and is testable. Web UI is a thin shell over the CLI commands once the API has
stabilized — every UI button is `axon foo --json`. The CLI is **not** the answer to the
"non-programmer in 10 minutes" pivot; that acid test waits for product v1.

### 2.6 The stdlib defines the paradigm, not the syntax

If the stdlib is `Vec / HashMap / Iterator`, we shipped Rust. The Axon stdlib must be
`Goal / Constraint / Budget / Principal / Agent / World / Belief / Plan / Trace /
Feedback / Reward / Schedule / Tool / Audit`. The stdlib is what cements the paradigm.

### 2.7 Examples define the language identity

The current `examples/` (fibonacci, factorial, GCD) tells the world we shipped a typed
scripting language. Replace with: optimization loops, multi-agent dialogues, world-model
simulations, AI-extraction pipelines, budget-bounded planners, simulate→redteam→deploy
workflows.

---

## 3. Architecture (final)

```
                      ┌────────────────────────────────────┐
                      │  Surface UX  (product v1)          │
                      │   structured-prose forms,          │
                      │   English-rendered fields,         │
                      │   approve typed AST,               │
                      │   reasoning trace viewer           │
                      └─────────────┬──────────────────────┘
                                    │  English → LLM-compiler →
                                    │  typed AST (user-approved)
                                    ▼
                      ┌────────────────────────────────────┐
                      │  Surface .ax  (high-level)         │
                      │   goal / agent / for! / search /   │
                      │   simulate / verify / deploy       │
                      └─────────────┬──────────────────────┘
                                    │  desugars to
                                    ▼
                      ┌────────────────────────────────────┐
                      │  Substrate .ax  (IR; library)      │
                      │   refinement types / effect rows / │
                      │   principals / stores / SMT-prove  │
                      └─────────────┬──────────────────────┘
                                    │  LLVM via inkwell + axon-rt
                                    ▼
                      ┌────────────────────────────────────┐
                      │  Userland OS Runtime (engineering v1)│
                      │   scheduler / store / supervision /│
                      │   LLM gateway / replay / audit /   │
                      │   sandbox / cost meter             │
                      └────────────────────────────────────┘
```

**Substrate vs. surface enforcement**: surface files declare `surface` at the top; the
checker forbids `unsafe`, raw effect rows, manual memory, untyped FFI. Substrate files
declare `substrate`; everything is allowed but the checker warns on goal/agent/`for!`
constructs (those belong on the surface).

---

## 4. Phase Order (corrected)

The previous "Phase 1 = language, Phase 2 = runtime, Phase 3 = kernel, Phase 4 = OS"
ordering is **dropped**. Replaced by:

| Phase | Layer focus | Headline deliverable | Exit criteria |
|---|---|---|---|
| **5** | Substrate | Refinement types + SMT (Z3) | `divide(n, d: i64 where _ != 0)` is statically proved at every call site; `@[verify]` on a `Known`-source Uncertain emits no runtime check. Spec: `spec/compiler-phase5.md` (drafted). |
| **6** | Substrate | Row-polymorphic effects + handlers | `fn f() -> T \| {IO, Net}` syntax; `pure` is the empty row; effect handlers compose; `@[contained]` becomes an effect-row constraint (deprecates the attribute). |
| **7** | Runtime | `Principal`, `Store<T>`, `Supervisor`, `LLM<Capabilities>` as runtime services | A `Principal` can mint a sub-Principal with a strict subset of capabilities + budget; `Store<T, Consistency, Lifetime>` ships with at-least-once and linearizable variants; `Supervisor` runs OTP-style trees (one_for_one / one_for_all / rest_for_one + backoff); `LLM<Caps>` mediates every AI call with budget metering. |
| **8** | Surface | `goal` / `agent` / `for!` / `search` keywords + stdlib | `goal { metric, constraints, budget, principal }` parses, type-checks, runs; `for!<MCTS> maximize x with proposer { ... }` desugars into substrate calls; the 12 Tier-1 stdlib types ship in `std::asi::*`. |
| **9** | Runtime | Replay, audit, sandbox | Every run produces a `(Trace, Seed)` pair that re-executes deterministically; every effect produces a typed `AuditEvent`; `Sandbox<P>` enforces effect rows at runtime for AI-generated tool execution. |
| **10** | Surface UX | Structured-prose surface + LLM-compiler + CLI | `axon intent compile`, `axon ast review`, `axon ast approve`, `axon goal run`, `axon trace replay`, `axon redteam`, `axon deploy`. Every command emits stable JSON. The Hello Goal target spec compiles + runs end-to-end. |
| **11** | Userland OS scope | Risk typing + simulation pipeline gate | `Risk` is *derived* from effect rows + budget magnitude + irreversibility annotation (users may raise, not lower); `Risk ≥ High` automatically triggers `simulate → stress → redteam → verify → deploy`; rejection at any gate halts the deploy. |
| **12** | Surface UX | Web UI as thin shell over CLI | Multi-pane approval flow (intent / AST diff / constraint resolution / predicted impact / simulator output / redteam output / deploy gate / post-deploy metrics). Backed entirely by Phase-10 CLI commands. |
| **13** | Probabilistic | Probabilistic refinement fragment | Refinements over `E[_]`, `Var[_]`, `P(_ ≤ k)` for tractable distributions (Gaussian, Beta, Categorical), discharged via interval arithmetic. Distribution<T> + `observe`/`condition`/`sample` keywords. |
| **14+** | Multi-agent | Distributed types + consensus | `Replicated<T>`, `Quorum<T>`, `CRDT<T>` as Tier-2 stdlib (Store constructors). Causal ordering as type. Cross-Principal goal coordination. |

Phases 5–8 are sequential (each depends on the prior). Phases 9–10 can run in parallel
to 8 once 7 has shipped runtime primitives. Phases 11–12 require 9 and 10. Phase 13 is
independent and can start any time after 5.

---

## 5. The Forcing Function: "Hello Goal"

Adopted as the integration test for whether Axon is real. Three artifacts produced as a
single deliverable in Phase 10:

1. **English intent** (≤ 30 lines, free prose with named sections):
   - one-sentence goal
   - constraints (free text)
   - budget (structured)
   - agents (named)
   - tools (named)

2. **Compiled typed AST** showing how each English fragment resolved to typed
   constructs, with confidence/uncertainty markers on fuzzy resolutions and explicit
   user-approval markers on each.

3. **CLI session transcript** demonstrating the full loop:
   ```
   axon intent compile signup.intent.md
   axon ast review signup.ax
   axon ast approve signup.ax            # user signs the AST
   axon goal run signup.ax
   axon trace show <run-id>
   axon goal improve <run-id>            # one improvement cycle
   axon redteam <plan-id>                # one safety catch
   axon deploy signup.ax --gate verify
   axon trace replay <run-id>            # deterministic replay
   ```

   Plus failure-mode transcripts: budget exhausts, constraint resolution unconfirmable,
   simulator/prod disagreement, LLM-compiler returns low-confidence AST.

When this works end-to-end, Axon is real.

---

## 6. Stdlib (Tier 1, shipped in `std::asi::*` by Phase 8)

| Type | Purpose | Tier |
|---|---|---|
| `Goal<M>` | metric M, constraints, budget, principal | 1 |
| `Constraint` | `Invariant(pred)` \| `Forbidden(pred)` | 1 |
| `Budget<R...>` | extensible cost over resource set R | 1 |
| `Principal` | id, parent, capabilities, audit_log, mintable_subset | 1 |
| `Effect` | row-polymorphic effect tag | 1 |
| `Refinement` | `T where pred` | 1 (kernel-level via Phase 5) |
| `Source` | `Constant \| User \| AI \| Net \| System` | 1 |
| `Store<T, C, L>` | typed persistent state | 1 |
| `Supervisor` | OTP tree + strategy | 1 |
| `Agent` | input / output / tools / policy / effects | 1 |
| `Tool` | typed callable with effect signature | 1 |
| `LLM<Caps>` | model + budget + fallback | 1 |
| `Trace` | replayable execution record | 1 |
| `AuditEvent` | typed effect record | 1 |
| `Sandbox<P>` | runtime effect-row enforcement | 1.5 |
| `Distribution<T>`, `Belief<T>` | probabilistic types | 2 (Phase 13) |
| `World<T>`, `Counterfactual<T>` | simulation primitives | 2 |
| `Replicated<T>`, `Quorum<T>`, `CRDT<T>` | distributed Store constructors | 2 (Phase 14) |
| `Reward<T>` | signed Metric alias | 2 |
| `Plan<T>` | Agent.propose() output | 2 |
| `Schedule` | when goals run | 2 |
| `Tainted<T>`, `Trusted<T>`, `Secret<T>`, `Public<T>` | info-flow lattice | 2 |
| `Feedback<G, M>` | typed improvement-loop output | 2 |

Detail spec: `spec/stdlib-asi.md` (to be drafted alongside Phase 8).

---

## 7. Trusted Computing Base (TCB)

Components users must trust unconditionally. Self-modification cannot weaken any of them.

```
TCB = {
    refinement_checker,        // Phase 5
    smt_solver,                // Phase 5 (Z3)
    effect_enforcer,           // Phase 6
    principal_authority,       // Phase 7
    capability_minter,         // Phase 7
    cost_meter,                // Phase 7
    scheduler,                 // Phase 7
    supervisor_root,           // Phase 7
    llm_gateway,               // Phase 7
    sandbox_enforcer,          // Phase 9
    audit_log_writer,          // Phase 9
    replay_engine,             // Phase 9
    risk_classifier,           // Phase 11
    deploy_gate                // Phase 11
}
```

**Attestation**: TCB is content-addressed at boot; mismatch with signed manifest = boot
fail. Update path: multi-sig of root Principals, full audit. Drawn-explicit threat-model
boundary: TCB stops at the Axon process; we trust the host kernel and hypervisor.

---

## 7.5 Compile-time Engineering Followups

These don't change semantics but are blocking developer ergonomics and CI throughput.

### Split `crates/axon-core/src/codegen.rs` into a module hierarchy

**Symptom**: full debug build of `axon-core` takes hours (observed 6h on WSL2; 25–30 min
on the canonical dev machine per CLAUDE.md). The bottleneck is rustc trait-monomorphization
on `inkwell`'s heavily-generic API across 8000+ lines in a single `impl Codegen<'ctx>`
block.

**Fix**: split into 8 modules. Target sizes from current line counts:

```
crates/axon-core/src/codegen/
├── mod.rs           ~600 lines  — Codegen struct, emit_program, declarations
├── types.rs         ~600 lines  — Axon type → LLVM type lowering
├── expr.rs        ~2,000 lines  — expressions: BinOp, Call, MethodCall, MatchExpr, FmtStr
├── stmt.rs        ~1,200 lines  — Let, Assign, While, For, Break, Continue
├── builtins.rs    ~1,000 lines  — emit_call dispatch for every builtin
├── asi.rs           ~800 lines  — adaptive registry, verify panic, uncertain helpers
├── runtime.rs       ~400 lines  — declare_builtins (LLVM declarations of runtime symbols)
└── link.rs          ~700 lines  — emit_object_and_link, build_axon_rt, build_axon_ai
```

**Rules**:
- Don't go below ~400 lines/module (over-fragmentation pays coordination tax — every helper becomes `pub(super)`)
- Don't go above ~2000 lines/module (back to monolithic; trait-cache misses dominate)
- The hardest split is `expr.rs` ↔ `builtins.rs` because `Expr::Call` dispatches into builtin emission; introduce a `pub(super) trait BuiltinEmit` or similar shared interface
- `String interpolation` (FmtStr) must live in `expr.rs` (recursive into Expr codegen)

**Expected speedup** — and what kind of "parallel":
- Clean build: **1.5x–3x** from better trait-cache locality + better CGU partitioning. *Not* from parallelism — rustc's front end (parse → trait solve → monomorphization collection) is **single-threaded** in stable rustc today, regardless of module structure.
- Incremental rebuild after touching one module: **50x–100x** — incremental cache is per-codegen-unit; touching one module only invalidates its CGU. **This is the real win.**

**Why module split alone doesn't unlock front-end parallelism**: the trait solver and monomorphization collector run on the whole crate as a single compilation unit. Modules are a namespace tool, not a parallelism boundary. The only intra-crate parallelism rustc provides today is the LLVM codegen-units phase (already at 16), which runs *after* the slow phase finishes.

### Stretch: split codegen further into separate **crates**

If module-split alone leaves clean-build > 15 min on canonical hardware, escalate:

```
crates/
├── axon-codegen-types       — type lowering (Axon → LLVM)
├── axon-codegen-runtime     — __axon_* ABI declarations
├── axon-codegen-expr        — expressions; depends on types + runtime
├── axon-codegen-stmt        — statements; depends on types + runtime
├── axon-codegen-builtins    — emit_call dispatch; depends on types + runtime
├── axon-codegen-asi         — adaptive/verify/uncertain; depends on types + runtime
├── axon-codegen-link        — emit_object_and_link
└── axon-codegen             — thin re-export façade for axon-core
```

cargo builds expr/stmt/builtins/asi in **parallel rustc invocations** (real OS-level
parallelism), each with a smaller trait-solving surface. Expected: 6h → ~30 min on
WSL2. Risk: forces a public API between every pair; ~3x the engineering effort of
module split.

### Stretch²: shim inkwell behind a monomorphic IR trait

If module-split + crate-split still leaves clean-build > 15 min, or if
backend-portability becomes a strategic requirement (MLIR / cranelift / WASM-direct):

```rust
trait IR {
    fn add(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn icmp_eq(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn phi(&mut self, ty: IRType, incoming: &[(IRValue, IRBlock)]) -> IRValue;
    // ~200 more methods
}
struct IRValue(u32); struct IRType(u32); struct IRBlock(u32);  // arena handles

struct InkwellBackend<'ctx> { /* keeps inkwell types in a Vec, indexed by handles */ }
impl<'ctx> IR for InkwellBackend<'ctx> { /* the only place 'ctx leaks */ }
```

Codegen.rs becomes monomorphic over `IRValue`/`IRType`/`IRBlock`. The trait
explosion is bounded to one impl block.

**Lifetime-erasure pattern**: arena with index handles (cranelift-codegen does this);
*not* `Arc<Context>` (requires unsafe `'static` transmutes; one mistake = UAF).

**Effort**: a few ASI iteration cycles to design + emit the shim trait + ~150–250
method wrappers + tests + migration of every codegen call site.  Bounded by
solver iteration count (one cycle to draft the shim, one cycle to migrate
callers, one cycle to validate against the existing test suite per backend).
Coordinates poorly with in-flight Phase-5/6 codegen churn — the merge
conflicts grow with every concurrent edit to codegen.

**Speedup**: plausibly 5–20x on the front end. Even bigger:
- Backend optionality: swap LLVM ↔ cranelift ↔ MLIR by writing a second `impl IR`
- Testability: mock `IR` for unit tests; today most codegen logic requires a live LLVM context

**When this is right**: pick the shim if **two or more** are true:
1. Axon committed for the long horizon (substrate stability matters more than per-feature speed)
2. MLIR / cranelift / WASM-direct is a real (not slogan) goal
3. Crate-split insufficient
4. ASI iteration budget for a multi-cycle refactor is available
5. No major Phase-5/6 codegen work in flight (merge-conflict risk)

**When it's wrong**: skip if codegen will likely move to MLIR anyway in near future
(sunk cost), or if crate-split brings build under 10 min (good-enough).

**Risk for module split**: bounded — a single ASI iteration cycle for the
file-move pattern (one file at a time, validated with `rustfmt --check` per
move) plus one validation cycle running the full `cargo build -p axon-core`
on canonical hardware to catch any visibility cascade.  Schedule on a
dedicated branch before Phase 5 implementation begins so the Phase-5 churn
happens in the new structure.

**Acceptance**: `cargo build -p axon-core` completes in ≤ 10 min on the canonical dev
machine; `cargo build` after editing only `codegen/expr.rs` completes in ≤ 60s.

**Progress as of 2026-05-04** (commits `62bcc6f` + `fbab931` + this):

| Phase | Module | Lines | Status |
|---|---|---|---|
| 1   | `codegen/link.rs`         |   281 | ✅ landed (62bcc6f) |
| 2.1 | `codegen/types.rs`        |   210 | ✅ landed (fbab931) |
| 2.2 | `codegen/asi.rs` (initial) |   279 | ✅ landed (fbab931) |
| 2.3 | `codegen/asi.rs` (+ `emit_binop_uncertain`) | total 391 | ✅ landed (this) |
| 2.4 | `codegen/match_pat.rs`     |   489 | ✅ landed (this) |
| 2.5 | `codegen/option_result.rs` |   224 | ✅ landed (this) |
| 2.6 | `codegen/output.rs`        |   121 | ✅ landed (this) |
| 2.7 | `codegen/expr.rs` (file-move only, no decomposition) | 1,759 | ✅ landed (this) |
| 2.8 | `codegen/builtins.rs` (file-move only, no decomposition) | 3,904 | ✅ landed (this) |

**Final state of `codegen/mod.rs`**: 8,135 → 984 lines (**88 % reduction**).  The
remaining mod.rs holds the Codegen struct definition, the constructor `new()`,
the orchestration methods (`declare_functions`, `emit_program`, `emit_vtable_*`,
`emit_fn`), and four internal helpers (`axon_type_to_semantic`,
`llvm_type_from_axon`, `value_type_hint`, `infer_expr_sem_type`,
`sem_type_of_expr`).

**Validation status**: every file passes `rustfmt --check` (parses cleanly).
The non-codegen-feature build (`/tmp/axon-check`) rebuilds clean and all 6
ASI demos still type-check.  **Full type+visibility validation requires
`cargo build -p axon-core` with the codegen feature** — deferred to a faster
machine because the inkwell trait-monomorphization build is pathologically
slow on the current WSL2 box (reached 6h+ before kill).

**Predictable failure modes when re-built on canonical hardware**:
1. Unused-import warnings in mod.rs (`Path`, `Command`, `inkwell::targets::*`,
   `OptimizationLevel`) — non-fatal.
2. Possible visibility cascades: if `emit_expr` (now `pub(super)` in expr.rs)
   needs to call back into something private in mod.rs, it'll surface as a
   "private field/method" diagnostic — mechanically fixable in minutes.
3. `declare_builtins` was a single 3,870-line method; the move to
   `builtins.rs` doesn't decompose it, so trait-cache hits within that method
   are unchanged.  The expected speedup comes from incremental rebuild
   (changing one of the 8 sibling files leaves the other 7 cached).

**Phase 3 status (landed in commits a9bcf87 + d1d9e25)**:
* 3.1: `emit_expr` (1,380 LoC giant method) → 15 per-Expr-variant helper methods
* 3.2: `declare_builtins` (3,870 LoC giant method) → 4 per-section helper methods

**Empirical finding from validation (2026-05-05)**: Phase 3 decomposition
delivered a measurable but **insufficient** improvement.  Compared to the
pre-decomposition pathological build (VmPeak 3.6 GB, 9h+ never finished):
* Phase 3 build reached LLVM codegen (**272 object files emitted** vs. 0).
* Early-phase VmPeak dropped to 1.59 GB (60% reduction).
* But the build then stalled mid-codegen: VmPeak climbed back to 3.1 GB,
  CPU sustained at 99% on one thread for 5h+, no further object file
  output.  Killed at the 5h 30m mark.

**Diagnosis**: the inkwell trait-explosion happens **per codegen unit
during LLVM IR construction**, not just during front-end monomorphization.
Module split (Phase 2) + method decomposition (Phase 3) reduced
front-end load substantially, but the per-CGU LLVM lowering still hits
the same wall when each CGU's share of inkwell calls is monomorphized
into IR.

**The real fix**: the inkwell shim ("Stretch²") below.  Wrap inkwell
behind a monomorphic IR trait so codegen no longer feeds inkwell's
generic API into LLVM lowering.  Phase 2 + Phase 3 are the *substrate*
the shim will sit on; the shim is what unlocks fast clean builds.

**Practical implication for ongoing work**: until the shim lands, the
parallel `/tmp/axon-check` tool (no-default-features, no LLVM) remains
the only way to validate `.ax` sources end-to-end.  Phase 5/6+ work can
proceed against `axon-check` validation alone; full `axon run` testing
is gated on the shim.

**Update 2026-05-05 — `-Z parallel-frontend` empirically tested and rejected**:
Tried nightly rustc with `RUSTFLAGS="-Z threads=8"` + Phase 2/3 splits +
`RUST_MIN_STACK=16777216`.  Build ran 4h 35m wall clock with 11 threads
spawned but **only one thread doing work** the entire time (LWP burned
4h 14m CPU; the other 7 worker threads at 0:00 throughout).  Same
single-threaded pathology, same memory growth (VmPeak 3.5 GB), 1 object
file emitted.

This confirms: nightly's parallel frontend either (a) doesn't parallelize
the specific trait queries that inkwell triggers, or (b) serializes on a
global lock somewhere.  Either way, throwing more cores at the same
workload is not the fix.

**Conclusion**: the inkwell shim ("Stretch²" above) is the only remaining
lever.  Module split + method decomposition + parallel frontend, all
combined, fail to bring this build into reasonable time.  The shim
fundamentally bounds inkwell's generic surface to one impl block;
codegen.rs becomes monomorphic over plain handles.  This is the answer.

**Recommendation for future ASI session**: schedule the shim refactor on
canonical hardware where each method-batch can be validated by a fast
build cycle.  Migration plan: define `trait IR` with arena handle types
(`IRValue(u32)`, `IRType(u32)`, `IRBlock(u32)`); implement
`InkwellBackend<'ctx>` as the *single* place inkwell appears; migrate
codegen call sites in batches grouped by file (asi.rs, option_result.rs,
types.rs are smallest, do first); cargo build after each batch to
isolate failures.  The Phase 2 + Phase 3 work already done is the right
substrate — files are split and methods are bounded — so each migration
batch is self-contained.

---

## 8. De-prioritized / Out of Scope

Documented here so it stops resurfacing.

- **Linux/kernel replacement** — graveyard. Userland OS only.
- **WASM/JS targets** — secondary; revisit after Phase 12.
- **Package registry / dependency manager** — fold into Phase 10 if needed for stdlib distribution.
- **Async/await syntax** — channels + agents cover the use cases.
- **Generic `assert_eq<T: Eq>`** — Option-B typed variants ship; full generic deferred indefinitely.
- **Universally quantified refinements over generics** — Phase 5 only handles concrete instantiations; `forall T` over closed primitive sets only.
- **Mutual recursion in `@[total]` functions** — Phase 5 handles direct recursion; mutual deferred.
- **Full probabilistic SMT** — Phase 13 ships a *fragment* (closed-form moments), not general probabilistic SMT.
- **Custom hardware target** — never.

---

## 9. Open Questions Still to Resolve (decision needed before phase opens)

| Question | Phase blocked | Default if undecided |
|---|---|---|
| Search-strategy syntax: `for!<MCTS>` vs. `search { strategy: MCTS, ... }` | 8 | Type parameter on `for!` |
| Probabilistic refinement fragment: closed-form moments vs. summary abstraction | 13 | Closed-form moments (Gaussian/Beta/Categorical) |
| Constraint resolution UX: free predicate edit vs. canonical-constraint library | 10 | Library of canonical predicates + free-form fallback with explicit confidence marker |
| LLM-compiler determinism: lock-on-approve vs. re-confirm-on-LLM-upgrade | 10 | Lock-on-approve; LLM upgrade = explicit re-confirmation event |
| Substrate/surface enforcement: file-level marker vs. per-item attribute | 6 | File-level marker (`surface` / `substrate` declaration at top) |
| Memory-tier story: parameterized `Store<T, …>` vs. specialized `EpisodicStore` / `SemanticStore` / `WorkingStore` | 7 | Parameterized; specializations ship as type aliases |
| Reward + Constraint composition: hard / soft / lexicographic | 8 | Hard by default; `@[soft(weight: λ)]` for soft; constraint priority via list order |
| Risk classification source: declared vs. derived | 11 | Derived (effect rows + budget + irreversibility); users may raise, not lower |
| First-customer profile | 10 | Research lab building agent products with hard safety constraints |
| Pricing/economics model | 10 | Margin on user LLM spend + per-goal-compile + per-deploy fee |

Each row gets resolved in writing before the relevant phase opens, with the decision
recorded in that phase's spec doc.

---

## 9.5 Friction-derived Gap List (from `examples/asi/optimize.ax`)

The first end-to-end ASI demo (`examples/asi/`) was built against shipped primitives
only. Each gap below is a concrete missing affordance encountered during the build —
prioritized as input to the Phase 5–10 schedule. Replace any conflicting handwave in
§9 with these entries when they overlap.

### Hard (block real use today)

| # | Gap | Phase target | Notes |
|---|---|---|---|
| F1 | `goal_run` only live-hill-climbs `fn(i64) -> i64`; every other signature falls back to retrospective best-observed lookup | 8 | Generalize search via strategy parameter (`for!<HillClimb>` over arbitrary domain). Today's encoding (variant catalog → integer index) is awkward for continuous params and string inputs. |
| F2 | `ai_complete` is non-deterministic; `axon trace replay` cannot reproduce a run | 9 | Without this, every "auditable" claim is weaker than advertised. Replay engine + seed capture + LLM-call memoization. |
| F3 | Provenance log is flat NDJSON `(fn_name, score)` — no Principal, no effect row, no causal link to the goal that triggered the call | 7 + 9 | Typed `AuditEvent` per effect; Principal-tagged; queryable as a stream. |
| F4 | No budget meter — token cost per `ai_complete` call is invisible; `max_evals` is a poor proxy | 7 | `LLM<Capabilities>` mediates every call, ticks `Budget<R...>`, halts on overrun. |
| F5 | No runtime sandbox for AI-emitted plans — `@[contained]` is static only | 9 | `Sandbox<P>` enforces effect rows at runtime for tool execution. |

### Soft (workable today, ergonomically wrong)

| # | Gap | Phase target | Notes |
|---|---|---|---|
| F6 | `@[verify]` predicate language is `confidence OP K` only — cannot express `value >= 0 AND confidence >= 0.9` or relations between two Uncertain values | 5 | Refinement types generalize the predicate language; trivial extension once Phase 5 lands. |
| F7 | ~~No string→digit-only filter builtin~~ — **closed** by `str_digits_only(s) -> str` (interpreter). `parse_int(str_digits_only("415-555-0142"))` → `Ok(4155550142)`. | stdlib | Done. |
| F8 | No `as` cast operator — must use `f64_to_i64` / `i64_to_f64` builtins | language | Stylistic only; not load-bearing. Defer. |
| F9 | `@[adaptive]` only single-arg `fn(i64) -> i64` is eligible for live hill-climb; multi-arg adaptive fns silently fall back | 8 | Generalize alongside F1 to multi-dim domain. |
| F10 | No reward-shaping syntax — score *is* the metric; cannot declaratively say `score = accuracy − 0.1·tokens` | 8 | `Reward<T>` as signed Metric with composition operators. |
| F11 | ~~`@[adaptive]` records only return values, not inputs — hill-climb cannot warm-start from the best previous input~~ — **closed** in the interpreter. The interp now logs `(input, score)` pairs in lock-step (`provenance_inputs`); exposed via `goal_best_input(name, target) -> i64` and `goal_history(name) -> [(i64, f64)]`. Native codegen still logs scores only. | 7 (codegen) | Interpreter done; codegen ABI extension still scheduled for Phase 7. |
| F12 | No `Agent` type — the redteam is just another fn; no tools, effects, policy; two agents cannot be composed | 7 + 8 | Tier-1 stdlib type. |
| F16 | String interpolation parser eagerly treats `{` as an interpolation start — embedding literal Rust/Axon code in a string requires `{{` `}}` escapes (caught at type-check time as "cannot find name") | language | Cheap fix: a `r"…"` raw-string literal that disables `{}` interpolation entirely. Or improve diagnostics so the error names the lexed slot expression rather than the slot's free vars. |

### Strategic (the demo is silent on these)

| # | Gap | Phase target | Notes |
|---|---|---|---|
| F13 | No structured-prose surface — non-programmer cannot author the demo today | 10 | English → AST compiler is the Phase 10 deliverable. |
| F14 | No human-in-the-loop approval between "best variant found" and "deploy_gate fires" | 12 | Web UI surfaces the AST diff + predicted impact + redteam findings for explicit approval. |
| F15 | No `simulate → stress → redteam → verify → deploy` pipeline; demo conflates "score on test set" with "simulate" | 11 | Risk-typed pipeline is the proper home; current demo is a single-stage approximation. |

**Cross-references** to existing §9 questions: F8 partially overlaps with the substrate/surface enforcement question (Phase 6); F15 partially overlaps with risk-classification source (Phase 11). No conflicts; the §9 defaults are still the right defaults.

**Update protocol**: every subsequent ASI demo (the five "cousins" listed in `examples/asi/README.md`) appends its own friction items here before it lands in `examples/`. The list is allowed to grow; it is not allowed to be discarded without a phase landing that closes its target.

---

## 9.6 Session findings (2026-05-28) — interpreter-first execution + native-build fix

A build session landed an end-to-end execution path that bypasses the native-codegen
stall and stood up the first Phase-10 surface command. Each item below is a concrete
forward task, not a status note. Companion docs: `BUILD_DIAGNOSIS.md`,
`CODEGEN_WRAPPER_PROTOTYPE.md`, `SESSION_STATUS.md`. Verifiable in git
(`0fb49c0..800d219`).

### Native codegen build — diagnosed, fix applied, validation pending

- **Root cause pinned** (`BUILD_DIAGNOSIS.md`): the multi-hour `cargo build` stall is
  **100% LLVM-IR generation** of monomorphized inkwell generics in `codegen::builtins`
  — serial, CGU-immune, *not* frontend monomorphization and *not* LLVM optimization
  (`cargo check` finishes in ~4.5 s; `cargo build` is unbounded). This **supersedes the
  §7.5 conclusion** that the inkwell IR-shim ("Stretch²") was the only remaining lever:
  the trait shim was a measured null result; the shim changes dispatch, not instantiation
  count.
- **Fix applied** (commits `f59e392` + `800d219`): all `.build_*` sites in
  `codegen/builtins.rs` now route through `#[inline(never)]` **non-generic** wrappers in
  `codegen/build_wrappers.rs` (each inkwell call monomorphized exactly once;
  `Copies = 1`). `cargo check` clean. Isolated repro (`CODEGEN_WRAPPER_PROTOTYPE.md`)
  measured **−43% IR / −36% RSS / ~1.7–3× wall-clock** — a **constant factor, not an
  asymptote fix** (the two giant functions still lower serially in one CGU each).
- **Remaining work**: (a) run a *finishing* native build on CI / beefy hardware and
  measure the real IR / time / RSS delta against the repro's projection; (b) optionally
  compound the wrappers with **per-builtin function-splitting** so `codegen-units` can
  parallelize the now-smaller bodies; (c) **decide whether native codegen is worth
  maintaining at all** versus interpreter-only — the interpreter already covers
  `run`/`check`/`test`/`goal`, so native is a release/AOT artifact, not a dev dependency.
  Default: keep native CI/release-only behind `--features codegen`; develop against
  `--no-default-features`.

### Interpreter is now the reference execution semantics

- A codegen-free tree-walking interpreter (`crates/axon-core/src/interp.rs`, commit
  `1f6d37a`) is the execution path. `axon run`/`check`/`test`/`goal` all work via the
  interpreter built `cargo build -p axon-core --no-default-features --bin axon` (sub-second).
  Builtin coverage is **90/90**, with an automated table-vs-interpreter audit reporting
  zero gaps (commits `4056608`, `8f1d117`).
- **Forward rule**: the interpreter is now the *de facto* reference semantics. Every new
  language/ASI feature (Phase 5+ refinements, Phase 6 effects, Phase 7 runtime types,
  Phase 8 `goal`/`agent`/`for!`) **must be implemented in `interp.rs`**, not only in
  codegen. Native codegen, if maintained, follows the interpreter — not the reverse. Add
  an interpreter-vs-codegen conformance check to CI once a native build finishes (item
  above), so the two paths cannot silently diverge.

### Phase-10 surface compiler — v1 lifts, v1.1 must generate

- `axon goal <file.md>` (commits `1f83f2a`, `e6ba0f8`) compiles a structured-prose goal
  (`axon-surface`) → `.ax` → type-check → interpret in one command. Surface v1 **lifts**
  real function bodies from the goal file's ` ```axon ` fenced blocks; goals can override
  `try_variant`, `assert_deployable`, and `redteam_check` (commits `f559e93`, `28ab154`,
  `3d36b0f`). This is the first concrete progress on **F13** (no structured-prose surface)
  — but only the *plumbing*; it does not yet author intent.
- **Next** (toward the §5 "Hello Goal" forcing function and Phase-10 exit criteria):
  1. **LLM-driven body *generation*** — v1 only lifts author-written bodies; the surface
     compiler must *propose* bodies from prose, with confidence markers and the mandatory
     user-approval-of-AST step (§2.4). This is the actual Phase-10 deliverable.
  2. **Bundle the `asi_prelude`** (`examples/stdlib/asi_prelude.ax`, commit `fb771cd`) so
     goals can call shared pure score/budget/confidence helpers instead of re-emitting
     scaffolding — the seed of `std::asi::*` (§6).
  3. **Cleaner verify-target extraction** from the Verify prose block (the predicate is
     the one block surface v1 does *not* lift); generalizes once Phase 5 refinements land
     (overlaps **F6**).

### Goal-safety gate — pattern proven, compose with capabilities next

- Three key-free demos in `examples/goals/` prove the safety-gate trio:
  `optimize-goal.md` (deploys), `verified-goal.md` (a confidence `@[verify]` gate blocks
  an under-target agent, exit 101), `redteam-goal.md` (an adversarial `redteam_check`
  blocks a high-scoring-but-unsafe agent, exit 1). The verify + redteam gate composition
  is now an executable pattern, not a slogan — partial closure of **F15** (the
  simulate→redteam→verify→deploy pipeline), still single-stage.
- **Next**: compose the gate with **Budget / Effect / Principal** Tier-1 stdlib
  capabilities (Phase 7) — today the gates are pure-predicate fns with no budget meter
  (**F4**), no effect rows (**F5**), and no Principal-tagged audit trail (**F3**). The
  proven gate shape is the integration point for those primitives as they land.

---

## 10. Three Acid Tests

The product is real iff all three pass. The first is engineering v1; the second and
third are product v1.

| Test | Audience | Time budget | Pass condition |
|---|---|---|---|
| **Hello Goal** | developer | 10 min | CLI session: define goal → run → improvement cycle → safety catch → deploy → replay |
| **First Goal** | non-programmer (structured-prose author) | 10 min | Define a useful goal in the surface UI; system compiles, presents typed AST, user approves, system runs |
| **First Improvement** | non-programmer | 30 min | System proposes an improvement, user reviews diff + predicted impact + redteam findings, approves, sees deploy + post-deploy metrics |
| **First Redteam Catch** | non-programmer | 60 min | System rejects a proposed change for safety; user sees the violated constraint and the reasoning trace in plain English |

Until all three pass, Axon is a demo, not a product.

---

## 11. One-line Summary

Axon ships in this order: **refinement (5) → effects (6) → runtime primitives (7) → goal/agent surface (8) → replay+audit+sandbox (9) → CLI surface (10) → risk-typed pipeline (11) → web UI (12) → probabilistic refinement (13) → distributed types (14+)**, with the typed Axon language serving as an IR rather than a human-authored surface, the userland OS replacing the kernel ambition, and the structured-prose surface compiling to user-approved typed AST as the legal artifact.
