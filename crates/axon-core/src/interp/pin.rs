//! The dispatch rule (C9 round 6, PSV-1, amendment 83).
//!
//! In a SEALED run, operator code may not dispatch an operator impl's method
//! on a receiver whose TYPE nothing on the operator side determined. The
//! impl is selected by the receiver's runtime type; for a value read from a
//! `Dict`, a channel, an unannotated lambda parameter or an unbound generic
//! position that type is whatever the candidate chose to store there, and with
//! it the operator's impl. This closes the class at the one place the key is
//! computed (`Expr::MethodCall` in `eval.rs`), not per container.
//!
//! A receiver is DETERMINED when its declared type is concrete at the call
//! site: a literal, a binding with a closed `let`/parameter annotation (the
//! value was cast to it), a call of a fn with a closed declared return type
//! (cast at its return), a cast (`x as T`), arithmetic on determined
//! operands, a struct/array/tuple/`Option`/`Result` whose parts are, a field
//! or element of a determined value. It is NOT determined when it came from
//! an untyped read (`dict_get`, `dict_values`, `recv`, …), an unannotated
//! lambda parameter, a type-parameter position, or a name that is ever
//! assigned such a value.
//!
//! The analysis is static and name-keyed per function (a name is determined
//! only if EVERY binding of it, and every assignment to it, is — a greatest
//! fixpoint), so it over-refuses rather than under-refuses. Its result is a
//! set of call-site keys (a structural hash of receiver and method — closure
//! bodies are cloned when a lambda is evaluated, so an address would not
//! survive), consulted at dispatch.

use crate::ast::{AxonType as T, BinOp, Expr, FnDef, Item, Pattern, Program, UnaryOp};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// The key of one method-call site.
pub(crate) fn call_key(receiver: &Expr, method: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{receiver:?}").hash(&mut h);
    method.hash(&mut h);
    h.finish()
}

/// The key of one arithmetic site.
pub(crate) fn binop_key(op: &BinOp, left: &Expr, right: &Expr) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{op:?}|{left:?}|{right:?}").hash(&mut h);
    h.finish()
}

#[derive(Default)]
pub(crate) struct Pins {
    /// Keys of operator method-call sites whose receiver is NOT determined.
    unpinned: HashSet<u64>,
    /// Method name -> how many distinct operator impl types define it. With
    /// fewer than two there is no impl to select between.
    impls: HashMap<String, HashSet<String>>,
}

/// Whether `t` names no type parameter, no trait object and no unstated part.
fn closed(t: &T, gp: &[String]) -> bool {
    match t {
        T::Named(n) => n != "?" && !gp.contains(n),
        T::TypeParam(_) | T::DynTrait(_) => false,
        T::Result { ok, err } => closed(ok, gp) && closed(err, gp),
        T::Option(x) | T::Chan(x) | T::Slice(x) | T::Ref(x) | T::RawPtr(x) => closed(x, gp),
        T::Generic { base, args } => !gp.contains(base) && args.iter().all(|a| closed(a, gp)),
        T::Fn { params, ret } => params.iter().all(|a| closed(a, gp)) && closed(ret, gp),
        T::Tuple(xs) | T::Union(xs) => xs.iter().all(|a| closed(a, gp)),
    }
}

/// A builtin's declared return type names a single-letter type variable
/// (`T`, `U`, `V`): its element comes from the arguments, untyped.
fn builtin_ret_open(ret: &str) -> bool {
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

enum Fact<'a> {
    Pinned,
    Unpinned,
    /// Determined iff this expression is.
    From(&'a Expr),
}

struct Ctx<'a> {
    /// Fn name -> declared return type closed (`None` = no such fn).
    ret_closed: HashMap<&'a str, bool>,
    /// Method name -> every fn of that name declares a closed return type.
    method_closed: HashMap<&'a str, bool>,
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
                    if let Some(c) = self.ret_closed.get(name.as_str()) {
                        *c
                    } else if let Some(r) = self.builtin_ret.get(name.as_str()) {
                        !builtin_ret_open(r)
                    } else {
                        false
                    }
                }
                _ => false,
            },
            Expr::MethodCall { method, .. } => self
                .method_closed
                .get(method.as_str())
                .copied()
                .unwrap_or(false),
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
    pub(crate) fn build(prog: &Program, skip: &dyn Fn(&FnDef) -> bool) -> Pins {
        let mut impls: HashMap<String, HashSet<String>> = HashMap::new();
        let mut ret_closed: HashMap<&str, bool> = HashMap::new();
        let mut method_closed: HashMap<&str, bool> = HashMap::new();
        let mut fns: Vec<(&FnDef, Vec<String>)> = Vec::new();
        let mut lets: Vec<(&str, &Expr)> = Vec::new();
        for item in &prog.items {
            match item {
                Item::FnDef(f) => {
                    let c = f
                        .return_type
                        .as_ref()
                        .is_some_and(|t| closed(t, &f.generic_params));
                    ret_closed
                        .entry(&f.name)
                        .and_modify(|x| *x &= c)
                        .or_insert(c);
                    fns.push((f, f.generic_params.clone()));
                }
                Item::ImplBlock(b) => {
                    let mut gp = b.generic_params.clone();
                    for m in &b.methods {
                        gp.extend(m.generic_params.clone());
                        let c = m.return_type.as_ref().is_some_and(|t| closed(t, &gp));
                        method_closed
                            .entry(&m.name)
                            .and_modify(|x| *x &= c)
                            .or_insert(c);
                        if !skip(m) {
                            impls
                                .entry(m.name.clone())
                                .or_default()
                                .insert(crate::doc::render_type(&b.for_type));
                        }
                        fns.push((m, gp.clone()));
                    }
                }
                Item::LetDef { name, value, .. } => lets.push((name, value)),
                _ => {}
            }
        }
        // Free fns can also be called as methods only through impls, but a
        // `Type::f(x)` path call names the fn: treat it as a method too.
        for (name, c) in &ret_closed {
            method_closed
                .entry(name)
                .and_modify(|x| *x &= *c)
                .or_insert(*c);
        }
        let mut ctx = Ctx {
            ret_closed,
            method_closed,
            builtin_ret: crate::builtins::BUILTINS
                .iter()
                .map(|b| (b.name, b.ret))
                .collect(),
            globals: HashSet::new(),
        };
        // Module-level lets: a greatest fixpoint over their initializers.
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
        let mut unpinned = HashSet::new();
        for (f, gp) in &fns {
            if skip(f) {
                continue;
            }
            analyze(f, gp, &ctx, &mut unpinned);
        }
        Pins { unpinned, impls }
    }

    /// Whether the method-call site is one whose receiver is not determined.
    pub(crate) fn undetermined(&self, receiver: &Expr, method: &str) -> bool {
        self.unpinned.contains(&call_key(receiver, method))
    }

    /// Whether `method` can select between operator impls (two or more types).
    /// Whether the arithmetic site has an operand that is not determined.
    pub(crate) fn undetermined_arith(&self, op: &BinOp, left: &Expr, right: &Expr) -> bool {
        self.unpinned.contains(&binop_key(op, left, right))
    }

    pub(crate) fn selects_between_impls(&self, method: &str) -> bool {
        self.impls.get(method).is_some_and(|s| s.len() >= 2)
    }
}

fn analyze(f: &FnDef, gp: &[String], ctx: &Ctx, out: &mut HashSet<u64>) {
    let mut facts: Vec<(String, Fact)> = Vec::new();
    for p in &f.params {
        facts.push((
            p.name.clone(),
            if closed(&p.ty, gp) {
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
        facts: &mut Vec<(String, Fact<'a>)>,
        calls: &mut Vec<&'a Expr>,
    ) {
        let mut visit = |x: &'a Expr| match x {
            Expr::Let { name, ty, value }
            | Expr::Own { name, ty, value }
            | Expr::RefBind { name, ty, value } => facts.push((
                name.clone(),
                match ty {
                    Some(t) if closed(t, gp) => Fact::Pinned,
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
                            Some(t) if closed(t, gp) => Fact::Pinned,
                            _ => Fact::Unpinned,
                        },
                    ));
                }
            }
            Expr::MethodCall { .. } | Expr::BinOp { .. } => calls.push(x),
            _ => {}
        };
        crate::ast::walk_expr(e, &mut visit);
    }
    collect(&f.body, gp, &mut facts, &mut calls);
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
                if !ctx.det(receiver, &local, &bound) {
                    out.insert(call_key(receiver, method));
                }
            }
            // Arithmetic whose operand's WIDTH nothing determined: a `u8`
            // wraps where an `i64` does not.
            Expr::BinOp { op, left, right }
                if !ctx.det(left, &local, &bound) || !ctx.det(right, &local, &bound) =>
            {
                out.insert(binop_key(op, left, right));
            }
            _ => {}
        }
    }
}
