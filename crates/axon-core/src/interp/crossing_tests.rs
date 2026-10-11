//! R51 S3: the seal CROSSING - calls between operator and sealed code
//! (`call_fn_sealed`, `seal_call`, the `&mut` hand-back, closure calls across
//! the seal, the dict and handler edges). Every sealed run is refused at build
//! until S6 (`SEAL_RUNTIME_COMPLETE`), so these drive an `Interp` made by
//! [`Interp::build_with`] directly, as `kernel_tests` does: the edges are
//! proven now, the live sealed run at S6.

use super::*;

/// The operator's suite: it calls into the candidate and is called back.
const SUITE: &str = "\
fn w(n: i64) -> i64 { n }
fn op_honest() -> i64 { solve(1) }
fn op_mut() -> i64 {
    let mut x = 1
    mutate(&mut x)
    x
}
fn op_mut_honest() -> i64 {
    let mut x = 1
    bump(&mut x)
    x
}
fn op_mut_dict() -> i64 {
    let mut d = dict_new()
    dict_set(d, \"k\", 1)
    swap_dict(&mut d)
    0
}
fn op_mut_arr() -> i64 {
    let mut a = []
    fill_arr(&mut a)
    0
}
fn op_closure() -> i64 { apply(|x| 7) }
fn op_closure_honest() -> i64 { apply_ok(|x| x + 1) }
fn op_closure_ret() -> i64 {
    let g = make()
    let v = g(1)
    7
}
fn op_closure_ret_honest() -> i64 {
    let g = make_ok()
    g(1)
}
fn op_dict() -> i64 {
    let d = dict_new()
    dict_set(d, \"k\", 1)
    fill(d)
}
fn op_dict_honest() -> i64 {
    let d = dict_new()
    dict_set(d, \"k\", 1)
    fill_ok(d)
}
fn op_eff() -> i64 {
    let d = dict_new()
    dict_set(d, \"k\", 1)
    with handler { on IO(p) => 3 } {
        fill_io(d)
    }
}
fn op_eff_back() -> i64 {
    let d = dict_new()
    dict_set(d, \"k\", 1)
    with handler { on IO(p) => resume(d) } {
        take_io()
    }
}
fn fiber_bad(n: i64) -> i64 {
    let s = solve(n)
    let z = n - n
    s / z
}
fn fiber_div(n: i64) -> i64 {
    let z = n - n
    n / z
}
";

/// The sealed candidate.
const CAND: &str = "\
fn solve(x: i64) -> i64 { x + 40 }
fn narrow(n: i64) -> u8 { 4 as u8 }
fn confuse() -> i64 {
    let d = dict_new()
    dict_set(d, \"k\", \"s\")
    match dict_get(d, \"k\") { Some(v) => v, None => 0 }
}
fn calls_operator() -> i64 { w(1) }
fn mutate(a: &mut i64) { a = \"s\" }
fn bump(a: &mut i64) { a = a + 1 }
fn swap_dict(d: &mut Dict) {
    let e = dict_new()
    dict_set(e, \"k\", \"s\")
    d = e
}
fn fill_arr(a: &mut [i64]) { a = [\"s\"] }
fn apply(f: fn(i64) -> i64) -> i64 {
    let g = f
    g(\"s\")
}
fn apply_ok(f: fn(i64) -> i64) -> i64 { f(1) }
fn make() -> fn(i64) -> i64 { |x| \"s\" }
fn make_ok() -> fn(i64) -> i64 { |x| x + 1 }
fn fill(d: Dict) -> i64 {
    dict_set(d, \"k\", \"s\")
    0
}
fn fill_ok(d: Dict) -> i64 {
    dict_set(d, \"k\", 2)
    0
}
fn fill_io(d: Dict) -> i64 {
    dict_set(d, \"k\", \"s\")
    println(d)
    0
}
fn take_io() -> i64 {
    let e = println(\"x\")
    dict_set(e, \"k\", \"s\")
    0
}
";

fn with_sealed<R>(tag: &str, f: impl FnOnce(&Interp<'_>) -> R) -> R {
    use crate::span::intern_source;
    let sdir = format!("/{tag}-sealed");
    let s = crate::parse_source_in(SUITE, intern_source(&format!("/{tag}-suite/h.ax"), SUITE))
        .expect("suite parses");
    let c = crate::parse_source_in(CAND, intern_source(&format!("{sdir}/f.ax"), CAND))
        .expect("candidate parses");
    let prog = Program {
        items: s.items.into_iter().chain(c.items).collect(),
    };
    let sdir = std::path::PathBuf::from(sdir);
    let ix = Interp::build_with(&prog, std::slice::from_ref(&sdir), Engine::Tree);
    assert!(ix.seal.active);
    f(&ix)
}

fn run(ix: &Interp<'_>, name: &str, args: Vec<Value>) -> R {
    let f = *ix
        .fns
        .get(name)
        .unwrap_or_else(|| panic!("`{name}` is defined"));
    ix.call_fn(f, args)
}

fn refused(r: R, needle: &str) {
    match r {
        Err(Flow::Panic(m)) => assert!(m.contains(needle), "refusal `{m}` lacks `{needle}`"),
        other => panic!("expected a refusal containing `{needle}`, got {other:?}"),
    }
}

/// An honest crossing: operator -> sealed -> back, the value intact, and the
/// callee's frame entered through `with_frame` (sealed provenance, counted).
#[test]
fn an_honest_call_crosses_into_a_sealed_frame_and_back() {
    with_sealed("r51-s3c-honest", |ix| {
        assert!(matches!(run(ix, "op_honest", vec![]), Ok(Value::Int(41))));
        assert!(
            !ix.frame_sealed.get(),
            "the operator's provenance is restored"
        );
        assert_eq!(ix.sealed_frames.get(), 0);
        assert!(ix.taint.entries.get() >= 1, "the sealed frame was entered");
    });
}

/// The parameter cast: a mistyped argument handed to a sealed fn is refused.
#[test]
fn a_mistyped_argument_is_refused_at_the_crossing() {
    with_sealed("r51-s3c-arg", |ix| {
        refused(
            run(ix, "solve", vec![Value::Str(Rc::new("s".into()))]),
            "runtime type confusion",
        );
        assert!(matches!(
            run(ix, "solve", vec![Value::Int(2)]),
            Ok(Value::Int(42))
        ));
    });
}

/// The return cast (veto interp.rs:4329-4352): a sealed fn declared `-> i64`
/// that returns a `str` laundered through an untyped `dict_get`.
#[test]
fn a_sealed_fns_mistyped_result_is_refused_at_the_crossing() {
    with_sealed("r51-s3c-ret", |ix| {
        refused(run(ix, "confuse", vec![]), "runtime type confusion");
    });
}

/// `seal_call`: a sealed frame may run only sealed fns; an operator fn is
/// refused in the same words as a missing one (`sealed_no_fn_msg`).
#[test]
fn a_sealed_frame_cannot_call_an_operator_fn() {
    with_sealed("r51-s3c-call", |ix| {
        let r = run(ix, "calls_operator", vec![]);
        refused(r, &Interp::sealed_no_fn_msg("w"));
        assert_eq!(
            Interp::sealed_no_fn_msg("w").replace("`w`", "`nope`"),
            Interp::sealed_no_fn_msg("nope"),
            "one text for an operator name and a missing one"
        );
    });
}

/// `replaced_ok_top` (veto interp.rs:3495-3530): a `&mut Dict` the operator
/// handed over may be replaced only by a value its held keys allow; the
/// declared `Dict` alone would accept `{k: "s"}`.
#[test]
fn a_mut_replacement_is_judged_against_what_the_operator_held() {
    with_sealed("r51-s3c-mutdict", |ix| {
        refused(
            run(ix, "op_mut_dict", vec![]),
            "the operator's did not allow",
        );
    });
}

/// The `&mut` cast proper: an empty array shows no element, so the held
/// value allows anything and the declared `[i64]` judges the element.
#[test]
fn a_mut_hand_back_into_an_empty_array_is_judged_by_the_declared_type() {
    with_sealed("r51-s3c-mutarr", |ix| {
        refused(run(ix, "op_mut_arr", vec![]), "is declared `&mut [i64]`");
    });
}

/// The `&mut` hand-back (veto `call_fn_mut`, interp.rs:3495): the parameter's
/// final value is a second return channel and is cast too.
#[test]
fn a_mut_hand_back_is_cast_to_the_declared_type() {
    with_sealed("r51-s3c-mut", |ix| {
        refused(run(ix, "op_mut", vec![]), "`&mut` argument");
        assert!(matches!(
            run(ix, "op_mut_honest", vec![]),
            Ok(Value::Int(2))
        ));
    });
}

/// `closure_args_check`: a sealed frame calling an OPERATOR closure crosses
/// the seal, so each argument is cast strictly to the closure's `fn` type
/// (the contract the closure took at `apply`'s parameter cast). The closure
/// itself ignores its argument, so only the edge can refuse.
#[test]
fn a_sealed_frame_calling_an_operator_closure_casts_its_arguments() {
    with_sealed("r51-s3c-clo", |ix| {
        refused(run(ix, "op_closure", vec![]), "runtime type confusion");
        assert!(matches!(
            run(ix, "op_closure_honest", vec![]),
            Ok(Value::Int(2))
        ));
    });
}

/// `closure_ret_check` (veto interp.rs:4888-4897): a CANDIDATE closure called
/// from operator code returns across the seal, so its result is cast strictly
/// to the `fn` type it crossed at `make`'s return. The operator discards the
/// result, so only the edge can refuse.
#[test]
fn a_candidate_closures_result_is_cast_on_the_way_back() {
    with_sealed("r51-s3c-cloret", |ix| {
        refused(run(ix, "op_closure_ret", vec![]), "runtime type confusion");
        assert!(matches!(
            run(ix, "op_closure_ret_honest", vec![]),
            Ok(Value::Int(2))
        ));
    });
}

/// A sealed frame's own `frame_sealed` at `with_frame` makes a fn frame's
/// parameters take ALL (spec §11 S3); an operator frame's take what they
/// arrived with.
#[test]
fn a_sealed_frames_parameters_take_all() {
    with_sealed("r51-s3c-params", |ix| {
        assert_eq!(ix.with_frame(true, || ix.t_params::<true>(0)), taint::ALL);
        assert_eq!(ix.with_frame(false, || ix.t_params::<true>(0)), 0);
        assert_eq!(ix.with_frame(true, || ix.t_params::<false>(0)), 0);
    });
}

/// `dict_edge_in`/`dict_edge_out`: a dict the operator handed in is
/// snapshotted, and a key it held must keep a value of that type on return.
#[test]
fn a_dict_the_operator_held_is_checked_on_the_way_back() {
    // Two interpreters: a refusal ends a sealed run, so nothing after one is
    // a crossing the seal has to judge (the refused dict stays dirty).
    with_sealed("r51-s3c-dict", |ix| {
        refused(run(ix, "op_dict", vec![]), "retyped a dict entry");
    });
    with_sealed("r51-s3c-dict-ok", |ix| {
        let r = run(ix, "op_dict_honest", vec![]);
        assert!(matches!(r, Ok(Value::Int(0))), "{r:?}");
    });
}

/// `taint.catchable` read at the crossing (veto interp.rs:3615-3623): operator
/// code in a scheduler fiber that called sealed code continues
/// control-dependent on the candidate, so the frame's sticky taint is raised.
#[test]
fn a_crossing_inside_a_fiber_makes_what_follows_sticky() {
    with_sealed("r51-s3c-sticky", |ix| {
        assert!(matches!(run(ix, "op_honest", vec![]), Ok(Value::Int(41))));
        assert_eq!(ix.taint.sticky.get() & taint::VAL, 0, "no fiber, no sticky");
    });
    with_sealed("r51-s3c-sticky-fiber", |ix| {
        ix.taint.catchable.set(1);
        let f = *ix.fns.get("solve").expect("solve");
        assert!(matches!(
            ix.call_fn(f, vec![Value::Int(1)]),
            Ok(Value::Int(41))
        ));
        assert_ne!(ix.taint.sticky.get() & taint::VAL, 0);
    });
}

/// The scheduler pass (veto builtins.rs:468-482): a fiber runs with
/// `catchable` raised, so a fiber whose root is the candidate's makes the
/// pass sticky at the crossing; a fiber that entered a sealed frame and then
/// FAILED makes it sticky too (its root frame's own sticky is restored when
/// it ends); a fiber that failed without entering one does not.
#[test]
fn a_scheduler_fiber_that_crossed_the_seal_makes_the_pass_sticky() {
    let pass = |tag: &str, root: &str, ok: i64| {
        with_sealed(tag, |ix| {
            let s = Value::Str(Rc::new(root.to_string()));
            ix.call_builtin("scheduler_spawn", &[s, Value::Int(1)])
                .expect("spawn")
                .expect("a builtin");
            ix.taint.sticky.set(0);
            assert!(matches!(ix.builtin_scheduler_run_once(), Ok(n) if n == ok));
            assert_eq!(ix.taint.catchable.get(), 0, "the count is restored");
            ix.taint.sticky.get() & taint::VAL
        })
    };
    assert_ne!(pass("r51-s3c-fib-root", "solve", 1), 0, "a sealed root");
    assert_ne!(
        pass("r51-s3c-fib-bad", "fiber_bad", 0),
        0,
        "entered, failed"
    );
    assert_eq!(
        pass("r51-s3c-fib-div", "fiber_div", 0),
        0,
        "failed, no entry"
    );
}

/// `sandbox_run` declares `-> i64`: a sealed fn's `u8` result is refused at
/// the crossing (veto builtins.rs:4488-4500); an `i64` one passes.
#[test]
fn sandbox_run_casts_a_sealed_fns_result_to_i64() {
    with_sealed("r51-s3c-sb", |ix| {
        let s = |x: &str| Value::Str(Rc::new(x.to_string()));
        let b = Value::Bool(true);
        let p = ix
            .call_builtin(
                "principal_root",
                &[s("op"), b.clone(), b.clone(), b, Value::Int(100)],
            )
            .expect("principal_root")
            .expect("a builtin");
        let sb = ix
            .call_builtin("sandbox_create", &[p, s("AI")])
            .expect("sb")
            .expect("a builtin");
        let r = ix.call_builtin("sandbox_run", &[sb.clone(), s("narrow"), Value::Int(1)]);
        refused(
            r.map(|v| v.unwrap_or(Value::Unit)),
            "runtime type confusion",
        );
        let r = ix.call_builtin("sandbox_run", &[sb, s("solve"), Value::Int(1)]);
        assert!(matches!(r, Ok(Some(Value::Int(41)))), "{r:?}");
    });
}

/// `handler_edge_into`: sealed code that retyped the operator's dict and then
/// performs an operation the operator's arm answers hands control (and the
/// dict, as the payload) across the seal. The arm aborts the `with` block, so
/// the call's own `dict_edge_out` never runs: only the edge into the arm can
/// refuse.
#[test]
fn an_aborting_operator_arm_checks_the_dicts_sealed_code_held() {
    with_sealed("r51-s3c-eff", |ix| {
        refused(run(ix, "op_eff", vec![]), "retyped a dict entry");
    });
}

/// `handler_edge_back`: an operator arm that resumes sealed code with the
/// operator's dict hands it across the seal, so it is snapshotted and a key
/// the operator held must keep its type when the sealed call returns.
#[test]
fn a_dict_resumed_into_sealed_code_is_checked_on_the_way_back() {
    with_sealed("r51-s3c-effback", |ix| {
        refused(run(ix, "op_eff_back", vec![]), "retyped a dict entry");
    });
}

/// `handler_may_answer`: a sealed handler frame answers only while no operator
/// frame has been entered since it was installed.
#[test]
fn a_sealed_handler_answers_only_its_own_frames() {
    with_sealed("r51-s3c-hma", |ix| {
        assert!(
            ix.handler_may_answer(false, 7),
            "an operator frame answers anything"
        );
        let n = ix.operator_frames.get();
        assert!(ix.handler_may_answer(true, n));
        ix.with_frame(false, || assert!(!ix.handler_may_answer(true, n)));
    });
}
