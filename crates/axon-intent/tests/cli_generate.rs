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

/// A per-test directory holding a stand-in `axon` that answers `card` (IG2's
/// verb, not needed by these tests' semantics) and hands every other verb to
/// the real interpreter.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d =
            std::env::temp_dir().join(format!("axon-intent-cli-gen-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        common::write_executable(
            &d.join("axon.sh"),
            format!(
                "#!/bin/sh\n[ \"$1\" = card ] && {{ echo 'CARD: test stand-in'; exit 0; }}\nexec '{}' \"$@\"\n",
                real_axon().display()
            ),
            0o755,
        );
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
            .env("AXON_BIN", self.0.join("axon.sh"))
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
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
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
    assert_eq!(e.status.code(), Some(2), "{}", text(&e));
    assert!(
        !d.path().join("e/job.approval").exists(),
        "ATTACK: emit --yes approved a generated program: {}",
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
