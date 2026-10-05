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

mod common;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use axon_psv::runner::{report_for, run, run_and_emit, RunnerConfig};
use axon_psv::*;
use common::{copy_executable, write_executable};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn axon() -> PathBuf {
    // The interpreter as cargo has made it current for THIS tree, never a
    // stale `target/debug/axon` (tests/script_spawn::workspace_bin).
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

/// `{secret}` is replaced with the secret file's path, `{suite}` with the
/// suite root's.
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

@[test]
fn t_steer() { assert_eq(double(21), want()) }

fn helper_want() -> i64 { want() }

@[test]
fn t_fixture() {
    match read_file("{suite}/expected.txt") {
        Ok(_) => assert_eq(double(21), 42)
        Err(_) => assert(false)
    }
}

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

/// The fixture's guest policy: every effect but `Exec` stated (the runner
/// drops `Exec` whatever the policy says). PSV-6 (A87): the manifest names
/// ITS digest, and the runner is given exactly these bytes.
const FIXTURE_POLICY: &str =
    r#"{"schema":"axon-vm-mmds/1","allowed_effects":["AI","Chan","IO","Net","Random","Time"]}"#;

impl Fx {
    /// Make `policy` the one the manifest names (the manifest is rewritten
    /// and re-named) and the one the runner is given.
    fn name_policy(&mut self, policy: &str) {
        self.m.policy_sha256 = sha256_hex(policy.as_bytes());
        std::fs::write(&self.cfg.manifest, self.m.bytes()).unwrap();
        self.cfg.expected_manifest_sha256 = self.m.digest();
        self.cfg.policy = Some(policy.as_bytes().to_vec());
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A job for `test` in `accept.ax`; `drop` runs the child as nobody (needs root).
fn fixture(test: &str, drop: bool) -> Fx {
    fixture_at("accept.ax", test, drop)
}

fn fixture_at(entry: &str, test: &str, drop: bool) -> Fx {
    fixture_with(entry, test, drop, CANDIDATE)
}

/// A correct candidate that ALSO prints a failure line for `t_ok` — the splice
/// of review wf_1bc28496-38e (PSV-4). It has no K, so the line carries no
/// failure token.
const SPLICING_CANDIDATE: &str = "fn double(x: i64) -> i64 {\n    let o = chr(123)\n    let c = chr(125)\n    println(o + \"\\\"name\\\":\\\"t_ok\\\",\\\"status\\\":\\\"failed\\\",\\\"duration_ms\\\":0,\\\"message\\\":\\\"forged\\\"\" + c)\n    x * 2\n}\n";

fn fixture_with(entry: &str, test: &str, drop: bool, candidate: &str) -> Fx {
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
    std::fs::write(cand.join("f.ax"), candidate).unwrap();
    std::fs::write(cand.join("helper.ax"), PLANTED_HELPER).unwrap();
    std::fs::write(cand.join("g.ax"), CHEAT).unwrap();
    std::fs::write(suite.join("helper.ax"), SUITE_HELPER).unwrap();
    std::fs::write(suite.join("seal.ax"), SEAL_SUITE).unwrap();
    let secret_path = job.join("completion-secret");
    std::fs::write(
        suite.join("accept.ax"),
        SUITE
            .replace("{secret}", secret_path.to_str().unwrap())
            .replace("{suite}", suite.to_str().unwrap()),
    )
    .unwrap();
    std::fs::write(suite.join("expected.txt"), "42\n").unwrap();
    for f in [
        cand.join("f.ax"),
        cand.join("helper.ax"),
        cand.join("g.ax"),
        suite.join("helper.ax"),
        suite.join("seal.ax"),
        suite.join("accept.ax"),
        suite.join("expected.txt"),
    ] {
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    // Printable ASCII on purpose: candidate code reads files as UTF-8, so a
    // binary secret would be unreadable to it for an incidental reason (an
    // encoding error) and hide whether the uid drop is what refuses it.
    let secret: [u8; 32] = std::array::from_fn(|i| b'A' + ((i as u8).wrapping_mul(7) % 26));
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
        policy_sha256: sha256_hex(FIXTURE_POLICY.as_bytes()),
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
        authority: AuthorityRef {
            epoch: 0,
            tenant_id: "tenant-t".into(),
            task_family: "family-f".into(),
        },
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
    copy_executable(axon(), &axon_copy, 0o755);
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
        policy: Some(FIXTURE_POLICY.as_bytes().to_vec()),
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

/// A failure is a verdict only with the interpreter's KEYED failure token: a
/// correct candidate that prints a failure line over its own genuine pass
/// yields no verdict, never a Failed (review wf_1bc28496-38e, PSV-4).
#[test]
fn a_failure_line_printed_by_the_candidate_is_not_a_verdict() {
    let fx = fixture_with("accept.ax", "t_ok", false, SPLICING_CANDIDATE);
    let v = run(&fx.cfg);
    let out = std::fs::read_to_string(fx.cfg.out.join("test-stdout")).unwrap();
    assert!(out.contains("\"forged\""), "the splice happened: {out}");
    assert_eq!(v.status, GuestStatus::Unknown, "{v:?}");
    // Control: the interpreter's own failure carries the keyed token.
    let fx = fixture("t_fail", false);
    let v = run(&fx.cfg);
    assert_eq!(v.status, GuestStatus::Failed, "{v:?}");
    let key = axon_psv::completion_key(&fx.secret, &fx.m);
    let out = std::fs::read_to_string(fx.cfg.out.join("test-stdout")).unwrap();
    assert_eq!(axon_psv::keyed_outcome(&out, "t_fail", &key), Some(false));
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

    // The secret is not a 32-byte secret: too short, and too long. The
    // length guard is the only check (the copy after it is total), so without
    // it the job would run under a padded or truncated secret.
    for bad in [&b"short"[..], &[7u8; 64][..]] {
        let fx = fixture("t_ok", false);
        std::fs::set_permissions(&fx.cfg.secret, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&fx.cfg.secret, bad).unwrap();
        let v = run(&fx.cfg);
        assert!(
            !ran(&fx) && v.status == GuestStatus::Refused,
            "ATTACK: a {}-byte completion secret was accepted and the test ran: {v:?}",
            bad.len()
        );
        assert!(v.refusal.unwrap().contains("not 32"));
    }
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
        // the secret was not UTF-8, so `read_file` failed on encoding before
        // permission ever mattered (found by the `no-drop` mutant). The
        // fixture's secret is now printable, so the drop is the only guard: as
        // root, without it, candidate code reads the 0400 file.
        assert!(
            !stdout.contains("SECRET-READ:"),
            "ATTACK: candidate code read the completion secret: {stdout}"
        );
        assert!(
            stdout.contains("SECRET-REFUSED:") && stdout.contains("Permission denied"),
            "{stdout}"
        );
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
    // The attack (M220): `t_pair_breaks`, which the filter matches as a
    // SUBSTRING, ran beside the registered test. Only the interpreter's own
    // result line for it counts (the verdict's report keeps just the named
    // test); any other failure is the control below.
    let out = std::fs::read_to_string(fx.cfg.out.join("test-stdout")).unwrap_or_default();
    assert!(
        !out.contains("\"name\":\"t_pair_breaks\""),
        "ATTACK: a suite sibling ran beside the registered test t_pair: {out} {v:?}"
    );
    assert_eq!(v.status, GuestStatus::Passed, "control: {v:?}");
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

/// The operator suite's own modules come first on the module path (M177). An
/// HONEST candidate that happens to hold a module named like one of the
/// suite's (`helper.ax`) is judged against the suite's helper and passes.
///
/// With the order reversed, the candidate's copy is the first match for the
/// suite's `use helper`: the sealed-import rule (M436) then refuses the run
/// (E0901, Unknown), and without that rule the candidate's `want` defines the
/// suite's helper (a keyed Failed for an honest candidate). Either way the
/// verdict is no longer the suite's, and the order is the ONLY guard: M436
/// turns a wrong verdict into no verdict, it does not restore the right one.
/// The attack assertion fires only on evidence that the candidate's file was
/// resolved first (its E0901, or the planted `want` failing `t_helper`); any
/// other failure is the control below.
#[test]
fn a_candidate_cannot_shadow_the_suites_own_modules() {
    let fx = fixture("t_helper", false);
    let v = run(&fx.cfg);
    let err = std::fs::read_to_string(fx.cfg.out.join("test-stderr")).unwrap_or_default();
    let candidate_first = err.contains("module `helper`: a sealed module may not supply it")
        || (v.status == GuestStatus::Failed
            && v.report
                .as_ref()
                .is_some_and(|r| r.failed.iter().any(|t| t == "t_helper")));
    assert!(
        !candidate_first,
        "ATTACK: the candidate's helper.ax was resolved before the suite's own helper: \
         {:?} {v:?} {err}",
        v.status
    );
    assert_eq!(v.status, GuestStatus::Passed, "control: {v:?} {err}");
}

/// The SECURITY reading of the same order: a candidate whose `double` is
/// broken plants a `helper.ax` that moves the expected value to match. The
/// suite's helper must judge it (Failed). Suite-first order (M177) and the
/// sealed-import rule (M436) each refuse this alone; only with both removed
/// does the planted helper define the suite's and the check PASS. The guest
/// twin of Fabric's `a_candidate_cannot_shadow_a_module_of_the_suite`.
#[test]
fn a_planted_helper_cannot_steer_the_suite_to_pass() {
    let fx = fixture_with(
        "accept.ax",
        "t_steer",
        false,
        "fn double(x: i64) -> i64 { x * 0 }\n",
    );
    let v = run(&fx.cfg);
    let err = std::fs::read_to_string(fx.cfg.out.join("test-stderr")).unwrap_or_default();
    assert_ne!(
        v.status,
        GuestStatus::Passed,
        "ATTACK: the candidate's helper.ax judged its own broken double: {v:?} {err}"
    );
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

/// If the interpreter dies before writing its own line, the output can hold a
/// LONE failure line printed by candidate code, with a failing exit. That is
/// still no verdict without the keyed failure token (review wf_1bc28496-38e,
/// PSV-4). Control: the same stand-in, holding K and emitting the correct
/// token, IS a failure.
#[test]
fn a_lone_unkeyed_failure_line_is_not_a_verdict() {
    for (keyed, want) in [(false, GuestStatus::Unknown), (true, GuestStatus::Failed)] {
        let mut fx = fixture("t_ok", false);
        let token = if keyed {
            r#"$(printf 'axon-test-failed/1\0t_ok' | openssl dgst -sha256 -mac HMAC -macopt hexkey:$K | sed 's/.*= //')"#
        } else {
            "0000"
        };
        let script = fx._d.path().join(format!("dying-axon-{keyed}.sh"));
        write_executable(
            &script,
            format!(
                "#!/bin/sh\nread K\nprintf '{{\"name\":\"t_ok\",\"status\":\"failed\",\"duration_ms\":0,\"message\":\"x\",\"completion\":\"%s\"}}\\n' \"{token}\"\nexit 1\n"
            ),
            0o755,
        );
        fx.cfg.axon = script;
        let v = run(&fx.cfg);
        if keyed {
            assert_eq!(v.status, want, "control: keyed={keyed}: {v:?}");
        } else {
            assert_eq!(
                v.status, want,
                "ATTACK: a lone failure line without K's failure token was recorded as a \
                 failure (keyed={keyed}): {v:?}"
            );
        }
    }
}

/// The candidate runs in the process that holds K, so that process gets a
/// ceiling WITHOUT `Exec`, whatever the guest policy granted (review
/// wf_ecfcd666-6c9, PSV-3: under an Exec grant a spawned helper read K from
/// its memory in the guest). A stand-in interpreter reports the ceiling it was
/// given; the other effects pass through.
#[test]
fn the_process_holding_k_is_given_no_exec() {
    let mut fx = fixture("t_ok", false);
    let script = fx._d.path().join("ceiling-axon.sh");
    write_executable(
        &script,
        "#!/bin/sh\nread K\necho \"CEIL:[$AXON_ALLOWED_EFFECTS]\"\nexit 1\n",
        0o755,
    );
    fx.cfg.axon = script;
    fx.name_policy(r#"{"schema":"axon-vm-mmds/1","allowed_effects":["IO","Exec","Net"]}"#);
    run(&fx.cfg);
    let out = std::fs::read_to_string(fx.cfg.out.join("test-stdout")).unwrap();
    assert!(out.contains("CEIL:[IO,Net]"), "{out}");
}

/// A pass needs BOTH the keyed completion token AND a clean exit. The token is
/// genuine here: the real interpreter runs the passing test and prints it. But
/// the run then exits 3, as a crash after the output would. That is Unknown,
/// never Passed. This is the `Some(0)` of the runner's Passed arm (mutation
/// M293). It replaces M176, whose guard was refactored into that arm and whose
/// named test no longer existed (C9 re-audit).
#[test]
fn a_genuine_keyed_pass_from_a_run_that_exits_non_zero_is_not_a_pass() {
    let mut fx = fixture("t_ok", false);
    let real = fx.cfg.axon.clone();
    let wrapper = real.with_file_name("axon-exit3");
    write_executable(
        &wrapper,
        format!("#!/bin/sh\n'{}' \"$@\"\nexit 3\n", real.display()),
        0o755,
    );
    fx.cfg.axon = wrapper;
    let v = run(&fx.cfg);
    let stdout = std::fs::read_to_string(fx.cfg.out.join("test-stdout")).unwrap_or_default();
    assert!(
        stdout.contains(&host_token(&fx.secret, &fx.m, "t_ok")),
        "setup: the real interpreter's genuine keyed pass must be in the output: {stdout}"
    );
    assert_eq!(v.exit_code, Some(3), "setup: the run exited non-zero");
    assert_eq!(
        v.status,
        GuestStatus::Unknown,
        "a genuine keyed pass from a run that exited non-zero was reported {:?}",
        v.status
    );
}

/// PSV-2 (C9 dev review), the reviewer's reproduction through the REAL
/// runner: a named POSIX ACL entry `user:65534:---` on a suite fixture leaves
/// its mode 0644 and the tree digest unchanged, but denies the dropped test
/// child the file, so a CORRECT candidate produced a genuine KEYED Failed.
/// The runner must refuse the input before anything executes. Control: the
/// same job without the ACL is a keyed pass.
#[test]
fn an_input_carrying_an_acl_is_refused_not_run() {
    let root = unsafe { libc::geteuid() } == 0;
    let fx = fixture("t_fixture", root);
    let v = run(&fx.cfg);
    assert_eq!(v.status, GuestStatus::Passed, "control: {v:?}");

    let fx = fixture("t_fixture", root);
    let f = fx.cfg.suite.join("expected.txt");
    let q = Quota::default();
    let before = axon_workspace_recipe::tree_version_ref(&fx.cfg.suite, &q).unwrap();
    let mut acl = 2u32.to_le_bytes().to_vec();
    for (tag, perm, id) in [
        (0x01u16, 6u16, u32::MAX),
        (0x02, 0, 65534),
        (0x04, 4, u32::MAX),
        (0x10, 4, u32::MAX),
        (0x20, 4, u32::MAX),
    ] {
        acl.extend(tag.to_le_bytes());
        acl.extend(perm.to_le_bytes());
        acl.extend(id.to_le_bytes());
    }
    let c = |s: &str| std::ffi::CString::new(s).unwrap();
    let (cp, cn) = (c(f.to_str().unwrap()), c("system.posix_acl_access"));
    let r = unsafe {
        libc::lsetxattr(
            cp.as_ptr(),
            cn.as_ptr(),
            acl.as_ptr() as *const libc::c_void,
            acl.len(),
            0,
        )
    };
    assert_eq!(
        r,
        0,
        "cannot set a POSIX ACL under TMPDIR ({}): this is a FAILURE, not a skip",
        std::io::Error::last_os_error()
    );
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o7777,
        0o644
    );
    assert_eq!(
        axon_workspace_recipe::tree_version_ref(&fx.cfg.suite, &q).unwrap(),
        before,
        "the digest sees the ACL: not the case it is blind to"
    );
    let v = run(&fx.cfg);
    assert!(
        v.status == GuestStatus::Refused && !ran(&fx),
        "ATTACK: an input carrying a POSIX ACL was not refused and the job ran \
         (status {:?}{}): the child's view of the input is not what was checked: {v:?}",
        v.status,
        if root {
            ""
        } else {
            "; not root, so the uid drop is not exercised"
        }
    );
    assert!(!v.inputs.matches);
    let why = v.refusal.unwrap_or_default();
    assert!(
        why.contains("expected.txt carrying extended attribute system.posix_acl_access"),
        "refused for another reason: {why}"
    );
}

/// C9 round 1b: the guest kernel's ext4 has no `noacl` mount option (measured:
/// the boot test's guest rebooted on "ext4: Unknown parameter 'noacl'"), so the
/// runner itself refuses job files that carry any extended attribute. An ACL on
/// the job drive could GRANT the unprivileged test uid the 0400 secret.
#[test]
fn a_job_file_carrying_an_acl_is_refused_not_run() {
    let root = unsafe { libc::geteuid() } == 0;
    let c = |s: &str| std::ffi::CString::new(s).unwrap();
    // user:65534:r-- on a 0400 file: a GRANT, not a restriction.
    let mut grant = 2u32.to_le_bytes().to_vec();
    for (tag, perm, id) in [
        (0x01u16, 4u16, u32::MAX),
        (0x02, 4, 65534),
        (0x04, 0, u32::MAX),
        (0x10, 4, u32::MAX),
        (0x20, 0, u32::MAX),
    ] {
        grant.extend(tag.to_le_bytes());
        grant.extend(perm.to_le_bytes());
        grant.extend(id.to_le_bytes());
    }
    for (which, name, value) in [
        ("secret", "system.posix_acl_access", grant.clone()),
        ("manifest", "user.axon", b"x".to_vec()),
        ("job dir", "user.axon", b"x".to_vec()),
    ] {
        let fx = fixture("t_fixture", root);
        let target = match which {
            "secret" => fx.cfg.secret.clone(),
            "manifest" => fx.cfg.manifest.clone(),
            _ => fx.cfg.secret.parent().unwrap().to_path_buf(),
        };
        let (cp, cn) = (c(target.to_str().unwrap()), c(name));
        let r = unsafe {
            libc::lsetxattr(
                cp.as_ptr(),
                cn.as_ptr(),
                value.as_ptr() as *const libc::c_void,
                value.len(),
                0,
            )
        };
        assert_eq!(
            r,
            0,
            "cannot set {name} under TMPDIR ({}): this is a FAILURE, not a skip",
            std::io::Error::last_os_error()
        );
        let v = run(&fx.cfg);
        assert!(
            v.status == GuestStatus::Refused && !ran(&fx),
            "ATTACK: a job file ({which}) carrying {name} was not refused and the job ran: {v:?}"
        );
        let why = v.refusal.unwrap_or_default();
        assert!(
            why.contains("carrying extended attribute"),
            "{which}: {why}"
        );
    }
    let fx = fixture("t_fixture", root);
    let v = run(&fx.cfg);
    assert_eq!(v.status, GuestStatus::Passed, "control: {v:?}");
}

/// PSV-6 (C9 round 4; A87), the in-guest guard. The manifest names policy P1;
/// the guest was booted under P2 (a wider ceiling), or under no policy word at
/// all. The runner refuses before anything runs, and its verdict names the
/// policy it WAS given, so Fabric and the loop can see which one it was.
/// Control: the manifest's own policy runs, and the verdict names it.
#[test]
fn the_guest_runs_only_the_policy_the_manifest_names() {
    let p2 = r#"{"schema":"axon-vm-mmds/1","allowed_effects":["AI","Exec","IO","Net","Time"]}"#;
    let mut fx = fixture("t_ok", false);
    fx.cfg.policy = Some(p2.as_bytes().to_vec());
    let v = run(&fx.cfg);
    assert!(
        v.status == GuestStatus::Refused && !ran(&fx),
        "ATTACK: the guest ran policy P2 while the launch manifest names P1: {v:?}"
    );
    let why = v.refusal.clone().unwrap_or_default();
    assert!(why.contains("not the policy_sha256"), "{why}");
    assert_eq!(v.policy_sha256, sha256_hex(p2.as_bytes()));

    let mut fx = fixture("t_ok", false);
    fx.cfg.policy = None;
    let v = run(&fx.cfg);
    assert!(
        v.status == GuestStatus::Refused && !ran(&fx),
        "ATTACK: the guest ran with no policy word while the launch manifest names one: {v:?}"
    );
    assert_eq!(v.policy_sha256, "");

    // Control: the manifest's policy.
    let fx = fixture("t_ok", false);
    let v = run(&fx.cfg);
    assert_eq!(v.status, GuestStatus::Passed, "control: {v:?}");
    assert_eq!(v.policy_sha256, fx.m.policy_sha256);
}

/// PSV-6 (A87): on the protected profile a policy that states no effect
/// ceiling (`allowed_effects` omitted, or `null`) is refused even when it IS
/// the manifest's policy: an absent ceiling is never read as "no ceiling".
/// Control: an explicitly EMPTY list is a ceiling, and the test runs under it
/// (deny every effect).
#[test]
fn a_policy_naming_no_ceiling_never_runs_unrestricted() {
    for p in [
        r#"{"schema":"axon-vm-mmds/1","budget_tokens":0}"#,
        r#"{"schema":"axon-vm-mmds/1","allowed_effects":null,"budget_tokens":0}"#,
    ] {
        let mut fx = fixture("t_ok", false);
        fx.name_policy(p);
        let v = run(&fx.cfg);
        assert!(
            v.status == GuestStatus::Refused && !ran(&fx),
            "ATTACK: a manifest policy with no allowed_effects ran the test with no effect \
             ceiling: {p} {v:?}"
        );
        assert_eq!(v.refusal.as_deref(), Some(NO_CEILING), "{p}");
    }
    // Control: `[]` states a ceiling; the test runs under it.
    let mut fx = fixture("t_ok", false);
    fx.name_policy(r#"{"schema":"axon-vm-mmds/1","allowed_effects":[]}"#);
    let script = fx._d.path().join("ceiling-axon.sh");
    write_executable(
        &script,
        "#!/bin/sh\nread K\necho \"CEIL:[${AXON_ALLOWED_EFFECTS-UNSET}]\"\nexit 1\n",
        0o755,
    );
    fx.cfg.axon = script;
    run(&fx.cfg);
    let out = std::fs::read_to_string(fx.cfg.out.join("test-stdout")).unwrap();
    assert!(
        out.contains("CEIL:[]"),
        "ATTACK: the test did not run under the manifest policy's (empty) ceiling: {out}"
    );
}

/// The policy the runner holds to the manifest is the cmdline word
/// `axon-guest-init` enforces: exactly one `axon.policy=` word, decoded.
#[test]
fn the_runner_reads_the_one_cmdline_policy_word() {
    use axon_psv::runner::policy_from_cmdline;
    let one = "console=ttyS0 axon.policy=eyJhIjoxfQ== axon.psv.manifest=00";
    assert_eq!(policy_from_cmdline(one).as_deref(), Some(&b"{\"a\":1}"[..]));
    assert_eq!(
        policy_from_cmdline("axon.policy=eyJhIjoxfQ== axon.policy=e30="),
        None,
        "two policy words are ambiguous"
    );
    assert_eq!(policy_from_cmdline("console=ttyS0"), None);
    assert_eq!(policy_from_cmdline("axon.policy=!!"), None);
}

// ── C9 round 4 fix wave, rows2 (M780, M781) ─────────────────────────────────

/// A3 (M780): the suite entry is a FILE IN THE SUITE TREE. A manifest whose
/// entry climbs out of it (`../candidate/f.ax`, the candidate's own module,
/// with the candidate's own `@[test]` named) would have the runner judge the
/// candidate by the candidate. The inputs, manifest, policy and test name
/// are otherwise genuine, so only the entry check refuses it: nothing runs.
/// Control: the suite's own entry runs.
#[test]
fn a_suite_entry_outside_the_suite_tree_never_runs() {
    let fx = fixture_at("../candidate/f.ax", "t_cand_probe", false);
    let v = run(&fx.cfg);
    assert!(
        v.status == GuestStatus::Refused && !ran(&fx),
        "ATTACK: the runner ran a suite entry outside the suite tree (the candidate's own \
         module): {v:?}"
    );
    assert!(
        v.refusal
            .as_deref()
            .unwrap_or("")
            .contains("is not a file in the suite tree"),
        "{v:?}"
    );
    let fx = fixture("t_ok", false);
    assert_eq!(run(&fx.cfg).status, GuestStatus::Passed, "control");
}

/// C9 round 4c, ADMIT (amendment 76): a refusal is a verdict of its own
/// (`refused` builds it), and a refusal reported as a pass would launder what
/// the runner declined to run. Same attack as
/// `a_suite_entry_outside_the_suite_tree_never_runs`, judged on the status.
#[test]
fn a_refusal_is_never_reported_as_a_pass() {
    let fx = fixture_at("../candidate/f.ax", "t_cand_probe", false);
    let v = run(&fx.cfg);
    if v.status == GuestStatus::Passed {
        panic!("ATTACK: a refused run (a suite entry outside the suite tree) was reported as a PASS: {v:?}");
    }
    assert_eq!(v.status, GuestStatus::Refused, "{v:?}");
}

/// A candidate that floods stderr past the manifest's output limit while the
/// registered test genuinely passes. The limit is part of the launch: a run
/// that exceeded it was truncated, and a truncated run yields no verdict.
const FLOODING_CANDIDATE: &str = "fn double(x: i64) -> i64 {\n    let i = 0\n    while i < 40000 {\n        eprintln(\"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\")\n        i = i + 1\n    }\n    x * 2\n}\n";

/// PSV limits (M781): a run whose output exceeded `limits.output_bytes` is
/// refused, never verdicted, even when its keyed pass line is intact (the
/// flood is on stderr). Control: the same candidate without the flood passes.
#[test]
fn a_run_that_exceeded_its_output_limit_yields_no_verdict() {
    let fx = fixture_with("accept.ax", "t_ok", false, FLOODING_CANDIDATE);
    let v = run(&fx.cfg);
    assert_eq!(
        v.status,
        GuestStatus::Refused,
        "ATTACK: a run whose output exceeded its limit was verdicted {:?}",
        v.status
    );
    assert!(
        v.refusal
            .as_deref()
            .unwrap_or("")
            .contains("output exceeded"),
        "{v:?}"
    );
    let fx = fixture("t_ok", false);
    assert_eq!(run(&fx.cfg).status, GuestStatus::Passed, "control");
}
