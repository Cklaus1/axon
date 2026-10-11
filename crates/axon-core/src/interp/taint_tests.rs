//! R51 S2: the taint channel's hooks, unit by unit (spec §8 T8.2-T8.5, the
//! subset that needs no live sealed run).
//!
//! Every sealed run is refused at build until S6 (`SEAL_RUNTIME_COMPLETE`), so
//! these drive the evaluator's sealed instantiation (`eval_t::<true>`) directly
//! on an `Interp` made by [`Interp::build_with`], which computes the seal state
//! without the refuse-all pin (as the Env factory tests in `interp/env.rs` do).
//! Each probe is an OPERATOR fn of the suite run in an operator frame, its
//! parameters bound with the taint the test gives them; the test reads the
//! taint of the probe's value (`taint.last`), of a returned shared object
//! (`t_obj`) or of the accumulator. A hook removed makes its test fail (the
//! S2 mutation table).

use super::taint::{ALL, TYP, VAL};
use super::*;

/// The operator's probes. `c` is the candidate's value (bound with taint
/// `ALL`), `k` and `n` the operator's own (bound clean).
const SUITE: &str = "\
type Pos = i64 where _ > 0
fn plain(c: i64, k: i64) -> i64 {
    let a = c
    let b = k
    b
}
fn plain_a(c: i64, k: i64) -> i64 {
    let a = c
    let b = k
    a
}
fn stmt_then_clean(c: i64, k: i64) -> i64 {
    let a = c
    k
}
fn assign_tainted(c: i64, k: i64) -> i64 {
    let x = k
    x = c
    x
}
fn assign_clean(c: i64, k: i64) -> i64 {
    let x = c
    x = k
    x
}
fn push_in_place(c: i64, k: i64) -> [i64] {
    let xs = [k]
    xs = arr_push(xs, c)
    xs
}
fn concat_in_place(c: str, k: str) -> str {
    let s = k
    s = s + c
    s
}
fn place_write(c: i64, k: i64) -> [i64] {
    let xs = [k, k]
    xs[0] = c
    xs
}
fn place_index(c: i64, k: i64) -> [i64] {
    let xs = [k, k]
    xs[c] = k
    xs
}
fn add(c: i64, k: i64) -> i64 { c + k }
fn compare(c: i64, k: i64) -> bool { c > k }
fn negate(c: bool, k: bool) -> bool { !c }
fn fmt(c: i64, k: i64) -> str { \"v={c}\" }
fn under_if(c: i64, k: i64) -> i64 {
    let x = k
    if c > 0 { x = 1 }
    x
}
fn under_else(c: i64, k: i64) -> i64 {
    let x = k
    if c > 0 { x = 1 } else { x = 2 }
    x
}
fn under_match(c: i64, k: i64) -> i64 {
    let x = k
    match c {
        1 => { x = 1 }
        _ => { x = 2 }
    }
    x
}
fn under_guard(c: i64, k: i64) -> i64 {
    let x = k
    match k {
        0 if c > 5 => { x = 1 }
        _ => { x = 2 }
    }
    x
}
fn under_while(c: i64, k: i64) -> i64 {
    let x = k
    let i = 0
    while i < c {
        x = 1
        i = i + 1
    }
    x
}
fn under_for(c: i64, k: i64) -> i64 {
    let x = k
    for i in 0..c { x = 1 }
    x
}
fn under_and(c: i64, k: i64) -> i64 {
    let x = k
    let ok = c > 0 && { x = 1
        true }
    x
}
fn after_break(c: i64, k: i64) -> i64 {
    let x = k
    let i = 0
    while i < 3 {
        if c > 0 { break }
        i = i + 1
    }
    x = 1
    x
}
fn after_question(c: Result<i64, str>, k: i64) -> Result<i64, str> {
    let v = c?
    let x = k
    x = 1
    Ok(x)
}
fn returns(c: i64, k: i64) -> i64 {
    if k > 0 { return c }
    k
}
fn bind_pattern(c: Option<i64>, k: i64) -> i64 {
    match c {
        Some(n) => n
        None => k
    }
}
fn while_let(c: Option<i64>, k: i64) -> i64 {
    let o = c
    let x = k
    while let Some(n) = o {
        x = n
        o = None
    }
    x
}
fn send(c: i64, k: i64) -> Chan<i64> {
    let ch = chan<i64>()
    ch.send(c)
    ch
}
fn send_clean(c: i64, k: i64) -> Chan<i64> {
    let ch = chan<i64>()
    ch.send(k)
    ch
}
fn dict_write(c: i64, k: i64) -> Dict {
    let d = dict_new()
    dict_set(d, \"k\", c)
    d
}
fn dict_write_clean(c: i64, k: i64) -> Dict {
    let d = dict_new()
    dict_set(d, \"k\", k)
    d
}
fn dict_compare(c: Dict, k: Dict) -> bool { c == k }
fn dict_read(c: Dict, k: i64) -> Option<i64> { dict_get(c, \"k\") }
fn clock(c: i64, k: i64) -> i64 { now_ms() }
fn if_typ(c: bool, k: i64) -> i64 { if c { k } else { k } }
fn match_typ(c: bool, k: i64) -> i64 {
    match c {
        _ => k
    }
}
fn index_read(c: [i64], k: i64) -> i64 { c[0] }
fn cond_put(d: Dict, i: i64, c: i64) -> bool {
    dict_set(d, \"k\", 1)
    i < c
}
fn while_seen(c: i64, k: i64) -> Dict {
    let d = dict_new()
    let i = 0
    while cond_put(d, i, c) { i = i + 1 }
    d
}
fn opt_put(d: Dict, o: Option<i64>) -> Option<i64> {
    dict_set(d, \"k\", 1)
    o
}
fn while_let_seen(c: Option<i64>, k: i64) -> Dict {
    let d = dict_new()
    let o = c
    while let Some(n) = opt_put(d, o) { o = None }
    d
}
fn while_let_store(c: Option<i64>, k: i64) -> i64 {
    let o = c
    let x = k
    while let Some(n) = o {
        x = 1
        o = None
    }
    x
}
fn question_err(c: Result<i64, str>, k: i64) -> Result<i64, str> {
    let v = c?
    Ok(k)
}
fn select_store(c: i64, k: i64) -> i64 {
    let ch = chan<i64>()
    ch.send(c)
    let x = k
    select { ch.recv() => x = 1 }
    x
}
fn callback_store(c: [i64], k: i64) -> Dict {
    let d = dict_new()
    let ys = arr_map(c, |v| { dict_set(d, \"k\", 1)
        v })
    d
}
fn native(c: i64, k: i64) -> i64 {
    let w = gfx::window_open(c, 64, \"t\")
    k
}
fn abortable(c: i64, k: i64) -> i64 {
    let x = k
    with handler { on IO(p) => 0 } {
        if c > 0 { x = 2 }
    }
    x = 1
    x
}
fn enters_sealed(c: i64, k: i64) -> i64 {
    let x = k
    with handler { on IO(p) => 0 } {
        let s = solve()
    }
    x = 1
    x
}
fn payload(c: str, k: i64) -> Dict {
    let d = dict_new()
    with handler { on IO(p) => resume(dict_set(d, \"k\", p)) } {
        println(c)
    }
    d
}
fn callee_id(x: i64) -> i64 {
    x
}
fn callee_ret(x: i64) -> i64 {
    return x
}
fn callee_sticky(x: i64) -> i64 {
    if x > 0 { return 1 }
    2
}
fn call_arg(c: i64, k: i64) -> i64 {
    callee_id(c)
}
fn call_arg_clean(c: i64, k: i64) -> i64 {
    let a = callee_id(c)
    callee_id(k)
}
fn call_ret(c: i64, k: i64) -> i64 {
    callee_ret(c)
}
fn after_call(c: i64, k: i64) -> i64 {
    let r = callee_sticky(c)
    let y = k
    y = 3
    y
}
fn capture_lend(c: i64, k: i64) -> i64 {
    let x = c
    let f = || x
    f()
}
fn capture_copy(c: i64, k: i64) -> i64 {
    let x = c
    let f = || x
    let g = f
    g()
}
fn capture_write_lend(c: i64, k: i64) -> i64 {
    let x = k
    let f = |v| { let o = x
        x = v
        o }
    let a = f(c)
    f(k)
}
fn capture_write_copy(c: i64, k: i64) -> i64 {
    let x = k
    let f = |v| { let o = x
        x = v
        o }
    let g = f
    let a = g(c)
    g(k)
}
fn closure_twice(c: i64, k: i64) -> i64 {
    let f = |v| v
    let a = f(c)
    f(k)
}
fn store_into(d: Dict, x: i64) -> i64 {
    dict_set(d, \"k\", x)
    0
}
fn call_store(c: i64, k: i64) -> Dict {
    let d = dict_new()
    let r = store_into(d, c)
    d
}
fn capture_ret(c: i64, k: i64) -> i64 {
    let x = c
    let f = || { if x > 0 { return 1 }
        2 }
    f()
}
";

const CAND: &str = "fn solve() -> i64 { 41 }\n";

/// The suite and a sealed candidate, as one program, and the sealed directory.
fn prog(tag: &str) -> (Program, std::path::PathBuf) {
    use crate::span::intern_source;
    let sdir = format!("/{tag}-sealed");
    let s = crate::parse_source_in(SUITE, intern_source(&format!("/{tag}-suite/h.ax"), SUITE))
        .expect("suite parses");
    let c = crate::parse_source_in(CAND, intern_source(&format!("{sdir}/f.ax"), CAND))
        .expect("candidate parses");
    let prog = Program {
        items: s.items.into_iter().chain(c.items).collect(),
    };
    (prog, std::path::PathBuf::from(sdir))
}

fn clear(ix: &Interp<'_>) {
    let t = &ix.taint;
    for c in [&t.acc, &t.last, &t.pc, &t.sticky, &t.ret] {
        c.set(0);
    }
}

/// Run the operator fn `name` on `args` (value, taint) in an operator frame
/// of the sealed instantiation; its value (or the value it returned) and the
/// taint of that value.
fn probe_in(ix: &Interp<'_>, name: &str, args: Vec<(Value, u8)>) -> (Value, u8) {
    let f = *ix.fns.get(name).expect("a probe of the suite");
    assert_eq!(f.params.len(), args.len(), "{name}: arity");
    clear(ix);
    assert!(!ix.frame_sealed.get(), "an operator frame");
    let mut env = ix.new_env();
    let taints: Vec<u8> = args.iter().map(|(_, t)| *t).collect();
    env.bind_fresh(
        f.params
            .iter()
            .zip(args)
            .map(|(p, (v, _))| (intern(&p.name), v)),
        |i| taints[i],
    );
    match ix.eval_t::<true>(&f.body, &mut env) {
        Ok(v) => (v, ix.taint.last.get()),
        Err(Flow::Return(v)) => (v, ix.taint.ret.get()),
        Err(e) => panic!("{name}: {e:?}"),
    }
}

/// A sealed run's `Interp`, not a runnable one (see
/// `the_unit_tests_bypass_only_the_pin_not_the_refusal`).
fn with_sealed<R>(tag: &str, f: impl FnOnce(&Interp<'_>) -> R) -> R {
    let (prog, sdir) = prog(tag);
    let ix = Interp::build_with(&prog, std::slice::from_ref(&sdir), Engine::Tree);
    assert!(ix.seal.active);
    f(&ix)
}

fn int(n: i64) -> Value {
    Value::Int(n)
}

/// `(c, k)` with `c` the candidate's.
fn ck(c: Value, k: Value) -> Vec<(Value, u8)> {
    vec![(c, ALL), (k, 0)]
}

fn taint_of(ix: &Interp<'_>, name: &str, c: Value, k: Value) -> u8 {
    probe_in(ix, name, ck(c, k)).1
}

/// T8.2: every binding writer keeps its taint in step with its position: a
/// tainted binding next to a clean one, read back by position.
#[test]
fn a_binding_keeps_its_own_taint_by_position() {
    with_sealed("r51-s2-t82", |ix| {
        assert_eq!(
            taint_of(ix, "plain", int(1), int(2)),
            0,
            "the clean binding"
        );
        assert_eq!(
            taint_of(ix, "plain_a", int(1), int(2)),
            ALL,
            "the tainted one"
        );
        assert_eq!(taint_of(ix, "assign_tainted", int(1), int(2)), ALL);
        assert_eq!(
            taint_of(ix, "assign_clean", int(1), int(2)),
            0,
            "an assignment replaces the binding's taint"
        );
    });
}

/// What a statement touched does not become the block's value (amendment
/// 102): only the tail expression is.
#[test]
fn a_statement_does_not_taint_the_blocks_value() {
    with_sealed("r51-s2-block", |ix| {
        assert_eq!(taint_of(ix, "stmt_then_clean", int(1), int(2)), 0);
    });
}

/// T8.3: a refinement predicate's fresh env keeps the bound value's taint, so
/// what the predicate computes from it is tainted.
#[test]
fn a_predicate_env_keeps_the_values_taint() {
    with_sealed("r51-s2-t83", |ix| {
        let ty = crate::ast::AxonType::Named("Pos".to_string());
        let s = intern("r51_s2_pred");
        for (vt, want) in [(VAL, VAL), (0, 0)] {
            clear(ix);
            let mut env = ix.new_env();
            ix.bind_let::<true>("r51_s2_pred", s, 0, Some(&ty), int(3), vt, &mut env)
                .expect("3 > 0");
            assert_eq!(
                ix.taint.acc.get() & VAL,
                want,
                "the predicate read `_` (vt {vt})"
            );
            let i = env.get_var_idx(s, 0).expect("bound");
            assert_eq!(env.taint(i), vt, "the binding carries the value's taint");
        }
    });
}

/// T8.4: a handler arm's payload binding takes the taint it is given, and a
/// handler or continuation snapshot carries every binding's taint.
#[test]
fn handler_arm_bindings_and_snapshots_keep_taint() {
    with_sealed("r51-s2-t84", |ix| {
        let p = intern("r51_s2_p");
        let pat = Pattern::Ident("r51_s2_p".to_string());
        let mut env = ix.new_env();
        env.push();
        assert!(ix
            .match_pattern_rt(&pat, &int(1), &mut env, VAL)
            .expect("binds"));
        let i = env.find(p).expect("bound");
        assert_eq!(env.taint(i), VAL, "the arm's payload binding");
        let q = intern("r51_s2_q");
        env.define(q, int(2), 0);
        let back = ix.env_from_snapshot(env.snapshot());
        assert_eq!(
            back.taint(back.find(p).expect("p")),
            VAL,
            "the snapshot keeps p's taint"
        );
        assert_eq!(back.taint(back.find(q).expect("q")), 0, "and q's (none)");
    });
}

/// T8.5: the AX-31 in-place appends and the place writes put the value's
/// taint on the binding; the AX-55 operand read is not taken in a sealed run.
#[test]
fn in_place_reads_and_writes_carry_the_taint() {
    with_sealed("r51-s2-t85", |ix| {
        assert_eq!(
            taint_of(ix, "push_in_place", int(1), int(2)) & VAL,
            VAL,
            "arr_push in place"
        );
        let (s, k) = (
            Value::Str(Rc::new("c".into())),
            Value::Str(Rc::new("k".into())),
        );
        assert_eq!(
            taint_of(ix, "concat_in_place", s, k) & VAL,
            VAL,
            "`s = s + c` in place"
        );
        assert_eq!(
            taint_of(ix, "place_write", int(1), int(2)) & VAL,
            VAL,
            "`xs[0] = c`"
        );
        assert_eq!(
            taint_of(ix, "place_index", int(1), int(2)) & VAL,
            VAL,
            "`xs[c] = k`"
        );
        assert_eq!(
            taint_of(ix, "add", int(1), int(2)),
            ALL,
            "an operand read in place"
        );
    });
}

/// A comparison, a negation and a format string yield a `bool` or `str` the
/// operator's operator chose: the value bit stays, the type bit goes.
#[test]
fn comparisons_negation_and_format_strings_untype() {
    with_sealed("r51-s2-untype", |ix| {
        assert_eq!(taint_of(ix, "compare", int(1), int(2)), VAL);
        assert_eq!(
            taint_of(ix, "negate", Value::Bool(true), Value::Bool(false)),
            VAL
        );
        assert_eq!(taint_of(ix, "fmt", int(1), int(2)), VAL);
    });
}

/// A store under a branch on the candidate's value carries the condition's
/// value bit (the pc), for every branching construct.
#[test]
fn a_store_under_a_tainted_branch_is_tainted() {
    with_sealed("r51-s2-pc", |ix| {
        for name in [
            "under_if",
            "under_else",
            "under_match",
            "under_guard",
            "under_while",
            "under_for",
            "under_and",
        ] {
            assert_eq!(taint_of(ix, name, int(7), int(0)), VAL, "{name}");
        }
        // The same constructs on the operator's own value store clean.
        let clean = |name: &str| probe_in(ix, name, vec![(int(7), 0), (int(0), 0)]).1;
        for name in [
            "under_if",
            "under_match",
            "under_while",
            "under_for",
            "under_and",
        ] {
            assert_eq!(clean(name), 0, "{name} on a clean condition");
        }
        assert_eq!(ix.taint.pc.get(), 0, "the pc is restored after each branch");
    });
}

/// An early exit out of a tainted branch, or a `?` on the candidate's value,
/// leaves everything stored after it control-dependent (sticky), and a
/// `return` carries its value's taint out.
#[test]
fn an_early_exit_on_a_tainted_value_is_sticky() {
    with_sealed("r51-s2-sticky", |ix| {
        assert_eq!(
            taint_of(ix, "after_break", int(1), int(2)),
            VAL,
            "after a break"
        );
        let (v, t) = probe_in(
            ix,
            "after_question",
            vec![(Value::Ok(Box::new(int(1))), ALL), (int(2), 0)],
        );
        assert!(matches!(v, Value::Ok(_)));
        assert_eq!(t & VAL, VAL, "after a `?`");
        let (v, t) = probe_in(ix, "returns", vec![(int(9), ALL), (int(1), 0)]);
        assert!(matches!(v, Value::Int(9)));
        assert_eq!(t, ALL, "`return c` carries c's taint");
    });
}

/// A pattern binding takes the subject's taint, in `match` and `while let`.
#[test]
fn a_pattern_binding_takes_the_subjects_taint() {
    with_sealed("r51-s2-pat", |ix| {
        let some = || Value::Some(Box::new(int(3)));
        assert_eq!(taint_of(ix, "bind_pattern", some(), int(0)), ALL);
        // `x = n` stores n's taint (ALL); the loop's pc alone would give VAL.
        assert_eq!(taint_of(ix, "while_let", some(), int(0)), ALL);
    });
}

/// A channel the operator sent the candidate's value on, and a dict it wrote
/// the candidate's value into, carry its taint; the same writes of the
/// operator's own value do not. A comparison reads both dicts deep.
#[test]
fn shared_objects_written_with_a_tainted_value_carry_it() {
    with_sealed("r51-s2-objs", |ix| {
        let (ch, _) = probe_in(ix, "send", ck(int(1), int(2)));
        assert_eq!(ix.t_obj(&ch) & VAL, VAL, "the channel sent on");
        let (ch, _) = probe_in(ix, "send_clean", ck(int(1), int(2)));
        assert_eq!(ix.t_obj(&ch), 0, "a clean send");
        let (d, _) = probe_in(ix, "dict_write", ck(int(1), int(2)));
        assert_eq!(ix.t_obj(&d) & VAL, VAL, "the dict written (t_builtin_out)");
        let (e, _) = probe_in(ix, "dict_write_clean", ck(int(1), int(2)));
        assert_eq!(ix.t_obj(&e), 0, "a clean write");
        let d2 = d.clone();
        let (_, t) = probe_in(ix, "dict_compare", vec![(e.clone(), 0), (d, 0)]);
        assert_eq!(t & VAL, VAL, "`==` reads the tainted dict deep");
        let (_, t) = probe_in(ix, "dict_compare", vec![(e.clone(), 0), (e, 0)]);
        assert_eq!(t, 0, "two clean dicts compare clean");
        let (_, t) = probe_in(ix, "dict_read", vec![(d2.clone(), 0), (int(0), 0)]);
        assert_eq!(
            t & VAL,
            VAL,
            "a builtin reading the tainted dict (t_builtin_in)"
        );
    });
}

/// Outside a sealed run the evaluator takes no taint: the same probes run on
/// the unsealed instantiation leave every taint cell at 0.
#[test]
fn an_ordinary_run_takes_no_taint() {
    let (prog, _) = prog("r51-s2-plain");
    let ix = Interp::build_with(&prog, &[], Engine::Tree);
    assert!(!ix.seal.active);
    let f = *ix.fns.get("under_if").expect("probe");
    let mut env = ix.new_env();
    env.bind_fresh(
        f.params
            .iter()
            .zip([int(7), int(0)])
            .map(|(p, v)| (intern(&p.name), v)),
        |_| 0,
    );
    assert!(matches!(
        ix.eval(&f.body, &mut env).expect("runs"),
        Value::Int(1)
    ));
    let t = &ix.taint;
    for (what, c) in [
        ("acc", &t.acc),
        ("last", &t.last),
        ("pc", &t.pc),
        ("sticky", &t.sticky),
    ] {
        assert_eq!(c.get(), 0, "{what}");
    }
    assert_eq!(TYP & VAL, 0);
}

/// The sealed builds these tests use are not runnable sealed runs:
/// `Interp::build_in` still refuses them (SEAL_RUNTIME_COMPLETE = false).
#[test]
fn the_unit_tests_bypass_only_the_pin_not_the_refusal() {
    let (prog, sdir) = prog("r51-s2-refuse");
    const { assert!(!SEAL_RUNTIME_COMPLETE) };
    assert_eq!(
        Interp::build_in(&prog, &[sdir]).err().as_deref(),
        Some(SEALED_RUN_REFUSED)
    );
}

/// Every builtin is in exactly one taint class, and every name a class table
/// lists is a builtin. A builtin added to `BUILTINS` is in no class until
/// someone decides which: it reads as `Unclassified` (treated as reading and
/// writing both kinds of state, so it can only over-taint) and this fails
/// (veto taint_tests.rs:651; `now_ns` is World like `now_ms`, TaintPort-14).
#[test]
fn every_builtin_has_a_taint_class() {
    use super::taint::{info, Class, DICT_WRITERS, KERNEL_PREFIXES, PURE_BUILTINS, WORLD_BUILTINS};
    let names: Vec<&str> = crate::builtins::BUILTINS.iter().map(|b| b.name).collect();
    for n in &names {
        let c = info(n).expect("every builtin has an entry").class;
        assert_ne!(
            c,
            Class::Unclassified,
            "DRIFT: builtin `{n}` is in no taint class (interp/taint.rs)"
        );
    }
    assert_eq!(info("now_ns").map(|i| i.class), Some(Class::World));
    for (what, list) in [
        ("WORLD_BUILTINS", WORLD_BUILTINS),
        ("PURE_BUILTINS", PURE_BUILTINS),
        ("DICT_WRITERS", DICT_WRITERS),
    ] {
        for n in list {
            assert!(
                names.contains(n),
                "DRIFT: {what} lists `{n}`, which is not a builtin"
            );
        }
    }
    for n in WORLD_BUILTINS {
        assert!(!PURE_BUILTINS.contains(n), "`{n}` is both world and pure");
        assert!(
            !KERNEL_PREFIXES.iter().any(|p| n.starts_with(p)),
            "`{n}` is both world and kernel"
        );
    }
    for n in PURE_BUILTINS {
        assert!(
            !KERNEL_PREFIXES.iter().any(|p| n.starts_with(p)),
            "`{n}` is both pure and kernel"
        );
    }
    // No two entries agree: a duplicate is a stale edit.
    let mut seen = std::collections::HashSet::new();
    for n in PURE_BUILTINS.iter().chain(WORLD_BUILTINS) {
        assert!(seen.insert(*n), "`{n}` is listed twice");
    }
}

/// An interpreter source file, its test module cut off.
fn source(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    let src = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    match src.find("\n#[cfg(test)]\nmod ") {
        Some(i) => src[..i].to_string(),
        None => src,
    }
}

/// Every builtin arm that turns a value into text is either a stringifier (its
/// result carries the taint of every object inside the argument) or an emitter
/// (it writes to a stream and returns nothing readable) (veto
/// taint_tests.rs:1432).
#[test]
fn every_builtin_that_renders_a_value_to_text_is_a_stringifier_or_an_emitter() {
    use super::taint::{EMITTERS, STRINGIFIERS};
    let src = source("interp/builtins.rs");
    let mut cur: Vec<String> = Vec::new();
    let mut found: Vec<String> = Vec::new();
    for line in src.lines() {
        let indent = line.len() - line.trim_start().len();
        let t = line.trim_start();
        if indent == 12 && t.starts_with('"') && t.contains("=>") {
            cur = t[..t.find("=>").unwrap()]
                .split('|')
                .map(|n| n.trim().trim_matches('"').to_string())
                .collect();
        } else if !t.starts_with("//") && t.contains("display(") && !t.contains("fn display") {
            found.extend(cur.iter().cloned());
        }
    }
    found.sort();
    found.dedup();
    let mut want: Vec<String> = STRINGIFIERS
        .iter()
        .chain(EMITTERS)
        .map(|s| s.to_string())
        .collect();
    want.sort();
    assert_eq!(
        found, want,
        "DRIFT: a builtin renders a value with display() and is neither a STRINGIFIER nor an EMITTER (interp/taint.rs)"
    );
    // The sink of text in the evaluator itself.
    let ev = source("interp/eval.rs");
    let fmt = ev.find("Expr::FmtStr { parts }").expect("FmtStr arm");
    assert!(
        ev[fmt..fmt + 900].contains("t_obj_deep"),
        "DRIFT: string interpolation no longer takes the taint of the objects it prints"
    );
}

/// `call_builtin` is called from exactly two places, and both route the taint:
/// the evaluator's call (which classifies the builtin before and after) and
/// `assign_in_place` (`arr_push`/`arr_concat`, pure, whose container binding
/// takes the appended value's taint itself) (veto taint_tests.rs:735).
#[test]
fn builtins_are_dispatched_only_where_the_taint_is_routed() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/interp");
    let mut files = vec!["interp.rs".to_string()];
    let mut walk = vec![dir.clone()];
    while let Some(d) = walk.pop() {
        for e in std::fs::read_dir(&d).expect("interp dir").flatten() {
            let p = e.path();
            if p.is_dir() {
                walk.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let rel = p.strip_prefix(dir.parent().unwrap()).unwrap();
                files.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    files.sort();
    let mut sites = Vec::new();
    for file in &files {
        if file.ends_with("taint_tests.rs") {
            continue;
        }
        for l in source(file).lines() {
            let t = l.trim();
            if t.starts_with("//") {
                continue;
            }
            if t.contains("call_builtin(") && !t.contains("fn call_builtin") {
                sites.push((file.clone(), t.to_string()));
            }
        }
    }
    assert_eq!(
        sites,
        vec![
            (
                "interp/eval.rs".to_string(),
                "if let Some(v) = self.call_builtin(name, argv.values())? {".to_string()
            ),
            ("interp/eval.rs".to_string(), ".call_builtin(f, &[x, y])?".to_string()),
        ],
        "DRIFT: a new `call_builtin` caller must route the taint (`t_builtin_in`/`t_builtin_out`) or be pure"
    );
}

/// A branch's condition keeps only its value bit in what the branch yields
/// (`t_control_val_only`): `if`/`match` on the candidate's `bool` returning the
/// operator's value carries VAL, not TYP.
#[test]
fn a_branch_keeps_only_the_conditions_value_bit() {
    with_sealed("r51-s2-cvo", |ix| {
        let b = Value::Bool(true);
        assert_eq!(taint_of(ix, "if_typ", b.clone(), int(2)), VAL, "if");
        assert_eq!(taint_of(ix, "match_typ", b, int(2)), VAL, "match");
    });
}

/// The other side of every two-way branch: the `else` taken, and the arm
/// taken because a guard on the candidate's value REFUSED the earlier one.
#[test]
fn a_store_on_the_branch_not_first_is_tainted_too() {
    with_sealed("r51-s2-else", |ix| {
        assert_eq!(taint_of(ix, "under_else", int(-1), int(0)), VAL, "else");
        assert_eq!(
            taint_of(ix, "under_guard", int(-1), int(0)),
            VAL,
            "after a refusing guard"
        );
    });
}

/// A loop's condition is evaluated under the taint of every earlier condition
/// (`seen`): what it stores on the second pass depends on the first.
#[test]
fn a_loop_condition_runs_under_the_earlier_conditions() {
    with_sealed("r51-s2-seen", |ix| {
        let (d, _) = probe_in(ix, "while_seen", ck(int(2), int(0)));
        assert_eq!(ix.t_obj(&d) & VAL, VAL, "while");
        let some = Value::Some(Box::new(int(3)));
        let (d, _) = probe_in(ix, "while_let_seen", ck(some.clone(), int(0)));
        assert_eq!(ix.t_obj(&d) & VAL, VAL, "while let");
        assert_eq!(
            taint_of(ix, "while_let_store", some, int(0)),
            VAL,
            "a store in a while-let body"
        );
    });
}

/// `?` on the candidate's `Err` returns with its taint.
#[test]
fn a_question_mark_returns_with_the_operands_taint() {
    with_sealed("r51-s2-qret", |ix| {
        let err = Value::Err(Box::new(Value::Str(Rc::new("e".into()))));
        let (v, t) = probe_in(ix, "question_err", vec![(err, ALL), (int(2), 0)]);
        assert!(matches!(v, Value::Err(_)));
        assert_eq!(t & VAL, VAL);
    });
}

/// `select` fires the arm of the first ready channel: a channel the candidate's
/// value was sent on chose it.
#[test]
fn a_select_on_a_tainted_channel_is_a_tainted_branch() {
    with_sealed("r51-s2-select", |ix| {
        assert_eq!(taint_of(ix, "select_store", int(1), int(0)), VAL);
    });
}

/// A builtin that calls back runs the callback under the taint of its
/// arguments: how many times, and which, is the candidate's array's.
#[test]
fn a_builtins_callback_runs_under_its_arguments_taint() {
    with_sealed("r51-s2-cb", |ix| {
        let xs = Value::Array(Rc::new(vec![int(1), int(2)]));
        let (d, _) = probe_in(ix, "callback_store", ck(xs, int(0)));
        assert_eq!(ix.t_obj(&d) & VAL, VAL);
    });
}

/// A native call writes the World state with what it was given: a World
/// builtin (the clock) read after it returns that taint.
#[test]
fn a_native_call_writes_the_world_state() {
    with_sealed("r51-s2-native", |ix| {
        let clean = vec![(int(0), 0), (int(0), 0)];
        assert_eq!(probe_in(ix, "clock", clean.clone()).1 & VAL, 0, "before");
        let _ = probe_in(ix, "native", ck(int(64), int(0)));
        assert_eq!(probe_in(ix, "clock", clean).1 & VAL, VAL, "after");
    });
}

/// Inside an abort-capable `with` body (an arm that is not a bare tail
/// `resume`) a branch on the candidate's value is a possible exit: what is
/// stored after it is sticky.
#[test]
fn an_abortable_handler_body_makes_what_follows_sticky() {
    with_sealed("r51-s2-abort", |ix| {
        assert_eq!(taint_of(ix, "abortable", int(1), int(0)), VAL);
        assert_eq!(ix.taint.abortable.get(), 0, "the count is restored");
    });
}

/// A sealed frame entered inside an abort-capable `with` body makes what
/// follows sticky (`eval_with_handler` compares `taint.entries`). Sealed frames
/// are entered only through `Interp::with_frame`, which has no caller before
/// the seal edges.
#[test]
#[ignore = "R51 S3: a sealed frame is entered only through the seal edge (seal_call -> with_frame); no caller before S3"]
fn a_sealed_frame_entered_in_an_abortable_handler_body_makes_what_follows_sticky() {
    with_sealed("r51-s2-entered", |ix| {
        assert_eq!(taint_of(ix, "enters_sealed", int(1), int(0)), VAL);
    });
}

/// A handler arm's payload binding carries the operation's arguments' taint.
#[test]
fn a_handler_arms_payload_carries_the_operations_taint() {
    with_sealed("r51-s2-payload", |ix| {
        let s = Value::Str(Rc::new("c".into()));
        let (d, _) = probe_in(ix, "payload", ck(s, int(0)));
        // ALL, not only the builtin's pc (VAL): the binding carries the
        // arguments' own taint.
        assert_eq!(ix.t_obj(&d), ALL);
    });
}

/// A read of an element of a binding takes the binding's taint even where the
/// element is read in place.
#[test]
fn an_element_read_takes_the_holders_taint() {
    with_sealed("r51-s2-index", |ix| {
        let xs = Value::Array(Rc::new(vec![int(1)]));
        assert_eq!(taint_of(ix, "index_read", xs, int(0)), ALL);
    });
}

/// Everything a SEALED frame computes is the candidate's.
#[test]
fn a_sealed_frame_computes_only_tainted_values() {
    with_sealed("r51-s2-fs", |ix| {
        let f = *ix.fns.get("plain").expect("probe");
        clear(ix);
        let mut env = ix.new_env();
        env.bind_fresh(
            f.params
                .iter()
                .zip([int(1), int(2)])
                .map(|(p, v)| (intern(&p.name), v)),
            |_| 0,
        );
        ix.frame_sealed.set(true);
        let r = ix.eval_t::<true>(&f.body, &mut env);
        ix.frame_sealed.set(false);
        assert!(matches!(r, Ok(Value::Int(2))));
        assert_eq!(ix.taint.last.get(), ALL);
    });
}

/// T8.3 (S2.3 step 4): a fn frame binds each parameter with its argument's
/// taint, and the call's value carries what the body's value carried.
#[test]
fn an_arguments_taint_reaches_the_parameter() {
    with_sealed("r51-s2-farg", |ix| {
        assert_eq!(taint_of(ix, "call_arg", int(1), int(0)), ALL);
        // What the callee STORES through the parameter carries it too (the
        // call's value would carry it through the entry taint alone).
        let (d, _) = probe_in(ix, "call_store", ck(int(1), int(0)));
        assert_eq!(ix.t_obj(&d) & VAL, VAL, "the parameter's binding");
        assert_eq!(
            taint_of(ix, "call_arg_clean", int(1), int(0)),
            0,
            "a clean argument binds a clean parameter"
        );
    });
}

/// A `return` out of a frame carries the returned value's taint (`taint.ret`
/// read at the frame's exit), not the clean accumulator it unwinds past.
#[test]
fn a_return_carries_its_values_taint_out_of_the_frame() {
    with_sealed("r51-s2-fret", |ix| {
        assert_eq!(taint_of(ix, "call_ret", int(1), int(0)), ALL);
        // The closure's args are clean: the result is the `return`'s own
        // taint (the tainted branch's pc, VAL), not the whole tainted `if`
        // statement it unwound out of.
        assert_eq!(taint_of(ix, "capture_ret", int(1), int(0)), VAL);
    });
}

/// An early exit out of a tainted branch inside the callee raises the
/// callee's sticky; the caller's pc and sticky are restored at the exit, so
/// what the caller stores after the call is clean.
#[test]
fn the_callers_pc_and_sticky_are_restored_after_the_call() {
    with_sealed("r51-s2-fctl", |ix| {
        assert_eq!(taint_of(ix, "after_call", int(1), int(0)), 0);
        assert_eq!(ix.taint.sticky.get(), 0, "sticky restored");
        assert_eq!(ix.taint.pc.get(), 0, "pc restored");
    });
}

/// TaintPort-5: a closure's capture keeps its binding's taint on the lend
/// path (the only reference) and the copy path (a second reference).
#[test]
fn a_closure_capture_keeps_its_taint_through_lend_and_copy() {
    with_sealed("r51-s2-cap", |ix| {
        assert_eq!(taint_of(ix, "capture_lend", int(1), int(0)), ALL, "lend");
        assert_eq!(taint_of(ix, "capture_copy", int(1), int(0)), ALL, "copy");
    });
}

/// A capture written by one call and read by the next: the lend moves the
/// written taint back into the cell, the copy path's write-back writes it.
#[test]
fn a_captures_written_taint_survives_into_the_next_call() {
    with_sealed("r51-s2-capw", |ix| {
        assert_eq!(
            taint_of(ix, "capture_write_lend", int(1), int(0)),
            ALL,
            "lend back"
        );
        assert_eq!(
            taint_of(ix, "capture_write_copy", int(1), int(0)),
            ALL,
            "write-back"
        );
    });
}

/// A frame taken from the pool after a tainted call starts clean: the second
/// call, on clean arguments, computes a clean value.
#[test]
fn a_reused_pooled_frame_starts_clean() {
    with_sealed("r51-s2-pool", |ix| {
        assert_eq!(taint_of(ix, "closure_twice", int(1), int(0)), 0, "closure");
        assert_eq!(taint_of(ix, "call_arg_clean", int(1), int(0)), 0, "fn");
    });
}
