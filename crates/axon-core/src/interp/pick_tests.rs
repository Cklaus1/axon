//! R51 S4: the PICK mark and the refusing closure sink (spec §4 Fork 3
//! "PICK", §8 T8.1), unit by unit.
//!
//! [`run_unit`] stands in for v0.22's `run`: it builds the program with a live
//! seal ([`Interp::build_with`], no engine pin), initialises the
//! module-level lets and runs the operator's `@[test] fn t` in an operator
//! frame of the sealed instantiation, with the test switches set on THIS
//! thread. Each attack has an honest control and is shown live with the taint
//! rules off (a refusal elsewhere would hide a hole).

use super::taint::VAL;
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The verdict of one program (v0.22 taint_tests.rs:56-79).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Verdict {
    /// `t` ran to its end.
    Ok,
    /// A seal rule refused the program.
    Refused,
    /// An assertion of the suite failed (the honest "wrong answer").
    Fails,
}

pub(super) fn verdict(r: &Result<Value, Flow>) -> Verdict {
    match r {
        Ok(_) | Err(Flow::Return(_)) => Verdict::Ok,
        Err(Flow::Panic(m))
            if m.contains("(runtime taint)")
                || m.contains("the candidate picked")
                || m.contains("nothing on the operator side determined") =>
        {
            Verdict::Refused
        }
        Err(_) => Verdict::Fails,
    }
}

/// Restores the test switches on drop.
struct Switches;
impl Drop for Switches {
    fn drop(&mut self) {
        DISPATCH_RULE_OFF.with(|c| c.set(false));
        TAINT_FORCE_ON.with(|c| c.set(false));
    }
}

/// Run `t` of `suite` with `cand` loaded from a sealed directory, on a unit
/// test `Interp` (see the module doc). `static_on` / `taint_on` are v0.22's
/// `Rules` (taint_tests.rs:41-48).
pub(super) fn run_unit(
    suite: &str,
    cand: &str,
    static_on: bool,
    taint_on: bool,
) -> Result<Value, Flow> {
    use crate::span::intern_source;
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let sdir = format!("/r51-s4-pick{n}-sealed");
    let s = crate::parse_source_in(
        suite,
        intern_source(&format!("/r51-s4-pick{n}/h.ax"), suite),
    )
    .expect("suite parses");
    let c = crate::parse_source_in(cand, intern_source(&format!("{sdir}/f.ax"), cand))
        .expect("candidate parses");
    let prog = Program {
        items: s.items.into_iter().chain(c.items).collect(),
    };
    let mut ix = Interp::build_with(&prog, &[std::path::PathBuf::from(sdir)], Engine::Tree);
    assert!(ix.seal.active);
    let _sw = Switches;
    DISPATCH_RULE_OFF.with(|c| c.set(!static_on));
    TAINT_FORCE_ON.with(|c| c.set(taint_on));
    ix.init_globals()?;
    let f = *ix.fns.get("t").expect("the suite's test `t`");
    let mut env = ix.new_env();
    let prev = ix.pin_fn.replace(f as *const FnDef as usize);
    let r = ix.with_frame(false, || ix.eval_t::<true>(&f.body, &mut env));
    ix.pin_fn.set(prev);
    r
}

/// [`run_unit`] with both layers on, and its verdict.
fn judge(suite: &str, cand: &str) -> (Verdict, Result<Value, Flow>) {
    let r = run_unit(suite, cand, true, true);
    (verdict(&r), r)
}

/// `t` with `body`, after `pre`.
fn suite(pre: &str, body: &str) -> String {
    format!("{pre}@[test]\nfn t() {{\n{body}\n}}\n")
}

/// An attack: refused with both layers on, and complete with the taint rules
/// off (live).
fn attack(why: &str, suite: &str, cand: &str) {
    let (v, r) = judge(suite, cand);
    assert_eq!(
        v,
        Verdict::Refused,
        "ATTACK: <{why}> was not refused: {r:?}"
    );
    let off = run_unit(suite, cand, true, false);
    assert!(
        off.is_ok(),
        "the attack <{why}> is not live without the taint rules: {off:?}"
    );
}

/// An honest control: runs to its end with both layers on.
fn control(why: &str, suite: &str, cand: &str) {
    let (v, r) = judge(suite, cand);
    assert_eq!(
        v,
        Verdict::Ok,
        "control <{why}> was refused or failed: {r:?}"
    );
}

const REF: &str = "fn reference(x: i64) -> i64 { x * 2 }\n";
const OPS: &str = "    let ops = [|x| 0, |x| x * 2]\n";
const IDX: &str = "fn idx() -> i64 { 1 }\n";

/// T8.1: a pick survives a cast's re-wrap. A cast makes a new `ClosureVal`
/// that shares the capture cell (spec Fork 3, option A); the pick is keyed by
/// the cell, so every reference a cast made of a picked closure is picked,
/// and a different closure of the same lambda is not.
#[test]
fn a_pick_survives_a_casts_rewrap_of_the_closure() {
    let src = suite(
        &format!("{REF}fn mk() -> fn(i64) -> i64 {{ |x| x * 2 }}\n"),
        "    0",
    );
    let (prog, sdir) = {
        use crate::span::intern_source;
        let s = crate::parse_source_in(&src, intern_source("/r51-s4-rewrap/h.ax", &src))
            .expect("parses");
        let c = crate::parse_source_in(IDX, intern_source("/r51-s4-rewrap-sealed/f.ax", IDX))
            .expect("parses");
        (
            Program {
                items: s.items.into_iter().chain(c.items).collect(),
            },
            std::path::PathBuf::from("/r51-s4-rewrap-sealed"),
        )
    };
    let ix = Interp::build_with(&prog, &[sdir], Engine::Tree);
    let mk = |ix: &Interp<'_>| {
        let f = *ix.fns.get("mk").expect("mk");
        let mut env = ix.new_env();
        match ix.eval_t::<true>(&f.body, &mut env) {
            Ok(v @ Value::Closure(_)) => v,
            other => panic!("mk: {other:?}"),
        }
    };
    let (picked, other) = (mk(&ix), mk(&ix));
    let (Value::Closure(p), Value::Closure(o)) = (&picked, &other) else {
        unreachable!()
    };
    ix.t_note(&picked, 0);
    assert!(!ix.t_is_picked(p), "a clean value is no pick");
    ix.t_note(&picked, VAL);
    assert!(ix.t_is_picked(p));
    let rewrap = ClosureVal {
        code: Held::new(Rc::clone(&p.code)),
        captured: Rc::clone(&p.captured),
        contract: None,
        seal: p.seal,
    };
    assert!(
        ix.t_is_picked(&rewrap),
        "ATTACK: a cast's re-wrap of a picked closure laundered the pick"
    );
    assert!(
        !ix.t_is_picked(o),
        "control: another closure of the same lambda is no pick"
    );
    TAINT_FORCE_ON.with(|c| c.set(true));
    let refused = ix.t_check_call_picked(&rewrap);
    let clean = ix.t_check_call_picked(o);
    TAINT_FORCE_ON.with(|c| c.set(false));
    assert!(
        matches!(&refused, Err(Flow::Panic(m)) if m.contains("the candidate picked")),
        "{refused:?}"
    );
    assert!(clean.is_ok(), "{clean:?}");
}

/// T8.1: a candidate PICKS an operator closure out of the table it was handed
/// and passes it through an operator fn's `fn(..)`-typed parameter (a cast
/// re-wrap at the crossing) that the operator then calls.
#[test]
fn a_closure_the_candidate_picked_is_refused_through_a_fn_typed_parameter() {
    let pre = format!("{REF}fn drive(f: fn(i64) -> i64) -> i64 {{ f(21) }}\n");
    // Sealed code sees no operator fn by name: the operator hands it `drive`
    // as a fn value.
    let cand = "fn pickit(ops: [fn(i64) -> i64], d: fn(fn(i64) -> i64) -> i64) -> i64 { d(ops[1]) }\nfn mine(d: fn(fn(i64) -> i64) -> i64) -> i64 { d(|x| x * 2) }\n";
    attack(
        "the candidate picks the closure and hands it to drive",
        &suite(
            &pre,
            &format!("{OPS}    assert(pickit(ops, drive) == reference(21))"),
        ),
        cand,
    );
    control(
        "the operator hands its own pick to drive",
        &suite(
            &pre,
            &format!("{OPS}    assert(drive(ops[1]) == reference(21))"),
        ),
        cand,
    );
    control(
        "the candidate hands drive its own closure",
        &suite(&pre, "    assert(mine(drive) == reference(21))"),
        cand,
    );
}

/// T8.1: a fn VALUE (the AX-25 forwarder `|#0| f(#0)`) the candidate picked
/// is an operator closure like any other: calling it is refused.
#[test]
fn a_fn_value_forwarder_the_candidate_picked_is_never_called() {
    let pre = format!("{REF}fn double(x: i64) -> i64 {{ 0 }}\n");
    let tab = "    let fs = [double, reference]\n";
    attack(
        "a forwarder picked by the candidate's index",
        &suite(
            &pre,
            &format!("{tab}    let f = fs[idx()]\n    assert(f(21) == reference(21))"),
        ),
        IDX,
    );
    control(
        "a forwarder picked by the operator's index",
        &suite(
            &pre,
            &format!("{tab}    let f = fs[1]\n    assert(f(21) == reference(21))"),
        ),
        IDX,
    );
}

/// An echo host on THIS thread for `host_await_val`: every request comes
/// back as the reply, so a value makes the `SendValue` round trip (deep copy:
/// a closure comes back with a NEW capture cell).
fn with_echo_host<R>(f: impl FnOnce() -> R) -> R {
    let (req_tx, req_rx) = std::sync::mpsc::channel::<SendValue>();
    let (rep_tx, rep_rx) = std::sync::mpsc::channel::<Option<SendValue>>();
    let echo = std::thread::spawn(move || {
        for r in req_rx {
            if rep_tx.send(Some(r)).is_err() {
                break;
            }
        }
    });
    HOST_AWAIT.with(|h| *h.borrow_mut() = Some(HostChannels { req_tx, rep_rx }));
    let out = f();
    HOST_AWAIT.with(|h| *h.borrow_mut() = None);
    echo.join().expect("echo host");
    out
}

/// T8.1: a `SendValue` round trip gives a picked closure a new capture cell,
/// so the pick's key is gone; the value's taint is not (the reply carries
/// what the request did), and the value read back is picked again.
#[test]
fn a_sendvalue_round_trip_does_not_launder_a_pick() {
    let body = |i: &str| {
        format!("{OPS}    let f = ops[{i}]\n    let g = host_await_val(f)\n    assert(g(21) == reference(21))")
    };
    with_echo_host(|| {
        attack(
            "a picked closure round-tripped through the host",
            &suite(REF, &body("idx()")),
            IDX,
        );
        control(
            "the operator's own pick round-tripped",
            &suite(REF, &body("1")),
            IDX,
        );
    });
}

/// The PICK sinks and edges main adds (spec Fork 3, T5; TaintPort §1.3 S4):
/// each is pinned by the code that implements it, so a refactor that drops
/// one fails here, not silently. Extends `sink_tests`'s main-only
/// classification (`every_main_only_sink_is_classified`) with the S4 hooks.
#[test]
fn every_pick_edge_and_sink_is_wired() {
    // (what, fn whose body implements it, needles)
    const PICKS: &[(&str, &str, &[&str])] = &[
        ("refusing sink: every closure call (local, expression, fn-typed parameter, builtin callback)", "call_closure_frame", &["self.t_check_call_picked(cv)"]),
        ("frame-exit pick and the candidate closure's hand-back", "call_closure_frame", &["self.t_frame_exit(entry_t, body_t, &v);", "if crossing && self.t_holds_op(&v) {", "self.t_touch(taint::PICK);"]),
        ("frame-exit pick of a named fn", "call_fn_frame", &["self.t_frame_exit(entry_t, body_t, &result);"]),
        ("a sealed fn's hand-back", "call_fn_sealed", &["self.t_touch(taint::PICK);"]),
        ("closure note on every tainted evaluation", "eval_tainted", &["self.t_note(v, mine);", "mine &= !PICK;"]),
        ("closure note at a local closure call (no eval of the callee)", "dispatch_named", &["self.t_note(&c, env.taint(i));"]),
        ("builtin pick on a sealed selector", "dispatch_named", &["self.taint.call_ats.take()", "self.t_builtin_pick("]),
        ("argument taints for the builtin pick", "eval_ident_call_sealed", &["self.taint.call_ats.replace(ats);"]),
        ("argument taints for the builtin pick (name sinks)", "eval_name_sink_call", &["self.taint.call_ats.replace(ats);"]),
        ("index pick (index_in_place and index_value)", "eval_arm", &["self.t_pick_index(it, el);"]),
        ("branch pick (if, match)", "eval_arm", &["self.t_pick_branch(ct, &r);"]),
        ("store pick (assign, place assign)", "eval_arm", &["self.t_stored_v(self.tl::<T>(), &v)", "self.t_stored_v(self.ta::<T>(), &v)"]),
    ];
    let mut bad = Vec::new();
    for (what, at, needles) in PICKS {
        let body = super::sink_tests::fn_body(at);
        for n in *needles {
            if !body.contains(n) {
                bad.push(format!("{what}: `{at}` no longer contains `{n}`"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "DRIFT: a PICK hook is gone:\n{}",
        bad.join("\n")
    );
    // The argument taints are read in one place only: `dispatch_named`.
    let takes: usize = vm::tests::r51_interp_sources()
        .iter()
        .filter(|(f, _)| !f.ends_with("_tests.rs"))
        .map(|(_, s)| s.matches("call_ats.take()").count())
        .sum();
    assert_eq!(
        takes, 1,
        "DRIFT: `taint.call_ats` is read outside dispatch_named"
    );
}

/// One program of a case table (v0.22 taint_tests.rs:81-103): the operator's
/// items and test body, the candidate, and the verdict.
pub(super) struct PickCase {
    pub name: &'static str,
    pub pre: String,
    pub body: String,
    pub cand: String,
    pub expect: Verdict,
}

fn case_suite(c: &PickCase) -> String {
    suite(&c.pre, &c.body)
}

/// v0.22's `check` (taint_tests.rs:119-153) on [`run_unit`]: every case's
/// verdict under the given layers; every mismatch is listed.
pub(super) fn check_unit(cases: &[PickCase], static_on: bool, taint_on: bool) {
    let mut bad: Vec<String> = Vec::new();
    for c in cases {
        let out = run_unit(&case_suite(c), &c.cand, static_on, taint_on);
        let got = verdict(&out);
        if got == c.expect {
            continue;
        }
        if got == Verdict::Ok && c.expect != Verdict::Ok {
            bad.push(format!(
                "ATTACK: <{}> completed (static {static_on}, taint {taint_on})",
                c.name
            ));
        } else {
            bad.push(format!(
                "(static {static_on}, taint {taint_on}): case `{}` should be {:?}, was {got:?}: {out:?}",
                c.name, c.expect
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// v0.22's `attacks_are_live` (taint_tests.rs:156-176): every case that must
/// be refused completes with no rule on.
pub(super) fn attacks_are_live_unit(cases: &[PickCase]) {
    for c in cases.iter().filter(|c| c.expect == Verdict::Refused) {
        let out = run_unit(&case_suite(c), &c.cand, false, false);
        if c.name.starts_with("the wrong") {
            // The control that picks the answer-failing alternative: with the
            // rule off it fails keyed; with the rule on it is refused, because
            // the rule judges who chose, not whether the answer was right.
            assert_eq!(verdict(&out), Verdict::Fails, "{}: {out:?}", c.name);
            continue;
        }
        assert!(
            out.is_ok(),
            "the attack `{}` is not live without the rule (a refusal elsewhere would hide a hole): {out:?}",
            c.name
        );
    }
}
