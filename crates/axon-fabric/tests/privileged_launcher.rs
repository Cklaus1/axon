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
# Amendment 107: what the root helper handed this child, as the child sees it: the
# initial environment exactly (/proc/PID/environ is the envp the exec was given, not
# what the shell made of it) and every argument, one per line.
N=launch; [ "$1" = "--verify-result" ] && N=verify
tr '\0' '\n' < /proc/$$/environ > "{rec}/environ-$N"
printf '%s\n' "$@" > "{rec}/argv-$N"
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
        rec = base.display(),
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
        // Amendment 79: the helper serves only the pinned Fabric program. A
        // non-self Fabric runs the helper from a shell AS its uid (`run`), so
        // that shell is the program.
        v["fabric"] = shell_fabric_pin(TEST_FABRIC_REVISION);
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
                // Not `exec`: the shell stays as the helper's PARENT, the
                // Fabric program the helper measures (amendment 79).
                c.args(["--", "sh", "-c", "\"$0\" \"$@\"; exit $?"])
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

/// A132 (M1851, amendment 79): the helper launches only for the Fabric
/// program the operator pinned. The pin is the digest of the executable the
/// caller's pidfd names; a caller running another program (here: the pin names
/// a digest this process is not) gets nothing launched, though its uid is the
/// Fabric's. Before, any program of the Fabric uid launched. Control: the pin
/// naming this process launches.
#[test]
fn a_helper_launches_only_for_the_fabric_program_the_operator_pinned() {
    let f = fx(None, "", |v| v["fabric"]["sha256"] = json!("9".repeat(64)));
    let (code, r) = f.run(&f.request("op-1"), None);
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: a helper launched for a caller that is not the pinned Fabric program: {code:?} {r}"
    );
    assert!(
        r["error"].as_str().unwrap_or("").contains("pinned Fabric"),
        "{r}"
    );
    let f = fx(None, "", |_| {});
    let (code, r) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: the pinned program launches: {r}");
    assert!(f.launched());
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

/// Amendment 107: the root helper's launcher child gets EXACTLY the environment and the
/// flags the helper builds. Round 11 edited, one at a time, with the whole suite green:
/// the helper's PATH constant prefixed with `/tmp:` (a root child resolving `jailer`'s
/// `cp`/`mount`/`ip` from a directory any uid can write), the `--timeout-s` flag renamed,
/// and the `--fc-bin` / `--jailer-bin` values swapped. All of them went through
/// `sealed_exec::command(.., &args, &env, ..)`, whose arguments are `vec![..]` and a const,
/// not a builder call. The child here dumps its own argv and its initial environ; this test
/// compares both to the exact expectation, for the launch and for the verify step.
#[test]
fn the_root_helper_hands_its_launcher_exactly_its_flags_and_its_path() {
    let f = fx(None, "", |_| {});
    let cfg: Value = serde_json::from_str(&std::fs::read_to_string(&f.cfg).unwrap()).unwrap();
    let request = f.request("op-1");
    let manifest_sha = request["psv_manifest_sha256"].as_str().unwrap().to_string();
    let (code, rep) = f.run(&request, None);
    assert!(
        code == Some(0) && f.launched(),
        "setup: the launch did not complete: {code:?} {rep}"
    );
    let read = |n: &str| std::fs::read_to_string(f.base.join(n)).unwrap();
    let want_env = "PATH=/usr/sbin:/usr/bin:/sbin:/bin\n";
    for n in ["environ-launch", "environ-verify"] {
        let got = read(n);
        assert!(
            got == want_env,
            "ATTACK: the root helper's {n} child ran with an environment other than exactly \
             {want_env:?}: {got:?}"
        );
    }
    let argv = read("argv-launch");
    let a: Vec<&str> = argv.lines().collect();
    let names: Vec<&str> = a.iter().step_by(2).copied().collect();
    let want_names = [
        "--policy",
        "--psv-candidate",
        "--psv-suite",
        "--psv-job",
        "--psv-manifest-sha",
        "--out",
        "--manifest",
        "--artifacts-dir",
        "--fc-bin",
        "--jailer-bin",
        "--timeout-s",
        "--id",
    ];
    assert!(
        names == want_names,
        "ATTACK: the root helper's launcher was started with the flags {names:?}, not exactly \
         {want_names:?}"
    );
    let value = |flag: &str| {
        let i = a.iter().position(|x| *x == flag).unwrap();
        a[i + 1].to_string()
    };
    for (flag, want) in [
        ("--artifacts-dir", cfg["artifacts_dir"].as_str().unwrap()),
        ("--fc-bin", cfg["firecracker"].as_str().unwrap()),
        ("--jailer-bin", cfg["jailer"].as_str().unwrap()),
        ("--timeout-s", "60"),
        ("--id", "fab-0123456789abcdef"),
    ] {
        let got = value(flag);
        assert!(
            got == want,
            "ATTACK: the root helper handed its launcher {flag} {got:?}, not {want:?}"
        );
    }
    let got = value("--psv-manifest-sha");
    assert!(
        got == manifest_sha,
        "ATTACK: the root helper handed its launcher --psv-manifest-sha {got:?}, not the \
         request's {manifest_sha}"
    );
    for flag in ["--out", "--manifest"] {
        let got = value(flag);
        assert!(
            got.starts_with("/dev/fd/"),
            "ATTACK: the root helper handed its launcher {flag} {got:?}, not a descriptor it holds"
        );
    }
    for (flag, leaf) in [
        ("--policy", "/policy.json"),
        ("--psv-candidate", "/candidate"),
        ("--psv-suite", "/check"),
        ("--psv-job", "/job"),
    ] {
        let got = value(flag);
        assert!(
            got.ends_with(leaf) && got.contains("helper-staging"),
            "ATTACK: the root helper handed its launcher {flag} {got:?}, not its staged copy \
             (…/helper-staging/…{leaf})"
        );
    }
    let v = read("argv-verify");
    let v: Vec<&str> = v.lines().collect();
    assert!(
        v.len() == 2 && v[0] == "--verify-result" && v[1].starts_with("/dev/fd/"),
        "ATTACK: the root helper's verify step was started with {v:?}, not exactly \
         [--verify-result, /dev/fd/N]"
    );
}

/// Amendment 107, ROOT ONLY: the INTERPRETER the root helper runs the launcher under is
/// opened with the operator's uid as the required owner too (`prepare`'s second
/// `open_verified`). Round 11 replaced it by `None` while the launcher's stayed, the suite
/// green: the existing test makes only the LAUNCHER another uid's. A bash another uid owns
/// can be rewritten after the digest is read and before the root exec.
#[test]
fn an_interpreter_another_uid_owns_launches_nothing_as_root() {
    if skip_unless_root() {
        return;
    }
    let mk = |owner: u32| {
        fx(None, "", move |v| {
            let base = Path::new(v["launcher"]["path"].as_str().unwrap())
                .parent()
                .unwrap()
                .to_path_buf();
            let b = base.join("bash-copy");
            copy_executable("/bin/bash", &b, 0o755);
            chown(&b, owner);
            v["interpreter"] = json!({"path": b, "sha256": sha256_file(&b)});
        })
    };
    let c = mk(0);
    let (code, rep) = c.run(&c.request("op-1"), None);
    assert!(
        code == Some(0) && c.launched(),
        "control: an operator-owned interpreter runs the launcher: {code:?} {rep}"
    );
    let f = mk(OTHER);
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: the helper ran its launcher under an interpreter owned by uid {OTHER}, who can \
         rewrite it after it is verified: {code:?} {rep}"
    );
}

/// Amendment 103, ROOT ONLY: the launcher program the operator pinned is opened
/// with the operator's uid as the required owner (`prepare`'s `Some(a.operator_uid)`),
/// the same use-time re-check as for the boot inputs above. Round 11's survey replaced
/// it by `None` ("any owner") with the whole suite green: only the firecracker binary
/// was ever made another uid's. Control: the same launcher, owned by the operator.
#[test]
fn a_launcher_program_another_uid_owns_launches_nothing() {
    if skip_unless_root() {
        return;
    }
    let c = fx(None, "", |_| {});
    let (code, rep) = c.run(&c.request("op-1"), None);
    assert!(
        code == Some(0) && c.launched(),
        "control: the operator's own launcher runs: {code:?} {rep}"
    );
    let f = fx(None, "", |_| {});
    chown(&f.base.join("launcher.sh"), OTHER);
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: the helper ran a launcher program owned by uid {OTHER}, who can rewrite it \
         after it is verified: {code:?} {rep}"
    );
}

/// Amendment 103, ROOT ONLY: the hand-over gives the out tree to the Fabric uid and
/// changes NOTHING ELSE about who owns it. `fchown(fd, uid, u32::MAX)` leaves the group
/// as the launcher left it; each of the four calls (a directory, a regular file, a
/// symlink never followed, the out dir itself) had its group argument replaced by 0 with
/// the whole suite green, because every file the launcher made was already group 0. The
/// launcher here puts every kind in group 4300.
#[test]
fn the_hand_over_changes_the_owner_of_the_out_tree_and_never_its_group() {
    if skip_unless_root() {
        return;
    }
    use std::os::unix::fs::MetadataExt;
    let f = fx(
        Some(FABRIC),
        "mkdir \"$OUT/g-dir\"; echo x > \"$OUT/g-dir/g-in\"; echo x > \"$OUT/g-file\"; \
         ln -s g-file \"$OUT/g-link\"\n\
         chgrp 4300 \"$OUT\" \"$OUT/g-dir\" \"$OUT/g-file\" && chgrp -h 4300 \"$OUT/g-link\"",
        |_| {},
    );
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert!(
        code == Some(0) && f.launched(),
        "setup: the launch did not complete: {code:?} {rep}"
    );
    let out = f.out_root.join("op-1");
    for (what, p) in [
        ("the out dir itself", out.clone()),
        ("a directory", out.join("g-dir")),
        ("a regular file", out.join("g-file")),
        ("a symlink", out.join("g-link")),
    ] {
        let m = std::fs::symlink_metadata(&p).unwrap();
        assert_eq!(
            m.uid(),
            FABRIC,
            "ATTACK: the hand-over left {what} owned by uid {}, not the Fabric uid",
            m.uid()
        );
        assert_eq!(
            m.gid(),
            4300,
            "ATTACK: the hand-over changed the GROUP of {what} to {} (it must leave it as the \
             launcher set it)",
            m.gid()
        );
    }
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
        // Test PROCESSES running at once (a sharded suite) share `target`, and
        // cargo REPLACES a binary whenever it relinks it -- which any change
        // in the tree (even a touched `.git/index`) causes, because the
        // build script watches the whole tree. A later process's build would
        // then remove or swap what an earlier one is executing. So the build
        // runs under an exclusive lock, and each process runs its OWN copies
        // of what cargo just built, which no later build touches.
        std::fs::create_dir_all(&target).unwrap();
        let lock = std::fs::File::create(target.join(".build.lock")).unwrap();
        lock.lock().unwrap();
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
                "--bin",
                "axon-observer",
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
        let built = target.join("debug");
        let per_process = target.join("per-process");
        if let Ok(entries) = std::fs::read_dir(&per_process) {
            for e in entries.flatten() {
                let pid = e.file_name().to_string_lossy().into_owned();
                if !Path::new("/proc").join(&pid).exists() {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        let bin = per_process.join(std::process::id().to_string());
        std::fs::create_dir_all(&bin).unwrap();
        for name in [
            "axon-protected-launcher",
            "axon-fabric",
            "axon-custodian",
            "axon-observer",
        ] {
            // A child `cp`, so this process holds no write descriptor to the
            // copy (ETXTBSY on a later exec).
            let st = Command::new("cp")
                .arg("--preserve=mode,timestamps")
                .arg("--")
                .arg(built.join(name))
                .arg(bin.join(name))
                .status()
                .unwrap();
            assert!(st.success(), "setup: copying {name} failed");
        }
        drop(lock);
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
            // Amendment 79: a protected custodian names the observer's own uid.
            "observer_uid": OBSERVER,
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
            // Amendment 79: the Fabric program is the shell that runs the
            // helper as the Fabric uid (every production helper run below
            // goes through `sh -c`, which stays the helper's parent).
            "fabric": shell_fabric_pin(&"0".repeat(40)),
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
            "custodian": {"socket": socket, "uid": CUSTODIAN,
                          "sha256": sha256_file(&bin.join("axon-custodian"))},
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

/// How long a namespace script waits for the custodian's activation socket
/// (polls of 50 ms; >= 180 s), and how long a client waits for its reply.
/// They were 10 s and 20 s, which a loaded host exceeded: under the 6-shard
/// paired-disable the socket appeared after the wait (the helper then found
/// no custodian, "No such file or directory") or the custodian was stopped
/// before it answered ("Terminated", no refusal written), failing CONTROLS
/// and leaving the attack assertions able to pass on a custodian that never
/// ran. Reproduced by delaying the activation 12 s (C9 round 4b,
/// integrate-3). A socket that never appears is now a SETUP failure, never a
/// verdict; a healthy run pays nothing for the longer bounds.
const ACTIVATION_POLLS: u32 = 3600;
const CLIENT_TIMEOUT_S: u32 = 180;

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
         n=0; while [ ! -S {sock} ] && [ $n -lt {ACTIVATION_POLLS} ]; do sleep 0.05; n=$((n+1)); done\n\
         [ -S {sock} ] || {{ echo \"setup: the activation socket {sock} never appeared\" >&2; \
         pkill -f \"$1/axon-custodian\" 2>/dev/null; exit 3; }}\n\
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
             --ambient-caps={caps} -- sh -c 'o=$(\"$0\"); c=$?; printf \"%s\\n\" \"$o\"; exit $c' \
             \"$1/axon-protected-launcher\" \
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
             sh -c 'o=$(/etc/axon/h); c=$?; printf \"%s\\n\" \"$o\"; exit $c' < \"$1/request.json\" > \"$1/control.json\"\n\
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
    // custodian refusing a spend from a uid other than 0 (M628), or by the
    // helper's verification of the pinned custodian program, which a helper
    // that is not root cannot perform (it cannot open the custodian's
    // /proc/<pid>/exe; M1489, amendment 66): any of these (four-cell record).
    // Any OTHER refusal means the fixture did not reach the spend, and
    // proves nothing.
    assert!(
        rep.contains("not 0")
            || rep.contains("is not the launcher uid 0")
            || rep
                .contains("is not served by the pinned custodian program: the sender's executable"),
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
             s.settimeout({CLIENT_TIMEOUT_S})\n\
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
/// The observer service's uid in a protected custodian config (amendment 79).
const OBSERVER: u32 = 4245;

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
         s.settimeout({CLIENT_TIMEOUT_S})\n\
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
        // Amendment 68: a production host observes through the observer
        // service (the helper's relay); it names no observer program.
        "observer": {"custodian": {"socket": e.join("run/custodian.sock"), "uid": CUSTODIAN}},
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

/// A94 on `ProtectedHost::operator()` (amendment 68, M1547), ROOT ONLY: a
/// production Fabric refuses a protected host config naming an observer
/// PROGRAM. Fabric would run it as its own uid, so its signing key would be
/// readable by the principal the observation constrains. Only this rule
/// refuses it (the program is pinned, operator-owned and present). Control:
/// the conforming host, which observes through the service.
#[test]
fn a_production_fabric_refuses_an_observer_program_on_a_protected_host() {
    if !root_with_etc_axon() {
        return;
    }
    a_conforming_production_host_is_accepted();
    let d = production_host_etc(
        |v| {
            v["observer"]["command"] = json!({"path": "/etc/axon/observer.sh",
                                              "sha256": axon_psv::sha256_hex(b"#!/bin/sh\n")})
        },
        |_| {},
    );
    let (code, rep) = fabric_on_production_host(d.path(), FABRIC);
    assert!(
        code == "4",
        "ATTACK: a production Fabric accepted a protected host whose observer is a program run \
         as the Fabric uid: exit {code} {rep}"
    );
    assert!(rep.contains("observer.command"), "{rep}");
}

/// A94 (amendment 68, M1548), PRODUCTION BUILD: the production observer
/// reads its config only from the operator's fixed path; `--test-config`
/// exists in test-trust builds alone. Offered a complete, valid test config
/// (its key, store and trust root in place), it refuses and never listens.
#[test]
fn a_production_observer_never_takes_its_config_from_a_path_its_caller_names() {
    let bin = production_build().join("axon-observer");
    let d = tempfile::tempdir().unwrap();
    let base = d.path();
    let store = base.join("records");
    std::fs::create_dir(&store).unwrap();
    set_mode(&store, 0o700);
    let key = observer_key(base, "observer", &[&observer_root(base)]);
    set_mode(&key.pk8, 0o400);
    let sock = base.join("observer.sock");
    let cfg = base.join("observer.json");
    let me = euid();
    std::fs::write(
        &cfg,
        json!({"schema": "axon-observer/1", "observer_uid": me, "fabric_uid": me,
               "caller_uid": me, "socket": sock, "store": store, "key_path": key.pk8,
               "test_paths": {"trust_root": observer_root(base),
                              "host_config": base.join("h.json"),
                              "helper_config": base.join("l.json")}})
        .to_string(),
    )
    .unwrap();
    let mut child = Command::new(&bin)
        .arg("--test-config")
        .arg(&cfg)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut code = None;
    for _ in 0..400 {
        if let Some(s) = child.try_wait().unwrap() {
            code = s.code();
            break;
        }
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let listening = sock.exists();
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        code == Some(2) && !listening,
        "ATTACK: a production observer took its config from a path its caller named \
         (--test-config) and served: exit {code:?}, listening {listening}"
    );
}

/// A94 (amendment 68, M1550; integration of amendments 68 and 71), ROOT ONLY,
/// PRODUCTION BUILD: the protected observer serves only from a record store
/// whose PARENT chain is the operator's, as the custodian does (M641). The
/// store itself is the observer's own 0700 directory (M1525), so a store
/// placed under a directory the Fabric uid owns passes that check, and the
/// Fabric uid could rename the store away and put back one of its own (an
/// empty record set, so a nonce observes twice). `axon-observer` with no
/// arguments reads /etc/axon/observer.json, runs socket-activated as its own
/// uid with its 0400 key trusted in the operator observer root; asked by
/// root (its only caller), it must answer nothing when the store's parent is
/// the Fabric uid's. Control: the same observer with the store under an
/// operator-owned parent answers (here, a refusal of a request of another
/// schema, which proves it served).
#[test]
fn a_protected_observer_serves_only_from_a_store_the_operator_placed() {
    if !root_with_etc_axon() {
        return;
    }
    const OBSERVER: u32 = 4245;
    let bin = production_build();
    for (parent, agent_owned) in [("agent", true), ("observer-state", false)] {
        let d = tempfile::tempdir_in("/var/tmp").unwrap();
        let s = d.path();
        set_mode(s, 0o755);
        copy_executable(bin.join("axon-observer"), s.join("axon-observer"), 0o755);
        let t = s.join("etc");
        std::fs::create_dir_all(t.join(parent).join("records")).unwrap();
        std::fs::create_dir_all(t.join("run")).unwrap();
        std::fs::create_dir_all(t.join("observer-key")).unwrap();
        let key = observer_key(
            &t.join("observer-key"),
            "observer",
            &[&t.join("trust/observer")],
        );
        let e = Path::new("/etc/axon");
        std::fs::write(
            t.join("observer.json"),
            json!({
                "schema": "axon-observer/1", "observer_uid": OBSERVER, "fabric_uid": FABRIC,
                "caller_uid": 0, "socket": e.join("run/observer.sock"),
                "store": e.join(parent).join("records"),
                "key_path": e.join("observer-key").join(key.pk8.file_name().unwrap()),
            })
            .to_string(),
        )
        .unwrap();
        let own = if agent_owned {
            format!("chown {FABRIC}:{FABRIC} /etc/axon/{parent}\n")
        } else {
            String::new()
        };
        let script = format!(
            "set -e\n\
             mount -t tmpfs -o mode=0755 tmpfs /etc/axon\n\
             cp -a \"$1/etc/.\" /etc/axon/\n\
             chown -R 0:0 /etc/axon\n\
             chmod -R go-w /etc/axon\n\
             chown {OBSERVER}:{OBSERVER} /etc/axon/{parent}/records /etc/axon/observer-key/observer.pk8\n\
             chmod 0700 /etc/axon/{parent}/records\n\
             chmod 0400 /etc/axon/observer-key/observer.pk8\n\
             {own}\
             {{ systemd-socket-activate -l /etc/axon/run/observer.sock setpriv --reuid={OBSERVER} \
             --regid={OBSERVER} --clear-groups -- \"$1/axon-observer\"; \
             echo $? > \"$1/observer.code\"; }} 2> \"$1/observer.err\" &\n\
             C=$!\n\
             set +e\n\
             n=0; while [ ! -S /etc/axon/run/observer.sock ] && [ $n -lt {ACTIVATION_POLLS} ]; do sleep 0.05; n=$((n+1)); done\n\
             [ -S /etc/axon/run/observer.sock ] || {{ echo \"setup: the activation socket never appeared\" >&2; \
             pkill -f \"$1/axon-observer\" 2>/dev/null; exit 3; }}\n\
             python3 -c '\n\
             import socket, sys\n\
             s = socket.socket(socket.AF_UNIX)\n\
             s.settimeout({CLIENT_TIMEOUT_S})\n\
             s.connect(\"/etc/axon/run/observer.sock\")\n\
             s.sendall(b\"{{\\\"schema\\\":\\\"not-an-observer-request\\\"}}\\n\")\n\
             sys.stdout.write(s.recv(65536).decode())\n\
             ' > \"$1/reply.json\" 2>/dev/null\n\
             pkill -f \"$1/axon-observer\" 2>/dev/null; wait $C 2>/dev/null\n\
             exit 0\n"
        );
        let st = Command::new("unshare")
            .args(["-m", "--propagation", "private", "sh", "-c", &script, "sh"])
            .arg(s)
            .status()
            .unwrap();
        assert!(st.success(), "setup: the namespace script failed");
        let reply = std::fs::read_to_string(s.join("reply.json")).unwrap_or_default();
        let err = std::fs::read_to_string(s.join("observer.err")).unwrap_or_default();
        if agent_owned {
            assert!(
                !reply.contains("axon-observer-reply/1"),
                "ATTACK: a protected observer served from a store whose parent the Fabric uid \
                 owns: {reply}"
            );
            assert!(err.contains("not root"), "{err}");
        } else {
            assert!(
                reply.contains("axon-observer-reply/1") && reply.contains("\"mode\":\"protected\""),
                "control: the protected observer serves from an operator-placed store: {reply} \
                 ({err})"
            );
        }
    }
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
             sh -c 'o=$(/etc/axon/h); c=$?; printf \"%s\\n\" \"$o\"; exit $c' < \"$1/request.json\" > \"$1/report.json\"\n\
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
        // Ends when it binds or EXITS (a refused start, at once); the bound
        // is a setup bound only (it was 5 s, which a loaded host exceeded:
        // the control then read as "did not serve").
        wait_until(SETUP_BOUND, || {
            sock.exists() || child.try_wait().unwrap().is_some()
        });
        let bound = sock.exists();
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

// ── C9 round 4 fix wave, ROWS2 wave 2, STRICT (rows M795-M814) ──────────────
//
// Refusal sites that were exempt as "dominated" (or with no stated kind).
// Each gets an attack that reaches it ALONE (an ACTIVE row), or the attack its
// EQUIVALENT_DID four-cell record is executed with (retired, never counted
// killed).

/// A request with the fixture's genuine observation, `edit` applied.
fn edited_request(f: &Fx, out: &str, edit: impl FnOnce(&mut Value)) -> Value {
    let mut r = f.request(out);
    edit(&mut r);
    r
}

/// A (M795): the helper takes one request schema. A request of another
/// schema, every field of the current one genuine, launches nothing.
/// Control: the same request with the current schema launches.
#[test]
fn a_request_of_another_schema_launches_nothing() {
    let f = fx(None, "", |_| {});
    let r = edited_request(&f, "op-1", |r| {
        r["schema"] = json!("axon-protected-launch-request/9")
    });
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper launched a request of another schema: {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("request schema is not"),
        "{rep}"
    );
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A (M796, retired EQUIVALENT_DID against M623 and M797; the attack of its
/// four-cell record): the request's `psv_manifest_sha256` is the snapshot
/// manifest's digest spelled in UPPER case, with an observation of that same
/// spelling. Three checks refuse it, each alone: the request's own format
/// rule (M796), the snapshot manifest's digest compared with it (M623), and
/// the custodian's rule that a spend names a lowercase sha256 (M797). Any
/// reason; only a launch is the attack. Control: the genuine request.
#[test]
fn a_request_naming_its_manifest_digest_in_another_spelling_launches_nothing() {
    let f = fx(None, "", |_| {});
    let up = axon_psv::sha256_hex(&f.manifest).to_uppercase();
    let mut o = observation_of(&f.manifest, &f.observer.key_id(), 0);
    o["intended_launch_manifest_sha256"] = json!(up);
    let o = serde_json::to_vec(&o).unwrap();
    let r = edited_request(&f, "op-u", |r| {
        r["psv_manifest_sha256"] = json!(up);
        r["observation"] = json!(String::from_utf8(o.clone()).unwrap());
        r["observation_signature"] = json!(f
            .observer
            .sign_for(axon_fabric::backend::TrustAuthority::Observer, &o));
    });
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper launched a request whose manifest digest is not a lowercase \
         sha256: {code:?} {rep}"
    );
    let f = fx(None, "", |_| {});
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A (M798): the request's timeout is bounded by the operator's
/// `max_timeout_s` (3600 in the fixture): a request asking for more launches
/// nothing (the launcher would be run with the Fabric's ceiling instead of
/// the operator's). Control: the operator's maximum itself launches.
#[test]
fn a_request_asking_for_more_time_than_the_operator_allows_launches_nothing() {
    let f = fx(None, "", |_| {});
    let r = edited_request(&f, "op-t", |r| r["timeout_s"] = json!(3601));
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper launched with a timeout above the operator's max_timeout_s: \
         {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap_or("").contains("timeout_s"),
        "{rep}"
    );
    let r = edited_request(&f, "op-1", |r| r["timeout_s"] = json!(3600));
    let (code, rep) = f.run(&r, None);
    assert_eq!(code, Some(0), "control: {rep}");
}

/// The fixture with the manifest naming `policy` (and a genuine observation
/// of that manifest), `policy` put beside the job.
fn fx_with_policy(policy: Vec<u8>) -> Fx {
    let f = fx(None, "", |_| {});
    let mut m: Value = serde_json::from_slice(&f.manifest).unwrap();
    m["policy_sha256"] = json!(axon_psv::sha256_hex(&policy));
    let manifest = serde_json::to_vec(&m).unwrap();
    let observation =
        serde_json::to_vec(&observation_of(&manifest, &f.observer.key_id(), 0)).unwrap();
    let f = Fx {
        manifest,
        observation,
        policy,
        ..f
    };
    f.put_inputs();
    f
}

/// A (M799): the guest policy the root helper copies is at most 64 KiB. One
/// byte more, even when it IS the manifest's policy and the observation of
/// that manifest is genuine, launches nothing. Control: exactly 64 KiB
/// launches (the padding is valid JSON whitespace; only the size refuses).
#[test]
fn a_policy_over_the_helpers_bound_launches_nothing() {
    let padded = |n: usize| {
        let mut p = TEST_GUEST_POLICY.as_bytes().to_vec();
        p.resize(n, b' ');
        p
    };
    let f = fx_with_policy(padded((64 << 10) + 1));
    let (code, rep) = f.run(&f.request("op-big"), None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper launched a policy over its 64 KiB bound: {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap_or("").contains("too large"),
        "{rep}"
    );
    let f = fx_with_policy(padded(64 << 10));
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A (M800): every psv input is spelled exactly `<out root>/<inputs>/<leaf>`.
/// A candidate path naming the same directory in another spelling (`./`, a
/// doubled or trailing `/`) launches nothing, although the snapshot would
/// read the same files. Control: the plain spelling launches.
#[test]
fn a_psv_input_spelled_another_way_launches_nothing() {
    let f = fx(None, "", |_| {});
    let i = f.out_root.join(INPUTS);
    for spelled in [
        format!("{}/./candidate", i.display()),
        format!("{}//candidate", i.display()),
        format!("{}/candidate/", i.display()),
    ] {
        let r = edited_request(&f, "op-s", |r| r["psv_candidate"] = json!(spelled));
        let (code, rep) = f.run(&r, None);
        assert!(
            code == Some(30) && f.launches() == 0,
            "ATTACK: the root helper launched a request whose candidate is spelled {spelled}: \
             {code:?} {rep}"
        );
    }
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A (M801, retired EQUIVALENT_DID against M800; the attack of its four-cell
/// record): a psv input naming another leaf of the inputs dir. The leaf-name
/// rule (M801) and the exact-spelling rule (M800) each refuse it alone.
/// Control: the plain request.
#[test]
fn a_psv_input_naming_another_leaf_launches_nothing() {
    let f = fx(None, "", |_| {});
    let other = f.out_root.join(INPUTS).join("shadow");
    let r = edited_request(&f, "op-l", |r| r["psv_candidate"] = json!(other));
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper launched a request whose candidate names another leaf: \
         {code:?} {rep}"
    );
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A (M804, retired EQUIVALENT_DID against M802; the attack of its four-cell
/// record): the request's out dir IS its inputs dir. The out==inputs rule
/// (M804) and the out dir's must-be-new rule (M802: the inputs dir exists)
/// each refuse it alone. Control: the plain request.
#[test]
fn an_out_dir_that_is_the_inputs_dir_launches_nothing() {
    let f = fx(None, "", |_| {});
    let r = edited_request(&f, INPUTS, |_| {});
    let (code, rep) = f.run(&r, None);
    assert!(
        code == Some(30) && f.launches() == 0,
        "ATTACK: the root helper launched into its own psv inputs dir: {code:?} {rep}"
    );
    let f = fx(None, "", |_| {});
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A (M802), ROOT ONLY: the root launcher writes only into an out dir the
/// helper CREATED. A root-owned directory already at the out path (a helper
/// out dir whose hand-over failed, holding what that launch left) is never
/// launched into: the must-be-new rule is the only check, since the dir is
/// root's and the owner re-check after it would pass. Control: a new out dir.
#[test]
fn a_root_owned_out_dir_that_already_exists_is_never_launched_into() {
    if skip_unless_root() {
        return;
    }
    let f = fx(Some(FABRIC), "", |_| {});
    let stale = f.out_root.join("op-1");
    std::fs::create_dir(&stale).unwrap();
    set_mode(&stale, 0o700);
    std::fs::write(stale.join("left-by-an-earlier-launch"), "x").unwrap();
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: the root helper launched into an out dir that already existed: {code:?} {rep}"
    );
    assert!(
        rep["error"].as_str().unwrap_or("").contains("must be new"),
        "{rep}"
    );
    // The out dir is made after the nonce is spent: a fresh launch.
    let f = fx(Some(FABRIC), "", |_| {});
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A (M803, retired EQUIVALENT_DID against M802; the attack of its four-cell
/// record), ROOT ONLY: the Fabric uid put its own directory at the out path
/// before the launch. The must-be-new rule (M802) and the owner re-check
/// after the open (M803: the dir is not root's) each refuse it alone.
/// Control: a new out dir.
#[test]
fn a_fabric_owned_out_dir_that_already_exists_is_never_launched_into() {
    if skip_unless_root() {
        return;
    }
    let f = fx(Some(FABRIC), "", |_| {});
    let planted = f.out_root.join("op-1");
    std::fs::create_dir(&planted).unwrap();
    chown(&planted, FABRIC);
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: the root helper launched into an out dir the Fabric uid made: {code:?} {rep}"
    );
    // The out dir is made after the nonce is spent: a fresh launch.
    let f = fx(Some(FABRIC), "", |_| {});
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert_eq!(code, Some(0), "control: {rep}");
}

/// A (M805), ROOT ONLY: the helper reads a psv input only from a directory the
/// Fabric uid owns. Here the candidate DIRECTORY is root's (its files stay
/// the Fabric's, so the per-file owner rule, M538, passes): only the leaf
/// owner rule refuses it. Control: the Fabric's own directory launches.
#[test]
fn a_root_owned_candidate_directory_is_never_read_by_the_helper() {
    if skip_unless_root() {
        return;
    }
    let f = fx(Some(FABRIC), "", |_| {});
    std::os::unix::fs::lchown(f.out_root.join(INPUTS).join("candidate"), Some(0), Some(0)).unwrap();
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: the root helper read a root-owned candidate directory and launched: \
         {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("psv input candidate is owned by uid 0"),
        "{rep}"
    );
    let f = fx(Some(FABRIC), "", |_| {});
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert_eq!(code, Some(0), "control: {rep}");
}

/// Sh for one request `body` (a JSON line) as `uid` to the custodian at
/// `sock`, the reply to `$1/<out>`.
fn ask_custodian(uid: u32, sock: &str, body: &str, out: &str) -> String {
    let body = body.replace('"', "\\\"");
    format!(
        "setpriv --reuid={uid} --regid={uid} --clear-groups -- python3 -c '\n\
         import socket, sys\n\
         s = socket.socket(socket.AF_UNIX)\n\
         s.settimeout({CLIENT_TIMEOUT_S})\n\
         s.connect(\"{sock}\")\n\
         s.sendall(b\"{body}\\n\")\n\
         sys.stdout.write(s.recv(4096).decode())\n\
         ' > \"$1/{out}\" 2>/dev/null\n"
    )
}

/// D6 on the PRODUCTION custodian (M806), ROOT ONLY: a request of another
/// schema is answered with a refusal, never with a nonce. Control: the same
/// request with the current schema is served.
#[test]
fn a_protected_custodian_answers_no_request_of_another_schema() {
    if !root_with_etc_axon() {
        return;
    }
    let d = production_etc_with_custodian(|_| {});
    let s = d.path();
    let issue = |schema: &str| json!({"schema": schema, "op": "issue", "epoch": 0}).to_string();
    in_production_etc(
        s,
        "custodian",
        &format!(
            "{}{}",
            ask_custodian(
                FABRIC,
                CUSTODIAN_SOCK,
                &issue("axon-custodian-request/9"),
                "attack.json"
            ),
            ask_custodian(
                FABRIC,
                CUSTODIAN_SOCK,
                &issue("axon-custodian-request/1"),
                "control.json"
            ),
        ),
    );
    let read = |n: &str| std::fs::read_to_string(s.join(n)).unwrap_or_default();
    assert!(
        read("control.json").contains("\"ok\":true"),
        "control: {} ({})",
        read("control.json"),
        read("custodian.err")
    );
    let reply = read("attack.json");
    assert!(
        !reply.contains("\"ok\":true"),
        "ATTACK: a protected custodian answered a request of another schema: {reply}"
    );
    assert!(reply.contains("request schema is not"), "{reply}");
}

/// A84 on the PRODUCTION custodian (M797), ROOT ONLY: a spend must name the
/// launch manifest it is spent for, as a lowercase sha256 (the record the
/// custodian keeps of which launch spent the nonce). A spend by the launcher
/// uid (root) naming anything else spends nothing. Control: the same spend
/// naming a sha256 is served.
#[test]
fn a_protected_custodian_spends_nothing_for_a_spend_naming_no_manifest() {
    if !root_with_etc_axon() {
        return;
    }
    let nonce = "0123456789abcdef0123456789abcdef";
    let spend = |manifest: &str| {
        json!({"schema": "axon-custodian-request/1", "op": "spend", "epoch": 0,
               "nonce": nonce, "manifest_sha256": manifest})
        .to_string()
    };
    let d = production_etc_with_custodian(|_| {});
    let s = d.path();
    in_production_etc(
        s,
        "custodian",
        &ask_custodian(0, CUSTODIAN_SOCK, &spend(&"A".repeat(64)), "attack.json"),
    );
    let reply = std::fs::read_to_string(s.join("attack.json")).unwrap_or_default();
    assert!(
        !reply.contains("\"ok\":true"),
        "ATTACK: a protected custodian spent a nonce on a spend naming no launch manifest: \
         {reply}"
    );
    assert!(reply.contains("names no launch manifest"), "{reply}");
    let d = production_etc_with_custodian(|_| {});
    let s = d.path();
    in_production_etc(
        s,
        "custodian",
        &ask_custodian(0, CUSTODIAN_SOCK, &spend(&"a".repeat(64)), "control.json"),
    );
    let reply = std::fs::read_to_string(s.join("control.json")).unwrap_or_default();
    assert!(
        reply.contains("\"ok\":true"),
        "control: {reply} ({})",
        std::fs::read_to_string(s.join("custodian.err")).unwrap_or_default()
    );
}

/// D6 (M813, retired EQUIVALENT_DID against M325; the attack of its
/// four-cell record), ROOT ONLY: the production custodian's store is a
/// SYMLINK (owned by the custodian uid) to its real 0700 store. The
/// not-a-directory rule (M813) and the no-group/other-access rule (M325: a
/// symlink's lstat mode is 0777 on Linux) each refuse it alone. Control: the
/// store itself serves.
#[test]
fn a_protected_custodian_never_serves_from_a_symlinked_store() {
    if !root_with_etc_axon() {
        return;
    }
    let d = production_etc_with_custodian(|v| v["store"] = json!("/etc/axon/custodian/link"));
    let s = d.path();
    std::os::unix::fs::symlink("nonces", s.join("etc/custodian/link")).unwrap();
    // The namespace script chowns the tree to root; the link must be the
    // custodian's (else its owner rule, M630, would refuse it first). Through
    // lchown(2) itself: this host's `chown -h` (uutils 0.8) leaves a symlink's
    // owner unchanged and exits 0 (measured).
    let script = format!(
        "python3 -c 'import os; os.lchown(\"/etc/axon/custodian/link\", {CUSTODIAN}, {CUSTODIAN})'\n{}",
        ask_issue(FABRIC, CUSTODIAN_SOCK, "reply.json")
    );
    in_production_etc_before_custodian(s, "custodian", &script);
    let reply = std::fs::read_to_string(s.join("reply.json")).unwrap_or_default();
    assert!(
        !reply.contains("\"ok\":true"),
        "ATTACK: a protected custodian served from a store that is a symlink: {reply}"
    );
    let d = production_etc_with_custodian(|_| {});
    let s = d.path();
    in_production_etc(
        s,
        "custodian",
        &ask_issue(FABRIC, CUSTODIAN_SOCK, "reply.json"),
    );
    let reply = std::fs::read_to_string(s.join("reply.json")).unwrap_or_default();
    assert!(reply.contains("\"ok\":true"), "control: {reply}");
}

/// [`in_production_etc`], with the first line of `body` (up to its first
/// newline) run as root BEFORE the custodian is started, the rest after.
fn in_production_etc_before_custodian(s: &Path, store_parent: &str, body: &str) {
    let (pre, rest) = body.split_once('\n').unwrap_or((body, ""));
    let sock = CUSTODIAN_SOCK;
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
         {pre}\n\
         {{ systemd-socket-activate -l {sock} setpriv --reuid={CUSTODIAN} \
         --regid={CUSTODIAN} --clear-groups -- \"$1/axon-custodian\"; \
         echo $? > \"$1/custodian.code\"; }} 2> \"$1/custodian.err\" &\n\
         C=$!\n\
         set +e\n\
         n=0; while [ ! -S {sock} ] && [ $n -lt {ACTIVATION_POLLS} ]; do sleep 0.05; n=$((n+1)); done\n\
         [ -S {sock} ] || {{ echo \"setup: the activation socket {sock} never appeared\" >&2; \
         pkill -f \"$1/axon-custodian\" 2>/dev/null; exit 3; }}\n\
         chmod 0666 {sock}\n\
         {rest}\n\
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

/// The trust preflight's probe list (M807, M808): `axon-fabric
/// protected-host-paths` lists exactly what `ProtectedHost::load` enforces, so
/// a host config `load` refuses gets no list either: one naming a relative
/// path (M807), or of another schema (M808). Control: the production host's
/// three configs list.
#[test]
fn the_probe_list_is_refused_for_a_host_config_load_refuses() {
    let paths = |edit: fn(&mut Value)| {
        let d = production_host_etc(edit, |_| {});
        let t = d.path().join("etc");
        let o = Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
            .arg("protected-host-paths")
            .arg("--config")
            .arg(t.join("protected-host.json"))
            .arg("--launcher-config")
            .arg(t.join("protected-launcher.json"))
            .arg("--custodian-config")
            .arg(t.join("custodian.json"))
            .output()
            .unwrap();
        (
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).to_string(),
            String::from_utf8_lossy(&o.stderr).to_string(),
        )
    };
    let (code, out, err) = paths(|_| {});
    assert!(
        code == Some(0) && out.contains("/etc/axon/launcher.sh"),
        "control: {code:?} {out} {err}"
    );
    for (edit, attack, why) in [
        (
            (|v: &mut Value| v["launcher"]["path"] = json!("etc/axon/launcher.sh"))
                as fn(&mut Value),
            "a relative launcher path",
            "is not absolute",
        ),
        (
            |v| v["schema"] = json!("axon-protected-host/0"),
            "a host config of another schema",
            "schema is not",
        ),
    ] {
        let (code, out, err) = paths(edit);
        assert!(
            code != Some(0),
            "ATTACK: the preflight probe list was printed for a host config load refuses \
             ({attack}): {out}"
        );
        assert!(format!("{out}{err}").contains(why), "{why}: {out} {err}");
    }
}

// ── C9 round 4b, workstream GAPS (amendment 65; rows M1470-M1486). ─────────
// The operator kit found three helper gaps: `harden()` had no test and no row
// (the setuid helper's inherited-state resets), the custodian PROGRAM was
// pinned nowhere (the helper trusted whatever its socket's listener ran), and
// a Fabric under NoNewPrivileges made the helper refuse with a misleading
// reason. Each test below drives the real binary.

/// What the stand-in launcher records about ITSELF and about its parent, the
/// root helper (`$PPID`), for the harden tests. Read as root, by the launcher.
const RECORD_STATE: &str = r#"P=$PPID
grep -E '^(SigIgn|SigBlk)' /proc/$P/status > "$OUT/helper-status"
readlink /proc/$P/cwd > "$OUT/helper-cwd"
for f in /proc/$P/fd/*; do readlink "$f"; done > "$OUT/helper-fds" 2>/dev/null
grep -E '^(SigIgn|SigBlk)' /proc/$$/status > "$OUT/launcher-status"
umask > "$OUT/launcher-umask"
cat /proc/$$/limits > "$OUT/launcher-limits"
for f in /proc/$$/fd/*; do readlink "$f"; done > "$OUT/launcher-fds" 2>/dev/null
env > "$OUT/launcher-env"
"#;

/// The hostile caller: as the Fabric uid, set every piece of process state a
/// setuid program inherits, then exec the helper (a process RUNNING as the
/// actor makes the exec, as in [`Fx::run`]).
const HOSTILE_CALLER: &str = r#"
import os, resource, signal, sys
helper, cfg, held, cwd, bash_env = sys.argv[1:6]
# The helper's PARENT is this process, the pinned Fabric program running as the
# Fabric uid (amendment 79); the hostile state is set in the child that execs
# the helper (everything below is inherited across fork, so it is the helper's
# state all the same).
pid = os.fork()
if pid:
    _, st = os.waitpid(pid, 0)
    if os.WIFSIGNALED(st):
        sig = os.WTERMSIG(st)
        signal.signal(sig, signal.SIG_DFL)
        os.kill(os.getpid(), sig)
        signal.pause()
    sys.exit(os.WEXITSTATUS(st))
for s in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
    signal.signal(s, signal.SIG_IGN)
signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGUSR1, signal.SIGALRM, signal.SIGTERM})
os.umask(0)
resource.setrlimit(resource.RLIMIT_FSIZE, (4 << 20, 4 << 20))
resource.setrlimit(resource.RLIMIT_CPU, (3000, 3000))
resource.setrlimit(resource.RLIMIT_NOFILE, (256, 256))
hard = resource.getrlimit(resource.RLIMIT_CORE)[1]
resource.setrlimit(resource.RLIMIT_CORE, (hard, hard))
fd = os.open(held, os.O_RDONLY)
os.dup2(fd, 9, inheritable=True)
os.close(fd)
os.chdir(cwd)
env = {"PATH": cwd, "BASH_ENV": bash_env, "ENV": bash_env, "LD_PRELOAD": "/nonexistent/x.so",
       "HOSTILE_CALLER": "1"}
os.execve(helper, [helper, "--test-config", cfg], env)
"#;

/// A `/proc/<pid>/status` mask line's value.
fn mask(status: &str, field: &str) -> u64 {
    status
        .lines()
        .find_map(|l| l.strip_prefix(field))
        .map(|v| u64::from_str_radix(v.trim(), 16).unwrap())
        .unwrap_or_else(|| panic!("setup: no {field} in {status:?}"))
}

/// A `/proc/<pid>/limits` row's (soft, hard), as written there.
fn limit(limits: &str, row: &str) -> (String, String) {
    let l = limits
        .lines()
        .find(|l| l.starts_with(row))
        .unwrap_or_else(|| panic!("setup: no {row} in {limits:?}"));
    let mut w = l[row.len()..].split_whitespace();
    (w.next().unwrap().into(), w.next().unwrap().into())
}

/// A (amendment 65; M1474-M1482), ROOT ONLY: `harden()` resets, before the
/// config or the request is read, every piece of process state a setuid
/// program inherits from its caller. The caller here is the Fabric uid
/// itself, setting all of it at once: ignored SIGTERM/SIGHUP/SIGINT, a
/// blocked mask, umask 0, lowered FSIZE/CPU/NOFILE limits, a raised core
/// limit, an inherited descriptor, a working directory of its own and a
/// hostile environment. The stand-in launcher records its own state and its
/// parent's (the root helper's) from /proc. Each property is a separate
/// assertion with its own marker; with all resets in place the launch runs
/// (the control: the honest launch still works from a hostile caller).
#[test]
fn a_callers_process_state_never_reaches_the_root_helper_or_its_launcher() {
    if skip_unless_root() {
        return;
    }
    let f = fx(Some(FABRIC), RECORD_STATE, |v| {
        v["fabric"] = python_fabric_pin()
    });
    // What the caller holds open, works in, and points BASH_ENV at: all the
    // Fabric uid's own.
    let held = f.base.join("held-by-caller");
    std::fs::write(&held, "the caller's descriptor\n").unwrap();
    let cwd = f.base.join("caller-cwd");
    std::fs::create_dir(&cwd).unwrap();
    let bash_env = cwd.join("bash_env.sh");
    let bash_env_ran = f.base.join("bash-env-ran");
    std::fs::write(&bash_env, format!("touch {}\n", bash_env_ran.display())).unwrap();
    for p in [&held, &cwd, &bash_env] {
        chown(p, FABRIC);
    }
    let mut child = Command::new("setpriv")
        .args([
            &format!("--reuid={FABRIC}"),
            &format!("--regid={FABRIC}"),
            "--clear-groups",
            "--",
            "python3",
            "-c",
            HOSTILE_CALLER,
        ])
        .arg(f.installed.as_ref().unwrap())
        .arg(&f.cfg)
        .arg(&held)
        .arg(&cwd)
        .arg(&bash_env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(f.request("op-1").to_string().as_bytes())
            .unwrap();
    }
    let o = child.wait_with_output().unwrap();
    let rep = String::from_utf8_lossy(&o.stdout).to_string();
    let out = f.out_root.join("op-1");
    let read = |n: &str| {
        std::fs::read_to_string(out.join(n)).unwrap_or_else(|e| {
            panic!(
                "setup: the launch from a hostile caller did not record {n} ({e}): exit {:?} \
                 {rep} {}",
                o.status.code(),
                String::from_utf8_lossy(&o.stderr)
            )
        })
    };
    assert_eq!(
        read("ruid").trim(),
        "0",
        "control: the launcher ran as root"
    );
    let (helper, launcher) = (read("helper-status"), read("launcher-status"));
    let caller_ignored =
        (1u64 << (libc::SIGTERM - 1)) | (1u64 << (libc::SIGHUP - 1)) | (1u64 << (libc::SIGINT - 1));
    // M1474: dispositions. An ignored signal survives exec, so the root
    // launcher (and the jailer, firecracker) would keep ignoring SIGTERM.
    assert!(
        mask(&launcher, "SigIgn:") & caller_ignored == 0
            && mask(&helper, "SigIgn:") & caller_ignored == 0,
        "ATTACK: the root launcher inherited signal dispositions its caller set (SIGTERM/SIGHUP/\
         SIGINT ignored): launcher {launcher:?} helper {helper:?}"
    );
    // M1481: the helper's own SIGPIPE is ignored (a closed report pipe must
    // not kill it mid-cleanup), whatever the caller left it as.
    assert!(
        mask(&helper, "SigIgn:") & (1u64 << (libc::SIGPIPE - 1)) != 0,
        "ATTACK: the root helper runs with SIGPIPE at its default action: a caller that closes \
         the report pipe kills it between the launch and the hand-over: {helper:?}"
    );
    // M1475: the mask. Blocked signals survive exec; the root helper must
    // answer SIGTERM (a service stop) rather than run on until SIGKILL.
    assert_eq!(
        mask(&helper, "SigBlk:"),
        0,
        "ATTACK: the root helper ran with signals its caller blocked: {helper:?}"
    );
    // M1476: umask. What the root launcher creates would be world-writable.
    assert_eq!(
        read("launcher-umask").trim(),
        "0022",
        "ATTACK: the root launcher ran under its caller's umask (0): what it creates as root \
         would be writable by every uid"
    );
    // M1477: the root helper holds no reference into a directory its caller
    // chose, and resolves nothing relative to one.
    assert_eq!(
        read("helper-cwd").trim(),
        "/",
        "ATTACK: the root helper kept its caller's working directory"
    );
    // M1478: an inherited descriptor (fd 9, the caller's file).
    let held_s = held.display().to_string();
    for who in ["launcher-fds", "helper-fds"] {
        assert!(
            !read(who).lines().any(|l| l == held_s),
            "ATTACK: a descriptor the caller left open reached the root ({who}): {}",
            read(who)
        );
    }
    let limits = read("launcher-limits");
    // M1479: a core dump of the root launcher (or what it runs) writes its
    // memory to disk.
    assert_eq!(
        limit(&limits, "Max core file size").0,
        "0",
        "ATTACK: the root launcher may dump core (its caller raised RLIMIT_CORE): {limits}"
    );
    // M1480: lowered limits make the root launch fail where its caller chose
    // (SIGXFSZ mid-write, SIGXCPU mid-boot).
    assert!(
        limit(&limits, "Max file size").0 == "unlimited"
            && limit(&limits, "Max cpu time").0 == "unlimited",
        "ATTACK: the root launcher ran under resource limits its caller lowered (FSIZE/CPU): \
         {limits}"
    );
    // M1482: the open-file limit is the helper's own.
    assert_eq!(
        limit(&limits, "Max open files"),
        ("65536".to_string(), "65536".to_string()),
        "ATTACK: the root launcher ran under its caller's open-file limit: {limits}"
    );
    // The environment, end to end: the launcher's environment is built from
    // nothing by sealed_exec::command (M228). harden()'s own clear has its
    // row (M1487) on the route where it is the only guard:
    // `the_root_helpers_address_layout_never_reaches_its_caller`.
    let env = read("launcher-env");
    assert!(
        !env.contains("HOSTILE_CALLER") && !env.contains("LD_PRELOAD") && !bash_env_ran.exists(),
        "ATTACK: the caller's environment reached the root launcher: {env}"
    );
    assert_eq!(o.status.code(), Some(0), "control: the launch ran: {rep}");
}

/// The caller of `--probe`: argv[1] is the setuid helper. With `closed`, the
/// helper's stdout is a pipe with NO read end anywhere: it is closed before
/// the fork, so the helper's report write fails EPIPE every time (SIGPIPE is
/// ignored there, M1481, so `println!` PANICS). It was once closed in the
/// parent AFTER the fork, and under load the helper's write could land in the
/// pipe first (measured: 11 of 40 runs under 40 CPU spinners exited 0). A
/// one-byte write confirms there is no reader before the helper starts.
/// Otherwise a pipe this caller drains and copies to its own stdout. The
/// helper's environment is exactly `RUST_BACKTRACE=full`.
const PROBE_CALLER: &str = r#"
import os, sys
helper, closed = sys.argv[1], sys.argv[2] == "closed"
r, w = os.pipe()
if closed:
    os.close(r)
    # python3 ignores SIGPIPE: with no reader the write raises EPIPE.
    try:
        os.write(w, b"x")
        sys.exit("setup: the closed report pipe still has a reader")
    except BrokenPipeError:
        pass
pid = os.fork()
if pid == 0:
    os.dup2(w, 1)
    os.execve(helper, [helper, "--probe"], {"RUST_BACKTRACE": "full"})
os.close(w)
out = b""
if not closed:
    while True:
        b = os.read(r, 65536)
        if not b:
            break
        out += b
_, st = os.waitpid(pid, 0)
sys.stdout.write(out.decode())
sys.exit(os.waitstatus_to_exitcode(st) & 0xff)
"#;

/// A (C9 round 4b, final; M1487), ROOT ONLY: harden()'s environment clear, on
/// the route where it is the ONLY guard. Nothing the helper launches sees its
/// environment (sealed_exec::command builds every child's envp from nothing,
/// M228), but the helper's OWN Rust runtime reads `RUST_BACKTRACE` when it
/// panics, and its caller can make it panic: `--probe` writes its report with
/// `println!`, SIGPIPE is ignored (M1481), so a report pipe whose read end is
/// closed makes the write fail EPIPE and the helper panic. With the caller's
/// `RUST_BACKTRACE=full` still in its environment, the panic hook prints the
/// root helper's stack, every frame with its ADDRESS, to the caller's stderr:
/// the setuid-root process's address layout handed to the unprivileged Fabric
/// uid. The panic happens after `harden()` (setup asserts it happened).
/// Control: the same caller with an open pipe reads the probe's report.
#[test]
fn the_root_helpers_address_layout_never_reaches_its_caller() {
    if skip_unless_root() {
        return;
    }
    let f = fx(Some(FABRIC), "", |_| {});
    let caller = |mode: &str| {
        Command::new("setpriv")
            .args([
                &format!("--reuid={FABRIC}"),
                &format!("--regid={FABRIC}"),
                "--clear-groups",
                "--",
                "python3",
                "-c",
                PROBE_CALLER,
            ])
            .arg(f.installed.as_ref().unwrap())
            .arg(mode)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let o = caller("closed");
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(
        err.contains("failed printing to stdout"),
        "setup: the helper's probe did not panic on the closed report pipe: exit {:?} {err}",
        o.status.code()
    );
    assert!(
        !err.contains("stack backtrace") && !err.contains("0x"),
        "ATTACK: the root helper printed its stack, with addresses, to its caller (the caller's \
         RUST_BACKTRACE reached the setuid helper's panic hook): {err}"
    );
    let o = caller("open");
    let out: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!(
            "control: the probe did not report to an open pipe ({e}): {:?} {}",
            o.status.code(),
            String::from_utf8_lossy(&o.stderr)
        )
    });
    assert_eq!(o.status.code(), Some(0), "control: the probe ran: {out}");
}

/// The impostor custodian: bound on `argv[1]` (or, with `fd`, serving
/// systemd's fd 3), it answers every request `ok` in `argv[2]`'s mode,
/// spending any nonce it is shown. The program is python3, not the pinned
/// `axon-custodian`.
const IMPOSTOR: &str = r#"
import os, socket, sys
where, mode = sys.argv[1], sys.argv[2]
if where == "fd":
    s = socket.socket(fileno=3)
else:
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.bind(where)
    s.listen(8)
    open(where + ".ready", "w").close()
while True:
    c, _ = s.accept()
    c.recv(4096)
    c.sendall(('{"schema":"axon-custodian-reply/1","ok":true,"mode":"%s","nonce":null,'
               '"error":null}\n' % mode).encode())
    c.close()
"#;

struct Impostor(std::process::Child);
impl Drop for Impostor {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_impostor(sock: &Path) -> Impostor {
    let mut c = Command::new("python3")
        .args(["-c", IMPOSTOR])
        .arg(sock)
        .arg("test")
        .spawn()
        .unwrap();
    let ready = PathBuf::from(format!("{}.ready", sock.display()));
    // Ready, or exited (never will be); SETUP_BOUND (was 5 s) only fails.
    wait_until(SETUP_BOUND, || {
        ready.exists() || c.try_wait().unwrap().is_some()
    });
    assert!(
        ready.exists(),
        "setup: the impostor custodian never listened"
    );
    Impostor(c)
}

/// A (amendment 65; M1483): the helper spends a nonce only through the
/// custodian PROGRAM its operator pinned. An impostor bound on the socket the
/// config names, running as the custodian uid, answers every spend "ok"
/// (here: a nonce it never issued). The kernel names the process that sent
/// the reply (SCM_PIDFD); its executable, opened by descriptor, is python3,
/// not the pinned axon-custodian. Control: the fixture's genuine custodian,
/// pinned, launches (`the_helper_launches_a_well_formed_request…`).
#[test]
fn a_custodian_program_the_operator_never_pinned_spends_nothing() {
    let imp_dir = tempfile::tempdir().unwrap();
    let sock = imp_dir.path().join("impostor.sock");
    let _imp = start_impostor(&sock);
    let f = fx(None, "", |v| v["custodian"]["socket"] = json!(sock));
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert!(
        code == Some(30) && !f.launched(),
        "ATTACK: the root helper spent the nonce through a custodian program the operator \
         never pinned (an impostor on the custodian socket): {code:?} {rep}"
    );
    assert!(
        rep["error"]
            .as_str()
            .unwrap_or("")
            .contains("not served by the pinned custodian program"),
        "{rep}"
    );
    // Control on the same fixture: the genuine custodian's socket, pinned.
    f.edit_config(|v| v["custodian"]["socket"] = json!(custodian_socket(&f.base)));
    f.put_inputs();
    let (code, rep) = f.run(&f.request("op-2"), None);
    assert_eq!(code, Some(0), "control: the pinned custodian spends: {rep}");
}

/// A custodian stand-in that writes ONE reply from TWO processes: the first
/// half from the accepting process, the second from a child it forks after
/// (both execute the same program, python3). The child stays alive until the
/// client has read its half, so the kernel still names it (SCM_PIDFD).
const TWO_SENDERS: &str = r#"
import os, socket, sys, time
where, mode = sys.argv[1], sys.argv[2]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.bind(where)
s.listen(8)
open(where + ".ready", "w").close()
reply = ('{"schema":"axon-custodian-reply/1","ok":true,"mode":"%s","nonce":null,'
         '"error":null}\n' % mode).encode()
while True:
    c, _ = s.accept()
    c.recv(4096)
    c.sendall(reply[:20])
    time.sleep(0.3)
    pid = os.fork()
    if pid == 0:
        c.sendall(reply[20:])
        time.sleep(2)
        os._exit(0)
    os.waitpid(pid, 0)
    c.close()
"#;

/// A (amendment 74; custodian.rs `seen != pid`): a custodian reply is ONE
/// process's. The program pin is checked per message, so a reply whose bytes
/// two processes wrote is refused even when BOTH execute the pinned program:
/// otherwise a process the custodian program let hold its connection (a fork)
/// could complete or rewrite a reply the identified sender began. Here the
/// operator pins the stand-in's own program (python3), so every message
/// passes the pin and only the one-process rule can refuse. Control: the same
/// pinned program answering from ONE process is accepted.
#[test]
fn a_reply_two_processes_wrote_is_refused_even_when_both_run_the_pinned_program() {
    use sha2::{Digest, Sha256};
    let me = euid();
    let run = |script: &str, tag: &str| -> Result<axon_fabric::custodian::Mode, String> {
        let d = tempfile::tempdir().unwrap();
        let sock = d.path().join(format!("{tag}.sock"));
        let c = Command::new("python3")
            .args(["-c", script])
            .arg(&sock)
            .arg("test")
            .spawn()
            .unwrap();
        let pid = c.id();
        let _guard = Impostor(c);
        let ready = PathBuf::from(format!("{}.ready", sock.display()));
        for _ in 0..200 {
            if ready.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            ready.exists(),
            "setup: the stand-in custodian never listened"
        );
        let exe = std::fs::read(format!("/proc/{pid}/exe")).expect("setup: python3's executable");
        let pin = Sha256::digest(&exe)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        axon_fabric::custodian::CustodianRef {
            socket: sock,
            uid: me,
            sha256: Some(pin),
        }
        .spend(&"ab".repeat(16), 0, &"cd".repeat(32))
    };
    let got = run(TWO_SENDERS, "two");
    assert!(
        got.is_err(),
        "ATTACK: a custodian reply that two processes wrote was accepted as the pinned \
         custodian's: {got:?}"
    );
    assert!(
        format!("{got:?}").contains("two processes"),
        "refused for another reason: {got:?}"
    );
    let one = run(IMPOSTOR, "one");
    assert!(
        one.is_ok(),
        "control: the pinned program answering from one process: {one:?}"
    );
}

/// A (amendment 65; M1484): the bytes hashed are the ones that ran only if no
/// other uid can rewrite the executable; one group- or other-writable is
/// refused even when its bytes are, at this moment, the pinned ones.
#[test]
fn a_custodian_executable_another_uid_can_rewrite_is_refused() {
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let copy = d.path().join("axon-custodian");
    copy_executable(env!("CARGO_BIN_EXE_axon-custodian"), &copy, 0o775);
    let store = d.path().join("custodian-nonces");
    std::fs::create_dir(&store).unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
    let me = euid();
    let sock = d.path().join("rw.sock");
    let cfg = d.path().join("custodian.json");
    std::fs::write(
        &cfg,
        json!({"schema": "axon-custodian/1", "custodian_uid": me, "fabric_uid": me,
               "launcher_uid": me, "socket": sock, "store": store, "max_age_s": 300})
        .to_string(),
    )
    .unwrap();
    std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o644)).unwrap();
    let child = Command::new(&copy)
        .arg("--test-config")
        .arg(&cfg)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut guard = Impostor(child);
    // It listens (the pin check is then what answers), or it exited; a
    // socket that never appeared is a SETUP failure, never the refusal (it
    // was a 5 s wait whose expiry let the client's connect error stand in).
    wait_until(SETUP_BOUND, || {
        sock.exists() || guard.0.try_wait().unwrap().is_some()
    });
    assert!(sock.exists(), "setup: the copied custodian never listened");
    let client = |pin: String| axon_fabric::custodian::CustodianRef {
        socket: sock.clone(),
        uid: me,
        sha256: Some(pin),
    };
    let got = client(custodian_program_sha256()).issue(0);
    assert!(
        got.is_err(),
        "ATTACK: a custodian whose executable another uid can rewrite (mode 0775) was trusted \
         as the pinned program: {got:?}"
    );
    assert!(
        format!("{got:?}").contains("no other uid can write"),
        "{got:?}"
    );
    // Control: the same copy, not writable by others, is the pinned program.
    std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o755)).unwrap();
    client(custodian_program_sha256())
        .issue(0)
        .expect("control: the pinned program, owned and not writable by others");
}

/// Amendment 95 (eqgate4), ROOT ONLY: the program that answers as the custodian
/// must be owned by root or by the asking uid. One owned by a STRANGER uid is
/// refused although its bytes are the pinned ones and no other uid can write it
/// (mode 0755): its owner can rewrite it after the hash. M1484 rows the mode
/// term of this guard; this is the owner term, which a row on the mode term
/// did not credit. Control: the same copy owned by the asking uid.
#[test]
fn a_custodian_executable_owned_by_a_stranger_uid_is_refused() {
    if euid() != 0 {
        eprintln!("skipped: needs root (an executable owned by another uid)");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let copy = d.path().join("axon-custodian");
    copy_executable(env!("CARGO_BIN_EXE_axon-custodian"), &copy, 0o755);
    let store = d.path().join("custodian-nonces");
    std::fs::create_dir(&store).unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
    let me = euid();
    let sock = d.path().join("stranger.sock");
    let cfg = d.path().join("custodian.json");
    std::fs::write(
        &cfg,
        json!({"schema": "axon-custodian/1", "custodian_uid": me, "fabric_uid": me,
               "launcher_uid": me, "socket": sock, "store": store, "max_age_s": 300})
        .to_string(),
    )
    .unwrap();
    std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o644)).unwrap();
    let child = Command::new(&copy)
        .arg("--test-config")
        .arg(&cfg)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut guard = Impostor(child);
    wait_until(SETUP_BOUND, || {
        sock.exists() || guard.0.try_wait().unwrap().is_some()
    });
    assert!(sock.exists(), "setup: the copied custodian never listened");
    let client = |pin: String| axon_fabric::custodian::CustodianRef {
        socket: sock.clone(),
        uid: me,
        sha256: Some(pin),
    };
    // CONTROL: owned by the asking uid (root here), not writable by others.
    client(custodian_program_sha256())
        .issue(0)
        .expect("control: the pinned program, owned by the asking uid, not writable by others");
    // ATTACK: the same file, now owned by a stranger uid (mode unchanged).
    std::os::unix::fs::chown(&copy, Some(65534), None).unwrap();
    let got = client(custodian_program_sha256()).issue(0);
    assert!(
        got.is_err(),
        "ATTACK: a custodian whose executable a stranger uid owns (mode 0755) was trusted as the \
         pinned program: {got:?}"
    );
    assert!(format!("{got:?}").contains("owned by uid 65534"), "{got:?}");
}

/// Run the setuid-root PRODUCTION helper as the Fabric uid in the production
/// namespace of `s` (custodian started with `prefix`), with `wrap` before the
/// exec (e.g. `setpriv --no-new-privs`). Returns (exit, report, launcher ruid).
fn production_helper_as_fabric(s: &Path, prefix: &str, wrap: &str) -> (String, String, String) {
    in_production_etc_with(
        s,
        "custodian",
        "/etc/axon/run/custodian.sock",
        prefix,
        &format!(
            "cp \"$1/axon-protected-launcher\" /etc/axon/h\n\
             chown 0:{FABRIC} /etc/axon/h\n\
             chmod 04750 /etc/axon/h\n\
             setpriv --reuid={FABRIC} --regid={FABRIC} --clear-groups {wrap} -- \
             sh -c 'o=$(/etc/axon/h); c=$?; printf \"%s\\n\" \"$o\"; exit $c' < \"$1/request.json\" > \"$1/report.json\"\n\
             echo $? > \"$1/code\"\n\
             cp -a /etc/axon/runs/op-1 \"$1/result\" 2>/dev/null"
        ),
    );
    let read = |n: &str| {
        std::fs::read_to_string(s.join(n))
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    (read("code"), read("report.json"), read("result/ruid"))
}

fn production_fixture() -> Option<tempfile::TempDir> {
    if skip_unless_root() {
        return None;
    }
    if !Path::new("/etc/axon").is_dir() {
        eprintln!("skipped: no /etc/axon mount point (this test never creates one)");
        return None;
    }
    let d = tempfile::tempdir_in("/var/tmp").unwrap();
    set_mode(d.path(), 0o755);
    production_etc(d.path(), "custodian");
    Some(d)
}

fn edit_production_helper_config(s: &Path, edit: impl FnOnce(&mut Value)) {
    let p = s.join("etc/protected-launcher.json");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    edit(&mut v);
    std::fs::write(&p, v.to_string()).unwrap();
}

/// A (amendment 65; M1483, M1485), ROOT ONLY, PRODUCTION BUILD: on a
/// protected host the custodian is the program the operator pinned. (1) A
/// helper config with no `custodian.sha256` is refused and nothing launches
/// (M1485): unpinned, whatever program the custodian socket's unit runs
/// spends nonces. (2) Pinned, an impostor socket-activated in the
/// custodian's place (its unit's ExecStart swapped for a program that answers
/// every spend "ok") spends nothing (M1483). Control: the genuine production
/// custodian, pinned, launches as root.
#[test]
fn a_production_helper_spends_only_through_the_pinned_custodian_program() {
    let Some(d) = production_fixture() else {
        return;
    };
    let s = d.path();
    edit_production_helper_config(s, |v| {
        v["custodian"].as_object_mut().unwrap().remove("sha256");
    });
    let (code, rep, ruid) = production_helper_as_fabric(s, "", "");
    assert!(
        code == "30" && ruid.is_empty(),
        "ATTACK: a production helper launched with no custodian program pin (any program on the \
         custodian socket spends): exit {code} ruid {ruid:?} {rep}"
    );
    assert!(rep.contains("custodian.sha256"), "{rep}");

    let Some(d) = production_fixture() else {
        return;
    };
    let s = d.path();
    std::fs::write(s.join("impostor.py"), IMPOSTOR).unwrap();
    let (code, rep, ruid) =
        production_helper_as_fabric(s, "python3 \"$1/impostor.py\" fd protected", "");
    assert!(
        code == "30" && ruid.is_empty(),
        "ATTACK: a production helper spent the nonce through an impostor socket-activated as the \
         custodian (a program the operator never pinned): exit {code} ruid {ruid:?} {rep}"
    );
    assert!(
        rep.contains("not served by the pinned custodian program"),
        "{rep}"
    );

    let Some(d) = production_fixture() else {
        return;
    };
    let s = d.path();
    let (code, rep, ruid) = production_helper_as_fabric(s, "", "");
    assert_eq!(
        (code.as_str(), ruid.as_str()),
        ("0", "0"),
        "control: the pinned production custodian spends and the helper launches as root: {rep} \
         (custodian: {})",
        std::fs::read_to_string(s.join("custodian.err")).unwrap_or_default()
    );
}

/// A (amendment 65), ROOT ONLY, PRODUCTION BUILD: a Fabric running under
/// NoNewPrivileges (systemd `NoNewPrivileges=yes`, `setpriv --no-new-privs`)
/// makes the kernel ignore the helper's set-id bit. Measured: the helper then
/// runs as the Fabric uid; it launches nothing, and now SAYS why (the euid
/// refusal alone named a missing setuid bit or a nosuid mount, which is how
/// this misconfiguration was misdiagnosed). No row: on every route the euid
/// rule (M602) and, for a test-trust build, the operator-file owner rule
/// refuse the same launch (amendment 65 records it as a diagnostic).
#[test]
fn a_fabric_under_no_new_privileges_is_told_why_the_helper_launches_nothing() {
    let Some(d) = production_fixture() else {
        return;
    };
    let s = d.path();
    let (code, rep, ruid) = production_helper_as_fabric(s, "", "--no-new-privs");
    assert!(
        code == "30" && ruid.is_empty(),
        "ATTACK: a production helper launched under its caller's NoNewPrivileges: exit {code} \
         ruid {ruid:?} {rep}"
    );
    assert!(
        rep.contains("NoNewPrivileges"),
        "the refusal names NoNewPrivileges: {rep}"
    );
    // The probe reports it, for the preflight.
    let p = Command::new("setpriv")
        .args([
            &format!("--reuid={FABRIC}"),
            &format!("--regid={FABRIC}"),
            "--clear-groups",
            "--no-new-privs",
            "--",
        ])
        .arg(helper_pin().path)
        .arg("--probe")
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&p.stdout).unwrap();
    assert_eq!(v["no_new_privs"], true, "{v}");
}

// ── C9 round 4c, workstream HARDEN (amendment 73; rows M1600-M1622, matrix
// A110-A117). A reviewer's reproductions: an interval timer the caller armed
// before exec killed the setuid helper with SIGALRM, and a ^C written to a
// pty the caller owns killed it with SIGINT. `harden()` is now derived from
// the full list of what a set-id exec preserves. Each test below arms the
// state as a NON-ROOT caller would hand it over (a few limits are raised by
// the service manager first, as root, then the uid is dropped), shows by a
// WITNESS (an ordinary exec of python3, no helper) that the state really
// survived the exec, runs a CONTROL launch with nothing armed, and then
// attacks: the setuid-root helper must be unaffected.

/// The caller. Run as root; arms `argv[2]`'s groups, drops to the Fabric uid
/// and execs the helper (mode `helper`) or a witness that reports its own
/// state (mode `witness`). The TSC trap is set last, through a pre-built
/// ctypes execve: any python run after it would fault on the clock.
const ARMING_CALLER: &str = r#"
import ctypes, json, os, platform, resource, signal, sys
F = int(sys.argv[1])
arms = set(a for a in sys.argv[2].split(",") if a)
mode, helper, cfg = sys.argv[3], sys.argv[4], sys.argv[5]
libc = ctypes.CDLL(None, use_errno=True)
IOPRIO_SET = {"x86_64": 251, "aarch64": 30}[platform.machine()]
IOPRIO_GET = {"x86_64": 252, "aarch64": 31}[platform.machine()]
WITNESS = r'''
import ctypes, json, os, platform, signal
libc = ctypes.CDLL(None)
get = @GET@
sub = ctypes.c_int(-1)
libc.prctl(37, ctypes.byref(sub), 0, 0, 0)
print(json.dumps({
    "itimers": [signal.getitimer(w)[1] for w in
                (signal.ITIMER_REAL, signal.ITIMER_VIRTUAL, signal.ITIMER_PROF)],
    "limits": open("/proc/self/limits").read(),
    "nice": os.getpriority(os.PRIO_PROCESS, 0),
    "ioprio_class": libc.syscall(get, 1, 0) >> 13,
    "sched": os.sched_getscheduler(0),
    "cpus": len(os.sched_getaffinity(0)),
    "oom": int(open("/proc/self/oom_score_adj").read()),
    "slack": int(open("/proc/self/timerslack_ns").read()),
    "persona": open("/proc/self/personality").read().strip(),
    "subreaper": sub.value,
}))
'''
def ok(r, what):
    if r == -1:
        raise OSError(ctypes.get_errno(), what)
def arm_late():
    # What a fork does not carry to a child (interval timers, the subreaper
    # flag) or what must be the exec'ing process's own (its process group).
    if "timers" in arms:
        for s in (signal.SIGALRM, signal.SIGVTALRM, signal.SIGPROF):
            signal.signal(s, signal.SIG_IGN)
        signal.setitimer(signal.ITIMER_REAL, 0.02, 0.02)
        signal.setitimer(signal.ITIMER_VIRTUAL, 0.001, 0.001)
        signal.setitimer(signal.ITIMER_PROF, 0.001, 0.001)
    if "kernel" in arms:
        ok(libc.prctl(36, 1, 0, 0, 0), "PR_SET_CHILD_SUBREAPER")
    if "leader" in arms:
        os.setpgid(0, 0)
if "limits" in arms:
    for r, v in [(resource.RLIMIT_STACK, 1 << 20), (resource.RLIMIT_RSS, 1 << 20),
                 (resource.RLIMIT_MEMLOCK, 0), (10, 7),   # RLIMIT_LOCKS
                 (11, 0), (12, 0),        # RLIMIT_SIGPENDING, RLIMIT_MSGQUEUE
                 (15, 1000000),           # RLIMIT_RTTIME
                 # What a service manager (LimitNICE=, LimitRTPRIO=) hands down:
                 # raised as root, before the uid drops.
                 (13, 40), (14, 50)]:  # RLIMIT_NICE, RLIMIT_RTPRIO
        resource.setrlimit(r, (v, v))
if "sched" in arms:
    os.setpriority(os.PRIO_PROCESS, 0, 19)
    ok(libc.syscall(IOPRIO_SET, 1, 0, 3 << 13), "ioprio_set")
    os.sched_setscheduler(0, os.SCHED_IDLE, os.sched_param(0))
    cpus = sorted(os.sched_getaffinity(0))
    os.sched_setaffinity(0, {cpus[0]})
if "kernel" in arms:
    open("/proc/self/oom_score_adj", "w").write("1000")
    ok(libc.prctl(29, 100000000, 0, 0, 0), "PR_SET_TIMERSLACK")
if "persona" in arms:
    ok(libc.personality(0x0008 | 0x20000), "personality")
os.setgroups([])
os.setresgid(F, F, F)
os.setresuid(F, F, F)
if mode == "witness":
    arm_late()
    path, argv = sys.executable, [sys.executable, "-c", WITNESS.replace("@GET@", str(IOPRIO_GET))]
else:
    path, argv = helper, [helper, "--test-config", cfg]
    # The helper's PARENT is this process, the Fabric program, running as the
    # Fabric uid (amendment 79: the helper serves only its pinned program).
    # What it is handed is armed in the CHILD that execs it.
    pid = os.fork()
    if pid:
        _, st = os.waitpid(pid, 0)
        if os.WIFSIGNALED(st):
            sig = os.WTERMSIG(st)
            signal.signal(sig, signal.SIG_DFL)
            os.kill(os.getpid(), sig)
            signal.pause()
        sys.exit(os.WEXITSTATUS(st))
    arm_late()
if "tsc" in arms:
    c = ctypes.c_char_p
    av = (c * (len(argv) + 1))(*[a.encode() for a in argv], None)
    ev = (c * 2)(b"PATH=/usr/bin:/bin", None)
    pb = path.encode()
    libc.prctl(26, 2, 0, 0, 0)
    libc.execve(pb, av, ev)
    os._exit(127)
os.execve(path, argv, {"PATH": "/usr/bin:/bin"})
"#;

/// What the stand-in launcher records about its parent, the root helper
/// (`$PPID`), and itself.
const HARDEN_RECORD: &str = r#"P=$PPID
echo $P > "$OUT/h-pid"
cp /proc/$P/limits "$OUT/h-limits"
sed 's/^.*) //' /proc/$P/stat | cut -d' ' -f17 > "$OUT/h-nice"
cat /proc/$P/oom_score_adj > "$OUT/h-oom"
cat /proc/$P/timerslack_ns > "$OUT/h-slack"
cat /proc/$P/personality > "$OUT/h-persona"
grep '^Cpus_allowed_list' /proc/$P/status > "$OUT/h-cpus"
ionice -p $P > "$OUT/h-ionice" 2>&1
chrt -p $P > "$OUT/h-sched" 2>&1
uname -m > "$OUT/l-uname-m"
( sleep 5 & echo $! > "$OUT/orphan-pid" )
sleep 0.2
sed 's/^.*) //' /proc/$(cat "$OUT/orphan-pid")/stat | cut -d' ' -f2 > "$OUT/orphan-ppid"
kill $(cat "$OUT/orphan-pid") 2>/dev/null
"#;

fn died_of(o: &std::process::Output) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    o.status.signal()
}

/// One launch of the setuid helper by the arming caller (a fresh fixture).
struct Armed {
    o: std::process::Output,
    out: PathBuf,
    _f: Fx,
}

/// Amendment 79: the Fabric program of a test whose caller is a python3
/// process (as the Fabric uid) that runs the helper as its child.
fn python_fabric_pin() -> Value {
    let python = String::from_utf8(
        Command::new("python3")
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    program_pin(Path::new(python.trim()), TEST_FABRIC_REVISION)
}

fn launch_armed(arms: &str, extra: &str) -> Armed {
    let f = fx(Some(FABRIC), extra, |v| v["fabric"] = python_fabric_pin());
    let mut child = Command::new("python3")
        .args(["-c", ARMING_CALLER])
        .arg(FABRIC.to_string())
        .arg(arms)
        .arg("helper")
        .arg(f.installed.as_ref().unwrap())
        .arg(&f.cfg)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        // A helper killed before it reads the request closes the pipe.
        let _ = child
            .stdin
            .take()
            .unwrap()
            .write_all(f.request("op-1").to_string().as_bytes());
    }
    let o = child.wait_with_output().unwrap();
    let out = f.out_root.join("op-1");
    Armed { o, out, _f: f }
}

impl Armed {
    fn rep(&self) -> String {
        format!(
            "exit {:?} signal {:?} stdout {} stderr {}",
            self.o.status.code(),
            died_of(&self.o),
            String::from_utf8_lossy(&self.o.stdout),
            String::from_utf8_lossy(&self.o.stderr)
        )
    }
    /// A file the launcher recorded; a launch that did not get that far is a
    /// setup failure (the attack checks run before any read).
    fn read(&self, n: &str) -> String {
        std::fs::read_to_string(self.out.join(n))
            .unwrap_or_else(|e| {
                panic!("setup: the launch did not record {n} ({e}): {}", self.rep())
            })
            .trim()
            .to_string()
    }
    fn launched_ok(&self) -> bool {
        self.o.status.code() == Some(0) && self.out.join("h-pid").exists()
    }
}

/// The caller's own view of the armed state (an ordinary exec, no helper).
fn witness(arms: &str) -> (std::process::Output, Value) {
    let o = Command::new("python3")
        .args(["-c", ARMING_CALLER])
        .arg(FABRIC.to_string())
        .arg(arms)
        .args(["witness", "-", "-"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let v = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
    (o, v)
}

/// The control: nothing armed, the helper launches and the launcher records
/// the helper's state.
fn control() -> Armed {
    let c = launch_armed("", HARDEN_RECORD);
    assert!(
        c.launched_ok(),
        "control: a launch with nothing armed did not run: {}",
        c.rep()
    );
    c
}

/// A (amendment 73; M1600-M1602), ROOT ONLY: interval timers survive exec.
/// The caller arms ITIMER_REAL, ITIMER_VIRTUAL and ITIMER_PROF (with the
/// signals ignored, so the caller itself lives to exec); `harden()` resets the
/// dispositions to default, so a timer left armed kills the setuid-root helper
/// (exit 142 for SIGALRM, the reviewer's reproduction). The signal that killed
/// it names the row.
#[test]
fn a_callers_interval_timers_never_signal_the_root_helper() {
    if skip_unless_root() {
        return;
    }
    let (w, v) = witness("timers");
    assert!(
        w.status.success()
            && v["itimers"]
                .as_array()
                .map(|a| a.len() == 3 && a.iter().all(|t| t.as_f64().unwrap_or(0.0) > 0.0))
                .unwrap_or(false),
        "setup: the three interval timers did not survive an ordinary exec: {w:?} {v}"
    );
    control();
    let a = launch_armed("timers", HARDEN_RECORD);
    match died_of(&a.o) {
        Some(s) if s == libc::SIGALRM => panic!(
            "ATTACK: the root helper died of SIGALRM (its caller's ITIMER_REAL survived the \
             exec): {}",
            a.rep()
        ),
        Some(s) if s == libc::SIGVTALRM => panic!(
            "ATTACK: the root helper died of SIGVTALRM (its caller's ITIMER_VIRTUAL survived \
             the exec): {}",
            a.rep()
        ),
        Some(s) if s == libc::SIGPROF => panic!(
            "ATTACK: the root helper died of SIGPROF (its caller's ITIMER_PROF survived the \
             exec): {}",
            a.rep()
        ),
        Some(s) => panic!("setup: the helper died of signal {s}: {}", a.rep()),
        None => {}
    }
    assert!(a.launched_ok(), "control: the launch ran: {}", a.rep());
}

/// The pty caller (root): a pty whose master it holds, a session whose
/// controlling terminal it is, and in that session's foreground process group
/// the WITNESS (`sleep`, signals at default) and the HELPER (the Fabric uid;
/// SIGINT ignored, as its caller left it, so only `harden()` makes it
/// killable). Once the stand-in launcher reports it is waiting, the caller
/// writes ^C to the master. It then releases the launcher and writes the
/// outcomes to `result`.
const PTY_CALLER: &str = r#"
import fcntl, os, signal, sys, termios, time
F, helper, cfg, waiting, go, result, ctrlc = int(sys.argv[1]), *sys.argv[2:8]
master, slave = os.openpty()
pid1 = os.fork()
if pid1 == 0:
    os.close(master)
    os.setsid()
    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
    witness = os.fork()
    if witness == 0:
        # A caller started detached (`setsid nohup cmd < /dev/null &`, a
        # non-interactive shell's `&`) hands down SIGINT IGNORED, and python
        # keeps an inherited ignore: the witness then survived the ^C and the
        # setup check failed (both the base and this branch, detached only).
        # The witness must have the disposition a terminal job has.
        signal.signal(signal.SIGINT, signal.SIG_DFL)
        dn = os.open("/dev/null", os.O_RDWR)
        for fd in (0, 1, 2):
            os.dup2(dn, fd)
        os.execv("/usr/bin/sleep", ["sleep", "30"])
    # AFTER the witness forked: a child forked while this was ignored inherits
    # the ignore until it resets it, and on a loaded host the ^C (written once
    # the helper is up) could land in that window, so the witness survived it
    # ("setup: the ^C did not reach the witness"; C9 shardflake). The witness
    # is forked with the default disposition and keeps it through its exec.
    signal.signal(signal.SIGINT, signal.SIG_IGN)
    h = os.fork()
    if h == 0:
        os.setgroups([])
        os.setresgid(F, F, F)
        os.setresuid(F, F, F)
        # The helper's PARENT is this process, the pinned Fabric program as
        # the Fabric uid (amendment 79); it reports the helper's own status.
        g = os.fork()
        if g:
            _, st = os.waitpid(g, 0)
            if os.WIFSIGNALED(st):
                sig = os.WTERMSIG(st)
                signal.signal(sig, signal.SIG_DFL)
                os.kill(os.getpid(), sig)
                signal.pause()
            os._exit(os.WEXITSTATUS(st))
        os.execve(helper, [helper, "--test-config", cfg], {"PATH": "/usr/bin:/bin"})
    _, hs = os.waitpid(h, 0)
    os.kill(witness, signal.SIGTERM)
    _, ws = os.waitpid(witness, 0)
    def fmt(s):
        return "sig=%d" % os.WTERMSIG(s) if os.WIFSIGNALED(s) else "exit=%d" % os.WEXITSTATUS(s)
    open(result, "w").write("helper %s\nwitness %s\n" % (fmt(hs), fmt(ws)))
    os._exit(0)
for _ in range(600):
    if os.path.exists(waiting):
        break
    time.sleep(0.05)
else:
    os.kill(pid1, signal.SIGKILL)
    sys.exit("setup: the helper never reached its launcher")
if ctrlc == "1":
    os.write(master, b"\x03")
time.sleep(0.5)
open(go, "w").close()
os.waitpid(pid1, 0)
"#;

/// A (amendment 73; M1603), ROOT ONLY: terminal-generated signals. The caller
/// owns a pty, is the session's controlling process, and the helper starts in
/// its foreground process group; ^C written to the master is SIGINT to that
/// whole group. `harden()` starts a new session (`setsid`), so the helper has
/// no controlling terminal and the group the terminal signals is not its own.
/// Setup: the witness `sleep` in the same group dies of that ^C, so the ^C
/// really was delivered. Control: the same launch without a ^C.
#[test]
fn a_terminal_its_caller_owns_never_signals_the_root_helper() {
    if skip_unless_root() {
        return;
    }
    let launch = |ctrl_c: bool| -> (Fx, String, std::process::Output) {
        // The launcher waits (in its own process, below the helper) until the
        // caller has written ^C and released it.
        let extra = "touch \"$OUT/waiting\"\ni=0\nwhile [ ! -e \"$OUT/../../go\" ] && [ $i -lt 200 ]; do sleep 0.05; i=$((i+1)); done\n";
        let f = fx(Some(FABRIC), extra, |v| v["fabric"] = python_fabric_pin());
        let result = f.base.join("result");
        // ALWAYS as a detached caller would start it: SIGINT ignored in the
        // environment this fixture inherits (the regression for the
        // detached-only failure; the fixture must not depend on it). `trap '' INT`
        // sets that disposition, which the exec'd interpreter inherits.
        let mut child = Command::new("sh")
            .args([
                "-c",
                "trap '' INT; exec python3 -c \"$0\" \"$@\"",
                PTY_CALLER,
            ])
            .arg(FABRIC.to_string())
            .arg(f.installed.as_ref().unwrap())
            .arg(&f.cfg)
            .arg(f.out_root.join("op-1/waiting"))
            .arg(f.base.join("go"))
            .arg(&result)
            .arg(if ctrl_c { "1" } else { "0" })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        {
            use std::io::Write;
            let _ = child
                .stdin
                .take()
                .unwrap()
                .write_all(f.request("op-1").to_string().as_bytes());
        }
        let o = child.wait_with_output().unwrap();
        let r = std::fs::read_to_string(&result).unwrap_or_else(|e| {
            panic!(
                "setup: the pty caller recorded no result ({e}): {:?} {}",
                o.status,
                String::from_utf8_lossy(&o.stderr)
            )
        });
        (f, r, o)
    };
    let (_f, r, o) = launch(false);
    assert!(
        r.contains("helper exit=0"),
        "control: without a ^C the helper launches: {r} {}",
        String::from_utf8_lossy(&o.stdout)
    );
    let (_f, r, o) = launch(true);
    assert!(
        r.contains(&format!("witness sig={}", libc::SIGINT)),
        "setup: the ^C did not reach the witness in the caller's foreground group: {r}"
    );
    assert!(
        !r.contains(&format!("helper sig={}", libc::SIGINT)),
        "ATTACK: a ^C written to a terminal its caller owns killed the root helper (SIGINT): \
         {r} {}",
        String::from_utf8_lossy(&o.stdout)
    );
    assert!(
        r.contains("helper exit=0"),
        "control: the helper launched under the ^C: {r} {}",
        String::from_utf8_lossy(&o.stdout)
    );
}

/// A (amendment 73; M1604), ROOT ONLY: `setsid` fails for a process-group
/// leader, which would leave the helper in its caller's session and under its
/// terminal's signals. The launch is then refused (exit 30, nothing ran).
/// Control: the same caller as an ordinary child launches.
#[test]
fn a_helper_that_could_not_leave_its_callers_session_launches_nothing() {
    if skip_unless_root() {
        return;
    }
    control();
    let a = launch_armed("leader", HARDEN_RECORD);
    assert!(
        a.o.status.code() != Some(0) && !a.out.join("h-pid").exists(),
        "ATTACK: a helper that stayed in its caller's session (a process-group leader, setsid \
         failed) launched anyway: {}",
        a.rep()
    );
    let rep: Value = serde_json::from_slice(&a.o.stdout).unwrap_or(Value::Null);
    assert_eq!(
        a.o.status.code(),
        Some(30),
        "refused, not unknown: {}",
        a.rep()
    );
    assert!(
        rep["error"]
            .as_str()
            .is_some_and(|e| e.contains("could not leave its caller's session")),
        "the refusal names its cause: {}",
        a.rep()
    );
    assert_eq!(rep["launched"], json!(false), "{}", a.rep());
}

/// The nine limits the first harden() left to the caller, as the helper's
/// launcher sees them: (the /proc row, what armed it, what harden() sets,
/// the marker's name).
type Pair = (&'static str, &'static str);
const NEW_LIMITS: [(&str, Pair, Pair, &str); 9] = [
    (
        "Max stack size",
        ("1048576", "1048576"),
        ("8388608", "unlimited"),
        "RLIMIT_STACK",
    ),
    (
        "Max resident set",
        ("1048576", "1048576"),
        ("unlimited", "unlimited"),
        "RLIMIT_RSS",
    ),
    (
        "Max locked memory",
        ("0", "0"),
        ("8388608", "8388608"),
        "RLIMIT_MEMLOCK",
    ),
    (
        "Max file locks",
        ("7", "7"),
        ("unlimited", "unlimited"),
        "RLIMIT_LOCKS",
    ),
    (
        "Max pending signals",
        ("0", "0"),
        ("unlimited", "unlimited"),
        "RLIMIT_SIGPENDING",
    ),
    (
        "Max msgqueue size",
        ("0", "0"),
        ("819200", "819200"),
        "RLIMIT_MSGQUEUE",
    ),
    ("Max nice priority", ("40", "40"), ("0", "0"), "RLIMIT_NICE"),
    (
        "Max realtime priority",
        ("50", "50"),
        ("0", "0"),
        "RLIMIT_RTPRIO",
    ),
    (
        "Max realtime timeout",
        ("1000000", "1000000"),
        ("unlimited", "unlimited"),
        "RLIMIT_RTTIME",
    ),
];

/// A (amendment 73; M1605-M1613), ROOT ONLY: the other nine of the sixteen
/// resource limits. The caller lowers STACK, RSS, MEMLOCK, LOCKS, SIGPENDING,
/// MSGQUEUE and RTTIME and hands down a raised NICE and RTPRIO ceiling (what
/// a service manager's `LimitNICE=`/`LimitRTPRIO=` does), and the root
/// launcher must run under the helper's own values. Each row is a separate
/// assertion with its own marker; the control reads the same values with
/// nothing armed.
#[test]
fn a_callers_other_resource_limits_never_reach_the_root_launch() {
    if skip_unless_root() {
        return;
    }
    let (w, v) = witness("limits");
    let wl = v["limits"].as_str().unwrap_or("").to_string();
    for (row, armed, _, name) in NEW_LIMITS {
        assert_eq!(
            limit(&wl, row),
            (armed.0.to_string(), armed.1.to_string()),
            "setup: {name} did not survive an ordinary exec: {w:?} {wl}"
        );
    }
    let c = control();
    let cl = c.read("h-limits");
    for (row, _, reset, name) in NEW_LIMITS {
        assert_eq!(
            limit(&cl, row),
            (reset.0.to_string(), reset.1.to_string()),
            "ATTACK: the root helper ran under its caller's {name}: (the ambient value reached it even with nothing armed; the control reads the helper's own): {cl}"
        );
    }
    let a = launch_armed("limits", HARDEN_RECORD);
    assert!(
        a.launched_ok(),
        "setup: the launch under lowered limits did not run: {}",
        a.rep()
    );
    let al = a.read("h-limits");
    for (row, _, reset, name) in NEW_LIMITS {
        assert_eq!(
            limit(&al, row),
            (reset.0.to_string(), reset.1.to_string()),
            "ATTACK: the root helper ran under its caller's {name}: {al}"
        );
    }
}

/// How many CPUs a `Cpus_allowed_list: 0-3,8` names.
fn cpu_count(line: &str) -> usize {
    line.split(':')
        .nth(1)
        .unwrap_or("")
        .trim()
        .split(',')
        .map(|r| match r.split_once('-') {
            Some((a, b)) => b.parse::<usize>().unwrap() - a.parse::<usize>().unwrap() + 1,
            None => 1,
        })
        .sum()
}

/// A (amendment 73; M1614-M1617), ROOT ONLY: scheduling attributes a fork and
/// an exec keep. The caller runs at nice 19, in the idle I/O class, under
/// SCHED_IDLE and pinned to one CPU; the root launcher must run at the
/// kernel's defaults, as the control does.
#[test]
fn a_callers_scheduling_state_never_reaches_the_root_launch() {
    if skip_unless_root() {
        return;
    }
    let (w, v) = witness("sched");
    let (_, base) = witness("");
    assert!(
        v["nice"] == json!(19)
            && v["ioprio_class"] == json!(3)
            && v["sched"] == json!(5)
            && v["cpus"] == json!(1)
            && base["cpus"].as_u64().unwrap_or(0) > 1,
        "setup: the scheduling state did not survive an ordinary exec (or this host has one \
         CPU): {w:?} {v} baseline {base}"
    );
    let c = control();
    let a = launch_armed("sched", HARDEN_RECORD);
    assert!(
        a.launched_ok(),
        "setup: the armed launch did not run: {}",
        a.rep()
    );
    assert_eq!(
        a.read("h-nice"),
        c.read("h-nice"),
        "ATTACK: the root helper kept its caller's nice value"
    );
    assert_eq!(
        a.read("h-ionice"),
        c.read("h-ionice"),
        "ATTACK: the root helper kept its caller's I/O scheduling class"
    );
    assert_eq!(
        a.read("h-sched").replace(&a.read("h-pid"), "P"),
        c.read("h-sched").replace(&c.read("h-pid"), "P"),
        "ATTACK: the root helper kept its caller's scheduling policy"
    );
    assert_eq!(
        cpu_count(&a.read("h-cpus")),
        cpu_count(&c.read("h-cpus")),
        "ATTACK: the root helper kept the CPU affinity its caller set: {} vs {}",
        a.read("h-cpus"),
        c.read("h-cpus")
    );
}

/// A (amendment 73; M1618, M1619, M1622), ROOT ONLY: the caller raises its
/// oom_score_adj to 1000 (first victim of the OOM killer), sets a 100 ms
/// timer slack and makes itself a child subreaper. The root launcher runs
/// with the control's values, and an orphan of the launcher is not adopted by
/// the helper.
#[test]
fn a_callers_oom_slack_and_subreaper_state_never_reaches_the_root_launch() {
    if skip_unless_root() {
        return;
    }
    let (w, v) = witness("kernel");
    assert!(
        v["oom"] == json!(1000) && v["slack"] == json!(100_000_000) && v["subreaper"] == json!(1),
        "setup: the state did not survive an ordinary exec: {w:?} {v}"
    );
    let c = control();
    assert_ne!(
        c.read("orphan-ppid"),
        c.read("h-pid"),
        "control: with nothing armed, an orphan is not adopted by the helper"
    );
    let a = launch_armed("kernel", HARDEN_RECORD);
    assert!(
        a.launched_ok(),
        "setup: the armed launch did not run: {}",
        a.rep()
    );
    assert_eq!(
        a.read("h-oom"),
        c.read("h-oom"),
        "ATTACK: the root helper kept its caller's oom_score_adj"
    );
    assert_eq!(
        a.read("h-slack"),
        c.read("h-slack"),
        "ATTACK: the root helper kept its caller's timer slack"
    );
    assert_ne!(
        a.read("orphan-ppid"),
        a.read("h-pid"),
        "ATTACK: the root helper is still its caller's child subreaper (it adopted an orphan of \
         its launcher)"
    );
}

/// A (amendment 73; M1620), ROOT ONLY: the personality survives exec (a
/// set-id exec clears only PER_CLEAR_ON_SETID). The caller sets PER_LINUX32
/// and UNAME26, which make `uname` of the root launcher lie about the
/// machine and the kernel release.
#[test]
fn a_callers_personality_never_reaches_the_root_launch() {
    if skip_unless_root() {
        return;
    }
    let (w, v) = witness("persona");
    assert_eq!(
        v["persona"],
        json!("00020008"),
        "setup: the personality did not survive an ordinary exec: {w:?} {v}"
    );
    let c = control();
    let a = launch_armed("persona", HARDEN_RECORD);
    assert!(
        a.launched_ok(),
        "setup: the armed launch did not run: {}",
        a.rep()
    );
    assert_eq!(
        a.read("h-persona"),
        c.read("h-persona"),
        "ATTACK: the root helper kept its caller's personality (uname: {})",
        a.read("l-uname-m")
    );
}

/// A (amendment 73; NO ROW, measured), ROOT ONLY: PR_SET_TSC survives exec,
/// but a caller that makes the timestamp counter fault (PR_TSC_SIGSEGV) kills
/// every program of this host's libc before its `main` (an ordinary exec of
/// python3, and of `true`, dies of SIGSEGV in the loader), so no reset inside
/// `harden()` can ever run and none is written: it would be a guard no attack
/// can distinguish from its absence. What is asserted is the fail-closed
/// shape: the helper either launches normally or dies before it has done
/// anything (no out dir, nothing launched).
#[test]
fn a_callers_timestamp_counter_trap_launches_nothing_it_cannot_finish() {
    if skip_unless_root() {
        return;
    }
    let (w, _) = witness("tsc");
    assert_eq!(
        died_of(&w),
        Some(libc::SIGSEGV),
        "setup: PR_TSC_SIGSEGV did not survive an ordinary exec: {w:?}"
    );
    control();
    let a = launch_armed("tsc", HARDEN_RECORD);
    assert!(
        a.launched_ok() || (died_of(&a.o) == Some(libc::SIGSEGV) && !a.out.exists()),
        "a helper under its caller's PR_TSC_SIGSEGV neither launched nor died before acting: {}",
        a.rep()
    );
}

// ── C9 round 6, EQGATE2 (amendment 87): permission modes and ownership ───────
//
// The root helper's snapshot of its inputs sets the modes the tree digest
// records (directories 0755, files 0644 / 0755 by the source's exec bit), and
// its hand-over gives the out tree to the Fabric uid KEEPING each entry's mode
// and setting the out dir itself to 0700. Each is a permission or ownership
// call that builds no `Err`; each was weakenable alone with the whole suite
// green. The stand-in launcher records the snapshot's modes; the test reads the
// out tree afterwards.
#[test]
fn the_snapshot_and_the_hand_over_keep_their_modes_and_owners() {
    if skip_unless_root() {
        return;
    }
    let extra = r#"find "$(dirname "$JOB")" -printf '%P %m\n' > "$OUT/stage-modes"
mkdir "$OUT/d"; chmod 750 "$OUT/d"; echo x > "$OUT/d/inner"; chmod 640 "$OUT/d/inner"
echo y > "$OUT/f"; chmod 640 "$OUT/f""#;
    let f = fx(Some(FABRIC), extra, |_| {});
    let cand = f.out_root.join(INPUTS).join("candidate");
    std::fs::create_dir_all(cand.join("sub")).unwrap();
    std::fs::write(cand.join("sub/g.ax"), "fn g() {}\n").unwrap();
    std::fs::write(cand.join("x.sh"), "#!/bin/sh\n").unwrap();
    set_mode(&cand.join("x.sh"), 0o755);
    for p in [cand.join("sub"), cand.join("sub/g.ax"), cand.join("x.sh")] {
        std::os::unix::fs::lchown(&p, Some(FABRIC), Some(FABRIC)).unwrap();
    }
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[])));
    assert_eq!(code, Some(0), "setup: the launch runs: {rep}");
    let out = f.out_root.join("op-1");
    let staged = std::fs::read_to_string(out.join("stage-modes")).unwrap();
    for (entry, why) in [
        (
            "candidate/sub 755",
            "ATTACK: the snapshot's directories are not 0755 (the tree digest's mode)",
        ),
        (
            "candidate/x.sh 755",
            "ATTACK: an executable input is not 0755 in the snapshot",
        ),
        (
            "candidate/f.ax 644",
            "ATTACK: a plain input is not 0644 in the snapshot (the helper's set_mode)",
        ),
        (
            "candidate/sub/g.ax 644",
            "ATTACK: a nested plain input is not 0644 in the snapshot",
        ),
    ] {
        assert!(staged.lines().any(|l| l == entry), "{why}: {staged}");
    }
    use std::os::unix::fs::MetadataExt;
    let st = |p: &Path| {
        let m = std::fs::symlink_metadata(p).unwrap();
        (m.mode() & 0o777, m.uid())
    };
    assert_eq!(
        st(&out),
        (0o700, FABRIC),
        "ATTACK: the out dir was not handed over 0700 to the Fabric uid"
    );
    assert_eq!(
        st(&out.join("d")),
        (0o750, FABRIC),
        "ATTACK: a directory of the out tree lost its mode or was not given to the Fabric uid"
    );
    assert_eq!(
        st(&out.join("d/inner")),
        (0o640, FABRIC),
        "ATTACK: a nested file of the out tree lost its mode or was not given to the Fabric uid"
    );
    assert_eq!(
        st(&out.join("f")),
        (0o640, FABRIC),
        "ATTACK: a file of the out tree lost its mode or was not given to the Fabric uid"
    );
}

/// C9 round 6, EQGATE2 (amendment 87): a setuid helper that runs as root takes
/// root's whole identity, not just its uid: no supplementary group of the
/// caller's survives (`setgroups(0)`) and every gid is 0 (`setresgid`). The
/// stand-in launcher records its parent's (the helper's) ids from /proc; the
/// caller carries an extra group.
#[test]
fn the_root_helper_takes_roots_identity_not_its_callers_groups() {
    if skip_unless_root() {
        return;
    }
    let extra = r#"grep -E '^(Uid|Gid|Groups):' /proc/$PPID/status > "$OUT/helper-ids""#;
    let f = fx(Some(FABRIC), extra, |_| {});
    let (code, rep) = f.run(&f.request("op-1"), Some((FABRIC, &[FABRIC, 4242])));
    assert_eq!(code, Some(0), "setup: the launch runs: {rep}");
    let ids = std::fs::read_to_string(f.out_root.join("op-1/helper-ids")).unwrap();
    assert!(
        ids.lines().any(|l| l.trim_end() == "Groups:"),
        "ATTACK: the root helper kept its caller's supplementary groups: {ids}"
    );
    assert!(
        ids.lines().any(|l| l.starts_with("Gid:\t0\t0\t0\t0")),
        "ATTACK: the root helper runs with its caller's gid, not root's: {ids}"
    );
    assert!(
        ids.lines().any(|l| l.starts_with("Uid:\t0\t0\t0\t0")),
        "ATTACK: the root helper did not take root's uid: {ids}"
    );
}

// ── C9 round 7, EQGATE3 (amendment 91): what the root launcher runs with ─────
//
// The helper runs the pinned launcher (and its verify step) through a `quiet`
// closure: stdin, stdout and stderr are /dev/null and the working directory is
// `/`. A launcher that inherits the helper's stdout can write into the reply
// pipe the Fabric parses; one that inherits stdin reads what is left of the
// request. Each is a builder call that builds no `Err`; each was removable
// alone with every suite green. The stand-in records what its own descriptors
// point at (the helper's three are pipes here).
#[test]
fn the_root_launcher_runs_with_null_stdio_in_the_root_directory() {
    let extra = r#"FDS=$(for i in 0 1 2; do readlink "/proc/$$/fd/$i"; done); echo "$FDS" > "$OUT/fds"; pwd > "$OUT/cwd"
echo "launcher noise on stdout"; echo "launcher noise on stderr" >&2"#;
    let f = fx(None, extra, |_| {});
    let o = {
        // f.run discards the raw streams: run the helper here.
        use std::io::Write;
        let mut child = Command::new(helper_pin().path)
            .arg("--test-config")
            .arg(&f.cfg)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(f.request("op-1").to_string().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let out = f.out_root.join("op-1");
    let fds = std::fs::read_to_string(out.join("fds"))
        .unwrap_or_else(|e| panic!("setup: the launcher did not record its descriptors: {e}"));
    assert_eq!(
        fds.lines().collect::<Vec<_>>(),
        ["/dev/null", "/dev/null", "/dev/null"],
        "ATTACK: the root launcher inherited a descriptor of the helper's (stdin, stdout, stderr \
         are the request and reply pipes): {fds}"
    );
    let reply = String::from_utf8_lossy(&o.stdout);
    assert!(
        !reply.trim().is_empty(),
        "ATTACK: the helper printed no reply (its report is the only thing the Fabric reads)"
    );
    assert!(
        serde_json::from_str::<serde_json::Value>(reply.trim()).is_ok(),
        "ATTACK: the launcher's output reached the helper's reply pipe: {reply:?}"
    );
    assert!(
        !String::from_utf8_lossy(&o.stderr).contains("launcher noise"),
        "ATTACK: the launcher's stderr reached the helper's: {:?}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(out.join("cwd")).unwrap().trim(),
        "/",
        "ATTACK: the root launcher ran in a directory other than /"
    );
}

/// C9 round 7, EQGATE3 (amendment 91): every descriptor the helper opens for
/// itself (`openat`: the operator-directory walk, the input snapshot, the
/// staging and out roots) is close-on-exec, so the root launcher inherits
/// exactly the descriptors the helper hands it on purpose (the pinned launcher
/// and manifest, the out dir) and no directory of the helper's own. The stand-in
/// lists its open descriptors.
#[test]
fn the_root_launcher_inherits_only_the_descriptors_it_is_handed() {
    let extra = r#"for f in /proc/$$/fd/*; do readlink "$f"; done > "$OUT/openfds""#;
    let f = fx(None, extra, |_| {});
    let (code, rep) = f.run(&f.request("op-1"), None);
    assert_eq!(code, Some(0), "setup: {rep}");
    let listed = std::fs::read_to_string(f.out_root.join("op-1/openfds")).unwrap();
    let allowed = |t: &str| {
        t == "/dev/null"
            || t.starts_with("pipe:")
            || t.ends_with("/openfds")
            || t == f.base.join("launcher.sh").to_str().unwrap()
            || t == f.base.join("manifest.json").to_str().unwrap()
            || t == f.out_root.join("op-1").to_str().unwrap()
    };
    let stray: Vec<&str> = listed.lines().filter(|t| !allowed(t)).collect();
    assert!(
        stray.is_empty(),
        "ATTACK: the root launcher inherited descriptors the helper opened for itself (not \
         close-on-exec): {stray:?} of {listed}"
    );
}
