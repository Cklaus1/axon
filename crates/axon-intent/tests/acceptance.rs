//! R22 acceptance checks (A1–A6 + Core) — these drive the REAL `axon-intent`
//! CLI exactly as an operator would (subprocess), against the deterministic mock
//! synthesizer (`AXON_AI_MOCK=1`, no network/key). The end-to-end-run +
//! cross-spec-handoff checks additionally drive the real `axon-os` (R21) + the
//! `axon` interpreter; those skip cleanly when the interpreter is absent (so a
//! codegen-less CI still passes), exactly like the R21 parity harnesses.
//!
//! Every §0 acceptance check name appears here as a real, non-stubbed test.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn intent_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_axon-intent"))
}

/// The R21 supervisor binary, built into the same target dir. None ⇒ skip the
/// cross-spec run checks.
fn axon_os_bin() -> Option<PathBuf> {
    let p = workspace_root().join("target/debug/axon-os");
    p.exists().then_some(p)
}

/// The interpreter the supervisor drives. None ⇒ skip end-to-end run checks.
fn axon_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AXON_BIN") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let p = workspace_root().join("target/debug/axon");
    p.exists().then_some(p)
}

fn intents() -> PathBuf {
    workspace_root().join("examples/intents")
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon-intent-acc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::create_dir_all(&d);
    d
}

struct Out {
    stdout: String,
    code: i32,
}

/// Invoke `axon-intent` under the deterministic mock (offline). `extra_env`
/// overrides/adds environment.
fn ai(args: &[&str], extra_env: &[(&str, &str)]) -> Out {
    let mut cmd = Command::new(intent_bin());
    cmd.args(args);
    cmd.env("AXON_AI_MOCK", "1");
    cmd.current_dir(workspace_root());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn axon-intent");
    Out {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        code: out.status.code().unwrap_or(-1),
    }
}

/// Invoke the R21 supervisor with the interpreter wired in.
fn os(bin: &Path, args: &[&str], axon: &Path) -> Out {
    let mut cmd = Command::new(bin);
    cmd.args(args);
    cmd.env("AXON_BIN", axon);
    cmd.current_dir(workspace_root());
    let out = cmd.output().expect("spawn axon-os");
    Out {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        code: out.status.code().unwrap_or(-1),
    }
}

/// Stage a runnable job dir: an emitted summarize triple plus the `./data/` it
/// reads, so `axon-os run <dir>/summarize.axjob` (which cd's into the dir) finds
/// `./data/report.txt`. Returns the dir.
fn stage_runnable_job(name: &str) -> PathBuf {
    let dir = tmp(name);
    let c = ai(
        &[
            "compile",
            "examples/intents/summarize.intent.md",
            "--out",
            dir.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(c.code, 0, "compile summarize: {}", c.stdout);
    // stage the input file the program reads + the output dir it writes to
    // (both relative to the job dir, which axon-os cd's into).
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(dir.join("out")).unwrap();
    std::fs::write(data.join("report.txt"), "Quarterly report. Revenue up.\n").unwrap();
    dir
}

// ── A1: the end-to-end operator journey through the real CLI ─────────────────
#[test]
fn acc_a1_smoke_intent_to_approval() {
    // 1. compile — ADMISSIBLE + confidence + the .ax/.axjob artifacts.
    let dir = tmp("a1");
    let c = ai(
        &[
            "compile",
            "examples/intents/summarize.intent.md",
            "--out",
            dir.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(c.code, 0, "compile succeeds: {}", c.stdout);
    assert!(
        c.stdout.contains("ADMISSIBLE"),
        "says ADMISSIBLE: {}",
        c.stdout
    );
    assert!(
        c.stdout.contains("confidence"),
        "reports confidence: {}",
        c.stdout
    );
    assert!(c.stdout.contains("risk Low") || c.stdout.contains("risk Medium"));
    assert!(dir.join("summarize.ax").exists(), ".ax artifact written");
    assert!(
        dir.join("summarize.axjob").exists(),
        ".axjob artifact written"
    );

    // 2. review — the legible "may / may NOT / budget" text.
    let r = ai(
        &[
            "review",
            dir.join("summarize.axjob").to_str().unwrap(),
            "--program",
            dir.join("summarize.ax").to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(r.code, 0);
    assert!(
        r.stdout.contains("WILL be allowed to"),
        "legible bound: {}",
        r.stdout
    );
    assert!(r.stdout.contains("may NOT"));
    assert!(r.stdout.contains("Budget"));

    // 3. approve — "✓ approved" + the .approval artifact.
    let a = ai(
        &[
            "approve",
            dir.join("summarize.axjob").to_str().unwrap(),
            "--by",
            "op",
            "--accept",
        ],
        &[],
    );
    assert_eq!(a.code, 0, "approve succeeds: {}", a.stdout);
    assert!(a.stdout.contains("approved"), "says approved: {}", a.stdout);
    assert!(dir.join("summarize.approval").exists(), ".approval written");

    // 4. (cross-spec) axon-os runs the approved triple to completion.
    if let (Some(os_bin), Some(axon)) = (axon_os_bin(), axon_bin()) {
        // restage the input + output dir next to the emitted program.
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::create_dir_all(dir.join("out")).unwrap();
        std::fs::write(dir.join("data/report.txt"), "Quarterly report.\n").unwrap();
        let run = os(
            &os_bin,
            &[
                "run",
                dir.join("summarize.axjob").to_str().unwrap(),
                "--run-id",
                "demo",
                "--out",
                dir.join("runs").to_str().unwrap(),
            ],
            &axon,
        );
        assert_eq!(run.code, 0, "axon-os runs the triple: {}", run.stdout);
        assert!(
            run.stdout.contains("completed"),
            "completes: {}",
            run.stdout
        );
    } else {
        eprintln!("acc_a1: axon-os / interpreter absent — skipping the run leg");
    }

    // 5. an under-specified intent is REFUSED in plain English, no artifacts.
    let vdir = tmp("a1-vague");
    let v = ai(
        &[
            "compile",
            "examples/intents/vague.intent.md",
            "--out",
            vdir.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(v.code, 5, "vague is refused (exit 5): {}", v.stdout);
    assert!(
        v.stdout.contains("REFUSED"),
        "plain-English refusal: {}",
        v.stdout
    );
    assert!(!vdir.join("vague.ax").exists(), "no triple on refusal");
}

// ── A2: the shipped example intents compile / are refused, with artifacts ─────
#[test]
fn acc_a2_example_intents_compile_and_run() {
    // summarize → admissible triple.
    let dir = stage_runnable_job("a2");
    assert!(dir.join("summarize.ax").exists());
    assert!(dir.join("summarize.axjob").exists());

    // overbroad → grant has net=∅ (least privilege) even though the intent
    // permits net.
    let odir = tmp("a2-over");
    let o = ai(
        &[
            "compile",
            "examples/intents/overbroad.intent.md",
            "--out",
            odir.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(o.code, 0, "overbroad compiles: {}", o.stdout);
    let axjob = std::fs::read_to_string(odir.join("overbroad.axjob")).unwrap();
    assert!(axjob.contains("net = []"), "net clamped to empty: {axjob}");
    assert!(
        o.stdout.contains("use the network"),
        "legible 'may NOT' net: {}",
        o.stdout
    );

    // vague → refused, no triple.
    let vdir = tmp("a2-vague");
    let v = ai(
        &[
            "compile",
            "examples/intents/vague.intent.md",
            "--out",
            vdir.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(v.code, 5, "vague refused: {}", v.stdout);
    assert!(!vdir.join("vague.ax").exists());

    // cross-spec: the summarize triple runs to completion under R21.
    if let (Some(os_bin), Some(axon)) = (axon_os_bin(), axon_bin()) {
        let run = os(
            &os_bin,
            &[
                "run",
                dir.join("summarize.axjob").to_str().unwrap(),
                "--run-id",
                "s",
                "--out",
                dir.join("runs").to_str().unwrap(),
            ],
            &axon,
        );
        assert_eq!(run.code, 0, "summarize runs under R21: {}", run.stdout);
        assert!(dir.join("out/summary.txt").exists(), "wrote its output");
    } else {
        eprintln!("acc_a2: axon-os / interpreter absent — skipping the run leg");
    }
    let _ = intents(); // path is referenced so a move breaks the test
}

// ── A3: the README quickstart commands are real (docs can't rot) ─────────────
#[test]
fn acc_a3_quickstart_commands_execute() {
    let readme = std::fs::read_to_string(workspace_root().join("README-axon-intent.md"))
        .expect("README-axon-intent.md exists");
    for verb in ["compile", "review", "approve"] {
        assert!(
            readme.contains(&format!("axon-intent {verb}")),
            "README documents `{verb}`"
        );
    }
    // Execute the documented sequence verbatim (mock).
    let dir = tmp("a3");
    assert_eq!(
        ai(
            &[
                "compile",
                "examples/intents/summarize.intent.md",
                "--out",
                dir.to_str().unwrap()
            ],
            &[]
        )
        .code,
        0
    );
    assert_eq!(
        ai(
            &[
                "review",
                dir.join("summarize.axjob").to_str().unwrap(),
                "--program",
                dir.join("summarize.ax").to_str().unwrap()
            ],
            &[]
        )
        .code,
        0
    );
    assert_eq!(
        ai(
            &[
                "approve",
                dir.join("summarize.axjob").to_str().unwrap(),
                "--by",
                "alice",
                "--accept"
            ],
            &[]
        )
        .code,
        0
    );
    // The documented "vague is refused, exit 5" line.
    assert_eq!(
        ai(
            &[
                "compile",
                "examples/intents/vague.intent.md",
                "--out",
                tmp("a3-v").to_str().unwrap()
            ],
            &[]
        )
        .code,
        5
    );
}

// ── A4: hermetic, isolated synthesis with a hard timeout ─────────────────────
#[test]
fn acc_a4_synthesis_isolated_timeout() {
    // Drive the REAL synthesizer (not the mock) against a fake `axon` that hangs,
    // under a tiny timeout. The child must be killed and synthesis must fail
    // closed (no triple). Skips if `sh` is unavailable.
    let dir = tmp("a4");
    // A stand-in "axon" that ignores args and sleeps far past the timeout.
    let fake = dir.join("hang.sh");
    std::fs::write(&fake, "#!/bin/sh\nsleep 30\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let start = std::time::Instant::now();
    let mut cmd = Command::new(intent_bin());
    cmd.args([
        "compile",
        "examples/intents/summarize.intent.md",
        "--out",
        dir.to_str().unwrap(),
    ]);
    // NOT mock: force the real subprocess synthesizer, pointed at the hang.
    cmd.env_remove("AXON_AI_MOCK");
    cmd.env("AXON_BIN", &fake);
    cmd.env("AXON_INTENT_TIMEOUT_MS", "400");
    cmd.current_dir(workspace_root());
    let out = cmd.output().expect("spawn axon-intent");
    let elapsed = start.elapsed();
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    assert_eq!(
        code, 5,
        "a timed-out synthesis fails closed (exit 5): {stdout}"
    );
    assert!(stdout.contains("timed out"), "cites the timeout: {stdout}");
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "must return promptly after the kill, not wait out the 30s sleep (took {elapsed:?})"
    );
    assert!(!dir.join("summarize.ax").exists(), "no triple on timeout");
}

// ── A5: deterministic — same (intent, seed) under mock ⇒ byte-identical triple ─
#[test]
fn acc_a5_deterministic_compile() {
    let d1 = tmp("a5-1");
    let d2 = tmp("a5-2");
    for d in [&d1, &d2] {
        let c = ai(
            &[
                "compile",
                "examples/intents/summarize.intent.md",
                "--out",
                d.to_str().unwrap(),
            ],
            &[],
        );
        assert_eq!(c.code, 0, "compile: {}", c.stdout);
        // approve so the full triple (incl. .approval) exists for the diff.
        ai(
            &[
                "approve",
                d.join("summarize.axjob").to_str().unwrap(),
                "--by",
                "alice",
                "--accept",
            ],
            &[],
        );
    }
    for f in ["summarize.ax", "summarize.axjob", "summarize.approval"] {
        let a = std::fs::read(d1.join(f)).unwrap();
        let b = std::fs::read(d2.join(f)).unwrap();
        assert_eq!(a, b, "{f} must be byte-identical across mock runs");
    }
}

// ── A6: the approval token binds (program, grant); any edit invalidates it ────
#[test]
fn acc_a6_approval_token_binds_and_tamper_detected() {
    use axon_intent::approval::{from_json, verify_token, ApprovalToken};
    use axon_os::manifest;

    let dir = tmp("a6");
    ai(
        &[
            "compile",
            "examples/intents/summarize.intent.md",
            "--out",
            dir.to_str().unwrap(),
        ],
        &[],
    );
    let a = ai(
        &[
            "approve",
            dir.join("summarize.axjob").to_str().unwrap(),
            "--by",
            "alice",
            "--accept",
        ],
        &[],
    );
    assert_eq!(a.code, 0, "{}", a.stdout);

    let token: ApprovalToken =
        from_json(&std::fs::read_to_string(dir.join("summarize.approval")).unwrap()).unwrap();
    let prog = std::fs::read_to_string(dir.join("summarize.ax")).unwrap();
    let axjob = std::fs::read_to_string(dir.join("summarize.axjob")).unwrap();
    let m = manifest::parse(&axjob, &dir).unwrap();

    // The intact triple verifies.
    assert!(
        verify_token(&token, &prog, &m.grant).is_ok(),
        "intact triple verifies"
    );

    // (a) one program byte appended → fail.
    let edited = format!("{prog}// x");
    assert!(
        verify_token(&token, &edited, &m.grant).is_err(),
        "edited program rejected"
    );

    // (b) one grant field changed → fail.
    let mut g = m.grant.clone();
    g.budget.tokens += 1;
    assert!(
        verify_token(&token, &prog, &g).is_err(),
        "edited grant rejected"
    );

    // (c) the token's decision flipped on disk → re-hash fails.
    let mut t = token.clone();
    t.decision = axon_intent::Decision::Rejected;
    assert!(
        verify_token(&t, &prog, &m.grant).is_err(),
        "flipped decision rejected"
    );
}

// ── Core: synthesized program's declared effects ⊆ inferred grant ────────────
#[test]
fn synthesized_job_is_self_admissible() {
    use axon_intent::{prove_admissible, synth::scan_effects};
    use axon_os::manifest;

    let dir = tmp("self-adm");
    ai(
        &[
            "compile",
            "examples/intents/summarize.intent.md",
            "--out",
            dir.to_str().unwrap(),
        ],
        &[],
    );
    let prog = std::fs::read_to_string(dir.join("summarize.ax")).unwrap();
    let axjob = std::fs::read_to_string(dir.join("summarize.axjob")).unwrap();
    let m = manifest::parse(&axjob, &dir).unwrap();
    let declared = scan_effects(&prog);
    // By construction the inferred grant in the emitted .axjob admits the program.
    assert!(
        prove_admissible(&declared, &m.grant).is_ok(),
        "the emitted job is admissible by construction"
    );
}

// ── Core: the inferred grant is least-privilege ──────────────────────────────
#[test]
fn grant_is_least_privilege() {
    // overbroad permits net; the program never uses it → net=∅ in the grant.
    let dir = tmp("lp");
    ai(
        &[
            "compile",
            "examples/intents/overbroad.intent.md",
            "--out",
            dir.to_str().unwrap(),
        ],
        &[],
    );
    let axjob = std::fs::read_to_string(dir.join("overbroad.axjob")).unwrap();
    assert!(
        axjob.contains("net = []"),
        "ceiling permits net but the program never uses it ⇒ net granted nothing: {axjob}"
    );
}

// ── Core: low-confidence synthesis is refused, not shipped ───────────────────
#[test]
fn low_confidence_synthesis_refused() {
    let dir = tmp("lowconf");
    let v = ai(
        &[
            "compile",
            "examples/intents/vague.intent.md",
            "--out",
            dir.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(
        v.code, 5,
        "under-specified intent refused (exit 5): {}",
        v.stdout
    );
    assert!(v.stdout.contains("REFUSED"));
    assert!(
        !dir.join("vague.ax").exists(),
        "no triple is emitted on refusal"
    );
}

// ── Core: approval is invalidated by ANY edit (program or grant) ─────────────
#[test]
fn approval_invalidated_by_any_edit() {
    use axon_intent::approval::{from_json, verify_token, ApprovalToken};
    use axon_os::manifest;

    let dir = tmp("inval");
    ai(
        &[
            "compile",
            "examples/intents/summarize.intent.md",
            "--out",
            dir.to_str().unwrap(),
        ],
        &[],
    );
    ai(
        &[
            "approve",
            dir.join("summarize.axjob").to_str().unwrap(),
            "--by",
            "alice",
            "--accept",
        ],
        &[],
    );
    let token: ApprovalToken =
        from_json(&std::fs::read_to_string(dir.join("summarize.approval")).unwrap()).unwrap();
    let prog = std::fs::read_to_string(dir.join("summarize.ax")).unwrap();
    let m = manifest::parse(
        &std::fs::read_to_string(dir.join("summarize.axjob")).unwrap(),
        &dir,
    )
    .unwrap();

    // Flip every single program byte once → every edit invalidates the token.
    let bytes = prog.as_bytes();
    for i in 0..bytes.len() {
        let mut b = bytes.to_vec();
        b[i] = if b[i] == b'/' { b'.' } else { b'/' };
        if let Ok(edited) = String::from_utf8(b) {
            if edited != prog {
                assert!(
                    verify_token(&token, &edited, &m.grant).is_err(),
                    "editing program byte {i} must invalidate the token"
                );
            }
        }
    }
    // And any grant edit.
    let mut g = m.grant.clone();
    g.fs_read = vec!["./elsewhere/".into()];
    assert!(
        verify_token(&token, &prog, &g).is_err(),
        "grant edit invalidates"
    );
}

// ── Cross-spec handoff (R22 → R21): R21 refuses an edited program after approval ─
#[test]
fn r21_refuses_edited_program_after_approval() {
    // R22's substance for the cross-spec contract (R22 §3.5): the approval token
    // binds the exact program; editing it after approval makes `verify_token`
    // (the check R21 calls on handoff) fail. We demonstrate the binding through
    // our own verifier — a passing, meaningful test.
    //
    // CROSS-CRATE HOOK (main agent): wire verify_token into axon-os::supervisor::run
    //   In axon-os `run`, when a `<run-id|name>.approval` is present beside the
    //   manifest, load it and call
    //   `axon_intent::approval::verify_token(&token, &program_src, &manifest.grant)`
    //   BEFORE gate/mint/exec; on Err return `Verdict::Denied{ reason:
    //   "unapproved or edited after approval", axis: "approval" }` (exit 8). That
    //   is the 1–2 line integration; the binding logic proven here is unchanged.
    use axon_intent::approval::{from_json, verify_token, ApprovalToken};
    use axon_os::manifest;

    let dir = tmp("xspec");
    ai(
        &[
            "compile",
            "examples/intents/summarize.intent.md",
            "--out",
            dir.to_str().unwrap(),
        ],
        &[],
    );
    ai(
        &[
            "approve",
            dir.join("summarize.axjob").to_str().unwrap(),
            "--by",
            "alice",
            "--accept",
        ],
        &[],
    );
    let token: ApprovalToken =
        from_json(&std::fs::read_to_string(dir.join("summarize.approval")).unwrap()).unwrap();
    let m = manifest::parse(
        &std::fs::read_to_string(dir.join("summarize.axjob")).unwrap(),
        &dir,
    )
    .unwrap();

    // Before editing: the on-disk program verifies (R21 would run it).
    let prog = std::fs::read_to_string(dir.join("summarize.ax")).unwrap();
    assert!(
        verify_token(&token, &prog, &m.grant).is_ok(),
        "approved triple is honored"
    );

    // Append a byte to the .ax (the edit-after-approval attack).
    std::fs::write(
        dir.join("summarize.ax"),
        format!("{prog}\n// sneaky edit\n"),
    )
    .unwrap();
    let edited = std::fs::read_to_string(dir.join("summarize.ax")).unwrap();
    assert!(
        verify_token(&token, &edited, &m.grant).is_err(),
        "R21's handoff check (verify_token) refuses an edited-after-approval program"
    );

    // And the library convenience R21 would call on the triple path:
    assert!(
        axon_intent::cli::verify_triple(&dir.join("summarize.axjob")).is_err(),
        "verify_triple refuses the tampered triple"
    );
}
