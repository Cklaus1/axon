# Axon Session Status

**Last update**: 2026-05-05 (multi-day session 2026-05-04 → 2026-05-05)
**Branch**: `merge-asi-layer3`
**TrainLoop session id**: `axon-20260505-8dbfd412` (tag: `axon`)

A snapshot of state across the recent multi-cycle work.  Companion to
`ROADMAP.md` (forward plan) and `STATUS.md` (Phase 4 shipped state).

---

## Recent commit graph (this multi-cycle work)

```
72198ad  MIGRATION.md: document architectural constraint on partial IR.3
76aab42  codegen IR.3 prep: wire `ir: InkwellBackend<'ctx>` into Codegen
0a0c76c  codegen: add MIGRATION.md — IR.3 per-module migration playbook
31db836  codegen IR.2.5: add unit tests to ir_inkwell.rs
deee900  codegen IR.2: implement InkwellBackend (the bounded inkwell impl)
6b25739  codegen: draft `ir.rs` — inkwell-shim trait surface (no impl yet)
b668711  ROADMAP §7.5: -Z parallel-frontend empirically rejected; shim is the answer
77f8724  ROADMAP §7.5: empirical Phase 3 finding — decomposition necessary but not sufficient
e51fbf4  specs/roadmap: replace human-time estimates with ASI-iteration framing
d1d9e25  codegen Phase 3.2: decompose declare_builtins into 4 section helpers
a9bcf87  codegen Phase 3.1: decompose emit_expr into 15 per-variant helpers
3e7a40a  codegen module-split Phase 2.3–2.8: full file decomposition
fbab931  codegen module-split Phase 2: extract types.rs + asi.rs
62bcc6f  ASI demos + roadmap + observability + module-split Phase 1
```

---

## Layered state

### Strategic (decided this session)

* Two-track product: typed-IR `.ax` + structured-prose surface UX
* Userland-OS only — kernel ambitions killed
* Co-evolution mandatory (language + runtime + surface together)
* Typed AST is the legal artifact, not the English intent
* Stdlib defines the paradigm (14 Tier-1 types planned)
* CLI-first (engineering v1) → Web UI (product v1) — one builds on the other

### ROADMAP'd (forward phases)

* Phase 5  — refinement types + SMT (drafted as `spec/compiler-phase5.md`)
* Phase 6  — row-polymorphic effects + handlers (drafted as `spec/compiler-phase6.md`)
* Phase 7  — Principal / Store / Supervisor / LLM\<Caps\> as runtime services
* Phase 8  — goal/agent/`for!` surface + Tier-1 stdlib
* Phase 9  — replay + audit + sandbox
* Phase 10 — CLI surface ("Hello Goal" forcing function)
* Phase 11 — risk-typed simulation gate
* Phase 12 — web UI thin shell
* Phase 13 — probabilistic refinement (closed-form fragment)
* Phase 14+ — distributed types

### Codegen module-split (Phases 1–3)

| Phase | What | Status |
|---|---|---|
| 1 | `link.rs` extracted (~280 LoC) | ✅ landed |
| 2 | 8 modules: types, asi, option_result, output, match_pat, builtins, expr, mod | ✅ landed |
| 3.1 | decompose `emit_expr` (1380 LoC) → 15 per-variant helpers | ✅ landed |
| 3.2 | decompose `declare_builtins` (3870 LoC) → 4 section helpers | ✅ landed |

**Empirical**: Phase 2 + Phase 3 alone do *not* fix the slow build.
Pre-decomposition build was 9h+ (never finished, 3.6 GB peak).
Phase 2/3 alone got to LLVM codegen (272 .o files emitted) but still
stalled mid-codegen at 5h+.  Nightly `-Z parallel-frontend` did NOT
parallelize the trait queries — only 1 of 8 threads working — also
stalled.

### IR shim (the real fix)

| Phase | What | Status |
|---|---|---|
| IR.1 | `ir.rs` — IR trait + 5 handle types + helper enums (~280 LoC) | ✅ landed |
| IR.2 | `ir_inkwell.rs` — `InkwellBackend<'ctx>` impl (~990 LoC, 3 tests) | ✅ landed |
| IR.3 prep | `ir: InkwellBackend<'ctx>` field wired into Codegen | ✅ landed |
| IR.3 | per-module migration to `self.ir.*` | ⚠️ blocked — see below |
| IR.4 | remove legacy `module/builder` fields | not started |
| IR.5 | validate `cargo build -p axon-core` <30 min | not started |

**Architectural blocker on IR.3**: `InkwellBackend` currently *owns* its
own `Module<'ctx>` separate from `Codegen::module`.  Symbol lookups
diverge — partial migration is unbuildable end-to-end.  Documented in
`MIGRATION.md`'s ⚠️ section.  Resolution: re-architect
`InkwellBackend` to *share* Codegen's Module + Builder.  In progress
(sub-agent task drafting `IR_REARCH.md`).

### ASI demo set (validation infrastructure)

| Demo | Pattern | Lines |
|---|---|---|
| `examples/asi/optimize.ax` | ai_complete + parse + adaptive + verify + redteam | 176 |
| `examples/asi/classify.ax` | ai_extract_uncertain_i64; confidence in score | ~140 |
| `examples/asi/summarize.ax` | composite metric (length × LLM-judged coverage) | ~150 |
| `examples/asi/code_review.ax` | two cooperating @[adaptive] fns | ~140 |
| `examples/asi/search_rank.ax` | adversarial search w/ prompt-injection redteam | ~140 |
| `examples/asi/pricing.ax` | multi-objective composite reward | ~120 |

All 6 type-check clean via `axon-check` (the no-default-features parallel
checker tool at `/tmp/axon-check`).  Validates the language pipeline
without paying inkwell tax.

### Observability infrastructure

| Tool | Purpose |
|---|---|
| `examples/asi/run.sh` | Phase-10 CLI surface simulator |
| `examples/asi/bench.sh` | per-run wall-clock + score statistics |
| `examples/asi/watch.sh` | live ASCII sparkline of scores |
| `examples/asi/analyze.py` | cross-run statistics + plateau detection |
| `examples/asi/llm_proxy.py` | stdlib-only Anthropic proxy → per-call latency/tokens/cost |
| `crates/axon-ai/src/lib.rs` | `ANTHROPIC_BASE_URL` env-var override (3 sites) |

---

## Current blockers (in priority order)

1. **IR_REARCH.md design** (in flight, sub-agent drafting) — must land
   before any IR.3 batch
2. **InkwellBackend re-architecture** (after IR_REARCH lands) — share
   Module + Builder with Codegen
3. **IR.3 batch migrations** (after re-arch) — 7 modules, smallest
   first, validate per batch
4. **IR.4 + IR.5** — remove legacy fields, validate full build
5. **Phase 5 (refinement) implementation** — currently spec-only

---

## Active loop

Cron `a4b6bd67` fires every minute, prompt asks for "single highest-leverage
next step + execute".  Self-extends; cancel via `CronDelete a4b6bd67`.

Recent loop iterations chose:
1. → draft `ir.rs` (IR.1)
2. → implement `ir_inkwell.rs` (IR.2)
3. → unit-test `ir_inkwell.rs` (IR.2.5)
4. → draft `MIGRATION.md`
5. → wire `ir` field into Codegen (Step 0)
6. → discover architectural constraint, document in MIGRATION.md
7. → spawn sub-agent for IR_REARCH design + write SESSION_STATUS.md (this)

---

## TrainLoop attribution

**Active**:
* Tag: `axon` (file `/tmp/trainloop-tag`)
* Session id: `axon-20260505-8dbfd412` (file `~/.trainloop-session`)
* Global git hooks: ✅ installed
* Gateway: ✅ up at `http://127.0.0.1:3456`

**Inactive for this session**:
* `ANTHROPIC_BASE_URL` and `OPENAI_BASE_URL` are unset → this session
  doesn't route through TrainLoop.  Fix in `~/.bashrc` + restart Claude
  Code.

Corpus state at session start: 26,888 turns / 58 sessions / 7,145
SFT-ready / 0 DPO pairs / 0 outcomes attributed.

---

## What unblocks shipping a working `axon` binary

The empirical evidence in this session is conclusive: only the IR shim
fully migrated + integrated unblocks a working build.  Path:

1. Land `IR_REARCH.md` (in flight)
2. Refactor `ir_inkwell.rs` per IR_REARCH (~30 lines)
3. Migrate 7 modules per `MIGRATION.md` (in batch order: asi → option_result → types → output → match_pat → expr → builtins → mod)
4. Remove `Codegen::module/builder` fields (IR.4)
5. Run `cargo build -p axon-core`; verify <30 min on canonical hardware (IR.5)

Estimated: a few ASI iteration cycles each for steps 2–4 + one fast-build
validation cycle for step 5.  Steps 2–4 can be parallelized across
sub-agents per file.

---

## Open questions / research items

* Will the inkwell shim actually deliver the speedup we hypothesize?
  Empirically untested.  Validation gate is IR.5.
* If IR.5 fails, fall-back is MLIR or cranelift (also speculative).
* When does the structured-prose surface (Phase 10) become urgent vs.
  defensive engineering on the binary?  Still unresolved.
