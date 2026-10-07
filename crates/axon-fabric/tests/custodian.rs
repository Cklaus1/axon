//! Amendment 50 (operator decision D6): the custodian, driven as the binary
//! it is (`axon-custodian`, a test-trust build's `--test-config`). The nonce
//! that makes one observation authorize one launch is issued, stored and
//! spent by the custodian as its OWN uid; Fabric holds only a client
//! connection. The root-only tests run the custodian as a separate uid and
//! act as the Fabric and as another uid through `setpriv`.

mod common;
use common::*;

use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn euid() -> u32 {
    unsafe { libc::geteuid() }
}

const FABRIC: u32 = 4242;
const OTHER: u32 = 4243;
const CUSTODIAN: u32 = 4244;

/// The custodian refuses to serve from a store another uid can reach: a
/// store its group can write (the Fabric's group, say) could have records
/// planted or erased. Control: the custodian's own 0700 store serves.
#[test]
fn a_custodian_refuses_a_store_others_can_reach() {
    let d = tempfile::tempdir().unwrap();
    let store = d.path().join("custodian-nonces");
    std::fs::create_dir(&store).unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o770)).unwrap();
    let got = try_start_custodian(d.path(), CustodianRun::Test, |_| {});
    assert!(
        got.is_err(),
        "ATTACK: the custodian served from a nonce store its group can write"
    );
    assert!(got.err().unwrap().contains("0700"));
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
    let c = try_start_custodian(d.path(), CustodianRun::Test, |_| {}).expect("control");
    assert_eq!(c.issue(0).len(), 32);
}

/// The custodian runs as the uid its config names, or not at all. Control:
/// its own uid serves.
#[test]
fn a_custodian_runs_only_as_its_configured_uid() {
    let d = tempfile::tempdir().unwrap();
    let got = try_start_custodian(d.path(), CustodianRun::Test, |v| {
        v["custodian_uid"] = json!(euid().wrapping_add(1))
    });
    assert!(
        got.is_err(),
        "ATTACK: a custodian ran as a uid other than its configured custodian uid"
    );
    try_start_custodian(d.path(), CustodianRun::Test, |_| {}).expect("control");
}

/// A request over the real socket as `uid` (through setpriv), with no
/// supplementary groups; returns the reply.
fn ask_as(uid: u32, socket: &Path, req: &Value) -> Value {
    let out = Command::new("setpriv")
        .arg(format!("--reuid={uid}"))
        .arg(format!("--regid={uid}"))
        .args(["--clear-groups", "--", "python3", "-c"])
        .arg(
            "import socket,sys\n\
             s=socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])\n\
             s.sendall(sys.argv[2].encode()+b'\\n'); s.shutdown(socket.SHUT_WR)\n\
             b=b''\n\
             while True:\n    c=s.recv(4096)\n    if not c: break\n    b+=c\n\
             sys.stdout.write(b.decode())",
        )
        .arg(socket)
        .arg(req.to_string())
        .output()
        .unwrap();
    serde_json::from_slice(&out.stdout).unwrap_or_else(
        |_| json!({"transport_error": String::from_utf8_lossy(&out.stderr).to_string()}),
    )
}

/// A custodian run as CUSTODIAN in `base/cust` (its own dir and 0700 store),
/// admitting FABRIC and spending for root. The socket is made connectable by
/// every uid, so its own peer check (not the file mode) is what answers.
fn separate_custodian(base: &Path) -> TestCustodian {
    std::fs::set_permissions(base, std::fs::Permissions::from_mode(0o755)).unwrap();
    let dir = base.join("cust");
    std::fs::create_dir(&dir).unwrap();
    std::os::unix::fs::chown(&dir, Some(CUSTODIAN), Some(CUSTODIAN)).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let store = dir.join("custodian-nonces");
    std::fs::create_dir(&store).unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::chown(&store, Some(CUSTODIAN), Some(CUSTODIAN)).unwrap();
    let c = try_start_custodian(&dir, CustodianRun::TestAs(CUSTODIAN), |v| {
        v["fabric_uid"] = json!(FABRIC);
        v["launcher_uid"] = json!(0);
    })
    .expect("the custodian starts as its own uid");
    std::fs::set_permissions(&c.socket, std::fs::Permissions::from_mode(0o777)).unwrap();
    c
}

fn issue_req() -> Value {
    json!({"schema": "axon-custodian-request/1", "op": "issue", "epoch": 0})
}

/// A83, ROOT ONLY: only the Fabric uid is issued a nonce, and the Fabric
/// uid cannot spend one (a spend happens only at the root launch boundary).
/// The caller is the kernel's answer (`SO_PEERCRED`), not a claim.
/// Control: the Fabric uid is issued one; root spends it.
#[test]
fn a_uid_that_is_not_the_fabric_is_never_issued_a_nonce() {
    if euid() != 0 {
        eprintln!("skipped: needs root to run the custodian and callers as service uids");
        return;
    }
    let d = tempfile::tempdir_in("/var/tmp").unwrap();
    let c = separate_custodian(d.path());
    let r = ask_as(OTHER, &c.socket, &issue_req());
    assert!(
        r["ok"] == json!(false) && r["nonce"].is_null(),
        "ATTACK: uid {OTHER}, not the Fabric uid, was issued a nonce: {r}"
    );
    let r = ask_as(FABRIC, &c.socket, &issue_req());
    assert_eq!(
        r["ok"],
        json!(true),
        "control: the Fabric is issued one: {r}"
    );
    let nonce = r["nonce"].as_str().unwrap().to_string();
    let spend = json!({"schema": "axon-custodian-request/1", "op": "spend", "epoch": 0,
                       "nonce": nonce, "manifest_sha256": "a".repeat(64)});
    let r = ask_as(FABRIC, &c.socket, &spend);
    assert!(
        r["ok"] == json!(false),
        "ATTACK: the Fabric uid spent a nonce itself, outside the root launch boundary: {r}"
    );
    let mode = c
        .client()
        .spend(&nonce, 0, &"a".repeat(64))
        .expect("control: root (the helper) spends it");
    assert_eq!(mode.as_str(), "test");
}

/// A83, ROOT ONLY: the Fabric uid cannot write the custodian's store (plant
/// an `.issued` record, or erase a `.used` one), and the custodian refuses
/// to serve from a store the Fabric uid owns. Control: the custodian, as
/// its own uid, keeps its records there.
#[test]
fn the_fabric_cannot_write_the_custodians_store() {
    if euid() != 0 {
        eprintln!("skipped: needs root to run the custodian and callers as service uids");
        return;
    }
    let d = tempfile::tempdir_in("/var/tmp").unwrap();
    let c = separate_custodian(d.path());
    let n = c.issue_as_fabric();
    assert!(c.store.join(format!("{n}.issued")).exists(), "control");
    let planted = c.store.join(format!("{}.issued", "ab".repeat(16)));
    let wrote = Command::new("setpriv")
        .arg(format!("--reuid={FABRIC}"))
        .arg(format!("--regid={FABRIC}"))
        .args(["--clear-groups", "--", "sh", "-c"])
        .arg("printf '{\"epoch\":0,\"issued_unix\":0}' > \"$0\"")
        .arg(&planted)
        .status()
        .unwrap();
    assert!(
        !wrote.success() && !planted.exists(),
        "ATTACK: the Fabric uid wrote a nonce record into the custodian's store"
    );
    drop(c);
    // The store handed to the Fabric uid: the custodian will not serve.
    let store = d.path().join("cust/custodian-nonces");
    std::os::unix::fs::chown(&store, Some(FABRIC), Some(FABRIC)).unwrap();
    std::fs::remove_file(d.path().join("cust/custodian.sock")).unwrap();
    let got = try_start_custodian(
        &d.path().join("cust"),
        CustodianRun::TestAs(CUSTODIAN),
        |v| v["fabric_uid"] = json!(FABRIC),
    );
    assert!(
        got.is_err(),
        "ATTACK: the custodian served from a nonce store the Fabric uid owns"
    );
}

impl TestCustodianExt for TestCustodian {
    fn issue_as_fabric(&self) -> String {
        let r = ask_as(FABRIC, &self.socket, &issue_req());
        r["nonce"].as_str().expect("issued").to_string()
    }
}

trait TestCustodianExt {
    fn issue_as_fabric(&self) -> String;
}

/// Amendment 98 (eqgate5): the custodian's answer to `check` names the unix
/// time after which the nonce is no longer spendable; Fabric's client refuses an
/// answer that names none. The omission read as `i64::MAX` left the whole suite
/// green, and "never expires" is the dangerous reading (a record keyed to the
/// nonce is kept for ever). A stand-in custodian (this process's uid, which the
/// client accepts as the custodian's) answers; control: an answer naming an
/// expiry is taken as given.
#[test]
fn a_custodian_check_that_names_no_expiry_is_refused() {
    use axon_fabric::custodian::{CustodianRef, REPLY_SCHEMA};
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    let d = tempfile::tempdir().unwrap();
    let sock = d.path().join("custodian.sock");
    let l = UnixListener::bind(&sock).unwrap();
    let serve = std::thread::spawn(move || {
        for answer in [
            json!({"schema": REPLY_SCHEMA, "ok": true, "mode": "dev", "nonce": null, "error": null}),
            json!({"schema": REPLY_SCHEMA, "ok": true, "mode": "dev", "nonce": null, "error": null,
                   "expires_unix": 1_700_000_000_i64}),
        ] {
            let (mut c, _) = l.accept().unwrap();
            let mut req = Vec::new();
            c.read_to_end(&mut req).unwrap();
            c.write_all(format!("{answer}\n").as_bytes()).unwrap();
        }
    });
    let client = CustodianRef {
        socket: sock,
        uid: euid(),
        sha256: None,
    };
    let nonce = "ab".repeat(16);
    let got = client.check(&nonce, 1);
    assert!(
        got.is_err(),
        "ATTACK: a custodian answer that names no expiry was read as a nonce that never expires: {got:?}"
    );
    assert!(got.unwrap_err().contains("names no expiry"));
    assert_eq!(
        client
            .check(&nonce, 1)
            .expect("control: an answer naming an expiry")
            .1,
        1_700_000_000,
        "control: the expiry the custodian names is the one returned"
    );
    serve.join().unwrap();
}
