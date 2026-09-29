//! PSV-1: a sealed candidate cannot steer the operator's random inputs.
//!
//! Regression for the C8 certifying review (wf_ae3a5a74-41e). The interpreter
//! used ONE process-global RNG stream. A WRONG candidate, running in its sealed
//! frame, learned the stream's state through `goal_run_random` +
//! `goal_best_inputs` on its own `@[adaptive]` probe, predicted it, and advanced
//! it until the operator's next draw landed where it is right. The operator's
//! randomized check then tested only easy inputs and returned a KEYED PASS
//! through the real runner. `goal_run_random` declares `{AI,Net,IO}`, not
//! `Random`, so the fix before it (keyed on the effect row) never saw it.
//!
//! The fix is structural: every draw comes from the running frame's KERNEL,
//! and the sealed kernel has its own stream. These tests drive the REAL runner
//! (`axon_psv::runner::run`) with the effect ceiling Fabric derives for the
//! developer grant, and the host re-derives the outcome under its own key.

use axon_psv::runner::{run, RunnerConfig};
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
        "these tests need the real interpreter at {} — build it with \
         `cargo build -p axon-core --no-default-features --bin axon` (or set AXON_BIN). \
         This is a FAILURE, not a skip.",
        p.display()
    );
    p
}

/// The operator's randomized acceptance check: 20 random probes over
/// [0, 1_000_000) looking for an input where `square` is wrong. It uses only
/// effects a Fabric grant ceiling can hold, which is why it searches with
/// `goal_run_random` rather than `random_i64`.
const SUITE: &str = r#"mod sol
use sol.{square}

@[adaptive]
fn wrong_at(x: i64) -> i64 { if square(x) == x * x { 0 } else { 1 } }

@[test]
fn accept() {
    let found = goal_run_random("wrong_at", 1.0, 20, 0, 1000000)
    println("best probe score {to_str(found)}")
    assert(found < 0.5)
}
"#;

/// Wrong for every x >= 1000. Before the fix it passed 5/5 by steering.
const STEERING_WRONG_CANDIDATE: &str = r#"@[adaptive]
fn probe2(a: i64, b: i64) -> i64 { 0 }
@[adaptive]
fn probe1(a: i64) -> i64 { 0 }

fn lshr(x: i64, k: i64) -> i64 { bit_and(shr(x, k), bit_not(shl(-1, 64 - k))) }
fn xs(x0: i64) -> i64 {
    let x = bit_xor(x0, shl(x0, 13))
    x = bit_xor(x, lshr(x, 7))
    bit_xor(x, shl(x, 17))
}
fn addmod(a: i64, b: i64, m: i64) -> i64 { if a >= m - b { a - (m - b) } else { a + b } }
fn umod(u: i64, m: i64) -> i64 {
    if u >= 0 { u % m } else {
        let r = ((u % m) + m) % m
        let p63 = ((9223372036854775807 % m) + 1) % m
        addmod(r, addmod(p63, p63, m), m)
    }
}
fn learn_state() -> i64 {
    let m = 9223372036854775807
    let _c = goal_clear("probe2")
    let _ = goal_run_random("probe2", 0.0, 1, 0, m)
    let ab = goal_best_inputs("probe2", 0.0)
    let a = ab[0]
    let b = ab[1]
    let s1 = if umod(xs(a), m) == b { a } else { wrapping_add(a, m) }
    xs(s1)
}
fn steer() -> i64 {
    let s = learn_state()
    let steps = 0
    while umod(xs(s), 1000000) >= 1000 {
        let _ = goal_run_random("probe1", 0.0, 1, 0, 2)
        s = xs(s)
        steps = steps + 1
    }
    steps
}
pub fn square(x: i64) -> i64 {
    let _ = steer()
    if x < 1000 { x * x } else { 0 }
}

let steered_at_load = steer()
"#;

const CORRECT_CANDIDATE: &str = "pub fn square(x: i64) -> i64 { x * x }\n";

/// Run the operator's check against `candidate` through the real runner.
/// Returns the verdict and the outcome the HOST derives under its own key.
fn check(candidate: &str) -> (GuestVerdict, Option<bool>) {
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
    }
    std::fs::write(cand.join("sol.ax"), candidate).unwrap();
    std::fs::write(suite.join("main.ax"), SUITE).unwrap();
    let secret: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
    let sp = job.join("completion-secret");
    std::fs::write(&sp, secret).unwrap();
    let q = Quota::default();
    let sv = axon_workspace_recipe::tree_version_ref(&suite, &q).unwrap();
    let cv = axon_workspace_recipe::tree_version_ref(&cand, &q).unwrap();
    let m = LaunchManifest {
        schema: LAUNCH_MANIFEST_SCHEMA.into(),
        operation_id: "op".into(),
        task_id: "t".into(),
        trial_id: "tr".into(),
        attempt_id: "a".into(),
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
            id: "s".into(),
            version: sv.clone(),
            entry: "main.ax".into(),
            test: "accept".into(),
            tree_digest: sv,
            registry_sha256: "c".repeat(64),
        },
        candidate: CandidateRef { workspace_version: cv.clone(), tree_digest: cv },
        completion: Completion { scheme: COMPLETION_SCHEME.into() },
        observation_nonce: "e".repeat(32),
        limits: Limits { wall_time_ms: 120_000, output_bytes: 1 << 20 },
    };
    std::fs::write(job.join("launch-manifest.json"), m.bytes()).unwrap();
    let axon = axon();
    let cfg = RunnerConfig {
        manifest: job.join("launch-manifest.json"),
        secret: sp,
        candidate: cand,
        suite,
        out,
        axon: axon.clone(),
        runner_exe: axon,
        expected_manifest_sha256: m.digest(),
        drop: None,
        // The developer-profile grant's ceiling as Fabric derives it (Exec is
        // then stripped by the runner). It never contains `Random`.
        effect_ceiling: Some("Net,AI,IO,Exec".into()),
    };
    let v = run(&cfg);
    let raw = std::fs::read_to_string(cfg.out.join("test-stdout")).unwrap_or_default();
    let host = keyed_outcome(&raw, "accept", &completion_key(&secret, &m));
    (v, host)
}

/// The attack the review executed. A candidate that steers must not pass:
/// its steering now moves only its own kernel's stream.
#[test]
fn a_candidate_steering_the_rng_through_goal_search_does_not_pass_a_randomized_check() {
    let (v, host) = check(STEERING_WRONG_CANDIDATE);
    assert_eq!(
        v.status,
        GuestStatus::Failed,
        "a wrong candidate that steers the RNG must fail the randomized check: {v:?}"
    );
    assert_eq!(host, Some(false), "the host must derive a keyed FAILURE, not a pass");
}

/// Control: the same randomized check still PASSES a correct candidate, so the
/// failure above is the check discriminating, not the check being broken.
#[test]
fn a_correct_candidate_passes_the_same_randomized_check() {
    let (v, host) = check(CORRECT_CANDIDATE);
    assert_eq!(v.status, GuestStatus::Passed, "control: {v:?}");
    assert_eq!(host, Some(true), "control: the host verifies the keyed pass");
}
