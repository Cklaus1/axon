//! What the fix-writing generator is shown, end to end through `cortex
//! repair`: the file without the hidden check, the target's signature, the
//! types it uses, the visible failing test and what the visible tests report —
//! and, across the first prompt and every retry, nothing of the hidden check:
//! not its name, not its source, not what it printed, not its failure message.
//!
//! The generator is a `cmd:` script that saves every prompt it is given and
//! answers from a fixed script, so each row reads the real prompts.

#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;

mod common;

use std::path::{Path, PathBuf};

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

/// The hidden check's fingerprints. Each must be absent from every prompt:
/// its name, a token from its source, what it prints, and the value in its
/// failure message (`assertion failed: 0 != 48611` on the broken file,
/// `1 != 48611` on the first wrong attempt).
const HIDDEN: &str = "zz_hidden_grader";
const SEALED: [&str; 4] = [HIDDEN, "SEALED_SOURCE_q9", "SEALED-PRINT-7f3a", "48611"];

/// `sched_new` drops its `every` parameter. The meaning of `strategy` lives in
/// a comment on the type, which the body alone never showed.
fn fixture(extra_comment: &str) -> String {
    format!(
        "// Strategy: 0=interval, 1=on_score, 2=budget{extra_comment}\n\
         type Sched = {{ strategy: i64, every: i64 }}\n\
         \n\
         fn sched_new(strategy: i64, every: i64) -> Sched {{\n\
         \x20   Sched {{ strategy: strategy, every: 0 }}\n\
         }}\n\
         \n\
         fn sibling_helper(x: i64) -> i64 {{ x + 1 }}\n\
         \n\
         @[test]\n\
         fn visible_every() {{\n\
         \x20   let s = sched_new(1, 5)\n\
         \x20   println(\"VISIBLE-PRINT\")\n\
         \x20   assert_eq(s.every, 5)\n\
         }}\n\
         \n\
         // the adjudicator; never shown\n\
         @[test]\n\
         fn {HIDDEN}() {{\n\
         \x20   let SEALED_SOURCE_q9 = sched_new(2, 48611)\n\
         \x20   println(\"SEALED-PRINT-7f3a\")\n\
         \x20   assert_eq(SEALED_SOURCE_q9.every, 48611)\n\
         }}\n\
         \n\
         fn main() {{ println(to_str(sibling_helper(1))) }}\n"
    )
}

/// A workspace holding `sched.ax`, and a generator that saves prompt N to
/// `prompts/N.txt` and answers with line N of `answers` (the last line once
/// they run out).
fn stage(name: &str, src: &str, answers: &[&str]) -> (PathBuf, PathBuf) {
    let ws = std::env::temp_dir().join(format!("cortex_genctx_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(ws.join("prompts")).unwrap();
    std::fs::write(ws.join("sched.ax"), src).unwrap();
    let answers_file = ws.join("answers.txt");
    std::fs::write(&answers_file, answers.join("\n") + "\n").unwrap();
    let gen = ws.join("gen.sh");
    common::write_executable(
        &gen,
        format!(
            "#!/bin/sh\n\
             d=\"{dir}\"\n\
             n=$(ls \"$d/prompts\" | wc -l)\n\
             n=$((n + 1))\n\
             cat > \"$d/prompts/$n.txt\"\n\
             total=$(wc -l < \"{answers}\")\n\
             if [ \"$n\" -gt \"$total\" ]; then n=$total; fi\n\
             sed -n \"${{n}}p\" \"{answers}\"\n",
            dir = ws.display(),
            answers = answers_file.display(),
        ),
        0o755,
    );
    (ws, gen)
}

fn repair(ws: &Path, gen: &Path) -> (i32, serde_json::Value, String) {
    let out = common::cortex()
        .args(["repair", "--workspace"])
        .arg(ws)
        .args([
            "--file",
            "sched.ax",
            "--check",
            HIDDEN,
            "--symbol",
            "sched_new",
        ])
        .args(["--write-prefix", "sched.ax", "--axon"])
        .arg(axon_bin())
        .arg("--generator")
        .arg(format!("cmd:{}", gen.display()))
        .arg("--json")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("no verdict ({e}): {stdout}{stderr}"));
    (out.status.code().unwrap_or(-1), v, stderr)
}

fn prompts(ws: &Path) -> Vec<String> {
    (1..)
        .map(|n| ws.join("prompts").join(format!("{n}.txt")))
        .take_while(|p| p.exists())
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect()
}

fn assert_sealed(prompts: &[String]) {
    for (i, p) in prompts.iter().enumerate() {
        for s in SEALED {
            assert!(
                !p.contains(s),
                "prompt {} carries `{s}` from the hidden check:\n{p}",
                i + 1
            );
        }
    }
}

/// The first prompt shows what the body alone never did; the retries carry
/// what the VISIBLE side said about each attempt; and no prompt carries
/// anything of the hidden check — although it failed every time, with a
/// distinctive message, and printed a distinctive line on every run.
#[test]
fn the_generator_sees_the_file_and_visible_feedback_but_never_the_hidden_check() {
    let (ws, gen) = stage(
        "view",
        &fixture(""),
        &[
            // Compiles, still wrong: the hidden check fails with `1 != 48611`.
            "    Sched { strategy: strategy, every: 1 }",
            // Does not compile.
            "    Sched { strategy: strategy, every: \"x\" }",
            // The repair.
            "    Sched { strategy: strategy, every: every }",
        ],
    );
    let (code, v, err) = repair(&ws, &gen);
    let ps = prompts(&ws);
    assert_eq!(code, 0, "{err}{v}\n{ps:?}");
    assert_eq!(ps.len(), 3, "three proposals: {err}");
    assert_sealed(&ps);

    // (b) The first prompt: signature, the type the target builds, a sibling,
    // the visible failing test's source and what it printed and asserted.
    let first = &ps[0];
    for want in [
        "Function: sched_new\n",
        "Signature: fn sched_new(strategy: i64, every: i64) -> Sched",
        "type Sched = { strategy: i64, every: i64 }",
        "0=interval, 1=on_score, 2=budget",
        "fn sibling_helper(x: i64) -> i64",
        "Failing visible test(s): visible_every",
        "assert_eq(s.every, 5)",
        "VISIBLE-PRINT",
        r#"{"failed_tests":[{"name":"visible_every","message":"assertion failed: 0 != 5"}]"#,
    ] {
        assert!(
            first.contains(want),
            "first prompt lacks `{want}`:\n{first}"
        );
    }

    // (d) A CheckRefused retry carries the visible test's failure, as data,
    // on the attempt that was refused.
    let second = &ps[1];
    assert!(
        second.contains("Attempt 1 was REJECTED because it compiled but did not fix the problem"),
        "{second}"
    );
    assert!(
        second.contains(
            r#"{"failed_tests":[{"name":"visible_every","message":"assertion failed: 1 != 5"}]"#
        ),
        "the retry must say what the visible test reported on attempt 1:\n{second}"
    );

    // A DidNotCompile retry carries the compiler's diagnostics.
    let third = &ps[2];
    assert!(
        third.contains("Attempt 2 was REJECTED because it did not compile"),
        "{third}"
    );
    assert!(
        third.contains("The compiler reported (JSON)") && third.contains(r#""code":"E0102""#),
        "the retry must carry the compiler's first diagnostics:\n{third}"
    );

    assert_eq!(v["generator_context"][0]["used"], "view", "{v}");
    assert!(v["generator_context"][0]["reason"].is_null(), "{v}");
    // The visible copy is gone; the file holds the repair only.
    let left: Vec<String> = std::fs::read_dir(&ws)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("cortex-visible"))
        .collect();
    assert!(left.is_empty(), "visible copies left behind: {left:?}");
}

/// (c) The hidden check's name appears outside its definition, so the view
/// cannot be shown: the generator gets the body-only prompt, and the run says
/// so on stderr and in `--json`, with the reason.
#[test]
fn a_leak_guard_refusal_falls_back_to_the_body_only_prompt_visibly() {
    let (ws, gen) = stage(
        "leak",
        &fixture(&format!(" (graded by {HIDDEN})")),
        &[
            "    Sched { strategy: strategy, every: 1 }",
            "    Sched { strategy: strategy, every: every }",
        ],
    );
    let (code, v, err) = repair(&ws, &gen);
    let ps = prompts(&ws);
    assert_eq!(code, 0, "{err}{v}");
    assert_sealed(&ps);
    assert!(
        err.contains("generator context for `sched_new` unavailable")
            && err.contains("hidden check's name appears outside its own definition")
            && err.contains("body-only prompt"),
        "the fallback must be said on stderr: {err}"
    );
    let ctx = &v["generator_context"][0];
    assert_eq!(ctx["used"], "body_only", "{v}");
    assert!(
        ctx["reason"]
            .as_str()
            .is_some_and(|r| r.contains("appears outside its own definition")),
        "{v}"
    );
    for p in &ps {
        assert!(
            !p.contains("Signature:") && !p.contains("type Sched"),
            "{p}"
        );
    }
    // The retry still names the rejection, with no visible feedback: the
    // visible run refuses on the same guard.
    assert!(
        ps[1].contains("Attempt 1 was REJECTED") && !ps[1].contains("visible tests reported"),
        "{}",
        ps[1]
    );
}
