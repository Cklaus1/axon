//! Operator decision E at the FREEZE (amendment 44; C9 round 3, A79):
//! `scripts/v022_freeze_manifest.py` binds a candidate's evidence only from a
//! STANDALONE CLONE, and only a guest manifest that is clean with no reasons.
//! Its refusals had no test and no row (review EQUIVALENCE, round 3). These
//! run the real script, copied from THIS tree (so a mutation of it is what
//! runs), in a scratch repository.

#[path = "common/git_attacks.rs"]
mod git_attacks;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use script_spawn::Bins;

use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GIT: &str = "/usr/bin/git";
/// The script and the modules it loads.
const COPIED: [&str; 4] = [
    "scripts/v022_freeze_manifest.py",
    "scripts/v022_g01_mutations.py",
    "scripts/v022_attack_markers.py",
    "scripts/guest_build_env.py",
];
const MANIFEST: &str = "profiles/linux-microvm/manifest.json";

fn git(r: &Path, args: &[&str]) {
    let st = Command::new(GIT)
        .arg("-C")
        .arg(r)
        .args(["-c", "user.name=t", "-c", "user.email=t@example"])
        .args(args)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

const PARENT: &str = "/root/.cache/axon-guest-build";
const BASE: &str = "/root/.cache/axon-guest-build/axon-guest-build-x";
const KBASE: &str = "/root/.cache/axon-guest-build/axon-kernel-build-x";
const CRT: &str = "-C target-feature=+crt-static";

/// The recorded ancestors of PARENT: each only root's.
fn ancestors() -> serde_json::Value {
    json!([{"path": "/", "uid": 0, "mode": "0o755"},
           {"path": "/root", "uid": 0, "mode": "0o700"},
           {"path": "/root/.cache", "uid": 0, "mode": "0o755"},
           {"path": PARENT, "uid": 0, "mode": "0o700"}])
}

fn tool(path: &str) -> serde_json::Value {
    json!({"path": path, "realpath": path, "sha256": "7".repeat(64), "version": "t 1"})
}

/// The per-build key the fixture's runner "made": under the builder-private
/// parent, 0400 in a 0700 `keys` directory, root's (the tests run as root, so
/// the fixture builder_uid 0 is the real owner). Written once per process.
const KEY_HEX: &str = "6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b";

fn fixture_key(id: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    // One writer at a time: the tests share the process and the directory.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let kd = Path::new(PARENT).join("keys");
    std::fs::create_dir_all(&kd).unwrap();
    std::fs::set_permissions(PARENT, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&kd, std::fs::Permissions::from_mode(0o700)).unwrap();
    let p = kd.join(format!("{id}.key"));
    let tmp = kd.join(format!("{id}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, KEY_HEX).unwrap();
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o400)).unwrap();
    std::fs::rename(&tmp, &p).unwrap();
    p
}

/// `rec` with the proof the runner would have written: HMAC-SHA256, under the
/// build's key, of the compact sorted-key JSON of the record with the proof's
/// own schema and id (scripts/guest_build_env.py `proof_payload`).
fn signed(mut rec: serde_json::Value, id: &str) -> serde_json::Value {
    fixture_key(id);
    sign_with(&mut rec, id, KEY_HEX.as_bytes());
    rec
}

fn sign_with(rec: &mut serde_json::Value, id: &str, key: &[u8]) {
    rec["proof"] = json!({"schema": "axon-guest-build-proof/1", "id": id});
    let payload = rec.to_string();
    let k = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let tag = ring::hmac::sign(&k, payload.as_bytes());
    let hex: String = tag.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    rec["proof"]["hmac"] = json!(hex);
}

/// Sign both build records of `m` as their runner would after the edit a test
/// just made: a row that lets an edited record through the STRUCTURAL judge
/// must reach the verdict, not be refused by the proof the edit broke (that
/// would be REFUSED_ELSEWHERE). Only the proof's own test writes edits unsigned.
fn resign(m: &mut serde_json::Value) {
    for (part, id) in [
        ("source", "axon-guest-build-x"),
        ("kernel", "axon-kernel-build-x"),
    ] {
        let be = &mut m[part]["build_environment"];
        if be.is_object() {
            fixture_key(id);
            sign_with(be, id, KEY_HEX.as_bytes());
        }
    }
}

/// The record scripts/guest_build_env.py `kernel` writes for a controlled
/// build of the fixture manifest's vmlinux from its pins.
fn controlled_kernel() -> serde_json::Value {
    signed(controlled_kernel_unsigned(), "axon-kernel-build-x")
}

fn controlled_kernel_unsigned() -> serde_json::Value {
    json!({
        "schema": "axon-guest-kernel-build/1", "controlled": true, "builder_uid": 0,
        "build_parent": PARENT, "build_parent_ancestors": ancestors(), "base": KBASE,
        "pin": {"version": "6.1.188", "tarball_sha256": "3".repeat(64),
                "config_sha256": "4".repeat(64), "overlay_sha256": "5".repeat(64)},
        "env": {"HOME": KBASE, "LC_ALL": "C", "PATH": "/usr/bin:/bin",
                "KBUILD_BUILD_TIMESTAMP": "1970-01-01", "KBUILD_BUILD_USER": "axon",
                "KBUILD_BUILD_HOST": "b263", "KBUILD_BUILD_VERSION": "1"},
        "make": [["/usr/bin/make", "ARCH=x86_64", "olddefconfig"],
                 ["/usr/bin/make", "ARCH=x86_64", "-j8", "vmlinux"]],
        "tools": {"make": tool("/usr/bin/make"), "gcc": tool("/usr/bin/gcc"),
                  "cc1": tool("/usr/libexec/gcc/x86_64-linux-gnu/15/cc1"),
                  "as": tool("/usr/bin/as"), "ld": tool("/usr/bin/ld")},
        "effective_config_sha256": "6".repeat(64),
        "vmlinux_sha256": "1".repeat(64),
    })
}

/// A guest manifest whose source block is `source` (every other component
/// the fixture's controlled one).
fn manifest(source: serde_json::Value) -> String {
    manifest_value(source).to_string()
}

fn manifest_value(source: serde_json::Value) -> serde_json::Value {
    let mut m = manifest_value_unsigned(source);
    resign(&mut m);
    m
}

fn manifest_value_unsigned(source: serde_json::Value) -> serde_json::Value {
    json!({"artifacts": {"vmlinux": {"sha256": "1".repeat(64)},
                         "rootfs.sqfs": {"sha256": "2".repeat(64)},
                         "axon": {"sha256": "a".repeat(64)},
                         "axon-guest-init": {"sha256": "b".repeat(64)},
                         "axon-psv-runner": {"sha256": "c".repeat(64)}},
           "kernel": {"version": "6.1.188", "tarball_sha256": "3".repeat(64),
                      "config_sha256": "4".repeat(64), "overlay_sha256": "5".repeat(64),
                      "effective_config_sha256": "6".repeat(64),
                      "build_environment": controlled_kernel()},
           "busybox": {"sha256": "9".repeat(64)},
           "guest_init": {"sha256": "8".repeat(64)},
           "source": source})
}

/// The record scripts/guest_build_env.py writes for a controlled build of
/// exactly the fixture manifest's three binaries and its rootfs.
fn controlled_build() -> serde_json::Value {
    signed(controlled_build_unsigned(), "axon-guest-build-x")
}

fn controlled_build_unsigned() -> serde_json::Value {
    let tc = "/root/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/bin";
    let check = json!({"origins": [], "foreign": []});
    let musl = "x86_64-unknown-linux-musl";
    let build = |name: &str, args: Vec<&str>| {
        json!({"name": name, "args": args, "rustflags": CRT,
               "config_before": check.clone(), "config_after": check.clone()})
    };
    json!({
        "schema": "axon-guest-build-env/2",
        "controlled": true,
        "toolchain": {"channel": "nightly", "cargo": format!("{tc}/cargo"),
                      "cargo_sha256": "d".repeat(64), "cargo_version": "cargo 1",
                      "rustc": format!("{tc}/rustc"), "rustc_sha256": "e".repeat(64),
                      "rustc_vV": "rustc 1\nhost: x86_64-unknown-linux-gnu",
                      "host_tools": {"cc": tool("/usr/bin/cc"), "ld": tool("/usr/bin/ld")}},
        "env": {"CARGO_HOME": format!("{BASE}/cargo-home"),
                "CARGO_TARGET_DIR": format!("{BASE}/target"), "HOME": BASE,
                "LC_ALL": "C", "PATH": format!("{tc}:/usr/bin:/bin"),
                "RUSTC": format!("{tc}/rustc")},
        "env_allowlist": ["CARGO_HOME", "CARGO_TARGET_DIR", "HOME", "LC_ALL", "PATH", "RUSTC"],
        "proxy_vars": [],
        "builder_uid": 0, "build_parent": PARENT, "build_parent_ancestors": ancestors(),
        "src_dir": format!("{BASE}/src"), "src_files": 1,
        "cargo_home": format!("{BASE}/cargo-home"), "cargo_home_created_empty": true,
        "target_dir": format!("{BASE}/target"), "target_dir_created_empty": true,
        "effective_config": {"origins": [], "foreign": [], "own_config": ".cargo/config.toml"},
        "builds": [
            build("axon", vec!["build", "--locked", "-p", "axon-core", "--target", musl,
                               "--no-default-features", "--bin", "axon", "--release", "--quiet"]),
            build("axon-guest-init", vec!["build", "--locked", "-p", "axon-guest-init",
                                          "--target", musl, "--release", "--quiet"]),
            build("axon-psv-runner", vec!["build", "--locked", "-p", "axon-psv", "--bin",
                                          "axon-psv-runner", "--target", musl, "--release",
                                          "--quiet"]),
        ],
        "artifacts": {"axon": "a".repeat(64), "axon-guest-init": "b".repeat(64),
                      "axon-psv-runner": "c".repeat(64)},
        "rootfs": {
            "tool": tool("/usr/bin/mksquashfs"),
            "argv": ["/usr/bin/mksquashfs", format!("{BASE}/rootfs-x"), "/r/dist/rootfs.sqfs",
                     "-noappend", "-all-root", "-no-xattrs", "-mkfs-time", "0", "-all-time", "0",
                     "-comp", "gzip", "-quiet"],
            "env": {"HOME": BASE, "LC_ALL": "C", "PATH": "/usr/bin:/bin"},
            "inputs": {"axon": "a".repeat(64), "axon-guest-init": "b".repeat(64),
                       "axon-psv-runner": "c".repeat(64), "busybox": "9".repeat(64),
                       "guest-init.sh": "8".repeat(64)},
            "sha256": "2".repeat(64),
        },
    })
}

fn clean_source() -> serde_json::Value {
    json!({"axon_git_rev_at_build": "0".repeat(40), "axon_tree_dirty_at_build": false,
           "axon_tree_dirty_reasons": [], "build_environment": controlled_build()})
}

/// The files a freeze reads, under `root`.
fn populate(root: &Path) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for f in COPIED {
        write(
            &root.join(f),
            &std::fs::read_to_string(src.join(f)).unwrap(),
        );
    }
    for f in [
        "governance/status/v022-psv-paired-disable.json",
        "governance/specs/v022-psv-protocol.md",
        "governance/specs/v022-psv-gap-map.md",
        "governance/specs/v022-psv-negative-matrix.md",
        "governance/specs/v022-protected-suite-verdict.md",
    ] {
        write(&root.join(f), "{}\n");
    }
    write(&root.join(MANIFEST), &manifest(clean_source()));
    // The pinned channel the build record's toolchain must be (amendment 63).
    write(
        &root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"nightly\"\n",
    );
    coverage_gate(root, "[]");
}

/// The refusal-site coverage gate the freeze consults (amendment 61). The
/// scratch repository holds no protected sources, so the real gate (run by
/// gate.sh over this tree) is stood in for by one whose verdict the test
/// chooses: `problems` is what `check(freeze=True)` returns, and a call
/// without freeze=True returns a problem, so a freeze that does not ask for
/// the freeze reading never passes.
fn coverage_gate(root: &Path, problems: &str) {
    write(
        &root.join("scripts/v022_refusal_coverage.py"),
        &format!(
            "OUT_OF_SCOPE = {{}}\n\
             def in_scope_files():\n    return []\n\
             def check(without=(), freeze=False, out=print):\n\
             \x20   return {problems} if freeze else ['the freeze did not ask for --freeze']\n"
        ),
    );
}

/// A committed standalone clone holding the freeze inputs.
fn clone(d: &Path) -> PathBuf {
    let r = d.join("repo");
    std::fs::create_dir_all(&r).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    populate(&r);
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "candidate"]);
    // Amendment 65: the operator pinned the clean fixture's host tools.
    write(
        &pin_file(&r),
        &operator_pin_of(&manifest_value(clean_source())).to_string(),
    );
    r
}

/// The rustc-wrapper variables the freeze refuses (a development sccache
/// run sets one). Each test states its own environment rather than inherit
/// the caller's, so a refusal it does not judge cannot answer first.
const WRAPPERS: [&str; 4] = [
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
];

/// Where a test puts the operator's host-toolchain pin it wants installed
/// (outside the clone): beside it. Absent: no pin is installed.
fn pin_file(root: &Path) -> PathBuf {
    root.parent().unwrap().join("host-toolchain-pin.json")
}

/// The operator's pin of the clean fixture's host tools, as the deployment
/// kit writes it (scripts/guest_build_env.py's own extraction, from this tree).
fn operator_pin_of(m: &serde_json::Value) -> serde_json::Value {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts");
    let o = Command::new("python3")
        .arg("-B")
        .arg("-c")
        .arg(
            "import json,sys; sys.path.insert(0, sys.argv[1]); import guest_build_env as g; \
             print(json.dumps(g.recorded_host_tools(json.loads(sys.argv[2]))))",
        )
        .arg(&src)
        .arg(m.to_string())
        .output()
        .unwrap();
    assert!(o.status.success(), "setup: {o:?}");
    let tools: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    json!({"schema": "axon-host-toolchain-pin/1", "status": "test", "tools": tools})
}

/// The freeze command from `root`, with no compiler wrapper in its env.
/// Amendment 65: it runs in a private mount namespace whose /etc/axon is an
/// empty root-owned tmpfs holding only [`pin_file`] (root-owned 0644) when
/// the test placed one, so the operator pin the freeze REQUIRES is the
/// test's, and the host's /etc is never written or read.
fn freeze_cmd(root: &Path) -> Command {
    assert!(
        unsafe { libc::geteuid() } == 0 && Path::new("/etc/axon").is_dir(),
        "setup: the freeze tests need root and an /etc/axon mount point (a private mount \
         namespace holds the operator's toolchain pin; /etc is never written)"
    );
    // The freeze runs git, never a binary this workspace builds. The private
    // namespace is a wrapper the spawn helper puts in front of the script.
    let pin = pin_file(root);
    let mut c = script_spawn::script_under(
        &[
            "unshare".as_ref(),
            "-m".as_ref(),
            "--propagation".as_ref(),
            "private".as_ref(),
            "sh".as_ref(),
            "-c".as_ref(),
            "set -e; mount -t tmpfs -o mode=0755 tmpfs /etc/axon; \
             if [ -e \"$1\" ]; then cp \"$1\" /etc/axon/host-toolchain-pin.json; \
             chown \"${PIN_OWNER:-0}\" /etc/axon/host-toolchain-pin.json; \
             chmod \"${PIN_MODE:-0644}\" /etc/axon/host-toolchain-pin.json; fi; \
             shift; exec \"$@\""
                .as_ref(),
            "sh".as_ref(),
            pin.as_os_str(),
        ],
        "python3",
        root.join("scripts/v022_freeze_manifest.py"),
        Bins::NoWorkspaceBinary,
    );
    c.arg("freeze.json")
        .arg(root.join("no-micode"))
        .current_dir(root)
        .env("V022_KEEP_TMPDIR", "1");
    for v in WRAPPERS {
        c.env_remove(v);
    }
    c
}

/// Run the freeze from `root`: Ok(stdout) or Err(stderr).
fn freeze(root: &Path) -> Result<String, String> {
    let o = freeze_cmd(root).output().unwrap();
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).to_string())
    }
}

fn refused(root: &Path, attack: &str, why: &str) {
    let got = freeze(root);
    assert!(
        got.is_err(),
        "ATTACK: the freeze bound evidence from {attack}: {got:?}"
    );
    let e = got.unwrap_err();
    assert!(e.contains(why), "expected {why:?}: {e}");
}

#[test]
fn a_standalone_clone_with_a_clean_guest_manifest_freezes() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let got = freeze(&r);
    assert!(got.is_ok(), "control: a standalone clone freezes: {got:?}");
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(r.join("freeze.json")).unwrap()).unwrap();
    assert_eq!(m["schema"], "axon-v022-psv-freeze/1");
}

/// A freeze is never made through a compiler wrapper (sccache is for
/// development runs only): each wrapper variable alone refuses it. Control:
/// the same clone with none set freezes.
#[test]
fn a_compiler_wrapper_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    for v in WRAPPERS {
        let o = freeze_cmd(&r).env(v, "sccache").output().unwrap();
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(
            !o.status.success(),
            "ATTACK: a freeze was made through a compiler wrapper ({v}=sccache)"
        );
        assert!(e.contains("compiler wrapper"), "{v}: {e}");
    }
    assert!(
        freeze(&r).is_ok(),
        "control: with no wrapper the clone freezes"
    );
}

#[test]
fn a_linked_worktree_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let wt = d.path().join("wt");
    git(
        &r,
        &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
    );
    refused(
        &wt,
        "a linked worktree (a gitfile names the repository)",
        "not a standalone clone",
    );
}

/// A79: the linked worktree's admin directory copied in as a real `.git`
/// (`commondir` naming the clone's). `os.path.isdir` passed it; the
/// repository git acts on is another one.
#[test]
fn a_linked_worktree_disguised_as_a_git_directory_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let wt = d.path().join("wt");
    git_attacks::disguise_worktree(&r, &wt);
    refused(
        &wt,
        "a linked worktree whose admin dir was placed as .git",
        "a linked worktree's git dir",
    );
}

/// `.git` a symlink to a clone's git dir: the repository is chosen
/// elsewhere, whatever git then answers.
#[test]
fn a_symlinked_git_dir_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let s = d.path().join("linked");
    std::fs::create_dir_all(&s).unwrap();
    populate(&s);
    std::os::unix::fs::symlink(r.join(".git"), s.join(".git")).unwrap();
    refused(
        &s,
        "a tree whose .git is a symlink to another clone's",
        ".git is not a real directory",
    );
}

#[test]
fn a_dirty_guest_manifest_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let mut src = clean_source();
    src["axon_tree_dirty_at_build"] = json!(true);
    write(&r.join(MANIFEST), &manifest(src));
    refused(
        &r,
        "a guest image built from a dirty tree",
        "the guest manifest is not clean",
    );
}

#[test]
fn a_guest_manifest_with_dirty_reasons_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    for reasons in [json!(["untracked file: build.rs"]), json!(null)] {
        let mut src = clean_source();
        src["axon_tree_dirty_reasons"] = reasons.clone();
        write(&r.join(MANIFEST), &manifest(src));
        refused(
            &r,
            &format!("a guest manifest marked clean with reasons {reasons}"),
            "the guest manifest is not clean",
        );
    }
}

/// The freeze binds `axon_sha` from the operator's git with the caller's
/// environment dropped: a GIT_DIR naming another repository does not choose
/// which HEAD is bound. Control: the clone's own HEAD.
#[test]
fn the_callers_git_environment_does_not_choose_the_bound_revision() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let other = d.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    git(
        &other,
        &["commit", "-q", "--allow-empty", "-m", "elsewhere"],
    );
    let head = git_attacks::rev(&r, "HEAD");
    let o = freeze_cmd(&r)
        .env("GIT_DIR", other.join(".git"))
        .status()
        .unwrap();
    assert!(o.success(), "control: the clone freezes");
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(r.join("freeze.json")).unwrap()).unwrap();
    assert_eq!(
        m["axon_sha"], head,
        "ATTACK: the caller's GIT_DIR chose the revision the freeze bound"
    );
}

/// C9 round 4 (FIELD-ORIGIN, PSV-2): the freeze binds only a guest image whose
/// bytes were built in the controlled environment scripts/guest_build_env.py
/// constructs. A list of wrapper variables checked in the FREEZE's own
/// environment said nothing about the environment that BUILT the image: an
/// ancestor config, RUSTC, RUSTFLAGS, a linker or a reused target dir all
/// passed it. Each uncontrolled build below is refused; the control (the
/// controlled record) freezes (a_standalone_clone_with_a_clean_guest_manifest_freezes).
#[test]
fn a_guest_image_not_built_in_the_controlled_environment_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    type Edit = Box<dyn Fn(&mut serde_json::Value)>;
    let cases: Vec<(&str, Edit)> = vec![
        (
            "a caller's RUSTC_WRAPPER reached cargo",
            Box::new(|b| {
                b["env"]["RUSTC_WRAPPER"] = json!("/usr/bin/sccache");
            }),
        ),
        (
            "a RUSTC other than the pinned toolchain's",
            Box::new(|b| {
                b["env"]["RUSTC"] = json!("/tmp/evil/rustc");
            }),
        ),
        (
            "an ancestor .cargo/config.toml named a wrapper",
            Box::new(|b| {
                let foreign =
                    json!(["build.rustc-wrapper = \"/w\" (from /var/tmp/.cargo/config.toml)"]);
                b["effective_config"]["foreign"] = foreign.clone();
                // Self-consistent: every invocation saw the same config as
                // begin (amendment 63's per-invocation check holds), so only
                // the begin-time judge can refuse it.
                for e in b["builds"].as_array_mut().unwrap() {
                    for k in ["config_before", "config_after"] {
                        e[k]["foreign"] = foreign.clone();
                    }
                }
            }),
        ),
        (
            "a reused target dir",
            Box::new(|b| {
                b["target_dir_created_empty"] = json!(false);
            }),
        ),
        (
            "a CARGO_HOME with config in it",
            Box::new(|b| {
                b["cargo_home_created_empty"] = json!(false);
            }),
        ),
        (
            "PATH led by a directory other than the pinned toolchain's",
            Box::new(|b| {
                b["env"]["PATH"] = json!("/tmp/evil/bin:/usr/bin:/bin");
            }),
        ),
        // Last: with no record at all the artifact binding refuses too, so
        // the record gate's own row is judged by the cases above.
        (
            "no build environment recorded (a bare cargo build)",
            Box::new(|b| *b = json!(null)),
        ),
    ];
    for (attack, edit) in cases {
        let mut src = clean_source();
        edit(&mut src["build_environment"]);
        write(&r.join(MANIFEST), &manifest(src));
        let got = freeze(&r);
        assert!(
            got.is_err(),
            "ATTACK: the freeze bound a guest image not built in the controlled environment \
             ({attack}): {got:?}"
        );
        let e = got.unwrap_err();
        assert!(e.contains("controlled build environment"), "{attack}: {e}");
    }
    write(&r.join(MANIFEST), &manifest(clean_source()));
    assert!(freeze(&r).is_ok(), "control: the controlled build freezes");
}

/// The controlled build's record is joined to the manifest: an artifact the
/// manifest pins must be the bytes that build produced. Control above.
#[test]
fn a_guest_artifact_the_controlled_build_did_not_produce_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    for name in ["axon", "axon-guest-init", "axon-psv-runner"] {
        let mut src = clean_source();
        // The record stays self-consistent (its rootfs was assembled from the
        // bytes it built): only the manifest's pin differs from what it made.
        src["build_environment"]["artifacts"][name] = json!("f".repeat(64));
        src["build_environment"]["rootfs"]["inputs"][name] = json!("f".repeat(64));
        write(&r.join(MANIFEST), &manifest(src));
        let got = freeze(&r);
        assert!(
            got.is_err(),
            "ATTACK: the freeze bound a guest {name} its controlled build did not produce: {got:?}"
        );
        assert!(got
            .unwrap_err()
            .contains("not the bytes its controlled build"));
    }
}

/// C9 round 5 (FIELD-ORIGIN, major-adjacent 2): the record the freeze judges
/// is the controlled RUNNER's, not a file anybody can write. Each record below
/// is structurally a controlled build's (every field the other judges read
/// holds, digests consistent between the record and the manifest) and is
/// refused by the builder's proof alone: no proof at all, the digests of a
/// binary built elsewhere written into a signed record, a proof under a key the
/// builder did not make, a kernel record edited after signing, a proof naming a
/// key that is not there. Control: the signed fixture freezes.
#[test]
fn a_guest_build_record_its_runner_did_not_sign_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let elsewhere = "e".repeat(64);
    type Edit = Box<dyn Fn(&mut serde_json::Value)>;
    let cases: Vec<(&str, &str, Edit)> = vec![
        (
            "a hand-written record with no proof",
            "carries no builder proof",
            Box::new(|m| {
                m["source"]["build_environment"]
                    .as_object_mut()
                    .unwrap()
                    .remove("proof");
            }),
        ),
        (
            "a signed record whose axon digest was rewritten to a binary built elsewhere",
            "does not hold",
            Box::new({
                let e = elsewhere.clone();
                move |m| {
                    m["artifacts"]["axon"]["sha256"] = json!(e);
                    m["source"]["build_environment"]["artifacts"]["axon"] = json!(e);
                    m["source"]["build_environment"]["rootfs"]["inputs"]["axon"] = json!(e);
                }
            }),
        ),
        (
            "a record signed under a key the builder did not make",
            "does not hold",
            Box::new(|m| {
                sign_with(
                    &mut m["source"]["build_environment"],
                    "axon-guest-build-x",
                    b"not the builders key",
                );
            }),
        ),
        (
            "a proof naming a key that is not there",
            "cannot be checked",
            Box::new(|m| {
                sign_with(
                    &mut m["source"]["build_environment"],
                    "no-such-build",
                    KEY_HEX.as_bytes(),
                );
            }),
        ),
        (
            "a signed kernel record whose vmlinux digest was rewritten",
            "kernel build record's proof does not hold",
            Box::new({
                let e = elsewhere.clone();
                move |m| {
                    m["artifacts"]["vmlinux"]["sha256"] = json!(e);
                    m["kernel"]["build_environment"]["vmlinux_sha256"] = json!(e);
                }
            }),
        ),
    ];
    fixture_key("axon-guest-build-x");
    for (attack, why, edit) in cases {
        let mut m = manifest_value(clean_source());
        edit(&mut m);
        write(&r.join(MANIFEST), &m.to_string());
        let got = freeze(&r);
        assert!(
            got.is_err(),
            "ATTACK: the freeze bound a guest image whose build record its runner did not sign \
             ({attack}): {got:?}"
        );
        let e = got.unwrap_err();
        assert!(e.contains(why), "{attack}: expected {why:?}: {e}");
    }
    write(&r.join(MANIFEST), &manifest(clean_source()));
    assert!(freeze(&r).is_ok(), "control: the signed image freezes");
}

type ManifestEdit = Box<dyn Fn(&mut serde_json::Value)>;

/// Each edit of the controlled manifest is refused by the freeze with `why`
/// in its message; `claim` names what the freeze would have bound. Control:
/// the unedited manifest freezes.
fn each_refused(claim: &str, why: &str, cases: Vec<(&str, ManifestEdit)>) {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    for (attack, edit) in cases {
        let mut m = manifest_value(clean_source());
        edit(&mut m);
        resign(&mut m);
        write(&r.join(MANIFEST), &m.to_string());
        let got = freeze(&r);
        assert!(
            got.is_err(),
            "ATTACK: the freeze bound {claim} ({attack}): {got:?}"
        );
        let e = got.unwrap_err();
        assert!(e.contains(why), "{attack}: {e}");
    }
    write(&r.join(MANIFEST), &manifest(clean_source()));
    assert!(freeze(&r).is_ok(), "control: the controlled image freezes");
}

/// The recorded build of the runner (index 2) or another protected binary.
fn build(m: &mut serde_json::Value, i: usize) -> &mut serde_json::Value {
    &mut m["source"]["build_environment"]["builds"][i]
}

/// C9 round 4b, finding 2: the judge reads every recorded cargo invocation:
/// args and RUSTFLAGS must be exactly the table's. A record showing a linker
/// in RUSTFLAGS, a `--config` wrapper or a dropped `--locked` was accepted.
#[test]
fn a_recorded_guest_build_with_other_args_or_flags_does_not_freeze() {
    each_refused(
        "a guest build whose recorded invocation was not its table's",
        "not its controlled invocation",
        vec![
            (
                "extra RUSTFLAGS naming a linker",
                Box::new(|m| {
                    build(m, 2)["rustflags"] =
                        json!("-C linker=/var/tmp/evil-ld -C target-feature=+crt-static");
                }),
            ),
            (
                "a --config naming a rustc wrapper",
                Box::new(|m| {
                    let a = build(m, 2)["args"].as_array_mut().unwrap();
                    a.insert(1, json!("build.rustc-wrapper=\"/var/tmp/evil\""));
                    a.insert(1, json!("--config"));
                }),
            ),
            (
                "a build without --locked",
                Box::new(|m| {
                    build(m, 0)["args"].as_array_mut().unwrap().remove(1);
                }),
            ),
            (
                "no RUSTFLAGS recorded",
                Box::new(|m| {
                    build(m, 1)["rustflags"] = json!(null);
                }),
            ),
        ],
    );
}

/// C9 round 4b, finding 1: each recorded invocation carries the effective
/// config check before AND after it, equal to begin's.
#[test]
fn a_guest_build_record_without_a_config_check_around_each_invocation_does_not_freeze() {
    each_refused(
        "a guest build whose invocations were not each held to begin's config",
        "no effective-config check equal to begin's",
        vec![
            (
                "a config that appeared during the runner's build",
                Box::new(|m| {
                    build(m, 2)["config_after"] = json!({"origins": ["/var/tmp/.cargo/config.toml"],
                        "foreign": ["build.rustc-wrapper (from /var/tmp/.cargo/config.toml)"]});
                }),
            ),
            (
                "an invocation with no check after it",
                Box::new(|m| {
                    build(m, 2).as_object_mut().unwrap().remove("config_after");
                }),
            ),
            (
                "an invocation with no check before it",
                Box::new(|m| {
                    build(m, 0).as_object_mut().unwrap().remove("config_before");
                }),
            ),
        ],
    );
}

/// C9 round 4b, finding 2: the binaries are exactly the protected builds, in
/// order -- not a record missing one, or carrying another.
#[test]
fn a_guest_image_whose_binaries_were_not_exactly_the_protected_builds_does_not_freeze() {
    each_refused(
        "a guest image whose binaries were not exactly the protected builds",
        "not exactly the protected builds",
        vec![
            (
                "no recorded build of the runner",
                Box::new(|m| {
                    m["source"]["build_environment"]["builds"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }),
            ),
            (
                "an extra development build",
                Box::new(|m| {
                    let b = json!({"name": "dev:initramfs-guest-init",
                        "args": ["build", "-p", "axon-guest-init", "--target",
                                 "x86_64-unknown-linux-musl", "--release", "--quiet"],
                        "rustflags": CRT,
                        "config_before": {"origins": [], "foreign": []},
                        "config_after": {"origins": [], "foreign": []}});
                    m["source"]["build_environment"]["builds"]
                        .as_array_mut()
                        .unwrap()
                        .push(b);
                }),
            ),
            (
                "the builds out of order",
                Box::new(|m| {
                    m["source"]["build_environment"]["builds"]
                        .as_array_mut()
                        .unwrap()
                        .swap(0, 2);
                }),
            ),
        ],
    );
}

/// C9 round 4b, finding 2: the recorded toolchain is the channel the tree
/// pins, and the record names the host linker's identity.
#[test]
fn a_guest_build_record_not_of_the_pinned_toolchain_does_not_freeze() {
    fn retool(m: &mut serde_json::Value, chan: &str, dir: &str) {
        let b = &mut m["source"]["build_environment"];
        b["toolchain"]["channel"] = json!(chan);
        b["toolchain"]["cargo"] = json!(format!("{dir}/cargo"));
        b["toolchain"]["rustc"] = json!(format!("{dir}/rustc"));
        b["env"]["PATH"] = json!(format!("{dir}:/usr/bin:/bin"));
        b["env"]["RUSTC"] = json!(format!("{dir}/rustc"));
    }
    each_refused(
        "a guest build not of the pinned toolchain",
        "not the pinned channel",
        vec![
            (
                "another channel's toolchain",
                Box::new(|m| {
                    retool(
                        m,
                        "stable",
                        "/root/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin",
                    )
                }),
            ),
            (
                "the pinned channel's name on another toolchain directory",
                Box::new(|m| retool(m, "nightly", "/var/tmp/evil/bin")),
            ),
            (
                "no linker identity",
                Box::new(|m| {
                    m["source"]["build_environment"]["toolchain"]["host_tools"] = json!({});
                }),
            ),
        ],
    );
}

/// C9 round 4b, finding 1: cargo ran on a private copy of the tree in a
/// directory whose every ancestor only root or the builder could write.
#[test]
fn a_guest_build_record_not_made_in_a_private_copy_does_not_freeze() {
    each_refused(
        "a guest build not made in a private copy",
        "private copy of the tree",
        vec![
            (
                "an ancestor any uid could write (a sticky /var/tmp)",
                Box::new(|m| {
                    m["source"]["build_environment"]["build_parent_ancestors"][2]["mode"] =
                        json!("0o1777");
                }),
            ),
            (
                "cargo run in the clone",
                Box::new(|m| {
                    m["source"]["build_environment"]["src_dir"] = json!("/var/tmp/clone");
                }),
            ),
            (
                "an ancestor owned by another uid",
                Box::new(|m| {
                    m["source"]["build_environment"]["build_parent_ancestors"][3]["uid"] =
                        json!(1000);
                }),
            ),
            (
                "ancestors that are not the parent's",
                Box::new(|m| {
                    m["source"]["build_environment"]["build_parent_ancestors"]
                        .as_array_mut()
                        .unwrap()
                        .remove(2);
                }),
            ),
        ],
    );
}

/// C9 round 4b, finding 3: vmlinux and rootfs.sqfs were made outside the
/// controlled environment (the caller's gcc, KCFLAGS, PATH; the caller's
/// mksquashfs), yet the freeze bound them. Every component is now the bytes
/// of a controlled step from the manifest's pins.
#[test]
fn a_guest_component_built_outside_the_controlled_environment_does_not_freeze() {
    fn kernel(m: &mut serde_json::Value) -> &mut serde_json::Value {
        &mut m["kernel"]["build_environment"]
    }
    fn rootfs(m: &mut serde_json::Value) -> &mut serde_json::Value {
        &mut m["source"]["build_environment"]["rootfs"]
    }
    each_refused(
        "a guest image component built outside the controlled environment",
        "produced outside the controlled build",
        vec![
            (
                "no kernel build record (vmlinux built outside it)",
                Box::new(|m| *kernel(m) = json!(null)),
            ),
            (
                "a vmlinux other than the controlled kernel build's",
                Box::new(|m| m["artifacts"]["vmlinux"]["sha256"] = json!("7".repeat(64))),
            ),
            (
                "a kernel built from a tarball other than the manifest's pin",
                Box::new(|m| kernel(m)["pin"]["tarball_sha256"] = json!("0".repeat(64))),
            ),
            (
                "a kernel make run with the caller's KCFLAGS",
                Box::new(|m| kernel(m)["env"]["KCFLAGS"] = json!("-DEVIL")),
            ),
            (
                "a kernel make with an extra argument",
                Box::new(|m| {
                    kernel(m)["make"][1]
                        .as_array_mut()
                        .unwrap()
                        .push(json!("CC=/var/tmp/evil-gcc"));
                }),
            ),
            (
                "a kernel build with no recorded gcc",
                Box::new(|m| {
                    kernel(m)["tools"].as_object_mut().unwrap().remove("gcc");
                }),
            ),
            (
                "a kernel built under a directory another uid could write",
                Box::new(|m| kernel(m)["build_parent_ancestors"][2]["mode"] = json!("0o1777")),
            ),
            (
                "a rootfs.sqfs other than the controlled assembly's",
                Box::new(|m| m["artifacts"]["rootfs.sqfs"]["sha256"] = json!("7".repeat(64))),
            ),
            (
                "a rootfs assembled from another busybox",
                Box::new(|m| rootfs(m)["inputs"]["busybox"] = json!("0".repeat(64))),
            ),
            (
                "a rootfs assembled from a runner the build did not produce",
                Box::new(|m| rootfs(m)["inputs"]["axon-psv-runner"] = json!("0".repeat(64))),
            ),
            (
                "no rootfs assembly recorded",
                Box::new(|m| *rootfs(m) = json!(null)),
            ),
            (
                "a rootfs made by a mksquashfs on the caller's PATH",
                Box::new(|m| {
                    rootfs(m)["tool"]["path"] = json!("/var/tmp/evil/mksquashfs");
                    rootfs(m)["argv"][0] = json!("/var/tmp/evil/mksquashfs");
                }),
            ),
            (
                "a rootfs made with other mksquashfs flags",
                Box::new(|m| rootfs(m)["argv"][4] = json!("-keep-as-directory")),
            ),
            (
                "a rootfs made in the caller's environment",
                Box::new(|m| rootfs(m)["env"]["LD_PRELOAD"] = json!("/var/tmp/evil.so")),
            ),
        ],
    );
}

/// Amendment 61: a freeze is refused while the refusal-site coverage gate does
/// not hold at a freeze (an in-scope protected file NOT YET SCANNED), so no
/// freeze binds evidence over a decision file nobody scanned. Control: the
/// same clone with the gate holding freezes
/// (a_standalone_clone_with_a_clean_guest_manifest_freezes).
#[test]
fn a_freeze_is_refused_while_a_protected_file_is_not_yet_scanned() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    coverage_gate(
        &r,
        "['crates/axon-loop/src/ledger.rs: NOT YET SCANNED at a freeze (18 uncovered sites)']",
    );
    git(&r, &["commit", "-q", "-am", "gate"]);
    refused(
        &r,
        "a tree whose refusal-site coverage does not hold at a freeze",
        "refusal-site coverage gate does not hold at a freeze",
    );
}

/// Amendment 64: the freeze asks the gate for its FREEZE reading. A gate that
/// holds day to day but not at a freeze (a protected file NOT YET SCANNED is
/// fine between freezes, never at one) refuses the freeze; a freeze that asked
/// for the ordinary reading would bind it. Control: the same clone with the
/// gate holding at a freeze freezes
/// (a_standalone_clone_with_a_clean_guest_manifest_freezes).
#[test]
fn a_freeze_asks_the_gate_for_its_freeze_reading() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    write(
        &r.join("scripts/v022_refusal_coverage.py"),
        "OUT_OF_SCOPE = {}\n\
         def in_scope_files():\n    return []\n\
         def check(without=(), freeze=False, out=print):\n\
         \x20   return ['crates/axon-loop/src/tel.rs: NOT YET SCANNED at a freeze (1 uncovered sites)'] \
         if freeze else []\n",
    );
    git(&r, &["commit", "-q", "-am", "gate"]);
    refused(
        &r,
        "a tree whose refusal-site coverage holds only outside a freeze",
        "refusal-site coverage gate does not hold at a freeze",
    );
}

/// Amendment 65 (M1470-M1473): the freeze binds only a guest image whose
/// recorded host tools are the OPERATOR's pin (/etc/axon/host-toolchain-pin.json,
/// written by the deployment kit), which it requires. Each case is refused:
/// a build whose rustc is not the pinned one (M1470); a build that recorded a
/// tool the pin does not name (M1471); no pin at all (M1472); and a pin that
/// matches a tampered build but is written by another uid, or writable by
/// one (M1473). Control: the operator's pin of the clean build freezes.
#[test]
fn a_guest_image_not_built_with_the_operators_pinned_tools_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let pin = pin_file(&r);
    let clean_pin = std::fs::read_to_string(&pin).unwrap();
    let tampered = |m: &mut serde_json::Value| {
        m["source"]["build_environment"]["toolchain"]["rustc_sha256"] = json!("d".repeat(64));
    };
    // M1470: the build ran another rustc than the operator pinned.
    let mut m = manifest_value(clean_source());
    tampered(&mut m);
    resign(&mut m);
    write(&r.join(MANIFEST), &m.to_string());
    let got = freeze(&r);
    assert!(
        got.is_err(),
        "ATTACK: the freeze bound a guest image built with a rustc other than the operator's \
         pinned one: {got:?}"
    );
    assert!(got.unwrap_err().contains("not the operator's pin"));
    // M1471: the build recorded a host tool the operator never pinned.
    let mut m = manifest_value(clean_source());
    m["kernel"]["build_environment"]["tools"]["flex"] = json!({"path": "/var/tmp/flex",
        "realpath": "/var/tmp/flex", "sha256": "f".repeat(64), "version": "x"});
    resign(&mut m);
    write(&r.join(MANIFEST), &m.to_string());
    let got = freeze(&r);
    assert!(
        got.is_err(),
        "ATTACK: the freeze bound a guest image whose build recorded a host tool the operator's \
         pin does not name: {got:?}"
    );
    assert!(got.unwrap_err().contains("does not name"));
    write(&r.join(MANIFEST), &manifest(clean_source()));
    // M1472: no operator pin.
    std::fs::remove_file(&pin).unwrap();
    let got = freeze(&r);
    assert!(
        got.is_err(),
        "ATTACK: the freeze bound a guest image with no operator host-toolchain pin to judge its \
         tools: {got:?}"
    );
    assert!(got.unwrap_err().contains("no operator host-toolchain pin"));
    // M1473: a pin naming the tampered build's rustc, written by another uid
    // (or writable by one).
    let mut m = manifest_value(clean_source());
    tampered(&mut m);
    resign(&mut m);
    write(&r.join(MANIFEST), &m.to_string());
    write(&pin, &operator_pin_of(&m).to_string());
    for (owner, mode) in [("4242", "0644"), ("0", "0666")] {
        let o = freeze_cmd(&r)
            .env("PIN_OWNER", owner)
            .env("PIN_MODE", mode)
            .output()
            .unwrap();
        assert!(
            !o.status.success(),
            "ATTACK: the freeze accepted a host-toolchain pin owned by uid {owner} with mode \
             {mode} (whoever writes it chooses the tools a freeze binds)"
        );
        assert!(
            String::from_utf8_lossy(&o.stderr).contains("is not the operator's"),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
    // Control: the operator's pin of the clean build.
    write(&pin, &clean_pin);
    write(&r.join(MANIFEST), &manifest(clean_source()));
    let got = freeze(&r);
    assert!(
        got.is_ok(),
        "control: the operator's pin of the clean build freezes: {got:?}"
    );
}
