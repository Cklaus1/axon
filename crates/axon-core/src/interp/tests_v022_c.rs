// R51 T3: v0.22 `interp::tests` rows ported under their v0.22 names (lane
// t3c). Included at the end of `interp.rs`'s `mod tests`, so `super::*` and
// the module's helpers are in scope.

/// A second `impl J for E` must never replace the first's methods. The
/// interpreter keys methods by (type, name) and keeps the LAST, so a
/// duplicate impl whose `check` is a no-op would silently disarm the
/// operator's. Two resolver checks refuse it (the impl-uniqueness check
/// and the per-(type, method) dispatch check), each on its own; this
/// accepts EITHER refusal (E0002), and fails "ATTACK:" only when the
/// pipeline accepts the program and the no-op actually ran.
#[test]
fn a_second_impl_never_replaces_the_first_impls_method() {
    let head = "type E = { want: i64 }\ntrait J { fn check(self: E, got: i64) }\n\
                impl J for E { fn check(self: E, got: i64) { assert_eq(got, self.want) } }\n\
                @[test]\nfn t() {\n    let e = E { want: 42 }\n    e.check(0)\n}\n";
    let errors = |src: &str| -> Vec<String> {
        crate::check_pipeline(src, "t.ax")
            .into_iter()
            .filter(|d| d.severity == "error")
            .map(|d| format!("{} {}", d.code, d.message))
            .collect()
    };
    // Control: the operator's check is live — the program is well formed
    // and its test FAILS on the wrong answer.
    assert!(errors(head).is_empty(), "control: {:?}", errors(head));
    let prog = crate::parse_source(head).expect("parses");
    assert!(
        run_test_fn_outcome(&prog, "t").is_err(),
        "control: check is live"
    );
    // Attack: a second impl whose `check` accepts anything.
    let src = format!("{head}impl J for E {{ fn check(self: E, got: i64) {{ }} }}\n");
    let e = errors(&src);
    if e.is_empty() {
        let prog = crate::parse_source(&src).expect("parses");
        let out = run_test_fn_outcome(&prog, "t");
        assert!(
            out.is_err(),
            "ATTACK: a second `impl J for E` replaced the operator's `check`: {out:?}"
        );
    }
    assert!(
        e.len() == 1 && e[0].starts_with("E0002"),
        "a duplicate impl is refused by exactly one E0002: {e:?}"
    );
}

#[test]
#[ignore = "R51 S0b: v0.22's addressed handler completion (Flow::HandlerDone carrying its with-depth) is not ported, so main's nearest `with` catches a foreign arm's completion (107 != 7); an unsealed-semantics change outside T3"]
fn no_control_transfer_escapes_a_frame() {
    // PCI candidate-1 review: `return` out of a predicate, and a `resume`
    // smuggled out of a handler arm in a closure, both unwound the
    // operator's test as a NORMAL completion (a completion token for a test
    // whose assertion never ran). The frame edge is now an allowlist
    // (`contain_frame`): whatever the transfer, it cannot leave the frame.
    let escapes = [
        ("param refinement", "fn solve(n: i64 where if n > 0 { return 0 } else { true }) -> i64 { n * 0 }\n"),
        ("return refinement", "fn solve(n: i64) -> (i64 where if _ == 0 { return 0 } else { true }) { n * 0 }\n"),
        ("verify", "@[verify(if value == 0 { return 0 } else { true })]\nfn solve(n: i64) -> i64 { n * 0 }\n"),
        (
            "resume closure",
            "fn grab() -> fn() -> str {\n    with handler { on IO(p) => || resume(\"go\") } {\n        println(\"x\")\n        || \"never\"\n    }\n}\n\
             fn solve(n: i64) -> i64 {\n    let k = grab()\n    let _ = k()\n    n * 0\n}\n",
        ),
    ];
    // Each escape is tried from two operator test shapes, separately — one
    // shape's own failure must not mask the other's escape: a direct call,
    // and a call from inside the operator's own handler arm (where a
    // smuggled `resume` lands).
    let bodies = [
        ("direct", "@[test]\nfn t() { assert_eq(solve(21), 42) }\n"),
        (
            "in the operator's arm",
            "fn check(p: str) -> str {\n    assert_eq(solve(21), 42)\n    p\n}\n\
             @[test]\nfn t() {\n    with handler { on IO(p) => resume(check(p)) } {\n        println(\"go\")\n    }\n}\n",
        ),
    ];
    // Through an OPERATOR HELPER (C9 round 3, rows): a `return` escaping
    // the candidate's frame lands in the helper, which then returns the
    // CANDIDATE's value as its own. The test body itself runs to its end,
    // so the completion rule (`EndedEarly`) cannot see it: the frame edge
    // is the only guard on this route. Checked first, so a shape the
    // completion rule also refuses cannot mask it.
    for (why, def) in [
        ("param refinement", "fn solve(n: i64 where if n > 0 { return 42 } else { true }) -> i64 { n * 0 }\n"),
        ("return refinement", "fn solve(n: i64) -> (i64 where if _ == 0 { return 42 } else { true }) { n * 0 }\n"),
        ("verify", "@[verify(if value == 0 { return 42 } else { true })]\nfn solve(n: i64) -> i64 { n * 0 }\n"),
    ] {
        let prog = crate::parse_source(&format!(
            "{def}fn check(n: i64) -> i64 {{\n    solve(n)\n}}\n\
             @[test]\nfn t() {{ assert_eq(check(21), 42) }}\n"
        ))
        .expect("parses");
        let end = run_test_fn_outcome(&prog, "t");
        assert!(
            end.is_err(),
            "ATTACK: a `return` escaped the candidate's {why} into the operator's helper \
             and the test completed ({why} (helper): {end:?})"
        );
    }
    for (why, def) in escapes {
        for (shape, body) in bodies {
            let prog = crate::parse_source(&format!("{def}{body}")).expect("parses");
            let end = run_test_fn_outcome(&prog, "t");
            assert!(end.is_err(), "{why} ({shape}): {end:?}");
        }
    }
    let struct_src = "type Arg = { n: i64 } where if _.n > 0 { return 0 } else { true }\n\
                      fn make(n: i64) -> i64 {\n    let a = Arg { n: n }\n    a.n * 0\n}\n\
                      @[test]\nfn t() { assert_eq(make(21), 42) }\n";
    let prog = crate::parse_source(struct_src).expect("parses");
    assert!(
        run_test_fn_outcome(&prog, "t").is_err(),
        "struct refinement"
    );

    let prog = crate::parse_source(
        // A handler completion is ADDRESSED: the IO arm (7, no resume)
        // finishes the operator's `with`, not the nearest one — the
        // candidate's Random handler used to swallow it (v was 107).
        "fn c() -> i64 {\n    with handler { on Random(p) => resume(1) } {\n        println(\"x\")\n        5\n    }\n}\n\
         @[test]\nfn t_handler() {\n    let v = with handler { on IO(p) => 7 } {\n        let r = c()\n        r + 100\n    }\n    assert_eq(v, 7)\n}\n\
         @[test]\nfn t_nan() { assert_eq_f64(0.0 / 0.0, 2.0) }\n\
         @[test]\nfn t_inf() { assert_eq_f64(1.0 / 0.0, 1.0 / 0.0) }\n\
         fn honest(n: i64 where n >= 0) -> (i64 where _ >= 0) { n * 2 }\n\
         @[test]\nfn t_honest() { assert_eq(honest(21), 42) }\n",
    )
    .expect("parses");
    let end = |t: &str| run_test_fn_outcome(&prog, t);
    assert_eq!(
        end("t_handler"),
        Ok(TestEnd::Completed),
        "ATTACK: a handler completion was caught by a `with` that did not install it"
    );
    assert!(end("t_nan").is_err(), "NaN passed an f64 assertion");
    assert_eq!(end("t_inf"), Ok(TestEnd::Completed));
    assert_eq!(end("t_honest"), Ok(TestEnd::Completed));

    // A property case that `exit(0)`s did not complete either.
    let prog = crate::parse_source(
        "fn bail(n: i64) -> i64 {\n    exit(0)\n    n\n}\n\
         @[test]\n@[forall]\nfn p(n: i64) { assert_eq(bail(n), 99) }\n",
    )
    .expect("parses");
    assert!(
        matches!(
            proptest::run_property_test(&prog, "p", 5),
            proptest::PropertyOutcome::Failed { .. }
        ),
        "an exit(0) property case passed"
    );
}
