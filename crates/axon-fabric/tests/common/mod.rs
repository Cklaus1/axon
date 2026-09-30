#![allow(dead_code)]
//! Shared fixtures for the submit-path tests: a real interpreter, a real
//! axon-loop store, a real journal file.

use std::path::{Path, PathBuf};

use axon_loop::store::{Config, ConfigSchema};
use axon_loop_contracts::{AuthorityEpoch, OpaqueRef, PolicyTransition, Scope};
use serde_json::{json, Value};

pub mod exec;
#[allow(unused_imports)]
pub use exec::{copy_executable, write_executable};

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
    write_executable(
        &p,
        format!(
            "#!/bin/sh\necho \"$1\" >> '{}'\nexec '{}' \"$@\"\n",
            spawns.display(),
            axon_bin().display()
        ),
        0o755,
    );
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
            protected_scopes: Vec::new(),
            trusted_monitors: Vec::new(),
            monitor_keys: Default::default(),
            observer_keys: Default::default(),
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
            protected_host: None,
            observer: None,
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
    /// `ed25519:<first 16 hex of sha256(public key)>`, as Fabric names it.
    pub fn key_id(&self) -> String {
        use sha2::{Digest, Sha256};
        let h = Sha256::digest(self.0.public_key().as_ref());
        format!("ed25519:{}", &hex(&h)[..16])
    }
    /// A QUALIFICATION-domain `axon-evidence-signature/2` over `bytes`.
    pub fn sign(&self, bytes: &[u8]) -> String {
        self.sign_for(axon_fabric::backend::TrustAuthority::Qualification, bytes)
    }
    /// An `axon-evidence-signature/2` for `authority` over `bytes`.
    pub fn sign_for(
        &self,
        authority: axon_fabric::backend::TrustAuthority,
        bytes: &[u8],
    ) -> String {
        let msg = axon_fabric::backend::evidence_signing_message(authority, bytes);
        json!({"schema":"axon-evidence-signature/2","alg":"ed25519",
               "domain": authority.dir_name(),
               "public_key": self.public_hex(),
               "signature": hex(self.0.sign(&msg).as_ref())})
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
    // The record names the key it is issued under (RULE:issuer-claimed),
    // unless the caller set it (to test that rule).
    let mut evidence = evidence.clone();
    if evidence.get("issuer_key_id").is_none() {
        evidence["issuer_key_id"] = json!(issuer.key_id());
    }
    issuer.write_signed(&dir.join("evidence.json"), &evidence);
    let manifest = dir.join("manifest.json");
    let mut trust = QualificationTrust::for_manifest(&manifest);
    trust.clock = Clock::FixedUnix(axon_fabric::backend::parse_utc(TEST_NOW).unwrap());
    // A launcher that exists and is pinned (RULE:launcher-pinned) but is never
    // meant to run here; tests that launch swap in a stand-in with set_launcher.
    let launcher = dir.join("no-launcher.sh");
    std::fs::write(&launcher, "#!/bin/sh\nexit 99\n").unwrap();
    LinuxProfileConfig {
        launcher_sha256: sha256_file(&launcher),
        launcher,
        manifest,
        artifacts_dir: None,
        evidence: dir.join("evidence.json"),
        evidence_signature: None,
        waivers: None,
        trust,
        out_root: dir.join("lx-out"),
        exec_owner: None,
        interpreter: None,
        privileged: None,
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
    write_executable(&p, body, 0o755);
    p
}

/// The system bash, pinned at its current bytes (D: a script authority runs
/// under a pinned interpreter).
pub fn bash_pin() -> axon_fabric::sealed_exec::Pinned {
    let path = PathBuf::from("/bin/bash");
    axon_fabric::sealed_exec::Pinned {
        sha256: sha256_file(&path),
        path,
    }
}

/// Point `lx` at `launcher` AND pin it: the two always move together, as the
/// operator's host config pins them (O1).
pub fn set_launcher(lx: &mut LinuxProfileConfig, launcher: PathBuf) {
    lx.launcher_sha256 = sha256_file(&launcher);
    lx.launcher = launcher;
}

// ── The protected profile runs ONLY an observed operator-suite check ────────
//
// PSV-6 (C9 dev review round 1; A54): `interpreter_run` is no longer offered on
// `linux-microvm-protected`, and every launch there goes through the launch
// manifest. A test that exercises the profile's OTHER properties (policy
// delivery, qualification, grant authority) therefore submits a registered
// operator-suite check: these helpers register suite `acc` and store the
// candidate, as the PSV dispatch tests do.

/// The operator suite `acc`. Its test names differ from the candidate
/// fixture's own `t_ok`/`t_bad` (a sealed candidate defining a name the suite
/// defines is refused: PCI, E0004).
pub const PSV_SUITE: &str = "mod f\nuse f.{double}\n\n@[test]\nfn t_psv_ok() { assert_eq(double(21), 42) }\n\n@[test]\nfn t_psv_fail() { assert_eq(double(1), 3) }\n";

/// A profile manifest that pins everything a launch manifest names.
pub fn full_lx_manifest(guest_axon_sha: &str) -> String {
    let mut m: Value = serde_json::from_str(&lx_manifest(guest_axon_sha)).unwrap();
    for (n, c) in [
        ("vmlinux", '1'),
        ("rootfs.sqfs", '2'),
        ("axon-guest-init", '3'),
    ] {
        m["artifacts"][n] = json!({"sha256": c.to_string().repeat(64)});
    }
    m.to_string()
}

/// Register suite `acc` in `env`'s check registry and store `env.ws` as the
/// candidate; returns the candidate's `workspace_version_ref`.
pub fn psv_suite(env: &Env) -> axon_loop_contracts::Acf1Ref {
    use axon_fabric::workspace::{Quota, WorkspaceStore, WorkspaceTree};
    let suite_root = env.dir.path().join("suites/acc");
    std::fs::create_dir_all(&suite_root).unwrap();
    std::fs::write(suite_root.join("accept.ax"), PSV_SUITE).unwrap();
    let suite_ref = WorkspaceTree::import_dir(&suite_root, &Quota::default())
        .unwrap()
        .reference()
        .to_string();
    let mut reg: Value =
        serde_json::from_str(&std::fs::read_to_string(&env.registry).unwrap()).unwrap();
    reg["checks"] = json!([{
        "id": "acc", "visibility": "hidden", "root": suite_root,
        "entry": "accept.ax", "workspace_version_ref": suite_ref,
    }]);
    std::fs::write(&env.registry, reg.to_string()).unwrap();
    WorkspaceStore::open(&env.cfg(0).state_dir, &scope().tenant_id)
        .unwrap()
        .import_dir(&env.ws, &Quota::default())
        .unwrap()
}

/// Make `r` a protected-profile request for the operator suite check
/// `check:acc` / `t_psv_ok` over `candidate`, run by the guest interpreter
/// pinned at `guest_axon_sha`.
pub fn as_protected_check(
    r: &mut Value,
    candidate: &axon_loop_contracts::Acf1Ref,
    guest_axon_sha: &str,
) {
    r["required"]["hardware_isolation"] = json!(true);
    r["required"]["os"] = json!("linux");
    r["job_kind"] = json!("registered_check");
    r["argv"] = json!(["check:acc", "t_psv_ok"]);
    r["workspace_version_ref"] = json!(candidate.as_str());
    r["registered_executable_ref"] = json!(axon_fabric::backend::LINUX_GUEST_AXON_ID);
    r["executable_digest"] = json!(axon_cortex::runner::fabric_executable_digest(
        axon_fabric::backend::LINUX_GUEST_AXON_ID,
        guest_axon_sha
    ));
}

// ── The stand-in preflight observer (M3) ─────────────────────────────────────
//
// It composes the observation FROM the launch manifest and signs it with
// `axon-fabric sign-evidence` (the operator tool). It proves the PROTOCOL —
// domain, root, joins, freshness, nonce — never a measurement.

pub struct ObserverKey {
    pub pk8: PathBuf,
    pub key_id: String,
    /// The public key, 64 hex.
    pub public_hex: String,
}

pub fn observer_key(dir: &Path, name: &str, trust_in: &[&Path]) -> ObserverKey {
    use ring::signature::KeyPair;
    let rng = ring::rand::SystemRandom::new();
    let doc = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let kp = ring::signature::Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
    let pk8 = dir.join(format!("{name}.pk8"));
    std::fs::write(&pk8, doc.as_ref()).unwrap();
    let hexpk = hex(kp.public_key().as_ref());
    for d in trust_in {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join(format!("{name}.pub")), format!("{hexpk}\n")).unwrap();
    }
    use sha2::{Digest, Sha256};
    let key_id = format!(
        "ed25519:{}",
        &hex(&Sha256::digest(kp.public_key().as_ref()))[..16]
    );
    ObserverKey {
        pk8,
        key_id,
        public_hex: hexpk,
    }
}

/// An observer program in `d`: `mode` applies ONE defect; `key` signs, as
/// `authority`. A genuine observation is also kept as `d/prev-observation.json`
/// (so a later test can REPLAY it).
pub fn observer_script(d: &Path, mode: &str, key: &ObserverKey, authority: &str) -> PathBuf {
    let script = d.join(format!("observer-{mode}-{authority}.sh"));
    write_executable(
        &script,
        format!(
            r#"#!/bin/sh
while [ $# -gt 0 ]; do case "$1" in --manifest) M="$2"; shift 2;; --out) O="$2"; shift 2;; *) shift;; esac; done
[ "{mode}" = exit ] && exit 1
if [ "{mode}" = wait ]; then
# Hold the observation open until the test says go (the epoch moves meanwhile).
touch "{d}/observer-waiting"; i=0
while [ ! -f "{d}/observer-go" ] && [ $i -lt 400 ]; do sleep 0.05; i=$((i+1)); done
fi
if [ "{mode}" = replay ]; then
cp "{prev}" "$O/observation.json" && cp "{prev}.sig" "$O/observation.json.sig"; exit $?
fi
python3 - "$M" "$O/observation.json" "{mode}" "{kid}" <<'PY'
import json, sys, hashlib, datetime
m_path, out, mode, kid = sys.argv[1:]
raw = open(m_path, "rb").read(); m = json.loads(raw)
now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
o = {{"schema": "axon-preflight-observation/1", "observer_key_id": kid,
 "nonce": m["observation_nonce"], "epoch": m["authority"]["epoch"], "observed_at": now,
 "host_profile": m["backend_profile"], "fabric_revision": m["fabric_revision"],
 "firecracker_sha256": m["firecracker_sha256"], "launcher_sha256": m["launcher_sha256"],
 "host_config_sha256": m["host_config_sha256"], "guest": m["guest"],
 "verifier_sha256": m["verifier_sha256"],
 "suite_registry_sha256": m["suite"]["registry_sha256"], "policy_sha256": m["policy_sha256"],
 "intended_launch_manifest_sha256": hashlib.sha256(raw).hexdigest()}}
if mode == "stale": o["observed_at"] = "2020-01-01T00:00:00Z"
if mode == "other-manifest": o["intended_launch_manifest_sha256"] = "0" * 64
if mode == "kernel": o["guest"] = dict(o["guest"], kernel_sha256="9" * 64)
if mode == "epoch": o["epoch"] = 7
if mode == "nonce-forged": o["nonce"] = "ab" * 16
if mode == "nonce-issued-elsewhere":
    # A nonce the custodian DID issue, for this epoch and just as fresh, but
    # not the one this manifest names (another launch's).
    nd = "{d}/custodian-nonces"
    other = "cd" * 16
    open(f"{{nd}}/{{other}}.issued", "w").write(open(f"{{nd}}/{{o['nonce']}}.issued").read())
    o["nonce"] = other
if mode == "claims-other-key": o["observer_key_id"] = "ed25519:0000000000000000"
if mode == "verifier": o["verifier_sha256"] = "7" * 64
json.dump(o, open(out, "w"))
PY
{fabric} sign-evidence --record "$O/observation.json" --key {key} --authority {authority} >/dev/null || exit 1
# Keep this genuine signed observation, so a later test can REPLAY it.
cp "$O/observation.json" "{prev}"; cp "$O/observation.json.sig" "{prev}.sig"
"#,
            kid = key.key_id,
            prev = d.join("prev-observation.json").display(),
            d = d.display(),
            fabric = env!("CARGO_BIN_EXE_axon-fabric"),
            key = key.pk8.display(),
        ),
        0o755,
    );
    script
}

// ── A: the privileged launcher helper, test-trust fixture ───────────────────
//
// The REAL `axon-protected-launcher` (a test-trust build: `--test-config`),
// run unprivileged as this uid. Its config names fake guest artifacts and a
// fake engine whose REAL digests the profile manifest pins, since the helper
// re-verifies every one of them itself.

/// The engine files and guest artifacts a helper fixture pins, under `dir`.
pub struct HelperInputs {
    pub dist: PathBuf,
    pub firecracker: PathBuf,
    pub jailer: PathBuf,
    pub fc_sha: String,
    pub jailer_sha: String,
    pub kernel_sha: String,
    pub rootfs_sha: String,
}

/// Write fake `dist/{vmlinux,rootfs.sqfs}` and `engine/{firecracker,jailer}`
/// under `dir` (0644, not group/other-writable).
pub fn helper_inputs(dir: &Path) -> HelperInputs {
    let dist = dir.join("dist");
    let engine = dir.join("engine");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::create_dir_all(&engine).unwrap();
    let put = |p: &Path, b: &str| {
        std::fs::write(p, b).unwrap();
        std::fs::set_permissions(p, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
        sha256_file(p)
    };
    let kernel_sha = put(&dist.join("vmlinux"), "fake kernel\n");
    let rootfs_sha = put(&dist.join("rootfs.sqfs"), "fake rootfs\n");
    let fc_sha = put(&engine.join("firecracker"), "fake firecracker\n");
    let jailer_sha = put(&engine.join("jailer"), "fake jailer\n");
    HelperInputs {
        dist,
        firecracker: engine.join("firecracker"),
        jailer: engine.join("jailer"),
        fc_sha,
        jailer_sha,
        kernel_sha,
        rootfs_sha,
    }
}

impl HelperInputs {
    /// `manifest` (a full profile manifest) re-pinned to these files.
    pub fn pin_manifest(&self, manifest: &str) -> String {
        let mut m: Value = serde_json::from_str(manifest).unwrap();
        m["engine"] = json!({"firecracker_sha256": self.fc_sha, "jailer_sha256": self.jailer_sha});
        m["artifacts"]["vmlinux"] = json!({"sha256": self.kernel_sha});
        m["artifacts"]["rootfs.sqfs"] = json!({"sha256": self.rootfs_sha});
        m.to_string()
    }
    /// A B263 record's engine, made to agree with [`Self::pin_manifest`].
    pub fn pin_evidence(&self, ev: &mut Value) {
        ev["engine"]["firecracker_sha256"] = json!(self.fc_sha);
        ev["engine"]["jailer_sha256"] = json!(self.jailer_sha);
    }
}

/// The helper binary this build produced, pinned.
pub fn helper_pin() -> axon_fabric::sealed_exec::Pinned {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_axon-protected-launcher"));
    axon_fabric::sealed_exec::Pinned {
        sha256: sha256_file(&path),
        path,
    }
}

/// Write the helper's `axon-protected-launcher/1` config at `dir/name` for `launcher` / `manifest` / `out_root`
/// (made the service's private 0700 dir), with a private staging root; the
/// Fabric uid is this uid. Returns the config path.
pub fn write_helper_config(
    dir: &Path,
    inputs: &HelperInputs,
    launcher: &Path,
    manifest: &Path,
    out_root: &Path,
    name: &str,
) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(out_root).unwrap();
    std::fs::set_permissions(out_root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let staging = dir.join("helper-staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700)).unwrap();
    let pin = |p: &Path| json!({"path": p, "sha256": sha256_file(p)});
    let bash = bash_pin();
    let cfg = json!({
        "schema": "axon-protected-launcher/1",
        "fabric_uid": unsafe { libc::getuid() },
        "interpreter": {"path": bash.path, "sha256": bash.sha256},
        "launcher": pin(launcher),
        "profile_manifest": pin(manifest),
        "artifacts_dir": inputs.dist,
        "firecracker": inputs.firecracker,
        "jailer": inputs.jailer,
        "out_root": out_root,
        "staging_root": staging,
        "max_timeout_s": 3600,
        "max_input_bytes": 1u64 << 30,
    });
    let p = dir.join(name);
    std::fs::write(&p, cfg.to_string()).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    p
}

/// Route `lx` through the test-trust helper with `config`.
pub fn use_helper(lx: &mut LinuxProfileConfig, config: &Path) {
    lx.privileged = Some(axon_fabric::backend::PrivilegedRoute {
        helper: helper_pin(),
        owner: unsafe { libc::geteuid() },
        test_config: Some(config.to_path_buf()),
    });
}
