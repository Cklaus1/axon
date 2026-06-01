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
| **R1** | Native compiler pipeline at Tier-1 perf (parse→typecheck→borrow→LLVM→native) | Compiler Architecture; Build Phases | 40 | ⚠️ Partial | LLVM build does not finish (`BUILD_DIAGNOSIS.md`); interpreter is the live path. Native binary not routinely producible. | `spec/compiler-phase1.md` + `BUILD_DIAGNOSIS.md` (fix: `CODEGEN_WRAPPER_PROTOTYPE.md`) | A native binary of `examples/*.ax` runs and matches interpreter output + a perf-tier benchmark |
| **R2** | Type system + borrow checker (HM inference, no null, Result/Option, 2-mode ownership) | Type System | 90 | ✅ Strong | Edge cases in generic + trait resolution; refinement types (Phase 5) not landed | `spec/compiler-phase{2,3}.md`; `spec/compiler-phase5.md` (draft) | `cargo test` infer/checker/borrow suites + `all_examples_typecheck_clean` |
| **R3** | AI as language primitive — `std.ai`, model routing, `#[ai(policy)]`, deterministic fallback | AI as Primitive; AI Policy | 40 | ⚠️ Partial | Only `ai_complete`/`ai_extract_*` against one provider; no routing, no `#[ai(policy)]`, no mandatory fallback | **TODO: `governance/specs/R3-ai-primitive.md`** | Policy-gated call honored; fallback fires offline; routing picks tier by cost |
| **R4** | Three code zones: Static / Adaptive / Agent + compiler-enforced provenance | Three Code Zones; Error/Perf Logging | 55 | ⚠️ Partial | `@[adaptive]`/`@[agent]` annotations + provenance JSONL exist; `#[experiment]` not distinct; provenance not *un-opt-out-able* in codegen | `ROADMAP.md` §6–7; `governance/specs/R4-code-zones.md` (TODO) | Adaptive fn cannot compile without provenance injection; agent action log mandatory |
| **R5** | Goal-directed optimization — `#[goal]` type, 3 strategies, deterministic test sets, eval hierarchy | Autonomous Optimization | 65 | ⚠️ Partial | 4 runtime strategies shipped (`goal_run`, `_continue`, `_random`, `_multistart`); no first-class `Goal` value, no `#[goal]` attr wiring metric+test_set, no eval hierarchy | `ROADMAP.md` §5; `spec/stdlib.md` | `#[goal(metric, test_set, target)]` runs to target on a held-out set; provenance per experiment |
| **R6** | Capability security — content-addressed imports, compile-time I/O restriction, AI audit on import | Supply Chain Security; Capability Permissions | 25 | ⚠️ Partial | `@[contained]` enforces net/fs/exec at fn boundary (CLI-gated); no registry, no content-addressing, no import audit, no reproducible-build proof | **TODO: `governance/specs/R6-capability-security.md`** | Import without declared capability fails compile; tampered content hash rejected |
| **R7** | Cross-platform targets — native / wasm / js / mobile from one source | Cross-Platform; Mobile; UI; 3D | 10 | ❌ Thin | Native only (and stalled). wasm/js/mobile = 0%. UI/3D = 0%. | **TODO: `governance/specs/R7-targets.md`** | Same `.ax` compiles+runs on native and wasm with identical observable results |
| **R8** | Built-in testing + structured errors — `#[test]`, property-based `forall`, AI-parseable diagnostics | Testing; Error Messages | 70 | ⚠️ Partial | `@[test]`/`should_fail` work; **no `forall` property testing**; errors structured-ish but not a stable machine schema | `TESTING_STANDARD.md`; `spec/stdlib.md` | `forall(a,b){assert(f(a,b)==f(b,a))}` runs N cases + shrinks; error JSON schema versioned |
| **R9** | Layer-1/3 alignment — `Uncertain<T>`, `Temporal<T>`, `#[contained]`+`#[corrigible]`, `#[verify]` | ASI Extensions; Structural Alignment; Formal Verification | 50 | ⚠️ Partial | Uncertain/Temporal/`@[verify]`(runtime, composite preds)/`@[contained]` done; **`#[corrigible]` absent**; no SMT-backed `#[verify]` | `spec/compiler-phase5.md` (SMT); `ROADMAP.md` §0,§7 | `#[corrigible]` kill-switch latches; `#[verify]` refinement proven by Z3 on a sample |
| **R10** | Self-improving compiler — learned optimization passes graduated from AI-discovered asm | Self-Improving Compiler; Intelligence Maturity | 0 | ❌ Not started | No profiler, no pattern store, no graduation pipeline. The recursive-improvement flywheel. | **TODO: `governance/specs/R10-self-improving-compiler.md`** | A discovered pattern verified correct + faster, added as a pass, applied automatically |

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
