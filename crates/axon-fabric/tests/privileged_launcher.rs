//! A — the privileged launcher helper (`axon-protected-launcher`), driven as
//! the binary it is (a test-trust build: `--test-config`). Operator decision
//! A, amendment 45: Fabric runs non-root and reaches a root launch only
//! through this helper, which takes a fixed per-launch request and nothing
//! else. The root-only tests install a setuid-root copy and act as a
//! non-root Fabric uid through `setpriv`.

mod common;
use common::*;

use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const INPUTS: &str = "fab-0123456789abcdef.psv-inputs";

fn euid() -> u32 {
    unsafe { libc::geteuid() }
}

/// A helper fixture: operator config, pinned inputs, a stand-in launcher,
/// the Fabric's out root with one launch's PSV inputs in it, and (amendment
/// 50) a running custodian, an observer key in the operator observer root,
/// and that launch's genuine observation.
struct Fx {
    _d: tempfile::TempDir,
    base: PathBuf,
    cfg: PathBuf,
    out_root: PathBuf,
    marker: PathBuf,
    /// The setuid-root copy of the helper (root tests only).
    installed: Option<PathBuf>,
    /// The Fabric uid the inputs belong to (root tests), or this uid.
    fabric: Option<u32>,
    _cust: TestCustodian,
    observer: Issuer,
    /// The launch manifest in the job dir, and its genuine observation.
    manifest: Vec<u8>,
    observation: Vec<u8>,
}

/// How the fixture's custodian runs.
struct Custody {
    run: CustodianRun,
    /// A subdirectory of the base for the custodian (its socket and store),
    /// owned by this uid (a custodian run as another uid binds there).
    dir_owner: Option<u32>,
}

impl Custody {
    fn test() -> Custody {
        Custody {
            run: CustodianRun::Test,
            dir_owner: None,
        }
    }
}

/// The stand-in launcher: it records that it ran, what it ran as, and what
/// it was handed; `extra` runs before it returns.
fn stand_in(base: &Path, extra: &str) -> String {
    format!(
        r#"#!/bin/sh
if [ "$1" = "--verify-result" ]; then
  O="$2"
  [ -e "$(cat "$O/job-path")/completion-secret" ] && echo yes > "$O/verify-secret-present" || echo no > "$O/verify-secret-present"
  exit 0
fi
while [ $# -gt 0 ]; do case "$1" in --out) OUT="$2"; shift 2;; --psv-job) JOB="$2"; shift 2;; *) shift;; esac; done
echo ran >> "{marker}"
id -ru > "$OUT/ruid"; id -u > "$OUT/euid"
echo "$JOB" > "$OUT/job-path"
[ -e "{source}/job/completion-secret" ] && echo yes > "$OUT/source-secret-present" || echo no > "$OUT/source-secret-present"
mkdir -p "$OUT/out"
echo '{{}}' > "$OUT/result.json"
{extra}
exit 0
"#,
        marker = base.join("launched").display(),
        source = base.join("runs").join(INPUTS).display(),
    )
}

/// Build the fixture under `base_in`. `fabric` is the uid the helper admits
/// (`None`: this uid, the helper run unprivileged).
fn fx(fabric: Option<u32>, extra: &str, edit: impl FnOnce(&mut Value)) -> Fx {
    fx_with(fabric, extra, edit, Custody::test())
}

fn fx_with(
    fabric: Option<u32>,
    extra: &str,
    edit: impl FnOnce(&mut Value),
    custody: Custody,
) -> Fx {
    let d = match fabric {
        // setuid needs a filesystem without nosuid; /var/tmp is the host's.
        Some(_) => tempfile::tempdir_in("/var/tmp").unwrap(),
        None => tempfile::tempdir().unwrap(),
    };
    let base = d.path().to_path_buf();
    std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755)).unwrap();
    let inputs = helper_inputs(&base);
    let manifest = base.join("manifest.json");
    std::fs::write(
        &manifest,
        inputs.pin_manifest(&full_lx_manifest(&"cd".repeat(32))),
    )
    .unwrap();
    let launcher = base.join("launcher.sh");
    write_executable(&launcher, stand_in(&base, extra), 0o755);
    let out_root = base.join("runs");
    let cfg = write_helper_config(
        &base,
        &inputs,
        &launcher,
        &manifest,
        &out_root,
        "protected-launcher.json",
    );
    // Amendment 50: the custodian (its own dir when it runs as another uid).
    let cust_dir = match custody.dir_owner {
        None => base.clone(),
        Some(u) => {
            let d = base.join("cust");
            std::fs::create_dir(&d).unwrap();
            std::os::unix::fs::chown(&d, Some(u), Some(u)).unwrap();
            let store = d.join("custodian-nonces");
            std::fs::create_dir(&store).unwrap();
            std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::os::unix::fs::chown(&store, Some(u), Some(u)).unwrap();
            d
        }
    };
    let cust = try_start_custodian(&cust_dir, custody.run, |_| {}).expect("the custodian starts");
    let observer = Issuer::generate();
    observer.trust_in(&observer_root(&base), "obs");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
    if let Some(u) = fabric {
        v["fabric_uid"] = json!(u);
    }
    v["custodian"]["socket"] = json!(cust.socket);
    edit(&mut v);
    std::fs::write(&cfg, v.to_string()).unwrap();
    // One launch's manifest, naming a nonce the custodian issued, and its
    // genuine observation.
    let manifest = serde_json::to_vec(&test_launch_manifest("op-1", &cust.issue(0))).unwrap();
    let observation =
        serde_json::to_vec(&observation_of(&manifest, &observer.key_id(), 0)).unwrap();
    let f = Fx {
        marker: base.join("launched"),
        _d: d,
        base: base.clone(),
        cfg,
        out_root: out_root.clone(),
        installed: None,
        fabric,
        _cust: cust,
        observer,
        manifest,
        observation,
    };
    f.put_inputs();
    let installed = fabric.map(|u| {
        let h = base.join("axon-protected-launcher");
        copy_executable(helper_pin().path, &h, 0o755);
        std::os::unix::fs::chown(&h, Some(0), Some(u)).unwrap();
        std::fs::set_permissions(&h, std::fs::Permissions::from_mode(0o4750)).unwrap();
        h
    });
    Fx { installed, ..f }
}

fn walk(p: &Path) -> Vec<PathBuf> {
    let mut v = vec![p.to_path_buf()];
    if p.is_dir() && !p.is_symlink() {
        for e in std::fs::read_dir(p).unwrap() {
            v.extend(walk(&e.unwrap().path()));
        }
    }
    v
}

impl Fx {
    /// The Fabric's inputs for one launch: candidate, suite, job (the
    /// manifest and the secret). Again after a launch consumed the job: the
    /// Fabric uid can always rebuild them.
    fn put_inputs(&self) {
        let i = self.out_root.join(INPUTS);
        for (sub, name, body) in [
            ("candidate", "f.ax", b"fn f() -> i64 { 1 }\n".as_slice()),
            ("check", "accept.ax", b"// suite\n".as_slice()),
            ("job", "launch-manifest.json", self.manifest.as_slice()),
        ] {
            std::fs::create_dir_all(i.join(sub)).unwrap();
            std::fs::write(i.join(sub).join(name), body).unwrap();
        }
        let secret = i.join("job/completion-secret");
        let _ = std::fs::remove_file(&secret);
        std::fs::write(&secret, [7u8; 32]).unwrap();
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o400)).unwrap();
        if let Some(u) = self.fabric {
            for p in walk(&self.out_root) {
                std::os::unix::fs::lchown(&p, Some(u), Some(u)).unwrap();
            }
        }
    }
    /// The request for this launch, carrying its genuine observation.
    fn request(&self, out: &str) -> Value {
        let i = self.out_root.join(INPUTS);
        json!({
            "schema": "axon-protected-launch-request/2",
            "id": "fab-0123456789abcdef",
            "out": self.out_root.join(out),
            "psv_candidate": i.join("candidate"),
            "psv_suite": i.join("check"),
            "psv_job": i.join("job"),
            "psv_manifest_sha256": sha256_file(&i.join("job/launch-manifest.json")),
            "policy_json": "{\"schema\":\"axon-vm-mmds/1\",\"allowed_effects\":[]}",
            "timeout_s": 60,
            "observation": String::from_utf8(self.observation.clone()).unwrap(),
            "observation_signature": self.observer.sign_for(
                axon_fabric::backend::TrustAuthority::Observer,
                &self.observation,
            ),
        })
    }
    /// A nonce the fixture's custodian issues for `epoch`.
    fn cust_issue(&self, epoch: u64) -> String {
        self._cust.issue(epoch)
    }
    /// How many times the stand-in launcher ran.
    fn launches(&self) -> usize {
        std::fs::read_to_string(&self.marker)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }
    /// Run the helper with `request`, as `as_uid` (`None`: this process;
    /// `Some((uid, groups))`: through setpriv, the setuid copy).
    fn run(&self, request: &Value, as_uid: Option<(u32, &[u32])>) -> (Option<i32>, Value) {
        let mut c = match as_uid {
            None => Command::new(helper_pin().path),
            Some((u, groups)) => {
                let mut c = Command::new("setpriv");
                c.arg(format!("--reuid={u}")).arg(format!("--regid={u}"));
                if groups.is_empty() {
                    c.arg("--clear-groups");
                } else {
                    let g: Vec<String> = groups.iter().map(u32::to_string).collect();
                    c.arg(format!("--groups={}", g.join(",")));
                }
                // A shell running AS the actor makes the exec: setpriv's own
                // exec still holds root's DAC override (measured).
                c.args(["--", "sh", "-c", "exec \"$0\" \"$@\""])
                    .arg(self.installed.as_ref().expect("root fixture"));
                c
            }
        };
        let mut child = c
            .arg("--test-config")
            .arg(&self.cfg)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let report = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        (out.status.code(), report)
    }
    fn launched(&self) -> bool {
        self.marker.exists()
    }
}

#[test]
fn the_helper_launches_a_well_formed_request_and_hands_the_out_dir_over() {
    let f = fx(None, "", |_| {});
    let (code, r) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "{r}");
    assert_eq!(r["launched"], true, "{r}");
    assert_eq!(r["launcher_exit"], 0, "{r}");
    assert_eq!(r["verify_exit"], 0, "{r}");
    assert_eq!(r["unchanged"], true, "{r}");
    assert_eq!(r["build"], "test-trust");
    assert!(f.launched());
    let out = f.out_root.join("op-1");
    assert!(out.join("result.json").is_file());
    // The job's source (the secret) was consumed before the launcher ran.
    assert!(!f.out_root.join(INPUTS).join("job").exists());
}

/// A: every path in a request is a plainly spelled child of the operator's
/// out root; anything else launches nothing.
#[test]
fn a_request_naming_a_path_outside_the_operator_roots_launches_nothing() {
    let f = fx(None, "", |_| {});
    let elsewhere = f.base.join("elsewhere");
    for (what, edit) in [
        ("out", json!({"out": elsewhere})),
        (
            "out traversal",
            json!({"out": f.out_root.join("../elsewhere")}),
        ),
        ("candidate", json!({"psv_candidate": f.base.join("dist")})),
    ] {
        let mut r = f.request("op-1");
        for (k, v) in edit.as_object().unwrap() {
            r[k] = v.clone();
        }
        let (code, rep) = f.run(&r, None);
        assert!(
            code == Some(30) && !f.launched(),
            "ATTACK: a request naming a path outside the operator roots ({what}) launched: \
             {code:?} {rep}"
        );
    }
}

#[test]
fn a_request_with_an_unknown_field_launches_nothing() {
    let f = fx(None, "", |_| {});
    let mut r = f.request("op-1");
    r["launcher"] = json!("/tmp/evil.sh");
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: a request with an unknown field launched: {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap().contains("unknown field"),
        "{rep}"
    );
}

/// A: the out root is the Fabric uid's PRIVATE dir. One others can write
/// would let another uid plant or swap what the root launcher writes.
#[test]
fn an_out_root_others_can_reach_launches_nothing() {
    let f = fx(None, "", |_| {});
    std::fs::set_permissions(&f.out_root, std::fs::Permissions::from_mode(0o770)).unwrap();
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: an out root group-accessible (0770) was launched into: {code:?} {rep}"
    );
    assert!(rep["error"].as_str().unwrap().contains("0700"), "{rep}");
}

/// A: the helper copies no more than the operator allows.
#[test]
fn inputs_over_the_operator_budget_launch_nothing() {
    let f = fx(None, "", |v| v["max_input_bytes"] = json!(8));
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: inputs over the operator's max_input_bytes were launched: {code:?} {rep}"
    );
}

/// A: the root launcher's out dir is handed to Fabric only as directories,
/// regular files and symlinks; a FIFO (or device node, or socket) a launch
/// left there (a hostile guest image's `rdump`) is removed, never handed over.
#[test]
fn a_special_file_the_launch_leaves_is_never_handed_to_fabric() {
    let f = fx(
        None,
        "mkfifo \"$OUT/fifo\"\nln -s result.json \"$OUT/link\"",
        |_| {},
    );
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "{rep}");
    let out = f.out_root.join("op-1");
    assert!(
        std::fs::symlink_metadata(out.join("fifo")).is_err(),
        "ATTACK: a FIFO the launch left in the out dir was handed to Fabric"
    );
    assert!(std::fs::symlink_metadata(out.join("link"))
        .unwrap()
        .file_type()
        .is_symlink());
}

/// A: the per-attempt secret exists ONCE while the launcher runs: the helper
/// consumes Fabric's job files when it snapshots them.
#[test]
fn the_secret_leaves_fabrics_job_dir_before_the_launcher_runs() {
    let f = fx(None, "", |_| {});
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "{rep}");
    let seen = std::fs::read_to_string(f.out_root.join("op-1/source-secret-present")).unwrap();
    assert_eq!(
        seen.trim(),
        "no",
        "ATTACK: the per-attempt secret was still in Fabric's job dir while the root launcher ran"
    );
}

/// A: the snapshot's copy of the secret is removed as soon as the launcher
/// returns, before `--verify-result` (or anything else) runs.
#[test]
fn the_snapshot_secret_is_gone_before_the_verify_step() {
    let f = fx(None, "", |_| {});
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "{rep}");
    let seen = std::fs::read_to_string(f.out_root.join("op-1/verify-secret-present")).unwrap();
    assert_eq!(
        seen.trim(),
        "no",
        "ATTACK: the snapshot's copy of the secret was still on disk when --verify-result ran"
    );
}

const FABRIC: u32 = 4242;
const OTHER: u32 = 4243;

/// A, ROOT ONLY: a non-root Fabric reaches a root launch ONLY through the
/// setuid helper. As the Fabric uid, the launcher run directly is not root
/// (the jailer refuses); the helper runs it as root in every id, and hands
/// the out dir back. A uid that is not the configured Fabric uid, even one
/// the file mode lets execute it, is refused and launches nothing.
#[test]
fn a_non_root_fabric_reaches_a_root_launch_only_through_the_helper() {
    if euid() != 0 {
        eprintln!("skipped: needs root to install a setuid helper and act as service uids");
        return;
    }
    let f = fx(Some(FABRIC), "", |_| {});
    // Directly, as the Fabric uid: no root.
    let direct = Command::new("setpriv")
        .args([
            &format!("--reuid={FABRIC}"),
            &format!("--regid={FABRIC}"),
            "--clear-groups",
            "--",
            "sh",
            "-c",
            "id -u",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&direct.stdout).trim(),
        FABRIC.to_string()
    );
    // A uid in the Fabric GROUP (so the mode lets it execute the helper) but
    // not the Fabric uid: refused, nothing launched.
    let (code, rep) = f.run(&f.request("op-other"), Some((OTHER, &[FABRIC])));
    assert!(
        code == Some(30) && !f.launched() && !f.out_root.join("op-other").exists(),
        "ATTACK: uid {OTHER}, not the configured Fabric uid, made the helper launch as root: \
         {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap().contains("caller uid"),
        "{rep}"
    );
    // Outside the group, the kernel refuses the exec itself (mode 04750):
    // the shell reports 126 and the helper never ran (no report at all).
    let (code, rep) = f.run(&f.request("op-none"), Some((OTHER, &[])));
    assert_eq!(code, Some(126), "{rep}");
    assert!(rep.is_null() && !f.launched());
    // The Fabric uid, through the helper: a root launch.
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    let out = f.out_root.join("op-1");
    let ruid = std::fs::read_to_string(out.join("ruid")).unwrap_or_default();
    assert_eq!(
        ruid.trim(),
        "0",
        "ATTACK: the launcher ran with the caller's real uid (ruid {:?}, not 0): the non-root \
         caller could signal or trace the root launch, and bash drops a setuid euid to it",
        ruid.trim()
    );
    assert_eq!(code, Some(0), "{rep}");
    let e = std::fs::read_to_string(out.join("euid")).unwrap();
    assert_eq!(e.trim(), "0", "the launcher ran as root");
    use std::os::unix::fs::MetadataExt;
    for p in walk(&out) {
        assert_eq!(
            std::fs::symlink_metadata(&p).unwrap().uid(),
            FABRIC,
            "{} not handed to the Fabric uid",
            p.display()
        );
    }
}

/// A, ROOT ONLY: the root helper reads only what the Fabric uid owns. A
/// root-owned file among the inputs (a hard link to a root file, planted by
/// an older tree) is refused, never copied into a guest image.
#[test]
fn a_root_owned_file_among_the_inputs_is_never_read_by_the_helper() {
    if euid() != 0 {
        eprintln!("skipped: needs root to install a setuid helper and act as service uids");
        return;
    }
    let f = fx(Some(FABRIC), "", |_| {});
    let planted = f.out_root.join(INPUTS).join("candidate/shadow");
    std::fs::write(&planted, "root's secret\n").unwrap();
    std::os::unix::fs::chown(&planted, Some(0), Some(0)).unwrap();
    std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: a root-owned file among the Fabric's inputs was read by the root helper and \
         launched: {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap()
            .contains("not the Fabric uid"),
        "{rep}"
    );
}

// ── C9 round 3, HARNESS workstream (EQUIVALENCE; rows M585-M609). ──────────
// Each guard below had no row, and the whole suite stayed green with it
// removed. Each test attacks the route where that guard is the ONLY one.

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

impl Fx {
    /// Move the out root (with the launch's inputs) to `rel` under the base
    /// and point the config at it.
    fn move_out_root(&mut self, rel: &str) {
        let to = self.base.join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::rename(&self.out_root, &to).unwrap();
        self.out_root = to;
        self.edit_config(|v| v["out_root"] = json!(self.out_root));
    }
    fn edit_config(&self, edit: impl FnOnce(&mut Value)) {
        let mut v: Value =
            serde_json::from_str(&std::fs::read_to_string(&self.cfg).unwrap()).unwrap();
        edit(&mut v);
        std::fs::write(&self.cfg, v.to_string()).unwrap();
    }
    fn staging_root(&self) -> PathBuf {
        self.base.join("helper-staging")
    }
    /// Install `bin` (another build of the helper) setuid-root in place of
    /// the test-trust copy.
    fn install(&mut self, bin: &Path, group: u32) {
        let h = self.base.join("axon-protected-launcher.installed");
        copy_executable(bin, &h, 0o755);
        std::os::unix::fs::chown(&h, Some(0), Some(group)).unwrap();
        set_mode(&h, 0o4750);
        self.installed = Some(h);
    }
}

/// A (M585): the helper's config is the operator's. A config file another
/// uid can write (group or other) is refused: whoever writes it chooses the
/// launcher, the pins and the roots the root helper obeys.
#[test]
fn a_helper_config_other_uids_can_write_is_never_obeyed() {
    for mode in [0o646, 0o664] {
        let f = fx(None, "", |_| {});
        set_mode(&f.cfg, mode);
        let (code, rep) = f.run(&f.request("op-1"), None);
        assert!(
            code == Some(30) && !f.launched(),
            "ATTACK: a helper config of mode {mode:o} (writable by another uid) was obeyed and \
             launched: {code:?} {rep}"
        );
    }
}

/// A (M586), ROOT ONLY: a config file owned by another uid than the
/// operator is refused, whatever its mode: its owner can rewrite it.
#[test]
fn a_helper_config_owned_by_another_uid_is_never_obeyed() {
    if skip_unless_root() {
        return;
    }
    let f = fx(None, "", |_| {});
    chown(&f.cfg, OTHER);
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: a helper config owned by uid {OTHER} (not the operator) was obeyed and \
         launched: {code:?} {rep}"
    );
}

/// A (M587, M589): every directory ABOVE the out root is an operator
/// directory. One another uid can write lets that uid rename the out root
/// away and put its own in its place between the helper's checks.
#[test]
fn an_out_root_below_a_directory_others_can_write_launches_nothing() {
    let mut f = fx(None, "", |_| {});
    f.move_out_root("shared/runs");
    set_mode(&f.base.join("shared"), 0o777);
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: the helper launched into an out root below a directory another uid can \
         write (mode 777): {code:?} {rep}"
    );
    // Control: the same tree, the directory the operator's.
    set_mode(&f.base.join("shared"), 0o755);
    let (code, rep) = f.run(&f.request("op-2"), None);
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A (M588), ROOT ONLY: the same, for a directory above the out root that
/// another uid OWNS (mode 0755): its owner can rename what is in it.
#[test]
fn an_out_root_below_a_directory_another_uid_owns_launches_nothing() {
    if skip_unless_root() {
        return;
    }
    let mut f = fx(None, "", |_| {});
    f.move_out_root("theirs/runs");
    chown(&f.base.join("theirs"), OTHER);
    set_mode(&f.base.join("theirs"), 0o755);
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: the helper launched into an out root below a directory uid {OTHER} owns: \
         {code:?} {rep}"
    );
}

/// A (M590): the helper re-verifies what the launch will boot, on its own
/// descriptors: the kernel, rootfs, firecracker and jailer at the profile
/// manifest's pins, each operator-owned. Bytes other than the pin launch
/// nothing; and (root only) an input with the PINNED bytes that another uid
/// owns launches nothing either: the launcher's own sha256 check would pass,
/// and the owner could rewrite it between that check and the boot.
#[test]
fn a_boot_input_the_helper_cannot_vouch_for_launches_nothing() {
    if euid() == 0 {
        let f = fx(None, "", |_| {});
        chown(&f.base.join("engine/firecracker"), OTHER);
        let (code, rep) = f.run(&f.request("op-1"), None);
        assert!(
            code == Some(30) && !f.launched(),
            "ATTACK: the helper launched with a firecracker binary owned by uid {OTHER}, who can \
             rewrite it after it is verified: {code:?} {rep}"
        );
    }
    let f = fx(None, "", |_| {});
    std::fs::write(f.base.join("dist/vmlinux"), "another kernel\n").unwrap();
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: the helper launched with a kernel image that is not the one the profile \
         manifest pins: {code:?} {rep}"
    );
}

/// A (M596/M597, retired as a PAIR under the four-cell rule), ROOT ONLY:
/// the snapshot holds the per-attempt secret, and no other uid may read it
/// while the launcher runs. Two guards keep it private: the staging root must
/// be 0700, and each launch's staging dir is created 0700. The launcher here
/// tries to read the secret as another uid.
#[test]
fn the_staged_secret_is_never_readable_by_another_uid() {
    if skip_unless_root() {
        return;
    }
    let f = fx(
        None,
        &format!(
            "setpriv --reuid={OTHER} --regid={OTHER} --clear-groups cat \"$JOB/completion-secret\" \
             >/dev/null 2>&1 && echo yes > \"$OUT/other-read\" || echo no > \"$OUT/other-read\""
        ),
        |_| {},
    );
    // A staging root others can list and traverse (never write).
    set_mode(&f.staging_root(), 0o755);
    let (code, rep) = f.run(&f.request("op-1"), None);
    let seen = std::fs::read_to_string(f.out_root.join("op-1/other-read")).unwrap_or_default();
    assert_ne!(
        seen.trim(),
        "yes",
        "ATTACK: uid {OTHER} read the per-attempt secret out of the helper's snapshot while \
         the launcher ran: {code:?} {rep}"
    );
}

/// A (M598), ROOT ONLY: the staging root is the operator's. One the Fabric
/// uid owns lets the Fabric rename the helper's (root-owned) snapshot away
/// after it was verified and copied, and put its own in its place: the
/// launcher then boots the Fabric's bytes. The launcher here does exactly
/// that swap, as the Fabric uid, before it reads the candidate.
#[test]
fn a_staging_root_the_fabric_owns_launches_nothing() {
    if skip_unless_root() {
        return;
    }
    let f = fx(
        Some(FABRIC),
        &format!(
            "S=\"$(dirname \"$JOB\")\"\n\
             setpriv --reuid={FABRIC} --regid={FABRIC} --clear-groups sh -c \
             'mv \"$1\" \"$1.moved\" && mkdir -p \"$1/candidate\" && echo SWAPPED > \"$1/candidate/f.ax\"' \
             sh \"$S\" 2>/dev/null\n\
             cat \"$S/candidate/f.ax\" > \"$OUT/candidate-read\" 2>/dev/null"
        ),
        |_| {},
    );
    chown(&f.staging_root(), FABRIC);
    set_mode(&f.staging_root(), 0o700);
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    let read = std::fs::read_to_string(f.out_root.join("op-1/candidate-read")).unwrap_or_default();
    assert!(
        !read.contains("SWAPPED") && code == Some(30) && !f.launched(),
        "ATTACK: the Fabric swapped the root helper's snapshot under a staging root it owns, \
         and the launcher read the Fabric's bytes ({:?}): {code:?} {rep}",
        read.trim()
    );
}

/// A (M599), ROOT ONLY: the helper takes a launch's inputs only from an
/// inputs dir the Fabric uid owns. A root-owned dir in the out root (a
/// helper-made out dir whose hand-over failed) named as the inputs dir must
/// not be used: the root helper would take the inputs from it and then
/// delete the job dir out of a directory the Fabric may not write.
#[test]
fn a_root_owned_inputs_dir_is_never_used_by_the_helper() {
    if skip_unless_root() {
        return;
    }
    let f = fx(Some(FABRIC), "", |_| {});
    let inputs = f.out_root.join(INPUTS);
    chown(&inputs, 0);
    set_mode(&inputs, 0o755);
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert!(
        code == Some(30) && !f.launched() && inputs.join("job").exists(),
        "ATTACK: the root helper took a launch's inputs from, and deleted the job dir out of, \
         a directory the Fabric uid does not own: {code:?} {rep}"
    );
}

/// A (M600): the jail id names the helper's staging dir. An id with a path
/// in it (`../x`) would stage the inputs, with the secret, OUTSIDE the
/// root-private staging root.
#[test]
fn a_jail_id_holding_a_path_never_stages_outside_the_staging_root() {
    let f = fx(None, "", |_| {});
    let mut r = f.request("op-1");
    r["id"] = json!("../escaped");
    let (code, rep) = f.run(&r, None);
    let staged = std::fs::read_to_string(f.out_root.join("op-1/job-path")).unwrap_or_default();
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: a jail id holding a path staged the launch's inputs at {:?}, outside the \
         staging root: {code:?} {rep}",
        staged.trim()
    );
}

/// PSV-4 (M603): a test-trust helper obeys `--test-config`, a config of its
/// caller's choosing, so it must say so: its report (and probe) names the
/// build `test-trust`, which a production Fabric never attests protected.
#[test]
fn a_test_trust_helper_never_reports_itself_as_a_production_build() {
    let out = Command::new(helper_pin().path)
        .arg("--probe")
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v["build"], "test-trust",
        "ATTACK: a test-trust helper (which obeys --test-config) reported itself as a \
         production build: {v}"
    );
}

// ── Production builds (no test-trust-root). Every `cargo test` build of this
// crate is a test-trust build (the dev-dependency on itself turns the
// feature on), so what a PRODUCTION helper or Fabric does is only observable
// from a separate build: this builds one, once per test process, into its
// own target dir beside this one's.

fn production_build() -> &'static Path {
    use std::sync::OnceLock;
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let exe = PathBuf::from(env!("CARGO_BIN_EXE_axon-protected-launcher"));
        let target = exe
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("production-build");
        let ws = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let out = Command::new(cargo)
            .current_dir(&ws)
            .args([
                "build",
                "--offline",
                "-j",
                "4",
                "-p",
                "axon-fabric",
                "--bin",
                "axon-protected-launcher",
                "--bin",
                "axon-fabric",
                "--target-dir",
            ])
            .arg(&target)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "setup: the production build failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let bin = target.join("debug");
        let probe = Command::new(bin.join("axon-protected-launcher"))
            .arg("--probe")
            .output()
            .unwrap();
        let v: Value = serde_json::from_slice(&probe.stdout).unwrap();
        assert_eq!(
            v["build"], "production",
            "setup: not a production build: {v}"
        );
        bin
    })
}

/// A (M601), ROOT ONLY, PRODUCTION BUILD: the installed setuid helper reads
/// its config only from the operator's fixed path. `--test-config` exists in
/// test-trust builds alone. Here a root-owned config the operator did not
/// install (a stale test config: root-owned, in a root-owned dir, naming a
/// launcher of its own) is offered by the Fabric uid through the flag.
#[test]
fn a_production_helper_never_takes_its_config_from_a_path_its_caller_names() {
    if skip_unless_root() {
        return;
    }
    let bin = production_build().join("axon-protected-launcher");
    let mut f = fx(Some(FABRIC), "", |_| {});
    f.install(&bin, FABRIC);
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert!(
        code == Some(2) && !f.launched(),
        "ATTACK: a production helper took its config from a path its caller named \
         (--test-config) and launched as root: {code:?} {rep}"
    );
}

/// PSV-4 (M605), PRODUCTION BUILD: the decision psv_receipt makes
/// (`backend::attests_protected`), read from a production Fabric's verifier
/// manifest. A launch by a test-trust helper never attests protected there,
/// nor one Fabric ran itself.
#[test]
fn a_production_fabric_never_lets_a_test_trust_helper_attest_protected() {
    let bin = production_build().join("axon-fabric");
    let out = Command::new(bin).arg("verifier-manifest").output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("setup: verifier-manifest: {e}: {out:?}"));
    assert_eq!(v["build"], "production", "setup: {v}");
    let r = &v["launch_routes_attesting_protected"];
    assert_eq!(r["privileged_production_helper"], true, "control: {r}");
    assert_eq!(
        r["privileged_test_trust_helper"], false,
        "ATTACK: a production Fabric lets a launch by a test-trust helper (which obeys a \
         caller-chosen --test-config) attest a protected verdict: {r}"
    );
    assert_eq!(
        r["direct"], false,
        "ATTACK: a production Fabric lets a launch it ran itself attest a protected verdict: {r}"
    );
}

/// A (M602), ROOT ONLY, PRODUCTION BUILD: the helper launches only when it
/// is root in every id (installed setuid-root). Installed any other way (on
/// a nosuid mount, or given file capabilities instead) it would run the
/// launch as its CALLER's uid, which can then signal or trace it. Here the
/// helper runs as the Fabric uid holding the capabilities it would need
/// (lease, DAC override, chown, fowner), so nothing but the euid check
/// stands in the way. The operator tree sits at the helper's FIXED config
/// path, /etc/axon, on a tmpfs in a PRIVATE mount namespace: the host's /etc
/// is never written.
#[test]
fn a_production_helper_that_is_not_root_launches_nothing() {
    if skip_unless_root() {
        return;
    }
    if !Path::new("/etc/axon").is_dir() {
        eprintln!("skipped: no /etc/axon mount point (this test never creates one)");
        return;
    }
    let bin = production_build().join("axon-protected-launcher");
    let d = tempfile::tempdir_in("/var/tmp").unwrap();
    let s = d.path();
    set_mode(s, 0o755);
    let e = Path::new("/etc/axon");
    let inputs = helper_inputs(s);
    std::fs::write(
        s.join("manifest.json"),
        inputs.pin_manifest(&full_lx_manifest(&"cd".repeat(32))),
    )
    .unwrap();
    write_executable(
        &s.join("launcher.sh"),
        "#!/bin/sh\n[ \"$1\" = \"--verify-result\" ] && exit 0\n\
         while [ $# -gt 0 ]; do case \"$1\" in --out) OUT=\"$2\"; shift 2;; *) shift;; esac; done\n\
         id -ru > \"$OUT/ruid\"\nexit 0\n",
        0o755,
    );
    copy_executable(&bin, s.join("axon-protected-launcher"), 0o755);
    let i = s.join("runs").join(INPUTS);
    for (sub, name, body) in [
        ("candidate", "f.ax", "fn f() -> i64 { 1 }\n"),
        ("check", "accept.ax", "// suite\n"),
        ("job", "launch-manifest.json", "{}"),
    ] {
        std::fs::create_dir_all(i.join(sub)).unwrap();
        std::fs::write(i.join(sub).join(name), body).unwrap();
    }
    std::fs::create_dir_all(s.join("staging")).unwrap();
    let pin = |p: &str| json!({"path": e.join(p), "sha256": sha256_file(&s.join(p))});
    let bash = bash_pin();
    std::fs::write(
        s.join("protected-launcher.json"),
        json!({
            "schema": "axon-protected-launcher/1",
            "fabric_uid": FABRIC,
            "interpreter": {"path": bash.path, "sha256": bash.sha256},
            "launcher": pin("launcher.sh"),
            "profile_manifest": pin("manifest.json"),
            "artifacts_dir": e.join("dist"),
            "firecracker": e.join("engine/firecracker"),
            "jailer": e.join("engine/jailer"),
            "out_root": e.join("runs"),
            "staging_root": e.join("staging"),
            "max_timeout_s": 3600,
            "max_input_bytes": 1u64 << 30,
        })
        .to_string(),
    )
    .unwrap();
    let ei = e.join("runs").join(INPUTS);
    std::fs::write(
        s.join("request.json"),
        json!({
            "schema": "axon-protected-launch-request/1",
            "id": "fab-0123456789abcdef",
            "out": e.join("runs/op-1"),
            "psv_candidate": ei.join("candidate"),
            "psv_suite": ei.join("check"),
            "psv_job": ei.join("job"),
            "psv_manifest_sha256": sha256_file(&i.join("job/launch-manifest.json")),
            "policy_json": "{\"schema\":\"axon-vm-mmds/1\",\"allowed_effects\":[]}",
            "timeout_s": 60,
        })
        .to_string(),
    )
    .unwrap();
    let caps = "+lease,+dac_override,+chown,+fowner";
    let script = format!(
        "set -e\n\
         mount -t tmpfs -o mode=0755 tmpfs /etc/axon\n\
         cp -a \"$1/.\" /etc/axon/\n\
         chown -R 0:0 /etc/axon\n\
         chmod 0755 /etc/axon\n\
         chown -R {FABRIC}:{FABRIC} /etc/axon/runs\n\
         chmod 0700 /etc/axon/runs /etc/axon/staging\n\
         set +e\n\
         setpriv --reuid={FABRIC} --regid={FABRIC} --clear-groups --inh-caps={caps} \
         --ambient-caps={caps} -- /etc/axon/axon-protected-launcher \
         < /etc/axon/request.json > \"$1/report.json\"\n\
         echo $? > \"$1/code\"\n\
         cp -a /etc/axon/runs/op-1 \"$1/result\" 2>/dev/null\n\
         exit 0\n"
    );
    let st = Command::new("unshare")
        .args(["-m", "--propagation", "private", "sh", "-c", &script, "sh"])
        .arg(s)
        .status()
        .unwrap();
    assert!(st.success(), "setup: the namespace script failed");
    let code = std::fs::read_to_string(s.join("code")).unwrap_or_default();
    let rep = std::fs::read_to_string(s.join("report.json")).unwrap_or_default();
    let ruid = std::fs::read_to_string(s.join("result/ruid")).ok();
    assert!(
        ruid.is_none() && code.trim() == "30",
        "ATTACK: a production helper that is not root in every id launched (the launcher ran \
         with ruid {ruid:?}, which the caller can signal or trace): exit {} {rep}",
        code.trim()
    );
    assert!(rep.contains("not 0"), "the refusal names the euid: {rep}");
}

// ── Amendment 50: the observation and its nonce, at the ROOT boundary ───────
//
// Driven against the helper alone: no Fabric check runs before it, so each
// refusal here is the helper's (or its custodian's) and nothing else's.

/// A84: ONE observation, ONE root launch. The Fabric uid rebuilds the same
/// inputs and sends the same request (the same manifest, the same genuine
/// observation) for a second out dir; the custodian has spent the nonce, so
/// the helper launches nothing. Control: the first launch.
#[test]
fn one_observation_launches_the_root_launcher_once() {
    let f = fx(None, "", |_| {});
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: the observed launch runs: {rep}");
    assert_eq!(f.launches(), 1);
    f.put_inputs();
    let (code, rep) = f.run(&f.request("op-2"), None);
    assert!(
        code == Some(30) && f.launches() == 1 && !f.out_root.join("op-2").exists(),
        "ATTACK: one observation launched the root launcher twice: {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap().contains("already used"),
        "{rep}"
    );
}

/// A84: no root launch without an observation. The request carries none (the
/// Fabric uid skipped the observer), or one no trusted observer signed (the
/// Fabric uid minted it with a key of its own). The manifest's nonce is a
/// genuine, unspent one, so the observation is the only thing missing.
/// Control: the same fixture's genuine observation launches.
#[test]
fn a_root_launch_without_an_observation_launches_nothing() {
    let f = fx(None, "", |_| {});
    let mut r = f.request("op-none");
    r["observation"] = json!("");
    r["observation_signature"] = json!("");
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper launched with no observation: {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap()
            .contains("no verified observation"),
        "{rep}"
    );
    // Minted: the right bytes, signed by a key the observer root lacks. (A
    // refused request has consumed the job, as every request does: the
    // Fabric uid rebuilds it.)
    f.put_inputs();
    let minted = Issuer::generate();
    let mut r = f.request("op-minted");
    r["observation_signature"] = json!(minted.sign_for(
        axon_fabric::backend::TrustAuthority::Observer,
        &f.observation
    ));
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper launched on an observation no trusted observer signed: \
         {code:?} {rep}"
    );
    f.put_inputs();
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: the genuine observation: {rep}");
    assert_eq!(f.launches(), 1);
}

/// A84: the observation must be of the manifest the helper launches. The
/// other manifest carries the SAME nonce and the same host facts, so only the
/// manifest joins refuse:
/// * (O) the request names the snapshot's manifest, but the observation is of
///   another one (its `intended_launch_manifest_sha256`);
/// * (S) the request and the observation name the other manifest, but the
///   snapshot the root launcher would boot is this one.
///
/// Control: this manifest with its own observation launches.
#[test]
fn an_observation_of_another_manifest_launches_nothing() {
    let f = fx(None, "", |_| {});
    let m: Value = serde_json::from_slice(&f.manifest).unwrap();
    let mut other = m.clone();
    other["operation_id"] = json!("op-other");
    other["candidate"]["tree_digest"] = json!("f".repeat(64));
    let other = serde_json::to_vec(&other).unwrap();
    let other_obs = serde_json::to_vec(&observation_of(&other, &f.observer.key_id(), 0)).unwrap();
    let signed = |o: &[u8]| {
        f.observer
            .sign_for(axon_fabric::backend::TrustAuthority::Observer, o)
    };
    // (O)
    let mut r = f.request("op-o");
    r["observation"] = json!(String::from_utf8(other_obs.clone()).unwrap());
    r["observation_signature"] = json!(signed(&other_obs));
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: an observation of another manifest launched the root launcher: {code:?} {rep}"
    );
    // (S)
    f.put_inputs();
    let mut r = f.request("op-s");
    r["psv_manifest_sha256"] = json!(axon_psv::sha256_hex(&other));
    r["observation"] = json!(String::from_utf8(other_obs.clone()).unwrap());
    r["observation_signature"] = json!(signed(&other_obs));
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper launched a snapshot manifest other than the one the request \
         and its observation name: {code:?} {rep}"
    );
    f.put_inputs();
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: {rep}");
}

/// D6: a DEV custodian (a manual `axon-custodian --dev`) never yields a
/// protected launch: the helper refuses the spend it answers, even though
/// the nonce, the observation and everything else are genuine. Control: the
/// same fixture with a test custodian launches
/// (`one_observation_launches_the_root_launcher_once`).
#[test]
fn a_dev_custodian_never_yields_a_protected_launch() {
    let f = fx_with(
        None,
        "",
        |_| {},
        Custody {
            run: CustodianRun::Dev,
            dir_owner: None,
        },
    );
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: a launch whose nonce a DEV custodian spent ran as a protected launch: \
         {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap().contains("dev custodian"),
        "{rep}"
    );
}

const CUSTODIAN: u32 = 4244;

/// A83, ROOT ONLY: the helper authenticates the custodian by the kernel. The
/// Fabric uid runs a custodian of its own at the configured socket (a full
/// one: it issued this launch's nonce and would spend it). The helper's
/// config names the custodian uid, so the helper refuses a socket the Fabric
/// uid serves. Control: the same launch, with the helper configured to trust
/// that uid, runs, so the peer check was the only refusal.
#[test]
fn a_custodian_socket_the_fabric_serves_is_refused_by_the_helper() {
    if euid() != 0 {
        eprintln!("skipped: needs root to install a setuid helper and act as service uids");
        return;
    }
    let f = fx_with(
        Some(FABRIC),
        "",
        |v| v["custodian"]["uid"] = json!(CUSTODIAN),
        Custody {
            // The Fabric's own "custodian": issues to this (root) test
            // process, spends for the root helper.
            run: CustodianRun::TestAs(FABRIC),
            dir_owner: Some(FABRIC),
        },
    );
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper spent the nonce through a custodian the Fabric uid serves: \
         {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap().contains("served by uid"),
        "{rep}"
    );
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&f.cfg).unwrap()).unwrap();
    v["custodian"]["uid"] = json!(FABRIC);
    std::fs::write(&f.cfg, v.to_string()).unwrap();
    f.put_inputs();
    let (code, rep) = f.run(&f.request("op-2"), Some((FABRIC, &[])));
    assert_eq!(
        code,
        Some(0),
        "control: the peer check was the refusal: {rep}"
    );
}

/// ADR-002 at the ROOT boundary: Fabric holds the host signer's private key
/// and can sign any domain. If that key is also in the observer root, Fabric
/// can mint an observation that verifies, so the helper refuses the root
/// (it knows the host signer from its operator config). Control: the same
/// key, when it is NOT the host signer, is a legitimate observer.
#[test]
fn an_observation_signed_with_the_host_signer_launches_nothing() {
    let signer = Issuer::generate();
    let f = fx(None, "", |v| {
        v["observer"]["host_signer_public_key"] = json!(signer.public_hex())
    });
    signer.trust_in(&observer_root(&f.base), "signer");
    let minted = serde_json::to_vec(&observation_of(&f.manifest, &signer.key_id(), 0)).unwrap();
    let mut r = f.request("op-minted");
    r["observation"] = json!(String::from_utf8(minted.clone()).unwrap());
    r["observation_signature"] =
        json!(signer.sign_for(axon_fabric::backend::TrustAuthority::Observer, &minted));
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper launched on an observation signed with the host signer's key: \
         {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap().contains("host signer"),
        "{rep}"
    );
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&f.cfg).unwrap()).unwrap();
    v["observer"]["host_signer_public_key"] = json!(TEST_HOST_SIGNER);
    std::fs::write(&f.cfg, v.to_string()).unwrap();
    f.put_inputs();
    let (code, rep) = f.run(&r, None);
    assert_eq!(code, Some(0), "control: the key in no other role: {rep}");
}

/// PSV-6 "of this epoch" at the root boundary: a genuine, correctly signed
/// observation of THIS manifest (same nonce, same facts) made for another
/// epoch than the manifest's `authority.epoch`. The nonce was issued for the
/// observation's epoch, so the custodian would spend it: the manifest join is
/// the only refusal. Control: the observation at the manifest's epoch
/// launches.
#[test]
fn an_observation_whose_epoch_is_not_the_manifests_launches_nothing() {
    let f = fx(None, "", |_| {});
    // A manifest naming a nonce issued for epoch 7 but authority epoch 0,
    // and an observation for epoch 7: the custodian would spend it (epoch 7
    // matches the issue), so only the manifest join can refuse.
    let mut m: Value = serde_json::from_slice(&f.manifest).unwrap();
    m["observation_nonce"] = json!(f.cust_issue(7));
    let manifest = serde_json::to_vec(&m).unwrap();
    let obs = serde_json::to_vec(&observation_of(&manifest, &f.observer.key_id(), 7)).unwrap();
    let f = Fx {
        manifest,
        observation: obs,
        ..f
    };
    f.put_inputs();
    let (code, rep) = f.run(&f.request("op-epoch"), None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: an observation for epoch 7 launched a manifest naming authority epoch 0: \
         {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap().contains("authority epoch"),
        "{rep}"
    );
    // Control: the fixture's own observation (epoch 0 = the manifest's).
    let f = fx(None, "", |_| {});
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: {rep}");
}
