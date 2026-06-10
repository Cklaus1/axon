<!-- Generated 2026-06-10 by a parallel scoping workflow (11 sonnet agents, one per work item,
each grounded against the real code). Synthesized roadmap; verify slices before implementing. -->

# Axon Compiler: Implementation Roadmap

## 1. Critical Path / Dependency DAG

```
Phase-6 resume runtime (Slices 1-2)
    ├── R13 native-FFI Handle type + callback ABI
    ├── R7c BrowserHost interactive tier
    └── R14 mobile AxonRuntime lifecycle adapter

R1c Slice A (dict_from_str) ──► R1c Slice B (arr_group_by)
                                      └── R1c Slice C (wasm32 dict bridges)
                                                ── R7 wasm dict AOT parity

R2a Slice 1 (ExprId infrastructure) ──► Slice 2 (thread TypeMap) ──► Slice 3 (delete heuristic)
                                                                            └── Slice 4 (checker wiring, optional)

R1d Slice 2A ──► Slice 2B ──► Slice 3 (drift test)
R1e Slice 2a (w_bitcast) ──► 2b/2c (bulk conversion) ──► 2d (remove expr.rs from allowlist)
R3 tail (AI tier native) — independent, additive
R7 wasm dict gap — independent of resume runtime, depends only on R1c Slice C
Phase-7 leftovers — independent, kernel-only, interp-only
CLI v1 demo polish — no compiler deps, do first or last
R0 — DONE, no action
```

**Should churn-reducing refactors land before heavy features?**

Yes, with one exception. The ordering recommendation is:

- **R1e Slice 2 (w_* migration)** should land before any new codegen work, because it reduces the reviewer surface area and makes the next codegen author produce consistently wrapper-style code. Cost is M, risk is low, and it is fully worktree-parallelizable.
- **R1d Slice 2 (inline IR porting)** should land before R2a, because deleting 262 lines of inline IR reduces the AST-site count that R2a's ExprId rename must touch.
- **R2a** is XL and architecturally serial (touches 16 files including codegen/mod.rs, ast.rs, infer.rs). It must NOT run concurrently with any work that adds new `Expr::` construction sites. Schedule it as a dedicated sprint.
- **R0 is complete**: confirmed at 2402 lines (within expected accretion range); close it in the matrix, no worktree needed.

---

## 2. Recommended Build Order

Items are labelled `[PARALLEL-SAFE]` when they touch no shared core files (codegen/mod.rs, interp.rs, builtins.rs, ast.rs), or `[SERIAL]` when they do.

### Tier 0 — Do immediately (no blockers, minimal churn)

| Order | Item | Rationale |
|---|---|---|
| 1 | **CLI v1 demo polish (S)** | Zero compiler changes; suppresses W1310 noise on goal demos; unblocks external demo usage today |
| 2 | **R3 tail: native AI tier routing (M)** `[PARALLEL-SAFE]` | Touches only axon-ai/src/lib.rs + codegen/builtins.rs; additive C ABI; low risk; fixes a silent semantic bug (strong tier silently routes to sonnet natively) |
| 3 | **R7 wasm dict gap A (dict_has/inc) (S)** `[PARALLEL-SAFE]` | Only axon-rt/src/lib.rs (wasm32 cfg blocks); unblocks wasm32 Dict programs |

### Tier 1 — Refactor sprint (parallel across worktrees)

| Order | Item | Rationale |
|---|---|---|
| 4 | **R1b str-returning builtins (M)** `[PARALLEL-SAFE]` | Ports str_to_upper/lower/trim/pad inline IR to axon-rt; fixes Unicode divergences; isolated to codegen/builtins.rs + axon-rt/src/lib.rs |
| 4 | **R1e Slice 2 (w_* migration) (M)** `[PARALLEL-SAFE]` | Pure mechanical codegen/expr.rs refactor; locks the tripwire; reduces expr.rs noise for R2a |
| 4 | **R1d Slices 2-4 (registry porting + drift test) (M)** `[PARALLEL-SAFE]` | Delete remaining inline IR blocks; single-source authoring path in place before R2a ExprId rename |
| 5 | **R7 wasm dict gap B+C (dict_set/get/remove + str_split + wasm32 bridges) (M)** `[PARALLEL-SAFE]` | Depends only on axon-rt; follows gap A |

### Tier 2 — Feature additions (targeted, kernel-only or codegen-only)

| Order | Item | Rationale |
|---|---|---|
| 6 | **Phase-7 leftovers: Budget handle + KernelGoal (M)** `[PARALLEL-SAFE]` | Interp-only kernel services; touches kernel.rs + interp/builtins.rs; no codegen; follows established handle pattern |
| 6 | **R1c dict_from_str + arr_group_by Slice A (M)** `[PARALLEL-SAFE]` | dict_from_str is straight pattern-match on dict_to_str inverse; Slice B (arr_group_by) can follow independently |

### Tier 3 — Architecturally serial (dedicated sprint)

| Order | Item | Rationale |
|---|---|---|
| 7 | **R2a Type-map threading (XL)** `[SERIAL]` | Touches 16 files; must not run concurrently with any Expr-site additions; Slice 1 alone is the largest rename in the codebase; Tier 1 refactors reduce the site count first |

### Tier 4 — Platform targets (gates on resume runtime)

| Order | Item | Rationale |
|---|---|---|
| 8 | **Phase-6 resume runtime Slices 1-2 (L)** `[SERIAL]` | Interpreter suspend/restore + kernel FiberState::Suspended; gates R13/R7c/R14 interactive tiers; Slice 3 (codegen widening) is independent but lower value |
| 9 | **R13 / R7c / R14** | Unblocked only after Slice 1+2 of resume runtime; R7c compute-only BrowserHost can parallel-start without resume |

---

## 3. Per-Item Details

### Phase-6 resume / true delimited-continuation runtime
**Effort: L**
**Parity gate:** New `scripts/suspend_resume_parity.sh` — two-invocation save/restore mock against a non-handler reference run. Extended `handler_resume_parity.sh` covers abort/return-arm codegen widening. `scripts/kernel_suspend_fiber_parity.sh` covers Slice 2 scheduler. All must pass under `gate.sh --strict`.
**Biggest risk:** Re-entrant Interp state across call-boundary save/restore. `interp.rs` uses `RefCell<>` pervasively; restoring a saved snapshot into a live Interp must leave no stale borrows. The replay mechanism (eval.rs:691 `Env::from_snapshot`) already does this for within-call replay, but cross-call-boundary restoration is new territory. Validate with a deliberate double-resume stress test.
**Key files:** `/home/cklaus/projects/axon/crates/axon-core/src/interp.rs`, `interp/eval.rs`, `interp/builtins.rs`, `kernel.rs`, `main.rs`

### R7 wasm32 dict ABI gap
**Effort: M**
**Parity gate:** New `scripts/wasm_dict_abi_parity.sh` exercising `dict_new/set/get` with str keys via wasmtime. `wasm_str_abi_parity.sh` extended for `str_split`. Native parity unchanged (bridges are `#[cfg(wasm32)]`-only).
**Biggest risk:** The dict out-param variants (`dict_set/get/remove`) pass AxonStr by value in a struct ABI; the wasm32 scalar expansion must expand exactly the right parameter slots. Validate against the `__axon_str_contains` template (lib.rs lines 869-881).
**Key file:** `/home/cklaus/projects/axon/crates/axon-rt/src/lib.rs`

### R1c dict_from_str + arr_group_by remainder
**Effort: M**
**Parity gate:** Extended `scripts/dict_parity.sh` — Slice A adds round-trip `dict_from_str → dict_get → parse_int` matching `persistent_bandit.ax` pattern. Slice B: `scripts/arr_group_by_parity.sh` with bucket-content assertions. Slice C: `scripts/wasm_dict_parity.sh`.
**Biggest risk:** Slice A has a hidden type-dispatch trap: `dict_get` on a `dict_from_str` result returns `Tag=2 (Str)`, but the current codegen reads the payload as `i64` unconditionally. The `persistent_bandit.ax` pattern calls `parse_int` on the result, so parity tests would pass vacuously (both engines parse a garbage int). Slice A **must** extend `dict_get`'s codegen with tag-dispatch to be I-2 compliant, making it M not S.
**Key files:** `/home/cklaus/projects/axon/crates/axon-rt/src/lib.rs`, `crates/axon-core/src/codegen/builtins.rs`, `codegen/expr.rs`

### R1b str-returning builtins remainder
**Effort: M**
**Parity gate:** Extended `scripts/str_utf8_parity.sh` with multibyte test cases (`str_to_upper("héllo") == "HÉLLO"`); `scripts/fuzz_parity.sh` extended with `str_pad_start/str_pad_end` ASCII cases. The Unicode test additions are the real correctness signal — existing ASCII corpus passes regardless of implementation.
**Biggest risk:** `str_to_upper/lower` can GROW the string (ß → SS); the axon-rt Rust fn must malloc `4*s.len()+1` bytes. Using the inline-IR length (1:1) as a bound would silently corrupt. Validate with a grow-case test in the parity harness.
**Key files:** `/home/cklaus/projects/axon/crates/axon-core/src/codegen/builtins.rs` (lines 3370-3860), `/home/cklaus/projects/axon/crates/axon-rt/src/lib.rs`

### R3 tail: native AI tier routing
**Status (2026-06-10): SOUNDNESS HOLE CLOSED via refusal (3a7d668); routing capability deferred.**
The silent-misroute bug is fixed: native codegen now REFUSES (E0910) a build whose fn directly
calls `ai_complete` under a non-`balanced`/unknown tier, instead of quietly routing `strong`/`cheap`
to the default sonnet model. `balanced`/no-policy fns build and run byte-identical to the interpreter
under `AXON_AI_MOCK` (the native runtime already honors mock + has `ai_complete_inner_model(prompt,
model)`). Tier resolution is single-sourced in `ai_routing::tier_from_attrs` (shared by interp's
`current_ai_tier` and the codegen refusal). Gated: `ai_routing` unit test + cli
`build_refuses_non_balanced_ai_tier_e0910_r3`.
**Remaining (the original "routing" ask — additive, lower priority now the hole is closed):** thread
the resolved model through a new C ABI so native HONORS cheap/strong (gains the capability) instead of
refusing. The runtime is ready; needs a model-carrying ABI + per-call-site `(env_key, default)`
constants from `Tier::api_model` emitted in codegen. Then replace the refusal with the routed call.
**Key files:** `crates/axon-ai/src/lib.rs`, `crates/axon-core/src/codegen/builtins.rs`, `crates/axon-core/src/ai_routing.rs`

### R2a: Type-map threading
**Effort: XL**
**Parity gate:** Slice 2's debug assertion `debug_assert_eq!(type_map.get(expr.id), infer_expr_sem_type(expr))` must be silent across all 28+ example programs under `AXON_AI_MOCK=1` before Slice 3 deletes the heuristic. Full `parity_all.sh` + `fuzz_parity.sh` is the Slice 3 gate. The done-signal: `rg 'infer_expr_sem_type' crates/axon-core/src/` returns empty.
**Biggest risk:** `Uncertain<T>` binop heuristic in `infer_expr_sem_type` (mod.rs:1550-1589) has custom propagation logic that HM's constraint solve may not record identically. This is the most likely source of Slice 2 assertion fires. The fallback keeps the heuristic live while investigating, so the risk is debug iteration time, not correctness regression.
**Key files:** `/home/cklaus/projects/axon/crates/axon-core/src/ast.rs`, `infer.rs`, `codegen/mod.rs`, `codegen/expr.rs`, `mono.rs` — plus 12 secondary files via Expr:: rename

### R1d: single-source builtins
**Effort: M**
**Parity gate:** `cargo test -p axon-core -p axon-rt` + `scripts/all_examples_parity.sh` + `scripts/str_utf8_parity.sh` after each batch. Slice 3's drift `#[test]` in `builtins.rs` becomes the permanent enforcement guard.
**Biggest risk:** str_trim/str_pad axon-rt fns must heap-allocate via the same global allocator as the inline IR versions. A `String::into_bytes()` that drops before the pointer is read would be a use-after-free. Use `Box::into_raw(vec.into_boxed_slice())` and document the ownership contract.
**Key files:** `crates/axon-core/src/codegen/builtin_externs.rs`, `crates/axon-core/src/codegen/builtins.rs`, `/home/cklaus/projects/axon/crates/axon-rt/src/lib.rs`

### R1e: retire dead IR-trait shim
**Effort: M**
**Parity gate:** `cargo test -p axon-core` (includes both tripwire tests) + `scripts/parity_all.sh`. Done-signal: `rg 'self\.ir\.builder\.build_' crates/axon-core/src/codegen/ | grep -v build_wrappers.rs` returns empty.
**Biggest risk:** The 48 `build_gep` sites require `unsafe { }` wrapping at each call site when converted to `w_gep`. Missing one would be a compile error (safe), but reviewing 48 unsafe blocks adds review burden. Batch them together in Slice 2b so the diff is reviewable in one pass.
**Key files:** `/home/cklaus/projects/axon/crates/axon-core/src/codegen/expr.rs` (217 sites), `build_wrappers.rs` (needs `w_bitcast`)

### R0: interp module split
**Effort: DONE**
No action. Optional call_builtin category sub-split (builtins.rs at 3199 lines) is cosmetic. Close in matrix.

### Phase-7 leftovers: Budget handle + KernelGoal
**Effort: M**
**Parity gate:** New `phase7_kernel_budget_handle` and `phase7_kernel_goal` tests in `crates/axon-core/tests/cli_run.rs` following the `phase7_kernel_llm_gateway` pattern. Budget handle behavior must byte-match the userland `examples/stdlib/budget.ax` oracle for equivalent operations.
**Biggest risk:** The `Budget` struct already exists in `kernel.rs` (lines 22-47) as an internal type held inside `PrincipalEntry`. Exposing it as a user-accessible handle means adding a `Vec<Budget>` handle pool to `Interp` and a `budget_open` builtin — but the existing `principal_budget_remaining` builtin already accesses budget state via the principal. The implementation must not double-account: `budget_spend(bh, n)` and `principal budget_remaining` should reflect the same underlying counter, requiring a shared reference rather than a separate pool.
**Key files:** `/home/cklaus/projects/axon/crates/axon-core/src/kernel.rs`, `interp/builtins.rs`, `builtins.rs`

### CLI v1 demo polish
**Effort: S**
**Parity gate:** `DEMO_NOPAUSE=1 AXON_AI_MOCK=1 examples/flagship/demo.sh` exits 0. Behavioral check: `axon goal --emit` diff before/after W1310 annotation suppression shows only the `@[ai(policy(tier: balanced))]` attribute added, no logic changes.
**Biggest risk:** None. All enforcement is already CI-guarded at `cli_run.rs:1187-1213`.
**Key files:** `README.md`, `examples/goals/agent-goal.md`, `examples/goals/agent-goal-evil.md`, `examples/flagship/run.sh`

---

## 4. Corrected Matrix: Staleness Found by Agents

- **R1 row**: States "Remaining: the i64→i32 wasm ABI retarget" — **wrong**. The retarget is done via axon-rt scalar-expansion bridges, not a codegen pointer-width change. Correct to: "Remaining: dict-str-key ABI bridge (wasm32), BrowserHost, js/mobile targets." Completion % should be ~78, not 70 (str/array ABI + reactor-mode link + dead-function pruning all landed).
- **R1 row**: Does not mention three E0910-gated dict builtins (`dict_from_str`, `dict_try_from_str`, `arr_group_by`) or wasm32 absence of all 17 dict externs.
- **R1b spec** (`R1b-str-return-abi.md`): Acceptance criteria are fully satisfied for its four named targets, but `str_to_upper/lower/trim/pad` remain inline IR with Unicode divergence — these are unscoped follow-on work absent from R1b, R1d, and the matrix. The `fuzz_parity.sh` ASCII-only corpus gives false confidence.
- **R1d spec** (`R1d-single-source-builtins.md`): Marked "Draft" in `governance/specs/README.md` even though Slice 1 is fully landed. Update to "Slice 1 LANDED, Slice 2 in progress."
- **R1e spec** (`governance/specs/README.md` line 15): Shows 154 direct IR sites — actual current count is 217 (grown from Phase 6/7 additions). Update site count and mark Slice 1 landed.
- **R1e tripwire comment** (`cli_run.rs` ~line 10200): Says "165 typed straggler sites" — actual is 217.
- **R2 row**: States "90%, refinement types not landed" — **wrong**. Phase 5 with all four refinement obligation sites is confirmed landed (CLAUDE.md Phase Status). Correct to: "90%, R2a type-map threading unstarted (3 derivations of type info, codegen uses HM-disagrees heuristic)." No pointer to R2a spec in matrix.
- **R3 row**: States "Remaining: first-class Budget value type (Phase-7); native-codegen tier threading." The Budget userland module (`examples/stdlib/budget.ax`) conflates with a compiler-level Budget handle. `Budget` struct exists in `kernel.rs` as an internal type in `PrincipalEntry`; `principal_budget_remaining` already exposes it. Correct gap text to: "Budget as user-accessible handle type (budget_open/spend/remaining builtins); native-codegen tier threading (__axon_ai_complete always routes to sonnet)."
- **R5/R12 row**: Commit 3aae955 labeled "Phase 7 complete" shipped only LLM<Caps>; KernelGoal and principal-scoped goal_open/step_kernel are unbuilt. Matrix gap cell is accidentally correct ("kernel Goal<M> still missing") but the commit message is misleading. R12 spec Slice 5 gate passes only its LLM half.
- **R7 row**: `governance/specs/README.md` R7c-browser-host.md listed as untracked (`??` in git status) — not referenced in the README index. R13-native-ffi.md and R14-mobile-targets.md similarly untracked. Add all three to `governance/specs/README.md` index.
- **R0**: No matrix row (correct — it is cross-cutting). R0-interp-module-split.md line-count entry says ~1885 lines; actual is 2402 (517 lines of Phase 5/6/7 additions after R0 closed). Update the spec's line count to 2402 and note accretion source.
- **CLAUDE.md Phase 6 remaining**: Correctly names "row-variable unification (E03) and resume/shallow-continuation runtime" but does not call out `FiberState::Suspended` as a sub-item of the scheduler work. Also does not note that R13/R7c/R14 are blocked on the resume runtime specifically.
- **CLAUDE.md "Adding a New Builtin"**: 5-step list (BUILTINS → codegen declare → infer fn_return_types → checker → examples) is obsolete for builtins that go through the `BUILTIN_EXTERNS` registry. Should be updated after R1d Slice 4.

---

## 5. Next 3 Workflows to Launch

### Workflow A: Refactor Parallel Sprint
**Fans out over:** R1b (str inline IR migration), R1e (w_* conversion), R1d Slice 2A-2B (remaining inline block porting)
**Parallelizable:** Yes — these three items touch disjoint file sets. R1b: `codegen/builtins.rs` + `axon-rt/src/lib.rs`. R1e: `codegen/expr.rs` + `build_wrappers.rs`. R1d: `codegen/builtin_externs.rs` + `axon-rt/src/lib.rs` (non-overlapping function entries with R1b).
**Gating:** All three complete before launching R2a (to minimize the ExprId rename site count). Gate: `scripts/parity_all.sh` green + `scripts/str_utf8_parity.sh` multibyte assertions pass + `cargo test` (R1e tripwire + R1d drift test) + `rg '\.builder\.build_' codegen/ | grep -v build_wrappers.rs` returns empty.
**Launch command:** Three parallel worktrees on `merge-asi-layer3`: one per item. Each worktree gates independently before merging to the branch.

### Workflow B: R3 + Phase-7 Kernel Additions
**Fans out over:** R3 tail (AI tier native ABI threading) and Phase-7 KernelGoal + Budget handle
**Parallelizable:** Yes — R3 touches `axon-ai/src/lib.rs` + `codegen/builtins.rs`; Phase-7 touches `kernel.rs` + `interp/builtins.rs`. No overlap.
**Gating:** R3: `scripts/ai_tier_parity.sh` PASS under `AXON_AI_MOCK=1` (byte-identical tier-strong program) + `cargo test -p axon-ai` (api_model_is_tier_specific_and_distinct unchanged). Phase-7: new `phase7_kernel_budget_handle` + `phase7_kernel_goal` tests in `cli_run.rs` PASS + Budget handle byte-matches `examples/stdlib/budget.ax` oracle. Both gated under `gate.sh --strict`. Launch after Workflow A completes (reduces active surface area).

### Workflow C: R2a ExprId Infrastructure (Serial Sprint)
**Fans out over:** Slice 1 (ExprId infrastructure, rename Expr→ExprKind, number_exprs pass) only — do not start Slice 2 until Slice 1 is green
**Serial:** Yes — this is the most cross-cutting change in the codebase (~1,635 Expr:: construction sites across 16 files). Must hold all other structural changes. No other worktree should touch ast.rs, parser.rs, infer.rs, or mono.rs during this window.
**Gating:** Slice 1 gate: `cargo build` (Rust catches all missed rename sites) + `cargo test` (zero behavior change) + `scripts/parity_all.sh` identical pass rate. Slice 2 additional gate: debug assertion `debug_assert_eq!(map_lookup, heuristic_result)` silent across entire `examples/` corpus + `scripts/fuzz_parity.sh`. Do NOT advance to Slice 3 (heuristic deletion) until Slice 2 has been green for at least one full `gate.sh --strict` run on the CI branch. The `Uncertain<T>` binop heuristic (mod.rs:1550-1589) is the highest-probability assertion site — investigate that divergence class first before declaring Slice 2 stable.
