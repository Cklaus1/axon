//! The trusted guest runner, driven on the host against the REAL interpreter
//! (`AXON_BIN`, default the workspace's `debug/axon`), so the completion
//! tokens are genuine PCI tokens.
//!
//! Each test states what the runner must DISCRIMINATE:
//! * wrong manifest digest, wrong candidate, wrong suite, a non-identifier
//!   test (all REFUSED, and nothing executed);
//! * a missing test, a substring match, a failing test;
//! * a replayed proof (another attempt or trial);
//! * secret custody: the child sees EOF on stdin, the secret file is unreadable
//!   to it, and neither S nor K appears in anything the runner emits.

use axon_psv::runner::{report_for, run, run_and_emit, RunnerConfig};
use axon_psv::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn axon() -> PathBuf {
    let p = std::env::var_os("AXON_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let t = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
            t.join("debug/axon")
        });
    assert!(
        p.exists(),
        "the runner tests need the real interpreter at {} — build it with \
         `cargo build -p axon-core --no-default-features --bin axon` (or set AXON_BIN). \
         This is a FAILURE, not a skip.",
        p.display()
    );
    p
}

/// It also defines its OWN `@[test]` — which the guest must never collect (B1).
const CANDIDATE: &str =
    "fn double(x: i64) -> i64 { x * 2 }\n\n@[test]\nfn t_cand_probe() { assert(true) }\n";
/// A candidate module that SHADOWS the operator suite's helper if the module
/// path ever puts the candidate first.
const PLANTED_HELPER: &str = "fn want() -> i64 { 0 }\n";
/// A candidate module that reads the suite's ANSWER — refused under sealing
/// (PCI, E0004), a cheat that passes without it.
const CHEAT: &str = "fn double2(x: i64) -> i64 { expected() }\n";
const SUITE_HELPER: &str = "fn want() -> i64 { 42 }\n";
const SEAL_SUITE: &str = "mod g\nuse g.{double2}\n\nfn expected() -> i64 { 42 }\n\n@[test]\nfn t_seal() { assert_eq(double2(21), expected()) }\n";

/// `{secret}` is replaced with the secret file's path.
const SUITE: &str = r#"mod f
mod helper
use f.{double}
use helper.{want}

@[test]
fn t_ok() { assert_eq(double(21), 42) }

@[test]
fn t_ok_edge() { assert_eq(double(1), 2) }

@[test]
fn t_fail() { assert_eq(double(1), 3) }

@[test]
fn t_pair() { assert_eq(double(1), 2) }

@[test]
fn t_pair_breaks() { assert_eq(double(1), 5) }

@[test]
fn t_helper() { assert_eq(helper_want(), 42) }

fn helper_want() -> i64 { want() }

@[test]
fn t_custody() {
    let line = read_line()
    println("STDIN-SAW:[{line}]")
    match read_file("{secret}") {
        Ok(s) => println("SECRET-READ:{s}")
        Err(e) => println("SECRET-REFUSED:{e}")
    }
    assert_eq(double(2), 4)
}
"#;

struct Fx {
    _d: tempfile::TempDir,
    cfg: RunnerConfig,
    m: LaunchManifest,
    secret: [u8; 32],
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A job for `test` in `accept.ax`; `drop` runs the child as nobody (needs root).
fn fixture(test: &str, drop: bool) -> Fx {
    fixture_at("accept.ax", test, drop)
}

fn fixture_at(entry: &str, test: &str, drop: bool) -> Fx {
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let (cand, suite, job, out) = (
        d.path().join("candidate"),
        d.path().join("suite"),
        d.path().join("job"),
        d.path().join("out"),
    );
    for p in [&cand, &suite, &job, &out] {
        std::fs::create_dir(p).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(cand.join("f.ax"), CANDIDATE).unwrap();
    std::fs::write(cand.join("helper.ax"), PLANTED_HELPER).unwrap();
    std::fs::write(cand.join("g.ax"), CHEAT).unwrap();
    std::fs::write(suite.join("helper.ax"), SUITE_HELPER).unwrap();
    std::fs::write(suite.join("seal.ax"), SEAL_SUITE).unwrap();
    let secret_path = job.join("completion-secret");
    std::fs::write(
        suite.join("accept.ax"),
        SUITE.replace("{secret}", secret_path.to_str().unwrap()),
    )
    .unwrap();
    for f in [
        cand.join("f.ax"),
        cand.join("helper.ax"),
        cand.join("g.ax"),
        suite.join("helper.ax"),
        suite.join("seal.ax"),
        suite.join("accept.ax"),
    ] {
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let secret: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
    std::fs::write(&secret_path, secret).unwrap();
    std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o400)).unwrap();
    let q = Quota::default();
    let m = LaunchManifest {
        schema: LAUNCH_MANIFEST_SCHEMA.into(),
        operation_id: "op-1".into(),
        task_id: "task-1".into(),
        trial_id: "trial-1".into(),
        attempt_id: "attempt-1".into(),
        backend_profile: PROTECTED_PROFILE.into(),
        fabric_revision: "f".repeat(40),
        verifier_sha256: "d".repeat(64),
        qualification_sha256: "1".repeat(64),
        host_config_sha256: "2".repeat(64),
        launcher_sha256: "3".repeat(64),
        firecracker_sha256: "4".repeat(64),
        profile_manifest_sha256: "5".repeat(64),
        guest: GuestDigests {
            kernel_sha256: "6".repeat(64),
            rootfs_sha256: "7".repeat(64),
            axon_sha256: "8".repeat(64),
            init_sha256: "9".repeat(64),
        },
        policy_sha256: "a".repeat(64),
        suite: SuiteRef {
            id: "acceptance".into(),
            version: axon_workspace_recipe::tree_version_ref(&suite, &q).unwrap(),
            entry: entry.into(),
            test: test.into(),
            tree_digest: axon_workspace_recipe::tree_version_ref(&suite, &q).unwrap(),
            registry_sha256: "c".repeat(64),
        },
        candidate: CandidateRef {
            workspace_version: axon_workspace_recipe::tree_version_ref(&cand, &q).unwrap(),
            tree_digest: axon_workspace_recipe::tree_version_ref(&cand, &q).unwrap(),
        },
        completion: Completion {
            scheme: COMPLETION_SCHEME.into(),
        },
        observation_nonce: "e".repeat(32),
        limits: Limits {
            wall_time_ms: 60_000,
            output_bytes: 1 << 20,
        },
    };
    std::fs::write(job.join("launch-manifest.json"), m.bytes()).unwrap();
    // The interpreter, copied where the unprivileged test uid can execute it:
    // the build's target dir may sit under a 0700 home (measured: the frozen
    // run's `$HOME/.cache` target made the dropped child fail to exec, so the
    // custody test depended on WHERE the build lived).
    let axon_copy = d.path().join("axon");
    std::fs::copy(axon(), &axon_copy).unwrap();
    std::fs::set_permissions(&axon_copy, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cfg = RunnerConfig {
        manifest: job.join("launch-manifest.json"),
        secret: secret_path,
        candidate: cand,
        suite,
        out,
        axon: axon_copy.clone(),
        runner_exe: axon_copy,
        expected_manifest_sha256: m.digest(),
        drop: drop.then_some((65534, 65534)),
        effect_ceiling: None,
    };
    Fx {
        _d: d,
        cfg,
        m,
        secret,
    }
}

/// What the HOST would expect for `test` under ITS OWN key (the interpreter's
/// token derivation: HMAC(K, "axon-test-completion/1\0" + name)).
fn host_token(secret: &[u8; 32], m: &LaunchManifest, test: &str) -> String {
    let k = completion_key(secret, m);
    let mut msg = b"axon-test-completion/1\0".to_vec();
    msg.extend_from_slice(test.as_bytes());
    hex(&hmac_sha256(&k, &msg))
}

fn ran(fx: &Fx) -> bool {
    fx.cfg.out.join("test-stdout").exists()
}

#[test]
fn the_named_test_passes_with_a_token_the_host_verifies() {
    let fx = fixture("t_ok", false);
    let (v, sha) = run_and_emit(&fx.cfg).unwrap();
    assert_eq!(v.status, GuestStatus::Passed, "{v:?}");
    assert!(v.inputs.matches);
    assert_eq!(v.launch_manifest_sha256, fx.m.digest());
    let r = v.report.unwrap();
    // Exactly the named test — `t_ok_edge` matched the substring filter and
    // is ignored.
    assert_eq!(r.passed, vec!["t_ok".to_string()]);
    assert_eq!(
        r.completion,
        vec![("t_ok".into(), host_token(&fx.secret, &fx.m, "t_ok"))]
    );
    assert_eq!(
        sha,
        sha256_hex(&std::fs::read(fx.cfg.out.join("verdict.json")).unwrap())
    );
}

#[test]
fn a_failing_or_absent_test_is_never_a_pass() {
    let fx = fixture("t_fail", false);
    assert_eq!(run(&fx.cfg).status, GuestStatus::Failed);
    let fx = fixture("t_absent", false);
    let v = run(&fx.cfg);
    assert_eq!(v.status, GuestStatus::Unknown, "{v:?}");
}

/// A wrong manifest digest, candidate, suite or test identity: REFUSED, and
/// the interpreter never ran.
#[test]
fn nothing_executes_unless_every_input_is_the_named_one() {
    // The digest Fabric named is another.
    let mut fx = fixture("t_ok", false);
    fx.cfg.expected_manifest_sha256 = "0".repeat(64);
    let v = run(&fx.cfg);
    assert_eq!(v.status, GuestStatus::Refused);
    assert!(v.refusal.unwrap().contains("is not the"));
    assert!(!ran(&fx));

    // The candidate changed after the manifest was built (A1).
    let fx = fixture("t_ok", false);
    std::fs::write(
        fx.cfg.candidate.join("f.ax"),
        "fn double(x: i64) -> i64 { 42 }\n",
    )
    .unwrap();
    let v = run(&fx.cfg);
    assert_eq!(v.status, GuestStatus::Refused);
    assert!(v.refusal.unwrap().starts_with("candidate tree is"));
    assert!(!ran(&fx));

    // The suite is another (A2).
    let fx = fixture("t_ok", false);
    std::fs::write(fx.cfg.suite.join("extra.ax"), "").unwrap();
    let v = run(&fx.cfg);
    assert!(v.refusal.unwrap().starts_with("suite tree is"));
    assert!(!ran(&fx));

    // A test name that is not an identifier (an option, a pattern) (A3).
    for bad in ["t_ok --json", "-x", "t_*", ""] {
        let fx = fixture(bad, false);
        let v = run(&fx.cfg);
        assert_eq!(v.status, GuestStatus::Refused, "{bad:?}");
        assert!(v.refusal.unwrap().contains("not an identifier"), "{bad:?}");
        assert!(!ran(&fx));
    }

    // The secret is not a 32-byte secret.
    let fx = fixture("t_ok", false);
    std::fs::set_permissions(&fx.cfg.secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&fx.cfg.secret, b"short").unwrap();
    let v = run(&fx.cfg);
    assert!(v.refusal.unwrap().contains("not 32"));
    assert!(!ran(&fx));
}

/// A11: a proof from one attempt (or trial) does not verify for another —
/// the host derives the key from ITS manifest.
#[test]
fn a_completion_proof_does_not_replay_across_attempts_or_trials() {
    let fx = fixture("t_ok", false);
    let v = run(&fx.cfg);
    let token = v.report.unwrap().completion[0].1.clone();
    assert_eq!(token, host_token(&fx.secret, &fx.m, "t_ok"));
    for edit in [
        |m: &mut LaunchManifest| m.attempt_id = "attempt-2".into(),
        |m: &mut LaunchManifest| m.trial_id = "trial-2".into(),
        |m: &mut LaunchManifest| m.suite.test = "t_ok_edge".into(),
    ] {
        let mut other = fx.m.clone();
        edit(&mut other);
        assert_ne!(token, host_token(&fx.secret, &other, "t_ok"));
    }
    // And under another attempt's secret.
    assert_ne!(token, host_token(&[0u8; 32], &fx.m, "t_ok"));
}

/// Secret custody. The child reads stdin and finds EOF (the key line was
/// consumed by the interpreter before program code ran); nothing the runner
/// emits contains S or K. As root, the child runs as nobody and cannot read
/// the secret file.
#[test]
fn neither_the_secret_nor_the_key_reaches_candidate_code_or_the_output() {
    let root = unsafe { libc::geteuid() } == 0;
    let fx = fixture("t_custody", root);
    let v = run(&fx.cfg);
    let stdout = std::fs::read_to_string(fx.cfg.out.join("test-stdout")).unwrap();
    assert!(
        stdout.contains("STDIN-SAW:[]"),
        "stdin was not at EOF: {stdout}"
    );
    let (s_hex, k_hex) = (hex(&fx.secret), hex(&completion_key(&fx.secret, &fx.m)));
    let verdict = String::from_utf8(v.bytes()).unwrap();
    for text in [&stdout, &verdict] {
        assert!(!text.contains(&s_hex) && !text.contains(&k_hex), "{text}");
    }
    if root {
        // Refused BECAUSE of the uid drop — not for an incidental reason. A
        // first version accepted any refusal and passed with the drop removed:
        // the secret is not UTF-8, so `read_file` failed on encoding before
        // permission ever mattered (found by the `no-drop` mutant).
        assert!(
            stdout.contains("SECRET-REFUSED:") && stdout.contains("Permission denied"),
            "{stdout}"
        );
        assert!(!stdout.contains("SECRET-READ:"), "{stdout}");
        assert_eq!(v.status, GuestStatus::Passed, "{v:?}");
    } else {
        eprintln!("note: not root, so the uid drop (secret-file refusal) is not exercised");
    }
}

/// The report reader: exactly one result line for the exact name, with a
/// token, is a pass; a duplicated name (a candidate printing a forged line) is
/// Unknown, never a pass.
#[test]
fn a_forged_or_duplicated_result_line_is_not_a_pass() {
    let ok = r#"{"name":"t_ok","status":"ok","duration_ms":1,"completion":"aa"}"#;
    assert_eq!(report_for(ok, "t_ok").0, GuestStatus::Passed);
    assert_eq!(
        report_for(&format!("{ok}\n{ok}"), "t_ok").0,
        GuestStatus::Unknown
    );
    let fail = r#"{"name":"t_ok","status":"failed","duration_ms":1,"message":"x"}"#;
    assert_eq!(
        report_for(&format!("{ok}\n{fail}"), "t_ok").0,
        GuestStatus::Unknown
    );
    let no_token = r#"{"name":"t_ok","status":"ok","duration_ms":1}"#;
    assert_eq!(report_for(no_token, "t_ok").0, GuestStatus::Unknown);
    assert_eq!(report_for(ok, "t_o").0, GuestStatus::Unknown, "substring");
}

/// With `--exact`, only the registered test runs: a substring sibling that
/// FAILS (`t_pair_breaks` for `t_pair`) never runs beside it, so it can neither
/// sink an honest candidate nor stand in for the named test.
#[test]
fn a_suite_sibling_does_not_run_beside_the_registered_test() {
    let fx = fixture("t_pair", false);
    let v = run(&fx.cfg);
    assert_eq!(v.status, GuestStatus::Passed, "{v:?}");
    assert_eq!(v.exit_code, Some(0));
    let r = v.report.unwrap();
    assert_eq!(r.passed, vec!["t_pair".to_string()]);
    assert!(r.failed.is_empty(), "{r:?}");
}

/// B1: a candidate's own `@[test]` is never collected — even under the very
/// name the manifest registers (the suite defines no such test). It yields no
/// verdict, never a Passed with a genuine token.
#[test]
fn a_candidate_cannot_supply_the_registered_test() {
    let fx = fixture("t_cand_probe", false);
    let v = run(&fx.cfg);
    assert_eq!(v.status, GuestStatus::Unknown, "{v:?}");
    assert!(v.report.unwrap().passed.is_empty());
}

/// The operator suite's own modules come first on the module path: a
/// candidate that plants `helper.ax` does not replace the suite's helper.
#[test]
fn a_candidate_cannot_shadow_the_suites_own_modules() {
    let fx = fixture("t_helper", false);
    let v = run(&fx.cfg);
    assert_eq!(v.status, GuestStatus::Passed, "{v:?}");
}

/// The candidate runs SEALED (PCI, E0004): a candidate module that reads the
/// suite's answer is refused, so the cheat never passes.
#[test]
fn a_sealed_candidate_cannot_read_the_suites_answer() {
    let fx = fixture_at("seal.ax", "t_seal", false);
    let v = run(&fx.cfg);

    assert_ne!(v.status, GuestStatus::Passed, "{v:?}");
    let err = std::fs::read_to_string(fx.cfg.out.join("test-stderr")).unwrap();
    assert!(err.contains("E0004"), "refused for another reason: {err}");
}
