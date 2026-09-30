//! The B263 protected-Linux profile actually RUNS its workload under
//! axon-guest-init, and the image build installs and pins it.
//!
//! Before this, `profiles/linux-microvm/guest-init.sh` exec'd
//! `env -i … /usr/bin/axon run` directly: no policy was read, no effect ceiling
//! or token cap exported, no seccomp installed — whatever the host put on the
//! cmdline. These tests pin the wiring by reading the shipped files (no VM is
//! booted here; a root lane does that) and by running the script's
//! policy-report block against fake cmdlines.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// Join shell `\`-continuations so a command is one logical line.
fn logical_lines(sh: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in sh.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            continue;
        }
        if let Some(head) = t.strip_suffix('\\') {
            cur.push_str(head);
            cur.push(' ');
        } else {
            cur.push_str(t);
            out.push(std::mem::take(&mut cur));
        }
    }
    out
}

#[test]
fn guest_init_sh_execs_the_workload_under_axon_guest_init_inside_env_i() {
    let sh = read("profiles/linux-microvm/guest-init.sh");
    let lines = logical_lines(&sh);
    let runs: Vec<&String> = lines
        .iter()
        .filter(|l| l.contains("/usr/bin/axon run"))
        .collect();
    assert_eq!(
        runs.len(),
        1,
        "exactly one workload launch expected: {runs:?}"
    );
    let l = runs[0];
    assert!(
        l.starts_with("exec env -i "),
        "workload must be exec'd inside `env -i`: {l}"
    );
    assert!(
        l.contains(" /usr/bin/axon-guest-init /usr/bin/axon run /work/job/program.ax"),
        "the workload must run UNDER axon-guest-init (the policy channel): {l}"
    );
}

fn policy_report(cmdline: &str) -> String {
    let sh = read("profiles/linux-microvm/guest-init.sh");
    let start = sh.find("# >>> policy-report").expect("start marker");
    let end = sh.find("# <<< policy-report").expect("end marker");
    let dir = std::env::temp_dir().join(format!(
        "axon-b263-policy-report-{}-{}",
        std::process::id(),
        cmdline.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let cmd_path = dir.join("cmdline");
    std::fs::write(&cmd_path, format!("{cmdline}\n")).unwrap();
    let block = sh[start..end]
        .replace("/proc/cmdline", &cmd_path.display().to_string())
        .replace(
            "/tmp/policy.json",
            &dir.join("policy.json").display().to_string(),
        );
    let out = Command::new("sh")
        .arg("-c")
        .arg(&block)
        .output()
        .expect("run sh");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "policy-report block failed: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn sha256_of(bytes: &[u8]) -> String {
    let dir = std::env::temp_dir().join(format!("axon-b263-sha-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("x");
    std::fs::write(&f, bytes).unwrap();
    let out = Command::new("sha256sum")
        .arg(&f)
        .output()
        .expect("sha256sum");
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8(out.stdout).unwrap()[..64].to_string()
}

#[test]
fn guest_init_sh_reports_the_digest_of_the_decoded_policy_on_serial() {
    use base64::Engine as _;
    let json = r#"{"schema":"axon-vm-mmds/1","run_id":"r1","allowed_effects":["IO"]}"#;
    let b64 = base64::engine::general_purpose::STANDARD.encode(json);
    let base = "console=ttyS0 reboot=k panic=1 pci=off init=/init -- /init *";
    assert_eq!(
        policy_report(&format!("{base} axon.policy={b64}")),
        format!("B263-POLICY sha={}", sha256_of(json.as_bytes()))
    );
    assert_eq!(policy_report(base), "B263-POLICY absent");
    assert_eq!(
        policy_report(&format!("{base} axon.policy={b64} axon.policy={b64}")),
        "B263-POLICY ambiguous words=2"
    );
    assert_eq!(
        policy_report(&format!("{base} axon.policy=!!!")),
        "B263-POLICY undecodable"
    );
}

#[test]
fn the_linux_rootfs_build_installs_a_default_features_axon_guest_init() {
    let s = read("scripts/build-guest-image.sh");
    let f = s
        .find("build_rootfs_linux() {")
        .expect("build_rootfs_linux");
    let body = &s[f..f + s[f..].find("\n}\n").expect("fn end")];
    let lines = logical_lines(body);
    let build = lines
        .iter()
        .find(|l| l.contains("cargo build") && l.contains("-p axon-guest-init"))
        .expect("build_rootfs_linux must build axon-guest-init");
    assert!(build.contains("--locked"), "{build}");
    assert!(build.contains("x86_64-unknown-linux-musl"), "{build}");
    assert!(
        !build.contains("--features") && !build.contains("--all-features"),
        "the image's axon-guest-init must be a DEFAULT-features build: {build}"
    );
    assert!(
        body.contains(r#"cp "$INIT_BIN" "$STAGE/usr/bin/axon-guest-init""#),
        "axon-guest-init must be installed into the rootfs"
    );
    assert!(
        body.contains("grep -qa 'AXON_GUEST_ALLOW_NO_POLICY'"),
        "the build must refuse a binary that carries the bypass"
    );
}

#[test]
fn the_profile_manifest_pins_axon_guest_init() {
    let s = read("scripts/linux_profile_manifest.py");
    let pinned = s
        .lines()
        .find(|l| l.trim_start().starts_with("for name in ("))
        .expect("linux_profile_manifest.py pins artifacts in a `for name in (…)` loop");
    // Every binary that holds or enforces authority in the guest is pinned:
    // the policy channel (axon-guest-init) and the suite-verdict runner that
    // holds the per-attempt secret (axon-psv-runner).
    for name in [
        "vmlinux",
        "rootfs.sqfs",
        "axon",
        "axon-guest-init",
        "axon-psv-runner",
    ] {
        assert!(
            pinned.contains(&format!("\"{name}\"")),
            "linux_profile_manifest.py must pin {name}'s sha256: {pinned}"
        );
    }
}

/// PSV-2: the three PSV input drives are mounted read-only, nodev, nosuid,
/// noexec, and with no option this guest kernel's ext4 refuses. `noacl` was
/// added in C9 round 1 and passed this textual test, yet the real guest
/// rebooted on "ext4: Unknown parameter 'noacl'" (boot test, C9 round 1b).
/// ACLs are kept out by the launcher and refused by the runner instead.
#[test]
fn guest_init_sh_mounts_every_psv_input_read_only() {
    let sh = read("profiles/linux-microvm/guest-init.sh");
    let mounts: Vec<String> = logical_lines(&sh)
        .into_iter()
        .filter(|l| l.contains("mount ") && l.contains(" /in/"))
        .collect();
    assert_eq!(
        mounts.len(),
        3,
        "three PSV input mounts expected: {mounts:?}"
    );
    for l in &mounts {
        let opts = l
            .split_whitespace()
            .skip_while(|w| *w != "-o")
            .nth(1)
            .unwrap_or_else(|| panic!("no -o options: {l}"));
        let opts: Vec<&str> = opts.split(',').collect();
        for want in ["ro", "nodev", "nosuid", "noexec"] {
            assert!(opts.contains(&want), "{want} missing: {l}");
        }
        assert!(
            !opts.contains(&"noacl"),
            "this guest kernel's ext4 refuses `noacl`: the mount fails and the guest \
             reboots: {l}"
        );
    }
}

/// PSV mode (C9 round 2, harness): the key-holding runner is exec'd like the
/// workload, under `env -i` and axon-guest-init. A `NAME=value` cmdline word
/// the kernel copied into PID 1's environment must not reach the process that
/// holds the completion secret. The older test above pins only the non-PSV
/// `axon run` route.
#[test]
fn guest_init_sh_execs_the_psv_runner_under_axon_guest_init_inside_env_i() {
    let sh = read("profiles/linux-microvm/guest-init.sh");
    let runs: Vec<String> = logical_lines(&sh)
        .into_iter()
        .filter(|l| l.contains("/usr/bin/axon-psv-runner") && l.starts_with("exec "))
        .collect();
    assert_eq!(
        runs.len(),
        1,
        "exactly one PSV runner launch expected: {runs:?}"
    );
    let l = &runs[0];
    assert!(
        l.starts_with("exec env -i ")
            && l.contains(" /usr/bin/axon-guest-init /usr/bin/axon-psv-runner"),
        "ATTACK: the PSV runner is launched outside `env -i` + axon-guest-init: {l}"
    );
}

/// The PSV block of guest-init.sh, run against a fake cmdline: prints the
/// manifest word it took, or the `B263-FAIL` reason it stopped on.
fn psv_manifest_block(cmdline: &str) -> String {
    let sh = read("profiles/linux-microvm/guest-init.sh");
    let start = sh
        .find("PSV_MSHA=\"\"; PSV_WORDS=0")
        .expect("PSV block start");
    let end_pat = "fail \"psv-ambiguous\"";
    let end = sh.find(end_pat).expect("PSV block end") + end_pat.len();
    let dir = std::env::temp_dir().join(format!(
        "axon-b263-psv-words-{}-{}",
        std::process::id(),
        cmdline.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let cmd_path = dir.join("cmdline");
    std::fs::write(&cmd_path, format!("{cmdline}\n")).unwrap();
    let script = format!(
        "fail() {{ echo \"B263-FAIL $1\"; exit 0; }}\nset -f\n{}\necho \"TOOK=$PSV_MSHA\"\n",
        sh[start..end].replace("/proc/cmdline", &cmd_path.display().to_string())
    );
    let out = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .output()
        .expect("run sh");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "PSV block failed: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// A cmdline naming TWO launch manifests stops the guest (`psv-ambiguous`):
/// the runner takes the FIRST `axon.psv.manifest=` word and /init reports the
/// LAST on serial, so two words would let the verdict and the serial report
/// name different manifests. Control: one word is taken as named.
#[test]
fn guest_init_sh_refuses_two_launch_manifest_words() {
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    assert_eq!(
        psv_manifest_block(&format!("console=ttyS0 axon.psv.manifest={a}")),
        format!("TOOK={a}"),
        "control"
    );
    let got = psv_manifest_block(&format!(
        "console=ttyS0 axon.psv.manifest={a} axon.psv.manifest={b}"
    ));
    assert_eq!(
        got, "B263-FAIL psv-ambiguous",
        "ATTACK: a cmdline naming two launch manifests was accepted: {got}"
    );
}
