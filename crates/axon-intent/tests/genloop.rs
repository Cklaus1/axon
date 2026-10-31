//! IG4 - the check->repair loop, driven by deterministic fake generators (no
//! network, no key) against the real `axon` checker and the axon-os sandbox.
//! Every test asserts both what the loop returns (a gated program or a
//! refusal) and what its `axon-intent-gen/1` transcript records.

mod common;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;

use axon_intent::generator::{CmdGenerator, Generator, ScriptedGenerator};
use axon_intent::genloop::{self, LoopConfig, LoopOutcome, RefusalClass, Stage};
use axon_intent::intent;
use axon_intent::Refusal;
use std::path::PathBuf;
use std::time::Duration;

fn axon_bin() -> PathBuf {
    script_spawn::workspace_bin(
        "AXON_BIN",
        &[
            "build",
            "-p",
            "axon-core",
            "--no-default-features",
            "--bin",
            "axon",
        ],
        "axon",
    )
}

fn cfg() -> LoopConfig {
    LoopConfig::new(
        axon_bin(),
        "CARD: the Axon language card (test stand-in)".into(),
    )
}

const HEAD: &str = r#"# Intent
Copy ./data/in.txt to ./out/copy.txt unchanged.

## Inputs
- ./data/in.txt

## Outputs
- ./out/copy.txt

## Allowed
- fs_read: ./data/
- fs_write: ./out/
- net: none
- exec: none
- max_label: internal

## Budget
- calls: 100
- tokens: 50000
- cost_micro: 1000000
"#;

const EXAMPLES: &str = r#"
## Examples

### copies two lines
- input ./data/in.txt:
```fixture
alpha
beta
```
- output ./out/copy.txt:
```fixture
alpha
beta
```
- exit: 0
"#;

fn with_examples() -> intent::Intent {
    intent::parse(&format!("{HEAD}{EXAMPLES}")).expect("the test intent parses")
}

fn without_examples() -> intent::Intent {
    intent::parse(HEAD).expect("the test intent parses")
}

const GOOD: &str = r#"fn main() -> i64 {
    match read_file("./data/in.txt") {
        Ok(text) => {
            match write_file("./out/copy.txt", text) {
                Ok(_) => 0
                Err(_) => 1
            }
        }
        Err(_) => 2
    }
}
"#;

const TYPE_ERROR: &str = r#"fn main() -> i64 {
    let n: i64 = "not a number"
    n
}
"#;

const WRONG_OUTPUT: &str = r#"fn main() -> i64 {
    match write_file("./out/copy.txt", "nope") {
        Ok(_) => 0
        Err(_) => 1
    }
}
"#;

const NET: &str = r#"fn main() -> i64 {
    match http_get("http://example.invalid/", "") {
        Ok(_) => 0
        Err(_) => 1
    }
}
"#;

fn warning_only() -> String {
    GOOD.replacen(
        "fn main() -> i64 {\n",
        "fn main() -> i64 {\n    let unused = 3\n",
        1,
    )
}

fn refused(r: &Result<axon_intent::Resolved, Refusal>) -> bool {
    matches!(r, Err(Refusal::LowConfidence { .. }))
        && r.as_ref().err().map(|e| e.exit_code()) == Some(5)
}

#[test]
fn a_type_error_fed_back_and_fixed_is_accepted() {
    let gen = ScriptedGenerator::new([TYPE_ERROR, GOOD]);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    let t = &out.transcript;
    assert_eq!(t.outcome, LoopOutcome::Accepted, "{}", t.to_json());
    assert_eq!(t.rounds.len(), 2);
    let r0 = t.rounds[0].report.as_ref().unwrap();
    assert_eq!(r0.stage, Stage::Check);
    assert!(r0
        .diagnostics
        .iter()
        .any(|d| d.contains("\"severity\":\"error\"")));
    let prompts = gen.prompts();
    assert!(
        prompts[1].contains("axon-diag/1"),
        "the diagnostic is fed back"
    );
    assert!(
        prompts[1].contains("let n: i64 = \"not a number\""),
        "with the program"
    );
    let resolved = out.resolved.expect("accepted");
    assert!(resolved.draft.generated);
    assert_eq!(
        resolved.draft.source.trim(),
        GOOD.trim(),
        "the loop never edits a candidate"
    );
}

#[test]
fn a_candidate_with_an_error_diagnostic_is_never_accepted() {
    // No examples: only `axon check` stands between a broken program and approval.
    let gen = ScriptedGenerator::new([TYPE_ERROR, GOOD]);
    let out = genloop::run(&without_examples(), &gen, &cfg());
    let r0 = out.transcript.rounds[0].report.as_ref().unwrap();
    assert!(
        r0.stage == Stage::Check && !r0.check_ok,
        "ATTACK: a candidate with an error diagnostic was accepted: {:?}",
        r0.stage
    );
    assert_eq!(out.transcript.outcome, LoopOutcome::Accepted);
    assert_eq!(out.resolved.unwrap().draft.source.trim(), GOOD.trim());
}

#[test]
fn a_failed_example_fed_back_and_fixed_is_accepted() {
    let gen = ScriptedGenerator::new([WRONG_OUTPUT, GOOD]);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    let r0 = out.transcript.rounds[0].report.as_ref().unwrap();
    assert!(
        r0.stage == Stage::Examples && !r0.examples[0].passed,
        "ATTACK: a candidate that failed an example was accepted: {}",
        out.transcript.to_json()
    );
    let fb = r0.feedback.as_deref().unwrap();
    assert!(
        fb.contains("output ./out/copy.txt: expected \"alpha\\nbeta\\n\", got \"nope\""),
        "{fb}"
    );
    assert!(gen.prompts()[1].contains(fb.lines().nth(1).unwrap()));
    assert_eq!(out.transcript.outcome, LoopOutcome::Accepted);
    assert!(out.resolved.is_ok());
}

#[test]
fn a_warning_only_candidate_is_accepted() {
    let w = warning_only();
    let gen = ScriptedGenerator::new([w.as_str()]);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    let r0 = out.transcript.rounds[0].report.as_ref().unwrap();
    assert!(r0.warnings >= 1, "the candidate does carry a warning");
    assert_eq!(
        out.transcript.outcome,
        LoopOutcome::Accepted,
        "{}",
        out.transcript.to_json()
    );
    assert_eq!(out.transcript.rounds.len(), 1);
    assert!(out.resolved.is_ok());
}

#[test]
fn a_static_overreach_refused_twice_ends_the_loop() {
    // The third candidate would pass; the loop must not ask for it.
    let gen = ScriptedGenerator::new([NET, NET, GOOD]);
    let out = genloop::run(&without_examples(), &gen, &cfg());
    let t = &out.transcript;
    assert_eq!(
        t.refusal_classes,
        vec![RefusalClass::StaticAxis, RefusalClass::StaticAxis],
        "ATTACK: a program needing the network passed the admission gates: {}",
        t.to_json()
    );
    assert_eq!(
        t.outcome,
        LoopOutcome::RefusedTwice,
        "ATTACK: a second capability refusal did not end the loop"
    );
    assert_eq!(gen.prompts().len(), 2);
    assert!(
        gen.prompts()[1].contains("never widens"),
        "the refusal is fed back once"
    );
    assert!(refused(&out.resolved), "exit 5, no program");
}

/// Passes check and the static gates (the copy's destination is not a
/// `write_file` literal), then writes outside `fs_write` during the example.
const WRITES_OUTSIDE: &str = r#"fn main() -> i64 {
    match read_file("./data/in.txt") {
        Ok(text) => {
            match write_file("./out/copy.txt", text) {
                Ok(_) => {
                    match file_copy("./out/copy.txt", "./data/leak.txt") {
                        Ok(_) => 0
                        Err(_) => 3
                    }
                }
                Err(_) => 1
            }
        }
        Err(_) => 2
    }
}
"#;

#[test]
fn a_write_outside_fs_write_during_an_example_is_a_runtime_scope_refusal() {
    let gen = ScriptedGenerator::new([WRITES_OUTSIDE, GOOD]);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    let t = &out.transcript;
    let r0 = t.rounds[0].report.as_ref().unwrap();
    assert_eq!(
        r0.refusal_class,
        Some(RefusalClass::RuntimeScope),
        "ATTACK: a sandbox scope refusal during an example was not classed: {}",
        t.to_json()
    );
    assert_eq!(r0.stage, Stage::Examples);
    assert_eq!(t.refusal_classes, vec![RefusalClass::RuntimeScope]);
    assert!(gen.prompts()[1].contains("sandbox"), "fed back once");
    assert_eq!(t.outcome, LoopOutcome::Accepted);
    assert_eq!(out.resolved.unwrap().draft.source.trim(), GOOD.trim());

    let gen = ScriptedGenerator::new([WRITES_OUTSIDE, WRITES_OUTSIDE, GOOD]);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    assert_eq!(out.transcript.outcome, LoopOutcome::RefusedTwice);
    assert_eq!(gen.prompts().len(), 2);
    assert!(refused(&out.resolved));
}

#[test]
fn four_bad_candidates_exhaust_the_loop_with_a_transcript_and_no_program() {
    let gen = ScriptedGenerator::new([TYPE_ERROR, TYPE_ERROR, WRONG_OUTPUT, TYPE_ERROR, GOOD]);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    let t = &out.transcript;
    assert_eq!(
        gen.prompts().len(),
        4,
        "ATTACK: the loop asked for a candidate past its last repair round"
    );
    assert_eq!(t.outcome, LoopOutcome::Exhausted);
    assert_eq!(t.rounds.len(), 4, "round 0 + 3 repair rounds");
    assert!(refused(&out.resolved));
    let json: serde_json::Value = serde_json::from_str(&t.to_json()).unwrap();
    assert_eq!(json["schema"], "axon-intent-gen/1");
    assert_eq!(json["outcome"], "exhausted");
    assert_eq!(json["generator"], "scripted");
    assert_eq!(json["rounds"].as_array().unwrap().len(), 4);
    assert_eq!(json["rounds"][2]["report"]["stage"], "examples");
    assert_eq!(json["card_sha256"].as_str().unwrap().len(), 64);
    assert!(json["rounds"][0]["candidate"]
        .as_str()
        .unwrap()
        .contains("not a number"));
    assert_eq!(
        json["rounds"][0]["prompt_sha256"].as_str().unwrap().len(),
        64
    );
}

#[test]
fn expected_outputs_are_never_present_in_the_sandbox() {
    // This candidate reads the output directory before it writes: if the loop
    // had staged an expected output, it would exit 7 and fail the example.
    let src = r#"fn main() -> i64 {
    match dir_list("./out/") {
        Ok(names) => {
            if len(names) > 0 {
                7
            } else {
                match read_file("./data/in.txt") {
                    Ok(text) => {
                        match write_file("./out/copy.txt", text) {
                            Ok(_) => 0
                            Err(_) => 1
                        }
                    }
                    Err(_) => 2
                }
            }
        }
        Err(_) => 3
    }
}
"#;
    let it = intent::parse(&format!(
        "{}{EXAMPLES}",
        HEAD.replace("- fs_read: ./data/", "- fs_read: ./data/, ./out/")
    ))
    .unwrap();
    let gen = ScriptedGenerator::new([src]);
    let out = genloop::run(&it, &gen, &cfg());
    assert_eq!(
        out.transcript.outcome,
        LoopOutcome::Accepted,
        "ATTACK: an expected output was visible to the program: {}",
        out.transcript.to_json()
    );
}

#[test]
fn a_cmd_generator_replaying_candidates_drives_the_loop() {
    // A `cmd:` generator: answers round N with candidate N from its directory.
    let dir = std::env::temp_dir().join(format!("axon-intent-genloop-cmd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("c0.ax"), TYPE_ERROR).unwrap();
    std::fs::write(dir.join("c1.ax"), GOOD).unwrap();
    let script = dir.join("gen.sh");
    common::write_executable(
        &script,
        format!(
            "#!/bin/sh\ncat > /dev/null\nd={}\nn=$(cat $d/n 2>/dev/null || echo 0)\necho $((n + 1)) > $d/n\ncat $d/c$n.ax\n",
            dir.display()
        ),
        0o755,
    );
    let gen = CmdGenerator::new(script.clone(), Duration::from_secs(20));
    assert!(gen.id().starts_with("cmd:"));
    let out = genloop::run(&with_examples(), &gen, &cfg());
    assert_eq!(
        out.transcript.outcome,
        LoopOutcome::Accepted,
        "{}",
        out.transcript.to_json()
    );
    assert_eq!(out.transcript.rounds.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_generator_failure_is_a_refusal_not_a_program() {
    let gen = ScriptedGenerator::new(Vec::<String>::new());
    let out = genloop::run(&with_examples(), &gen, &cfg());
    assert_eq!(out.transcript.outcome, LoopOutcome::GeneratorFailed);
    assert!(out.transcript.rounds[0].generator_failure.is_some());
    assert!(refused(&out.resolved));
}

#[test]
fn arm_b_is_round_zero_of_the_loop() {
    // Round 0's candidate and its full evaluation are in the transcript, so the
    // generation-only arm needs no separate call; `evaluate_candidate` grades it
    // again (e.g. on hidden examples) through the same code.
    let gen = ScriptedGenerator::new([WRONG_OUTPUT, GOOD]);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    let r0 = &out.transcript.rounds[0];
    assert_eq!(r0.round, 0);
    let (again, resolved) =
        genloop::evaluate_candidate(&with_examples(), r0.candidate.as_deref().unwrap(), &cfg());
    assert_eq!(again.stage, Stage::Examples);
    assert!(resolved.is_none());
    let _: &dyn Generator = &gen;
}
