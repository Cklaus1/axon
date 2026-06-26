# R21 — Exact `Decimal` fixed-point numeric type

**Status:** Slice 1 landed (interpreter-complete; codegen E0910-refused).
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
`i128::MAX / 10^9 ≈ 1.7 × 10^29`. The cost is a fixed 9-dp precision ceiling
(a literal with >9 fractional digits is a clean compile/parse error, never a
silent truncation). For a money type this is the right trade: exactness and
zero alignment bugs over arbitrary precision.

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
**never a silent wrap**, matching Axon's checked-integer discipline (I-9). Lossy
money math is exactly what this type prevents.

| operator | semantics |
|----------|-----------|
| `+` `-`  | exact i128 add/sub; overflow → panic |
| `*`      | full i128 product rescaled by `10^9`, **banker's rounding** |
| `/`      | scale-9 quotient, **banker's (half-even) default** |
| `%`      | exact remainder on same-scale mantissas |
| `== != < > <= >=` | exact comparison on the mantissa |

- **No implicit `f64` ↔ `Decimal` coercion.** That would defeat exactness; an
  explicit conversion (builtin / `as`) is required. `Decimal` is `is_numeric()`
  (so arithmetic binops type-check) but is its own scalar kind.
- **Division rounding** is explicit via `decimal_div(a, b, mode)`. The `/`
  operator uses the half-even default. Division by zero → graceful panic.

### Rounding modes

`half_even` (banker's, the default — unbiased over many operations, so totals
don't drift), `half_up`, `down` (truncate toward zero), `up` (toward +∞).
A single rounding kernel (`rescale_div`) backs mul, div, and `decimal_round`, so
every rounding path agrees exactly.

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
runtime and **exits 6** (`REFINE_VIOLATION`) — the compiler refuses to let an
invalid (negative) balance exist rather than produce a silent overdraft.

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
builtins / `sandbox_run`. The refusal is a **conservative over-approximation**
(a debug-format scan of the `FnDef`): it can only over-refuse, never let a
miscompile through. The LLVM type (`i128`) and literal lowering ARE wired, so
finishing codegen is a localized follow-up (the binop + builtin IR kernels).

**Deferred (Slice 2, when needed):** native i128 codegen for all ops with a
banker's-rounding IR kernel, gated by an interp↔codegen parity harness.

## 7. Verification

- `crates/axon-core/src/decimal.rs` — 10 unit tests (parse/format round-trip,
  `0.1+0.2==0.3`, exact add/sub/mul, rounding modes, negative half-even,
  overflow-is-error, excess-precision rejection, abs/neg).
- `examples/fintech/ledger.ax` — 11 `@[test]`s (the acceptance suite).
- `examples/fintech/overdraft.ax` — overdraft → exit 6 demo.
- `crates/axon-core/tests/cli_run.rs` — 2 end-to-end tests.
- `scripts/decimal_parity.sh` — locks the interp contracts (exact sum, overflow
  panic, div-zero panic, overdraft exit 6, legal-balance pass) AND the codegen
  E0910 refusal (skips cleanly without LLVM).

## 8. Pipeline touch-points (every visitor arm)

token (`Token::Decimal`), lexer regex, AST (`Literal::Decimal`), parser
(literal + `let`-where), types (`Type::Decimal`, `from_name`, `is_numeric`,
`display`), infer (literal typing), checker (`PRIMITIVE_NAMES`,
`axon_type_to_type`, syntactic-fallback, scalar-kind), comptime (refuse),
complexity (lit bits), fmt (re-emit `Nd`), interp (`Value::Decimal`,
`lit_to_val`, `eval_binop_vals`, `values_equal`, `display`, `as_decimal`,
`SendValue::Decimal` for host_await), builtins (table + dispatch + `to_str`),
codegen (`llvm_type`, `emit_literal`, `infer_expr_sem_type`, E0910 refusal).
