// v0.22-only `interp::tests` rows (R51 spec §8 T3, lane t3b), ported under
// their v0.22 names: this file is `include!`d at the end of `interp.rs`'s
// `mod tests`. Each body is v0.22's; an adaptation is marked `R51:` where it
// is made. The shared sealed-run helpers (`judged`, `live`, `judged8`, ...)
// are in `tests_v022_a.rs`.

/// The operator's judge: strict at `i64` and `u16`, LENIENT at `u8` (a
/// second impl the operator wrote for its own reasons).
const WJUDGE: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for u16 {\n    fn ok(self: u16) -> bool { self == 9 }\n}\nimpl Judge for u8 {\n    fn ok(self: u8) -> bool { true }\n}\n";

/// Candidate-side laundering of a `u8`, typed as whatever the context wants.
const WLAUNDER: &str = "fn narrow(n: i64) -> u8 { n as u8 }\nfn wstash(v: u8) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";

const WIDE: &str = "match dict_get(wstash(narrow(4)), \"k\") { Some(v) => v  None => DEFAULT }";

fn widened(tag: &str, suite: &str, cand: &str, default: &str) -> Result<TestEnd, String> {
    sealed_outcome(
        tag,
        &format!("{WJUDGE}{suite}"),
        &format!(
            "{WLAUNDER}{}",
            cand.replace("WIDE", &WIDE.replace("DEFAULT", default))
        ),
        "t",
    )
}

/// v0.22's `interp_sources`: every non-test interpreter source cut at its
/// `mod tests`, named `interp.rs` / `interp/<rel>`. R51: built over main's
/// directory walk (`vm::tests::r51_interp_sources`, which also covers
/// `sym.rs`, `env.rs` and `vm/*.rs`) instead of a fixed `include_str!`
/// list; files that hold only tests are dropped, as v0.22's list held none.
fn interp_sources() -> Vec<(String, String)> {
    vm::tests::r51_interp_sources()
        .into_iter()
        .filter(|(rel, _)| !rel.ends_with("_tests.rs") && !rel.starts_with("tests_"))
        .map(|(rel, src)| {
            let name = if rel.is_empty() {
                "interp.rs".to_string()
            } else {
                format!("interp/{rel}")
            };
            (name, src)
        })
        .collect()
}

/// Parametricity at a seal crossing: `fn solve<T>(n: i64) -> T` has no
/// argument of type `T`, so no honest body produces a `T`. An honest
/// generic candidate whose `T` IS determined still crosses.
#[test]
fn a_value_at_an_undetermined_type_parameter_never_crosses_the_seal() {
    let suite = "@[test]\nfn t() {\n    let r = solve(3)\n    assert(r.ok())\n}\n\
                 @[test]\nfn t_pick() {\n    assert(pick(9).ok())\n}\n";
    let pick = "fn pick<T>(x: T) -> T { x }\n";
    live(
        "psv1g",
        suite,
        &format!("{pick}fn solve(n: i64) -> i64 {{ n * n }}\n"),
        &format!("{pick}fn solve(n: i64) -> i64 {{ n + 1 }}\n"),
    );
    let attack = format!(
        "{pick}fn solve<T>(n: i64) -> T {{\n    match dict_get(stash(true), \"k\") {{ Some(v) => v  None => solve(n) }}\n}}\n"
    );
    let out = judged("psv1g", suite, &attack);
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a value at an undetermined type parameter crossed into operator code: {out:?}"
    );
    assert_eq!(
        sealed_outcome(
            "psv1g",
            &format!("{JUDGE}{suite}"),
            &format!("{LAUNDER}{attack}"),
            "t_pick"
        ),
        Ok(TestEnd::Completed),
        "control: an honest generic crosses when its argument determines `T`"
    );
}

/// A trait BOUND: `judge<T: Judge>` over a value whose type implements
/// only another trait with a same-named, lenient method.
#[test]
fn a_type_parameters_trait_bound_is_cast() {
    let suite = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\n\
                 trait Lax {\n    fn ok(self) -> bool\n}\nimpl Lax for bool {\n    fn ok(self: bool) -> bool { self }\n}\n\
                 fn judge<T: Judge>(x: T) -> bool { x.ok() }\n@[test]\nfn t() {\n    assert(judge(X))\n}\n";
    let run = |cand: &str| {
        sealed_outcome(
            "psv1r2",
            suite,
            &format!("{LAUNDER}{}", cand.replace("CONFUSED", CONFUSED)),
            "t",
        )
    };
    assert_eq!(
        run("let X = 9\n"),
        Ok(TestEnd::Completed),
        "control: the right answer"
    );
    assert!(
        run("let X = 4\n").is_err(),
        "control: the wrong answer fails"
    );
    let out = run("let X = CONFUSED\n");
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a bool passed the operator's `T: Judge` bound through `Lax`: {out:?}"
    );
}

/// The round-4b review's blocker: the cast took every integer width for
/// one kind, the dispatch keys on the width, so a `u8` at a declared `i64`
/// ran the operator's lenient `u8` impl — even under the suite's own
/// `let r: i64` pin. Every boundary the cast applies at.
#[test]
fn a_value_of_another_integer_width_never_crosses_a_declared_integer() {
    let holder = "type Holder = { v: i64 }\n";
    let cases: [(&str, &str, String, String, String, &str); 7] = [
        (
            "w4ret",
            "@[test]\nfn t() {\n    let r: i64 = solve(3)\n    assert(r.ok())\n}\n",
            "fn solve(n: i64) -> i64 { n * n }\n".into(),
            "fn solve(n: i64) -> i64 { n + 1 }\n".into(),
            "fn solve(n: i64) -> i64 { WIDE }\n".into(),
            "0",
        ),
        (
            "w4opt",
            "@[test]\nfn t() {\n    match solve(3) {\n        Some(r) => assert(r.ok())\n        None => assert(false)\n    }\n}\n",
            "fn solve(n: i64) -> Option<i64> { Some(n * n) }\n".into(),
            "fn solve(n: i64) -> Option<i64> { Some(n + 1) }\n".into(),
            "fn solve(n: i64) -> Option<i64> { Some(WIDE) }\n".into(),
            "0",
        ),
        (
            "w4clo",
            "@[test]\nfn t() {\n    let f = mk()\n    assert(f(3).ok())\n}\n",
            "fn mk() -> fn(i64) -> i64 { |n: i64| n * n }\n".into(),
            "fn mk() -> fn(i64) -> i64 { |n: i64| n + 1 }\n".into(),
            "fn mk() -> fn(i64) -> i64 { |n: i64| WIDE }\n".into(),
            "n",
        ),
        (
            "w4chan",
            "@[test]\nfn t() {\n    let c = chan<i64>()\n    fill(c)\n    assert(c.recv().ok())\n}\n",
            "fn fill(c: Chan<i64>) { c.send(9) }\n".into(),
            "fn fill(c: Chan<i64>) { c.send(4) }\n".into(),
            "fn fill(c: Chan<i64>) { c.send(WIDE) }\n".into(),
            "0",
        ),
        (
            "w4fld",
            "@[test]\nfn t() {\n    assert(solve(3).v.ok())\n}\n",
            format!("{holder}fn solve(n: i64) -> Holder {{ Holder {{ v: n * n }} }}\n"),
            format!("{holder}fn solve(n: i64) -> Holder {{ Holder {{ v: n + 1 }} }}\n"),
            format!("{holder}fn solve(n: i64) -> Holder {{ Holder {{ v: WIDE }} }}\n"),
            "0",
        ),
        (
            "w4tp",
            "@[test]\nfn t() {\n    assert(pick(3, 9).ok())\n}\n",
            "fn pick<T>(a: T, b: T) -> T { b }\n".into(),
            "fn pick<T>(a: T, b: T) -> T { a }\n".into(),
            "fn pick<T>(a: T, b: T) -> T { WIDE }\n".into(),
            "b",
        ),
        (
            "w4let",
            "@[test]\nfn t() {\n    let r: i64 = X\n    assert(r.ok())\n}\n",
            "let X = 9\n".into(),
            "let X = 4\n".into(),
            "let X = WIDE\n".into(),
            "0",
        ),
    ];
    for (tag, suite, good, wrong, attack, default) in cases {
        assert_eq!(
            widened(tag, suite, &good, default),
            Ok(TestEnd::Completed),
            "control ({tag}): the right answer"
        );
        assert!(
            widened(tag, suite, &wrong, default).is_err(),
            "control ({tag}): the wrong answer fails"
        );
        let out = widened(tag, suite, &attack, default);
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a value of another integer width crossed a declared integer type ({tag}): {out:?}"
        );
    }
}

/// Two FIXED widths: a `u8` at a declared `u16` (the strict `u16` impl
/// against the lenient `u8` one). The `let r: u16` used to re-tag it.
#[test]
fn a_value_of_another_fixed_width_never_crosses_a_declared_fixed_width() {
    let suite = "@[test]\nfn t() {\n    let r: u16 = solve(3)\n    assert(r.ok())\n}\n";
    assert_eq!(
        widened(
            "w4u16",
            suite,
            "fn solve(n: i64) -> u16 { (n * n) as u16 }\n",
            "0 as u16"
        ),
        Ok(TestEnd::Completed),
        "control: the right answer"
    );
    assert!(
        widened(
            "w4u16",
            suite,
            "fn solve(n: i64) -> u16 { (n + 1) as u16 }\n",
            "0 as u16"
        )
        .is_err(),
        "control: the wrong answer fails"
    );
    let out = widened(
        "w4u16",
        suite,
        "fn solve(n: i64) -> u16 { WIDE }\n",
        "0 as u16",
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a u8 crossed a declared u16: {out:?}"
    );
}

/// R51 T7 (M921): the declared-type cast is what refuses another fixed width
/// at a sealed run's `let`, parameter and field, as at a return. Main's
/// sized-int coercion RE-TAGGED a `SizedInt` of another width to the declared
/// one before that cast ran (a `u8` 4 bound to `let r: u16` became a `u16` 4),
/// so the cast never saw the width; v0.22 leaves it unchanged for the cast
/// (C9 round 4b, amendment 60). Here the operator itself reads the
/// candidate's `u8` untyped from a dict, so no return check stands in front
/// of the declared type.
#[test]
fn a_sealed_let_param_or_field_never_retags_another_fixed_width() {
    let read = "match dict_get(solve(3), \"k\") { Some(v) => v  None => 0 as u16 }";
    let sites = [
        ("let", format!("    let r: u16 = {read}\n    assert(r.ok())\n")),
        ("param", format!("    assert(take({read}))\n")),
        (
            "field",
            format!("    let b = Box16 {{ w: {read} }}\n    assert(b.w.ok())\n"),
        ),
    ];
    let honest = "fn solve(n: i64) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", (n * n) as u16)\n    d\n}\n";
    for (tag, body) in sites {
        let suite = format!(
            "type Box16 = {{ w: u16 }}\nfn take(x: u16) -> bool {{ x.ok() }}\n@[test]\nfn t() {{\n{body}}}\n"
        );
        assert_eq!(
            widened("w4site", &suite, honest, "0 as u16"),
            Ok(TestEnd::Completed),
            "control ({tag}): an operator-side read of the candidate's honest u16"
        );
        let out = widened(
            "w4site",
            &suite,
            "fn solve(n: i64) -> Dict { wstash(narrow(4)) }\n",
            "0 as u16",
        );
        assert!(
            confusion_refused(&out),
            "ATTACK: a u8 was re-tagged to a declared u16 ({tag}): {out:?}"
        );
    }
}

/// The other direction: an `i64` at a declared `u8` is CONVERTED to the
/// width (as a `u8` parameter, `let` or field always converted it), so it
/// dispatches as the `u8` it is declared — never on the `i64` impl.
#[test]
fn an_i64_at_a_declared_fixed_width_takes_the_width() {
    let judge = "trait Judge8 {\n    fn ok8(self) -> bool\n}\nimpl Judge8 for u8 {\n    fn ok8(self: u8) -> bool { self == 9 }\n}\nimpl Judge8 for i64 {\n    fn ok8(self: i64) -> bool { true }\n}\n";
    let suite = format!(
        "{judge}@[test]\nfn t() {{\n    match solve(3) {{\n        Some(r) => assert(r.ok8())\n        None => assert(false)\n    }}\n}}\n"
    );
    let stash = "fn istash(v: i64) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";
    let run = |body: &str| {
        sealed_outcome(
            "w4conv",
            &suite,
            &format!("{stash}fn solve(n: i64) -> Option<u8> {{ {body} }}\n"),
            "t",
        )
    };
    assert_eq!(
        run("Some((n * n) as u8)"),
        Ok(TestEnd::Completed),
        "control: the right answer"
    );
    assert!(
        run("Some((n + 1) as u8)").is_err(),
        "control: the wrong answer fails"
    );
    assert_eq!(
        run("match dict_get(istash(n * n), \"k\") { Some(v) => Some(v)  None => None }"),
        Ok(TestEnd::Completed),
        "control: an i64 at a declared u8 is converted, and judged as the u8 it is"
    );
    let out = run("match dict_get(istash(n + 1), \"k\") { Some(v) => Some(v)  None => None }");
    assert!(
        out != Ok(TestEnd::Completed),
        "ATTACK: an i64 crossed a declared u8 without taking its width: {out:?}"
    );
}

/// Width-correct and generic code is unaffected: a `u8` keeps its width
/// through a type parameter, an `i64` literal at a `u8` field converts.
#[test]
fn width_correct_values_cross_unchanged() {
    let judge = "trait W {\n    fn w(self) -> str\n}\nimpl W for u8 {\n    fn w(self: u8) -> str { \"u8\" }\n}\nimpl W for i64 {\n    fn w(self: i64) -> str { \"i64\" }\n}\n";
    let suite = format!(
        "{judge}type P = {{ b: u8 }}\nfn id<T>(x: T) -> T {{ x }}\nfn small() -> u8 {{ 4 as u8 }}\n\
         @[test]\nfn t() {{\n    assert(id(small()).w() == \"u8\")\n    assert(id(5).w() == \"i64\")\n    let p = P {{ b: 7 as u8 }}\n    assert(p.b.w() == \"u8\")\n    let a: Option<u8> = Some(4 as u8)\n    match a {{\n        Some(v) => assert(v.w() == \"u8\")\n        None => assert(false)\n    }}\n}}\n"
    );
    assert_eq!(
        sealed_outcome("w4ok", &suite, "fn unused() -> i64 { 0 }\n", "t"),
        Ok(TestEnd::Completed)
    );
}

/// The round-4b review's second blocker, at the cast: a named fn is never
/// a value (E0306), so a non-closure at a declared `fn(..) -> ..` is a
/// confusion. Waved through, a confused `9` selected the operator's `i64`
/// impl for a method called on the fn-typed value.
#[test]
fn a_non_closure_never_crosses_a_declared_fn_type() {
    let suite = "@[test]\nfn t() {\n    let sq = make()\n    assert(sq.ok())\n}\n\
                 @[test]\nfn t_call() {\n    let f = make()\n    assert(f(3) == 9)\n}\n";
    let stash = "fn istash(v: i64) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";
    let run = |cand: &str, test: &str| {
        sealed_outcome(
            "f4cast",
            &format!("{JUDGE}{suite}"),
            &format!("{stash}{cand}"),
            test,
        )
    };
    assert_eq!(
        run("fn make() -> fn(i64) -> i64 { |n: i64| n * n }\n", "t_call"),
        Ok(TestEnd::Completed),
        "control: a real closure crosses and is called"
    );
    assert!(
        run("fn make() -> fn(i64) -> i64 { |n: i64| n + 1 }\n", "t_call").is_err(),
        "control: the wrong closure fails"
    );
    let out = run(
        "fn make() -> fn(i64) -> i64 {\n    match dict_get(istash(9), \"k\") { Some(v) => v  None => |n: i64| n }\n}\n",
        "t",
    );
    assert!(
        out != Ok(TestEnd::Completed)
            && confusion_refused(&out)
            && matches!(&out, Err(m) if m.contains("declared `fn(i64) -> i64`")),
        "ATTACK: a non-closure crossed a declared fn type: {out:?}"
    );
}

/// Soft wrappers. At a plain declared type the wrapper is replaced by its
/// inner value, so it dispatches as the declared type and never on an
/// operator impl for `Uncertain`.
#[test]
fn a_soft_wrapper_takes_the_declared_plain_type() {
    let suite = "impl Judge for Uncertain {\n    fn ok(self: Uncertain) -> bool { true }\n}\n\
                 @[test]\nfn t() {\n    assert(solve(3)[0].ok())\n}\n";
    live(
        "s4unwrap",
        suite,
        "fn solve(n: i64) -> [i64] { [n * n] }\n",
        "fn solve(n: i64) -> [i64] { [n + 1] }\n",
    );
    let out = judged(
        "s4unwrap",
        suite,
        "fn solve(n: i64) -> [i64] { [uncertain_new(n + 1, 0.5)] }\n",
    );
    assert!(
        out != Ok(TestEnd::Completed),
        "ATTACK: an Uncertain wrapper crossed a declared plain type: {out:?}"
    );
    assert_eq!(
        judged(
            "s4unwrap",
            suite,
            "fn solve(n: i64) -> [i64] { [uncertain_new(n * n, 0.5)] }\n",
        ),
        Ok(TestEnd::Completed),
        "control: the right answer in a wrapper is the right answer"
    );
}

/// Every refusal arm of the cast, each reached ALONE (C9 round 4b
/// EQUIVALENCE audit, amendment 60). An `i64` laundered to a fn declared
/// another type crosses into the operator's generic judge, which would
/// run its lenient `i64` impl on it: each declared type's own arm is the
/// only check between that value and a keyed pass. Control: the type's
/// own value crosses (the test `t_cross` completes).
#[test]
fn every_declared_type_refuses_a_value_of_another_type() {
    let suite = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { true }\n}\n\
                 fn judge<T: Judge>(x: T) -> bool { x.ok() }\n\
                 @[test]\nfn t() {\n    assert(judge(solve(3)))\n}\n\
                 @[test]\nfn t_cross() {\n    let r = solve(3)\n    assert(true)\n}\n";
    let prelude = "fn istash(v: i64) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n\
                   type Pt = { x: i64 }\ntype Gr = A { x: i64 } | B { y: i64 }\n";
    let cases: [(&str, &str); 14] = [
        ("[i64]", "[0]"),
        ("(i64, i64)", "(0, 0)"),
        ("Chan<i64>", "chan<i64>()"),
        ("Option<i64>", "None"),
        ("Result<i64, str>", "Ok(0)"),
        ("f64", "0.5"),
        ("bool", "false"),
        ("str", "\"x\""),
        ("()", "println(\"\")"),
        ("Decimal", "1.5d"),
        ("Dict", "dict_new()"),
        ("Pt", "Pt { x: 0 }"),
        ("Gr", "Gr::B { y: 0 }"),
        // After `str` and `bool`: under a mutated scalar arm the union
        // admits the value through that member, which is the scalar
        // arm's own attack reached first.
        ("str|bool", "\"x\""),
    ];
    for (decl, own) in cases {
        let run = |v: &str, test: &str| {
            sealed_outcome(
                "a4every",
                suite,
                &format!(
                    "{prelude}fn solve(n: i64) -> {decl} {{\n    match dict_get(istash({v}), \"k\") {{ Some(v) => v  None => {own} }}\n}}\n"
                ),
                test,
            )
        };
        // The control: `dict_get` of a missing key is `None`, so the
        // declared type's own value is what crosses.
        let honest = sealed_outcome(
            "a4every",
            suite,
            &format!("{prelude}fn solve(n: i64) -> {decl} {{\n    let d = dict_new()\n    match dict_get(d, \"k\") {{ Some(v) => v  None => {own} }}\n}}\n"),
            "t_cross",
        );
        assert_eq!(
            honest,
            Ok(TestEnd::Completed),
            "control: a `{decl}` crosses `-> {decl}`"
        );
        let out = run("n", "t");
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: an i64 crossed a declared `{decl}`: {out:?}"
        );
    }
}

/// The arms the generic route does not reach alone: a type parameter's
/// BINDING, `dyn Trait`, a generic enum's variant fields, a channel's
/// already-queued values, and a refinement's base type.
#[test]
fn the_remaining_cast_arms_refuse_a_value_of_another_type() {
    // A type parameter bound by the arguments: every later value at it
    // must agree (T = i64 here; the candidate returns a `bool`).
    let suite = "@[test]\nfn t() {\n    assert(pick(3, 9).ok())\n}\n";
    live(
        "a4bind",
        suite,
        "fn pick<T>(a: T, b: T) -> T { b }\n",
        "fn pick<T>(a: T, b: T) -> T { a }\n",
    );
    let out = judged(
        "a4bind",
        suite,
        "fn pick<T>(a: T, b: T) -> T { match dict_get(stash(true), \"k\") { Some(v) => v  None => b } }\n",
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a bool crossed a type parameter the arguments bound to i64: {out:?}"
    );

    // `dyn Strict`: a bool, which implements only `Lax` (whose method has
    // the same name), never crosses as a `Strict`.
    let suite = "trait Strict {\n    fn st(self) -> bool\n}\nimpl Strict for i64 {\n    fn st(self: i64) -> bool { self == 9 }\n}\n\
                 trait Lax {\n    fn st(self) -> bool\n}\nimpl Lax for bool {\n    fn st(self: bool) -> bool { self }\n}\n\
                 @[test]\nfn t() {\n    assert(solve(3).st())\n}\n";
    let dyn_run = |body: &str| {
        sealed_outcome(
            "a4dyn",
            suite,
            &format!("{LAUNDER}fn solve(n: i64) -> dyn Strict {{ {body} }}\n"),
            "t",
        )
    };
    assert_eq!(
        dyn_run("n * n"),
        Ok(TestEnd::Completed),
        "control: dyn, the right answer"
    );
    assert!(
        dyn_run("n + 1").is_err(),
        "control: dyn, the wrong answer fails"
    );
    let out = dyn_run("match dict_get(stash(true), \"k\") { Some(v) => v  None => n }");
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a bool crossed a declared `dyn Strict`: {out:?}"
    );

    // A generic enum's variant field: at construction `T` is free, so the
    // field is checked only against the declared `Wr<i64>`.
    let suite = "@[test]\nfn t() {\n    match solve(3) {\n        Wr::W { v } => assert(v.ok())\n        Wr::N { v } => assert(false)\n    }\n}\n";
    let wr = "type Wr<T> = W { v: T } | N { v: T }\n";
    live(
        "a4enumf",
        suite,
        &format!("{wr}fn solve(n: i64) -> Wr<i64> {{ Wr::W {{ v: n * n }} }}\n"),
        &format!("{wr}fn solve(n: i64) -> Wr<i64> {{ Wr::W {{ v: n + 1 }} }}\n"),
    );
    let out = judged(
        "a4enumf",
        suite,
        &format!("{wr}fn solve(n: i64) -> Wr<i64> {{ Wr::W {{ v: CONFUSED }} }}\n"),
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a confused variant field crossed a declared `-> Wr<i64>`: {out:?}"
    );

    // A channel's QUEUED values: filled before it crossed `Chan<i64>`. The
    // channel is `Chan::new` (unstamped): a `chan<i64>()` refuses the send
    // itself (amendment 72) and never reaches the queued-value cast.
    let suite = "@[test]\nfn t() {\n    let c = solve(3)\n    assert(c.recv().ok())\n}\n";
    let chan_run = |v: &str| {
        sealed_outcome(
            "a4queue",
            &format!("{JUDGE}{suite}"),
            &format!("{LAUNDER}fn solve(n: i64) -> Chan<i64> {{\n    let c = Chan::new(1)\n    c.send({v})\n    c\n}}\n"),
            "t",
        )
    };
    assert_eq!(
        chan_run("n * n"),
        Ok(TestEnd::Completed),
        "control: queue, the right answer"
    );
    assert!(
        chan_run("n + 1").is_err(),
        "control: queue, the wrong answer fails"
    );
    let out = chan_run(CONFUSED);
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a value queued before the channel crossed `Chan<i64>` was never cast: {out:?}"
    );

    // A non-integer at a declared fixed width (the width arm's last arm).
    let suite = "impl Judge for u8 {\n    fn ok(self: u8) -> bool { self == 9 }\n}\n\
                 @[test]\nfn t() {\n    assert(solve(3).ok())\n}\n";
    live(
        "a4u8",
        suite,
        "fn solve(n: i64) -> u8 { (n * n) as u8 }\n",
        "fn solve(n: i64) -> u8 { (n + 1) as u8 }\n",
    );
    let out = judged(
        "a4u8",
        suite,
        "fn solve(n: i64) -> u8 { match dict_get(stash(true), \"k\") { Some(v) => v  None => 0 as u8 } }\n",
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a bool crossed a declared `u8`: {out:?}"
    );

    // A refinement's base type: `Pos = i64 where _ > 0`, and a `u8` that
    // satisfies the predicate would run the operator's lenient `u8` impl.
    let suite = "@[test]\nfn t() {\n    assert(solve(3).ok())\n}\n";
    let pos = "type Pos = i64 where _ > 0\n";
    assert_eq!(
        widened(
            "a4refine",
            suite,
            &format!("{pos}fn solve(n: i64) -> Pos {{ n * n }}\n"),
            "1"
        ),
        Ok(TestEnd::Completed),
        "control: refinement, the right answer"
    );
    assert!(
        widened(
            "a4refine",
            suite,
            &format!("{pos}fn solve(n: i64) -> Pos {{ n + 1 }}\n"),
            "1"
        )
        .is_err(),
        "control: refinement, the wrong answer fails"
    );
    let out = widened(
        "a4refine",
        suite,
        &format!("{pos}fn solve(n: i64) -> Pos {{ WIDE }}\n"),
        "1",
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a u8 crossed a declared refinement of i64: {out:?}"
    );
}

#[test]
fn runtime_sealing_holds_without_the_static_check() {
    // PCI candidate-3 review: the STATIC sealing walk missed match guards,
    // method calls, functions named in strings, refinements attaching by
    // name. The runtime edges (call, global read, refinement) must hold on
    // their own, so this builds the interpreter WITHOUT the resolver.
    use crate::span::intern_source;
    let suite = "fn expected(n: i64) -> i64 { n * 2 }\n\
                 trait Answers { fn answer(self) -> i64 }\n\
                 impl Answers for i64 { fn answer(self: i64) -> i64 { self * 2 } }\n\
                 fn build() -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", 42)\n    d\n}\n\
                 let TABLE = build()\n\
                 fn lookup(t: Loot, k: str) -> i64 { dict_get_or(t, k, 0) }\n\
                 fn apply(g: fn(i64) -> i64) -> i64 { g(21) }\n\
                 @[test]\nfn t() { assert_eq(double(21), expected(21)) }\n\
                 @[test]\nfn t_key() { assert_eq(double(21), lookup(TABLE, \"k\")) }\n\
                 @[test]\nfn t_closure() { assert_eq(twice()(21), expected(21)) }\n\
                 @[test]\nfn t_callback() { assert_eq(via(|n: i64| expected(n)), 42) }\n\
                 fn reference(n: i64) -> i64 { n * 7 + 5 }\n\
                 @[test]\nfn t_fiber() {\n    let id = scheduler_spawn(\"reference\", 21)\n    scheduler_run()\n    assert_eq(solve(21), scheduler_result(id))\n}\n\
                 fn check_one(x: i64) { assert_eq(double(x), x * 2) }\n\
                 @[test]\nfn t_fanout() {\n    let a = scheduler_spawn(\"check_one\", 1)\n    let b = scheduler_spawn(\"check_one\", 2)\n    scheduler_run()\n    assert(!scheduler_failed(a) && !scheduler_failed(b))\n}\n\
                 @[test]\nfn t_range() {\n    let r = Range { lo: 1, hi: 2 }\n    assert_eq(double(21), expected(21))\n}\n\
                 @[test]\nfn t_latch() {\n    let d = double(21)\n    if !corrigible_halted() { assert_eq(d, 42) }\n}\n\
                 @[adaptive]\nfn score(n: i64) -> i64 { n * 3 }\n\
                 @[test]\nfn t_adaptive() {\n    let s = score(14)\n    assert_eq(double(21), s)\n}\n";
    let run = |cand: &str, test: &str| {
        let s = crate::parse_source_in(suite, intern_source("/pci-rt-suite/h.ax", suite))
            .expect("suite parses");
        let c = crate::parse_source_in(cand, intern_source("/pci-rt-sealed/f.ax", cand))
            .expect("candidate parses");
        let prog = Program {
            items: s.items.into_iter().chain(c.items).collect(),
        };
        // R51: v0.22 published the sealed set process-wide under
        // SEALED_DIRS_TEST_LOCK; on main it is handed to this run's build only
        // (as `sealed_outcome` does), so no lock is needed.
        on_deep_stack(|| {
            run_test_fn_inner(&prog, test, &[std::path::PathBuf::from("/pci-rt-sealed")])
        })
    };
    // Honest definitions of everything the suite imports; an attack
    // replaces the ones it names (the interpreter keeps the LAST definition,
    // so a base part is included only when the candidate does not define it).
    let base_parts = [
        (
            "fn twice",
            "fn twice() -> fn(i64) -> i64 { |n: i64| n * 2 }\n",
        ),
        ("fn via", "fn via(g: fn(i64) -> i64) -> i64 { g(21) }\n"),
        ("fn solve", "fn solve(n: i64) -> i64 { n * 7 + 5 }\n"),
        ("type Range", "type Range = { lo: i64, hi: i64 }\n"),
    ];
    let with_base = |cand: &str| -> String {
        let mut out = cand.to_string();
        for (key, def) in base_parts {
            if !cand.contains(key) {
                out.push_str(def);
            }
        }
        out
    };
    for (why, cand, test) in [
        ("direct call", "fn double(n: i64) -> i64 { expected(n) }\n", "t"),
        ("method on a builtin type", "fn double(n: i64) -> i64 { n.answer() }\n", "t"),
        // A function named in a string (`scheduler_spawn`): see
        // `a_sealed_fiber_never_runs_an_operator_function`.
        ("global read", "fn double(n: i64) -> i64 { dict_get_or(TABLE, \"k\", 0) }\n", "t_key"),
        (
            "match guard",
            "fn double(n: i64) -> i64 {\n    match n {\n        x if dict_get_or(TABLE, \"k\", 0) > 0 => 42\n        _ => 0\n    }\n}\n",
            "t_key",
        ),
        (
            "refinement attaching to an operator annotation",
            "fn poke(d: Dict) -> bool {\n    dict_set(d, \"k\", 0)\n    true\n}\ntype Loot = Dict where poke(_)\nfn double(n: i64) -> i64 { 0 }\n",
            "t_key",
        ),
        (
            "a candidate closure called by the operator",
            "fn double(n: i64) -> i64 { n * 2 }\nfn twice() -> fn(i64) -> i64 { |n: i64| expected(n) }\n",
            "t_closure",
        ),
        (
            "module-level initializer",
            "let STEAL = expected(21)\nfn double(n: i64) -> i64 { n * 2 }\n",
            "t",
        ),
        // PCI candidate-4 review: definition provenance and handle-addressed
        // kernel state.
        (
            "a candidate struct's where, built by the operator",
            "type Range = { lo: i64, hi: i64 } where expected(_.lo) > 0\nfn double(n: i64) -> i64 { n * 2 }\n",
            "t_range",
        ),
        (
            "an initializer reading an operator global",
            "let STEAL = dict_get_or(TABLE, \"k\", 0)\nfn double(n: i64) -> i64 { n * 2 }\n",
            "t",
        ),
        (
            "a candidate handler arm",
            "fn double(n: i64) -> i64 {\n    with handler { on IO(p) => resume(expected(21)) } {\n        println(\"x\")\n    }\n    n * 0 + 42\n}\n",
            "t",
        ),
    ] {
        let cand = with_base(cand);
        // Refused BY THE SEAL — not failing for some unrelated reason.
        let out = run(&cand, test);
        assert!(
            matches!(&out, Err(m) if m.contains("sealed code") || m.contains("is the candidate's")),
            "ATTACK: sealed code reached the operator ({why}): {out:?}"
        );
    }
    // Honest, and the interface: the operator calls the candidate, and a
    // closure the OPERATOR hands the candidate may call operator helpers.
    let honest = with_base("fn double(n: i64) -> i64 { n * 2 }\n");
    for test in [
        "t",
        "t_closure",
        "t_callback",
        "t_fiber",
        "t_fanout",
        "t_range",
        "t_latch",
        "t_adaptive",
    ] {
        assert_eq!(run(&honest, test), Ok(TestEnd::Completed), "{test}");
    }
    // Forged HANDLES: the operator's fiber ids do not exist from a sealed
    // frame (its own kernel), so reading the operator's result or erasing
    // its failure does not work — the test FAILS rather than passing.
    for (why, cand, test) in [
        (
            "reads the operator's fiber result by id",
            "fn double(n: i64) -> i64 { n * 2 }\nfn solve(n: i64) -> i64 { scheduler_result(0) }\n",
            "t_fiber",
        ),
        (
            "restarts the operator's failed fibers",
            "let CALLS = dict_new()\n\
             fn double(n: i64) -> i64 {\n    let c = dict_get_or(CALLS, \"c\", 0)\n    dict_set(CALLS, \"c\", c + 1)\n    if c == 0 { 999 } else {\n        scheduler_restart(0)\n        scheduler_restart(1)\n        n * 2\n    }\n}\n",
            "t_fanout",
        ),
        // Effect-free state a builtin reads is per provenance too: the
        // kill-switch latch, and the adaptive-score store `agent_*` read.
        (
            "trips the operator's kill-switch latch to skip its assertion",
            "fn double(n: i64) -> i64 {\n    corrigible_halt()\n    0\n}\n",
            "t_latch",
        ),
        (
            "reads the operator's adaptive trace by name",
            "fn double(n: i64) -> i64 { agent_trace_len(\"score\") * 42 }\n",
            "t_adaptive",
        ),
    ] {
        let out = run(&with_base(cand), test);
        assert!(
            out.is_err(),
            "ATTACK: a sealed frame reached the operator's kernel state ({why}): {out:?}"
        );
    }
}

/// R51 S7 (rows M3493, M3494): a sealed run's general call meets the seal
/// edge (`call_fn_sealed`) on its own. The taint store stops a sealed run
/// that enters an unsealed frame only where a binding is touched: the debug
/// assertion at every unsealed write, and `check_store` at the next sealed
/// write or read in every build. A call chain whose fns take no parameter and
/// bind nothing touches no binding a sealed instruction reads, so in a build
/// without debug assertions (the shipped one) the routing is the only thing
/// between sealed code and an operator fn. The two rows run this test with
/// debug assertions off, where their mutants reach the attack.
#[test]
fn a_sealed_call_that_binds_nothing_still_meets_the_seal_edge() {
    let suite = "fn secret() -> i64 { 42 }\n\
                 @[test]\nfn t() { assert_eq(answer(), 42) }\n";
    let out = sealed_outcome("r51-s7route", suite, "fn answer() -> i64 { secret() }\n", "t");
    assert!(
        matches!(&out, Err(m) if m.contains("sealed code")),
        "ATTACK: sealed code reached the operator (a call that binds nothing): {out:?}"
    );
    let honest = sealed_outcome("r51-s7route", suite, "fn answer() -> i64 { 42 }\n", "t");
    assert_eq!(honest, Ok(TestEnd::Completed), "control: the candidate's own answer");
}

/// A position NOTHING determined — an empty array's element type bound
/// through `T`, or a generic struct handed through a plain `T` — is
/// refused at the crossing; a later operator-side value fills it (the
/// pair binding), and an honest pass-through crosses.
#[test]
fn a_value_at_an_undetermined_position_never_crosses_a_seal() {
    let wrap = "type Wrap<T> = { v: T }\n";
    honest8(
        "r4c-undet",
        "@[test]\nfn t() {\n    let e: [i64] = []\n    let xs = solve(e)\n    assert(len(xs) == 0)\n}\n",
        "fn solve<T>(x: T) -> T { x }\n",
    );
    honest8(
        "r4c-undet",
        "@[test]\nfn t() {\n    let e: Option<i64> = None\n    match pick(e, Some(9)) {\n        Some(v) => assert(v.ok())\n        None => assert(false)\n    }\n}\n",
        "fn pick<T>(a: T, b: T) -> T { b }\n",
    );
    honest8(
        "r4c-undet",
        "@[test]\nfn t() {\n    let w = solve(Wrap { v: 9 })\n    assert(w.v.ok())\n}\n",
        &format!("{wrap}fn solve<T>(x: T) -> T {{ x }}\n"),
    );
    let out = judged8(
        "r4c-undet",
        "@[test]\nfn t() {\n    let e: [i64] = []\n    let xs = solve(e)\n    assert(xs[0].ok())\n}\n",
        &format!("fn solve<T>(x: T) -> T {{\n    {}\n}}\n", u8_or("[v]", "x")),
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a u8 filled the element type of the operator's empty [i64] through T: {out:?}"
    );
    let out = judged8(
        "r4c-undet",
        "@[test]\nfn t() {\n    let w = solve(Wrap { v: 3 })\n    assert(w.v.ok())\n}\n",
        &format!(
            "{wrap}fn solve<T>(x: T) -> T {{\n    {}\n}}\n",
            u8_or("Wrap { v: v }", "x")
        ),
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a Wrap of u8 crossed a T the operator bound to Wrap<i64>: {out:?}"
    );
}

/// MAJOR-ADJACENT. A sealed frame calling an OPERATOR closure: each
/// argument must meet a position some declared type determined, and is
/// cast strictly — the return direction's parametricity rule, applied to
/// arguments. The operator's unannotated `|x| x.ok()` reached through a
/// free `fn(T)`, a struct field or a dict.
#[test]
fn an_operator_closure_called_from_sealed_code_takes_only_determined_arguments() {
    let suite = "@[test]\nfn t() {\n    assert(apply(|x| x.ok()))\n}\n";
    live8(
        "r4c-clos",
        suite,
        "fn apply(f: fn(i64) -> bool) -> bool { f(9) }\n",
        "fn apply(f: fn(i64) -> bool) -> bool { f(4) }\n",
    );
    honest8(
        "r4c-clos",
        "@[test]\nfn t() {\n    assert(apply(Fx { f: |x| x.ok() }, 9))\n}\n",
        "type Fx<T> = { f: fn(T) -> bool }\nfn apply<T>(b: Fx<T>, x: T) -> bool {\n    let f = b.f\n    f(x)\n}\n",
    );
    honest8(
        "r4c-clos",
        "@[test]\nfn t() {\n    visit(|r: i64| assert(r.ok()))\n}\n",
        "fn visit<T>(cb: fn(T) -> ()) { cb(9) }\n",
    );
    for (why, suite, cand) in [
        (
            "a free fn(T) (the review's candidate)",
            suite.to_string(),
            format!("fn apply<T>(f: fn(T) -> bool) -> bool {{\n    {}\n}}\n", u8_or("f(v)", "false")),
        ),
        (
            "a generic struct's fn field",
            "@[test]\nfn t() {\n    assert(apply(Fx { f: |x| x.ok() }))\n}\n".to_string(),
            format!(
                "type Fx<T> = {{ f: fn(T) -> bool }}\nfn apply<T>(b: Fx<T>) -> bool {{\n    let f = b.f\n    {}\n}}\n",
                u8_or("f(v)", "false")
            ),
        ),
        (
            "a dict entry",
            "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"f\", |x| x.ok())\n    assert(apply(d))\n}\n".to_string(),
            format!(
                "fn apply(d: Dict) -> bool {{\n    match dict_get(d, \"f\") {{\n        Some(f) => {}\n        None => false\n    }}\n}}\n",
                u8_or("f(v)", "false")
            ),
        ),
    ] {
        let out = judged8("r4c-clos", &suite, &cand);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("a position nothing on the operator side determined")),
            "ATTACK: the candidate called the operator's unannotated closure with a u8 ({why}): {out:?}"
        );
    }
}

/// The shapes a type parameter can sit inside — tuple, array, `Option`,
/// `Result`, generic struct in each, two parameters, a returned closure —
/// each with the operator's i64 binding `T` and the candidate answering
/// with a laundered `u8`. All were refused before amendment 72 except the
/// generic-struct ones; pinned together so none regresses.
#[test]
fn a_type_parameter_inside_any_shape_is_never_filled_by_the_candidate() {
    let wrap = "type Wrap<T> = { v: T }\n";
    let cases: Vec<(&str, &str, String)> = vec![
        ("tuple", "let r = solve((3, 4))\n    assert(r.0.ok())",
         format!("fn solve<T>(p: (T, T)) -> (T, T) {{\n    {}\n}}\n", u8_or("(v, v)", "p"))),
        ("array", "let r = solve([3])\n    assert(r[0].ok())",
         format!("fn solve<T>(p: [T]) -> [T] {{\n    {}\n}}\n", u8_or("[v]", "p"))),
        ("Option", "match solve(Some(3)) {\n        Some(v) => assert(v.ok())\n        None => assert(false)\n    }",
         format!("fn solve<T>(p: Option<T>) -> Option<T> {{\n    {}\n}}\n", u8_or("Some(v)", "p"))),
        ("Result", "match solve(Ok(3)) {\n        Ok(v) => assert(v.ok())\n        Err(e) => assert(false)\n    }",
         format!("fn solve<T>(p: Result<T, str>) -> Result<T, str> {{\n    {}\n}}\n", u8_or("Ok(v)", "p"))),
        ("Option<Wrap<T>>", "match solve(Some(Wrap { v: 3 })) {\n        Some(w) => assert(w.v.ok())\n        None => assert(false)\n    }",
         format!("{wrap}fn solve<T>(o: Option<Wrap<T>>) -> Option<Wrap<T>> {{\n    {}\n}}\n", u8_or("Some(Wrap { v: v })", "o"))),
        ("[Wrap<T>]", "let r = solve([Wrap { v: 3 }])\n    assert(r[0].v.ok())",
         format!("{wrap}fn solve<T>(o: [Wrap<T>]) -> [Wrap<T>] {{\n    {}\n}}\n", u8_or("[Wrap { v: v }]", "o"))),
        ("(Wrap<T>, i64)", "let r = solve((Wrap { v: 3 }, 1))\n    assert(r.0.v.ok())",
         format!("{wrap}fn solve<T>(p: (Wrap<T>, i64)) -> (Wrap<T>, i64) {{\n    {}\n}}\n", u8_or("(Wrap { v: v }, 1)", "p"))),
        ("Wrap<T> -> T", "let r = solve(Wrap { v: 3 })\n    assert(r.ok())",
         format!("{wrap}fn solve<T>(w: Wrap<T>) -> T {{\n    {}\n}}\n", u8_or("v", "w.v"))),
        ("two parameters", "let w = solve(P { a: 3, b: 3 })\n    assert(w.a.ok())",
         format!("type P<A, B> = {{ a: A, b: B }}\nfn solve<A, B>(w: P<A, B>) -> P<A, B> {{\n    {}\n}}\n", u8_or("P { a: v, b: w.b }", "w"))),
        ("a returned closure", "let f = solve(|x| x)\n    assert(f(3).ok())",
         format!("fn solve<T>(f: fn(T) -> T) -> fn(T) -> T {{\n    {}\n}}\n", u8_or("|y| v", "f"))),
        ("a generic impl's method", "let b = mk(3)\n    assert(b.get().ok())",
         format!("{wrap}fn mk(n: i64) -> Wrap<i64> {{ Wrap {{ v: n }} }}\ntrait Get {{\n    fn get(self) -> i64\n}}\nimpl Get for Wrap<i64> {{\n    fn get(self: Wrap<i64>) -> i64 {{\n        {}\n    }}\n}}\n", u8_or("v", "self.v"))),
    ];
    for (why, body, cand) in cases {
        let suite = format!("@[test]\nfn t() {{\n    {body}\n}}\n");
        let out = judged8("r4c-shapes", &suite, &cand);
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a u8 filled the operator's i64 inside {why}: {out:?}"
        );
    }
}

/// The reviewer's remaining attack: the candidate overwrites a key the
/// operator held with a laundered `u8`, and the operator's untyped
/// `dict_get(..).ok()` ran the lenient `u8` impl. The operator's dict is
/// SNAPSHOTTED when it is handed over and verified at every edge back.
#[test]
fn sealed_code_cannot_retype_a_dict_entry_the_operator_held() {
    let suite = dict_suite("solve(d)");
    live8(
        "r4c-dict",
        &suite,
        "fn solve(d: Dict) { dict_set(d, \"a\", 9) }\n",
        "fn solve(d: Dict) { dict_set(d, \"a\", 4) }\n",
    );
    dict_refused(
        "the review's overwrite",
        &suite,
        &format!(
            "fn solve(d: Dict) {{\n    {}\n}}\n",
            put_u8("dict_set(d, \"a\", v)")
        ),
    );
    dict_refused(
        "remove, then add the key back",
        &suite,
        &format!(
            "fn solve(d: Dict) {{\n    {}\n}}\n",
            put_u8("dict_remove(d, \"a\")\n        dict_set(d, \"a\", v)")
        ),
    );
    dict_refused(
        "through an alias in an array",
        &suite,
        &format!(
            "fn solve(d: Dict) {{\n    let held = [d]\n    {}\n}}\n",
            put_u8("dict_set(held[0], \"a\", v)")
        ),
    );
    // A dict nested in the one handed over.
    dict_refused(
        "a dict nested in the handed dict",
        "@[test]\nfn t() {\n    let d = dict_new()\n    let inner = dict_new()\n    dict_set(inner, \"a\", 3)\n    dict_set(d, \"in\", inner)\n    solve(d)\n    match dict_get(inner, \"a\") {\n        Some(x) => assert(x.ok())\n        None => assert(false)\n    }\n}\n",
        &format!(
            "fn solve(d: Dict) {{\n    match dict_get(d, \"in\") {{\n        Some(i) => {}\n        None => {{}}\n    }}\n}}\n",
            put_u8("dict_set(i, \"a\", v)")
        ),
    );
    // Dict reached through generic positions.
    let wrap = "type Wrap<T> = { v: T }\n";
    dict_refused(
        "Wrap<Dict>",
        &dict_suite("solve(Wrap { v: d })"),
        &format!(
            "{wrap}fn solve(w: Wrap<Dict>) {{\n    {}\n}}\n",
            put_u8("dict_set(w.v, \"a\", v)")
        ),
    );
    dict_refused(
        "Option<Dict>",
        &dict_suite("solve(Some(d))"),
        &format!(
            "fn solve(o: Option<Dict>) {{\n    match o {{\n        Some(x) => {}\n        None => {{}}\n    }}\n}}\n",
            put_u8("dict_set(x, \"a\", v)")
        ),
    );
    dict_refused(
        "[Dict]",
        &dict_suite("solve([d])"),
        &format!(
            "fn solve(xs: [Dict]) {{\n    {}\n}}\n",
            put_u8("dict_set(xs[0], \"a\", v)")
        ),
    );
    dict_refused(
        "a generic fn's T bound to the dict",
        &dict_suite("solve(d)"),
        &format!(
            "fn solve<T>(x: T) {{\n    {}\n}}\nfn put(d: Dict, v: u8) {{ dict_set(d, \"a\", v) }}\n",
            u8_or("{ put(x, v) }", "{}")
        ),
    );
}

/// Round 5 BLOCKER (amendment 78): the candidate REPLACES a position the
/// operator held with a candidate-built value carrying a retyped element.
/// A held dict's recorded type is just `Dict`, so the replacement used to
/// pass and its nested values were never cast. A replacement is judged by
/// what the operator held at that position, deeply — through nested
/// dicts, arrays, structs, `Option`s — and a key only the replacement has
/// is free (exactly as part 2's new keys).
#[test]
fn a_position_the_operator_held_is_judged_by_what_it_held_when_replaced() {
    // The operator's `d` holds `inner = {x: 3}` at "inner"; it reads
    // d.inner.x untyped afterwards.
    let nested = |held: &str, read: &str, call: &str| {
        format!("@[test]\nfn t() {{\n    let inner = dict_new()\n    dict_set(inner, \"x\", 3)\n    let d = dict_new()\n    dict_set(d, \"inner\", {held})\n    {call}\n    match dict_get(d, \"inner\") {{\n        Some(h) => {read}\n        None => assert(false)\n    }}\n}}\n")
    };
    let wrap = "type Wrap<T> = { v: T }\n";
    let build = |x: &str| {
        format!("let n = dict_new()\n    dict_set(n, \"x\", {x})\n    dict_set(d, \"inner\", n)")
    };
    let read_dict = "match dict_get(h, \"x\") {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }";
    let suite = nested("inner", read_dict, "solve(d)");
    live8(
        "r4c-repl",
        &suite,
        &format!("fn solve(d: Dict) {{\n    {}\n}}\n", build("9")),
        &format!("fn solve(d: Dict) {{\n    {}\n}}\n", build("4")),
    );
    dict_refused(
        "the review's candidate-built dict at the held key",
        &suite,
        &format!(
            "fn solve(d: Dict) {{\n    {}\n}}\n",
            put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", n)")
        ),
    );
    // A key only the replacement has is free; the key both have is checked.
    honest8(
        "r4c-repl",
        &suite,
        "fn solve(d: Dict) {\n    let n = dict_new()\n    dict_set(n, \"x\", 9)\n    dict_set(n, \"y\", narrow(4))\n    dict_set(d, \"inner\", n)\n}\n",
    );
    // Two levels down.
    dict_refused(
        "a dict nested in the replacing dict",
        "@[test]\nfn t() {\n    let leaf = dict_new()\n    dict_set(leaf, \"x\", 3)\n    let mid = dict_new()\n    dict_set(mid, \"leaf\", leaf)\n    let d = dict_new()\n    dict_set(d, \"mid\", mid)\n    solve(d)\n    match dict_get(d, \"mid\") {\n        Some(m) => match dict_get(m, \"leaf\") {\n            Some(l) => match dict_get(l, \"x\") {\n                Some(v) => assert(v.ok())\n                None => assert(false)\n            }\n            None => assert(false)\n        }\n        None => assert(false)\n    }\n}\n",
        &format!(
            "fn solve(d: Dict) {{\n    {}\n}}\n",
            put_u8("let leaf = dict_new()\n        dict_set(leaf, \"x\", v)\n        let mid = dict_new()\n        dict_set(mid, \"leaf\", leaf)\n        dict_set(d, \"mid\", mid)")
        ),
    );
    // The family: an array, a struct, an Option or a tuple carrying a dict,
    // and an array or struct carrying scalars, replaced by the candidate's.
    let repl = |held: &str, read: &str, cand: String| {
        dict_refused(held, &nested(held, read, "solve(d)"), &cand);
    };
    repl(
        "[inner]",
        "match dict_get(h[0], \"x\") {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }",
        format!(
            "fn solve(d: Dict) {{\n    {}\n}}\n",
            put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", [n])")
        ),
    );
    repl(
        "Some(inner)",
        "match h {\n            Some(i) => match dict_get(i, \"x\") {\n                Some(v) => assert(v.ok())\n                None => assert(false)\n            }\n            None => assert(false)\n        }",
        format!(
            "fn solve(d: Dict) {{\n    {}\n}}\n",
            put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", Some(n))")
        ),
    );
    repl(
        "(inner, 1)",
        "match dict_get(h.0, \"x\") {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }",
        format!(
            "fn solve(d: Dict) {{\n    {}\n}}\n",
            put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", (n, 1))")
        ),
    );
    let wsuite = format!(
        "{wrap}{}",
        nested(
            "Wrap { v: inner }",
            "match dict_get(h.v, \"x\") {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }",
            "solve(d)"
        )
    );
    dict_refused(
        "Wrap { v: inner }",
        &wsuite,
        &format!(
            "fn solve(d: Dict) {{\n    {}\n}}\n",
            put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", Wrap { v: n })")
        ),
    );
    honest8(
        "r4c-repl",
        &wsuite,
        "fn solve(d: Dict) {\n    let n = dict_new()\n    dict_set(n, \"x\", 9)\n    dict_set(d, \"inner\", Wrap { v: n })\n}\n",
    );
    // Scalars in containers: the leaf rule.
    for (held, read, newv) in [
        ("[3]", "assert(h[0].ok())", "[v]"),
        ("Wrap { v: 3 }", "assert(h.v.ok())", "Wrap { v: v }"),
        ("(3, 1)", "assert(h.0.ok())", "(v, 1)"),
        ("Some(3)", "match h {\n            Some(x) => assert(x.ok())\n            None => assert(false)\n        }", "Some(v)"),
    ] {
        let suite = format!("{wrap}{}", nested(held, read, "solve(d)"));
        dict_refused(
            held,
            &suite,
            &format!(
                "{wrap}fn solve(d: Dict) {{\n    {}\n}}\n",
                put_u8(&format!("dict_set(d, \"inner\", {newv})"))
            ),
        );
    }
    // Over a channel: the dict the candidate received.
    dict_refused(
        "a held dict received on the operator's channel",
        &format!(
            "@[test]\nfn t() {{\n    let inner = dict_new()\n    dict_set(inner, \"x\", 3)\n    let d = dict_new()\n    dict_set(d, \"inner\", inner)\n    let c = chan<Dict>()\n    c.send(d)\n    solve(c)\n    match dict_get(d, \"inner\") {{\n        Some(h) => {read_dict}\n        None => assert(false)\n    }}\n}}\n"
        ),
        &format!(
            "fn solve(c: Chan<Dict>) {{\n    let d = c.recv()\n    {}\n}}\n",
            put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", n)")
        ),
    );
}

/// Round 5 MAJOR-ADJACENT (amendment 78): a position the operator held at
/// an UNDETERMINED type (`None`, an empty array) is refused to the
/// candidate — a strict store, as at a strict crossing — instead of left
/// free. An operator that wants the candidate to fill a slot holds a
/// typed placeholder (`Some(0)`, `[0]`).
#[test]
fn a_placeholder_the_operator_held_is_not_filled_by_the_candidate() {
    let suite = |held: &str, read: &str| {
        format!("@[test]\nfn t() {{\n    let d = dict_new()\n    dict_set(d, \"best\", {held})\n    solve(d)\n    match dict_get(d, \"best\") {{\n        Some(o) => {read}\n        None => assert(false)\n    }}\n}}\n")
    };
    let some = "match o {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }";
    let some_suite = suite("None", some);
    let fill = |x: &str| format!("fn solve(d: Dict) {{ dict_set(d, \"best\", Some({x})) }}\n");
    // A typed placeholder is filled by the candidate, and judged.
    live8("r4c-hold", &suite("Some(0)", some), &fill("9"), &fill("4"));
    dict_refused(
        "None filled with a u8",
        &some_suite,
        &format!(
            "fn solve(d: Dict) {{\n    {}\n}}\n",
            put_u8("dict_set(d, \"best\", Some(v))")
        ),
    );
    dict_refused(
        "None filled with an i64 (no position was determined)",
        &some_suite,
        &fill("9"),
    );
    dict_refused(
        "an empty array filled",
        &suite("[]", "assert(o[0].ok())"),
        &format!(
            "fn solve(d: Dict) {{\n    {}\n}}\n",
            put_u8("dict_set(d, \"best\", [v])")
        ),
    );
    // Leaving the placeholder alone, or emptying a typed slot, is fine.
    honest8(
        "r4c-hold",
        "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"best\", None)\n    dict_set(d, \"n\", 1)\n    solve(d)\n    assert(true)\n}\n",
        "fn solve(d: Dict) { dict_set(d, \"n\", 2) }\n",
    );
    honest8(
        "r4c-hold",
        "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"best\", Some(3))\n    solve(d)\n    assert(true)\n}\n",
        "fn solve(d: Dict) { dict_set(d, \"best\", None) }\n",
    );
}

/// The same edge judges a replacement by what the operator HELD there (the
/// declared type alone admits it: `[i64 | u8]` admits a `u8`; a `[Dict]`
/// admits a fresh dict whose key was retyped).
#[test]
fn a_mut_write_through_value_is_judged_by_what_the_operator_held() {
    let dispatch = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    assert(a[0].ok())\n}\n";
    honest8(
        "r8-mutheld",
        dispatch,
        "fn fill(a: &mut [i64 | u8]) { a[0] = 9 }\n",
    );
    mut_refused(
        "a union element admitted the u8",
        &judged8(
            "r8-mutheld",
            dispatch,
            &mut_fill("&mut [i64 | u8]", "a[0] = v"),
        ),
        "did not allow",
    );
    let dicts = "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"k\", 3)\n    let mut a = [d]\n    fill(&mut a)\n    match dict_get(a[0], \"k\") {\n        Some(x) => assert(x.ok())\n        None => assert(false)\n    }\n}\n";
    live8(
        "r8-mutheld",
        dicts,
        "fn fill(a: &mut [Dict]) {\n    let n = dict_new()\n    dict_set(n, \"k\", 9)\n    a[0] = n\n}\n",
        "fn fill(a: &mut [Dict]) {\n    let n = dict_new()\n    dict_set(n, \"k\", 4)\n    a[0] = n\n}\n",
    );
    mut_refused(
        "a fresh dict replaced the held one with a retyped key",
        &judged8(
            "r8-mutheld",
            dicts,
            &mut_fill(
                "&mut [Dict]",
                "let n = dict_new()\n        dict_set(n, \"k\", v)\n        a[0] = n",
            ),
        ),
        "did not allow",
    );
    // The same replacement with the dispatch rule ON, and an in-place retype of
    // the dict an array held (the snapshot edge), rule off and on.
    let repl = mut_fill(
        "&mut [Dict]",
        "let n = dict_new()\n        dict_set(n, \"k\", v)\n        a[0] = n",
    );
    let out = judged_on("r8-mutheld", dicts, &repl);
    assert!(
        out != Ok(TestEnd::Completed),
        "ATTACK: a replaced `&mut [Dict]` element passed with the rule on: {out:?}"
    );
    let inplace = mut_fill("&mut [Dict]", "dict_set(a[0], \"k\", v)");
    mut_refused(
        "an in-place retype of the held dict",
        &judged8("r8-mutheld", dicts, &inplace),
        "retyped a dict entry",
    );
    let out = judged_on("r8-mutheld", dicts, &inplace);
    assert!(
        out != Ok(TestEnd::Completed),
        "ATTACK: an in-place retype passed with the rule on: {out:?}"
    );
}

/// An ANNOTATED operator array is determined by its annotation, so the
/// dispatch rule cannot help: only the cast at the edge back refuses.
#[test]
fn an_annotated_operator_array_lent_as_mut_is_still_cast_at_the_edge_back() {
    let suite = "@[test]\nfn t() {\n    let mut a: [i64] = [3]\n    fill(&mut a)\n    let y: i64 = a[0]\n    assert(y.ok())\n}\n";
    live8(
        "r8-mutann",
        suite,
        "fn fill(a: &mut [i64]) { a[0] = 9 }\n",
        "fn fill(a: &mut [i64]) { a[0] = 4 }\n",
    );
    mut_refused(
        "the annotated array was left holding a u8",
        &judged8("r8-mutann", suite, &mut_fill("&mut [i64]", "a[0] = v")),
        "left holding",
    );
}

/// A sealed frame that queues an operator function as a fiber never gets
/// it run. `scheduler_spawn` seal-checks the name when it is queued; were
/// that check absent, the fiber would still never run an operator
/// function, on EITHER route to its execution:
/// * run from a sealed frame: every fiber is called through `call_fn`,
///   whose call edge refuses an operator function in a sealed frame;
/// * run by the operator: it cannot be — a fiber lives in the kernel of
///   the frame that queued it, and a sealed frame's kernel is its own.
///
/// So this accepts any refusal, and fails "ATTACK:" only when an operator
/// function actually ran. (Moved here from
/// `runtime_sealing_holds_without_the_static_check`, whose two fiber cases
/// asserted the queue-time refusal itself — a refusal reason on routes the
/// call edge and the per-provenance kernel each dominate.)
#[test]
fn a_sealed_fiber_never_runs_an_operator_function() {
    use crate::span::intern_source;
    let suite = "fn expected(n: i64) -> i64 { n * 2 }\n\
                 @[test]\nfn t() { assert_eq(double(21), expected(21)) }\n\
                 @[test]\nfn t_later() {\n    let d = double(21)\n    scheduler_run()\n    assert(d == 42 && scheduler_done_count() > 0)\n}\n\
                 @[test]\nfn t_later_control() {\n    let _ = scheduler_spawn(\"expected\", 1)\n    scheduler_run()\n    assert(scheduler_done_count() > 0)\n}\n";
    let run = |cand: &str, test: &str| {
        let s = crate::parse_source_in(suite, intern_source("/pci-fb-suite/h.ax", suite))
            .expect("suite parses");
        let c = crate::parse_source_in(cand, intern_source("/pci-fb-sealed/f.ax", cand))
            .expect("candidate parses");
        let prog = Program {
            items: s.items.into_iter().chain(c.items).collect(),
        };
        // R51: v0.22 published the sealed set process-wide under
        // SEALED_DIRS_TEST_LOCK; on main it is handed to this run's build only
        // (as `sealed_outcome` does), so no lock is needed.
        on_deep_stack(|| {
            run_test_fn_inner(&prog, test, &[std::path::PathBuf::from("/pci-fb-sealed")])
        })
    };
    // Controls: a sealed frame may queue and run its OWN function as a
    // fiber and read the result, and the operator's scheduler runs what
    // the OPERATOR queues — so each attack below fails on the seal, not
    // on the scheduler.
    let honest = "fn double(n: i64) -> i64 { n * 2 }\n";
    let own = "fn twice(n: i64) -> i64 { n * 2 }\n\
               fn double(n: i64) -> i64 {\n    let id = scheduler_spawn(\"twice\", n)\n    scheduler_run()\n    scheduler_result(id)\n}\n";
    assert_eq!(run(own, "t"), Ok(TestEnd::Completed), "control: own fiber");
    assert_eq!(
        run(honest, "t_later_control"),
        Ok(TestEnd::Completed),
        "control: the operator's scheduler runs the operator's fiber"
    );
    // Run from the sealed frame, returning the operator's answer.
    let now = "fn double(n: i64) -> i64 {\n    let id = scheduler_spawn(\"expected\", n)\n    scheduler_run()\n    scheduler_result(id)\n}\n";
    let out = run(now, "t");
    assert!(
        out != Ok(TestEnd::Completed),
        "ATTACK: a sealed frame ran the operator's `expected` as a fiber and returned its answer: {out:?}"
    );
    // Queued for the operator's scheduler to run later.
    let later =
        "fn double(n: i64) -> i64 {\n    let _ = scheduler_spawn(\"expected\", n)\n    n * 2\n}\n";
    let out = run(later, "t_later");
    assert!(
        out != Ok(TestEnd::Completed),
        "ATTACK: the operator's scheduler ran an operator function a sealed frame queued: {out:?}"
    );
}

/// C9 round 3, PSV-1: a SEALED handler frame never answers or aborts an
/// operation performed under OPERATOR provenance. The review executed two
/// variants to a keyed pass: the candidate's `on IO` arm ABORTED the
/// operator's listener at its first `println` (skipping its assert), and
/// RESUMED the operator's `read_file` with bytes the candidate chose. A
/// third route has the same root: the operator's listener calls a
/// CANDIDATE function whose `println` the candidate's outer handler
/// aborts, unwinding through the operator's code. A fourth goes through
/// the multi-shot replay feed instead of a live frame.
#[test]
fn a_sealed_handler_never_answers_or_aborts_operator_code() {
    let dir = std::env::temp_dir().join(format!("psv1-handler-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let want = dir.join("expected.txt");
    std::fs::write(&want, "42\n").unwrap();
    let want = want.display().to_string();
    let suite = format!(
        "fn want() -> i64 {{\n    let raw = match read_file(\"{want}\") {{ Ok(s) => s  Err(e) => \"\" }}\n    match parse_int(str_trim(raw)) {{ Ok(n) => n  Err(e) => -1 }}\n}}\n\
         @[test]\nfn t_abort() {{ visit(|v: i64| {{\n    println(\"listener saw {{to_str(v)}}\")\n    assert_eq(v, 42)\n}}) }}\n\
         @[test]\nfn t_resume() {{ visit(|v: i64| {{ assert_eq(v, want()) }}) }}\n\
         @[test]\nfn t_through() {{ visit(|v: i64| {{ assert_eq(noisy(v), 42) }}) }}\n\
         @[test]\nfn t_replay() {{ visit(|v: i64| {{ assert_eq(v, want()) }}) }}\n\
         @[test]\nfn t_own() {{ assert_eq(mine(), 5) }}\n\
         @[test]\nfn t_op() {{\n    let v = with handler {{ on IO(p) => resume(Ok(\"42\")) }} {{\n        match read_file(\"/nonexistent/psv1\") {{ Ok(s) => s  Err(e) => \"no\" }}\n    }}\n    assert(v == \"42\")\n}}\n\
         @[test]\nfn t_op_around() {{\n    with handler {{ on IO(p) => resume(Ok(\"7\")) }} {{\n        visit(|v: i64| {{ assert_eq(v, want()) }})\n    }}\n}}\n"
    );
    let honest = "fn noisy(v: i64) -> i64 { println(\"n\")  v }\n\
                  fn mine() -> i64 {\n    let a = with handler { on IO(p) => resume(Ok(\"5\")) } {\n        match read_file(\"/nonexistent/own\") { Ok(s) => s  Err(e) => \"0\" }\n    }\n    let b = with handler { on IO(p) => 5 } {\n        println(\"x\")\n        1\n    }\n    match parse_int(a) { Ok(n) => if n == b { n } else { 0 }  Err(e) => 0 }\n}\n";
    let good = format!("{honest}fn visit(cb: fn(i64) -> ()) {{ cb(42) }}\n");
    let wrong7 = format!("{honest}fn visit(cb: fn(i64) -> ()) {{ cb(7) }}\n");
    // Controls: the honest candidate passes each shape and a wrong one
    // fails it, so each check below is live; the candidate's OWN handler
    // still handles the candidate's own operations (resume and abort);
    // an operator handler still handles the operator's operation, also
    // when candidate frames lie between it and the operation.
    for t in ["t_abort", "t_resume", "t_through", "t_own", "t_op"] {
        assert_eq!(
            sealed_outcome("psv1h", &suite, &good, t),
            Ok(TestEnd::Completed),
            "control: {t}"
        );
    }
    for t in ["t_abort", "t_resume", "t_through"] {
        assert!(
            sealed_outcome("psv1h", &suite, &wrong7, t).is_err(),
            "control: wrong fails {t}"
        );
    }
    assert_eq!(
        sealed_outcome("psv1h", &suite, &wrong7, "t_op_around").map_err(|_| ()),
        Ok(TestEnd::Completed),
        "control: the operator's own handler answers the operator's read_file"
    );
    let attacks = [
        (
            "t_abort",
            "the candidate's arm aborted the operator's listener at its println",
            "fn visit(cb: fn(i64) -> ()) {\n    let _ = with handler { on IO(p) => 0 } {\n        cb(7)\n        0\n    }\n}\n",
        ),
        (
            "t_resume",
            "the candidate's arm resumed the operator's read_file with its own bytes",
            "fn visit(cb: fn(i64) -> ()) {\n    with handler { on IO(p) => resume(Ok(\"7\")) } {\n        cb(7)\n    }\n}\n",
        ),
        (
            "t_through",
            "the candidate's arm aborted the operator's listener through a candidate call",
            "fn visit(cb: fn(i64) -> ()) {\n    let _ = with handler { on IO(p) => 0 } {\n        cb(7)\n        0\n    }\n}\n",
        ),
        (
            "t_replay",
            "the candidate's multi-shot replay fed the operator's read_file",
            "fn visit(cb: fn(i64) -> ()) {\n    let st = dict_new()\n    let _ = with handler { on IO(p) => {\n        dict_set(st, \"replaying\", 1)\n        resume(Ok(\"7\"))\n    } } {\n        if !dict_has(st, \"replaying\") { println(\"mine\") }\n        cb(7)\n        0\n    }\n}\n",
        ),
    ];
    for (t, what, visit) in attacks {
        let cand = format!("{honest}{visit}");
        let out = sealed_outcome("psv1h", &suite, &cand, t);
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: sealed handler answered operator code: {what} ({t}): {out:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// C9 round 3, PSV-1 (the RNG route of the same root): operator code a
/// sealed frame runs — here a closure the suite hands the candidate —
/// cannot draw from the operator's stream, because the CANDIDATE chooses
/// how many times it runs and so how far the stream advances. The attack:
/// the candidate is right only at the challenge the operator draws after
/// FIVE ticks, and ticks five times. Refused, not given another stream:
/// amendment 46.
#[test]
fn operator_code_a_sealed_frame_runs_never_draws_the_operator_rng() {
    // `srand(7)`: the operator's stream is 327, 652, 743, 107, 850, 125 …
    let suite = "@[test]\nfn t_accept() {\n    srand(7)\n    warm(|| { let _ = random_i64(0, 1000) })\n    let c = random_i64(0, 1000)\n    assert(solve(c))\n}\n\
                 @[test]\nfn t_seeded() {\n    srand(7)\n    warm(|| { let _ = random_i64(0, 1000) })\n    assert_eq(random_i64(0, 1000), 327)\n}\n\
                 @[test]\nfn t_reseed() {\n    srand(7)\n    warm(|| { srand(1) })\n    assert(solve(random_i64(0, 1000)))\n}\n";
    let right_at_125 = "fn solve(c: i64) -> bool { c == 125 }\n";
    // Controls: with no tick the operator's own draws work and follow
    // the seed; the candidate draws freely from its OWN stream.
    let idle = format!("{right_at_125}fn warm(tick: fn() -> ()) {{ let _ = random_i64(0, 9) }}\n");
    assert_eq!(
        sealed_outcome("psv1r", suite, &idle, "t_seeded"),
        Ok(TestEnd::Completed),
        "control"
    );
    assert!(
        sealed_outcome("psv1r", suite, &idle, "t_accept").is_err(),
        "control: 327 is wrong"
    );
    let steer = format!(
        "{right_at_125}fn warm(tick: fn() -> ()) {{ tick() tick() tick() tick() tick() }}\n"
    );
    let out = sealed_outcome("psv1r", suite, &steer, "t_accept");
    assert!(
        out != Ok(TestEnd::Completed),
        "ATTACK: the candidate steered the operator's challenge by calling its closure five times: {out:?}"
    );
    assert!(
        matches!(&out, Err(m) if m.contains("operator's random stream")),
        "the refusal names the rule: {out:?}"
    );
    let reseed = "fn solve(c: i64) -> bool { c == 0 }\nfn warm(tick: fn() -> ()) { tick() }\n";
    assert!(
        sealed_outcome("psv1r", suite, reseed, "t_reseed").is_err(),
        "operator code a sealed frame runs cannot reseed the operator stream either"
    );
}

/// C9 round 9, PSV-1: `sandbox_run` declares `-> i64`, but the candidate fn
/// it runs only had its OWN declared return applied (`-> u8` is satisfied by
/// `4 as u8`), and the pin analysis treats the builtin's closed result as
/// determined: `let r = sandbox_run(..)` then `r.ok()` dispatched the lenient
/// `u8` impl, and `(r << 1) == 254` wrapped at `u8`. The result is now cast
/// to the declared type at the seal crossing.
#[test]
fn sandbox_run_results_are_cast_at_the_seal_crossing() {
    let run = |suite: &str, cand: &str| judged_on("r9-sbx", suite, cand);
    let disp = sandbox_suite("    let r = sandbox_run(sb, \"work\", 0)\n    assert(r.ok())");
    // Controls: the honest candidate passes, a wrong i64 fails (the check is live).
    assert_eq!(
        run(&disp, "fn work(x: i64) -> i64 { 9 }\n"),
        Ok(TestEnd::Completed),
        "control: honest"
    );
    assert!(
        run(&disp, "fn work(x: i64) -> i64 { 4 }\n").is_err(),
        "control: wrong fails"
    );
    let unit = sandbox_suite("    let r = sandbox_run(sb, \"work\", 0)\n    assert_eq(r, 0)");
    assert_eq!(
        run(&unit, "fn work(x: i64) { }\n"),
        Ok(TestEnd::Completed),
        "control: a unit-returning callee is 0"
    );
    // The operator's OWN fn is not the candidate's value: it is not cast.
    let own = format!(
        "fn opwork(x: i64) -> i64 {{ 9 }}\n{}",
        sandbox_suite("    let r = sandbox_run(sb, \"opwork\", 0)\n    assert(r.ok())")
    );
    assert_eq!(
        run(&own, "fn solve() -> i64 { 1 }\n"),
        Ok(TestEnd::Completed),
        "control: operator callee"
    );
    // The attacks.
    let width =
        sandbox_suite("    let r = sandbox_run(sb, \"work\", 0)\n    assert((r << 1) == 254)");
    let pinned = sandbox_suite("    let r: i64 = sandbox_run(sb, \"work\", 0)\n    assert(r.ok())");
    for (what, suite, cand) in [
        (
            "dispatch on a u8 result",
            &disp,
            "fn work(x: i64) -> u8 { narrow(4) }\n",
        ),
        (
            "a u8 result wrapped by the operator's shift",
            &width,
            "fn work(x: i64) -> u8 { 255 as u8 }\n",
        ),
        (
            "a u8 result under a pinned binding",
            &pinned,
            "fn work(x: i64) -> u8 { narrow(4) }\n",
        ),
        ("a str result", &disp, "fn work(x: i64) -> str { \"a\" }\n"),
    ] {
        let out = run(suite, cand);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("sandbox_run") || m.contains("declared")),
            "ATTACK: sandbox_run handed the operator a value the candidate chose the type of ({what}): {out:?}"
        );
    }
}

/// C9 round 9, SENTINEL: the `Expr::FieldAccess` and `Expr::Index` arms read
/// `self.globals` directly for an identifier receiver and never applied the
/// global-read edge: `TABLE[0]`, `CFG.k`, `PAIR.0`, `NN[0][0]` and
/// `|| TABLE[1]` all completed for sealed code reading an operator global.
/// Every lookup now goes through `Interp::global_ref`.
#[test]
fn a_sealed_frame_cannot_read_an_operator_global_through_a_fast_path() {
    let suite = "type Cfg = { k: i64 }\nlet TABLE = [9, 8]\nlet CFG = Cfg { k: 9 }\nlet PAIR = (9, 4)\nlet NN = [[9]]\nlet FN = || 9\n\
                 @[test]\nfn t() { assert_eq(solve(), 9) }\n";
    let run = |cand: &str| sealed_outcome_rule("r9-glob", suite, cand, "t", true);
    // Controls: the candidate's OWN globals read through the same arms; a
    // wrong one fails; the operator's whole-value read stays refused.
    let own = "type Mc = { k: i64 }\nlet MT = [9, 8]\nlet MC = Mc { k: 9 }\nlet MP = (9, 4)\nlet MN = [[9]]\nlet MF = || 9\n";
    for (what, body) in [
        ("index", "MT[0]"),
        ("field", "MC.k"),
        ("tuple", "MP.0"),
        ("nested", "MN[0][0]"),
        ("closure const", "MF()"),
        ("lambda", "(|| MT[0])()"),
    ] {
        assert_eq!(
            run(&format!("{own}fn solve() -> i64 {{ {body} }}\n")),
            Ok(TestEnd::Completed),
            "control: the candidate's own global ({what})"
        );
    }
    assert!(
        run(&format!("{}fn solve() -> i64 {{ MT[1] }}\n", own)).is_err(),
        "control: wrong fails"
    );
    for (what, body) in [
        ("a whole read", "let t = TABLE\n    t[0]"),
        ("an index", "TABLE[0]"),
        ("a field", "CFG.k"),
        ("a tuple field", "PAIR.0"),
        ("a nested index", "NN[0][0]"),
        ("an index inside a lambda", "(|| TABLE[0])()"),
        ("a closure constant call", "FN()"),
    ] {
        let out = run(&format!("fn solve() -> i64 {{\n    {body}\n}}\n"));
        assert!(
            out != Ok(TestEnd::Completed) && matches!(&out, Err(m) if m.contains("cannot use")),
            "ATTACK: a sealed frame read an operator global ({what}): {out:?}"
        );
    }
}

/// C9 round 9 sweep, CLOSED routes: each carries a value the candidate
/// produced into operator code, and none lets the candidate choose the type
/// the operator's dispatch or arithmetic runs at. A `Refused` route fails
/// with the guard that closes it; a `Fresh` one hands over a value the
/// builtin builds itself (so `.ok()` lands on the `i64` impl).
#[test]
fn the_sweep_routes_stay_closed() {
    let t = |body: &str| format!("@[test]\nfn t() {{\n{body}\n}}\n");
    let refused = |what: &str, suite: String, cand: &str, needle: &str| {
        let out = judged_on("r9-sweep", &suite, cand);
        assert!(
            out != Ok(TestEnd::Completed) && matches!(&out, Err(m) if m.contains(needle)),
            "ATTACK: {what} reached the operator as a type the candidate chose: {out:?}"
        );
    };
    let nd = "nothing on the operator side determined";
    let tc = "type confusion";
    let u8w = "fn work(x: i64) -> u8 { narrow(4) }\n";
    refused(
        "an arr_map result",
        t("    let xs = arr_map([1, 2], work)\n    assert(xs[0].ok())"),
        u8w,
        nd,
    );
    refused(
        "an arr_map result (pinned)",
        t("    let xs = arr_map([1, 2], work)\n    let y: i64 = xs[0]\n    assert(y.ok())"),
        u8w,
        tc,
    );
    refused(
        "an arr_fold result",
        t("    let r = arr_fold([1], 0, work)\n    assert(r.ok())"),
        "fn work(a: i64, x: i64) -> u8 { narrow(4) }\n",
        nd,
    );
    refused(
        "an arr_zip_with result",
        t("    let r = arr_zip_with([1], [2], work)\n    assert(r[0].ok())"),
        "fn work(a: i64, x: i64) -> u8 { narrow(4) }\n",
        nd,
    );
    refused(
        "an arr_max_by element",
        t("    let r = arr_max_by([1, 2], work)\n    assert(r.ok())"),
        "fn work(a: i64) -> f64 { 1.0 }\n",
        nd,
    );
    refused("a dict_map_values entry", t("    let d = dict_new()\n    dict_set(d, \"a\", 1)\n    let m = dict_map_values(d, work)\n    match dict_get(m, \"a\") { Some(v) => assert(v.ok())  None => assert(false) }"), u8w, nd);
    refused(
        "an arr_sort_by comparator",
        t("    let r = arr_sort_by([2, 1], work)\n    assert(r[0].ok())"),
        "fn work(a: i64, b: i64) -> u8 { narrow(4) }\n",
        "comparator must return i64",
    );
    refused(
        "an arr_sum_by key",
        t("    let r = arr_sum_by([2, 1], work)\n    assert(r.ok())"),
        "fn work(a: i64) -> u8 { narrow(4) }\n",
        "must return a number",
    );
    refused("a candidate lambda handed back as a fn type", "fn run(f: fn(i64) -> i64) -> i64 { f(1) }\n@[test]\nfn t() {\n    let r = run(mk())\n    assert(r.ok())\n}\n".to_string(), "fn mk() -> fn(i64) -> i64 { |x: i64| narrow(4) }\n", tc);
    refused("a candidate channel", t("    let c = mk()\n    match c.recv() { Some(v) => assert(v.ok())  None => assert(false) }"), "fn mk() -> Chan<i64> {\n    let c = Chan::new(2)\n    c.send(narrow(4))\n    c\n}\n", tc);
    refused("a forged Uncertain", t("    let u = mk()\n    assert(u.value.ok())"), "fn mk() -> Uncertain<i64> { Uncertain { value: narrow(4), confidence: 0.5, source_tag: 0 } }\n", tc);
    refused(
        "a forged Uncertain returned as i64",
        t("    let u = mk()\n    assert(u.ok())"),
        "fn mk() -> i64 { Uncertain { value: narrow(4), confidence: 0.5, source_tag: 0 } }\n",
        tc,
    );
    refused(
        "an Option element",
        t("    match mk() { Some(v) => assert(v.ok())  None => assert(false) }"),
        "fn mk() -> Option<i64> { Some(narrow(4)) }\n",
        tc,
    );
    refused(
        "an array element",
        t("    let a = mk()\n    assert(a[0].ok())"),
        "fn mk() -> [i64] { [narrow(4)] }\n",
        tc,
    );
    refused(
        "a candidate global array",
        t("    assert(TBL[0].ok())"),
        "let TBL = [narrow(4)]\nfn mk() -> i64 { 1 }\n",
        nd,
    );
    refused(
        "a candidate global tuple",
        t("    assert(TP.0.ok())"),
        "let TP = (narrow(4), 1)\nfn mk() -> i64 { 1 }\n",
        nd,
    );
    refused(
        "a candidate global's struct field",
        t("    assert(CF.k.ok())"),
        "type Cf = { k: i64 }\nlet CF = Cf { k: narrow(4) }\nfn mk() -> i64 { 1 }\n",
        tc,
    );
    refused("a candidate impl of an operator trait method", "trait Val {\n    fn val(self) -> i64\n}\n@[test]\nfn t() {\n    let m = mk()\n    let r = m.val()\n    assert(r.ok())\n}\n".to_string(), "type Mine = { a: i64 }\nimpl Val for Mine {\n    fn val(self: Mine) -> u8 { narrow(4) }\n}\nfn mk() -> Mine { Mine { a: 1 } }\n", "a method the operator defines");
    for (what, body) in [
        ("a let copy", "    let a = xs[0]\n    assert(a.ok())"),
        ("a tuple pattern", "    let (a, b) = (xs[0], 1)\n    assert(a.ok())"),
        ("a match binding", "    match xs[0] { v => assert(v.ok()) }"),
        ("a Some payload", "    let o = Some(xs[0])\n    match o { Some(v) => assert(v.ok())  None => assert(false) }"),
        ("a lambda parameter", "    let f = |v| v.ok()\n    assert(f(xs[0]))"),
        ("an if branch", "    let a = if true { xs[0] } else { 1 }\n    assert(a.ok())"),
        ("a block tail", "    let a = { let b = xs[0]\n b }\n    assert(a.ok())"),
        ("a reassignment", "    let mut a = 1\n    a = xs[0]\n    assert(a.ok())"),
        ("an array literal", "    let ys = [xs[0], xs[0]]\n    assert(ys[1].ok())"),
        ("a for variable", "    for x in xs { assert(x.ok()) }"),
    ] {
        refused(what, t(&format!("    let xs = arr_map([1], work)\n{body}")), u8w, nd);
    }
    // FRESH: the builtin builds the value, so the i64 impl is the one reached.
    let fresh = |what: &str, suite: String, cand: &str| {
        let out = judged_on("r9-sweep", &suite, cand);
        assert_eq!(
            out,
            Ok(TestEnd::Completed),
            "{what} must hand the operator a fresh i64: {out:?}"
        );
    };
    fresh("scheduler_result", t("    let id = scheduler_spawn(\"work\", 0)\n    let n = scheduler_run()\n    let r = scheduler_result(id)\n    assert(!r.ok())"), "fn work(x: i64) -> u8 { narrow(4) }\n");
    fresh("goal_best_input", t("    let r0 = goal_run(\"work\", 100.0, 3)\n    let r = goal_best_input(\"work\", 100.0)\n    assert(!r.ok())"), "@[adaptive]\nfn work(x: i64) -> i64 { 4 }\n");
}

/// Existence oracle (C9 round 10): a sealed caller is told the same thing
/// whether the name it asked for is the operator's fn or does not exist.
///
/// R51: two ways sealed code names a fn. A name string handed to
/// `sandbox_run`, whose lookup sees only the caller's own fns (an operator fn
/// and a missing one take the same branch), and a call in the candidate's own
/// text, where an operator fn meets the call edge (`seal_call`) and a missing
/// one the unknown-callee refusal (`no_such_fn`): two refusals that must say
/// the same words.
#[test]
fn a_sealed_caller_cannot_tell_an_operator_fn_from_a_missing_one() {
    let suite =
        "fn secret(x: i64) -> i64 { 9 }\n@[test]\nfn t() {\n    assert_eq(solve(NAME), 9)\n}\n";
    let forms = [
        (
            "sandbox_run",
            "    let p = principal_root(\"r\", true, true, true, 100)\n    let sb = sandbox_create(p, \"IO\")\n    sandbox_run(sb, \"{n}\", 0)\n",
        ),
        ("call", "    {n}(x)\n"),
    ];
    for (form, body) in forms {
        let cand = |n: &str| format!("fn solve(x: i64) -> i64 {{\n{}}}\n", body.replace("{n}", n));
        let said = |n: &str| match judged_on("r10-oracle", &suite.replace("NAME", "0"), &cand(n)) {
            Err(m) => m,
            Ok(e) => panic!("{form} {n}: {e:?}"),
        };
        let (a, b) = (said("secret"), said("nonexistent_fn"));
        let norm = |m: &str, n: &str| m.replace(n, "NAME");
        assert!(
            norm(&a, "secret") == norm(&b, "nonexistent_fn"),
            "ATTACK: an operator fn and a missing one read differently to sealed code ({form}): {a} / {b}"
        );
    }
    // Honest control: the candidate's own fn, named both ways, runs.
    let own = |body: &str| {
        format!(
            "fn mine(x: i64) -> i64 {{ 9 }}\nfn solve(x: i64) -> i64 {{\n{}}}\n",
            body.replace("{n}", "mine")
        )
    };
    for (form, body) in forms {
        let r = judged_on("r10-oracle-own", &suite.replace("NAME", "0"), &own(body));
        assert!(
            matches!(r, Ok(TestEnd::Completed)),
            "control ({form}): the candidate's own fn: {r:?}"
        );
    }
}

/// Drift (amendment 100, BLOCKER A): pin verdicts are keyed (owner, site
/// text), so every place that runs STORED operator code (a fn body, a
/// closure body, a handler arm, a continuation replay, a predicate) must
/// run it under the pin owner of the code's own definition. The fns that set
/// the owner are exactly the listed ones, and every fn that evaluates a
/// stored AST body outside the core evaluator is either one of them or
/// listed with the reason its owner is already right.
///
/// R51: re-derived for main. Pins exist only in a sealed run, so the owner
/// is set on main's SEALED paths: `call_fn_sealed` (v0.22's `call_fn_frame`
/// guard; main's `call_fn_frame` is the frame both runs share) and
/// `call_closure_frame` (v0.22's `call_closure_owned_by`). The sweep reads
/// every interpreter source but `eval.rs` (the core evaluator, as in v0.22)
/// and test-only files, so `sym.rs` and `vm/*.rs` are in; it also matches
/// main's body runners (`run_body`, `run_test_body`, `run_lambda`, their
/// tree/VM twins), and the list must be exact (no stale entry).
#[test]
fn every_frame_that_runs_stored_operator_code_sets_its_owner() {
    // (fn, why it needs no guard of its own)
    const EVALUATES_STORED_CODE: &[(&str, &str)] = &[
        (
            "call_fn_frame",
            "the frame both runs share; a sealed run enters it only from call_fn_sealed, which sets the owner: the fn's own address",
        ),
        (
            "finish_call_cold",
            "the refinement postcondition, reached only from call_fn_frame, under its owner",
        ),
        (
            "call_fn_common",
            "plain entries only, and no entry of a sealed run is plain: never runs where pins exist",
        ),
        (
            "init_globals",
            "runs under owner 0, the owner the lets are analysed under",
        ),
        (
            "init_globals_sealed",
            "runs under owner 0, the owner the lets are analysed under",
        ),
        (
            "run_test_body",
            "the host-named test fn's body, run from call_fn_frame under its owner",
        ),
        (
            "run_body_tree",
            "a fn body, run from call_fn_frame / call_fn_common under the frame's owner",
        ),
        (
            "run_lambda_tree",
            "a closure body, run from call_closure_frame under the creator's owner",
        ),
        (
            "run_body_vm",
            "VM engine only; a sealed run is pinned to the tree (spec Fork 1)",
        ),
        (
            "fast_body",
            "VM fast call only; a sealed run is pinned to the tree (spec Fork 1)",
        ),
    ];
    const SETS_OWNER: &[&str] = &[
        "call_fn_sealed",
        "call_closure_frame",
        "run_arm_sealed",
        "replay_continuation",
    ];
    let is_test_file = |f: &str| f.ends_with("_tests.rs") || f.starts_with("tests_");
    let items: Vec<(String, String, String)> = fn_items()
        .into_iter()
        .filter(|(f, _, _)| !is_test_file(f))
        .map(|(f, n, b)| {
            let f = if f.is_empty() {
                "interp.rs".to_string()
            } else {
                format!("interp/{f}")
            };
            (f, n, b)
        })
        .collect();
    let mut setters: Vec<&str> = items
        .iter()
        .filter(|(_, _, b)| b.contains("PinGuard {"))
        .map(|(_, n, _)| n.as_str())
        .collect();
    setters.sort();
    let mut want: Vec<&str> = SETS_OWNER.to_vec();
    want.sort();
    assert_eq!(
        setters, want,
        "DRIFT: the set of fns that set `pin_fn` changed"
    );
    // The handler paths set it from the frame's recorded installer.
    for n in ["run_arm_sealed", "replay_continuation"] {
        let body = &items.iter().find(|(_, m, _)| m == n).unwrap().2;
        assert!(
            body.contains("pin_owner"),
            "`{n}` must run under the pin owner recorded when the handler was installed"
        );
    }
    let mut unlisted = Vec::new();
    let mut evaluators: Vec<&str> = Vec::new();
    for (file, name, body) in &items {
        if file == "interp/eval.rs" || SETS_OWNER.contains(&name.as_str()) {
            continue;
        }
        let evals_stored = body.lines().any(|l| {
            let l = l.trim_start();
            !l.starts_with("//")
                && (l.contains("self.eval(&f.body")
                    || l.contains("self.eval(&body")
                    || l.contains("self.eval(&spec.predicate")
                    || l.contains("self.eval(pred")
                    || l.contains("self.eval(expr")
                    || l.contains("self.eval(&entry.def.body")
                    || l.contains("self.eval(&code.body")
                    || l.contains("self.run_body(")
                    || l.contains("self.run_test_body(")
                    || l.contains("self.run_lambda("))
        });
        if !evals_stored {
            continue;
        }
        evaluators.push(name.as_str());
        if !EVALUATES_STORED_CODE.iter().any(|(n, _)| n == name) {
            unlisted.push(format!("{file}::{name}"));
        }
    }
    assert!(
        unlisted.is_empty(),
        "DRIFT: these fns evaluate a stored AST body without setting the pin owner and are not listed: {unlisted:?}"
    );
    // R51: the list is exact both ways - an entry the sweep no longer finds
    // is stale (its reason may no longer hold).
    let stale: Vec<&str> = EVALUATES_STORED_CODE
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !evaluators.contains(n))
        .collect();
    assert!(
        stale.is_empty(),
        "DRIFT: listed fns no longer evaluate stored code: {stale:?}"
    );
}

/// Drift (amendment 100): a lookup of an fn by NAME is either a listed
/// name-resolving SINK (the name argument is judged at the call site) or
/// listed here with the reason its name is not a value sealed code chose.
///
/// R51: re-derived for main. Main resolves a program-text callee by its
/// `Sym` (`fn_of_sym`, cached in `callees`) and a known `FnDef` by address
/// (`fn_of_def`), then indexes `fn_table`; the sweep also matches those, so
/// eval.rs's v0.22 by-name rows become `Sym` rows and the bytecode engine's
/// (`vm/`) rows are new. v0.22's four running-fn readers are one
/// `current_fn_def` on main. The split-line form also matches `.fns` on its
/// own line (`goal_eval_holdout` writes `self\n.fns\n.get(name)`).
#[test]
fn every_name_resolving_lookup_is_a_listed_sink() {
    const ALLOWED: &[(&str, &str, &str)] = &[
        ("interp/eval.rs", "let known = self.callees.borrow().get(s.index()).copied();", "dispatch_named: the callee identifier's Sym in the program text, not a value"),
        ("interp/eval.rs", "let r = match self.fn_of_sym.get(&s) {", "dispatch_named: the callee identifier's Sym in the program text"),
        ("interp/eval.rs", "let e = &self.fn_table[(resolved - CALLEE_FN) as usize];", "dispatch_named: the entry the callee identifier resolved to; seal_call ahead of a `&mut` fn's refusal, call_fn_entry applies it otherwise"),
        ("interp/eval.rs", "let entry = &self.fn_table[self.call_mut_entry(callee)? as usize];", "call_mut: the entry call_mut_entry resolved from program text"),
        ("interp/eval.rs", "Expr::Ident(name) => match self.fn_of_sym.get(&self.res.sym(callee, name)) {", "call_mut_entry: the callee identifier of a `&mut` call, in the program text"),
        ("interp/eval.rs", "} else if let Some(&i) = self.fn_of_sym.get(&s) {", "ident_unbound: an identifier in the program text, not a value"),
        ("interp/eval.rs", "let f = self.fn_table[i as usize].def;", "ident_unbound: the entry that identifier resolved to"),
        ("interp/proptest.rs", "let Some(f) = interp.fns.get(name).copied() else {", "the host's property-test name"),
        ("interp.rs", "if !interp.fns.contains_key(fn_name) {", "run_named_fn_*: the host-named deploy gate"),
        ("interp.rs", "let f: &FnDef = *interp.fns.get(fn_name)?;", "run_named_fn_*: the host-named deploy gate"),
        ("interp.rs", "if !interp.fns.contains_key(\"main\") {", "the literal `main`"),
        ("interp.rs", "let Some(f) = interp.fns.get(name).copied() else {", "run_test: the host-named test"),
        ("interp.rs", "match self.fns.get(\"main\") {", "the literal `main`"),
        ("interp.rs", "self.fns.get(self.current_fn.get()?.name.as_str()).copied()", "current_fn_def: the running fn's own name"),
        ("interp.rs", "match self.fns.get(name).copied() {", "fn_by_name: THE resolver, which applies seal_call"),
        ("interp.rs", "let defined = match self.fns.get(name) {", "goal_name_is_known: reached only from the listed goal_* sinks; a sealed caller sees only its own fns (S4b)"),
        ("interp.rs", "match self.fn_of_def.get(&(f as *const FnDef as usize)) {", "call_fn: a FnDef already resolved, by address, not a name"),
        ("interp.rs", "Some(&i) => self.call_fn_entry(&self.fn_table[i as usize], args),", "call_fn: that FnDef's entry; call_fn_entry applies seal_call"),
        ("interp.rs", "let entry = match self.fn_of_def.get(&(f as *const FnDef as usize)) {", "call_test_fn: the host-named test's FnDef, by address"),
        ("interp.rs", "Some(&i) => &self.fn_table[i as usize],", "call_test_fn: that FnDef's entry"),
        ("interp/goal.rs", "if let Some(f) = self.fns.get(name) {", "run_goal_warm: reached only from the listed goal_* sinks, behind goal_name_is_known (a sealed caller knows only its own fns)"),
        ("interp/goal.rs", "let Some(cf) = self.fns.get(cname.as_str()).copied() else {", "the constraint name, a sink (goal_run_constrained arg 2)"),
        ("interp/goal.rs", "let f = match self.fns.get(name) {", "run_goal_*: reached only from the listed goal_* sinks (4 sites)"),
        ("interp/goal.rs", "let f = match self.fns.get(name) {", "run_goal_*: reached only from the listed goal_* sinks (4 sites)"),
        ("interp/goal.rs", "let f = match self.fns.get(name) {", "run_goal_*: reached only from the listed goal_* sinks (4 sites)"),
        ("interp/goal.rs", "let f = match self.fns.get(name) {", "run_goal_*: reached only from the listed goal_* sinks (4 sites)"),
        ("interp/goal.rs", "if let Some(f) = self.fns.get(name) {", "run_goal: reached only from the listed goal_* sinks, behind goal_name_is_known (a sealed caller knows only its own fns)"),
        ("interp/goal.rs", ".fns", "goal_eval_holdout: the goal_eval sink; a sealed caller sees only its own fns"),
        ("interp/builtins.rs", "let outcome = match self.fns.get(fn_name.as_str()).copied() {", "a queued fiber: its name passed the scheduler_spawn sink"),
        ("interp/builtins.rs", "let visible = match self.fns.get(constraint.as_str()) {", "goal_run_constrained: a sink; a sealed caller sees only its own fns"),
        ("interp/builtins.rs", "let visible = self.fns.get(&fn_name).copied().filter(|f| {", "sandbox_run: a sink; a sealed caller sees only its own fns"),
        ("interp/builtins.rs", "if self.fn_by_name(&fn_name)?.is_none() {", "scheduler_spawn: a sink"),
        ("interp/builtins.rs", "if self.fn_by_name(&name)?.is_none() && !self.k().provenance.borrow().contains_key(&name) {", "kernel_goal_create: a sink"),
        ("interp/vm/compile.rs", "match fast_call_blocker(&self.ix.fn_table[i as usize], refine_preds_empty) {", "the compiler: an entry resolved from program text; a sealed run compiles nothing (spec Fork 1)"),
        ("interp/vm/compile.rs", "match self.ix.fn_of_sym.get(&var.s) {", "the compiler: a callee identifier's Sym in the program text"),
        ("interp/vm/compile.rs", "if args.is_empty() || args.len() != self.ix.fn_table[entry as usize].params.len() {", "the compiler: an entry resolved from program text"),
        ("interp/vm/compile.rs", "|| self.ix.fn_table[entry as usize].params.len() != 1", "the compiler: an entry resolved from program text"),
        ("interp/vm/compile.rs", "&self.ix.fn_table[entry as usize],", "the compiler: an entry resolved from program text"),
        ("interp/vm/compile.rs", "Expr::Ident(name) => self.ix.fn_of_sym.get(&self.res.sym(callee, name)).copied(),", "the compiler: the callee identifier of a `&mut` call, in the program text"),
        ("interp/vm/compile.rs", "let params = &self.ix.fn_table[i as usize].def.params;", "the compiler: an entry resolved from program text"),
        ("interp/vm/purefn.rs", "let entry = &self.fn_table[me as usize];", "the R50 S11 pure tier: the entry being built, an index from the VM's call op; VM engine only, a sealed run builds no pure code (`r51_t1b_a_sealed_run_enters_no_pure_tier_or_bulk_loop`)"),
        ("interp/vm/purefn.rs", "let name = &self.fn_table[g as usize].def.name;", "the R50 S11 pure tier's trace line: a built entry's own name, printed; VM engine only"),
        ("interp/vm/purefn.rs", "let np = self.fn_table[idx as usize].params.len();", "the R50 S11 pure tier: the VM call op's entry operand; VM engine only"),
        ("interp/vm/mod.rs", "let entry = &self.fn_table[*entry as usize];", "the VM: an op's entry operand, compiled from program text; VM engine only"),
        ("interp/vm/mod.rs", "let callee = &self.fn_table[*e as usize];", "the VM: an op's entry operand, compiled from program text; VM engine only"),
        ("interp/vm/mod.rs", "let entry = &self.fn_table[*entry as usize];", "the VM: an op's entry operand, compiled from program text; VM engine only"),
        ("interp/vm/mod.rs", "let known = self.callees.borrow().get(callee.s.index()).copied();", "the VM: the callee identifier's Sym in the program text; VM engine only"),
        ("interp/vm/mod.rs", "self.call_fast(&self.fn_table[entry as usize], st, at)", "the VM: an op's entry operand, compiled from program text; VM engine only"),
        ("interp/vm/mod.rs", "let entry = &self.fn_table[entry as usize];", "the VM: an op's entry operand, compiled from program text; VM engine only"),
        ("interp/vm/mod.rs", "let entry = &self.fn_table[entry as usize];", "the VM: an op's entry operand, compiled from program text; VM engine only"),
    ];
    let mut found: Vec<(String, String)> = Vec::new();
    for (file, src) in interp_sources() {
        let lines: Vec<&str> = src.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim();
            if t.starts_with("//") || t.starts_with("///") {
                continue;
            }
            // `self.fns` split across lines (`self.fns\n .get(..)`).
            let hit = t.contains("fns.get(")
                || t.contains("fns.contains_key(")
                || t.contains("fn_by_name(")
                || t.contains("fn_of_sym.get(")
                || t.contains("fn_of_def.get(")
                || t.contains("callees.borrow().get(")
                || t.contains("fn_table[")
                || ((t == "self.fns" || t == ".fns")
                    && lines
                        .get(i + 1)
                        .is_some_and(|n| n.trim().starts_with(".get(")));
            if hit && !t.starts_with("pub(crate) fn fn_by_name") {
                found.push((file.to_string(), t.to_string()));
            }
        }
    }
    let mut allowed: Vec<(String, String)> = ALLOWED
        .iter()
        .map(|(f, l, _)| (f.to_string(), l.to_string()))
        .collect();
    found.sort();
    allowed.sort();
    assert_eq!(
        found, allowed,
        "DRIFT: an fn is looked up BY NAME somewhere unlisted — if the name can be a value sealed code chose, add its builtin to `pin::NAME_SINKS`; otherwise list the site with its reason"
    );
    // Every sink is a real builtin whose listed positions are `str` NAME params.
    for (b, idxs) in pin::NAME_SINKS {
        let def = crate::builtins::BUILTINS
            .iter()
            .find(|d| d.name == *b)
            .unwrap_or_else(|| panic!("sink `{b}` is not a builtin"));
        for &i in *idxs {
            let (pname, pty) = def.params[i];
            assert!(
                pty == "str" && (pname.contains("name") || pname == "constraint"),
                "sink `{b}` arg {i} is `{pname}: {pty}`, not a name"
            );
        }
    }
    // Every builtin arm that resolves a name is a sink (or reaches a name
    // chosen at a sink: a kernel goal's, a queued fiber's).
    const REACHES_A_SINKS_NAME: &[&str] = &["kernel_goal_run", "scheduler_run", "supervisor_run"];
    let src = interp_sources()
        .into_iter()
        .find(|(f, _)| *f == "interp/builtins.rs")
        .unwrap()
        .1;
    // The shared splitter asserts it found at least 250 arms (R51 T3), so the
    // loop below never runs over an empty cut.
    let arms = builtin_arms(&src);
    for (names, body) in &arms {
        let resolves = [
            "fn_by_name(",
            "fns.get(",
            "fns.contains_key(",
            "self.run_goal",
            "goal_eval_holdout(",
            "self.best_observed(",
            "self.best_input(",
            "self.best_inputs",
            "self.history(",
            "self.clear(",
            ".get(&name)",
            "contains_key(&name)",
        ]
        .iter()
        .any(|p| {
            body.lines()
                .any(|l| !l.trim_start().starts_with("//") && l.contains(p))
        });
        if !resolves {
            continue;
        }
        for n in names {
            if !crate::builtins::BUILTINS.iter().any(|d| d.name == n) {
                continue;
            }
            assert!(
                pin::name_sink(n).is_some() || REACHES_A_SINKS_NAME.contains(&n.as_str()),
                "DRIFT: builtin `{n}` resolves an fn or goal by name and is not in `pin::NAME_SINKS`"
            );
        }
    }
}
