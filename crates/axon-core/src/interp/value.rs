//! Runtime `Value` formatting + value-level operations for the interpreter
//! (R0 slice 2 — extracted verbatim from `interp.rs`, zero behavior change).
//! `display`/`fields_display` render values for print/interpolation; `fmt_g`
//! is the `%.6g` float formatter converged onto C's printf (R1f slice 2b);
//! `eval_binop_vals` carries Uncertain<T> confidence through arithmetic;
//! `values_equal` is structural equality; `uncertain_parts` unpacks an
//! Uncertain value. `use super::*` pulls in `Value`, `BinOp`, `R`, `HashMap`.

use super::*;

pub(super) fn uncertain_parts(v: &Value) -> Option<(Value, f64)> {
    if let Value::Struct { name, fields } = v {
        if name == "Uncertain" {
            let inner = fields.get("value").cloned().unwrap_or(Value::Int(0));
            let conf = match fields.get("confidence") {
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
    if let Value::Struct { name, fields } = v {
        if name == "Temporal" {
            return Some(fields.get("value").cloned().unwrap_or(Value::Int(0)));
        }
    }
    None
}

/// The plain inner value of a SOFT-TYPED wrapper — `Uncertain<T>` or `Temporal<T>`
/// (both carry a `value` field 0) — for the soft-typing rule that lets such a
/// value flow into a plain-`T` slot (if/while condition, scalar param, scalar
/// return). `None` for any other value. Confidence/horizon are dropped at the
/// T-typed boundary.
pub(super) fn soft_inner(v: &Value) -> Option<Value> {
    if let Value::Struct { name, fields } = v {
        if name == "Uncertain" || name == "Temporal" {
            return Some(fields.get("value").cloned().unwrap_or(Value::Int(0)));
        }
    }
    None
}

// ── R19 Slice B — width-correct integer helpers ───────────────────────────────

/// Interpret the stored i64 bit-pattern as the unsigned value for the type.
/// For unsigned types, mask to the type's range. For signed types, sign-extend.
fn to_display_val(val: i64, ty: &crate::types::Type) -> i64 {
    match ty {
        crate::types::Type::U8 => (val as u8) as i64,
        crate::types::Type::U16 => (val as u16) as i64,
        crate::types::Type::U32 => (val as u32) as i64,
        crate::types::Type::U64 => val, // stored as i64 bits; display as unsigned below
        crate::types::Type::I8 => (val as i8) as i64,
        crate::types::Type::I16 => (val as i16) as i64,
        crate::types::Type::I32 => (val as i32) as i64,
        _ => val,
    }
}

/// Display value for SizedInt. Unsigned types show as unsigned decimal.
pub(super) fn display_sized(val: i64, ty: &crate::types::Type) -> String {
    match ty {
        crate::types::Type::U8 => (val as u8).to_string(),
        crate::types::Type::U16 => (val as u16).to_string(),
        crate::types::Type::U32 => (val as u32).to_string(),
        crate::types::Type::U64 => (val as u64).to_string(),
        crate::types::Type::I8 => (val as i8).to_string(),
        crate::types::Type::I16 => (val as i16).to_string(),
        crate::types::Type::I32 => (val as i32).to_string(),
        _ => val.to_string(),
    }
}

/// Width-correct checked arithmetic for SizedInt. Returns the result
/// masked/clamped to the width boundary, panicking on overflow (I-9).
fn sized_checked_add(a: i64, b: i64, ty: &crate::types::Type) -> super::R {
    match ty {
        crate::types::Type::U8 => (a as u8)
            .checked_add(b as u8)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u8 {} + {} exceeds 255",
                    a as u8, b as u8
                ))
            }),
        crate::types::Type::U16 => (a as u16)
            .checked_add(b as u16)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u16 {} + {} exceeds 65535",
                    a as u16, b as u16
                ))
            }),
        crate::types::Type::U32 => (a as u32)
            .checked_add(b as u32)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u32 {} + {} exceeds {}",
                    a as u32,
                    b as u32,
                    u32::MAX
                ))
            }),
        crate::types::Type::U64 => (a as u64)
            .checked_add(b as u64)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u64 {} + {} exceeds {}",
                    a as u64,
                    b as u64,
                    u64::MAX
                ))
            }),
        crate::types::Type::I8 => (a as i8)
            .checked_add(b as i8)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: i8 {} + {} out of range",
                    a as i8, b as i8
                ))
            }),
        crate::types::Type::I16 => (a as i16)
            .checked_add(b as i16)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: i16 {} + {} out of range",
                    a as i16, b as i16
                ))
            }),
        crate::types::Type::I32 => (a as i32)
            .checked_add(b as i32)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: i32 {} + {} out of range",
                    a as i32, b as i32
                ))
            }),
        _ => Ok(Value::SizedInt {
            val: a.wrapping_add(b),
            ty: ty.clone(),
        }),
    }
}

fn sized_checked_sub(a: i64, b: i64, ty: &crate::types::Type) -> super::R {
    match ty {
        crate::types::Type::U8 => (a as u8)
            .checked_sub(b as u8)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u8 {} - {} underflows",
                    a as u8, b as u8
                ))
            }),
        crate::types::Type::U16 => (a as u16)
            .checked_sub(b as u16)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u16 {} - {} underflows",
                    a as u16, b as u16
                ))
            }),
        crate::types::Type::U32 => (a as u32)
            .checked_sub(b as u32)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u32 {} - {} underflows",
                    a as u32, b as u32
                ))
            }),
        crate::types::Type::U64 => (a as u64)
            .checked_sub(b as u64)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u64 {} - {} underflows",
                    a as u64, b as u64
                ))
            }),
        crate::types::Type::I8 => (a as i8)
            .checked_sub(b as i8)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: i8 {} - {} out of range",
                    a as i8, b as i8
                ))
            }),
        crate::types::Type::I16 => (a as i16)
            .checked_sub(b as i16)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: i16 {} - {} out of range",
                    a as i16, b as i16
                ))
            }),
        crate::types::Type::I32 => (a as i32)
            .checked_sub(b as i32)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: i32 {} - {} out of range",
                    a as i32, b as i32
                ))
            }),
        _ => Ok(Value::SizedInt {
            val: a.wrapping_sub(b),
            ty: ty.clone(),
        }),
    }
}

fn sized_checked_mul(a: i64, b: i64, ty: &crate::types::Type) -> super::R {
    match ty {
        crate::types::Type::U8 => (a as u8)
            .checked_mul(b as u8)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u8 {} * {} exceeds 255",
                    a as u8, b as u8
                ))
            }),
        crate::types::Type::U16 => (a as u16)
            .checked_mul(b as u16)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u16 {} * {} exceeds 65535",
                    a as u16, b as u16
                ))
            }),
        crate::types::Type::U32 => (a as u32)
            .checked_mul(b as u32)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u32 {} * {} exceeds {}",
                    a as u32,
                    b as u32,
                    u32::MAX
                ))
            }),
        crate::types::Type::U64 => (a as u64)
            .checked_mul(b as u64)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: u64 {} * {} exceeds {}",
                    a as u64,
                    b as u64,
                    u64::MAX
                ))
            }),
        crate::types::Type::I8 => (a as i8)
            .checked_mul(b as i8)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: i8 {} * {} out of range",
                    a as i8, b as i8
                ))
            }),
        crate::types::Type::I16 => (a as i16)
            .checked_mul(b as i16)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: i16 {} * {} out of range",
                    a as i16, b as i16
                ))
            }),
        crate::types::Type::I32 => (a as i32)
            .checked_mul(b as i32)
            .map(|v| Value::SizedInt {
                val: v as i64,
                ty: ty.clone(),
            })
            .ok_or_else(|| {
                super::Flow::Panic(format!(
                    "integer overflow: i32 {} * {} out of range",
                    a as i32, b as i32
                ))
            }),
        _ => Ok(Value::SizedInt {
            val: a.wrapping_mul(b),
            ty: ty.clone(),
        }),
    }
}

fn sized_div(a: i64, b: i64, ty: &crate::types::Type) -> super::R {
    if b == 0 {
        return Err(super::Flow::Panic(format!(
            "integer division by zero ({} / 0)",
            ty.display()
        )));
    }
    let v = match ty {
        crate::types::Type::U8 => ((a as u8) / (b as u8)) as i64,
        crate::types::Type::U16 => ((a as u16) / (b as u16)) as i64,
        crate::types::Type::U32 => ((a as u32) / (b as u32)) as i64,
        crate::types::Type::U64 => ((a as u64) / (b as u64)) as i64,
        crate::types::Type::I8 => ((a as i8) / (b as i8)) as i64,
        crate::types::Type::I16 => ((a as i16) / (b as i16)) as i64,
        crate::types::Type::I32 => ((a as i32) / (b as i32)) as i64,
        _ => a / b,
    };
    Ok(Value::SizedInt {
        val: v,
        ty: ty.clone(),
    })
}

fn sized_rem(a: i64, b: i64, ty: &crate::types::Type) -> super::R {
    if b == 0 {
        return Err(super::Flow::Panic(format!(
            "integer remainder by zero ({} % 0)",
            ty.display()
        )));
    }
    let v = match ty {
        crate::types::Type::U8 => ((a as u8) % (b as u8)) as i64,
        crate::types::Type::U16 => ((a as u16) % (b as u16)) as i64,
        crate::types::Type::U32 => ((a as u32) % (b as u32)) as i64,
        crate::types::Type::U64 => ((a as u64) % (b as u64)) as i64,
        crate::types::Type::I8 => ((a as i8) % (b as i8)) as i64,
        crate::types::Type::I16 => ((a as i16) % (b as i16)) as i64,
        crate::types::Type::I32 => ((a as i32) % (b as i32)) as i64,
        _ => a % b,
    };
    Ok(Value::SizedInt {
        val: v,
        ty: ty.clone(),
    })
}

fn sized_cmp(op: &BinOp, a: i64, b: i64, ty: &crate::types::Type) -> bool {
    // Unsigned types: compare as unsigned values; signed: compare as signed.
    match ty {
        crate::types::Type::U8 => {
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
        crate::types::Type::U16 => {
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
        crate::types::Type::U32 => {
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
        crate::types::Type::U64 => {
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
        crate::types::Type::I8 => {
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
        crate::types::Type::I16 => {
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
        crate::types::Type::I32 => {
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
        _ => match op {
            BinOp::Eq => a == b,
            BinOp::NotEq => a != b,
            BinOp::Lt => a < b,
            BinOp::Gt => a > b,
            BinOp::LtEq => a <= b,
            BinOp::GtEq => a >= b,
            _ => false,
        },
    }
}

fn sized_shl(a: i64, shift: u32, ty: &crate::types::Type) -> super::R {
    let v = match ty {
        crate::types::Type::U8 => ((a as u8).wrapping_shl(shift)) as i64,
        crate::types::Type::U16 => ((a as u16).wrapping_shl(shift)) as i64,
        crate::types::Type::U32 => ((a as u32).wrapping_shl(shift)) as i64,
        crate::types::Type::U64 => ((a as u64).wrapping_shl(shift)) as i64,
        crate::types::Type::I8 => ((a as i8).wrapping_shl(shift)) as i64,
        crate::types::Type::I16 => ((a as i16).wrapping_shl(shift)) as i64,
        crate::types::Type::I32 => ((a as i32).wrapping_shl(shift)) as i64,
        _ => a.wrapping_shl(shift),
    };
    Ok(Value::SizedInt {
        val: v,
        ty: ty.clone(),
    })
}

fn sized_shr(a: i64, shift: u32, ty: &crate::types::Type) -> super::R {
    // Unsigned types: logical right-shift (>>); signed: arithmetic right-shift.
    let v = match ty {
        crate::types::Type::U8 => ((a as u8).wrapping_shr(shift)) as i64,
        crate::types::Type::U16 => ((a as u16).wrapping_shr(shift)) as i64,
        crate::types::Type::U32 => ((a as u32).wrapping_shr(shift)) as i64,
        crate::types::Type::U64 => ((a as u64).wrapping_shr(shift)) as i64,
        crate::types::Type::I8 => ((a as i8).wrapping_shr(shift)) as i64,
        crate::types::Type::I16 => ((a as i16).wrapping_shr(shift)) as i64,
        crate::types::Type::I32 => ((a as i32).wrapping_shr(shift)) as i64,
        _ => a.wrapping_shr(shift),
    };
    Ok(Value::SizedInt {
        val: v,
        ty: ty.clone(),
    })
}

fn sized_bitand(a: i64, b: i64, ty: &crate::types::Type) -> Value {
    Value::SizedInt {
        val: a & b,
        ty: ty.clone(),
    }
}
fn sized_bitor(a: i64, b: i64, ty: &crate::types::Type) -> Value {
    Value::SizedInt {
        val: a | b,
        ty: ty.clone(),
    }
}
fn sized_bitxor(a: i64, b: i64, ty: &crate::types::Type) -> Value {
    Value::SizedInt {
        val: a ^ b,
        ty: ty.clone(),
    }
}

pub(super) fn eval_binop_vals(op: &BinOp, l: Value, r: Value) -> R {
    use BinOp::*;
    use Value::{Bool, Float, Int, Str};

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
    {
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
    {
        let lt = soft_temporal_inner(&l);
        let rt = soft_temporal_inner(&r);
        if lt.is_some() || rt.is_some() {
            let lv = lt.unwrap_or_else(|| l.clone());
            let rv = rt.unwrap_or_else(|| r.clone());
            return eval_binop_vals(op, lv, rv);
        }
    }

    match (op, l, r) {
        // Integer arithmetic — checked by default. Overflow is a *graceful
        // panic* (catchable, exits non-zero at the CLI), never a silent
        // wrap: a wrapped value masquerading as success is the worst class
        // of bug for an autonomous consumer (BUG_HUNT #6, ARCHITECTURE
        // INVARIANTS I-9). Use the `wrapping_*` builtins for intentional
        // modular arithmetic.
        (Add, Int(a), Int(b)) => a
            .checked_add(b)
            .map(Int)
            .ok_or_else(|| Flow::Panic(format!("integer overflow: {a} + {b} exceeds i64"))),
        (Sub, Int(a), Int(b)) => a
            .checked_sub(b)
            .map(Int)
            .ok_or_else(|| Flow::Panic(format!("integer overflow: {a} - {b} exceeds i64"))),
        (Mul, Int(a), Int(b)) => a
            .checked_mul(b)
            .map(Int)
            .ok_or_else(|| Flow::Panic(format!("integer overflow: {a} * {b} exceeds i64"))),
        (Div, Int(a), Int(b)) => {
            if b == 0 {
                return Err(Flow::Panic("integer division by zero".into()));
            }
            Ok(Int(a.wrapping_div(b)))
        }
        (Rem, Int(a), Int(b)) => {
            if b == 0 {
                return Err(Flow::Panic("integer remainder by zero".into()));
            }
            Ok(Int(a.wrapping_rem(b)))
        }
        // Float arithmetic
        (Add, Float(a), Float(b)) => Ok(Float(a + b)),
        (Sub, Float(a), Float(b)) => Ok(Float(a - b)),
        (Mul, Float(a), Float(b)) => Ok(Float(a * b)),
        (Div, Float(a), Float(b)) => Ok(Float(a / b)),
        (Rem, Float(a), Float(b)) => Ok(Float(a % b)),
        // ── R21 — exact fixed-point Decimal arithmetic ────────────────────────
        // Same-scale i128 ops. Checked: overflow / div-by-zero → graceful panic,
        // never a silent wrap (money math must never lie). Division uses the
        // banker's-rounding (HalfEven) default; an explicit mode is available via
        // the `decimal_div` builtin.
        (Add, Value::Decimal(a), Value::Decimal(b)) => crate::decimal::add(a, b)
            .map(Value::Decimal)
            .map_err(Flow::Panic),
        (Sub, Value::Decimal(a), Value::Decimal(b)) => crate::decimal::sub(a, b)
            .map(Value::Decimal)
            .map_err(Flow::Panic),
        (Mul, Value::Decimal(a), Value::Decimal(b)) => crate::decimal::mul(a, b)
            .map(Value::Decimal)
            .map_err(Flow::Panic),
        (Div, Value::Decimal(a), Value::Decimal(b)) => {
            crate::decimal::div(a, b, crate::decimal::RoundMode::HalfEven)
                .map(Value::Decimal)
                .map_err(Flow::Panic)
        }
        (Rem, Value::Decimal(a), Value::Decimal(b)) => crate::decimal::rem(a, b)
            .map(Value::Decimal)
            .map_err(Flow::Panic),
        (Eq, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(a == b)),
        (NotEq, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(a != b)),
        (Lt, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(a < b)),
        (Gt, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(a > b)),
        (LtEq, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(a <= b)),
        (GtEq, Value::Decimal(a), Value::Decimal(b)) => Ok(Bool(a >= b)),

        // String concat
        (Add, Str(a), Str(b)) => Ok(Str(a + &b)),
        // Integer comparisons
        (Eq, Int(a), Int(b)) => Ok(Bool(a == b)),
        (NotEq, Int(a), Int(b)) => Ok(Bool(a != b)),
        (Lt, Int(a), Int(b)) => Ok(Bool(a < b)),
        (Gt, Int(a), Int(b)) => Ok(Bool(a > b)),
        (LtEq, Int(a), Int(b)) => Ok(Bool(a <= b)),
        (GtEq, Int(a), Int(b)) => Ok(Bool(a >= b)),
        // Float comparisons
        (Eq, Float(a), Float(b)) => Ok(Bool(a == b)),
        (NotEq, Float(a), Float(b)) => Ok(Bool(a != b)),
        (Lt, Float(a), Float(b)) => Ok(Bool(a < b)),
        (Gt, Float(a), Float(b)) => Ok(Bool(a > b)),
        (LtEq, Float(a), Float(b)) => Ok(Bool(a <= b)),
        (GtEq, Float(a), Float(b)) => Ok(Bool(a >= b)),
        // Bool / string equality
        (Eq, Bool(a), Bool(b)) => Ok(Bool(a == b)),
        (NotEq, Bool(a), Bool(b)) => Ok(Bool(a != b)),
        (Eq, Str(a), Str(b)) => Ok(Bool(a == b)),
        (NotEq, Str(a), Str(b)) => Ok(Bool(a != b)),
        // Integer bitwise
        (BitAnd, Int(a), Int(b)) => Ok(Int(a & b)),
        (BitOr, Int(a), Int(b)) => Ok(Int(a | b)),
        (BitXor, Int(a), Int(b)) => Ok(Int(a ^ b)),
        (Shl, Int(a), Int(b)) => Ok(Int(a.wrapping_shl(b as u32))),
        (Shr, Int(a), Int(b)) => Ok(Int(a.wrapping_shr(b as u32))),
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
            sized_checked_add(a, b, &ty)
        }
        (Sub, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_checked_sub(a, b, &ty)
        }
        (Mul, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_checked_mul(a, b, &ty)
        }
        (Div, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_div(a, b, &ty)
        }
        (Rem, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_rem(a, b, &ty)
        }
        // Comparisons → bool (unsigned or signed per type).
        (
            Eq | NotEq | Lt | Gt | LtEq | GtEq,
            Value::SizedInt { val: a, ty },
            Value::SizedInt { val: b, .. },
        ) => Ok(Bool(sized_cmp(op, a, b, &ty))),
        // Bitwise ops.
        (BitAnd, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            Ok(sized_bitand(a, b, &ty))
        }
        (BitOr, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            Ok(sized_bitor(a, b, &ty))
        }
        (BitXor, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            Ok(sized_bitxor(a, b, &ty))
        }
        (Shl, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_shl(a, b as u32, &ty)
        }
        (Shr, Value::SizedInt { val: a, ty }, Value::SizedInt { val: b, .. }) => {
            sized_shr(a, b as u32, &ty)
        }
        // Mixed: bare Int literal op SizedInt — coerce the literal to the SizedInt's width.
        // This handles patterns like `255u8 + 1` where the 1 is a bare Int.
        (Add, Value::SizedInt { val: a, ty }, Int(b)) => sized_checked_add(a, b, &ty),
        (Sub, Value::SizedInt { val: a, ty }, Int(b)) => sized_checked_sub(a, b, &ty),
        (Mul, Value::SizedInt { val: a, ty }, Int(b)) => sized_checked_mul(a, b, &ty),
        (Div, Value::SizedInt { val: a, ty }, Int(b)) => sized_div(a, b, &ty),
        (Rem, Value::SizedInt { val: a, ty }, Int(b)) => sized_rem(a, b, &ty),
        (Add, Int(a), Value::SizedInt { val: b, ty }) => sized_checked_add(a, b, &ty),
        (Sub, Int(a), Value::SizedInt { val: b, ty }) => sized_checked_sub(a, b, &ty),
        (Mul, Int(a), Value::SizedInt { val: b, ty }) => sized_checked_mul(a, b, &ty),
        (Div, Int(a), Value::SizedInt { val: b, ty }) => sized_div(a, b, &ty),
        (Rem, Int(a), Value::SizedInt { val: b, ty }) => sized_rem(a, b, &ty),
        (Eq | NotEq | Lt | Gt | LtEq | GtEq, Value::SizedInt { val: a, ty }, Int(b)) => {
            Ok(Bool(sized_cmp(op, a, b, &ty)))
        }
        (Eq | NotEq | Lt | Gt | LtEq | GtEq, Int(a), Value::SizedInt { val: b, ty }) => {
            Ok(Bool(sized_cmp(op, a, b, &ty)))
        }
        (Shl, Value::SizedInt { val: a, ty }, Int(b)) => sized_shl(a, b as u32, &ty),
        (Shr, Value::SizedInt { val: a, ty }, Int(b)) => sized_shr(a, b as u32, &ty),
        (BitAnd, Value::SizedInt { val: a, ty }, Int(b)) => Ok(sized_bitand(a, b, &ty)),
        (BitOr, Value::SizedInt { val: a, ty }, Int(b)) => Ok(sized_bitor(a, b, &ty)),
        (BitXor, Value::SizedInt { val: a, ty }, Int(b)) => Ok(sized_bitxor(a, b, &ty)),

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
            to_display_val(*x, tx) == to_display_val(*y, tx)
        }
        (SizedInt { val: x, ty }, Int(y)) => to_display_val(*x, ty) == *y,
        (Int(x), SizedInt { val: y, ty }) => *x == to_display_val(*y, ty),
        (Float(x), Float(y)) => x == y,
        (Decimal(x), Decimal(y)) => x == y,
        (Bool(x), Bool(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (Unit, Unit) => true,
        (None, None) => true,
        (Some(x), Some(y)) | (Ok(x), Ok(y)) | (Err(x), Err(y)) => values_equal(x, y),
        (Array(x), Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| values_equal(p, q))
        }
        (
            Struct {
                name: n1,
                fields: f1,
            },
            Struct {
                name: n2,
                fields: f2,
            },
        ) => n1 == n2 && fields_equal(f1, f2),
        (
            Enum {
                enum_name: e1,
                variant: v1,
                fields: f1,
            },
            Enum {
                enum_name: e2,
                variant: v2,
                fields: f2,
            },
        ) => e1 == e2 && v1 == v2 && fields_equal(f1, f2),
        (Tuple(x), Tuple(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| values_equal(p, q))
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

pub(super) fn fields_equal(a: &HashMap<String, Value>, b: &HashMap<String, Value>) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|(k, v)| b.get(k).map(|w| values_equal(v, w)).unwrap_or(false))
}

/// Render a value for `print`/`println`/string interpolation. A `str` renders
/// as its raw contents (no quotes); everything else gets a reasonable form.
pub(super) fn display(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::SizedInt { val, ty } => display_sized(*val, ty),
        Value::Float(f) => fmt_g(*f),
        Value::Decimal(m) => crate::decimal::format_decimal(*m),
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
        Value::Struct { name, fields } => format!("{name} {{ {} }}", fields_display(fields)),
        Value::Enum {
            enum_name,
            variant,
            fields,
        } => {
            if fields.is_empty() {
                format!("{enum_name}::{variant}")
            } else {
                format!("{enum_name}::{variant} {{ {} }}", fields_display(fields))
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
        Value::Handle { module, name, .. } => format!("<native {module}::{name}>"),
    }
}

pub(super) fn fields_display(fields: &HashMap<String, Value>) -> String {
    let mut parts: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{k}: {}", display(v)))
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
