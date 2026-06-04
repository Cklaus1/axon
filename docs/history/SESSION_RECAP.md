# Axon Session Recap (2026-05-05 → 2026-05-06)

Snapshot of what happened to Axon this session, what's worth keeping,
and what's worth branching off as a research artifact.

## Working-tree state

11 modified files in `crates/axon-core/src/codegen/`, plus untracked:
- `MITOSIS_AUDIT.md` (hand-audit of Axon repo, byproduct of Mitosis design)
- `examples/goals/` (new directory with Phase-10 surface-format prototype)

Net: ~3,252 insertions, ~3,566 deletions.  Most of the volume is the
IR shim experiment.

## What happened

### Hour 0–3: IR shim experiment

Pursued the inkwell-monomorphization-firewall hypothesis from
`SESSION_STATUS.md`'s blocker list.  Goal: get `cargo build -p
axon-core` to finish in <30 min (was 5h+, never completing).

**Migrated**:
- 8 of 8 codegen modules from raw `self.ir.builder.*` calls to
  `self.ir.X` IR-trait calls (per `MIGRATION.md` recipes).
- output.rs (4 sites, smoke test).
- types.rs (29 sites).
- option_result.rs (29 sites).
- asi.rs (37 sites).
- mod.rs (39 sites).
- match_pat.rs (36 sites).
- expr.rs (262 sites).
- builtins.rs (1,692 → 97 raw sites; 94% reduction).

**Outcomes**:
- ✅ `cargo check -p axon-core` passes in 0.7s post-migration.
- ⚠ `cargo build -p axon-core` ran for 6+ hours without completing
  (was killed for `-Z time-passes` profiling, also stalled).
- ❌ The hypothesis is **NOT confirmed by this experiment** —
  builtins.rs body bodies still expand inkwell-generic call sites
  even though they go through the IR trait surface.  The shim
  helped with memory (sub-2GB peak vs 3.6GB pre-shim) but not with
  time.

### Hour 3+: Pivot to Mitosis (separate repo)

Strategic re-frame from "IR shim is load-bearing" to "the shim is
research; Phase 10 surface UX is what's load-bearing".  See
`/home/cklaus/projects/mitosis/` for the new repo built in
parallel.

Mitosis was applied to the Axon repo as a forcing-function dogfood;
its cartographer correctly identified `module:axon-rt::provenance`
as the #1 spinout candidate (matching the hand-audit at
`MITOSIS_AUDIT.md`).

### Concurrent: small Axon improvements

- **asi.rs IR_REARCH bug fix**: post-IR_REARCH commit `3f759ef` had
  a `this.builder` reference that the rename script missed (closure-
  captured `this: &Self`).  Fixed: `this.ir.builder`.  Without this,
  the codegen-feature build doesn't even compile.
- **types.rs unused-import cleanup**: dropped `BasicType` from the
  use-list (was unused after migration).
- **MIGRATION.md constraint #2**: documented the architectural
  finding that BasicValueEnum signatures across codegen modules
  block per-file IR.3 migration; only output.rs is signature-clean.
  Plus the staging trilemma (atomic A / bridge helpers B /
  abandon C) with my recommendation (A staged via temporary B
  scaffolding).
- **examples/goals/**: new directory with `hello-goal.md` (structured-
  prose surface format) + `hello-goal.ax` (typed-AST artifact it
  compiles to).  Concrete demonstration of the Phase-10 two-track
  architecture from `ROADMAP.md`.

## What's worth keeping

These changes are clean small wins, independent of the IR shim
experiment — they should land regardless:

- **asi.rs bug fix** (~2 lines): fixes a regression in IR_REARCH
  that broke the codegen-feature build.
- **types.rs unused-import cleanup** (~1 line): trivial.
- **MIGRATION.md constraint #2** (+63 lines): documents the
  architectural finding so future iterations don't re-discover it.
- **examples/goals/hello-goal.md + hello-goal.ax**: real Phase-10
  surface artifact.  Valid even if IR shim work is reverted.

These are 4 distinct, small, reviewable commits.

## What's worth branching off

The IR shim experiment itself (~3,000 lines of changes across 7
codegen files) should likely move to a `research/ir-shim-2026-05`
branch:

- Empirical evidence shows the hypothesis is NOT confirmed
  (cargo build still stalls).
- Code compiles (cargo check clean) but build performance is
  unchanged.
- Useful as a documented null result — Mitosis flagged this exact
  pattern (research artifact buried in main) when run against the
  Axon repo.

Procedure:
1. Create branch `research/ir-shim-2026-05` from current HEAD.
2. Cherry-pick the 4 small wins above to a clean branch.
3. Reset `merge-asi-layer3` to that clean state.
4. The research branch keeps the work + null-result findings.

## What's broken

- `cargo build -p axon-core`: still stalls.  Pre-shim it was 5h+ never
  finishing.  Post-shim it's the same shape (memory bounded but time
  unbounded).  The hypothesis-test was inconclusive.
- The fundamental question "what makes axon-core take so long to
  build?" remains open.

## Open strategic questions

From `SESSION_STATUS.md` and unanswered through this session:

- When does Phase 10 (structured-prose surface) become urgent vs
  defensive engineering on the binary?  We hit the answer this
  session: now.  But the binary is still slow and there's no
  working `axon` executable for end-to-end Phase-10 work.
- Cranelift backend (`cargo build -Zcodegen-backend=cranelift`) was
  attempted, also stalled on similar shape.  MLIR remains unexplored.
- `axon-check` (no-default-features) builds in 0.04s and is enough
  for type-checking workflows.  Users could use this as a
  development binary while the codegen-feature build is broken.

## Mitosis's recommendation for Axon

`mitosis analyze /home/cklaus/projects/axon` (with sub-sub-module
detection) ranks these as easiest extractions:

1. `module:axon-rt::provenance` — the #1 spinout candidate.  330 LoC,
   cohesion 1.00, only depends on stdlib + serde_json.  Could become
   `evlog` or similar.
2. `module:axon-core::error` — 305 LoC, 52 pub items.  An
   error-types crate.
3. `module:axon-core::codegen::ir` — 276 LoC, 10 pub items.  The IR
   shim itself, captured as a research artifact for the
   `research/ir-shim-2026-05` branch hypothesis.

See `MITOSIS_AUDIT.md` for the hand-written rank.

## Next session

Suggested order of operations:

1. Cherry-pick the 4 small wins (asi.rs, types.rs, MIGRATION.md,
   examples/goals/) to a clean branch.
2. Branch the rest as `research/ir-shim-2026-05`.
3. Run `cargo build -p axon-core --offline` overnight; if it still
   stalls past 4h, the hypothesis is truly falsified — pivot to
   cranelift / MLIR / abandon-codegen.
4. With or without a working binary: ship the Phase-10 prose-to-AST
   compiler skeleton.  `examples/goals/hello-goal.md` is the input;
   the compiler produces `hello-goal.ax`-shaped output.
5. Apply Mitosis's recommendation: extract `axon-rt::provenance`
   into its own crate (testing the spinout playbook end-to-end).
