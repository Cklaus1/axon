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

/// The record scripts/guest_build_env.py `kernel` writes for a controlled
/// build of the fixture manifest's vmlinux from its pins.
fn controlled_kernel() -> serde_json::Value {
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
}

/// A committed standalone clone holding the freeze inputs.
fn clone(d: &Path) -> PathBuf {
    let r = d.join("repo");
    std::fs::create_dir_all(&r).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    populate(&r);
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "candidate"]);
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

/// The freeze command from `root`, with no compiler wrapper in its env.
fn freeze_cmd(root: &Path) -> Command {
    // The freeze runs git, never a binary this workspace builds.
    let mut c = script_spawn::script(
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
