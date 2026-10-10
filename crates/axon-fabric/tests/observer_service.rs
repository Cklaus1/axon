//! Amendment 68 (operator decisions G and G1 = A): the observer SERVICE
//! (`axon-observer`) and the privileged helper's `--observe` relay, driven as
//! the binaries they are (test-trust builds: `--test-config`). Negative
//! matrix A94: Fabric mints an observation.
//!
//! Every test is an ATTACK with a CONTROL on the same fixture. Root-only
//! tests install a setuid-root helper copy or run the observer as another
//! uid through `setpriv`.

mod common;
use common::*;

use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const FABRIC: u32 = 4242;
const OTHER: u32 = 4243;
const NOT_OTHER: u32 = 4244;

fn euid() -> u32 {
    unsafe { libc::geteuid() }
}

fn skip_unless_root() -> bool {
    if euid() != 0 {
        eprintln!("skipped: needs root (ownership by another uid, setuid, setpriv)");
        return true;
    }
    false
}

fn set_mode(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn chown(p: &Path, uid: u32) {
    std::os::unix::fs::chown(p, Some(uid), Some(uid)).unwrap();
}

fn observer_program_sha256() -> String {
    sha256_file(Path::new(env!("CARGO_BIN_EXE_axon-observer")))
}

/// The sha256 of the executable a process running `p` executes (what the
/// helper measures of its parent through `/proc/<pid>/exe`).
fn exe_sha256(p: &Path) -> String {
    sha256_file(&std::fs::canonicalize(p).unwrap())
}

fn this_exe_sha256() -> String {
    // The running IMAGE, not the path it was started from (that can be
    // relinked under a sharded run): `/proc/<pid>/exe` is what the helper measures.
    sha256_file(&axon_fabric::readiness::running_image())
}

/// A running process, killed when dropped.
struct Running(std::process::Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The observer fixture: what the observer measures (an operator host config
/// naming a launcher, a suite registry, a qualification record, a profile
/// manifest and the guest artifacts; the helper's config naming firecracker),
/// the helper's test config with `observer.service`, and the observer's own
/// config, key (0400, trusted in the operator observer root) and store.
struct Obs {
    _d: tempfile::TempDir,
    base: PathBuf,
    helper_cfg: PathBuf,
    obs_dir: PathBuf,
    obs_cfg: PathBuf,
    socket: PathBuf,
    key: ObserverKey,
    observer: Option<Running>,
    /// Facts the honest manifest names (what an honest observer measures).
    facts: Value,
    /// Amendment 79: the custodian whose nonces the observer accepts (the
    /// observer asks it over the helper config's custodian section).
    cust: TestCustodian,
}

fn obs() -> Obs {
    obs_with(euid(), CustodianRun::Test)
}

/// [`obs`], the custodian answering `check` for `observer_uid` (the uid the
/// observer service runs as).
fn obs_as(observer_uid: u32) -> Obs {
    obs_with(observer_uid, CustodianRun::Test)
}

/// [`obs`], with a custodian of mode `run` answering `check` for `observer_uid`.
fn obs_with(observer_uid: u32, run: CustodianRun) -> Obs {
    let d = tempfile::tempdir_in("/var/tmp").unwrap();
    let base = d.path().to_path_buf();
    set_mode(&base, 0o755);
    let inputs = helper_inputs(&base);
    let manifest = base.join("manifest.json");
    std::fs::write(
        &manifest,
        inputs.pin_manifest(&full_lx_manifest(&"cd".repeat(32))),
    )
    .unwrap();
    let launcher = base.join("launcher.sh");
    write_executable(&launcher, "#!/bin/sh\nexit 0\n", 0o755);
    let registry = base.join("registry.json");
    std::fs::write(&registry, "{\"registry\":1}\n").unwrap();
    let record = base.join("evidence.json");
    std::fs::write(&record, "{\"b263\":1}\n").unwrap();
    let helper_cfg = write_helper_config(
        &base,
        &inputs,
        &launcher,
        &manifest,
        &base.join("runs"),
        "protected-launcher.json",
    );
    let host_cfg = base.join("protected-host.json");
    std::fs::write(
        &host_cfg,
        json!({
            "launcher": {"path": launcher}, "suite_registry": {"path": registry},
            "qualification": {"record": record}, "profile_manifest": {"path": manifest},
            "artifacts_dir": inputs.dist,
        })
        .to_string(),
    )
    .unwrap();
    let obs_dir = base.join("obs");
    std::fs::create_dir(&obs_dir).unwrap();
    set_mode(&obs_dir, 0o755);
    let store = obs_dir.join("records");
    std::fs::create_dir(&store).unwrap();
    set_mode(&store, 0o700);
    let key = observer_key(&obs_dir, "observer", &[&observer_root(&base)]);
    set_mode(&key.pk8, 0o400);
    let socket = obs_dir.join("observer.sock");
    let me = euid();
    let obs_cfg = obs_dir.join("observer.json");
    std::fs::write(
        &obs_cfg,
        json!({
            "schema": "axon-observer/1", "observer_uid": me, "fabric_uid": me,
            "caller_uid": me, "socket": socket, "store": store, "key_path": key.pk8,
            "test_paths": {"trust_root": observer_root(&base), "host_config": host_cfg,
                           "helper_config": helper_cfg},
        })
        .to_string(),
    )
    .unwrap();
    set_mode(&obs_cfg, 0o644);
    let cust = try_start_custodian(&base, run, |c| c["observer_uid"] = json!(observer_uid))
        .expect("setup: the custodian starts");
    let facts = json!({
        "host_config_sha256": sha256_file(&host_cfg),
        "launcher_sha256": sha256_file(&launcher),
        "firecracker_sha256": inputs.fc_sha,
        "guest.kernel_sha256": inputs.kernel_sha,
        "guest.rootfs_sha256": inputs.rootfs_sha,
        "suite.registry_sha256": sha256_file(&registry),
        "qualification_sha256": sha256_file(&record),
        "profile_manifest_sha256": sha256_file(&manifest),
        "verifier_sha256": this_exe_sha256(),
        // Amendment 79: the pinned program's build revision, and the digests
        // the measured profile manifest names for the guest's axon and init.
        "fabric_revision": TEST_FABRIC_REVISION,
        "guest.axon_sha256": "cd".repeat(32),
        "guest.init_sha256": "3".repeat(64),
    });
    let o = Obs {
        _d: d,
        base,
        helper_cfg,
        obs_dir,
        obs_cfg,
        socket,
        key,
        observer: None,
        facts,
        cust,
    };
    o.edit_helper(|v| {
        v["observer"]["service"] =
            json!({"socket": o.socket, "uid": me, "sha256": observer_program_sha256()})
    });
    o
}

/// Set `field` (dotted) of a manifest value.
fn set_field(m: &mut Value, field: &str, to: Value) {
    let mut at = m;
    let parts: Vec<&str> = field.split('.').collect();
    for p in &parts[..parts.len() - 1] {
        at = &mut at[*p];
    }
    at[*parts.last().unwrap()] = to;
}

impl Obs {
    fn edit_helper(&self, edit: impl FnOnce(&mut Value)) {
        let mut v: Value =
            serde_json::from_slice(&std::fs::read(&self.helper_cfg).unwrap()).unwrap();
        edit(&mut v);
        std::fs::write(&self.helper_cfg, v.to_string()).unwrap();
    }
    fn edit_observer(&self, edit: impl FnOnce(&mut Value)) {
        let mut v: Value = serde_json::from_slice(&std::fs::read(&self.obs_cfg).unwrap()).unwrap();
        edit(&mut v);
        std::fs::write(&self.obs_cfg, v.to_string()).unwrap();
    }
    /// Start `axon-observer [pre] --test-config CFG` (through `setpriv` as
    /// `as_uid`). `Err(stderr)` when it exits instead of listening.
    fn try_start(&mut self, pre: &[&str], as_uid: Option<u32>) -> Result<(), String> {
        let bin = env!("CARGO_BIN_EXE_axon-observer");
        let mut c = match as_uid {
            None => Command::new(bin),
            Some(u) => {
                let mut c = Command::new("setpriv");
                c.arg(format!("--reuid={u}"))
                    .arg(format!("--regid={u}"))
                    .arg("--clear-groups")
                    .arg("--")
                    .arg(bin);
                c
            }
        };
        let mut child = c
            .args(pre)
            .arg("--test-config")
            .arg(&self.obs_cfg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        for _ in 0..400 {
            if child.try_wait().unwrap().is_some() {
                let mut e = String::new();
                use std::io::Read;
                let _ = child.stderr.take().unwrap().read_to_string(&mut e);
                return Err(e);
            }
            if std::os::unix::net::UnixStream::connect(&self.socket).is_ok() {
                self.observer = Some(Running(child));
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let _ = child.kill();
        Err("the observer did not start listening".into())
    }
    fn start(&mut self) {
        self.try_start(&[], None)
            .expect("setup: the observer starts");
    }
    /// A canonical protected launch manifest naming `nonce`, whose measured
    /// facts are the installed ones; `edit` then changes it.
    fn manifest(&self, nonce: &str, edit: impl FnOnce(&mut Value)) -> Vec<u8> {
        let mut m = test_launch_manifest("op-obs", nonce);
        let h = |c: char| c.to_string().repeat(64);
        m["candidate"] = json!({"workspace_version": h('e'), "tree_digest": h('e')});
        m["suite"]["version"] = json!(h('c'));
        m["suite"]["tree_digest"] = json!(h('c'));
        for (k, v) in self.facts.as_object().unwrap() {
            set_field(&mut m, k, v.clone());
        }
        edit(&mut m);
        let lm: axon_psv::LaunchManifest = serde_json::from_value(m).unwrap();
        lm.bytes()
    }
    /// The helper's `--observe`, run by THIS process (its parent is this
    /// test process, whose executable is the facts' verifier).
    fn relay_request(&self, request: &Value) -> (Option<i32>, Value) {
        let mut child = Command::new(helper_pin().path)
            .arg("--observe")
            .arg("--test-config")
            .arg(&self.helper_cfg)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        use std::io::Write;
        let _ = child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes());
        let out = child.wait_with_output().unwrap();
        (
            out.status.code(),
            serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
        )
    }
    fn relay(&self, manifest: &[u8]) -> (Option<i32>, Value) {
        self.relay_request(&json!({
            "schema": "axon-protected-observe-request/1",
            "manifest": String::from_utf8(manifest.to_vec()).unwrap(),
        }))
    }
    /// One raw request line to the observer, as this process.
    fn ask(&self, body: &Value) -> Value {
        use std::io::{Read, Write};
        let mut s = std::os::unix::net::UnixStream::connect(&self.socket).unwrap();
        s.write_all(format!("{body}\n").as_bytes()).unwrap();
        let _ = s.shutdown(std::net::Shutdown::Write);
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        serde_json::from_slice(&out).unwrap_or(Value::Null)
    }
    fn ask_manifest(&self, manifest: &[u8]) -> Value {
        self.ask(&json!({
            "schema": "axon-observer-request/1",
            "manifest": String::from_utf8(manifest.to_vec()).unwrap(),
            "caller_sha256": self.facts["verifier_sha256"],
        }))
    }
    /// Fabric's observer config on the relay route (as `ProtectedHost::load`
    /// builds it for a host config naming no observer program).
    fn fabric_cfg(&self) -> axon_fabric::observer::ObserverConfig {
        self.fabric_cfg_via(axon_fabric::backend::PrivilegedRoute {
            helper: helper_pin(),
            owner: euid(),
            test_config: Some(self.helper_cfg.clone()),
        })
    }
    fn fabric_cfg_via(
        &self,
        relay: axon_fabric::backend::PrivilegedRoute,
    ) -> axon_fabric::observer::ObserverConfig {
        axon_fabric::observer::ObserverConfig {
            command: PathBuf::new(),
            command_sha256: String::new(),
            interpreter: None,
            exec_owner: None,
            trust: axon_fabric::observer::ObserverTrust::for_test(&observer_root(&self.base)),
            custodian: axon_fabric::custodian::Custodian::InProcess(
                axon_fabric::observer::NonceStore {
                    dir: self.base.join("unused-nonces"),
                },
            ),
            max_age_s: 300,
            clock: axon_fabric::backend::Clock::System,
            relay: Some(relay),
        }
    }
    /// Fabric's `observer::observe` of `manifest` on the relay route.
    fn fabric_observe(
        &self,
        manifest: &[u8],
        work: &str,
    ) -> Result<axon_fabric::psv::VerifiedObservation, String> {
        self.fabric_observe_cfg(&self.fabric_cfg(), manifest, work)
    }
    fn fabric_observe_cfg(
        &self,
        cfg: &axon_fabric::observer::ObserverConfig,
        manifest: &[u8],
        work: &str,
    ) -> Result<axon_fabric::psv::VerifiedObservation, String> {
        let file = self.base.join(format!("{work}.json"));
        std::fs::write(&file, manifest).unwrap();
        let m: axon_psv::LaunchManifest = serde_json::from_slice(manifest).unwrap();
        axon_fabric::observer::observe(
            cfg,
            &m,
            &axon_psv::sha256_hex(manifest),
            &file,
            m.authority.epoch,
            &self.base.join(work),
        )
    }
}

fn ok(r: &Value) -> bool {
    r["ok"] == true
}

impl Obs {
    /// A nonce the fixture's custodian issues now, for epoch 0 (the epoch the
    /// fixture's manifests name).
    fn nonce(&self) -> String {
        self.cust.issue(0)
    }
}

/// Amendment 107: Fabric asks the helper for an observation with exactly `--observe
/// --test-config FILE` under exactly `PATH=/usr/sbin:/usr/bin:/sbin:/bin`, observed in the
/// CHILD (a compiled stand-in that dumps what it was started with). The call site is a
/// `vec![..]` and an array literal handed to `sealed_exec::command`, which no source form
/// saw: renaming `--observe` or prefixing that PATH kept the suite green.
#[test]
fn fabric_asks_the_helper_for_an_observation_with_exactly_its_flags_and_its_path() {
    let o = obs();
    let helper = dump_helper(&o.base);
    let dump = PathBuf::from(format!("{}.dump", helper.path.display()));
    let cfg_file = o.base.join("some-helper-config");
    let cfg = o.fabric_cfg_via(axon_fabric::backend::PrivilegedRoute {
        helper,
        owner: euid(),
        test_config: Some(cfg_file.clone()),
    });
    let m = o.manifest(&o.nonce(), |_| {});
    let _ = o.fabric_observe_cfg(&cfg, &m, "w-dump");
    let (args, env) = read_dump(&dump);
    let want = vec![
        "--observe".to_string(),
        "--test-config".to_string(),
        cfg_file.display().to_string(),
    ];
    assert!(
        args == want,
        "ATTACK: Fabric asked the helper for an observation with {args:?}, not exactly {want:?}"
    );
    assert!(
        env == ["PATH=/usr/sbin:/usr/bin:/sbin:/bin".to_string()],
        "ATTACK: Fabric ran the observe relay with an environment other than exactly \
         PATH=/usr/sbin:/usr/bin:/sbin:/bin: {env:?}"
    );
}

/// Amendment 68, the CONTROL every observer test stands on, and A94's replay
/// half (M1540): Fabric, on the relay route, obtains an observation the
/// observer SERVICE made of its manifest (the helper measured this running
/// process for it), and it verifies under the operator observer root; a
/// second observation for the same nonce is refused, whatever asks.
#[test]
fn the_observer_service_observes_through_the_helper_once_per_nonce() {
    let mut o = obs();
    o.start();
    let m = o.manifest(&o.nonce(), |_| {});
    let v = o
        .fabric_observe(&m, "w1")
        .expect("control: the relayed observation verifies at Fabric");
    let got: Value = serde_json::from_slice(&v.bytes).unwrap();
    assert_eq!(
        got["observer_key_id"],
        json!(o.key.key_id),
        "control: {got}"
    );
    assert_eq!(
        got["verifier_sha256"], o.facts["verifier_sha256"],
        "control: the running Fabric was measured"
    );
    let again = o.fabric_observe(&m, "w2").map(|v| v.sha256);
    assert!(
        again.is_err(),
        "ATTACK: the observer signed a second observation for one nonce: {again:?}"
    );
    assert!(
        format!("{again:?}").contains("already observed"),
        "{again:?}"
    );
}

/// C9 round 15 (amendment 121, EQUIVALENCE): the prune that runs on every
/// observation (`prune`, `expires.is_none_or(|t| t < now)`) is the local guard behind
/// "one observation per nonce": the record of a nonce the custodian still honours
/// must SURVIVE another request's prune and refuse the replay, and the record of
/// one it no longer honours is the only kind dropped. The first request's record
/// is read back from the store after a SECOND request (whose prune ran), and the
/// replay is made after that.
#[test]
fn a_record_of_a_live_nonce_survives_another_requests_prune_and_refuses_its_replay() {
    let mut o = obs();
    o.start();
    let n = o.nonce();
    let m = o.manifest(&n, |_| {});
    o.fabric_observe(&m, "w1")
        .expect("control: the first observation of the nonce is signed");
    let m2 = o.manifest(&o.nonce(), |_| {});
    o.fabric_observe(&m2, "w2")
        .expect("control: a second nonce's observation is signed (its request pruned the store)");
    let rec = o.obs_dir.join("records").join(format!("{n}.observed"));
    assert!(
        rec.exists(),
        "ATTACK: another request's prune dropped the record of a nonce the custodian still honours"
    );
    let again = o.fabric_observe(&m, "w3").map(|v| v.sha256);
    assert!(
        again.is_err() && format!("{again:?}").contains("already observed"),
        "ATTACK: a nonce was observed twice after another request's prune: {again:?}"
    );
}

/// A94 (M1531-M1539): the observer signs only what it MEASURED. A launch
/// manifest naming, for one installed artifact, a digest other than the
/// installed bytes is not signed. One case per measured field, each with its
/// own nonce; control: the honest manifest is signed.
#[test]
fn the_observer_signs_nothing_it_did_not_measure() {
    let mut o = obs();
    o.start();
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert!(code == Some(0) && ok(&rep), "control: {code:?} {rep}");
    let fields: Vec<String> = o.facts.as_object().unwrap().keys().cloned().collect();
    for field in fields.iter() {
        let m = o.manifest(&o.nonce(), |m| set_field(m, field, json!("9".repeat(64))));
        let (code, rep) = o.relay(&m);
        assert!(
            code == Some(30) && !ok(&rep),
            "ATTACK: the observer signed a launch manifest naming another {field} than it \
             measured: {code:?} {rep}"
        );
        assert!(
            rep["error"]
                .as_str()
                .unwrap_or("")
                .contains(&format!("the launch manifest's {field} is")),
            "{field}: {rep}"
        );
    }
}

/// A94 (M1527): the observer observes only for its configured caller (uid 0,
/// the root helper, on a protected host). Here its caller is another uid than
/// this process's, which relays: refused. Control: the fixture's caller.
#[test]
fn an_observer_observes_only_for_its_caller_uid() {
    let mut o = obs();
    o.edit_observer(|v| v["caller_uid"] = json!(euid().wrapping_add(7)));
    o.start();
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert!(
        code == Some(30) && !ok(&rep),
        "ATTACK: the observer observed for a uid that is not its caller: {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("is not the observer's caller uid"),
        "{rep}"
    );
    drop(o.observer.take());
    let _ = std::fs::remove_file(&o.socket);
    o.edit_observer(|v| v["caller_uid"] = json!(euid()));
    o.start();
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert_eq!(code, Some(0), "control: {rep}");
}

/// M1528: a request of another schema is refused (the observer's request is
/// one fixed shape). Control: the same request at its schema.
#[test]
fn an_observer_request_of_another_schema_is_refused() {
    let mut o = obs();
    o.start();
    let m = o.manifest(&o.nonce(), |_| {});
    let mut body = json!({
        "schema": "axon-observer-request/0",
        "manifest": String::from_utf8(m.clone()).unwrap(),
        "caller_sha256": o.facts["verifier_sha256"],
    });
    let r = o.ask(&body);
    assert!(
        !ok(&r),
        "ATTACK: the observer answered a request of another schema: {r}"
    );
    body["schema"] = json!("axon-observer-request/1");
    assert!(ok(&o.ask(&body)), "control");
}

/// M1529: the observer signs only a protected launch manifest (canonical,
/// `/2`, the protected profile): a manifest for the development profile,
/// every measured fact the installed one, is not observed. Control: the same
/// manifest for the protected profile.
#[test]
fn an_observer_never_observes_a_manifest_that_is_not_a_protected_launch() {
    let mut o = obs();
    o.start();
    let m = o.manifest(&o.nonce(), |m| {
        m["backend_profile"] = json!("linux-microvm")
    });
    let r = o.ask_manifest(&m);
    assert!(
        !ok(&r),
        "ATTACK: the observer signed a manifest for another profile than the protected one: {r}"
    );
    assert!(
        ok(&o.ask_manifest(&o.manifest(&o.nonce(), |_| {}))),
        "control"
    );
}

/// A custodian impostor of the custodian uid (the observer authenticates the
/// custodian by uid, not by program): bound on `argv[1]`, it answers every
/// `check` "outstanding, expires far in the future", whatever the nonce.
const AGREEABLE_CUSTODIAN: &str = r#"
import socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.bind(sys.argv[1]); s.listen(8)
open(sys.argv[1] + ".ready", "w").close()
while True:
    c, _ = s.accept()
    c.recv(65536)
    c.sendall(b'{"schema":"axon-custodian-reply/1","ok":true,"mode":"test","nonce":null,'
              b'"error":null,"expires_unix":4000000000}\n')
    c.close()
"#;

/// M1530: the nonce names the observer's record, so it is a nonce or
/// nothing: one that spells a path is refused before anything is written
/// (else the record lands outside the store). The custodian would refuse such
/// a nonce too, but the observer does not rest on it: it authenticates the
/// custodian by uid only, so a custodian impostor of that uid (here: one that
/// says yes to anything) must not be able to name a path. Control: a nonce.
#[test]
fn an_observer_never_records_a_nonce_that_names_a_path() {
    let mut o = obs();
    o.start();
    let sock = o.base.join("agreeable.sock");
    let _imp = Running(
        Command::new("python3")
            .args(["-c", AGREEABLE_CUSTODIAN])
            .arg(&sock)
            .spawn()
            .unwrap(),
    );
    let ready = PathBuf::from(format!("{}.ready", sock.display()));
    for _ in 0..200 {
        if ready.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(
        ready.exists(),
        "setup: the agreeable custodian never listened"
    );
    let real = o.cust.socket.clone();
    o.edit_helper(|v| v["custodian"]["socket"] = json!(sock));
    let m = o.manifest("../../escaped", |_| {});
    let r = o.ask_manifest(&m);
    let escaped = o.obs_dir.parent().unwrap().join("escaped.observed");
    assert!(
        !ok(&r) && !escaped.exists(),
        "ATTACK: the observer recorded and signed a nonce that names a path ({} exists: {}): {r}",
        escaped.display(),
        escaped.exists()
    );
    assert!(
        r["error"]
            .as_str()
            .unwrap_or("")
            .contains("custodian issues"),
        "{r}"
    );
    o.edit_helper(|v| v["custodian"]["socket"] = json!(real));
    assert!(
        ok(&o.ask_manifest(&o.manifest(&o.nonce(), |_| {}))),
        "control"
    );
}

/// POSIX access ACL granting `uid` read, written as its xattr.
fn grant_acl_read(path: &Path, uid: u32) -> bool {
    Command::new("python3")
        .args([
            "-c",
            "import os,struct,sys\n\
             p,u=sys.argv[1],int(sys.argv[2]);X=0xFFFFFFFF\n\
             e=[(1,4,X),(2,4,u),(4,0,X),(0x10,4,X),(0x20,0,X)]\n\
             os.setxattr(p,'system.posix_acl_access',struct.pack('<I',2)+b''.join(struct.pack('<HHI',*x) for x in e))",
        ])
        .arg(path)
        .arg(uid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A94 (M1522): the observer's key is readable by no other uid. A key whose
/// mode lets its group or anyone read it (the Fabric uid among them), or a
/// POSIX ACL naming the Fabric uid (its mask is the group bits), is refused
/// and the observer serves nothing. Control: the 0400 key.
#[test]
fn an_observer_key_another_uid_can_read_is_refused() {
    for mode in [0o440, 0o444] {
        let mut o = obs();
        set_mode(&o.key.pk8, mode);
        let got = o.try_start(&[], None);
        assert!(
            got.is_err(),
            "ATTACK: the observer started with a key another uid can read (mode {mode:o})"
        );
        assert!(
            got.unwrap_err()
                .contains("must be a regular file owned by the observer uid"),
            "mode {mode:o}"
        );
    }
    let mut o = obs();
    if grant_acl_read(&o.key.pk8, FABRIC) {
        let got = o.try_start(&[], None);
        assert!(
            got.is_err(),
            "ATTACK: the observer started with a key an ACL lets the Fabric uid read"
        );
    } else {
        eprintln!("note: ACLs unavailable here; the ACL case was not exercised");
    }
    let mut o = obs();
    o.try_start(&[], None)
        .expect("control: a 0400 key the observer owns");
}

/// A94 (M1523), ROOT ONLY: a key owned by another uid (the Fabric's) is not
/// the observer's, even when the observer could read it (here it runs as
/// root). Control: the same key owned by the observer uid.
#[test]
fn an_observer_key_owned_by_another_uid_is_refused() {
    if skip_unless_root() {
        return;
    }
    let mut o = obs();
    chown(&o.key.pk8, FABRIC);
    let got = o.try_start(&[], None);
    assert!(
        got.is_err(),
        "ATTACK: the observer started with a key the Fabric uid owns"
    );
    chown(&o.key.pk8, 0);
    o.try_start(&[], None)
        .expect("control: the key the observer uid owns");
}

/// M1524: the observer's key is one the operator observer root trusts; a
/// key no verifier trusts is refused at start, not at the first launch.
#[test]
fn an_observer_key_the_observer_root_does_not_hold_is_refused() {
    let mut o = obs();
    let stray = observer_key(&o.obs_dir, "stray", &[]);
    set_mode(&stray.pk8, 0o400);
    o.edit_observer(|v| v["key_path"] = json!(stray.pk8));
    let got = o.try_start(&[], None);
    assert!(
        got.is_err(),
        "ATTACK: the observer started with a key the observer root does not hold"
    );
    assert!(got.unwrap_err().contains("is not in the observer root"));
    o.edit_observer(|v| v["key_path"] = json!(o.key.pk8));
    o.try_start(&[], None).expect("control: the trusted key");
}

/// M1525: the observer's record store is its own and private; one its group
/// can write (where the Fabric could erase a nonce's record and have it
/// observed again) is refused. Control: 0700.
#[test]
fn an_observer_refuses_a_store_others_can_reach() {
    let mut o = obs();
    set_mode(&o.obs_dir.join("records"), 0o770);
    let got = o.try_start(&[], None);
    assert!(
        got.is_err(),
        "ATTACK: the observer served from a store its group can write"
    );
    set_mode(&o.obs_dir.join("records"), 0o700);
    o.try_start(&[], None).expect("control: a 0700 store");
}

/// M1526: the observer runs only as the uid its config names (the uid its
/// key belongs to). Control: its own uid.
#[test]
fn an_observer_runs_only_as_its_configured_uid() {
    let mut o = obs();
    o.edit_observer(|v| v["observer_uid"] = json!(euid().wrapping_add(7)));
    let got = o.try_start(&[], None);
    assert!(
        got.is_err(),
        "ATTACK: the observer ran as a uid its config does not name"
    );
    o.edit_observer(|v| v["observer_uid"] = json!(euid()));
    o.try_start(&[], None).expect("control");
}

/// The impostor observer: bound on `argv[1]`, it answers every request with a
/// well-formed `ok` reply carrying an observation it made up. The program is
/// python3, not the pinned `axon-observer`.
const IMPOSTOR: &str = r#"
import socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.bind(sys.argv[1]); s.listen(8)
open(sys.argv[1] + ".ready", "w").close()
while True:
    c, _ = s.accept()
    c.recv(65536)
    c.sendall(b'{"schema":"axon-observer-reply/1","ok":true,"mode":"test","observation":"{}",'
              b'"signature":"{}","error":null}\n')
    c.close()
"#;

/// A94 (M1542; the comparison is M1483's, the verification as a whole
/// M1489's): the helper relays only what the observer PROGRAM the operator
/// pinned sent. An impostor on the observer socket (this uid, python3)
/// answers `ok` with an observation of its own. Control: the genuine
/// observer (`the_observer_service_observes_through_the_helper_once_per_nonce`).
#[test]
fn an_observer_program_the_operator_never_pinned_is_never_relayed() {
    let o = obs();
    let _imp = Running(
        Command::new("python3")
            .args(["-c", IMPOSTOR])
            .arg(&o.socket)
            .spawn()
            .unwrap(),
    );
    let ready = PathBuf::from(format!("{}.ready", o.socket.display()));
    for _ in 0..200 {
        if ready.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(ready.exists(), "setup: the impostor never listened");
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert!(
        code == Some(30) && !ok(&rep),
        "ATTACK: the helper relayed an observation from an observer program the operator \
         never pinned: {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("not served by the pinned observer program"),
        "{rep}"
    );
}

/// D6's rule for the observer (M1543): a DEV observer's observation is never
/// relayed (a test observer's only by a test-trust helper; a production
/// helper relays only a protected one). Control: the test observer.
#[test]
fn a_dev_observer_is_never_relayed() {
    let mut o = obs();
    o.try_start(&["--dev"], None)
        .expect("setup: a dev observer");
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert!(
        code == Some(30) && !ok(&rep),
        "ATTACK: the helper relayed a dev observer's observation: {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap_or("").contains("dev observer"),
        "{rep}"
    );
    drop(o.observer.take());
    let _ = std::fs::remove_file(&o.socket);
    o.start();
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert_eq!(code, Some(0), "control: the test observer: {rep}");
}

/// M1545: the helper's observe request is one fixed schema. Control: the
/// same request at its schema.
#[test]
fn an_observe_request_of_another_schema_relays_nothing() {
    let mut o = obs();
    o.start();
    let m = String::from_utf8(o.manifest(&o.nonce(), |_| {})).unwrap();
    let (code, rep) =
        o.relay_request(&json!({"schema": "axon-protected-observe-request/0", "manifest": m}));
    assert!(
        code == Some(30) && !ok(&rep),
        "ATTACK: the helper relayed an observe request of another schema: {code:?} {rep}"
    );
    let (code, rep) =
        o.relay_request(&json!({"schema": "axon-protected-observe-request/1", "manifest": m}));
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A (M531, the helper's ONE caller gate, now `authenticated`): the observe
/// relay authenticates its caller exactly as a launch does. Control: the
/// configured Fabric uid.
#[test]
fn an_observe_relay_for_a_caller_that_is_not_the_fabric_relays_nothing() {
    let mut o = obs();
    o.start();
    o.edit_helper(|v| v["fabric_uid"] = json!(euid().wrapping_add(7)));
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert!(
        code == Some(30) && !ok(&rep),
        "ATTACK: the helper relayed an observation for a caller that is not the Fabric uid: \
         {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("is not the configured Fabric uid"),
        "{rep}"
    );
    o.edit_helper(|v| v["fabric_uid"] = json!(unsafe { libc::getuid() }));
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A94 (M1541; M626's twin on the observer client), ROOT ONLY: the helper
/// relays only from a socket whose listener is the observer uid (or root's
/// activation). Here the GENUINE axon-observer runs as another uid than the
/// one the helper's config names, with a key the observer root trusts: only
/// the listener's uid tells it apart. Control: the config naming that uid.
#[test]
fn an_observer_socket_another_uid_serves_is_never_relayed() {
    if skip_unless_root() {
        return;
    }
    // The observer runs as OTHER here, so it is OTHER the custodian answers.
    let mut o = obs_as(OTHER);
    set_mode(&o.cust.socket, 0o666);
    for p in [&o.obs_dir, &o.obs_dir.join("records"), &o.key.pk8] {
        chown(p, OTHER);
    }
    o.edit_observer(|v| {
        v["observer_uid"] = json!(OTHER);
        v["caller_uid"] = json!(0);
    });
    o.try_start(&[], Some(OTHER))
        .expect("setup: the observer runs as its own uid");
    o.edit_helper(|v| v["observer"]["service"]["uid"] = json!(NOT_OTHER));
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert!(
        code == Some(30) && !ok(&rep),
        "ATTACK: the helper relayed an observation from a socket a uid other than the observer's \
         serves: {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("is served by uid"),
        "{rep}"
    );
    o.edit_helper(|v| v["observer"]["service"]["uid"] = json!(OTHER));
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert_eq!(code, Some(0), "control: the observer uid's socket: {rep}");
}

/// A94 (M1544), ROOT ONLY: the helper measures the RUNNING Fabric, its
/// parent, and relays nothing when that parent is not the Fabric uid. The
/// setuid-root helper is executed with the Fabric uid as its real uid, but
/// by a process of another uid (here root: `setpriv` then `exec`, so the
/// helper's parent is this test). Control: a Fabric-uid process (python3)
/// that runs the helper itself, whose executable the manifest names.
#[test]
fn an_observe_relay_whose_parent_is_not_the_fabric_relays_nothing() {
    if skip_unless_root() {
        return;
    }
    let mut o = obs();
    o.edit_helper(|v| v["fabric_uid"] = json!(FABRIC));
    o.edit_observer(|v| v["caller_uid"] = json!(0));
    // The custodian the observer asks is reached through this socket, and the
    // observer runs as this (root) uid: nothing else changes here.
    o.start();
    let h = o.base.join("axon-protected-launcher");
    copy_executable(helper_pin().path, &h, 0o755);
    std::os::unix::fs::chown(&h, Some(0), Some(FABRIC)).unwrap();
    set_mode(&h, 0o4750);
    let python = std::fs::canonicalize(
        String::from_utf8(
            Command::new("sh")
                .args(["-c", "command -v python3"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim(),
    )
    .unwrap();
    let request = |verifier: &str, n: u8| -> PathBuf {
        let p = o.base.join(format!("observe-{n}.json"));
        std::fs::write(
            &p,
            json!({"schema": "axon-protected-observe-request/1",
                   "manifest": String::from_utf8(o.manifest(&o.nonce(), |m| {
                       m["verifier_sha256"] = json!(verifier)
                   })).unwrap()})
            .to_string(),
        )
        .unwrap();
        set_mode(&p, 0o644);
        p
    };
    let run = |shell: &str, req: PathBuf| -> Value {
        let out = Command::new("setpriv")
            .arg(format!("--reuid={FABRIC}"))
            .arg(format!("--regid={FABRIC}"))
            .arg("--clear-groups")
            .args(["--", "sh", "-c", shell])
            .arg(&h)
            .arg(&o.helper_cfg)
            .arg(req)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null)
    };
    // The helper's parent is this (root) test process: sh execs it.
    let rep = run(
        "exec \"$0\" --observe --test-config \"$1\" < \"$2\"",
        request(o.facts["verifier_sha256"].as_str().unwrap(), 14),
    );
    assert!(
        !ok(&rep),
        "ATTACK: the helper relayed an observation for a parent that is not the Fabric uid: {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("not the Fabric uid"),
        "{rep}"
    );
    // Amendment 79: the control Fabric is the python3 that runs the helper,
    // which the operator pins (the attack above is refused before any pin).
    o.edit_helper(|v| v["fabric"] = program_pin(&python, TEST_FABRIC_REVISION));
    let rep = run(
        "exec python3 -c 'import subprocess,sys; \
         p=subprocess.run([sys.argv[1],\"--observe\",\"--test-config\",sys.argv[2]], \
         stdin=open(sys.argv[3],\"rb\"), capture_output=True); sys.stdout.buffer.write(p.stdout)' \
         \"$0\" \"$1\" \"$2\"",
        request(&exe_sha256(&python), 15),
    );
    assert!(
        ok(&rep),
        "control: the Fabric-uid parent is measured: {rep}"
    );
}

// ── Amendment 79 (C9 round 5, PSV-6): what the observer countersigns ─────────

/// A131 (M1853-M1855 by the field loop above; the reviewer's reproduction):
/// a manifest naming a Fabric revision, a guest init digest and a guest axon
/// digest that are not the operator's was relayed and SIGNED, because the
/// observer copied those three from the manifest. They are now the pinned
/// program's revision and the digests the measured profile manifest names.
/// Control: the honest manifest. The same manifest's `policy_sha256` and
/// authority epoch are still the principal's word, and this test pins that
/// (the stated non-claim of amendment 79: policy is bound at the root helper
/// to the bytes it boots, the epoch to the loop's scope pointer at intake).
#[test]
fn the_observer_signs_the_operators_values_for_fabric_revision_and_guest_digests() {
    let mut o = obs();
    o.start();
    let h = "9".repeat(64);
    let m = o.manifest(&o.nonce(), |m| {
        set_field(m, "guest.init_sha256", json!(h.clone()));
        set_field(m, "guest.axon_sha256", json!(h.clone()));
        set_field(m, "fabric_revision", json!("not-the-running-revision"));
    });
    let (code, rep) = o.relay(&m);
    assert!(
        code == Some(30) && !ok(&rep),
        "ATTACK: the observer signed a launch manifest naming a Fabric revision and guest \
         digests that are not the operator's: {code:?} {rep}"
    );
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert!(code == Some(0) && ok(&rep), "control: {code:?} {rep}");
    // The stated non-claim: the policy digest and the epoch are told.
    let told = "8".repeat(64);
    let epoch = 987_654_321u64;
    let n = o.cust.issue(epoch);
    let m = o.manifest(&n, |m| {
        set_field(m, "policy_sha256", json!(told.clone()));
        set_field(m, "authority.epoch", json!(epoch));
    });
    let (code, rep) = o.relay(&m);
    assert!(
        code == Some(0) && ok(&rep),
        "the policy digest and the epoch are the principal's word (amendment 79): {code:?} {rep}"
    );
    let obs: Value = serde_json::from_str(rep["observation"].as_str().unwrap()).unwrap();
    assert_eq!(obs["policy_sha256"], json!(told), "signed as told: {obs}");
    assert_eq!(obs["epoch"], json!(epoch), "signed as told: {obs}");
}

/// A131 (M1853): the observer names only the Fabric program the operator
/// pinned. Asked directly (as the caller uid) with a `caller_sha256` that is
/// not the pin while the manifest claims the pinned program, it signs nothing:
/// the observer does not rest on the manifest's word that its caller is the
/// pinned Fabric. Control: the pinned program's digest.
#[test]
fn an_observer_names_only_the_fabric_program_the_operator_pinned() {
    let mut o = obs();
    o.start();
    let other = "9".repeat(64);
    let m = o.manifest(&o.nonce(), |_| {});
    let r = o.ask(&json!({
        "schema": "axon-observer-request/1",
        "manifest": String::from_utf8(m).unwrap(),
        "caller_sha256": other,
    }));
    assert!(
        !ok(&r),
        "ATTACK: the observer signed an observation naming a Fabric program the operator never \
         pinned as the verifier: {r}"
    );
    assert!(
        r["error"].as_str().unwrap_or("").contains("pinned program"),
        "{r}"
    );
    assert!(
        ok(&o.ask_manifest(&o.manifest(&o.nonce(), |_| {}))),
        "control"
    );
}

/// A132 (the same gate as M1851, on the observe route): the helper relays an
/// observation only for the Fabric program the operator pinned. The only
/// process that can ask here is this test process; its pin is changed to
/// another digest. Control: the pin put back.
#[test]
fn an_observe_relay_for_a_program_the_operator_never_pinned_relays_nothing() {
    let mut o = obs();
    o.start();
    let real = o.helper_fabric_sha();
    o.edit_helper(|v| v["fabric"]["sha256"] = json!("9".repeat(64)));
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert!(
        code == Some(30) && !ok(&rep),
        "ATTACK: the helper relayed an observation for a caller that is not the pinned Fabric \
         program: {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("pinned Fabric"),
        "{rep}"
    );
    o.edit_helper(|v| v["fabric"]["sha256"] = json!(real));
    let (code, rep) = o.relay(&o.manifest(&o.nonce(), |_| {}));
    assert_eq!(code, Some(0), "control: {rep}");
}

impl Obs {
    fn helper_fabric_sha(&self) -> String {
        let v: Value = serde_json::from_slice(&std::fs::read(&self.helper_cfg).unwrap()).unwrap();
        v["fabric"]["sha256"].as_str().unwrap().to_string()
    }
}

/// A133 (M1860): the observer observes only a nonce the custodian issued.
/// Before, any 32-hex string in a manifest got a rootfs hash, a record and a
/// signature, and Fabric could fill the observer's store with them. A nonce
/// the custodian never issued is refused with no record written and nothing
/// signed. Control: an issued nonce.
#[test]
fn an_observer_signs_nothing_for_a_nonce_the_custodian_never_issued() {
    let mut o = obs();
    o.start();
    let invented = "ab".repeat(16);
    let r = o.ask_manifest(&o.manifest(&invented, |_| {}));
    let record = o
        .obs_dir
        .join("records")
        .join(format!("{invented}.observed"));
    assert!(
        !ok(&r) && !record.exists(),
        "ATTACK: the observer observed a nonce the custodian never issued (record exists: {}): {r}",
        record.exists()
    );
    assert!(
        r["error"]
            .as_str()
            .unwrap_or("")
            .contains("does not honour this nonce"),
        "{r}"
    );
    assert!(
        ok(&o.ask_manifest(&o.manifest(&o.nonce(), |_| {}))),
        "control: an issued nonce"
    );
}

/// A133 (M1861): and for the epoch it was issued for. A nonce issued for
/// epoch 0, in a manifest naming authority epoch 5, is not observed.
/// Control: the same nonce in a manifest naming epoch 0.
#[test]
fn an_observer_signs_nothing_for_a_nonce_issued_for_another_epoch() {
    let mut o = obs();
    o.start();
    let n = o.nonce();
    let r = o.ask_manifest(&o.manifest(&n, |m| set_field(m, "authority.epoch", json!(5))));
    assert!(
        !ok(&r),
        "ATTACK: the observer observed a nonce for an epoch other than the one it was issued \
         for: {r}"
    );
    assert!(r["error"].as_str().unwrap_or("").contains("epoch"), "{r}");
    assert!(ok(&o.ask_manifest(&o.manifest(&n, |_| {}))), "control");
}

/// A133 (M1863): a nonce a DEV custodian issued is not observed by a test or
/// protected observer (the same rule the helper applies at the spend). The
/// fixture's custodian is a dev one here. Control: the same flow against a
/// test custodian (`the_observer_service_observes_through_the_helper_once_per_nonce`).
#[test]
fn an_observer_takes_no_nonce_from_a_dev_custodian() {
    let mut o = obs_with(euid(), CustodianRun::Dev);
    o.start();
    let r = o.ask_manifest(&o.manifest(&o.nonce(), |_| {}));
    assert!(
        !ok(&r),
        "ATTACK: the observer observed a nonce a dev custodian issued: {r}"
    );
    assert!(
        r["error"].as_str().unwrap_or("").contains("dev custodian"),
        "{r}"
    );
}

/// A134 (M1864): the observer's store holds a record only as long as the
/// custodian honours the nonce. A record whose nonce has expired is dropped at
/// the next observation; one still honoured stays; the new one carries the
/// custodian's expiry. Control: the fresh record stays.
#[test]
fn an_observer_drops_the_records_of_nonces_the_custodian_no_longer_honours() {
    let mut o = obs();
    o.start();
    let store = o.obs_dir.join("records");
    let stale = store.join(format!("{}.observed", "cd".repeat(16)));
    let fresh = store.join(format!("{}.observed", "ef".repeat(16)));
    std::fs::write(
        &stale,
        json!({"manifest_sha256": "a", "expires_unix": 1}).to_string(),
    )
    .unwrap();
    std::fs::write(
        &fresh,
        json!({"manifest_sha256": "a", "expires_unix": 4_000_000_000i64}).to_string(),
    )
    .unwrap();
    let n = o.nonce();
    let r = o.ask_manifest(&o.manifest(&n, |_| {}));
    assert!(ok(&r), "setup: an honest observation: {r}");
    assert!(
        !stale.exists(),
        "ATTACK: the observer kept the record of a nonce the custodian no longer honours"
    );
    assert!(fresh.exists(), "control: a record still honoured stays");
    let mine: Value =
        serde_json::from_slice(&std::fs::read(store.join(format!("{n}.observed"))).unwrap())
            .unwrap();
    let now = axon_fabric::backend::Clock::System.now_unix();
    let expires = mine["expires_unix"].as_i64().unwrap();
    assert!(
        expires > now && expires <= now + 300,
        "the record carries the custodian's expiry (300 s from issue): {mine}"
    );
}

// ── Amendment 85 (C9 round 6, PSV-6): the exec race ──────────────────────────

/// The attacker: a Fabric-uid python that is NOT the pinned program. A forked
/// worker holds the read end of a pipe; the parent spawns `helper --observe`
/// with the pipe as its stdout and an attacker-authored manifest on stdin, then
/// immediately `execv`s the pinned file. The helper measures /proc/<ppid>/exe
/// AFTER the exec and sees the pinned file. The worker writes what it read.
const EXEC_RACE_ATTACKER: &str = r#"
import os, sys, subprocess
h, cfg, req, out, pinned = sys.argv[1:6]
r, w = os.pipe()
pid = os.fork()
if pid == 0:
    os.close(w)
    data = b''
    while True:
        b = os.read(r, 65536)
        if not b:
            break
        data += b
    open(out, 'wb').write(data)
    os._exit(0)
os.close(r)
subprocess.Popen([h, '--observe', '--test-config', cfg], stdin=open(req, 'rb'), stdout=w)
os.close(w)
os.execv(pinned, [pinned, '1'])
"#;

/// The genuine caller: the pinned program (python3 here) runs the helper and
/// reads the reply itself, the shape of the real Fabric.
const GENUINE_CALLER: &str = r#"
import subprocess, sys
h, cfg, req, out = sys.argv[1:5]
p = subprocess.run([h, '--observe', '--test-config', cfg], stdin=open(req, 'rb'), capture_output=True)
open(out, 'wb').write(p.stdout)
"#;

/// A149 (M2050, M2052; amendment 85), ROOT ONLY. EXECUTED by the round-6
/// reviewer: 18 of 20 attempts of the exec race got an observer-signed
/// observation naming the pinned verifier. The helper now serves a request only
/// when no process but itself and its parent holds the pipe the reply is
/// written to: the worker is refused, 20 of 20. Control: the pinned program
/// itself (python3), reading its own pipe, is served.
#[test]
fn a_program_that_execs_the_pinned_file_after_spawning_the_helper_gets_no_observation() {
    if skip_unless_root() {
        return;
    }
    let mut o = obs();
    o.edit_helper(|v| {
        v["fabric_uid"] = json!(FABRIC);
        v["private_reply_channel"] = json!(true);
    });
    o.edit_observer(|v| v["caller_uid"] = json!(0));
    o.start();
    let h = o.base.join("axon-protected-launcher");
    copy_executable(helper_pin().path, &h, 0o755);
    std::os::unix::fs::chown(&h, Some(0), Some(FABRIC)).unwrap();
    set_mode(&h, 0o4750);
    let outd = o.base.join("outd");
    std::fs::create_dir(&outd).unwrap();
    std::os::unix::fs::chown(&outd, Some(FABRIC), Some(FABRIC)).unwrap();
    let run = |script: &str, extra: &[&Path], verifier: &str, tag: &str| -> Value {
        let s = o.base.join(format!("{tag}.py"));
        std::fs::write(&s, script).unwrap();
        set_mode(&s, 0o644);
        let req = o.base.join(format!("{tag}-req.json"));
        let man = String::from_utf8(o.manifest(&o.nonce(), |m| {
            m["verifier_sha256"] = json!(verifier);
        }))
        .unwrap();
        std::fs::write(
            &req,
            json!({"schema":"axon-protected-observe-request/1","manifest":man}).to_string(),
        )
        .unwrap();
        set_mode(&req, 0o644);
        let out = outd.join(format!("{tag}.out"));
        let _ = std::fs::remove_file(&out);
        let st = Command::new("setpriv")
            .arg(format!("--reuid={FABRIC}"))
            .arg(format!("--regid={FABRIC}"))
            .arg("--clear-groups")
            .args(["--", "python3"])
            .arg(&s)
            .arg(&h)
            .arg(&o.helper_cfg)
            .arg(&req)
            .arg(&out)
            .args(extra)
            .stdin(Stdio::null())
            .status()
            .unwrap();
        let _ = st;
        // The worker writes after the helper's reply (or refusal) closes the pipe.
        for _ in 0..200 {
            if std::fs::metadata(&out)
                .map(|m| m.len() > 0)
                .unwrap_or(false)
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        serde_json::from_slice(&std::fs::read(&out).unwrap_or_default()).unwrap_or(Value::Null)
    };
    // Control: the genuine pinned program, reading its own pipe.
    let python = std::fs::canonicalize(
        String::from_utf8(
            Command::new("sh")
                .args(["-c", "command -v python3"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim(),
    )
    .unwrap();
    o.edit_helper(|v| v["fabric"] = program_pin(&python, TEST_FABRIC_REVISION));
    let rep = run(GENUINE_CALLER, &[], &exe_sha256(&python), "genuine");
    assert!(
        ok(&rep),
        "control: the pinned program reading its own pipe is served: {rep}"
    );
    // Attack: a non-pinned program that execs the pinned file (sleep).
    let sleep = std::fs::canonicalize("/usr/bin/sleep").unwrap();
    let pin = exe_sha256(&sleep);
    o.edit_helper(|v| v["fabric"] = program_pin(&sleep, TEST_FABRIC_REVISION));
    let mut got = 0;
    for n in 0..20 {
        let rep = run(EXEC_RACE_ATTACKER, &[&sleep], &pin, &format!("race{n}"));
        if ok(&rep) {
            got += 1;
        }
    }
    assert!(
        got == 0,
        "ATTACK: {got} of 20 observations were relayed to a program that is not the pinned one and \
         exec'd the pinned file after spawning the helper"
    );
}

/// Amendment 95 (eqgate4): the observer key must be a REGULAR file, whatever
/// else about it holds. A FIFO the observer uid owns at mode 0400 passes the
/// owner and mode terms, and a FIFO whose writer has gone and whose buffer
/// still holds the key's bytes is READ TO EOF by `load_key` (the buffer is kept
/// alive by a second descriptor), so with the type term removed the key is
/// accepted from a pipe; a directory or a device would be refused later by the
/// read itself, which is why this one is a FIFO. Control: the same bytes in a
/// regular 0400 file load.
#[test]
fn an_observer_key_that_is_a_fifo_is_refused_even_when_it_holds_the_key() {
    let d = tempfile::tempdir_in("/var/tmp").unwrap();
    set_mode(d.path(), 0o700);
    let k = observer_key(d.path(), "fifo-src", &[]);
    let bytes = std::fs::read(&k.pk8).unwrap();
    let me = euid();
    // CONTROL: a regular 0400 file with these bytes loads.
    let regular = d.path().join("regular.pk8");
    std::fs::write(&regular, &bytes).unwrap();
    set_mode(&regular, 0o400);
    axon_fabric::observer_service::load_key(&regular, me)
        .expect("control: a regular 0400 key the observer owns");
    // ATTACK: the key's bytes in a FIFO, mode 0400, writer gone.
    let fifo = d.path().join("key.fifo");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(
        unsafe { libc::mkfifo(c.as_ptr(), 0o600) },
        0,
        "setup: mkfifo"
    );
    use std::os::unix::fs::OpenOptionsExt;
    let hold = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&fifo)
        .expect("setup: a descriptor that keeps the pipe buffer alive");
    {
        use std::io::Write;
        let mut w = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .expect("setup: the writer");
        w.write_all(&bytes).expect("setup: the key fits the pipe");
    }
    set_mode(&fifo, 0o400);
    let got = axon_fabric::observer_service::load_key(&fifo, me);
    drop(hold);
    match got {
        Ok(_) => panic!("ATTACK: the observer accepted its signing key from a FIFO"),
        Err(e) => assert!(e.contains("must be a regular file"), "{e}"),
    }
}

/// Amendment 103: the owner Fabric passes to `open_verified` for the relay helper
/// (`Some(h.owner)`) is the use-time re-check that the helper's bytes cannot be
/// changed by a stranger after they are verified. Round 10 replaced it by `None`
/// ("any owner: development only") with the whole suite green: the load-time
/// ownership walk and the digest pin dominate, so nothing observed THIS argument.
/// A helper owned by neither root nor the configured owner is never executed.
/// Control: the same file with its own uid as the configured owner is.
#[test]
fn an_observe_relay_helper_owned_by_a_stranger_is_never_executed() {
    if skip_unless_root() {
        return;
    }
    let mut o = obs();
    o.start();
    let m = o.manifest(&o.nonce(), |_| {});
    let h = o.base.join("stranger-owned-helper");
    copy_executable(helper_pin().path, &h, 0o755);
    chown(&h, OTHER);
    let route = |owner: u32| axon_fabric::backend::PrivilegedRoute {
        helper: axon_fabric::sealed_exec::Pinned {
            sha256: sha256_file(&h),
            path: h.clone(),
        },
        owner,
        test_config: Some(o.helper_cfg.clone()),
    };
    o.fabric_observe_cfg(&o.fabric_cfg_via(route(OTHER)), &m, "w-owner")
        .expect("control: a helper owned by its configured owner is executed");
    let m2 = o.manifest(&o.nonce(), |_| {});
    let e = o
        .fabric_observe_cfg(&o.fabric_cfg_via(route(FABRIC)), &m2, "w-stranger")
        .err()
        .unwrap_or_else(|| {
            panic!(
                "ATTACK: the relay executed a helper owned by uid {OTHER}, neither root nor the \
                 configured owner {FABRIC}"
            )
        });
    assert!(
        e.contains("is owned by uid"),
        "ATTACK: the relay refused for another reason than the owner of the helper: {e}"
    );
}
