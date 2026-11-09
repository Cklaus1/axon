//! IG5 - the `axon-intent compile|emit` program sources, end to end through the
//! real binary, the real `axon` checker and the axon-os sandbox. Exactly one
//! source per run and never a fallback: a fenced program is lifted and then
//! checked, gated and run on the intent's examples; `--generator` runs the
//! check->repair loop; with neither (and no `AXON_AI_MOCK`) `compile` exits 2
//! naming the flag. `emit --yes` never approves a generated program. The
//! generator is a deterministic `cmd:` script (no network, no key).

mod common;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn real_axon() -> PathBuf {
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

/// A per-test directory; the CLI runs against the real `axon` (checker,
/// sandbox and IG2's `axon card`).
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d =
            std::env::temp_dir().join(format!("axon-intent-cli-gen-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write_intent(&self, body: &str) -> PathBuf {
        let p = self.0.join("job.intent.md");
        std::fs::write(&p, body).unwrap();
        p
    }

    /// A `cmd:` generator answering round N with `candidates[N]`.
    fn generator(&self, candidates: &[&str]) -> String {
        for (i, c) in candidates.iter().enumerate() {
            std::fs::write(self.0.join(format!("c{i}.ax")), c).unwrap();
        }
        let script = self.0.join("gen.sh");
        common::write_executable(
            &script,
            format!(
                "#!/bin/sh\ncat > /dev/null\nd={}\nn=$(cat $d/n 2>/dev/null || echo 0)\necho $((n + 1)) > $d/n\ncat $d/c$n.ax\n",
                self.0.display()
            ),
            0o755,
        );
        format!("cmd:{}", script.display())
    }

    fn intent_cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_axon-intent"))
            .args(args)
            .env_remove("AXON_AI_MOCK")
            .env_remove("AXON_APPROVAL_KEY")
            .env("AXON_BIN", real_axon())
            .current_dir(&self.0)
            .output()
            .expect("spawn axon-intent")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(o: &Output) -> String {
    format!(
        "rc={:?}\nstdout:\n{}\nstderr:\n{}",
        o.status.code(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
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

const WRONG_OUTPUT: &str = r#"fn main() -> i64 {
    match write_file("./out/copy.txt", "nope") {
        Ok(_) => 0
        Err(_) => 1
    }
}
"#;

const OTHER_OUTPUT: &str = r#"fn main() -> i64 {
    match read_file("./data/in.txt") {
        Ok(text) => {
            match write_file("./out/other.txt", text) {
                Ok(_) => 0
                Err(_) => 1
            }
        }
        Err(_) => 2
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

fn with_program(program: &str) -> String {
    format!("{HEAD}\n## Program\n```axon\n{program}```\n")
}

#[test]
fn no_program_and_no_generator_exits_2_naming_the_flag() {
    let d = Dir::new("noflag");
    let intent = d.write_intent(HEAD);
    let o = d.intent_cli(&["compile", intent.to_str().unwrap(), "--out", "o"]);
    assert_eq!(
        o.status.code(),
        Some(2),
        "ATTACK: compile without a program source fell back to another source: {}",
        text(&o)
    );
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("--generator"),
        "ATTACK: compile without a program source did not name --generator: {}",
        text(&o)
    );
    assert!(!d.path().join("o/job.ax").exists(), "no triple");
}

#[test]
fn a_lifted_program_passing_its_examples_compiles_and_emits() {
    let d = Dir::new("lift-ok");
    let intent = d.write_intent(&with_program(GOOD));
    let o = d.intent_cli(&["compile", intent.to_str().unwrap(), "--out", "o"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert_eq!(
        std::fs::read_to_string(d.path().join("o/job.ax")).unwrap(),
        GOOD
    );
    let e = d.intent_cli(&[
        "emit",
        intent.to_str().unwrap(),
        "--by",
        "alice",
        "--out",
        "e",
        "--yes",
    ]);
    assert_eq!(e.status.code(), Some(0), "{}", text(&e));
    assert!(d.path().join("e/job.approval").exists(), "{}", text(&e));
}

#[test]
fn a_lifted_program_failing_its_example_is_refused() {
    let d = Dir::new("lift-ex");
    let intent = d.write_intent(&with_program(WRONG_OUTPUT));
    let o = d.intent_cli(&["compile", intent.to_str().unwrap(), "--out", "o"]);
    assert_eq!(
        o.status.code(),
        Some(5),
        "ATTACK: an author program failing its example was compiled: {}",
        text(&o)
    );
    assert!(!d.path().join("o/job.ax").exists(), "no triple");
}

/// genviab grades hidden examples by lifting a candidate: `--transcript` on
/// the lift path writes the program's own report (it was silently ignored).
#[test]
fn a_lifted_program_writes_its_report_to_the_transcript_path() {
    let d = Dir::new("lift-rep");
    let intent = d.write_intent(&with_program(WRONG_OUTPUT));
    let o = d.intent_cli(&[
        "compile",
        intent.to_str().unwrap(),
        "--out",
        "o",
        "--transcript",
        "lift.json",
    ]);
    assert_eq!(o.status.code(), Some(5), "{}", text(&o));
    let raw = std::fs::read_to_string(d.path().join("lift.json"))
        .unwrap_or_else(|e| panic!("no lift report: {e}\n{}", text(&o)));
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["schema"], "axon-intent-lift/1");
    assert_eq!(v["report"]["stage"], "examples");
    let ex = &v["report"]["examples"][0];
    assert_eq!(ex["example"], "copies two lines");
    assert_eq!(ex["passed"], false);
    assert!(
        ex["diffs"][0]
            .as_str()
            .unwrap_or("")
            .contains("out/copy.txt"),
        "{raw}"
    );

    let ok = d.write_intent(&with_program(GOOD));
    let o = d.intent_cli(&[
        "compile",
        ok.to_str().unwrap(),
        "--out",
        "o",
        "--transcript",
        "lift-ok.json",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("lift-ok.json")).unwrap())
            .unwrap();
    assert_eq!(v["report"]["stage"], "accepted");
}

#[test]
fn a_lifted_program_writing_an_unasked_output_is_refused_by_confidence() {
    let d = Dir::new("lift-conf");
    let intent = d.write_intent(&with_program(OTHER_OUTPUT));
    let o = d.intent_cli(&["compile", intent.to_str().unwrap(), "--out", "o"]);
    assert_eq!(o.status.code(), Some(5), "{}", text(&o));
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("outputs"),
        "{}",
        text(&o)
    );
    assert!(!d.path().join("o/job.ax").exists(), "no triple");
}

#[test]
fn a_lifted_program_needing_the_network_is_refused_by_the_grant() {
    let d = Dir::new("lift-net");
    let intent = d.write_intent(&with_program(NET));
    let o = d.intent_cli(&["compile", intent.to_str().unwrap(), "--out", "o"]);
    assert_eq!(o.status.code(), Some(8), "{}", text(&o));
    assert!(!d.path().join("o/job.ax").exists(), "no triple");
}

#[test]
fn a_generated_program_compiles_with_a_transcript_and_needs_a_human_approve() {
    let d = Dir::new("gen-ok");
    let intent = d.write_intent(HEAD);
    let gen = d.generator(&[WRONG_OUTPUT, GOOD]);
    let o = d.intent_cli(&[
        "compile",
        intent.to_str().unwrap(),
        "--out",
        "o",
        "--generator",
        &gen,
        "--transcript",
        "t.json",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert_eq!(
        std::fs::read_to_string(d.path().join("o/job.ax")).unwrap(),
        GOOD
    );
    let t: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("t.json")).unwrap()).unwrap();
    assert_eq!(t["schema"], "axon-intent-gen/1");
    assert_eq!(t["outcome"], "accepted");
    assert_eq!(t["rounds"].as_array().unwrap().len(), 2);
    // IG8: the printed approval request says it was generated and how.
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        out.contains("in 2 round(s); 1/1 example run(s) passed")
            && out.contains("Refused during generation: none."),
        "{}",
        text(&o)
    );
    // The prompt carried the real language card (IG2), byte for byte.
    let card_sha = Command::new(real_axon())
        .args(["card", "--sha256"])
        .output()
        .expect("spawn axon card --sha256");
    assert_eq!(
        t["card_sha256"].as_str().unwrap(),
        String::from_utf8_lossy(&card_sha.stdout).trim(),
        "ATTACK: the loop's card is not the one `axon card` prints"
    );

    // `emit --yes` writes the triple but never approves a generated program.
    let _ = std::fs::remove_file(d.path().join("n"));
    let e = d.intent_cli(&[
        "emit",
        intent.to_str().unwrap(),
        "--by",
        "alice",
        "--out",
        "e",
        "--yes",
        "--generator",
        &gen,
    ]);
    assert!(
        !d.path().join("e/job.approval").exists() && e.status.code() == Some(2),
        "ATTACK: emit --yes approved a generated program: rc={:?} {}",
        e.status.code(),
        text(&e)
    );
    // The human approves it explicitly.
    let a = d.intent_cli(&["approve", "e/job.axjob", "--by", "alice", "--accept"]);
    assert_eq!(a.status.code(), Some(0), "{}", text(&a));
    assert!(d.path().join("e/job.approval").exists());
}

#[test]
fn a_generator_overreaching_twice_is_refused_with_no_program() {
    let d = Dir::new("gen-net");
    let intent = d.write_intent(HEAD);
    let gen = d.generator(&[NET, NET, GOOD]);
    let o = d.intent_cli(&[
        "compile",
        intent.to_str().unwrap(),
        "--out",
        "o",
        "--generator",
        &gen,
    ]);
    assert_eq!(o.status.code(), Some(5), "{}", text(&o));
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("second capability refusal"),
        "{}",
        text(&o)
    );
    assert!(!d.path().join("o/job.ax").exists(), "no triple");
}

#[test]
fn an_unrecognized_invocation_exits_2() {
    let d = Dir::new("unknown");
    let o = d.intent_cli(&["frobnicate", "x"]);
    assert_eq!(
        o.status.code(),
        Some(2),
        "ATTACK: an unrecognized invocation did not exit 2: {}",
        text(&o)
    );
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("unrecognized invocation"),
        "{}",
        text(&o)
    );
}

/// `approve --reject` records the rejection: exit 3 and no token.
#[test]
fn approve_reject_writes_no_approval_and_exits_3() {
    let d = Dir::new("reject");
    let intent = d.write_intent(&with_program(GOOD));
    let o = d.intent_cli(&["compile", intent.to_str().unwrap(), "--out", "o"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let a = d.intent_cli(&["approve", "o/job.axjob", "--by", "alice", "--reject"]);
    assert!(
        a.status.code() == Some(3) && !d.path().join("o/job.approval").exists(),
        "ATTACK: approve --reject wrote an approval: {}",
        text(&a)
    );
}

/// A transcript that cannot be written is a usage error on the lift path:
/// exit 2 and no triple, never a compile without the report asked for.
#[test]
fn an_unwritable_lift_transcript_exits_2_with_no_triple() {
    let d = Dir::new("lift-unw");
    let intent = d.write_intent(&with_program(GOOD));
    let o = d.intent_cli(&[
        "compile",
        intent.to_str().unwrap(),
        "--out",
        "o",
        "--transcript",
        "missing/lift.json",
    ]);
    assert!(
        o.status.code() == Some(2) && !d.path().join("o/job.ax").exists(),
        "ATTACK: the lift report could not be written and compile went on: {}",
        text(&o)
    );
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("cannot write the transcript"),
        "{}",
        text(&o)
    );
}

/// The same on the generator path.
#[test]
fn an_unwritable_generation_transcript_exits_2_with_no_triple() {
    let d = Dir::new("gen-unw");
    let intent = d.write_intent(HEAD);
    let gen = d.generator(&[GOOD]);
    let o = d.intent_cli(&[
        "compile",
        intent.to_str().unwrap(),
        "--out",
        "o",
        "--generator",
        &gen,
        "--transcript",
        "missing/t.json",
    ]);
    assert!(
        o.status.code() == Some(2) && !d.path().join("o/job.ax").exists(),
        "ATTACK: the generation transcript could not be written and compile went on: {}",
        text(&o)
    );
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("cannot write the transcript"),
        "{}",
        text(&o)
    );
}
