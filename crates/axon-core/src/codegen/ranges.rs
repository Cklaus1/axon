//! AX-77: which checked `+`/`-`/`*` in a function body provably cannot
//! overflow, from the value ranges of its locals.
//!
//! Each name bound in the body gets the interval hull of every value it can
//! hold: the hull of all `let`/assignment right-hand sides that bind or write
//! it, or `[lo, hi - 1]` for a `for` variable over `lo..hi`. A name the
//! analysis cannot see every write of is unbounded: parameters, pattern
//! bindings, a name borrowed (`&n`, `&mut n`), used as a method receiver or as
//! the root of a place assignment, a `for` variable the body assigns, and a
//! name that also names a module-level constant or function (a read before
//! the local binding sees that one). A body holding a closure, `spawn`,
//! `select`, an effect handler, `comptime`, `own`/`ref` bindings or inline asm
//! yields nothing at all.
//!
//! Intervals are computed in `i128` on the exact integer values: every
//! `+`/`-`/`*` on the way is checked (it faults rather than wrap), `x % k`
//! has `|x % k| < |k|` and the sign of `x`, `x / k` is monotone in `x`,
//! anything else is unbounded. The hulls are a fixpoint, reached by Kleene
//! iteration from the empty interval; a name still growing after
//! [`WIDEN_AFTER`] changes is widened to unbounded, so the iteration
//! terminates. A binding of any type joins the hull of its name: a union only
//! adds values, so every binding's values stay inside it, and the operands of
//! an i64 op are i64 expressions, which only i64 ops and i64 names build. An
//! op whose operand intervals keep the exact result inside i64 cannot
//! overflow; codegen lowers it to a plain `nsw` op instead of an overflow
//! intrinsic, a fault branch and a continuation block.

use std::collections::{HashMap, HashSet};

use crate::ast;

/// Changes of one name's hull before it is widened to unbounded.
const WIDEN_AFTER: u32 = 4;
/// Expression depth past which a sub-expression counts as unbounded, so a
/// deep generated expression cannot exhaust the stack here.
const MAX_DEPTH: u32 = 64;

const MIN: i128 = i64::MIN as i128;
const MAX: i128 = i64::MAX as i128;

/// The values an expression can take: none seen yet, an interval, or any.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Range {
    Empty,
    Iv(i128, i128),
    Top,
}

impl Range {
    fn hull(self, o: Range) -> Range {
        match (self, o) {
            (Range::Empty, x) | (x, Range::Empty) => x,
            (Range::Top, _) | (_, Range::Top) => Range::Top,
            (Range::Iv(a, b), Range::Iv(c, d)) => Range::Iv(a.min(c), b.max(d)),
        }
    }

    /// `Iv(lo, hi)` when it fits i64, else unbounded (the op faulted or the
    /// value is not an i64 at all).
    fn in_i64(lo: i128, hi: i128) -> Range {
        if lo >= MIN && hi <= MAX {
            Range::Iv(lo, hi)
        } else {
            Range::Top
        }
    }
}

/// How a name gets a value.
enum Def<'a> {
    Value(&'a ast::Expr),
    ForVar {
        start: &'a ast::Expr,
        end: &'a ast::Expr,
        inclusive: bool,
    },
}

struct Analysis<'a> {
    slots: HashMap<&'a str, usize>,
    env: Vec<Range>,
    top: Vec<bool>,
    defs: Vec<(usize, Def<'a>)>,
}

/// The `+`/`-`/`*` nodes of `body` whose result provably stays in i64.
/// `params` are the function's parameter names; `is_outer` says whether a
/// name also resolves outside the body (module constant, function).
pub(super) fn no_overflow_ops<'a>(
    params: impl IntoIterator<Item = &'a str>,
    body: &'a ast::Expr,
    is_outer: impl Fn(&str) -> bool,
) -> HashSet<*const ast::Expr> {
    let mut out = HashSet::new();
    let Some(mut a) = Analysis::collect(body) else {
        return out;
    };
    for p in params {
        a.force_top(p);
    }
    let outer: Vec<usize> = a
        .slots
        .iter()
        .filter(|(n, _)| is_outer(n))
        .map(|(_, &s)| s)
        .collect();
    for s in outer {
        a.top[s] = true;
    }
    a.solve();
    ast::walk_expr(body, &mut |e| {
        if let ast::Expr::BinOp {
            op: op @ (ast::BinOp::Add | ast::BinOp::Sub | ast::BinOp::Mul),
            left,
            right,
        } = e
        {
            let l = a.eval(left, 0);
            let r = a.eval(right, 0);
            if let (Range::Iv(..), Range::Iv(..), Range::Iv(..)) = (l, r, arith(op, l, r)) {
                out.insert(e as *const ast::Expr);
            }
        }
    });
    out
}

/// The exact result range of `l op r` for `+`/`-`/`*`.
fn arith(op: &ast::BinOp, l: Range, r: Range) -> Range {
    let (Range::Iv(a, b), Range::Iv(c, d)) = (l, r) else {
        return if l == Range::Empty || r == Range::Empty {
            Range::Empty
        } else {
            Range::Top
        };
    };
    match op {
        ast::BinOp::Add => Range::in_i64(a + c, b + d),
        ast::BinOp::Sub => Range::in_i64(a - d, b - c),
        _ => {
            // |operands| <= 2^63, so the products fit i128.
            let p = [a * c, a * d, b * c, b * d];
            Range::in_i64(*p.iter().min().unwrap(), *p.iter().max().unwrap())
        }
    }
}

impl<'a> Analysis<'a> {
    /// Gather every binding and write of `body`, or `None` when it holds a
    /// construct whose writes the analysis does not see.
    fn collect(body: &'a ast::Expr) -> Option<Self> {
        use ast::Expr as E;
        let mut a = Analysis {
            slots: HashMap::new(),
            env: Vec::new(),
            top: Vec::new(),
            defs: Vec::new(),
        };
        let mut opaque = false;
        let mut assigned: HashSet<&'a str> = HashSet::new();
        let mut for_vars: Vec<&'a str> = Vec::new();
        ast::walk_expr(body, &mut |e| match e {
            E::Lambda { .. }
            | E::Spawn(_)
            | E::Select(_)
            | E::WithHandler { .. }
            | E::Comptime(_)
            | E::Own { .. }
            | E::RefBind { .. }
            | E::InlineAsm { .. } => opaque = true,
            E::Let { name, value, .. } => {
                let s = a.slot(name);
                a.defs.push((s, Def::Value(value)));
            }
            E::Assign { name, value } => {
                let s = a.slot(name);
                a.defs.push((s, Def::Value(value)));
                assigned.insert(name);
            }
            E::For {
                var,
                start,
                end,
                inclusive,
                ..
            } => {
                let s = a.slot(var);
                a.defs.push((
                    s,
                    Def::ForVar {
                        start,
                        end,
                        inclusive: *inclusive,
                    },
                ));
                for_vars.push(var);
            }
            E::Match { arms, .. } => {
                for arm in arms {
                    a.pattern_top(&arm.pattern);
                }
            }
            E::WhileLet { pattern, .. } => a.pattern_top(pattern),
            E::UnaryOp {
                op: ast::UnaryOp::Ref | ast::UnaryOp::RefMut,
                operand,
            } => a.idents_top(operand),
            E::MethodCall { receiver, .. } => a.idents_top(receiver),
            E::AssignTo { place, .. } => a.idents_top(place),
            _ => {}
        });
        if opaque {
            return None;
        }
        // A `for` variable the body writes also takes those values and
        // their increments; the analysis does not follow that.
        for v in for_vars {
            if assigned.contains(v) {
                a.force_top(v);
            }
        }
        Some(a)
    }

    fn slot(&mut self, name: &'a str) -> usize {
        let next = self.env.len();
        let s = *self.slots.entry(name).or_insert(next);
        if s == next {
            self.env.push(Range::Empty);
            self.top.push(false);
        }
        s
    }

    fn force_top(&mut self, name: &'a str) {
        let s = self.slot(name);
        self.top[s] = true;
    }

    /// Every name a pattern binds (and every struct field name, in case a
    /// shorthand field binds it) is unbounded.
    fn pattern_top(&mut self, p: &'a ast::Pattern) {
        match p {
            ast::Pattern::Ident(n) => self.force_top(n),
            ast::Pattern::Some(p) | ast::Pattern::Ok(p) | ast::Pattern::Err(p) => {
                self.pattern_top(p)
            }
            ast::Pattern::Struct { fields, .. } => {
                for (f, p) in fields {
                    self.force_top(f);
                    self.pattern_top(p);
                }
            }
            ast::Pattern::Tuple(ps) => {
                for p in ps {
                    self.pattern_top(p);
                }
            }
            ast::Pattern::Wildcard | ast::Pattern::Literal(_) | ast::Pattern::None => {}
        }
    }

    /// Every name read inside `e` is unbounded.
    fn idents_top(&mut self, e: &'a ast::Expr) {
        let mut names = Vec::new();
        ast::walk_expr(e, &mut |x| {
            if let ast::Expr::Ident(n) = x {
                names.push(n.as_str());
            }
        });
        for n in names {
            self.force_top(n);
        }
    }

    /// Kleene iteration to the hulls' fixpoint, widening a name that keeps
    /// growing.
    fn solve(&mut self) {
        for (s, t) in self.top.iter().enumerate() {
            if *t {
                self.env[s] = Range::Top;
            }
        }
        let mut changes = vec![0u32; self.env.len()];
        loop {
            let mut changed = false;
            for i in 0..self.defs.len() {
                let s = self.defs[i].0;
                if self.env[s] == Range::Top {
                    continue;
                }
                let v = match self.defs[i].1 {
                    Def::Value(e) => self.eval(e, 0),
                    Def::ForVar {
                        start,
                        end,
                        inclusive,
                    } => self.for_var(start, end, inclusive),
                };
                let new = self.env[s].hull(v);
                if new != self.env[s] {
                    changes[s] += 1;
                    self.env[s] = if changes[s] > WIDEN_AFTER {
                        Range::Top
                    } else {
                        new
                    };
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// The values a `for` variable takes in the body: `start` up to the last
    /// value below (or, inclusive, at) `end`. An inclusive loop ending at
    /// `i64::MAX` wraps its counter, so it is unbounded.
    fn for_var(&self, start: &ast::Expr, end: &ast::Expr, inclusive: bool) -> Range {
        match (self.eval(start, 0), self.eval(end, 0)) {
            (Range::Empty, _) | (_, Range::Empty) => Range::Empty,
            (Range::Iv(lo, _), Range::Iv(_, hi)) => {
                let last = if inclusive {
                    if hi >= MAX {
                        return Range::Top;
                    }
                    hi
                } else {
                    hi - 1
                };
                Range::Iv(lo, last.max(lo))
            }
            _ => Range::Top,
        }
    }

    fn eval(&self, e: &ast::Expr, depth: u32) -> Range {
        use ast::Expr as E;
        if depth > MAX_DEPTH {
            return Range::Top;
        }
        let d = depth + 1;
        match e {
            E::Literal(ast::Literal::Int(k)) => Range::Iv(*k as i128, *k as i128),
            E::Ident(n) => match self.slots.get(n.as_str()) {
                Some(&s) => self.env[s],
                None => Range::Top,
            },
            E::BinOp { op, left, right } => match op {
                ast::BinOp::Add | ast::BinOp::Sub | ast::BinOp::Mul => {
                    arith(op, self.eval(left, d), self.eval(right, d))
                }
                ast::BinOp::Rem => rem(self.eval(left, d), self.eval(right, d)),
                ast::BinOp::Div => div(self.eval(left, d), self.eval(right, d)),
                _ => Range::Top,
            },
            E::UnaryOp {
                op: ast::UnaryOp::Neg,
                operand,
            } => match self.eval(operand, d) {
                // `-i64::MIN` wraps.
                Range::Iv(lo, hi) if lo > MIN => Range::Iv(-hi, -lo),
                Range::Empty => Range::Empty,
                _ => Range::Top,
            },
            E::Block(stmts) => match stmts.last() {
                Some(s) => self.eval(&s.expr, d),
                None => Range::Top,
            },
            E::If {
                then,
                else_: Some(else_),
                ..
            } => self.eval(then, d).hull(self.eval(else_, d)),
            _ => Range::Top,
        }
    }
}

/// `x % y`: `|x % y| < |y|` and `|x % y| <= |x|`, with the sign of `x`
/// (the fault on `y == 0` never yields a value).
fn rem(x: Range, y: Range) -> Range {
    if x == Range::Empty || y == Range::Empty {
        return Range::Empty;
    }
    let mag = |r: Range| match r {
        Range::Iv(lo, hi) => Some(lo.abs().max(hi.abs())),
        _ => None,
    };
    let m = match (mag(x), mag(y).map(|m| m - 1)) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => return Range::Top,
    };
    match x {
        Range::Iv(lo, _) if lo >= 0 => Range::Iv(0, m),
        Range::Iv(_, hi) if hi <= 0 => Range::Iv(-m, 0),
        _ => Range::Iv(-m, m),
    }
}

/// `x / k` for a constant `k`: truncating division is monotone in `x`
/// (increasing for `k > 0`, decreasing for `k < 0`).
fn div(x: Range, y: Range) -> Range {
    match (x, y) {
        (Range::Empty, _) | (_, Range::Empty) => Range::Empty,
        (Range::Iv(lo, hi), Range::Iv(k, k2)) if k == k2 && k != 0 => {
            let (a, b) = (lo / k, hi / k);
            Range::in_i64(a.min(b), a.max(b))
        }
        _ => Range::Top,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(src: &str) -> ast::Program {
        let tokens = crate::lexer::Lexer::tokenize(src)
            .expect("lexes")
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        crate::parser::Parser::new(tokens)
            .parse_program()
            .expect("parses")
    }

    /// The glyphs of the proven-safe ops of `fn f`, in walk order.
    fn proven(src: &str) -> Vec<String> {
        let prog = body(src);
        let f = prog
            .items
            .iter()
            .find_map(|i| match i {
                ast::Item::FnDef(f) if f.name == "f" => Some(f),
                _ => None,
            })
            .expect("fn f");
        let set = no_overflow_ops(f.params.iter().map(|p| p.name.as_str()), &f.body, |_| false);
        let mut out = Vec::new();
        ast::walk_expr(&f.body, &mut |e| {
            if let ast::Expr::BinOp { op, .. } = e {
                if set.contains(&(e as *const ast::Expr)) {
                    out.push(format!("{op:?}"));
                }
            }
        });
        out
    }

    #[test]
    fn remainder_bounds_the_next_ops() {
        // `a` is in (-1000003, 1000003); `a * 7 + 1` cannot overflow, the
        // ops on the unbounded parameter can.
        let src = "fn f(x: i64) -> i64 {\n    let a = (x * 3 + 1) % 1000003\n    a * 7 + 1\n}\n";
        assert_eq!(proven(src), vec!["Add", "Mul"]);
    }

    #[test]
    fn loop_variable_and_reassigned_locals_stay_bounded() {
        let src = "fn f(x: i64) -> i64 {\n    let c = x % 1000\n    for i in 0..7 {\n        c = (c * c + i) % 1000\n    }\n    c\n}\n";
        assert_eq!(proven(src), vec!["Add", "Mul"]);
    }

    #[test]
    fn a_growing_local_is_widened() {
        // `s` doubles every trip: no finite hull, so `s * 2` stays checked.
        let src = "fn f(n: i64) -> i64 {\n    let s = 1\n    for i in 0..n {\n        s = s * 2\n    }\n    s\n}\n";
        assert_eq!(proven(src), Vec::<String>::new());
    }

    #[test]
    fn borrowed_params_and_pattern_bindings_are_unbounded() {
        let src = "fn f(x: i64) -> i64 {\n    let a = 5\n    let r = match Some(x) {\n        Some(a) => a * 2,\n        None => 0,\n    }\n    r + a\n}\n";
        assert_eq!(proven(src), Vec::<String>::new());
    }

    #[test]
    fn a_closure_disables_the_analysis() {
        let src = "fn f(x: i64) -> i64 {\n    let a = 5\n    let g = || a + 1\n    a * 2\n}\n";
        assert_eq!(proven(src), Vec::<String>::new());
    }

    #[test]
    fn huge_products_stay_checked() {
        let src = "fn f(x: i64) -> i64 {\n    let a = x % 4000000000\n    a * a * 2\n}\n";
        // `a * a` < 1.6e19 does not fit i64 either.
        assert_eq!(proven(src), Vec::<String>::new());
    }
}
