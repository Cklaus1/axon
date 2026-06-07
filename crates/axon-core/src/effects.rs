//! Phase 6 effect-row checking (§2 semantic rules E02 / E05).
//!
//! A function may declare an effect row (`fn f() -> T | {IO, Net}`). This pass
//! enforces **subsumption** (E02): at every call site inside a fn that declared
//! a row, the callee's effect row must be a subset of the caller's declared row.
//! A function that declares the explicit empty row `| {}` is pure and may call
//! nothing effectful (rule E05) — that falls out of the subset check against an
//! empty set.
//!
//! **Migration-window semantics (opt-in).** A function with NO row clause is
//! treated as OPEN (unconstrained), not as the empty/pure row. This is a
//! deliberate, temporary relaxation of spec rule E01 so that every Phase 1–5
//! source — which has no row clauses and freely does IO in helper functions —
//! keeps compiling unchanged (the spec's backwards-compatibility guarantee).
//! You opt a function into effect checking by writing its row: `| {IO}` admits
//! IO, `| {}` forbids all effects. Once the stdlib/examples carry rows, the
//! default can flip to E01 (absent = pure). Mirrors how `@[pure]`/`@[total]`
//! were inert until applied.
//!
//! Builtin rows come from [`crate::builtins::builtin_effect_row`] (§3 catalog);
//! user-function rows come from their declared clause. A call to an unknown name
//! (e.g. a type constructor, a not-yet-resolved symbol) contributes no effects —
//! name resolution reports unknowns; we never invent an effect.
//!
//! **`main` escape hatch (§3):** `main`'s default declared row is "everything",
//! so a top-level program that prints / calls AI without annotating `main`
//! still checks. Concretely, a `main` with NO row clause is treated as allowing
//! every effect; a `main` WITH an explicit clause is held to it like any fn.
//!
//! Row VARIABLES (`...e`) make a row open: such a function is treated as
//! allowing any effect (a forwarded callback can carry anything). Closing the
//! row (rule E03) and handler discharge (E04) are later slices; until then an
//! open row conservatively admits, never falsely rejects.
//!
//! Enforcement is intentionally erase-only at runtime: this pass produces
//! diagnostics; codegen and the interpreter ignore rows entirely.

use crate::ast::{Expr, Item, Program};
use crate::error::E1310;
use crate::span::Span;
use std::collections::{HashMap, HashSet};

/// One effect-row violation (a leaked effect at a call site).
#[derive(Debug, Clone)]
pub struct EffectError {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

/// The allowed effects of a function: a concrete set, or "open" (a row variable
/// or the `main` escape hatch) which admits any effect.
#[derive(Clone)]
enum Allowed {
    Open,
    Set(HashSet<String>),
}

impl Allowed {
    fn admits(&self, eff: &str) -> bool {
        match self {
            Allowed::Open => true,
            Allowed::Set(s) => s.contains(eff),
        }
    }
}

/// Check effect-row subsumption across `program`. Returns one E1310 per call
/// site that performs an effect outside the enclosing function's declared row.
pub fn check_effects(program: &Program) -> Vec<EffectError> {
    // fn name → its declared concrete effect set (independent of openness).
    let mut declared: HashMap<String, HashSet<String>> = HashMap::new();
    // fn name → whether its row is open (a row variable present).
    let mut open: HashMap<String, bool> = HashMap::new();
    for item in &program.items {
        if let Item::FnDef(f) = item {
            let (set, is_open) = match &f.effect_row {
                Some(row) => (
                    row.effects.iter().cloned().collect(),
                    row.row_var.is_some(),
                ),
                None => (HashSet::new(), false),
            };
            declared.insert(f.name.clone(), set);
            open.insert(f.name.clone(), is_open);
        }
    }

    // INFERRED effects per fn: the transitive closure of effects a call to it
    // actually performs — its body's direct builtin effects ∪ the inferred
    // effects of every fn it calls. This closes the transitive-laundering hole:
    // a fn declaring `| {}` that calls an UN-annotated helper which does IO must
    // still be flagged, because the helper's inferred set includes IO even
    // though its DECLARED row is empty/open. A fn's OWN declared row does not cap
    // its inferred set (we infer what it does, then separately check that
    // against what it declared). Computed to a fixpoint over the call graph.
    let inferred = infer_effects(program);

    let mut errors = Vec::new();
    for item in &program.items {
        let Item::FnDef(f) = item else { continue };
        // The caller's allowed effects. Three ways to be Open (unconstrained):
        //   1. no row clause at all (migration window — opt-in; see module docs),
        //   2. `main` (the top-level escape hatch — admits everything),
        //   3. an open row variable `...e`.
        // Otherwise the caller is held to its declared concrete set.
        let is_open = f.effect_row.is_none()
            || f.name == "main"
            || open.get(&f.name).copied().unwrap_or(false);
        let allowed = if is_open {
            Allowed::Open
        } else {
            Allowed::Set(declared.get(&f.name).cloned().unwrap_or_default())
        };
        // Open callers can never leak — skip the walk entirely.
        if matches!(allowed, Allowed::Open) {
            continue;
        }
        check_expr(&f.body, &f.name, &allowed, &HashSet::new(), &inferred, &mut errors);
    }
    errors
}

/// Compute each function's inferred (transitive) effect set to a fixpoint. A
/// fn's effects are the builtin effects of every call in its body plus the
/// inferred effects of every user-fn it calls. Iterated until no set grows, so
/// effects propagate up the call graph through arbitrarily long helper chains
/// (recursion converges because sets only grow and the effect universe is
/// finite). This is what subsumption is checked against — not the declared row —
/// so an effect cannot be laundered through an un-annotated intermediary.
fn infer_effects(program: &Program) -> HashMap<String, HashSet<String>> {
    // Pre-collect each fn's body so we can re-walk cheaply each iteration.
    let fns: Vec<&crate::ast::FnDef> = program
        .items
        .iter()
        .filter_map(|it| match it {
            Item::FnDef(f) => Some(f),
            _ => None,
        })
        .collect();
    // Seed each fn with its DECLARED concrete effects: a declared row is a
    // promise the fn may perform those effects (e.g. a `| {Net}` stub whose body
    // is still `{ 0 }`), so callers must honour them even before the body does.
    let mut inferred: HashMap<String, HashSet<String>> = fns
        .iter()
        .map(|f| {
            let mut seed: HashSet<String> = f
                .effect_row
                .as_ref()
                .map(|r| r.effects.iter().cloned().collect())
                .unwrap_or_default();
            // §4: a `@[contained]` capability spec also DECLARES effects — the
            // capabilities it grants imply the rows it may exercise. This lets
            // the effect checker see through a contained helper just like a
            // row-annotated one (one unified effect/capability model).
            if let Some(spec) = &f.contained {
                for eff in contained_effect_row(spec) {
                    seed.insert(eff);
                }
            }
            (f.name.clone(), seed)
        })
        .collect();

    loop {
        let mut changed = false;
        for f in &fns {
            // Gather the names this fn calls, each paired with the set of effects
            // discharged by handlers wrapping that call site (E04). An effect a
            // call performs that is in its `handled` context is dropped — it does
            // NOT propagate to this fn's transitive set, so a fn that fully
            // handles an effect internally is pure to its callers (and a `| {}`
            // caller of it is not falsely flagged).
            let mut calls: Vec<(String, HashSet<String>)> = Vec::new();
            collect_called_names_ctx(&f.body, &HashSet::new(), &mut calls);
            let mut acc: HashSet<String> = HashSet::new();
            for (name, handled) in &calls {
                for eff in crate::builtins::builtin_effect_row(name) {
                    if !handled.contains(*eff) {
                        acc.insert((*eff).to_string());
                    }
                }
                if let Some(callee_set) = inferred.get(name) {
                    for eff in callee_set {
                        if !handled.contains(eff) {
                            acc.insert(eff.clone());
                        }
                    }
                }
            }
            let cur = inferred.get_mut(&f.name).expect("seeded");
            let before = cur.len();
            cur.extend(acc);
            if cur.len() != before {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    inferred
}

/// §4: derive the effect row a `@[contained(...)]` spec declares. The granted
/// capabilities imply the effects the function may perform: filesystem
/// read/write and process exec are forms of `IO`; a network allowlist grants
/// `Net`. A `never:`-only spec grants nothing. This is the bridge that lets the
/// Phase-4 capability sandbox participate in the Phase-6 effect model — a
/// contained fn contributes these effects to its callers' inferred sets.
fn contained_effect_row(spec: &crate::ast::ContainedSpec) -> Vec<String> {
    let mut effs = Vec::new();
    if !spec.fs_read.is_empty() || !spec.fs_write.is_empty() || spec.exec_allowed {
        effs.push("IO".to_string());
    }
    if !spec.net_allow.is_empty() {
        effs.push("Net".to_string());
    }
    effs
}

/// The effects an INLINE handler discharges: the effect name of each `on E(p)`
/// arm. A `return(...)` arm has an empty effect name and discharges nothing. A
/// NAMED handler (`with retry { … }`) discharges nothing here — its definition
/// is not resolved yet (a later slice), and assuming it handles an effect would
/// be unsound (it would reopen a laundering hole). E04 discharge therefore
/// applies only to inline handlers, where the handled set is syntactically
/// present on the arms.
fn handler_discharges(handler: &crate::ast::HandlerExpr) -> HashSet<String> {
    let mut h = HashSet::new();
    if let crate::ast::HandlerExpr::Inline { arms, .. } = handler {
        for arm in arms {
            if !arm.effect.is_empty() {
                h.insert(arm.effect.clone());
            }
        }
    }
    h
}

/// Collect the names of every directly-called function/builtin in `e` whose
/// effects are NOT discharged by an enclosing inline handler. `handled` is the
/// set of effects discharged by handlers wrapping `e`. When a called name's
/// effects are fully covered by `handled`, the call is dropped from the result
/// so the transitive `infer_effects` fixpoint does not propagate a handled
/// effect up to callers (E04). A call performing a mix of handled and
/// un-handled effects is kept (the un-handled ones still leak); the per-effect
/// filtering against `handled` happens in `infer_effects` itself, which has the
/// catalog/inferred sets — here we only manage the `handled` context so handler
/// bodies vs arms are scoped correctly.
fn collect_called_names_ctx(
    e: &Expr,
    handled: &HashSet<String>,
    out: &mut Vec<(String, HashSet<String>)>,
) {
    if let Expr::Call { callee, .. } = e {
        if let Expr::Ident(name) = callee.as_ref() {
            out.push((name.clone(), handled.clone()));
        }
    }
    if let Expr::WithHandler { handler, body } = e {
        // Body sees the handler's discharged effects added to the context.
        let mut inner = handled.clone();
        for eff in handler_discharges(handler) {
            inner.insert(eff);
        }
        collect_called_names_ctx(body, &inner, out);
        // Handler ARM bodies are checked with the ORIGINAL context — an arm does
        // not discharge its own effects (an `on Net` arm that logs via IO still
        // performs IO against the enclosing row).
        if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
            for arm in arms {
                collect_called_names_ctx(&arm.body, handled, out);
            }
            if let Some(ra) = return_arm {
                collect_called_names_ctx(&ra.body, handled, out);
            }
        }
        return;
    }
    for_each_child(e, &mut |child| collect_called_names_ctx(child, handled, out));
}


/// The effects a call to `callee` performs: the builtin catalog row, or the
/// callee's INFERRED (transitive) effect set, or nothing for an unknown name.
fn callee_effects(
    callee: &str,
    inferred: &HashMap<String, HashSet<String>>,
) -> Vec<String> {
    let builtin = crate::builtins::builtin_effect_row(callee);
    if !builtin.is_empty() {
        return builtin.iter().map(|s| s.to_string()).collect();
    }
    if let Some(set) = inferred.get(callee) {
        return set.iter().cloned().collect();
    }
    Vec::new()
}

fn check_expr(
    e: &Expr,
    caller: &str,
    allowed: &Allowed,
    handled: &HashSet<String>,
    inferred: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<EffectError>,
) {
    if let Expr::Call { callee, .. } = e {
        if let Expr::Ident(name) = callee.as_ref() {
            // The callee's INFERRED (transitive) effects — so an effect a helper
            // performs is attributed to it even if its declared row is empty/open
            // (no laundering through un-annotated intermediaries). Builtins use
            // their catalog row directly.
            for eff in callee_effects(name, inferred) {
                // E04: an effect discharged by an enclosing inline handler is
                // not a leak — the handler intercepts it here.
                if !allowed.admits(&eff) && !handled.contains(&eff) {
                    errors.push(EffectError {
                        code: E1310,
                        message: format!(
                            "`{caller}` calls `{name}`, which performs effect `{eff}`, \
                             but `{caller}`'s declared effect row does not include `{eff}` \
                             (add `{eff}` to the row, or handle the effect)"
                        ),
                        // Phase-6 Expr nodes carry no span yet, so the
                        // diagnostic is whole-file; the message names both fns
                        // and the effect, which is enough to locate it. A future
                        // slice can thread call-site spans through.
                        span: Span::dummy(),
                    });
                }
            }
        }
    }
    // A `with H { body }` discharges H's inline-handled effects FROM THE BODY
    // (E04). The handler's own arm bodies are checked WITHOUT that discharge —
    // an arm does not handle its own effects.
    if let Expr::WithHandler { handler, body } = e {
        let mut inner = handled.clone();
        for eff in handler_discharges(handler) {
            inner.insert(eff);
        }
        check_expr(body, caller, allowed, &inner, inferred, errors);
        if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
            for arm in arms {
                check_expr(&arm.body, caller, allowed, handled, inferred, errors);
            }
            if let Some(ra) = return_arm {
                check_expr(&ra.body, caller, allowed, handled, inferred, errors);
            }
        }
        return;
    }
    // Recurse into children regardless, so nested calls are checked.
    for_each_child(e, &mut |child| {
        check_expr(child, caller, allowed, handled, inferred, errors)
    });
}

/// Apply `f` to each direct child expression of `e`. Mirrors the shape of the
/// checker's own expression walker so every call site is reached.
fn for_each_child(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    match e {
        Expr::Call { callee, args, .. } => {
            f(callee);
            for a in args {
                f(a);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            f(receiver);
            for a in args {
                f(a);
            }
        }
        Expr::BinOp { left, right, .. } => {
            f(left);
            f(right);
        }
        Expr::UnaryOp { operand, .. } => f(operand),
        Expr::Question(inner) => f(inner),
        Expr::If { cond, then, else_ } => {
            f(cond);
            f(then);
            if let Some(e) = else_ {
                f(e);
            }
        }
        Expr::Match { subject, arms } => {
            f(subject);
            for arm in arms {
                f(&arm.body);
            }
        }
        Expr::Block(stmts) => {
            for s in stmts {
                f(&s.expr);
            }
        }
        Expr::Let { value, .. } => f(value),
        Expr::Assign { value, .. } => f(value),
        Expr::While { cond, body } => {
            f(cond);
            for s in body {
                f(&s.expr);
            }
        }
        Expr::WhileLet { expr, body, .. } => {
            f(expr);
            for s in body {
                f(&s.expr);
            }
        }
        Expr::Lambda { body, .. } => f(body),
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                f(v);
            }
        }
        Expr::FieldAccess { receiver, .. } => f(receiver),
        Expr::Index { receiver, index } => {
            f(receiver);
            f(index);
        }
        Expr::Array(items) | Expr::Tuple(items) => {
            for it in items {
                f(it);
            }
        }
        Expr::Ok(inner) | Expr::Err(inner) | Expr::Some(inner) => f(inner),
        Expr::FmtStr { parts } => {
            for p in parts {
                if let crate::ast::FmtPart::Expr(e) = p {
                    f(e);
                }
            }
        }
        Expr::Return(Some(inner)) => f(inner),
        Expr::For { start, end, body, .. } => {
            f(start);
            f(end);
            for s in body {
                f(&s.expr);
            }
        }
        Expr::AssignTo { place, value } => {
            f(place);
            f(value);
        }
        // A `with H { body }` does NOT hide the body's effects from the checker.
        // Until handlers actually DISCHARGE effects (E04), the body's effects
        // still count toward the enclosing fn's row — otherwise a `| {}` fn
        // could perform IO inside a `with` block and escape E1310 (an effect-
        // laundering hole, same class as the transitive-helper hole). Recurse
        // into the body AND the inline handler arm bodies (an arm can itself
        // perform effects, e.g. an `on Net` arm that logs via IO).
        Expr::WithHandler { handler, body } => {
            f(body);
            if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                for arm in arms {
                    f(&arm.body);
                }
                if let Some(ra) = return_arm {
                    f(&ra.body);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    fn check(src: &str) -> Vec<EffectError> {
        check_effects(&parse_source(src).expect("parse"))
    }

    #[test]
    fn explicit_empty_row_fn_calling_io_builtin_leaks() {
        // Opting in with the explicit empty row `| {}` makes the fn pure; calling
        // println (which is {IO}) then leaks.
        let errs = check("fn f(x: i64) -> i64 | {} { println(\"hi\") x }");
        assert_eq!(errs.len(), 1, "expected one E1310, got {errs:?}");
        assert_eq!(errs[0].code, E1310);
        assert!(errs[0].message.contains("IO"), "must name the IO effect: {}", errs[0].message);
    }

    #[test]
    fn io_inside_a_with_handler_body_still_leaks() {
        // An effect performed inside `with H { … }` must NOT escape the effect
        // checker. Until handlers actually DISCHARGE effects (E04), the body's
        // effects still count toward the enclosing fn's row — otherwise a `| {}`
        // fn could launder IO through a `with` block (the for_each_child walker
        // used to skip WithHandler entirely, so this was a real hole).
        let errs = check(
            "fn f() -> i64 | {} { with retry { println(\"hi\") 0 } }",
        );
        assert_eq!(errs.len(), 1, "with-block must not hide IO, got {errs:?}");
        assert_eq!(errs[0].code, E1310);
        assert!(errs[0].message.contains("IO"), "must name IO: {}", errs[0].message);
    }

    #[test]
    fn io_inside_a_for_loop_body_still_leaks() {
        // Same laundering class for `for` loops — the walker used to skip the
        // For body, so a `| {}` fn could hide IO inside a loop.
        let errs = check(
            "fn f() -> i64 | {} { for i in 0..3 { println(\"hi\") } 0 }",
        );
        assert_eq!(errs.len(), 1, "for-body must not hide IO, got {errs:?}");
        assert_eq!(errs[0].code, E1310);
        assert!(errs[0].message.contains("IO"), "must name IO: {}", errs[0].message);
    }

    #[test]
    fn inline_handler_discharges_its_arm_effect() {
        // E04: `with handler { on Net(e) => … } { fetch() }` HANDLES Net, so a
        // `| {}` fn wrapping a Net-performing call in that handler is pure — no
        // leak. This is the discharge that the laundering-hole fix set up.
        let errs = check(
            "fn fetch() -> i64 | {Net} { 0 }\n\
             fn safe() -> i64 | {} { with handler { on Net(e) => 0 } { fetch() } }",
        );
        assert!(errs.is_empty(), "inline handler should discharge Net, got {errs:?}");
    }

    #[test]
    fn named_handler_does_not_discharge() {
        // A NAMED handler's definition is not resolved yet, so it discharges
        // NOTHING — assuming otherwise would reopen a laundering hole. The Net
        // call inside `with retry { … }` still leaks.
        let errs = check(
            "fn fetch() -> i64 | {Net} { 0 }\n\
             fn f() -> i64 | {} { with retry { fetch() } }",
        );
        assert_eq!(errs.len(), 1, "named handler must not discharge, got {errs:?}");
        assert!(errs[0].message.contains("Net"));
    }

    #[test]
    fn handler_arm_body_does_not_self_discharge() {
        // An `on Net(e) => println(...)` arm performs IO; that IO is NOT handled
        // by the arm itself, so it leaks against a `| {}` row. (Net IS discharged
        // for the body; the arm's own IO is not.)
        let errs = check(
            "fn fetch() -> i64 | {Net} { 0 }\n\
             fn f() -> i64 | {} { with handler { on Net(e) => println(\"log\") } { fetch() } }",
        );
        assert_eq!(errs.len(), 1, "arm IO must leak, got {errs:?}");
        assert!(errs[0].message.contains("IO"), "arm leaks IO not Net: {}", errs[0].message);
    }

    #[test]
    fn discharge_is_transitive_consistent() {
        // A fn that fully handles Net internally is PURE to its callers — the
        // discharge applies in infer_effects too, so a `| {}` caller is not
        // falsely flagged. (If discharge were only in the subsumption check and
        // not the transitive set, `caller` would wrongly see Net here.)
        let clean = check(
            "fn fetch() -> i64 | {Net} { 0 }\n\
             fn safe() -> i64 | {} { with handler { on Net(e) => 0 } { fetch() } }\n\
             fn caller() -> i64 | {} { safe() }",
        );
        assert!(clean.is_empty(), "handled-internally fn must be pure to callers, got {clean:?}");
        // Control: a fn that does NOT handle Net propagates it to its caller.
        let leaks = check(
            "fn fetch() -> i64 | {Net} { 0 }\n\
             fn leaky() -> i64 | {Net} { fetch() }\n\
             fn caller() -> i64 | {} { leaky() }",
        );
        assert_eq!(leaks.len(), 1, "un-handled Net must reach the caller, got {leaks:?}");
        assert!(leaks[0].message.contains("Net"));
    }

    #[test]
    fn io_inside_a_for_loop_body_is_fine_when_row_admits_it() {
        // Guard against over-correction: a fn that DECLARES {IO} may freely do IO
        // inside a for-loop body — no false positive from the deeper walk.
        let errs = check(
            "fn f() -> i64 | {IO} { for i in 0..3 { println(\"hi\") } 0 }",
        );
        assert!(errs.is_empty(), "row {{IO}} should admit loop IO, got {errs:?}");
    }

    #[test]
    fn no_row_clause_is_open_during_migration() {
        // A fn with NO clause is unconstrained (opt-in) — Phase 1-5 helpers that
        // do IO without annotation must keep compiling. This is the migration
        // relaxation of rule E01.
        let errs = check("fn helper(x: i64) -> i64 { println(\"hi\") x }");
        assert!(errs.is_empty(), "no-clause fn should be open, got {errs:?}");
    }

    #[test]
    fn fn_declaring_the_effect_is_ok() {
        // The same call is fine once the fn declares `| {IO}`.
        let errs = check("fn f(x: i64) -> i64 | {IO} { println(\"hi\") x }");
        assert!(errs.is_empty(), "declared row should admit the call, got {errs:?}");
    }

    #[test]
    fn main_escape_hatch_admits_everything() {
        // `main` with no clause is the top-level escape hatch — IO/AI/etc are OK.
        let errs = check("fn main() { println(\"hi\") }");
        assert!(errs.is_empty(), "main escape hatch should admit IO, got {errs:?}");
    }

    #[test]
    fn subset_is_allowed_superset_leaks() {
        // Declared {IO, Net}: an {IO} call is a subset (ok). Declared {IO}: a
        // {Net}-bearing call (fetch declares Net) leaks.
        let ok = check(
            "fn fetch(u: str) -> i64 | {Net} { 0 }\n\
             fn g() -> i64 | {IO, Net} { fetch(\"x\") }",
        );
        assert!(ok.is_empty(), "Net ⊆ {{IO,Net}} should be ok, got {ok:?}");

        let leak = check(
            "fn fetch(u: str) -> i64 | {Net} { 0 }\n\
             fn g() -> i64 | {IO} { fetch(\"x\") }",
        );
        assert_eq!(leak.len(), 1, "Net ⊄ {{IO}} should leak once, got {leak:?}");
        assert!(leak[0].message.contains("Net"));
    }

    #[test]
    fn open_row_caller_admits_anything() {
        // A caller whose row is open (`...e`) is conservatively allowed to call
        // anything — no false leak (full row-var unification is a later slice).
        let errs = check(
            "fn g() -> i64 | {...e} { println(\"hi\") 0 }",
        );
        assert!(errs.is_empty(), "open-row caller should not leak, got {errs:?}");
    }

    #[test]
    fn transitive_effect_is_not_launderable() {
        // `claims_pure` declares `| {}` but calls an UN-annotated helper that
        // does IO. The effect must be attributed transitively — declaring an
        // empty row while reaching IO through an intermediary is still a leak.
        let errs = check(
            "fn does_io(x: i64) -> i64 { println(\"io {to_str(x)}\") x }\n\
             fn claims_pure(x: i64) -> i64 | {} { does_io(x) }",
        );
        assert!(
            errs.iter().any(|e| e.message.contains("claims_pure") && e.message.contains("IO")),
            "transitive IO must be flagged on claims_pure, got {errs:?}"
        );
    }

    #[test]
    fn transitive_effect_through_two_hops() {
        // IO laundered through two un-annotated hops still reaches the `| {}` fn.
        let errs = check(
            "fn a(x: i64) -> i64 { println(\"{to_str(x)}\") x }\n\
             fn b(x: i64) -> i64 { a(x) }\n\
             fn pure_c(x: i64) -> i64 | {} { b(x) }",
        );
        assert!(
            errs.iter().any(|e| e.message.contains("pure_c") && e.message.contains("IO")),
            "IO through a→b→pure_c must be flagged, got {errs:?}"
        );
    }

    #[test]
    fn transitive_declared_row_is_satisfiable() {
        // The same two-hop chain is fine when the fn declares the effect it
        // transitively performs.
        let errs = check(
            "fn a(x: i64) -> i64 { println(\"{to_str(x)}\") x }\n\
             fn b(x: i64) -> i64 { a(x) }\n\
             fn io_c(x: i64) -> i64 | {IO} { b(x) }",
        );
        assert!(errs.is_empty(), "declared {{IO}} should admit the transitive IO, got {errs:?}");
    }

    #[test]
    fn contained_capability_contributes_effects() {
        // A `@[contained(net: [...])]` helper declares the Net effect through its
        // capability spec; a caller declaring `| {}` that calls it must be flagged.
        let errs = check(
            "@[contained(net: [\"api.example.com\"])]\n\
             fn fetch(u: str) -> i64 { 0 }\n\
             fn caller() -> i64 | {} { fetch(\"x\") }",
        );
        assert!(
            errs.iter().any(|e| e.message.contains("caller") && e.message.contains("Net")),
            "contained net cap must surface as a Net effect leak, got {errs:?}"
        );

        // Declaring the matching row admits it.
        let ok = check(
            "@[contained(net: [\"api.example.com\"])]\n\
             fn fetch(u: str) -> i64 { 0 }\n\
             fn caller() -> i64 | {Net} { fetch(\"x\") }",
        );
        assert!(ok.is_empty(), "declared {{Net}} should admit the contained call, got {ok:?}");
    }

    #[test]
    fn pure_calling_pure_is_clean() {
        // No effects anywhere — the common case must stay silent.
        let errs = check("fn add(a: i64, b: i64) -> i64 { a + b }\nfn g() -> i64 { add(1, 2) }");
        assert!(errs.is_empty(), "pure→pure must be clean, got {errs:?}");
    }

    #[test]
    fn leak_through_nested_call_position() {
        // The effectful call is buried in an if-branch, not the tail. The fn
        // opts into checking with the explicit empty row.
        let errs = check(
            "fn f() -> i64 | {} { if true { println(\"x\") 1 } else { 2 } }",
        );
        assert_eq!(errs.len(), 1, "nested IO call should still leak, got {errs:?}");
    }
}
