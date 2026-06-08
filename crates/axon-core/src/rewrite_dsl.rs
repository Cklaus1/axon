//! Self-improving compiler, Layer 3 (prototype): AI-authored passes as DATA.
//!
//! See `spec/self-improving-layer3.md`. The central safety decision: an
//! AI-authored optimization pass is a **declarative `RewriteSpec` value**, never
//! Rust compiled into the TCB. A reviewed evaluator (this module) interprets the
//! spec; the AI never executes. The spec is:
//!   * **total** — a single bounded bottom-up pass, no recursion in rules;
//!   * **pure** — it may only rearrange/​delete bound subtrees and emit literals
//!     via a closed set of total, behavior-preserving builtins;
//!   * **capability-free by construction** — the grammar has no production for a
//!     capability call, so one cannot even be expressed (the firewall's G2 still
//!     checks dynamically as defense-in-depth).
//!
//! A validated spec compiles to a `Pass` (`Fn(&Program)->Program`) that is then
//! run through the EXISTING four-gate firewall (`improve::verify_pass`) and
//! multi-sig graduation — both unchanged. Layer 3 adds only the validate +
//! compile-to-pass steps; it never touches how a pass is *admitted*.
//!
//! This prototype ships the DSL MINIMAL (spec §F2): the rule kinds re-express the
//! four shipped registry passes as data. The DSL widens only as each new rule
//! kind is red-teamed.

use crate::ast::{BinOp, Expr, Item, Literal, Program, UnaryOp};

/// A proposal-stage (E15xx) validation error — fail-closed, raised BEFORE the
/// firewall runs. A malformed / non-total / impure / capability-expressing spec
/// is rejected here and never compiled to a runnable pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecError {
    pub code: &'static str,
    pub message: String,
}

// Proposal-stage codes live in the R10 self-improving band (error.rs is the
// single source of truth; the gate enforces registration). E15xx was already
// taken by the #[goal] feature, so L3 uses the free E14xx slots after E1408.
pub use crate::error::{
    E1409 as E_NONTOTAL, E1411 as E_BAD_RULE, E1412 as E_CAPABILITY, E1413 as E_BUDGET,
};

/// One rewrite rule. Each variant is a reviewed, TOTAL, behavior-preserving
/// transform shape — the closed vocabulary an AI proposer may compose. The AI's
/// freedom is in *which rules* (and their literal parameters) it picks; it cannot
/// author a new rule kind (that needs a reviewed commit extending this enum,
/// exactly the registry-extension boundary L2 already has).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewriteRule {
    /// Fold `<int-lit> OP <int-lit>` to the result literal, using checked
    /// arithmetic (never folds an overflow / div-or-rem by zero) — `constant-fold`.
    FoldIntLiteral,
    /// Collapse an arithmetic identity to its surviving operand — `x+0`/`0+x`/
    /// `x-0`/`x*1`/`1*x` → `x` (`fold-arith-identities`).
    FoldArithIdentity,
    /// Simplify boolean negation — `!true`→`false`, `!false`→`true`, `!(!x)`→`x`
    /// (`bool-simplify`).
    SimplifyBoolNot,
    /// Fold a constant-condition `if`/`else` to the taken branch
    /// (`redundant-branch-fold`).
    FoldConstBranch,
    /// Fold a logical `&&`/`||` with a constant operand — but ONLY the
    /// short-circuit-sound cases, where no EVALUATED operand is dropped:
    ///   `false && R → false`   (R is never evaluated in the original)
    ///   `true  && R → R`  ;  `L && true → L`
    ///   `true  || R → true`    (R is never evaluated)
    ///   `false || R → R`  ;  `L || false → L`
    /// CRITICALLY it does NOT fold `L && false → false` or `L || true → true`:
    /// the LEFT operand is always evaluated, so dropping it would erase L's side
    /// effects / panic (an unsoundness G1 would catch — proven by a red-team
    /// test). A genuinely NEW optimization (not a re-expression of a shipped
    /// pass), added under the "widen only as red-teamed" discipline.
    FoldLogicalShortCircuit,
}

impl RewriteRule {
    /// A stable name (for spec text / manifest / errors).
    pub fn name(&self) -> &'static str {
        match self {
            RewriteRule::FoldIntLiteral => "fold-int-literal",
            RewriteRule::FoldArithIdentity => "fold-arith-identity",
            RewriteRule::SimplifyBoolNot => "simplify-bool-not",
            RewriteRule::FoldConstBranch => "fold-const-branch",
            RewriteRule::FoldLogicalShortCircuit => "fold-logical",
        }
    }

    fn from_name(s: &str) -> Option<RewriteRule> {
        match s {
            "fold-int-literal" => Some(RewriteRule::FoldIntLiteral),
            "fold-arith-identity" => Some(RewriteRule::FoldArithIdentity),
            "simplify-bool-not" => Some(RewriteRule::SimplifyBoolNot),
            "fold-const-branch" => Some(RewriteRule::FoldConstBranch),
            "fold-logical" => Some(RewriteRule::FoldLogicalShortCircuit),
            _ => None,
        }
    }

    /// Apply this rule at a SINGLE node (already-folded children). Returns
    /// `Some(rewritten)` if the rule fired, `None` if it doesn't apply here.
    /// (`Expr` has no `PartialEq`, so a fired/not-fired Option drives the
    /// fixed-point loop instead of value comparison.) Every arm is total and
    /// behavior-preserving — the soundness lives here, reviewed.
    fn apply_here(&self, e: &Expr) -> Option<Expr> {
        match self {
            RewriteRule::FoldIntLiteral => {
                if let Expr::BinOp { op, left, right } = e {
                    if let (Expr::Literal(Literal::Int(a)), Expr::Literal(Literal::Int(b))) =
                        (left.as_ref(), right.as_ref())
                    {
                        if let Some(v) = checked_fold(op, *a, *b) {
                            return Some(Expr::Literal(Literal::Int(v)));
                        }
                    }
                }
                None
            }
            RewriteRule::FoldArithIdentity => {
                if let Expr::BinOp { op, left, right } = e {
                    let l = left.as_ref();
                    let r = right.as_ref();
                    match op {
                        BinOp::Add if is_int(l, 0) => return Some(r.clone()),
                        BinOp::Add if is_int(r, 0) => return Some(l.clone()),
                        BinOp::Sub if is_int(r, 0) => return Some(l.clone()),
                        BinOp::Mul if is_int(l, 1) => return Some(r.clone()),
                        BinOp::Mul if is_int(r, 1) => return Some(l.clone()),
                        _ => {}
                    }
                }
                None
            }
            RewriteRule::SimplifyBoolNot => {
                if let Expr::UnaryOp { op: UnaryOp::Not, operand } = e {
                    match operand.as_ref() {
                        Expr::Literal(Literal::Bool(b)) => {
                            return Some(Expr::Literal(Literal::Bool(!b)))
                        }
                        Expr::UnaryOp { op: UnaryOp::Not, operand: inner } => {
                            return Some((**inner).clone())
                        }
                        _ => {}
                    }
                }
                None
            }
            RewriteRule::FoldConstBranch => {
                if let Expr::If { cond, then, else_ } = e {
                    if let (Expr::Literal(Literal::Bool(b)), Some(eb)) = (cond.as_ref(), else_) {
                        return Some(if *b { (**then).clone() } else { (**eb).clone() });
                    }
                }
                None
            }
            RewriteRule::FoldLogicalShortCircuit => {
                if let Expr::BinOp { op, left, right } = e {
                    let l = left.as_ref();
                    let r = right.as_ref();
                    match op {
                        // `false && R → false` — R is never evaluated in the
                        // original (short-circuit), so dropping it is sound.
                        BinOp::And if is_bool(l, false) => {
                            return Some(Expr::Literal(Literal::Bool(false)))
                        }
                        // `true && R → R` ; `L && true → L` — the dropped operand
                        // is a literal (no side effect), the kept one is evaluated.
                        BinOp::And if is_bool(l, true) => return Some(r.clone()),
                        BinOp::And if is_bool(r, true) => return Some(l.clone()),
                        // `true || R → true` — R never evaluated (short-circuit).
                        BinOp::Or if is_bool(l, true) => {
                            return Some(Expr::Literal(Literal::Bool(true)))
                        }
                        // `false || R → R` ; `L || false → L`.
                        BinOp::Or if is_bool(l, false) => return Some(r.clone()),
                        BinOp::Or if is_bool(r, false) => return Some(l.clone()),
                        // NOTE: `L && false` and `L || true` are deliberately NOT
                        // folded — the LEFT operand is always evaluated, so dropping
                        // it would erase L's side effects (unsound; G1 would reject).
                        _ => {}
                    }
                }
                None
            }
        }
    }
}

fn is_int(e: &Expr, n: i64) -> bool {
    matches!(e, Expr::Literal(Literal::Int(v)) if *v == n)
}

fn is_bool(e: &Expr, b: bool) -> bool {
    matches!(e, Expr::Literal(Literal::Bool(v)) if *v == b)
}

/// Checked integer fold — mirrors `interp::value::eval_binop_vals`; `None` when
/// folding would erase a runtime panic (overflow / div-or-rem 0 / MIN/-1).
fn checked_fold(op: &BinOp, a: i64, b: i64) -> Option<i64> {
    match op {
        BinOp::Add => a.checked_add(b),
        BinOp::Sub => a.checked_sub(b),
        BinOp::Mul => a.checked_mul(b),
        BinOp::Div if b != 0 && !(a == i64::MIN && b == -1) => Some(a.wrapping_div(b)),
        BinOp::Rem if b != 0 && !(a == i64::MIN && b == -1) => Some(a.wrapping_rem(b)),
        _ => None,
    }
}

/// An AI-authored candidate pass, as DATA. A list of rule kinds applied bottom-up
/// in one bounded pass. Serializable to/from a simple line-based text form (one
/// rule name per line) so a proposer emits text, not code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteSpec {
    pub rules: Vec<RewriteRule>,
}

impl RewriteSpec {
    /// Parse a RewriteSpec from text: one rule name per non-empty, non-`#` line.
    /// An unknown rule name is E1411 (outside the closed reviewed vocabulary —
    /// the proposer cannot invent a rule kind). Fail-closed.
    pub fn parse(text: &str) -> Result<RewriteSpec, SpecError> {
        let mut rules = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            match RewriteRule::from_name(line) {
                Some(r) => rules.push(r),
                None => {
                    return Err(SpecError {
                        code: E_BAD_RULE,
                        message: format!(
                            "unknown rewrite rule `{line}` — not in the closed reviewed \
                             vocabulary {{fold-int-literal, fold-arith-identity, \
                             simplify-bool-not, fold-const-branch}}"
                        ),
                    });
                }
            }
        }
        Ok(RewriteSpec { rules })
    }

    /// Validate the spec (proposal-stage, E15xx, fail-closed). The rule kinds are
    /// each reviewed-total/pure/capability-free by construction, so validation
    /// reduces to: (a) every rule is in the closed set (enforced at parse), and
    /// (b) the spec is non-empty and within the rule-count budget (a runaway
    /// proposer can't submit a 10^6-rule spec to exhaust the evaluator). A future
    /// richer DSL (free patterns) would add totality/purity/cap-shape checks here.
    pub fn validate(&self) -> Result<(), SpecError> {
        const MAX_RULES: usize = 64;
        if self.rules.is_empty() {
            return Err(SpecError {
                code: E_NONTOTAL,
                message: "empty RewriteSpec proposes no transform".to_string(),
            });
        }
        if self.rules.len() > MAX_RULES {
            return Err(SpecError {
                code: E_BUDGET,
                message: format!(
                    "RewriteSpec has {} rules, over the budget of {MAX_RULES} \
                     (a runaway proposal is rejected, never run)",
                    self.rules.len()
                ),
            });
        }
        Ok(())
    }

    /// Capability-shape check (E1412): a defense-in-DESIGN backstop. The rule
    /// kinds can ONLY rearrange/delete subtrees or emit int/bool literals — none
    /// can synthesize a `Call`, so a capability is unrepresentable. This method
    /// makes that invariant explicit and testable; it can never fail for the
    /// current closed vocabulary (asserted by a test), and exists so that adding a
    /// future rule kind that *could* emit a call forces a conscious update here.
    pub fn cannot_express_capability(&self) -> bool {
        self.rules.iter().all(|r| {
            matches!(
                r,
                RewriteRule::FoldIntLiteral
                    | RewriteRule::FoldArithIdentity
                    | RewriteRule::SimplifyBoolNot
                    | RewriteRule::FoldConstBranch
                    | RewriteRule::FoldLogicalShortCircuit
            )
        })
    }
}

/// Compile a VALIDATED RewriteSpec into a runnable `Pass`. This is the reviewed
/// evaluator — the only thing that executes; the AI never does. The resulting
/// closure applies every rule bottom-up in one bounded pass per fn body.
/// (Caller must `validate()` first; this trusts that.)
pub fn compile(spec: &RewriteSpec) -> impl Fn(&Program) -> Program {
    let rules = spec.rules.clone();
    move |program: &Program| Program {
        items: program
            .items
            .iter()
            .map(|item| match item {
                Item::FnDef(f) => {
                    let mut nf = f.clone();
                    nf.body = rewrite_expr(&f.body, &rules);
                    Item::FnDef(nf)
                }
                Item::ImplBlock(b) => {
                    let mut nb = b.clone();
                    nb.methods = b
                        .methods
                        .iter()
                        .map(|m| {
                            let mut nm = m.clone();
                            nm.body = rewrite_expr(&m.body, &rules);
                            nm
                        })
                        .collect();
                    Item::ImplBlock(nb)
                }
                other => other.clone(),
            })
            .collect(),
    }
}

/// RED-TEAM ONLY (test builds): compile a deliberately UNSOUND transform that
/// folds `<int> / <int>` to a literal WITHOUT the zero/overflow guard — i.e. it
/// would fold `10 / 0` to `0`, erasing a runtime panic. This mimics what a buggy
/// or malicious *future* DSL rule kind could emit, so a test can prove the
/// firewall (`verify_pass` G1) REJECTS it. The point of Layer 3: the data path is
/// gated by the same interpreter oracle as everything else — the DSL's curated
/// soundness is defense-in-depth, NOT the guarantee. This is `#[cfg(test)]` so it
/// can never reach a shipping binary or the closed `RewriteRule` vocabulary.
#[cfg(test)]
pub fn compile_unsound_div_fold_for_redteam() -> impl Fn(&Program) -> Program {
    fn fold(e: &Expr) -> Expr {
        let folded = map_children(e, fold);
        if let Expr::BinOp { op: BinOp::Div, left, right } = &folded {
            if let (Expr::Literal(Literal::Int(a)), Expr::Literal(Literal::Int(b))) =
                (left.as_ref(), right.as_ref())
            {
                // UNSOUND: emit a literal for ANY int/int division, with NO zero
                // guard — so `10 / 0` (which must panic, exit 101) becomes the
                // literal `0`, erasing the panic. (We don't actually divide in
                // Rust — we emit a bogus literal — so the builder itself can't
                // panic; the unsoundness is in the WRONG AST it produces.)
                let bogus = if *b == 0 { 0 } else { a.wrapping_div(*b) };
                return Expr::Literal(Literal::Int(bogus));
            }
        }
        folded
    }
    |program: &Program| Program {
        items: program
            .items
            .iter()
            .map(|item| match item {
                Item::FnDef(f) => {
                    let mut nf = f.clone();
                    nf.body = fold(&f.body);
                    Item::FnDef(nf)
                }
                other => other.clone(),
            })
            .collect(),
    }
}

/// One bounded bottom-up rewrite: fold children, then apply every rule at this
/// node (in spec order) until a fixed point or the per-node fuel runs out.
/// Totality: children recursion is bounded by AST depth; the per-node loop is
/// bounded by `FUEL` (a rule set that thrashes can't hang).
fn rewrite_expr(e: &Expr, rules: &[RewriteRule]) -> Expr {
    const FUEL: usize = 32;
    let mut cur = map_children(e, |c| rewrite_expr(c, rules));
    let mut fuel = FUEL;
    loop {
        let mut changed = false;
        for rule in rules {
            if let Some(next) = rule.apply_here(&cur) {
                cur = next;
                changed = true;
            }
        }
        fuel -= 1;
        if !changed || fuel == 0 {
            break;
        }
    }
    cur
}

/// Rebuild `e` with each direct child replaced by `f(child)` (self-contained;
/// the rule kinds only need to descend the arithmetic/bool/if-bearing variants,
/// so unlisted variants are returned as-is — conservative, never a miscompile).
fn map_children(e: &Expr, f: impl Fn(&Expr) -> Expr) -> Expr {
    let fb = |b: &Expr| Box::new(f(b));
    match e {
        Expr::BinOp { op, left, right } => {
            Expr::BinOp { op: op.clone(), left: fb(left), right: fb(right) }
        }
        Expr::UnaryOp { op, operand } => Expr::UnaryOp { op: op.clone(), operand: fb(operand) },
        Expr::If { cond, then, else_ } => Expr::If {
            cond: fb(cond),
            then: fb(then),
            else_: else_.as_ref().map(|b| fb(b)),
        },
        Expr::Let { name, ty, value } => {
            Expr::Let { name: name.clone(), ty: ty.clone(), value: fb(value) }
        }
        Expr::Own { name, ty, value } => {
            Expr::Own { name: name.clone(), ty: ty.clone(), value: fb(value) }
        }
        Expr::RefBind { name, ty, value } => {
            Expr::RefBind { name: name.clone(), ty: ty.clone(), value: fb(value) }
        }
        Expr::Call { callee, args, tier } => Expr::Call {
            callee: fb(callee),
            args: args.iter().map(&f).collect(),
            tier: tier.clone(),
        },
        Expr::MethodCall { receiver, method, args } => Expr::MethodCall {
            receiver: fb(receiver),
            method: method.clone(),
            args: args.iter().map(&f).collect(),
        },
        Expr::Block(stmts) => Expr::Block(
            stmts
                .iter()
                .map(|s| {
                    let mut ns = s.clone();
                    ns.expr = f(&s.expr);
                    ns
                })
                .collect(),
        ),
        Expr::Match { subject, arms } => Expr::Match {
            subject: fb(subject),
            arms: arms
                .iter()
                .map(|a| {
                    let mut na = a.clone();
                    na.body = f(&a.body);
                    na
                })
                .collect(),
        },
        Expr::Return(Some(b)) => Expr::Return(Some(fb(b))),
        Expr::FieldAccess { receiver, field } => {
            Expr::FieldAccess { receiver: fb(receiver), field: field.clone() }
        }
        Expr::Index { receiver, index } => Expr::Index { receiver: fb(receiver), index: fb(index) },
        Expr::Tuple(xs) => Expr::Tuple(xs.iter().map(&f).collect()),
        Expr::Array(xs) => Expr::Array(xs.iter().map(&f).collect()),
        Expr::While { cond, body } => Expr::While {
            cond: fb(cond),
            body: body
                .iter()
                .map(|s| {
                    let mut ns = s.clone();
                    ns.expr = f(&s.expr);
                    ns
                })
                .collect(),
        },
        Expr::Ok(b) => Expr::Ok(fb(b)),
        Expr::Err(b) => Expr::Err(fb(b)),
        Expr::Some(b) => Expr::Some(fb(b)),
        Expr::StructLit { name, fields } => Expr::StructLit {
            name: name.clone(),
            fields: fields.iter().map(|(k, v)| (k.clone(), f(v))).collect(),
        },
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    fn prog(src: &str) -> Program {
        parse_source(src).expect("parse")
    }

    fn body_tail(p: &Program) -> Expr {
        match &p.items[0] {
            Item::FnDef(f) => match &f.body {
                Expr::Block(stmts) => stmts.last().unwrap().expr.clone(),
                other => other.clone(),
            },
            _ => panic!("expected fn"),
        }
    }

    #[test]
    fn parse_rejects_unknown_rule_e1504() {
        let err = RewriteSpec::parse("fold-int-literal\nexec-shell").unwrap_err();
        assert_eq!(err.code, E_BAD_RULE, "{}", err.message);
        assert!(err.message.contains("exec-shell"));
    }

    #[test]
    fn validate_rejects_empty_and_oversized() {
        assert_eq!(RewriteSpec { rules: vec![] }.validate().unwrap_err().code, E_NONTOTAL);
        let huge = RewriteSpec { rules: vec![RewriteRule::FoldIntLiteral; 65] };
        assert_eq!(huge.validate().unwrap_err().code, E_BUDGET);
    }

    #[test]
    fn closed_vocabulary_cannot_express_a_capability() {
        // Defense-in-design: NO rule kind can emit a Call, so a capability is
        // unrepresentable. (If a future rule could, this must be revisited.)
        let spec = RewriteSpec::parse(
            "fold-int-literal\nfold-arith-identity\nsimplify-bool-not\nfold-const-branch",
        )
        .unwrap();
        assert!(spec.cannot_express_capability());
    }

    #[test]
    fn compiled_constant_fold_spec_folds_arithmetic() {
        // The DSL can EXPRESS constant-fold as data, and the compiled pass folds.
        let spec = RewriteSpec::parse("fold-int-literal").unwrap();
        spec.validate().unwrap();
        let pass = compile(&spec);
        let out = pass(&prog("fn main() -> i64 { 2 + 3 * 4 }"));
        // 2 + 3*4 → 2 + 12 → 14 (bottom-up + fixed point).
        assert!(matches!(body_tail(&out), Expr::Literal(Literal::Int(14))));
    }

    #[test]
    fn compiled_spec_preserves_a_would_panic_division() {
        // The DSL evaluator must NOT fold `10 / 0` (erasing the panic) — same
        // soundness the Rust pass has, now proven through the data path.
        let spec = RewriteSpec::parse("fold-int-literal").unwrap();
        let pass = compile(&spec);
        let out = pass(&prog("fn main() -> i64 { 10 / 0 }"));
        // Still a division, not a literal.
        assert!(matches!(body_tail(&out), Expr::BinOp { .. }), "div-by-zero must NOT be folded");
    }

    #[test]
    fn compiled_multi_rule_spec_composes() {
        // A spec composing all four rule kinds simplifies `if !(false) { 1+0 }
        // else { 2 }` → (bool-not) if true {1+0} else {2} → (const-branch) 1+0 →
        // (arith-identity) 1.
        let spec = RewriteSpec::parse(
            "simplify-bool-not\nfold-const-branch\nfold-arith-identity\nfold-int-literal",
        )
        .unwrap();
        spec.validate().unwrap();
        let pass = compile(&spec);
        let out = pass(&prog("fn main() -> i64 { if !(false) { 1 + 0 } else { 2 } }"));
        // The taken branch `{ 1 + 0 }` is a Block; after const-branch + arith
        // folding its tail is the literal 1. Unwrap the (possibly nested) block.
        let mut tail = body_tail(&out);
        while let Expr::Block(stmts) = &tail {
            tail = stmts.last().unwrap().expr.clone();
        }
        assert!(
            matches!(tail, Expr::Literal(Literal::Int(1))),
            "composed rules should reduce to 1: {tail:?}"
        );
    }

    // ── fold-logical (a NEW rule kind, added under the red-team discipline) ────

    /// Build the bare `BinOp` for a logical operator over two raw expr operands.
    fn logical(op: BinOp, l: Expr, r: Expr) -> Expr {
        Expr::BinOp { op, left: Box::new(l), right: Box::new(r) }
    }
    fn b(v: bool) -> Expr {
        Expr::Literal(Literal::Bool(v))
    }
    fn id(n: &str) -> Expr {
        Expr::Ident(n.to_string())
    }

    #[test]
    fn fold_logical_folds_only_short_circuit_sound_cases() {
        let rule = RewriteRule::FoldLogicalShortCircuit;
        // SOUND folds (no evaluated operand dropped):
        // false && R → false  (R never evaluated)
        assert!(matches!(
            rule.apply_here(&logical(BinOp::And, b(false), id("r"))),
            Some(Expr::Literal(Literal::Bool(false)))
        ));
        // true && R → R ; L && true → L
        assert!(matches!(rule.apply_here(&logical(BinOp::And, b(true), id("r"))), Some(Expr::Ident(n)) if n == "r"));
        assert!(matches!(rule.apply_here(&logical(BinOp::And, id("l"), b(true))), Some(Expr::Ident(n)) if n == "l"));
        // true || R → true ; false || R → R ; L || false → L
        assert!(matches!(
            rule.apply_here(&logical(BinOp::Or, b(true), id("r"))),
            Some(Expr::Literal(Literal::Bool(true)))
        ));
        assert!(matches!(rule.apply_here(&logical(BinOp::Or, b(false), id("r"))), Some(Expr::Ident(n)) if n == "r"));
        assert!(matches!(rule.apply_here(&logical(BinOp::Or, id("l"), b(false))), Some(Expr::Ident(n)) if n == "l"));
    }

    #[test]
    fn fold_logical_refuses_the_drop_left_unsound_cases() {
        // THE SOUNDNESS BOUNDARY: `L && false` and `L || true` must NOT fold —
        // the LEFT operand is always evaluated, so dropping it would erase L's
        // side effects/panic. The rule returns None (no fold) for these.
        let rule = RewriteRule::FoldLogicalShortCircuit;
        assert!(
            rule.apply_here(&logical(BinOp::And, id("l"), b(false))).is_none(),
            "L && false must NOT fold to false — would drop the evaluated L"
        );
        assert!(
            rule.apply_here(&logical(BinOp::Or, id("l"), b(true))).is_none(),
            "L || true must NOT fold to true — would drop the evaluated L"
        );
        // A non-constant `a && b` is untouched.
        assert!(rule.apply_here(&logical(BinOp::And, id("a"), id("b"))).is_none());
    }

    #[test]
    fn compiled_fold_logical_spec_preserves_behavior_on_a_real_program() {
        // The data path: a spec naming the new rule folds `false && (1 > 0)` to
        // `false` (the rhs is never evaluated, so this is sound).
        let spec = RewriteSpec::parse("fold-logical").unwrap();
        spec.validate().unwrap();
        let pass = compile(&spec);
        let out = pass(&prog("fn main() -> i64 { if false && (1 > 0) { 1 } else { 0 } }"));
        // The condition folds to `false`; the if is left for fold-const-branch.
        // We only assert the && collapsed — find the If's condition.
        if let Item::FnDef(f) = &out.items[0] {
            let body = match &f.body {
                Expr::Block(s) => s.last().unwrap().expr.clone(),
                o => o.clone(),
            };
            if let Expr::If { cond, .. } = &body {
                assert!(
                    matches!(cond.as_ref(), Expr::Literal(Literal::Bool(false))),
                    "false && _ should fold to false: {cond:?}"
                );
            } else {
                panic!("expected an if, got {body:?}");
            }
        }
    }
}
