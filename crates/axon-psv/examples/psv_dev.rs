//! DEVELOPMENT tool for the PSV boot test (`scripts/psv_guest_boot_test.sh`).
//! It stands in for the Fabric side that M2 builds: it makes a job and checks
//! a verdict with the SAME protocol crate. It is not an authority and earns
//! nothing.
//!
//!   psv_dev make-job --candidate DIR --suite DIR --entry FILE --test NAME --job DIR [--attempt ID]
//!       writes DIR/launch-manifest.json + DIR/completion-secret (32 fresh bytes)
//!       and prints {"manifest_sha256": …}
//!   psv_dev check-verdict --job DIR --verdict FILE
//!       re-derives K from the job's secret and manifest (never from the
//!       verdict) and prints {"status", "manifest_joins", "token_verifies", "refusal"}

use axon_psv::*;
use std::path::PathBuf;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn req(args: &[String], name: &str) -> String {
    arg(args, name).unwrap_or_else(|| {
        eprintln!("psv_dev: {name} is required");
        std::process::exit(2)
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("make-job") => make_job(&args),
        Some("check-verdict") => check_verdict(&args),
        _ => {
            eprintln!("usage: psv_dev make-job … | check-verdict …");
            std::process::exit(2)
        }
    }
}

fn make_job(args: &[String]) {
    let (cand, suite) = (
        PathBuf::from(req(args, "--candidate")),
        PathBuf::from(req(args, "--suite")),
    );
    let job = PathBuf::from(req(args, "--job"));
    let q = Quota::default();
    let digest = |p: &PathBuf| {
        axon_workspace_recipe::tree_version_ref(p, &q).unwrap_or_else(|e| {
            eprintln!("psv_dev: {}: {e}", p.display());
            std::process::exit(2)
        })
    };
    let z = |c: char| std::iter::repeat_n(c, 64).collect::<String>();
    let m = LaunchManifest {
        schema: LAUNCH_MANIFEST_SCHEMA.into(),
        operation_id: "op-dev".into(),
        task_id: "task-dev".into(),
        trial_id: "trial-dev".into(),
        attempt_id: arg(args, "--attempt").unwrap_or_else(|| "attempt-1".into()),
        backend_profile: PROTECTED_PROFILE.into(),
        fabric_revision: "0".repeat(40),
        verifier_sha256: "d".repeat(64),
        qualification_sha256: z('0'),
        host_config_sha256: z('0'),
        launcher_sha256: z('0'),
        firecracker_sha256: z('0'),
        profile_manifest_sha256: z('0'),
        guest: GuestDigests {
            kernel_sha256: z('0'),
            rootfs_sha256: z('0'),
            axon_sha256: z('0'),
            init_sha256: z('0'),
        },
        policy_sha256: z('0'),
        suite: SuiteRef {
            id: "dev-suite".into(),
            version: digest(&suite),
            entry: req(args, "--entry"),
            test: req(args, "--test"),
            tree_digest: digest(&suite),
            registry_sha256: z('0'),
        },
        candidate: CandidateRef {
            workspace_version: digest(&cand),
            tree_digest: digest(&cand),
        },
        completion: Completion {
            scheme: COMPLETION_SCHEME.into(),
        },
        observation_nonce: "dev".into(),
        limits: Limits {
            wall_time_ms: 60_000,
            output_bytes: 1 << 20,
        },
    };
    let mut secret = [0u8; 32];
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut secret))
        .expect("urandom");
    std::fs::create_dir_all(&job).unwrap();
    std::fs::write(job.join("launch-manifest.json"), m.bytes()).unwrap();
    std::fs::write(job.join("completion-secret"), secret).unwrap();
    println!("{}", serde_json::json!({"manifest_sha256": m.digest()}));
}

fn check_verdict(args: &[String]) {
    let job = PathBuf::from(req(args, "--job"));
    let mb = std::fs::read(job.join("launch-manifest.json")).unwrap();
    let m = LaunchManifest::verify(&mb, &sha256_hex(&mb)).unwrap();
    let secret: [u8; 32] = std::fs::read(job.join("completion-secret"))
        .unwrap()
        .try_into()
        .expect("32 bytes");
    let v: GuestVerdict =
        serde_json::from_slice(&std::fs::read(req(args, "--verdict")).unwrap()).unwrap();
    let k = completion_key(&secret, &m);
    let mut msg = b"axon-test-completion/1\0".to_vec();
    msg.extend_from_slice(m.suite.test.as_bytes());
    let want: String = hmac_sha256(&k, &msg)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let token_verifies = v.report.as_ref().is_some_and(|r| {
        r.completion
            .iter()
            .any(|(n, t)| *n == m.suite.test && *t == want)
    });
    println!(
        "{}",
        serde_json::json!({
            "status": v.status,
            "manifest_joins": v.launch_manifest_sha256 == m.digest(),
            "token_verifies": token_verifies,
            "refusal": v.refusal,
            "runner": v.runner,
        })
    );
}
