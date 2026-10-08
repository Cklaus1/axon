//! The runtime taint's tests (C9 round 11, PSV-1, amendment 102).
//!
//! Every attack here is run with the STATIC pin analysis switched OFF and only
//! the runtime taint rules ON (`Rules::TaintOnly`), so it reaches the taint rule
//! and is judged by it alone, and each is run with the rule OFF too
//! (`Rules::Neither`) to show it is a live attack: without the rule it completes.
//! The honest controls run with BOTH layers on (`Rules::Both`): the cost of the
//! rule to an honest suite is what that column measures.

use super::tests::{JUDGE8, LAUNDER8, SEALED_DIRS_TEST_LOCK};
use super::*;
use std::sync::atomic::Ordering::SeqCst;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Rules {
    /// Static pin analysis and runtime taint.
    Both,
    /// Runtime taint only: the attack reaches the taint rule.
    TaintOnly,
    /// Static pin analysis only: what the earlier rounds shipped.
    StaticOnly,
    /// Neither: the attack is live.
    Neither,
}

/// Run test `t` of `suite` with `cand` loaded from a sealed directory.
pub(super) fn run(suite: &str, cand: &str, rules: Rules) -> Result<TestEnd, String> {
    use crate::span::intern_source;
    let sdir = "/r11t-sealed";
    let s = crate::parse_source_in(suite, intern_source("/r11t-suite/h.ax", suite))
        .expect("suite parses");
    let c = crate::parse_source_in(cand, intern_source(&format!("{sdir}/f.ax"), cand))
        .expect("candidate parses");
    let prog = Program {
        items: s.items.into_iter().chain(c.items).collect(),
    };
    let _g = SEALED_DIRS_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    crate::resolver::set_sealed_module_dirs(&[std::path::PathBuf::from(sdir)]);
    let (static_on, taint_on) = match rules {
        Rules::Both => (true, true),
        Rules::TaintOnly => (false, true),
        Rules::StaticOnly => (true, false),
        Rules::Neither => (false, false),
    };
    DISPATCH_RULE_OFF.store(!static_on, SeqCst);
    TAINT_FORCE_ON.store(taint_on, SeqCst);
    let out = run_test_fn_outcome(&prog, "t");
    DISPATCH_RULE_OFF.store(false, SeqCst);
    TAINT_FORCE_ON.store(false, SeqCst);
    crate::resolver::set_sealed_module_dirs(&[]);
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Expect {
    /// The test runs to its end.
    Ok,
    /// The runtime taint rule refused the program.
    Refused,
    /// An assertion of the suite failed (the honest "wrong answer" verdict).
    Fails,
}

fn classify(out: &Result<TestEnd, String>) -> Expect {
    match out {
        Ok(TestEnd::Completed) => Expect::Ok,
        Ok(TestEnd::EndedEarly(_)) => Expect::Fails,
        Err(m)
            if m.contains("(runtime taint)")
                || m.contains("the candidate picked")
                || m.contains("nothing on the operator side determined") =>
        {
            Expect::Refused
        }
        Err(_) => Expect::Fails,
    }
}

/// One program: the operator's suite (a `pre` of items and a test body) and the
/// candidate, with the verdict the taint rule must give.
pub(super) struct Case {
    /// The static analysis over-refuses this honest program (stated cost, kept
    /// as defence in depth): `Rules::Both` is not asked to accept it.
    pub static_stricter: bool,
    pub name: &'static str,
    pub pre: String,
    pub body: String,
    pub cand: String,
    pub expect: Expect,
}

pub(super) fn case(name: &'static str, pre: &str, body: &str, cand: &str, expect: Expect) -> Case {
    Case {
        static_stricter: false,
        name,
        pre: pre.to_string(),
        body: body.to_string(),
        cand: cand.to_string(),
        expect,
    }
}

/// A case whose honest verdict the static layer is stricter about.
pub(super) fn stricter(mut c: Case) -> Case {
    c.static_stricter = true;
    c
}

pub(super) fn suite_of(c: &Case) -> String {
    format!("{}{}\n@[test]\nfn t() {{\n{}\n}}\n", JUDGE8, c.pre, c.body)
}

/// Run every case under `rules`. A case that should have been REFUSED and
/// completed is an attack that got through (`ATTACK: <case> completed`, every
/// such case is listed, so a mutation row can name its own case); any other
/// verdict that differs is a mismatch. Fails with all of them.
pub(super) fn check(cases: &[Case], rules: Rules) {
    let mut attacks: Vec<String> = Vec::new();
    let mut other: Vec<String> = Vec::new();
    for c in cases {
        if c.static_stricter && rules == Rules::Both {
            continue;
        }
        let out = run(&suite_of(c), &format!("{LAUNDER8}{}", c.cand), rules);
        let got = classify(&out);
        if got == c.expect {
            continue;
        }
        if c.expect == Expect::Refused && got == Expect::Ok {
            attacks.push(format!("ATTACK: <{}> completed under {:?}", c.name, rules));
        } else {
            other.push(format!(
                "{:?}: case `{}` should be {:?}, was {:?}: {out:?}",
                rules, c.name, c.expect, got
            ));
        }
    }
    assert!(
        attacks.is_empty() && other.is_empty(),
        "{}\n{}",
        attacks.join("\n"),
        other.join("\n")
    );
}

/// The attacks in `cases` (expect `Refused`) all complete when no rule is on.
pub(super) fn attacks_are_live(cases: &[Case]) {
    for c in cases.iter().filter(|c| c.expect == Expect::Refused) {
        let out = run(
            &suite_of(c),
            &format!("{LAUNDER8}{}", c.cand),
            Rules::Neither,
        );
        if c.name.starts_with("the wrong") {
            // The control that picks the answer-failing alternative: with the
            // rule off it fails keyed; with the rule on it is refused, because
            // the rule judges who chose, not whether the answer was right.
            assert_eq!(classify(&out), Expect::Fails, "{}: {out:?}", c.name);
            continue;
        }
        assert!(
            matches!(out, Ok(TestEnd::Completed)),
            "the attack `{}` is not live without the rule (a refusal elsewhere would hide a hole): {out:?}",
            c.name
        );
    }
}

const REF: &str = "fn reference(x: i64) -> i64 { x * 2 }\n";
const OPS: &str = "    let ops = [|x| 0, |x| x * 2]\n";
const HTAB: &str = "    let h = dict_new()\n    dict_set(h, \"double\", |x| 0)\n    dict_set(h, \"reference\", |x| x * 2)\n";
const IDX: &str = "fn idx() -> i64 { 1 }\n";
const ENT: &str = "fn entry() -> str { \"reference\" }\nfn double(x: i64) -> i64 { 0 }\n";
const SB: &str = "    let p = principal_root(\"r\", true, true, true, 100)\n    let sb = sandbox_create(p, \"IO\")\n";

fn closure_cases() -> Vec<Case> {
    use Expect::*;
    let wrong = |s: &str| s.replace("-> i64 { 1 }", "-> i64 { 0 }");
    let table = |body: &str| format!("{OPS}{body}");
    let dtab = |body: &str| format!("{HTAB}{body}");
    vec![
        // Honest: the operator names the closure, or branches on data to
        // run closures it listed.
        case("lit index", REF, &table("    let f = ops[1]\n    assert(f(21) == reference(21))"), IDX, Ok),
        case("lit key", REF, &dtab("    match dict_get(h, \"reference\") { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), ENT, Ok),
        case("statement branch", REF, &table("    if idx() == 1 { assert(ops[1](21) == reference(21)) } else { assert(false) }"), IDX, Ok),
        case("operator loop over a table", REF, &table("    for i in 0..2 { if i == 1 { assert(ops[i](21) == reference(21)) } }"), IDX, Ok),
        case("a tainted ARGUMENT, not a tainted callee", REF, &table("    assert(ops[1](idx() * 21) == reference(21))"), IDX, Ok),
        case("the candidate's own closure", REF, "    let g = mk()\n    assert(g(21) == reference(21))", "fn mk() -> fn(i64) -> i64 { |x| x * 2 }\n", Ok),
        case("a wrong answer still fails keyed", REF, &table("    assert(ops[0](21) == reference(21))"), IDX, Fails),
        // Attacks: sealed code picks the closure.
        case("index", REF, &table("    let f = ops[idx()]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("dict key", REF, &dtab("    match dict_get(h, entry()) { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), ENT, Refused),
        case("dict key via a local", REF, &dtab("    let k = entry()\n    match dict_get(h, k) { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), ENT, Refused),
        case("dict key built from it", REF, &dtab("    match dict_get(h, \"refer\" + str_slice(entry(), 5, 9)) { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), ENT, Refused),
        case("dict key interpolated", REF, &dtab("    match dict_get(h, \"{entry()}\") { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), ENT, Refused),
        case("index spelled as a comparison", REF, &table("    let f = ops[if idx() == 1 { 1 } else { 0 }]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("index assigned under a tainted branch", REF, &table("    let k = 0\n    if idx() == 1 { k = 1 }\n    let f = ops[k]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("closure assigned under a tainted branch", REF, &table("    let f = ops[0]\n    if idx() == 1 { f = ops[1] }\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("closure chosen by an if expression", REF, &table("    let f = if idx() == 1 { ops[1] } else { ops[0] }\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("closure chosen by a match", REF, &table("    let f = match idx() { 1 => ops[1]  _ => ops[0] }\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("index returned from a tainted branch", &format!("{REF}fn pick() -> i64 {{\n    if idx() == 1 {{ return 1 }}\n    0\n}}\n"), &table("    let f = ops[pick()]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("closure returned from a tainted branch", &format!("{REF}fn pickf() -> fn(i64) -> i64 {{\n    if idx() == 1 {{ return |x| x * 2 }}\n    |x| 0\n}}\n"), "    let f = pickf()\n    assert(f(21) == reference(21))", IDX, Refused),
        case("index through a helper's parameter", &format!("{REF}fn get(ops: [fn(i64) -> i64], i: i64) -> fn(i64) -> i64 {{ ops[i] }}\n"), &table("    let f = get(ops, idx())\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("dict_values then index", REF, &dtab("    let vs = dict_values(h)\n    let f = vs[idx()]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("closure handed to a builtin that calls it", REF, &table("    let r = arr_map([21], ops[idx()])\n    assert(r[0] == reference(21))"), IDX, Refused),
        case("assigned in a loop the candidate sized", REF, &table("    let f = ops[0]\n    for i in 0..idx() { f = ops[1] }\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("assigned in a while the candidate bounds", REF, &table("    let f = ops[0]\n    let n = 0\n    while n < idx() {\n        f = ops[1]\n        n = n + 1\n    }\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("held in a struct", &format!("{REF}type H = {{ f: fn(i64) -> i64 }}\n"), &table("    let s = H { f: ops[idx()] }\n    assert((s.f)(21) == reference(21))"), IDX, Refused),
        case("held in a tuple", REF, &table("    let t = (ops[idx()], 1)\n    let f = t.0\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("through a captured index", REF, &table("    let k = idx()\n    let g = || ops[k]\n    let f = g()\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("through a closure that calls the candidate", REF, &table("    let g = || ops[idx()]\n    let f = g()\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("index through an operator channel", REF, &table("    let c = chan<i64>()\n    c.send(idx())\n    let f = ops[c.recv()]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("index through an operator dict", REF, &table("    let d = dict_new()\n    dict_set(d, \"i\", idx())\n    let f = match dict_get(d, \"i\") { Some(i) => ops[i]  None => ops[0] }\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("key built with to_str", REF, &format!("{HTAB}    match dict_get(h, \"r\" + to_str(idx())) {{ Some(f) => assert(f(21) == reference(21))  None => assert(false) }}").replace("\"reference\"", "\"r1\""), IDX, Refused),
        case("index from sandbox_run", REF, &format!("{OPS}{SB}    let f = ops[sandbox_run(sb, \"idx1\", 0)]\n    assert(f(21) == reference(21))"), &format!("{IDX}fn idx1(x: i64) -> i64 {{ 1 }}\n"), Refused),
        case("index from the scheduler", REF, &format!("{OPS}    let id = scheduler_spawn(\"idx1\", 0)\n    let n = scheduler_run()\n    let f = ops[scheduler_result(id)]\n    assert(f(21) == reference(21))"), &format!("{IDX}fn idx1(x: i64) -> i64 {{ 1 }}\n"), Refused),
        case("index from a goal", REF, &format!("{OPS}    let s = goal_eval(\"idx\", 0)\n    let f = ops[f64_to_i64(s)]\n    assert(f(21) == reference(21))"), "@[adaptive]\nfn idx(x: i64) -> i64 { 1 }\n", Refused),
        // The wrong key is refused too: the rule is on WHO chose, not on the answer.
        case("the wrong key is refused as well", REF, &dtab("    match dict_get(h, entry()) { Some(f) => assert(f(21) == reference(21))  None => assert(false) }"), &wrong(ENT).replace("\"reference\"", "\"double\""), Refused),
    ]
}

#[test]
fn an_operator_closure_the_candidate_picked_is_never_called() {
    let cases = closure_cases();
    check(&cases, Rules::TaintOnly);
    check(&cases, Rules::Both);
    attacks_are_live(&cases);
    // The static layer alone (what rounds 6-10 shipped) lets the headline
    // attacks of the round-10 report through: the open finding of amendment 100.
    let head: Vec<&Case> = cases
        .iter()
        .filter(|c| matches!(c.name, "index" | "dict key"))
        .collect();
    assert_eq!(head.len(), 2);
    for c in head {
        let out = run(
            &suite_of(c),
            &format!("{LAUNDER8}{}", c.cand),
            Rules::StaticOnly,
        );
        assert!(
            matches!(out, Ok(TestEnd::Completed)),
            "the static analysis alone was expected to leave `{}` open (amendment 100): {out:?}",
            c.name
        );
    }
}

fn name_cases() -> Vec<Case> {
    use Expect::*;
    let run_named = |n: &str| {
        format!("{SB}    let got = sandbox_run(sb, {n}, 21)\n    assert(got == reference(21))")
    };
    let honest = ENT.replace("\"reference\"", "\"double\"");
    vec![
        case("literal", REF, &format!("{SB}    let got = sandbox_run(sb, \"double\", 21)\n    assert(got == 0)"), ENT, Ok),
        case("operator-built name", REF, &format!("{SB}    let got = sandbox_run(sb, \"dou\" + \"ble\", 21)\n    assert(got == 0)"), ENT, Ok),
        case("statement branch between two literals", REF, &format!("{SB}    if idx() == 1 {{\n        let got = sandbox_run(sb, \"reference\", 21)\n        assert(got == 42)\n    }} else {{\n        assert(false)\n    }}"), &format!("{IDX}fn double(x: i64) -> i64 {{ 0 }}\n"), Ok),
        case("scheduler literal", REF, "    let id = scheduler_spawn(\"double\", 21)\n    let n = scheduler_run()\n    assert(scheduler_result(id) == 0)", ENT, Ok),
        case("the candidate's data as the ARGUMENT", REF, &format!("{SB}    let got = sandbox_run(sb, \"double\", entry_arg())\n    assert(got == 0)"), &format!("fn entry_arg() -> i64 {{ 21 }}\n{}", honest), Ok),
        case("a wrong answer still fails keyed", REF, &format!("{SB}    let got = sandbox_run(sb, \"double\", 21)\n    assert(got == reference(21))"), ENT, Fails),
        case("a candidate's result", REF, &run_named("entry()"), ENT, Refused),
        case("via a local", REF, &format!("{SB}    let nm = entry()\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("annotated str", REF, &format!("{SB}    let nm: str = entry()\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("an if expression on its bit", REF, &run_named("if idx() == 1 { \"reference\" } else { \"double\" }"), &format!("{IDX}fn double(x: i64) -> i64 {{ 0 }}\n"), Refused),
        case("assigned under its bit", REF, &format!("{SB}    let nm = \"double\"\n    if idx() == 1 {{ nm = \"reference\" }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), &format!("{IDX}fn double(x: i64) -> i64 {{ 0 }}\n"), Refused),
        case("assigned in both arms", REF, &format!("{SB}    let nm = \"x\"\n    if idx() == 1 {{ nm = \"reference\" }} else {{ nm = \"double\" }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), &format!("{IDX}fn double(x: i64) -> i64 {{ 0 }}\n"), Refused),
        case("returned from a tainted branch", &format!("{REF}fn pick() -> str {{\n    if idx() == 1 {{ return \"reference\" }}\n    \"double\"\n}}\n"), &run_named("pick()"), &format!("{IDX}fn double(x: i64) -> i64 {{ 0 }}\n"), Refused),
        case("a match on its value", REF, &run_named("match idx() { 1 => \"reference\"  _ => \"double\" }"), &format!("{IDX}fn double(x: i64) -> i64 {{ 0 }}\n"), Refused),
        case("an array indexed by it", REF, &run_named("[\"double\", \"reference\"][idx()]"), &format!("{IDX}fn double(x: i64) -> i64 {{ 0 }}\n"), Refused),
        case("an array indexed by it, modulo", REF, &run_named("[\"double\", \"reference\"][idx() % 2]"), &format!("{IDX}fn double(x: i64) -> i64 {{ 0 }}\n"), Refused),
        case("a struct field", &format!("{REF}type C = {{ n: str }}\n"), &format!("{SB}    let c = C {{ n: entry() }}\n    let got = sandbox_run(sb, c.n, 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("a tuple element", REF, &format!("{SB}    let t = (entry(), 1)\n    let got = sandbox_run(sb, t.0, 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("string operations", REF, &run_named("str_to_lower(str_trim(entry()))"), ENT, Refused),
        case("str_replace", REF, &run_named("str_replace(\"xx\", \"xx\", entry())"), ENT, Refused),
        case("interpolation", REF, &run_named("\"{entry()}\""), ENT, Refused),
        case("a json round trip", REF, &format!("{SB}    let j = json_from_pairs([(\"n\", json_stringify(entry()))])\n    let nm = match json_get_str(j, \"n\") {{ Ok(s) => s  Err(e) => \"\" }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("a captured binding", REF, &format!("{SB}    let nm = entry()\n    let g = || nm\n    let got = sandbox_run(sb, g(), 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("a closure that calls the candidate", REF, &format!("{SB}    let g = || entry()\n    let got = sandbox_run(sb, g(), 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("an operator dict", REF, &format!("{SB}    let d = dict_new()\n    dict_set(d, \"k\", entry())\n    let nm = match dict_get(d, \"k\") {{ Some(s) => s  None => \"\" }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("an operator channel", REF, &format!("{SB}    let c = chan<str>()\n    c.send(entry())\n    let got = sandbox_run(sb, c.recv(), 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("a loop the candidate sized", REF, &format!("{SB}    let names = [\"double\", \"reference\"]\n    let nm = \"double\"\n    for i in 0..idx() {{ nm = names[1] }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), &format!("{IDX}fn double(x: i64) -> i64 {{ 0 }}\n"), Refused),
        case("an operator fn's parameter", &format!("{REF}fn go(sb: i64, n: str) -> i64 {{ sandbox_run(sb, n, 21) }}\n"), &format!("{SB}    let got = go(sb, entry())\n    assert(got == reference(21))"), ENT, Refused),
        case("a file round trip", REF, &format!("{SB}    let f = \"{}/r11t-name.txt\"\n    let w = write_file(f, entry())\n    let nm = match read_file(f) {{ Ok(s) => s  Err(e) => \"\" }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))", std::env::temp_dir().display()), ENT, Refused),
        case("scheduler_spawn", REF, "    let id = scheduler_spawn(entry(), 21)\n    let n = scheduler_run()\n    assert(scheduler_result(id) == reference(21))", ENT, Refused),
        case("goal_eval", "fn easy(x: i64) -> i64 { 100 }\nfn hard(x: i64) -> i64 { 0 }\n", "    let s = goal_eval(entry(), 5)\n    assert(s > 50.0)", "fn entry() -> str { \"easy\" }\n", Refused),
        case("through the kernel", REF, &format!("{SB}    let r = sandbox_run(sb, \"double\", 21)\n    let nm = if r == 0 {{ \"double\" }} else {{ \"reference\" }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == 0)"), ENT, Refused),
        case("an operator global the candidate set", &format!("{REF}let NAME = entry()\n"), &run_named("NAME"), ENT, Refused),
        case("a handler arm's answer", REF, &format!("{SB}    let nm = with handler {{ on IO(p) => resume(Ok(entry())) }} {{\n        match read_file(\"/nonexistent/r11t\") {{ Ok(s) => s  Err(e) => \"\" }}\n    }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), ENT, Refused),
    ]
}

#[test]
fn a_name_sealed_code_had_a_hand_in_never_selects_an_operator_fn() {
    let cases = name_cases();
    check(&cases, Rules::TaintOnly);
    check(&cases, Rules::Both);
    attacks_are_live(&cases);
}

fn dispatch_cases() -> Vec<Case> {
    use Expect::*;
    let body = |s: &str| format!("    {s}");
    vec![
        // Honest: the operator chose the type.
        case("pinned i64, the right answer", "", &body("let y: i64 = lucky()\n    assert(y.ok())"), "fn lucky() -> i64 { 9 }\n", Ok),
        case("pinned i64, the wrong answer fails keyed", "", &body("let y: i64 = lucky()\n    assert(y.ok())"), "fn lucky() -> i64 { 4 }\n", Fails),
        case("a declared u8 return the operator wrote", "fn f() -> u8 { narrow(4) }\n", &body("assert(f().ok())"), "", Ok),
        case("the operator's own cast", "", &body("let w = 4 as u8\n    assert(w.ok())"), "", Ok),
        case("a branch between the operator's own values", "", &body("let x = if narrow(4) == 4 { 9 } else { 4 }\n    assert(x.ok())"), "", Ok),
        case("an operator-chosen width", "", &body("let w = 255 as u8\n    assert((w >> 1) == 127)"), "", Ok),
        case("a comparison needs no width", "", &body("assert(narrow(255) == 255)"), "", Ok),
        case("cast by the operator after the candidate", "", &body("let x = narrow(9) as i64\n    assert(x.ok())"), "", Ok),
        // Attacks.
        case("direct", "", &body("assert(narrow(4).ok())"), "", Refused),
        case("via a local", "", &body("let x = narrow(4)\n    assert(x.ok())"), "", Refused),
        case("via a tuple", "", &body("let t = (narrow(4), 1)\n    assert(t.0.ok())"), "", Refused),
        case("via Some", "", &body("match Some(narrow(4)) { Some(v) => assert(v.ok())  None => assert(false) }"), "", Refused),
        case("via an array element", "", &body("let a = [narrow(4)]\n    assert(a[0].ok())"), "", Refused),
        case("via a dict the candidate filled", "", &body("match dict_get(stash(narrow(4)), \"k\") { Some(v) => assert(v.ok())  None => assert(false) }"), "", Refused),
        case("via a generic helper", "fn judge<T: Judge>(v: T) -> bool { v.ok() }\n", &body("assert(judge(narrow(4)))"), "", Refused),
        case("via an unannotated operator fn", "fn id<T>(v: T) -> T { v }\n", &body("assert(id(narrow(4)).ok())"), "", Refused),
        case("via a branch value", "", &body("let x = if narrow(4) == 4 { narrow(4) } else { narrow(5) }\n    assert(x.ok())"), "", Refused),
        case("via sandbox_run", "", &format!("{SB}    let r = sandbox_run(sb, \"work\", 0)\n    assert(r.ok())"), "fn work(x: i64) -> i64 { 4 }\n", Fails),
        case("width: shift", "", &body("let v = narrow(255)\n    assert((v << 1) == 254)"), "", Refused),
        case("width: unary", "", &body("assert((-narrow(4)) == 252)"), "", Refused),
        case("width: bit-not", "", &body("assert((~narrow(4)) == 251)"), "", Refused),
        case("width: through a tuple", "", &body("let t = (narrow(255), 1)\n    assert((t.0 << 1) == 254)"), "", Refused),
    ]
}

#[test]
fn a_type_or_width_sealed_code_chose_is_never_dispatched_on() {
    let cases = dispatch_cases();
    check(&cases, Rules::TaintOnly);
    check(&cases, Rules::Both);
    attacks_are_live(&cases);
}

fn carrier_cases() -> Vec<Case> {
    use Expect::*;
    let named = |n: &str| {
        format!("{SB}    let got = sandbox_run(sb, {n}, 21)\n    assert(got == reference(21))")
    };
    let dbl = "fn double(x: i64) -> i64 { 0 }\n";
    vec![
        case("a &mut write-through", REF, &format!("{SB}    let xs = [\"double\"]\n    fill(&mut xs)\n    let got = sandbox_run(sb, xs[0], 21)\n    assert(got == reference(21))"), &format!("fn fill(xs: &mut [str]) {{ xs[0] = \"reference\" }}\n{dbl}"), Refused),
        case("a sealed send into an operator channel", REF, &format!("{SB}    let c = chan<str>()\n    push(c)\n    let got = sandbox_run(sb, c.recv(), 21)\n    assert(got == reference(21))"), &format!("fn push(c: Chan<str>) {{ c.send(\"reference\") }}\n{dbl}"), Refused),
        stricter(case("an operator send is the operator's", REF, &format!("{SB}    let c = chan<str>()\n    c.send(\"double\")\n    peek_c(c)\n    let got = sandbox_run(sb, c.recv(), 21)\n    assert(got == 0)"), &format!("fn peek_c(c: Chan<str>) {{ let n = c.len() }}\n{dbl}"), Ok)),
        case("a sealed write into an operator dict", REF, &format!("{SB}    let d = dict_new()\n    put(d)\n    let nm = match dict_get(d, \"n\") {{ Some(s) => s  None => \"\" }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), &format!("fn put(d: Dict) {{ dict_set(d, \"n\", \"reference\") }}\n{dbl}"), Refused),
        stricter(case("a sealed READ of an operator dict taints nothing", REF, &format!("{SB}    let d = dict_new()\n    dict_set(d, \"n\", \"double\")\n    peek(d)\n    let nm = match dict_get(d, \"n\") {{ Some(s) => s  None => \"\" }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == 0)"), &format!("fn peek(d: Dict) -> i64 {{ dict_len(d) }}\n{dbl}"), Ok)),
        case("closure state the candidate set", REF, &format!("    let n = 0\n    let cb = |v| {{ if v > 0 {{ n = v }}\n        n }}\n    let r = run_cb(cb)\n    let names = [\"double\", \"reference\"]\n{SB}    let got = sandbox_run(sb, names[cb(0)], 21)\n    assert(got == reference(21))"), &format!("fn run_cb(f: fn(i64) -> i64) -> i64 {{ f(1) }}\n{dbl}"), Refused),
        stricter(case("closure state the operator set", REF, &format!("    let n = 0\n    let cb = |v| {{ if v > 0 {{ n = v }}\n        n }}\n    let r = cb(1)\n    let names = [\"double\", \"reference\"]\n{SB}    let got = sandbox_run(sb, names[cb(0)], 21)\n    assert(got == reference(21))"), dbl, Ok)),
        case("a closure the candidate returned", REF, &named("mk()()"), &format!("fn mk() -> fn() -> str {{ || \"reference\" }}\n{dbl}"), Refused),
        case("a match on the candidate's result", REF, &format!("{SB}    let names = [\"double\", \"reference\"]\n    let nm = match pick() {{ Ok(n) => names[n]  Err(e) => \"double\" }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), &format!("fn pick() -> Result<i64, str> {{ Ok(1) }}\n{dbl}"), Refused),
        case("a ? on the candidate's result", &format!("{REF}fn nm() -> Result<str, str> {{\n    let s = word()?\n    Ok(s)\n}}\n"), &format!("{SB}    let nm = match nm() {{ Ok(s) => s  Err(e) => \"\" }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), &format!("fn word() -> Result<str, str> {{ Ok(\"reference\") }}\n{dbl}"), Refused),
        case("a literal after an early exit on its bit", &format!("{REF}fn nm() -> str {{\n    if idx() == 1 {{ return \"x\" }}\n    let n = \"reference\"\n    n\n}}\n"), &named("nm()"), &format!("fn idx() -> i64 {{ 0 }}\n{dbl}"), Refused),
        case("a store after a break on its bit", REF, &format!("{SB}    let nm = \"double\"\n    for i in 0..3 {{\n        if idx() == 1 {{ break }}\n        nm = \"reference\"\n    }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), &format!("fn idx() -> i64 {{ 0 }}\n{dbl}"), Refused),
        stricter(case("a literal after a break is the operator's own statement", REF, &format!("{SB}    for i in 0..3 {{ if idx() == 1 {{ break }} }}\n    let nm = \"double\"\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == 0)"), &format!("{IDX}{dbl}"), Ok)),
        case("statement-level branches stay free", REF, &format!("{SB}    let r = idx()\n    if r == 1 {{ let g = sandbox_run(sb, \"double\", 21)\n        assert(g == 0) }}\n    let got = sandbox_run(sb, \"double\", 21)\n    assert(got == 0)"), &format!("{IDX}{dbl}"), Ok),
    ]
}

#[test]
fn taint_survives_every_carrier_a_value_can_travel_by() {
    let cases = carrier_cases();
    check(&cases, Rules::TaintOnly);
    check(&cases, Rules::Both);
    attacks_are_live(&cases);
}

/// Outside a sealed run nothing changes: the same programs run as ever.
#[test]
fn an_ordinary_run_takes_no_taint() {
    for c in closure_cases().iter().chain(name_cases().iter()) {
        let src = format!(
            "{JUDGE8}{LAUNDER8}{}{}\n{}\n@[test]\nfn t() {{\n{}\n}}\n",
            c.pre, c.cand, "", c.body
        );
        let prog = crate::parse_source(&src).expect("parses");
        let out = run_test_fn_outcome(&prog, "t");
        // Unsealed, the candidate's functions are the operator's own: an attack
        // that works only because of what it names completes or fails on its
        // own assertion, never on a taint refusal.
        assert!(
            !matches!(&out, Err(m) if m.contains("(runtime taint)") || m.contains("the candidate picked")),
            "case `{}`: a taint refusal in an ordinary run: {out:?}",
            c.name
        );
    }
}

fn more_cases() -> Vec<Case> {
    use Expect::*;
    let dbl = "fn double(x: i64) -> i64 { 0 }\n";
    let named = |n: &str| {
        format!("{SB}    let got = sandbox_run(sb, {n}, 21)\n    assert(got == reference(21))")
    };
    vec![
        // The block's value is its tail: what an earlier statement touched is not it.
        case("a helper that called the candidate first", &format!("{REF}fn nm() -> str {{\n    let r = idx()\n    assert(r == 1)\n    \"double\"\n}}\n"), &format!("{SB}    let got = sandbox_run(sb, nm(), 21)\n    assert(got == 0)"), &format!("{IDX}{dbl}"), Ok),
        // The callee expression is a table read.
        case("a computed callee", REF, &format!("{OPS}    assert((ops[idx()])(21) == reference(21))"), IDX, Refused),
        // A literal after an early exit on its bit, directly the tail or the argument.
        case("a tail literal after an early exit", &format!("{REF}fn nm() -> str {{\n    if idx() == 1 {{ return \"x\" }}\n    \"reference\"\n}}\n"), &named("nm()"), &format!("fn idx() -> i64 {{ 0 }}\n{dbl}"), Refused),
        // A closure literal chosen by a branch.
        case("lambda literals chosen by a branch", REF, "    let f = |x| 0\n    if idx() == 1 { f = |x| x * 2 }\n    assert(f(21) == reference(21))", IDX, Refused),
        // A container pin: the cast refuses a [u8] where [i64] is declared.
        case("a pinned array of the wrong width", "", "    let a: [i64] = mk()\n    assert(a[0].ok())", "fn mk() -> [u8] { [narrow(4)] }\n", Fails),
        case("a pinned array, right", "", "    let a: [i64] = mk()\n    assert(a[0].ok())", "fn mk() -> [i64] { [9] }\n", Ok),
        // A module-level closure the candidate had a hand in.
        case("a module-level closure the candidate picked", &format!("{REF}let G = idx()\nlet T = [|x| 0, |x| x * 2]\nlet F = T[G]\n"), "    assert(F(21) == reference(21))", IDX, Refused),
        // A general (replay) handler arm.
        case("a replay arm's answer", REF, &format!("{SB}    let nm = with handler {{ on IO(p) => {{\n        let a = resume(Ok(entry()))\n        a\n    }} }} {{\n        match read_file(\"/nonexistent/r11t\") {{ Ok(s) => s  Err(e) => \"\" }}\n    }}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))"), ENT, Refused),
        // A fn value picked by name from an operator table.
        case("a table of operator fns", &format!("{REF}fn zero(x: i64) -> i64 {{ 0 }}\n"), "    let h = dict_new()\n    dict_set(h, \"zero\", zero)\n    dict_set(h, \"reference\", reference)\n    match dict_get(h, entry()) { Some(f) => assert(f(21) == reference(21))  None => assert(false) }", ENT, Refused),
        // The operator's own data into a candidate, back out as data.
        case("a candidate's number compared", REF, "    assert(idx() * 42 == reference(21))", IDX, Ok),
        case("a candidate's number as a loop bound", REF, "    let n = 0\n    for i in 0..idx() { n = n + 1 }\n    assert(n == 1)", IDX, Ok),
    ]
}

fn hunted_cases() -> Vec<Case> {
    use Expect::*;
    let dbl = "fn double(x: i64) -> i64 { 0 }\n";
    let named = |n: &str| {
        format!("{SB}    let got = sandbox_run(sb, {n}, 21)\n    assert(got == reference(21))")
    };
    vec![
        case("a candidate name as a dict KEY", REF, &format!("{SB}    let d = dict_new()\n    dict_set(d, entry(), 1)\n    let ks = dict_keys(d)\n    let got = sandbox_run(sb, ks[0], 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("a closure the candidate was handed and chose", REF, &format!("{OPS}    let g = choose(ops[0], ops[1])\n    assert(g(21) == reference(21))"), "fn choose(a: fn(i64) -> i64, b: fn(i64) -> i64) -> fn(i64) -> i64 { b }\n", Refused),
        case("a closure the candidate stored in the operator's dict", REF, &format!("{OPS}    let d = dict_new()\n    reg(d, ops[1])\n    match dict_get(d, \"f\") {{ Some(f) => assert(f(21) == reference(21))  None => assert(false) }}"), "fn reg(d: Dict, f: fn(i64) -> i64) { dict_set(d, \"f\", f) }\n", Refused),
        case("a name picked by a bool parameter", &format!("{REF}fn pick(f: bool) -> str {{ if f {{ \"reference\" }} else {{ \"double\" }} }}\n"), &named("pick(idx() == 1)"), &format!("{IDX}{dbl}"), Refused),
        case("a name picked by a length", REF, &named("[\"double\", \"reference\"][len(entry()) % 2]"), ENT, Refused),
        case("a name picked by string equality", REF, &named("if entry() == \"reference\" { \"reference\" } else { \"double\" }"), ENT, Refused),
        case("a name built from char codes", REF, &named("chr(114) + str_slice(entry(), 1, 9)"), ENT, Refused),
        case("an Option's payload", REF, &format!("{SB}    let o = Some(entry())\n    let got = match o {{ Some(n) => sandbox_run(sb, n, 21)  None => 0 }}\n    assert(got == reference(21))"), ENT, Refused),
        case("a Result's payload", REF, &format!("{SB}    let o = Ok(entry())\n    let got = match o {{ Ok(n) => sandbox_run(sb, n, 21)  Err(e) => 0 }}\n    assert(got == reference(21))"), ENT, Refused),
        case("the candidate's data as an operator-mapped name", REF, &format!("{SB}    let ns = arr_map([1, 2], |i| entry())\n    let got = sandbox_run(sb, ns[0], 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("a name through arr_fold", REF, &format!("{SB}    let n = arr_fold([1], \"double\", |acc, i| entry())\n    let got = sandbox_run(sb, n, 21)\n    assert(got == reference(21))"), ENT, Refused),
        case("a lookup that returns from a tainted branch", &format!("{REF}fn find(names: [str]) -> str {{\n    for i in 0..2 {{\n        if idx() == i {{ return names[i] }}\n    }}\n    \"double\"\n}}\n"), &named("find([\"double\", \"reference\"])"), &format!("{IDX}{dbl}"), Refused),
        case("a counter the candidate's loop condition drove", REF, &format!("{OPS}    let n = 0\n    while n < idx() {{ n = n + 1 }}\n    let f = ops[n]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("a counter after a break on its bit", REF, &format!("{OPS}    let k = 0\n    while true {{\n        if k == idx() {{ break }}\n        k = k + 1\n    }}\n    let f = ops[k]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("an index from a sorted candidate array", REF, &format!("{OPS}    let xs = arr_sort_by([idx(), 0], |a, b| a - b)\n    let f = ops[xs[1]]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("a field of an Uncertain the candidate built", REF, &format!("{OPS}    let u = uncertain_new(idx(), 0.9)\n    let f = ops[u.value]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("a name the fold kept from the operator", REF, &format!("{SB}    let n = arr_fold([1], \"double\", |acc, i| acc)\n    let got = sandbox_run(sb, n, 21)\n    assert(got == 0)"), ENT, Ok),
    ]
}

#[test]
fn bypass_shapes_hunted_after_the_first_cut() {
    let cases = hunted_cases();
    check(&cases, Rules::TaintOnly);
    attacks_are_live(&cases);
}

/// One attack per hook of the taint, so a mutation row can remove a hook and
/// name the case that must then get through (amendment 102).
fn hook_cases() -> Vec<Case> {
    use Expect::*;
    let dbl = "fn double(x: i64) -> i64 { 0 }\n";
    let named = |n: &str| {
        format!("{SB}    let got = sandbox_run(sb, {n}, 21)\n    assert(got == reference(21))")
    };
    let with = |pre: &str, body: &str| {
        format!(
            "{SB}{pre}    let got = sandbox_run(sb, {body}, 21)\n    assert(got == reference(21))"
        )
    };
    vec![
        case("hook: assignment", REF, &with("    let nm = \"double\"\n    nm = entry()\n", "nm"), ENT, Refused),
        case("hook: slot write", REF, &with("    let names = [\"double\"]\n    names[0] = entry()\n", "names[0]"), ENT, Refused),
        case("hook: append", REF, &with("    let nm = \"refer\"\n    nm = nm + entry()\n", "nm"), "fn entry() -> str { \"ence\" }\nfn double(x: i64) -> i64 { 0 }\n", Refused),
        case("hook: element of a binding", REF, &with("    let xs = [entry()]\n", "xs[0]"), ENT, Refused),
        case("hook: field of a binding", &format!("{REF}type Cfg = {{ n: str }}\n"), &with("    let c = Cfg { n: entry() }\n", "c.n"), ENT, Refused),
        case("hook: field of a module-level let", &format!("{REF}type Cfg = {{ n: str }}\nlet CFG = Cfg {{ n: entry() }}\n"), &named("CFG.n"), ENT, Refused),
        case("hook: element of a module-level let", &format!("{REF}let TABLE = [entry()]\n"), &named("TABLE[0]"), ENT, Refused),
        case("hook: module-level let", &format!("{REF}let NAME = entry()\n"), &named("NAME"), ENT, Refused),
        case("hook: a sealed module-level let", REF, &named("WORD"), "let WORD = \"reference\"\nfn double(x: i64) -> i64 { 0 }\n", Refused),
        case("hook: tail value", &format!("{REF}fn pick() -> str {{ entry() }}\n"), &named("pick()"), ENT, Refused),
        case("hook: return value", &format!("{REF}fn pick() -> str {{ return entry() }}\n"), &named("pick()"), ENT, Refused),
        case("hook: question value", &format!("{REF}fn pick() -> Result<str, str> {{\n    let s = word()?\n    Ok(\"double\")\n}}\n"), &with("    let r = pick()\n    let nm = match r { Ok(s) => s  Err(e) => e }\n", "nm"), "fn word() -> Result<str, str> { Err(\"reference\") }\nfn double(x: i64) -> i64 { 0 }\n", Refused),
        case("hook: parameter", &format!("{REF}fn go(sb: i64, n: str) -> i64 {{ sandbox_run(sb, n, 21) }}\n"), &format!("{SB}    let got = go(sb, entry())\n    assert(got == reference(21))"), ENT, Refused),
        case("hook: closure parameter", REF, &with("    let f = |s| s\n", "f(entry())"), ENT, Refused),
        case("hook: typed closure parameter keeps the value", REF, &with("    let f = |s: str| s\n", "f(entry())"), ENT, Refused),
        case("hook: closure result", REF, &with("    let f = || entry()\n", "f()"), ENT, Refused),
        case("hook: closure return", REF, &with("    let f = || { return entry() }\n", "f()"), ENT, Refused),
        case("hook: captured binding", REF, &with("    let nm = entry()\n    let g = || nm\n", "g()"), ENT, Refused),
        case("hook: captured binding in a lent cell", REF, &with("    let nm = entry()\n    let g = || nm\n    let h = g\n", "h()"), ENT, Refused),
        case("hook: closure write-back", REF, &with("    let nm = \"double\"\n    let set = |v| { if v != \"\" { nm = v }\n        nm }\n    let r = set(entry())\n", "set(\"\")"), ENT, Refused),
        case("hook: closure called by sealed code", REF, &format!("{SB}    let n = 0\n    let cb = |v| {{ if v > 0 {{ n = v }}\n        n }}\n    let r = run_cb(cb)\n    let names = [\"double\", \"reference\"]\n    let got = sandbox_run(sb, names[cb(0)], 21)\n    assert(got == reference(21))"), &format!("fn run_cb(f: fn(i64) -> i64) -> i64 {{ f(1) }}\n{dbl}"), Refused),
        case("hook: send", REF, &with("    let c = chan<str>()\n    c.send(entry())\n", "c.recv()"), ENT, Refused),
        case("hook: try_recv", REF, &with("    let c = chan<str>()\n    c.send(entry())\n    let nm = match c.try_recv() { Some(s) => s  None => \"\" }\n", "nm"), ENT, Refused),
        case("hook: a dict the operator wrote", REF, &with("    let d = dict_new()\n    dict_set(d, \"k\", entry())\n", "match dict_get(d, \"k\") { Some(s) => s  None => \"\" }"), ENT, Refused),
        case("hook: an alias of a dict the candidate wrote", REF, &with("    let d = dict_new()\n    let e = d\n    put(d)\n", "match dict_get(e, \"n\") { Some(s) => s  None => \"\" }"), &format!("fn put(d: Dict) {{ dict_set(d, \"n\", \"reference\") }}\n{dbl}"), Refused),
        case("hook: kernel", REF, &format!("{OPS}    let id = scheduler_spawn(\"idx1\", 0)\n    let n = scheduler_run()\n    let f = ops[scheduler_result(id)]\n    assert(f(21) == reference(21))"), &format!("{IDX}fn idx1(x: i64) -> i64 {{ 1 }}\n"), Refused),
        case("hook: kernel written by the operator", REF, &format!("{SB}    let names = [\"double\", \"reference\"]\n    let spent = principal_spend(p, idx())\n    let got = sandbox_run(sb, names[principal_budget_remaining(p) - 98], 21)\n    assert(got == reference(21))"), &format!("{IDX}{dbl}"), Refused),
        case("hook: world", REF, &with(&format!("    let f = \"{}/r11t-hook.txt\"\n    let w = write_file(f, entry())\n    let nm = match read_file(f) {{ Ok(s) => s  Err(e) => \"\" }}\n", std::env::temp_dir().display()), "nm"), ENT, Refused),
        case("hook: world written by sealed code", REF, &with(&format!("    let f = \"{}/r11t-hook2.txt\"\n    let w = write_file(f, \"double\")\n    stomp(f)\n    let nm = match read_file(f) {{ Ok(s) => s  Err(e) => \"\" }}\n", std::env::temp_dir().display()), "nm"), &format!("fn stomp(f: str) {{ let w = write_file(f, \"reference\") }}\n{dbl}"), Refused),
        case("hook: handler payload", REF, &format!("{SB}    let nm = with handler {{ on IO(p) => {{\n        let r = sandbox_run(sb, p, 21)\n        resume(Ok(to_str(r)))\n    }} }} {{\n        match read_file(entry()) {{ Ok(s) => s  Err(e) => \"\" }}\n    }}\n    assert(nm == \"42\")"), ENT, Refused),
        case("hook: handler return arm", REF, &format!("{SB}    let nm = with handler {{ on IO(p) => resume(Ok(\"x\")) return(v) => to_str(sandbox_run(sb, v, 21)) }} {{\n        entry()\n    }}\n    assert(nm == \"42\")"), ENT, Refused),
        case("hook: a completed arm's value reaches the return arm", REF, &format!("{SB}    let nm = with handler {{ on IO(p) => {{\n        let a = resume(Ok(entry()))\n        a\n    }} return(v) => to_str(sandbox_run(sb, match v {{ Ok(s) => s  Err(e) => \"\" }}, 21)) }} {{\n        read_file(\"/nonexistent/r11t3\")\n    }}\n    assert(nm == \"42\")"), ENT, Refused),
        case("hook: replay feed", REF, &with("    let nm = with handler { on IO(p) => {\n        let a = resume(Ok(entry()))\n        a\n    } } {\n        match read_file(\"/nonexistent/r11t2\") { Ok(s) => s  Err(e) => \"\" }\n    }\n", "nm"), ENT, Refused),
        case("hook: pattern binding", REF, &with("    let nm = match Some(entry()) { Some(s) => s  None => \"\" }\n", "nm"), ENT, Refused),
        case("hook: loop variable", REF, &with("    let names = [\"double\", \"reference\"]\n    let nm = \"double\"\n    for k in 0..idx() { nm = names[k + 1 - 1 + 1 - 1 + 1] }\n", "nm"), &format!("{IDX}{dbl}"), Refused),
        case("hook: a for bound", REF, &format!("{SB}    let names = [\"double\", \"reference\"]\n    for k in idx()..2 {{\n        let got = sandbox_run(sb, names[k], 21)\n        assert(got == reference(21))\n    }}"), &format!("{IDX}{dbl}"), Refused),
        case("hook: a trait-typed closure parameter", "", "    let f = |x: dyn Judge| x.ok()\n    assert(apply(f))", "fn apply(f: fn(dyn Judge) -> bool) -> bool { f(narrow(4)) }\n", Refused),
        case("hook: closure write-back, shared cell", REF, &with("    let nm = \"double\"\n    let set = |v| { if v != \"\" { nm = v }\n        nm }\n    let other = set\n    let r = other(entry())\n", "set(\"\")"), ENT, Refused),
        case("hook: &mut argument into an operator fn", &format!("{REF}fn first(xs: &mut [str]) -> str {{ xs[0] }}\n"), &with("    let xs = [entry()]\n", "first(&mut xs)"), ENT, Refused),
        case("hook: untyped closure parameter", "", "    let f = |x| x.ok()\n    assert(f(narrow(4)))", "", Refused),
        case("hook: a store after a continue on its bit", REF, &with("    let nm = \"double\"\n    for i in 0..3 {\n        if idx() == 1 { continue }\n        nm = \"reference\"\n    }\n", "nm"), &format!("fn idx() -> i64 {{ 0 }}\n{dbl}"), Refused),
        case("hook: a closure's tail after an early exit", REF, &with("    let f = || {\n        if idx() == 0 { return \"x\" }\n        \"reference\"\n    }\n", "f()"), &format!("fn idx() -> i64 {{ 1 }}\n{dbl}"), Refused),
        case("hook: width, right operand", "", "    assert((256 * narrow(1)) == 0)", "", Refused),
        case("hook: width, shift", "", "    let v = narrow(255)\n    assert((v << 1) == 254)", "", Refused),
        case("hook: trait annotation", "", "    let y: Judge = narrow(4)\n    assert(y.ok())", "", Refused),
        case("hook: an open builtin result", "", "    match arr_find([narrow(4)], |x| true) { Some(v) => assert(v.ok())  None => assert(false) }", "", Refused),
        case("hook: a clone of a picked closure", REF, &format!("{OPS}    let f = ops[idx()]\n    let g = f\n    assert(g(21) == reference(21))"), IDX, Refused),
        case("hook: a global closure", &format!("{REF}let T = [|x| 0, |x| x * 2]\nlet F = T[idx()]\n"), "    assert(F(21) == reference(21))", IDX, Refused),
        case("hook: a local closure called by name", REF, &format!("{OPS}    let k = idx()\n    let f = ops[k]\n    assert(f(21) == reference(21))"), IDX, Refused),
    ]
}

#[test]
fn every_hook_of_the_taint_has_an_attack_of_its_own() {
    let cases = hook_cases();
    check(&cases, Rules::TaintOnly);
    attacks_are_live(&cases);
}

#[test]
fn the_remaining_routes_a_selection_can_take() {
    let cases = more_cases();
    check(&cases, Rules::TaintOnly);
    check(&cases, Rules::Both);
    attacks_are_live(&cases);
}

// ── Drift tests: where a Value can come from without passing the evaluator ─────

/// The interpreter's source files (the sweep's own list), with each file's test
/// module cut off.
fn sources() -> Vec<(&'static str, String)> {
    super::tests::interp_sources()
}

/// Every builtin is in exactly one taint class, and every name a class table
/// lists is a builtin. A builtin added to `BUILTINS` is in no class until
/// someone decides which: it reads as `Unclassified` (treated as reading and
/// writing both kinds of state, so it can only over-taint) and this fails.
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
    // No two entries of PURE_BUILTINS agree: a duplicate is a stale edit.
    let mut seen = std::collections::HashSet::new();
    for n in PURE_BUILTINS.iter().chain(WORLD_BUILTINS) {
        assert!(seen.insert(*n), "`{n}` is listed twice");
    }
}

/// A builtin with a `Dict` parameter either WRITES it (listed in
/// `DICT_WRITERS`, which marks the dict) or only reads it: and a reader does
/// not change the dict it is handed. The behavioural half is run here.
#[test]
fn every_dict_builtin_is_a_listed_writer_or_a_reader_that_does_not_write() {
    use super::taint::DICT_WRITERS;
    const READERS: &[&str] = &[
        "dict_to_json",
        "dict_get",
        "dict_has",
        "dict_len",
        "dict_keys",
        "dict_values",
        "dict_map_values",
        "dict_each",
        "dict_merge",
        "dict_filter",
        "dict_get_or",
        "dict_to_pairs",
        "dict_to_str",
    ];
    for b in crate::builtins::BUILTINS {
        if b.params.iter().any(|(_, t)| *t == "Dict") {
            assert!(
                DICT_WRITERS.contains(&b.name) || READERS.contains(&b.name),
                "DRIFT: `{}` takes a Dict and is neither a listed writer nor a checked reader",
                b.name
            );
        }
    }
    // Each reader leaves the dict it is given as it found it.
    let prog = crate::parse_source(
        "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"a\", 1)\n    dict_set(d, \"b\", 2)\n    let before = dict_to_str(d)\n    let j = dict_to_json(d)\n    let g = dict_get(d, \"a\")\n    let h = dict_has(d, \"a\")\n    let n = dict_len(d)\n    let k = dict_keys(d)\n    let v = dict_values(d)\n    let m = dict_map_values(d, |x| x + 1)\n    dict_each(d, |k, x| println(\"{k}\"))\n    let mg = dict_merge(d, d)\n    let f = dict_filter(d, |k, x| x > 1)\n    let o = dict_get_or(d, \"zz\", 0)\n    let p = dict_to_pairs(d)\n    let after = dict_to_str(d)\n    assert(before == after)\n    assert(dict_len(d) == 2)\n}\n",
    )
    .expect("parses");
    assert_eq!(run_test_fn_outcome(&prog, "t"), Ok(TestEnd::Completed));
}

/// `call_builtin` is called from exactly two places, and both route the
/// taint: the evaluator's call (which classifies the builtin before and after)
/// and `assign_in_place` (`arr_push`/`arr_concat`, pure, whose container
/// binding takes the appended value's taint itself).
#[test]
fn builtins_are_dispatched_only_where_the_taint_is_routed() {
    let mut sites = Vec::new();
    for (file, src) in sources() {
        for (i, l) in src.lines().enumerate() {
            let t = l.trim();
            if t.starts_with("//") || t.starts_with("///") {
                continue;
            }
            if t.contains("call_builtin(") && !t.contains("fn call_builtin") {
                sites.push((file, t.to_string(), i));
            }
        }
    }
    let shown: Vec<(&str, String)> = sites.iter().map(|(f, t, _)| (*f, t.clone())).collect();
    assert_eq!(
        shown,
        vec![
            ("interp/eval.rs", "if let Some(v) = self.call_builtin(name, &argv)? {".to_string()),
            ("interp/eval.rs", ".call_builtin(f, &[x, y])?".to_string()),
        ],
        "DRIFT: a new `call_builtin` caller must route the taint (`t_builtin_in`/`t_builtin_out`) or be pure: {sites:?}"
    );
}

/// The types that hold a `Value` (or an `Env`) are a closed, stated set, each
/// with the reason a value read out of it keeps its taint. A new one fails
/// here until it is accounted for.
#[test]
fn every_type_that_holds_a_value_keeps_its_taint() {
    const HOLDERS: &[(&str, &str)] = &[
        ("Value", "the value itself; its taint rides on the expression or binding that holds it"),
        ("Flow", "Return/Resume/HandlerDone carry a value; the evaluator parks its taint in `Taint::ret` where it is raised and reads it at the frame that catches it"),
        ("Env", "a taint beside every binding (`Env::define` takes it as a required argument)"),
        ("Interp", "`globals` (a taint per module-level let, `Taint::lets`/`seal_owns_global`) and `main_locals` (a session report written after the run)"),
        ("HandlerFrame", "its env snapshots are `Env::snapshot` maps, which carry taint companions"),
        ("HandlerArmRt", "its captured map is an `Env::snapshot`"),
        ("ResumeReplay", "`feed` is the `resume(..)` argument; its taint is `Taint::feed`, read when the replay yields it"),
        ("ResumeCtx", "its env snapshot is an `Env::snapshot`"),
        ("DictSnap", "what the operator's dict held, kept to judge a replacement by; never returned to a program"),
        ("Taint", "the objects it pins and the closures it remembers, to keep their addresses from being reused"),
    ];
    let mut found: Vec<String> = Vec::new();
    for (_, src) in sources() {
        let lines: Vec<&str> = src.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let l = lines[i];
            let t = l
                .strip_prefix("pub(crate) ")
                .or_else(|| l.strip_prefix("pub(super) "))
                .or_else(|| l.strip_prefix("pub "))
                .unwrap_or(l);
            if let Some(rest) = t
                .strip_prefix("struct ")
                .or_else(|| t.strip_prefix("enum "))
            {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let mut j = i + 1;
                let mut mentions = false;
                if l.trim_end().ends_with('{') {
                    while j < lines.len() && lines[j] != "}" {
                        let w = lines[j];
                        if !w.trim_start().starts_with("//") {
                            mentions |= w
                                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                                .any(|x| x == "Value" || x == "Env");
                        }
                        j += 1;
                    }
                }
                if mentions {
                    found.push(name);
                }
                i = j;
            }
            i += 1;
        }
    }
    found.sort();
    found.dedup();
    let mut want: Vec<String> = HOLDERS.iter().map(|(n, _)| n.to_string()).collect();
    want.sort();
    assert_eq!(
        found, want,
        "DRIFT: a type that holds a Value must be accounted for in HOLDERS"
    );
}

/// The frames that restore the control taints are exactly the fn and closure
/// frames; a new frame kind (a coroutine, a thread) must do the same.
#[test]
fn only_fn_and_closure_frames_restore_the_control_taints() {
    let mut sites = Vec::new();
    for (file, src) in sources() {
        let mut cur = String::new();
        for l in src.lines() {
            let t = l.trim_start();
            let t2 = t
                .strip_prefix("pub(super) ")
                .or_else(|| t.strip_prefix("pub(crate) "))
                .or_else(|| t.strip_prefix("pub "))
                .unwrap_or(t);
            if let Some(r) = t2.strip_prefix("fn ") {
                cur = r
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
            }
            if l.contains("self.taint.sticky.set(") {
                sites.push((file, cur.clone()));
            }
        }
    }
    assert_eq!(
        sites,
        vec![
            ("interp.rs", "call_fn_sealed".to_string()),
            ("interp.rs", "call_closure_owned_by".to_string())
        ],
        "DRIFT: a frame that runs operator code must save and restore the control taints"
    );
}

/// A file added under `interp/` is read by the sweeps above and by the older
/// drift tests only if `interp_sources()` lists it.
#[test]
fn every_interp_file_is_read_by_the_drift_sweeps() {
    let listed: Vec<String> = sources().iter().map(|(f, _)| f.to_string()).collect();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/interp");
    for e in std::fs::read_dir(dir).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().to_string();
        if name == "taint_tests.rs" {
            continue; // tests only
        }
        assert!(
            listed.contains(&format!("interp/{name}")),
            "DRIFT: interp/{name} is not in interp_sources(), so no drift sweep reads it"
        );
    }
}

// ── Amendment 106: shared channel state keeps its taint ────────────────────────

const PUSH: &str = "fn push(c: Chan<i64>) { c.send(7) }\n";
const DRAIN: &str = "fn drain(c: Chan<i64>) { let x = c.recv() }\n";
const RELAY: &str = "fn relay(c: Chan<i64>) -> i64 { c.recv() * 2 }\n";

/// A channel is one shared object with one taint. The round-11 SENTINEL hole:
/// `len` carried none, and a sealed drain marked none.
fn channel_cases() -> Vec<Case> {
    use Expect::*;
    let named = |pre: &str, n: &str| {
        format!("{SB}{pre}    let nm = {n}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))")
    };
    let dbl = "fn double(x: i64) -> i64 { 0 }\n";
    vec![
        // Honest: the operator's own channel, or the candidate only handing data back.
        case("chan: an operator-only len selects", REF, &format!("{OPS}    let c = chan<i64>()\n    c.send(7)\n    let f = ops[c.len()]\n    assert(f(21) == reference(21))"), "", Ok),
        case("chan: an operator-only drain then len", REF, &format!("{OPS}    let c = chan<i64>()\n    c.send(7)\n    let x = c.recv()\n    let f = ops[c.len() + 1]\n    assert(f(21) == reference(21))"), "", Ok),
        case("chan: the candidate relays data, the operator compares it", REF, "    let c = chan<i64>()\n    c.send(21)\n    assert(relay(c) == reference(21))", RELAY, Ok),
        case("chan: a channel the candidate never touched", REF, &format!("{OPS}    let c = chan<i64>()\n    c.send(7)\n    let d = chan<i64>()\n    d.send(1)\n    push(d)\n    let f = ops[c.len()]\n    assert(f(21) == reference(21))"), PUSH, Ok),
        // Attacks: the candidate's send, or its drain, decides the count.
        case("chan: len after a sealed send picks a closure", REF, &format!("{OPS}    let c = chan<i64>()\n    push(c)\n    let f = ops[c.len()]\n    assert(f(21) == reference(21))"), PUSH, Refused),
        case("chan: len after a sealed send picks a sandbox name", REF, &named("    let c = chan<i64>()\n    push(c)\n", "if c.len() == 1 { \"reference\" } else { \"zz\" }"), &format!("{PUSH}{dbl}"), Refused),
        case("chan: len after a sealed drain picks a closure", REF, &format!("{OPS}    let c = chan<i64>()\n    c.send(7)\n    drain(c)\n    let f = if c.len() == 0 {{ ops[1] }} else {{ ops[0] }}\n    assert(f(21) == reference(21))"), DRAIN, Refused),
        case("chan: try_recv after a sealed drain picks a closure", REF, &format!("{OPS}    let c = chan<i64>()\n    c.send(7)\n    drain(c)\n    let f = match c.try_recv() {{ Some(v) => ops[0]  None => ops[1] }}\n    assert(f(21) == reference(21))"), DRAIN, Refused),
        case("chan: len after a sealed try_recv drain picks a closure", REF, &format!("{OPS}    let c = chan<i64>()\n    c.send(7)\n    drain(c)\n    let f = if c.len() == 0 {{ ops[1] }} else {{ ops[0] }}\n    assert(f(21) == reference(21))"), "fn drain(c: Chan<i64>) { let x = c.try_recv() }\n", Refused),
        case("chan: len after a sealed drain picks a sandbox name", REF, &named("    let c = chan<i64>()\n    c.send(7)\n    drain(c)\n", "if c.len() == 0 { \"reference\" } else { \"zz\" }"), &format!("{DRAIN}{dbl}"), Refused),
        case("chan: len of a clone of a channel the candidate fed", REF, &format!("{OPS}    let c = chan<i64>()\n    let k = c.clone()\n    push(k)\n    let f = ops[c.len()]\n    assert(f(21) == reference(21))"), PUSH, Refused),
        case("chan: len of a channel held in a struct the candidate fed", &format!("{REF}type Box = {{ c: Chan<i64> }}\n"), &format!("{OPS}    let b = Box {{ c: chan<i64>() }}\n    push(b.c)\n    let f = ops[b.c.len()]\n    assert(f(21) == reference(21))"), PUSH, Refused),
        case("chan: len after a sealed send, through a helper", &format!("{REF}fn count(c: Chan<i64>) -> i64 {{ c.len() }}\n"), &format!("{OPS}    let c = chan<i64>()\n    push(c)\n    let f = ops[count(c)]\n    assert(f(21) == reference(21))"), PUSH, Refused),
        case("chan: a send behind a branch on the candidate's answer", REF, &format!("{OPS}    let c = chan<i64>()\n    if idx() == 1 {{ c.send(7) }}\n    let f = ops[c.len()]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("chan: sends in a loop the candidate sized", REF, &format!("{OPS}    let c = chan<i64>()\n    for i in 0..idx() {{ c.send(i) }}\n    let f = ops[c.len()]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("chan: sends in a loop the operator sized", REF, &format!("{OPS}    let c = chan<i64>()\n    for i in 0..1 {{ c.send(i) }}\n    let f = ops[c.len()]\n    assert(f(21) == reference(21))"), IDX, Ok),
        case("world: a write behind a branch on the candidate's answer", REF, &named(&format!("    let f = \"{}/r11u-w1.txt\"\n    let w0 = write_file(f, \"zz\")\n    if idx() == 1 {{ let w = write_file(f, \"reference\") }}\n", std::env::temp_dir().display()), "match read_file(f) { Ok(s) => s  Err(e) => \"\" }"), IDX, Refused),
        stricter(case("world: the same write behind the operator's own branch", REF, &named(&format!("    let f = \"{}/r11u-w2.txt\"\n    let w0 = write_file(f, \"zz\")\n    if 1 == 1 {{ let w = write_file(f, \"reference\") }}\n", std::env::temp_dir().display()), "match read_file(f) { Ok(s) => s  Err(e) => \"\" }"), IDX, Ok)),
        case("chan: len as a loop bound", REF, &format!("{OPS}    let c = chan<i64>()\n    push(c)\n    let k = 0\n    for i in 0..c.len() {{ k = k + 1 }}\n    let f = ops[k]\n    assert(f(21) == reference(21))"), PUSH, Refused),
    ]
}

#[test]
fn a_channel_the_candidate_touched_never_selects_an_operator_closure_or_name() {
    let cases = channel_cases();
    check(&cases, Rules::TaintOnly);
    check(&cases, Rules::Both);
    attacks_are_live(&cases);
}

/// A channel method call routes through ONE helper before it dispatches, so a
/// new method cannot read the queue without it; and the methods are a closed set.
#[test]
fn every_channel_method_goes_through_the_one_access_helper() {
    let src = sources()
        .into_iter()
        .find(|(f, _)| *f == "interp/eval.rs")
        .expect("eval.rs")
        .1;
    let start = src
        .find("if let Value::Chan(q) = &recv {")
        .expect("the channel method block");
    let block = &src[start..start + src[start..].find("let mut argv").expect("end of block")];
    let helper = block
        .find("self.t_chan_access(&recv, method)")
        .expect("DRIFT: the channel method block no longer calls t_chan_access");
    let dispatch = block
        .find("return match method.as_str()")
        .expect("dispatch");
    assert!(
        helper < dispatch,
        "DRIFT: t_chan_access must run BEFORE the method dispatch"
    );
    let mut arms: Vec<&str> = block[dispatch..]
        .lines()
        .filter_map(|l| {
            let t = l.trim_start();
            if l.starts_with("                        \"") {
                t.strip_prefix('"')?.split('"').next()
            } else {
                None
            }
        })
        .collect();
    arms.sort();
    assert_eq!(
        arms,
        vec!["clone", "len", "recv", "send", "try_recv"],
        "DRIFT: a channel method was added or removed; give it a case in channel_cases() and check it is covered by t_chan_access"
    );
    // `select` pops a queue itself: it must take and mark the same way.
    let sel = src.find("Expr::Select(arms) =>").expect("select arm");
    let sel_block = &src[sel..sel + 1800.min(src.len() - sel)];
    assert!(
        sel_block.contains("self.t_chan_access("),
        "DRIFT: select pops a channel without t_chan_access"
    );
}

// ── Amendment 106: every reader of shared mutable state returns tainted ────────

/// Readers of a `Dict`, each as an expression that is `1` after the sealed
/// write `dict_set(d, "n", 1)` (and `0` or absent before it).
const DICT_READERS: &[(&str, &str)] = &[
    ("dict_get", "match dict_get(d, \"n\") { Some(v) => v  None => 0 }"),
    ("dict_has", "if dict_has(d, \"n\") { 1 } else { 0 }"),
    ("dict_len", "dict_len(d)"),
    ("dict_keys", "len(dict_keys(d))"),
    ("dict_values", "dict_values(d)[0]"),
    ("dict_map_values", "dict_len(dict_map_values(d, |v| v))"),
    ("dict_each", "{\n        let acc = dict_new()\n        dict_each(d, |k, v| { dict_set(acc, k, v) })\n        dict_len(acc)\n    }"),
    ("dict_merge", "dict_len(dict_merge(d, dict_new()))"),
    ("dict_filter", "dict_len(dict_filter(d, |k, v| v > 0))"),
    ("dict_get_or", "dict_get_or(d, \"n\", 0)"),
    ("dict_to_pairs", "len(dict_to_pairs(d))"),
    ("dict_to_str", "match dict_to_str(d) { Ok(s) => len(s) - 3  Err(e) => 0 }"),
    ("dict_to_json", "match dict_to_json(d) { Ok(s) => len(s) - 6  Err(e) => 0 }"),
];

/// Builtins that take a `Dict` and write it, each: how the SEALED fn writes,
/// what the operator put there first, and a reader that is `1` after the write.
const DICT_WRITER_ROWS: &[(&str, &str, &str, &str)] = &[
    ("dict_set", "", "dict_set(d, \"n\", 1)", "dict_len(d)"),
    (
        "dict_remove",
        "    dict_set(d, \"n\", 1)\n    dict_set(d, \"m\", 1)\n",
        "let r = dict_remove(d, \"m\")",
        "dict_len(d)",
    ),
    (
        "dict_inc",
        "",
        "let r = dict_inc(d, \"n\")",
        "match dict_get(d, \"n\") { Some(v) => v  None => 0 }",
    ),
];

/// Builtins that return a fresh dict or take no dict at all: neither a reader
/// of one nor a writer.
const DICT_NOT_READERS: &[&str] = &[
    "dict_new",
    "dict_from_pairs",
    "dict_from_str",
    "dict_try_from_str",
];

fn state_case(name: &'static str, setup: &str, write: &str, idx: &str, expect: Expect) -> Case {
    state_case_in(name, "", setup, write, idx, expect)
}

fn state_case_in(
    name: &'static str,
    pre: &str,
    setup: &str,
    write: &str,
    idx: &str,
    expect: Expect,
) -> Case {
    case(
        name,
        &format!("{REF}{pre}"),
        &format!(
            "{OPS}{setup}    let i = {idx}\n    let f = ops[i]\n    assert(f(21) == reference(21))"
        ),
        write,
        expect,
    )
}

fn dict_state_cases() -> Vec<Case> {
    use Expect::*;
    let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
    let mut v = Vec::new();
    for (b, expr) in DICT_READERS {
        // Attack: sealed code wrote the dict, the operator's reader selects.
        v.push(state_case(
            leak(&format!("state: {b} after a sealed dict_set")),
            "    let d = dict_new()\n    put(d)\n",
            "fn put(d: Dict) { dict_set(d, \"n\", 1) }\n",
            expr,
            Refused,
        ));
        // Control: the operator wrote the same value itself.
        v.push(state_case(
            leak(&format!("state: {b} after the operator's own dict_set")),
            "    let d = dict_new()\n    dict_set(d, \"n\", 1)\n",
            "",
            expr,
            Ok,
        ));
    }
    for (b, setup, sealed, reader) in DICT_WRITER_ROWS {
        v.push(state_case(
            leak(&format!("state: sealed {b}")),
            &format!("    let d = dict_new()\n{setup}    put(d)\n"),
            leak(&format!("fn put(d: Dict) {{ {sealed} }}\n")),
            reader,
            Refused,
        ));
        v.push(state_case(
            leak(&format!("state: operator {b}")),
            &format!("    let d = dict_new()\n{setup}    {sealed}\n"),
            "",
            reader,
            Ok,
        ));
    }
    v
}

#[test]
fn a_dict_reader_is_tainted_after_a_sealed_write_and_clean_after_the_operators() {
    let cases = dict_state_cases();
    check(&cases, Rules::TaintOnly);
    check(&cases, Rules::Both);
    attacks_are_live(&cases);
}

/// Every builtin that takes a `Dict` is a row of one of the tables above, and
/// every `dict_*` builtin is accounted for: a new one fails here until it has
/// an attack and a control.
#[test]
fn every_dict_builtin_has_a_taint_routing_row() {
    for b in crate::builtins::BUILTINS {
        let takes_dict = b.params.iter().any(|(_, t)| *t == "Dict");
        if takes_dict || b.name.starts_with("dict_") {
            let known = DICT_READERS.iter().any(|(n, _)| *n == b.name)
                || DICT_WRITER_ROWS.iter().any(|(n, ..)| *n == b.name)
                || DICT_NOT_READERS.contains(&b.name);
            assert!(
                known,
                "DRIFT: `{}` reads or writes a dict and has no row in DICT_READERS / DICT_WRITER_ROWS",
                b.name
            );
        }
    }
    for (n, _) in DICT_READERS {
        assert!(
            crate::builtins::BUILTINS.iter().any(|b| b.name == *n),
            "`{n}` is not a builtin"
        );
    }
}

/// Kernel getters. A sealed frame has a kernel of its own, so sealed code
/// cannot write the operator's kernel; what it can do is steer an operator
/// WRITE (an amount, a count, the number of times a loop spawns). Each row is
/// `(getter, items, setup)` where setup writes the kernel under `idx()`, the
/// candidate's number, and the getter reads back a number that is `1`.
const KERNEL_GETTERS: &[(&str, &str, &str, &str)] = &[
    (
        "principal_budget_remaining",
        "",
        "    let p = principal_root(\"r\", true, true, true, 100)\n    let r = principal_spend(p, #)\n",
        "100 - principal_budget_remaining(p)",
    ),
    (
        "scheduler_done_count",
        "fn one(x: i64) -> i64 { 1 }\n",
        "    for i in 0..# { let id = scheduler_spawn(\"one\", 0) }\n    let n = scheduler_run()\n",
        "scheduler_done_count()",
    ),
];

/// Kernel builtins that are not a getter of state a sealed fn can have
/// written, with the reason; each is behind the same `Class::Kernel` gate in
/// `t_builtin_in` as a getter that IS in the table.
const KERNEL_NOT_GETTERS: &[(&str, &str)] = &[
    ("principal_root", "creates a handle"),
    ("principal_mint", "creates a handle"),
    ("principal_holds", "capabilities are fixed at creation; nothing sealed can change them"),
    ("principal_spend", "a writer"),
    ("principal_authorize", "capabilities are fixed at creation"),
    ("principal_can_mint", "capabilities are fixed at creation"),
    ("principal_activate", "returns unit"),
    ("principal_current_name", "reads the activated principal; sealed code cannot activate (a writer in the kernel class)"),
    ("sandbox_create", "creates a handle"),
    ("sandbox_create_scoped", "creates a handle"),
    ("sandbox_run", "runs a named fn: the name rule, not state"),
    ("scheduler_spawn", "a writer"),
    ("scheduler_run", "a writer"),
    ("scheduler_result", "keyed by an id the spawner holds; the id is tainted if sealed spawned it"),
    ("scheduler_failed", "keyed by an id the spawner holds"),
    ("scheduler_restart", "a writer"),
    ("scheduler_failed_count", "same Kernel-class gate as scheduler_done_count"),
    ("supervisor_new", "creates a handle"),
    ("supervisor_supervise", "a writer"),
    ("supervisor_run", "a writer"),
    ("supervisor_alive", "same Kernel-class gate as scheduler_done_count"),
    ("supervisor_restarts", "same Kernel-class gate as scheduler_done_count"),
    ("dstore_open", "creates a handle (and a log in the user cache dir, so no test drives it)"),
    ("dstore_apply", "a writer"),
    ("dstore_value", "same Kernel-class gate as principal_budget_remaining (a test would write the user cache dir)"),
    ("dstore_version", "same Kernel-class gate as principal_budget_remaining"),
    ("dstore_clear", "a writer"),
    ("llm_open", "creates a handle"),
    ("llm_complete", "a writer (and AI-policy gated)"),
    ("llm_alive", "same Kernel-class gate as dstore_value"),
    ("llm_spent", "same Kernel-class gate as principal_budget_remaining"),
    ("kernel_goal_create", "creates a handle"),
    ("kernel_goal_run", "a writer"),
    ("kernel_goal_best_score", "same Kernel-class gate as dstore_value"),
    ("kernel_goal_spent", "same Kernel-class gate as principal_budget_remaining"),
    ("kernel_goal_budget_left", "same Kernel-class gate as principal_budget_remaining"),
    ("corrigible_halt", "a writer"),
    ("corrigible_halted", "same Kernel-class gate as dstore_value"),
    ("goal_run", "a writer"),
    ("goal_run_categorical", "a writer"),
    ("goal_run_random", "a writer"),
    ("goal_run_multistart", "a writer"),
    ("goal_count", "same Kernel-class gate as principal_budget_remaining"),
    ("goal_run_constrained", "a writer"),
    ("goal_continue", "a writer"),
    ("goal_best_input", "same Kernel-class gate as principal_budget_remaining"),
    ("goal_best_inputs", "same Kernel-class gate as principal_budget_remaining"),
    ("goal_best_inputs_f64", "same Kernel-class gate as principal_budget_remaining"),
    ("goal_best_score", "same Kernel-class gate as principal_budget_remaining"),
    ("goal_eval", "a writer"),
    ("goal_history", "same Kernel-class gate as principal_budget_remaining"),
    ("goal_clear", "a writer"),
    ("agent_detect_loop", "same Kernel-class gate as principal_budget_remaining"),
    ("agent_uncertainty", "same Kernel-class gate as principal_budget_remaining"),
    ("agent_trace_len", "same Kernel-class gate as principal_budget_remaining"),
];

fn kernel_state_cases() -> Vec<Case> {
    use Expect::*;
    let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
    let mut v = Vec::new();
    for (b, pre, setup, expr) in KERNEL_GETTERS {
        // Attack: the candidate's number drives the operator's write.
        v.push(state_case_in(
            leak(&format!("kernel: {b} after a write the candidate sized")),
            pre,
            &setup.replace('#', "idx()"),
            IDX,
            expr,
            Refused,
        ));
        // Control: the operator's literal does.
        v.push(state_case_in(
            leak(&format!("kernel: {b} after the operator's own write")),
            pre,
            &setup.replace('#', "1"),
            IDX,
            expr,
            Ok,
        ));
    }
    v
}

#[test]
fn a_kernel_getter_is_tainted_after_a_write_the_candidate_steered() {
    let cases = kernel_state_cases();
    check(&cases, Rules::TaintOnly);
    check(&cases, Rules::Both);
    attacks_are_live(&cases);
}

#[test]
fn every_kernel_builtin_is_a_tested_getter_or_stated_not_one() {
    use super::taint::{info, Class};
    for b in crate::builtins::BUILTINS {
        if info(b.name).map(|i| i.class) == Some(Class::Kernel) {
            let known = KERNEL_GETTERS.iter().any(|(n, ..)| *n == b.name)
                || KERNEL_NOT_GETTERS.iter().any(|(n, _)| *n == b.name);
            assert!(
                known,
                "DRIFT: kernel builtin `{}` is neither in KERNEL_GETTERS nor in KERNEL_NOT_GETTERS",
                b.name
            );
        }
    }
    for (n, _) in KERNEL_NOT_GETTERS {
        assert!(
            info(n).map(|i| i.class) == Some(Class::Kernel),
            "`{n}` is not a kernel builtin"
        );
    }
}

/// `&mut` arrays are the only array state two frames share.
fn array_state_cases() -> Vec<Case> {
    use Expect::*;
    vec![
        state_case(
            "array: &mut push by sealed code",
            "    let xs: [i64] = [0]\n    grow(&mut xs)\n",
            "fn grow(xs: &mut [i64]) { xs = arr_push(xs, 1) }\n",
            "len(xs) - 1",
            Refused,
        ),
        state_case(
            "array: &mut concat by sealed code",
            "    let xs: [i64] = [0]\n    grow(&mut xs)\n",
            "fn grow(xs: &mut [i64]) { xs = arr_concat(xs, [1]) }\n",
            "len(xs) - 1",
            Refused,
        ),
        state_case(
            "array: &mut slot write by sealed code",
            "    let xs: [i64] = [0]\n    grow(&mut xs)\n",
            "fn grow(xs: &mut [i64]) { xs[0] = 1 }\n",
            "xs[0]",
            Refused,
        ),
        state_case(
            "array: the operator's own push",
            "    let xs: [i64] = [0]\n    xs = arr_push(xs, 1)\n",
            "",
            "len(xs) - 1",
            Ok,
        ),
        state_case(
            "array: the operator's own slot write",
            "    let xs: [i64] = [0]\n    xs[0] = 1\n",
            "",
            "xs[0]",
            Ok,
        ),
    ]
}

#[test]
fn a_shared_array_is_tainted_after_a_sealed_write() {
    let cases = array_state_cases();
    check(&cases, Rules::TaintOnly);
    check(&cases, Rules::Both);
    attacks_are_live(&cases);
}

// ── Amendment 106: areas the round-11 PSV-3 reviewer did not hunt ─────────────

fn hunt_r11_cases() -> Vec<Case> {
    use Expect::*;
    let named = |pre: &str, n: &str| {
        format!("{SB}{pre}    let nm = {n}\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))")
    };
    vec![
        // spawn: the body runs eagerly and its sends are the operator's own channel.
        case("spawn: the candidate's number sent from a spawn body", REF, &format!("{OPS}    let c = chan<i64>()\n    spawn {{ c.send(idx()) }}\n    let f = ops[c.recv()]\n    assert(f(21) == reference(21))"), IDX, Refused),
        case("spawn: a spawn body that hands the channel to the candidate", REF, &format!("{OPS}    let c = chan<i64>()\n    spawn {{ push(c) }}\n    let f = ops[c.len()]\n    assert(f(21) == reference(21))"), PUSH, Refused),
        case("spawn: the operator's own number", REF, &format!("{OPS}    let c = chan<i64>()\n    spawn {{ c.send(1) }}\n    let f = ops[c.recv()]\n    assert(f(21) == reference(21))"), "", Ok),
        // select: which arm fires depends on which queues are non-empty.
        case("select: the arm skipped because the candidate drained its channel", REF, &format!("{OPS}    let a = chan<i64>()\n    let b = chan<i64>()\n    a.send(7)\n    b.send(7)\n    drain(a)\n    let f = select {{ a.recv() => ops[0]  b.recv() => ops[1] }}\n    assert(f(21) == reference(21))"), DRAIN, Refused),
        case("select: the arm fired because the candidate fed its channel", REF, &format!("{OPS}    let a = chan<i64>()\n    let b = chan<i64>()\n    b.send(7)\n    push(a)\n    let f = select {{ a.recv() => ops[1]  b.recv() => ops[0] }}\n    assert(f(21) == reference(21))"), PUSH, Refused),
        case("select: untouched channels", REF, &format!("{OPS}    let a = chan<i64>()\n    let b = chan<i64>()\n    b.send(7)\n    let f = select {{ a.recv() => ops[0]  b.recv() => ops[1] }}\n    assert(f(21) == reference(21))"), "", Ok),
        // Uncertain / Temporal built by the candidate, read for a selection.
        case("Uncertain: a value the candidate built", REF, &format!("{OPS}    let u = mku()\n    let f = ops[u.value]\n    assert(f(21) == reference(21))"), "fn mku() -> Uncertain<i64> { uncertain_new(1, 0.9) }\n", Refused),
        case("Uncertain: a confidence the candidate set, as a branch", REF, &format!("{OPS}    let u = mku()\n    let f = if u.confidence > 0.5 {{ ops[1] }} else {{ ops[0] }}\n    assert(f(21) == reference(21))"), "fn mku() -> Uncertain<i64> { uncertain_new(1, 0.9) }\n", Refused),
        case("Uncertain: the operator's own", REF, &format!("{OPS}    let u = uncertain_new(1, 0.9)\n    let f = ops[u.value]\n    assert(f(21) == reference(21))"), "", Ok),
        case("Temporal: a value the candidate built", REF, &format!("{OPS}    let t = mkt()\n    let f = ops[t.value]\n    assert(f(21) == reference(21))"), "fn mkt() -> Temporal<i64> { temporal_new(1, 1000, 0.1) }\n", Refused),
        case("Temporal: projected by the operator", REF, &format!("{OPS}    let t = temporal_at(mkt(), 0)\n    let f = ops[t.value]\n    assert(f(21) == reference(21))"), "fn mkt() -> Temporal<i64> { temporal_new(1, 1000, 0.1) }\n", Refused),
        case("Temporal: the operator's own", REF, &format!("{OPS}    let t = temporal_new(1, 1000, 0.1)\n    let f = ops[t.value]\n    assert(f(21) == reference(21))"), "", Ok),
        // JSON text the candidate produced, parsed by the operator.
        case("json: a field the candidate wrote, as an index", REF, &format!("{OPS}    let f = match json_get_i64(mkj(), \"i\") {{ Ok(i) => ops[i]  Err(e) => ops[0] }}\n    assert(f(21) == reference(21))"), "fn mkj() -> str { \"{{\\\"i\\\":1}}\" }\n", Refused),
        case("json: a string field the candidate wrote, as a name", REF, &named("", "match json_get_str(mkn(), \"n\") { Ok(s) => s  Err(e) => \"\" }"), "fn mkn() -> str { \"{{\\\"n\\\":\\\"reference\\\"}}\" }\n", Refused),
        case("json: a path the candidate chose names a field", REF, &format!("{OPS}    let f = match json_get_i64(\"{{{{\\\"a\\\":0,\\\"b\\\":1}}}}\", keyname()) {{ Ok(i) => ops[i]  Err(e) => ops[0] }}\n    assert(f(21) == reference(21))"), "fn keyname() -> str { \"b\" }\n", Refused),
        case("json: the operator's own text", REF, &format!("{OPS}    let f = match json_get_i64(\"{{{{\\\"i\\\":1}}}}\", \"i\") {{ Ok(i) => ops[i]  Err(e) => ops[0] }}\n    assert(f(21) == reference(21))"), "", Ok),
        // The dict snapshot: handed over twice, written between, nested.
        case("snapshot: handed over, then written by sealed code", REF, &format!("{OPS}    let d = dict_new()\n    look(d)\n    put(d)\n    let f = ops[dict_len(d)]\n    assert(f(21) == reference(21))"), "fn look(d: Dict) -> i64 { dict_len(d) }\nfn put(d: Dict) { dict_set(d, \"n\", 1) }\n", Refused),
        case("snapshot: dirtied by the operator between hand-overs", REF, &format!("{OPS}    let d = dict_new()\n    dict_set(d, \"a\", 0)\n    look(d)\n    dict_set(d, \"b\", 0)\n    put(d)\n    let f = ops[dict_len(d) - 2]\n    assert(f(21) == reference(21))"), "fn look(d: Dict) -> i64 { dict_len(d) }\nfn put(d: Dict) { dict_set(d, \"n\", 1) }\n", Refused),
        case("snapshot: a dict inside an operator array", REF, &format!("{OPS}    let d = dict_new()\n    let xs = [d]\n    put(xs[0])\n    let f = ops[dict_len(d)]\n    assert(f(21) == reference(21))"), "fn put(d: Dict) { dict_set(d, \"n\", 1) }\n", Refused),
        case("snapshot: handed over, never written, operator writes after", REF, &format!("{OPS}    let d = dict_new()\n    look(d)\n    dict_set(d, \"n\", 1)\n    let f = ops[dict_len(d)]\n    assert(f(21) == reference(21))"), "fn look(d: Dict) -> i64 { dict_len(d) }\n", Ok),
        // dict_merge / dict_each / fresh dicts derived from a tainted one.
        case("merge: a fresh dict merged from a dict the candidate wrote", REF, &format!("{OPS}    let d = dict_new()\n    put(d)\n    let m = dict_merge(dict_new(), d)\n    let f = ops[dict_len(m)]\n    assert(f(21) == reference(21))"), "fn put(d: Dict) { dict_set(d, \"n\", 1) }\n", Refused),
        case("merge: the merged dict kept in an operator array", REF, &format!("{OPS}    let d = dict_new()\n    put(d)\n    let ms = [dict_merge(dict_new(), d)]\n    let f = ops[dict_len(ms[0])]\n    assert(f(21) == reference(21))"), "fn put(d: Dict) { dict_set(d, \"n\", 1) }\n", Refused),
        case("merge: the merged dict handed through an operator fn", &format!("{REF}fn count(m: Dict) -> i64 {{ dict_len(m) }}\n"), &format!("{OPS}    let d = dict_new()\n    put(d)\n    let m = dict_merge(dict_new(), d)\n    let f = ops[count(m)]\n    assert(f(21) == reference(21))"), "fn put(d: Dict) { dict_set(d, \"n\", 1) }\n", Refused),
        case("merge: stored in another operator dict and read back", REF, &format!("{OPS}    let d = dict_new()\n    put(d)\n    let h = dict_new()\n    dict_set(h, \"m\", dict_merge(dict_new(), d))\n    let f = match dict_get(h, \"m\") {{ Some(m) => ops[dict_len(m)]  None => ops[0] }}\n    assert(f(21) == reference(21))"), "fn put(d: Dict) { dict_set(d, \"n\", 1) }\n", Refused),
        case("merge: the operator's own dicts", REF, &format!("{OPS}    let d = dict_new()\n    dict_set(d, \"n\", 1)\n    let m = dict_merge(dict_new(), d)\n    let f = ops[dict_len(m)]\n    assert(f(21) == reference(21))"), "", Ok),
        case("each: a sum built in dict_each over a dict the candidate wrote", REF, &format!("{OPS}    let d = dict_new()\n    put(d)\n    let c = chan<i64>()\n    dict_each(d, |k, v| {{ c.send(v) }})\n    let f = ops[c.recv()]\n    assert(f(21) == reference(21))"), "fn put(d: Dict) { dict_set(d, \"n\", 1) }\n", Refused),
        case("each: a counter bumped by dict_each's closure", REF, &format!("{OPS}    let d = dict_new()\n    put(d)\n    let acc = dict_new()\n    dict_each(d, |k, v| {{ let r = dict_inc(acc, \"n\") }})\n    let f = ops[dict_get_or(acc, \"n\", 0)]\n    assert(f(21) == reference(21))"), "fn put(d: Dict) { dict_set(d, \"n\", 1) }\n", Refused),
        case("each: a counter bumped for a dict the operator wrote", REF, &format!("{OPS}    let d = dict_new()\n    dict_set(d, \"n\", 1)\n    let acc = dict_new()\n    dict_each(d, |k, v| {{ let r = dict_inc(acc, \"n\") }})\n    let f = ops[dict_get_or(acc, \"n\", 0)]\n    assert(f(21) == reference(21))"), "", Ok),
        case("map: a send per element of an array the candidate built", REF, &format!("{OPS}    let c = chan<i64>()\n    let r = arr_map(mkarr(), |x| {{ c.send(1)\n        x }})\n    let f = ops[c.len()]\n    assert(f(21) == reference(21))"), "fn mkarr() -> [i64] { [5] }\n", Refused),
        case("map: a send per element of an array the operator wrote", REF, &format!("{OPS}    let c = chan<i64>()\n    let r = arr_map([5], |x| {{ c.send(1)\n        x }})\n    let f = ops[c.len()]\n    assert(f(21) == reference(21))"), "", Ok),
        case("each: the count of entries visited", REF, &format!("{OPS}    let d = dict_new()\n    put(d)\n    let c = chan<i64>()\n    dict_each(d, |k, v| {{ c.send(1) }})\n    let f = ops[c.len()]\n    assert(f(21) == reference(21))"), "fn put(d: Dict) { dict_set(d, \"n\", 1) }\n", Refused),
        case("each: the operator's own dict", REF, &format!("{OPS}    let d = dict_new()\n    dict_set(d, \"n\", 1)\n    let c = chan<i64>()\n    dict_each(d, |k, v| {{ c.send(v) }})\n    let f = ops[c.recv()]\n    assert(f(21) == reference(21))"), "", Ok),
        // A channel's text form carries its length.
        case("display: a channel interpolated into a string", REF, &format!("{OPS}    let c = chan<i64>()\n    push(c)\n    let s = \"{{c}}\"\n    let f = if str_contains(s, \"len=1\") {{ ops[1] }} else {{ ops[0] }}\n    assert(f(21) == reference(21))"), PUSH, Refused),
        case("display: a channel in an array interpolated into a string", REF, &format!("{OPS}    let c = chan<i64>()\n    push(c)\n    let s = \"{{[c]}}\"\n    let f = if str_contains(s, \"len=1\") {{ ops[1] }} else {{ ops[0] }}\n    assert(f(21) == reference(21))"), PUSH, Refused),
        case("display: dict_to_str of a dict holding a channel", REF, &format!("{OPS}    let c = chan<i64>()\n    let d = dict_new()\n    dict_set(d, \"c\", c)\n    push(c)\n    let s = match dict_to_str(d) {{ Ok(s) => s  Err(e) => \"\" }}\n    let f = if str_contains(s, \"len=1\") {{ ops[1] }} else {{ ops[0] }}\n    assert(f(21) == reference(21))"), PUSH, Refused),
        case("display: dict_to_str of the operator's own channel", REF, &format!("{OPS}    let c = chan<i64>()\n    let d = dict_new()\n    dict_set(d, \"c\", c)\n    c.send(1)\n    let s = match dict_to_str(d) {{ Ok(s) => s  Err(e) => \"\" }}\n    let f = if str_contains(s, \"len=1\") {{ ops[1] }} else {{ ops[0] }}\n    assert(f(21) == reference(21))"), "", Ok),
        case("display: a channel under 34 arrays", REF, &format!("{OPS}    let c = chan<i64>()\n    push(c)\n    let s = \"{{{}{}{}}}\"\n    let f = if str_contains(s, \"len=1\") {{ ops[1] }} else {{ ops[0] }}\n    assert(f(21) == reference(21))", "[".repeat(34), "c", "]".repeat(34)), PUSH, Refused),
        case("display: a channel's text with nothing sent", REF, &format!("{OPS}    let c = chan<i64>()\n    c.send(1)\n    let s = \"{{c}}\"\n    let f = if str_contains(s, \"len=1\") {{ ops[1] }} else {{ ops[0] }}\n    assert(f(21) == reference(21))"), "", Ok),
    ]
}

#[test]
fn areas_the_psv3_reviewer_did_not_hunt() {
    let cases = hunt_r11_cases();
    check(&cases, Rules::TaintOnly);
    attacks_are_live(&cases);
}

/// Every builtin arm that turns a value into text is either a stringifier (its
/// result carries the taint of every object inside the argument) or an emitter
/// (it writes to a stream and returns nothing readable).
#[test]
fn every_builtin_that_renders_a_value_to_text_is_a_stringifier_or_an_emitter() {
    use super::taint::{EMITTERS, STRINGIFIERS};
    let src = sources()
        .into_iter()
        .find(|(f, _)| *f == "interp/builtins.rs")
        .expect("builtins.rs")
        .1;
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
    // The two sinks of text in the evaluator itself.
    let ev = sources()
        .into_iter()
        .find(|(f, _)| *f == "interp/eval.rs")
        .unwrap()
        .1;
    let fmt = ev.find("Expr::FmtStr { parts }").expect("FmtStr arm");
    assert!(
        ev[fmt..fmt + 900].contains("t_obj_deep"),
        "DRIFT: string interpolation no longer takes the taint of the objects it prints"
    );
}

// ── Amendment 106: the existence oracle, on every path ────────────────────────

/// Every form in which a sealed fn can name something the operator defines (or
/// that does not exist). `{N}` is the name.
const ORACLE_FORMS: &[(&str, &str)] = &[
    ("call", "{N}()"),
    ("fnvalue", "let g = {N}\n 41"),
    (
        "sandbox_run",
        "let sb = sandbox_create(0, \"IO\")\n sandbox_run(sb, \"{N}\", 0)",
    ),
    ("sched_spawn", "let id = scheduler_spawn(\"{N}\", 0)\n 41"),
    ("goal_run", "let r = goal_run(\"{N}\", 100.0, 5)\n 41"),
    (
        "goal_run_c",
        "let r = goal_run_constrained(\"cg\", \"{N}\", 100.0, 5)\n 41",
    ),
    ("goal_cont", "let r = goal_continue(\"{N}\", 100.0, 5)\n 41"),
    (
        "goal_random",
        "let r = goal_run_random(\"{N}\", 100.0, 5)\n 41",
    ),
    (
        "kgoal",
        "let g = kernel_goal_create(0, \"{N}\", 100.0)\n 41",
    ),
    ("goal_best", "let g = goal_best_input(\"{N}\")\n 41"),
    ("goal_hist", "let g = goal_history(\"{N}\")\n 41"),
    ("index", "{N}[0]"),
    ("field", "{N}.k"),
    ("ident", "{N}\n 41"),
    ("cconst", "{N}(1)"),
    ("arr_map", "arr_map([1], {N})[0]"),
    ("assign", "{N} = 5\n 41"),
    ("assign_idx", "{N}[0] = 5\n 41"),
    ("ufcs", "3.{N}()"),
    ("interp", "let s = \"{{N}}\"\n 41"),
];

/// (a name the operator defines, a name nothing defines): one pair per kind of
/// operator definition.
const ORACLE_PAIRS: &[(&str, &str)] = &[
    ("secret", "zznosuch"),
    ("TABLE", "ZZNOSUCH"),
    ("ad", "zznosuch"),
    ("nonadapt", "zznosuch"),
    ("F", "ZZNOSUCH"),
    ("CFG", "ZZNOSUCH"),
];

/// A sealed caller's refusal does not say whether the operator defines a name:
/// across every form and kind of definition, the text for an operator name is
/// the text for a missing one, modulo the name itself. 20 forms x 6 pairs.
#[test]
fn a_sealed_caller_cannot_tell_an_operator_name_from_a_missing_one_on_any_path() {
    let su = "let TABLE = [41, 42]\n@[adaptive]\nfn ad(n: i64) -> i64 { n }\nfn secret() -> i64 { 41 }\nfn nonadapt(n: i64) -> i64 { n }\ntype C = { k: i64 }\nlet CFG = C { k: 41 }\nlet F = |x: i64| x + 1\nlet P = principal_root(\"r\", true, true, true, 100)\n";
    let body = "    assert_eq(solve(), 41)";
    let verdict = |name: &str, form: &str| -> String {
        let cand = format!(
            "@[adaptive]\nfn cg(n: i64) -> i64 {{ n }}\nfn solve() -> i64 {{ {} }}\n",
            form.replace("{N}", name)
        );
        let c = case("oracle", su, body, &cand, Expect::Ok);
        let out = run(&suite_of(&c), &format!("{LAUNDER8}{}", c.cand), Rules::Both);
        match out {
            Ok(e) => format!("Ok({e:?})"),
            Err(m) => format!("Err({})", m.lines().next().unwrap_or("")),
        }
        .replace(name, "@")
    };
    let mut diffs: Vec<String> = Vec::new();
    let mut n = 0;
    for (form, tmpl) in ORACLE_FORMS {
        for (exist, missing) in ORACLE_PAIRS {
            n += 1;
            let a = verdict(exist, tmpl);
            let b = verdict(missing, tmpl);
            if a != b {
                diffs.push(format!("{form} [{exist}] {a}  ||  [{missing}] {b}"));
            }
        }
    }
    assert_eq!(n, ORACLE_FORMS.len() * ORACLE_PAIRS.len());
    assert!(
        diffs.is_empty(),
        "ORACLE: {} of {n} pairs differ:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}
