# Tech Spec — Kernel `Goal` (principal-scoped, budgeted objective runner)

Status: **Draft** (2026-06-10) — Gate-1 spec for the missing half of R12 Slice 5.
Author: autonomous build loop. Needs review before implementation (Structural change).

## Kernel `Goal`

### 1. Motivation

R12 Slice 5 was specified as **`Goal<M>` + `LLM<Caps>`**. Only the `LLM<Caps>` half
shipped — `kernel.rs` has `LlmGateway` (per-token cost metering debited from a
Slice-1 `Principal`'s budget, graceful fallback+latch on overrun). The `Goal` half
is a **comment stub** (`kernel.rs:485-493`): *"A kernel `Goal` runs an objective
under the same principal budget and refuses to exceed it (E1604)."* No
`KernelGoal` struct, no registry, no principal-scoped goal builtins exist.

The interpreter already has the OPTIMIZATION ENGINE (`goal_run` / `goal_run_random`
/ `goal_continue` over an `@[adaptive]` fn, accumulating provenance). What is
missing is the KERNEL framing: running that engine **under a principal's
authority + budget**, so that an autonomous goal-directed loop (the ASI thesis)
cannot spend beyond what its principal was granted — authority (Slice 1) and
spend (the cost meter) as ONE model. This is the last unshipped R12 slice.

### 2. Requirement link

R12 (kernel runtime services), Slice 5, the `Goal` half. Closes the Phase-7 gap
called out in CLAUDE.md ("kernel `Goal<M>` is NOT built — only the `LLM<Caps>`
half of slice 5 shipped"). Composes with Slice 1 (`Principal`/`Budget`), the
`LlmGateway` (Slice 5 LLM half), and `Supervisor` (Slice 3).

NOTE: this spec is the KERNEL realization (interp-only builtins, like the other
kernel services). The *language-level* `Goal<M>` value type (a goal you pass,
split, and thread as a typed value, with `M` the metric/model type) remains a
separate, deferred surface — out of scope here, same two-track split as the rest
of the kernel (the kernel struct is the semantic core; the language type is sugar).

### 3. Surface (what the user writes)

Interp-only builtins (no codegen; E0910-refused under `axon build`, like the other
`kernel_*` / `principal_*` builtins), keyed to a `Principal` handle from Slice 1:

```axon
let root = principal_root("root", /*net*/true, /*fs*/false, /*exec*/false, /*budget*/100)
// Create a goal: optimize the @[adaptive] fn `name` toward `target`, scoped to
// `principal` (its budget bounds total spend). Returns an opaque goal handle.
let g = kernel_goal_create(root, "tune_metric", /*target*/ 0.0)
// Run up to `max_evals` evaluations. Each evaluation debits the principal's
// budget by 1 (or by the LlmGateway token-cost if the metric makes AI calls).
// Returns the best score; REFUSES (exit 7 / E1604) if the budget is exhausted
// mid-run — the partial best is still queryable.
let best = kernel_goal_run(g, /*max_evals*/ 20)
let score = kernel_goal_best_score(g)     // best observed score (no new spend)
let spent = kernel_goal_spent(g)          // evaluations charged to the principal
let live  = kernel_goal_budget_left(g)    // principal budget remaining
```

### 4. Semantics (what it does) — behavior table

A `KernelGoal` is a registry entry (like `PrincipalRegistry` / fibers): an opaque
`usize` handle into an interp-owned `RefCell<Vec<KernelGoal>>`. It records
`{principal, name, target, evals_spent, best_score, best_input}`.

| # | Scenario | Behavior |
|---|----------|----------|
| B1 | `kernel_goal_create(p, name, target)` with `name` an `@[adaptive]` fn | new handle; `evals_spent=0`; no spend yet |
| B2 | `kernel_goal_create` with an unknown/ non-adaptive `name` | typo guard → `Flow::Panic` (exit 101), like `goal_run` |
| B3 | `kernel_goal_run(g, k)` with principal budget ≥ k | runs ≤ k evals via the existing optimizer; debits the principal `min(k, evals_run)`; returns best score |
| B4 | `kernel_goal_run(g, k)` with budget < k | runs until the budget hits 0, then STOPS and exits **7** (`E1604` — goal budget exhausted); `best_score`/`best_input` reflect the work done so far (queryable) |
| B5 | metric makes AI calls via the principal's `LlmGateway` | each call's µ$ cost is debited from the SAME principal budget (one budget, both axes); B4 overrun applies |
| B6 | `kernel_goal_best_score(g)` / `_spent` / `_budget_left` | pure queries; NO new spend; deterministic |
| B7 | a second `kernel_goal_run` on the same handle | resumes (warm-start, like `goal_continue`); continues debiting; B4 still bounds total |

Budget conservation invariant: `sum of all evals + LLM µ$ charged to principal P
across all its goals ≤ P.budget.cap`. The kernel debits P's `Budget` (Slice 1);
when `remaining()==0`, the next charge is refused (B4).

### 5. Type rules

None new at the type level (interp builtins return `i64`/`f64`/opaque handles, as
the other `kernel_*` builtins do). The language-level `Goal<M>` type is deferred.

### 6. Error codes

- **E1604** — kernel goal budget exhausted. Process exit code **7** — VERIFIED FREE:
  interp.rs defines 3=verify, 4=halted, 5=ai-policy, 6=refine (101=panic, 2=static);
  7 is unused, so add `GOAL_BUDGET_EXIT_CODE: i32 = 7` alongside them. A supervisor
  can then branch on "goal ran out of budget" distinctly from a crash, exactly as
  corrigible-halt got its own 4.
- Unknown/non-adaptive `name` at create → `Flow::Panic` (101), matching `goal_run`.

### 7. Invariants touched

- **I-4 (authority):** a goal can only be created against a `Principal` the caller
  holds; spend is bounded by that principal's granted budget — no goal can exceed
  its principal's authority. Unforgeable opaque handles (like the kernel's other
  `*mut`/index handles).
- **Budget conservation:** total charge ≤ granted budget (B4). The single source
  of truth is the Slice-1 `Budget`; the goal never mutates a budget it wasn't
  scoped to.
- **I-2:** interp-only (no codegen divergence to manage); deterministic under
  `AXON_AI_MOCK`/`AXON_SEED` so the test plan is reproducible.

### 8. Test plan (maps 1:1 to §4)

- B1/B2: create on an adaptive fn (handle returned) vs an unknown fn (panic 101).
- B3: budget 20, `kernel_goal_run(g, 10)` → 10 evals charged, `budget_left`=10.
- B4 (load-bearing): budget 5, `kernel_goal_run(g, 100)` → stops at 5 evals, exit
  7, `best_score` reflects 5 evals (partial work preserved + queryable).
- B5: a metric that AI-calls under the principal's gateway → µ$ debits the SAME
  budget; overrun → exit 7.
- B6: queries don't move `spent`/`budget_left`.
- B7: two runs accumulate spend; total still bounded by B4.
- Determinism: identical results under fixed `AXON_SEED` + `AXON_AI_MOCK=1`.

### 9. Acceptance criteria (the done gate)

- [ ] `KernelGoal` struct + registry on the interp (mirrors `PrincipalRegistry`).
- [ ] `kernel_goal_create/run/best_score/spent/budget_left` builtins (interp).
- [ ] Spend debits the Slice-1 `Principal.budget`; B4 overrun → exit 7 (E1604).
- [ ] AI calls inside the metric route through the principal's `LlmGateway` and
      debit the same budget (B5).
- [ ] Codegen E0910-refuses the new builtins (interp-only, sound by refusal).
- [ ] All B1–B7 gated; deterministic under mock+seed.

### 10. Performance budget

Negligible — a goal handle is a small struct; runs reuse the existing optimizer.
No new allocation hot path. Zero cost when no goal is created (registry empty).

### 11. Rollout & rollback

Additive (new builtins + a registry field). Rollback = drop the builtins; the
existing `goal_run` family + `LlmGateway` are untouched. No migration.

### 12. Open questions

- **Q1 — exit code 7:** RESOLVED — 7 is free (3/4/5/6 used in interp.rs), so a
  dedicated `GOAL_BUDGET_EXIT_CODE = 7`, like corrigible-halt got 4.
- **Q2 — eval cost model:** is one `kernel_goal_run` evaluation worth 1 budget
  unit, or should the unit be the LLM µ$ only (compute evals free)? *Lean: 1 unit
  per eval AND µ$ for AI calls — both consume the principal's grant; a pure-compute
  goal still has a finite eval budget so it can't spin forever.*
- **Q3 — relationship to the interp `goal_run` provenance store:** does a
  `KernelGoal` get its OWN provenance namespace (isolated per goal/principal) or
  share the global one keyed by `name`? *Lean: share by `name` for v1 (reuse the
  engine); isolation is a follow-on if cross-goal leakage matters.*
- **Q4 — language `Goal<M>` type:** deferred; this kernel realization is the
  semantic core it would lower to.
