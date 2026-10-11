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
