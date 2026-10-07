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
        .find(|l| l.contains(" build ") && l.contains("-p axon-guest-init"))
        .expect("build_rootfs_linux must build axon-guest-init");
    // Through the controlled build environment (scripts/guest_build_env.py),
    // never a bare cargo that inherits the caller's wrapper, rustc or flags.
    assert!(build.trim_start().starts_with("gcargo "), "{build}");
    assert!(build.contains("--locked"), "{build}");
    assert!(build.contains("x86_64-unknown-linux-musl"), "{build}");
    assert!(
        !build.contains("--features") && !build.contains("--all-features"),
        "the image's axon-guest-init must be a DEFAULT-features build: {build}"
    );
    // The rootfs is assembled by the controlled step (C9 round 4b, amendment
    // 63), which installs the recorded axon-guest-init at /usr/bin.
    assert!(
        body.contains(r#"python3 scripts/guest_build_env.py rootfs "$BUILD_ENV""#),
        "the rootfs must be assembled by the controlled build step"
    );
    assert!(
        read("scripts/guest_build_env.py")
            .contains(r#""axon-guest-init": "usr/bin/axon-guest-init""#),
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
        // The EFFECTIVE option set: mount(8) and busybox apply the options in
        // order, so a later `rw` undoes an earlier `ro` (C9 round 3: `,rw`
        // appended passed the old membership test and mounted read-write).
        // The behavioural test below is the one that decides; this is the
        // reading of the text a non-root lane can do.
        let mut eff: std::collections::BTreeMap<&str, &str> = Default::default();
        for o in &opts {
            let (key, val) = match *o {
                "ro" | "rw" => ("ro", *o),
                "exec" | "noexec" => ("exec", *o),
                "dev" | "nodev" => ("dev", *o),
                "suid" | "nosuid" => ("suid", *o),
                "defaults" => {
                    for (k, v) in [
                        ("ro", "rw"),
                        ("exec", "exec"),
                        ("dev", "dev"),
                        ("suid", "suid"),
                    ] {
                        eff.insert(k, v);
                    }
                    continue;
                }
                _ => continue,
            };
            eff.insert(key, val);
        }
        for (key, want) in [
            ("ro", "ro"),
            ("dev", "nodev"),
            ("suid", "nosuid"),
            ("exec", "noexec"),
        ] {
            assert_eq!(
                eff.get(key).copied(),
                Some(want),
                "{want} is not in effect (later options override earlier ones): {l}"
            );
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

// ── C9 round 3, HARNESS workstream: the guest's input mounts and runner
// environment, decided by BEHAVIOUR. The textual tests above read the script;
// `,rw` appended to an option list passed them while the drive really mounted
// read-write (both util-linux and busybox mount apply options in order). These
// run guest-init.sh's OWN text, taken from the shipped file, and read what the
// kernel and the exec'd child actually got. The boot test's `mounts` case
// (scripts/psv_guest_boot_test.sh) reads /proc/mounts inside a real guest; this
// is the same check without a VM, runnable in `cargo test` as root.

#[path = "../../axon-fabric/tests/common/exec.rs"]
mod exec;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon-gi-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// guest-init.sh's PSV input-mount block, exactly as shipped: the lines
/// between the PSV `if` and the PSV-MANIFEST echo.
fn psv_mount_block(sh: &str) -> String {
    let start_pat = "if [ -n \"$PSV_MSHA\" ]; then\n";
    let start = sh.find(start_pat).expect("PSV mount block start") + start_pat.len();
    let end = start
        + sh[start..]
            .find("    echo \"PSV-MANIFEST")
            .expect("PSV mount block end");
    let block = &sh[start..end];
    assert_eq!(
        block.matches("mount ").count(),
        3,
        "setup: the PSV mount block holds three mounts: {block}"
    );
    block.to_string()
}

const DRIVES: [(&str, &str); 3] = [
    ("/dev/vdc", "/in/candidate"),
    ("/dev/vdd", "/in/suite"),
    ("/dev/vde", "/in/job"),
];

/// PSV-2 guest (rows M493-M496, M607-M609), ROOT ONLY: each PSV input drive
/// is mounted ro, nodev, nosuid and noexec IN EFFECT. The script's own mount
/// block runs against three loop-device ext4 images in a private mount
/// namespace, once with util-linux mount and once with busybox mount (the
/// guest's), and the options are read back from /proc/mounts.
#[test]
fn guest_init_sh_psv_input_mounts_are_in_effect_read_only() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("skipped: needs root (loop devices, mount namespaces)");
        return;
    }
    let sh = read("profiles/linux-microvm/guest-init.sh");
    let block = psv_mount_block(&sh);
    let d = scratch("mounts");
    let mut loops = vec![];
    let mut script = String::from("fail() { echo \"B263-FAIL $1\"; exit 1; }\n");
    let mut body = block.clone();
    for (i, (dev, mp)) in DRIVES.iter().enumerate() {
        let img = d.join(format!("d{i}.img"));
        let st = Command::new("sh")
            .arg("-c")
            .arg("truncate -s 8M \"$1\" && mkfs.ext4 -q -F \"$1\"")
            .arg("sh")
            .arg(&img)
            .status()
            .unwrap();
        assert!(st.success(), "setup: mkfs.ext4");
        let out = Command::new("losetup")
            .args(["-f", "--show"])
            .arg(&img)
            .output()
            .unwrap();
        assert!(out.status.success(), "setup: losetup: {out:?}");
        let lo = String::from_utf8(out.stdout).unwrap().trim().to_string();
        loops.push(lo.clone());
        let target = d.join(mp.trim_start_matches('/'));
        std::fs::create_dir_all(&target).unwrap();
        body = body
            .replace(&format!("{dev} "), &format!("{lo} "))
            .replace(&format!(" {mp} "), &format!(" {} ", target.display()));
    }
    script.push_str(&body);
    script.push_str("cat /proc/self/mounts\n");
    // busybox's mount, the guest's, first on PATH in the second run.
    let bb = d.join("bb");
    std::fs::create_dir_all(&bb).unwrap();
    let have_busybox = Path::new("/usr/bin/busybox").exists() || Path::new("/bin/busybox").exists();
    if have_busybox {
        let busybox = if Path::new("/usr/bin/busybox").exists() {
            "/usr/bin/busybox"
        } else {
            "/bin/busybox"
        };
        std::os::unix::fs::symlink(busybox, bb.join("mount")).unwrap();
    }
    let mut runs = vec![("util-linux", "/usr/sbin:/usr/bin:/sbin:/bin".to_string())];
    if have_busybox {
        runs.push((
            "busybox",
            format!("{}:/usr/sbin:/usr/bin:/sbin:/bin", bb.display()),
        ));
    }
    let mut failure = None;
    for (which, path) in &runs {
        let out = Command::new("unshare")
            .args(["-m", "--propagation", "private", "sh", "-c", &script])
            .env("PATH", path)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            failure = Some(format!(
                "setup: the mount block failed under {which}: {text} {}",
                String::from_utf8_lossy(&out.stderr)
            ));
            break;
        }
        for (dev, mp) in DRIVES {
            let target = d.join(mp.trim_start_matches('/'));
            let t = target.display().to_string();
            let line = text
                .lines()
                .find(|l| l.split(' ').nth(1) == Some(t.as_str()))
                .unwrap_or_else(|| panic!("setup: {mp} not mounted under {which}: {text}"));
            let opts: Vec<&str> = line.split(' ').nth(3).unwrap_or("").split(',').collect();
            let drive = mp.trim_start_matches("/in/");
            for want in ["ro", "noexec", "nodev", "nosuid"] {
                if !opts.contains(&want) && failure.is_none() {
                    failure = Some(format!(
                        "ATTACK: the {drive} drive is in effect mounted without {want} ({which} \
                         mount of {dev}: {line})"
                    ));
                }
            }
        }
        if failure.is_some() {
            break;
        }
    }
    for lo in &loops {
        let _ = Command::new("losetup").arg("-d").arg(lo).status();
    }
    let _ = std::fs::remove_dir_all(&d);
    if let Some(f) = failure {
        panic!("{f}");
    }
}

/// guest-init.sh's workload block, exactly as shipped: from B263-START to the
/// line that takes its exit status, with the guest's paths pointed into `d`
/// and axon-guest-init replaced by a stub that records the environment and
/// argv it was exec'd with. PID 1's environment is POLLUTED the way a kernel
/// cmdline `NAME=value` word pollutes it.
fn run_workload_block(d: &Path, psv: bool) -> (Vec<String>, String) {
    let sh = read("profiles/linux-microvm/guest-init.sh");
    let start = sh
        .find("echo \"B263-START\"\n")
        .expect("workload block start");
    let end = start + sh[start..].find("RC=$?").expect("workload block end");
    let stub = d.join("axon-guest-init");
    exec::write_executable(
        &stub,
        format!(
            "#!/bin/sh\ntr '\\0' '\\n' < /proc/$$/environ > '{env}'\necho \"$1\" > '{argv}'\n",
            env = d.join("env").display(),
            argv = d.join("argv").display()
        ),
        0o755,
    );
    let work = d.join("work");
    std::fs::create_dir_all(work.join("out")).unwrap();
    let block = sh[start..end]
        .replace("/usr/bin/axon-guest-init", &stub.display().to_string())
        .replace(
            "/sys/fs/cgroup/job/cgroup.procs",
            &d.join("cgroup.procs").display().to_string(),
        )
        .replace("/work", &work.display().to_string());
    let prelude = if psv {
        "PSV_MSHA=abc\nARGS=\"\"\n"
    } else {
        "PSV_MSHA=\"\"\nARGS=\"\"\n"
    };
    let out = Command::new("env")
        .args([
            "-i",
            "PATH=/usr/bin:/bin",
            "POISON=from-the-kernel-cmdline",
            "AXON_GUEST_ALLOW_NO_POLICY=1",
        ])
        .args(["sh", "-c"])
        .arg(format!("{prelude}{block}"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "setup: the workload block failed: {out:?}"
    );
    let env = std::fs::read_to_string(d.join("env"))
        .unwrap_or_else(|e| panic!("setup: the stub did not run: {e}"));
    let argv = std::fs::read_to_string(d.join("argv")).unwrap_or_default();
    let mut env: Vec<String> = env.lines().map(str::to_string).collect();
    env.sort();
    (env, argv.trim().to_string())
}

/// PSV guest (row M498): the key-holding runner starts under
/// axon-guest-init with EXACTLY the three fixed variables. A variable in
/// PID 1's environment (a kernel cmdline `NAME=value` word) must not reach
/// the process that holds the completion secret.
#[test]
fn guest_init_sh_psv_runner_starts_with_exactly_the_fixed_environment() {
    let d = scratch("psv-env");
    let (env, argv) = run_workload_block(&d, true);
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(
        argv, "/usr/bin/axon-psv-runner",
        "ATTACK: the PSV runner was not started under axon-guest-init: {argv:?}"
    );
    assert_eq!(
        env,
        [
            "HOME=/tmp",
            "PATH=/bin:/usr/bin",
            "XDG_CACHE_HOME=/tmp/cache"
        ],
        "ATTACK: the PSV runner started with a variable from PID 1's environment: {env:?}"
    );
}

/// B263 guest (row M499): the same for the non-PSV workload route.
#[test]
fn guest_init_sh_workload_starts_with_exactly_the_fixed_environment() {
    let d = scratch("wl-env");
    let (env, argv) = run_workload_block(&d, false);
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(
        argv, "/usr/bin/axon",
        "ATTACK: the workload was not started under axon-guest-init: {argv:?}"
    );
    let want: Vec<String> = [
        "HOME=".to_string() + &d.join("work").display().to_string(),
        "PATH=/bin:/usr/bin".into(),
        "XDG_CACHE_HOME=/tmp/cache".into(),
    ]
    .into();
    assert_eq!(
        env, want,
        "ATTACK: the workload started with a variable from PID 1's environment: {env:?}"
    );
}

/// C9 round 4: the guest's serial record was cut by its own reboot (1 in 40
/// boots of the pass case ended `PSV-VERDICT-INIT` -- one 16-byte UART FIFO --
/// then the kernel's `reboot: Restarting system`). Console output sits in the
/// tty and UART buffers, and `reboot -f` does not wait for them. Every reboot
/// goes through `halt_guest`, which drains the console first (busybox stty
/// applies a setting with TCSETSW, i.e. after all output has left the UART).
/// Run here with the real functions and logging stand-ins for the three
/// commands: the drain comes after sync and before the reboot, on the failure
/// path; and no other line of the script reboots.
#[test]
fn every_guest_reboot_first_drains_the_serial_console() {
    let sh = read("profiles/linux-microvm/guest-init.sh");
    let func = |name: &str| {
        let s = sh
            .find(&format!("\n{name}() {{\n"))
            .unwrap_or_else(|| panic!("{name}() in guest-init.sh"));
        let e = s + sh[s..].find("\n}\n").expect("function end") + 3;
        sh[s..e].to_string()
    };
    let d = scratch("halt");
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = d.join("calls");
    for cmd in ["sync", "stty", "reboot"] {
        exec::write_executable(
            &bin.join(cmd),
            format!(
                "#!/bin/sh\necho \"{cmd} $*\" >> '{}'\n[ {cmd} = reboot ] && exit 0\nexit 0\n",
                log.display()
            ),
            0o755,
        );
    }
    let script = format!(
        "{}{}\nfail workspace-mount\n",
        func("halt_guest"),
        func("fail")
    );
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .env_clear()
        .env("PATH", &bin)
        .output()
        .unwrap();
    assert!(out.status.success(), "setup: {out:?}");
    let calls: Vec<String> = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .collect();
    let _ = std::fs::remove_dir_all(&d);
    let reboot = calls.iter().position(|c| c == "reboot -f");
    let drain = calls
        .iter()
        .position(|c| c.starts_with("stty -F /dev/console "));
    assert!(
        matches!((drain, reboot), (Some(a), Some(b)) if a < b),
        "ATTACK: the guest reboots without draining its serial console first: {calls:?}"
    );
    // ...and nothing else in the script reboots around halt_guest.
    let halt = func("halt_guest");
    let outside = sh.replace(&halt, "");
    let stray: Vec<&str> = outside
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && l.contains("reboot"))
        .collect();
    assert!(
        stray.is_empty(),
        "ATTACK: a reboot outside halt_guest: {stray:?}"
    );
    assert!(
        sh.trim_end().ends_with("halt_guest"),
        "the script's last action is halt_guest"
    );
}
