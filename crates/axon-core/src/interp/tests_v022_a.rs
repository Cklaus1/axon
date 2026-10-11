// v0.22-only `interp::tests` rows (R51 spec §8 T3), ported under their v0.22
// names: this file is `include!`d at the end of `interp.rs`'s `mod tests`.
// Each body is v0.22's; an adaptation is marked where it is made.

// ── C9 round 4, PSV-1 (amendment 53, matrix A86): the candidate never
// chooses the code that runs under the operator's judging method. The
// review's keyed pass: a candidate returned its own `Fake` from a fn
// declared `-> i64` (a type confusion through `dict_get`, whose value
// type is free), and the operator's `r.ok()` dispatched on the RUNTIME
// type to the candidate's `Fake::ok`. Two sources close it: every value
// is CAST to the declared type at each boundary it crosses, and in
// operator code an operator-defined method name is the operator's.
//
// `ok` on an i64 is the real check (3 * 3 = 9); `ok` on a bool reads the
// bool — a second, lenient impl the operator wrote for its own reasons.
// A confused `true` selects it, so each attack below reaches a PASS
// unless its own guard refuses it (no candidate method is involved, so
// the dispatch edge cannot stand in for the cast).
const JUDGE: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for bool {\n    fn ok(self: bool) -> bool { self }\n}\n";
/// Candidate-side laundering: a `true`, typed as whatever the context
/// wants.
const LAUNDER: &str =
    "fn stash(v: bool) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";
const CONFUSED: &str = "match dict_get(stash(true), \"k\") { Some(v) => v  None => 0 }";

fn judged(tag: &str, suite: &str, cand: &str) -> Result<TestEnd, String> {
    sealed_outcome(
        tag,
        &format!("{JUDGE}{suite}"),
        &format!("{LAUNDER}{}", cand.replace("CONFUSED", CONFUSED)),
        "t",
    )
}

/// A control pair on one suite: the right candidate completes, the wrong
/// one fails — so the attack meets a live check.
fn live(tag: &str, suite: &str, good: &str, wrong: &str) {
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

fn confusion_refused(out: &Result<TestEnd, String>) -> bool {
    matches!(out, Err(m) if m.contains("runtime type confusion"))
}

fn judged8(tag: &str, suite: &str, cand: &str) -> Result<TestEnd, String> {
    sealed_outcome(
        tag,
        &format!("{JUDGE8}{suite}"),
        &format!("{LAUNDER8}{cand}"),
        "t",
    )
}

fn live8(tag: &str, suite: &str, good: &str, wrong: &str) {
    assert_eq!(
        judged8(tag, suite, good),
        Ok(TestEnd::Completed),
        "control: the right answer"
    );
    assert!(
        judged8(tag, suite, wrong).is_err(),
        "control: the wrong answer fails"
    );
}

fn honest8(tag: &str, suite: &str, cand: &str) {
    assert_eq!(
        judged8(tag, suite, cand),
        Ok(TestEnd::Completed),
        "control: an honest program crosses"
    );
}

/// The suite for the dict tests: the operator's dict holds an i64 at
/// "a"; `body` hands it to the candidate; the operator then reads "a"
/// UNTYPED and calls the judge on it (amendment 72 part 2).
fn dict_suite(body: &str) -> String {
    format!("@[test]\nfn t() {{\n    let d = dict_new()\n    dict_set(d, \"a\", 3)\n    {body}\n    match dict_get(d, \"a\") {{\n        Some(x) => assert(x.ok())\n        None => assert(false)\n    }}\n}}\n")
}

fn dict_refused(why: &str, suite: &str, cand: &str) {
    let out = judged8("r4c-dict", suite, cand);
    assert!(
        out != Ok(TestEnd::Completed)
            && matches!(&out, Err(m) if m.contains("retyped a dict entry")),
        "ATTACK: sealed code retyped the operator's dict entry to a u8 ({why}): {out:?}"
    );
}

/// `match <laundered u8> { Some(v) => { act } None => {} }`
fn put_u8(act: &str) -> String {
    u8_or(&format!("{{ {act} }}"), "{}")
}

fn mut_refused(why: &str, out: &Result<TestEnd, String>, needle: &str) {
    assert!(
        out != &Ok(TestEnd::Completed) && matches!(out, Err(m) if m.contains(needle)),
        "ATTACK: a `&mut` write-through carried a candidate-chosen value to the operator ({why}): {out:?}"
    );
}

/// Scalar kind: a confused `true` from a fn declared `-> i64`.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_confused_scalar_never_crosses_a_declared_return() {
    let suite = "@[test]\nfn t() {\n    let r = solve(3)\n    assert(r.ok())\n}\n";
    live(
        "psv1s",
        suite,
        "fn solve(n: i64) -> i64 { n * n }\n",
        "fn solve(n: i64) -> i64 { n + 1 }\n",
    );
    let out = judged("psv1s", suite, "fn solve(n: i64) -> i64 { CONFUSED }\n");
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a confused bool crossed a declared `-> i64` return: {out:?}"
    );
}

/// Struct name: an `Other` returned from a fn declared `-> Point`.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_confused_struct_never_crosses_as_another_struct() {
    let suite = "impl Judge for Point {\n    fn ok(self: Point) -> bool { self.x == 9 }\n}\n\
                 impl Judge for Other {\n    fn ok(self: Other) -> bool { true }\n}\n\
                 @[test]\nfn t() {\n    assert(solve(3).ok())\n}\n";
    let types = "type Point = { x: i64 }\ntype Other = { x: i64 }\n";
    live(
        "psv1n",
        suite,
        &format!("{types}fn solve(n: i64) -> Point {{ Point {{ x: n * n }} }}\n"),
        &format!("{types}fn solve(n: i64) -> Point {{ Point {{ x: n + 1 }} }}\n"),
    );
    let attack = format!(
        "{types}fn keep(v: Other) -> Dict {{\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}}\n\
         fn solve(n: i64) -> Point {{\n    match dict_get(keep(Other {{ x: n }}), \"k\") {{ Some(v) => v  None => Point {{ x: 0 }} }}\n}}\n"
    );
    let out = judged("psv1n", suite, &attack);
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: an `Other` crossed a declared `-> Point` return: {out:?}"
    );
}

/// Array elements, `Option` payloads, tuple elements.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_confused_element_never_crosses_inside_an_array() {
    let suite = "@[test]\nfn t() {\n    assert(solve(3)[0].ok())\n}\n";
    live(
        "psv1a",
        suite,
        "fn solve(n: i64) -> [i64] { [n * n] }\n",
        "fn solve(n: i64) -> [i64] { [n + 1] }\n",
    );
    let out = judged("psv1a", suite, "fn solve(n: i64) -> [i64] { [CONFUSED] }\n");
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a confused element crossed a declared `-> [i64]` return: {out:?}"
    );
}

#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_confused_payload_never_crosses_inside_an_option() {
    let suite = "@[test]\nfn t() {\n    match solve(3) {\n        Some(r) => assert(r.ok())\n        None => assert(false)\n    }\n}\n";
    live(
        "psv1o",
        suite,
        "fn solve(n: i64) -> Option<i64> { Some(n * n) }\n",
        "fn solve(n: i64) -> Option<i64> { Some(n + 1) }\n",
    );
    let out = judged(
        "psv1o",
        suite,
        "fn solve(n: i64) -> Option<i64> { Some(CONFUSED) }\n",
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a confused payload crossed a declared `-> Option<i64>` return: {out:?}"
    );
}

#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_confused_element_never_crosses_inside_a_tuple() {
    let suite = "@[test]\nfn t() {\n    assert(solve(3).0.ok())\n}\n";
    live(
        "psv1t",
        suite,
        "fn solve(n: i64) -> (i64, i64) { (n * n, 0) }\n",
        "fn solve(n: i64) -> (i64, i64) { (n + 1, 0) }\n",
    );
    let out = judged(
        "psv1t",
        suite,
        "fn solve(n: i64) -> (i64, i64) { (CONFUSED, 0) }\n",
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a confused element crossed a declared `-> (i64, i64)` return: {out:?}"
    );
}

/// A struct's fields at the return: the field was ASSIGNED after
/// construction, so only the deep return cast sees it.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_confused_field_never_crosses_inside_a_struct() {
    let suite = "@[test]\nfn t() {\n    assert(solve(3).v.ok())\n}\n";
    let ty = "type Holder = { v: i64 }\n";
    live(
        "psv1f",
        suite,
        &format!("{ty}fn solve(n: i64) -> Holder {{ Holder {{ v: n * n }} }}\n"),
        &format!("{ty}fn solve(n: i64) -> Holder {{ Holder {{ v: n + 1 }} }}\n"),
    );
    let out = judged(
        "psv1f",
        suite,
        &format!("{ty}fn solve(n: i64) -> Holder {{\n    let h = Holder {{ v: 0 }}\n    h.v = CONFUSED\n    h\n}}\n"),
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a confused field crossed a declared `-> Holder` return: {out:?}"
    );
}

/// A field at CONSTRUCTION: a candidate global the suite reads crosses
/// no fn boundary, so the struct literal is the declared type it meets.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_confused_field_is_refused_at_construction() {
    let suite = "@[test]\nfn t() {\n    assert(H.v.ok())\n}\n";
    let ty = "type Holder = { v: i64 }\n";
    live(
        "psv1c",
        suite,
        &format!("{ty}let H = Holder {{ v: 9 }}\n"),
        &format!("{ty}let H = Holder {{ v: 4 }}\n"),
    );
    let out = judged(
        "psv1c",
        suite,
        &format!("{ty}let H = Holder {{ v: CONFUSED }}\n"),
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a confused field was constructed into a candidate global: {out:?}"
    );
}

/// A declared PARAMETER: an unannotated candidate global handed to the
/// operator's typed helper.
#[test]
#[ignore = "R51 S6: the parameter cast at the seal crossing (call_fn_sealed) ported (S3); needs a live sealed run"]
fn a_confused_argument_never_enters_a_declared_parameter() {
    let suite = "fn judge(x: i64) -> bool { x.ok() }\n@[test]\nfn t() {\n    assert(judge(X))\n}\n";
    live("psv1p", suite, "let X = 9\n", "let X = 4\n");
    let out = judged("psv1p", suite, "let X = CONFUSED\n");
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a confused value entered the operator's `x: i64` parameter: {out:?}"
    );
}

/// A closure's RESULT under the `fn` type it crossed.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_closures_confused_result_never_crosses_its_declared_type() {
    let suite = "@[test]\nfn t() {\n    let f = mk()\n    assert(f(3).ok())\n}\n";
    live(
        "psv1k",
        suite,
        "fn mk() -> fn(i64) -> i64 { |n| n * n }\n",
        "fn mk() -> fn(i64) -> i64 { |n| n + 1 }\n",
    );
    let out = judged(
        "psv1k",
        suite,
        "fn mk() -> fn(i64) -> i64 { |n| CONFUSED }\n",
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a closure declared `fn(i64) -> i64` returned a confused bool: {out:?}"
    );
}

/// A closure's ARGUMENTS: the candidate calls the operator's listener
/// with a confused value under the `fn(i64) -> ()` it declared.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_closures_confused_argument_never_crosses_its_declared_type() {
    let suite = "@[test]\nfn t() {\n    visit(|r| assert(r.ok()))\n}\n";
    live(
        "psv1l",
        suite,
        "fn visit(cb: fn(i64) -> ()) { cb(9) }\n",
        "fn visit(cb: fn(i64) -> ()) { cb(4) }\n",
    );
    let out = judged(
        "psv1l",
        suite,
        "fn visit(cb: fn(i64) -> ()) { cb(CONFUSED) }\n",
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: the operator's listener was called with a confused bool: {out:?}"
    );
}

/// The listener's OWN annotation: the candidate declares `fn(T) -> ()`,
/// so only the operator's `|r: i64|` states the type.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_lambdas_annotated_parameter_is_cast() {
    let suite = "@[test]\nfn t() {\n    visit(|r: i64| assert(r.ok()))\n}\n";
    live(
        "psv1m",
        suite,
        "fn visit<T>(cb: fn(T) -> ()) { cb(9) }\n",
        "fn visit<T>(cb: fn(T) -> ()) { cb(4) }\n",
    );
    let out = judged(
        "psv1m",
        suite,
        "fn visit<T>(cb: fn(T) -> ()) { cb(CONFUSED) }\n",
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: the operator's `|r: i64|` listener was called with a confused bool: {out:?}"
    );
}

/// A channel is one invariant object: what the candidate sends on a
/// `Chan<i64>` is cast.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_confused_value_is_never_sent_on_a_declared_channel() {
    let suite =
        "@[test]\nfn t() {\n    let c = chan<i64>()\n    fill(c)\n    assert(c.recv().ok())\n}\n";
    live(
        "psv1q",
        suite,
        "fn fill(c: Chan<i64>) { c.send(9) }\n",
        "fn fill(c: Chan<i64>) { c.send(4) }\n",
    );
    let out = judged(
        "psv1q",
        suite,
        "fn fill(c: Chan<i64>) { c.send(CONFUSED) }\n",
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a confused bool was sent on a `Chan<i64>` the operator reads: {out:?}"
    );
}

/// A `let` annotation: the suite pins the type of a candidate global.
#[test]
#[ignore = "R51 S6: the let-annotation cast ported (S3); needs a live sealed run"]
fn a_let_annotation_is_cast() {
    let suite = "@[test]\nfn t() {\n    let r: i64 = X\n    assert(r.ok())\n}\n";
    live("psv1b", suite, "let X = 9\n", "let X = 4\n");
    let out = judged("psv1b", suite, "let X = CONFUSED\n");
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a confused bool was bound to the operator's `let r: i64`: {out:?}"
    );
}

/// At a declared soft type: the wrapper of THAT name, its inner value
/// cast to `T`; or a plain value cast to `T` (soft typing); the bare name
/// takes only its wrapper.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_declared_soft_type_casts_its_wrapper_and_its_inner_value() {
    let soft =
        "impl Judge for Uncertain {\n    fn ok(self: Uncertain) -> bool { self.value == 9 }\n}\n\
                impl Judge for Temporal {\n    fn ok(self: Temporal) -> bool { true }\n}\n\
                impl Judge for f64 {\n    fn ok(self: f64) -> bool { true }\n}\n";
    let s_wrap = format!(
        "{soft}@[test]\nfn t() {{\n    let r: Uncertain<i64> = solve(3)\n    assert(r.ok())\n}}\n\
         @[test]\nfn t_inner() {{\n    let r: Uncertain<i64> = solve(3)\n    assert(r.value.ok())\n}}\n"
    );
    let good = "fn solve(n: i64) -> Uncertain<i64> { uncertain_new(n * n, 0.9) }\n";
    let wrong = "fn solve(n: i64) -> Uncertain<i64> { uncertain_new(n + 1, 0.9) }\n";
    let laundered = |v: &str| {
        format!(
            "fn solve(n: i64) -> Uncertain<i64> {{\n    let d = dict_new()\n    dict_set(d, \"k\", {v})\n    match dict_get(d, \"k\") {{ Some(v) => v  None => uncertain_new(0, 0.9) }}\n}}\n"
        )
    };
    for test in ["t", "t_inner"] {
        assert_eq!(
            sealed_outcome(
                "s4soft",
                &format!("{JUDGE}{s_wrap}"),
                &format!("{LAUNDER}{good}"),
                test
            ),
            Ok(TestEnd::Completed),
            "control ({test}): the right answer"
        );
        assert!(
            sealed_outcome(
                "s4soft",
                &format!("{JUDGE}{s_wrap}"),
                &format!("{LAUNDER}{wrong}"),
                test
            )
            .is_err(),
            "control ({test}): the wrong answer fails"
        );
    }
    let attempt = |v: &str, test: &str| {
        sealed_outcome(
            "s4soft",
            &format!("{JUDGE}{s_wrap}"),
            &format!("{LAUNDER}{}", laundered(v)),
            test,
        )
    };
    let out = attempt("temporal_new(4, 100, 0.1)", "t");
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a Temporal crossed a declared Uncertain: {out:?}"
    );
    let out = attempt("uncertain_new_f64(4.0, 0.5)", "t_inner");
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: an Uncertain of another inner type crossed a declared Uncertain<i64>: {out:?}"
    );
    let out = attempt("true", "t");
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a plain value of another type crossed a declared Uncertain<i64>: {out:?}"
    );
    // The bare name.
    let s_bare = format!(
        "{soft}@[test]\nfn t() {{\n    let r: Uncertain = solve(3)\n    assert(r.ok())\n}}\n"
    );
    let bare = |body: &str| {
        sealed_outcome(
            "s4bare",
            &format!("{JUDGE}{s_bare}"),
            &format!("{LAUNDER}fn solve(n: i64) -> Uncertain {{ {body} }}\n"),
            "t",
        )
    };
    assert_eq!(
        bare("uncertain_new(n * n, 0.9)"),
        Ok(TestEnd::Completed),
        "control: bare"
    );
    assert!(
        bare("uncertain_new(n + 1, 0.9)").is_err(),
        "control: bare, wrong"
    );
    let out =
        bare("match dict_get(stash(true), \"k\") { Some(v) => v  None => uncertain_new(0, 0.9) }");
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a plain value crossed a declared bare Uncertain: {out:?}"
    );
}

/// The round-4b EQUIVALENCE blocker: the ENUM-name check had no row. An
/// `Other::A` laundered into a declared `-> Grade` ran the operator's
/// lenient `impl Judge for Other`.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_confused_enum_never_crosses_as_another_enum() {
    let suite = "impl Judge for Grade {\n    fn ok(self: Grade) -> bool { match self { Grade::A { x } => x == 9  Grade::B { x } => false } }\n}\n\
                 impl Judge for Other {\n    fn ok(self: Other) -> bool { true }\n}\n\
                 @[test]\nfn t() {\n    assert(solve(3).ok())\n}\n";
    let types =
        "type Grade = A { x: i64 } | B { x: i64 }\ntype Other = A { x: i64 } | B { x: i64 }\n";
    live(
        "e4name",
        suite,
        &format!("{types}fn solve(n: i64) -> Grade {{ Grade::A {{ x: n * n }} }}\n"),
        &format!("{types}fn solve(n: i64) -> Grade {{ Grade::A {{ x: n + 1 }} }}\n"),
    );
    let attack = format!(
        "{types}fn keep(v: Other) -> Dict {{\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}}\n\
         fn solve(n: i64) -> Grade {{\n    match dict_get(keep(Other::A {{ x: n }}), \"k\") {{ Some(v) => v  None => Grade::A {{ x: 0 }} }}\n}}\n"
    );
    let out = judged("e4name", suite, &attack);
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: an `Other::A` crossed a declared `-> Grade` return: {out:?}"
    );
}

/// B1. The operator's `chan<i64>()` STATES its element type: the channel
/// is stamped with it at creation, so a candidate's generic `Chan<T>`
/// binds `T` from it, and a value of another type is never sent. The
/// second attack reaches the stamp alone: through a dict hop the
/// candidate re-declares the channel `Chan<u8>` (a determined type of its
/// OWN choosing), which the send-side rule accepts.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_channel_carries_the_element_type_its_creation_states() {
    let suite =
        "@[test]\nfn t() {\n    let c = chan<i64>()\n    fill(c)\n    assert(c.recv().ok())\n}\n";
    live8(
        "r4c-chan",
        suite,
        "fn fill(c: Chan<i64>) { c.send(9) }\n",
        "fn fill(c: Chan<i64>) { c.send(4) }\n",
    );
    honest8(
        "r4c-chan",
        suite,
        "fn fill<T>(c: Chan<T>) {\n    let d: Chan<T> = c\n    put(d)\n}\nfn put(c: Chan<i64>) { c.send(9) }\n",
    );
    let out = judged8(
        "r4c-chan",
        suite,
        &format!(
            "fn fill<T>(c: Chan<T>) {{\n    {}\n}}\n",
            u8_or("c.send(v)", "{}")
        ),
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a u8 was sent on the operator's chan<i64>() through a generic Chan<T>: {out:?}"
    );
    let hop = "fn fill<T>(c: Chan<T>) {\n    let d = dict_new()\n    dict_set(d, \"c\", c)\n    match dict_get(d, \"c\") { Some(x) => put(x)  None => {} }\n}\nfn put(c: Chan<u8>) { c.send(narrow(4)) }\n";
    let out = judged8("r4c-chan", suite, hop);
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: the candidate re-declared the operator's chan<i64>() as Chan<u8> and sent a u8: {out:?}"
    );
}

/// B1, the return direction: a candidate returning `Chan<T>` at a `T`
/// nothing determined is refused — even with values queued, which the
/// candidate chose.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_channel_returned_at_an_undetermined_element_type_is_refused() {
    let suite = "@[test]\nfn t() {\n    let c = mk(9)\n    assert(c.recv().ok())\n}\n";
    honest8(
        "r4c-chanret",
        suite,
        "fn mk<T>(x: T) -> Chan<T> {\n    let c = Chan::new(1)\n    c.send(x)\n    c\n}\n",
    );
    let out = judged8(
        "r4c-chanret",
        suite,
        "fn mk<T>(n: i64) -> Chan<T> {\n    let c = chan<u8>()\n    c.send(narrow(4))\n    c\n}\n",
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: the candidate returned its own Chan<u8> at a Chan<T> nothing determined: {out:?}"
    );
}

/// B3. A generic struct's or enum's type argument is never erased: the
/// caller's `T` stays `T`, so the operator's argument binds it and the
/// candidate's field of another type is refused at the return.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_generic_struct_or_enum_argument_binds_its_type_parameter() {
    let wrap = "type Wrap<T> = { v: T }\n";
    let suite = "@[test]\nfn t() {\n    let w = solve(Wrap { v: 3 })\n    assert(w.v.ok())\n}\n";
    live8(
        "r4c-wrap",
        suite,
        &format!("{wrap}fn solve(w: Wrap<i64>) -> Wrap<i64> {{ Wrap {{ v: w.v * w.v }} }}\n"),
        &format!("{wrap}fn solve(w: Wrap<i64>) -> Wrap<i64> {{ Wrap {{ v: w.v + 1 }} }}\n"),
    );
    honest8(
        "r4c-wrap",
        "@[test]\nfn t() {\n    let w = solve(Wrap { v: Wrap { v: 9 } })\n    assert(w.v.v.ok())\n}\n",
        &format!("{wrap}fn solve<T>(w: Wrap<Wrap<T>>) -> Wrap<Wrap<T>> {{ Wrap {{ v: Wrap {{ v: w.v.v }} }} }}\n"),
    );
    let cases = [
        (
            "a Wrap<T> field (the review's candidate)",
            suite.to_string(),
            format!("{wrap}fn solve<T>(w: Wrap<T>) -> Wrap<T> {{\n    {}\n}}\n", u8_or("Wrap { v: v }", "w")),
        ),
        (
            "a nested Wrap<Wrap<T>> field",
            "@[test]\nfn t() {\n    let w = solve(Wrap { v: Wrap { v: 3 } })\n    assert(w.v.v.ok())\n}\n".to_string(),
            format!(
                "{wrap}fn solve<T>(w: Wrap<Wrap<T>>) -> Wrap<Wrap<T>> {{\n    {}\n}}\n",
                u8_or("Wrap { v: Wrap { v: v } }", "w")
            ),
        ),
        (
            "a generic enum variant's field",
            "@[test]\nfn t() {\n    match solve(Opt::Has { v: 3 }) {\n        Opt::Has { v } => assert(v.ok())\n        Opt::Nada => assert(false)\n    }\n}\n".to_string(),
            format!(
                "type Opt<T> = Has {{ v: T }} | Nada\nfn solve<T>(o: Opt<T>) -> Opt<T> {{\n    {}\n}}\n",
                u8_or("Opt::Has { v: v }", "o")
            ),
        ),
    ];
    for (why, suite, cand) in cases {
        let out = judged8("r4c-wrap", &suite, &cand);
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a u8 crossed a generic type argument the operator fixed to i64 ({why}): {out:?}"
        );
    }
}

/// B2. A fn with NO declared return type: the checker types its call
/// `()`, so at the seal crossing the operator receives `()` — never the
/// value the body ended on, which the operator's method call would
/// dispatch on. (The checker now also refuses `.ok()` on `()` with no
/// impl; this test reaches the runtime rule without the checker.)
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_fn_with_no_declared_return_type_hands_the_operator_unit() {
    let suite = "@[test]\nfn t() {\n    assert(solve(3).ok())\n}\n";
    live8(
        "r4c-noret",
        suite,
        "fn solve(n: i64) -> i64 { n * n }\n",
        "fn solve(n: i64) -> i64 { n + 1 }\n",
    );
    // Control: an honest unit fn whose body ends on a value still runs.
    honest8(
        "r4c-noret",
        "@[test]\nfn t() {\n    note(3)\n    assert(true)\n}\n",
        "fn note(n: i64) {\n    let d = dict_new()\n    dict_set(d, \"n\", n)\n    d\n}\n",
    );
    for (why, suite, cand) in [
        ("a fn", suite.to_string(), format!("fn solve(n: i64) {{\n    {}\n}}\n", u8_or("v", "0"))),
        (
            "an impl method",
            "@[test]\nfn t() {\n    assert(mk(3).val().ok())\n}\n".to_string(),
            format!(
                "type Sq = {{ n: i64 }}\ntrait Api {{\n    fn val(self)\n}}\nimpl Api for Sq {{\n    fn val(self: Sq) {{\n        {}\n    }}\n}}\nfn mk(n: i64) -> Sq {{ Sq {{ n: n }} }}\n",
                u8_or("v", "0")
            ),
        ),
    ] {
        let out = judged8("r4c-noret", &suite, &cand);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("no method `ok` on type `()`")),
            "ATTACK: the u8 a unit {why} ended on reached the operator's method call: {out:?}"
        );
    }
}

/// A native handle bound to a type parameter: only a handle of that kind
/// crosses back at it.
#[test]
#[ignore = "R51 S6: the type-parameter binding cast at the return edge ported (S3); needs a live sealed run"]
fn a_handle_binding_admits_only_that_handle() {
    let suite = "@[test]\nfn t() {\n    let w = gfx::window_open(8, 8, \"x\")\n    let r = pass(w)\n    assert(r.ok())\n}\n";
    let ok = judged8("r4c-handle", "@[test]\nfn t() {\n    let w = gfx::window_open(8, 8, \"x\")\n    let r = pass(w)\n    assert(true)\n}\n", "fn pass<T>(x: T) -> T { x }\n");
    assert_eq!(
        ok,
        Ok(TestEnd::Completed),
        "control: the handle crosses back"
    );
    let out = judged8(
        "r4c-handle",
        suite,
        &format!("fn pass<T>(x: T) -> T {{\n    {}\n}}\n", u8_or("v", "x")),
    );
    assert!(
        out != Ok(TestEnd::Completed) && confusion_refused(&out),
        "ATTACK: a u8 crossed a T the operator bound to a native handle: {out:?}"
    );
}

/// `host_await_val` is unavailable to a run with no host driver (every
/// sealed `axon test`), so no payload crosses a suspend inside a seal.
#[test]
#[ignore = "R51 S6: host_await_val refuses outside a suspendable run on main; needs a live sealed run"]
fn a_host_await_crossing_is_unavailable_inside_a_sealed_test_run() {
    let out = judged8(
        "r4c-await",
        "@[test]\nfn t() {\n    assert(solve(3).ok())\n}\n",
        "fn solve<T>(x: T) -> T {\n    host_await_val(x)\n}\n",
    );
    assert!(
        out != Ok(TestEnd::Completed)
            && matches!(&out, Err(m) if m.contains("outside a suspendable run")),
        "ATTACK: a host_await_val payload came back inside a sealed test run: {out:?}"
    );
}

/// The other edges: a dict handed to an operator CLOSURE the candidate
/// calls (the operator's `|x| …` reads it untyped), over a channel, and
/// through the candidate's own closure the operator calls.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_dict_the_candidate_mutated_is_verified_at_every_edge_back() {
    let closure_suite = "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"a\", 3)\n    assert(run(d, |x| {\n        match dict_get(x, \"a\") {\n            Some(y) => y.ok()\n            None => false\n        }\n    }))\n}\n";
    honest8(
        "r4c-dict",
        closure_suite,
        "fn run(d: Dict, f: fn(Dict) -> bool) -> bool {\n    dict_set(d, \"a\", 9)\n    f(d)\n}\n",
    );
    let out = judged8(
        "r4c-dict",
        closure_suite,
        &format!(
            "fn run(d: Dict, f: fn(Dict) -> bool) -> bool {{\n    {}\n    f(d)\n}}\n",
            put_u8("dict_set(d, \"a\", v)")
        ),
    );
    assert!(
        out != Ok(TestEnd::Completed)
            && matches!(&out, Err(m) if m.contains("retyped a dict entry")),
        "ATTACK: the operator's closure read a dict entry the candidate retyped: {out:?}"
    );
    // Over a channel the operator filled.
    let chan_suite = dict_suite("let c = chan<Dict>()\n    c.send(d)\n    solve(c)");
    honest8(
        "r4c-dict",
        &chan_suite,
        "fn solve(c: Chan<Dict>) {\n    let d = c.recv()\n    dict_set(d, \"a\", 9)\n}\n",
    );
    dict_refused(
        "received from the operator's channel",
        &chan_suite,
        &format!(
            "fn solve(c: Chan<Dict>) {{\n    let d = c.recv()\n    {}\n}}\n",
            put_u8("dict_set(d, \"a\", v)")
        ),
    );
    // A closure replaced by a non-closure: refused where it is stored, not
    // only when the operator later calls it.
    let fnonc = "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"f\", |n: i64| n * n)\n    solve(d)\n    match dict_get(d, \"f\") {\n        Some(f) => assert(true)\n        None => assert(false)\n    }\n}\n";
    honest8(
        "r4c-dict",
        fnonc,
        "fn solve(d: Dict) { dict_set(d, \"g\", 1) }\n",
    );
    let out = judged8(
        "r4c-dict",
        fnonc,
        "fn solve(d: Dict) { dict_set(d, \"f\", 0) }\n",
    );
    assert!(
        out != Ok(TestEnd::Completed)
            && matches!(&out, Err(m) if m.contains("retyped a dict entry")),
        "ATTACK: a closure the operator held was replaced by a non-closure: {out:?}"
    );
    // The candidate's closure, stored where the operator held its own.
    let fsuite = "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"f\", |n: i64| n * n)\n    solve(d)\n    match dict_get(d, \"f\") {\n        Some(f) => assert(f(3).ok())\n        None => assert(false)\n    }\n}\n";
    honest8(
        "r4c-dict",
        fsuite,
        "fn solve(d: Dict) { dict_set(d, \"g\", 1) }\n",
    );
    let out = judged8(
        "r4c-dict",
        fsuite,
        "fn solve(d: Dict) { dict_set(d, \"f\", |n: i64| narrow(n)) }\n",
    );
    assert!(
        out != Ok(TestEnd::Completed)
            && matches!(&out, Err(m) if m.contains("retyped a dict entry")),
        "ATTACK: the candidate's closure replaced the operator's in its dict: {out:?}"
    );
}

/// What the rule does NOT touch: keys the candidate ADDS, a dict the
/// candidate builds, the same type written again, and a dict the operator
/// itself retypes between two calls (the snapshot is retaken).
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_dict_the_candidate_adds_to_or_builds_still_crosses() {
    let suite = dict_suite("solve(d)");
    honest8(
        "r4c-dict",
        &suite,
        "fn solve(d: Dict) {\n    dict_set(d, \"a\", 9)\n    dict_set(d, \"b\", narrow(4))\n}\n",
    );
    honest8(
        "r4c-dict",
        "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"a\", 3)\n    solve(d)\n    dict_set(d, \"a\", 7.5)\n    touch(d)\n    assert(true)\n}\n",
        "fn solve(d: Dict) { dict_set(d, \"a\", 9) }\nfn touch(d: Dict) { dict_set(d, \"a\", 8.5) }\n",
    );
    honest8(
        "r4c-dict",
        "@[test]\nfn t() {\n    match dict_get(make(), \"a\") {\n        Some(x) => assert(x.ok())\n        None => assert(false)\n    }\n}\n",
        "fn make() -> Dict {\n    let d = dict_new()\n    dict_set(d, \"a\", 9)\n    d\n}\n",
    );
}

/// C9 round 8, PSV-1 blocker: a candidate fn that writes through `&mut`
/// returns control to operator code with the parameter's FINAL value, which
/// moved into the operator's binding with no seal edge. Cast at the edge
/// back against the DECLARED parameter type (dispatch arm and width arm).
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_mut_write_through_value_is_cast_at_the_seal_edge_back() {
    let dispatch = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    assert(a[0].ok())\n}\n";
    let width = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    assert((a[0] << 1) == 254)\n}\n";
    live8(
        "r8-mutcast",
        dispatch,
        "fn fill(a: &mut [i64]) { a[0] = 9 }\n",
        "fn fill(a: &mut [i64]) { a[0] = 4 }\n",
    );
    live8(
        "r8-mutcast",
        width,
        "fn fill(a: &mut [i64]) { a[0] = 127 }\n",
        "fn fill(a: &mut [i64]) { a[0] = 200 }\n",
    );
    // First: into an EMPTY output array, which shows no type at the position
    // (so what the operator held cannot judge it): the honest fill passes, and
    // the declared type alone refuses an appended `u8` — or a value at an
    // undetermined type parameter.
    let empty = "@[test]\nfn t() {\n    let mut a: [i64] = []\n    fill(&mut a)\n    assert(a[0].ok())\n}\n";
    live8(
        "r8-mutcast",
        empty,
        "fn fill(a: &mut [i64]) { a = arr_concat(a, [9]) }\n",
        "fn fill(a: &mut [i64]) { a = arr_concat(a, [4]) }\n",
    );
    mut_refused(
        "an appended u8 into an empty output array",
        &judged8(
            "r8-mutcast",
            empty,
            &mut_fill("&mut [i64]", "a = arr_concat(a, [v])"),
        ),
        "left holding",
    );
    mut_refused(
        "a generic element into an empty output array",
        &judged8(
            "r8-mutcast",
            empty,
            &mut_fill("&mut [T]", "a = arr_concat(a, [v])").replace("fn fill(", "fn fill<T>("),
        ),
        "left holding",
    );
    let atk = mut_fill("&mut [i64]", "a[0] = v");
    mut_refused(
        "the u8 impl answered",
        &judged8("r8-mutcast", dispatch, &atk),
        "left holding",
    );
    let atk255 = atk.replace("narrow(4)", "narrow(255)");
    mut_refused(
        "255 as u8 wrapped in the operator's shift",
        &judged8("r8-mutcast", width, &atk255),
        "left holding",
    );
}

/// The cast runs on EVERY outcome: an operator handler that aborts the call
/// (`HandlerDone`) after the candidate wrote a `u8` must not leave it in the
/// operator's binding.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_mut_value_is_cast_when_an_operator_handler_aborts_the_call() {
    let suite = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    let _r = with handler { on IO(p) => 0 } {\n        fill(&mut a)\n        1\n    }\n    assert(a[0].ok())\n}\n";
    // Control: an honest write then an aborted call keeps the honest value.
    let honest = "fn fill(a: &mut [i64]) {\n    a[0] = 9\n    println(\"x\")\n}\n";
    assert_eq!(
        judged8("r8-mutabort", suite, honest),
        Ok(TestEnd::Completed),
        "control"
    );
    let out = judged8(
        "r8-mutabort",
        suite,
        &mut_fill("&mut [i64]", "a[0] = v\n        println(\"x\")"),
    );
    assert!(
        out != Ok(TestEnd::Completed),
        "ATTACK: a `&mut` u8 survived an aborted call into the operator's binding: {out:?}"
    );
}

/// C9 round 9, SENTINEL minor: a candidate calling its OWN fn value with
/// arguments (`let g = inc; g(n)`) was refused as "sealed code called an
/// operator closure", because a fn value carried no sealed mark. The
/// operator's fn values stay unmarked, so the refusal stands for them, and a
/// candidate fn value cannot take the place of an operator closure a dict held.
#[test]
#[ignore = "R51 S6: the sealed_fnval mark, closure_args_check and replaced_ok ported (S3); needs a live sealed run"]
fn a_candidates_own_fn_value_takes_arguments_and_an_operators_still_does_not() {
    // R51 adaptation: v0.22 ran this with `sealed_outcome_rule(.., true)`
    // (the receiver dispatch rule on); that switch arrives with S4, whose merge
    // restores the call (it passes either way with the seal flipped).
    let run = |suite: &str, cand: &str| sealed_outcome("r9-fnval", suite, cand, "t");
    let s = "@[test]\nfn t() { assert_eq(solve(3), 4) }\n";
    let own = "fn inc(n: i64) -> i64 { n + 1 }\nfn solve(n: i64) -> i64 {\n    let g = inc\n    g(n)\n}\n";
    assert_eq!(
        run(s, own),
        Ok(TestEnd::Completed),
        "the candidate's own fn value with an argument"
    );
    assert!(
        run(s, &own.replace("n + 1", "n + 2")).is_err(),
        "control: wrong fails"
    );
    // A fn value a candidate fn mutates a dict through, and one stored and
    // called back, still pass the dict edge.
    let dict = "fn bump(d: Dict) -> i64 {\n    dict_set(d, \"k\", 2)\n    2\n}\nfn solve(n: i64) -> i64 {\n    let d = dict_new()\n    dict_set(d, \"k\", 1)\n    let g = bump\n    g(d) + n - 1\n}\n";
    assert_eq!(
        run(s, dict),
        Ok(TestEnd::Completed),
        "a dict through the candidate's own fn value"
    );
    // An operator fn value held in a dict, called by sealed code with an
    // argument no contract determined: refused as before.
    let held = "fn plain(n: i64) -> i64 { n }\n@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"f\", plain)\n    assert_eq(solve(d), 3)\n}\n";
    let out = run(
        held,
        "fn solve(d: Dict) -> i64 {\n    match dict_get(d, \"f\") { Some(f) => f(3)  None => 0 }\n}\n",
    );
    assert!(
        out != Ok(TestEnd::Completed)
            && matches!(&out, Err(m) if m.contains("nothing on the operator side determined")),
        "ATTACK: sealed code called an operator fn value with an undetermined argument: {out:?}"
    );
    // A candidate fn value does not replace an operator closure a dict held.
    let slot = "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"f\", || 9)\n    solve(d)\n    match dict_get(d, \"f\") { Some(f) => assert_eq(f(), 9)  None => assert(false) }\n}\n";
    let out = run(
        slot,
        "fn mine() -> i64 { 9 }\nfn solve(d: Dict) {\n    dict_set(d, \"f\", mine)\n}\n",
    );
    assert!(
        out != Ok(TestEnd::Completed) && matches!(&out, Err(m) if m.contains("operator closure")),
        "ATTACK: a candidate fn value replaced an operator closure in the operator's dict: {out:?}"
    );
}

/// A candidate fn's value, called by the OPERATOR, still reaches only the
/// candidate's fn, which cannot reach an operator fn (a regression control:
/// no guard of its own, the call edge in `call_fn_sealed` refuses).
#[test]
#[ignore = "R51 S6: the call edge (call_fn_sealed) ported (S3); needs a live sealed run"]
fn a_candidate_fn_value_the_operator_calls_still_cannot_reach_operator_fns() {
    let suite =
        "fn secret() -> i64 { 9 }\n@[test]\nfn t() { let g = give()\n    assert_eq(g(), 9) }\n";
    // The candidate's own fn calls the operator's `secret`: refused whether the
    // call is direct or through the value the operator invokes.
    // R51 adaptation: v0.22 ran this with `sealed_outcome_rule(.., true)`
    // (the receiver dispatch rule on); that switch arrives with S4, whose merge
    // restores the call (it passes either way with the seal flipped).
    let out = sealed_outcome(
        "r8-fnval2",
        suite,
        "fn mine() -> i64 { secret() }\nfn give() -> fn() -> i64 { mine }\n",
        "t",
    );
    assert!(
        out != Ok(TestEnd::Completed) && matches!(&out, Err(m) if m.contains("cannot use")),
        "ATTACK: the operator's call of a candidate fn value ran operator code in the candidate's name: {out:?}"
    );
}

/// d1 (round 6 blocker): a candidate closure that captured the operator's
/// dict retypes an entry AFTER the operator changed it (the snapshot was
/// from the hand-over). Refused by the dict layer (rule off) AND by the
/// dispatch rule (rule on); the pinned suite refuses it by the cast.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_closure_that_captured_the_operators_dict_cannot_retype_after_an_operator_write() {
    let suite = "@[test]\nfn t() {\n    let d = dict_new()\n    let f = make(d)\n    dict_set(d, \"answer\", 3)\n    f()\n    match dict_get(d, \"answer\") {\n        Some(v) => assert(v.ok())\n        None => assert(false)\n    }\n}\n";
    let cand = |v: &str| {
        format!("fn make(d: Dict) -> fn() -> i64 {{\n    || {{\n        dict_set(d, \"answer\", {v})\n        0\n    }}\n}}\n")
    };
    let out = judged8(
        "r6-stale",
        suite,
        &format!("{LAUNDER8}{}", cand("narrow(4)")),
    );
    assert!(
        out != Ok(TestEnd::Completed)
            && matches!(&out, Err(m) if m.contains("retyped a dict entry")),
        "ATTACK: a captured-dict closure retyped an entry against a stale snapshot: {out:?}"
    );
    // With the rule on too, it is refused (by whichever layer meets it first).
    let on = judged_on("r6-stale", suite, &cand("narrow(4)"));
    assert!(
        matches!(&on, Err(m) if m.contains("retyped a dict entry") || m.contains("whose type nothing on the operator side determined")),
        "ATTACK: the captured-dict closure's retype passed with the dispatch rule on: {on:?}"
    );
    honest8("r6-stale", suite, &cand("9"));
}

/// A dict too big to snapshot is REFUSED at the crossing, never skipped.
#[test]
#[ignore = "R51 S6: dict_edge_in's DICT_SNAP_MAX refusal ported (S3); needs a live sealed run"]
fn a_dict_over_the_snapshot_bound_is_refused_not_skipped() {
    let suite = "@[test]\nfn t() {\n    let d = dict_new()\n    let i = 0\n    while i <= 1000000 {\n        dict_set(d, to_str(i), i)\n        i = i + 1\n    }\n    solve(d)\n    assert(true)\n}\n";
    let out = judged8("r4c-dict", suite, "fn solve(d: Dict) { }\n");
    assert!(
        out != Ok(TestEnd::Completed)
            && matches!(&out, Err(m) if m.contains("cannot cross a seal")),
        "ATTACK: a dict past the snapshot bound crossed unrecorded: {out:?}"
    );
}

/// B1, the channel the operator created WITHOUT a closed element type
/// (`Chan::new`, or `chan<T>()` in its own generic code): a sealed send
/// on it needs an element type something on the operator side
/// determined. A free `T` of the candidate's never counts.
#[test]
#[ignore = "R51 S4: needs sealed_outcome to switch the receiver dispatch rule off (v0.22 DISPATCH_RULE_OFF), else the rule refuses the honest control's untyped `.ok()`; sealed control refused until S6"]
fn a_sealed_send_on_an_operator_channel_needs_a_determined_element_type() {
    let suite = "@[test]\nfn t() {\n    assert(run(9))\n}\nfn run<T: Judge>(x: T) -> bool {\n    let c = chan<T>()\n    c.send(x)\n    fill(c)\n    c.recv().ok()\n}\n";
    // Control: an honest generic relay — `U` is bound from the value the
    // operator queued, so the send meets a determined type.
    honest8(
        "r4c-opchan",
        suite,
        "fn fill<U>(c: Chan<U>) {\n    let v = c.recv()\n    c.send(v)\n}\n",
    );
    // Control: the candidate's OWN unstamped channel is its business.
    honest8(
        "r4c-opchan",
        "@[test]\nfn t() {\n    assert(mine())\n}\n",
        "fn mine() -> bool {\n    let c = Chan::new(2)\n    c.send(9)\n    c.recv() == 9\n}\n",
    );
    // The attack: nothing queued, so nothing binds the candidate's `U`.
    let empty = "@[test]\nfn t() {\n    assert(run(9))\n}\nfn run<T: Judge>(x: T) -> bool {\n    let c = chan<T>()\n    fill(c)\n    c.recv().ok()\n}\n";
    let cand = format!(
        "fn fill<U>(c: Chan<U>) {{\n    {}\n}}\n",
        u8_or("c.send(v)", "{}")
    );
    let out = judged8("r4c-opchan", empty, &cand);
    assert!(
        out != Ok(TestEnd::Completed)
            && matches!(&out, Err(m) if m.contains("nothing on the operator side determined")),
        "ATTACK: a u8 was sent on the operator's unstamped chan<T>() at the candidate's free U: {out:?}"
    );
    let suite_new =
        "@[test]\nfn t() {\n    let c = Chan::new(4)\n    fill(c)\n    assert(c.recv().ok())\n}\n";
    let out = judged8(
        "r4c-opchan",
        suite_new,
        &format!(
            "fn fill<U>(c: Chan<U>) {{\n    {}\n}}\n",
            u8_or("c.send(v)", "{}")
        ),
    );
    assert!(
        out != Ok(TestEnd::Completed)
            && matches!(&out, Err(m) if m.contains("nothing on the operator side determined")),
        "ATTACK: a u8 was sent on the operator's Chan::new(4) at the candidate's free U: {out:?}"
    );
}

/// C9 round 8 (sentinel blocker): a top-level fn named in VALUE position is
/// a first-class closure. Made by a SEALED frame from an OPERATOR fn it was
/// a forwarding body that ran in an operator frame, where the call edge no
/// longer applied (`let g = secret; g()` ran the operator's `secret`; a
/// global `SECRET` leaked through `let g = peek`). The edge now applies at
/// creation.
#[test]
#[ignore = "R51 S6: the creation edge for an operator fn named in value position (eval.rs ident_unbound, sealed_fnval) ported (S3); needs a live sealed run"]
fn a_sealed_frame_cannot_take_an_operator_fn_as_a_value() {
    let suite = "fn secret() -> i64 { 9 }\nlet SECRET = 41\nfn peek() -> i64 { SECRET }\n\
                 fn id_of(g: fn() -> i64) -> i64 { g() }\n\
                 @[test]\nfn t() { assert_eq(solve(), 9) }\n\
                 @[test]\nfn t_leak() { assert_eq(solve(), 41) }\n\
                 @[test]\nfn t_pass() { assert_eq(take(secret), 9) }\n\
                 @[test]\nfn t_back() { let g = give()\n    assert_eq(g(), 9) }\n\
                 @[test]\nfn t_arg() { assert_eq(solve(), 9) }\n";
    // R51 adaptation: v0.22 ran this with `sealed_outcome_rule(.., true)`
    // (the receiver dispatch rule on); that switch arrives with S4, whose merge
    // restores the call.
    let run = |cand: &str, t: &str| sealed_outcome("r8-fnval", suite, cand, t);
    // Controls: the candidate names ITS OWN fn as a value and calls it; the
    // operator names ITS OWN fn as a value and hands it to the candidate;
    // a candidate fn value returned to the operator runs.
    let own = "fn mine() -> i64 { 9 }\nfn solve() -> i64 {\n    let g = mine\n    g()\n}\n\
               fn take(f: fn() -> i64) -> i64 { f() }\nfn give() -> fn() -> i64 { mine }\n";
    for t in ["t", "t_pass", "t_back"] {
        assert_eq!(run(own, t), Ok(TestEnd::Completed), "control: {t}");
    }
    let wrong = own.replace("fn mine() -> i64 { 9 }", "fn mine() -> i64 { 4 }");
    assert!(run(&wrong, "t").is_err(), "control: wrong fails");
    // The attacks.
    let rest = "fn take(f: fn() -> i64) -> i64 { f() }\nfn give() -> fn() -> i64 { secret }\n";
    for (t, what, solve) in [
        ("t", "ran the operator's `secret` through a fn value", "fn solve() -> i64 {\n    let g = secret\n    g()\n}\n"),
        ("t_leak", "read the operator's global through a fn value", "fn solve() -> i64 {\n    let g = peek\n    g()\n}\n"),
        ("t", "stored the operator's fn in an array", "fn solve() -> i64 {\n    let gs = [secret]\n    gs[0]()\n}\n"),
        ("t", "stored the operator's fn in a dict", "fn solve() -> i64 {\n    let d = dict_new()\n    dict_set(d, \"f\", secret)\n    match dict_get(d, \"f\") { Some(f) => f()  None => 0 }\n}\n"),
        ("t", "handed the operator's fn to its own helper", "fn run(f: fn() -> i64) -> i64 { f() }\nfn solve() -> i64 { run(secret) }\n"),
    ] {
        let out = run(&format!("{rest}{solve}"), t);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("cannot use")),
            "ATTACK: a sealed frame took an operator fn as a value ({what}): {out:?}"
        );
    }
    // Returned to the operator, which calls it: the creation is refused.
    let out = run("fn solve() -> i64 { 0 }\nfn take(f: fn() -> i64) -> i64 { f() }\nfn give() -> fn() -> i64 { secret }\n", "t_back");
    assert!(
        out != Ok(TestEnd::Completed) && matches!(&out, Err(m) if m.contains("cannot use")),
        "ATTACK: the candidate returned the operator's fn as a value: {out:?}"
    );
}

// ── Moved to their v0.22 path (T3): main's ports of v0.22 rows that lived in
// another module. ──────────────────────────────────────────────────────────

/// `(file under interp/, fn, code)` for every non-comment line of the
/// interpreter (`interp.rs` and `interp/**`, `interp/vm/` included; test
/// modules excluded) that contains `needle` as a whole word after a `.`.
fn field_sites(needle: &str) -> Vec<(String, String, String)> {
    let sources = vm::tests::r51_interp_sources();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = Vec::new();
    for (file, src) in &sources {
        if file.ends_with("_tests.rs") {
            continue;
        }
        let mut func = String::new();
        for line in src.lines() {
            let code = line.find("//").map_or(line, |i| &line[..i]);
            let b = code.as_bytes();
            let mut from = 0;
            while let Some(i) = code[from..].find("fn ") {
                let at = from + i;
                from = at + 3;
                if at > 0 && ident(b[at - 1]) {
                    continue;
                }
                let name: String = code[at + 3..]
                    .bytes()
                    .take_while(|&c| ident(c))
                    .map(char::from)
                    .collect();
                if !name.is_empty() {
                    func = name;
                }
            }
            let pat = format!(".{needle}");
            let mut from = 0;
            while let Some(i) = code[from..].find(&pat) {
                let end = from + i + pat.len();
                from = end;
                if end < b.len() && ident(b[end]) && ident(*pat.as_bytes().last().unwrap()) {
                    continue;
                }
                out.push((file.clone(), func.clone(), code.trim().to_string()));
            }
        }
    }
    assert!(
        sources.iter().any(|(f, _)| f == "vm/mod.rs"),
        "the sweep reads interp/vm/"
    );
    out
}

/// v0.22 C9 round 9 (veto interp.rs:9632, widened per TaintPort-9 to main's
/// identifiers and `interp/vm/`): the global-read edge lives in ONE lookup.
/// Every line of the interpreter that touches a `globals` map is listed with
/// the reason it is not a read by running code; everything else reads through
/// [`Interp::global_ref`] (or asks existence only, [`Interp::is_global`]).
#[test]
fn every_global_read_goes_through_global_ref() {
    const ALLOWED: &[(&str, &str, &str, &str)] = &[
        ("", "of", "seal.globals.insert(intern(name));", "the seal's own set of sealed global names, built once"),
        ("", "merged_bindings", ".globals", "the end-of-run binding dump, not running code"),
        ("", "seal_global", "if self.seal.active && self.frame_sealed.get() && !self.seal.globals.contains(&s) {", "the edge itself (the sealed names)"),
        ("", "is_global", "self.globals.contains_key(&s)", "existence only, no value: the index arm's and the VM's in-place test, followed by `global_ref`"),
        ("", "init_globals_sealed", "let sealed = self.seal.globals.contains(&s);", "the seal's set of sealed global names: the initialiser's provenance"),
        ("", "init_globals_sealed", "self.globals.insert(s, v);", "the write at build, one initialiser at a time (a later one reads it through `global_ref`)"),
        ("", "global_ref", "match self.globals.get(&s) {", "THE lookup, which applies `seal_global`"),
        ("", "init_globals", "self.globals = env.snapshot().into_parts().0.into_iter().collect();", "the write at build"),
        ("pin.rs", "det", "self.globals.contains(n)", "the static pin analysis' set of global NAMES"),
        ("pin.rs", "build", "ctx.globals = lets.iter().map(|(n, _)| n.to_string()).collect();", "the static pin analysis' set of global NAMES"),
        ("pin.rs", "build", ".filter(|(n, e)| ctx.globals.contains(*n) && ctx.det(e, &none, &none))", "the static pin analysis' set of global NAMES"),
        ("pin.rs", "build", "if next == ctx.globals {", "the static pin analysis' set of global NAMES"),
        ("pin.rs", "build", "ctx.globals = next;", "the static pin analysis' set of global NAMES"),
    ];
    let mut found: Vec<(String, String, String)> = field_sites("globals");
    let mut allowed: Vec<(String, String, String)> = ALLOWED
        .iter()
        .map(|(f, n, l, _)| (f.to_string(), n.to_string(), l.to_string()))
        .collect();
    found.sort();
    allowed.sort();
    assert_eq!(
        found, allowed,
        "DRIFT: a mention of the `globals` map bypasses `global_ref` (and so `seal_global`); \
         route it through `Interp::global_ref`, or list it with its reason"
    );
    // The lookup has its readers: the `Ident` arm's rest, both in-place fast
    // paths and the closure-constant call (no vacuous pass).
    let mut readers: Vec<String> = field_sites("global_ref(")
        .into_iter()
        .map(|(_, f, _)| f)
        .collect();
    readers.sort();
    readers.dedup();
    for want in [
        "dispatch_named",
        "field_in_place",
        "ident_unbound",
        "index_in_place",
    ] {
        assert!(
            readers.iter().any(|r| r == want),
            "DRIFT: `{want}` no longer reads globals through `global_ref`: {readers:?}"
        );
    }
}
