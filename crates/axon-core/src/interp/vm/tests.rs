//! R50 unit tests (spec §8 unit row): the S1 compiler's lowered set, the
//! `ScopePush`/`ScopePop` placement of every scoped construct, the
//! `FnEntry::compiled` discriminator, and the op loop's scope discipline on
//! every exit path (normal, caught `break`/`continue`, propagated error,
//! `return`), each checked against the tree-walker.

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
            Op::StoreBin { .. } | Op::StoreLocalInt { .. } | Op::StoreLocalLocal { .. } => {
                "store-bin"
            }
            Op::Bin { .. } => "bin",
            Op::ShortCircuit { .. } => "short",
            Op::Logic(_) => "logic",
            Op::Unary(_) => "unary",
            Op::Jump(_) => "jump",
            Op::PopJump(_) => "pop-jump",
            Op::BranchFalse { .. } => "branch",
            Op::BranchCmp { .. } | Op::BranchLocalInt { .. } | Op::BranchLocalLocal { .. } => {
                "branch-cmp"
            }
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
            Op::IndexTest(_) => "index-test",
            Op::PlaceIndex => "place-index",
            Op::WritePlace { .. } | Op::WriteIndexLocal { .. } => "write-place",
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
        })
        .collect()
}

#[test]
fn s1_compiles_fib_without_tree_ops() {
    let prog = program();
    let interp = Interp::build(&prog);
    let body = compile(&interp.res, body_of(&interp, "fib"));
    assert_eq!(body.tree_nodes(), 0, "{:?}", kinds(&body));
}

#[test]
fn s1_leaves_only_unlowered_variants_on_the_tree() {
    let prog = program();
    let interp = Interp::build(&prog);
    let body = compile(&interp.res, body_of(&interp, "main"));
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
    let interp = Interp::build(&prog);
    let mut seen = std::collections::HashSet::new();
    for item in &prog.items {
        let crate::ast::Item::FnDef(f) = item else {
            continue;
        };
        crate::ast::walk_expr(&f.body, &mut |e| {
            let body = compile(&interp.res, e);
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
    let interp = Interp::build(&prog);
    let body = compile(&interp.res, body_of(&interp, "f"));
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

// ── Scope placement: a push/pop exactly where `eval` has one ────────────────

/// The op kinds of fn `f`'s body in `src`.
fn kinds_of(src: &str, f: &str) -> Vec<&'static str> {
    let prog = crate::parse_source(src).expect("parses");
    let interp = Interp::build(&prog);
    kinds(&compile(&interp.res, body_of(&interp, f)))
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
            "define", // let n = 0
            "load",
            "call",
            "while-let+scope+body",
            "load",
            "define",
            "store-bin",
            "while-let-next",
            "load",
            "call",
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
    let interp = Interp::build(&prog);
    let body = compile(&interp.res, body_of(&interp, "f"));
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
    let interp = Interp::build(&prog);
    let body = compile(&interp.res, body_of(&interp, "f"));
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
    let interp = Interp::build(&prog);
    let body = body_of(&interp, f);
    let sentinel = intern("sentinel");
    let frame = || {
        let mut env = Env::new();
        env.define(sentinel, Value::Int(-1));
        env.push();
        env
    };
    let check = |env: &Env, engine: &str| {
        assert_eq!(env.marks.len(), 1, "{f} ({engine}): scopes left pushed");
        assert_eq!(env.vars.len(), 1, "{f} ({engine}): bindings left behind");
    };
    let mut env = frame();
    let tree = match interp.eval(body, &mut env) {
        Err(Flow::Return(v)) => Ok(v),
        r => r,
    };
    check(&env, "tree");
    let mut env = frame();
    let vm = interp.exec(&compile(&interp.res, body), &mut env);
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

#[test]
fn exec_catches_break_out_of_a_tree_op_and_a_callee() {
    assert!(matches!(run_both("brk_from_tree_op"), Ok(Value::Int(3))));
    assert!(matches!(run_both("brk_from_callee"), Ok(Value::Int(4))));
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
    let interp = Interp::build(&prog);
    let a = intern("a");
    let mut env = Env::new();
    env.define(a, Value::Int(1));
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
    };
    let r = interp.exec(&body, &mut env);
    assert!(matches!(r, Ok(Value::Int(7))), "{r:?}");
    assert_eq!(env.marks.len(), 0);
    assert!(matches!(env.get(a), Some(Value::Int(1))));
    let empty = Body {
        ops: Box::new([]),
        loops: Box::new([]),
        max_stack: 0,
        leaf: None,
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
    env.define(a, Value::Int(5));
    assert!(matches!(e.eval(&env), Some(Scalar::Int(15))), "first run");
    assert!(
        matches!(e.eval(&env), Some(Scalar::Int(15))),
        "register code"
    );
    *env.get_mut(a).expect("bound") = Value::Float(1.5);
    assert!(e.eval(&env).is_none(), "float against int code");
    *env.get_mut(a).expect("bound") = Value::Int(i64::MAX);
    assert!(e.eval(&env).is_none(), "overflow");
    *env.get_mut(a).expect("bound") = Value::Str(Rc::new("x".to_string()));
    assert!(e.eval(&env).is_none(), "str");
    assert!(e.eval(&Env::new()).is_none(), "unbound");
}
