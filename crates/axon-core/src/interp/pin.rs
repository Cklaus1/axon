//! The dispatch rule (C9 round 6, PSV-1, amendment 83; rebuilt in round 7,
//! amendment 88).
//!
//! In a SEALED run, operator code may not dispatch an operator impl's method
//! on a receiver whose TYPE the OPERATOR did not choose, unless the receiver's
//! runtime value is an operator-defined struct or enum (which the candidate
//! cannot construct: E0004). Every "determined" rule below is justified as
//! "the operator chose this type", and the analysis is FAIL-CLOSED: a site the
//! analysis did not record as determined is undetermined.
//!
//! DETERMINED: a literal; a binding with a closed `let`/parameter annotation
//! (the value was cast to it); a binding every one of whose initialisers is
//! determined; a call of an OPERATOR fn with a closed declared return (cast at
//! its return) — never a candidate fn, whatever it declares, since the
//! candidate chose that declaration; a builtin whose declared return names no
//! type variable; `x as T`; arithmetic on determined operands; containers of
//! determined parts; a field/element/match-binding of a determined value; a
//! call of an operator METHOD name every definition of which is the
//! operator's and closed.
//!
//! "Closed" (what an annotation can pin): scalars, `Dict`, `Option`/`Result`/
//! `[T]`/tuples/`Chan` of closed, and OPERATOR-defined structs, enums and
//! refinements whose fields are closed. NOT closed: a trait name, `dyn`, a
//! type parameter, `Self`, any type a sealed module defines, any unknown
//! name — none of which constrains the concrete runtime type to one the
//! operator chose.
//!
//! NOT DETERMINED: an untyped read (`dict_get`, `recv`, ...), an unannotated
//! lambda parameter, a call of a LOCAL binding (a closure value named like an
//! operator fn), a call of a candidate fn, a channel method, anything open.
//!
//! The analysis is static and name-keyed per function (a name is determined
//! only if EVERY binding of it, and every assignment to it, is — a greatest
//! fixpoint). A site is keyed by (the fn that owns it, a structural hash of
//! the site): within one fn, equal text means equal names and so an equal
//! verdict, and across fns the keys never meet. A lambda body is owned by the
//! fn that created it (the closure remembers it).

use crate::ast::{AxonType as T, BinOp, Expr, FnDef, Item, Pattern, Program, UnaryOp};
use crate::span::Span;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// The key of one method-call site (within its owning fn).
pub(crate) fn call_key(receiver: &Expr, method: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{receiver:?}").hash(&mut h);
    method.hash(&mut h);
    h.finish()
}

/// The key of one arithmetic site (within its owning fn).
pub(crate) fn binop_key(op: &BinOp, left: &Expr, right: &Expr) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{op:?}|{left:?}|{right:?}").hash(&mut h);
    h.finish()
}

/// The key of one UNARY arithmetic site (`-x`, `~x`) within its owning fn.
pub(crate) fn unary_key(op: &UnaryOp, operand: &Expr) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("unary|{op:?}|{operand:?}").hash(&mut h);
    h.finish()
}

#[derive(Default)]
pub(crate) struct Pins {
    /// (owning fn, site key) of every site whose receiver/operands ARE
    /// determined. Fail-closed: a site not here is undetermined.
    determined: HashSet<(usize, u64)>,
    /// Method name -> the operator impl types that define it. With fewer than
    /// two there is no impl to select between.
    impls: HashMap<String, HashSet<String>>,
    /// Struct and enum names the OPERATOR defines (and no sealed module does).
    op_types: HashSet<String>,
}

/// What an annotation can pin, and what the operator defines.
#[derive(Default)]
struct Tys {
    /// Operator structs: generics and field types.
    structs: HashMap<String, (Vec<String>, Vec<T>)>,
    /// Operator enums: generics and every variant's field types.
    enums: HashMap<String, (Vec<String>, Vec<T>)>,
    refines: HashSet<String>,
    /// Names that never pin: every trait, and every type a sealed module defines.
    open: HashSet<String>,
}

const SCALARS: &[&str] = &[
    "i64", "i32", "i16", "i8", "u64", "u32", "u16", "u8", "isize", "usize", "f64", "f32", "bool",
    "str", "String", "()", "Decimal", "Dict",
];

impl Tys {
    /// Whether `t` names only types the operator chose and that constrain the
    /// runtime type. `ok`: type parameters of the struct/enum being expanded,
    /// whose arguments were checked at the use site.
    fn closed(&self, t: &T, gp: &[String], ok: &[String], seen: &mut HashSet<String>) -> bool {
        let go = |x: &T, seen: &mut HashSet<String>| self.closed(x, gp, ok, seen);
        match t {
            T::Named(n) => {
                if n == "?" || gp.contains(n) || self.open.contains(n) {
                    return false;
                }
                if ok.contains(n) || SCALARS.contains(&n.as_str()) || self.refines.contains(n) {
                    return true;
                }
                match self.structs.get(n).or_else(|| self.enums.get(n)) {
                    Some((g, fields)) if g.is_empty() => self.fields_closed(n, fields, &[], seen),
                    _ => false,
                }
            }
            T::TypeParam(n) => ok.contains(n) && !gp.contains(n),
            T::DynTrait(_) => false,
            T::Result { ok: o, err } => go(o, seen) && go(err, seen),
            T::Option(x) | T::Chan(x) | T::Slice(x) | T::Ref(x) | T::RefMut(x) | T::RawPtr(x) => {
                go(x, seen)
            }
            T::Generic { base, args } => {
                if gp.contains(base) || self.open.contains(base) {
                    return false;
                }
                if !args.iter().all(|a| go(a, seen)) {
                    return false;
                }
                match self.structs.get(base).or_else(|| self.enums.get(base)) {
                    Some((g, fields)) if g.len() == args.len() => {
                        self.fields_closed(base, fields, g, seen)
                    }
                    _ => matches!(base.as_str(), "Option" | "Result" | "Dict" | "Chan"),
                }
            }
            T::Fn { params, ret } => params.iter().all(|a| go(a, seen)) && go(ret, seen),
            T::Tuple(xs) => xs.iter().all(|a| go(a, seen)),
            // A union admits more than one runtime type (`i64 | u8`): it does
            // not pin the one the operator chose.
            T::Union(_) => false,
        }
    }

    fn fields_closed(
        &self,
        name: &str,
        fields: &[T],
        generics: &[String],
        seen: &mut HashSet<String>,
    ) -> bool {
        if !seen.insert(name.to_string()) {
            return true; // a recursive type: its other fields decide
        }
        let r = fields.iter().all(|f| self.closed(f, &[], generics, seen));
        seen.remove(name);
        r
    }

    fn is_closed(&self, t: &T, gp: &[String]) -> bool {
        self.closed(t, gp, &[], &mut HashSet::new())
    }
}

/// A builtin's declared return type names a single-letter type variable
/// (`T`, `U`, `V`): its element comes from the arguments, untyped.
pub(crate) fn builtin_ret_open(ret: &str) -> bool {
    let mut cur = String::new();
    for c in ret.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || c == '_' {
            cur.push(c);
        } else {
            if cur.len() == 1 && cur.chars().all(|c| c.is_ascii_uppercase()) {
                return true;
            }
            cur.clear();
        }
    }
    false
}

/// Method names a channel value answers itself (`eval.rs`, before any impl
/// lookup): an operator method of the same name does not make them operator-typed.
const CHAN_METHODS: &[&str] = &["send", "recv", "try_recv", "len", "clone"];

enum Fact<'a> {
    Pinned,
    Unpinned,
    /// Determined iff this expression is.
    From(&'a Expr),
}

struct Ctx<'a> {
    tys: Tys,
    /// OPERATOR free fn name -> every definition declares a closed return.
    op_ret: HashMap<&'a str, bool>,
    /// OPERATOR method name -> every definition declares a closed return.
    op_method: HashMap<&'a str, bool>,
    /// Names of free fns a sealed module defines (a method call a sealed impl
    /// would answer is refused by `seal_method`, not decided here).
    sealed_names: HashSet<&'a str>,
    builtin_ret: HashMap<&'static str, &'static str>,
    globals: HashSet<String>,
}

fn pattern_names(p: &Pattern, out: &mut Vec<String>) {
    match p {
        Pattern::Ident(n) => out.push(n.clone()),
        Pattern::Some(x) | Pattern::Ok(x) | Pattern::Err(x) => pattern_names(x, out),
        Pattern::Struct { fields, .. } => fields.iter().for_each(|(_, p)| pattern_names(p, out)),
        Pattern::Tuple(xs) => xs.iter().for_each(|p| pattern_names(p, out)),
        Pattern::Wildcard | Pattern::Literal(_) | Pattern::None => {}
    }
}

fn place_root(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n) => Some(n),
        Expr::FieldAccess { receiver, .. } | Expr::Index { receiver, .. } => place_root(receiver),
        _ => None,
    }
}

impl<'a> Ctx<'a> {
    fn det(&self, e: &Expr, local: &HashSet<String>, bound: &HashSet<String>) -> bool {
        let d = |x: &Expr| self.det(x, local, bound);
        match e {
            Expr::Literal(_) | Expr::None | Expr::Lambda { .. } | Expr::FmtStr { .. } => true,
            Expr::Ident(n) => {
                if bound.contains(n) {
                    local.contains(n)
                } else {
                    self.globals.contains(n)
                }
            }
            Expr::Some(x) | Expr::Ok(x) | Expr::Err(x) | Expr::Question(x) | Expr::Comptime(x) => {
                d(x)
            }
            Expr::Tuple(xs) | Expr::Array(xs) => xs.iter().all(d),
            Expr::StructLit { fields, .. } => fields.iter().all(|(_, x)| d(x)),
            Expr::FieldAccess { receiver, .. } | Expr::Index { receiver, .. } => d(receiver),
            Expr::BinOp { op, left, right } => match op {
                BinOp::Eq
                | BinOp::NotEq
                | BinOp::Lt
                | BinOp::Gt
                | BinOp::LtEq
                | BinOp::GtEq
                | BinOp::And
                | BinOp::Or => true,
                _ => d(left) && d(right),
            },
            Expr::UnaryOp { op, operand } => matches!(op, UnaryOp::Not) || d(operand),
            Expr::Block(ss) => ss.last().map(|s| d(&s.expr)).unwrap_or(true),
            Expr::If { then, else_, .. } => d(then) && else_.as_ref().is_none_or(|x| d(x)),
            Expr::Match { arms, .. } => arms.iter().all(|a| d(&a.body)),
            Expr::Assign { value, .. } => d(value),
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::Ident(name) => {
                    // A local binding (a closure value) is judged by ITS
                    // initialiser, never by a global fn that shares its name,
                    // and a closure's result is untyped: not determined.
                    if bound.contains(name) || self.sealed_names.contains(name.as_str()) {
                        false
                    } else if let Some(c) = self.op_ret.get(name.as_str()) {
                        *c && !self.builtin_ret.contains_key(name.as_str())
                    } else if let Some(r) = self.builtin_ret.get(name.as_str()) {
                        !builtin_ret_open(r)
                    } else {
                        false
                    }
                }
                _ => false,
            },
            Expr::MethodCall { method, .. } => {
                !CHAN_METHODS.contains(&method.as_str())
                    && self
                        .op_method
                        .get(method.as_str())
                        .copied()
                        .unwrap_or(false)
            }
            Expr::Let { .. }
            | Expr::Own { .. }
            | Expr::RefBind { .. }
            | Expr::While { .. }
            | Expr::WhileLet { .. }
            | Expr::For { .. }
            | Expr::Break
            | Expr::Continue
            | Expr::Return(_)
            | Expr::AssignTo { .. } => true,
            Expr::Spawn(_)
            | Expr::Select(_)
            | Expr::InlineAsm { .. }
            | Expr::WithHandler { .. } => false,
        }
    }
}

impl Pins {
    pub(crate) fn build(prog: &Program, sealed: &dyn Fn(Span) -> bool) -> Pins {
        let mut tys = Tys::default();
        let mut impls: HashMap<String, HashSet<String>> = HashMap::new();
        let mut sealed_names: HashSet<&str> = HashSet::new();
        let mut cand_types: HashSet<String> = HashSet::new();
        let mut op_fns: Vec<(&FnDef, Vec<String>)> = Vec::new();
        let mut lets: Vec<(&str, &Expr)> = Vec::new();
        // Pass 1: provenance of every named thing.
        for item in &prog.items {
            match item {
                Item::TraitDef(t) => {
                    tys.open.insert(t.name.clone());
                }
                Item::TypeDef(t) if sealed(t.span) => {
                    cand_types.insert(t.name.clone());
                }
                Item::EnumDef(e) if sealed(e.span) => {
                    cand_types.insert(e.name.clone());
                }
                Item::RefineDef(r) if sealed(r.span) => {
                    cand_types.insert(r.name.clone());
                }
                Item::TypeDef(t) => {
                    tys.structs.insert(
                        t.name.clone(),
                        (
                            t.generic_params.clone(),
                            t.fields.iter().map(|f| f.ty.clone()).collect(),
                        ),
                    );
                }
                Item::EnumDef(e) => {
                    tys.enums.insert(
                        e.name.clone(),
                        (
                            e.generic_params.clone(),
                            e.variants
                                .iter()
                                .flat_map(|v| v.fields.iter().map(|f| f.ty.clone()))
                                .collect(),
                        ),
                    );
                }
                Item::RefineDef(r) => {
                    tys.refines.insert(r.name.clone());
                }
                _ => {}
            }
        }
        for n in &cand_types {
            tys.open.insert(n.clone());
            tys.structs.remove(n);
            tys.enums.remove(n);
            tys.refines.remove(n);
        }
        let mut op_ret: HashMap<&str, bool> = HashMap::new();
        let mut op_method: HashMap<&str, bool> = HashMap::new();
        // Pass 2: fns and impls.
        for item in &prog.items {
            match item {
                Item::FnDef(f) if sealed(f.span) => {
                    sealed_names.insert(&f.name);
                }
                Item::FnDef(f) => {
                    let c = f
                        .return_type
                        .as_ref()
                        .is_some_and(|t| tys.is_closed(t, &f.generic_params));
                    op_ret.entry(&f.name).and_modify(|x| *x &= c).or_insert(c);
                    op_fns.push((f, f.generic_params.clone()));
                }
                Item::ImplBlock(b) => {
                    let mut gp = b.generic_params.clone();
                    for m in &b.methods {
                        if sealed(m.span) || sealed(b.span) {
                            continue;
                        }
                        gp.extend(m.generic_params.clone());
                        let c = m
                            .return_type
                            .as_ref()
                            .is_some_and(|t| tys.is_closed(t, &gp));
                        op_method
                            .entry(&m.name)
                            .and_modify(|x| *x &= c)
                            .or_insert(c);
                        impls
                            .entry(m.name.clone())
                            .or_default()
                            .insert(crate::doc::render_type(&b.for_type));
                        op_fns.push((m, gp.clone()));
                    }
                }
                Item::LetDef { name, value, span } if !sealed(*span) => lets.push((name, value)),
                _ => {}
            }
        }
        let op_types: HashSet<String> = tys
            .structs
            .keys()
            .chain(tys.enums.keys())
            .filter(|n| !cand_types.contains(*n))
            .cloned()
            .collect();
        let mut ctx = Ctx {
            tys,
            op_ret,
            op_method,
            sealed_names,
            builtin_ret: crate::builtins::BUILTINS
                .iter()
                .map(|b| (b.name, b.ret))
                .collect(),
            globals: HashSet::new(),
        };
        // Module-level lets (the operator's only: a sealed let is not pushed): a
        // greatest fixpoint over their initializers.
        ctx.globals = lets.iter().map(|(n, _)| n.to_string()).collect();
        loop {
            let none = HashSet::new();
            let next: HashSet<String> = lets
                .iter()
                .filter(|(n, e)| ctx.globals.contains(*n) && ctx.det(e, &none, &none))
                .map(|(n, _)| n.to_string())
                .collect();
            if next == ctx.globals {
                break;
            }
            ctx.globals = next;
        }
        let mut determined = HashSet::new();
        for (f, gp) in &op_fns {
            let params: Vec<(&str, &T)> =
                f.params.iter().map(|p| (p.name.as_str(), &p.ty)).collect();
            analyze(
                &f.body,
                &params,
                gp,
                &ctx,
                *f as *const FnDef as usize,
                &mut determined,
            );
        }
        for (_, e) in &lets {
            analyze(e, &[], &[], &ctx, 0, &mut determined);
        }
        Pins {
            determined,
            impls,
            op_types,
        }
    }

    /// Whether the method-call site, owned by fn `owner`, has a determined receiver.
    pub(crate) fn determined(&self, owner: usize, receiver: &Expr, method: &str) -> bool {
        self.determined
            .contains(&(owner, call_key(receiver, method)))
    }

    /// Whether the arithmetic site has only determined operands.
    pub(crate) fn determined_arith(
        &self,
        owner: usize,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
    ) -> bool {
        self.determined
            .contains(&(owner, binop_key(op, left, right)))
    }

    /// Whether a unary arithmetic site (`-x`, `~x`) has a determined operand.
    pub(crate) fn determined_unary(&self, owner: usize, op: &UnaryOp, operand: &Expr) -> bool {
        self.determined.contains(&(owner, unary_key(op, operand)))
    }

    /// Whether `method` can select between operator impls (two or more types).
    pub(crate) fn selects_between_impls(&self, method: &str) -> bool {
        self.impls.get(method).is_some_and(|s| s.len() >= 2)
    }

    /// Whether `name` is a struct or enum only the operator defines — a value
    /// of it cannot be built by sealed code (E0004), so its impl was chosen by
    /// the operator whatever the static analysis says about the receiver.
    pub(crate) fn is_operator_type(&self, name: &str) -> bool {
        self.op_types.contains(name)
    }
}

fn analyze(
    body: &Expr,
    params: &[(&str, &T)],
    gp: &[String],
    ctx: &Ctx,
    owner: usize,
    out: &mut HashSet<(usize, u64)>,
) {
    let mut facts: Vec<(String, Fact)> = Vec::new();
    for (name, ty) in params {
        facts.push((
            name.to_string(),
            if ctx.tys.is_closed(ty, gp) {
                Fact::Pinned
            } else {
                Fact::Unpinned
            },
        ));
    }
    let mut calls: Vec<&Expr> = Vec::new();
    fn collect<'a>(
        e: &'a Expr,
        gp: &[String],
        tys: &Tys,
        facts: &mut Vec<(String, Fact<'a>)>,
        calls: &mut Vec<&'a Expr>,
    ) {
        let mut visit = |x: &'a Expr| match x {
            Expr::Let { name, ty, value }
            | Expr::Own { name, ty, value }
            | Expr::RefBind { name, ty, value } => facts.push((
                name.clone(),
                match ty {
                    Some(t) if tys.is_closed(t, gp) => Fact::Pinned,
                    Some(_) => Fact::Unpinned,
                    None => Fact::From(value),
                },
            )),
            Expr::Assign { name, value } => facts.push((name.clone(), Fact::From(value))),
            Expr::AssignTo { place, value } => {
                if let Some(r) = place_root(place) {
                    facts.push((r.to_string(), Fact::From(value)));
                }
            }
            // `f(&mut x)`: the callee may leave ANY value in `x` (a candidate fn
            // writes through the borrow), so `x` takes its value from a source
            // nothing on the operator side determined — fail closed (C9 round 8).
            Expr::Call { args, .. } => {
                for a in args {
                    if let Expr::UnaryOp {
                        op: UnaryOp::RefMut,
                        operand,
                    } = a
                    {
                        if let Some(r) = place_root(operand) {
                            facts.push((r.to_string(), Fact::Unpinned));
                        }
                    }
                }
            }
            Expr::Match { subject, arms } => {
                for a in arms {
                    let mut ns = Vec::new();
                    pattern_names(&a.pattern, &mut ns);
                    for n in ns {
                        facts.push((n, Fact::From(subject)));
                    }
                }
            }
            Expr::WhileLet { pattern, expr, .. } => {
                let mut ns = Vec::new();
                pattern_names(pattern, &mut ns);
                for n in ns {
                    facts.push((n, Fact::From(expr)));
                }
            }
            Expr::For { var, .. } => facts.push((var.clone(), Fact::Pinned)),
            Expr::Lambda { params, .. } => {
                for p in params {
                    facts.push((
                        p.name.clone(),
                        match &p.ty {
                            Some(t) if tys.is_closed(t, gp) => Fact::Pinned,
                            _ => Fact::Unpinned,
                        },
                    ));
                }
            }
            Expr::MethodCall { .. } | Expr::BinOp { .. } => calls.push(x),
            Expr::UnaryOp {
                op: UnaryOp::Neg | UnaryOp::BitNot,
                ..
            } => calls.push(x),
            _ => {}
        };
        crate::ast::walk_expr(e, &mut visit);
    }
    collect(body, gp, &ctx.tys, &mut facts, &mut calls);
    let bound: HashSet<String> = facts.iter().map(|(n, _)| n.clone()).collect();
    let mut local: HashSet<String> = bound.clone();
    loop {
        let mut next = local.clone();
        for (n, fact) in &facts {
            let ok = match fact {
                Fact::Pinned => true,
                Fact::Unpinned => false,
                Fact::From(e) => ctx.det(e, &local, &bound),
            };
            if !ok {
                next.remove(n);
            }
        }
        if next == local {
            break;
        }
        local = next;
    }
    for c in calls {
        match c {
            Expr::MethodCall {
                receiver, method, ..
            } => {
                if ctx.det(receiver, &local, &bound) {
                    out.insert((owner, call_key(receiver, method)));
                }
            }
            Expr::BinOp { op, left, right }
                if ctx.det(left, &local, &bound) && ctx.det(right, &local, &bound) =>
            {
                out.insert((owner, binop_key(op, left, right)));
            }
            Expr::UnaryOp { op, operand } if ctx.det(operand, &local, &bound) => {
                out.insert((owner, unary_key(op, operand)));
            }
            _ => {}
        }
    }
}
