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
    assert_eq!(
        r0.refusal_class,
        None,
        "ATTACK: a candidate that merely failed its example was classed as a sandbox scope refusal: {}",
        out.transcript.to_json()
    );
    let fb = r0.feedback.as_deref().unwrap();
    assert!(
        fb.contains("output ./out/copy.txt: expected \"alpha\\nbeta\\n\", got \"nope\""),
        "ATTACK: the failed example's diff was not fed back: {fb}"
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

/// F12 (a)-(e) in one correct program: a comment holding the word `mod`, a
/// `TODO` comment, a write target that is a variable rather than a literal, and
/// `append_file` (a write the old text scan did not know). With IG0's
/// AST-based admission it passes check, gates and the example in round 0.
const F12_SHAPED: &str = r#"// total mod 10 is not an import
// TODO: nothing - this comment is not a stub
fn main() -> i64 {
    let out = "./out/copy.txt"
    match read_file("./data/in.txt") {
        Ok(text) => {
            match append_file(out, text) {
                Ok(_) => 0
                Err(_) => 1
            }
        }
        Err(_) => 2
    }
}
"#;

#[test]
fn an_f12_shaped_candidate_is_accepted() {
    let gen = ScriptedGenerator::new([F12_SHAPED]);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    assert_eq!(
        out.transcript.outcome,
        LoopOutcome::Accepted,
        "ATTACK: an F12-shaped correct candidate was refused: {}",
        out.transcript.to_json()
    );
    assert_eq!(out.transcript.rounds.len(), 1, "accepted in round 0");
    let resolved = out.resolved.expect("accepted");
    assert!(
        resolved.grant.effect_set().fs_write,
        "append_file is a write"
    );
    assert!(
        !resolved.grant.effect_set().net,
        "a `mod` comment is not an import"
    );
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
        "ATTACK: the gates' refusal was not fed back to the generator"
    );
    assert!(refused(&out.resolved), "exit 5, no program");
}

/// IG8 / R38 §8: a generated program's approval request names the rounds,
/// the example results and every capability attempted and refused, so the
/// human approves a program knowing it first reached for the network.
#[test]
fn the_approval_request_names_a_capability_refused_during_generation() {
    let gen = ScriptedGenerator::new([NET, GOOD]);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    assert_eq!(
        out.transcript.outcome,
        LoopOutcome::Accepted,
        "{}",
        out.transcript.to_json()
    );
    let legible = out.resolved.expect("accepted").request.legible;
    assert!(
        legible.contains("in 2 round(s); 1/1 example run(s) passed (copies two lines)"),
        "{legible}"
    );
    assert!(
        legible.contains("Refused during generation: round 0: static-axis:"),
        "ATTACK: the approval hides a capability the generator was refused: {legible}"
    );

    let clean = genloop::run(&with_examples(), &ScriptedGenerator::new([GOOD]), &cfg());
    let legible = clean.resolved.expect("accepted").request.legible;
    assert!(
        legible.contains("Refused during generation: none."),
        "{legible}"
    );
}

/// Passes check and the static gates (the copy's destination is a variable, so
/// IG0's AST scan leaves it unresolved rather than refusing it), then writes
/// outside `fs_write` during the example.
const WRITES_OUTSIDE: &str = r#"fn main() -> i64 {
    let dest = "./data/leak.txt"
    match read_file("./data/in.txt") {
        Ok(text) => {
            match write_file("./out/copy.txt", text) {
                Ok(_) => {
                    match file_copy("./out/copy.txt", dest) {
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
    assert_eq!(
        json["schema"], "axon-intent-gen/1",
        "ATTACK: the transcript is not tagged axon-intent-gen/1"
    );
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

/// A `cmd:` generator answering round N with candidate N and reporting
/// `tokens` per call on stderr; returns (generator, its directory).
fn token_reporting_generator(
    tag: &str,
    candidates: &[&str],
    tokens: u64,
) -> (CmdGenerator, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("axon-intent-genloop-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (i, c) in candidates.iter().enumerate() {
        std::fs::write(dir.join(format!("c{i}.ax")), c).unwrap();
    }
    let script = dir.join("gen.sh");
    common::write_executable(
        &script,
        format!(
            "#!/bin/sh\ncat > /dev/null\nd={}\nn=$(cat $d/n 2>/dev/null || echo 0)\necho $((n + 1)) > $d/n\necho 'axon-intent-tokens: {tokens}' >&2\ncat $d/c$n.ax\n",
            dir.display()
        ),
        0o755,
    );
    (CmdGenerator::new(script, Duration::from_secs(20)), dir)
}

#[test]
fn repair_rounds_stop_once_reported_tokens_pass_the_intent_budget() {
    // The intent's budget is 50000 tokens. At 30000 per call, round 0 leaves
    // the budget unspent, round 1 passes it, so round 2 (which would have
    // been accepted) is never asked for.
    let gen = ScriptedGenerator::new([TYPE_ERROR, TYPE_ERROR, GOOD]).with_tokens(30_000);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    let json: serde_json::Value = serde_json::from_str(&out.transcript.to_json()).unwrap();
    assert_eq!(
        (
            json["outcome"].as_str(),
            json["rounds"].as_array().map(Vec::len)
        ),
        (Some("token-budget-exhausted"), Some(2)),
        "ATTACK: a repair round ran past the intent's token budget: {json}"
    );
    assert_eq!(gen.prompts().len(), 2);
    assert_eq!(json["tokens"], 60000);
    assert_eq!(json["token_budget"], 50000);
    assert!(refused(&out.resolved));
    let reason = format!("{:?}", out.resolved.err());
    assert!(reason.contains("token budget of 50000"), "{reason}");

    // Under the budget the same answers repair to acceptance.
    let gen = ScriptedGenerator::new([TYPE_ERROR, TYPE_ERROR, GOOD]).with_tokens(10_000);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    assert_eq!(out.transcript.outcome, LoopOutcome::Accepted);
    assert_eq!(out.transcript.tokens, Some(30000));
}

#[test]
fn a_cmd_generator_s_reported_tokens_count_against_the_budget() {
    let (gen, dir) = token_reporting_generator("tok", &[TYPE_ERROR, TYPE_ERROR, GOOD], 30000);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        (out.transcript.outcome, out.transcript.tokens),
        (LoopOutcome::TokenBudgetExhausted, Some(60000)),
        "ATTACK: the tokens a cmd generator reported on stderr were not counted: {}",
        out.transcript.to_json()
    );
}

#[test]
fn an_intent_naming_no_tokens_is_held_to_the_default_budget() {
    let no_tokens = intent::parse(&format!(
        "{}{EXAMPLES}",
        HEAD.replace("- tokens: 50000\n", "")
    ))
    .unwrap();
    assert_eq!(no_tokens.budget.tokens, 0, "the fixture names no tokens");
    let gen = ScriptedGenerator::new([TYPE_ERROR, TYPE_ERROR, GOOD]).with_tokens(60_000);
    let out = genloop::run(&no_tokens, &gen, &cfg());
    assert_eq!(
        (
            out.transcript.outcome,
            out.transcript.token_budget,
            gen.prompts().len()
        ),
        (
            LoopOutcome::TokenBudgetExhausted,
            genloop::DEFAULT_TOKEN_BUDGET,
            2
        ),
        "ATTACK: an intent naming no tokens was not held to DEFAULT_TOKEN_BUDGET: {}",
        out.transcript.to_json()
    );
}

#[test]
fn a_program_s_own_stderr_and_exit_status_do_not_fail_its_example() {
    // Writes the right file, says alarming-looking words on stderr and stops
    // with `exit(2)` - which is exactly what its example states.
    let it = intent::parse(&format!(
        "{HEAD}{}",
        EXAMPLES.replace("- exit: 0", "- exit: 2")
    ))
    .unwrap();
    let src = r#"fn main() -> i64 {
    eprintln("capability check ok")
    eprintln("note: no parse error in input")
    eprintln("line budget not exceeded")
    let wrote = match read_file("./data/in.txt") {
        Ok(text) => {
            match write_file("./out/copy.txt", text) {
                Ok(_) => true
                Err(_) => false
            }
        }
        Err(_) => false
    }
    if wrote {
        exit(2)
    }
    1
}
"#;
    let gen = ScriptedGenerator::new([src]);
    let out = genloop::run(&it, &gen, &cfg());
    assert_eq!(
        (
            out.transcript.outcome,
            out.transcript.refusal_classes.clone()
        ),
        (LoopOutcome::Accepted, vec![]),
        "ATTACK: a program's stderr words or its exit(2) decided its example: {}",
        out.transcript.to_json()
    );
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

#[test]
fn the_loop_repairs_max_repair_rounds_times_and_never_more() {
    let bad = [TYPE_ERROR; 6];
    let gen = ScriptedGenerator::new(bad);
    let out = genloop::run(&without_examples(), &gen, &cfg());
    assert_eq!(
        gen.prompts().len(),
        4,
        "ATTACK: a default LoopConfig did not allow exactly MAX_REPAIR_ROUNDS (3) repair rounds: {}",
        out.transcript.to_json()
    );
    assert!(
        refused(&out.resolved),
        "ATTACK: an exhausted loop's refusal is not exit 5 (low confidence): {:?}",
        out.resolved.as_ref().err()
    );

    let mut wide = cfg();
    wide.max_repair_rounds = 10;
    let gen = ScriptedGenerator::new(bad);
    let out = genloop::run(&without_examples(), &gen, &wide);
    assert_eq!(
        (gen.prompts().len(), out.transcript.max_repair_rounds),
        (4, 3),
        "ATTACK: max_repair_rounds above MAX_REPAIR_ROUNDS was not clamped: {}",
        out.transcript.to_json()
    );
}

#[test]
fn the_default_deadlines_let_a_good_candidate_through() {
    // The real checker and sandbox under `LoopConfig::new`'s CHECK_TIMEOUT and
    // EXAMPLE_TIMEOUT: neither deadline may cut a quick, correct run short.
    let gen = ScriptedGenerator::new([GOOD]);
    let out = genloop::run(&with_examples(), &gen, &cfg());
    assert_eq!(
        out.transcript.outcome,
        LoopOutcome::Accepted,
        "ATTACK: a default deadline (CHECK_TIMEOUT 60 s, EXAMPLE_TIMEOUT 20 s) cut a good candidate short: {}",
        out.transcript.to_json()
    );
}

#[test]
fn a_lifted_program_the_gates_refuse_keeps_the_gates_own_refusal() {
    let (rep, resolved) = genloop::evaluate_lifted(&without_examples(), NET, &cfg());
    assert!(resolved.is_none());
    assert_eq!(rep.stage, Stage::Gates, "{rep:?}");
    let gate = rep.gate_error.clone().expect("the gates' refusal");
    assert_eq!(gate.exit_code(), 8, "{gate:?}");
    let r = rep.refusal();
    assert_eq!(
        (r.exit_code(), r.legible()),
        (gate.exit_code(), gate.legible()),
        "ATTACK: a lifted program the gates refused was reported without the gates' own refusal"
    );
}

// -- a fake `axon`: the checker's and the card's failure paths -----------------

/// A fake `axon` (a `/bin/sh` script) in its own directory: (directory, binary).
/// It runs under `env_clear`, so every tool it calls is named absolutely.
fn fake_axon(tag: &str, body: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "axon-intent-genloop-fake-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("axon");
    common::write_executable(&bin, format!("#!/bin/sh\n{body}"), 0o755);
    (dir, bin)
}

fn fake_cfg(bin: PathBuf) -> LoopConfig {
    LoopConfig::new(bin, "CARD: the Axon language card (test stand-in)".into())
}

/// This process's environment names a child of `env_clear` must not see
/// (`/bin/sh` adds PWD itself).
fn ambient_names() -> Vec<String> {
    let names: Vec<String> = std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| !matches!(k.as_str(), "PATH" | "PWD" | "OLDPWD" | "SHLVL" | "_"))
        .collect();
    assert!(
        !names.is_empty(),
        "the test process has an environment to leak"
    );
    names
}

fn leaked<'a>(lines: &str, names: &'a [String]) -> Vec<&'a str> {
    names
        .iter()
        .filter(|n| lines.lines().any(|l| l.starts_with(&format!("{n}="))))
        .map(String::as_str)
        .collect()
}

#[test]
fn axon_card_runs_with_a_cleared_environment_and_any_failure_is_an_error() {
    let t = Duration::from_secs(20);
    let path = std::env::var("PATH").expect("the test process has a PATH");
    let ambient = ambient_names();
    let mut dirs = Vec::new();

    let (dir, bin) = fake_axon("card-env", "echo \"argv: $*\"\n/usr/bin/env\n");
    dirs.push(dir);
    let card = genloop::load_card(&bin, t).expect("a card");
    assert!(
        card.lines().any(|l| l == "argv: card"),
        "ATTACK: the language card was asked for with another argv: {card}"
    );
    let leak = leaked(&card, &ambient);
    assert!(
        leak.is_empty(),
        "ATTACK: `axon card` inherited the caller's environment: {leak:?}"
    );
    assert!(
        card.lines().any(|l| l == format!("PATH={path}")),
        "ATTACK: `axon card` ran without the caller's PATH: {card}"
    );

    let (dir, bin) = fake_axon(
        "card-exit",
        "echo 'a card'\necho 'card: broken' >&2\nexit 3\n",
    );
    dirs.push(dir);
    let e = genloop::load_card(&bin, t)
        .expect_err("ATTACK: a failed `axon card` was taken as the card");
    assert!(e.contains("exited Some(3): card: broken"), "{e}");

    let (dir, bin) = fake_axon("card-empty", "exit 0\n");
    dirs.push(dir);
    let e = genloop::load_card(&bin, t)
        .expect_err("ATTACK: an empty `axon card` answer was taken as the card");
    assert!(e.contains("printed no card"), "{e}");

    let (dir, bin) = fake_axon(
        "card-big",
        "/usr/bin/head -c 1100000 /dev/zero | /usr/bin/tr '\\0' a\n",
    );
    dirs.push(dir);
    let e = genloop::load_card(&bin, t)
        .expect_err("ATTACK: an `axon card` answer over 1 MiB was taken, truncated, as the card");
    assert!(e.contains("printed no card (or more than 1 MiB)"), "{e}");

    let (dir, bin) = fake_axon("card-hang", "exec /usr/bin/sleep 10\n");
    dirs.push(dir);
    let e = genloop::load_card(&bin, Duration::from_millis(300)).expect_err("a hung card");
    assert!(
        e.contains("timed out after 300 ms"),
        "ATTACK: a hung `axon card` was not reported as timed out: {e}"
    );
    for d in dirs {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn axon_check_runs_on_the_staged_candidate_with_a_cleared_environment() {
    let path = std::env::var("PATH").expect("the test process has a PATH");
    let ambient = ambient_names();
    let (dir, bin) = fake_axon(
        "check-env",
        "echo \"argv: $*\"\necho \"staged: $(/usr/bin/cat main.ax)\"\n/usr/bin/env\nexit 1\n",
    );
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let src = "fn main() -> i64 { 0 }";
    let rep =
        genloop::check_source(&bin, src, &work, Duration::from_secs(20)).expect("the checker ran");
    assert!(!rep.ok, "exit 1 is a rejection");
    let out = rep.errors.join("\n");
    assert!(
        rep.errors.iter().any(|l| l == "argv: check --json main.ax"),
        "ATTACK: the candidate was checked with another argv: {out}"
    );
    assert!(
        rep.errors.iter().any(|l| *l == format!("staged: {src}")),
        "ATTACK: `axon check` did not run in the candidate's directory: {out}"
    );
    let leak = leaked(&out, &ambient);
    assert!(
        leak.is_empty(),
        "ATTACK: `axon check` inherited the caller's environment: {leak:?}"
    );
    assert!(
        rep.errors.iter().any(|l| *l == format!("PATH={path}")),
        "ATTACK: `axon check` ran without the caller's PATH: {out}"
    );

    let (hang, bin) = fake_axon("check-hang", "exec /usr/bin/sleep 10\n");
    match genloop::check_source(&bin, src, &work, Duration::from_millis(300)) {
        Err(e) => assert!(
            e.contains("timed out after 300 ms"),
            "ATTACK: a hung `axon check` was not reported as timed out: {e}"
        ),
        Ok(r) => panic!("ATTACK: a hung `axon check` was not reported as timed out: {r:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&hang);
}

#[test]
fn the_check_scratch_dir_is_private_and_the_checker_s_feedback_is_capped() {
    // 200 error diagnostics of about 100 bytes; the first names the mode of
    // the directory the candidate is checked in.
    let (dir, bin) = fake_axon(
        "check-cap",
        "m=$(/usr/bin/stat -c %a .)\n\
         echo '{\"schema\":\"axon-diag/1\",\"severity\":\"error\",\"message\":\"scratch mode '$m'\"}'\n\
         i=0\nwhile [ $i -lt 200 ]; do\n\
         echo '{\"schema\":\"axon-diag/1\",\"severity\":\"error\",\"message\":\"padding padding padding padding padding\"}'\n\
         i=$((i + 1))\ndone\nexit 1\n",
    );
    let (rep, resolved) = genloop::evaluate_candidate(&without_examples(), GOOD, &fake_cfg(bin));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(resolved.is_none());
    assert_eq!(rep.stage, Stage::Check, "{rep:?}");
    assert!(
        rep.diagnostics[0].contains("scratch mode 700"),
        "ATTACK: the candidate's scratch dir is not private (0700): {}",
        rep.diagnostics[0]
    );
    let fb = rep.feedback.as_deref().unwrap();
    assert!(
        fb.len() <= 2048 + 64 && fb.contains("more line(s) omitted"),
        "ATTACK: the checker's feedback was not capped at 2048 bytes: {} bytes",
        fb.len()
    );
}

#[test]
fn a_sandbox_that_cannot_start_is_an_infrastructure_failure() {
    // The fake checker passes the candidate and deletes itself, so the
    // example's sandbox run cannot be spawned.
    let (dir, bin) = fake_axon("not-run", "/usr/bin/rm -f \"$0\"\nexit 0\n");
    let (rep, resolved) = genloop::evaluate_candidate(&with_examples(), GOOD, &fake_cfg(bin));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(resolved.is_none());
    assert!(
        rep.stage == Stage::Infrastructure
            && rep
                .infrastructure
                .as_deref()
                .is_some_and(|i| i.contains("the sandbox could not start the run")),
        "ATTACK: a sandbox that could not start a run was graded as the program's own failure: {rep:?}"
    );
}

#[test]
fn a_fixture_with_an_absolute_path_is_never_staged() {
    let escape =
        std::env::temp_dir().join(format!("axon-intent-genloop-escape-{}", std::process::id()));
    let _ = std::fs::remove_file(&escape);
    let mut it = with_examples();
    it.examples[0].runs[0].inputs[0].path = escape.display().to_string();
    let (dir, bin) = fake_axon("abs-fixture", "exit 0\n");
    let (rep, resolved) = genloop::evaluate_candidate(&it, GOOD, &fake_cfg(bin));
    let _ = std::fs::remove_dir_all(&dir);
    let written = escape.exists();
    let _ = std::fs::remove_file(&escape);
    assert!(
        !written,
        "ATTACK: a fixture with an absolute path was written outside the example's directory: {}",
        escape.display()
    );
    assert_eq!(rep.stage, Stage::Infrastructure, "{rep:?}");
    assert!(resolved.is_none());
    assert!(
        rep.infrastructure
            .as_deref()
            .is_some_and(|i| i.contains("is not relative")),
        "{rep:?}"
    );
}
