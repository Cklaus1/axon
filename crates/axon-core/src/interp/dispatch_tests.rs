//! R51 S4: the receiver DISPATCH rule at run time (`t_check_dispatch`, v0.22
//! taint.rs:1414-1476) at the tree's `MethodCall` arm (`impl_method_sealed`),
//! the per-thread test switches (`DISPATCH_RULE_OFF` for the static rule,
//! `TAINT_FORCE_ON` for the runtime taint rules, v0.22 interp.rs:475-486)
//! and the sealed caller's uniform refusal at the goal-name sinks (v0.22
//! goal.rs:272-554, interp.rs:4946-4967).
//!
//! These drive an `Interp` made by [`Interp::build_with`] through the
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

/// The operator's judge: a strict `i64` impl, a lenient `u8` one.
/// The operator's judge: a strict `i64` impl, a lenient `u8` one.
const J: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for u8 {\n    fn ok(self: u8) -> bool { true }\n}\n";

/// The undetermined-position operator: it reads back a value the candidate
/// stored (a key it never held), through a channel it filled, a fn with no
/// declared return type, an erased `Wrap<T>` and a free `fn(T)` (veto
/// sealed_frames.rs:629, cases B1-B4), and judges each with `.ok()`.
const SUITE_POS: &str = "\
fn t_dict() -> bool {
    let d = dict_new()
    fill_d(d)
    match dict_get(d, \"k\") {
        Some(v) => v.ok()
        None => false
    }
}
fn t_dict_pinned() -> bool {
    let d = dict_new()
    fill_d(d)
    match dict_get(d, \"k\") {
        Some(v) => {
            let y: i64 = v
            y.ok()
        }
        None => false
    }
}
fn t_chan() -> bool {
    let c = chan<i64>()
    fill(c)
    let r: i64 = c.recv()
    r.ok()
}
fn t_noret() -> bool {
    let r: i64 = solve(3)
    r.ok()
}
fn t_wrap() -> bool {
    let w = solve_w(Wrap { v: 3 })
    let x: i64 = w.v
    x.ok()
}
fn t_clos() -> bool { apply(|x: i64| x.ok()) }
";

/// The candidate's laundering helpers (veto sealed_frames.rs:631): a `u8`
/// made out of an `i64`, read back untyped out of a dict.
const LAUNDER: &str = "\
fn narrow(n: i64) -> u8 { n as u8 }
fn stash(v: u8) -> Dict {
    let d = dict_new()
    dict_set(d, \"k\", v)
    d
}
pub type Wrap<T> = { v: T }
";

/// A candidate for [`SUITE_POS`]: `v` is the value each entry point hands
/// back (`9` GOOD, `4` WRONG, `LAUNDERED` the attack's `u8`).
fn pos_cand(v: &str) -> String {
    let some = |use_v: &str| {
        if v == LAUNDERED {
            let use_v = use_v.replace("{V}", "v");
            format!("match dict_get(stash(narrow(4)), \"k\") {{\n        Some(v) => {use_v}\n        None => {{}}\n    }}")
        } else {
            use_v.replace("{V}", v)
        }
    };
    format!(
        "{LAUNDER}pub fn fill_d(d: Dict) {{\n    {}\n}}\npub fn fill<T>(c: Chan<T>) {{\n    {}\n}}\npub fn solve(n: i64) -> i64 {{\n    {}\n    0\n}}\npub fn solve_w<T>(w: Wrap<T>) -> Wrap<T> {{\n    {}\n    w\n}}\npub fn apply<T>(f: fn(T) -> bool) -> bool {{\n    {}\n    false\n}}\n",
        some("dict_set(d, \"k\", {V})"),
        some("c.send({V})"),
        some("return {V}"),
        some("return Wrap { v: {V} }"),
        some("return f({V})"),
    )
}

/// Marks [`pos_cand`]'s attack.
const LAUNDERED: &str = "laundered";

/// Which seal layers judge a run (veto taint_tests.rs:14-24).
#[derive(Clone, Copy, Debug)]
enum Rules {
    /// The static pin analysis and the runtime taint.
    Both,
    /// The runtime taint only (`DISPATCH_RULE_OFF`): what it refuses alone.
    TaintOnly,
    /// The static pin analysis only: the earlier rounds' layer.
    StaticOnly,
    /// Neither: the attack is live.
    Neither,
}

/// Set this thread's switches for `r`.
fn rules(r: Rules) {
    let (static_on, taint_on) = match r {
        Rules::Both => (true, true),
        Rules::TaintOnly => (false, true),
        Rules::StaticOnly => (true, false),
        Rules::Neither => (false, false),
    };
    DISPATCH_RULE_OFF.with(|c| c.set(!static_on));
    TAINT_FORCE_ON.with(|c| c.set(taint_on));
}

const ALL_RULES: [Rules; 4] = [
    Rules::Both,
    Rules::TaintOnly,
    Rules::StaticOnly,
    Rules::Neither,
];

/// v0.22 `an_undetermined_type_position_never_selects_the_operators_impl`
/// (veto sealed_frames.rs:629, C9 round 4c, amendment 72), unit level: the
/// operator never runs its lenient `u8` impl of `Judge` on a value whose
/// type a candidate chose at a position nothing on the operator side
/// determined. An untyped read back out of a dict the candidate filled is
/// refused by the static rule (`seal_dispatch`) and, with it off, by the
/// runtime one (`t_check_dispatch`): live only with both off. The four
/// positions veto names (a generic `Chan<T>`, no declared return, an erased
/// `Wrap<T>`, a free `fn(T)`) are refused under every rule setting by the
/// declared-type casts (S3), which is where main closes them. Controls:
/// GOOD (`9`) is a pass and WRONG (`4`) a failure everywhere a pin decides.
#[test]
fn an_undetermined_type_position_never_selects_the_operators_impl() {
    let cases = ["t_chan", "t_noret", "t_wrap", "t_clos"];
    let mut fails = Vec::new();
    for r in ALL_RULES {
        rules(r);
        let tag = format!("r51-s4-pos-{r:?}");
        with_src(
            &tag,
            &format!("{J}{SUITE_POS}"),
            &pos_cand("9"),
            true,
            |ix| {
                for p in cases.iter().chain(&["t_dict_pinned"]) {
                    let o = go(ix, p, false, vec![]);
                    if !yields(&o, &Value::Bool(true)) {
                        fails.push(format!("control GOOD {r:?} {p}: {o:?}"));
                    }
                }
            },
        );
        with_src(
            &format!("{tag}-w"),
            &format!("{J}{SUITE_POS}"),
            &pos_cand("4"),
            true,
            |ix| {
                for p in cases.iter().chain(&["t_dict_pinned"]) {
                    let o = go(ix, p, false, vec![]);
                    if !yields(&o, &Value::Bool(false)) {
                        fails.push(format!("control WRONG {r:?} {p}: {o:?}"));
                    }
                }
            },
        );
        with_src(
            &format!("{tag}-a"),
            &format!("{J}{SUITE_POS}"),
            &pos_cand(LAUNDERED),
            true,
            |ix| {
                for p in cases.iter().chain(&["t_dict_pinned"]) {
                    let o = go(ix, p, false, vec![]);
                    if o.is_ok() {
                        fails.push(format!(
                            "ATTACK {r:?} {p} (an undetermined position): {o:?}"
                        ));
                    }
                }
                let o = go(ix, "t_dict", false, vec![]);
                let want = match r {
                    Rules::Neither => None,
                    Rules::TaintOnly => Some(TAINT),
                    Rules::Both | Rules::StaticOnly => Some(UNDETERMINED),
                };
                match want {
                    None if !yields(&o, &Value::Bool(true)) => {
                        fails.push(format!("rules off: the attack is live: {o:?}"));
                    }
                    Some(w) if !(refused(&o, UNDETERMINED) && refused(&o, w)) => {
                        fails.push(format!("ATTACK {r:?} t_dict (an untyped read): {o:?}"));
                    }
                    _ => {}
                }
            },
        );
    }
    rules_reset();
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// Back to a unit test's defaults: the static rule on, the taint rules off.
fn rules_reset() {
    DISPATCH_RULE_OFF.with(|c| c.set(false));
    TAINT_FORCE_ON.with(|c| c.set(false));
}

/// The by-name half (veto sealed_frames.rs:548, amendment 60): a table the
/// candidate filled holds `0` where the operator expects a closure; the
/// operator calls it through the local `square`, which also names its own
/// `fn square`. In a sealed run the local is what the name means, and a `0`
/// is not callable: the call never falls through to the operator's fn. The
/// by-width half: a `u8` at a declared `i64` return is refused by the return
/// cast, pinned or not. Under every rule setting. Controls: a closure table
/// (GOOD `n * n`, WRONG `n + 1`), and an unsealed run, which keeps main's
/// fall-through to the fn.
#[test]
fn the_candidate_never_selects_the_operators_code_by_width_or_by_name() {
    const SUITE_T: &str = "\
fn square(n: i64) -> i64 { n * n }
fn t_table() -> bool {
    match dict_get(table(), \"sq\") {
        Some(square) => square(3) == 9 && square(5) == 25
        None => false
    }
}
fn judge(x: i64) -> bool { x.ok() }
fn t_width() -> bool {
    let r: i64 = solve_width(3)
    judge(r)
}
fn t_width_bare() -> bool {
    let r: i64 = solve_width(3)
    r.ok()
}
";
    let cand = |sq: &str, width: &str| {
        format!(
            "{LAUNDER}pub fn table() -> Dict {{\n    let d = dict_new()\n    dict_set(d, \"sq\", {sq})\n    d\n}}\npub fn solve_width(n: i64) -> i64 {{\n    {width}\n}}\n"
        )
    };
    let laundered =
        "match dict_get(stash(narrow(4)), \"k\") {\n        Some(v) => v\n        None => 0\n    }";
    let suite = format!("{J}{SUITE_T}");
    let mut fails = Vec::new();
    for r in ALL_RULES {
        rules(r);
        let tag = format!("r51-s4-wn-{r:?}");
        for (k, sq, w, want) in [
            ("g", "|n: i64| n * n", "n * n", true),
            ("w", "|n: i64| n + 1", "n + 1", false),
        ] {
            with_src(&format!("{tag}-{k}"), &suite, &cand(sq, w), true, |ix| {
                for p in ["t_table", "t_width", "t_width_bare"] {
                    let o = go(ix, p, false, vec![]);
                    if !yields(&o, &Value::Bool(want)) {
                        fails.push(format!("control {k} {r:?} {p}: {o:?}"));
                    }
                }
            });
        }
        with_src(
            &format!("{tag}-a"),
            &suite,
            &cand("0", laundered),
            true,
            |ix| {
                let o = go(ix, "t_table", false, vec![]);
                if !refused(&o, "value of type i64 is not callable") {
                    fails.push(format!(
                        "ATTACK {r:?}: a 0 in a table ran the operator's `square`: {o:?}"
                    ));
                }
                for p in ["t_width", "t_width_bare"] {
                    let o = go(ix, p, false, vec![]);
                    if !refused(&o, "runtime type confusion") {
                        fails.push(format!("ATTACK {r:?} {p}: a u8 at a declared i64: {o:?}"));
                    }
                }
            },
        );
    }
    rules_reset();
    with_src("r51-s4-wn-u", &suite, &cand("0", "n * n"), false, |ix| {
        let o = go(ix, "t_table", false, vec![]);
        if !yields(&o, &Value::Bool(true)) {
            fails.push(format!(
                "unsealed: main's fall-through to the fn is unchanged: {o:?}"
            ));
        }
    });
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// v0.22 amendment 117 (loop findings 38/39; veto goal.rs:272-554,
/// interp.rs:4946-4967), the goal-name half of
/// `a_sealed_caller_is_refused_in_the_same_words_for_an_operator_name_and_a_missing_one`
/// (sealed_frames.rs:1847-1852): a sealed caller naming an operator fn at a
/// goal builtin is told the same words as for a name nothing defines - the
/// common no-such-name text - for `goal_run`, `goal_run_categorical`,
/// `goal_run_random` and `goal_run_multistart`, including an operator goal
/// that already has recorded provenance (the call edge, `seal_call`).
/// Controls: the operator's own call of the same goal runs, and its call of
/// a missing name keeps the goal-specific text.
#[test]
fn a_sealed_caller_is_refused_in_the_same_words_for_an_operator_goal_name_and_a_missing_one() {
    const SUITE_G: &str = "\
fn nonadapt(n: i64) -> i64 { n }
fn op_run() -> f64 { goal_run(\"nonadapt\", 100.0, 3) }
fn op_missing() -> f64 { goal_run(\"zznosuch\", 100.0, 3) }
";
    let calls = [
        "goal_run(\"N\", 100.0, 5)",
        "goal_run_categorical(\"N\", 3, 100.0, 5)",
        "goal_run_random(\"N\", 100.0, 5, 0, 10)",
        "goal_run_multistart(\"N\", 100.0, 2, 3, 0, 10)",
    ];
    // The honest control: the candidate's own goal fn, run by the candidate.
    let mut cand = String::from("pub fn mygoal(n: i64) -> i64 { n }\n");
    for (i, c) in calls.iter().enumerate() {
        cand.push_str(&format!(
            "pub fn c{i}_own() -> f64 {{ {} }}\n",
            c.replace('N', "mygoal")
        ));
        for (k, n) in [("op", "nonadapt"), ("miss", "zznosuch")] {
            cand.push_str(&format!(
                "pub fn c{i}_{k}() -> f64 {{ {} }}\n",
                c.replace('N', n)
            ));
        }
    }
    let mut fails = Vec::new();
    for (fresh, tag) in [(true, "fresh"), (false, "recorded")] {
        with_src(&format!("r51-s4-goal-{tag}"), SUITE_G, &cand, true, |ix| {
            if !fresh {
                // The operator's own run records `nonadapt`'s provenance.
                let o = go(ix, "op_run", false, vec![]);
                if o.is_err() {
                    fails.push(format!("control: the operator's own goal_run: {o:?}"));
                }
            }
            for (i, call) in calls.iter().enumerate() {
                let o = go(ix, &format!("c{i}_own"), true, vec![]);
                if o.is_err() {
                    fails.push(format!(
                        "control ({tag}) {call}: the candidate's own goal: {o:?}"
                    ));
                }
                let text = |k: &str, name: &str| match go(ix, &format!("c{i}_{k}"), true, vec![]) {
                    Err(Flow::Panic(m)) => m.replace(name, "NAME"),
                    o => format!("not refused: {o:?}"),
                };
                let (op, miss) = (text("op", "nonadapt"), text("miss", "zznosuch"));
                let want = sink_tests::no_fn("NAME");
                if op != want || miss != want {
                    fails.push(format!(
                        "ATTACK ({tag}) {call}: operator {op:?} vs missing {miss:?}"
                    ));
                }
            }
        });
    }
    with_src("r51-s4-goal-op", SUITE_G, &cand, true, |ix| {
        let o = go(ix, "op_missing", false, vec![]);
        if !refused(&o, "goal function `zznosuch` is not defined") {
            fails.push(format!("control: the operator's missing goal: {o:?}"));
        }
    });
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// Drift (R51 S4): every goal search that resolves its goal by name in
/// interp/goal.rs applies the call edge (`seal_call`) to the fn it found, so a
/// sealed caller naming an operator goal gets the no-such-fn text. The two
/// lookups that do not are listed: the held-out eval filters its lookup by the
/// frame (same text), and a constraint fn is named by the operator's own
/// `@[adaptive]` attribute, never by a caller.
#[test]
fn every_goal_name_lookup_applies_the_call_edge() {
    let src = include_str!("goal.rs");
    let lines: Vec<&str> = src.lines().collect();
    let (mut edged, mut listed) = (0, 0);
    for (i, l) in lines.iter().enumerate() {
        if !l.contains("self.fns.get(") && !(l.trim() == ".fns" && lines[i + 1].contains(".get(")) {
            continue;
        }
        let window = lines[i..(i + 16).min(lines.len())].join("\n");
        if window.contains("self.seal_call(f)?") {
            edged += 1;
        } else if window.contains("self.fn_is_sealed(f)") || l.contains("cname.as_str()") {
            listed += 1;
        } else {
            panic!(
                "ATTACK: DRIFT: goal.rs:{} resolves a goal by name without the call edge: {}",
                i + 1,
                l.trim()
            );
        }
    }
    assert_eq!(
        (edged, listed),
        (6, 2),
        "DRIFT: goal.rs's goal-name lookups changed; classify the new one"
    );
}
