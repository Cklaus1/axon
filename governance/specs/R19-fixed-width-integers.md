# R19 — Fixed-width & unsigned integer support

**Spec ID:** `R19-fixed-width-integers` (advances `REQUIREMENTS.md` R2 type-system; unblocks `R17-freestanding-substrate.md` §12 Q5 — bare-metal MMIO needs unsigned)
**Status:** **Slice A LANDED 2026-06-19 — construction surface complete (let + struct-field + param/call-arg).** Integer literals coerce to fixed-width/unsigned annotations at `let`/`own`/`ref` bindings, struct-literal fields, AND function-call arguments, with compile-time range-check (E1900). Infer owns the coercion (`try_int_literal_coercion`); the checker R06 arg-type check (`is_int_width`) skips the literal case to avoid a double E0306. A non-literal int → unsigned still soundly mismatches (E0102 + `as` hint); unsigned *arithmetic* stays rejected pending Slice B (NO i64-backed half-measure, I-9). Full fast suite green (1004 tests). **Return coercion DEFERRED** — the return path has a separate checker `E0307` + fn-body-type check beyond the infer constraint (reverted, unsound to half-do). **Pending:** return (Slice A-cont-2), width-correct ops (Slice B — the real unlock), codegen+parity (Slice C).
**Risk class:** Structural (touches HM integer inference + codegen ABI; I-2 parity)
**Author / date:** cklaus, 2026-06-19 (ASI build-loop iter 1–4: de-risked → scoped → spec)

> **Why this is a spec and not a one-shot edit:** integer literals are hard-typed `I64`
> (`infer.rs:538`) and the only integer-flex logic that exists is signed-only, widening-only
> (`is_int_widening`, `infer.rs:30`). Making `let a: u32 = N` work *correctly* needs literal
> polymorphism + width-correct semantics + codegen + parity. The tempting minimal fix — "let any int
> bind to any int annotation, i64-backed" — is **unsound** (a `u32` that overflows like `i64` violates
> I-9), so it is explicitly rejected here. This spec scopes the correct, sliced implementation.

---

### 1. Motivation

The `Type` enum already has `I8/I16/I32/U8/U16/U32/U64`, and `Type::from_name` resolves them, so the
*vocabulary* looks present — but they are **non-functional**: `let a: u32 = 4000000000` is **E0102**
("expected u32, found i64") because every integer literal is `I64` with no coercion to a narrower/unsigned
annotation (code-verified, ASI-loop iter 1; memory `unsigned-types-nonfunctional`). Hardware/OS code (R17
MMIO registers, byte buffers, bitfields) is all unsigned fixed-width, so this is a hard prerequisite there;
it is also a general correctness gap (you cannot express a `u8` byte today).

### 2. Requirement link

`REQUIREMENTS.md` **R2** (type system: "edge cases in generic + refinement resolution" → add fixed-width
ints) and **R17 §12 Q5** (the bare-metal prerequisite). Acceptance: `let a: u32 = N`, `fn f(x: u8)`,
`-> u64`, and struct fields of unsigned type all type-check, and unsigned arithmetic is **width-correct**
(wrapping + unsigned div/cmp/shift), native==interp (I-2).

### 3. Surface (what the user writes)

```axon
let a: u32 = 4_000_000_000      // OK (today: E0102)
let b: u8  = 255                 // OK; 256 → compile error (out of range) or wraps per semantics (§4)
fn checksum(buf: [u8]) -> u32 { ... }
type Reg = { flags: u16, status: u8 }
let c = a + 1                    // unsigned wrapping arithmetic; c: u32
```

**Error cases:** `let a: u8 = 300` → out-of-range literal (E19xx, §6); mixing widths without a cast
(`let x: u32 = a_u8 + b_u64`) → E0102 unless an explicit `as` cast is present (the `as` operator already
exists, F8).

### 4. Semantics (behavior table)

| Input class | Behavior |
|---|---|
| integer literal, no annotation | stays the default `i64` (back-compat; literal is `{integer}` defaulting to `i64`) |
| `let x: U = <int literal>` (U any int type) | literal takes type `U`; checked in range of `U` at compile time |
| `let x: U = <int expr of type U>` | OK |
| `let x: U = <int expr of type V>`, U≠V | **E0102** (no implicit cross-width coercion; require `as`) |
| unsigned arithmetic `+ - * / % << >>` | **width-correct**: wrapping on overflow (or checked per the i64 policy), `udiv`/`urem`/`lshr`/unsigned `icmp` — NOT i64 semantics |
| unsigned literal out of range (`u8 = 256`) | compile error (E19xx) |
| `as` cast between widths | uses the existing `as_*` builtin path (F8) |
| codegen of a non-i64 width **before the codegen slice lands** | **E0910-refuse** (sound-by-refusal; interp is the oracle, I-2) — never a wrong binary |

**The soundness rule (load-bearing):** a `u32` must behave like a `u32`, not an `i64` with a label.
Width-correct ops are part of the definition of done; an i64-backed half-measure is out of scope (I-9).

### 5. Type rules

- Integer literals become a **polymorphic integer type** — a fresh inference var tagged "integer", which
  unifies with any concrete integer width and **defaults to `i64`** if unconstrained (Rust's `{integer}`
  model). This replaces the hard `AstLiteral::Int(_) => Type::I64` at `infer.rs:538`.
- `constrain`/unify learns: an integer-literal var unifies with any `I8..U64`; two *concrete* distinct int
  widths do NOT unify (→ E0102, requires `as`).
- `is_int_widening` (`infer.rs:30`) extends to the unsigned ranks (or is superseded by the literal-var
  model). Param/return/struct-field annotation sites get the same literal-coercion as `let` (`infer.rs:577`).
- Interp `Value` must carry the integer **width** (or a width tag) so ops can be width-correct; codegen
  selects the LLVM int type + signed/unsigned op variant by width.

### 6. Error codes

| Code | Trigger | Message shape |
|---|---|---|
| E1900 | integer literal out of range for its annotated width | `literal 256 out of range for u8 (0..=255)` |
| E0102 (reuse) | binding an int of width V to an annotation of width U≠V without `as` | `type mismatch: expected u32, found u8 — add `as u32`` |
| E0910 (reuse) | codegen of a non-i64 width before the codegen slice | `non-i64 integer widths not yet supported by native codegen — use `axon run`` |

### 7. Invariants touched

- **I-2** (interpreter is the oracle): interp lands first + is authoritative; codegen E0910-refuses widths
  until its slice + parity land. No dual-path divergence can ship.
- **I-9** (no silent wrong behavior): the soundness rule (§4) — width-correct ops, range-checked literals —
  is what keeps this from being a silent-lie half-measure.
- **I-7** (`i64` is the default): preserved — an *unannotated* literal still defaults to `i64`.
- **I-1** pipeline order preserved (change is localized to infer + checker + interp + codegen).

### 8. Test plan (maps to §4)

- [ ] Unit (infer): `let a: u32 = N` type-checks; `let a: u8 = 256` → E1900; `u32 + u8` without `as` → E0102.
- [ ] Unit (interp): unsigned wrapping (`u8` 255+1 == 0), unsigned div/cmp (`u32` large values), shifts.
- [ ] CLI e2e: `axon run` of an unsigned program prints width-correct results.
- [ ] Parity (interp↔codegen): the unsigned op suite is byte-identical native==interp (`scripts/` new
      `unsigned_parity.sh`); before codegen lands, assert codegen **E0910-refuses** (sound).
- [ ] Adversarial: out-of-range literals at every width; mixed-width arithmetic; `u64` > i64::MAX.
- [ ] Regression: the full existing suite stays green (literal-polymorphism must not perturb i64 inference).

### 9. Acceptance criteria

- [ ] `fixed_width_int_let_binding_typechecks` (the `let a: u32 = N` repro from iter 1 now passes).
- [ ] `unsigned_arithmetic_is_width_correct` (wrapping + unsigned div/cmp in interp).
- [ ] `unsigned_out_of_range_literal_is_e1900`.
- [ ] `unsigned_parity.sh` green OR codegen cleanly E0910-refuses (per slice).
- [ ] Full suite green (no i64 regression).

### 10. Performance budget

N/A beyond "no regression to i64 codegen." Width selection is compile-time; runtime ops are single
instructions.

### 11. Rollout & rollback (sliced)

| Slice | Deliverable | Gate |
|---|---|---|
| **A — interp typing** | literal polymorphism + annotation coercion at let/param/return/struct (`infer.rs`), range-check (E1900) | full fast suite green |
| **B — interp width-correct ops** | `Value` carries width; wrapping + unsigned div/cmp/shift in interp eval | unit + `axon run` correctness |
| **C — codegen** | LLVM width selection + signed/unsigned op variants; until then E0910-refuse | `unsigned_parity.sh` native==interp |

Each slice is independently revertible; A+B are fast-build (interp oracle), C needs the codegen-parity build.

#### Slice B design (scoped 2026-06-19, code-grounded — Slice A construction surface is now LANDED)

Confirmed by inspection: the interp is **dynamically typed** — `Value::Int(i64)` (`interp.rs:36`) is **flat with
no width**, bindings store bare `Int` (`eval.rs:254`), and `eval_binop` (`value.rs:101+`) dispatches on
`(Op, Int(a), Int(b))` with no type context. So width-correctness **cannot** be inferred at the op site, and
there is **no sound contained increment** (typing unsigned arithmetic while the interp does i64 ops is exactly
the I-9 violation the gate forbids). Slice B is therefore a **structural refactor**:

1. **Width-aware `Value`** — add ONE variant (e.g. `Value::SizedInt { val: i64, width: IntWidth }` capturing
   bits+signedness) and **keep `Value::Int(i64)` as the i64 default** so the ~102 `Int` sites in
   `builtins.rs` (and elsewhere) are *untouched* — only non-i64 widths use the new variant. This isolates the
   blast radius.
2. **Construction coercion** — at the binding sites Slice A already touches (`let`/`own`/`ref` — the interp
   let-arm `Expr::Let { ty, .. }` at `eval.rs:42` **has the annotation `ty` available** ✓; struct fields;
   call args/params), coerce the produced `Int(i64)` → `SizedInt` when the declared type is a non-i64 width.
3. **`eval_binop` arms** — `SizedInt op SizedInt`: width-correct (`+ - *` wrap-or-checked per the existing
   i64 checked-overflow policy at the width boundary; `Div/Rem/<</>>` and comparisons use **unsigned**
   semantics by signedness). Mixed `Int`(bare literal) `op SizedInt` → coerce the literal to the operand width.
4. **`to_str`/display + builtin interactions** — handle `SizedInt` (print the unsigned value; feed builtins).

**Blast-radius risk: MODERATE-HIGH** (a new Value variant + ~4 construction sites + the binop arms + display).
Mitigated by keeping `Int(i64)` as default (most sites untouched) and the full-suite gate. **Per the loop
gate, if this can't land sound+green in one pass, do a contained increment (one width / one op family with
the variant) or land the variant+coercion first and the op-arms next — never an i64-backed half-measure.**

**Blast-radius risk for Slice A** (now landed): literal polymorphism touched the inference core — the
full-suite gate held (1004 green); the let/struct/param-call sites use the shared `try_int_literal_coercion`.

#### Slice B soundness finding (iter 9, 2026-06-19 — why it's not a one-pass autonomous grind)

Deeper analysis revealed two coupled problems that make Slice B test-driven and multi-iteration, **not** a
safe single gate-green pass:

1. **Completeness is required for soundness.** Overflow-producing ops (`*`, `<<`, `+` near the width
   boundary) on a u32 value MUST know its width to mask. A u32 value left as a bare `Int(i64)` at *any*
   missed construction site would compute in i64 and exceed u32 range — **silently unsound (I-9)**. So the
   width-aware `SizedInt` must be produced at **every** static-type-introduction site (let/struct/param/
   return/`as`-cast), not just some. (Right-shift/div/cmp of in-range non-negative values happen to agree
   with i64, but `*`/`<<`/`+`-overflow do **not** — so partial coercion is unsound, not merely incomplete.)
2. **The existing gate cannot verify it.** No test in the current 1004-suite exercises unsigned arithmetic,
   so a green suite would **not** imply soundness — the cardinal anti-pattern (a green that doesn't test the
   thing that matters). Slice B therefore requires its **own dedicated unsigned-arithmetic test suite**
   (wrap at width boundary; unsigned div/rem/shift/cmp; value-flow through let/struct/param/return/array) as
   the real gate, authored alongside the implementation.

**Conclusion:** Slice B is a deliberate, test-driven, multi-iteration effort whose correctness the autonomous
loop's existing gate cannot certify. The loop correctly **stops here** (construction surface is a clean,
useful landing) rather than risk a green-but-unsound grind. Implementation should: (a) author the unsigned
test suite first; (b) add `SizedInt` + comprehensive coercion at ALL static-type sites; (c) width-correct
`eval_binop`; (d) gate on existing-suite (no i64 regression) **AND** the new unsigned suite.

### 12. Open questions

1. **Overflow policy:** wrapping vs checked for unsigned arithmetic. *Default: match the existing i64
   checked-overflow policy (graceful panic), with explicit `wrapping_*` builtins if needed — consistency with
   `native-checked-arithmetic` over silent wrap.*
2. **Literal range-check vs wrap at binding:** `let a: u8 = 256` — compile error (E1900) is the §4 default;
   confirm no example relies on truncation.
3. **Slice A regression risk** is the real unknown — literal polymorphism is the core change; the full-suite
   gate decides whether the model is adopted or reworked. This is why A is its own gated slice.
