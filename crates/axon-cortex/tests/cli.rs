//! The `cortex repair` contract: what each exit code means, and what none of
//! them may be rounded up to.
//!
//! The CLI is the production caller for everything in this crate. Its contract
//! is its EXIT CODES — that is what a script, a CI job or a supervising agent
//! actually reads — so a change that left the codes intact and the behaviour
//! wrong would be invisible to every other test here.
//!
//! The row that matters most is the first one: exit 0 must mean a hidden check
//! the generator never saw accepted the repair, not that the loop finished.

use std::path::{Path, PathBuf};
use std::process::Command;

fn axon_bin() -> PathBuf {
    // Resolved from the MANIFEST, and it fails rather than falling back.
    //
    // This used to look next to the test binary and, failing that, return the
    // bare name `axon` — so with a custom CARGO_TARGET_DIR the candidate never
    // existed and every CLI test silently ran whatever `axon` was on PATH.
    // On this machine that happened to be a symlink to the very binary under
    // test, so the results stood; on any other machine the suite would have
    // been measuring something nobody chose. A fallback that cannot say which
    // binary it ran is not a fallback, it is an unlogged substitution.
    let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("target/debug/axon");
    assert!(
        bin.exists(),
        "the CLI suite needs the interpreter at {}; build it with \
         `cargo build -p axon-core --no-default-features --bin axon`",
        bin.display()
    );
    bin
}

/// A fresh workspace holding the broken fixture, named per-test so concurrent
/// tests cannot edit one another's copy.
/// A staged copy of `broken.ax`, in a directory named for the caller.
///
/// `name` must be UNIQUE across this file. Tests run in parallel and this
/// removes the directory first, so two tests sharing a name delete each
/// other's files — which is exactly what happened, and it presented as a
/// generator that "did not exist".
fn workspace(name: &str) -> PathBuf {
    let ws = std::env::temp_dir().join(format!("cortex_cli_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/broken.ax");
    std::fs::copy(src, ws.join("broken.ax")).unwrap();
    ws
}

fn repair(ws: &Path, extra: &[&str]) -> (i32, String) {
    repair_with(ws, &["--check", "hidden_completion"], extra)
}

/// `repair` with the adjudicating check spelled by the caller, for the rows
/// whose subject IS which check was named.
fn repair_with(ws: &Path, check: &[&str], extra: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(ws)
        .args(["--file", "broken.ax", "--symbol", "double"])
        .args(check)
        .arg("--axon")
        .arg(axon_bin())
        .args(extra)
        .output()
        .expect("run cortex");
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

#[test]
fn cli_exit_zero_means_a_hidden_check_accepted_the_repair() {
    let ws = workspace("ok");
    let (code, text) = repair(
        &ws,
        &[
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:\n    n * 2\n",
        ],
    );
    assert_eq!(code, 0, "a correct repair must exit 0: {text}");
    // Exit 0 is a claim ABOUT the file. Checking the file keeps it from being
    // satisfied by a loop that reports success without changing anything.
    let after = std::fs::read_to_string(ws.join("broken.ax")).unwrap();
    assert!(
        after.contains("n * 2") && !after.contains("n + 2"),
        "exit 0 must mean the file was actually repaired: {after}"
    );
}

#[test]
fn cli_a_wrong_repair_never_exits_zero() {
    // The control for the row above. A body that compiles and is still wrong
    // must not be rounded up to success — the failure mode that looks exactly
    // like the success case from the outside.
    let ws = workspace("wrong");
    let (code, text) = repair(
        &ws,
        &[
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:\n    n + 3\n",
        ],
    );
    assert_ne!(code, 0, "a wrong repair must not exit 0: {text}");
}

#[test]
fn cli_each_failure_mode_has_its_own_exit_code() {
    // Distinct codes because the remedies are different. A single non-zero
    // would tell an operator that something went wrong and nothing about
    // whether to widen a grant, supply a generator, or fix their PATH.

    // 23 — authority. No --write-prefix means nothing may be written, and that
    // is decided BEFORE any body is generated: an edit that could never apply
    // must not spend a model call, and must not then be reported as missing
    // content when what is missing is permission.
    let ws = workspace("refused");
    let (code, text) = repair(&ws, &["--generator", "literal:\n    n * 2\n"]);
    assert_eq!(code, 23, "an ungranted write must exit 23: {text}");
    assert!(
        std::fs::read_to_string(ws.join("broken.ax"))
            .unwrap()
            .contains("n + 2"),
        "a refused repair must leave the file alone"
    );

    // 24 — content. Authority is fine; there is no generator.
    let ws = workspace("needs");
    let (code, text) = repair(&ws, &["--write-prefix", "broken.ax"]);
    assert_eq!(code, 24, "a missing generator must exit 24: {text}");

    // 22 — environment. The checker cannot run, so the observation is Unknown
    // and the loop must block carrying that reason rather than guessing.
    let ws = workspace("blocked");
    let (code, text) = repair(
        &ws,
        &["--write-prefix", "broken.ax", "--axon", "no-such-binary"],
    );
    assert_eq!(code, 22, "an unrunnable checker must exit 22: {text}");

    // 21 — futility. The proposal is byte-identical to what is there, so the
    // loop goes in circles; that is not a budget problem and must not be
    // reported as one.
    let ws = workspace("circles");
    let (code, text) = repair(
        &ws,
        &[
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:\n    n + 2\n",
        ],
    );
    assert_eq!(code, 21, "a loop going in circles must exit 21: {text}");
}

#[test]
fn cli_a_malformed_request_decides_nothing() {
    // Exit 2 with no outcome. A typo must not be reported as a repair verdict:
    // nothing ran, so there is nothing to say about the code.
    for args in [
        vec!["repair", "--file", "x.ax"],                  // no --symbol
        vec!["repair", "--file", "x.ax", "--symbol", "f"], // no --check
        vec!["repair", "--nonsense"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?} must be a usage error, not an outcome"
        );
        let text = String::from_utf8_lossy(&out.stdout);
        for verdict in ["verified_done", "no_progress", "needs_input"] {
            assert!(
                !text.contains(verdict),
                "a usage error must not print an outcome: {text}"
            );
        }
    }

    // A check that does not exist is a REQUEST error, caught before the loop
    // starts. It cannot produce a false success — a filter matching nothing
    // reports zero tests and the runner refuses to call that a pass — but left
    // to run it produces something worse to debug: the claim is refused every
    // time and a mistyped flag comes back as exit 21, "going in circles". The
    // loop would be reporting accurately on an experiment that could never
    // have concluded.
    let ws_typo = workspace("typo");
    let (code, text) = repair_with(
        &ws_typo,
        &["--check", "hidden_completon"],
        &["--write-prefix", "broken.ax"],
    );
    assert_eq!(code, 2, "a nonexistent check is a usage error: {text}");
    assert!(
        text.contains("hidden_completon"),
        "the error must name the check that does not exist: {text}"
    );

    // AND a name that IS a real function but not a test. This is the case a
    // cheaper `fn NAME(` grep could never catch — it passed the grep and was
    // caught downstream, so the two guards produced the same exit for the same
    // input and neither could be tested apart from the other. The grep is
    // gone; this is what the remaining check is for.
    let ws_fn = workspace("nontest_check");
    let (code_fn, text_fn) = repair_with(
        &ws_fn,
        &["--check", "main"],
        &["--write-prefix", "broken.ax"],
    );
    assert_eq!(
        code_fn, 2,
        "a function that is not a test cannot adjudicate: {text_fn}"
    );
    assert!(
        text_fn.contains("must name an @[test]"),
        "and the refusal must say why a mere function is not an adjudicator: {text_fn}"
    );

    // An unknown generator is a REQUEST error too, caught before the loop
    // starts. Discovering it three steps in would report a typo as NeedsInput
    // — a verdict about the task rather than about the command line.
    let ws = workspace("badgen");
    let (code, _) = repair(&ws, &["--generator", "gpt-9"]);
    assert_eq!(code, 2, "an unknown generator spec is a usage error");
}

#[test]
fn cli_localizes_its_own_target_and_refuses_to_guess() {
    // `--symbol` asked the operator to do the interesting half: decide what is
    // broken. Without it Cortex localizes from the failing checks — and the
    // refusal rows matter more than the success one, because a wrong target
    // fails every attempt without ever saying why.

    // 1. One failing check naming one function: repaired without being told.
    let ws = workspace("loc_ok");
    let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws)
        .args([
            "--file",
            "broken.ax",
            "--check",
            "hidden_completion",
            "--axon",
        ])
        .arg(axon_bin())
        .args([
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:\n    n * 2\n",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // It shows its working. A target arrived at silently cannot be told from a
    // guess by anyone reading the log afterwards.
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("double") && err.contains("visible_repro"),
        "the localization must name the target AND its evidence: {err}"
    );

    // 2. Ambiguity is WALKED, not refused. This row asserted exit 25 when a
    //    tie meant "give up"; measuring the ranking on the real corpus showed
    //    the true function is at rank 2 in 17 of the 34 cases where rank 1 is
    //    wrong, so refusing a tie discarded cases the evidence could decide.
    //    The tied candidates are tried in order, and the run says so.
    let ws2 = std::env::temp_dir().join(format!("cortex_cli_amb_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws2);
    std::fs::create_dir_all(&ws2).unwrap();
    std::fs::write(
        ws2.join("two.ax"),
        // `beta` is the broken one and `alpha` is correct. Both are reached
        // by the failing check so they tie, and the ranking breaks ties
        // alphabetically — which puts `alpha` first. That makes `beta` the
        // candidate an explicit `--symbol` must reach and localization would
        // not, which is what row 3 needs in order to test anything.
        "fn alpha(n: i64) -> i64 { n * 2 }\n\
         fn beta(n: i64) -> i64 { n }\n\
         @[test]\n\
         fn t() { assert_eq(alpha(1) + beta(1), 4) }\n\
         @[test]\n\
         fn hidden() { assert_eq(beta(1), 2) assert_eq(alpha(1), 2) }\n\
         fn main() { println(to_str(alpha(1))) }\n",
    )
    .unwrap();
    let out2 = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws2)
        .args(["--file", "two.ax", "--check", "hidden", "--axon"])
        .arg(axon_bin())
        .args(["--write-prefix", "two.ax"])
        .output()
        .unwrap();
    let err2 = String::from_utf8_lossy(&out2.stderr);
    assert!(
        err2.contains("alpha") && err2.contains("beta"),
        "a tie must be reported as an ORDERED plan, not a refusal: {err2}"
    );
    // And the plan is CARRIED OUT. The line above is printed before the
    // attempt loop, so asserting only it left this row green with the walk
    // deleted — it pinned that a plan was announced, not that anything
    // happened. The hand-off line is printed only by a second attempt.
    assert!(
        err2.contains("did not repair it; trying"),
        "the second candidate must actually be attempted: {err2}"
    );

    // 2b. Exit 25 now means what it says: no candidate AT ALL. Note `hidden`
    //     must itself FAIL here: a check that already passes is refused before
    //     this point, because it would accept the file unchanged. A failing check
    //     that reaches no function defined in the file has nothing to offer,
    //     and that is a different statement from "several, and I cannot
    //     choose" — which is now an ordered plan rather than a dead end.
    std::fs::write(
        ws2.join("none.ax"),
        "@[test]\n\
         fn t() { assert_eq(1, 2) }\n\
         @[test]\n\
         fn hidden() { assert_eq(1, 2) }\n\
         fn main() { println(to_str(1)) }\n",
    )
    .unwrap();
    let out2b = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws2)
        .args(["--file", "none.ax", "--check", "hidden", "--axon"])
        .arg(axon_bin())
        .args(["--write-prefix", "none.ax"])
        .output()
        .unwrap();
    assert_eq!(
        out2b.status.code(),
        Some(25),
        "a failing check reaching no defined function must exit 25: {}",
        String::from_utf8_lossy(&out2b.stderr)
    );

    // 3. An explicit --symbol is an INSTRUCTION, not a hypothesis to
    //    second-guess. Note `t` expects 3, not 4: the repair below has to
    //    leave the WHOLE file passing, because a run that fixes the
    //    adjudicator and leaves another check failing is not a repair: the same ambiguous file repairs fine when told which.
    //    Without this row, "refuses when ambiguous" is satisfied by a build
    //    that refuses always.
    let out3 = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws2)
        // `beta`, NOT `alpha`. The two tie at 1.0 and the ranking breaks ties
        // alphabetically, so naming `alpha` is naming what localization would
        // have picked anyway — the row passed with the `--symbol` branch
        // deleted entirely. Naming the OTHER one is what makes it a test of
        // the instruction being honoured.
        .args(["--file", "two.ax", "--symbol", "beta", "--check", "hidden"])
        .arg("--axon")
        .arg(axon_bin())
        .args([
            "--write-prefix",
            "two.ax",
            "--generator",
            "literal:\n    n + 1\n",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out3.status.code(),
        Some(0),
        "being told the target must bypass localization: {}",
        String::from_utf8_lossy(&out3.stderr)
    );
}

#[test]
fn cli_json_reports_the_outcome_and_the_evidence_behind_it() {
    let ws = workspace("json");
    let (code, text) = repair(
        &ws,
        &[
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:\n    n * 2\n",
            "--json",
        ],
    );
    assert_eq!(code, 0, "{text}");
    let v: serde_json::Value = serde_json::from_str(text.trim()).expect("valid JSON");
    assert_eq!(v["schema"], "cortex-repair/1");
    assert_eq!(v["outcome"], "verified_done");
    // The exit code appears in the payload AND is the process's exit code. A
    // reader that trusts one must not be able to disagree with a reader that
    // trusts the other.
    assert_eq!(v["exit_code"], 0);
    // The verdict is a summary OF the episode, not a substitute for it: the
    // patch and its attribution have to be there to be audited.
    //
    // Read as STRUCTURE, not as a substring of a Debug dump. The episode used
    // to ship as `format!("{:?}", …)` inside a JSON string, and an assertion
    // that greps that string passes for any record merely MENTIONING the
    // words — including one where they appear in a reason or a path.
    let events = v["episode"]["events"]
        .as_array()
        .expect("the episode ships as data, not as a Debug string");
    assert!(
        events
            .iter()
            .any(|e| e.get("kind").and_then(|k| k.as_str()) == Some("patch_applied")),
        "the record must show the patch: {events:?}"
    );
    assert!(
        events.iter().any(|e| e
            .get("name")
            .and_then(|n| n.as_str())
            .is_some_and(|n| n.contains("literal@1"))),
        "and who proposed it: {events:?}"
    );
}

/// C16 — the run walks the ranked candidates, and each attempt starts clean.
///
/// On the measured corpus the top candidate is the sole most-suspicious one
/// far less often than the truth reaches the top three, so stopping at the
/// first discards cases the evidence could already decide. Current figures
/// live in benchmarks/; they are not quoted here because a number in a doc
/// comment outlives the measurement it came from.
///
/// `ambiguous.ax` is built so that stopping at one cannot pass: `alpha` and
/// `beta` tie exactly, `beta` is the broken one, and it sorts second. The
/// fixture also sets the trap that makes the isolation row necessary —
/// patching `alpha` makes the VISIBLE check pass while the program stays
/// wrong, so a loop grading itself on the evidence it can see would stop there
/// and report success.
#[test]
fn cli_walks_the_ranking_and_isolates_each_attempt() {
    let ws = std::env::temp_dir().join(format!("cortex_cli_walk_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ambiguous.ax");
    let before = std::fs::read_to_string(&src).unwrap();
    std::fs::copy(&src, ws.join("ambiguous.ax")).unwrap();

    let run = |extra: &[&str]| -> (i32, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
            .args(["repair", "--workspace"])
            .arg(&ws)
            .args([
                "--file",
                "ambiguous.ax",
                "--check",
                "hidden_completion",
                "--axon",
            ])
            .arg(axon_bin())
            .args(["--write-prefix", "ambiguous.ax"])
            .args(extra)
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };

    let (code, out, err) = run(&["--generator", "literal:\n    n + 2\n"]);
    assert_eq!(
        code, 0,
        "walking to the second candidate must succeed: {err}{out}"
    );
    assert!(
        err.contains("`alpha` did not repair it; trying `beta`"),
        "the run must say which candidate it abandoned and why it moved on: {err}"
    );
    assert!(
        out.contains("repaired `beta`"),
        "with a walked ranking, WHICH candidate worked is no longer implied by \
         the command line: {out}"
    );

    // ATTEMPT ISOLATION. `alpha` was patched, that patch made the visible
    // check pass, and it is gone. Without the restore, `beta` would have been
    // adjudicated against a file the first attempt had already altered, and
    // its success would not mean what it says.
    let after = std::fs::read_to_string(ws.join("ambiguous.ax")).unwrap();
    assert!(
        after.contains("n * 2"),
        "a failed attempt must leave no trace: alpha was not restored\n{after}"
    );
    // Exactly one function differs. Compared on the function BODIES rather
    // than by a whole-file string replace, which also rewrote the fixture's
    // own comments and made this assertion fail for a reason that had nothing
    // to do with the code under test.
    let body = |src: &str, name: &str| -> String {
        let at = src
            .find(&format!("fn {name}("))
            .expect("the function exists");
        let open = at + src[at..].find('{').unwrap();
        let close = open + src[open..].find('}').unwrap();
        src[open + 1..close].trim().to_string()
    };
    assert_eq!(
        body(&after, "alpha"),
        body(&before, "alpha"),
        "alpha must be untouched"
    );
    assert_eq!(
        body(&after, "beta"),
        "n + 2",
        "beta must be the one that changed"
    );

    // THE CONTROL. Confined to one candidate, the same run fails — so the row
    // above is about the walk, not about a fixture that any build repairs.
    std::fs::copy(&src, ws.join("ambiguous.ax")).unwrap();
    let (code1, _, _) = run(&["--generator", "literal:\n    n + 2\n", "--candidates", "1"]);
    assert_ne!(
        code1, 0,
        "one candidate cannot reach the broken function here"
    );
    // And it left the workspace as it found it: a run that reports failure
    // having rewritten a function is reporting on a workspace nobody asked for.
    assert_eq!(
        std::fs::read_to_string(ws.join("ambiguous.ax")).unwrap(),
        before,
        "a failed run must restore the file"
    );
}

/// C17 — `cortex locate` answers "what is broken?" without repairing anything.
#[test]
fn cli_locate_reports_the_ranking_with_its_scores() {
    let ws = workspace("locate_json");
    let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["locate", "--workspace"])
        .arg(&ws)
        .args([
            "--file",
            "broken.ax",
            "--check",
            "hidden_completion",
            "--axon",
        ])
        .arg(axon_bin())
        .arg("--json")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(text.trim()).expect("valid JSON");
    assert_eq!(v["schema"], "cortex-locate/1");
    assert_eq!(v["ranked"][0]["symbol"], "double");
    // The SCORES, not just the order. The spread between first and second is
    // what says whether the evidence decided anything, and a ranking reported
    // without it cannot be scored for its own quality.
    assert!(v["ranked"][0]["score"].as_f64().unwrap() > 0.0);
    // The hidden check is absent from both spectra — localizing from the
    // grader would make the target a function of the answer.
    let spectra = format!("{}{}", v["failing"], v["passing"]);
    assert!(
        !spectra.contains("hidden_completion"),
        "the adjudicating check must not appear in the evidence: {spectra}"
    );
    assert!(spectra.contains("visible_repro"));
}

/// C18 — the grader is not a repair target, and an unwitnessable check is not
/// a pass.
///
/// Both rows are false successes found by measuring against real code, and
/// both are the same collapse wearing different clothes: something that was
/// never established being reported as established.
#[test]
fn cli_refuses_to_grade_a_repair_against_bytes_the_repair_wrote() {
    // 1. Naming the adjudicator as the target rewrote it to a tautology, left
    //    the defect untouched, and exited 0 — `verified_done — repaired
    //    hidden_completion`. That is the crate's headline claim exactly
    //    inverted.
    let ws = workspace("grader");
    let before = std::fs::read_to_string(ws.join("broken.ax")).unwrap();
    let (code, text) = repair(
        &ws,
        &[
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:\n    assert(true)\n",
            "--symbol",
            "hidden_completion",
        ],
    );
    assert_ne!(code, 0, "patching the grader must never verify: {text}");
    assert_eq!(
        std::fs::read_to_string(ws.join("broken.ax")).unwrap(),
        before,
        "the adjudicating check must be byte-identical afterwards"
    );

    // 2. A check that ALREADY PASSES cannot witness a repair — it would accept
    //    the file unchanged. Measured: 54 of 80 oracle runs reported
    //    verified_done, most at step 1, having changed nothing, because the
    //    named check never exercised the broken function.
    let ws2 = workspace("unwitnessable");
    let before2 = std::fs::read_to_string(ws2.join("broken.ax")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws2)
        .args(["--file", "broken.ax", "--symbol", "double"])
        // `clean_helper` passes on the broken file: it says nothing about
        // `double`.
        .args(["--check", "clean_helper", "--axon"])
        .arg(axon_bin())
        .args([
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:\n    n * 2\n",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(26),
        "a check that already passes must be refused as an adjudicator: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(ws2.join("broken.ax")).unwrap(),
        before2,
        "nothing may be written when no adjudication is possible"
    );

    // 3. A symbol that does not exist is its own answer. It used to come back
    //    as exit 22 "the checker could not run" — sending an operator to debug
    //    a toolchain that was fine.
    let ws3 = workspace("nosuch");
    let (code3, text3) = repair(
        &ws3,
        &[
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:\n    n * 2\n",
            "--symbol",
            "no_such_function",
        ],
    );
    assert_eq!(code3, 25, "a missing symbol must exit 25: {text3}");
}

/// C19 — a file that compiles WITH WARNINGS can still be repaired.
///
/// This is the single largest defect the corpus found. Every fixture here
/// compiled clean, which is not what real Axon code does — most of
/// `examples/stdlib/*.ax` emits at least one warning — and the difference is
/// not cosmetic.
///
/// A clean file makes selection claim completion; the claim is refused, and
/// THAT refusal is what drives it to propose a patch. A warned file made
/// selection run a check instead, and running a check changes no bytes, so the
/// next step saw the same workspace and the same choice and stopped as
/// `NoProgress` — before ever reaching a patch. Nine of the corpus trials
/// failed this way, every one of them with the right function ranked first or
/// second.
#[test]
fn cli_repairs_a_file_that_compiles_with_warnings() {
    let ws = std::env::temp_dir().join(format!("cortex_cli_warned_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/warned_broken.ax");
    std::fs::copy(&src, ws.join("warned_broken.ax")).unwrap();

    // Precondition: it really does warn. Without this the row silently becomes
    // a duplicate of the clean-file test the moment the fixture stops warning
    // — the guard testing nothing while still passing.
    let check = Command::new(axon_bin())
        .arg("check")
        .arg(ws.join("warned_broken.ax"))
        .output()
        .unwrap();
    let diag = String::from_utf8_lossy(&check.stdout).to_string()
        + &String::from_utf8_lossy(&check.stderr);
    assert!(
        diag.contains("\"severity\":\"warning\""),
        "the fixture must COMPILE WITH A WARNING, or this row tests the clean \
         path over again: {diag}"
    );
    assert!(check.status.success(), "and it must still compile");

    let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws)
        .args([
            "--file",
            "warned_broken.ax",
            "--check",
            "hidden_scale",
            "--axon",
        ])
        .arg(axon_bin())
        .args([
            "--write-prefix",
            "warned_broken.ax",
            "--generator",
            "literal:\n    n * 2\n",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "a warned file must still be repairable: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // Asked of the FILE. The exit code is the claim under test.
    let after = std::fs::read_to_string(ws.join("warned_broken.ax")).unwrap();
    assert!(
        after.contains("n * 2"),
        "the repair must actually be in the file: {after}"
    );

    // AND when the symbol's name matches NO test name.
    //
    // The first version of this fix only worked when the visible check —
    // which is filtered by the SYMBOL's name — happened to match something.
    // For 421 of the 580 candidate symbols in `examples/**.ax` (72.6%) it
    // matches nothing, and "matched nothing" rendered as the same value the
    // episode started with, so the loop chose the same action against an
    // unchanged workspace and stalled exactly as before.
    //
    // `helper_alpha` is covered by `visible_case` and `hidden_case`, neither
    // of which contains its name.
    std::fs::write(
        ws.join("nomatch.ax"),
        "fn helper_alpha(n: i64) -> i64 {\n\
         \x20   let n = n + 0\n\
         \x20   n + 3\n\
         }\n\
         @[test]\n\
         fn visible_case() { assert_eq(helper_alpha(2), 4) }\n\
         @[test]\n\
         fn hidden_case() { assert_eq(helper_alpha(5), 10) }\n\
         fn main() { println(to_str(helper_alpha(2))) }\n",
    )
    .unwrap();
    let out2 = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws)
        .args(["--file", "nomatch.ax", "--check", "hidden_case", "--axon"])
        .arg(axon_bin())
        .args([
            "--write-prefix",
            "nomatch.ax",
            "--generator",
            "literal:\n    n * 2\n",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out2.status.code(),
        Some(0),
        "a symbol matching no test name must still be repairable: {}{}",
        String::from_utf8_lossy(&out2.stdout),
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(std::fs::read_to_string(ws.join("nomatch.ax"))
        .unwrap()
        .contains("n * 2"));
}

/// C20 — the operator's own program as the generator.
///
/// `cmd:PATH` writes the prompt to a program's stdin and reads the proposed
/// body from its stdout. It exists so Cortex can be driven by whatever model
/// the operator already has, with no credentials in this process and no
/// provider baked into the crate.
///
/// It also makes one claim testable end to end for the first time: what the
/// generator is SHOWN. Until now "the generator never sees the grader" was
/// asserted against a struct field; here it is checked against the bytes a
/// real external process received.
#[test]
fn cli_drives_an_external_generator_and_shows_it_no_grader() {
    let ws = std::env::temp_dir().join(format!("cortex_cli_cmd_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    std::fs::copy(fixtures.join("broken.ax"), ws.join("broken.ax")).unwrap();

    let script = |name: &str, body: &str| -> std::path::PathBuf {
        let p = ws.join(name);
        std::fs::write(&p, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    };
    let run = |gen: &str| -> (i32, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
            .args(["repair", "--workspace"])
            .arg(&ws)
            .args([
                "--file",
                "broken.ax",
                "--check",
                "hidden_completion",
                "--axon",
            ])
            .arg(axon_bin())
            .args(["--write-prefix", "broken.ax", "--generator", gen])
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };

    // 1. A program that proposes the right body repairs the file, and the
    //    prompt it received is captured for inspection.
    let good = script(
        "good.sh",
        "#!/bin/sh\ncat > \"$(dirname \"$0\")/prompt.txt\"\nprintf '\\n    n * 2\\n'\n",
    );
    let (code, out, err) = run(&format!("cmd:{}", good.display()));
    assert_eq!(
        code, 0,
        "an external generator must be able to repair: {err}{out}"
    );
    assert!(std::fs::read_to_string(ws.join("broken.ax"))
        .unwrap()
        .contains("n * 2"));

    // WHAT IT WAS SHOWN. The prompt carries the body under repair and the
    // observed facts — and nothing from the check that grades the result. A
    // generator that could read the grader would be writing against it, and
    // passing it would stop being evidence of anything.
    let prompt = std::fs::read_to_string(ws.join("prompt.txt")).expect("the prompt reached it");
    assert!(
        prompt.contains("n + 2") && prompt.contains("double"),
        "the generator must be shown the body it is replacing: {prompt}"
    );
    for leaked in ["hidden_completion", "double(7)", "double(0)", "14"] {
        assert!(
            !prompt.contains(leaked),
            "the prompt leaked `{leaked}` from the adjudicating check:\n{prompt}"
        );
    }
    // And the episode records WHICH program produced the patch.
    assert!(
        out.contains("repaired"),
        "the run must name what it repaired: {out}"
    );

    // 2. A program that RAN and refused is Declined — the remedy is the task
    //    or the prompt, not the installation.
    std::fs::copy(fixtures.join("broken.ax"), ws.join("broken.ax")).unwrap();
    let refuses = script(
        "no.sh",
        "#!/bin/sh\ncat >/dev/null\necho 'not today' >&2\nexit 3\n",
    );
    let (code2, out2, err2) = run(&format!("cmd:{}", refuses.display()));
    assert_eq!(code2, 24, "a refusing generator is missing CONTENT: {err2}");
    let said2 = format!("{out2}{err2}");
    assert!(
        said2.contains("declined") && said2.contains("exited 3"),
        "the refusal must carry the program's own exit status: {said2}"
    );

    // 3. A program that cannot be STARTED is Unavailable — an infrastructure
    //    fact about the operator's setup, not a statement about the task. The
    //    distinction is the same one a missing API key gets.
    let (code3, out3, err3) = run("cmd:/nonexistent/generator");
    assert_eq!(code3, 24, "{err3}");
    let said3 = format!("{out3}{err3}");
    assert!(
        said3.contains("unavailable") && said3.contains("cannot run"),
        "an unstartable generator must not read as a refusal: {said3}"
    );

    // 4. The file is untouched by either failure.
    assert!(
        std::fs::read_to_string(ws.join("broken.ax"))
            .unwrap()
            .contains("n + 2"),
        "a failed generation must leave the workspace alone"
    );
}

/// C21 — a patch lands on the symbol that was SELECTED, not on one whose name
/// it is a prefix of.
///
/// The body lookup searched for `fn NAME` without the opening paren, so
/// selecting `fib` found `fib_rec` defined earlier in the file. Every caller
/// was affected: Inspect read the wrong body, the generator was SHOWN the
/// wrong body, and the patch overwrote the wrong function — so the selected
/// symbol was unrepairable by construction and a working one took the damage.
///
/// Corpus-reachable: 5 collisions across 4 files in `examples/`
/// (`fib`/`fib_rec`, `belief_map`/`belief_map_index`, `approx_eq`/`approx_eq_b`).
/// It also re-opened the grader-not-patchable guard, which compares the symbol
/// by exact name while the write resolved by prefix.
#[test]
fn cli_patches_the_selected_symbol_not_a_name_it_prefixes() {
    let ws = std::env::temp_dir().join(format!("cortex_cli_prefix_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    // `scale_twice` is defined FIRST and its name begins with `scale`, so a
    // prefix search for `fn scale` finds it rather than `scale`.
    std::fs::write(
        ws.join("p.ax"),
        "fn scale_twice(n: i64) -> i64 { n * 4 }\n\
         fn scale(n: i64) -> i64 { n + 3 }\n\
         @[test]\n\
         fn visible() { assert_eq(scale(2), 4) }\n\
         @[test]\n\
         fn hidden_c() { assert_eq(scale(5), 10) assert_eq(scale_twice(2), 8) }\n\
         fn main() { println(to_str(scale(2))) }\n",
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws)
        .args([
            "--file", "p.ax", "--symbol", "scale", "--check", "hidden_c", "--axon",
        ])
        .arg(axon_bin())
        .args(["--write-prefix", "p.ax", "--generator", "literal: n * 2 "])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "`scale` must be repairable even though `scale_twice` is defined first: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let after = std::fs::read_to_string(ws.join("p.ax")).unwrap();
    // The neighbour is UNTOUCHED. Asserting only that the file passes would
    // miss the case where both were rewritten.
    assert!(
        after.contains("fn scale_twice(n: i64) -> i64 { n * 4 }"),
        "the earlier-defined neighbour must be byte-identical:\n{after}"
    );
    assert!(
        after.contains("fn scale(n: i64) -> i64 { n * 2 }"),
        "the selected symbol must be the one that changed:\n{after}"
    );
}

/// C22 — a misbehaving generator fails the loop cleanly instead of stopping it.
///
/// `cmd:` runs a program the operator supplies, so the loop has to survive
/// every way a program can misbehave. Three of these hung or aborted before:
/// one that never exits blocked forever (no budget applies to a child
/// process, and `--budget` counts steps, not seconds); one that writes before
/// reading its stdin deadlocked on a full pipe buffer; and one that writes
/// without bound was read into memory entirely, because the size limit is
/// applied to what gets APPLIED and never to what gets buffered.
#[test]
fn cli_survives_a_generator_that_misbehaves() {
    // The name here must not collide with any `workspace(...)` name above.
    // It did: the same directory was built by two tests, they run in parallel,
    // and the other one's `remove_dir_all` deleted these scripts mid-run —
    // which presented as a generator that "did not exist". Two actors, one
    // directory.
    let ws = std::env::temp_dir().join(format!("cortex_cli_misbehave_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");

    let script = |name: &str, body: &str| -> std::path::PathBuf {
        let p = ws.join(name);
        std::fs::write(&p, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    };
    let run = |gen: &std::path::PathBuf| -> (i32, String) {
        std::fs::copy(fixtures.join("broken.ax"), ws.join("broken.ax")).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
            .args(["repair", "--workspace"])
            .arg(&ws)
            .args([
                "--file",
                "broken.ax",
                "--check",
                "hidden_completion",
                "--axon",
            ])
            .arg(axon_bin())
            .args(["--write-prefix", "broken.ax", "--generator"])
            .arg(format!("cmd:{}", gen.display()))
            // Seconds are not something a suite can wait for, and an untested
            // deadline is exactly the kind of check that turns out never to
            // fire.
            .env("AXON_CORTEX_GENERATOR_TIMEOUT_MS", "700")
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    };

    // 1. Never exits. Must hit the deadline, not block.
    let began = std::time::Instant::now();
    let (code, said) = run(&script("hang.sh", "#!/bin/sh\nsleep 600\n"));
    assert_eq!(
        code, 24,
        "a generator that never answers is missing CONTENT: {said}"
    );
    assert!(said.contains("did not answer within"), "{said}");
    assert!(
        began.elapsed() < std::time::Duration::from_secs(30),
        "the deadline must actually bound the wait, took {:?}",
        began.elapsed()
    );

    // 2. Writes 200 KB BEFORE reading its stdin — more than a pipe buffer, so
    //    a loop that writes the whole prompt before reading deadlocks. It must
    //    be read and then rejected on its merits, not time out.
    let began2 = std::time::Instant::now();
    let (code2, said2) = run(&script(
        "flood.sh",
        "#!/bin/sh\nhead -c 200000 /dev/zero | tr '\\0' 'x'\ncat >/dev/null\n",
    ));
    assert_eq!(code2, 24, "{said2}");
    assert!(
        said2.contains("bytes, limit is"),
        "it must be rejected for its SIZE, not for timing out: {said2}"
    );
    assert!(
        began2.elapsed() < std::time::Duration::from_secs(10),
        "no deadlock: {:?}",
        began2.elapsed()
    );

    // 3. Exits 0 having written nothing. An empty body would delete the
    //    function while looking like a proposal.
    let (code3, said3) = run(&script("silent.sh", "#!/bin/sh\ncat >/dev/null\n"));
    assert_eq!(code3, 24, "{said3}");
    assert!(said3.contains("empty body"), "{said3}");

    // 4. Whatever happened, the workspace is as it was found.
    assert!(
        std::fs::read_to_string(ws.join("broken.ax"))
            .unwrap()
            .contains("n + 2"),
        "a misbehaving generator must not leave the file altered"
    );
}

/// C23 — the two guards that only a hostile environment exercises.
///
/// Both survived a mutation pass because nothing reached them. They are the
/// kind of guard that matters precisely when something else has already gone
/// wrong, so the test has to manufacture that wrongness.
#[test]
fn cli_fails_closed_when_the_recheck_cannot_run() {
    let ws = std::env::temp_dir().join(format!("cortex_cli_hostile_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    std::fs::copy(fixtures.join("broken.ax"), ws.join("broken.ax")).unwrap();

    let exe = |name: &str, body: String| -> std::path::PathBuf {
        let p = ws.join(name);
        std::fs::write(&p, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    };

    // 1. THE RE-CHECK FAILS, AND ONLY IT.
    //
    // After the adjudicator passes, the loop re-runs the WHOLE file to confirm
    // nothing else fails — the only `axon test` call made without `--filter`.
    // That asymmetry is what makes this testable: a wrapper that dies when no
    // filter is given leaves every other call working.
    //
    // This guard replaced an `unwrap_or_default()`, which turned "the checks
    // could not be run" into "nothing is failing" on the ONE path that can
    // return success. It must fail CLOSED.
    let fake_axon = exe(
        "axon-wrapper.sh",
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = \"--filter\" ] && exec {real} \"$@\"; done\n\
             case \"$1\" in test) exit 2 ;; esac\nexec {real} \"$@\"\n",
            real = axon_bin().display()
        ),
    );
    let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws)
        .args([
            "--file",
            "broken.ax",
            "--symbol",
            "double",
            "--check",
            "hidden_completion",
        ])
        .arg("--axon")
        .arg(&fake_axon)
        .args([
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:\n    n * 2\n",
        ])
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(
        out.status.code(),
        Some(0),
        "a re-check that could not run must NEVER read as a clean file: {said}"
    );
    assert!(
        said.contains("could not be re-checked") || said.contains("blocked"),
        "and it must say what could not be established: {said}"
    );

    // 2. A GENERATOR THAT WRITES WITHOUT BOUND IS CUT OFF, not buffered.
    //
    // The size limit in `validate` applies to what gets APPLIED, never to what
    // gets READ — so without a read bound a child writing gigabytes is
    // buffered entirely and the process dies on allocation, taking the episode
    // record with it.
    //
    // The observable: cut off, the child dies on a closed pipe and is reported
    // by its EXIT STATUS. Read in full, it exits cleanly and is rejected for
    // its size. So the absence of a size complaint is the evidence.
    std::fs::copy(fixtures.join("broken.ax"), ws.join("broken.ax")).unwrap();
    let flood = exe(
        "gush.sh",
        "#!/bin/sh\ncat >/dev/null\nhead -c 3000000 /dev/zero | tr '\\0' 'y'\n".to_string(),
    );
    let out2 = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws)
        .args([
            "--file",
            "broken.ax",
            "--check",
            "hidden_completion",
            "--axon",
        ])
        .arg(axon_bin())
        .args(["--write-prefix", "broken.ax", "--generator"])
        .arg(format!("cmd:{}", flood.display()))
        .output()
        .unwrap();
    let said2 = format!(
        "{}{}",
        String::from_utf8_lossy(&out2.stdout),
        String::from_utf8_lossy(&out2.stderr)
    );
    assert_eq!(out2.status.code(), Some(24), "{said2}");
    assert!(
        !said2.contains("limit is"),
        "3MB must be CUT OFF at the read, not buffered and then measured: {said2}"
    );
}

/// C24 — on the warned-file path too, every proposal replaces the SAME body.
///
/// A file that compiles with warnings takes a different route through
/// selection: check → patch → check → patch, never reaching a completion
/// claim. The undo lived on the claim path only, so on that route — the
/// majority of real files — patches accumulated and the generator was shown
/// its own rejected proposal as "the body currently" while simultaneously
/// being told that body had been rejected.
///
/// The generator below refuses to answer unless it is shown the body it
/// started from, which is the only way to observe the difference: with the
/// undo it converges, without it the second call is handed the wrong body and
/// declines.
#[test]
fn cli_shows_the_generator_the_same_body_on_the_warned_path() {
    let ws = std::env::temp_dir().join(format!("cortex_cli_samebody_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/warned_broken.ax");
    std::fs::copy(&src, ws.join("warned_broken.ax")).unwrap();

    // Proposes a wrong body first, so a SECOND call must happen; that second
    // call is where the difference shows. It exits non-zero if the body it is
    // shown is not the one it started from.
    //
    // Both proposals KEEP the `let n = n + 0` line. Without that the first
    // patch removes the very thing that makes the file warn, the episode
    // switches to the claim path, and the claim-path undo — a different
    // mechanism — quietly does the work. An earlier version of this test did
    // exactly that and passed with the undo under test deleted.
    let gen = ws.join("picky.sh");
    std::fs::write(
        &gen,
        "#!/bin/sh\n\
         p=$(cat)\n\
         case \"$p\" in *'n + 3'*) ;; *) echo 'not the body I started from' >&2; exit 1 ;; esac\n\
         case \"$p\" in *'Already tried'*) printf '\\n    let n = n + 0\\n    n * 2\\n' ;; \
                        *) printf '\\n    let n = n + 0\\n    n + 4\\n' ;; esac\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gen, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws)
        .args([
            "--file",
            "warned_broken.ax",
            "--check",
            "hidden_scale",
            "--axon",
        ])
        .arg(axon_bin())
        .args([
            "--write-prefix",
            "warned_broken.ax",
            "--budget",
            "12",
            "--generator",
        ])
        .arg(format!("cmd:{}", gen.display()))
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "the second proposal must be asked about the ORIGINAL body: {said}"
    );
    assert!(std::fs::read_to_string(ws.join("warned_broken.ax"))
        .unwrap()
        .contains("n * 2"));
}

/// C26 — the evidence a consumer receives can be checked by that consumer.
///
/// The episode was shipped as `format!("{:?}", …)` inside a JSON string. So
/// the one artifact a caller gets could not be parsed by this crate's own
/// strict parser, its digest could not be recomputed, and its verdict could
/// not be re-evaluated — while `episode.rs` calls it append-only and
/// replayable "so a replay can prove it re-ran the same episode rather than a
/// similar one".
///
/// This asserts the three things that claim requires, from the outside: it
/// parses, it re-digests to the value shipped beside it, and its own verdict
/// agrees with the exit code.
#[test]
fn cli_ships_evidence_a_reader_can_verify() {
    use axon_cortex::episode::Episode;

    let ws = workspace("evidence");
    let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws)
        .args([
            "--file",
            "broken.ax",
            "--check",
            "hidden_completion",
            "--axon",
        ])
        .arg(axon_bin())
        .args([
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:\n    n * 2\n",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(text.trim()).expect("valid JSON");

    // 1. IT PARSES, through this crate's own strict parser — the one the
    //    Debug-formatted string could never have survived.
    let ep: Episode = axon_cortex::parse_strict(&v["episode"].to_string())
        .expect("the shipped episode must parse as an Episode");
    assert!(
        ep.events.len() >= 4,
        "and it must carry the run's events: {:?}",
        ep.events
    );

    // 2. IT RE-DIGESTS to the value shipped beside it, so a reader can check
    //    the transport rather than trust it.
    assert_eq!(
        ep.digest().expect("digestable"),
        v["episode_digest"].as_str().unwrap_or_default(),
        "the digest must be recomputable from what was shipped"
    );

    // 3. ITS OWN VERDICT AGREES WITH THE EXIT CODE. Two independent readers of
    //    the same run — a script reading the code, an auditor reading the
    //    record — must not be able to disagree.
    assert!(
        ep.verified_ok(),
        "exit 0 must mean the episode itself says a check verified: {:?}",
        ep.events
    );

    // And the negative: a run that does NOT verify must not carry an episode
    // claiming it did.
    let ws2 = workspace("evidence_fail");
    let out2 = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws2)
        .args([
            "--file",
            "broken.ax",
            "--check",
            "hidden_completion",
            "--axon",
        ])
        .arg(axon_bin())
        .args([
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:\n    n + 3\n",
            "--json",
        ])
        .output()
        .unwrap();
    assert_ne!(out2.status.code(), Some(0));
    let v2: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out2.stdout).trim()).expect("valid JSON");
    let ep2: Episode = axon_cortex::parse_strict(&v2["episode"].to_string()).expect("parses");
    assert!(
        !ep2.verified_ok(),
        "a non-zero run must not ship an episode claiming verification"
    );
}

/// C29 — a run that ends mid-repair leaves the workspace as it found it.
///
/// The CLI restores the file when a run exits non-zero. That was believed
/// covered by the candidate-walk test, but it is not: there the abandoned
/// attempt ends after a REFUSED CLAIM, and the episode's own undo has already
/// put the file back — so the assertion held with the CLI restore deleted.
/// The named mechanism was shadowed by a different one.
///
/// The case the CLI restore actually exists for is an episode that stops with
/// a patch still applied. A budget that expires immediately after a patch step
/// does exactly that: measured, with the restore disabled the file keeps the
/// patch at `--budget 2` and `--budget 4`, and is clean at 3 — the parity of
/// where the budget lands decides it, which is why one arbitrary budget is not
/// enough.
#[test]
fn cli_restores_a_workspace_when_a_run_stops_mid_repair() {
    let ws = workspace("midrepair");
    let before = std::fs::read_to_string(ws.join("broken.ax")).unwrap();

    for budget in ["2", "4"] {
        std::fs::write(ws.join("broken.ax"), &before).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
            .args(["repair", "--workspace"])
            .arg(&ws)
            .args(["--file", "broken.ax", "--symbol", "double"])
            .args(["--check", "hidden_completion", "--axon"])
            .arg(axon_bin())
            .args([
                "--write-prefix",
                "broken.ax",
                "--budget",
                budget,
                // Compiles, does not repair — so the episode never reaches a
                // terminal state that keeps its patch deliberately.
                "--generator",
                "literal:\n    n + 7\n",
            ])
            .output()
            .unwrap();
        assert_ne!(
            out.status.code(),
            Some(0),
            "budget {budget} must not verify"
        );
        assert_eq!(
            std::fs::read_to_string(ws.join("broken.ax")).unwrap(),
            before,
            "budget {budget}: a run that stopped mid-repair must leave the file \
             byte-identical, and this is the case the episode's own undo does \
             NOT cover"
        );
    }
}
