// v0.22 interp::tests rows that R51 S4 owns (spec T3, port map): the
// receiver dispatch rule, the unary width rule under the dispatch-rule switch,
// and the handler-arm and closure pin owner. Ported under their v0.22 names
// (veto interp.rs:6052, 9292, 10178, 10446-10911); included at the end of
// `interp::tests`. Each is a sealed run (`judged_on`), refused until S6
// (`SEALED_RUN_REFUSED`); their unit-level counterparts that run now are in
// `interp/dispatch_tests.rs`. The last one, the width rule (veto 10752), was
// main's unit test in interp/sink_tests.rs: it runs now and asserts its
// runner half is refused until S6. The v0.22 helpers they share live in
// `v022_s4`, glob-imported so that an identically named helper another
// port defines in `tests` shadows them instead of colliding.

use v022_s4::*;

mod v022_s4 {
    use super::*;

    // `ok` on an i64 is the real check (3 * 3 = 9); `ok` on a bool reads the
    // bool — a second, lenient impl the operator wrote for its own reasons.
    // A confused `true` selects it, so each attack below reaches a PASS
    // unless its own guard refuses it (no candidate method is involved, so
    // the dispatch edge cannot stand in for the cast).
    pub(super) const JUDGE: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for bool {\n    fn ok(self: bool) -> bool { self }\n}\n";
    /// Candidate-side laundering: a `true`, typed as whatever the context
    /// wants.
    pub(super) const LAUNDER: &str = "fn stash(v: bool) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";
    pub(super) const CONFUSED: &str =
        "match dict_get(stash(true), \"k\") { Some(v) => v  None => 0 }";

    pub(super) fn judged(tag: &str, suite: &str, cand: &str) -> Result<TestEnd, String> {
        sealed_outcome(
            tag,
            &format!("{JUDGE}{suite}"),
            &format!("{LAUNDER}{}", cand.replace("CONFUSED", CONFUSED)),
            "t",
        )
    }

    /// A control pair on one suite: the right candidate completes, the wrong
    /// one fails — so the attack meets a live check.
    pub(super) fn live(tag: &str, suite: &str, good: &str, wrong: &str) {
        assert_eq!(
            judged(tag, suite, good),
            Ok(TestEnd::Completed),
            "control: the right answer"
        );
        assert!(
            judged(tag, suite, wrong).is_err(),
            "control: the wrong answer fails"
        );
    }

    pub(super) fn confusion_refused(out: &Result<TestEnd, String>) -> bool {
        matches!(out, Err(m) if m.contains("runtime type confusion"))
    }

    /// A suite reading the candidate-written `result` of a dict, either
    /// UNPINNED (`x.ok()` on the read) or PINNED (`let y: i64 = x`).
    pub(super) fn out_suite(setup: &str, unpinned: bool) -> String {
        let read = if unpinned {
            "assert(x.ok())"
        } else {
            "{ let y: i64 = x\n            assert(y.ok()) }"
        };
        format!("@[test]\nfn t() {{\n    {setup}\n    match dict_get(out, \"result\") {{\n        Some(x) => {read}\n        None => assert(false)\n    }}\n}}\n")
    }

    pub(super) const ARM_PRE: &str = "fn v_one() -> i64 { 1 }\n";
    pub(super) const ARM_OVERFLOW: &str =
        "arr_sum_i64([9223372036854775807, 1]) + 0 + (9223372036854775807 + v_one())";

    pub(super) fn arm_suites() -> (String, String, String, String, String) {
        let colliding = "fn helper() -> i64 {\n    let v: i64 = 9\n    println(\"p\")\n    if v.ok() { 1 } else { 0 }\n}\n";
        let plain = "fn helper() -> i64 {\n    println(\"p\")\n    1\n}\n";
        let dispatch = |helper: &str, bind: &str| {
            format!(
            "{ARM_PRE}{helper}@[test]\nfn t() {{\n  {bind}\n  let r = with handler {{ on IO(p) => resume(if v.ok() {{ 0 }} else {{ {ARM_OVERFLOW} }}) }} {{ helper() }}\n  assert(r == 1)\n}}\n"
        )
        };
        let general = format!(
            "{ARM_PRE}{colliding}@[test]\nfn t() {{\n  let v = work(0)\n  let r = with handler {{ on IO(p) => {{\n      let k = resume(0)\n      if v.ok() {{ k }} else {{ {ARM_OVERFLOW} }}\n  }} }} {{ helper() }}\n  assert(r == 1)\n}}\n"
        );
        let width = format!(
            "{ARM_PRE}fn helper() -> i64 {{\n    let v: i64 = 255\n    println(\"p\")\n    let w = v << 1\n    w\n}}\n@[test]\nfn t() {{\n  let v = work(0)\n  let r = with handler {{ on IO(p) => resume(if (v << 1) == 254 {{ 0 }} else {{ {ARM_OVERFLOW} }}) }} {{ helper() }}\n  assert(r == 510)\n}}\n"
        );
        (
            dispatch(colliding, "let v: i64 = work(0)"),
            dispatch(colliding, "let v = work(0)"),
            dispatch(plain, "let v = work(0)"),
            general,
            width,
        )
    }

    /// BLOCKER A. A handler arm runs under the pin owner of the fn that INSTALLED
    /// it. Before, it ran under the owner of the fn that PERFORMED the effect, so
    /// a performer holding an identical, determined site text (`let v: i64 = 9
    /// ... v.ok()`) lent its verdict to the arm's undetermined `v.ok()`.
    pub(super) fn arm_attack(what: &str, suite: &str, cand: &str, msg: &str) {
        let out = judged_on("r10-arm", suite, cand);
        assert!(
            out != Ok(TestEnd::Completed) && matches!(&out, Err(m) if m.contains(msg)),
            "ATTACK: a handler arm borrowed the performer's pin verdict ({what}): {out:?}"
        );
    }

    pub(super) const U8_CAND: &str = "fn work(x: i64) -> u8 { narrow(4) }\n";
    pub(super) const DISP_MSG: &str = "whose type nothing on the operator side determined";
}

/// What stays unaffected: every PINNED receiver dispatches, a method with
/// no impl to choose between dispatches on anything, and outside a sealed
/// run nothing changes.
#[test]
#[ignore = "R51 S6: the receiver dispatch rule seal_dispatch/t_check_dispatch ported (S4); needs a live sealed run"]
fn a_determined_receiver_dispatches_and_so_does_an_unambiguous_method() {
    let pinned = [
        "let r: i64 = solve(3)\n    assert(r.ok())",
        "assert(9.ok())",
        "let w = Wrap { v: 9 }\n    assert(w.v.ok())",
        "let xs = [9]\n    assert(xs[0].ok())",
        "let r: i64 = solve(3)\n    assert((r + 0).ok())",
        "assert((solve(3) as i64).ok())",
    ];
    for body in pinned {
        let suite = format!("@[test]\nfn t() {{\n    {body}\n}}\n");
        let out = judged_on(
            "r6-pin",
            &format!("type Wrap<T> = {{ v: T }}\n{suite}"),
            "fn solve(n: i64) -> i64 { n * n }\n",
        );
        assert_eq!(
            out,
            Ok(TestEnd::Completed),
            "control (pinned): {body}: {out:?}"
        );
    }
    // One impl type only: nothing to choose between.
    let one = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\n@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"k\", solve(3))\n    match dict_get(d, \"k\") {\n        Some(v) => assert(v.ok())\n        None => assert(false)\n    }\n}\n";
    let out = sealed_outcome_rule(
        "r6-one",
        one,
        "fn solve(n: i64) -> i64 { n * n }\n",
        "t",
        true,
    );
    assert_eq!(
        out,
        Ok(TestEnd::Completed),
        "control (a single impl): {out:?}"
    );
    // Outside a sealed run the same program is unchanged.
    let src = format!("{JUDGE8}@[test]\nfn t() {{\n    let d = dict_new()\n    dict_set(d, \"k\", 9)\n    match dict_get(d, \"k\") {{\n        Some(v) => assert(v.ok())\n        None => assert(false)\n    }}\n}}\n");
    let prog = crate::parse_source(&src).expect("parses");
    assert_eq!(run_test_fn_outcome(&prog, "t"), Ok(TestEnd::Completed));
}

/// B1: a type the CANDIDATE declared pins nothing — its fn's declared
/// return, a type it defines, a candidate method's name.
#[test]
#[ignore = "R51 S6: the receiver dispatch rule seal_dispatch/t_check_dispatch ported (S4); needs a live sealed run"]
fn a_type_the_candidate_declared_does_not_determine_the_receiver() {
    let t = |body: &str| format!("@[test]\nfn t() {{\n    {body}\n}}\n");
    r7_refused(
        "a candidate fn declared -> u8",
        &t("assert(solve(3).ok())"),
        "fn solve(n: i64) -> u8 { 255 as u8 }\n",
    );
    r7_refused(
        "the same through an unannotated let",
        &t("let r = solve(3)\n    assert(r.ok())"),
        "fn solve(n: i64) -> u8 { 255 as u8 }\n",
    );
    r7_refused(
        "a candidate-defined struct's u8 field",
        &t("let p: P = solve()\n    assert(p.x.ok())"),
        "type P = { x: u8 }\nfn solve() -> P { P { x: 4 as u8 } }\n",
    );
    r7_refused(
        "arithmetic on a candidate fn declared -> u8",
        &t("assert((solve(3) << 1) == 254)"),
        "fn solve(n: i64) -> u8 { 255 as u8 }\n",
    );
    // The operator's own pin on the SAME call still determines: the cast
    // to the operator's type is the operator's choice.
    let pinned = t("let r: i64 = solve(3)\n    assert(r.ok())");
    assert_eq!(
        judged_on("r7", &pinned, "fn solve(n: i64) -> i64 { n * n }\n"),
        Ok(TestEnd::Completed)
    );
    assert!(judged_on("r7", &pinned, "fn solve(n: i64) -> i64 { n + 1 }\n").is_err());
    r7_refused(
        "a u8 at the i64 pin",
        &pinned,
        "fn solve(n: i64) -> u8 { 255 as u8 }\n",
    );
}

/// The rest of the container family the rule must cover: a payload, a
/// struct field, a tuple element, a generic enum, a channel receive, a
/// closure result, a `match` on the untyped value before the dispatch.
#[test]
#[ignore = "R51 S6: the receiver dispatch rule seal_dispatch/t_check_dispatch ported (S4); needs a live sealed run"]
fn operator_code_never_dispatches_on_a_value_from_any_untyped_position() {
    let wrap = "type Wrap<T> = { v: T }\ntype Opt<T> = Has { v: T } | Nada\n";
    let stash_in = |what: &str| format!("fn solve(d: Dict) {{ dict_set(d, \"k\", {what}) }}\n");
    let cases: Vec<(&str, String, String)> = vec![
        (
            "an Option payload read from a dict",
            "let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(o) => match o {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }\n        None => assert(false)\n    }".into(),
            stash_in("Some(narrow(4))"),
        ),
        (
            "a struct field of a dict value",
            "let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(w) => assert(w.v.ok())\n        None => assert(false)\n    }".into(),
            format!("{wrap}{}", stash_in("Wrap { v: narrow(4) }")),
        ),
        (
            "a tuple element of a dict value",
            "let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(t) => assert(t.0.ok())\n        None => assert(false)\n    }".into(),
            stash_in("(narrow(4), 1)"),
        ),
        (
            "a generic enum payload",
            "let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(e) => match e {\n            Opt::Has { v } => assert(v.ok())\n            Opt::Nada => assert(false)\n        }\n        None => assert(false)\n    }".into(),
            format!("{wrap}{}", stash_in("Opt::Has { v: narrow(4) }")),
        ),
        (
            "a channel receive",
            "let c = chan<i64>()\n    c.send(3)\n    fill(c)\n    assert(c.recv().ok())".into(),
            "fn fill(c: Chan<i64>) { c.send(9) }\n".into(),
        ),
        (
            "an unannotated lambda's result",
            "let f = |x| x\n    assert(f(3).ok())".into(),
            "fn unused() {}\n".into(),
        ),
        (
            "an unannotated lambda parameter",
            "let f = |x| assert(x.ok())\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(v) => f(v)\n        None => assert(false)\n    }".into(),
            stash_in("narrow(4)"),
        ),
        (
            "a match on the untyped value before the dispatch",
            "let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(v) => match v {\n            w => assert(w.ok())\n        }\n        None => assert(false)\n    }".into(),
            stash_in("narrow(4)"),
        ),
    ];
    for (why, body, cand) in cases {
        let suite = format!("@[test]\nfn t() {{\n    {body}\n}}\n");
        // The wrap/enum types are the candidate's here only for the dict
        // cases; the unannotated-lambda and channel cases need no attack.
        dispatch_refused(why, &suite, &cand);
    }
}

/// d2/d3 and the empty accumulator: the candidate writes (or returns) a
/// dict the operator then reads. Unpinned: refused at the dispatch. Pinned
/// by `let y: i64`: GOOD passes, WRONG fails, the u8 is refused by the cast.
#[test]
#[ignore = "R51 S6: the receiver dispatch rule seal_dispatch/t_check_dispatch ported (S4); needs a live sealed run"]
fn operator_code_never_dispatches_on_a_value_read_untyped_from_a_dict() {
    for (why, setup, fill) in [
        (
            "an output dict the candidate fills",
            "let out = dict_new()\n    solve(out)",
            "fn solve(out: Dict) { dict_set(out, \"result\", VAL) }\n",
        ),
        (
            "a returned dict",
            "let out = solve()",
            "fn solve() -> Dict {\n    let d = dict_new()\n    dict_set(d, \"result\", VAL)\n    d\n}\n",
        ),
        (
            "an empty accumulator",
            "let out = dict_new()\n    let n = 0\n    fill(out, n)",
            "fn fill(out: Dict, n: i64) { dict_set(out, \"result\", VAL) }\n",
        ),
    ] {
        let cand = |v: &str| fill.replace("VAL", v);
        let atk = cand("narrow(4)");
        dispatch_refused(why, &out_suite(setup, true), &atk);
        let pinned = out_suite(setup, false);
        assert_eq!(judged_on("r6-disp", &pinned, &cand("9")), Ok(TestEnd::Completed), "control ({why}): GOOD, pinned");
        assert!(judged_on("r6-disp", &pinned, &cand("4")).is_err(), "control ({why}): WRONG fails");
        let out = judged_on("r6-disp", &pinned, &atk);
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a u8 passed the suite's `let y: i64` pin ({why}): {out:?}"
        );
    }
}

/// What is NOT a dispatch, so the rule leaves it alone: interpolation and
/// `to_str` of an untyped read, and a comparison — none selects an
/// operator impl (the language has no operator overloading and no trait
/// default methods), so the candidate's type choice picks no operator code.
#[test]
#[ignore = "R51 S6: the receiver dispatch rule seal_dispatch/t_check_dispatch ported (S4); needs a live sealed run"]
fn interpolation_and_comparison_of_an_untyped_read_select_no_operator_impl() {
    let suite = "@[test]\nfn t() {\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(v) => {\n            assert(\"{v}\" == \"9\")\n            assert(to_str(v) == \"9\")\n            assert(v == 9)\n        }\n        None => assert(false)\n    }\n}\n";
    let out = judged_on(
        "r6-nd",
        suite,
        "fn solve(d: Dict) { dict_set(d, \"k\", 9) }\n",
    );
    assert_eq!(out, Ok(TestEnd::Completed), "control: {out:?}");
    assert!(judged_on(
        "r6-nd",
        suite,
        "fn solve(d: Dict) { dict_set(d, \"k\", 4) }\n"
    )
    .is_err());
}

/// Adjacent (e): an operator method named `recv` does not make a channel
/// read look determined.
#[test]
#[ignore = "R51 S6: the receiver dispatch rule seal_dispatch/t_check_dispatch ported (S4); needs a live sealed run"]
fn an_operator_method_named_like_a_channel_method_does_not_determine_a_recv() {
    // The candidate's own unstamped channel, read through the operator's dict.
    let dsuite = "trait R {\n    fn recv(self) -> i64\n}\nimpl R for bool {\n    fn recv(self: bool) -> i64 { 1 }\n}\nimpl R for i64 {\n    fn recv(self: i64) -> i64 { 2 }\n}\n".to_string()
        + &format!("{JUDGE8}@[test]\nfn t() {{\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"c\") {{\n        Some(c) => assert(c.recv().ok())\n        None => assert(false)\n    }}\n}}\n");
    let out = sealed_outcome_rule(
        "r7",
        &dsuite,
        &format!("{LAUNDER8}fn solve(d: Dict) {{\n    let c = Chan::new(2)\n    c.send(narrow(4))\n    dict_set(d, \"c\", c)\n}}\n"),
        "t",
        true,
    );
    assert!(
        matches!(&out, Err(m) if m.contains("nothing on the operator side determined")),
        "ATTACK: an operator method named recv made a dict-read channel's value look determined: {out:?}"
    );
}

/// C9 round 9 sweep: the dispatch rule's arithmetic arm covered `x + y` but
/// not `-x` / `~x`, which wrap at a width the candidate chose just the same:
/// `(-xs[0]) == 252` completed for a candidate returning `4 as u8`.
#[test]
#[ignore = "R51 S6: seal edge seal_width_unary ported (S3) under the dispatch-rule switch (S4); needs a live sealed run"]
fn operator_unary_arithmetic_never_runs_at_a_width_the_candidate_chose() {
    let run = |suite: &str, cand: &str| judged_on("r9-unary", suite, cand);
    let t =
        |body: &str| format!("@[test]\nfn t() {{\n    let xs = arr_map([1], work)\n{body}\n}}\n");
    let neg = t("    assert((-xs[0]) == 252)");
    let not = t("    assert((~xs[0]) == 251)");
    // Controls: an i64 result is plain arithmetic (-4 != 252: the check is live);
    // a width the OPERATOR chose is determined and wraps as written.
    assert!(
        run(&neg, "fn work(x: i64) -> i64 { 4 }\n").is_err(),
        "control: i64 -4 != 252"
    );
    let own = "@[test]\nfn t() {\n    let w = as_u8(5)\n    assert((-w) == 251)\n    assert((~w) == 250)\n}\n";
    assert_eq!(
        run(own, "fn work(x: i64) -> i64 { 4 }\n"),
        Ok(TestEnd::Completed),
        "control: the operator's own u8"
    );
    let ok_i64 = t("    assert((-xs[0]) == -4)");
    assert_eq!(
        run(&ok_i64, "fn work(x: i64) -> i64 { 4 }\n"),
        Ok(TestEnd::Completed),
        "control: honest i64"
    );
    for (what, suite) in [("negation", &neg), ("bitwise not", &not)] {
        let out = run(suite, "fn work(x: i64) -> u8 { narrow(4) }\n");
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("did arithmetic on a fixed-width integer")),
            "ATTACK: the operator's unary {what} wrapped at a width the candidate chose: {out:?}"
        );
    }
}

/// Adjacent (d): operator-only polymorphic helpers pass (the receiver is
/// an operator struct), a candidate-influenced one is refused.
#[test]
#[ignore = "R51 S6: the receiver dispatch rule seal_dispatch/t_check_dispatch ported (S4); needs a live sealed run"]
fn operator_only_polymorphism_dispatches_and_a_candidate_influenced_one_does_not() {
    let shapes = "trait Shape {\n    fn area(self) -> i64\n}\ntype Sq = { s: i64 }\ntype Rect = { w: i64, h: i64 }\nimpl Shape for Sq {\n    fn area(self: Sq) -> i64 { self.s * self.s }\n}\nimpl Shape for Rect {\n    fn area(self: Rect) -> i64 { self.w * self.h }\n}\nimpl Shape for u8 {\n    fn area(self: u8) -> i64 { 0 }\n}\n";
    for helper in [
        "fn total(a: dyn Shape, b: dyn Shape) -> i64 { a.area() + b.area() }",
        "fn total<T: Shape, U: Shape>(a: T, b: U) -> i64 { a.area() + b.area() }",
    ] {
        let suite = format!("{shapes}{helper}\n@[test]\nfn t() {{\n    let r: i64 = solve(1)\n    assert_eq(total(Sq {{ s: 2 }}, Rect {{ w: 2, h: 3 }}), r)\n}}\n");
        let good = judged_on("r7", &suite, "fn solve(n: i64) -> i64 { 10 }\n");
        assert_eq!(
            good,
            Ok(TestEnd::Completed),
            "control (honest): {helper}: {good:?}"
        );
        assert!(judged_on("r7", &suite, "fn solve(n: i64) -> i64 { 11 }\n").is_err());
    }
    // The candidate supplies a u8 to the generic helper.
    let suite = format!("{shapes}fn one<T: Shape>(a: T) -> i64 {{ a.area() }}\n@[test]\nfn t() {{\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {{\n        Some(v) => assert_eq(one(v), 0)\n        None => assert(false)\n    }}\n}}\n");
    r7_refused(
        "a candidate u8 handed to a generic helper",
        &suite,
        "fn solve(d: Dict) { dict_set(d, \"k\", 4 as u8) }\n",
    );
}

/// The dispatch edge. No type confusion at all: the candidate DECLARES
/// `-> Fake`, and `Fake` has its own `ok`. The operator's `r.ok()` would
/// run it. The candidate's own method names (its API) stay callable.
#[test]
#[ignore = "R51 S6: seal edge seal_method ported (S3) under the dispatch-rule switch (S4); needs a live sealed run"]
fn a_candidates_method_never_runs_under_the_operators_method_name() {
    let suite = "@[test]\nfn t() {\n    let r = solve(3)\n    assert(r.ok())\n}\n\
                 @[test]\nfn t_api() {\n    assert_eq(sq(3).twice(), 18)\n}\n";
    let api = "type Sq = { v: i64 }\ntrait Api {\n    fn twice(self) -> i64\n}\nimpl Api for Sq {\n    fn twice(self: Sq) -> i64 { self.v * 2 }\n}\nfn sq(n: i64) -> Sq { Sq { v: n * n } }\n";
    live(
        "psv1d",
        suite,
        &format!("{api}fn solve(n: i64) -> i64 {{ n * n }}\n"),
        &format!("{api}fn solve(n: i64) -> i64 {{ n + 1 }}\n"),
    );
    let fake = format!(
        "{api}type Fake = {{ v: i64 }}\ntrait Mine {{\n    fn ok(self) -> bool\n}}\nimpl Mine for Fake {{\n    fn ok(self: Fake) -> bool {{ true }}\n}}\nfn solve(n: i64) -> Fake {{ Fake {{ v: n }} }}\n"
    );
    let out = judged("psv1d", suite, &fake);
    assert!(
        out != Ok(TestEnd::Completed),
        "ATTACK: the candidate's `ok` ran under the operator's method name: {out:?}"
    );
    assert!(
        matches!(&out, Err(m) if m.contains("a method the operator defines")),
        "the refusal names the rule: {out:?}"
    );
    assert_eq!(
        sealed_outcome(
            "psv1d",
            &format!("{JUDGE}{suite}"),
            &format!("{LAUNDER}{fake}"),
            "t_api"
        ),
        Ok(TestEnd::Completed),
        "control: the suite still calls the candidate's own API"
    );
}

#[test]
#[ignore = "R51 S6: handler arm pin owner (S3) and the receiver dispatch rule seal_dispatch/t_check_dispatch ported (S4); needs a live sealed run"]
fn a_handler_arm_dispatch_runs_under_the_installers_pin_owner() {
    let (pinned, colliding, plain, _, _) = arm_suites();
    let run = |suite: &str, cand: &str| judged_on("r10-arm", suite, cand);
    // Controls: honest pinned i64 9 passes, the wrong i64 fails, and without a
    // colliding site the u8 is refused by its OWN site.
    assert_eq!(
        run(&pinned, "fn work(x: i64) -> i64 { 9 }\n"),
        Ok(TestEnd::Completed),
        "control: honest pinned"
    );
    assert!(
        run(&pinned, "fn work(x: i64) -> i64 { 4 }\n").is_err(),
        "control: wrong i64 fails"
    );
    let out = run(&plain, U8_CAND);
    assert!(
        matches!(&out, Err(m) if m.contains(DISP_MSG)),
        "control: no colliding site: {out:?}"
    );
    arm_attack(
        "colliding site, dispatch arm",
        &colliding,
        U8_CAND,
        DISP_MSG,
    );
}

#[test]
#[ignore = "R51 S6: handler arm pin owner (S3) and the receiver dispatch rule seal_dispatch/t_check_dispatch ported (S4); needs a live sealed run"]
fn a_handler_arm_replay_runs_under_the_installers_pin_owner() {
    let (_, _, _, general, _) = arm_suites();
    arm_attack(
        "colliding site, general replay arm",
        &general,
        U8_CAND,
        DISP_MSG,
    );
}

#[test]
#[ignore = "R51 S6: handler arm pin owner (S3) and the receiver dispatch rule seal_dispatch/t_check_dispatch ported (S4); needs a live sealed run"]
fn a_handler_arm_arithmetic_runs_under_the_installers_pin_owner() {
    let (_, _, _, _, width) = arm_suites();
    // Control: the suite discriminates. An i64 255 does not wrap, so the arm
    // takes its overflow branch and the run FAILS (a keyed failure, not a pass).
    let wide = judged_on("r10-arm", &width, "fn work(x: i64) -> i64 { 255 }\n");
    assert!(
        wide.is_err() && !matches!(&wide, Err(m) if m.contains("determined")),
        "control: honest i64 255 fails on the overflow, not on the rule: {wide:?}"
    );
    arm_attack(
        "colliding site, arithmetic arm",
        &width,
        "fn work(x: i64) -> u8 { narrow(255) }\n",
        "whose width nothing on the operator side determined",
    );
}

/// A closure made INSIDE an arm remembers the owner the arm ran under
/// (`PIN_FN_MARK`), and so is judged by the installing fn too.
#[test]
#[ignore = "R51 S6: closure pin owner (make_closure, S4) and the receiver dispatch rule ported (S4); needs a live sealed run"]
fn a_closure_made_in_a_handler_arm_is_judged_by_the_installing_fn() {
    let colliding = "fn helper() -> i64 {\n    let v: i64 = 9\n    println(\"p\")\n    if v.ok() { 1 } else { 0 }\n}\n";
    let suite = |bind: &str| {
        format!(
        "{ARM_PRE}{colliding}@[test]\nfn t() {{\n  {bind}\n  let r = with handler {{ on IO(p) => {{\n      let f = |a: i64| if v.ok() {{ 0 }} else {{ {ARM_OVERFLOW} }}\n      let k = resume(f(1))\n      k\n  }} }} {{ helper() }}\n  assert(r == 1)\n}}\n"
    )
    };
    assert_eq!(
        judged_on(
            "r10-arm",
            &suite("let v: i64 = work(0)"),
            "fn work(x: i64) -> i64 { 9 }\n"
        ),
        Ok(TestEnd::Completed),
        "control: honest pinned"
    );
    arm_attack(
        "colliding site, closure made in an arm",
        &suite("let v = work(0)"),
        U8_CAND,
        DISP_MSG,
    );
}

/// v0.22 interp.rs:10752 (moved here from interp/sink_tests.rs, T3: the v0.22
/// module path), with main's unit-level strengthening and the unary form
/// (10178): operator arithmetic on
/// a fixed-width integer whose width nothing on the operator side determined
/// is refused - statically (`seal_width`, `seal_width_unary`: a value read
/// untyped from a dict) and by the runtime taint (`t_check_width`: a typed
/// parameter whose value carries the candidate's type taint). A comparison is
/// not arithmetic; a `let x: u8` pin determines the width; a sealed frame's
/// own arithmetic is its own. v0.22's own half (the candidate stores
/// `255 as u8` and the operator's untyped `v << 1 == 254` truncates to a pass)
/// runs through the sealed runner, so until S6 it asserts that the runner
/// refuses the run (`SEALED_RUN_REFUSED`).
#[test]
fn operator_arithmetic_never_runs_at_a_width_the_candidate_chose() {
    sink_tests::with_ix("r51-s3-width", true, |ix| {
        let d = || sink_tests::dict_k(sink_tests::u8v(255));
        let r = sink_tests::go(ix, "shift", false, vec![d()]);
        assert!(
            sink_tests::refused(&r, sink_tests::WIDTH),
            "ATTACK: `v << 1` at the candidate's width: {r:?}"
        );
        let r = sink_tests::go(ix, "neg", false, vec![d()]);
        assert!(
            sink_tests::refused(&r, sink_tests::WIDTH),
            "ATTACK: `-v` at the candidate's width: {r:?}"
        );
        assert!(
            sink_tests::go(ix, "compare", false, vec![d()]).is_ok(),
            "a comparison"
        );
        assert!(
            sink_tests::go(ix, "shift", false, vec![sink_tests::dict_k(Value::Int(4))]).is_ok(),
            "an i64"
        );
        assert!(
            sink_tests::go(ix, "shift_pinned", false, vec![d()]).is_ok(),
            "pinned by `let x: u8`"
        );
        assert!(
            sink_tests::go(ix, "shift", true, vec![d()]).is_ok(),
            "the sealed frame's own"
        );
        assert!(
            sink_tests::go(ix, "neg", true, vec![d()]).is_ok(),
            "the sealed frame's own"
        );
        sink_tests::taint_rules_on();
        // The runtime rule: determined statically, the value's TYPE taint
        // says the candidate chose it.
        assert!(sink_tests::go_t(ix, "shift_t", false, vec![(sink_tests::u8v(255), 0)]).is_ok());
        let r = sink_tests::go_t(
            ix,
            "shift_t",
            false,
            vec![(sink_tests::u8v(255), super::taint::ALL)],
        );
        assert!(
            sink_tests::refused(&r, sink_tests::WIDTH) && sink_tests::refused(&r, "runtime taint"),
            "ATTACK: a tainted width: {r:?}"
        );
        let r = sink_tests::go_t(
            ix,
            "neg_t",
            false,
            vec![(sink_tests::u8v(4), super::taint::ALL)],
        );
        assert!(
            sink_tests::refused(&r, "runtime taint"),
            "ATTACK: a tainted unary width: {r:?}"
        );
        assert!(
            sink_tests::go_t(
                ix,
                "neg_t",
                false,
                vec![(sink_tests::u8v(4), super::taint::VAL)]
            )
            .is_ok(),
            "super::taint::VAL is not TYP"
        );
    });
    sink_tests::with_ix("r51-s3-width-u", false, |ix| {
        assert!(
            sink_tests::go(
                ix,
                "shift",
                false,
                vec![sink_tests::dict_k(sink_tests::u8v(255))]
            )
            .is_ok(),
            "unsealed"
        );
    });
    // v0.22's half, through the sealed runner.
    let suite = |read: &str| {
        format!("@[test]\nfn t() {{\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {{\n        Some(v) => {read}\n        None => assert(false)\n    }}\n}}\n")
    };
    let cand = |v: &str| format!("fn solve(d: Dict) {{ dict_set(d, \"k\", {v}) }}\n");
    let out = judged_on(
        "r6-arith",
        &suite("assert((v << 1) == 254)"),
        &cand("255 as u8"),
    );
    if !SEAL_RUNTIME_COMPLETE {
        assert_eq!(
            out,
            Err(SEALED_RUN_REFUSED.to_string()),
            "until S6 the runner refuses"
        );
        return;
    }
    assert!(
        out != Ok(TestEnd::Completed)
            && matches!(&out, Err(m) if m.contains("whose width nothing on the operator side determined")),
        "ATTACK: a u8 the candidate chose truncated the operator's arithmetic into a pass: {out:?}"
    );
    let pinned = suite("{ let y: i64 = v\n            assert((y << 1) == 254) }");
    assert_eq!(
        judged_on("r6-arith", &pinned, &cand("127")),
        Ok(TestEnd::Completed),
        "control: GOOD, pinned"
    );
    assert!(
        judged_on("r6-arith", &pinned, &cand("5")).is_err(),
        "control: WRONG fails"
    );
    let out = judged_on("r6-arith", &pinned, &cand("255 as u8"));
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a u8 passed the suite's `let y: i64` pin: {out:?}"
    );
}
