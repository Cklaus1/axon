//! Runtime `Value` formatting + value-level operations for the interpreter
//! (R0 slice 2 — extracted verbatim from `interp.rs`, zero behavior change).
//! `display`/`fields_display` render values for print/interpolation; `fmt_g`
//! is the `%.6g` float formatter converged onto C's printf (R1f slice 2b);
//! `eval_binop_vals` carries Uncertain<T> confidence through arithmetic;
//! `values_equal` is structural equality; `uncertain_parts` unpacks an
//! Uncertain value. `use super::*` pulls in `Value`, `BinOp`, `R`, `HashMap`.

use super::*;

pub(super) fn uncertain_parts(v: &Value) -> Option<(Value, f64)> {
    if let Value::Struct(s) = v {
        if s.name == SYM_UNCERTAIN {
            let inner = s.fields.get(SYM_VALUE).cloned().unwrap_or(Value::Int(0));
            let conf = match s.fields.get(SYM_CONFIDENCE) {
                Some(Value::Float(c)) => *c,
                _ => 1.0,
            };
            return Some((inner, conf));
        }
    }
    None
}

/// The inner present `value` of a `Temporal<T>`, or `None` otherwise. Used by the
/// Temporal binary-op soft-typing path.
fn soft_temporal_inner(v: &Value) -> Option<Value> {
    if let Value::Struct(s) = v {
        if s.name == SYM_TEMPORAL {
            return Some(s.fields.get(SYM_VALUE).cloned().unwrap_or(Value::Int(0)));
        }
    }
    None
}

/// The plain inner value of a SOFT-TYPED wrapper — `Uncertain<T>` or `Temporal<T>`
/// (both carry a `value` field 0) — for the soft-typing rule that lets such a
/// value flow into a plain-`T` slot (if/while condition, scalar param, scalar
/// return). `None` for any other value. Confidence/horizon are dropped at the
/// T-typed boundary.
///
/// Inline, so the common case (not a struct) costs one compare at every
/// boundary (cost only, R50 S5).
#[inline(always)]
pub(super) fn soft_inner(v: &Value) -> Option<Value> {
    match v {
        Value::Struct(_) => soft_inner_struct(v),
        _ => None,
    }
}

#[inline(never)]
fn soft_inner_struct(v: &Value) -> Option<Value> {
    if let Value::Struct(s) = v {
        if s.name == SYM_UNCERTAIN || s.name == SYM_TEMPORAL {
            return Some(s.fields.get(SYM_VALUE).cloned().unwrap_or(Value::Int(0)));
        }
    }
    None
}

// ── R19 Slice B — width-correct integer helpers ───────────────────────────────

/// Interpret the stored i64 bit-pattern as the unsigned value for the type.
/// For unsigned types, mask to the type's range. For signed types, sign-extend.
fn to_display_val(val: i64, ty: IntWidth) -> i64 {
    match ty {
        IntWidth::U8 => (val as u8) as i64,
        IntWidth::U16 => (val as u16) as i64,
        IntWidth::U32 => (val as u32) as i64,
        IntWidth::U64 => val, // stored as i64 bits; display as unsigned below
        IntWidth::I8 => (val as i8) as i64,
        IntWidth::I16 => (val as i16) as i64,
        IntWidth::I32 => (val as i32) as i64,
    }
}

/// Display value for SizedInt. Unsigned types show as unsigned decimal.
pub(super) fn display_sized(val: i64, ty: IntWidth) -> String {
    match ty {
        IntWidth::U8 => (val as u8).to_string(),
        IntWidth::U16 => (val as u16).to_string(),
        IntWidth::U32 => (val as u32).to_string(),
        IntWidth::U64 => (val as u64).to_string(),
        IntWidth::I8 => (val as i8).to_string(),
        IntWidth::I16 => (val as i16).to_string(),
        IntWidth::I32 => (val as i32).to_string(),
    }
}

/// Width-correct checked arithmetic for SizedInt. Returns the result
/// masked/clamped to the width boundary, panicking on overflow (I-9).
fn sized_checked_add(a: i64, b: i64, ty: IntWidth) -> super::R {
    match ty {
        IntWidth::U8 => (a as u8)
            .checked_add(b as u8)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!("integer overflow: u8 {} + {} exceeds 255", a as u8, b as u8).into(),
                )
            }),
        IntWidth::U16 => (a as u16)
            .checked_add(b as u16)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: u16 {} + {} exceeds 65535",
                        a as u16, b as u16
                    )
                    .into(),
                )
            }),
        IntWidth::U32 => (a as u32)
            .checked_add(b as u32)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: u32 {} + {} exceeds {}",
                        a as u32,
                        b as u32,
                        u32::MAX
                    )
                    .into(),
                )
            }),
        IntWidth::U64 => (a as u64)
            .checked_add(b as u64)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: u64 {} + {} exceeds {}",
                        a as u64,
                        b as u64,
                        u64::MAX
                    )
                    .into(),
                )
            }),
        IntWidth::I8 => (a as i8)
            .checked_add(b as i8)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: i8 {} + {} out of range",
                        a as i8, b as i8
                    )
                    .into(),
                )
            }),
        IntWidth::I16 => (a as i16)
            .checked_add(b as i16)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: i16 {} + {} out of range",
                        a as i16, b as i16
                    )
                    .into(),
                )
            }),
        IntWidth::I32 => (a as i32)
            .checked_add(b as i32)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: i32 {} + {} out of range",
                        a as i32, b as i32
                    )
                    .into(),
                )
            }),
    }
}

fn sized_checked_sub(a: i64, b: i64, ty: IntWidth) -> super::R {
    match ty {
        IntWidth::U8 => (a as u8)
            .checked_sub(b as u8)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!("integer overflow: u8 {} - {} underflows", a as u8, b as u8).into(),
                )
            }),
        IntWidth::U16 => (a as u16)
            .checked_sub(b as u16)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: u16 {} - {} underflows",
                        a as u16, b as u16
                    )
                    .into(),
                )
            }),
        IntWidth::U32 => (a as u32)
            .checked_sub(b as u32)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: u32 {} - {} underflows",
                        a as u32, b as u32
                    )
                    .into(),
                )
            }),
        IntWidth::U64 => (a as u64)
            .checked_sub(b as u64)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: u64 {} - {} underflows",
                        a as u64, b as u64
                    )
                    .into(),
                )
            }),
        IntWidth::I8 => (a as i8)
            .checked_sub(b as i8)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: i8 {} - {} out of range",
                        a as i8, b as i8
                    )
                    .into(),
                )
            }),
        IntWidth::I16 => (a as i16)
            .checked_sub(b as i16)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: i16 {} - {} out of range",
                        a as i16, b as i16
                    )
                    .into(),
                )
            }),
        IntWidth::I32 => (a as i32)
            .checked_sub(b as i32)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: i32 {} - {} out of range",
                        a as i32, b as i32
                    )
                    .into(),
                )
            }),
    }
}

fn sized_checked_mul(a: i64, b: i64, ty: IntWidth) -> super::R {
    match ty {
        IntWidth::U8 => (a as u8)
            .checked_mul(b as u8)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!("integer overflow: u8 {} * {} exceeds 255", a as u8, b as u8).into(),
                )
            }),
        IntWidth::U16 => (a as u16)
            .checked_mul(b as u16)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: u16 {} * {} exceeds 65535",
                        a as u16, b as u16
                    )
                    .into(),
                )
            }),
        IntWidth::U32 => (a as u32)
            .checked_mul(b as u32)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: u32 {} * {} exceeds {}",
                        a as u32,
                        b as u32,
                        u32::MAX
                    )
                    .into(),
                )
            }),
        IntWidth::U64 => (a as u64)
            .checked_mul(b as u64)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: u64 {} * {} exceeds {}",
                        a as u64,
                        b as u64,
                        u64::MAX
                    )
                    .into(),
                )
            }),
        IntWidth::I8 => (a as i8)
            .checked_mul(b as i8)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: i8 {} * {} out of range",
                        a as i8, b as i8
                    )
                    .into(),
                )
            }),
        IntWidth::I16 => (a as i16)
            .checked_mul(b as i16)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: i16 {} * {} out of range",
                        a as i16, b as i16
                    )
                    .into(),
                )
            }),
        IntWidth::I32 => (a as i32)
            .checked_mul(b as i32)
            .map(|v| Value::SizedInt { val: v as i64, ty })
            .ok_or_else(|| {
                super::Flow::Panic(
                    format!(
                        "integer overflow: i32 {} * {} out of range",
                        a as i32, b as i32
                    )
                    .into(),
                )
            }),
    }
}

fn sized_div(a: i64, b: i64, ty: IntWidth) -> super::R {
    if b == 0 {
        return Err(super::Flow::Panic(
            format!("integer division by zero ({} / 0)", ty.name()).into(),
        ));
    }
    let v = match ty {
        IntWidth::U8 => ((a as u8) / (b as u8)) as i64,
        IntWidth::U16 => ((a as u16) / (b as u16)) as i64,
        IntWidth::U32 => ((a as u32) / (b as u32)) as i64,
        IntWidth::U64 => ((a as u64) / (b as u64)) as i64,
        IntWidth::I8 => ((a as i8) / (b as i8)) as i64,
        IntWidth::I16 => ((a as i16) / (b as i16)) as i64,
        IntWidth::I32 => ((a as i32) / (b as i32)) as i64,
    };
    Ok(Value::SizedInt { val: v, ty })
}

fn sized_rem(a: i64, b: i64, ty: IntWidth) -> super::R {
    if b == 0 {
        return Err(super::Flow::Panic(
            format!("integer remainder by zero ({} % 0)", ty.name()).into(),
        ));
    }
    let v = match ty {
        IntWidth::U8 => ((a as u8) % (b as u8)) as i64,
        IntWidth::U16 => ((a as u16) % (b as u16)) as i64,
        IntWidth::U32 => ((a as u32) % (b as u32)) as i64,
        IntWidth::U64 => ((a as u64) % (b as u64)) as i64,
        IntWidth::I8 => ((a as i8) % (b as i8)) as i64,
        IntWidth::I16 => ((a as i16) % (b as i16)) as i64,
        IntWidth::I32 => ((a as i32) % (b as i32)) as i64,
    };
    Ok(Value::SizedInt { val: v, ty })
}

fn sized_cmp(op: &BinOp, a: i64, b: i64, ty: IntWidth) -> bool {
    // Unsigned types: compare as unsigned values; signed: compare as signed.
    match ty {
        IntWidth::U8 => {
            let (au, bu) = (a as u8, b as u8);
            match op {
                BinOp::Eq => au == bu,
                BinOp::NotEq => au != bu,
                BinOp::Lt => au < bu,
                BinOp::Gt => au > bu,
                BinOp::LtEq => au <= bu,
                BinOp::GtEq => au >= bu,
                _ => false,
            }
        }
        IntWidth::U16 => {
            let (au, bu) = (a as u16, b as u16);
            match op {
                BinOp::Eq => au == bu,
                BinOp::NotEq => au != bu,
                BinOp::Lt => au < bu,
                BinOp::Gt => au > bu,
                BinOp::LtEq => au <= bu,
                BinOp::GtEq => au >= bu,
                _ => false,
            }
        }
        IntWidth::U32 => {
            let (au, bu) = (a as u32, b as u32);
            match op {
                BinOp::Eq => au == bu,
                BinOp::NotEq => au != bu,
                BinOp::Lt => au < bu,
                BinOp::Gt => au > bu,
                BinOp::LtEq => au <= bu,
                BinOp::GtEq => au >= bu,
                _ => false,
            }
        }
        IntWidth::U64 => {
            let (au, bu) = (a as u64, b as u64);
            match op {
                BinOp::Eq => au == bu,
                BinOp::NotEq => au != bu,
                BinOp::Lt => au < bu,
                BinOp::Gt => au > bu,
                BinOp::LtEq => au <= bu,
                BinOp::GtEq => au >= bu,
                _ => false,
            }
        }
        // Signed narrow types — compare as their native type for sign-correctness.
        IntWidth::I8 => {
            let (ai, bi) = (a as i8, b as i8);
            match op {
                BinOp::Eq => ai == bi,
                BinOp::NotEq => ai != bi,
                BinOp::Lt => ai < bi,
                BinOp::Gt => ai > bi,
                BinOp::LtEq => ai <= bi,
                BinOp::GtEq => ai >= bi,
                _ => false,
            }
        }
        IntWidth::I16 => {
            let (ai, bi) = (a as i16, b as i16);
            match op {
                BinOp::Eq => ai == bi,
                BinOp::NotEq => ai != bi,
                BinOp::Lt => ai < bi,
                BinOp::Gt => ai > bi,
                BinOp::LtEq => ai <= bi,
                BinOp::GtEq => ai >= bi,
                _ => false,
            }
        }
        IntWidth::I32 => {
            let (ai, bi) = (a as i32, b as i32);
            match op {
                BinOp::Eq => ai == bi,
                BinOp::NotEq => ai != bi,
                BinOp::Lt => ai < bi,
                BinOp::Gt => ai > bi,
                BinOp::LtEq => ai <= bi,
                BinOp::GtEq => ai >= bi,
                _ => false,
            }
        }
    }
}

fn sized_shl(a: i64, shift: u32, ty: IntWidth) -> super::R {
    let v = match ty {
        IntWidth::U8 => ((a as u8).wrapping_shl(shift)) as i64,
        IntWidth::U16 => ((a as u16).wrapping_shl(shift)) as i64,
        IntWidth::U32 => ((a as u32).wrapping_shl(shift)) as i64,
        IntWidth::U64 => ((a as u64).wrapping_shl(shift)) as i64,
        IntWidth::I8 => ((a as i8).wrapping_shl(shift)) as i64,
        IntWidth::I16 => ((a as i16).wrapping_shl(shift)) as i64,
        IntWidth::I32 => ((a as i32).wrapping_shl(shift)) as i64,
    };
    Ok(Value::SizedInt { val: v, ty })
}

fn sized_shr(a: i64, shift: u32, ty: IntWidth) -> super::R {
    // Unsigned types: logical right-shift (>>); signed: arithmetic right-shift.
    let v = match ty {
        IntWidth::U8 => ((a as u8).wrapping_shr(shift)) as i64,
        IntWidth::U16 => ((a as u16).wrapping_shr(shift)) as i64,
        IntWidth::U32 => ((a as u32).wrapping_shr(shift)) as i64,
        IntWidth::U64 => ((a as u64).wrapping_shr(shift)) as i64,
        IntWidth::I8 => ((a as i8).wrapping_shr(shift)) as i64,
        IntWidth::I16 => ((a as i16).wrapping_shr(shift)) as i64,
        IntWidth::I32 => ((a as i32).wrapping_shr(shift)) as i64,
    };
    Ok(Value::SizedInt { val: v, ty })
}

fn sized_bitand(a: i64, b: i64, ty: IntWidth) -> Value {
    Value::SizedInt { val: a & b, ty }
}
fn sized_bitor(a: i64, b: i64, ty: IntWidth) -> Value {
    Value::SizedInt { val: a | b, ty }
}
fn sized_bitxor(a: i64, b: i64, ty: IntWidth) -> Value {
    Value::SizedInt { val: a ^ b, ty }
}

/// A checked `Decimal` operation's result as a value; its error a panic.
fn decimal_result(r: Result<i128, String>) -> R {
    r.map(Value::decimal).map_err(|m| Flow::Panic(m.into()))
}

/// `a op b` on two `i64`s; `None` for an operator `i64` does not have (it
/// falls through to `eval_binop_vals`' error arm). AX-46: the evaluator calls
/// this before handing both operands to `eval_binop_vals`, so the hot integer
/// case skips the general dispatch; both paths share these semantics.
#[inline]
pub(super) fn int_binop(op: &BinOp, a: i64, b: i64) -> Option<R> {
    use BinOp::*;
    use Value::{Bool, Int};
    Some(match op {
        // Integer arithmetic — checked by default. Overflow is a *graceful
        // panic* (catchable, exits non-zero at the CLI), never a silent
        // wrap: a wrapped value masquerading as success is the worst class
        // of bug for an autonomous consumer (BUG_HUNT #6, ARCHITECTURE
        // INVARIANTS I-9).
        //
        // This used to say "use the `wrapping_*` builtins for intentional
        // modular arithmetic". There are NO such builtins — `axon reference`
        // lists none and `builtins.rs` defines none — so the comment named an
        // escape hatch that was never built. Intentional modular arithmetic has
        // no expression in the language today; logged in tasks/opportunities.md
        // rather than left as a promise in a comment.
        Add => match a.checked_add(b) {
            Some(v) => Ok(Int(v)),
            None => int_overflow(a, "+", b),
        },
        Sub => match a.checked_sub(b) {
            Some(v) => Ok(Int(v)),
            None => int_overflow(a, "-", b),
        },
        Mul => match a.checked_mul(b) {
            Some(v) => Ok(Int(v)),
            None => int_overflow(a, "*", b),
        },
        Div => {
            if b == 0 {
                return Some(Err(Flow::Panic("integer division by zero".into())));
            }
            // `i64::MIN / -1` is the one division that OVERFLOWS: the true
            // answer is 2^63, which i64 cannot hold. `wrapping_div` returned
            // `i64::MIN` — a silent wrong answer, in the same function whose
            // comment says arithmetic must never silently wrap and directly
            // below three arms that use `checked_*`. Native agreed with it, so
            // no parity harness could ever have found this: the reference
            // oracle shared the bug.
            match a.checked_div(b) {
                Some(v) => Ok(Int(v)),
                None => int_overflow(a, "/", b),
            }
        }
        Rem => {
            if b == 0 {
                return Some(Err(Flow::Panic("integer remainder by zero".into())));
            }
            // NOT `checked_rem`: `i64::MIN % -1` is mathematically 0, which
            // i64 holds perfectly well. Rust's `checked_rem` returns `None`
            // there only because the x86 `idiv` instruction traps on the pair,
            // which is a fact about the hardware and not about the answer.
            // Panicking would replace a correct result with a crash, so the
            // wrapping form stays — and native agrees, measured.
            Ok(Int(a.wrapping_rem(b)))
        }
        Eq => Ok(Bool(a == b)),
        NotEq => Ok(Bool(a != b)),
        Lt => Ok(Bool(a < b)),
        Gt => Ok(Bool(a > b)),
        LtEq => Ok(Bool(a <= b)),
        GtEq => Ok(Bool(a >= b)),
        BitAnd => Ok(Int(a & b)),
        BitOr => Ok(Int(a | b)),
        BitXor => Ok(Int(a ^ b)),
        Shl => Ok(Int(a.wrapping_shl(b as u32))),
        Shr => Ok(Int(a.wrapping_shr(b as u32))),
        And | Or => return None,
    })
}

/// The overflow panic of `a op b` on `i64`s; out of line, it is the cold path.
#[cold]
fn int_overflow(a: i64, op: &str, b: i64) -> R {
    Err(Flow::Panic(
        format!("integer overflow: {a} {op} {b} exceeds i64").into(),
    ))
}

/// `a op b` on two `f64`s; `None` for an operator `f64` does not have. The
/// float half of [`int_binop`].
#[inline]
pub(super) fn float_binop(op: &BinOp, a: f64, b: f64) -> Option<R> {
    use BinOp::*;
    use Value::{Bool, Float};
    Some(Ok(match op {
        Add => Float(a + b),
        Sub => Float(a - b),
        Mul => Float(a * b),
        Div => Float(a / b),
        Rem => Float(a % b),
        Eq => Bool(a == b),
        NotEq => Bool(a != b),
        Lt => Bool(a < b),
        Gt => Bool(a > b),
        LtEq => Bool(a <= b),
        GtEq => Bool(a >= b),
        And | Or | BitAnd | BitOr | BitXor | Shl | Shr => return None,
    }))
}

pub(super) fn eval_binop_vals(op: &BinOp, l: Value, r: Value) -> R {
    use BinOp::*;
    use Value::{Bool, Float, Int, Str};

    match (&l, &r) {
        (Int(a), Int(b)) => {
            if let Some(res) = int_binop(op, *a, *b) {
                return res;
            }
        }
        (Float(a), Float(b)) => {
            if let Some(res) = float_binop(op, *a, *b) {
                return res;
            }
        }
        _ => {}
    }

    // ── ASI: Uncertain<T> binary-op propagation ─────────────────────────────
    // If EITHER side is `Uncertain`, operate on the underlying values and carry
    // the MINIMUM confidence forward (a chain is only as certain as its least-
    // certain input). A non-Uncertain side contributes confidence 1.0. The
    // result is itself `Uncertain` (with a bool inner for comparisons) — so
    // confidence flows through arithmetic and `@[verify(confidence >= K)]` can
    // gate it. This is the interpreter counterpart to codegen's
    // `emit_binop_uncertain`; before this, an `Uncertain + Uncertain` fell
    // through to the `unsupported binary op` arm and panicked, so confidence-
    // propagating code could be compiled but not interpreted (PRD gap).
    // Both soft wrappers are structs, so scalar operands (the hot case) skip
    // the probing entirely.
    let soft_operand = matches!(l, Value::Struct(_)) || matches!(r, Value::Struct(_));
    if soft_operand {
        let lu = uncertain_parts(&l);
        let ru = uncertain_parts(&r);
        if lu.is_some() || ru.is_some() {
            let (lv, lc) = lu.unwrap_or_else(|| (l.clone(), 1.0));
            let (rv, rc) = ru.unwrap_or_else(|| (r.clone(), 1.0));
            let inner = eval_binop_vals(op, lv, rv)?;
            let new_conf = lc.min(rc);
            return Ok(make_uncertain(inner, new_conf));
        }
    }

    // ── Temporal<T> binary-op soft typing ────────────────────────────────────
    // A `Temporal<T>` operand is soft-compatible with `T`: operate on the inner
    // PRESENT value and return a PLAIN result (Temporal's horizon/decay aren't
    // meaningfully composed by a single binop, so `t > 5` yields a plain bool,
    // `t + n` a plain int — distinct from Uncertain, which stays Uncertain to
    // carry confidence). Lets `if t > 5` / `t + n` work instead of panicking
    // "cannot apply Gt to Temporal". Matched by codegen's Temporal-binop path so
    // native==interp. The decayed value at a time is read via `temporal_at`.
    if soft_operand {
        let lt = soft_temporal_inner(&l);
        let rt = soft_temporal_inner(&r);
        if lt.is_some() || rt.is_some() {
            let lv = lt.unwrap_or_else(|| l.clone());
            let rv = rt.unwrap_or_else(|| r.clone());
            return eval_binop_vals(op, lv, rv);
        }
    }

    match (op, l, r) {
        // ── R21 — exact fixed-point Decimal arithmetic ────────────────────────
        // Same-scale i128 ops. Checked: overflow / div-by-zero → graceful panic,
        // never a silent wrap (money math must never lie). Division uses the
        // banker's-rounding (HalfEven) default; an explicit mode is available via
        // the `decimal_div` builtin.
        (Add, Value::Decimal(a), Value::Decimal(b)) => decimal_result(crate::decimal::add(*a, *b)),
        (Sub, Value::Decimal(a), Value::Decimal(b)) => decimal_result(crate::decimal::sub(*a, *b)),
        (Mul, Value::Decimal(a), Value::Decimal(b)) => decimal_result(crate::decimal::mul(*a, *b)),
        (Div, Value::Decimal(a), Value::Decimal(b)) => decimal_result(crate::decimal::div(
            *a,
            *b,
            crate::decimal::RoundMode::HalfEven,
        )),
        (Rem, Value::Decimal(a), Value::Decimal(b)) => decimal_result(crate::decimal::rem(*a, *b)),
        (Eq, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(*a == *b)),
        (NotEq, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(*a != *b)),
        (Lt, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(*a < *b)),
        (Gt, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(*a > *b)),
        (LtEq, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(*a <= *b)),
        (GtEq, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(*a >= *b)),

        // String concat. Appends in place when `a` is the only reference (a
        // temporary, as in `s + t + u`); a shared `a` is copied first, so no
        // other binding sees the write. `s = s + t` is `Interp::assign_in_place`.
        (Add, Str(mut a), Str(b)) => {
            Rc::make_mut(&mut a).push_str(&b);
            Ok(Str(a))
        }
        // N2b: `[T] + [T]` is concatenation. Unlike the string arm above — which
        // existed all along and was only refused by the checker — this had no
        // implementation anywhere. Copy semantics, matching `arr_push`: the
        // operands are unaffected and a fresh array is returned.
        (Add, Value::Array(a), Value::Array(b)) => {
            let mut out = a;
            Rc::make_mut(&mut out).extend(b.iter().cloned());
            Ok(Value::Array(out))
        }
        // Bool / string equality
        (Eq, Bool(a), Bool(b)) => Ok(Bool(a == b)),
        (NotEq, Bool(a), Bool(b)) => Ok(Bool(a != b)),
        (Eq, Str(a), Str(b)) => Ok(Bool(a == b)),
        (NotEq, Str(a), Str(b)) => Ok(Bool(a != b)),
        // Logical and/or on already-evaluated bools. `eval_binop` short-circuits
        // these for the common case; this value-level arm is reached when an
        // `Uncertain<bool>` operand routed both sides through here (no
        // short-circuit possible when combining confidences).
        (And, Bool(a), Bool(b)) => Ok(Bool(a && b)),
        (Or, Bool(a), Bool(b)) => Ok(Bool(a || b)),

        // ── R19 Slice B — width-correct SizedInt arithmetic ───────────────────
        // SizedInt op SizedInt: arithmetic uses the left operand's type (both
        // types must agree; the infer/checker gate ensures this at compile time).
        (Add, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_checked_add(a, b, ty)
        }
        (Sub, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_checked_sub(a, b, ty)
        }
        (Mul, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_checked_mul(a, b, ty)
        }
        (Div, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_div(a, b, ty)
        }
        (Rem, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_rem(a, b, ty)
        }
        // Comparisons → bool (unsigned or signed per type).
        (
            Eq | NotEq | Lt | Gt | LtEq | GtEq,
            Value::SizedInt { val: a, ty },
            Value::SizedInt { val: b, .. },
        ) => Ok(Bool(sized_cmp(op, a, b, ty))),
        // Bitwise ops.
        (BitAnd, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            Ok(sized_bitand(a, b, ty))
        }
        (BitOr, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            Ok(sized_bitor(a, b, ty))
        }
        (BitXor, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            Ok(sized_bitxor(a, b, ty))
        }
        (Shl, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_shl(a, b as u32, ty)
        }
        (Shr, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_shr(a, b as u32, ty)
        }
        // Mixed: bare Int literal op SizedInt — coerce the literal to the SizedInt's width.
        // This handles patterns like `255u8 + 1` where the 1 is a bare Int.
        (Add, Value::SizedInt { val: a, ty }, Int(b)) => sized_checked_add(a, b, ty),
        (Sub, Value::SizedInt { val: a, ty }, Int(b)) => sized_checked_sub(a, b, ty),
        (Mul, Value::SizedInt { val: a, ty }, Int(b)) => sized_checked_mul(a, b, ty),
        (Div, Value::SizedInt { val: a, ty }, Int(b)) => sized_div(a, b, ty),
        (Rem, Value::SizedInt { val: a, ty }, Int(b)) => sized_rem(a, b, ty),
        (Add, Int(a), Value::SizedInt { val: b, ty }) => sized_checked_add(a, b, ty),
        (Sub, Int(a), Value::SizedInt { val: b, ty }) => sized_checked_sub(a, b, ty),
        (Mul, Int(a), Value::SizedInt { val: b, ty }) => sized_checked_mul(a, b, ty),
        (Div, Int(a), Value::SizedInt { val: b, ty }) => sized_div(a, b, ty),
        (Rem, Int(a), Value::SizedInt { val: b, ty }) => sized_rem(a, b, ty),
        (Eq | NotEq | Lt | Gt | LtEq | GtEq, Value::SizedInt { val: a, ty }, Int(b)) => {
            Ok(Bool(sized_cmp(op, a, b, ty)))
        }
        (Eq | NotEq | Lt | Gt | LtEq | GtEq, Int(a), Value::SizedInt { val: b, ty }) => {
            Ok(Bool(sized_cmp(op, a, b, ty)))
        }
        (Shl, Value::SizedInt { val: a, ty }, Int(b)) => sized_shl(a, b as u32, ty),
        (Shr, Value::SizedInt { val: a, ty }, Int(b)) => sized_shr(a, b as u32, ty),
        (BitAnd, Value::SizedInt { val: a, ty }, Int(b)) => Ok(sized_bitand(a, b, ty)),
        (BitOr, Value::SizedInt { val: a, ty }, Int(b)) => Ok(sized_bitor(a, b, ty)),
        (BitXor, Value::SizedInt { val: a, ty }, Int(b)) => Ok(sized_bitxor(a, b, ty)),

        // Structural equality for composite values (structs, enums, arrays,
        // Option/Result). Primitives are handled above; this catches the rest,
        // matching the `values_equal` used by `assert_eq`.
        (Eq, l, r) => Ok(Bool(values_equal(&l, &r))),
        (NotEq, l, r) => Ok(Bool(!values_equal(&l, &r))),
        (op, l, r) => panic(format!(
            "cannot apply {op:?} to {} / {}",
            l.type_name(),
            r.type_name()
        )),
    }
}

/// Structural equality for runtime values.
pub(super) fn values_equal(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => x == y,
        // SizedInt equality: compare by value within the width's representable range.
        (SizedInt { val: x, ty: tx }, SizedInt { val: y, .. }) => {
            to_display_val(*x, *tx) == to_display_val(*y, *tx)
        }
        (SizedInt { val: x, ty }, Int(y)) => to_display_val(*x, *ty) == *y,
        (Int(x), SizedInt { val: y, ty }) => *x == to_display_val(*y, *ty),
        (Float(x), Float(y)) => x == y,
        (Decimal(x), Decimal(y)) => x == y,
        (Bool(x), Bool(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (Unit, Unit) => true,
        (None, None) => true,
        (Some(x), Some(y)) | (Ok(x), Ok(y)) | (Err(x), Err(y)) => values_equal(x, y),
        (Array(x), Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| values_equal(p, q))
        }
        (Struct(x), Struct(y)) => x.name == y.name && x.fields.equal(&y.fields),
        (Enum(x), Enum(y)) => {
            x.enum_name == y.enum_name && x.variant == y.variant && x.fields.equal(&y.fields)
        }
        (Tuple(x), Tuple(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| values_equal(p, q))
        }
        (Dict(d1), Dict(d2)) => {
            // Two dicts are equal iff they have the same key set and the
            // values agree pairwise. Iterating BTreeMaps is sorted, so a
            // direct paired scan suffices.
            let m1 = d1.borrow();
            let m2 = d2.borrow();
            m1.len() == m2.len()
                && m1
                    .iter()
                    .zip(m2.iter())
                    .all(|((k1, v1), (k2, v2))| k1 == k2 && values_equal(v1, v2))
        }
        _ => false,
    }
}

/// Render a value for `print`/`println`/string interpolation. A `str` renders
/// as its raw contents (no quotes); everything else gets a reasonable form.
pub(super) fn display(v: &Value) -> String {
    match v {
        Value::Str(s) => String::clone(s),
        Value::Int(n) => n.to_string(),
        Value::SizedInt { val, ty } => display_sized(*val, *ty),
        Value::Float(f) => fmt_g(*f),
        Value::Decimal(m) => crate::decimal::format_decimal(**m),
        Value::Bool(b) => b.to_string(),
        Value::Unit => "()".into(),
        Value::None => "None".into(),
        Value::Some(x) => format!("Some({})", display(x)),
        Value::Ok(x) => format!("Ok({})", display(x)),
        Value::Err(x) => format!("Err({})", display(x)),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(display).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Struct(s) => format!("{} {{ {} }}", sym_name(s.name), fields_display(&s.fields)),
        Value::Enum(e) => {
            if e.fields.is_empty() {
                format!("{}::{}", sym_name(e.enum_name), sym_name(e.variant))
            } else {
                format!(
                    "{}::{} {{ {} }}",
                    sym_name(e.enum_name),
                    sym_name(e.variant),
                    fields_display(&e.fields)
                )
            }
        }
        Value::Closure { .. } => "<fn>".into(),
        Value::Chan(q) => format!("<chan len={}>", q.borrow().len()),
        Value::Tuple(items) => {
            let parts: Vec<String> = items.iter().map(display).collect();
            format!("({})", parts.join(", "))
        }
        Value::Dict(d) => {
            let m = d.borrow();
            let parts: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{k}: {}", display(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        // R13: a handle is opaque — render its nominal identity, never its
        // payload index (which is an internal slab slot, not user-meaningful).
        Value::Handle(h) => format!("<native {}::{}>", h.module, h.name),
    }
}

/// `name: value` per field, sorted as rendered text (so `a1: …` sorts
/// before `a: …`, as it always has).
pub(super) fn fields_display(fields: &Fields) -> String {
    let mut parts: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{}: {}", sym_name(k), display(v)))
        .collect();
    parts.sort();
    parts.join(", ")
}

/// Approximate C's `%.6g` (used by codegen's `to_str_f64`): 6 significant
/// digits, trailing zeros trimmed.
pub(super) fn fmt_g(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.into();
    }
    let p: i32 = 6;
    let e = x.abs().log10().floor() as i32;
    if e < -4 || e >= p {
        // Exponential form. Match C's `%.6g` exactly (the standard the native
        // codegen path emits via snprintf, so I-2 holds): trim trailing zeros
        // from the mantissa AND print the exponent as a sign plus at least two
        // digits — `1e+06`, `1.23457e+06`, `1e-07`. Rust's `{:e}` gives neither
        // (it yields `1.00000e6`), which is the divergence the differential
        // fuzzer caught. Split Rust's output on `e`, trim the mantissa, then
        // reformat the exponent.
        let raw = format!("{:.*e}", (p - 1) as usize, x);
        let (mantissa, exp) = raw.split_once('e').unwrap_or((raw.as_str(), "0"));
        let mut m = mantissa.to_string();
        if m.contains('.') {
            while m.ends_with('0') {
                m.pop();
            }
            if m.ends_with('.') {
                m.pop();
            }
        }
        let exp_n: i32 = exp.parse().unwrap_or(0);
        let sign = if exp_n < 0 { '-' } else { '+' };
        return format!("{m}e{sign}{:02}", exp_n.abs());
    }
    let decimals = (p - 1 - e).max(0) as usize;
    let mut s = format!("{:.*}", decimals, x);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}
