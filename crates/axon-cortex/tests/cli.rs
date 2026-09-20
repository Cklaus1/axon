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
    // Next to the test binary, which is where cargo puts the workspace's
    // binaries. Falls back to the PATH name so a failure reads as "the checker
    // could not run" rather than as a mysterious Blocked.
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    let candidate = p.join("axon");
    if candidate.exists() {
        candidate
    } else {
        PathBuf::from("axon")
    }
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
        err.contains("localized `double`") && err.contains("visible_repro"),
        "the localization must name the target AND its evidence: {err}"
    );

    // 2. Ambiguity is refused with its own exit code — not rounded into the
    //    usage error above (the command line was fine) nor into an episode
    //    outcome below (no episode ran).
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
    assert_eq!(
        out2.status.code(),
        Some(25),
        "an ambiguous target must exit 25: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let err2 = String::from_utf8_lossy(&out2.stderr);
    assert!(
        err2.contains("alpha") && err2.contains("beta"),
        "the refusal must name the candidates it would not choose between: {err2}"
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
