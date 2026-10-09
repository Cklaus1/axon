//! R50 S0 unit tests (spec §8 unit row, the parts S0 has): the S0 compiler's
//! one-`Tree`-op bodies, the `FnEntry::compiled` discriminator, and the op
//! loop's scope discipline on every exit path.

use super::*;
use crate::ast::Literal;

const PROG: &str = "\
fn fib(n: i64) -> i64 {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}
type P = { x: i64 }
trait Get {
    fn get(self) -> i64
}
impl Get for P {
    fn get(self: P) -> i64 { self.x }
}
fn main() -> i64 {
    let xs = [1, 2, 3]
    let s = 0
    for i in 0..3 { s = s + xs[i] }
    while s > 100 { s = s - 1 }
    match Some(s) { Some(v) if v > 0 => v, _ => 0 }
}
";

fn program() -> crate::ast::Program {
    crate::parse_source(PROG).expect("test program parses")
}

fn fib_index(interp: &Interp<'_>) -> usize {
    interp
        .fn_table
        .iter()
        .position(|e| e.def.name == "fib")
        .expect("fib is in the fn table")
}

#[test]
fn s0_compiles_every_fn_body_to_one_tree_op_over_the_root() {
    let prog = program();
    let interp = Interp::build(&prog);
    assert!(!interp.fn_table.is_empty());
    for entry in &interp.fn_table {
        let body = compile(&entry.def.body);
        assert_eq!(body.ops.len(), 1, "{}", entry.def.name);
        assert_eq!(body.tree_nodes(), 1, "{}", entry.def.name);
        match &body.ops[0] {
            Op::Tree(e) => assert!(std::ptr::eq(*e, &entry.def.body), "{}", entry.def.name),
            _ => panic!("{}: expected a Tree op", entry.def.name),
        }
    }
}

#[test]
fn s0_every_expr_node_falls_back_to_one_tree_op() {
    let prog = program();
    let mut seen = std::collections::HashSet::new();
    for item in &prog.items {
        let crate::ast::Item::FnDef(f) = item else {
            continue;
        };
        crate::ast::walk_expr(&f.body, &mut |e| {
            let body = compile(e);
            assert_eq!(body.ops.len(), 1);
            assert!(matches!(body.ops[0], Op::Tree(t) if std::ptr::eq(t, e)));
            seen.insert(compile::variant_name(e));
        });
    }
    // The walk reached a spread of variants, not just the roots.
    for v in [
        "Block", "If", "BinOp", "Call", "Let", "For", "While", "Match", "Index",
    ] {
        assert!(seen.contains(v), "{v} not exercised: {seen:?}");
    }
}

#[test]
fn compiled_is_some_only_for_fn_table_entries() {
    let prog = program();
    let interp = Interp::build(&prog);
    for entry in &interp.fn_table {
        let cell = entry.compiled.as_ref().expect("table entries carry a cell");
        assert!(cell.get().is_none(), "nothing compiled before a run");
    }
    // The owned entry `call_fn` builds for a def missing from `fn_of_def`.
    let owned = FnEntry::new(interp.fn_table[fib_index(&interp)].def, &interp.fns);
    assert!(owned.compiled.is_none());
}

#[test]
fn vm_engine_compiles_a_body_on_first_run_and_tree_never_does() {
    let prog = program();
    for (engine, compiled) in [(Engine::Tree, false), (Engine::Vm, true)] {
        let mut interp = Interp::build(&prog);
        interp.engine = engine;
        interp.vm_trace = false;
        let i = fib_index(&interp);
        let got = interp
            .call_fn_entry(&interp.fn_table[i], vec![Value::Int(10)])
            .expect("fib(10) runs");
        assert!(matches!(got, Value::Int(55)), "{engine:?}: {got:?}");
        let cell = interp.fn_table[i].compiled.as_ref().unwrap();
        assert_eq!(cell.get().is_some(), compiled, "{engine:?}");
    }
}

#[test]
fn vm_engine_runs_an_owned_entry_on_the_tree() {
    let prog = program();
    let mut interp = Interp::build(&prog);
    interp.engine = Engine::Vm;
    interp.vm_trace = false;
    let owned = FnEntry::new(interp.fn_table[fib_index(&interp)].def, &interp.fns);
    let got = interp
        .call_fn_entry(&owned, vec![Value::Int(10)])
        .expect("fib(10) runs");
    assert!(matches!(got, Value::Int(55)), "{got:?}");
    assert!(owned.compiled.is_none());
}

#[test]
fn engine_parse_accepts_only_vm_and_tree() {
    assert_eq!(Engine::parse("vm"), Ok(Engine::Vm));
    assert_eq!(Engine::parse("tree"), Ok(Engine::Tree));
    assert_eq!(
        Engine::parse("bogus"),
        Err("AXON_ENGINE must be \"vm\" or \"tree\" (got \"bogus\")".to_string())
    );
    assert!(Engine::parse("").is_err());
    assert!(Engine::parse("VM").is_err());
}

// ── Op loop: scopes pop on every exit path ──────────────────────────────────

fn int(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n))
}

/// Run `ops` against a frame whose base scope binds `a = 1` and one pushed
/// scope binds `a = 2`; checks the frame is exactly as it was afterwards (the
/// same marks, and `a` still reads the inner binding), and returns the result.
fn run_ops(ops: Vec<Op<'_>>) -> R {
    let prog = program();
    let interp = Interp::build(&prog);
    let a = intern("a");
    let mut env = Env::new();
    env.define(a, Value::Int(1));
    env.push();
    env.define(a, Value::Int(2));
    let body = Body {
        ops: ops.into_boxed_slice(),
    };
    let r = interp.exec(&body, &mut env);
    assert_eq!(
        env.marks.len(),
        1,
        "scopes pushed by the body must all be popped"
    );
    assert_eq!(env.vars.len(), 2, "bindings of popped scopes must be gone");
    assert!(matches!(env.get(a), Some(Value::Int(2))));
    r
}

fn shadow_a() -> Expr {
    Expr::Let {
        name: "a".into(),
        ty: None,
        value: Box::new(int(99)),
    }
}

#[test]
fn exec_normal_exit_keeps_scopes_balanced_and_yields_the_last_value() {
    let (l, v) = (shadow_a(), int(7));
    let r = run_ops(vec![
        Op::ScopePush,
        Op::Tree(&l),
        Op::Tree(&v),
        Op::ScopePop,
    ]);
    assert!(matches!(r, Ok(Value::Int(7))), "{r:?}");
}

#[test]
fn exec_pops_every_pushed_scope_when_an_op_errs() {
    let (l, brk) = (shadow_a(), Expr::Break);
    let r = run_ops(vec![
        Op::ScopePush,
        Op::Tree(&l),
        Op::ScopePush,
        Op::Tree(&l),
        Op::Tree(&brk),
        Op::ScopePop,
        Op::ScopePop,
    ]);
    // A `break` outside any loop of this body propagates unchanged.
    assert!(matches!(r, Err(Flow::Break)), "{r:?}");

    let undefined = Expr::Ident("no_such_name".into());
    let r = run_ops(vec![Op::ScopePush, Op::Tree(&l), Op::Tree(&undefined)]);
    assert!(
        matches!(&r, Err(Flow::Panic(m)) if m.contains("undefined identifier `no_such_name`")),
        "{r:?}"
    );
}

#[test]
fn exec_turns_return_into_the_body_value_after_popping() {
    let (l, ret) = (shadow_a(), Expr::Return(Some(Box::new(int(42)))));
    let r = run_ops(vec![
        Op::ScopePush,
        Op::Tree(&l),
        Op::ScopePush,
        Op::Tree(&ret),
    ]);
    assert!(matches!(r, Ok(Value::Int(42))), "{r:?}");
}

#[test]
fn exec_of_an_empty_body_is_unit() {
    let r = run_ops(Vec::new());
    assert!(matches!(r, Ok(Value::Unit)), "{r:?}");
}
