#![allow(dead_code)]
//! Shared fixtures for the submit-path tests: a real interpreter, a real
//! axon-loop store, a real journal file.

use std::path::{Path, PathBuf};

use axon_loop::store::{Config, ConfigSchema};
use axon_loop_contracts::{AuthorityEpoch, OpaqueRef, PolicyTransition, Scope};
use serde_json::{json, Value};

pub const ADMITTER: &str = "admitter:fabric-test";

/// Locate a workspace binary these tests drive but cargo does not build for
/// this crate (so there is no `CARGO_BIN_EXE_*` for it).
///
/// Order: the explicit env var (`AXON_BIN` / `CORTEX_BIN`), then the SAME
/// target directory this test's own `axon-fabric` binary was built into — which
/// is how a custom `CARGO_TARGET_DIR` is honoured — then the workspace
/// `target/debug/`. It FAILS, never skips, when none exists: these are
/// production-caller tests, and one that silently passes proves nothing. It
/// used to hard-code `../../target/debug/`, so under a custom CARGO_TARGET_DIR
/// every such test failed spuriously (or ran a stale binary left in the tree).
/// An env var naming a missing file is an error, not a cue to fall back.
pub fn workspace_bin(name: &str, env_var: &str, build_hint: &str) -> PathBuf {
    if let Some(p) = std::env::var_os(env_var) {
        let p = PathBuf::from(p);
        assert!(p.exists(), "{env_var}={} does not exist", p.display());
        return p.canonicalize().unwrap();
    }
    let own_profile_dir = Path::new(env!("CARGO_BIN_EXE_axon-fabric"))
        .parent()
        .map(Path::to_path_buf);
    let mut looked = Vec::new();
    for dir in own_profile_dir.into_iter().chain(std::iter::once(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug"),
    )) {
        let p = dir.join(name);
        if p.exists() {
            return p.canonicalize().unwrap();
        }
        looked.push(p.display().to_string());
    }
    panic!(
        "needs the `{name}` binary (looked in: {}; or set {env_var}) — {build_hint}",
        looked.join(", ")
    )
}

pub fn axon_bin() -> PathBuf {
    workspace_bin(
        "axon",
        "AXON_BIN",
        "cargo build -p axon-core --no-default-features --bin axon",
    )
}

pub fn sha256_file(p: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(std::fs::read(p).unwrap()))
}

pub fn scope() -> Scope {
    axon_fabric::submit::scope("tenant-t", "family-f").unwrap()
}

pub const FIXTURE: &str = "\
fn double(n: i64) -> i64 { n * 2 }

@[test]
fn t_ok() { assert_eq(double(2), 4) }

@[test]
fn t_bad() { assert_eq(double(2), 5) }
";

/// A wrapper around the real interpreter that appends one line to `spawns`
/// per invocation — the observable "a process was spawned" fact.
pub fn spawn_counting_wrapper(dir: &Path, spawns: &Path) -> PathBuf {
    let p = dir.join("axon-counting.sh");
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\necho \"$1\" >> '{}'\nexec '{}' \"$@\"\n",
            spawns.display(),
            axon_bin().display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

pub fn spawn_count(spawns: &Path) -> usize {
    std::fs::read_to_string(spawns)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// The default test grant: nothing but the filesystem, unscoped — so the
/// check runs under `AXON_ALLOWED_EFFECTS=IO`.
pub const GRANT_FS: &str = "\
profile = \"restricted\"
[grant]
fs_read = [\"*\"]
fs_write = [\"*\"]
max_label = \"internal\"
[grant.budget]
cost_micro = 1000
";

/// A grant that withholds nothing (developer profile, every axis `*`): the
/// only kind the Linux profile can honour (no guest policy channel, x1).
pub const GRANT_OPEN: &str = "\
profile = \"developer\"
[grant]
max_label = \"internal\"
[grant.budget]
cost_micro = 1000
";

pub const PRINCIPAL: &str = "principal:test";

/// Write `<dir>/<name>.axgrant` and return (file name, sha256).
pub fn write_grant(dir: &Path, name: &str, body: &str) -> (String, String) {
    let file = format!("{name}.axgrant");
    std::fs::write(dir.join(&file), body).unwrap();
    let sha = sha256_file(&dir.join(&file));
    (file, sha)
}

/// Write a grant registry binding each (grant_ref, principal, body).
pub fn write_grant_registry(path: &Path, grants: &[(&str, &str, &str)]) {
    let dir = path.parent().unwrap();
    let entries: Vec<Value> = grants
        .iter()
        .map(|(gref, principal, body)| {
            let (file, sha) = write_grant(dir, &gref.replace(':', "_"), body);
            json!({"grant_ref": gref, "principal_ref": principal, "path": file, "sha256": sha})
        })
        .collect();
    std::fs::write(
        path,
        json!({"schema": "axon-fabric-grant-registry/1", "grants": entries}).to_string(),
    )
    .unwrap();
}

pub struct Env {
    pub dir: tempfile::TempDir,
    pub grant_registry: PathBuf,
    pub ws: PathBuf,
    pub journal: PathBuf,
    pub store: PathBuf,
    pub registry: PathBuf,
    pub spawns: PathBuf,
    pub exe: PathBuf,
}

impl Env {
    pub fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("f.ax"), FIXTURE).unwrap();
        let spawns = dir.path().join("spawns.log");
        let exe = spawn_counting_wrapper(dir.path(), &spawns);
        let store = dir.path().join("loop-store");
        let st = axon_loop::Store::open_dir(&store).unwrap();
        st.write_config(&Config {
            schema: ConfigSchema,
            trusted_admitters: vec![OpaqueRef::new(ADMITTER).unwrap()],
            trusted_verifiers: vec![],
            trusted_observers: vec![],
            verifier_keys: Default::default(),
            verifier_pins: Default::default(),
            task_acceptance: Default::default(),
        })
        .unwrap();
        let registry = dir.path().join("registry.json");
        write_registry(&registry, &exe, None);
        let gdir = dir.path().join("grants");
        std::fs::create_dir_all(&gdir).unwrap();
        let grant_registry = gdir.join("grants.json");
        write_grant_registry(
            &grant_registry,
            &[
                ("grant:test", PRINCIPAL, GRANT_FS),
                ("grant:open", PRINCIPAL, GRANT_OPEN),
            ],
        );
        Env {
            grant_registry,
            journal: dir.path().join("ops.journal"),
            ws,
            store,
            registry,
            spawns,
            exe,
            dir,
        }
    }

    pub fn registry(&self) -> axon_cortex::runner::CheckRegistry {
        axon_cortex::runner::CheckRegistry::load(&self.registry).unwrap()
    }

    pub fn cfg(&self, expected_epoch: u64) -> axon_fabric::SubmitConfig {
        axon_fabric::SubmitConfig {
            journal: self.journal.clone(),
            registry: self.registry(),
            epoch: axon_fabric::EpochSource::LoopStore {
                store: self.store.clone(),
                scope: scope(),
            },
            expected_epoch: AuthorityEpoch::new(expected_epoch).unwrap(),
            workspace: self.ws.clone(),
            state_dir: self.dir.path().join("fabric-state"),
            budget: axon_fabric::ResourceVector {
                model_micro_usd: 1_000,
                exec_ms: 1_000_000,
                verify_ms: 1_000_000,
                retries: 100,
            },
            grants: axon_fabric::GrantRegistry::load(&self.grant_registry).unwrap(),
            linux: None,
            pre_launch_hook: None,
            fault_hook: None,
        }
    }

    /// Advance the scope's authority epoch by one (a trusted pause).
    pub fn bump_epoch(&self) -> u64 {
        let st = axon_loop::Store::open_dir(&self.store).unwrap();
        let cur = axon_loop::epoch::current(&st, &scope()).unwrap().get();
        let p = axon_loop::pointer::load(&st, &scope()).unwrap();
        let t: PolicyTransition = axon_loop_contracts::parse(
            &json!({
                "schema": "axon.closed-loop.transition/1",
                "transition_id": format!("bump-{cur}"),
                "kind": "pause",
                "scope": scope(),
                "expected_policy_ref": p.expected_ref(),
                "target_policy_ref": null,
                "expected_epoch": cur,
                "next_epoch": cur + 1,
                "admission_ref": null,
                "reason_ref": format!("cl22:{}", "e".repeat(64)),
                "issuer_ref": ADMITTER,
                "mechanism_test": true
            })
            .to_string(),
        )
        .unwrap();
        axon_loop::pointer::transition(&st, &t).unwrap();
        cur + 1
    }

    pub fn journal_text(&self) -> String {
        std::fs::read_to_string(&self.journal).unwrap_or_default()
    }

    pub fn launch_records(&self) -> usize {
        self.journal_text()
            .lines()
            .filter(|l| l.contains("\"kind\":\"launched\""))
            .count()
    }
}

pub fn write_registry(path: &Path, axon: &Path, fabric: Option<&Path>) {
    let mut ex = vec![json!({
        "id": "axon-test-local",
        "path": axon,
        "sha256": sha256_file(axon),
    })];
    if let Some(f) = fabric {
        ex.push(json!({"id": "axon-fabric", "path": f, "sha256": sha256_file(f)}));
    }
    std::fs::write(
        path,
        json!({"schema": "cortex-check-registry/1", "executors": ex}).to_string(),
    )
    .unwrap();
}

/// A valid `registered_check` request for `f.ax`, filter `filter`.
pub fn request(env: &Env, op: &str, filter: &str) -> Value {
    let bytes = std::fs::read(env.ws.join("f.ax")).unwrap();
    json!({
        "schema": "acf-compute-request/1",
        "operation_id": op,
        "task_id": "task-1",
        "trial_id": "trial-1",
        "attempt_id": "attempt-1",
        "principal_ref": PRINCIPAL,
        "grant_ref": "grant:test",
        "approval_ref": null,
        "job_kind": "registered_check",
        "registered_executable_ref": "axon-test-local",
        "executable_digest": axon_cortex::runner::fabric_executable_digest(
            "axon-test-local", &sha256_file(&env.exe)),
        "workspace_version_ref": axon_cortex::runner::fabric_workspace_digest("f.ax", &bytes),
        "semantic_state_ref": null,
        "policy_digest": format!("acf1:{}", "c".repeat(64)),
        "required": {
            "engine": "axon_interpreter",
            "hardware_isolation": false,
            "os": "none",
            "architecture": "x86_64",
            "network_mode": "deny",
            "checkpoint_kind": "none"
        },
        "limits": {
            "cpu_millicores": 1000,
            "memory_bytes": 268435456,
            "disk_bytes": 268435456,
            "wall_time_ms": 60000,
            "output_bytes": 1048576,
            "max_cost_micro": 100,
            "currency_code": "USD",
            "price_schedule_ref": "unpriced:test"
        },
        "argv": ["f.ax", filter],
        "result_schema_ref": "cortex-check-report/1"
    })
}

// ── Linux profile qualification: a throwaway issuer, generated per test ─────
//
// No private key is ever written to the repository; each test mints its own
// Ed25519 key pair, trusts its public half, and signs the evidence with it.

use axon_fabric::backend::{Clock, LinuxProfileConfig, QualificationTrust};
use ring::signature::KeyPair as _;

/// "Now" for every qualification test: 2026-09-25T12:00:00Z.
pub const TEST_NOW: &str = "2026-09-25T12:00:00Z";
/// A fresh evidence `end`: one hour before `TEST_NOW`.
pub const TEST_END: &str = "2026-09-25T11:00:00Z";
pub const TEST_FC_SHA: &str = "96d25e000e5fcbf5b11dca0e1275b5d7dc0922ff8b8206aeaed98e515a8468dc";
pub const TEST_JAILER_SHA: &str =
    "8965e9ee855537561ac3adaf0b086a3882456fdc55472691f3bbe489199014ce";
pub const TEST_CAVEAT: &str = "Nested virtualization under Hyper-V; the L0 hypervisor is outside \
                               the qualified boundary. (operator decision D2)";

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub struct Issuer(pub ring::signature::Ed25519KeyPair);

impl Issuer {
    pub fn generate() -> Issuer {
        let rng = ring::rand::SystemRandom::new();
        let doc = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        Issuer(ring::signature::Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap())
    }
    pub fn public_hex(&self) -> String {
        hex(self.0.public_key().as_ref())
    }
    /// An `axon-evidence-signature/1` over exactly `bytes`.
    pub fn sign(&self, bytes: &[u8]) -> String {
        json!({"schema":"axon-evidence-signature/1","alg":"ed25519",
               "public_key": self.public_hex(),
               "signature": hex(self.0.sign(bytes).as_ref())})
        .to_string()
    }
    /// Write `v` to `path` and its detached signature to `path.sig`.
    pub fn write_signed(&self, path: &Path, v: &Value) {
        let bytes = serde_json::to_vec_pretty(v).unwrap();
        std::fs::write(path, &bytes).unwrap();
        std::fs::write(sig_of(path), self.sign(&bytes)).unwrap();
    }
    /// Trust this issuer: write its public key into `dir/<name>.pub`.
    pub fn trust_in(&self, dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(format!("{name}.pub")), self.public_hex() + "\n").unwrap();
    }
}

pub fn sig_of(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".sig");
    PathBuf::from(s)
}

/// A manifest built from a CLEAN tree, pinning the guest interpreter.
pub fn lx_manifest(guest_axon_sha: &str) -> String {
    json!({"schema":"axon-linux-microvm-profile/1","profile":"linux-microvm-protected",
           "source":{"axon_tree_dirty_at_build": false},
           "engine":{"firecracker_sha256": TEST_FC_SHA, "jailer_sha256": TEST_JAILER_SHA},
           "artifacts":{"axon":{"sha256": guest_axon_sha}}})
    .to_string()
}

/// A complete, fresh, clean `PASS` record for `manifest_sha`.
pub fn good_evidence(manifest_sha: &str) -> Value {
    json!({
        "schema": "axon-b263-evidence/1",
        "work_package": "B263",
        "host": "WSL2-nested",
        "caveat": TEST_CAVEAT,
        "source": {"axon_git_rev": "0".repeat(40), "tree_dirty": false},
        "engine": {"firecracker": "Firecracker v1.10.1", "firecracker_sha256": TEST_FC_SHA,
                   "jailer": "Jailer v1.10.1", "jailer_sha256": TEST_JAILER_SHA},
        "profile": {"name": "linux-microvm-protected", "manifest_sha256": manifest_sha},
        "assertions": [
            {"name": "a1_boot_runs_ax_expected_stdout", "status": "PASS"},
            {"name": "a2_guest_is_pinned_linux_kernel", "status": "PASS"},
            {"name": "x3_l0_hypervisor_boundary", "status": "PASS"}
        ],
        "counts": {"total": 3, "PASS": 3, "FAIL": 0, "BLOCKED": 0},
        "result": "PASS",
        "start": "2026-09-25T10:59:30Z",
        "end": TEST_END
    })
}

/// Sign `evidence` with `issuer` into `dir/evidence.json(.sig)`, trust the
/// issuer in `dir/trusted_issuers/`, and point a config at `dir/manifest.json`
/// (which the caller has written). Clock pinned at `TEST_NOW`, max age 30 d.
pub fn qualified_linux_cfg(dir: &Path, issuer: &Issuer, evidence: &Value) -> LinuxProfileConfig {
    issuer.trust_in(&dir.join("trusted_issuers"), "operator");
    issuer.write_signed(&dir.join("evidence.json"), evidence);
    let manifest = dir.join("manifest.json");
    let mut trust = QualificationTrust::for_manifest(&manifest);
    trust.clock = Clock::FixedUnix(axon_fabric::backend::parse_utc(TEST_NOW).unwrap());
    LinuxProfileConfig {
        launcher: dir.join("no-launcher.sh"),
        manifest,
        artifacts_dir: None,
        evidence: dir.join("evidence.json"),
        evidence_signature: None,
        waivers: None,
        trust,
        out_root: dir.join("lx-out"),
    }
}

/// A stand-in for `scripts/fc_linux_profile.sh` that writes the documented
/// `axon-linux-microvm-result/1` shape and appends to `<out_root>/launches`
/// each time it is actually run (so "the launcher never ran" is checkable).
/// It also records what the Fabric handed it: its argv, one line per run, in
/// `<out_root>/argv`, and a copy of the `--policy FILE` contents in
/// `<out_root>/delivered-policy.json` (absent when no `--policy` was given).
pub fn stand_in_launcher(
    env: &Env,
    exit: i32,
    bound: bool,
    cleanup_ok: bool,
    verify_exit: i32,
) -> std::path::PathBuf {
    let p = env.dir.path().join(format!(
        "fake-launcher-{exit}-{bound}-{cleanup_ok}-{verify_exit}.sh"
    ));
    let body = format!(
        r#"#!/bin/sh
if [ "$1" = "--verify-result" ]; then exit {verify_exit}; fi
ARGV="$*"
OUT=""
POLICY=""
while [ $# -gt 0 ]; do case "$1" in --out) OUT="$2"; shift 2;; --policy) POLICY="$2"; shift 2;; *) shift;; esac; done
mkdir -p "$OUT/out"
echo launched >> "$OUT/../launches"
echo "$ARGV" >> "$OUT/../argv"
if [ -n "$POLICY" ]; then cat "$POLICY" > "$OUT/../delivered-policy.json"; fi
cat > "$OUT/result.json" <<J
{{"schema":"axon-linux-microvm-result/1","status":"x","workload_exit":0,
 "output_bound":{bound},"outputs":{{"stdout":{{"sha256":"ab","bytes":1}}}},
 "cleanup":{{"complete":{cleanup_ok},"left_behind":[]}}}}
J
exit {exit}
"#
    );
    std::fs::write(&p, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}
