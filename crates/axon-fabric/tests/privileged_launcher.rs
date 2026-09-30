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
/// the Fabric's out root with one launch's PSV inputs in it.
struct Fx {
    _d: tempfile::TempDir,
    base: PathBuf,
    cfg: PathBuf,
    out_root: PathBuf,
    marker: PathBuf,
    /// The setuid-root copy of the helper (root tests only).
    installed: Option<PathBuf>,
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
    std::fs::write(&launcher, stand_in(&base, extra)).unwrap();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out_root = base.join("runs");
    let cfg = write_helper_config(
        &base,
        &inputs,
        &launcher,
        &manifest,
        &out_root,
        "protected-launcher.json",
    );
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
    if let Some(u) = fabric {
        v["fabric_uid"] = json!(u);
    }
    edit(&mut v);
    std::fs::write(&cfg, v.to_string()).unwrap();
    // The Fabric's inputs for one launch: candidate, suite, job.
    let i = out_root.join(INPUTS);
    for (sub, name, body) in [
        ("candidate", "f.ax", "fn f() -> i64 { 1 }\n"),
        ("check", "accept.ax", "// suite\n"),
        ("job", "launch-manifest.json", "{}"),
    ] {
        std::fs::create_dir_all(i.join(sub)).unwrap();
        std::fs::write(i.join(sub).join(name), body).unwrap();
    }
    std::fs::write(i.join("job/completion-secret"), [7u8; 32]).unwrap();
    std::fs::set_permissions(
        i.join("job/completion-secret"),
        std::fs::Permissions::from_mode(0o400),
    )
    .unwrap();
    let installed = fabric.map(|u| {
        for p in walk(&out_root) {
            std::os::unix::fs::lchown(&p, Some(u), Some(u)).unwrap();
        }
        let h = base.join("axon-protected-launcher");
        std::fs::copy(helper_pin().path, &h).unwrap();
        std::os::unix::fs::chown(&h, Some(0), Some(u)).unwrap();
        std::fs::set_permissions(&h, std::fs::Permissions::from_mode(0o4750)).unwrap();
        h
    });
    Fx {
        marker: base.join("launched"),
        _d: d,
        base,
        cfg,
        out_root,
        installed,
    }
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
    fn request(&self, out: &str) -> Value {
        let i = self.out_root.join(INPUTS);
        json!({
            "schema": "axon-protected-launch-request/1",
            "id": "fab-0123456789abcdef",
            "out": self.out_root.join(out),
            "psv_candidate": i.join("candidate"),
            "psv_suite": i.join("check"),
            "psv_job": i.join("job"),
            "psv_manifest_sha256": sha256_file(&i.join("job/launch-manifest.json")),
            "policy_json": "{\"schema\":\"axon-vm-mmds/1\",\"allowed_effects\":[]}",
            "timeout_s": 60,
        })
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
