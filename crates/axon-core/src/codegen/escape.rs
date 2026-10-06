//! Escape analysis for native array-literal storage (AX-12).
//!
//! An array literal used to `malloc` its buffer on every evaluation and nothing
//! ever freed it, so `let v = [i, i + 1]` in a hot loop grew RSS without bound.
//! This pass finds the literal sites whose buffer provably never outlives the
//! enclosing frame and can never be observed after the same site is evaluated
//! again; `emit_array_lit` backs exactly those with a fixed entry-block stack
//! slot (reused by every evaluation of that site) instead of the heap. Every
//! other literal keeps the heap buffer. The rules and the soundness argument
//! are in `spec/runtime.md` §3 "Array literal storage"; in short:
//!
//! * A name is LOCAL when every occurrence of it is an index read (`n[i]`), a
//!   place-write root (`n[i] = x`), a direct argument of a callee parameter
//!   that does not retain its argument, or the initializer of another binding
//!   (`let m = n`, which makes `n` escape iff `m` does). Any other occurrence -
//!   returned, a block's value, stored into an array/struct/tuple/`Some`/`Ok`,
//!   assigned to an existing variable, matched on, an operand, a method
//!   receiver, or ANY occurrence inside a lambda, `spawn`, `select`,
//!   `comptime` or a handler arm - makes the name escape.
//! * A callee parameter does not retain its argument when it is a whitelisted
//!   read-only builtin's array parameter, or a user fn parameter whose name is
//!   LOCAL in the callee (least fixpoint over the call graph, so recursion is
//!   handled; calls through a locally bound name are never trusted).
//! * Stack-backed sites: `let n = [..]` with `n` LOCAL; `n = [..]` with `n`
//!   LOCAL, bound in this fn, and never used as another binding's
//!   initializer; `[..]` passed directly to a non-retaining parameter; `[..][i]`.
//!
//! Sites are identified by the address of the literal's element vector, which
//! is stable for the lifetime of the `ast::FnDef` being emitted. A literal that
//! codegen re-synthesises (clones) is simply not found and stays on the heap.

use std::collections::{HashMap, HashSet};

use inkwell::types::BasicTypeEnum;

use crate::ast::{self, Expr, HandlerExpr, Pattern, Stmt};

/// Largest literal, by a conservative byte estimate, that gets a stack slot.
/// Every slot is a fixed part of its function's frame (once per recursion
/// level), so a big table keeps the heap buffer instead.
const STACK_SLOT_MAX_BYTES: u64 = 1024;

/// Does an `n`-element literal of `elem` fit `STACK_SLOT_MAX_BYTES`? The
/// estimate over-approximates the ABI size (every scalar rounded up to 8
/// bytes, 8 bytes of padding slack per struct field), so it never admits a
/// slot larger than the limit.
pub(super) fn fits_stack_slot(elem: BasicTypeEnum<'_>, n: u32) -> bool {
    fn upper(t: BasicTypeEnum<'_>) -> u64 {
        match t {
            BasicTypeEnum::IntType(i) => u64::from(i.get_bit_width()).div_ceil(64) * 8,
            BasicTypeEnum::FloatType(f) => {
                let ctx = f.get_context();
                if f == ctx.f64_type() || f == ctx.f32_type() || f == ctx.f16_type() {
                    8
                } else {
                    16
                }
            }
            BasicTypeEnum::PointerType(_) => 8,
            BasicTypeEnum::StructType(s) => s
                .get_field_types()
                .into_iter()
                .map(|f| upper(f).saturating_add(8))
                .fold(0, u64::saturating_add),
            BasicTypeEnum::ArrayType(a) => u64::from(a.len()).saturating_mul(upper(a.get_element_type())),
            BasicTypeEnum::VectorType(v) => u64::from(v.get_size()).saturating_mul(16),
        }
    }
    u64::from(n).saturating_mul(upper(elem)) <= STACK_SLOT_MAX_BYTES
}

/// Builtins that only READ their array argument while the call runs and
/// return a scalar (or `Option` of one): every native lowering is an inline
/// loop over the buffer and none has a runtime extern that could keep the
/// pointer. Anything not listed is assumed to retain its arguments.
const READ_ONLY_ARRAY_BUILTINS: &[&str] = &[
    "len",
    "arr_sum_i64",
    "arr_sum_f64",
    "arr_mean_i64",
    "arr_mean_f64",
    "arr_std_f64",
    "arr_max_i64",
    "arr_min_i64",
    "arr_max_f64",
    "arr_min_f64",
    "arr_argmax_i64",
    "arr_argmin_i64",
    "arr_argmax_f64",
    "arr_argmin_f64",
    "arr_contains",
    "arr_index_of",
    "arr_count_if",
    "arr_all",
    "arr_any",
];

/// Under which condition a literal site may live in a stack slot.
enum SiteCond {
    /// `let n = [..]` (also `own` / `ref`): `n` must not escape.
    Bound(String),
    /// `n = [..]`: `n` must not escape, must be bound in this fn, and must
    /// never initialise another binding (an alias created before the next
    /// evaluation of this site would otherwise observe the overwrite).
    Reassigned(String),
    /// Argument `.1` of user fn `.0`: that parameter must not retain.
    Arg(String, usize),
    /// Consumed on the spot (`[..][i]`, read-only builtin argument).
    Always,
}

/// Flow facts gathered from one function body.
#[derive(Default)]
struct BodyFacts {
    /// Names with at least one occurrence outside a LOCAL position.
    escaping: HashSet<String>,
    /// `(src, dst)` for `let dst = src`: src escapes iff dst does.
    aliases: Vec<(String, String)>,
    /// `(name, callee, param index)` for `callee(.., name, ..)`.
    passed: Vec<(String, String, usize)>,
    sites: Vec<(usize, SiteCond)>,
}

/// Every name bound anywhere in a function (params, `let`/`own`/`ref`, `for`
/// variables, lambda params, pattern binders). A call through one of these
/// names may be a closure, so its parameter summary is never trusted.
fn collect_binders(f: &ast::FnDef) -> HashSet<String> {
    fn pat(p: &Pattern, out: &mut HashSet<String>) {
        match p {
            Pattern::Ident(n) => {
                out.insert(n.clone());
            }
            Pattern::Some(q) | Pattern::Ok(q) | Pattern::Err(q) => pat(q, out),
            Pattern::Struct { fields, .. } => fields.iter().for_each(|(_, q)| pat(q, out)),
            Pattern::Tuple(qs) => qs.iter().for_each(|q| pat(q, out)),
            Pattern::Wildcard | Pattern::Literal(_) | Pattern::None => {}
        }
    }
    let mut out: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
    ast::walk_expr(&f.body, &mut |e| match e {
        Expr::Let { name, .. } | Expr::Own { name, .. } | Expr::RefBind { name, .. } => {
            out.insert(name.clone());
        }
        Expr::For { var, .. } => {
            out.insert(var.clone());
        }
        Expr::Lambda { params, .. } => out.extend(params.iter().map(|p| p.name.clone())),
        Expr::WhileLet { pattern, .. } => pat(pattern, &mut out),
        Expr::Match { arms, .. } => arms.iter().for_each(|a| pat(&a.pattern, &mut out)),
        Expr::WithHandler { handler, .. } => {
            if let HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                arms.iter().for_each(|a| pat(&a.binding, &mut out));
                if let Some(ra) = return_arm {
                    pat(&ra.binding, &mut out);
                }
            }
        }
        _ => {}
    });
    out
}

/// `&e` is a no-op on the native value, so look through it.
fn strip_ref(mut e: &Expr) -> &Expr {
    while let Expr::UnaryOp {
        op: ast::UnaryOp::Ref,
        operand,
    } = e
    {
        e = operand;
    }
    e
}

struct Scanner<'a> {
    /// Param names of every user fn, keyed by the name call sites use.
    params: &'a HashMap<String, Vec<String>>,
    binders: HashSet<String>,
    facts: BodyFacts,
}

impl Scanner<'_> {
    fn site(&mut self, elems: &[Expr], cond: SiteCond, opaque: bool) {
        if !opaque && !elems.is_empty() {
            self.facts.sites.push((elems.as_ptr() as usize, cond));
        }
        self.exprs(elems, opaque);
    }

    fn exprs(&mut self, es: &[Expr], opaque: bool) {
        for e in es {
            self.expr(e, opaque);
        }
    }

    fn stmts(&mut self, ss: &[Stmt], opaque: bool) {
        for s in ss {
            self.expr(&s.expr, opaque);
        }
    }

    fn escape(&mut self, n: &str) {
        self.facts.escaping.insert(n.to_string());
    }

    /// The place of `place = value`: its root is written in place, never copied
    /// out, so the root is a LOCAL position; index operands are ordinary reads.
    fn place(&mut self, place: &Expr, opaque: bool) {
        match place {
            Expr::Index { receiver, index } => {
                self.expr(index, opaque);
                self.place(receiver, opaque);
            }
            Expr::FieldAccess { receiver, .. } => self.place(receiver, opaque),
            Expr::Ident(n) if opaque => self.escape(n),
            Expr::Ident(_) => {}
            other => self.expr(other, opaque),
        }
    }

    /// `opaque`: inside a lambda / spawn / select / comptime / handler arm -
    /// code that may run in another frame or thread, or be emitted more than
    /// once. Every name there escapes and no literal there is a site.
    ///
    /// Exhaustive on purpose (no `_` arm), like `ast::walk_expr`: a new `Expr`
    /// variant must be classified here or this stops compiling.
    fn expr(&mut self, e: &Expr, opaque: bool) {
        match e {
            Expr::Ident(n) => self.escape(n),
            Expr::Index { receiver, index } => {
                match strip_ref(receiver) {
                    Expr::Ident(_) if !opaque => {}
                    Expr::Array(elems) => self.site(elems, SiteCond::Always, opaque),
                    _ => self.expr(receiver, opaque),
                }
                self.expr(index, opaque);
            }
            Expr::Let { name, value, .. }
            | Expr::Own { name, value, .. }
            | Expr::RefBind { name, value, .. } => match strip_ref(value) {
                Expr::Ident(src) if !opaque => {
                    self.facts.aliases.push((src.clone(), name.clone()));
                }
                Expr::Array(elems) => self.site(elems, SiteCond::Bound(name.clone()), opaque),
                _ => self.expr(value, opaque),
            },
            Expr::Assign { name, value } => match strip_ref(value) {
                Expr::Array(elems) => {
                    self.site(elems, SiteCond::Reassigned(name.clone()), opaque)
                }
                _ => self.expr(value, opaque),
            },
            Expr::AssignTo { place, value } => {
                self.place(place, opaque);
                self.expr(value, opaque);
            }
            Expr::Call { callee, args, .. } => {
                let target = match callee.as_ref() {
                    Expr::Ident(f) if !opaque && !self.binders.contains(f) => Some(f.as_str()),
                    _ => None,
                };
                if target.is_none() {
                    self.expr(callee, opaque);
                }
                // A user fn named like a builtin: codegen may dispatch either
                // one (the inline builtin lowerings match on the name first),
                // so neither summary is trusted.
                let builtin_name =
                    target.is_some_and(|f| crate::builtins::BUILTINS.iter().any(|b| b.name == f));
                let user_params = target
                    .and_then(|f| self.params.get(f))
                    .filter(|_| !builtin_name);
                let read_only_builtin = target.is_some_and(|f| {
                    READ_ONLY_ARRAY_BUILTINS.contains(&f) && !self.params.contains_key(f)
                });
                let arity_ok = user_params.is_some_and(|ps| ps.len() == args.len());
                for (i, a) in args.iter().enumerate() {
                    let a_core = strip_ref(a);
                    match (target, a_core) {
                        (Some(f), Expr::Ident(n)) if arity_ok => {
                            self.facts.passed.push((n.clone(), f.to_string(), i));
                        }
                        (Some(_), Expr::Ident(_)) if read_only_builtin => {}
                        (Some(f), Expr::Array(elems)) if arity_ok => {
                            self.site(elems, SiteCond::Arg(f.to_string(), i), opaque)
                        }
                        (Some(_), Expr::Array(elems)) if read_only_builtin => {
                            self.site(elems, SiteCond::Always, opaque)
                        }
                        _ => self.expr(a, opaque),
                    }
                }
            }
            Expr::Lambda { body, captures, .. } => {
                for (c, _) in captures {
                    self.escape(c);
                }
                self.expr(body, true);
            }
            Expr::Spawn(b) | Expr::Comptime(b) => self.expr(b, true),
            Expr::Select(arms) => {
                for a in arms {
                    self.expr(&a.recv, true);
                    self.expr(&a.body, true);
                }
            }
            Expr::WithHandler { handler, body } => {
                if let HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                    for a in arms {
                        self.expr(&a.body, true);
                    }
                    if let Some(ra) = return_arm {
                        self.expr(&ra.body, true);
                    }
                }
                self.expr(body, opaque);
            }
            Expr::Block(ss) => self.stmts(ss, opaque),
            Expr::MethodCall { receiver, args, .. } => {
                self.expr(receiver, opaque);
                self.exprs(args, opaque);
            }
            Expr::BinOp { left, right, .. } => {
                self.expr(left, opaque);
                self.expr(right, opaque);
            }
            Expr::UnaryOp { operand, .. } => self.expr(operand, opaque),
            Expr::Question(b) | Expr::Ok(b) | Expr::Err(b) | Expr::Some(b) => {
                self.expr(b, opaque)
            }
            Expr::Match { subject, arms } => {
                self.expr(subject, opaque);
                for a in arms {
                    if let Some(g) = &a.guard {
                        self.expr(g, opaque);
                    }
                    self.expr(&a.body, opaque);
                }
            }
            Expr::If { cond, then, else_ } => {
                self.expr(cond, opaque);
                self.expr(then, opaque);
                if let Some(b) = else_ {
                    self.expr(b, opaque);
                }
            }
            Expr::Return(inner) => {
                if let Some(b) = inner {
                    self.expr(b, opaque);
                }
            }
            Expr::FieldAccess { receiver, .. } => self.expr(receiver, opaque),
            Expr::Tuple(xs) | Expr::Array(xs) => self.exprs(xs, opaque),
            Expr::FmtStr { parts } => {
                for p in parts {
                    if let ast::FmtPart::Expr(inner) = p {
                        self.expr(inner, opaque);
                    }
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields {
                    self.expr(v, opaque);
                }
            }
            Expr::While { cond, body } => {
                self.expr(cond, opaque);
                self.stmts(body, opaque);
            }
            Expr::WhileLet { expr, body, .. } => {
                self.expr(expr, opaque);
                self.stmts(body, opaque);
            }
            Expr::For {
                start, end, body, ..
            } => {
                self.expr(start, opaque);
                self.expr(end, opaque);
                self.stmts(body, opaque);
            }
            Expr::Literal(_)
            | Expr::None
            | Expr::Break
            | Expr::Continue
            | Expr::InlineAsm { .. } => {}
        }
    }
}

/// Program-wide parameter summaries plus the per-fn site query.
#[derive(Default)]
pub(super) struct ArrayEscape {
    params: HashMap<String, Vec<String>>,
    /// `retains[f][i]`: user fn `f` may keep (or return an alias of) the
    /// buffer passed as argument `i`.
    retains: HashMap<String, Vec<bool>>,
}

impl ArrayEscape {
    /// Summarise every function the program emits (`(call-site name, def)`).
    pub(super) fn analyze(fns: &[(String, ast::FnDef)]) -> Self {
        let mut params: HashMap<String, Vec<String>> = HashMap::new();
        let mut ambiguous: HashSet<&str> = HashSet::new();
        for (n, f) in fns {
            let ps = f.params.iter().map(|p| p.name.clone()).collect();
            if params.insert(n.clone(), ps).is_some() {
                ambiguous.insert(n.as_str());
            }
        }
        // A name defined twice can't be resolved to one body here: assume
        // every parameter of it retains.
        let retains = params
            .iter()
            .map(|(n, ps)| (n.clone(), vec![ambiguous.contains(n.as_str()); ps.len()]))
            .collect();
        let mut this = Self { params, retains };
        let facts: Vec<(&str, BodyFacts)> = fns
            .iter()
            .filter(|(n, _)| !ambiguous.contains(n.as_str()))
            .map(|(n, f)| (n.as_str(), this.scan(f)))
            .collect();
        // Least fixpoint of "may retain": start from nothing retained and only
        // ever flip false -> true, so this terminates and recursion through a
        // param that is otherwise only read stays non-retaining.
        loop {
            let mut changed = false;
            for (name, body) in &facts {
                let esc = this.escaped(body);
                let flips: Vec<usize> = this.params[*name]
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| esc.contains(*p) && !this.retains[*name][*i])
                    .map(|(i, _)| i)
                    .collect();
                if let Some(r) = this.retains.get_mut(*name) {
                    for i in flips {
                        r[i] = true;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        this
    }

    fn scan(&self, f: &ast::FnDef) -> BodyFacts {
        let mut s = Scanner {
            params: &self.params,
            binders: collect_binders(f),
            facts: BodyFacts::default(),
        };
        s.expr(&f.body, false);
        s.facts
    }

    fn param_retains(&self, callee: &str, i: usize) -> bool {
        self.retains
            .get(callee)
            .and_then(|r| r.get(i).copied())
            .unwrap_or(true)
    }

    /// Close `escaping` under aliasing and retaining calls.
    fn escaped(&self, body: &BodyFacts) -> HashSet<String> {
        let mut esc = body.escaping.clone();
        loop {
            let mut changed = false;
            for (src, dst) in &body.aliases {
                if esc.contains(dst) && esc.insert(src.clone()) {
                    changed = true;
                }
            }
            for (n, callee, i) in &body.passed {
                if self.param_retains(callee, *i) && esc.insert(n.clone()) {
                    changed = true;
                }
            }
            if !changed {
                return esc;
            }
        }
    }

    /// Literal sites of `f` whose buffer may live in a per-site stack slot.
    pub(super) fn stack_sites(&self, f: &ast::FnDef) -> HashSet<usize> {
        let body = self.scan(f);
        let esc = self.escaped(&body);
        let binders = collect_binders(f);
        let aliased: HashSet<&str> = body.aliases.iter().map(|(s, _)| s.as_str()).collect();
        body.sites
            .iter()
            .filter(|(_, cond)| match cond {
                SiteCond::Bound(n) => !esc.contains(n),
                // Never a parameter: a by-reference parameter mode would store
                // the slot's address into the CALLER's variable.
                SiteCond::Reassigned(n) => {
                    !esc.contains(n)
                        && binders.contains(n)
                        && !f.params.iter().any(|p| &p.name == n)
                        && !aliased.contains(n.as_str())
                }
                SiteCond::Arg(callee, i) => !self.param_retains(callee, *i),
                SiteCond::Always => true,
            })
            .map(|(site, _)| *site)
            .collect()
    }
}
