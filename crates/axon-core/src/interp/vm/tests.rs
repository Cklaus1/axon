//! R50 unit tests (spec §8 unit row): the S1 compiler's lowered set, the
//! `ScopePush`/`ScopePop` placement of every scoped construct, the
//! `FnEntry::compiled` discriminator, and the op loop's scope discipline on
//! every exit path (normal, caught `break`/`continue`, propagated error,
//! `return`), each checked against the tree-walker.

use super::*;
use crate::ast::Literal;

/// [`compile::compile`], which gives up only on wasm32 (AX-56).
fn compile<'p>(ix: &Interp<'_>, body: &'p crate::ast::Expr) -> Body<'p> {
    compile::compile(ix, body).expect("compiles off wasm32")
}

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

fn body_of<'p>(interp: &Interp<'p>, name: &str) -> &'p Expr {
    &interp
        .fn_table
        .iter()
        .find(|e| e.def.name == name)
        .unwrap_or_else(|| panic!("{name} is in the fn table"))
        .def
        .body
}

/// The op kinds of `body`, for placement checks.
fn kinds(body: &Body<'_>) -> Vec<&'static str> {
    body.ops
        .iter()
        .map(|op| match op {
            Op::Tree(_) => "tree",
            Op::ScopePush => "push",
            Op::ScopePop => "pop",
            Op::Const(_) => "const",
            Op::Load(_) => "load",
            Op::Drop => "drop",
            Op::Define(_) => "define",
            Op::Let { .. } => "let",
            Op::AssignInPlace { .. } => "in-place",
            Op::Store(_) => "store",
            Op::StoreBin { .. }
            | Op::StoreLocalInt { .. }
            | Op::StoreLocalLocal { .. }
            | Op::StoreLocalStack { .. } => "store-bin",
            Op::Bin { .. } | Op::BinStack(_) => "bin",
            Op::ShortCircuit { .. } => "short",
            Op::Logic(_) => "logic",
            Op::Unary(_) => "unary",
            Op::Jump(_) => "jump",
            Op::PopJump(_) => "pop-jump",
            Op::BranchFalse { .. } => "branch",
            Op::BranchCmp { .. }
            | Op::BranchLocalInt { .. }
            | Op::BranchLocalLocal { .. }
            | Op::BranchReturn { .. } => "branch-cmp",
            Op::BranchIndexLocal { .. } | Op::BranchIndexInt { .. } => "branch-index",
            Op::StrictInt => "strict-int",
            Op::ForTest { scoped: true, .. } => "for-test+body",
            Op::ForTest { scoped: false, .. } => "for-test",
            Op::ForNext { scoped: true, .. } => "for-next+body",
            Op::ForNext { scoped: false, .. } => "for-next",
            Op::Return => "return",
            Op::Break => "break",
            Op::Continue => "continue",
            Op::Question => "?",
            Op::WrapSome => "some",
            Op::WrapOk => "ok",
            Op::WrapErr => "err",
            Op::FmtNew => "fmt",
            Op::FmtLit(_) => "fmt-lit",
            Op::FmtPush => "fmt-push",
            Op::Call { .. } => "call",
            Op::MakeArray(_) => "array",
            Op::MakeTuple(_) => "tuple",
            Op::Record(_) => "record",
            Op::FieldLocal { .. } | Op::Field { .. } => "field",
            Op::IndexLocal { .. } | Op::IndexIdent(_) | Op::IndexValue => "index",
            Op::IndexDefine { .. } => "index-define",
            Op::IndexTest(_) => "index-test",
            Op::PlaceIndex => "place-index",
            Op::WritePlace { .. } | Op::WriteIndexLocal { .. } | Op::WriteIndexIndex { .. } => {
                "write-place"
            }
            Op::PlaceInvalid => "place-invalid",
            Op::MatchArm { scoped: true, .. } => "arm+scope",
            Op::MatchArm { scoped: false, .. } => "arm",
            Op::Guard { .. } => "guard",
            Op::MatchEnd { scoped: true, .. } => "arm-end+pop",
            Op::MatchEnd { scoped: false, .. } => "arm-end",
            Op::NoMatch => "no-match",
            Op::WhileLet { scoped, body, .. } => match (scoped, body) {
                (true, true) => "while-let+scope+body",
                (true, false) => "while-let+scope",
                (false, true) => "while-let+body",
                (false, false) => "while-let",
            },
            Op::WhileLetNext { .. } => "while-let-next",
            Op::MethodRecv { .. } => "method-recv",
            Op::MethodCall { .. } => "method",
            Op::Lambda(_) => "lambda",
            Op::Pure(_) => "pure",
            Op::PureLoop { .. } => "pure-loop",
            Op::CallFast { args, .. } if args.is_empty() => "call-fast",
            Op::CallFast { .. } => "call-fast-inline",
            Op::CallFastLocalInt { .. } => "call-fast-local-int",
            Op::CallMut { .. } => "call-mut",
        })
        .collect()
}

#[test]
fn s1_compiles_fib_without_tree_ops() {
    let prog = program();
    let interp = Interp::build(&prog).expect("unsealed build");
    let body = compile(&interp, body_of(&interp, "fib"));
    assert_eq!(body.tree_nodes(), 0, "{:?}", kinds(&body));
}

/// R51 §4 Fork 1: no VM body is ever built for a sealed run, whatever asks.
#[test]
#[should_panic(expected = "R51: a sealed run never compiles a VM body")]
fn r51_compile_refuses_a_sealed_run() {
    let prog = program();
    let mut interp = Interp::build(&prog).expect("unsealed build");
    interp.seal.active = true;
    let _ = compile::compile(&interp, body_of(&interp, "fib"));
}

#[test]
fn s1_leaves_only_unlowered_variants_on_the_tree() {
    let prog = program();
    let interp = Interp::build(&prog).expect("unsealed build");
    let body = compile(&interp, body_of(&interp, "main"));
    let trees: Vec<&str> = body
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Tree(e) => Some(compile::variant_name(e)),
            _ => None,
        })
        .collect();
    assert!(trees.is_empty(), "{trees:?}");
}

#[test]
fn s1_every_expr_node_compiles_to_a_balanced_body() {
    // `compile` checks (debug) that a body leaves one value and no scope.
    let prog = program();
    let interp = Interp::build(&prog).expect("unsealed build");
    let mut seen = std::collections::HashSet::new();
    for item in &prog.items {
        let crate::ast::Item::FnDef(f) = item else {
            continue;
        };
        crate::ast::walk_expr(&f.body, &mut |e| {
            let body = compile(&interp, e);
            let root_is_tree = matches!(&body.ops[..], [Op::Tree(t)] if std::ptr::eq(*t, e));
            // Every node of the program lowers: none compiles to a root `Tree`.
            assert!(!root_is_tree, "{}", compile::variant_name(e));
            seen.insert(compile::variant_name(e));
        });
    }
    for v in [
        "Block", "If", "BinOp", "Call", "Let", "For", "While", "Match", "Index", "Assign",
    ] {
        assert!(seen.contains(v), "{v} not exercised: {seen:?}");
    }
}

/// R50 S2 (§4 rows `place_index`/`write_place`, Lowered set): every place
/// write lowers; a place rooted at a call compiles to its value and index,
/// then `PlaceInvalid`, with the root never evaluated (no call op); an
/// `E[..]` read stays one `Tree` op; a fused index compare is one op.
#[test]
fn s2_place_writes_lower_and_e_index_stays_on_the_tree() {
    let src = "\
fn mk() -> [i64] { [1] }
fn f(xs: [i64], g: [[i64]], i: i64) -> i64 {
    xs[i] = 1
    g[i][i + 1] = 2
    mk()[i + 1] = 3
    let a = E[xs]
    if xs[i] < i { 1 } else { 0 }
}
";
    let prog = crate::parse_source(src).expect("parses");
    let interp = Interp::build(&prog).expect("unsealed build");
    let body = compile(&interp, body_of(&interp, "f"));
    let k = kinds(&body);
    let count = |name: &str| k.iter().filter(|&&x| x == name).count();
    assert_eq!(count("write-place"), 2, "{k:?}");
    assert_eq!(count("place-index"), 3, "{k:?}");
    assert_eq!(count("place-invalid"), 1, "{k:?}");
    assert_eq!(count("call"), 0, "{k:?}");
    assert_eq!(count("branch-index"), 1, "{k:?}");
    assert_eq!(body.tree_nodes(), 1, "{k:?}");
}

#[test]
fn compiled_is_some_only_for_fn_table_entries() {
    let prog = program();
    let interp = Interp::build(&prog).expect("unsealed build");
    for entry in &interp.fn_table {
        let cell = entry.compiled.as_ref().expect("table entries carry a cell");
        assert!(cell.get().is_none(), "nothing compiled before a run");
    }
    // The owned entry `call_fn` builds for a def missing from `fn_of_def`.
    let owned = FnEntry::new(interp.fn_table[fib_index(&interp)].def, &interp.fns, false);
    assert!(owned.compiled.is_none());
}

#[test]
fn vm_engine_compiles_a_body_on_first_run_and_tree_never_does() {
    let prog = program();
    for (engine, compiled) in [(Engine::Tree, false), (Engine::Vm, true)] {
        let mut interp = Interp::build(&prog).expect("unsealed build");
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
    let mut interp = Interp::build(&prog).expect("unsealed build");
    interp.engine = Engine::Vm;
    interp.vm_trace = false;
    let owned = FnEntry::new(interp.fn_table[fib_index(&interp)].def, &interp.fns, false);
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

// ── Scope placement: a push/pop exactly where `eval` has one ────────────────

/// The op kinds of fn `f`'s body in `src`.
fn kinds_of(src: &str, f: &str) -> Vec<&'static str> {
    let prog = crate::parse_source(src).expect("parses");
    let interp = Interp::build(&prog).expect("unsealed build");
    kinds(&compile(&interp, body_of(&interp, f)))
}

#[test]
fn block_pushes_one_scope_around_its_statements() {
    let k = kinds_of("fn f() -> i64 { let a = 1\n { let b = 2 }\n a }", "f");
    assert_eq!(
        k,
        [
            "push", "const", "define", // let a = 1
            "push", "const", "define", "pop", // { let b = 2 } as a statement
            "load", "pop",
        ]
    );
}

#[test]
fn a_block_that_binds_nothing_has_no_scope_ops() {
    // An empty scope is unobservable, so `id`'s body is one load, and the
    // `if` arms (blocks without a `let`) push nothing either.
    assert_eq!(kinds_of("fn id(x: i64) -> i64 { x }", "id"), ["load"]);
    let k = kinds_of("fn f(n: i64) -> i64 { if n < 2 { n } else { 0 } }", "f");
    assert_eq!(k, ["branch-cmp", "load", "jump", "const"]);
    // A `let` anywhere below keeps the scope, nested blocks included.
    let k = kinds_of(
        "fn f(n: i64) -> i64 { if n < 2 { { let a = n }\n n } else { 0 } }",
        "f",
    );
    assert_eq!(
        k,
        [
            "push",
            "branch-cmp",
            "push",
            "push",
            "load",
            "define",
            "pop",
            "load",
            "pop",
            "jump",
            "const",
            "pop",
        ]
    );
}

#[test]
fn while_pushes_one_scope_per_iteration_after_the_condition() {
    // The fused condition pushes the iteration's scope when it holds; the
    // back edge pops it. The S7 `PureLoop` ahead of the loop pushes none.
    let k = kinds_of(
        "fn f() -> i64 { let i = 0\n while i < 3 { let t = 1\n i = i + t }\n i }",
        "f",
    );
    assert_eq!(
        k,
        [
            "push",
            "const",
            "define",    // let i = 0
            "pure-loop", // S7: the loop in registers, declining into it
            "branch-cmp",
            "const",
            "define",
            "store-bin",
            "pop-jump", // while
            "load",
            "pop",
        ]
    );
}

#[test]
fn while_without_a_binding_runs_its_iterations_unscoped() {
    let k = kinds_of(
        "fn f() -> i64 { let i = 0\n while i < 3 { i = i + 1 }\n i }",
        "f",
    );
    assert_eq!(
        k,
        [
            "push",
            "const",
            "define",
            "pure-loop",
            "branch-cmp",
            "store-bin",
            "jump",
            "load",
            "pop"
        ]
    );
}

#[test]
fn match_arm_pushes_its_scope_only_when_it_can_bind() {
    // The subject stays on the stack under every arm; an arm whose pattern
    // binds (`Some(x)`) has a scope, which the guard's false branch and the
    // arm's end each pop; `None` and `_` bind nothing and run unscoped.
    let k = kinds_of(
        "fn f(v: Option<i64>) -> i64 { match v { Some(x) if x > 1 => x, None => 0, _ => 1 } }",
        "f",
    );
    assert_eq!(
        k,
        [
            "load",
            "arm+scope",
            "bin",
            "guard",
            "load",
            "arm-end+pop",
            "arm",
            "const",
            "arm-end",
            "arm",
            "const",
            "arm-end",
            "no-match",
        ]
    );
}

#[test]
fn while_let_pushes_the_pattern_scope_then_the_body_scope() {
    let k = kinds_of(
        "fn g(n: i64) -> Option<i64> { None }\n\
         fn f() -> i64 { let n = 0\n while let Some(x) = g(n) { let t = x\n n = n + t }\n while let None = g(n) { n = n + 1 }\n n }",
        "f",
    );
    assert_eq!(
        k,
        [
            "push",
            "const",
            "define",           // let n = 0
            "call-fast-inline", // S5: the argument is read by the call
            "while-let+scope+body",
            "load",
            "define",
            "store-bin",
            "while-let-next",
            "call-fast-inline",
            "while-let",
            "store-bin",
            "while-let-next",
            "load",
            "pop",
        ]
    );
}

#[test]
fn for_converts_both_bounds_then_pushes_per_iteration() {
    // `for-test`/`for-next` push the variable's scope and (`+body`) the
    // body's, and `for-next` pops them before the increment; a body that
    // binds nothing gets no scope of its own.
    let k = kinds_of(
        "fn f() -> i64 { let s = 0\n for i in 0..3 { s = s + i }\n s }",
        "f",
    );
    assert_eq!(
        k,
        [
            "push",
            "const",
            "define",
            "const",
            "strict-int",
            "const",
            "strict-int",
            "for-test",
            "store-bin",
            "for-next",
            "drop",
            "drop",
            "load",
            "pop",
        ]
    );
    let k = kinds_of(
        "fn f() -> i64 { let s = 0\n for i in 0..3 { let t = i\n s = s + t }\n s }",
        "f",
    );
    assert_eq!(&k[7..11], ["for-test+body", "load", "define", "store-bin"]);
    assert_eq!(k[11], "for-next+body");
}

#[test]
fn ax31_shaped_assign_runs_assign_in_place_first() {
    // Both operands inline: one fused op that runs `assign_in_place` first.
    let prog =
        crate::parse_source("fn f() -> str { let s = \"\"\n s = s + \"x\"\n s }").expect("parses");
    let interp = Interp::build(&prog).expect("unsealed build");
    let body = compile(&interp, body_of(&interp, "f"));
    assert!(body.ops.iter().any(|op| matches!(
        op,
        Op::StoreBin {
            in_place: Some(_),
            ..
        }
    )));
    // A computed operand: an `AssignInPlace` op before the value.
    let k = kinds_of(
        "fn f() -> [i64] { let x = [0]\n x = arr_push(x, 1 + 2)\n x }",
        "f",
    );
    assert_eq!(&k[4..9], ["in-place", "load", "bin", "call", "store"]);
    // Not AX-31-shaped: no `assign_in_place` call at all.
    let prog = crate::parse_source("fn f() -> i64 { let i = 0\n i = 1 + i\n i }").unwrap();
    let interp = Interp::build(&prog).expect("unsealed build");
    let body = compile(&interp, body_of(&interp, "f"));
    assert!(body.ops.iter().all(|op| !matches!(
        op,
        Op::AssignInPlace { .. }
            | Op::StoreBin {
                in_place: Some(_),
                ..
            }
    )));
}

// ── Op loop: scopes pop on every exit path ──────────────────────────────────

const EXITS: &str = "\
fn stop() -> i64 { break }
fn brk_nested() -> i64 {
    let s = 0
    while true { let t = 1
        { let u = 2
            if s > 2 { break } }
        s = s + t }
    s
}
fn cont_for() -> i64 {
    let s = 0
    for i in 0..5 { let t = i
        { let u = 0
            if i == 2 { continue } }
        s = s + t + u_free() }
    s
}
fn cont_while() -> i64 {
    let i = 0
    let s = 0
    while i < 5 { let t = i
        i = i + 1
        { let u = 0
            if t == 2 { continue } }
        s = s + t }
    s
}
fn u_free() -> i64 { 0 }
fn brk_inner_only() -> i64 {
    let n = 0
    for i in 0..3 { let a = i
        for j in 0..10 { let b = j
            if j == 2 { break }
            n = n + 1 }
        n = n + 100 }
    n
}
fn brk_from_tree_op() -> i64 {
    let n = 0
    while true { let a = 1
        n = n + 1
        match n { 3 => { let z = 0
            break }, _ => 0 } }
    n
}
fn brk_from_callee() -> i64 {
    let n = 0
    while true { let a = 1
        n = n + 1
        if n == 4 { stop() } }
    n
}
fn panics_in_loop() -> i64 {
    let n = 0
    while true { let a = 1
        { let b = 2
            let c = a / 0 } }
    n
}
fn returns_from_loop() -> i64 {
    for i in 0..10 { let a = i
        while true { let b = a
            { let c = b
                if c == 3 { return c * 10 } }
            break } }
    0
}
fn question_none() -> Option<i64> {
    let o = None
    { let a = 1
        let v = o?
        Some(v + a) }
}
fn break_outside_loop() -> i64 {
    let a = 1
    { let b = 2
        break }
}
fn mixed_scopes() -> i64 {
    let s = 0
    for i in 0..4 {
        let j = 0
        while j < 4 {
            j = j + 1
            if j == 2 { continue }
            if j == 4 { break }
            for k in 0..3 { if k == 1 { continue }
                s = s + k + j }
            for k in 0..3 { if k == 1 { break }
                s = s + 1 } }
        if i == 2 { break } }
    s
}
fn unscoped_return() -> i64 {
    let n = 3
    while true {
        if n > 1 { { return n * 2 } }
        break }
    0
}
fn unscoped_panic() -> i64 {
    let n = 0
    for i in 0..3 { while true { if i == 1 { n / 0 }
        break } }
    n
}
";

/// Run fn `f` of [`EXITS`] on both engines in a frame whose base scope binds
/// a sentinel; checks each frame is exactly as it was afterwards (one mark,
/// one binding), and that both engines agree. Returns the vm's result.
fn run_both(f: &str) -> R {
    let prog = crate::parse_source(EXITS).expect("parses");
    let interp = Interp::build(&prog).expect("unsealed build");
    let body = body_of(&interp, f);
    let sentinel = intern("sentinel");
    let frame = || {
        let mut env = interp.new_env();
        env.define(sentinel, Value::Int(-1), 0);
        env.push();
        env
    };
    let check = |env: &Env, engine: &str| {
        assert_eq!(env.depth(), 1, "{f} ({engine}): scopes left pushed");
        assert_eq!(env.len(), 1, "{f} ({engine}): bindings left behind");
    };
    let mut env = frame();
    let tree = match interp.eval(body, &mut env) {
        Err(Flow::Return(v)) => Ok(v),
        r => r,
    };
    check(&env, "tree");
    let mut env = frame();
    let vm = interp.exec(&compile(&interp, body), &mut env);
    check(&env, "vm");
    assert_eq!(format!("{vm:?}"), format!("{tree:?}"), "{f}");
    vm
}

#[test]
fn exec_catches_break_and_continue_in_the_innermost_loop() {
    assert!(matches!(run_both("brk_nested"), Ok(Value::Int(3))));
    assert!(matches!(run_both("cont_for"), Ok(Value::Int(8))));
    assert!(matches!(run_both("cont_while"), Ok(Value::Int(8))));
    assert!(matches!(run_both("brk_inner_only"), Ok(Value::Int(306))));
}

/// A `break` out of a tree op ends the loop it is in; a callee's `break`
/// never does (R51 S0b, veto 4e7851bc): it is an error at the call, and the
/// loop's scopes are still popped on the way out.
#[test]
fn exec_catches_break_out_of_a_tree_op_and_contains_a_callees() {
    assert!(matches!(run_both("brk_from_tree_op"), Ok(Value::Int(3))));
    let r = run_both("brk_from_callee");
    assert!(
        matches!(&r, Err(Flow::Panic(m)) if m.contains("outside a loop in `stop`")),
        "{r:?}"
    );
}

#[test]
fn exec_pops_every_pushed_scope_when_an_op_errs() {
    let r = run_both("panics_in_loop");
    assert!(matches!(&r, Err(Flow::Panic(_))), "{r:?}");
    // A `break` outside any loop of this body propagates unchanged.
    assert!(matches!(run_both("break_outside_loop"), Err(Flow::Break)));
}

#[test]
fn exec_exits_through_elided_scopes() {
    // Loops and blocks that bind nothing push no scope; every exit still
    // leaves the frame as it was, mixed with scoped ones around them.
    assert!(matches!(run_both("mixed_scopes"), Ok(Value::Int(42))));
    assert!(matches!(run_both("unscoped_return"), Ok(Value::Int(6))));
    let r = run_both("unscoped_panic");
    assert!(matches!(&r, Err(Flow::Panic(_))), "{r:?}");
}

#[test]
fn exec_turns_return_into_the_body_value_after_popping() {
    assert!(matches!(run_both("returns_from_loop"), Ok(Value::Int(30))));
    assert!(matches!(run_both("question_none"), Ok(Value::None)));
}

#[test]
fn exec_runs_hand_built_ops_with_balanced_scopes() {
    let prog = program();
    let interp = Interp::build(&prog).expect("unsealed build");
    let a = intern("a");
    let mut env = interp.new_env();
    env.define(a, Value::Int(1), 0);
    let shadow = Expr::Let {
        name: "a".into(),
        ty: None,
        value: Box::new(Expr::Literal(Literal::Int(99))),
    };
    let body = Body {
        ops: vec![
            Op::ScopePush,
            Op::Tree(&shadow),
            Op::Drop,
            Op::ScopePush,
            Op::Const(Value::Int(7)),
            Op::Return,
        ]
        .into_boxed_slice(),
        loops: Box::new([]),
        max_stack: 1,
        leaf: None,
        moves: None,
    };
    let r = interp.exec(&body, &mut env);
    assert!(matches!(r, Ok(Value::Int(7))), "{r:?}");
    assert_eq!(env.depth(), 0);
    assert!(matches!(env.get(a), Some(Value::Int(1))));
    let empty = Body {
        ops: Box::new([]),
        loops: Box::new([]),
        max_stack: 0,
        leaf: None,
        moves: None,
    };
    assert!(matches!(interp.exec(&empty, &mut env), Ok(Value::Unit)));
}

/// The vm's int fast path yields exactly `int_binop`'s `Ok` value, and
/// declines (`None`) exactly where `int_binop` panics or has no arm, so the
/// general path it falls back to produces the same panic.
#[test]
fn int_fast_agrees_with_int_binop() {
    use crate::ast::BinOp::*;
    let ops = [
        Add, Sub, Mul, Div, Rem, Eq, NotEq, Lt, Gt, LtEq, GtEq, And, Or, BitAnd, BitOr, BitXor,
        Shl, Shr,
    ];
    let vals = [
        i64::MIN,
        i64::MIN + 1,
        -65,
        -64,
        -7,
        -2,
        -1,
        0,
        1,
        2,
        3,
        7,
        63,
        64,
        65,
        i64::MAX - 1,
        i64::MAX,
    ];
    for op in &ops {
        for &a in &vals {
            for &b in &vals {
                let fast = int_fast(op, a, b).map(Scalar::value);
                match int_binop(op, a, b) {
                    Some(Ok(v)) => assert_eq!(
                        fast.as_ref().map(display),
                        Some(display(&v)),
                        "{op:?} {a} {b}"
                    ),
                    Some(Err(_)) | None => assert!(fast.is_none(), "{op:?} {a} {b}"),
                }
            }
        }
    }
}

/// The vm's float fast path yields exactly `float_binop`'s value, and
/// declines exactly where `float_binop` has no arm.
#[test]
fn float_fast_agrees_with_float_binop() {
    use crate::ast::BinOp::*;
    let ops = [
        Add, Sub, Mul, Div, Rem, Eq, NotEq, Lt, Gt, LtEq, GtEq, And, Or, BitAnd, BitOr, BitXor,
        Shl, Shr,
    ];
    let vals = [
        f64::NEG_INFINITY,
        f64::MIN,
        -1.5,
        -0.0,
        0.0,
        0.1,
        1.0,
        f64::MAX,
        f64::INFINITY,
        f64::NAN,
    ];
    for op in &ops {
        for &a in &vals {
            for &b in &vals {
                let fast = float_fast(op, a, b).map(Scalar::value);
                match float_binop(op, a, b) {
                    Some(Ok(v)) => assert_eq!(
                        fast.as_ref().map(display),
                        Some(display(&v)),
                        "{op:?} {a} {b}"
                    ),
                    Some(Err(_)) | None => assert!(fast.is_none(), "{op:?} {a} {b}"),
                }
            }
        }
    }
}

/// R50 S7: a [`pure::PureExpr`] yields exactly the scalar fast path's value
/// for every operator on two ints and two floats, `==`/`!=` on two bools
/// (`eval_binop_vals`), `&&`/`||` on bools, and declines (`None`) exactly
/// where the spec's rules say (no fast-path arm, mixed kinds, another
/// operator on bools, `&&`/`||` on a non-bool), on the first run (the tree
/// walk) and on later runs (the register code).
#[test]
fn pure_ops_agree_with_scalar_fast() {
    use crate::ast::BinOp::*;
    use pure::{Pure, PureExpr};
    let ops = [
        Add, Sub, Mul, Div, Rem, Eq, NotEq, Lt, Gt, LtEq, GtEq, And, Or, BitAnd, BitOr, BitXor,
        Shl, Shr,
    ];
    let ints = [i64::MIN, -7, -1, 0, 1, 2, 63, 64, i64::MAX];
    let floats = [f64::NEG_INFINITY, -1.5, -0.0, 0.0, 0.1, 1.0, f64::NAN];
    let mut leaves: Vec<(Pure<'static>, Scalar)> = Vec::new();
    for &n in &ints {
        leaves.push((Pure::Int(n), Scalar::Int(n)));
    }
    for &f in &floats {
        leaves.push((Pure::Float(f), Scalar::Float(f)));
    }
    for b in [false, true] {
        leaves.push((Pure::Bool(b), Scalar::Bool(b)));
    }
    let leaf = |s: Scalar| match s {
        Scalar::Int(n) => Pure::Int(n),
        Scalar::Float(f) => Pure::Float(f),
        Scalar::Bool(b) => Pure::Bool(b),
    };
    let show = |s: Option<Scalar>| s.map(|s| display(&s.value()));
    let env = Env::new();
    for op in &ops {
        for (_, a) in &leaves {
            for (_, b) in &leaves {
                let want = match (op, *a, *b) {
                    (And, Scalar::Bool(x), Scalar::Bool(y)) => Some(Scalar::Bool(x && y)),
                    (Or, Scalar::Bool(x), Scalar::Bool(y)) => Some(Scalar::Bool(x || y)),
                    (And | Or, Scalar::Bool(false), _) if matches!(op, And) => {
                        Some(Scalar::Bool(false))
                    }
                    (Or, Scalar::Bool(true), _) => Some(Scalar::Bool(true)),
                    (And | Or, _, _) => None,
                    (_, Scalar::Int(x), Scalar::Int(y)) => int_fast(op, x, y),
                    (_, Scalar::Float(x), Scalar::Float(y)) => float_fast(op, x, y),
                    (Eq, Scalar::Bool(x), Scalar::Bool(y)) => Some(Scalar::Bool(x == y)),
                    (NotEq, Scalar::Bool(x), Scalar::Bool(y)) => Some(Scalar::Bool(x != y)),
                    _ => None,
                };
                let k = Box::new([leaf(*a), leaf(*b)]);
                let tree = match op {
                    And => Pure::And(k),
                    Or => Pure::Or(k),
                    op => Pure::Bin(op.clone(), k),
                };
                let e = PureExpr::new(tree);
                let first = e.eval(&env);
                let later = e.eval(&env);
                assert_eq!(show(first), show(want), "first {op:?} {a:?} {b:?}");
                assert_eq!(show(later), show(want), "later {op:?} {a:?} {b:?}");
            }
        }
    }
}

/// R50 S7: register code specialised on a local's first kind declines when
/// the local later holds another kind, or a value that is not a plain
/// scalar, or is unbound.
#[test]
fn pure_rechecks_leaf_kinds() {
    use crate::ast::BinOp::*;
    use pure::{Pure, PureExpr};
    let name = String::from("a");
    let a = intern("a");
    let var = Var {
        s: a,
        slot: 0,
        name: &name,
    };
    // a * 2 + a
    let tree = Pure::Bin(
        Add,
        Box::new([
            Pure::Bin(Mul, Box::new([Pure::Local(var), Pure::Int(2)])),
            Pure::Local(var),
        ]),
    );
    let e = PureExpr::new(tree);
    let mut env = Env::new();
    env.define(a, Value::Int(5), 0);
    assert!(matches!(e.eval(&env), Some(Scalar::Int(15))), "first run");
    assert!(
        matches!(e.eval(&env), Some(Scalar::Int(15))),
        "register code"
    );
    *env.get_mut(a, 0).expect("bound") = Value::Float(1.5);
    assert!(e.eval(&env).is_none(), "float against int code");
    *env.get_mut(a, 0).expect("bound") = Value::Int(i64::MAX);
    assert!(e.eval(&env).is_none(), "overflow");
    *env.get_mut(a, 0).expect("bound") = Value::Str(Rc::new("x".to_string()));
    assert!(e.eval(&env).is_none(), "str");
    assert!(e.eval(&Env::new()).is_none(), "unbound");
}

// -- S5: fast calls ----------------------------------------------------------

/// R50 §4 S5 (§8 unit row): an `FnEntry` with `has_ref_mut` set is never a
/// fast-call candidate; a plain fn with no refinement in the program is.
#[test]
fn has_ref_mut_entry_is_not_fast_call_eligible() {
    let prog = crate::parse_source(
        "fn t(a: &mut [i64]) { a[0] = 1 }\nfn p(x: i64) -> i64 { x }\n\
         fn main() -> i64 { let a = [0]\n t(&mut a)\n p(1) }",
    )
    .expect("parses");
    let interp = Interp::build(&prog).expect("unsealed build");
    let entry = |name: &str| interp.fn_table.iter().find(|e| e.def.name == name).unwrap();
    assert!(entry("t").has_ref_mut);
    assert_eq!(
        compile::fast_call_blocker(entry("t"), true),
        Some("has_ref_mut")
    );
    assert_eq!(compile::fast_call_blocker(entry("p"), true), None);
    assert_eq!(
        compile::fast_call_blocker(entry("p"), false),
        Some("refine_preds")
    );
}

/// R50 §4 S5: a fast call keeps `call_fn_in`'s order: the depth check
/// before the arity check, each with the tree's panic. The body runs three
/// times: the first call goes through `dispatch_named` (which proves the
/// name), the next two through the fast path, the last one at the depth
/// limit.
#[test]
fn fast_call_checks_depth_before_arity() {
    let prog = crate::parse_source("fn f(x: i64) -> i64 { x }\nfn g() -> i64 { f(1, 2) }")
        .expect("parses");
    let interp = Interp::build(&prog).expect("unsealed build");
    let g = body_of(&interp, "g");
    let body = compile(&interp, g);
    assert!(kinds(&body).contains(&"call-fast"), "{:?}", kinds(&body));
    let both = || {
        let mut env = interp.new_env();
        env.push();
        let tree = format!("{:?}", interp.eval(g, &mut env));
        let mut env = interp.new_env();
        env.push();
        let vm = format!("{:?}", interp.exec(&body, &mut env));
        assert_eq!(vm, tree);
        vm
    };
    for _ in 0..2 {
        assert!(both().contains("f: expected 1 args, got 2"));
    }
    let depth = interp.call_depth.replace(interp.max_depth);
    let at_limit = both();
    interp.call_depth.set(depth);
    assert!(at_limit.contains("recursion limit exceeded"), "{at_limit}");
}

/// R51 S0b (§3 row 5, veto 4e7851bc/2175cc1b): a callee's `break`/`continue`
/// is an error at the call on the VM's fast paths exactly as on the tree,
/// never a jump in the caller's loop. Each caller loops, so from its second
/// iteration the call runs proven: `CallFastLocalInt` (frameless leaf arm
/// declined, and taken before the escaping `else`), and an inline-argument
/// `CallFast`.
#[test]
fn loop_control_never_leaves_a_fast_call() {
    let src = "fn stop(n: i64) -> i64 {\n if n > 3 { break }\n n\n}\n\
               fn leaf(n: i64) -> i64 {\n if n < 4 { n } else { break }\n}\n\
               fn skip(n: i64) -> i64 {\n if n > 2 { continue }\n n\n}\n\
               fn a() -> i64 {\n let s = 0\n let i = 0\n while i < 6 {\n s = s + stop(i + 1)\n i = i + 1\n }\n s\n}\n\
               fn b() -> i64 {\n let s = 0\n let i = 0\n while i < 6 {\n s = s + leaf(i + 1)\n i = i + 1\n }\n s\n}\n\
               fn c() -> i64 {\n let s = 0\n for i in 0..6 {\n s = s + skip(i)\n }\n s\n}\n";
    let prog = crate::parse_source(src).expect("parses");
    let interp = Interp::build(&prog).expect("unsealed build");
    for (f, op, msg) in [
        ("a", "call-fast-local-int", "outside a loop in `stop`"),
        ("b", "call-fast-local-int", "outside a loop in `leaf`"),
        ("c", "call-fast-inline", "outside a loop in `skip`"),
    ] {
        let body = compile(&interp, body_of(&interp, f));
        assert!(kinds(&body).contains(&op), "{f}: {:?}", kinds(&body));
        let mut env = interp.new_env();
        env.push();
        let tree = format!("{:?}", interp.eval(body_of(&interp, f), &mut env));
        let mut env = interp.new_env();
        env.push();
        let vm = format!("{:?}", interp.exec(&body, &mut env));
        assert_eq!(vm, tree, "{f}");
        assert!(vm.contains("Panic") && vm.contains(msg), "{f}: {vm}");
    }
}

// ── R51 T1: the sealed-engine pin (spec §4 Fork 1, §8 T1) ───────────────────

/// R51 T1a: [`engine_for`] over every input, 48 rows. A sealed run is
/// refused while the runtime seal is incomplete, refused when the VM is
/// requested or `AXON_ENGINE` is invalid, and otherwise runs on the tree,
/// `AXON_ENGINE` unset included; it never gets the VM. An unsealed run's
/// selection is R50 §3's, whatever `complete` says. `complete` is the
/// const `false` on this build, so `complete = true` is reached only here.
#[test]
fn r51_t1a_engine_for_truth_table() {
    use std::ffi::OsStr;
    use Engine::{Tree, Vm};
    use EngineRefusal::{SealIncomplete, SealedVm};
    // The inputs are exactly these four: `AXON_VM_EAGER` (and the trace)
    // is not one, by the signature.
    type Selection = Result<Engine, EngineRefusal>;
    let _: fn(bool, bool, Option<Engine>, Option<&OsStr>) -> Selection = engine_for;
    let invalid = || EngineRefusal::Invalid(Engine::parse("garbage").unwrap_err());
    const ENVS: [Option<&str>; 4] = [None, Some("tree"), Some("vm"), Some("garbage")];
    const OVERRIDES: [Option<Engine>; 3] = [None, Some(Tree), Some(Vm)];
    // (override, env) -> selection, sealed with the seal complete.
    let sealed_complete: [[Result<Engine, EngineRefusal>; 4]; 3] = [
        [Ok(Tree), Ok(Tree), Err(SealedVm), Err(invalid())],
        [Ok(Tree), Ok(Tree), Err(SealedVm), Err(invalid())],
        [Err(SealedVm), Err(SealedVm), Err(SealedVm), Err(SealedVm)],
    ];
    // (override, env) -> selection, unsealed (R50 §3: override, else env,
    // else the default VM).
    let unsealed: [[Result<Engine, EngineRefusal>; 4]; 3] = [
        [Ok(Vm), Ok(Tree), Ok(Vm), Err(invalid())],
        [Ok(Tree), Ok(Tree), Ok(Tree), Ok(Tree)],
        [Ok(Vm), Ok(Vm), Ok(Vm), Ok(Vm)],
    ];
    let mut rows = 0;
    for sealed in [false, true] {
        for complete in [false, true] {
            for (o, override_) in OVERRIDES.into_iter().enumerate() {
                for (e, env) in ENVS.into_iter().enumerate() {
                    let got = engine_for(sealed, complete, override_, env.map(OsStr::new));
                    let want = match (sealed, complete) {
                        (false, _) => unsealed[o][e].clone(),
                        (true, false) => Err(SealIncomplete),
                        (true, true) => sealed_complete[o][e].clone(),
                    };
                    let at = format!(
                        "sealed={sealed} complete={complete} override={override_:?} env={env:?}"
                    );
                    assert_eq!(got, want, "{at}");
                    if sealed {
                        assert!(
                            got.is_err() || got == Ok(Tree),
                            "{at}: a sealed run got {got:?}"
                        );
                    }
                    rows += 1;
                }
            }
        }
    }
    assert_eq!(rows, 48);
    // The refusals print the spec §3 messages.
    assert_eq!(SealIncomplete.to_string(), SEALED_RUN_REFUSED);
    assert_eq!(
        SealedVm.to_string(),
        "a sealed run executes on the tree engine only (AXON_ENGINE=vm cannot carry the seal); unset AXON_ENGINE"
    );
    assert_eq!(
        invalid().to_string(),
        "AXON_ENGINE must be \"vm\" or \"tree\" (got \"garbage\")"
    );
    // Nor does the live selection read `AXON_VM_EAGER`.
    let src = r51_source("vm/mod.rs");
    for f in ["fn engine_for(", "fn engine_at_build("] {
        let start = src.find(f).unwrap_or_else(|| panic!("{f} in vm/mod.rs"));
        let body = &src[start..start + src[start..].find("\n}\n").expect("fn end")];
        assert!(
            !body.contains("EAGER") && !body.contains("eager"),
            "{f} reads the eager flag"
        );
    }
}

/// The T1b program: every VM path a body can take, so the unsealed control
/// shows each cell can fill. `hot` holds a `while` (S9 hot on entry);
/// `twice` runs three times (second-entry compile); `fib` is a one-param int
/// recursion (`CallFastLocalInt`, `leaf_arm`); `add` a two-param leaf
/// (`leaf_call`); `swap` a `&mut [i64]` helper (`MovesCall`); `arr_map` and
/// `arr_fold` run lambdas (`run_lambda`, `fold_leaf`).
const R51_T1B_SRC: &str = "\
fn hot(n: i64) -> i64 {
    let s = 0
    let i = 0
    while i < n {
        s = s + i
        i = i + 1
    }
    s
}
fn twice(x: i64) -> i64 { x * 2 }
fn fib(n: i64) -> i64 {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}
fn add(a: i64, b: i64) -> i64 { a + b }
fn swap(a: &mut [i64], i: i64, j: i64) {
    let t = a[i]
    a[i] = a[j]
    a[j] = t
}
fn t() -> i64 {
    let xs = [1, 2, 3, 4]
    swap(&mut xs, 0, 3)
    let ys = arr_map(xs, |x: i64| x + 1)
    let total = arr_fold(ys, 0, |acc: i64, x: i64| acc + x)
    hot(10) + twice(1) + twice(2) + twice(3) + fib(10) + add(1, 2) + total + xs[0]
}
";

/// Run `t` of [`R51_T1B_SRC`] on an `Interp` built with `sealed` and
/// `engine`, eager (every body compiles on its first entry under the VM).
/// Returns `t`'s value and the names of the fn-table bodies and the number
/// of lambda bodies that were compiled.
fn r51_t1b_run(
    prog: &crate::ast::Program,
    sealed: bool,
    engine: Engine,
) -> (i64, Vec<String>, usize) {
    // A sealed run is a non-empty sealed set; this one seals no file of the
    // program, so only the bit (and the engine pin) differ from the control.
    let dirs = if sealed {
        vec![std::path::PathBuf::from("/r51-t1b-sealed")]
    } else {
        Vec::new()
    };
    let mut interp = Interp::build_with(prog, &dirs, engine);
    assert_eq!((interp.seal.active, interp.engine), (sealed, engine));
    interp.vm_eager = true;
    interp.vm_trace = false;
    interp.init_globals().expect("no globals");
    let got = interp.call_fn(interp.fns["t"], vec![]).expect("t runs");
    let Value::Int(n) = got else {
        panic!("t gave {got:?}")
    };
    let compiled = interp
        .fn_table
        .iter()
        .filter(|e| e.compiled.as_ref().is_some_and(|c| c.get().is_some()))
        .map(|e| e.def.name.clone())
        .collect();
    let lambdas = interp
        .res
        .lambda_codes()
        .filter(|c| {
            c.compiled
                .as_ref()
                .is_some_and(|lb| lb.compiled().is_some())
        })
        .count();
    (n, compiled, lambdas)
}

/// R51 T1b: a sealed run (seal complete, the engine `engine_for` picks for
/// the production shape: no override, `AXON_ENGINE` unset) runs on the tree
/// and compiles nothing, not one fn-table body nor one lambda body, even
/// eager. Control: the same program unsealed on the VM fills every one of
/// those cells. The seal state is a build parameter here
/// (`Interp::build_with`), so no other test can observe it.
/// `r51_compile_refuses_a_sealed_run` is the other half: `compile` itself
/// panics under seal.
#[test]
fn r51_t1b_a_sealed_run_compiles_nothing() {
    let prog = crate::parse_source(R51_T1B_SRC).expect("parses");
    let (want, vm_bodies, vm_lambdas) = r51_t1b_run(&prog, false, Engine::Vm);
    let mut vm_bodies = vm_bodies;
    vm_bodies.sort();
    assert_eq!(
        vm_bodies,
        ["add", "fib", "hot", "swap", "t", "twice"],
        "control"
    );
    assert_eq!(vm_lambdas, 2, "control: both lambdas compiled");
    assert_eq!(want, 45 + 2 + 4 + 6 + 55 + 3 + 14 + 4);

    let engine = engine_for(true, true, None, None).expect("a sealed run is allowed");
    assert_eq!(engine, Engine::Tree);
    let (got, bodies, lambdas) = r51_t1b_run(&prog, true, engine);
    assert_eq!(got, want);
    assert!(bodies.is_empty(), "sealed run compiled {bodies:?}");
    assert_eq!(lambdas, 0, "sealed run compiled a lambda body");
}

/// The source of `interp/<rel>` (`interp.rs` for `""`).
fn r51_source(rel: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let p = if rel.is_empty() {
        root.join("interp.rs")
    } else {
        root.join("interp").join(rel)
    };
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Every interpreter source the T1d sweep reads: `interp.rs` and every
/// `interp/**/*.rs` (recursive, so `interp/vm/` and `interp/sym.rs` are in),
/// except `vm/tests.rs` and the `tests_v022_*.rs` files `include!`d into
/// `interp.rs`'s `mod tests` (test code, like the cut below); each cut at its
/// first `mod tests {`. Keyed by the path under `interp/` (`""` for
/// `interp.rs`). Shared with the R51 S5 builtin sweep in `interp::tests`.
pub(in crate::interp) fn r51_interp_sources() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/interp");
    let mut rels = vec![String::new()];
    let mut stack = vec![dir.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).expect("read interp dir") {
            let p = e.expect("dir entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let rel = p
                    .strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if rel != "vm/tests.rs" && !rel.starts_with("tests_v022_") {
                    rels.push(rel);
                }
            }
        }
    }
    rels.sort();
    let sources: Vec<(String, String)> = rels
        .into_iter()
        .map(|rel| {
            let mut src = r51_source(&rel);
            if let Some(cut) = src.find("mod tests {") {
                src.truncate(cut);
            }
            (rel, src)
        })
        .collect();
    for want in [
        "",
        "eval.rs",
        "builtins.rs",
        "sym.rs",
        "vm/mod.rs",
        "vm/compile.rs",
        "vm/pure.rs",
    ] {
        assert!(
            sources.iter().any(|(r, _)| r == want),
            "the sweep reads interp/{want}"
        );
    }
    sources
}

fn r51_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// One code site the T1d sweep matched: its file, the fn it is in (for a
/// `call_builtin` arm, `builtins <arm>`), and the text just before the match
/// and just after it on that line.
struct R51Site<'s> {
    file: &'s str,
    func: String,
    before: &'s str,
    after: &'s str,
}

impl R51Site<'_> {
    fn at(&self) -> String {
        if self.file.is_empty() {
            format!("interp.rs::{}", self.func)
        } else {
            format!("interp/{}::{}", self.file, self.func)
        }
    }
}

/// Every occurrence of `needle` in the code (not `//` comments) of
/// `sources`, with the fn it is in: the last `fn <name>` above it.
fn r51_sites<'s>(sources: &'s [(String, String)], needle: &str) -> Vec<R51Site<'s>> {
    let mut out = Vec::new();
    for (file, src) in sources {
        let mut func = String::new();
        let mut arm = String::new();
        for line in src.lines() {
            let code = line.find("//").map_or(line, |i| &line[..i]);
            let bytes = code.as_bytes();
            let mut from = 0;
            while let Some(i) = code[from..].find("fn ") {
                let at = from + i;
                from = at + 3;
                if at > 0 && r51_ident_char(bytes[at - 1]) {
                    continue;
                }
                let name: String = code[at + 3..]
                    .bytes()
                    .take_while(|&c| r51_ident_char(c))
                    .map(char::from)
                    .collect();
                if !name.is_empty() {
                    func = name;
                    arm.clear();
                }
            }
            let t = code.trim_start();
            if func == "call_builtin" && t.starts_with('"') && t.contains("=>") {
                arm = t[1..].split('"').next().unwrap_or("").to_string();
            }
            let mut from = 0;
            while let Some(i) = code[from..].find(needle) {
                let at = from + i;
                from = at + needle.len();
                out.push(R51Site {
                    file,
                    func: if arm.is_empty() {
                        func.clone()
                    } else {
                        format!("builtins {arm}")
                    },
                    before: &code[..at],
                    after: &code[at + needle.len()..],
                });
            }
        }
    }
    out
}

/// The sorted, de-duplicated `at()` of `sites`.
fn r51_fns(sites: &[&R51Site<'_>]) -> Vec<String> {
    let mut v: Vec<String> = sites.iter().map(|s| s.at()).collect();
    v.sort();
    v.dedup();
    v
}

/// R51 T1d (1, 2): the `engine` field is read only at the four engine
/// gates (`run_test_body` is S0b's `@[test]`-frame twin of `run_body`) and
/// the trace, and written after build only by `vm/tests.rs`.
#[test]
fn r51_t1d_engine_is_read_only_at_the_gates() {
    let sources = r51_interp_sources();
    let sites: Vec<_> = r51_sites(&sources, ".engine")
        .into_iter()
        .filter(|s| !s.after.bytes().next().is_some_and(r51_ident_char))
        .collect();
    let (writes, reads): (Vec<_>, Vec<_>) = sites.iter().partition(|s| {
        let a = s.after.trim_start();
        a.starts_with('=') && !a.starts_with("==")
    });
    assert!(
        reads.len() >= 4,
        "the sweep found {} `.engine` reads",
        reads.len()
    );
    assert_eq!(
        r51_fns(&reads),
        [
            "interp/vm/mod.rs::run_body",
            "interp/vm/mod.rs::run_lambda",
            "interp/vm/mod.rs::run_test_body",
            "interp/vm/mod.rs::vm_trace_tree",
            "interp/vm/pure.rs::fold_leaf",
        ],
        "DRIFT: a new reader of `Interp::engine` is a new engine gate; a sealed run must not reach the VM through it"
    );
    assert!(
        writes.is_empty(),
        "DRIFT: `.engine` written after build at {:?}; only vm/tests.rs may",
        r51_fns(&writes)
    );
    let tests = [("vm/tests.rs".to_string(), r51_source("vm/tests.rs"))];
    let test_writes = r51_sites(&tests, ".engine = ").len();
    assert!(
        test_writes >= 2,
        "vm/tests.rs writes `.engine` {test_writes} times"
    );
}

/// R51 T1d (3): `vm::compile::compile`, the only constructor of a `Body`,
/// is called only from `compile_entry` and `compile_lambda`, and refuses a
/// sealed run.
#[test]
fn r51_t1d_compile_has_two_callers() {
    let sources = r51_interp_sources();
    // In vm/ every `compile(` is the VM's (regex.rs has a compiler of its
    // own); elsewhere the VM's is reachable only by path.
    let calls: Vec<_> = r51_sites(&sources, "compile(")
        .into_iter()
        .filter(|s| {
            let prev = s.before.bytes().last();
            !prev.is_some_and(r51_ident_char) && !s.before.trim_end().ends_with("fn")
        })
        .filter(|s| {
            s.file.starts_with("vm/")
                || s.before.ends_with("vm::")
                || s.before.ends_with("compile::")
        })
        .collect();
    let calls: Vec<&R51Site<'_>> = calls.iter().collect();
    assert!(
        calls.len() >= 2,
        "the sweep found {} `compile(` calls",
        calls.len()
    );
    assert_eq!(
        r51_fns(&calls),
        [
            "interp/vm/mod.rs::compile_entry",
            "interp/vm/mod.rs::compile_lambda"
        ],
        "DRIFT: a new caller of vm::compile::compile builds a VM body outside the two gated paths"
    );
    let src = r51_source("vm/compile.rs");
    let start = src.find("fn compile<").expect("fn compile");
    let body = &src[start..start + src[start..].find("\n}\n").expect("fn end")];
    assert!(
        body.contains("!ix.seal.active"),
        "compile refuses a sealed run"
    );
    assert_eq!(
        src.matches("Body {").count(),
        1,
        "compile is the only `Body` constructor"
    );
}

/// R51 T1d (4): outside `interp/vm/`, the VM is entered only from the fn
/// call paths, the closure call and `arr_fold`'s fold leaf.
#[test]
fn r51_t1d_vm_entry_callers_outside_vm() {
    const ENTRIES: [&str; 20] = [
        "run_body",
        "run_body_vm",
        "run_test_body",
        "run_lambda",
        "fold_leaf",
        "exec",
        "exec_raw",
        "exec_on",
        "fast_body",
        "call_fast",
        "leaf_call",
        "leaf_arm",
        "leaf_value",
        "leaf_store",
        "run_moves",
        "moves_call",
        "call_mut_op",
        "call_mut_general",
        "compile",
        "compile_body",
    ];
    let is_entry = |name: &str| {
        ENTRIES.contains(&name) || name.starts_with("call_fast_") || name.starts_with("compile_")
    };
    let sources: Vec<_> = r51_interp_sources()
        .into_iter()
        .filter(|(rel, _)| !rel.starts_with("vm/"))
        .collect();
    let sites = r51_sites(&sources, "(");
    let calls: Vec<&R51Site<'_>> = sites
        .iter()
        .filter(|s| {
            let b = s.before.as_bytes();
            let start = b
                .iter()
                .rposition(|&c| !r51_ident_char(c))
                .map_or(0, |i| i + 1);
            let name = &s.before[start..];
            if !is_entry(name) || s.before[..start].trim_end().ends_with("fn") {
                return false;
            }
            // regex.rs's `compile`/`self.compile(` are its own pattern
            // compiler's, which holds no `Interp`.
            if s.file == "regex.rs" && name.starts_with("compile") {
                return false;
            }
            let head = &s.before[..start];
            // `h.exec(` is the host's, `regex::compile(` the regex
            // compiler's: a method on another receiver or a path into
            // another module is not the VM.
            if let Some(recv) = head.strip_suffix('.') {
                let r = recv.as_bytes();
                let rs = r
                    .iter()
                    .rposition(|&c| !r51_ident_char(c))
                    .map_or(0, |i| i + 1);
                return matches!(&recv[rs..], "self" | "ix" | "interp");
            }
            if let Some(path) = head.strip_suffix("::") {
                return path.ends_with("vm") || path.ends_with("compile");
            }
            true
        })
        .collect();
    assert!(
        calls.len() >= 4,
        "the sweep found {} VM entry calls",
        calls.len()
    );
    assert_eq!(
        r51_fns(&calls),
        [
            "interp.rs::call_closure_frame",
            "interp.rs::call_fn_common",
            "interp.rs::call_fn_frame",
            "interp/builtins.rs::builtins arr_fold",
        ],
        "DRIFT: a fn enters the VM outside the sealed-engine gates; route it through run_body/run_lambda or list it with its reason"
    );
}
