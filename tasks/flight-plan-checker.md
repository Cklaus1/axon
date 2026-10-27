# Flight plan — checker-soundness + capability-surface batch

**Mission:** make `axon check` accept exactly what the interpreter can run for refinement and `@[verify]` predicates, shadowed builtins and four O-S1-24 false positives, and make effect reporting see verify predicates, native-module calls and `ai_extract::<T>`; 17 items in one integrated batch.

**Plan options** (Running **Fastest at 90 %**. Want a different one? Default option, not chosen by the user.)
- **Fastest (active):** 10–14 h, ≈ 60 % in range, ≈ 10 % slip, caught by the integrated `--strict` gate (1–2 h re-fix). *Done by tomorrow morning; about 1 in 10 runs lets a small mistake into a lane, and the big final test catches it.*
- **Most reliable:** 14–19 h, `--strict` after each merge (4 gates), ≈ 3 % slip. *Done tomorrow afternoon; the big test runs four times, so mistakes show up sooner.*
- ⚠ production-level: capability/effect reports feed G2, E1203, deploy risk and the VM manifest, and verify enforcement changes user-visible exits. Most reliable costs ≈ +4–5 h; Fastest keeps the strict gate, the agreement fixtures and two sweeps.

**Engineering decisions (canon, from the specs):**
- E-1…E-8 (checker §7) as written; E-6 int literal vs f64 operand → refuse; E-7 `return`/`break` in a predicate → refuse at check; E-8 user type named like a builtin → E0005.
- Purity of verify predicates → E1104 in `effects::check_effects` (cross-spec CAPA#1); E1103 covers names/types/binders only.
- CAP-3 → no full walker merge; one classifier; agreement test. CAP-4 → shared `effect_row_name`. Q3–Q8 (cap §6) as written; `temporal_new` admitted as a clock read.
- Lanes: 3 disjoint-source lanes; append-only tails (`cli_run.rs`, g01 rows, markers) concatenated by the integrator.

**Human decisions (2026-10-27):** NH-1(a) verify binds params (hello/learn/file-budget goals rc 0 → 3 unless CS-13); NH-2(a) field refinement binds the field name; NH-3(d) arrays and `&[T]` copy, `str` moves; Q1(a2) verify may read the clock only; Q2 deep E1209 → separate spec. Needs-human subtree pruned: none.

**Step 1 revisions:** checker 79 `[REVISED]` markers (CSA 19+1 partial, CSD 18); capability 31 (CAPA/CAPD 27+1 partial). Largest: CS-2 binder table per return shape; CS-1 one predicate pass for all files; CAP-1 reach walk over the whole predicate.

**Critical path:** CS-1 predicate-scope pass and CAP-2 `program_reach`. Extra proof: 290-file check sweep = baseline except named files, 108 benchmark programs rc 0, caps sweep diff only where specced, M3760–M3764 stay killed.

**Shape:** 17 items, 26 DAG nodes, 3 tiers (P1 CS-1/2/4; P2 CS-3/5/11, CAP-1/2/4; P3 rest), longest chain 9 items (LaneType) + CAP-1 + integration; 3 parallel tracks. `<TEST_CMD>` = `gate.sh --strict` (≈ 43 min); baseline at 1f6b761a: see build-loop-checker.md; `<RUN_BUDGET>` none; paid spend $0 (no receipt trigger).

**First 3 tasks (parallel):** LaneCap CAP-2 · LaneType-B1 CS-11 · LaneParse CS-10.
