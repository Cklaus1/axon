# Tech Spec — R0: Split interp.rs into an interp/ directory (mirror codegen/)

**Status:** 🟡 Slices 1+2+3 LANDED (2026-06-04). `interp/provenance.rs` (304 LoC,
`51e5a4e`), `interp/value.rs` (262 LoC, `fe102d0`), and `interp/goal.rs` (1069
LoC, `53939d5`) extracted — interp.rs **6790 → 5169 lines (−24%)**, each a pure
code move with gate.sh --strict green (parity_all 21/2/0, the I-2 oracle
unchanged). The `interp/` directory now mirrors `codegen/`.

**Mechanics now proven for BOTH kinds of cluster:**
- *Free-fn clusters* (provenance, value): moved private free fns become
  `pub(super)`; parent keeps `use NAME::*` so unqualified call sites are
  unchanged; `pub use NAME::{…}` re-exports the public API at the original
  `interp::` path (main.rs untouched). `use super::*` in the submodule inherits
  the parent's imports (don't re-`use std::io::Write` etc. — redundant-import
  warning).
- *impl-method clusters* (goal): methods move into a SECOND
  `impl<'p> Interp<'p>` block in the submodule. Inherent methods resolve across
  split impl blocks, so parent call sites are unchanged. A private inherent
  method in a child module's impl is NOT visible to the parent impl (E0624) →
  the moved methods become `pub(super)`; the reverse (submodule calling the
  parent's private `call_fn`/`eval`) works because a child module sees ancestors'
  private items. CRITICAL: `mod NAME;` must be at module scope, NOT left inside
  the impl block the methods were cut from (`error: module is not supported in
  impls`).

**Remaining slices are the HARD TAIL, deferred to focused turns:** the AI
builtins (`ai_complete`/`ai_extract_*`) are INLINE `match` arms inside
`call_builtin` (interp.rs ~4207–4360), not separable methods — so "asi.rs" can't
be a clean impl-method move like goal.rs; it's entangled with the same
`call_builtin` split as slice 5 (`builtins.rs`), which needs the load-bearing
`ok!`-macro → module-scope `macro_rules!` + `want`-closure → `pub(super) fn`
promotion fork (below) resolved first. Only the small AI-policy helpers
(`current_ai_tier/budget/fallback`) are separable, and they're tightly coupled to
those arms. These deserve their own careful effort, not a rushed tail-end pass.
— Low-risk, high-mechanical structural cleanup of the lone remaining monolith.
Pure code-move, zero behavior change.

**Requirement:** none (cross-cutting maintainability). Sibling to R1e (which
modularized codegen's IR surface) — this gives the *other* engine the same
treatment.

---

## The problem (measured 2026-06-04)

`crates/axon-core/src/interp.rs` is **6,749 lines in one flat module** — the
reference execution oracle (I-2). Its sibling engine `codegen/` was already
refactored into a **13-file directory** (`expr.rs`, `builtins.rs`,
`match_pat.rs`, `option_result.rs`, `output.rs`, `asi.rs`, `types.rs`, …). The
interpreter is the only monolith of the two engines.

Line-count breakdown by responsibility (verified):

| Section | Lines | Span | % |
|---|---:|---|---:|
| `call_builtin` (one `match`, ~183 arms) | 2,514 | 2923–5437 | 37% |
| Goal/optimization (`run_goal*`/`hill_climb_*`/history/holdout) | 1,112 | 1229–2341 | 16% |
| Tests | 484 | 6265–6749 | 7% |
| `Interp` core (`build`/`call_fn`/`call_closure`/AI-policy) | 488 | 740–1228 | 7% |
| Provenance / JSONL / AI-call logging / RNG | 398 | 5518–5916 | 6% |
| `eval()` expr dispatch | 376 | 2342–2718 | 6% |
| Value ops (`eval_binop_vals`/`values_equal`/`display`/`fmt_g`) | 347 | 5917–6264 | 5% |
| (smaller: harness, type defs, depth-guard, helpers) | ~830 | — | — |

The top two sections alone are **53% of the file** and are cleanly separable.

---

## Decisive fork

*Is this a true module split (multiple files, `mod` tree) or just section
banners in one file?* — and the real sub-fork: **how do the `call_builtin`
sub-categories share the per-arm `want(n)` / `ok!` boilerplate once the giant
match is cut across files?**

- **(a) `interp/` directory mirroring `codegen/`:** `mod.rs` (Value/Flow/Env/
  Interp struct + run harness), `eval.rs`, `builtins.rs` (the router +
  `call_builtin_{array,str,math,dict,asi}` sub-methods), `asi.rs`, `goal.rs`,
  `provenance.rs`, `value.rs`. Inherent-`impl Interp` methods split across files
  via Rust's split-impl support; `pub(super)` visibility bumps on the shared
  seams (`call_fn`, `current_ai_tier`, goal-history fields, `panic`/`as_*`).
- **(b) Keep one file, add section modules inline** — cosmetic, doesn't reduce
  the cognitive load of a 6.7k-line file.

**→ Lean (a).** It follows a pattern *already proven in this same crate*
(codegen's 13-file split) and the seams are clean: `provenance.rs` and
`value.rs` are dominated by **free functions** (pure moves); `goal.rs` and
`asi.rs` are single-purpose `impl Interp` method clusters.

The sub-fork (the only fiddly part): the `ok!` macro and the `want(n)` closure
are defined *inside* `call_builtin` today. When that match is split into
`call_builtin_array` etc., they must become a **shared crate-private macro +
free fn** (`want(args, n) -> Result<…>`) so each sub-method reuses them. Resolve
in review: promote `ok!` to a `macro_rules!` at module scope and `want` to a
`pub(super) fn` — small, mechanical, the load-bearing decision for the split.

---

## Why this is worth doing

- Attacks the two largest blocks (`call_builtin` 37% + goal 16% = >half).
- Mirrors a proven in-crate pattern → reviewers already know the shape.
- Gives `value.rs`/`fmt_g` a named home — the prerequisite extraction point for
  a *future* shared formatting crate consumed by both engines (the real
  drift-killer for `%.6g`, named in the review but out of scope here).

## Slices (each: pure move, gate green = done)

1. **`provenance.rs`** — `append_*_jsonl`, `sha256_hex`, `json_quote`,
   `ProvRecord`, `read_provenance`. Nearly all free functions. Lowest risk first.
2. **`value.rs`** — `display`/`fields_display`/`fmt_g`/`eval_binop_vals`/
   `values_equal`/`uncertain_parts`.
3. **`goal.rs`** — the 1,112-line optimizer cluster.
4. **`asi.rs`** — `ai_complete`/`ai_extract_*` + AI-policy plumbing +
   `AiBudgetGuard`.
5. **`builtins.rs`** — promote `ok!`/`want` to shared scope (the sub-fork), then
   split `call_builtin` into category routers.
6. **`eval.rs`** — `eval`/`eval_block`/`eval_call`/`eval_binop`/`match_pattern`.

## Risk

- **Low / mechanical.** No behavior change → the 484-line in-file test suite +
  the full parity suite (`parity_all.sh`, now gate-wired) are the net.
- Each slice is independently committable and gate-checkable; abort/revert is
  trivial because nothing's semantics move.
- Does NOT address codegen *drift* (the `%.6g`/overflow dual-impl) — that needs
  a shared semantics crate, a separate larger effort this split merely makes
  easier to start.

## Naming note

Filed as **R0** (no requirement number — it's pure maintainability, sequenced
before the R-series numbering) to avoid implying it's a product requirement.
Rename on review if the index prefers a different scheme.
