# Flight plan: R51

**Mission:** make `main` run v0.22's sealed suite verdicts on the tree engine with the PSV-1 seal ported onto
R50's data model, while unsealed runs keep the VM and its speed.

**Decisions (Step 2):**
- Q4 pin location → `Interp::build`, computed with `seal.active` → psv/cortex children run `env -i`, so main.rs never sees them.
- Defence in depth → `vm::compile` refuses under seal → the only `Body` constructor; covers all 3 engine gates.
- Q6 closures → option A (`ClosureVal` shares `captured`, marks as typed fields) → keeps veto's per-reference contracts.
- Fork 2 taint → private `Bindings`, every mutator takes `t: u8`, no twins → no site can drop a taint.
- Fork 3 → no deep copy, no address-keyed taint on COW values → COW already blocks aliasing; taint rides the binding.
- Q2 → `eval_t<const T>` monomorphisation → the unsealed tree path carries no taint code.
- Order → S0 merge carries a refuse-all pin; seal pieces land S5→S2→S3→S4; flip at S6 → no unprotected commit.
- ⚠ close call: Q7 → port veto's non-seal interpreter fixes (S0b), changing unsealed behaviour → v0.22 already ratified them, and its suites need them.
- Q1 → no VM taint; S6 records sealed wall time → new spec if too slow.
- Q5 → wasm `nest_cost` may be re-measured with old/new stated; depth ratios fixed.

**needs-human (excluded):** N1 merging `r51/unify` into `main` and freezing `v022/veto`; N2 certifying review.

**Step 1 revisions:** 34 `[REVISED]` markers. Main ones: the pin moves to `Interp::build`; the tree fast paths on
main are seal holes too (VmPin-4); deep-copy and address-keyed struct taint are dropped (TaintPort-1/2); the
S0→S1 order is reversed into a refuse-all pin (S0Merge-1); the evidence scope is 426 rows, not 263 (EG-2); §10
gets numeric bounds (EG-7); "No language change" becomes a stated table.

**Critical path:** S0.1 (merge + refuse-all pin), S1 (T1 truth table + drift test, mutation-proven).

**Shape:** 21 tasks, 7 gate tiers, chain of 19, one parallel track (S0.2/3/4); baseline axon-core 1772/0;
`gate.sh` baseline in `tasks/r51-baseline.md`. Estimated effort `[INFERENCE]`: S0 3-5 days (S0Merge), S2-S5
≈ 8,000-9,500 non-test lines (TaintPort).

**First 3:** S0.1 merge c9ed2ddd with core conflicts + pin; S0.2 scripts; S0.3 cli_run.
