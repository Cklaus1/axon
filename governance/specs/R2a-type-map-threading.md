# Tech Spec — R2a: Thread the inferred type map to the back-end (stop re-deriving types)

**Status:** 🟡 Slice 0 LANDED (2026-06-04); full refactor RE-SCOPED — fork-first.
The single deepest structural simplification surfaced by the 2026-06-04
architecture review: expression types are computed **three times** because
Hindley-Milner's result is thrown away.

**⚠️ Scoping correction (2026-06-04, found during slice 1 prep):** the draft
below assumed "the AST has spans but no node identity." That is FALSE at the
`Expr` granularity — `Expr` is a **bare enum with no `span` and no `id` field**
(only `Stmt` carries a span). So slice 1 ("add `Expr.id`, pure addition,
behavior-identical") is NOT cheap: giving a bare enum stable identity means a
struct-wrapper (`Expr { kind, id }`) or a per-variant field, touching a large
share of the **1,369 `Expr::` construct/match sites across 16 files**. Worse,
**monomorphization rebuilds the AST** (`main.rs` builds a fresh `concrete`
Program from `mono.fns` between inference and codegen), so pointer-identity keys
break and any `ExprId` must be assigned PRE-mono and survive the clone. The full
thread-the-map refactor is therefore a multi-day, high-blast-radius change — NOT
an autonomous-loop-sized slice. It should be done deliberately, behind the now-
landed `parity_all.sh` + `fuzz_parity.sh` safety nets, as its own focused effort.

**Slice 0 (landed, the cheap value):** the codegen heuristic's per-builtin
*fixed-shape* cases (`arr_range→[i64]`, `dict_keys→[str]`, `dict_values→[i64]`,
`arr_enumerate/arr_zip→[(i64,i64)]`, `arr_chunk→[[i64]]`, `arr_partition`) were
moved out of inline `if name == "…"` branches threaded into the 160-line
`infer_expr_sem_type` and into a single declarative
`Codegen::fixed_collection_return_type(name) -> Option<Type>` match. This
captures the most-felt slice of R2a's value — "the file every fixed-shape
collection builtin must be registered in" is now ONE focused table (a match arm),
not a branch buried in the heuristic — without the 1,369-site identity rewrite.
The input-propagating cases (arr_reverse/map/filter/… → same slice type as the
arg) stay in the heuristic (structural, not constant). Verified native==interp
on all six fixed shapes; gate.sh --strict green. The HM-type-threading endgame
remains as below, now correctly budgeted.

**Requirement:** R2 (type system). Cross-cuts R1 (codegen) — it deletes the
codegen type-guessing heuristic that every collection builtin must be patched
into.

---

## The problem (measured 2026-06-04)

`infer.rs` runs full Hindley-Milner, then **discards everything but function
signatures**. The discard is literal:

- `lib.rs:861` — `let _subst = infer_ctx.infer_program(&program);`
- `main.rs:2560` — `let _subst = …;`
- `infer.rs:1340` — `pub fn infer_program(&mut self, program) -> Substitution`
  returns only the unifier's variable→type map. There is **no node→type table**,
  and `Expr` AST nodes carry **no inferred-type slot** (the only `ty` fields in
  `ast.rs` are *explicit user annotations* from the parser).

Consequence — expression types are re-derived in **three** independent places
that must agree but are maintained separately:

1. **`infer.rs`** (2,168 LoC) — authoritative HM. Result dropped after
   monomorphization instantiation is resolved.
2. **`checker.rs`** (3,565 LoC, the largest file) — `check_expr` threads its
   **own** `HashMap<String, Type>` scope and re-computes expression types for
   the R01–R12 semantic rules (`checker.rs:843`).
3. **`codegen/mod.rs:1113`** — `infer_expr_sem_type`, a **162-line heuristic**
   (1113–1274) with hardcoded per-builtin special cases (`arr_range→[i64]`,
   `dict_keys→[str]`, `dict_values→[i64]`, `arr_zip→[(i64,i64)]`,
   `arr_chunk→[[i64]]`, …) plus a parallel `local_types: HashMap<String,Type>`
   type environment (`mod.rs:167`) rebuilt during lowering.

The codegen heuristic is the load-bearing smell: **every new collection builtin
must be added to it** (the dict_values slice on 2026-06-04 had to touch it, as
did every `arr_*`), and when it disagrees with HM the result is a silent
mis-layout — the exact bug class the parity harnesses chase.

---

## Decisive fork

*How does codegen (and the checker) learn an expression's type — re-derive it,
or read what HM already computed?*

- **(a) Persist + thread a node→type map.** Give `infer.rs` an output
  `HashMap<ExprId, Type>` (a side table keyed by a stable expression id);
  `infer_program` returns it; the checker and codegen **look up** instead of
  re-deriving. Delete `infer_expr_sem_type` and `local_types`.
- **(b) Annotate the AST in place.** Add `ty: Cell<Option<Type>>` to every
  `Expr` node; HM fills it; downstream reads `expr.ty`.
- **(c) Status quo** — keep three derivations, keep patching the heuristic.

**→ Lean (a), but the fork the spec must resolve first is the KEY: a stable
`ExprId`.** The AST has spans but no node identity. Option (a) needs a key that
survives from parse through inference to codegen. Three sub-options, and this is
the decision that gates all the implementation:

- **(a1) Span-as-key.** Spans already exist on every node and are unique by
  source position. Zero AST change. Risk: desugaring/synthesized nodes (string
  interpolation lowering, `?` expansion) may share or lack spans → map misses.
- **(a2) A `u32` `ExprId` assigned by a single post-parse numbering pass.**
  One `Expr.id: u32` field, filled by a walk before inference. Clean key,
  survives desugar if numbering runs after it. Touches `ast.rs` + every Expr
  constructor.
- **(a3) Pointer/index identity in an arena.** Largest change; rejected unless
  the AST moves to an arena for other reasons.

The fork resolution drives cost: **a1 is ~0 AST churn but leaky; a2 is the
robust choice at the cost of one field + a numbering pass.** Recommendation to
settle in review: **a2**, because the whole point is to *delete* the heuristic
permanently, and a leaky key (a1) would force keeping the heuristic as a
fallback — defeating the purpose.

---

## Why this is high value

- Collapses **three type-derivation implementations to one source of truth.**
- **Deletes** `infer_expr_sem_type` (162 LoC) + `local_types`, removing the file
  every collection builtin currently has to be registered in (the recurring tax
  measured across `arr_*`/`dict_*`).
- **Shrinks `checker.rs`** (the largest file) by replacing its private
  type-scope re-derivation with lookups.
- **Kills a bug class:** codegen's heuristic disagreeing with HM (silent
  mis-layout) becomes unrepresentable — codegen reads HM's answer.

## Slices (gated by the existing parity suite — see `parity_all.sh`)

1. **ExprId infrastructure** (fork a2): add `Expr.id`, a numbering pass, and have
   `infer_program` populate + return `HashMap<ExprId, Type>`. No consumer yet —
   pure addition, behavior-identical. Gate: full suite green (proves nothing
   broke; the map is computed but unused).
2. **Codegen reads the map.** Replace `infer_expr_sem_type` lookups with
   `type_map[expr.id]`; keep the heuristic ONLY as a fallback behind a debug
   assertion that fires when the two disagree (the migration safety net). Gate:
   parity suite + the assertion never fires across the corpus.
3. **Delete the heuristic + `local_types`** once slice 2's assertion has been
   green across the full example corpus. Gate: parity suite.
4. **Checker reads the map** (optional, follow-on): replace `check_expr`'s
   private scope with map lookups. Larger; can defer.

## Risk

- The map must cover desugared/synthesized nodes (interpolation, `?`, implicit
  returns). Fork a2 mitigates by numbering **after** desugar; slice 2's
  disagreement-assertion is the empirical proof of coverage before the heuristic
  is deleted.
- Monomorphization: HM resolves generic instantiations (`infer.rs:1362`); the
  per-call type must be the *instantiated* type codegen needs, not the
  polymorphic scheme. The map records post-substitution types.
- Pure refactor, zero behavior change → the 22-harness parity suite (now
  gate-wired) + the example corpus are the net. No new observable surface.

## Out of scope

The diagnostic-struct unification (3 parallel `AxonError`/`CheckError`/
`Diagnostic` structs + duplicated E-code constants across 5 files) is a sibling
cleanup from the same review — tracked separately (see `R8b` below if written),
not bundled here.
