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
        (Float(x), Float(y)) => x == y,
        (Bool(x), Bool(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (Unit, Unit) => true,
        (None, None) => true,
        (Some(x), Some(y)) | (Ok(x), Ok(y)) | (Err(x), Err(y)) => values_equal(x, y),
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| values_equal(p, q)),
        (Struct { name: n1, fields: f1 }, Struct { name: n2, fields: f2 }) => {
            n1 == n2 && fields_equal(f1, f2)
        }
        (
            Enum { enum_name: e1, variant: v1, fields: f1 },
            Enum { enum_name: e2, variant: v2, fields: f2 },
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
                && m1.iter().zip(m2.iter()).all(|((k1, v1), (k2, v2))| {
                    k1 == k2 && values_equal(v1, v2)
                })
        }
        _ => false,
    }
}

pub(super) fn fields_equal(a: &HashMap<String, Value>, b: &HashMap<String, Value>) -> bool {
    a.len() == b.len()
        && a.iter().all(|(k, v)| b.get(k).map(|w| values_equal(v, w)).unwrap_or(false))
}

/// Render a value for `print`/`println`/string interpolation. A `str` renders
/// as its raw contents (no quotes); everything else gets a reasonable form.
pub(super) fn display(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => fmt_g(*f),
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
        Value::Enum { enum_name, variant, fields } => {
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
    }
}

pub(super) fn fields_display(fields: &HashMap<String, Value>) -> String {
    let mut parts: Vec<String> = fields.iter().map(|(k, v)| format!("{k}: {}", display(v))).collect();
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
