//! `&mut [T]` parameter mode (AX-08): the static rules.
//!
//! A free function may declare an array parameter `xs: &mut [T]`; the caller
//! passes `&mut a` where `a` is a local variable, and every write the callee
//! makes to `xs` (`xs[i] = v`, `xs[i].f = v`, `xs = [..]`, passing it on as
//! `&mut xs`) is visible in `a` after the call. Both engines implement this
//! without copying: the interpreter moves the caller's value into the callee and
//! back out on return, native code passes the address of the caller's slot.
//!
//! Before this mode existed, writes through a shared `&[T]` parameter silently
//! mutated a callee-local copy. This pass makes that a compile error and
//! enforces the shapes both engines rely on:
//!
//! - **E0604** a write through a shared `&T` parameter (index / field
//!   assignment rooted at it, or `&mut` of it).
//! - **E0605** a malformed `&mut`: the operand is not a local variable (a
//!   field, an element, a global, a captured variable, a temporary); a `&mut`
//!   argument for a parameter that is not `&mut`, or a plain argument for one
//!   that is; `&mut` anywhere but a top-level `[T]` parameter of a free
//!   function; a `&mut`-taking function used as a value, spawned, or a `&mut`
//!   parameter written from inside a closure (the closure owns a copy).
//! - **E0606** aliasing within one call: the same variable borrowed `&mut`
//!   twice, or `&mut a` alongside any other argument that mentions `a`.

use std::collections::HashMap;

use crate::ast::{
    AxonType, Expr, FmtPart, FnDef, HandlerExpr, Item, Pattern, Program, Stmt, UnaryOp,
};
use crate::checker::CheckError;
use crate::error::{E0604, E0605, E0606};
use crate::span::Span;

/// How a local name was introduced; decides which writes are legal.
#[derive(Clone, Copy, PartialEq)]
enum Local {
    /// A `&T` parameter: read-only view of the caller's value.
    Shared,
    /// A `&mut [T]` parameter: writes reach the caller.
    MutParam,
    /// Any other local (`let`, by-value param, pattern binding, loop var).
    Plain,
}

/// Is this a `&mut [T]` parameter type (the only `&mut` form accepted)?
pub fn is_mut_slice_param(ty: &AxonType) -> bool {
    matches!(ty, AxonType::RefMut(inner) if matches!(inner.as_ref(), AxonType::Slice(_)))
}

/// Run the `&mut` rules over every function body in the program.
pub fn check_program(program: &Program) -> Vec<CheckError> {
    // Parameter modes of every free function, by name.
    let mut modes: HashMap<String, Vec<bool>> = HashMap::new();
    for item in &program.items {
        if let Item::FnDef(f) = item {
            modes.insert(
                f.name.clone(),
                f.params.iter().map(|p| matches!(p.ty, AxonType::RefMut(_))).collect(),
            );
        }
    }
    let mut w = Walker {
        modes: &modes,
        scopes: Vec::new(),
        closure_floor: 0,
        span: Span::dummy(),
        errors: Vec::new(),
    };
    for item in &program.items {
        match item {
            Item::FnDef(f) => w.check_fn(f, true),
            Item::ImplBlock(b) => {
                for m in &b.methods {
                    w.check_fn(m, false);
                }
            }
            Item::TypeDef(t) => {
                for fld in &t.fields {
                    w.no_ref_mut(&fld.ty, &format!("field `{}.{}`", t.name, fld.name), t.span);
                }
            }
            Item::EnumDef(e) => {
                for v in &e.variants {
                    for fld in &v.fields {
                        w.no_ref_mut(&fld.ty, &format!("variant `{}::{}`", e.name, v.name), e.span);
                    }
                }
            }
            Item::TraitDef(t) => {
                for m in &t.methods {
                    for p in &m.params {
                        w.no_ref_mut(&p.ty, &format!("trait method `{}`", m.name), m.span);
                    }
                    if let Some(r) = &m.return_type {
                        w.no_ref_mut(r, &format!("trait method `{}`", m.name), m.span);
                    }
                }
            }
            Item::LetDef { value, span, .. } => {
                w.span = *span;
                w.visit(value);
            }
            Item::RefineDef(r) => w.no_ref_mut(&r.base, &format!("refinement `{}`", r.name), r.span),
            Item::ModDecl(_) | Item::UseDecl(_) => {}
        }
    }
    w.errors
}

struct Walker<'a> {
    modes: &'a HashMap<String, Vec<bool>>,
    scopes: Vec<HashMap<String, Local>>,
    /// Scopes below this index belong to an enclosing fn body as seen from
    /// inside a closure: names there are captured copies, not places.
    closure_floor: usize,
    /// Span of the statement being checked (expressions carry none).
    span: Span,
    errors: Vec<CheckError>,
}

/// Root identifier of a place chain (`a`, `a.f`, `a[i].f[j]`).
fn place_root(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n) => Some(n.as_str()),
        Expr::FieldAccess { receiver, .. } | Expr::Index { receiver, .. } => place_root(receiver),
        _ => None,
    }
}

fn pattern_names<'p>(p: &'p Pattern, out: &mut Vec<&'p str>) {
    match p {
        Pattern::Ident(n) => out.push(n),
        Pattern::Some(i) | Pattern::Ok(i) | Pattern::Err(i) => pattern_names(i, out),
        Pattern::Struct { fields, .. } => {
            for (_, fp) in fields {
                pattern_names(fp, out);
            }
        }
        Pattern::Tuple(ps) => {
            for q in ps {
                pattern_names(q, out);
            }
        }
        Pattern::Wildcard | Pattern::Literal(_) | Pattern::None => {}
    }
}

fn contains_ref_mut(ty: &AxonType) -> bool {
    match ty {
        AxonType::RefMut(_) => true,
        AxonType::Ref(i)
        | AxonType::Slice(i)
        | AxonType::Option(i)
        | AxonType::Chan(i)
        | AxonType::RawPtr(i) => contains_ref_mut(i),
        AxonType::Result { ok, err } => contains_ref_mut(ok) || contains_ref_mut(err),
        AxonType::Generic { args, .. } => args.iter().any(contains_ref_mut),
        AxonType::Fn { params, ret } => params.iter().any(contains_ref_mut) || contains_ref_mut(ret),
        AxonType::Tuple(es) => es.iter().any(contains_ref_mut),
        _ => false,
    }
}

impl Walker<'_> {
    fn err(&mut self, code: &'static str, msg: String, help: &str) {
        self.errors
            .push(CheckError::new(code, msg).fix(help).with_span(self.span));
    }

    fn no_ref_mut(&mut self, ty: &AxonType, what: &str, span: Span) {
        if contains_ref_mut(ty) {
            self.span = span;
            self.err(
                E0605,
                format!("`&mut` is not allowed in the type of {what}"),
                "`&mut [T]` is only a parameter mode of a free function: \
                 `fn f(xs: &mut [T])`, called as `f(&mut a)`",
            );
        }
    }

    fn lookup(&self, name: &str) -> Option<(Local, bool)> {
        for (i, s) in self.scopes.iter().enumerate().rev() {
            if let Some(k) = s.get(name) {
                return Some((*k, i < self.closure_floor));
            }
        }
        None
    }

    fn define(&mut self, name: &str, kind: Local) {
        if let Some(s) = self.scopes.last_mut() {
            s.insert(name.to_string(), kind);
        }
    }

    /// A name that refers to a free function taking `&mut` params (not
    /// shadowed by a local).
    fn mut_fn(&self, name: &str) -> Option<&Vec<bool>> {
        if self.lookup(name).is_some() {
            return None;
        }
        self.modes.get(name).filter(|m| m.iter().any(|b| *b))
    }

    fn check_fn(&mut self, f: &FnDef, free: bool) {
        self.span = f.span;
        self.scopes.clear();
        self.closure_floor = 0;
        self.scopes.push(HashMap::new());
        for p in &f.params {
            let kind = match &p.ty {
                AxonType::RefMut(inner) => {
                    self.span = p.span;
                    if !free {
                        self.err(
                            E0605,
                            format!(
                                "method `{}` declares `&mut` parameter `{}` — only free \
                                 functions take `&mut` parameters",
                                f.name, p.name
                            ),
                            "move the in-place work into a free function \
                             `fn f(xs: &mut [T])` and call it as `f(&mut a)`",
                        );
                    } else if !is_mut_slice_param(&p.ty) {
                        self.err(
                            E0605,
                            format!(
                                "parameter `{}` of `{}` is `&mut` of a non-array type — only \
                                 `&mut [T]` is supported",
                                p.name, f.name
                            ),
                            "pass the value in and return the updated value instead",
                        );
                    } else if contains_ref_mut(inner) {
                        self.no_ref_mut(inner, &format!("parameter `{}`", p.name), p.span);
                    }
                    Local::MutParam
                }
                AxonType::Ref(inner) => {
                    self.no_ref_mut(inner, &format!("parameter `{}`", p.name), p.span);
                    Local::Shared
                }
                other => {
                    self.no_ref_mut(other, &format!("parameter `{}`", p.name), p.span);
                    Local::Plain
                }
            };
            self.define(&p.name, kind);
        }
        if let Some(r) = &f.return_type {
            self.no_ref_mut(r, &format!("the return type of `{}`", f.name), f.span);
        }
        self.span = f.span;
        self.visit(&f.body);
        self.scopes.clear();
    }

    fn visit_stmts(&mut self, stmts: &[Stmt]) {
        self.scopes.push(HashMap::new());
        for s in stmts {
            if !s.span.is_dummy() {
                self.span = s.span;
            }
            self.visit(&s.expr);
        }
        self.scopes.pop();
    }

    /// Check a `&mut` operand: it must be a whole local variable that this
    /// frame owns. Returns the borrowed name when it is one.
    fn mut_place<'e>(&mut self, operand: &'e Expr) -> Option<&'e str> {
        let help_local = "bind the value to a local first (`let tmp = ...`), pass `&mut tmp`, \
                          then store it back";
        match operand {
            Expr::Ident(name) => match self.lookup(name) {
                Some((Local::Shared, _)) => {
                    self.err(
                        E0604,
                        format!("cannot pass `{name}` as `&mut` — it is a shared `&` parameter"),
                        "declare the parameter `&mut [T]` (and pass `&mut a` at its call sites) \
                         so the write reaches the caller",
                    );
                    None
                }
                Some((_, true)) => {
                    self.err(
                        E0605,
                        format!(
                            "cannot borrow `{name}` as `&mut` inside a closure — the closure \
                             holds a copy of it"
                        ),
                        "call the `&mut` function outside the closure",
                    );
                    None
                }
                Some(_) => Some(name),
                None => {
                    self.err(
                        E0605,
                        format!("cannot borrow `{name}` as `&mut` — it is not a local variable"),
                        help_local,
                    );
                    None
                }
            },
            Expr::FieldAccess { .. } | Expr::Index { .. } => {
                self.err(
                    E0605,
                    "`&mut` of a field or element is not supported — only a whole local \
                     variable can be borrowed `&mut`"
                        .to_string(),
                    help_local,
                );
                self.visit(operand);
                None
            }
            _ => {
                self.err(
                    E0605,
                    "`&mut` needs a local variable — this expression is a temporary value"
                        .to_string(),
                    "bind it to a local first: `let tmp = ...` then pass `&mut tmp`",
                );
                self.visit(operand);
                None
            }
        }
    }

    fn visit_call(&mut self, callee: &Expr, args: &[Expr]) {
        let fn_name = match callee {
            Expr::Ident(n) if self.lookup(n).is_none() => Some(n.as_str()),
            _ => None,
        };
        let modes = fn_name.and_then(|n| self.modes.get(n)).cloned();
        if fn_name.is_none() {
            self.visit(callee);
        }
        let mut borrowed: Vec<(usize, &str)> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let param_mut = modes.as_ref().and_then(|m| m.get(i)).copied().unwrap_or(false);
            match a {
                Expr::UnaryOp {
                    op: UnaryOp::RefMut,
                    operand,
                } => {
                    if param_mut {
                        if let Some(name) = self.mut_place(operand) {
                            borrowed.push((i, name));
                        }
                    } else {
                        let what = match (fn_name, &modes) {
                            (Some(n), Some(_)) => format!(
                                "argument {} of `{n}` is passed `&mut`, but that parameter is \
                                 not declared `&mut`",
                                i + 1
                            ),
                            _ => "`&mut` can only be passed to a `&mut [T]` parameter of a \
                                  free function"
                                .to_string(),
                        };
                        self.err(
                            E0605,
                            what,
                            "pass the value as-is or as `&a`, or declare the parameter `&mut [T]`",
                        );
                        self.visit(operand);
                    }
                }
                _ => {
                    if param_mut {
                        self.err(
                            E0605,
                            format!(
                                "argument {} of `{}` must be passed as `&mut <local>` — the \
                                 parameter is `&mut`",
                                i + 1,
                                fn_name.unwrap_or("?")
                            ),
                            "write `&mut a` where `a` is the local variable to update in place",
                        );
                    }
                    self.visit(a);
                }
            }
        }
        // Aliasing: one `&mut` borrow per variable, and no other argument of
        // the same call may read it (the callee owns it for the call).
        for (k, (i, name)) in borrowed.iter().enumerate() {
            if borrowed[..k].iter().any(|(_, n)| n == name) {
                self.err(
                    E0606,
                    format!("`{name}` is borrowed `&mut` more than once in this call"),
                    "a call can borrow a variable `&mut` only once; copy it into a second \
                     local if two arrays are really meant",
                );
                continue;
            }
            let mentioned = args.iter().enumerate().any(|(j, a)| {
                if j == *i || matches!(a, Expr::UnaryOp { op: UnaryOp::RefMut, .. }) {
                    return false;
                }
                let mut hit = false;
                crate::ast::walk_expr(a, &mut |e| {
                    if matches!(e, Expr::Ident(n) if n == name) {
                        hit = true;
                    }
                });
                hit
            });
            if mentioned {
                self.err(
                    E0606,
                    format!(
                        "`{name}` is borrowed `&mut` and also used by another argument of the \
                         same call"
                    ),
                    "compute the other argument into a local before the call (e.g. \
                     `let n = len(a)`)",
                );
            }
        }
    }

    fn visit(&mut self, e: &Expr) {
        match e {
            Expr::Block(stmts) => self.visit_stmts(stmts),
            Expr::Let { name, ty, value }
            | Expr::Own { name, ty, value }
            | Expr::RefBind { name, ty, value } => {
                if let Some(t) = ty {
                    let span = self.span;
                    self.no_ref_mut(t, &format!("`{name}`"), span);
                }
                self.visit(value);
                self.define(name, Local::Plain);
            }
            Expr::Call { callee, args, .. } => self.visit_call(callee, args),
            Expr::MethodCall { receiver, args, .. } => {
                self.visit(receiver);
                for a in args {
                    if let Expr::UnaryOp {
                        op: UnaryOp::RefMut,
                        operand,
                    } = a
                    {
                        self.err(
                            E0605,
                            "`&mut` can only be passed to a `&mut [T]` parameter of a free \
                             function, not to a method"
                                .to_string(),
                            "call a free function `f(&mut a)` instead",
                        );
                        self.visit(operand);
                    } else {
                        self.visit(a);
                    }
                }
            }
            Expr::UnaryOp {
                op: UnaryOp::RefMut,
                operand,
            } => {
                self.err(
                    E0605,
                    "`&mut` is only allowed as an argument to a `&mut [T]` parameter".to_string(),
                    "write `f(&mut a)` where `f` declares `xs: &mut [T]`",
                );
                self.visit(operand);
            }
            Expr::UnaryOp { operand, .. } => self.visit(operand),
            Expr::BinOp { left, right, .. } => {
                self.visit(left);
                self.visit(right);
            }
            Expr::Question(b) | Expr::Comptime(b) | Expr::Ok(b) | Expr::Err(b) | Expr::Some(b) => {
                self.visit(b)
            }
            Expr::Spawn(b) => {
                if let Expr::Call { args, .. } = b.as_ref() {
                    if args
                        .iter()
                        .any(|a| matches!(a, Expr::UnaryOp { op: UnaryOp::RefMut, .. }))
                    {
                        self.err(
                            E0605,
                            "a spawned call cannot take a `&mut` argument".to_string(),
                            "pass the array by value to the spawned fn and send results back \
                             over a channel",
                        );
                    }
                }
                self.visit(b);
            }
            Expr::Return(inner) => {
                if let Some(b) = inner {
                    self.visit(b);
                }
            }
            Expr::Match { subject, arms } => {
                self.visit(subject);
                for arm in arms {
                    self.scopes.push(HashMap::new());
                    let mut names = Vec::new();
                    pattern_names(&arm.pattern, &mut names);
                    for n in names {
                        self.define(n, Local::Plain);
                    }
                    if let Some(g) = &arm.guard {
                        self.visit(g);
                    }
                    self.visit(&arm.body);
                    self.scopes.pop();
                }
            }
            Expr::If { cond, then, else_ } => {
                self.visit(cond);
                self.visit(then);
                if let Some(b) = else_ {
                    self.visit(b);
                }
            }
            Expr::Select(arms) => {
                for a in arms {
                    self.visit(&a.recv);
                    self.visit(&a.body);
                }
            }
            Expr::Lambda { params, body, .. } => {
                let saved_floor = self.closure_floor;
                self.closure_floor = self.scopes.len();
                self.scopes.push(HashMap::new());
                for p in params {
                    if let Some(t) = &p.ty {
                        let span = self.span;
                        self.no_ref_mut(t, &format!("closure parameter `{}`", p.name), span);
                    }
                    self.define(&p.name, Local::Plain);
                }
                self.visit(body);
                self.scopes.pop();
                self.closure_floor = saved_floor;
            }
            Expr::FieldAccess { receiver, .. } => self.visit(receiver),
            Expr::Index { receiver, index } => {
                self.visit(receiver);
                self.visit(index);
            }
            Expr::Tuple(xs) | Expr::Array(xs) => {
                for x in xs {
                    self.visit(x);
                }
            }
            Expr::Ident(name) => {
                if self.mut_fn(name).is_some() {
                    self.err(
                        E0605,
                        format!(
                            "function `{name}` takes `&mut` parameters and can only be called \
                             directly, not used as a value"
                        ),
                        "call it directly as `f(&mut a, ...)`, or wrap the call in a function \
                         without `&mut` parameters",
                    );
                }
            }
            Expr::FmtStr { parts } => {
                for p in parts {
                    if let FmtPart::Expr(inner) = p {
                        self.visit(inner);
                    }
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields {
                    self.visit(v);
                }
            }
            Expr::While { cond, body } => {
                self.visit(cond);
                self.visit_stmts(body);
            }
            Expr::WhileLet {
                pattern,
                expr,
                body,
            } => {
                self.visit(expr);
                self.scopes.push(HashMap::new());
                let mut names = Vec::new();
                pattern_names(pattern, &mut names);
                for n in names {
                    self.define(n, Local::Plain);
                }
                self.visit_stmts(body);
                self.scopes.pop();
            }
            Expr::For {
                var,
                start,
                end,
                body,
                ..
            } => {
                self.visit(start);
                self.visit(end);
                self.scopes.push(HashMap::new());
                self.define(var, Local::Plain);
                self.visit_stmts(body);
                self.scopes.pop();
            }
            Expr::Assign { name, value } => {
                if let Some((Local::MutParam, true)) = self.lookup(name) {
                    self.err(
                        E0605,
                        format!(
                            "cannot assign `&mut` parameter `{name}` inside a closure — the \
                             closure holds a copy, the caller would never see it"
                        ),
                        "do the write outside the closure",
                    );
                }
                self.visit(value);
            }
            Expr::AssignTo { place, value } => {
                if let Some(root) = place_root(place) {
                    match self.lookup(root) {
                        Some((Local::Shared, _)) => self.err(
                            E0604,
                            format!(
                                "cannot write through `{root}` — it is a shared `&` parameter, \
                                 so the write would only change a copy the caller never sees"
                            ),
                            "declare the parameter `&mut [T]` and pass `&mut a` at the call \
                             site; or, to modify a local copy on purpose, `let copy = xs` and \
                             write that",
                        ),
                        Some((Local::MutParam, true)) => self.err(
                            E0605,
                            format!(
                                "cannot write `&mut` parameter `{root}` inside a closure — the \
                                 closure holds a copy, the caller would never see it"
                            ),
                            "do the write outside the closure",
                        ),
                        _ => {}
                    }
                }
                self.visit(place);
                self.visit(value);
            }
            Expr::WithHandler { handler, body } => {
                if let HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                    for arm in arms.iter().chain(return_arm.as_deref()) {
                        self.scopes.push(HashMap::new());
                        let mut names = Vec::new();
                        pattern_names(&arm.binding, &mut names);
                        for n in names {
                            self.define(n, Local::Plain);
                        }
                        self.visit(&arm.body);
                        self.scopes.pop();
                    }
                }
                self.visit(body);
            }
            Expr::InlineAsm { .. }
            | Expr::Literal(_)
            | Expr::None
            | Expr::Break
            | Expr::Continue => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errs(src: &str) -> Vec<(String, String)> {
        let program = crate::parse_source(src).expect("parse");
        check_program(&program)
            .into_iter()
            .map(|e| (e.code.to_string(), e.message))
            .collect()
    }

    fn codes(src: &str) -> Vec<String> {
        errs(src).into_iter().map(|(c, _)| c).collect()
    }

    #[test]
    fn pattern_binding_shadows_shared_param() {
        let src = "fn f(xs: &[i64], o: Option<[i64]>) -> i64 {\n  match o {\n    \
                   Some(xs) => {\n      xs[0] = 1\n      xs[0]\n    }\n    None => xs[0]\n  }\n}\n";
        assert!(codes(src).is_empty(), "{:?}", errs(src));
    }

    #[test]
    fn write_through_shared_ref_is_e0604() {
        let src = "fn setfirst(xs: &[i64]) -> i64 {\n  xs[0] = 55\n  0\n}\n";
        assert_eq!(codes(src), vec!["E0604"]);
    }

    #[test]
    fn field_write_through_shared_ref_is_e0604() {
        let src = "type P = { v: i64 }\nfn f(ps: &[P]) {\n  ps[0].v = 1\n}\n";
        assert_eq!(codes(src), vec!["E0604"]);
    }

    #[test]
    fn shadowed_shared_param_is_a_plain_local() {
        let src = "fn f(xs: &[i64]) -> i64 {\n  let xs = [1, 2]\n  xs[0] = 5\n  xs[0]\n}\n";
        assert!(codes(src).is_empty());
    }

    #[test]
    fn mut_param_accepts_writes_and_reborrows() {
        let src = "fn g(xs: &mut [i64]) { xs[0] = 1 }\n\
                   fn f(xs: &mut [i64], ys: &[i64]) -> i64 {\n  xs[1] = ys[0]\n  g(&mut xs)\n  \
                   xs = [9]\n  len(xs)\n}\n\
                   fn main() {\n  let a = [1, 2]\n  let b = [3]\n  f(&mut a, &b)\n}\n";
        assert!(codes(src).is_empty(), "{:?}", errs(src));
    }

    #[test]
    fn reborrowing_a_shared_param_as_mut_is_e0604() {
        let src = "fn g(xs: &mut [i64]) { xs[0] = 1 }\nfn f(xs: &[i64]) { g(&mut xs) }\n";
        assert_eq!(codes(src), vec!["E0604"]);
    }

    #[test]
    fn mut_of_non_place_is_e0605() {
        let src = "type S = { xs: [i64] }\nfn g(xs: &mut [i64]) { xs[0] = 1 }\n\
                   fn main() {\n  let s = S { xs: [1] }\n  g(&mut [1, 2])\n  g(&mut s.xs)\n}\n";
        assert_eq!(codes(src), vec!["E0605", "E0605"]);
    }

    #[test]
    fn mode_mismatch_is_e0605() {
        let src = "fn g(xs: &mut [i64]) { xs[0] = 1 }\nfn h(xs: &[i64]) -> i64 { xs[0] }\n\
                   fn main() {\n  let a = [1]\n  g(a)\n  g(&a)\n  h(&mut a)\n  len(&mut a)\n}\n";
        assert_eq!(codes(src), vec!["E0605"; 4]);
    }

    #[test]
    fn aliasing_in_one_call_is_e0606() {
        let src = "fn two(a: &mut [i64], b: &mut [i64]) { a[0] = b[0] }\n\
                   fn mixed(a: &mut [i64], b: &[i64]) { a[0] = b[0] }\n\
                   fn idx(a: &mut [i64], i: i64) { a[i] = 0 }\n\
                   fn main() {\n  let x = [1]\n  two(&mut x, &mut x)\n  mixed(&mut x, &x)\n  \
                   idx(&mut x, len(x) - 1)\n}\n";
        assert_eq!(codes(src), vec!["E0606"; 3]);
    }

    #[test]
    fn mut_outside_param_position_is_e0605() {
        let src = "type S = { xs: &mut [i64] }\nfn f() -> &mut [i64] { [1] }\n\
                   fn g(x: &mut i64) { }\nfn main() {\n  let a = [1]\n  let r = &mut a\n}\n";
        assert_eq!(codes(src), vec!["E0605"; 4]);
    }

    #[test]
    fn mut_fn_as_value_and_closure_writes_are_e0605() {
        let src = "fn g(xs: &mut [i64]) { xs[0] = 1 }\n\
                   fn f(xs: &mut [i64]) {\n  let h = g\n  let k = |i| { xs[i] = 0 }\n  \
                   let m = |i| g(&mut xs)\n}\n";
        assert_eq!(codes(src), vec!["E0605"; 3]);
    }
}
