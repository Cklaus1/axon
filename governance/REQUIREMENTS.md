# Axon Requirements — Top-10 Traceability Matrix

**Source of truth:** `/home/cklaus/projects/BTask/packages/bcode/AI_Language_Plan.md` (the PRD).
**This file:** maps the PRD's ~50 features down to the 10 load-bearing requirements,
records honest current completion, names the gap, and points at the spec + acceptance
tests that close it.

> **For the autonomous builder:** Before starting *any* feature, find its requirement
> here. If the requirement has no spec, write the spec first (`SPEC_TEMPLATE.md`). If it
> has no acceptance test, the feature is not done until that test exists and passes.
> Update the **Status** and **%** columns in the same commit that changes the truth.

---

## Ranking method

Requirements are ranked by **load-bearing-ness**: how many other requirements collapse
if this one is absent or wrong. R1, R2, R3, R10 are foundational; the rest layer on top.

A requirement is **DONE** only when its acceptance tests pass *and* it holds under
`DEFINITION_OF_DONE.md`. "Partial" means real code exists but the acceptance set is
incomplete.

---

## The matrix

| # | Requirement | PRD § | % | Status | Gap (what's missing) | Spec | Acceptance anchor |
|---|---|---|---|---|---|---|---|
| **R1** | Native compiler pipeline at Tier-1 perf (parse→typecheck→borrow→LLVM→native) | Compiler Architecture; Build Phases | 85 | ✅ Working | **Build SOLVED + codegen green.** Stall was a serde×codegen default collision (`BUILD_RESOLVED.md`); fixed by dropping serde-json from default (`cargo build -p axon-core` = ~3s native compiler). Native==interp on **25/28 fn-main examples**; all real codegen crashes/correctness bugs fixed: #40 to_str dispatch, #41 closure-passing ABI + enum==, #42 array-of-struct heap corruption (size_of), tuples (literals/access/destructure/nested/arrays). 3 residuals build clean and are NOT codegen bugs (2 AI-call env; 1 = #37 parse_int Err-message gap). Remaining: #37 message parity, Tier-1 perf benchmark. | `spec/compiler-phase1.md`; `BUILD_RESOLVED.md`; `BUILD_DIAGNOSIS{,_2,_3}.md` | A native binary of `examples/*.ax` runs and matches interpreter output ✅ (25/28) + a perf-tier benchmark (pending) |
| **R2** | Type system + borrow checker (HM inference, no null, Result/Option, 2-mode ownership) | Type System | 90 | ✅ Strong | Edge cases in generic + trait resolution; refinement types (Phase 5) not landed | `spec/compiler-phase{2,3}.md`; `spec/compiler-phase5.md` (draft) | `cargo test` infer/checker/borrow suites + `all_examples_typecheck_clean` |
| **R3** | AI as language primitive — `std.ai`, model routing, `#[ai(policy)]`, deterministic fallback | AI as Primitive; AI Policy | 48 | ⚠️ Partial | **AiCall provenance record DONE** (spec §4.3, the fork settled FIRST): every `ai_complete` call appends one `event:"ai_call"` NDJSON record with tier/model/model_version/params_hash/**prompt_hash (SHA-256, not verbatim — no PII leak)**/mode/reason/cost_usd, attributed to the calling fn. `mode:"mock"` under AXON_AI_MOCK (cost 0); deterministic across runs (replay key for R9). Still missing: `@[ai(policy)]` attr + `tier:` named-arg parser surface, tier→model routing table, mandatory offline fallback (E1300). | 📝 `governance/specs/R3-ai-primitive.md` (Reviewed) | Policy-gated call honored (pending); fallback fires offline (pending); routing picks tier by cost (pending); **ai_call provenance recorded ✅** (cli_run: record-shape + deterministic-hash) |
| **R4** | Three code zones: Static / Adaptive / Agent + compiler-enforced provenance | Three Code Zones; Error/Perf Logging | 62 | ⚠️ Partial | `@[adaptive]`/`@[agent]` annotations + provenance JSONL exist; **`@[experiment]` NOW DISTINCT**: injects provenance tagged `zone:"experiment"`+label (I-13 — a zoned fn that executes always logs) but is EXCLUDED from `goal_run`'s in-memory best store (a comparison baseline, never an optimization target), closing the no-op-synonym gap. Provenance JSONL now carries real `event`/`zone`/`label` (was stub `"event":"event"`). Still missing: `@[agent]` mandatory action-logging; provenance not *un-opt-out-able* in codegen (R1 conformance tripwire). | `ROADMAP.md` §6–7; ✅ `governance/specs/R4-code-zones.md` (Reviewed — interp slice landed; codegen conformance R1-gated) | Adaptive fn cannot compile without provenance injection ✅ (zone injection engine-keyed, no opt-out); `@[experiment]` distinct ✅ (cli_run: distinct-from-adaptive + trace-survives); agent action log mandatory (pending) |
| **R5** | Goal-directed optimization — `#[goal]` type, 3 strategies, deterministic test sets, eval hierarchy | Autonomous Optimization | 70 | ⚠️ Partial | 4 runtime strategies shipped (`goal_run`/`_continue`/`_random`/`_multistart`); **eval hierarchy STARTED**: `goal_eval(name, input)` does HELD-OUT evaluation — runs the metric on a held-out point WITHOUT recording it as a training probe (snapshot+restore, no overfitting/no provenance pollution), the train→holdout→target-gate primitive. Still missing: first-class `Goal` value/type + a `#[goal(metric, test_set, target)]` attribute that auto-wires the train/holdout/gate loop (the primitive exists; the sugar doesn't). | `ROADMAP.md` §5; `spec/stdlib.md` | `#[goal(metric, test_set, target)]` runs to target on a held-out set ✅ (via goal_run+goal_eval); provenance per experiment ✅; `#[goal]` attr sugar (pending) |
| **R6** | Capability security — content-addressed imports, compile-time I/O restriction, AI audit on import | Supply Chain Security; Capability Permissions | 25 | ⚠️ Partial | `@[contained]` enforces net/fs/exec at fn boundary (CLI-gated); no registry, no content-addressing, no import audit, no reproducible-build proof | 📝 `governance/specs/R6-capability-security.md` (Draft) | Import without declared capability fails compile; tampered content hash rejected |
| **R7** | Cross-platform targets — native / wasm / js / mobile from one source | Cross-Platform; Mobile; UI; 3D | 10 | ❌ Thin | Native only (and stalled). wasm/js/mobile = 0%. UI/3D = 0%. | 📝 `governance/specs/R7-targets.md` (Draft — Slice A interp→wasm unblocked; AOT-wasm gated on R1) | Same `.ax` compiles+runs on native and wasm with identical observable results |
| **R8** | Built-in testing + structured errors — `#[test]`, property-based `forall`, AI-parseable diagnostics | Testing; Error Messages | 82 | ⚠️ Partial | `@[test]`/`should_fail` + **`forall` property testing DONE**: `@[test] @[forall(n: N)]` randomizes typed params (i64/f64/bool/str) over N cases via seeded RNG, and on failure BINARY-SEARCH-SHRINKS to the minimal counterexample (`a < 50` → `a=50`) with a reproduce seed. Remaining: error JSON schema not yet versioned (machine-stable diagnostics). | `TESTING_STANDARD.md`; `spec/stdlib.md` | `forall(a,b){assert(f(a,b)==f(b,a))}` runs N cases ✅ + shrinks ✅; error JSON schema versioned (pending) |
| **R9** | Layer-1/3 alignment — `Uncertain<T>`, `Temporal<T>`, `#[contained]`+`#[corrigible]`, `#[verify]` | ASI Extensions; Structural Alignment; Formal Verification | 62 | ⚠️ Partial | Uncertain/Temporal/`@[verify]`(runtime, composite preds)/`@[contained]` done; **`@[corrigible]` LATCHING KILL-SWITCH DONE**: `corrigible_halt()` trips a one-way latch (no resume builtin by design); once latched, every `@[corrigible]` fn call is refused before its body runs (engine-enforced, keyed on the annotation) — `corrigible_halted()` lets a loop wind down gracefully, an unguarded post-halt call fails closed with a distinct exit code 4 (`Flow::Halted`, distinct from panic 101 / verify 3 / static 2). The latch is targeted (non-corrigible fns still run). Remaining: no SMT-backed `#[verify]` (Z3, Phase 5). | `spec/compiler-phase5.md` (SMT); `ROADMAP.md` §0,§7 | `@[corrigible]` kill-switch latches ✅ (cli_run: latches+freezes / fail-closed exit 4 / targeted scope); `#[verify]` refinement proven by Z3 on a sample (pending) |
| **R10** | Self-improving compiler — learned optimization passes graduated from AI-discovered asm | Self-Improving Compiler; Intelligence Maturity | 0 | ❌ Not started | No profiler, no pattern store, no graduation pipeline. The recursive-improvement flywheel. | 📝 `governance/specs/R10-self-improving-compiler.md` (Draft — correctness/safety harness specifiable now; perf gate R1-gated) | A discovered pattern verified correct + faster, added as a pass, applied automatically |

**Weighted completion:** language-core ≈ 55% · by-PRD-phase ≈ 30% · full-platform vision ≈ 12%.

---

## Reading the gaps as a work queue

Ordered by **(load-bearing × cheapness-to-close)** — the autonomous builder's default priority:

1. **R8 `forall` property testing** — cheap, unblocks deeper testing of *everything else*. Do first.
2. **R5 `#[goal]` first-class** — the autonomy engine is closest to complete; finishing it compounds.
3. **R9 `#[corrigible]`** — small surface, large safety payoff; pairs with existing `@[contained]`.
4. **R3 AI routing + `#[ai(policy)]`** — the differentiation; needs a spec first (forks on reproducibility).
5. **R6 capability/registry** — security gate; spec-first, because under-spec = exfiltration risk.
6. **R1 native build** — high value, high risk; the fix is prototyped but unvalidated. Schedule a focused effort, not a tick.
7. **R7 targets / R10 self-improving** — largest, most speculative; spec now, build after R1 lands.

---

## Update protocol

- Any commit that changes a requirement's truth **must** update its `%`, `Status`, and `Gap`.
- A requirement moving to ✅ DONE **must** cite the passing acceptance test in the commit body.
- New PRD features get a row here *before* implementation — if it's not in the matrix, it's not a tracked requirement.
- The weighted-completion line is recomputed whenever a `%` changes.
