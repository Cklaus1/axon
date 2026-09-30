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
    /// `<inputs>/policy.json` (the manifest's own, [`TEST_GUEST_POLICY`],
    /// unless a test puts another there).
    policy: Vec<u8>,
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
while [ $# -gt 0 ]; do case "$1" in --out) OUT="$2"; shift 2;; --psv-job) JOB="$2"; shift 2;; --policy) POL="$2"; shift 2;; *) shift;; esac; done
echo ran >> "{marker}"
id -ru > "$OUT/ruid"; id -u > "$OUT/euid"
sha256sum "$POL" | cut -d' ' -f1 > "$OUT/policy-sha"
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
        policy: TEST_GUEST_POLICY.as_bytes().to_vec(),
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
        // PSV-6 (A87): the guest policy, beside the job (never in the request).
        let _ = std::fs::remove_file(i.join("policy.json"));
        std::fs::write(i.join("policy.json"), &self.policy).unwrap();
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
            "schema": "axon-protected-launch-request/3",
            "id": "fab-0123456789abcdef",
            "out": self.out_root.join(out),
            "psv_candidate": i.join("candidate"),
            "psv_suite": i.join("check"),
            "psv_job": i.join("job"),
            "psv_manifest_sha256": sha256_file(&i.join("job/launch-manifest.json")),
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
        // A refusal may come before the request is read: outside the Fabric
        // group the kernel refuses the exec itself (126) and nothing reads
        // stdin. Whether the write then lands in the pipe or meets a closed
        // one is timing, so a broken pipe is not a test failure; the exit
        // code and report below are what each case judges.
        match child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
        {
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
            r => r.unwrap(),
        }
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
                "--bin",
                "axon-custodian",
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

/// The /etc/axon tree of a PRODUCTION helper and a PROTECTED custodian, built
/// under `s/etc` (copied onto a tmpfs at /etc/axon inside a private mount
/// namespace by [`in_production_etc`]): the helper's config, pinned inputs, a
/// stand-in launcher, one launch's inputs (a manifest naming a nonce the
/// custodian's store holds as issued for epoch 0, now), that manifest's
/// genuine observation signed under the operator observer root, and the
/// custodian's config (launcher_uid 0). Writes the request to
/// `s/request.json`. `store_parent` is where the custodian's store sits,
/// below /etc/axon.
fn production_etc(s: &Path, store_parent: &str) {
    let bin = production_build();
    let t = s.join("etc");
    std::fs::create_dir(&t).unwrap();
    let e = Path::new("/etc/axon");
    let inputs = helper_inputs(&t);
    std::fs::write(
        t.join("manifest.json"),
        inputs.pin_manifest(&full_lx_manifest(&"cd".repeat(32))),
    )
    .unwrap();
    write_executable(
        &t.join("launcher.sh"),
        "#!/bin/sh\n[ \"$1\" = \"--verify-result\" ] && exit 0\n\
         while [ $# -gt 0 ]; do case \"$1\" in --out) OUT=\"$2\"; shift 2;; *) shift;; esac; done\n\
         id -ru > \"$OUT/ruid\"\nexit 0\n",
        0o755,
    );
    copy_executable(
        bin.join("axon-protected-launcher"),
        s.join("axon-protected-launcher"),
        0o755,
    );
    copy_executable(bin.join("axon-custodian"), s.join("axon-custodian"), 0o755);
    let nonce = "0123456789abcdef0123456789abcdef";
    let manifest = serde_json::to_vec(&test_launch_manifest("op-1", nonce)).unwrap();
    let observer = Issuer::generate();
    observer.trust_in(&t.join("observer"), "obs");
    let observation =
        serde_json::to_vec(&observation_of(&manifest, &observer.key_id(), 0)).unwrap();
    let i = t.join("runs").join(INPUTS);
    for (sub, name, body) in [
        ("candidate", "f.ax", b"fn f() -> i64 { 1 }\n".as_slice()),
        ("check", "accept.ax", b"// suite\n".as_slice()),
        ("job", "launch-manifest.json", manifest.as_slice()),
    ] {
        std::fs::create_dir_all(i.join(sub)).unwrap();
        std::fs::write(i.join(sub).join(name), body).unwrap();
    }
    std::fs::write(i.join("job/completion-secret"), [7u8; 32]).unwrap();
    set_mode(&i.join("job/completion-secret"), 0o400);
    std::fs::write(i.join("policy.json"), TEST_GUEST_POLICY).unwrap();
    std::fs::create_dir_all(t.join("staging")).unwrap();
    std::fs::create_dir_all(t.join("run")).unwrap();
    let nonces = t.join(store_parent).join("nonces");
    std::fs::create_dir_all(&nonces).unwrap();
    let now = axon_fabric::backend::Clock::System.now_unix();
    std::fs::write(
        nonces.join(format!("{nonce}.issued")),
        json!({"epoch": 0, "issued_unix": now}).to_string(),
    )
    .unwrap();
    let socket = e.join("run/custodian.sock");
    std::fs::write(
        t.join("custodian.json"),
        json!({
            "schema": "axon-custodian/1",
            "custodian_uid": CUSTODIAN, "fabric_uid": FABRIC, "launcher_uid": 0,
            "socket": socket, "store": e.join(store_parent).join("nonces"), "max_age_s": 300,
        })
        .to_string(),
    )
    .unwrap();
    let pin = |p: &str| json!({"path": e.join(p), "sha256": sha256_file(&t.join(p))});
    let bash = bash_pin();
    std::fs::write(
        t.join("protected-launcher.json"),
        json!({
            "schema": "axon-protected-launcher/2",
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
            "observer": {"root": e.join("observer"), "max_age_s": 300,
                         "host_signer_public_key": TEST_HOST_SIGNER},
            "custodian": {"socket": socket, "uid": CUSTODIAN},
        })
        .to_string(),
    )
    .unwrap();
    let ei = e.join("runs").join(INPUTS);
    std::fs::write(
        s.join("request.json"),
        json!({
            "schema": "axon-protected-launch-request/3",
            "id": "fab-0123456789abcdef",
            "out": e.join("runs/op-1"),
            "psv_candidate": ei.join("candidate"),
            "psv_suite": ei.join("check"),
            "psv_job": ei.join("job"),
            "psv_manifest_sha256": sha256_file(&i.join("job/launch-manifest.json")),
            "timeout_s": 60,
            "observation": String::from_utf8(observation.clone()).unwrap(),
            "observation_signature": observer.sign_for(
                axon_fabric::backend::TrustAuthority::Observer,
                &observation,
            ),
        })
        .to_string(),
    )
    .unwrap();
}

/// Run `body` (sh, `$1` = `s`) in a private mount namespace where `s/etc` is
/// /etc/axon (root-owned; runs the Fabric uid's, the custodian's store its
/// uid's, 0700) and the PROTECTED custodian runs as its unit would start it:
/// its socket bound by root and passed as fd 3 (`systemd-socket-activate`,
/// which execs it on the first connection; mode 0666, the unit's
/// `SocketMode=`, so the Fabric uid can reach it), the custodian itself as
/// its uid.
/// The custodian's stderr goes to `s/custodian.err`; it is stopped after
/// `body`. The host's /etc is never written.
fn in_production_etc(s: &Path, store_parent: &str, body: &str) {
    in_production_etc_with(s, store_parent, "/etc/axon/run/custodian.sock", "", body)
}

/// [`in_production_etc`], with the custodian's activation socket at `sock`
/// and `prefix` run between `setpriv` and the custodian (e.g. an `env` that
/// changes what systemd passed).
fn in_production_etc_with(s: &Path, store_parent: &str, sock: &str, prefix: &str, body: &str) {
    let script = format!(
        "set -e\n\
         mount -t tmpfs -o mode=0755 tmpfs /etc/axon\n\
         cp -a \"$1/etc/.\" /etc/axon/\n\
         chown -R 0:0 /etc/axon\n\
         chmod 0755 /etc/axon\n\
         chown -R {FABRIC}:{FABRIC} /etc/axon/runs\n\
         chmod 0700 /etc/axon/runs /etc/axon/staging\n\
         chown -R {CUSTODIAN}:{CUSTODIAN} /etc/axon/{store_parent}/nonces\n\
         chmod 0700 /etc/axon/{store_parent}/nonces\n\
         {{ systemd-socket-activate -l {sock} setpriv --reuid={CUSTODIAN} \
         --regid={CUSTODIAN} --clear-groups -- {prefix} \"$1/axon-custodian\"; \
         echo $? > \"$1/custodian.code\"; }} 2> \"$1/custodian.err\" &\n\
         C=$!\n\
         set +e\n\
         n=0; while [ ! -S {sock} ] && [ $n -lt 200 ]; do sleep 0.05; n=$((n+1)); done\n\
         chmod 0666 {sock}\n\
         {body}\n\
         pkill -f \"$1/axon-custodian\" 2>/dev/null; wait $C 2>/dev/null\n\
         exit 0\n"
    );
    let st = Command::new("unshare")
        .args(["-m", "--propagation", "private", "sh", "-c", &script, "sh"])
        .arg(s)
        .status()
        .unwrap();
    assert!(st.success(), "setup: the namespace script failed");
}

/// A (M602), ROOT ONLY, PRODUCTION BUILD: the helper launches only when it
/// is root in every id (installed setuid-root). Installed any other way (on
/// a nosuid mount, or given file capabilities instead) it would run the
/// launch as its CALLER's uid, which can then signal or trace it. Here the
/// helper runs as the Fabric uid holding the capabilities it would need
/// (lease, DAC override, chown, fowner), with everything else a launch needs
/// GENUINE (C9 round 3, rows): the snapshot's launch manifest, its
/// observation signed under the operator observer root, and a PROTECTED
/// custodian (the production `axon-custodian`, socket-activated as its own
/// uid) holding the manifest's nonce. Two checks then stand in the way, each
/// alone: this euid check (M602) and the custodian's rule that only the
/// launcher uid, which its config must set to 0 (M640), spends a nonce
/// (M628). Control: the same launch (its inputs put back, as the Fabric uid
/// can always rebuild them) through the helper installed setuid-root runs,
/// so the fixture reaches a real launch.
#[test]
fn a_production_helper_that_is_not_root_launches_nothing() {
    if skip_unless_root() {
        return;
    }
    if !Path::new("/etc/axon").is_dir() {
        eprintln!("skipped: no /etc/axon mount point (this test never creates one)");
        return;
    }
    let d = tempfile::tempdir_in("/var/tmp").unwrap();
    let s = d.path();
    set_mode(s, 0o755);
    production_etc(s, "custodian");
    let caps = "+lease,+dac_override,+chown,+fowner";
    in_production_etc(
        s,
        "custodian",
        &format!(
            "setpriv --reuid={FABRIC} --regid={FABRIC} --clear-groups --inh-caps={caps} \
             --ambient-caps={caps} -- \"$1/axon-protected-launcher\" \
             < \"$1/request.json\" > \"$1/report.json\"\n\
             echo $? > \"$1/code\"\n\
             cp -a /etc/axon/runs/op-1 \"$1/result\" 2>/dev/null\n\
             rm -rf /etc/axon/runs/op-1 /etc/axon/runs/{INPUTS}\n\
             cp -a \"$1/etc/runs/{INPUTS}\" /etc/axon/runs/\n\
             chown -R {FABRIC}:{FABRIC} /etc/axon/runs/{INPUTS}\n\
             cp \"$1/axon-protected-launcher\" /etc/axon/h\n\
             chown 0:{FABRIC} /etc/axon/h\n\
             chmod 04750 /etc/axon/h\n\
             setpriv --reuid={FABRIC} --regid={FABRIC} --clear-groups -- \
             sh -c 'exec /etc/axon/h' < \"$1/request.json\" > \"$1/control.json\"\n\
             echo $? > \"$1/control.code\"\n\
             cp -a /etc/axon/runs/op-1 \"$1/control\" 2>/dev/null"
        ),
    );
    let read = |n: &str| std::fs::read_to_string(s.join(n)).unwrap_or_default();
    let (code, rep, cust_err) = (read("code"), read("report.json"), read("custodian.err"));
    let ruid = std::fs::read_to_string(s.join("result/ruid")).ok();
    assert!(
        ruid.is_none() && code.trim() == "30",
        "ATTACK: a production helper that is not root in every id launched (the launcher ran \
         with ruid {ruid:?}, which the caller can signal or trace): exit {} {rep}",
        code.trim()
    );
    // Refused by the euid check (M602) or, with it removed, by the protected
    // custodian refusing a spend from a uid other than 0 (M628): either
    // reason (four-cell record, C9 round 3). Any OTHER refusal means the
    // fixture did not reach the spend, and proves nothing.
    assert!(
        rep.contains("not 0") || rep.contains("is not the launcher uid 0"),
        "the refusal is the euid's or the protected custodian's spend rule: {rep} \
         (custodian: {cust_err})"
    );
    assert_eq!(
        (
            read("control.code").trim(),
            read("control/ruid").trim().to_string()
        ),
        ("0", "0".to_string()),
        "control: the setuid-root helper launches as root: {} (custodian: {cust_err})",
        read("control.json")
    );
}

/// The PROTECTED custodian's store must sit where the operator put it: every
/// directory above it root-owned and not group/other-writable (the store
/// itself is the custodian's own, checked by `check_store`). A store in a
/// directory the Fabric uid owns could be renamed away and replaced with one
/// holding nonces nobody issued (C9 round 3, rows; M641). It was checked by
/// listing the parent's ENTRIES too, which refused every correctly deployed
/// store (the store is the custodian's uid's): no protected custodian could
/// start. Control: the same custodian with an operator-owned parent serves.
#[test]
fn a_protected_custodian_serves_only_from_a_store_the_operator_placed() {
    if skip_unless_root() {
        return;
    }
    if !Path::new("/etc/axon").is_dir() {
        eprintln!("skipped: no /etc/axon mount point (this test never creates one)");
        return;
    }
    // A client as the Fabric uid: one issue request, the reply to `$1/<n>`.
    let ask = |n: &str| {
        format!(
            "setpriv --reuid={FABRIC} --regid={FABRIC} --clear-groups -- python3 -c '\n\
             import socket, sys\n\
             s = socket.socket(socket.AF_UNIX)\n\
             s.settimeout(20)\n\
             s.connect(\"/etc/axon/run/custodian.sock\")\n\
             s.sendall(b\"{{\\\"schema\\\":\\\"axon-custodian-request/1\\\",\\\"op\\\":\\\"issue\\\",\\\"epoch\\\":0}}\\n\")\n\
             sys.stdout.write(s.recv(4096).decode())\n\
             ' > \"$1/{n}\" 2>/dev/null"
        )
    };
    for (parent, agent_owned) in [("agent", true), ("custodian", false)] {
        let d = tempfile::tempdir_in("/var/tmp").unwrap();
        let s = d.path();
        set_mode(s, 0o755);
        production_etc(s, parent);
        let own = if agent_owned {
            format!("chown {FABRIC}:{FABRIC} /etc/axon/{parent}\n")
        } else {
            String::new()
        };
        in_production_etc(s, parent, &format!("{own}{}", ask("reply.json")));
        let reply = std::fs::read_to_string(s.join("reply.json")).unwrap_or_default();
        let err = std::fs::read_to_string(s.join("custodian.err")).unwrap_or_default();
        if agent_owned {
            assert!(
                !reply.contains("\"ok\":true"),
                "ATTACK: a protected custodian served from a store whose parent the Fabric uid \
                 owns: {reply}"
            );
            assert!(err.contains("not root"), "{err}");
        } else {
            assert!(
                reply.contains("\"ok\":true") && reply.contains("\"mode\":\"protected\""),
                "control: the protected custodian serves from an operator-placed store: {reply} \
                 ({err})"
            );
        }
    }
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

/// PSV-6 (C9 round 4; A87), the reviewer's reproduction as an attack. The
/// Fabric uid holds a GENUINE observation of a launch manifest naming policy
/// P1 (`allowed_effects: []`), and puts P2 (IO, Net, Time) where the helper
/// takes the guest policy from. The helper refuses before the nonce is spent,
/// so the SAME manifest and observation then launch with P1 (control), and the
/// launcher is handed exactly the manifest's policy. The old channel (a
/// `policy_json` in the request) is not accepted at all.
#[test]
fn a_genuine_observation_of_one_policy_never_launches_another() {
    let p2 = r#"{"schema":"axon-vm-mmds/1","allowed_effects":["IO","Net","Time"]}"#;
    let f = fx(None, "", |_| {});
    let named = serde_json::from_slice::<Value>(&f.manifest).unwrap()["policy_sha256"].clone();
    assert_eq!(
        named,
        json!(axon_psv::sha256_hex(TEST_GUEST_POLICY.as_bytes()))
    );
    let f = Fx {
        policy: p2.as_bytes().to_vec(),
        ..f
    };
    f.put_inputs();
    let (code, rep) = f.run(&f.request("op-p2"), None);
    assert!(
        code == Some(30) && f.launches() == 0 && !f.out_root.join("op-p2").exists(),
        "ATTACK: the root helper launched policy P2 under a genuine observation of a manifest \
         naming P1: {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("not the policy_sha256"),
        "{rep}"
    );
    // The old channel: a policy beside the manifest in the request itself.
    let f = Fx {
        policy: TEST_GUEST_POLICY.as_bytes().to_vec(),
        ..f
    };
    f.put_inputs();
    let mut r = f.request("op-p2-req");
    r["policy_json"] = json!(p2);
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "the request carries no policy: {code:?} {rep}"
    );
    // Control: the refusal came before the spend, so the same manifest and
    // observation launch with the manifest's policy, which is what the
    // launcher is handed.
    f.put_inputs();
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: {rep}");
    let got = std::fs::read_to_string(f.out_root.join("op-1/policy-sha")).unwrap();
    assert_eq!(
        json!(got.trim()),
        named,
        "the launcher was handed another policy"
    );
}

/// PSV-6 (A87): on the protected profile a policy with no effect ceiling
/// (`allowed_effects` omitted) is refused at the root boundary even when it IS
/// the manifest's and the observation of that manifest is genuine: an absent
/// ceiling is never read as "no ceiling". Control: the fixture's explicit
/// `allowed_effects: []` launches (`the_helper_launches_a_well_formed_request…`,
/// and the control below).
#[test]
fn a_manifest_policy_naming_no_ceiling_launches_nothing() {
    let p0 = r#"{"schema":"axon-vm-mmds/1","budget_tokens":0}"#;
    let f = fx(None, "", |_| {});
    let mut m: Value = serde_json::from_slice(&f.manifest).unwrap();
    m["policy_sha256"] = json!(axon_psv::sha256_hex(p0.as_bytes()));
    let manifest = serde_json::to_vec(&m).unwrap();
    let observation =
        serde_json::to_vec(&observation_of(&manifest, &f.observer.key_id(), 0)).unwrap();
    let f = Fx {
        manifest,
        observation,
        policy: p0.as_bytes().to_vec(),
        ..f
    };
    f.put_inputs();
    let (code, rep) = f.run(&f.request("op-p0"), None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper launched a manifest policy that states no effect ceiling: \
         {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("names no allowed_effects"),
        "{rep}"
    );
    let f = fx(None, "", |_| {});
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: {rep}");
}

// ── C9 round 4, EQUIVALENCE (rows workstream; M690-M719) ────────────────────
//
// Protected rules enforced in production through calls with no row, or rows
// killed only by a unit test calling the rule directly. Each test here drives
// the PRODUCTION entry: the production-build binaries, reading their config
// from /etc/axon (a tmpfs in a private mount namespace; the host's /etc is
// never written), socket-activated as their units start them.

/// Sh for one `issue` request as `uid` to the custodian at `sock`, the reply
/// to `$1/<out>`.
fn ask_issue(uid: u32, sock: &str, out: &str) -> String {
    format!(
        "setpriv --reuid={uid} --regid={uid} --clear-groups -- python3 -c '\n\
         import socket, sys\n\
         s = socket.socket(socket.AF_UNIX)\n\
         s.settimeout(20)\n\
         s.connect(\"{sock}\")\n\
         s.sendall(b\"{{\\\"schema\\\":\\\"axon-custodian-request/1\\\",\\\"op\\\":\\\"issue\\\",\\\"epoch\\\":0}}\\n\")\n\
         sys.stdout.write(s.recv(4096).decode())\n\
         ' > \"$1/{out}\" 2>/dev/null\n"
    )
}

fn root_with_etc_axon() -> bool {
    if skip_unless_root() {
        return false;
    }
    if !Path::new("/etc/axon").is_dir() {
        eprintln!("skipped: no /etc/axon mount point (this test never creates one)");
        return false;
    }
    true
}

/// A fresh production /etc/axon (see [`production_etc`]) in a new temp dir,
/// with the custodian's config edited by `edit`.
fn production_etc_with_custodian(edit: impl FnOnce(&mut Value)) -> tempfile::TempDir {
    let d = tempfile::tempdir_in("/var/tmp").unwrap();
    set_mode(d.path(), 0o755);
    production_etc(d.path(), "custodian");
    let p = d.path().join("etc/custodian.json");
    let mut v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    edit(&mut v);
    std::fs::write(&p, v.to_string()).unwrap();
    d
}

const CUSTODIAN_SOCK: &str = "/etc/axon/run/custodian.sock";

/// A83 on the PRODUCTION custodian (M700; M629 and M640 re-anchored here;
/// M701), ROOT ONLY: `axon-custodian` with no arguments reads
/// /etc/axon/custodian.json and applies the PROTECTED config rules there
/// (`load_config` -> `check(true)`) before serving anything. Three configs
/// an operator could mistype, each breaking one rule and nothing else: the
/// custodian is the Fabric uid (M629), a uid other than 0 spends (M640), the
/// Fabric is root (M701). The custodian itself runs, socket-activated, as the
/// uid its config names, from its own 0700 store, so no other check refuses
/// it; asked for a nonce by the configured Fabric uid, it must serve nothing.
/// Control: the operator's conforming config serves, in protected mode.
#[test]
fn a_protected_custodian_under_a_config_breaking_a83_serves_nothing() {
    if !root_with_etc_axon() {
        return;
    }
    let d = production_etc_with_custodian(|_| {});
    let s = d.path();
    in_production_etc(
        s,
        "custodian",
        &ask_issue(FABRIC, CUSTODIAN_SOCK, "reply.json"),
    );
    let reply = std::fs::read_to_string(s.join("reply.json")).unwrap_or_default();
    assert!(
        reply.contains("\"ok\":true") && reply.contains("\"mode\":\"protected\""),
        "control: the production custodian serves under the operator's config: {reply} ({})",
        std::fs::read_to_string(s.join("custodian.err")).unwrap_or_default()
    );
    for (fabric, launcher, attack, why) in [
        (
            CUSTODIAN,
            0,
            "a protected custodian whose config names the Fabric uid as the custodian served",
            "is the Fabric uid",
        ),
        (
            FABRIC,
            FABRIC,
            "a protected custodian whose config lets uid 4242 spend nonces served",
            "not 0",
        ),
        (
            0,
            0,
            "a protected custodian whose config names root as the Fabric served",
            "neither the custodian nor the Fabric runs as root",
        ),
    ] {
        let d = production_etc_with_custodian(|v| {
            v["fabric_uid"] = json!(fabric);
            v["launcher_uid"] = json!(launcher);
        });
        let s = d.path();
        in_production_etc(
            s,
            "custodian",
            &ask_issue(fabric, CUSTODIAN_SOCK, "reply.json"),
        );
        let reply = std::fs::read_to_string(s.join("reply.json")).unwrap_or_default();
        let err = std::fs::read_to_string(s.join("custodian.err")).unwrap_or_default();
        assert!(!reply.contains("\"ok\":true"), "ATTACK: {attack}: {reply}");
        assert!(err.contains(why), "{why}: {err}");
    }
}

/// D6 on the PRODUCTION custodian (M702, M703), ROOT ONLY: a protected
/// custodian serves only the listener its systemd socket unit passed, at the
/// socket its config names. Started with the socket at fd 3 but without the
/// activation's `LISTEN_PID`/`LISTEN_FDS` (not by its unit: M702), or
/// activated on another socket than the configured one (M703), it serves
/// nothing, although the socket it holds would take the request. Control:
/// its unit's activation on the configured socket serves.
#[test]
fn a_protected_custodian_serves_only_its_units_activation_on_its_socket() {
    if !root_with_etc_axon() {
        return;
    }
    let other = "/etc/axon/run/other.sock";
    for (sock, prefix, attack, why) in [
        (CUSTODIAN_SOCK, "", "", ""),
        (
            CUSTODIAN_SOCK,
            "env -u LISTEN_PID -u LISTEN_FDS",
            "a protected custodian not started by its socket unit served",
            "not socket-activated",
        ),
        (
            other,
            "",
            "a protected custodian activated on another socket than its configured one served",
            "not the configured",
        ),
    ] {
        let d = production_etc_with_custodian(|_| {});
        let s = d.path();
        in_production_etc_with(
            s,
            "custodian",
            sock,
            prefix,
            &ask_issue(FABRIC, sock, "reply.json"),
        );
        let reply = std::fs::read_to_string(s.join("reply.json")).unwrap_or_default();
        let err = std::fs::read_to_string(s.join("custodian.err")).unwrap_or_default();
        if attack.is_empty() {
            assert!(
                reply.contains("\"ok\":true") && reply.contains("\"mode\":\"protected\""),
                "control: the unit's activation serves: {reply} ({err})"
            );
            continue;
        }
        assert!(!reply.contains("\"ok\":true"), "ATTACK: {attack}: {reply}");
        assert!(err.contains(why), "{why}: {err}");
    }
}

/// A production /etc/axon for `ProtectedHost::operator()`: [`production_etc`]
/// (the helper's config and pinned inputs, the custodian's config) plus the
/// host config /etc/axon/protected-host.json describing the SAME launch path
/// (launcher, profile manifest, out root, custodian, host signer), a
/// qualification and an observer trust root at /etc/axon/trust/, and the
/// production `axon-fabric` at `s/axon-fabric`. `host` and `helper` edit the
/// two configs.
fn production_host_etc(
    host: impl FnOnce(&mut Value),
    helper: impl FnOnce(&mut Value),
) -> tempfile::TempDir {
    let d = tempfile::tempdir_in("/var/tmp").unwrap();
    let s = d.path();
    set_mode(s, 0o755);
    production_etc(s, "custodian");
    let bin = production_build();
    copy_executable(bin.join("axon-fabric"), s.join("axon-fabric"), 0o755);
    let t = s.join("etc");
    let e = Path::new("/etc/axon");
    copy_executable(
        bin.join("axon-protected-launcher"),
        t.join("protected-launcher"),
        0o755,
    );
    std::fs::write(t.join("observer.sh"), "#!/bin/sh\n").unwrap();
    std::fs::write(t.join("registry.json"), "{}\n").unwrap();
    std::fs::write(t.join("evidence.json"), "{}\n").unwrap();
    std::fs::create_dir_all(t.join("keys")).unwrap();
    Issuer::generate().trust_in(&t.join("trust/qualification"), "operator");
    Issuer::generate().trust_in(&t.join("trust/observer"), "observer");
    let pin = |p: &str| json!({"path": e.join(p), "sha256": sha256_file(&t.join(p))});
    let mut v = json!({
        "schema": "axon-protected-host/1",
        "launcher": pin("launcher.sh"),
        "privileged_launcher": pin("protected-launcher"),
        "profile_manifest": pin("manifest.json"),
        "artifacts_dir": e.join("dist"),
        "qualification": {"record": e.join("evidence.json"), "signature": null,
                          "waivers": null, "max_age_s": 2_592_000},
        "suite_registry": pin("registry.json"),
        "signer": {"issuer_ref": "verifier:fabric", "public_key": TEST_HOST_SIGNER,
                   "key_path": e.join("keys/attest.pk8")},
        "out_root": e.join("runs"),
        "observer": {"command": pin("observer.sh"),
                     "custodian": {"socket": e.join("run/custodian.sock"), "uid": CUSTODIAN}},
    });
    host(&mut v);
    std::fs::write(t.join("protected-host.json"), v.to_string()).unwrap();
    let p = t.join("protected-launcher.json");
    let mut h: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    helper(&mut h);
    std::fs::write(&p, h.to_string()).unwrap();
    d
}

/// The production `axon-fabric submit` in `s`'s /etc/axon, as `uid` (whose
/// out root it is): `ProtectedHost::operator()` decides first. A request
/// that does not exist makes an ACCEPTED host exit 2 (`io`) at the request,
/// and a refused one exit 4 (`protected host: …`). Returns (exit, report).
fn fabric_on_production_host(s: &Path, uid: u32) -> (String, String) {
    in_production_etc(
        s,
        "custodian",
        &format!(
            "chown {uid}:{uid} /etc/axon/runs\n\
             setpriv --reuid={uid} --regid={uid} --clear-groups -- \"$1/axon-fabric\" submit \
             --request /nonexistent/request.json > \"$1/fabric.json\"\n\
             echo $? > \"$1/fabric.code\""
        ),
    );
    let read = |n: &str| std::fs::read_to_string(s.join(n)).unwrap_or_default();
    (read("fabric.code").trim().to_string(), read("fabric.json"))
}

/// The control every `ProtectedHost::operator()` test starts from: the
/// conforming production host, as the Fabric uid, is accepted (the run
/// reaches its request).
fn a_conforming_production_host_is_accepted() {
    let d = production_host_etc(|_| {}, |_| {});
    let (code, rep) = fabric_on_production_host(d.path(), FABRIC);
    assert!(
        code == "2" && rep.contains("\"kind\":\"io\""),
        "control: a conforming protected host is accepted (the run reaches its request): \
         exit {code} {rep}"
    );
}

/// A on `ProtectedHost::operator()` (M704, M546; four-cell records against
/// M548), ROOT ONLY: a production `axon-fabric` running as ROOT on a host
/// whose /etc/axon/protected-host.json exists is refused. Two rules refuse
/// it, each alone: `fabric_is_not_root`, and `helper_agrees`, because the
/// helper's config admits the Fabric uid, never root (a helper config
/// admitting root is refused by its own `load_config`, M536). The Fabric's
/// out root is root's here, as it would be for a root service, so nothing
/// else refuses: either reason.
#[test]
fn a_production_fabric_running_as_root_is_refused_on_a_protected_host() {
    if !root_with_etc_axon() {
        return;
    }
    a_conforming_production_host_is_accepted();
    let d = production_host_etc(|_| {}, |_| {});
    let (code, rep) = fabric_on_production_host(d.path(), 0);
    assert!(
        code == "4",
        "ATTACK: a production Fabric running as root was accepted on a protected host: \
         exit {code} {rep}"
    );
    assert!(
        rep.contains("running as root")
            || rep.contains("admits uid 4242, but Fabric runs as uid 0"),
        "{rep}"
    );
}

/// A83 on `ProtectedHost::operator()` (M705, M634; four-cell records against
/// M633), ROOT ONLY: a host config whose custodian IS the Fabric's uid (with
/// the helper's config naming that same custodian, so the two agree) is
/// refused. Two rules refuse it, each alone: `custodian_is_separate`, and
/// the helper config's own rule (its custodian is its Fabric uid): either
/// reason.
#[test]
fn a_production_fabric_refuses_a_protected_host_whose_custodian_is_the_fabric() {
    if !root_with_etc_axon() {
        return;
    }
    a_conforming_production_host_is_accepted();
    let d = production_host_etc(
        |v| v["observer"]["custodian"]["uid"] = json!(FABRIC),
        |h| h["custodian"]["uid"] = json!(FABRIC),
    );
    let (code, rep) = fabric_on_production_host(d.path(), FABRIC);
    assert!(
        code == "4",
        "ATTACK: a production Fabric accepted a protected host whose custodian is the Fabric's \
         own uid: exit {code} {rep}"
    );
    assert!(
        rep.contains("is a separate uid") || rep.contains("is the Fabric uid or root"),
        "{rep}"
    );
}

/// A, A83 and ADR-002 on `ProtectedHost::operator()` (M706, M707; M547,
/// M548, M636 and M637 re-anchored here), ROOT ONLY: `operator()` reads the
/// helper's operator config under the PRODUCTION rules, and refuses one that
/// describes another launch path than the host config. Each attack is one
/// operator mistake, refused by one check alone:
///
/// * a host with no observer, and a helper config whose custodian is the
///   Fabric uid: only the production rules refuse it (M706; with no observer
///   nothing compares custodians);
/// * the helper runs another launcher (M547), admits another uid (M548),
///   spends through another custodian (M636), or names another host signer
///   (M637) than the host config: only `helper_agrees` refuses each (M707,
///   its call).
#[test]
fn a_production_fabric_refuses_a_helper_config_that_disagrees_with_its_host() {
    if !root_with_etc_axon() {
        return;
    }
    a_conforming_production_host_is_accepted();
    let d = production_host_etc(
        |v| {
            v.as_object_mut().unwrap().remove("observer");
        },
        |h| h["custodian"]["uid"] = json!(FABRIC),
    );
    let (code, rep) = fabric_on_production_host(d.path(), FABRIC);
    assert!(
        code == "4",
        "ATTACK: a production Fabric read the helper config under development rules and \
         accepted a helper whose custodian is the Fabric uid: exit {code} {rep}"
    );
    assert!(rep.contains("is the Fabric uid or root"), "{rep}");
    for (edit, attack, why) in [
        (
            (|h: &mut Value| h["launcher"]["sha256"] = json!("b".repeat(64))) as fn(&mut Value),
            "a helper config running another launcher than the host pins",
            "runs launcher",
        ),
        (
            |h| h["fabric_uid"] = json!(OTHER),
            "a helper config admitting another uid than the Fabric's",
            "admits uid",
        ),
        (
            |h| h["custodian"]["uid"] = json!(4245),
            "a helper config spending through another custodian than the host's",
            "spends through custodian",
        ),
        (
            |h| h["observer"]["host_signer_public_key"] = json!("d".repeat(64)),
            "a helper config naming another host signer than the host's",
            "names another host signer",
        ),
    ] {
        let d = production_host_etc(|_| {}, edit);
        let (code, rep) = fabric_on_production_host(d.path(), FABRIC);
        assert!(
            code == "4",
            "ATTACK: a production Fabric accepted {attack}: exit {code} {rep}"
        );
        assert!(rep.contains(why), "{why}: {rep}");
    }
}

/// One launch by `helper` (a file in `s`), installed setuid-root at
/// /etc/axon/h, from the Fabric uid, in a fresh production /etc/axon; with
/// `no_lease`, the test-trust switch `/etc/axon/TEST-no-read-lease` exists
/// (every read lease refused, as on a filesystem that grants none). Returns
/// (exit, report, the launcher's ruid if it ran).
fn production_launch(helper: &str, no_lease: bool) -> (String, String, Option<String>) {
    let d = tempfile::tempdir_in("/var/tmp").unwrap();
    let s = d.path();
    set_mode(s, 0o755);
    production_etc(s, "custodian");
    copy_executable(
        env!("CARGO_BIN_EXE_axon-protected-launcher"),
        s.join("test-trust-launcher"),
        0o755,
    );
    let switch = if no_lease {
        "touch /etc/axon/TEST-no-read-lease\n"
    } else {
        ""
    };
    in_production_etc(
        s,
        "custodian",
        &format!(
            "{switch}cp \"$1/{helper}\" /etc/axon/h\n\
             chown 0:{FABRIC} /etc/axon/h\n\
             chmod 04750 /etc/axon/h\n\
             setpriv --reuid={FABRIC} --regid={FABRIC} --clear-groups -- \
             sh -c 'exec /etc/axon/h' < \"$1/request.json\" > \"$1/report.json\"\n\
             echo $? > \"$1/code\"\n\
             cp -a /etc/axon/runs/op-1 \"$1/result\" 2>/dev/null"
        ),
    );
    let read = |n: &str| std::fs::read_to_string(s.join(n)).unwrap_or_default();
    (
        read("code").trim().to_string(),
        read("report.json"),
        std::fs::read_to_string(s.join("result/ruid"))
            .ok()
            .map(|r| r.trim().to_string()),
    )
}

/// D on the PRODUCTION helper's route (M708; M591 re-anchored here; M709),
/// ROOT ONLY: installed setuid-root and reading its operator config
/// (`Authority::production()`), the helper opens every authority program
/// under `Lease::Required` (M708 selects it, M591 refuses without one).
/// Without the lease a writer that opens the launcher after its hash goes
/// undetected, so the bytes that run need not be the bytes verified. The
/// kernel always grants a root helper a lease on a root-owned file, so the
/// refusal is driven with the test-trust switch that makes every lease
/// unavailable (a filesystem that grants none): the test-trust helper, in
/// PRODUCTION mode, launches nothing. Control: the same helper with the
/// lease available launches, as root. And the switch is not in a production
/// build (M709): the production helper, with the switch present, launches.
#[test]
fn a_production_helper_launches_nothing_it_cannot_lease() {
    if !root_with_etc_axon() {
        return;
    }
    let (code, rep, ruid) = production_launch("test-trust-launcher", false);
    assert_eq!(
        (code.as_str(), ruid.as_deref()),
        ("0", Some("0")),
        "control: the helper, in production mode, launches as root when it holds the lease: \
         {rep}"
    );
    let (code, rep, ruid) = production_launch("test-trust-launcher", true);
    assert!(
        code == "30" && ruid.is_none(),
        "ATTACK: the production helper launched an authority program it could not lease: \
         exit {code} ruid {ruid:?} {rep}"
    );
    assert!(rep.contains("no read lease"), "{rep}");
    let (code, rep, ruid) = production_launch("axon-protected-launcher", true);
    assert_eq!(
        (code.as_str(), ruid.as_deref()),
        ("0", Some("0")),
        "ATTACK: a production helper obeyed the test-trust lease switch (the test seam is in \
         the production build): {rep}"
    );
    let prod = std::fs::read(production_build().join("axon-protected-launcher")).unwrap();
    let test_trust = std::fs::read(env!("CARGO_BIN_EXE_axon-protected-launcher")).unwrap();
    let has = |b: &[u8]| {
        b.windows(b"TEST-no-read-lease".len())
            .any(|w| w == b"TEST-no-read-lease")
    };
    assert!(
        has(&test_trust),
        "control: the test-trust helper carries the switch"
    );
    assert!(
        !has(&prod),
        "ATTACK: the production helper binary carries the test-trust lease switch"
    );
}

// ── C9 round 4 fix wave, ROWS2 (EQUIVALENCE (4); rows M760-M819) ────────────
//
// `scripts/v022_refusal_coverage.py` now lists the refusal sites of the
// custodian, its binary, readiness and the protected host config too. Each
// test below attacks, through the PRODUCTION entry, a site that had neither a
// row nor an exemption.

/// A83 on the PRODUCTION custodian (M760, M761), ROOT ONLY: the config rules
/// that hold in every mode are applied to /etc/axon/custodian.json before the
/// protected custodian serves anything: its schema (M760) and a positive
/// nonce lifetime (M761). Each config below breaks that one rule and nothing
/// else (the three principals stay separate, the store is the custodian's own,
/// it is activated on its configured socket), so only that rule refuses it.
/// Control: the operator's conforming config serves.
#[test]
fn a_protected_custodian_under_a_malformed_config_serves_nothing() {
    if !root_with_etc_axon() {
        return;
    }
    for (edit, attack, why) in [
        ((|_: &mut Value| {}) as fn(&mut Value), "", ""),
        (
            |v| v["schema"] = json!("axon-custodian/0"),
            "a protected custodian whose config is of another schema served",
            "schema is not axon-custodian/1",
        ),
        (
            |v| v["max_age_s"] = json!(0),
            "a protected custodian whose config sets no nonce lifetime (max_age_s 0) served",
            "max_age_s must be positive",
        ),
    ] {
        let d = production_etc_with_custodian(edit);
        let s = d.path();
        in_production_etc(
            s,
            "custodian",
            &ask_issue(FABRIC, CUSTODIAN_SOCK, "reply.json"),
        );
        let reply = std::fs::read_to_string(s.join("reply.json")).unwrap_or_default();
        let err = std::fs::read_to_string(s.join("custodian.err")).unwrap_or_default();
        if attack.is_empty() {
            assert!(
                reply.contains("\"ok\":true") && reply.contains("\"mode\":\"protected\""),
                "control: the production custodian serves under the operator's config: \
                 {reply} ({err})"
            );
            continue;
        }
        assert!(!reply.contains("\"ok\":true"), "ATTACK: {attack}: {reply}");
        assert!(err.contains(why), "{why}: {err}");
    }
}

/// D6 (M763), PRODUCTION BUILD: the installed custodian reads its config only
/// from the operator's fixed path. `--test-config` (a config file of its
/// CALLER's choosing) exists in test-trust builds alone, like the helper's
/// (M601). Here a config that the test-trust custodian serves under (this
/// uid, its own 0700 store) is offered to the production build: it must not
/// serve. Control: the test-trust custodian, given the same flag and file,
/// serves, so the config itself is one a custodian would run under.
#[test]
fn a_production_custodian_never_takes_its_config_from_a_path_its_caller_names() {
    let d = tempfile::tempdir_in("/var/tmp").unwrap();
    let store = d.path().join("store");
    std::fs::create_dir(&store).unwrap();
    set_mode(&store, 0o700);
    let serves = |bin: &Path, tag: &str| -> (bool, String) {
        let sock = d.path().join(format!("{tag}.sock"));
        let cfg = d.path().join(format!("{tag}.json"));
        std::fs::write(
            &cfg,
            json!({
                "schema": "axon-custodian/1",
                "custodian_uid": euid(), "fabric_uid": euid(), "launcher_uid": euid(),
                "socket": sock, "store": store, "max_age_s": 300,
            })
            .to_string(),
        )
        .unwrap();
        let mut child = Command::new(bin)
            .arg("--test-config")
            .arg(&cfg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut bound = false;
        for _ in 0..200 {
            if sock.exists() {
                bound = true;
                break;
            }
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let _ = child.kill();
        let out = child.wait_with_output().unwrap();
        (bound, String::from_utf8_lossy(&out.stderr).to_string())
    };
    let (bound, err) = serves(Path::new(env!("CARGO_BIN_EXE_axon-custodian")), "test");
    assert!(
        bound,
        "control: the test-trust custodian serves under --test-config: {err}"
    );
    let (bound, err) = serves(&production_build().join("axon-custodian"), "prod");
    assert!(
        !bound,
        "ATTACK: the production custodian took its config from a path its caller named and \
         served: {err}"
    );
    assert!(err.contains("usage"), "{err}");
}

/// [`fabric_on_production_host`], with `pre` run as root in the namespace
/// just before the Fabric starts.
fn fabric_on_production_host_after(s: &Path, uid: u32, pre: &str) -> (String, String) {
    in_production_etc(
        s,
        "custodian",
        &format!(
            "chown {uid}:{uid} /etc/axon/runs\n\
             {pre}\n\
             setpriv --reuid={uid} --regid={uid} --clear-groups -- \"$1/axon-fabric\" submit \
             --request /nonexistent/request.json > \"$1/fabric.json\"\n\
             echo $? > \"$1/fabric.code\""
        ),
    );
    let read = |n: &str| std::fs::read_to_string(s.join(n)).unwrap_or_default();
    (read("fabric.code").trim().to_string(), read("fabric.json"))
}

/// `ProtectedHost::operator()` (M764-M766), ROOT ONLY, PRODUCTION BUILD: a
/// production `axon-fabric` refuses a host config it cannot vouch for, each
/// refused by one rule alone:
///
/// * a config of another schema (M764);
/// * a config naming the helper's `test_config`, a test-trust-build key: a
///   production helper reads only its fixed path (M765);
/// * a host whose config cannot be stat'ed (here /etc/axon is 0700, so the
///   Fabric uid gets EACCES): it cannot tell whether it is a protected host,
///   so nothing runs, rather than running as a development host (M766).
///
/// Control: the conforming host is accepted (the run reaches its request).
#[test]
fn a_production_fabric_refuses_a_host_config_it_cannot_vouch_for() {
    if !root_with_etc_axon() {
        return;
    }
    a_conforming_production_host_is_accepted();
    for (edit, pre, attack, why) in [
        (
            (|v: &mut Value| v["schema"] = json!("axon-protected-host/0")) as fn(&mut Value),
            "",
            "a host config of another schema",
            "schema is not axon-protected-host/1",
        ),
        (
            |v| v["privileged_launcher"]["test_config"] = json!("/etc/axon/test-helper.json"),
            "",
            "a host config naming the helper's test-trust config",
            "test-trust-build key",
        ),
        (
            |_| {},
            "chmod 0700 /etc/axon",
            "a host whose config it cannot stat, as a development host",
            "cannot tell whether this is a protected host",
        ),
    ] {
        let d = production_host_etc(edit, |_| {});
        let (code, rep) = fabric_on_production_host_after(d.path(), FABRIC, pre);
        assert!(
            code == "4",
            "ATTACK: a production Fabric accepted {attack}: exit {code} {rep}"
        );
        assert!(rep.contains(why), "{why}: {rep}");
    }
}

/// A (M774), ROOT ONLY: the guest policy is read, like every other psv input,
/// only as a regular file of the Fabric uid. Here `policy.json` is a
/// ROOT-owned file (a hard link to a root file, as an older tree could plant)
/// whose bytes are exactly the manifest's policy, so the digest join after it
/// holds and only the owner check stands between the root helper and reading
/// a file the Fabric uid does not own. Control: the same launch with the
/// Fabric's own policy.json runs.
#[test]
fn a_root_owned_policy_is_never_read_by_the_helper() {
    if euid() != 0 {
        eprintln!("skipped: needs root to install a setuid helper and act as service uids");
        return;
    }
    let f = fx(Some(FABRIC), "", |_| {});
    let policy = f.out_root.join(INPUTS).join("policy.json");
    std::os::unix::fs::chown(&policy, Some(0), Some(0)).unwrap();
    set_mode(&policy, 0o644);
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: a root-owned policy.json among the Fabric's inputs was read by the root helper \
         and launched: {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("not a regular file of the Fabric uid"),
        "{rep}"
    );
    let f = fx(Some(FABRIC), "", |_| {});
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A84 (M773, M774): the root helper launches only on an observation that
/// joins the snapshot's manifest field for field. Here the observation is of
/// THIS manifest (its `intended_launch_manifest_sha256`, nonce and every
/// other joined fact are the manifest's, and the observer signed it), except
/// the guest init it saw: `guest.init_sha256` is joined apart from the other
/// guest digests, and only that join refuses it. Control: the genuine
/// observation launches.
#[test]
fn an_observation_of_another_guest_init_launches_nothing() {
    let f = fx(None, "", |_| {});
    let mut o = observation_of(&f.manifest, &f.observer.key_id(), 0);
    o["guest"]["init_sha256"] = json!("e".repeat(64));
    let o = serde_json::to_vec(&o).unwrap();
    let mut r = f.request("op-i");
    r["observation"] = json!(String::from_utf8(o.clone()).unwrap());
    r["observation_signature"] = json!(f
        .observer
        .sign_for(axon_fabric::backend::TrustAuthority::Observer, &o));
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: an observation of another guest init launched the root launcher: {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap_or("").contains("init_sha256"),
        "{rep}"
    );
    f.put_inputs();
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: {rep}");
}
