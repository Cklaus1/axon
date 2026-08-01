# R21 — Exact `Decimal` fixed-point numeric type

**Status:** Slice 1 landed (interpreter-complete; codegen E0910-refused).
Slice 1.5 (§11) is OPEN and blocks Slice 2 — it closes a live rounding-mode
correctness hazard (§3, Q1), the non-sound E0910 refusal (§6), and Decimal's
missing static refinement discharge (§5). Threat model: §9. Caller failure
contract: §10.
**Motivation:** the fintech-credibility blocker. You cannot handle money on
`f64`: `0.1 + 0.2 != 0.3` in binary floating point, and the error compounds
across a ledger. Axon needs an *exact* base-10 numeric type so money math is
trustworthy for an autonomous agent that moves real value.

## 1. Representation

`Decimal` is an **exact base-10 fixed-point** number:

```
value = mantissa / 10^SCALE        SCALE = 9 (fixed, global)
```

- **`mantissa: i128`** — the only stored datum. The rational value is the
  mantissa divided by `10^9`.
- **`SCALE = 9`** fractional digits, the SAME for every `Decimal`.
- Runtime: `Value::Decimal(i128)` (interp); AST literal `Literal::Decimal(i128)`;
  semantic type `Type::Decimal`; LLVM type `i128`.

### Why a single fixed scale (the tradeoff)

The alternative — a `{ i128 mantissa, u8 scale }` per-value pair — buys flexible
precision at the cost of **scale-alignment logic on every operation** (and the
bugs that come with it: which scale wins on `a + b`? does `==` normalize first?).
A single global scale makes the common operations **trivially sound**:

| op | with fixed scale |
|----|------------------|
| `+ - == != < > <= >=` | direct i128 ops on the mantissa, no alignment |
| `*` | `(a*b)` is scale 18 → divide by `10^9`, round half-even |
| `/` | `a*10^9 / b`, round per explicit mode |

**Range vs precision:** 9 dp covers every fiat currency (2–4 dp) and crypto
(≤8–9 dp) with headroom. The i128 mantissa keeps the integer part enormous:
`i128::MAX / 10^9 ≈ 1.7 × 10^29` — **but only for `+`/`-`/compare**.
*(Corrected 2026-07-31 — the original claim overstated the usable range; the
first correction understated the `*` onset, fixed same day:)* `*`
takes the full scale-18 intermediate product in i128, so it panics whenever
`mantissa_a * mantissa_b` overflows i128 even when the true scale-9 result
would fit. For two equal operands the panic onset is a value of
`sqrt(i128::MAX) / 10^9 ≈ 1.3 × 10^10`; between ≈ 1.3×10^10 and
`sqrt(i128::MAX / 10^9) ≈ 4.1 × 10^14` the true scale-9 result would fit yet
the intermediate overflows (e.g. two operands of 2×10^14 panic despite an
in-range product). `/`
scales the numerator by `10^9` first, capping the *dividend* at
`i128::MAX / 10^18 ≈ 1.7 × 10^20` — a value near the 1.7×10^29 ceiling cannot
be divided even by `1d`. See §3 "Intermediate-overflow contract".
The cost is a fixed 9-dp precision ceiling
(a literal with >9 fractional digits is a clean compile/parse error, never a
silent truncation). For a money type this is the right trade: exactness and
zero alignment bugs over arbitrary precision.

### `Decimal` is unit-less — money is not a number *(added 2026-07-31)*

A `Decimal` carries a **quantity and nothing else**: no currency, no
rate-vs-amount distinction, no dimension. `usd + eur` type-checks. `amount *
rate` and `amount * amount` are indistinguishable to the compiler. Unit safety
is **the caller's responsibility**, not this type's — stated here so it is a
known scope boundary rather than a silent assumption (see §11 for the deferred
dimension-checking slice and §12 Q6).

The defence is available today at zero runtime cost, because named struct types
are **nominally** distinguished despite the language's structural-typing
principle:

```axon
type Usd = { v: Decimal }
type Eur = { v: Decimal }
fn pay(amount: Usd) -> Decimal { amount.v }
pay(Eur { v: 100.0d })   // E0102 / E0306 — expected `Usd`, found `Eur`
```

**Recommended pattern:** wrap money in a per-currency newtype and let the
compiler enforce the unit. This matters more as callers are machine-authored:
currency/rate confusion is rare for one human holding a whole ledger in their
head, and common for a generator assembling handlers across a payments API, an
FX feed, and a fee schedule where the unit lives in a JSON field name three
files away. A unit mixup passes exactness, passes interp↔native parity, and
passes `where _ >= 0d` — it produces a number that is precisely, auditably,
exactly wrong. Examples that teach bare `Decimal` as the money type teach the
unsafe pattern; `examples/fintech/ledger.ax` and §5's `withdraw` are to be
converted to the newtype form (§11).

## 2. Literal syntax

A **`d` suffix** on an integer-or-fixed-point form:

```axon
let price   = 19.99d      // Decimal
let tax     = 0.0825d
let qty     = 3d          // whole number, still Decimal
let zero    = 0d
let neg     = 0d - 5.50d  // unary minus is a separate op; -5.50d also lexes
```

- Lexed by `Token::Decimal(String)`, tried **before** the float/int regexes so
  the trailing `d` is consumed (not left as an identifier).
- **No scientific notation** in a literal — a decimal is an exact digit string.
- The captured digit string is parsed into the i128 mantissa **at parse time**
  (`decimal::parse_decimal`), so excess precision / overflow is a clean compile
  error, and the literal NEVER round-trips through `f64`.
- `_` digit separators are allowed (`1_000.50d`).

## 3. Arithmetic & rules

All arithmetic is **checked** — overflow is a graceful panic (exit 101),
**never an observable wrap**, matching Axon's checked-integer discipline (I-9).
Lossy money math is exactly what this type prevents. *(Precision added
2026-07-31: `%` is the one operator implemented via `wrapping_rem` rather than
a checked op — sound only because its sole wrapping input, MIN `%` −1, wraps to
the mathematically exact remainder `0`; see the `%` bullet in the
intermediate-overflow contract below.)*

| operator | semantics |
|----------|-----------|
| `+` `-`  | exact i128 add/sub; overflow → panic |
| `*`      | full i128 product rescaled by `10^9`, **banker's rounding** |
| `/`      | scale-9 quotient, **banker's (half-even) default** |
| `%`      | exact remainder on same-scale mantissas; no overflow case (see contract below) |
| `== != < > <= >=` | exact comparison on the mantissa |

- **No implicit `f64` ↔ `Decimal` coercion.** That would defeat exactness; an
  explicit conversion (builtin / `as`) is required. `Decimal` is `is_numeric()`
  (so arithmetic binops type-check) but is its own scalar kind.
- **Division rounding** is explicit via `decimal_div(a, b, mode)`. The `/`
  operator uses the half-even default. Division by zero → graceful panic.

### Rounding modes

`half_even` (banker's, the default — unbiased over many operations, so totals
don't drift), `half_up`, `down` (truncate toward zero), `up`.
A single rounding kernel (`rescale_div`) backs mul, div, and `decimal_round`, so
every rounding path agrees exactly.

> **⚠ `up` semantics contradiction — a LIVE SLICE-1 hazard, not only a Slice-2
> blocker (rescoped 2026-07-31; Q1 in §12).** The earlier fold-in scoped this as
> "blocks the Slice-2 IR kernel". That scoping was too narrow: the mode is not a
> compile-time implementation detail, it is a **runtime `str` argument to
> shipped, interp-complete builtins**. `RoundMode::from_name` (decimal.rs:50)
> accepts `"up" | "ceil" | "ceiling"` today and `decimal_round`/`decimal_div`
> dispatch it (interp/builtins.rs:524,540), so any `.ax` program **right now**
> can call `decimal_round(x, 2, "ceiling")` on a negative amount and get
> away-from-zero (−1.5 → −2) while this spec, the enum doc comment, and the
> alias name all promise ceiling (−1.5 → −1). Coverage is nil in both suites.
> **Required action is fail-closed, in Slice 1, before Q1 is decided:**
> `RoundMode::Up` and the `"ceil"`/`"ceiling"` aliases must be a hard error at
> the builtin boundary until the semantics are settled. An *unavailable* mode is
> strictly safer than a mode that does the opposite of its documentation. Do not
> carry an undecided semantics forward as a callable mode.
>
> This document is **generator input, not only implementer documentation**: in a
> project whose thesis is that `.ax` is an IR and the surface is prose compiled
> to AST (`axon intent compile` feeds prose specs to a model), a model asked to
> "round merchant fees up" reads the mode list, sees `up`, and emits it — and
> the sign-dependent divergence lands on refunds and chargebacks, which is
> exactly where negative amounts live. Two contradictory ground truths are a
> hazard proportional to the number of authors consuming them, and that number
> is rising.
>
> Evidence: this spec previously defined `up` as "(toward +∞)", and
> `decimal.rs` agrees in prose (the enum doc says "Round toward +∞ (ceiling)"
> and `from_name` aliases `"ceil"`/`"ceiling"` to it) — but the **landed kernel
> implements away-from-zero**: `rescale_div` sets `RoundMode::Up => round_away`
> unconditionally and does `q - 1` for a negative numerator, so `-1.5` rounds
> to `-2` (away from zero), not `-1` (ceiling). No unit test exercises `Up` at
> all (only Down/HalfEven are covered) and `decimal_parity.sh` has no
> rounding-mode case, so the divergence is unlocked. **The intended semantics
> must be decided and the losing side (spec+doc+aliases, or `rescale_div`)
> fixed, with negative-operand `Up` unit tests added, BEFORE the Slice-2 IR
> kernel is written** — otherwise the implementer has two contradictory ground
> truths and "parity" is ill-defined.

### Rounding policy is untyped runtime data *(recorded 2026-07-31)*

`decimal_round(Decimal, i64 dp, str mode)` and `decimal_div(Decimal, Decimal,
str mode)` take the rounding policy as a bare `str`. Three gaps, all verified:

1. **No static validation even for a literal.** `decimal_round(x, 2,
   "hlaf_even")` passes `axon check` clean and fails only at runtime with
   `panic: decimal_round: unknown rounding mode`. Every other
   constant-argument error class in this language is a checker diagnostic; a
   typo in a rounding mode is a production incident instead of a build failure.
2. **Non-constant modes are unconstrained.** Nothing prevents the mode string
   from arriving from config, a file, a network response, or `ai_complete`
   output. §4 does not require a literal and no checker rule enforces one.
3. **No record of the policy used.** `axon trace` records AI calls and tiers;
   nothing records that a computation rounded `down` rather than `half_even`.

This is a **threat-model** gap, not a usability nit (§9). Rounding *direction*
is the canonical value-leak channel in financial software — round credits down,
debits up, pocket the residue — and it is invisible to every acceptance
criterion this spec currently defines: it passes exactness (each op is exactly
what the named mode says), passes interp↔native parity (both backends agree),
passes §5's `where _ >= 0d` (no balance goes negative), and passes the §6
corpus (which tests modes in isolation, never the *policy of choosing between
them*). The paragraph above even names the property an adversary would want to
break — half_even is unbiased "so totals don't drift" — and then puts the lever
to break it in an unvalidated string. **Required (§11 slice):** the mode
argument must be a string literal, validated in the checker with an E-code;
non-literal modes rejected or explicitly gated; the effective mode recorded in
the audit record for value-moving operations; and "asymmetric rounding across
credit and debit paths" added to the redteam corpus for `examples/fintech/`.
Promoting the mode to a real enum/refined type (checkable rather than
parseable) is the better end state and interacts with Q2's C-ABI question.

### Intermediate-overflow contract (normative for Slice-2 parity)

The panic points of the interp kernel are part of the observable contract that
the Slice-2 IR kernel must reproduce **exactly** (same sites, same exit 101):

- **`*`** panics when the scale-18 intermediate `mantissa_a * mantissa_b`
  overflows i128 (`checked_mul` in `mul`, decimal.rs) — even when the true
  scale-9 product would fit in range. The Slice-2 lowering must use
  `llvm.smul.with.overflow.i128` on the *intermediate* product, not the final
  rescaled value.
- **`/`** panics when `mantissa_a * 10^9` overflows i128 (numerator scaling in
  `div`), i.e. for any dividend whose **mantissa** exceeds
  `i128::MAX / 10^9 ≈ 1.7 × 10^29` — equivalently whose **value** exceeds
  `i128::MAX / 10^18 ≈ 1.7 × 10^20` — regardless of the divisor. *(Corrected
  2026-07-31, same day: an earlier fold-in wrote the value cap as the mantissa
  cap, placing the panic 10^9 too early.)*
- **`%`** has **NO overflow panic site** (division-by-zero aside): `rem` uses
  `wrapping_rem`, whose only wrapping input is MIN-mantissa `%` −1-mantissa —
  and there the wrapped result `0` IS the exact remainder, so no wrap is ever
  observable. (MIN mantissa is unreachable via `parse_decimal` but reachable
  via checked `a - b`.) The Slice-2 lowering must **special-case MIN / −1
  before `srem` and yield 0, not trap** — LLVM `srem` is UB on that input, and
  a "checked `%` → panic" reading of §6 item 1 would diverge from the interp.
- `decimal_round` panics on the re-expansion `checked_mul` (round_dp), reachable
  only near the extreme of the mantissa range.

These ceilings are conservative but *defined*; widening them (e.g. a 256-bit
intermediate) is a semantic change requiring its own slice, not a codegen
"optimization".

## 4. Builtins

| builtin | signature | notes |
|---------|-----------|-------|
| `decimal_from_str` | `(str) -> Result<Decimal,str>` | parse; `Err` on malformed / >9 dp |
| `decimal_to_str`   | `(Decimal) -> str` | canonical form, trailing zeros stripped |
| `decimal_round`    | `(Decimal, i64 dp, str mode) -> Decimal` | round to `dp` (0–9) |
| `decimal_div`      | `(Decimal, Decimal, str mode) -> Decimal` | explicit-rounding division |
| `decimal_abs`      | `(Decimal) -> Decimal` | checked |
| `decimal_neg`      | `(Decimal) -> Decimal` | checked |

`to_str` is also polymorphic over `Decimal`. `decimal_from_str` / `decimal_to_str`
round-trip exactly.

## 5. Refinement support

A `Decimal` refinement predicate evaluates and is enforced at all four
obligation sites (param / return / struct-field / let-binding). The headline:

```axon
fn withdraw(balance: Decimal, amount: Decimal) -> Decimal {
    let new_balance: Decimal where _ >= 0d = balance - amount
    new_balance
}
```

A withdrawal that overdraws the account fails the `where _ >= 0d` invariant at
runtime and **exits 6** (`REFINE_VIOLATION`) — an invalid (negative) balance
cannot escape the function; the process dies instead of producing a silent
overdraft.

> **⚠ This guarantee is RUNTIME-ONLY and PATH-DEPENDENT (recorded 2026-07-31).**
> An earlier wording said "the compiler refuses to let an invalid balance
> exist". The compiler does no such thing for `Decimal`. Verified side by side:
>
> ```axon
> let bad: i64     where _ >= 0  = 0 - 5      // E1209 at `axon check` time
> let bad: Decimal where _ >= 0d = 0d - 5d    // `axon check` CLEAN; runtime exit 6
> ```
>
> The checker's constant-predicate machinery is i64-based (`const_eval_int`);
> `Decimal` appears in `checker.rs` only as type plumbing (`PRIMITIVE_NAMES`,
> literal typing, `from_name`, scalar-kind naming) and never in the predicate
> evaluator. `smt.rs` and `verify.rs` contain **no** `Decimal` support at all,
> so the ∀-inputs SMT discharge that Phase 5 wired into the *default* pipeline
> (514e059) never applies to Decimal. **Decimal therefore has the weakest static
> refinement enforcement of any numeric type in the language** — strictly weaker
> than plain `i64` — which is exactly backwards for the type whose entire
> justification is that mistakes cost money.
>
> **Why this expires as authorship shifts to machines:** a runtime,
> path-triggered check is a reasonable fallback when a human wrote the ledger,
> reviewed the branches, and shipped a handful of them. Across a
> machine-generated corpus the failure mode moves from "the common path is wrong
> and testing catches it" to "a rare reconciliation branch nobody exercised
> produces a negative balance in production". Exit 6 on the taken path is close
> to no guarantee at volume.
>
> It is also the cheapest unrealised lever in this spec: a Decimal refinement
> predicate (`_ >= 0d`, `_ <= cap`, `lo <= _ && _ <= hi`) is a **linear
> constraint over an i128 mantissa** — directly expressible in QF_LIA/QF_BV with
> no new theory, no new solver, and no new syntax, in a project that already has
> the encoder, the `Discharged` set, the default-pipeline wiring, and
> `smt_discharge_parity.sh`. Scheduled as **Slice 1.5** (§11); it must land
> before Slice 2, because native refinement lowering (§6 item 5) then only has
> to carry the residual checks the solver could not discharge.
>
> Until Slice 1.5 lands, §5's obligations are: **all four sites enforced at
> runtime; none statically discharged.**

> Note: this slice also enabled inline refinements on `let`/`own`/`ref`
> bindings generally (`let x: T where P = v`) — previously only param/return/
> field positions desugared the inline `where`. The binding-site obligation was
> already specified (Phase 5) but the surface parse for the `let` form was
> missing; `parse_opt_binding_type` now desugars it.

## 6. Codegen status

**Decimal is INTERP-ONLY in Slice 1.** Native codegen **E0910-refuses** any
function that touches `Decimal` (literal, signature, or `decimal_*` builtin):

```
codegen error [E0910]: native codegen cannot yet lower the `Decimal` fixed-point
type used by `main` — run it on the interpreter (`axon run`); exact Decimal
arithmetic is interp-only in this slice (R21)
```

This is **sound-by-refusal** (I-2): the add/sub/compare ops would be a
straightforward `i128` + `llvm.sadd.with.overflow.i128` lowering, but the
rounding-bearing ops (`*` rescale, `/`, `decimal_round`) require a faithful
banker's-rounding IR kernel. Rather than risk emitting subtly-wrong money IR,
the whole type is refused in codegen — exactly like `host_await` / kernel
builtins / `sandbox_run`. The LLVM type (`i128`) and literal lowering ARE wired,
so finishing codegen is a localized follow-up (the binop + builtin IR kernels).

#### ⚠ The refusal is NOT a sound over-approximation *(corrected 2026-07-31)*

An earlier wording claimed the refusal is "a conservative over-approximation (a
debug-format scan of the `FnDef`): it can only over-refuse, never let a
miscompile through." **That claim is false and is withdrawn.**
`fn_uses_decimal` (`crates/axon-core/src/codegen/mod.rs:171`) checks the
signature via `type_mentions_decimal`, then scans `format!("{:?}", f.body)` for
`Decimal(` or `decimal_`. **A Decimal that reaches a function through a
struct-typed parameter matches none of those**: the param type debug-renders as
`Named("Money")`, there is no Decimal literal, and there is no `decimal_*` call.
Verified — this program emits **zero** E0910 diagnostics and codegen proceeds to
lower the function:

```axon
type Money = { amt: Decimal }
fn twice(m: Money) -> Money { Money { amt: m.amt * m.amt } }
fn main() { println("hi") }
```

It does not produce a wrong binary *today*, but only incidentally: it dies on an
unrelated internal panic (`Result::unwrap()` on `Err(GEPIndex)` at
`codegen/expr.rs:6713`, surfacing as `worker thread panicked`, exit 101, with no
`axon-diag/1` record at all).

**The real containment property is a value-origin argument, stated here for the
first time so it becomes maintained rather than accidental:**

> **Decimal containment invariant (load-bearing).** Every `Decimal` value must
> be *born* in a function the scan does catch — via a Decimal literal, a
> `decimal_*` builtin, or a Decimal-mentioning signature. Given that, no
> clean-scanning function can ever come to hold one in a whole program, so the
> refusal is whole-program sound *by construction of the value sources*, not by
> the scan being complete.

That invariant holds today, is untested, and is **one new Decimal-producing
surface away from failing** — note §8 already lists `SendValue::Decimal` for
`host_await` as a Decimal-carrying channel.

**Slice-2 landing order is therefore ALL-OR-NOTHING.** The tempting incremental
path — land item 1 (binops: the easy `llvm.sadd/ssub/smul.with.overflow.i128`
work) and keep the refusal for the rest — is **forbidden**. The moment binop
lowering exists, this leak stops being a crash and becomes a **silent
wrong-money path**: a raw i128 `*` reached through the struct route has no
`10^9` rescale, so the answer is off by a factor of a billion, with the refusal
never firing. That is precisely the class the refusal exists to prevent. Item 1
must not land before item 2.

**Required fixes (Slice 1.5, §11):**

1. Replace the debug-string scan with a **post-inference** check: refuse any fn
   where the inferred semantic type of any expression, local, param, return, or
   reachable struct field is `Type::Decimal`. Codegen already has
   `infer_expr_sem_type` and `struct_field_sem_types` — exactly the information
   the string scan is guessing at.
2. Add the struct-carrier case above to `scripts/decimal_parity.sh` §2 as a
   refusal assertion. It is currently the only part of the refusal surface with
   **no test**.
3. Make the `GEPIndex` path an E-coded diagnostic rather than an `unwrap` panic.
   As authorship shifts to machines this matters directly: an agent pipeline
   consuming `axon-diag/1` JSON gets nothing actionable from `worker thread
   panicked: Any { .. }`, and an automated build loop cannot distinguish
   "Decimal is unsupported here" from a compiler crash.

### Deferred — Slice 2 (native codegen), scoped 2026-07-31

*(Expanded from the earlier one-sentence deferral, which understated scope.)*
Slice 2 is native i128 codegen for **all** Decimal ops, gated by a full
interp↔native differential parity harness. Concretely:

**Lowering inventory** — **all-or-nothing**: every item must land in one landing
or the E0910 refusal stays. Partial landing (item 1 without item 2) converts the
struct-carrier refusal leak from a crash into a silent wrong-money path — see
"The refusal is NOT a sound over-approximation" above. Slice 1.5 (§11) is a
prerequisite for this whole slice.

1. **Binops** — `+ - * / %` and the 6 comparisons on `i128` mantissas, with
   checked overflow (`llvm.sadd/ssub/smul.with.overflow.i128`) at exactly the
   interp panic sites (see §3 "Intermediate-overflow contract"). Exception per
   that contract: `%` has NO overflow trap — MIN / −1 must be special-cased
   before `srem` (UB in LLVM) to yield 0, matching the interp.
2. **Rounding kernel** — a faithful IR port of `rescale_div` (all four modes,
   negative operands), shared by `*`, `decimal_div`, `decimal_round`.
   **Blocked on Q1 (§12)** — the `up`-mode contradiction must be resolved first.
3. **Builtins by lowering class** — `decimal_abs` / `decimal_neg` are the
   simple-extern class, but `decimal_from_str -> Result<Decimal,str>` and
   `decimal_to_str -> str` are the CLAUDE.md **full-path bespoke-lowering**
   class (str/Result construction, likely axon-rt externs — note i128 over the
   C ABI needs an explicit convention, e.g. pointer-out or two-i64 split).
   `decimal_div` / `decimal_round` take a `str` mode argument (parse-or-panic
   at the call site or in the extern).
4. **`to_str` polymorphism** — an `emit_call` arm for `to_str(Decimal)`.
5. **Refinement enforcement** — §5's runtime obligations (`where _ >= 0d`,
   exit 6) must work in *native* codegen too, which requires Decimal comparison
   lowering inside the runtime-refinement machinery at all four obligation
   sites; without it Slice 2 would silently drop the overdraft guard.

**Gate inversion (do not forget):** `scripts/decimal_parity.sh` §2 currently
ASSERTS the E0910 refusal and that no binary is produced — landing Slice 2
**turns the current gate red by design**. The refusal check must be replaced
(in the same commit) by a full interp↔native differential run; the refusal
assertion must not be deleted before the differential half exists. *(Noted
2026-07-31: this inversion is enforced only via `parity_all.sh`, which
`gate.sh` runs exclusively under `--strict` — a non-strict gate run will not
go red. Slice-2 acceptance must therefore be checked with
`gate.sh --strict` on an LLVM box.)*

**Parity corpus — GENERATED, with the list below as required seeds** *(reworded
2026-07-31; it was previously an enumerated "minimum corpus", which sets the
gate ceiling at whatever someone thought to list).* Decimal is currently absent
from the repo's own differential fuzzer: `grep -n "decimal\|Decimal"
scripts/fuzz_parity.sh scripts/parity_all.sh` returns one incidental f64
comment. R1f built exactly the machinery this type needs (~51 builtins under
differential parity) and the money type — enormous operand space, sign-sensitive
rounding, adversarial half-way ties — is the one type that opted out in favour
of a hand-written list. The repo's own history says which finds bugs: the
inline-IR str Unicode divergence shipped precisely because the fuzz corpus was
ASCII, i.e. enumerated by someone's imagination; the Q1 hole is the same shape,
a shipped/documented/callable mode with zero coverage for the whole life of
Slice 1. **Required:** add a Decimal row to `fuzz_parity.sh` — random i128
mantissas across the range, all four modes, both signs, tie-heavy generation —
compared differentially interp↔native **and against an independent oracle**
(`rust_decimal` or Python `decimal`, which would have caught Q1 immediately).

Required seed cases: all 4 rounding modes × positive AND negative
operands (the repo's known divergence class — cf. inline-IR str Unicode,
checked-arithmetic parity); half-way ties (`.5` cases) for half_even/half_up;
intermediate-overflow panic points for `*` and `/` (§3 contract, exit 101);
**MIN-mantissa `%` −1 → exactly `0`, exit 0** (the `srem`-UB corner — §3 `%`
bullet); div-by-zero (exit 101); refinement violation (exit 6); `decimal_from_str` /
`decimal_to_str` round-trip incl. negative and trailing-zero forms; exit-code
parity byte-identical (extend `exit_code_parity.sh` or equivalent).

## 7. Verification

- `crates/axon-core/src/decimal.rs` — 10 unit tests (parse/format round-trip,
  `0.1+0.2==0.3`, exact add/sub/mul, rounding modes, negative half-even,
  overflow-is-error, excess-precision rejection, abs/neg). *(Gap, recorded
  2026-07-31: no test exercises `RoundMode::Up` or `HalfUp`, and none covers
  negative operands for those modes — see Q1 in §12.)*
- `examples/fintech/ledger.ax` — 11 `@[test]`s (the acceptance suite).
- `examples/fintech/overdraft.ax` — overdraft → exit 6 demo.
- `crates/axon-core/tests/cli_run.rs` — 2 end-to-end tests.
- `scripts/decimal_parity.sh` — locks the interp contracts (exact sum, overflow
  panic, div-zero panic, overdraft exit 6, legal-balance pass) AND the codegen
  E0910 refusal. The refusal half skips **only when no `llvm-config` is on
  PATH**; with LLVM present, a failed codegen build is a harness FAILURE, not a
  skip *(hardened 2026-07-31 — previously any cargo failure, error discarded,
  was reported as "LLVM absent" and the harness passed on the interp half
  alone)*.

### What the verification above does NOT establish *(added 2026-07-31)*

The parity gate proves **kernel self-consistency**: that the interp and native
Decimal kernels agree with each other. R21's stated claim is larger — that money
math is trustworthy for an autonomous agent moving real value. These are
different acceptance bars and only the first is met. Backend disagreement is the
*least* likely failure mode against a competent (or optimizing) author: nobody's
generator attacks `llvm.smul.with.overflow`. The likely failure is a ledger that
is **parity-consistent and still wrong** — rounding-policy asymmetry (§3), a
missing `where` clause (§5), a currency/rate mixup (§1), a partial mutation left
by a mid-batch panic (§10). None of those are detectable by any check listed
above.

The separate acceptance artifact that *would* establish the actual claim is an
**adversarial/redteamed fintech corpus** (§11): ledgers written to pass every
gate here while leaking value. Until that exists, §7 should be read as
"the arithmetic kernel is verified", not "money handling is verified".

## 8. Pipeline touch-points (every visitor arm)

token (`Token::Decimal`), lexer regex, AST (`Literal::Decimal`), parser
(literal + `let`-where), types (`Type::Decimal`, `from_name`, `is_numeric`,
`display`), infer (literal typing), checker (`PRIMITIVE_NAMES`,
`axon_type_to_type`, syntactic-fallback, scalar-kind), comptime (refuse),
complexity (lit bits), fmt (re-emit `Nd`), interp (`Value::Decimal`,
`lit_to_val`, `eval_binop_vals`, `values_equal`, `display`, `as_decimal`,
`SendValue::Decimal` for host_await), builtins (table + dispatch + `to_str`),
codegen (`llvm_type`, `emit_literal`, `infer_expr_sem_type`, E0910 refusal).

## 9. Threat model *(added 2026-07-31)*

R21 previously had **no threat model**. Every hazard it named (rounding
divergence, intermediate overflow, `srem` UB) is a **careless-implementer**
hazard, and every mitigation was a differential-parity test between two backends
of the *same* kernel. This section states the adversary explicitly, because the
project's own thesis (CLAUDE.md, ROADMAP: containment of code the author does
not trust) is the opposite assumption.

### The adversary this spec must survive

Not a tired engineer who slips. A **generator optimizing against whatever gate
is defined** — a machine author producing ledger code at volume, possibly with
its own objective. Against that author, interp↔native parity is the *least*
likely failure mode; a **parity-consistent but value-leaking ledger** is the
likely one.

### What R21 defends, and what it does not

| | |
|---|---|
| **Defends** | the arithmetic *kernel*: exactness by construction, single global scale, checked overflow at defined sites, backend agreement |
| **Does NOT defend** | the *ledger*: unit correctness, rounding policy, value provenance, deployment gating, audit of value movement, state left behind by an abort |

**Nothing in the representation expires.** 9-dp base-10 fixed point is right
regardless of who writes the caller. Everything *around* the number is where the
assumptions bind.

### Money is invisible to every containment mechanism this project has landed

Verified against the tree; each is a live gap, not a hypothetical:

1. **Money does not raise risk.** `derive_risk_from_ast`
   (`crates/axon-core/src/main.rs:5236`) maps effect rows to risk: exec →
   Critical, net+fs → High, net|fs → Medium, empty row → Low. Decimal arithmetic
   is **pure** and carries no effect, so a program whose entire purpose is moving
   balances derives **`Risk = Low`**, `risk_requires_pipeline` is false, and
   `axon deploy` takes the short verify→deploy path — **no simulate, no stress,
   no redteam_check**. The Phase-11 gate chain, this project's main deployment
   defence, does not fire for the one workload whose defining property is
   irreversibility. Phase 11 has an `irreversible` axis in `risk_derive` and R21
   never mentions it.
2. **Provenance is not preserved through parsing.** `decimal_from_str: (str) ->
   Result<Decimal,str>` (§4) is the boundary where external and model-produced
   text becomes money, and the resulting `Value::Decimal(i128)` is
   indistinguishable from one written as a literal. The repo has a landed
   info-flow lattice (`examples/stdlib/tainted.ax`: `Tainted`, `trust_validate`,
   source-trust 0 = AI) whose canonical headline is "an AI-sourced value cannot
   drive a consequential action without passing `trust_validate`" — and the most
   consequential action in the language does not use it.
3. **Value movement leaves no audit record.** Phase 9 F3 audit records carry
   `effect_row` and `principal` for `ai_call` and `agent_action`. Decimal ops are
   pure, so `axon trace` shows an agent's AI calls and **none of its money**.
   Phase 7 meters an LLM budget per principal; nothing meters or attributes
   value.
4. **Rounding policy is an unvalidated runtime `str`** — see §3 "Rounding policy
   is untyped runtime data". Asymmetric rounding across credit and debit paths
   passes exactness, parity, `where _ >= 0d`, and the §6 corpus.

Required mitigations are scoped in §11; the strategic choices they force are
open questions in §12 (Q3–Q6), not decided here.

### Stated limit (an assumption that expires)

**R21 as it stands assumes a human reviews each generated ledger.** Every money
guarantee it offers below the arithmetic kernel is either runtime-and-path
dependent (§5), or depends on the author choosing correctly among unchecked
options (§3 rounding mode, §1 units). Those are acceptable when a person wrote
one ledger, read the branches, and shipped it; they degrade in proportion to the
volume of machine-authored callers. This limit is recorded so it is a **stated
scope boundary that must be revisited**, not an implicit background condition.
Slice 1.5 and the §11 items are what move each guarantee from "reviewed" to
"enforced".

**Non-negotiable:** no item in §11 may weaken the §6 E0910 refusal, the checked
arithmetic discipline (I-9), or the exit-6 refinement kill. Static discharge
*replaces* a runtime check only where it proves the obligation for all inputs;
otherwise the runtime check stays.

## 10. Failure semantics *(added 2026-07-31)*

§3 makes overflow a graceful panic (exit 101) and §5 makes a refinement
violation exit 6. Both are the right **local** decision — refusing beats
wrapping. Neither says anything about the state left behind, and R21 previously
had no atomicity clause, no idempotency requirement, and no link to the
durability machinery this project already landed.

For the actor R21 names — "an autonomous agent that moves real value" — a
process abort partway through a multi-step ledger update is **not** a safe
failure unless the mutations were staged. The panic points are not theoretical
and §1 quantifies them: `*` panics from ≈ 1.3 × 10^10 for equal operands, with a
whole band up to ≈ 4.1 × 10^14 where the true scale-9 result would fit yet the
intermediate overflows. §3 rightly calls these ceilings conservative but
*defined* — but a defined **abort point** is only a safety property if the
caller's state model makes aborting safe.

**Contract (normative for callers):**

1. A Decimal operation may abort the process at the defined points in §3.
2. Therefore money mutations **must be staged** and committed only after all
   Decimal arithmetic for the batch completes.
3. Callers **must be idempotent under supervisor restart**. The intended
   mechanism is `Store<T,C>` plus the Phase-7 / R12 supervisor — which is
   precisely the component that will restart the aborted fiber and re-apply a
   half-written batch. Whether that restart is a recovery or a double-spend is
   decided entirely by this contract.
4. **Acceptance:** `examples/fintech/` must carry a test that takes a
   `*`-overflow panic mid-batch and asserts **no partial ledger mutation
   survives**. §7's suite today tests that the panic happens, never what it
   leaves behind.

## 11. Deferred slices *(added 2026-07-31)*

Scoped work, in landing order. Slice 1.5 gates Slice 2.

**Slice 1.5 — close the Slice-1 correctness and enforcement gaps.**
Must land *before* any Slice-2 codegen.

1. **Fail-closed `up`** — hard-error `RoundMode::Up` and the
   `"ceil"`/`"ceiling"` aliases at the builtin boundary until Q1 is decided
   (§3). Then implement the decision, split or remove the aliases, add
   negative-operand `Up`/`HalfUp` unit tests and a `decimal_parity.sh`
   rounding-mode case.
2. **Post-inference E0910 refusal** — replace the debug-string scan with a
   semantic-type check (§6); add the struct-carrier refusal assertion to
   `decimal_parity.sh` §2; write the containment invariant into §6 (done); make
   the `GEPIndex` path an E-coded diagnostic instead of an `unwrap` panic.
3. **Static refinement discharge for Decimal** — extend the checker's
   constant-predicate evaluator to `Literal::Decimal` so E1209 fires exactly as
   it does on `i64` (the i64/Decimal asymmetry in §5 becomes a regression test);
   add `Type::Decimal` to the SMT encoder as an i128 linear term so
   `where _ >= 0d` is discharged ∀-inputs; add the discharge case to
   `decimal_parity.sh`; state per-obligation in §5 which sites are static vs
   runtime.
4. **Checked rounding-mode argument** — require a string literal, validate it in
   the checker with an E-code, reject or explicitly gate non-literal modes (§3).
5. **Decimal row in `fuzz_parity.sh`** — random i128 mantissas, all modes, both
   signs, tie-heavy, differential interp↔native **plus an independent oracle**
   (§6).

**Slice 2 — native codegen.** As inventoried in §6, all-or-nothing, gated by the
generated parity corpus and `gate.sh --strict`.

**Slice 3 — containment bindings for money** (the §9 gaps; strategic shape open
in §12 Q3–Q5):

- treat Decimal-valued mutation as an irreversibility signal in
  `derive_risk_from_ast`, or require fintech deploys to declare `--risk high`,
  so the simulate→stress→redteam chain actually runs on money code;
- require a Decimal originating from `decimal_from_str` over AI/Net-sourced text
  to clear a `trust_validate` floor before reaching a consequential sink, with a
  `@[test]` in `examples/fintech/`;
- emit a provenance/audit record (including the effective rounding mode) for
  value-moving Decimal operations, so `axon trace` can answer "what did this
  agent *move*" alongside "what did it ask the model".

**Slice 4 — adversarial acceptance corpus.** Ledgers written to pass every
existing gate while leaking value; "asymmetric rounding across credit and debit
paths" is the seed case. This is the artifact that would substantiate R21's
headline claim (§7).

**Deferred, not scheduled — dimension/unit checking.** Compiler-enforced
currency and rate-vs-amount dimensions (§1). The newtype pattern is the
available workaround today; a first-class dimension system is a language-level
change well beyond R21. Recorded here as a known scope boundary (Q6).

## 12. Open questions / strategic questions

- **Q1 (OPEN, 2026-07-31; RESCOPED same day to a SLICE-1 correctness blocker)
  — `up` rounding-mode semantics.** Spec/doc/aliases
  say ceiling (toward +∞); the landed `rescale_div` kernel implements
  away-from-zero (`-1.5 → -2`). Decide the intended semantics, fix the losing
  side (§3 note has the full evidence), and add negative-operand `Up`/`HalfUp`
  unit tests + a `decimal_parity.sh` rounding-mode case. This was filed as a
  Slice-2 blocker; it is in fact **live in Slice 1** — the mode is a runtime
  `str` accepted by shipped builtins, so a wrong-signed result is reachable
  today. **Fail closed first** (hard-error `Up`/`"ceil"`/`"ceiling"` at the
  builtin boundary, §11 Slice 1.5 item 1), then decide. It remains a hard
  blocker for the Slice-2 IR rounding kernel — until resolved, "parity" for
  `up` is ill-defined. Note: in mainstream decimal libraries (e.g. Java `BigDecimal`)
  `UP` conventionally means away-from-zero and `CEILING` means toward +∞; if
  that convention is adopted, the fix is the doc comment + the
  `"ceil"`/`"ceiling"` aliases (which should become a *distinct* ceiling mode
  or be removed), not the kernel.
- **Q2 (OPEN, 2026-07-31) — i128 C-ABI convention for `decimal_from_str` /
  `decimal_to_str` axon-rt externs** (pointer-out vs. two-i64 split) — must be
  chosen before the Slice-2 bespoke lowerings (§6 item 3). Interacts with Q4: if
  the rounding mode becomes an enum rather than a `str`, its ABI is decided
  here too.

The remaining questions are **strategic and deliberately unanswered** — they set
scope for §11 Slice 3 and are recorded rather than guessed at.

- **Q3 (OPEN, 2026-07-31) — should money raise derived risk, and by what
  signal?** Decimal ops are pure, so a fund-transfer program derives
  `Risk = Low` and skips the entire Phase-11 gate chain (§9). Options: (a) treat
  any Decimal-valued mutation as an irreversibility signal in
  `derive_risk_from_ast`; (b) require fintech deploys to pass `--risk high`
  explicitly; (c) introduce a `Value`/`Money` effect so money shows up in the
  effect row like `Net`/`Exec` do. (c) is the most principled and the most
  invasive (it touches the row-polymorphic effect system and every fintech
  signature); (a) risks over-triggering the heavy pipeline on any program that
  merely *computes* with Decimals. Undecided.
- **Q4 (OPEN, 2026-07-31) — should the rounding mode be a type rather than a
  string?** A checked enum/refined type removes the whole class of unvalidated-
  policy failures (§3) but adds a first-class type to the surface language and
  forces a C-ABI decision (Q2). The cheaper interim — literal-only + checker
  validation — is scoped in §11 Slice 1.5 item 4 and does not preclude the
  type later. Undecided which is the end state.
- **Q5 (OPEN, 2026-07-31) — what is the trust floor for a `decimal_from_str`
  result, and where is it enforced?** The landed `Tainted`/`trust_validate`
  lattice makes "AI-sourced value must clear a floor before a consequential
  sink" expressible, but nothing connects it to money (§9). Open: whether the
  floor is a compiler rule (Decimal-from-untrusted-str is a distinct type until
  validated), a userland convention gated by a `@[test]`, or a runtime taint
  carried in `Value::Decimal`. The first is sound and expensive; the second is
  cheap and unenforced.
- **Q6 (OPEN, 2026-07-31) — units/dimensions: newtype convention or language
  feature?** `Decimal` is unit-less (§1) and the nominal-struct newtype gives
  compiler-enforced currency safety today at zero cost. Whether Axon should
  eventually carry dimensions in the type system (and whether that is R21's
  problem at all) is unresolved; recorded so the current state is a known
  boundary rather than an oversight.
