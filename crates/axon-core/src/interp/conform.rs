//! Declared-type conformance at the value boundaries (C9 round 4, PSV-1,
//! amendment 53).
//!
//! Axon's checker is GRADUAL: a builtin whose result type is a free variable
//! (`dict_get`, `dict_remove`, `dict_values`, `dict_to_pairs`,
//! `host_await_val`, the `V` of `dict_get_or`) yields `Deferred`, which
//! unifies with anything. So a value stored as one type type-checks as any
//! other, and the interpreter — which dispatches a method call on the value's
//! RUNTIME type — would run whatever method that runtime type has. The C9
//! round-4 review executed the consequence: a candidate returned its own
//! `Fake` from a fn declared `-> i64`, and the operator's `r.ok()` ran the
//! candidate's `Fake::ok` instead of the operator's `i64::ok`.
//!
//! The remedy is the one gradual typing prescribes: a CAST at every point
//! where a value meets a DECLARED type. Every value one piece of code hands
//! another crosses such a point — a named fn or method's parameters and
//! return, a closure's parameters and return under the `fn(..) -> ..` type it
//! crossed, a channel's element type, a `let x: T` annotation, a struct or
//! enum field — so this is the one check, applied at each, rather than a
//! check per producing builtin:
//!
//! * scalars by the SAME key a method call dispatches on (`type_name`): an
//!   integer by its width (an `i64` at a fixed width is converted in place,
//!   another width refused), `str`, `bool`, `()`, `Dict`; a soft wrapper by
//!   its name, and replaced by its inner value where the declared type is a
//!   plain concrete `T`;
//! * at a `fn(..) -> ..` type only a closure (no named fn is ever a value);
//! * a struct or enum by NAME, then its fields against the declared field
//!   types; `Option`/`Result` by constructor, then the payload; arrays and
//!   tuples element by element — as far as the declared type states them;
//! * `dyn Trait` and a type parameter's trait bounds by an impl of that trait;
//! * a type parameter is BOUND from the first value that meets it (the
//!   arguments, in order) and every later value at it must agree; at a seal
//!   crossing (a candidate fn or closure returning to operator code) a value
//!   at a type parameter no argument determined is refused — by parametricity
//!   no honest body can produce one;
//! * a closure crossing a declared `fn(..) -> ..` is WRAPPED (a per-reference
//!   contract, as gradual typing's function proxies): its arguments and
//!   result are cast on every later call; a channel crossing `Chan<T>` is
//!   stamped (a channel is one invariant object) and every later send is cast.
//!
//! A declared type the interpreter cannot read (an unknown name, a builtin
//! handle) is accepted: the check refuses only what it can show is a
//! different type, so no honest program is rejected.

use super::*;
use crate::ast::AxonType as T;

/// Type-parameter bindings of one activation, shared with the closure and
/// channel contracts it stamps (so a binding a closure's result makes is seen
/// by the activation's own return check).
pub(crate) type Binds = Rc<RefCell<HashMap<String, T>>>;

/// `(type parameter, the traits it is bounded by)`, in declaration order.
type Bounds = Vec<(String, Vec<String>)>;

/// The environment a declared type is read in.
#[derive(Debug, Clone, Default)]
pub(crate) struct Cx {
    // `None` rather than an empty `Rc`: most signatures have no type
    // parameters, and a cast runs at every call, so building one allocates
    // nothing.
    tparams: Option<Rc<Vec<String>>>,
    bounds: Option<Rc<Bounds>>,
    binds: Option<Binds>,
    self_ty: Option<Rc<T>>,
    /// A value at an unbound type parameter is refused (seal crossing).
    strict: bool,
    /// For the FIELD environment of a generic struct or enum: the environment
    /// its type arguments are read in. A type argument that is the caller's
    /// own (possibly still unbound) type parameter is resolved THERE, so it is
    /// bound by the field's value or refused at a strict crossing — never
    /// erased (C9 round 4c, amendment 72).
    parent: Option<Rc<Cx>>,
}

impl Cx {
    /// The environment of `f`'s signature (its own and its impl's type
    /// parameters, their bounds, and `Self`).
    pub(crate) fn of_fn(f: &FnDef, imp: Option<&ImplBlock>) -> Cx {
        let mut tparams = f.generic_params.clone();
        let mut bounds = f.generic_bounds.clone();
        if let Some(b) = imp {
            tparams.extend(b.generic_params.iter().cloned());
            bounds.extend(b.generic_bounds.iter().cloned());
        }
        let binds = if tparams.is_empty() {
            None
        } else {
            Some(Rc::new(RefCell::new(HashMap::new())))
        };
        Cx {
            tparams: (!tparams.is_empty()).then(|| Rc::new(tparams)),
            bounds: (!bounds.is_empty()).then(|| Rc::new(bounds)),
            binds,
            self_ty: imp.map(|b| Rc::new(b.for_type.clone())),
            strict: false,
            parent: None,
        }
    }

    /// The environment a bound type parameter's binding is read in: the
    /// parent's for a field environment, otherwise the empty one — with this
    /// environment's strictness either way.
    fn binding_cx(&self) -> Cx {
        match &self.parent {
            Some(p) => p.strict(self.strict),
            None => Cx::default().strict(self.strict),
        }
    }

    /// The outermost environment (where a type that [`resolve`] leaves free
    /// has its parameters).
    fn root(&self) -> Cx {
        match &self.parent {
            Some(p) => p.root(),
            None => self.clone(),
        }
    }

    /// This environment with FRESH type-parameter bindings (one activation).
    pub(crate) fn fresh(&self) -> Cx {
        Cx {
            binds: self
                .binds
                .as_ref()
                .map(|_| Rc::new(RefCell::new(HashMap::new()))),
            ..self.clone()
        }
    }

    pub(crate) fn strict(&self, strict: bool) -> Cx {
        Cx {
            strict,
            ..self.clone()
        }
    }

    fn is_tparam(&self, n: &str) -> bool {
        self.tparams
            .as_ref()
            .is_some_and(|t| t.iter().any(|p| p == n))
    }
}

/// One `fn(..) -> ..` (or channel element) type a closure (channel) crossed.
/// Closure contracts chain: the OUTERMOST is the latest crossing.
#[derive(Debug)]
pub struct Contract {
    params: Vec<T>,
    ret: T,
    cx: Cx,
    next: Option<Rc<Contract>>,
}

/// The "any" type: a position whose type is not stated.
fn any() -> T {
    T::Named("?".into())
}

/// A position of a type NOTHING determined — a part of a binding the value
/// it was taken from did not show (an empty array's element, `None`'s
/// payload), or a generic struct's missing type argument. Unlike `?` (a
/// position whose type is not stated, e.g. an unannotated lambda parameter),
/// a value here is refused at a strict (seal) crossing: no operator-side
/// value stated its type, so its runtime type would be the candidate's
/// choice (C9 round 4c, amendment 72).
const UNDET: &str = "?undetermined";

fn undet() -> T {
    T::Named(UNDET.into())
}

/// The prefix of the type a native handle value is evidence of.
const HANDLE: &str = "handle:";

/// Whether `t` has a position nothing determined.
fn has_undet(t: &T) -> bool {
    match t {
        T::Named(n) => n == UNDET,
        T::TypeParam(_) | T::DynTrait(_) => false,
        T::Result { ok, err } => has_undet(ok) || has_undet(err),
        T::Option(x) | T::Chan(x) | T::Slice(x) | T::Ref(x) | T::RefMut(x) | T::RawPtr(x) => {
            has_undet(x)
        }
        T::Generic { args, .. } => args.iter().any(has_undet),
        T::Fn { params, ret } => params.iter().any(has_undet) || has_undet(ret),
        T::Tuple(xs) | T::Union(xs) => xs.iter().any(has_undet),
    }
}

/// `Option<x>`/`Result<o, e>` spelled as a generic, as their own forms.
fn norm(t: &T) -> T {
    match t {
        T::Generic { base, args } if base == "Option" && args.len() == 1 => {
            T::Option(Box::new(args[0].clone()))
        }
        T::Generic { base, args } if base == "Result" && args.len() == 2 => T::Result {
            ok: Box::new(args[0].clone()),
            err: Box::new(args[1].clone()),
        },
        _ => t.clone(),
    }
}

/// `a` with each position nothing determined filled from `b`.
fn merge(a: &T, b: &T) -> T {
    if matches!(a, T::Named(n) if n == UNDET) {
        return b.clone();
    }
    if !has_undet(a) {
        return a.clone();
    }
    let m = |x: &T, y: &T| Box::new(merge(x, y));
    match (&norm(a), &norm(b)) {
        (T::Option(x), T::Option(y)) => T::Option(m(x, y)),
        (T::Slice(x), T::Slice(y)) => T::Slice(m(x, y)),
        (T::Chan(x), T::Chan(y)) => T::Chan(m(x, y)),
        (T::Result { ok: o1, err: e1 }, T::Result { ok: o2, err: e2 }) => T::Result {
            ok: m(o1, o2),
            err: m(e1, e2),
        },
        (T::Tuple(xs), T::Tuple(ys)) if xs.len() == ys.len() => {
            T::Tuple(xs.iter().zip(ys).map(|(x, y)| merge(x, y)).collect())
        }
        (T::Generic { base: b1, args: a1 }, T::Generic { base: b2, args: a2 })
            if b1 == b2 && a1.len() == a2.len() =>
        {
            T::Generic {
                base: b1.clone(),
                args: a1.iter().zip(a2).map(|(x, y)| merge(x, y)).collect(),
            }
        }
        (
            T::Fn {
                params: p1,
                ret: r1,
            },
            T::Fn {
                params: p2,
                ret: r2,
            },
        ) if p1.len() == p2.len() => T::Fn {
            params: p1.iter().zip(p2).map(|(x, y)| merge(x, y)).collect(),
            ret: m(r1, r2),
        },
        _ => a.clone(),
    }
}

/// Bind the type parameters of `cx` that the declared type `decl` mentions
/// from the type `actual` occupying it (structurally). A parameter already
/// bound only has its undetermined positions filled.
fn bind_from(decl: &T, actual: &T, cx: &Cx) {
    let Some(binds) = &cx.binds else { return };
    match (&norm(decl), &norm(actual)) {
        (T::Named(n) | T::TypeParam(n), a) if cx.is_tparam(n) => {
            let cur = binds.borrow().get(n).cloned();
            let next = match cur {
                None => a.clone(),
                Some(b) => merge(&b, a),
            };
            binds.borrow_mut().insert(n.clone(), next);
        }
        (T::Ref(x) | T::RefMut(x), a) => bind_from(x, a, cx),
        (T::Option(x), T::Option(y)) | (T::Slice(x), T::Slice(y)) | (T::Chan(x), T::Chan(y)) => {
            bind_from(x, y, cx)
        }
        (T::Result { ok: o1, err: e1 }, T::Result { ok: o2, err: e2 }) => {
            bind_from(o1, o2, cx);
            bind_from(e1, e2, cx);
        }
        (T::Tuple(xs), T::Tuple(ys)) if xs.len() == ys.len() => {
            for (x, y) in xs.iter().zip(ys) {
                bind_from(x, y, cx);
            }
        }
        (T::Generic { base: b1, args: a1 }, T::Generic { base: b2, args: a2 })
            if b1 == b2 && a1.len() == a2.len() =>
        {
            for (x, y) in a1.iter().zip(a2) {
                bind_from(x, y, cx);
            }
        }
        (
            T::Fn {
                params: p1,
                ret: r1,
            },
            T::Fn {
                params: p2,
                ret: r2,
            },
        ) if p1.len() == p2.len() => {
            for (x, y) in p1.iter().zip(p2) {
                bind_from(x, y, cx);
            }
            bind_from(r1, r2, cx);
        }
        _ => {}
    }
}

/// `t` with every type parameter of `cx` (and, for a field environment, of
/// each enclosing environment in turn) replaced by its binding. What is left
/// free is a parameter of `cx.root()`.
fn resolve(t: &T, cx: &Cx) -> T {
    let t = subst(t, cx, false);
    match &cx.parent {
        Some(p) => resolve(&t, p),
        None => t,
    }
}

/// Replace every type parameter of `cx` by its binding (or `?` when unbound
/// and `erase` is set).
fn subst(t: &T, cx: &Cx, erase: bool) -> T {
    let go = |t: &T| subst(t, cx, erase);
    match t {
        T::Named(n) | T::TypeParam(n) if cx.is_tparam(n) => {
            match cx.binds.as_ref().and_then(|b| b.borrow().get(n).cloned()) {
                Some(b) => b,
                None if erase => any(),
                None => t.clone(),
            }
        }
        T::Named(n) if n == "Self" => cx.self_ty.as_deref().cloned().unwrap_or_else(|| t.clone()),
        T::Named(_) | T::TypeParam(_) | T::DynTrait(_) => t.clone(),
        T::Result { ok, err } => T::Result {
            ok: Box::new(go(ok)),
            err: Box::new(go(err)),
        },
        T::Option(x) => T::Option(Box::new(go(x))),
        T::Chan(x) => T::Chan(Box::new(go(x))),
        T::Slice(x) => T::Slice(Box::new(go(x))),
        T::Ref(x) => T::Ref(Box::new(go(x))),
        T::RefMut(x) => T::RefMut(Box::new(go(x))),
        T::RawPtr(x) => T::RawPtr(Box::new(go(x))),
        T::Generic { base, args } => T::Generic {
            base: base.clone(),
            args: args.iter().map(go).collect(),
        },
        T::Fn { params, ret } => T::Fn {
            params: params.iter().map(go).collect(),
            ret: Box::new(go(ret)),
        },
        T::Tuple(xs) => T::Tuple(xs.iter().map(go).collect()),
        T::Union(xs) => T::Union(xs.iter().map(go).collect()),
    }
}

/// Whether `t` still mentions a type parameter of `cx`.
fn mentions_tparam(t: &T, cx: &Cx) -> bool {
    let go = |t: &T| mentions_tparam(t, cx);
    match t {
        T::Named(n) | T::TypeParam(n) => cx.is_tparam(n),
        T::DynTrait(_) => false,
        T::Result { ok, err } => go(ok) || go(err),
        T::Option(x) | T::Chan(x) | T::Slice(x) | T::Ref(x) | T::RefMut(x) | T::RawPtr(x) => go(x),
        T::Generic { args, .. } => args.iter().any(go),
        T::Fn { params, ret } => params.iter().any(go) || go(ret),
        T::Tuple(xs) | T::Union(xs) => xs.iter().any(go),
    }
}

/// The FIELD environment of a struct/enum definition read at `args`.
///
/// Each of the definition's type parameters is bound to its type ARGUMENT as
/// written, read in `outer` (the parent): an argument that is the caller's own
/// type parameter stays that parameter, so a field's value binds it (an
/// argument determines it) or, at a strict crossing, is refused when nothing
/// did. It was erased to `?` here, which waved every such field through (C9
/// round 4c, amendment 72). A MISSING argument is a position nothing
/// determined.
fn field_cx(generics: &[String], args: &[T], outer: &Cx) -> Cx {
    if generics.is_empty() {
        return Cx::default().strict(outer.strict);
    }
    let map: HashMap<String, T> = generics
        .iter()
        .enumerate()
        .map(|(i, g)| (g.clone(), args.get(i).cloned().unwrap_or_else(undet)))
        .collect();
    Cx {
        tparams: Some(Rc::new(generics.to_vec())),
        binds: Some(Rc::new(RefCell::new(map))),
        strict: outer.strict,
        parent: Some(Rc::new(outer.clone())),
        ..Cx::default()
    }
}

fn mismatch(ty: &T, v: &Value) -> String {
    format!(
        "declared `{}`, got a value of type `{}`",
        crate::doc::render_type(ty),
        v.type_name()
    )
}

/// How deep a value the cast follows; deeper is refused, never waved through.
const MAX_CAST_DEPTH: usize = 1_000_000;

impl<'p> Interp<'p> {
    /// Cast `v` to the declared type `ty` read in `cx`: `Ok` when the value
    /// conforms (closures are wrapped and channels stamped in place), `Err`
    /// with the reason when it is shown to be another type.
    pub(crate) fn cast(&self, v: &mut Value, ty: &T, cx: &Cx) -> Result<(), String> {
        self.cast_at(v, ty, cx, 0)
    }

    fn cast_at(&self, v: &mut Value, ty: &T, cx: &Cx, depth: usize) -> Result<(), String> {
        if depth > MAX_CAST_DEPTH {
            return Err("the value is nested too deeply to check against its declared type".into());
        }
        let d = depth + 1;
        // Soft typing: `Uncertain<T>`/`Temporal<T>` stands for its plain `T`.
        // Where the declared type fixes the runtime type, the wrapper is
        // REPLACED by its inner value, so the value's runtime type (what a
        // method call dispatches on) is the declared one rather than
        // `Uncertain` (C9 round 4b, amendment 60). At a position whose type
        // is not stated (`?`, an unbound type parameter, an unknown name)
        // the wrapper is kept: honest generic code passes it through.
        if !is_soft_decl(ty) {
            if let Some(mut inner) = value::soft_inner(v) {
                self.cast_at(&mut inner, ty, cx, d)?;
                if self.pins_runtime_type(ty, cx) {
                    *v = inner;
                }
                return Ok(());
            }
        }
        match ty {
            T::Named(n) => self.cast_named(v, n, &[], ty, cx, d),
            T::TypeParam(n) if cx.is_tparam(n) => self.cast_tparam(v, n, cx, d),
            T::TypeParam(_) | T::RawPtr(_) => Ok(()),
            T::Ref(inner) | T::RefMut(inner) => self.cast_at(v, inner, cx, d),
            T::Option(inner) => self.cast_option(v, inner, ty, cx, d),
            T::Result { ok, err } => self.cast_result(v, ok, err, ty, cx, d),
            T::Generic { base, args } => match (base.as_str(), args.as_slice()) {
                ("Option", [inner]) => self.cast_option(v, inner, ty, cx, d),
                ("Result", [ok, err]) => self.cast_result(v, ok, err, ty, cx, d),
                ("Uncertain" | "Temporal", args) => {
                    self.cast_soft(v, base, args.first(), ty, cx, d)
                }
                _ => self.cast_named(v, base, args, ty, cx, d),
            },
            T::Slice(inner) => match v {
                Value::Array(xs) => {
                    for x in Rc::make_mut(xs).iter_mut() {
                        self.cast_at(x, inner, cx, d)?;
                    }
                    Ok(())
                }
                _ => Err(mismatch(ty, v)),
            },
            T::Tuple(ts) => match v {
                Value::Tuple(xs) if xs.len() == ts.len() => {
                    for (x, t) in xs.iter_mut().zip(ts) {
                        self.cast_at(x, t, cx, d)?;
                    }
                    Ok(())
                }
                _ => Err(mismatch(ty, v)),
            },
            T::Union(ts) => {
                for t in ts {
                    let mut probe = v.clone();
                    if self.cast_at(&mut probe, t, cx, d).is_ok() {
                        *v = probe;
                        return Ok(());
                    }
                }
                Err(mismatch(ty, v))
            }
            T::DynTrait(tr) => self.check_impl(v, tr),
            T::Fn { params, ret } => match v {
                Value::Closure {
                    params: ps,
                    contract,
                    ..
                } => {
                    if ps.len() != params.len() {
                        return Err(format!(
                            "declared `{}`, got a closure of {} parameter(s)",
                            crate::doc::render_type(ty),
                            ps.len()
                        ));
                    }
                    // Bound parameters are fixed now; free ones stay free
                    // and bind through the SHARED activation cell (read
                    // through a field environment to the activation's own).
                    let root = cx.root();
                    let params: Vec<T> = params.iter().map(|t| resolve(t, cx)).collect();
                    let ret = resolve(ret, cx);
                    let free = params.iter().any(|t| mentions_tparam(t, &root))
                        || mentions_tparam(&ret, &root);
                    // A closure crossing the same closed type again (a
                    // recursive helper handing it down) is not re-wrapped.
                    if !free {
                        if let Some(c) = contract {
                            if !mentions_tparam_any(c) && same_sig(&c.params, &c.ret, &params, &ret)
                            {
                                return Ok(());
                            }
                        }
                    }
                    let cx = if free {
                        root.strict(false)
                    } else {
                        Cx::default()
                    };
                    *contract = Some(Rc::new(Contract {
                        params,
                        ret,
                        cx,
                        next: contract.take(),
                    }));
                    Ok(())
                }
                // Only a closure is a fn VALUE: a named fn is never one
                // (`let g = double` is E0306), so anything else here is a
                // confused value — refused, never waved through to a later
                // call or method dispatch (C9 round 4b, amendment 60).
                _ => Err(mismatch(ty, v)),
            },
            T::Chan(elem) => match v {
                Value::Chan(q) => self.stamp_chan(q, elem, cx),
                _ => Err(mismatch(ty, v)),
            },
        }
    }

    fn cast_option(
        &self,
        v: &mut Value,
        inner: &T,
        ty: &T,
        cx: &Cx,
        d: usize,
    ) -> Result<(), String> {
        match v {
            Value::None => Ok(()),
            Value::Some(x) => self.cast_at(x, inner, cx, d),
            _ => Err(mismatch(ty, v)),
        }
    }

    fn cast_result(
        &self,
        v: &mut Value,
        ok: &T,
        err: &T,
        ty: &T,
        cx: &Cx,
        d: usize,
    ) -> Result<(), String> {
        match v {
            Value::Ok(x) => self.cast_at(x, ok, cx, d),
            Value::Err(x) => self.cast_at(x, err, cx, d),
            _ => Err(mismatch(ty, v)),
        }
    }

    fn cast_named(
        &self,
        v: &mut Value,
        n: &str,
        args: &[T],
        ty: &T,
        cx: &Cx,
        d: usize,
    ) -> Result<(), String> {
        if cx.is_tparam(n) {
            return self.cast_tparam(v, n, cx, d);
        }
        let kind_ok = |ok: bool| if ok { Ok(()) } else { Err(mismatch(ty, v)) };
        match n {
            "?" => return Ok(()),
            // A position nothing determined: at a seal crossing the value's
            // runtime type would be the candidate's choice (amendment 72).
            UNDET if cx.strict => {
                return Err(format!(
                    "a value of type `{}` at a position no operator-side value determined",
                    v.type_name()
                ))
            }
            UNDET => return Ok(()),
            "Self" => {
                return match cx.self_ty.clone() {
                    Some(s) => self.cast_at(v, &s, &Cx::default().strict(cx.strict), d),
                    None => Ok(()),
                }
            }
            // Integers by WIDTH, the key a method call dispatches on
            // (`Value::type_name`), never by kind: a `u8` at a declared `i64`
            // would run the operator's `u8` impl (C9 round 4b, amendment 60).
            // An `i64` value (`Value::Int`, also an integer literal's
            // representation) at a fixed width is CONVERTED in place, as a
            // parameter, `let` or field of that width always converts it; a
            // value of another fixed width is refused, as the checker refuses
            // it (E0307). `isize`/`usize` are represented as `i64`.
            "i64" | "isize" | "usize" => return kind_ok(matches!(v, Value::Int(_))),
            "i32" | "i16" | "i8" | "u64" | "u32" | "u16" | "u8" => {
                let width = sized_width(n);
                return match v {
                    Value::Int(x) => {
                        let val = *x;
                        *v = Value::SizedInt { val, ty: width };
                        Ok(())
                    }
                    Value::SizedInt { ty: w, .. } if *w == width => Ok(()),
                    _ => Err(mismatch(ty, v)),
                };
            }
            // One runtime float representation (`f64`) for both names.
            "f64" | "f32" => return kind_ok(matches!(v, Value::Float(_))),
            // The map primitive: only a dict is one (C9 round 4b).
            "Dict" => return kind_ok(matches!(v, Value::Dict(_))),
            // A bare soft name: only that wrapper.
            "Uncertain" | "Temporal" => return self.cast_soft(v, n, None, ty, cx, d),
            "bool" => return kind_ok(matches!(v, Value::Bool(_))),
            "str" | "String" => return kind_ok(matches!(v, Value::Str(_))),
            "()" => return kind_ok(matches!(v, Value::Unit)),
            "Decimal" => return kind_ok(matches!(v, Value::Decimal(_))),
            _ => {}
        }
        if let Some(td) = self.structs.get(n).copied() {
            let Value::Struct { name, fields } = v else {
                return Err(mismatch(ty, v));
            };
            if name != n {
                return Err(mismatch(ty, v));
            }
            let fcx = field_cx(&td.generic_params, args, cx);
            // Walk the VALUE's fields and find each one's declaration (a
            // short list), rather than hashing every declared name.
            for (fname, fv) in fields.iter_mut() {
                if let Some(tf) = td.fields.iter().find(|f| f.name == *fname) {
                    self.cast_at(fv, &tf.ty, &fcx, d)
                        .map_err(|e| format!("field `{}` of `{n}`: {e}", tf.name))?;
                }
            }
            return Ok(());
        }
        if let Some(ed) = self.enums.get(n).copied() {
            let Value::Enum {
                enum_name,
                variant,
                fields,
            } = v
            else {
                return Err(mismatch(ty, v));
            };
            if enum_name != n {
                return Err(mismatch(ty, v));
            }
            let Some(vd) = ed.variants.iter().find(|x| x.name == *variant) else {
                return Err(format!("`{n}` has no variant `{variant}`"));
            };
            let fcx = field_cx(&ed.generic_params, args, cx);
            for (fname, fv) in fields.iter_mut() {
                if let Some(tf) = vd.fields.iter().find(|f| f.name == *fname) {
                    self.cast_at(fv, &tf.ty, &fcx, d)
                        .map_err(|e| format!("field `{}` of `{n}::{variant}`: {e}", tf.name))?;
                }
            }
            return Ok(());
        }
        if let Some(base) = self.refine_bases.get(n).copied() {
            return self.cast_at(v, base, cx, d);
        }
        // A binding taken from a native handle: only a handle of that kind.
        if let Some(h) = n.strip_prefix(HANDLE) {
            return if matches!(v, Value::Handle { .. }) && v.type_name() == h {
                Ok(())
            } else {
                Err(mismatch(ty, v))
            };
        }
        // A name the interpreter does not know (a builtin handle, `Dict`, a
        // type from outside this program): nothing to show it is different.
        Ok(())
    }

    /// A declared `Uncertain<T>`/`Temporal<T>` (or the bare name): the wrapper
    /// of THAT name with its inner value cast to `T` in place, or (soft
    /// typing) a plain value cast to `T`. The other wrapper is another type.
    fn cast_soft(
        &self,
        v: &mut Value,
        base: &str,
        inner_ty: Option<&T>,
        ty: &T,
        cx: &Cx,
        d: usize,
    ) -> Result<(), String> {
        match v {
            Value::Struct { name, fields } if name == "Uncertain" || name == "Temporal" => {
                if name != base {
                    return Err(mismatch(ty, v));
                }
                match (inner_ty, fields.get_mut("value")) {
                    (Some(t), Some(x)) => self.cast_at(x, t, cx, d),
                    _ => Ok(()),
                }
            }
            _ => match inner_ty {
                Some(t) => self.cast_at(v, t, cx, d),
                None => Err(mismatch(ty, v)),
            },
        }
    }

    /// Whether the declared type `ty` (read in `cx`) fixes the runtime type of
    /// a value cast to it — a concrete type, not `?`, an unbound type
    /// parameter, or a name the interpreter does not know.
    fn pins_runtime_type(&self, ty: &T, cx: &Cx) -> bool {
        match ty {
            T::Named(n) | T::TypeParam(n) if cx.is_tparam(n) => {
                match cx.binds.as_ref().and_then(|b| b.borrow().get(n).cloned()) {
                    Some(b) => self.pins_runtime_type(&b, &cx.binding_cx()),
                    None => false,
                }
            }
            T::Named(n) if n == "Self" => cx
                .self_ty
                .as_deref()
                .is_some_and(|s| self.pins_runtime_type(s, &Cx::default())),
            _ => self.pins_concrete(ty, cx),
        }
    }

    fn pins_concrete(&self, ty: &T, cx: &Cx) -> bool {
        match ty.clone() {
            T::Named(n) | T::Generic { base: n, .. } => {
                !cx.is_tparam(&n)
                    && (matches!(
                        n.as_str(),
                        "i64"
                            | "i32"
                            | "i16"
                            | "i8"
                            | "u64"
                            | "u32"
                            | "u16"
                            | "u8"
                            | "isize"
                            | "usize"
                            | "f64"
                            | "f32"
                            | "bool"
                            | "str"
                            | "String"
                            | "()"
                            | "Decimal"
                            | "Dict"
                            | "Option"
                            | "Result"
                    ) || self.structs.contains_key(n.as_str())
                        || self.enums.contains_key(n.as_str())
                        || self.refine_bases.contains_key(n.as_str()))
            }
            T::TypeParam(_) | T::RawPtr(_) => false,
            T::Ref(x) | T::RefMut(x) => self.pins_runtime_type(&x, cx),
            T::Option(_)
            | T::Result { .. }
            | T::Slice(_)
            | T::Tuple(_)
            | T::Union(_)
            | T::DynTrait(_)
            | T::Fn { .. }
            | T::Chan(_) => true,
        }
    }

    fn cast_tparam(&self, v: &mut Value, n: &str, cx: &Cx, d: usize) -> Result<(), String> {
        let bound = cx.binds.as_ref().and_then(|b| b.borrow().get(n).cloned());
        match bound {
            Some(t) => {
                // Read in the environment the binding was written in, with
                // THIS cast's strictness: a strict crossing stays strict
                // through a binding (amendment 72).
                self.cast_at(v, &t, &cx.binding_cx(), d)?;
                // An operator-side value fills what the binding left
                // undetermined (`[]` then `[1]` at the same `T`).
                if !cx.strict && cx.parent.is_none() && has_undet(&t) {
                    if let Some(b) = &cx.binds {
                        let next = merge(&t, &self.value_type(v, 0));
                        b.borrow_mut().insert(n.to_string(), next);
                    }
                }
                Ok(())
            }
            None if cx.strict => Err(format!(
                "a value at the type parameter `{n}`, which no argument determined — \
                 no honest body can produce one"
            )),
            None => {
                for (p, traits) in cx.bounds.iter().flat_map(|b| b.iter()) {
                    if p == n {
                        for tr in traits {
                            self.check_impl(v, tr)?;
                        }
                    }
                }
                if let Some(b) = &cx.binds {
                    let t = self.value_type(v, 0);
                    b.borrow_mut().insert(n.to_string(), t);
                }
                Ok(())
            }
        }
    }

    /// `v`'s runtime type implements the program's trait `tr` (a trait the
    /// program does not define is not checked).
    fn check_impl(&self, v: &Value, tr: &str) -> Result<(), String> {
        let v = value::soft_inner(v).unwrap_or_else(|| v.clone());
        let tn = v.type_name();
        if self.user_traits.contains(tr)
            && !self.trait_impls.contains(&(tn.clone(), tr.to_string()))
        {
            return Err(format!(
                "a value of type `{tn}`, which does not implement `{tr}`"
            ));
        }
        Ok(())
    }

    /// The declared type a runtime value is evidence of — what a type
    /// parameter binds to. Parts the value does not show (an empty array's
    /// element, `None`'s payload, a generic struct's argument no field
    /// shows) are UNDETERMINED, never `?` (amendment 72).
    pub(crate) fn value_type(&self, v: &Value, depth: usize) -> T {
        if depth > MAX_CAST_DEPTH {
            return undet();
        }
        let d = depth + 1;
        let named = |n: &str| T::Named(n.into());
        match v {
            Value::Int(_) => named("i64"),
            Value::SizedInt { ty, .. } => T::Named(ty.display()),
            Value::Float(_) => named("f64"),
            Value::Decimal(_) => named("Decimal"),
            Value::Bool(_) => named("bool"),
            Value::Str(_) => named("str"),
            Value::Unit => named("()"),
            Value::Dict(_) => named("Dict"),
            Value::Handle { .. } => T::Named(format!("{HANDLE}{}", v.type_name())),
            Value::Array(xs) => {
                let mut t = undet();
                for x in xs.iter() {
                    if !has_undet(&t) {
                        break;
                    }
                    t = merge(&t, &self.value_type(x, d));
                }
                T::Slice(Box::new(t))
            }
            Value::Struct { name, fields } => match self.structs.get(name.as_str()) {
                Some(td) if !td.generic_params.is_empty() => {
                    let decls = fields.iter().filter_map(|(f, fv)| {
                        td.fields.iter().find(|x| x.name == *f).map(|x| (&x.ty, fv))
                    });
                    self.generic_type(name, &td.generic_params, decls, d)
                }
                _ => T::Named(name.clone()),
            },
            Value::Enum {
                enum_name,
                variant,
                fields,
            } => match self.enums.get(enum_name.as_str()) {
                Some(ed) if !ed.generic_params.is_empty() => {
                    let vd = ed.variants.iter().find(|x| x.name == *variant);
                    let decls = fields.iter().filter_map(|(f, fv)| {
                        vd.and_then(|vd| vd.fields.iter().find(|x| x.name == *f))
                            .map(|x| (&x.ty, fv))
                    });
                    self.generic_type(enum_name, &ed.generic_params, decls, d)
                }
                _ => T::Named(enum_name.clone()),
            },
            Value::Some(x) => T::Option(Box::new(self.value_type(x, d))),
            Value::None => T::Option(Box::new(undet())),
            Value::Ok(x) => T::Result {
                ok: Box::new(self.value_type(x, d)),
                err: Box::new(undet()),
            },
            Value::Err(x) => T::Result {
                ok: Box::new(undet()),
                err: Box::new(self.value_type(x, d)),
            },
            Value::Tuple(xs) => T::Tuple(xs.iter().map(|x| self.value_type(x, d)).collect()),
            Value::Chan(q) => T::Chan(Box::new(self.chan_stamp(q).unwrap_or_else(undet))),
            Value::Closure { params, .. } => T::Fn {
                params: params.iter().map(|_| undet()).collect(),
                ret: Box::new(undet()),
            },
        }
    }

    /// A generic struct's or enum's type, its arguments read off the fields
    /// that show them.
    fn generic_type<'a>(
        &self,
        name: &str,
        generics: &[String],
        fields: impl Iterator<Item = (&'a T, &'a Value)>,
        d: usize,
    ) -> T {
        let cx = Cx {
            tparams: Some(Rc::new(generics.to_vec())),
            binds: Some(Rc::new(RefCell::new(HashMap::new()))),
            ..Cx::default()
        };
        for (decl, fv) in fields {
            bind_from(decl, &self.value_type(fv, d), &cx);
        }
        let binds = cx
            .binds
            .as_ref()
            .map(|b| b.borrow().clone())
            .unwrap_or_default();
        T::Generic {
            base: name.to_string(),
            args: generics
                .iter()
                .map(|g| binds.get(g).cloned().unwrap_or_else(undet))
                .collect(),
        }
    }

    /// The element type a channel was stamped with that fixes its elements:
    /// the first closed, fully determined one.
    fn chan_stamp(&self, q: &Rc<RefCell<VecDeque<Value>>>) -> Option<T> {
        let tab = self.chan_contracts.borrow();
        let entry = tab.get(&(Rc::as_ptr(q) as usize))?;
        entry.1.iter().find_map(|c| {
            let t = resolve(&c.ret, &c.cx);
            (!mentions_tparam(&t, &c.cx) && !has_undet(&t) && !is_unstated(&t)).then_some(t)
        })
    }

    /// The channel table entry for `q`, made fresh when the address belonged
    /// to a channel that no longer exists.
    fn chan_entry<'t>(
        tab: &'t mut super::ChanContracts,
        q: &Rc<RefCell<VecDeque<Value>>>,
    ) -> &'t mut super::ChanEntry {
        let key = Rc::as_ptr(q) as usize;
        if tab.len() > 64 && tab.len().is_power_of_two() {
            tab.retain(|_, e| e.0.strong_count() > 0);
        }
        let stale = tab.get(&key).is_some_and(|e| e.0.upgrade().is_none());
        if stale {
            tab.remove(&key);
        }
        tab.entry(key)
            .or_insert_with(|| (Rc::downgrade(q), Vec::new(), false))
    }

    /// A channel is CREATED: record which side made it, and stamp it with the
    /// element type its `chan<T>()` states when that type is closed — so a
    /// generic `Chan<T>` it later crosses binds `T` from it, and a send of
    /// another type is refused (C9 round 4c, amendment 72). The text is the
    /// type rendered into the lowered callee name (`chan::<T>`).
    pub(crate) fn chan_created(&self, q: &Rc<RefCell<VecDeque<Value>>>, elem: Option<&str>) {
        let stamp = elem
            .and_then(crate::parser::parse_type_text)
            .filter(|t| self.is_known_closed(t));
        let operator_side = !(self.seal.active && self.frame_sealed.get());
        let mut tab = self.chan_contracts.borrow_mut();
        let key = Rc::as_ptr(q) as usize;
        tab.remove(&key);
        let entry = Self::chan_entry(&mut tab, q);
        entry.2 = operator_side;
        if let Some(t) = stamp {
            entry.1.push(Rc::new(Contract {
                params: Vec::new(),
                ret: t,
                cx: Cx::default(),
                next: None,
            }));
        }
    }

    /// Every name in `t` is a type this program (or the language) defines —
    /// no type parameter, no unknown name.
    fn is_known_closed(&self, t: &T) -> bool {
        let known = |n: &str| {
            matches!(
                n,
                "i64"
                    | "i32"
                    | "i16"
                    | "i8"
                    | "u64"
                    | "u32"
                    | "u16"
                    | "u8"
                    | "isize"
                    | "usize"
                    | "f64"
                    | "f32"
                    | "bool"
                    | "str"
                    | "String"
                    | "()"
                    | "Decimal"
                    | "Dict"
                    | "Option"
                    | "Result"
            ) || self.structs.contains_key(n)
                || self.enums.contains_key(n)
                || self.refine_bases.contains_key(n)
        };
        let go = |t: &T| self.is_known_closed(t);
        match t {
            T::Named(n) => known(n),
            T::TypeParam(_) | T::RawPtr(_) => false,
            T::DynTrait(_) => true,
            T::Generic { base, args } => known(base) && args.iter().all(go),
            T::Result { ok, err } => go(ok) && go(err),
            T::Option(x) | T::Chan(x) | T::Slice(x) | T::Ref(x) | T::RefMut(x) => go(x),
            T::Fn { params, ret } => params.iter().all(go) && go(ret),
            T::Tuple(xs) | T::Union(xs) => xs.iter().all(go),
        }
    }

    /// A channel is ONE invariant object: the element type it crossed is
    /// stamped on the object, the values already queued are cast now, and
    /// every later send is cast ([`Interp::chan_send_check`]). At `Chan<T>`
    /// with `T` unbound, `T` is bound from the channel's own stamp (the type
    /// it was created or first declared with); at a strict crossing a `T`
    /// nothing determined is refused (amendment 72).
    fn stamp_chan(
        &self,
        q: &Rc<RefCell<VecDeque<Value>>>,
        elem: &T,
        cx: &Cx,
    ) -> Result<(), String> {
        let root = cx.root();
        let mut elem = resolve(elem, cx);
        if mentions_tparam(&elem, &root) {
            if cx.strict {
                return Err(format!(
                    "a channel at the element type `{}`, which no argument determined — \
                     no honest body can produce one",
                    crate::doc::render_type(&elem)
                ));
            }
            if let Some(stamp) = self.chan_stamp(q) {
                bind_from(&elem, &stamp, &root);
                elem = subst(&elem, &root, false);
            }
        }
        let closed = !mentions_tparam(&elem, &root);
        let ecx = if closed {
            Cx::default()
        } else {
            root.strict(false)
        };
        for x in q.borrow_mut().iter_mut() {
            self.cast(x, &elem, &ecx.strict(cx.strict))?;
        }
        let mut tab = self.chan_contracts.borrow_mut();
        let entry = Self::chan_entry(&mut tab, q);
        let rendered = crate::doc::render_type(&elem);
        let dup = closed
            && entry
                .1
                .iter()
                .any(|c| !mentions_tparam_any(c) && crate::doc::render_type(&c.ret) == rendered);
        if !dup {
            entry.1.push(Rc::new(Contract {
                params: Vec::new(),
                ret: elem,
                cx: ecx,
                next: None,
            }));
        }
        Ok(())
    }

    /// Cast a value about to be sent on `q` to every element type the
    /// channel crossed. A SEALED frame's send is a seal crossing: it is cast
    /// strictly, and on a channel the operator created it needs an element
    /// type some operator-side value determined (amendment 72).
    pub(crate) fn chan_send_check(
        &self,
        q: &Rc<RefCell<VecDeque<Value>>>,
        v: &mut Value,
    ) -> Result<(), Flow> {
        let sealed = self.seal.active && self.frame_sealed.get();
        let (contracts, operator_side): (Vec<Rc<Contract>>, bool) =
            match self.chan_contracts.borrow().get(&(Rc::as_ptr(q) as usize)) {
                Some((_, cs, op)) => (cs.clone(), *op),
                None => (Vec::new(), false),
            };
        if sealed && operator_side && !contracts.iter().any(|c| determined(&c.ret, &c.cx)) {
            return panic(format!(
                "sealed code sent {} on a channel the operator created, whose element type \
                 nothing on the operator side determined — a runtime type confusion",
                value::display(v)
            ));
        }
        // Determined element types first, strictly; then the rest (a type
        // the channel crossed whose parameter is still free), which the
        // value — already pinned to a determined type — may bind.
        let (det, rest): (Vec<_>, Vec<_>) = contracts
            .into_iter()
            .partition(|c| determined(&c.ret, &c.cx));
        for (c, strict) in det
            .into_iter()
            .map(|c| (c, sealed))
            .chain(rest.into_iter().map(|c| (c, false)))
        {
            if let Err(why) = self.cast(v, &c.ret, &c.cx.strict(strict)) {
                return panic(format!(
                    "a channel declared `Chan<{}>` was sent {} — a runtime type confusion ({why})",
                    crate::doc::render_type(&c.ret),
                    value::display(v)
                ));
            }
        }
        Ok(())
    }

    /// Cast a closure call's arguments to every `fn` type the closure
    /// crossed, outermost (latest) first. `crossing`: a SEALED frame calls an
    /// OPERATOR closure — a seal crossing, so each argument is cast strictly
    /// and must meet a position some contract determined (amendment 72; the
    /// return direction's parametricity rule, applied to arguments).
    pub(crate) fn closure_args_check(
        &self,
        contract: &Option<Rc<Contract>>,
        args: &mut [Value],
        crossing: bool,
    ) -> Result<(), Flow> {
        if crossing {
            for (i, a) in args.iter().enumerate() {
                let mut c = contract.as_ref();
                let mut ok = false;
                while let Some(k) = c {
                    if k.params.get(i).is_some_and(|t| determined(t, &k.cx)) {
                        ok = true;
                        break;
                    }
                    c = k.next.as_ref();
                }
                if !ok {
                    return panic(format!(
                        "sealed code called an operator closure with {} as argument {}, a \
                         position nothing on the operator side determined — a runtime type \
                         confusion",
                        value::display(a),
                        i + 1
                    ));
                }
            }
        }
        // Outermost first. At a crossing, each argument meets the layers that
        // determine its position first, strictly; then the others (a type
        // the closure crossed whose parameter is still free, e.g. the
        // operator's own generic struct literal), which the argument —
        // already pinned to a determined type — may bind.
        let mut chain = Vec::new();
        let mut c = contract.as_ref();
        while let Some(k) = c {
            chain.push(k.clone());
            c = k.next.as_ref();
        }
        for (i, a) in args.iter_mut().enumerate() {
            let det = |k: &Rc<Contract>| k.params.get(i).is_some_and(|t| determined(t, &k.cx));
            let order = chain
                .iter()
                .filter(|k| !crossing || det(k))
                .map(|k| (k, crossing))
                .chain(
                    chain
                        .iter()
                        .filter(|k| crossing && !det(k))
                        .map(|k| (k, false)),
                );
            for (k, strict) in order {
                let Some(t) = k.params.get(i) else { continue };
                if let Err(why) = self.cast(a, t, &k.cx.strict(strict)) {
                    return panic(format!(
                        "argument {} of a closure declared `{}` — a runtime type confusion ({why})",
                        i + 1,
                        render_fn(k)
                    ));
                }
            }
        }
        Ok(())
    }

    /// Cast a closure's result to every `fn` type it crossed, innermost
    /// (earliest) first. `strict` at a seal crossing into operator code.
    pub(crate) fn closure_ret_check(
        &self,
        contract: &Option<Rc<Contract>>,
        v: &mut Value,
        strict: bool,
    ) -> Result<(), Flow> {
        let mut chain = Vec::new();
        let mut c = contract.as_ref();
        while let Some(k) = c {
            chain.push(k.clone());
            c = k.next.as_ref();
        }
        for k in chain.iter().rev() {
            if let Err(why) = self.cast(v, &k.ret, &k.cx.strict(strict)) {
                return panic(format!(
                    "a closure declared `{}` returned {} — a runtime type confusion ({why})",
                    render_fn(k),
                    value::display(v)
                ));
            }
        }
        Ok(())
    }

    /// Cast the value given for field `field` of the struct `owner` (or the
    /// enum variant `Enum::Variant`) to the field's declared type. The
    /// definition's own type parameters are unconstrained here.
    pub(crate) fn cast_field(&self, owner: &str, field: &str, v: &mut Value) -> Result<(), String> {
        let (generics, fields) = match owner.split_once("::") {
            Some((e, var)) => match self.enums.get(e).copied() {
                Some(ed) => match ed.variants.iter().find(|x| x.name == var) {
                    Some(vd) => (&ed.generic_params, &vd.fields),
                    None => return Ok(()),
                },
                None => return Ok(()),
            },
            None => match self.structs.get(owner).copied() {
                Some(td) => (&td.generic_params, &td.fields),
                None => return Ok(()),
            },
        };
        let Some(tf) = fields.iter().find(|f| f.name == field) else {
            return Ok(());
        };
        let cx = Cx {
            tparams: (!generics.is_empty()).then(|| Rc::new(generics.clone())),
            ..Cx::default()
        };
        self.cast(v, &tf.ty, &cx)
    }

    /// The contract a lambda's own parameter annotations state.
    pub(crate) fn lambda_contract(params: &[crate::ast::LambdaParam]) -> Option<Rc<Contract>> {
        if params.iter().all(|p| p.ty.is_none()) {
            return None;
        }
        Some(Rc::new(Contract {
            params: params
                .iter()
                .map(|p| p.ty.clone().unwrap_or_else(any))
                .collect(),
            ret: any(),
            cx: Cx::default(),
            next: None,
        }))
    }
}

/// A declared `Uncertain<T>`/`Temporal<T>` (or the bare name).
pub(crate) fn is_soft_decl(ty: &T) -> bool {
    match ty {
        T::Generic { base: n, .. } | T::Named(n) => n == "Uncertain" || n == "Temporal",
        _ => false,
    }
}

/// The fixed width a declared integer name stands for (`i64`, `isize` and
/// `usize` are `Value::Int`, not a `SizedInt`).
fn sized_width(n: &str) -> crate::types::Type {
    use crate::types::Type as W;
    match n {
        "i32" => W::I32,
        "i16" => W::I16,
        "i8" => W::I8,
        "u64" => W::U64,
        "u32" => W::U32,
        "u16" => W::U16,
        _ => W::U8,
    }
}

/// `t` (read in `cx`) states a type for every position: no type parameter
/// left unbound, nothing undetermined, nothing unstated.
fn determined(t: &T, cx: &Cx) -> bool {
    let t = resolve(t, cx);
    !mentions_tparam(&t, &cx.root()) && !has_undet(&t) && !is_unstated(&t)
}

/// Whether `t` has a position whose type is not stated (`?`).
fn is_unstated(t: &T) -> bool {
    match t {
        T::Named(n) => n == "?",
        T::TypeParam(_) | T::DynTrait(_) => false,
        T::Result { ok, err } => is_unstated(ok) || is_unstated(err),
        T::Option(x) | T::Chan(x) | T::Slice(x) | T::Ref(x) | T::RefMut(x) | T::RawPtr(x) => {
            is_unstated(x)
        }
        T::Generic { args, .. } => args.iter().any(is_unstated),
        T::Fn { params, ret } => params.iter().any(is_unstated) || is_unstated(ret),
        T::Tuple(xs) | T::Union(xs) => xs.iter().any(is_unstated),
    }
}

fn mentions_tparam_any(c: &Contract) -> bool {
    c.params.iter().any(|t| mentions_tparam(t, &c.cx)) || mentions_tparam(&c.ret, &c.cx)
}

fn same_sig(a: &[T], ar: &T, b: &[T], br: &T) -> bool {
    use crate::doc::render_type as r;
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| r(x) == r(y)) && r(ar) == r(br)
}

fn render_fn(c: &Contract) -> String {
    crate::doc::render_type(&T::Fn {
        params: c.params.clone(),
        ret: Box::new(c.ret.clone()),
    })
}

// ── The operator's dicts (C9 round 4c, amendment 72 part 2) ─────────────────
//
// A `Dict` carries no element types, so a value the candidate stores in one
// the OPERATOR handed it meets no declared type on its way back, and the
// operator's untyped `dict_get(d, "k").ok()` ran the impl of whatever type the
// candidate chose. The position IS determined, though: by what the operator
// put there. So every dict the operator hands into sealed code is SNAPSHOTTED
// (the type of each value it holds, by `value_type`), and at every edge back
// to operator code a dict sealed code MUTATED is checked against it: a key the
// operator held may not now hold a value of another type. Keys the candidate
// ADDS are not constrained (nothing operator-side determined them — the same
// as a candidate-built dict, which operator code reads and compares like any
// other candidate output; an operator that needs a type pins it with
// `let x: T`).

/// The most entries one dict may have to cross a seal. Refused above this
/// (never skipped): the snapshot is one type per entry, taken once per dict
/// and refreshed only after an operator-side mutation.
pub(crate) const DICT_SNAP_MAX: usize = 1_000_000;

type DictRc = Rc<RefCell<std::collections::BTreeMap<String, Value>>>;

/// What the operator held in one dict when it handed it to sealed code.
pub(crate) struct DictSnap {
    weak: std::rc::Weak<RefCell<std::collections::BTreeMap<String, Value>>>,
    /// The value at each key as the operator held it (a clone: a nested dict is
    /// the same `Rc`, with its own snapshot). What a replacement is judged
    /// against, structurally (amendment 78).
    held: std::collections::BTreeMap<String, Value>,
    /// Sealed code mutated the dict since it was last verified.
    dirty: bool,
    /// [`Interp::dict_epoch`] when the snapshot was taken.
    epoch: u64,
}

impl<'p> Interp<'p> {
    /// OPERATOR code hands `v` to sealed code (call arguments, a candidate
    /// closure's arguments, an operator closure's result, a channel send):
    /// every dict in it is snapshotted. A snapshot taken since the last
    /// operator-side mutation, and not dirtied, is kept (O(1) per dict).
    pub(crate) fn dict_edge_in(&self, v: &Value) -> Result<(), Flow> {
        if !self.seal.active {
            return Ok(());
        }
        let mut found = Vec::new();
        let mut seen = std::collections::HashSet::new();
        self.walk_fresh(v, &mut seen, &mut found, 0)?;
        for m in found {
            self.dict_snapshot(&m)?;
        }
        Ok(())
    }

    /// The dicts of `v` that need a (re)snapshot: walks into a dict's values
    /// only when the dict itself does.
    fn walk_fresh(
        &self,
        v: &Value,
        seen: &mut std::collections::HashSet<usize>,
        out: &mut Vec<DictRc>,
        d: usize,
    ) -> Result<(), Flow> {
        Self::walk_depth_ok(d)?;
        match v {
            Value::Dict(m) => {
                let key = Rc::as_ptr(m) as *const () as usize;
                if !seen.insert(key) {
                    return Ok(());
                }
                let fresh = self.dict_snaps.borrow().get(&key).is_some_and(|s| {
                    !s.dirty
                        && s.epoch == self.dict_epoch.get()
                        && s.weak.upgrade().is_some_and(|w| Rc::ptr_eq(&w, m))
                });
                if fresh {
                    return Ok(());
                }
                out.push(m.clone());
                for x in m.borrow().values() {
                    self.walk_fresh(x, seen, out, d + 1)?;
                }
            }
            Value::Array(_) | Value::Tuple(_) => {
                let xs: &[Value] = match v {
                    Value::Array(a) => a.as_slice(),
                    Value::Tuple(t) => t.as_slice(),
                    _ => unreachable!(),
                };
                for x in xs {
                    self.walk_fresh(x, seen, out, d + 1)?;
                }
            }
            Value::Struct { fields, .. } | Value::Enum { fields, .. } => {
                for x in fields.values() {
                    self.walk_fresh(x, seen, out, d + 1)?;
                }
            }
            Value::Some(x) | Value::Ok(x) | Value::Err(x) => {
                self.walk_fresh(x, seen, out, d + 1)?
            }
            Value::Chan(q) => {
                for x in q.borrow().iter() {
                    self.walk_fresh(x, seen, out, d + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A value nested deeper than the cast's own bound is REFUSED, never left
    /// unvisited (its dicts would go unrecorded).
    pub(crate) fn walk_depth_ok(d: usize) -> Result<(), Flow> {
        if d > MAX_CAST_DEPTH {
            return panic(format!(
                "a value nested more than {MAX_CAST_DEPTH} deep cannot cross a seal: its dicts \
                 could not all be recorded"
            ));
        }
        Ok(())
    }

    fn dict_snapshot(&self, m: &DictRc) -> Result<(), Flow> {
        let len = m.borrow().len();
        if len > DICT_SNAP_MAX {
            return panic(format!(
                "a dict of {len} entries cannot cross a seal (more than {DICT_SNAP_MAX}): its \
                 values' types could not be recorded, so the candidate could retype them"
            ));
        }
        let held: std::collections::BTreeMap<String, Value> = m
            .borrow()
            .iter()
            .map(|(k, x)| (k.clone(), x.clone()))
            .collect();
        let key = Rc::as_ptr(m) as *const () as usize;
        let mut tab = self.dict_snaps.borrow_mut();
        if tab.len() > 64 && tab.len().is_power_of_two() {
            tab.retain(|_, s| s.weak.strong_count() > 0);
        }
        tab.insert(
            key,
            DictSnap {
                weak: Rc::downgrade(m),
                held,
                dirty: false,
                epoch: self.dict_epoch.get(),
            },
        );
        Ok(())
    }

    /// A dict was mutated (`dict_set`, `dict_remove`, `dict_inc`). By sealed
    /// code: if the operator handed it over, it is dirty until verified. By
    /// operator code: every snapshot is out of date (the next hand-over
    /// re-takes it).
    pub(crate) fn dict_mutated(&self, m: &DictRc) {
        if !self.seal.active {
            return;
        }
        if !self.frame_sealed.get() {
            self.dict_epoch.set(self.dict_epoch.get() + 1);
            return;
        }
        let key = Rc::as_ptr(m) as *const () as usize;
        if let Some(s) = self.dict_snaps.borrow_mut().get_mut(&key) {
            if s.weak.upgrade().is_some_and(|w| Rc::ptr_eq(&w, m)) {
                // The FIRST sealed mutation since the last verification: what
                // the dict holds NOW (before this mutation) is what the
                // operator held — it may have changed the dict since the
                // hand-over (round 6: a candidate closure that captured the
                // dict mutated it after the operator did, against a stale
                // snapshot). Operator code cannot have run in between without
                // an edge that verified, so the contents are operator-held.
                if !s.dirty {
                    s.held = m
                        .borrow()
                        .iter()
                        .map(|(k, x)| (k.clone(), x.clone()))
                        .collect();
                }
                s.dirty = true;
            }
        }
    }

    /// SEALED code returns control to OPERATOR code (a candidate fn or closure
    /// returns, sealed code calls an operator closure or an effect-handler
    /// arm): every dict sealed code mutated is checked against what the
    /// operator held — a key it held keeps a value of that type.
    pub(crate) fn dict_edge_out(&self) -> Result<(), Flow> {
        if !self.seal.active {
            return Ok(());
        }
        let dirty: Vec<(usize, DictRc)> = self
            .dict_snaps
            .borrow()
            .iter()
            .filter(|(_, s)| s.dirty)
            .filter_map(|(k, s)| s.weak.upgrade().map(|m| (*k, m)))
            .collect();
        for (key, m) in dirty {
            let verdict = {
                let tab = self.dict_snaps.borrow();
                let Some(s) = tab.get(&key) else { continue };
                let cur = m.borrow();
                let mut bad = None;
                let mut seen: std::collections::HashSet<(usize, usize)> =
                    std::collections::HashSet::new();
                for (k, old) in &s.held {
                    let Some(now) = cur.get(k) else { continue };
                    if let Err(why) = self.replaced_ok(old, now, &mut seen, 0) {
                        bad = Some(format!("key `{k}` {why}"));
                        break;
                    }
                }
                bad
            };
            if let Some(why) = verdict {
                return panic(format!(
                    "sealed code retyped a dict entry the operator handed it — a runtime type \
                     confusion ({why})"
                ));
            }
            if let Some(s) = self.dict_snaps.borrow_mut().get_mut(&key) {
                s.dirty = false;
            }
        }
        Ok(())
    }

    /// Whether `new`, now at a position where the operator held `old`, is a
    /// legitimate occupant (amendment 78). The position is determined by what
    /// the operator put there, DEEPLY: (1) `new` casts STRICTLY to the type
    /// `old` showed, so a position `old` did not show (a `None`'s payload, an
    /// empty array's element) is refused rather than left free; (2) a dict
    /// that REPLACES a held dict is judged against the held one's entries —
    /// a key present in both keeps its type, recursively (keys only `new`
    /// has are free, exactly as part 2's new keys); (3) the same through
    /// arrays, tuples, struct and enum fields and `Option`/`Result` payloads,
    /// so a container carrying a dict is never judged by its bare type; (4)
    /// an operator closure is never replaced by a candidate's.
    fn replaced_ok(
        &self,
        old: &Value,
        new: &Value,
        seen: &mut std::collections::HashSet<(usize, usize)>,
        d: usize,
    ) -> Result<(), String> {
        Self::walk_depth_ok(d).map_err(|_| {
            String::from("is nested too deeply to compare with what the operator held")
        })?;
        match (old, new) {
            (Value::Closure { captured: oc, .. }, Value::Closure { captured: nc, .. }) => {
                return if !oc.borrow().contains_key(SEALED_CLOSURE_MARK)
                    && nc.borrow().contains_key(SEALED_CLOSURE_MARK)
                {
                    Err("held an operator closure and now holds the candidate's".into())
                } else {
                    Ok(())
                };
            }
            (Value::Closure { .. }, _) => {
                return Err(format!(
                    "held a closure and now holds {}",
                    value::display(new)
                ))
            }
            (Value::Dict(o), Value::Dict(n)) => {
                if Rc::ptr_eq(o, n) {
                    return Ok(());
                }
                let (po, pn) = (
                    Rc::as_ptr(o) as *const () as usize,
                    Rc::as_ptr(n) as *const () as usize,
                );
                if !seen.insert((po, pn)) {
                    return Ok(());
                }
                // The held dict's entries as the operator held them: its own
                // snapshot when it has a live one, else what it holds now.
                let held: Vec<(String, Value)> = match self.dict_snaps.borrow().get(&po) {
                    Some(s) if s.weak.upgrade().is_some_and(|w| Rc::ptr_eq(&w, o)) => {
                        s.held.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                    }
                    _ => o
                        .borrow()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                };
                for (k, ov) in held {
                    let nv = n.borrow().get(&k).cloned();
                    if let Some(nv) = nv {
                        self.replaced_ok(&ov, &nv, seen, d + 1)
                            .map_err(|e| format!("(in the replacing dict) key `{k}` {e}"))?;
                    }
                }
                return Ok(());
            }
            _ => {}
        }
        // The leaf rule: strictly, against what `old` showed (a closure's
        // signature is not shown, so it is not judged here).
        let t = unshown_fn(&self.value_type(old, 0));
        let mut c = new.clone();
        if let Err(why) = self.cast(&mut c, &t, &Cx::default().strict(true)) {
            return Err(format!(
                "held a value of type `{}` and now holds {} ({why})",
                crate::doc::render_type(&t),
                value::display(new)
            ));
        }
        match (old, new) {
            (Value::Array(_), Value::Array(_)) | (Value::Tuple(_), Value::Tuple(_)) => {
                let (a, b): (&[Value], &[Value]) = match (old, new) {
                    (Value::Array(a), Value::Array(b)) => (a.as_slice(), b.as_slice()),
                    (Value::Tuple(a), Value::Tuple(b)) => (a.as_slice(), b.as_slice()),
                    _ => unreachable!(),
                };
                for (x, y) in a.iter().zip(b) {
                    self.replaced_ok(x, y, seen, d + 1)?;
                }
            }
            (
                Value::Struct {
                    name: n1,
                    fields: f1,
                },
                Value::Struct {
                    name: n2,
                    fields: f2,
                },
            ) if n1 == n2 => {
                for (k, x) in f1 {
                    if let Some(y) = f2.get(k) {
                        self.replaced_ok(x, y, seen, d + 1)?;
                    }
                }
            }
            (
                Value::Enum {
                    enum_name: e1,
                    variant: v1,
                    fields: f1,
                },
                Value::Enum {
                    enum_name: e2,
                    variant: v2,
                    fields: f2,
                },
            ) if e1 == e2 && v1 == v2 => {
                for (k, x) in f1 {
                    if let Some(y) = f2.get(k) {
                        self.replaced_ok(x, y, seen, d + 1)?;
                    }
                }
            }
            (Value::Some(x), Value::Some(y))
            | (Value::Ok(x), Value::Ok(y))
            | (Value::Err(x), Value::Err(y)) => self.replaced_ok(x, y, seen, d + 1)?,
            _ => {}
        }
        Ok(())
    }

    /// An effect-handler arm of provenance `arm_sealed` is about to run for an
    /// operation the running frame performed with `payload`: the edge between
    /// the two provenances.
    pub(crate) fn handler_edge_into(&self, arm_sealed: bool, payload: &Value) -> Result<(), Flow> {
        match (self.frame_sealed.get(), arm_sealed) {
            (true, false) => self.dict_edge_out(),
            (false, true) => self.dict_edge_in(payload),
            _ => Ok(()),
        }
    }

    /// The arm resumed the operation with `v`: back across the same edge.
    pub(crate) fn handler_edge_back(&self, arm_sealed: bool, v: &Value) -> Result<(), Flow> {
        match (self.frame_sealed.get(), arm_sealed) {
            (true, false) => self.dict_edge_in(v),
            _ => Ok(()),
        }
    }
}

/// `t` with every `fn` type replaced by the unstated `?`: a closure's
/// signature is not shown by the value, so a strict cast must not refuse it.
fn unshown_fn(t: &T) -> T {
    let u = |x: &T| Box::new(unshown_fn(x));
    match t {
        T::Fn { .. } => any(),
        T::Result { ok, err } => T::Result {
            ok: u(ok),
            err: u(err),
        },
        T::Option(x) => T::Option(u(x)),
        T::Chan(x) => T::Chan(u(x)),
        T::Slice(x) => T::Slice(u(x)),
        T::Ref(x) => T::Ref(u(x)),
        T::RefMut(x) => T::RefMut(u(x)),
        T::RawPtr(x) => T::RawPtr(u(x)),
        T::Generic { base, args } => T::Generic {
            base: base.clone(),
            args: args.iter().map(unshown_fn).collect(),
        },
        T::Tuple(xs) => T::Tuple(xs.iter().map(unshown_fn).collect()),
        T::Union(xs) => T::Union(xs.iter().map(unshown_fn).collect()),
        T::Named(_) | T::TypeParam(_) | T::DynTrait(_) => t.clone(),
    }
}

#[cfg(test)]
mod walk_bound_tests {
    use super::*;

    /// The walk for a handed value's dicts REFUSES a value nested deeper than
    /// the cast's own bound; it never returns with the rest unvisited. (A real
    /// value that deep overflows the thread stack first, so the bound's
    /// logic is tested directly.)
    #[test]
    fn a_value_nested_past_the_bound_is_refused_not_left_unvisited() {
        assert!(Interp::walk_depth_ok(0).is_ok());
        assert!(Interp::walk_depth_ok(MAX_CAST_DEPTH).is_ok());
        let past = Interp::walk_depth_ok(MAX_CAST_DEPTH + 1);
        assert!(
            matches!(&past, Err(Flow::Panic(m)) if m.contains("cannot cross a seal")),
            "ATTACK: a value nested past the walk bound crossed unvisited: {past:?}"
        );
    }
}
