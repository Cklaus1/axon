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
fn op_closure() -> i64 { apply(|x: i64| x + 1) }
fn op_closure_honest() -> i64 { apply_ok(|x: i64| x + 1) }
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
";

/// The sealed candidate.
const CAND: &str = "\
fn solve(x: i64) -> i64 { x + 40 }
fn confuse() -> i64 {
    let d = dict_new()
    dict_set(d, \"k\", \"s\")
    match dict_get(d, \"k\") { Some(v) => v, None => 0 }
}
fn calls_operator() -> i64 { w(1) }
fn mutate(a: &mut i64) { a = \"s\" }
fn bump(a: &mut i64) { a = a + 1 }
fn apply(f: fn(i64) -> i64) -> i64 {
    let g = f
    g(\"s\")
}
fn apply_ok(f: fn(i64) -> i64) -> i64 { f(1) }
fn fill(d: Dict) -> i64 {
    dict_set(d, \"k\", \"s\")
    0
}
fn fill_ok(d: Dict) -> i64 {
    dict_set(d, \"k\", 2)
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
/// the seal, so each argument is cast strictly to the closure's `fn` type.
#[test]
fn a_sealed_frame_calling_an_operator_closure_casts_its_arguments() {
    with_sealed("r51-s3c-clo", |ix| {
        assert!(run(ix, "op_closure", vec![]).is_err());
        assert!(matches!(
            run(ix, "op_closure_honest", vec![]),
            Ok(Value::Int(2))
        ));
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
