//! Dev review round wf_bf757240-925 (FIELD-ORIGIN): the pinned launcher runs
//! as root, and its inline Python must never import from the caller's working
//! directory (`sys.path[0]` is the cwd for `python3 -c` / `python3 -`). Every
//! call is isolated (`python3 -I`). Control: the same launcher with `-I`
//! stripped does import the planted module.

#[path = "common/exec.rs"]
mod exec;
use exec::write_executable;
use std::path::Path;
use std::process::Command;

fn run_from_hostile_cwd(launcher: &Path) -> bool {
    let d = tempfile::tempdir().unwrap();
    let cwd = d.path().join("cwd");
    let vd = d.path().join("vd");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&vd).unwrap();
    let marker = d.path().join("imported-from-cwd");
    std::fs::write(
        cwd.join("json.py"),
        format!("open({:?}, 'w').write('x')\n", marker.display().to_string()),
    )
    .unwrap();
    for f in ["workspace.img", "serial.log", "result.json"] {
        std::fs::write(vd.join(f), "{}").unwrap();
    }
    let _ = Command::new("bash")
        .arg(launcher)
        .arg("--verify-result")
        .arg(&vd)
        .current_dir(&cwd)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .output()
        .unwrap();
    marker.exists()
}

#[test]
fn the_launchers_python_never_imports_from_the_callers_cwd() {
    let launcher = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/fc_linux_profile.sh");
    assert!(
        !run_from_hostile_cwd(&launcher),
        "the launcher imported a module from the caller's working directory"
    );
    // Control: without -I the planted module IS imported.
    let d = tempfile::tempdir().unwrap();
    let weak = d.path().join("launcher-no-isolation.sh");
    std::fs::write(
        &weak,
        std::fs::read_to_string(&launcher)
            .unwrap()
            .replace("python3 -I -", "python3 -"),
    )
    .unwrap();
    assert!(
        run_from_hostile_cwd(&weak),
        "control: the planted module is imported without -I"
    );
}

/// Set extended attribute `name` on `p`. A filesystem that cannot hold it
/// FAILS the test: a skipped plant would leave the guard untested.
fn plant_xattr(p: &Path, name: &str, value: &[u8]) {
    let c = |s: &str| std::ffi::CString::new(s).unwrap();
    let (cp, cn) = (c(p.to_str().unwrap()), c(name));
    let r = unsafe {
        libc::lsetxattr(
            cp.as_ptr(),
            cn.as_ptr(),
            value.as_ptr() as *const libc::c_void,
            value.len(),
            0,
        )
    };
    assert_eq!(
        r,
        0,
        "cannot set {name} on {} ({}): TMPDIR must hold user xattrs and POSIX \
         ACLs; this is a FAILURE, not a skip",
        p.display(),
        std::io::Error::last_os_error()
    );
}

/// A POSIX access ACL (v2) keeping mode 0644 (file) / 0755 (dir) with a named
/// entry `user:65534:---`.
fn acl_denying_nobody(dir: bool) -> Vec<u8> {
    let (o, r) = if dir { (7u16, 5u16) } else { (6, 4) };
    let mut a = 2u32.to_le_bytes().to_vec();
    for (tag, perm, id) in [
        (0x01u16, o, u32::MAX),
        (0x02, 0, 65534),
        (0x04, r, u32::MAX),
        (0x10, r, u32::MAX),
        (0x20, r, u32::MAX),
    ] {
        a.extend(tag.to_le_bytes());
        a.extend(perm.to_le_bytes());
        a.extend(id.to_le_bytes());
    }
    a
}

fn sbin(tool: &str) -> String {
    for d in ["/usr/sbin", "/sbin", "/usr/bin", "/bin"] {
        let p = format!("{d}/{tool}");
        if Path::new(&p).exists() {
            return p;
        }
    }
    panic!("{tool} is not installed: the launcher needs e2fsprogs; this is a FAILURE, not a skip")
}

/// What one run of the launcher's `psv_image` (its text, `script`) left:
/// the xattrs the STAGING tree held when mkfs was called, and the xattrs and
/// modes of the IMAGE, read back with debugfs.
struct Imaged {
    staged: String,
    image_xattrs: Vec<String>,
    image_ls: String,
}

fn run_psv_image(script: &str) -> Imaged {
    let start = script
        .find("\npsv_image() {")
        .expect("the launcher defines psv_image");
    let end = start + script[start..].find("\n}\n").expect("psv_image ends") + 3;
    let func = &script[start..end];

    let d = tempfile::tempdir().unwrap();
    let (src, out, wrap) = (
        d.path().join("src"),
        d.path().join("out"),
        d.path().join("wrap"),
    );
    for p in [&src, &src.join("lib"), &out, &wrap] {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(src.join("f.ax"), "fn f() {}\n").unwrap();
    std::fs::write(src.join("lib/run.sh"), "#!/bin/sh\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    for (p, m) in [
        (src.clone(), 0o755),
        (src.join("lib"), 0o755),
        (src.join("f.ax"), 0o644),
        (src.join("lib/run.sh"), 0o755),
    ] {
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(m)).unwrap();
    }
    // The reviewer's plant, plus every other shape: a file ACL, a directory
    // access + default ACL, a user.* xattr, and one on the ROOT of the tree.
    plant_xattr(
        &src.join("f.ax"),
        "system.posix_acl_access",
        &acl_denying_nobody(false),
    );
    plant_xattr(
        &src.join("lib"),
        "system.posix_acl_access",
        &acl_denying_nobody(true),
    );
    plant_xattr(
        &src.join("lib"),
        "system.posix_acl_default",
        &acl_denying_nobody(true),
    );
    plant_xattr(&src.join("lib/run.sh"), "user.planted", b"x");
    plant_xattr(&src, "user.root", b"x");

    // A `mkfs.ext4` ahead on PATH that records what the STAGING tree carries
    // (the copy's layer), then plants an attribute there — what an inherited
    // default ACL or a host LSM label would add — before the real mkfs runs
    // (mkfs's own layer must keep it out of the image).
    let staged = d.path().join("staged.txt");
    let wrapper = wrap.join("mkfs.ext4");
    write_executable(
        &wrapper,
        format!(
            r#"#!/bin/bash
st=""; prev=""
for a in "$@"; do [[ "$prev" == -d ]] && st="$a"; prev="$a"; done
python3 -I - "$st" > {staged:?} <<'PY'
import os, sys
root = sys.argv[1]
for dp, dns, fns in os.walk(root):
    for p in [dp] + [os.path.join(dp, n) for n in fns]:
        for x in os.listxattr(p, follow_symlinks=False):
            print(os.path.relpath(p, root), x)
PY
python3 -I -c 'import os,sys; os.setxattr(sys.argv[1], "user.inherited", b"1")' "$st/f.ax" || exit 97
exec {real} "$@"
"#,
            real = sbin("mkfs.ext4"),
        ),
        0o755,
    );

    let body = format!(
        "set -u\nOUT={out:?}\nPSV_IMGS=()\ndeclare -A PSV_IMG_SHA=()\n{func}\npsv_image candidate {src:?} || exit 9\n"
    );
    let r = Command::new("bash")
        .arg("-c")
        .arg(&body)
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/sbin:/usr/bin:/sbin:/bin", wrap.display()),
        )
        .env("TMPDIR", d.path())
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "psv_image failed: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let img = out.join("candidate.img");
    let debugfs = sbin("debugfs");
    let dbg = |cmd: &str| {
        let o = Command::new(&debugfs)
            .arg("-R")
            .arg(cmd)
            .arg(&img)
            .output()
            .unwrap();
        assert!(o.status.success(), "debugfs {cmd}");
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let mut image_xattrs = Vec::new();
    for p in ["/", "/f.ax", "/lib", "/lib/run.sh"] {
        let o = dbg(&format!("ea_list {p}"));
        if o.contains("Extended attributes:") {
            image_xattrs.push(format!("{p}: {}", o.trim()));
        }
    }
    Imaged {
        staged: std::fs::read_to_string(&staged).unwrap_or_else(|_| "<not written>".into()),
        image_xattrs,
        image_ls: dbg("ls -p /lib"),
    }
}

/// PSV-2 (C9 dev review): the launcher's `psv_image` used `cp -a` (which
/// preserves POSIX ACLs and xattrs) and `mkfs.ext4 -d` (which copies them),
/// so an ACL on a candidate or suite input reached the guest image, where it
/// restricts the test uid behind a normalised mode and an unchanged digest.
/// Both layers are checked on their own: the staging copy carries no
/// attribute, and an attribute the staging tree DOES carry never reaches the
/// image. The exec bit (which the digest records) survives the copy.
#[test]
fn the_launchers_input_image_carries_no_extended_attribute() {
    let launcher = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/fc_linux_profile.sh");
    let script = std::fs::read_to_string(&launcher).unwrap();
    let got = run_psv_image(&script);
    assert!(
        got.staged.trim().is_empty(),
        "ATTACK: psv_image's copy carried extended attributes of the input into the \
         staging tree it images: {}",
        got.staged
    );
    assert!(
        got.image_xattrs.is_empty(),
        "ATTACK: an extended attribute on the staging tree reached the guest input image: {:?}",
        got.image_xattrs
    );
    assert!(
        got.image_ls.contains("/100755/0/0/run.sh/"),
        "the exec bit (part of the tree digest) did not survive the copy: {}",
        got.image_ls
    );

    // Controls: the pre-fix text leaks at BOTH layers.
    let old_copy = script.replace("    cp -R \"$2/.\" \"$st/\"", "    cp -a \"$2/.\" \"$st/\"");
    assert_ne!(old_copy, script, "the copy line was found");
    let got = run_psv_image(&old_copy);
    for want in [
        "f.ax system.posix_acl_access",
        "lib system.posix_acl_default",
        "lib/run.sh user.planted",
        ". user.root",
    ] {
        assert!(
            got.staged.contains(want),
            "control: cp -a carries {want}: {}",
            got.staged
        );
    }
    let old_mkfs = script.replace("root_owner=0:0,no_copy_xattrs", "root_owner=0:0");
    assert_ne!(old_mkfs, script, "the mkfs line was found");
    let got = run_psv_image(&old_mkfs);
    assert!(
        got.image_xattrs
            .iter()
            .any(|x| x.contains("user.inherited")),
        "control: without no_copy_xattrs mkfs copies the staging tree's attributes: {:?}",
        got.image_xattrs
    );
}

/// Run the real launcher in PSV mode with a launch manifest naming
/// `named`'s digest and `--policy` holding `given`. Returns its exit and
/// stderr. Nothing is acquired either way: there is no engine here, so a
/// policy the launcher ACCEPTS is refused later, for the missing VMM.
fn psv_launch_with_policy(named: &str, given: &str) -> (Option<i32>, String) {
    use sha2::{Digest, Sha256};
    let sha = |b: &[u8]| format!("{:x}", Sha256::digest(b));
    let d = tempfile::tempdir().unwrap();
    let p = |n: &str| d.path().join(n);
    for dir in ["cand", "suite", "job"] {
        std::fs::create_dir(p(dir)).unwrap();
    }
    std::fs::write(p("cand/f.ax"), "fn f() -> i64 { 1 }\n").unwrap();
    std::fs::write(p("suite/accept.ax"), "// suite\n").unwrap();
    let m = format!("{{\"policy_sha256\":\"{}\"}}", sha(named.as_bytes()));
    std::fs::write(p("job/launch-manifest.json"), &m).unwrap();
    std::fs::write(p("job/completion-secret"), [7u8; 32]).unwrap();
    std::fs::write(p("policy.json"), given).unwrap();
    let launcher = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/fc_linux_profile.sh");
    let o = Command::new("bash")
        .arg(&launcher)
        .args(["--psv-candidate"])
        .arg(p("cand"))
        .arg("--psv-suite")
        .arg(p("suite"))
        .arg("--psv-job")
        .arg(p("job"))
        .arg("--psv-manifest-sha")
        .arg(sha(m.as_bytes()))
        .arg("--policy")
        .arg(p("policy.json"))
        .arg("--out")
        .arg(p("out"))
        .arg("--fc-bin")
        .arg(p("absent/firecracker"))
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .output()
        .unwrap();
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// PSV-6 (C9 round 4; A87): the launcher, which both routes run (the
/// privileged helper's and the direct one), boots only the policy the launch
/// manifest names, and only one that states an effect ceiling:
/// * the manifest names P1 (`allowed_effects: []`), `--policy` is P2 (IO, Net,
///   Time): refused, for that reason;
/// * the manifest names P0, which states NO ceiling, and `--policy` is P0:
///   refused (an absent ceiling is never read as none).
///
/// Control: `--policy` is the manifest's P1, and the launcher goes past the
/// policy check (to the missing engine, nothing acquired).
#[test]
fn the_launcher_boots_only_the_policy_the_manifest_names() {
    let p1 = r#"{"schema":"axon-vm-mmds/1","allowed_effects":[]}"#;
    let p2 = r#"{"schema":"axon-vm-mmds/1","allowed_effects":["IO","Net","Time"]}"#;
    let p0 = r#"{"schema":"axon-vm-mmds/1","budget_tokens":0}"#;
    let (code, err) = psv_launch_with_policy(p1, p2);
    assert!(
        code == Some(22) && err.contains("not the policy_sha256"),
        "ATTACK: the launcher went past a --policy the launch manifest does not name: \
         {code:?} {err}"
    );
    let (code, err) = psv_launch_with_policy(p0, p0);
    assert!(
        code == Some(22) && err.contains("names no allowed_effects"),
        "ATTACK: the launcher went past a manifest policy that states no effect ceiling: \
         {code:?} {err}"
    );
    let (code, err) = psv_launch_with_policy(p1, p1);
    assert!(
        code == Some(22)
            && !err.contains("not the policy_sha256")
            && !err.contains("allowed_effects")
            && err.contains("firecracker"),
        "control: the manifest's policy passes the policy check: {code:?} {err}"
    );
}
