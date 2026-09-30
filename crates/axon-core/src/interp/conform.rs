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
//! * scalars by kind (every integer width is one kind), `str`, `bool`, `()`;
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
//! handle, `Dict`, `Uncertain<T>`) is accepted: the check refuses only what it
//! can show is a different type, so no honest program is rejected.

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

/// The declared type a runtime value is evidence of — what a type parameter
/// binds to. Unknown parts are `?`.
fn shape(v: &Value) -> T {
    match v {
        Value::Int(_) => T::Named("i64".into()),
        Value::SizedInt { ty, .. } => T::Named(ty.display()),
        Value::Float(_) => T::Named("f64".into()),
        Value::Decimal(_) => T::Named("Decimal".into()),
        Value::Bool(_) => T::Named("bool".into()),
        Value::Str(_) => T::Named("str".into()),
        Value::Unit => T::Named("()".into()),
        Value::Array(xs) => T::Slice(Box::new(xs.first().map(shape).unwrap_or_else(any))),
        Value::Struct { name, .. } => T::Named(name.clone()),
        Value::Enum { enum_name, .. } => T::Named(enum_name.clone()),
        Value::Some(x) => T::Option(Box::new(shape(x))),
        Value::None => T::Option(Box::new(any())),
        Value::Ok(x) => T::Result {
            ok: Box::new(shape(x)),
            err: Box::new(any()),
        },
        Value::Err(x) => T::Result {
            ok: Box::new(any()),
            err: Box::new(shape(x)),
        },
        Value::Tuple(xs) => T::Tuple(xs.iter().map(shape).collect()),
        Value::Chan(_) => T::Chan(Box::new(any())),
        Value::Closure { .. } | Value::Dict(_) | Value::Handle { .. } => any(),
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
        T::Option(x) | T::Chan(x) | T::Slice(x) | T::Ref(x) | T::RawPtr(x) => go(x),
        T::Generic { args, .. } => args.iter().any(go),
        T::Fn { params, ret } => params.iter().any(go) || go(ret),
        T::Tuple(xs) | T::Union(xs) => xs.iter().any(go),
    }
}

/// The FIELD environment of a struct/enum definition read at `args`.
fn field_cx(generics: &[String], args: &[T], outer: &Cx) -> Cx {
    if generics.is_empty() {
        return Cx::default();
    }
    let map: HashMap<String, T> = generics
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let a = args
                .get(i)
                .map(|a| subst(a, outer, true))
                .unwrap_or_else(any);
            (g.clone(), a)
        })
        .collect();
    Cx {
        tparams: Some(Rc::new(generics.to_vec())),
        binds: Some(Rc::new(RefCell::new(map))),
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
        if !matches!(ty, T::Generic { base, .. } if base == "Uncertain" || base == "Temporal") {
            if let Some(mut inner) = value::soft_inner(v) {
                return self.cast_at(&mut inner, ty, cx, d);
            }
        }
        match ty {
            T::Named(n) => self.cast_named(v, n, &[], ty, cx, d),
            T::TypeParam(n) if cx.is_tparam(n) => self.cast_tparam(v, n, cx, d),
            T::TypeParam(_) | T::RawPtr(_) => Ok(()),
            T::Ref(inner) => self.cast_at(v, inner, cx, d),
            T::Option(inner) => self.cast_option(v, inner, ty, cx, d),
            T::Result { ok, err } => self.cast_result(v, ok, err, ty, cx, d),
            T::Generic { base, args } => match (base.as_str(), args.as_slice()) {
                ("Option", [inner]) => self.cast_option(v, inner, ty, cx, d),
                ("Result", [ok, err]) => self.cast_result(v, ok, err, ty, cx, d),
                ("Uncertain" | "Temporal", _) => Ok(()),
                _ => self.cast_named(v, base, args, ty, cx, d),
            },
            T::Slice(inner) => match v {
                Value::Array(xs) => {
                    for x in xs.iter_mut() {
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
                    // and bind through the SHARED activation cell.
                    let params: Vec<T> = params.iter().map(|t| subst(t, cx, false)).collect();
                    let ret = subst(ret, cx, false);
                    let free =
                        params.iter().any(|t| mentions_tparam(t, cx)) || mentions_tparam(&ret, cx);
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
                        cx.strict(false)
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
                // A named fn is referred to by name; its own signature is
                // checked when it is called.
                _ => Ok(()),
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
            "Self" => {
                return match cx.self_ty.clone() {
                    Some(s) => self.cast_at(v, &s, &Cx::default(), d),
                    None => Ok(()),
                }
            }
            "i64" | "i32" | "i16" | "i8" | "u64" | "u32" | "u16" | "u8" | "isize" | "usize" => {
                return kind_ok(matches!(v, Value::Int(_) | Value::SizedInt { .. }))
            }
            "f64" | "f32" => return kind_ok(matches!(v, Value::Float(_))),
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
        // A name the interpreter does not know (a builtin handle, `Dict`, a
        // type from outside this program): nothing to show it is different.
        Ok(())
    }

    fn cast_tparam(&self, v: &mut Value, n: &str, cx: &Cx, d: usize) -> Result<(), String> {
        let bound = cx.binds.as_ref().and_then(|b| b.borrow().get(n).cloned());
        match bound {
            Some(t) => self.cast_at(v, &t, &Cx::default(), d),
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
                    b.borrow_mut().insert(n.to_string(), shape(v));
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

    /// A channel is ONE invariant object: the element type it crossed is
    /// stamped on the object, the values already queued are cast now, and
    /// every later send is cast ([`Interp::chan_send_check`]).
    fn stamp_chan(
        &self,
        q: &Rc<RefCell<VecDeque<Value>>>,
        elem: &T,
        cx: &Cx,
    ) -> Result<(), String> {
        let elem = subst(elem, cx, false);
        let ecx = if mentions_tparam(&elem, cx) {
            cx.strict(false)
        } else {
            Cx::default()
        };
        for x in q.borrow_mut().iter_mut() {
            self.cast(x, &elem, &ecx)?;
        }
        let key = Rc::as_ptr(q) as usize;
        let mut tab = self.chan_contracts.borrow_mut();
        if tab.len() > 64 && tab.len().is_power_of_two() {
            tab.retain(|_, (w, _)| w.strong_count() > 0);
        }
        let entry = tab
            .entry(key)
            .or_insert_with(|| (Rc::downgrade(q), Vec::new()));
        let rendered = crate::doc::render_type(&elem);
        let closed = !mentions_tparam(&elem, cx);
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
    /// channel crossed.
    pub(crate) fn chan_send_check(
        &self,
        q: &Rc<RefCell<VecDeque<Value>>>,
        v: &mut Value,
    ) -> Result<(), Flow> {
        let contracts: Vec<Rc<Contract>> =
            match self.chan_contracts.borrow().get(&(Rc::as_ptr(q) as usize)) {
                Some((_, cs)) => cs.clone(),
                None => return Ok(()),
            };
        for c in contracts {
            if let Err(why) = self.cast(v, &c.ret, &c.cx) {
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
    /// crossed, outermost (latest) first.
    pub(crate) fn closure_args_check(
        &self,
        contract: &Option<Rc<Contract>>,
        args: &mut [Value],
    ) -> Result<(), Flow> {
        let mut c = contract.as_ref();
        while let Some(k) = c {
            for (i, (a, t)) in args.iter_mut().zip(&k.params).enumerate() {
                if let Err(why) = self.cast(a, t, &k.cx) {
                    return panic(format!(
                        "argument {} of a closure declared `{}` — a runtime type confusion ({why})",
                        i + 1,
                        render_fn(k)
                    ));
                }
            }
            c = k.next.as_ref();
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
