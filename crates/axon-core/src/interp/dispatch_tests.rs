//! R51 S4: the receiver DISPATCH rule at run time (`t_check_dispatch`, v0.22
//! taint.rs:1414-1476) at the tree's `MethodCall` arm (`impl_method_sealed`),
//! the per-thread test switches (`DISPATCH_RULE_OFF` for the static rule,
//! `TAINT_FORCE_ON` for the runtime taint rules, v0.22 interp.rs:475-486)
//! and the sealed caller's uniform refusal at the goal-name sinks (v0.22
//! goal.rs:272-554, interp.rs:4946-4967).
//!
//! Every sealed run is refused at build until S6 (`SEAL_RUNTIME_COMPLETE`), so
//! these drive an `Interp` made by [`Interp::build_with`] through the
//! `sink_tests` helpers: a suite fn's body runs through `eval_t::<true>` in an
//! operator frame (or a sealed one), its parameters bound with the taint the
//! test gives them.

use super::sink_tests::{dict_k, go, go_t, refused, u8v, with_src, yields};
use super::taint::{PICK, TYP, VAL};
use super::*;

/// The operator: one trait with impls on `i64` (the strict one), `u8` (the
/// lenient one), and two operator record types; a method only `i64` defines.
const SUITE: &str = "\
trait Judge {
    fn ok(self) -> bool
}
impl Judge for i64 {
    fn ok(self: i64) -> bool { self == 9 }
}
impl Judge for u8 {
    fn ok(self: u8) -> bool { true }
}
type Sq = { s: i64 }
type Tri = { t: i64 }
impl Judge for Sq {
    fn ok(self: Sq) -> bool { self.s == 9 }
}
impl Judge for Tri {
    fn ok(self: Tri) -> bool { true }
}
trait Solo {
    fn only(self) -> bool
}
impl Solo for i64 {
    fn only(self: i64) -> bool { self == 9 }
}
fn judge_p(x: i64) -> bool { x.ok() }
fn judge_pinned(x: i64) -> bool {
    let y: i64 = x
    y.ok()
}
fn judge_sq(x: Sq) -> bool { x.ok() }
fn judge_only(x: i64) -> bool { x.only() }
fn mk_sq(n: i64) -> Sq { Sq { s: n } }
fn mk_tri() -> Tri { Tri { t: 1 } }
";

/// The sealed candidate.
const CAND: &str = "\
fn solve() -> i64 { 41 }
";

const UNDETERMINED: &str = "whose type nothing on the operator side determined";
const PICKED: &str = "on a value of an operator type that the candidate picked";
const TAINT: &str = "(runtime taint)";

/// v0.22 `t_check_dispatch(_tainted)` (taint.rs:1414-1476): with the taint
/// rules on, operator code never selects between its impls by a receiver
/// whose runtime TYPE sealed code chose (`TYP`), nor by an operator-typed
/// value sealed code PICKED (`PICK`). The static rule cannot see either: the
/// receiver is a parameter the operator annotated. Honest controls: a clean
/// receiver, a `VAL`-only one, a `let y: i64` pin, an operator value only
/// `TYP`-tainted, a method no second impl defines, a sealed frame's own
/// dispatch and an unsealed run. With the rules off the attacks are live.
#[test]
fn operator_code_never_dispatches_on_a_receiver_type_the_candidate_chose() {
    with_src("r51-s4-tdisp", SUITE, CAND, true, |ix| {
        let tri = go(ix, "mk_tri", false, vec![]).expect("a Tri");
        let sq = |n| go(ix, "mk_sq", false, vec![Value::Int(n)]).expect("a Sq");
        // Rules off (a unit test's default): each attack is live, a keyed pass.
        let live = [
            go_t(ix, "judge_p", false, vec![(u8v(4), TYP)]),
            go_t(ix, "judge_sq", false, vec![(tri.clone(), PICK)]),
        ];
        for r in &live {
            assert!(yields(r, &Value::Bool(true)), "the attack is live: {r:?}");
        }
        sink_tests::taint_rules_on();
        let r = go_t(ix, "judge_p", false, vec![(u8v(4), TYP)]);
        assert!(
            refused(&r, UNDETERMINED) && refused(&r, TAINT) && refused(&r, "`u8`"),
            "ATTACK: the candidate's u8 chose the lenient impl: {r:?}"
        );
        let r = go_t(ix, "judge_sq", false, vec![(tri.clone(), PICK)]);
        assert!(
            refused(&r, PICKED) && refused(&r, "`Tri`"),
            "ATTACK: an operator value the candidate picked chose the impl: {r:?}"
        );
        let r = go_t(ix, "judge_sq", false, vec![(sq(9), PICK | TYP)]);
        assert!(refused(&r, PICKED), "a picked Sq, honest or not: {r:?}");
        // Controls.
        let ok = |r: Result<Value, Flow>, want: bool, why: &str| {
            assert!(yields(&r, &Value::Bool(want)), "control ({why}): {r:?}");
        };
        ok(
            go_t(ix, "judge_p", false, vec![(Value::Int(9), 0)]),
            true,
            "clean GOOD",
        );
        ok(
            go_t(ix, "judge_p", false, vec![(Value::Int(4), 0)]),
            false,
            "clean WRONG",
        );
        ok(
            go_t(ix, "judge_p", false, vec![(Value::Int(9), VAL)]),
            true,
            "VAL is not TYP",
        );
        ok(
            go_t(ix, "judge_pinned", false, vec![(Value::Int(9), TYP)]),
            true,
            "pinned by `let y: i64`",
        );
        ok(
            go_t(ix, "judge_sq", false, vec![(sq(9), TYP)]),
            true,
            "an operator type, not picked",
        );
        ok(
            go_t(ix, "judge_sq", false, vec![(sq(4), TYP)]),
            false,
            "an operator type, WRONG",
        );
        ok(
            go_t(ix, "judge_only", false, vec![(Value::Int(9), TYP)]),
            true,
            "no impl to choose between",
        );
        // In a sealed frame the method edge answers first (an operator method
        // is invisible to sealed code), never the operator's dispatch rule.
        let r = go_t(ix, "judge_p", true, vec![(u8v(4), TYP)]);
        assert!(
            refused(&r, &sink_tests::no_fn("ok")) && !refused(&r, TAINT),
            "control (a sealed frame's dispatch is not the operator's): {r:?}"
        );
    });
    with_src("r51-s4-tdisp-u", SUITE, CAND, false, |ix| {
        sink_tests::taint_rules_on();
        let r = go_t(ix, "judge_p", false, vec![(u8v(4), TYP)]);
        assert!(yields(&r, &Value::Bool(true)), "unsealed: no rule: {r:?}");
    });
}

/// The test switches are per thread and OFF by default (v0.22's were
/// process-global under `SEALED_DIRS_TEST_LOCK`; main has no such lock): a
/// fresh thread sees the static rule ON and the taint rules OFF, and setting
/// either on one thread leaves another's untouched. `DISPATCH_RULE_OFF`
/// switches the static receiver rule (`seal_dispatch`) and nothing else.
#[test]
fn the_seal_test_switches_are_per_thread_and_off_by_default() {
    let fresh = || {
        (
            DISPATCH_RULE_OFF.with(Cell::get),
            TAINT_FORCE_ON.with(Cell::get),
        )
    };
    assert_eq!(fresh(), (false, false), "defaults");
    DISPATCH_RULE_OFF.with(|c| c.set(true));
    TAINT_FORCE_ON.with(|c| c.set(true));
    assert_eq!(
        std::thread::spawn(fresh).join().expect("probe thread"),
        (false, false),
        "another thread's switches"
    );
    DISPATCH_RULE_OFF.with(|c| c.set(false));
    TAINT_FORCE_ON.with(|c| c.set(false));
    // The static rule refuses a receiver read untyped from a dict, unless switched off.
    with_src("r51-s4-switch", SUITE_DICT, CAND, true, |ix| {
        let d = || dict_k(u8v(4));
        let r = go(ix, "judge_dict", false, vec![d()]);
        assert!(
            refused(&r, UNDETERMINED) && !refused(&r, TAINT),
            "static rule on: {r:?}"
        );
        DISPATCH_RULE_OFF.with(|c| c.set(true));
        let r = go(ix, "judge_dict", false, vec![d()]);
        DISPATCH_RULE_OFF.with(|c| c.set(false));
        assert!(
            yields(&r, &Value::Bool(true)),
            "static rule off: the attack is live: {r:?}"
        );
        assert!(yields(
            &go(ix, "judge_lit", false, vec![]),
            &Value::Bool(true)
        ));
    });
}

/// [`SUITE`] with a receiver read untyped from a dict.
const SUITE_DICT: &str = "\
trait Judge {
    fn ok(self) -> bool
}
impl Judge for i64 {
    fn ok(self: i64) -> bool { self == 9 }
}
impl Judge for u8 {
    fn ok(self: u8) -> bool { true }
}
fn judge_dict(d: Dict) -> bool {
    match dict_get(d, \"k\") {
        Some(v) => v.ok()
        None => false
    }
}
fn judge_lit() -> bool { 9.ok() }
";
