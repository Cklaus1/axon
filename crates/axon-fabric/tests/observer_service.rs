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
    exe_sha256(&std::env::current_exe().unwrap())
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
}

fn obs() -> Obs {
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
            relay: Some(axon_fabric::backend::PrivilegedRoute {
                helper: helper_pin(),
                owner: euid(),
                test_config: Some(self.helper_cfg.clone()),
            }),
        }
    }
    /// Fabric's `observer::observe` of `manifest` on the relay route.
    fn fabric_observe(
        &self,
        manifest: &[u8],
        work: &str,
    ) -> Result<axon_fabric::psv::VerifiedObservation, String> {
        let file = self.base.join(format!("{work}.json"));
        std::fs::write(&file, manifest).unwrap();
        let m: axon_psv::LaunchManifest = serde_json::from_slice(manifest).unwrap();
        axon_fabric::observer::observe(
            &self.fabric_cfg(),
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

fn nonce(n: u8) -> String {
    format!("{n:02x}").repeat(16)
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
    let m = o.manifest(&nonce(1), |_| {});
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

/// A94 (M1531-M1539): the observer signs only what it MEASURED. A launch
/// manifest naming, for one installed artifact, a digest other than the
/// installed bytes is not signed. One case per measured field, each with its
/// own nonce; control: the honest manifest is signed.
#[test]
fn the_observer_signs_nothing_it_did_not_measure() {
    let mut o = obs();
    o.start();
    let (code, rep) = o.relay(&o.manifest(&nonce(2), |_| {}));
    assert!(code == Some(0) && ok(&rep), "control: {code:?} {rep}");
    let fields: Vec<String> = o.facts.as_object().unwrap().keys().cloned().collect();
    for (i, field) in fields.iter().enumerate() {
        let m = o.manifest(&nonce(16 + i as u8), |m| {
            set_field(m, field, json!("9".repeat(64)))
        });
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
    let (code, rep) = o.relay(&o.manifest(&nonce(3), |_| {}));
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
    let (code, rep) = o.relay(&o.manifest(&nonce(3), |_| {}));
    assert_eq!(code, Some(0), "control: {rep}");
}

/// M1528: a request of another schema is refused (the observer's request is
/// one fixed shape). Control: the same request at its schema.
#[test]
fn an_observer_request_of_another_schema_is_refused() {
    let mut o = obs();
    o.start();
    let m = o.manifest(&nonce(4), |_| {});
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
    let m = o.manifest(&nonce(5), |m| m["backend_profile"] = json!("linux-microvm"));
    let r = o.ask_manifest(&m);
    assert!(
        !ok(&r),
        "ATTACK: the observer signed a manifest for another profile than the protected one: {r}"
    );
    assert!(
        ok(&o.ask_manifest(&o.manifest(&nonce(5), |_| {}))),
        "control"
    );
}

/// M1530: the nonce names the observer's record, so it is a nonce or
/// nothing: one that spells a path is refused before anything is written
/// (else the record lands outside the store). Control: a nonce.
#[test]
fn an_observer_never_records_a_nonce_that_names_a_path() {
    let mut o = obs();
    o.start();
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
        ok(&o.ask_manifest(&o.manifest(&nonce(6), |_| {}))),
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
    let (code, rep) = o.relay(&o.manifest(&nonce(7), |_| {}));
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
    let (code, rep) = o.relay(&o.manifest(&nonce(8), |_| {}));
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
    let (code, rep) = o.relay(&o.manifest(&nonce(9), |_| {}));
    assert_eq!(code, Some(0), "control: the test observer: {rep}");
}

/// M1545: the helper's observe request is one fixed schema. Control: the
/// same request at its schema.
#[test]
fn an_observe_request_of_another_schema_relays_nothing() {
    let mut o = obs();
    o.start();
    let m = String::from_utf8(o.manifest(&nonce(10), |_| {})).unwrap();
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
    let (code, rep) = o.relay(&o.manifest(&nonce(11), |_| {}));
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
    let (code, rep) = o.relay(&o.manifest(&nonce(11), |_| {}));
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
    let mut o = obs();
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
    let (code, rep) = o.relay(&o.manifest(&nonce(12), |_| {}));
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
    let (code, rep) = o.relay(&o.manifest(&nonce(13), |_| {}));
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
                   "manifest": String::from_utf8(o.manifest(&nonce(n), |m| {
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
