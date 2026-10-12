//! The guest's PID-1 supervisor refuses to start the workload under a kernel
//! cmdline policy it cannot trust (C9 round 4c, r4c-fixes part 2, amendment
//! 71). Every test runs the REAL `axon-guest-init` binary, ROOT ONLY, in a
//! private mount namespace whose `/proc/cmdline` is a file the test wrote (the
//! binary reads that fixed path, as in the guest). The workload is a script
//! that prints `RAN` and the effect ceiling it was given: a refusal is
//! "`REFUSING to start the guest`" and no `RAN`. The CONTROL is a valid policy,
//! under which the workload runs with exactly that policy.
use base64::Engine as _;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_axon-guest-init");

fn skip_unless_root() -> bool {
    // SAFETY: geteuid cannot fail.
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("SKIP: needs root (a private mount namespace over /proc/cmdline)");
        return true;
    }
    false
}

struct Run {
    ran: bool,
    out: String,
    err: String,
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "axon-guest-init-policy-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Boot `axon-guest-init` under `cmdline` with the recording workload.
fn boot(dir: &Path, cmdline: &str) -> Run {
    boot_with(
        dir,
        cmdline,
        "#!/bin/sh\necho RAN\necho \"effects=${AXON_ALLOWED_EFFECTS-unset}\"\n",
    )
}

/// As `boot`, with the workload script `script`.
fn boot_with(dir: &Path, cmdline: &str, script: &str) -> Run {
    let cmd = dir.join("cmdline");
    std::fs::write(&cmd, format!("{cmdline}\n")).unwrap();
    let payload = dir.join("workload.sh");
    std::fs::write(&payload, script).unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&payload, std::fs::Permissions::from_mode(0o755)).unwrap();
    let o = Command::new("/usr/bin/unshare")
        .args(["-m", "--propagation", "private", "--", "/bin/sh", "-c"])
        .arg("mount --bind \"$1\" /proc/cmdline || exit 97; exec \"$2\" \"$3\"")
        .arg("sh")
        .arg(&cmd)
        .arg(BIN)
        .arg(&payload)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert_ne!(
        o.status.code(),
        Some(97),
        "setup: /proc/cmdline could not be replaced: {err}"
    );
    Run {
        ran: out.lines().any(|l| l == "RAN"),
        out,
        err,
    }
}

fn word(json: &str) -> String {
    format!(
        "axon.policy={}",
        base64::engine::general_purpose::STANDARD.encode(json)
    )
}

const BASE: &str = "console=ttyS0 reboot=k panic=1 pci=off nomodules init=/init --";
const VALID: &str = r#"{"schema":"axon-vm-mmds/1","allowed_effects":["IO"]}"#;

/// The policy `case` must be refused before the workload starts. ATTACK if
/// the workload ran: when the refusal was decided but not acted on (the boot
/// log says REFUSING and the workload ran anyway) that is the refusal arm;
/// otherwise the policy was taken as valid.
fn refused(dir: &Path, case: &str, why: &str, cmdline: &str) {
    let r = boot(dir, cmdline);
    if r.ran {
        if r.err.contains("REFUSING to start the guest") {
            panic!(
                "ATTACK: the guest started although its policy was refused ({case}): {} {}",
                r.out, r.err
            );
        }
        panic!(
            "ATTACK: the guest started under a {case} cmdline policy: {} {}",
            r.out, r.err
        );
    }
    assert!(
        r.err.contains("REFUSING to start the guest") && r.err.contains(why),
        "setup: {case} was refused for another reason (want {why:?}): {}",
        r.err
    );
}

/// A (amendment 71), ROOT ONLY: each cmdline policy the supervisor must not
/// trust is refused, and the refusal is acted on.
#[test]
fn an_untrustworthy_cmdline_policy_starts_no_workload() {
    if skip_unless_root() {
        return;
    }
    let d = scratch("refuse");
    let valid = word(VALID);
    // The kernel keeps at most COMMAND_LINE_SIZE bytes: a line that reaches
    // the safe limit may have lost the policy's tail.
    let pad = "x".repeat(2100);
    refused(
        &d,
        "POSSIBLY TRUNCATED",
        "POSSIBLY TRUNCATED",
        &format!("{BASE} {valid} pad={pad}"),
    );
    refused(
        &d,
        "AMBIGUOUS (repeated)",
        "AMBIGUOUS",
        &format!(
            "{BASE} {valid} {}",
            word(r#"{"schema":"axon-vm-mmds/1","allowed_effects":["IO","Net","Exec"]}"#)
        ),
    );
    refused(
        &d,
        "CONSTRAINS NOTHING",
        "CONSTRAINS NOTHING",
        &format!("{BASE} {}", word(r#"{"schema":"axon-vm-mmds/1"}"#)),
    );
    refused(
        &d,
        "WRONG SCHEMA",
        "WRONG SCHEMA",
        &format!(
            "{BASE} {}",
            word(r#"{"schema":"axon-vm-mmds/0","allowed_effects":["IO"]}"#)
        ),
    );
    refused(
        &d,
        "DUPLICATE KEY",
        "DUPLICATE KEY",
        &format!(
            "{BASE} {}",
            word(
                r#"{"schema":"axon-vm-mmds/1","allowed_effects":["IO"],"allowed_effects":["IO","Net","Exec"]}"#
            )
        ),
    );
    // CONTROL: a valid policy starts the workload under exactly its ceiling.
    let r = boot(&d, &format!("{BASE} {valid}"));
    assert!(
        r.ran && r.out.contains("effects=IO\n"),
        "control: a valid cmdline policy starts the workload: {} {}",
        r.out,
        r.err
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// One BPF instruction (`struct sock_filter`, 8 bytes): `code jt jf k`.
fn insn(code: u16, k: u32) -> Vec<u8> {
    let mut v = code.to_ne_bytes().to_vec();
    v.extend([0u8, 0u8]);
    v.extend(k.to_ne_bytes());
    v
}

fn seccomp_policy(bpf: &[u8]) -> String {
    word(&format!(
        r#"{{"schema":"axon-vm-mmds/1","allowed_effects":["IO"],"seccomp_bpf_b64":"{}"}}"#,
        base64::engine::general_purpose::STANDARD.encode(bpf)
    ))
}

/// A (amendment 71), ROOT ONLY: a policy that carries a seccomp filter starts
/// the workload only with that filter installed. A program that is not whole
/// instructions, or that the kernel rejects, starts nothing; and a filter that
/// failed to apply is never followed by the workload.
#[test]
fn a_seccomp_filter_that_does_not_apply_starts_no_workload() {
    if skip_unless_root() {
        return;
    }
    let d = scratch("seccomp");
    // BPF_RET|BPF_K, SECCOMP_RET_ALLOW: a filter that allows everything.
    let allow = insn(0x06, 0x7fff_0000);
    let check = |case: &str, bpf: Vec<u8>| {
        let r = boot(&d, &format!("{BASE} {}", seccomp_policy(&bpf)));
        if r.ran {
            if r.err.contains("seccomp apply failed") {
                panic!(
                    "ATTACK: the guest started after its seccomp filter failed to apply ({case}): \
                     {} {}",
                    r.out, r.err
                );
            }
            panic!(
                "ATTACK: the guest started under a seccomp program {case}: {} {}",
                r.out, r.err
            );
        }
        assert!(
            r.err.contains("seccomp apply failed"),
            "setup: {case}: refused for another reason: {}",
            r.err
        );
    };
    // Nine bytes: one whole ALLOW instruction and a stray byte. Taken in
    // part, the kernel would install the ALLOW prefix.
    let mut ragged = allow.clone();
    ragged.push(0);
    check("that is not whole instructions", ragged);
    // An opcode the kernel's filter check rejects (EINVAL).
    check("the kernel rejects", insn(0xffff, 0));
    // CONTROL: a valid filter is applied and the workload runs.
    let r = boot(&d, &format!("{BASE} {}", seccomp_policy(&allow)));
    assert!(
        r.ran && r.err.contains("seccomp applied (1 instructions)"),
        "control: a valid filter applies and the workload runs: {} {}",
        r.out,
        r.err
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Amendment 95 (eqgate4), ROOT ONLY: the policy's fields reach the workload as
/// the environment the runtime enforces. `AXON_BUDGET_TOKENS` is the AI token
/// cap and `AXON_ALLOWED_EFFECTS` the effect ceiling; `AXON_PRINCIPAL`,
/// `AXON_RUN_ID` and `AXON_SOURCE_HASH` are the audit labels. Each is set from
/// the policy in `child_main` by an `env::set_var` no earlier test observed
/// except the ceiling: with the cap's line removed a guest ran with no token
/// cap and every suite stayed green. The workload prints each variable, `unset`
/// when it is absent; each assertion names its own variable, so a missing one
/// cannot be read as another's.
#[test]
fn the_workload_receives_exactly_the_policys_environment() {
    if skip_unless_root() {
        return;
    }
    let d = scratch("env");
    let script = "#!/bin/sh\necho RAN\n\
        echo \"effects=${AXON_ALLOWED_EFFECTS-unset}\"\n\
        echo \"budget=${AXON_BUDGET_TOKENS-unset}\"\n\
        echo \"principal=${AXON_PRINCIPAL-unset}\"\n\
        echo \"run_id=${AXON_RUN_ID-unset}\"\n\
        echo \"source_hash=${AXON_SOURCE_HASH-unset}\"\n";
    let full = word(
        r#"{"schema":"axon-vm-mmds/1","allowed_effects":["IO","FS"],"budget_tokens":1234,"principal":"agent-7","run_id":"run-42","source_hash":"abcd"}"#,
    );
    let r = boot_with(&d, &format!("{BASE} {full}"), script);
    assert!(
        r.ran,
        "setup: the workload did not start: {} {}",
        r.out, r.err
    );
    for (line, attack) in [
        (
            "effects=IO,FS",
            "ATTACK: the workload ran without the policy's AXON_ALLOWED_EFFECTS ceiling",
        ),
        (
            "budget=1234",
            "ATTACK: the workload ran without the policy's AXON_BUDGET_TOKENS cap",
        ),
        (
            "principal=agent-7",
            "ATTACK: the workload ran without the policy's AXON_PRINCIPAL",
        ),
        (
            "run_id=run-42",
            "ATTACK: the workload ran without the policy's AXON_RUN_ID",
        ),
        (
            "source_hash=abcd",
            "ATTACK: the workload ran without the policy's AXON_SOURCE_HASH",
        ),
    ] {
        assert!(
            r.out.lines().any(|l| l == line),
            "{attack}: wanted {line:?}: {} {}",
            r.out,
            r.err
        );
    }
    // CONTROL: a policy that names only a ceiling sets nothing else (a label
    // is not invented, and the cap is not defaulted).
    let r = boot_with(&d, &format!("{BASE} {}", word(VALID)), script);
    for line in [
        "effects=IO",
        "budget=unset",
        "principal=unset",
        "run_id=unset",
        "source_hash=unset",
    ] {
        assert!(
            r.out.lines().any(|l| l == line),
            "control: wanted {line:?}: {} {}",
            r.out,
            r.err
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}
