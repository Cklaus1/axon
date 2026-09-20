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
        "fn alpha(n: i64) -> i64 { n }\n\
         fn beta(n: i64) -> i64 { n }\n\
         @[test]\n\
         fn t() { assert_eq(alpha(1) + beta(1), 4) }\n\
         @[test]\n\
         fn hidden() { assert_eq(alpha(1), 2) }\n\
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
    //    second-guess: the same ambiguous file repairs fine when told which.
    //    Without this row, "refuses when ambiguous" is satisfied by a build
    //    that refuses always.
    let out3 = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["repair", "--workspace"])
        .arg(&ws2)
        .args(["--file", "two.ax", "--symbol", "alpha", "--check", "hidden"])
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
    let ep = v["episode"].as_str().unwrap();
    assert!(
        ep.contains("PatchApplied") && ep.contains("literal@1"),
        "the record must show the patch and who proposed it: {ep}"
    );
}

/// C16 — the run walks the ranked candidates, and each attempt starts clean.
///
/// Measured on the real corpus, the top-ranked candidate is right 57.5% of the
/// time and the top three cover 90%. Stopping at the first discards a third of
/// the cases the evidence could already decide.
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
