# Spec — make the model-natural accumulator statement legal, then re-measure

**Status:** DRAFT · 2026-08-07
**Source:** adversarial review (fable) of the interpreter for RLM, plus this
session's measurements. Prototype patch at
`scratchpad/option2-interp.patch` (prior session), demonstrated working.
**Risk class:** Structural — one language-surface change (N2), one change to how
a session scopes bindings (N1).

---

## The finding that defines this spec

The model writes one statement, over and over, in every stateful failure:

```
rows = rows + [record]
```

It has **two independent walls**, and fixing either alone leaves the other:

| wall | error | why |
|---|---|---|
| assignment | `E0001` cannot assign to function name | module-level `let` registers as `Symbol::Fn` (`resolver.rs:575`), and assignment to a non-`Symbol::Local` is refused (`resolver.rs:1150`) |
| `+` | `E0102` arithmetic operand has non-numeric type | `+` is undefined for `[T]` and `str` (`checker.rs:3967`) — **verified at LOCAL scope too**, where mutation is already legal |

I previously believed only the first existed and planned to fix it alone. That
would have measured no change, because the statement would still be refused —
the third time this session that a fix revealed a wall behind it. **So both walls
fall in one slice, or the measurement is not attributable.**

Two corrections this also forces:
- `str_concat` **does not exist** (`builtins.rs:2599` references it only inside a
  test). Recommending it — as I did — would have taught a function that isn't there.
- `arr_concat` **does** exist (`builtins.rs:522`) and models never write it, the
  same way they never wrote `arr_push`. Adding builtins models do not reach for
  is not a fluency fix.

## Target

| axis | now | done |
|---|---|---|
| stateful chain correctness | 1/5 | measured, with both walls down |
| R9 primed | 7/8 stable | measured, with both walls down |
| `cargo test --workspace` | 1794/0 | no new failures |

Deliberately **not** a numeric target. Two walls fell; the honest deliverable is
an attributable measurement, not a number promised in advance.

---

## N1 — a cell's statements run where mutation is legal (session scoping)

Today the session promotes cell bindings to MODULE scope so they persist, and
module scope is exactly where assignment is refused. The fix keeps persistence
and moves the bindings inside `main`.

**Mechanism, established by prototype rather than assumed:**
- The dump reads only `Interp::globals` (`interp.rs:1854`); `main`'s locals are a
  local of `call_fn` and are dropped on return.
- **The naive hook captures nothing** — `f.body` is an `Expr::Block` and
  `eval_block` pushes *and pops* its own scope (`eval.rs:643`), so a post-eval
  snapshot is empty. The first prototype hit exactly this.
- Working hook: for `main` at depth 1 with `AXON_DUMP_BINDINGS` set, evaluate the
  block's statements against the outer `env`, snapshot it, merge over globals.
- `goal_met` (injected at `interp.rs:2493`) must be filtered out.
- Session `compose()` emits prior bindings and cell lets INSIDE `main`.

**Known costs, to be handled not discovered:**
- Declared `fn`s can no longer see session bindings (they previously could via
  the globals fallback, `eval.rs:74`). Arguably correct — it forces parameters
  and fails closed — but it is a behaviour change and must be documented.
- **W0006 `unused variable` per unreferenced prelude binding, every cell.** This
  grows with the session and pollutes model-facing stderr. Must be suppressed
  for prelude bindings; a warning storm is its own defect.

**Acceptance:** a `while` loop mutating a persisted binding works and the value
survives to the next cell; a wrong-type cell is still refused and leaves the
session unchanged; no W0006 noise from prelude bindings.

## N2 — `+` concatenates `str` and `[T]` ⚠ language surface

Authorised explicitly by the user, same class as the `mut` decision.

`+` on two strings, or two arrays, has no other meaning in Axon today — it is
purely an error — so defining it introduces no ambiguity. Every neighbouring
language does it, and it is what models write. Interp + checker + infer is
enough for RLM; codegen may reuse the interpolation-concat runtime or
E0910-refuse, but **must not diverge** (invariant I-2).

**Acceptance:** `"a" + "b"` → `"ab"`; `[1] + [2]` → `[1, 2]`; mixed operands
(`[1] + "a"`, `1 + "a"`) still refused; native either matches or refuses.

> **[REVISED — N2 is TWO different jobs, not one, and the string half is nearly
> done already.]**
>
> `crates/axon-core/src/interp/value.rs:681` already contains
> `(Add, Str(a), Str(b)) => Ok(Str(a + &b))`. **The interpreter has implemented
> string concatenation all along**; the refusal is the CHECKER's alone
> (`E0102`, checker.rs:3967) — verified: `"a" + "b"` fails `check` and the
> interpreter arm is never reached.
>
> There is no corresponding `Array` arm, so `[T] + [T]` is unimplemented
> everywhere.
>
> So N2 splits:
> - **N2a — `str + str`: a CHECKER-ONLY change.** The evaluator, and therefore
>   interp/native parity for the string case, is already settled. Much smaller
>   than the spec assumed, and it should land first because it is the one that
>   blocks the R9 fluency task.
> - **N2b — `[T] + [T]`: checker AND evaluator.** A new `Add` arm in
>   `eval_binop_vals`, plus the codegen decision.
>
> This also reframes the defect: for strings, the checker has been refusing
> something the reference oracle can already do — the same
> checker-more-restrictive-than-interpreter shape as M4, in the opposite
> direction. Worth stating in the commit, because "add string concat" and
> "stop refusing string concat" are different claims and only the second is true.

## N3 — re-measure, both walls down

Stateful head-to-head and R9 primed ×3. Report both correctness AND completion:
advice-blindness means a refusal protects correctness while possibly costing
throughput, and either number alone misleads.

---

## Open questions for Step 2

1. **N1's blast radius on non-session programs.** The hook fires only under
   `AXON_DUMP_BINDINGS`, so ordinary `axon run` is untouched — verify that, do
   not assume it.
2. **N2 and `+=`-style repeated concatenation in a loop** — is the interpreter's
   string concat O(n²) over a long accumulation? Measure before shipping a
   primitive that invites accumulation loops.
3. **Does N2 need `arr_concat`/`str_join` deleted?** No — they are not obsolete,
   they are the explicit forms. The inner-loop "delete the superseded path" rule
   does not apply to a public builtin with existing callers.

## Deliberately out of scope

Subprocess timeout in the driver, dict cross-cell dump form, lexer-based cell
splitting, prelude pruning for the O(state) cost. All are real (verified by the
review) and all are logged; none is on the path to an attributable measurement.

**Do not build:** a daemon or forkserver, the `axon session` CLI verb, mutable
module-level bindings language-wide, or more language-card lines.

## Stop condition

```
DONE = N1, N2 done or blocked-and-logged
   AND the stateful head-to-head and R9 primed x3 re-run and BOTH reported
   AND cargo test --workspace shows no new failures vs 1794/0
   AND a while-loop accumulator persists across cells (the headline case)
```

---

## Decisions — Step 2, 2026-08-07

### D1 — sequence: N2a first, then N1, then N2b (engineering)

N2a (`str + str`, checker-only) is the smallest and unblocks the R9 fluency
task on its own, so it is measurable independently. N1 is the largest and its
prototype is proven. N2b (`[T] + [T]`) needs a new evaluator arm and is the one
most likely to surface a codegen decision, so it goes last where a block costs
least.

### D2 — Open Q1, N1's blast radius: gate on the env var, and PROVE it (engineering)

The hook fires only when `AXON_DUMP_BINDINGS` is set. The gate is not the
`if` — it is a test asserting that an ordinary `axon run` with the var UNSET
produces byte-identical output before and after N1. An `if` nobody tested is a
claim, not a guard.

### D3 — Open Q2, quadratic concatenation: measure, do not pre-optimise (engineering)

`Str(a + &b)` allocates a fresh string per concat, so an n-step accumulation is
O(n²) in bytes copied. Measure it at n=10k before shipping. If it is bad, the
answer is NOT to withhold the primitive — a language whose `+` invites a loop
should make that loop work — it is to log the optimisation. Withholding `+` to
avoid a performance shape is how the current situation arose.

### D4 — Open Q3: `arr_concat`/`str_join` stay (engineering)

Not obsolete; they are the explicit forms and have callers. The "delete the
superseded path" rule governs code whose callers we control, not a public
builtin surface.

### D5 — N2b's codegen story (engineering, decided when reached)

Native must match or refuse; never diverge (I-2). Prefer refusing with E0910 if
lowering a generic array concat is not cheap — `arr_push` set exactly this
precedent one commit ago and it is the repo's established answer.

### D6 — `needs-human`: none outstanding

`+` on `str`/`[T]` is a language-surface change and WAS explicitly authorised by
the user in response to a message that named it as such. Recorded here so the
authorisation is auditable rather than assumed.
