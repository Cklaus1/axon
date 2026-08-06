# Spec — make Axon a real RLM engine, and take the measurement that decides it

**Status:** DRAFT · 2026-08-06
**Source:** adversarial review (fable) of `RLM_MODE_SPEC.md`, `engine.rs`,
`stateful.rs`, `r9.rs`, `NOTES.md`, `AXON_FOR_RLM.md`, the session spike, and
`interp.rs`. Plus this session's measurements.
**Risk class:** Standard — one language-surface change (M5), the rest are fixes.

---

## Two corrections that reorder everything

**1. The `arr_push` blocker was wrong.** I claimed `stateful.rs`'s chain fixture
was not expressible in Axon because `arr_push` is `([i64], i64) -> [i64]`. The
review ran **all five chain steps end-to-end** — as one program and cell-by-cell
through the session — and got 635 / 315 / 0.49606 with values persisting.
`arr_map` is already generic (`builtins.rs:426`), and `str_split` (:666),
`arr_drop`, `parse_int_or` (:867), `arr_filter` (:432), `arr_fold` (:438) cover
the rest. `arr_push`'s monomorphism blocks the **model-natural imperative path**
(`let mut rows = [] … arr_push(rows, Row{…})`), not expressibility.

So the head-to-head is **not blocked** and is the next measurement, not a distant
one. Everything below is sequenced to let it be taken honestly.

**2. There is a session-wedging bug in the value dump.** A computed string
containing `{` is dumped un-escaped, and every subsequent cell then fails with
`unclosed \`{\` in interpolated string`. **The session is permanently bricked** —
even `let n = 1` is refused, with no recovery path. Models build JSON-ish strings
constantly. This is a one-line fix and it is the highest-severity item here.

## Target — what "best RLM engine" means, measurably

| axis | now | done |
|---|---|---|
| Fluency (R9, primed) | 7/8 | **8/8**, stable ×3, validated on the never-tuned 16-task `tasks_hard.rs` |
| Chain fixture | expressible ✅ | `preflight()` green **through the session driver** |
| `Engine` trait | eval ✅ read ✅ | every method met, none lying about its cost |
| Stateful head-to-head | never run | **Axon scored against CPython's 3/5 blind-reuse** |

Not in scope, and deliberately: `axon session` as a CLI verb, `--require-contained`,
better parse diagnostics as a *model* lever (measured not to work — advice-blindness).

---

## Tier 1 — the session must not brick, and must carry real values

### M1 — escape `{` and `}` in dumped string literals ★CRITICAL★

`value_as_literal` (`interp.rs:857-864`) escapes `\\ " \n \t` but not braces, and
Axon strings interpolate. A value containing `{` wedges the session permanently.
Fix: `{` → `{{`, and `}` → `}}` (lone `}` is literal at `parser.rs:85-87`, but
escape it for symmetry).

**Acceptance:** bind a computed string containing `{` and `}`; the next cell must
succeed and `read` must return the original string byte-for-byte.

### M2 — `value_as_literal` for Option / Result / Tuple / Enum

`let o = parse_int("5")` currently yields `// SKIPPED o: Ok(Int(5))`. All four
have literal syntax (`Some(5)`, `Ok(5)`, `(1, 2)`, `Shape::Circle { r: 5.0 }`)
and fall into the `other => Err` arm (`interp.rs:887-891`). A realistic session
binds `Result` constantly, so this is the cheapest large win.

**Acceptance:** each of the four round-trips through a session and is readable in
the next cell.

### M3 — brace-aware `split_lets`

`scripts/axon_session.py:102-115` is line-based, so a multi-line `let` (a closure
body) gets its opening promoted to module level and its body left in `main` →
garbage `E0000`. Models write multi-line code. Use the brace counting
`split_cell` already does.

### M4 — named functions as values: a check/run divergence

`arr_map(lines, parse_row)` **passes `axon check` and panics at run**
("undefined identifier `parse_row`"). Passing a named fn to a higher-order
builtin is the first thing a model writes. This is a soundness divergence, not
an ergonomics gap.

**Decision required in Step 2:** refuse in the checker, or resolve fn names as
values in the interpreter. Refusing is smaller and honest; resolving is what the
model expects.

## Tier 2 — fluency to 8/8

### M5 — accept `let mut x` as a no-op, with an INFO diagnostic ⚠ language surface

`AXON_FOR_RLM.md` §3 rejected this: *"silently accepting a keyword that means
something elsewhere teaches the wrong model of the language."* **The review
overturns it on its own terms**, and the new evidence did not exist when §3 was
written:

- Rust's `mut` claims "this binding is reassignable". **Every Axon local already
  is**, with no marker — the compiler's own help text says exactly that. So
  accepting it asserts nothing false about Axon. It is *not* like `def` or
  `const`, which name things Axon does not have.
- All three other channels are now **measured failures** on this defect:
  error-side (a perfect diagnostic, not applied, both repair arms — advice
  blindness), prompt-side (the card names `mut` with a numeric example and it did
  not generalise to a string accumulator), repair-side (unchanged).
- The parser is the only remaining channel that needs **zero model cooperation**.

Not overfitting: `let mut acc = …` plus a loop is the dominant accumulator idiom
in every neighbouring language. It is a class fix. Chasing 8/8 with another card
line *would* be overfitting, which is why M7 validates on the never-tuned set.

Emit an **INFO** (not a warning-free silent accept) so a human still learns.

### M6 — generic `arr_push`

Not needed for expressibility; needed for the imperative path a model reaches
for. Interp-side this is the deferred-type treatment `arr_map` already gets.

## Tier 3 — take the measurement

### M7 — run `stateful.rs` head-to-head, model writing Axon

The only measurement that tests Axon's differentiated claim: a typed namespace
kills blind reuse **fail-closed**. CPython's baseline is 3/5 with silent
wrong answers; D2's arm A fixed it by paying **+52% input tokens every turn** for
a shape inventory. Axon's pitch is that the type checker does requirement 6 for
free.

**Report both numbers, and expect them to disagree:** advice-blindness means
refusal protects *correctness* but may cost *throughput* — the model may not
repair a refused cell. Report correctness AND completion, never one alone.

Also re-run R9 primed ×3 and the 16-task `tasks_hard.rs` for M5/M6's effect.

---

## Open questions for Step 2

1. **M4:** refuse in checker, or resolve fn-names-as-values in interp?
2. **M5's diagnostic code** — a new I-code, or reuse an existing INFO?
3. **The help text is wrong at session scope.** The `mut` help says "assign with
   `x = …` (no `let`)", but a cell statement *cannot* reassign a module-level
   binding ("cannot assign to function name"); the working idiom is re-`let`. If
   M5 lands the help fires less often, but where it does it is misleading.

## Known ceiling, recorded not fixed

Per-cell cost is **O(live state)**: a 200k-int array → 1.49MB bindings file → a
trivial cell costs **466ms vs 50ms empty** (~10×), extrapolating to ~2.3s/cell at
1M ints against ipykernel's 2ms warm. Every cell re-lexes, re-parses,
re-type-checks and re-evaluates all live state; cumulative session cost is
quadratic. **Sound at chain-fixture scale, structurally wrong at the
context-offload scale RLM exists for.** Not fixed here — fixing it means an
in-process session, which would trade away the best snapshot/cancel story on the
board (a fresh process per eval means cancel loses nothing, and the session *is*
a plain-text snapshot). Do not build a daemon to fix a cost nobody has yet
measured as binding.

## Stop condition

```
DONE = M1–M6 done or blocked-and-logged
   AND R9 primed = 8/8 stable ×3, and tasks_hard shows no regression
   AND stateful.rs runs against Axon and both numbers are reported
   AND cargo test --workspace shows no new failures vs baseline
```
