//! R51 S4: the PICK mark and the refusing closure sink (spec §4 Fork 3
//! "PICK", §8 T8.1), unit by unit.
//!
//! Every sealed run is refused at build until S6 (`SEAL_RUNTIME_COMPLETE`), so
//! [`run_unit`] stands in for v0.22's `run`: it builds the program with a live
//! seal ([`Interp::build_with`], no refuse-all pin), initialises the
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
        matches!(off, Ok(_)),
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
        code: Rc::clone(&p.code),
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
            matches!(out, Ok(_)),
            "the attack `{}` is not live without the rule (a refusal elsewhere would hide a hole): {out:?}",
            c.name
        );
    }
}

// v0.22 taint_tests.rs:178-183.
const V_REF: &str = "fn reference(x: i64) -> i64 { x * 2 }\n";
const V_OPS: &str = "    let ops = [|x| 0, |x| x * 2]\n";
const V_HTAB: &str = "    let h = dict_new()\n    dict_set(h, \"double\", |x| 0)\n    dict_set(h, \"reference\", |x| x * 2)\n";
const V_IDX: &str = "fn idx() -> i64 { 1 }\n";
const V_ENT: &str = "fn entry() -> str { \"reference\" }\nfn double(x: i64) -> i64 { 0 }\n";
const V_SB: &str = "    let p = principal_root(\"r\", true, true, true, 100)\n    let sb = sandbox_create(p, \"IO\")\n";

/// v0.22 taint_tests.rs:185-231 verbatim (C9 round 10/11, amendment 102): an
/// operator closure sealed code picked is never called; the operator naming it,
/// or branching on data to run closures it listed, is honest.
fn closure_cases() -> Vec<PickCase> {
    use Verdict::*;
    let case = |name: &'static str, pre: &str, body: &str, cand: &str, expect| PickCase {
        name,
        pre: pre.to_string(),
        body: body.to_string(),
        cand: cand.to_string(),
        expect,
    };
    let wrong = |s: &str| s.replace("-> i64 { 1 }", "-> i64 { 0 }");
    let table = |body: &str| format!("{V_OPS}{body}");
    let dtab = |body: &str| format!("{V_HTAB}{body}");
    vec![
        // Honest: the operator names the closure, or branches on data to
        // run closures it listed.
        case("lit index", V_REF, &table("    let f = ops[1]\n    assert(f(21) == reference(21))"), V_IDX, Ok),
        case("lit key", V_REF, &dtab("    match dict_get(h, \"reference\") { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), V_ENT, Ok),
        case("statement branch", V_REF, &table("    if idx() == 1 { assert(ops[1](21) == reference(21)) } else { assert(false) }"), V_IDX, Ok),
        case("operator loop over a table", V_REF, &table("    for i in 0..2 { if i == 1 { assert(ops[i](21) == reference(21)) } }"), V_IDX, Ok),
        case("a tainted ARGUMENT, not a tainted callee", V_REF, &table("    assert(ops[1](idx() * 21) == reference(21))"), V_IDX, Ok),
        case("the candidate's own closure", V_REF, "    let g = mk()\n    assert(g(21) == reference(21))", "fn mk() -> fn(i64) -> i64 { |x| x * 2 }\n", Ok),
        case("a wrong answer still fails keyed", V_REF, &table("    assert(ops[0](21) == reference(21))"), V_IDX, Fails),
        // Attacks: sealed code picks the closure.
        case("index", V_REF, &table("    let f = ops[idx()]\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("dict key", V_REF, &dtab("    match dict_get(h, entry()) { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), V_ENT, Refused),
        case("dict key via a local", V_REF, &dtab("    let k = entry()\n    match dict_get(h, k) { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), V_ENT, Refused),
        case("dict key built from it", V_REF, &dtab("    match dict_get(h, \"refer\" + str_slice(entry(), 5, 9)) { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), V_ENT, Refused),
        case("dict key interpolated", V_REF, &dtab("    match dict_get(h, \"{entry()}\") { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), V_ENT, Refused),
        case("index spelled as a comparison", V_REF, &table("    let f = ops[if idx() == 1 { 1 } else { 0 }]\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("index assigned under a tainted branch", V_REF, &table("    let k = 0\n    if idx() == 1 { k = 1 }\n    let f = ops[k]\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("closure assigned under a tainted branch", V_REF, &table("    let f = ops[0]\n    if idx() == 1 { f = ops[1] }\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("closure chosen by an if expression", V_REF, &table("    let f = if idx() == 1 { ops[1] } else { ops[0] }\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("closure chosen by a match", V_REF, &table("    let f = match idx() { 1 => ops[1]  _ => ops[0] }\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("index returned from a tainted branch", &format!("{V_REF}fn pick() -> i64 {{\n    if idx() == 1 {{ return 1 }}\n    0\n}}\n"), &table("    let f = ops[pick()]\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("closure returned from a tainted branch", &format!("{V_REF}fn pickf() -> fn(i64) -> i64 {{\n    if idx() == 1 {{ return |x| x * 2 }}\n    |x| 0\n}}\n"), "    let f = pickf()\n    assert(f(21) == reference(21))", V_IDX, Refused),
        case("index through a helper's parameter", &format!("{V_REF}fn get(ops: [fn(i64) -> i64], i: i64) -> fn(i64) -> i64 {{ ops[i] }}\n"), &table("    let f = get(ops, idx())\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("dict_values then index", V_REF, &dtab("    let vs = dict_values(h)\n    let f = vs[idx()]\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("closure handed to a builtin that calls it", V_REF, &table("    let r = arr_map([21], ops[idx()])\n    assert(r[0] == reference(21))"), V_IDX, Refused),
        case("assigned in a loop the candidate sized", V_REF, &table("    let f = ops[0]\n    for i in 0..idx() { f = ops[1] }\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("assigned in a while the candidate bounds", V_REF, &table("    let f = ops[0]\n    let n = 0\n    while n < idx() {\n        f = ops[1]\n        n = n + 1\n    }\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("held in a struct", &format!("{V_REF}type H = {{ f: fn(i64) -> i64 }}\n"), &table("    let s = H { f: ops[idx()] }\n    assert((s.f)(21) == reference(21))"), V_IDX, Refused),
        case("held in a tuple", V_REF, &table("    let t = (ops[idx()], 1)\n    let f = t.0\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("through a captured index", V_REF, &table("    let k = idx()\n    let g = || ops[k]\n    let f = g()\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("through a closure that calls the candidate", V_REF, &table("    let g = || ops[idx()]\n    let f = g()\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("index through an operator channel", V_REF, &table("    let c = chan<i64>()\n    c.send(idx())\n    let f = ops[c.recv()]\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("index through an operator dict", V_REF, &table("    let d = dict_new()\n    dict_set(d, \"i\", idx())\n    let f = match dict_get(d, \"i\") { Some(i) => ops[i]  None => ops[0] }\n    assert(f(21) == reference(21))"), V_IDX, Refused),
        case("key built with to_str", V_REF, &format!("{V_HTAB}    match dict_get(h, \"r\" + to_str(idx())) {{ Some(f) => assert(f(21) == reference(21))  None => assert(false) }}").replace("\"reference\"", "\"r1\""), V_IDX, Refused),
        case("index from sandbox_run", V_REF, &format!("{V_OPS}{V_SB}    let f = ops[sandbox_run(sb, \"idx1\", 0)]\n    assert(f(21) == reference(21))"), &format!("{V_IDX}fn idx1(x: i64) -> i64 {{ 1 }}\n"), Refused),
        case("index from the scheduler", V_REF, &format!("{V_OPS}    let id = scheduler_spawn(\"idx1\", 0)\n    let n = scheduler_run()\n    let f = ops[scheduler_result(id)]\n    assert(f(21) == reference(21))"), &format!("{V_IDX}fn idx1(x: i64) -> i64 {{ 1 }}\n"), Refused),
        case("index from a goal", V_REF, &format!("{V_OPS}    let s = goal_eval(\"idx\", 0)\n    let f = ops[f64_to_i64(s)]\n    assert(f(21) == reference(21))"), "@[adaptive]\nfn idx(x: i64) -> i64 { 1 }\n", Refused),
        // The wrong key is refused too: the rule is on WHO chose, not on the answer.
        case("the wrong key is refused as well", V_REF, &dtab("    match dict_get(h, entry()) { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), &wrong(V_ENT).replace("\"reference\"", "\"double\""), Refused),
    ]
}

/// v0.22's `an_operator_closure_the_candidate_picked_is_never_called`
/// (taint_tests.rs:233-258) on the unit-level runner: the closure sink.
#[test]
fn an_operator_closure_the_candidate_picked_is_never_called_on_the_unit_runner() {
    let cases = closure_cases();
    check_unit(&cases, false, true);
    check_unit(&cases, true, true);
    attacks_are_live_unit(&cases);
}
