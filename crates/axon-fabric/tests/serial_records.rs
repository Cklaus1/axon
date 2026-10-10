//! A serial record counts only as a WHOLE, TERMINATED line (C9 round 4).
//!
//! The guest's console output can be cut by its reboot or spliced with a
//! kernel message: a boot of the pass case ended
//! `PSV-VERDICT-INIT[    0.256427] reboot: Restarting system`. The launcher
//! read every serial field with a substring pattern (`.*B263-OUT ...`,
//! `grep B263-DONE`, `.*sha256=<64 hex>.*`), so a record spliced into another
//! line, or the unterminated text after the last newline, was read as the
//! record. Every field now goes through `serial_record` in
//! scripts/fc_linux_profile.sh, which takes a line only in full and only when
//! a newline ends it. These run the REAL launcher's `--verify-result` (the
//! same primitive the launch path uses) on a returned-drive fixture.

#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use script_spawn::{repo_root, Bins};
use sha2::{Digest, Sha256};
use std::process::Command;

fn sha(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}

const STDOUT: &[u8] = b"hello from the guest\n";
const VERDICT: &[u8] = b"{\"status\":\"passed\"}\n";
const POLICY: &[u8] = b"{\"allowed_effects\":[\"IO\"]}";

/// A returned-drive directory as the launcher leaves it, whose serial log is
/// `serial` (with `{V}`, `{D}`, `{P}` standing for the verdict, stdout and
/// policy digests). Returns `--verify-result`'s exit code.
fn verify(serial: &str) -> Option<i32> {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    std::fs::create_dir_all(root.join("out")).unwrap();
    std::fs::write(root.join("out/stdout"), STDOUT).unwrap();
    std::fs::write(root.join("out/verdict.json"), VERDICT).unwrap();
    let vd = d.path().join("vd");
    std::fs::create_dir_all(&vd).unwrap();
    let st = Command::new("/usr/sbin/mkfs.ext4")
        .args(["-q", "-F", "-d"])
        .arg(&root)
        .arg(vd.join("workspace.img"))
        .arg("8M")
        .status()
        .unwrap();
    assert!(st.success(), "setup: mkfs.ext4 -d");
    let (v, o, p) = (sha(VERDICT), sha(STDOUT), sha(POLICY));
    std::fs::write(
        vd.join("result.json"),
        serde_json::json!({"outputs": {"stdout": {"sha256": o}},
                           "policy_sha256": p, "psv": {"verdict_sha256": v}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        vd.join("serial.log"),
        serial
            .replace("{V}", &v)
            .replace("{D}", &o)
            .replace("{P}", &p),
    )
    .unwrap();
    let launcher = repo_root().join("scripts/fc_linux_profile.sh");
    let out = script_spawn::script("bash", &launcher, Bins::NoWorkspaceBinary)
        .arg("--verify-result")
        .arg(&vd)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .output()
        .unwrap();
    out.status.code()
}

const HEAD: &str = "B263-BOOT 6.1.188\r\nB263-POLICY sha={P}\r\nB263-START\r\n";

/// Control: whole, terminated records bind.
#[test]
fn whole_serial_records_bind() {
    let s = format!(
        "{HEAD}PSV-VERDICT-INIT sha256={{V}}\r\nB263-OUT stdout={{D}} exit=0\r\nB263-DONE\r\n"
    );
    assert_eq!(verify(&s), Some(0), "control: whole records bind");
}

/// The verdict record spliced with the kernel's reboot message (the shape a
/// real boot produced) is not a verdict record: refused, 27.
#[test]
fn a_verdict_record_spliced_with_a_kernel_message_is_no_record() {
    let s = format!(
        "{HEAD}PSV-VERDICT-INIT sha256={{V}}[    0.256427] reboot: Restarting system\r\n\
         B263-OUT stdout={{D}} exit=0\r\nB263-DONE\r\n"
    );
    let rc = verify(&s);
    assert_eq!(
        rc,
        Some(27),
        "ATTACK: a serial verdict record spliced with a kernel message was read as the verdict \
         (exit {rc:?})"
    );
}

/// The text after the last newline -- a record the reboot cut before its end
/// -- is not a record: refused, 23.
#[test]
fn an_unterminated_serial_record_is_no_record() {
    let s = format!("{HEAD}PSV-VERDICT-INIT sha256={{V}}\r\nB263-OUT stdout={{D}} exit=0");
    let rc = verify(&s);
    assert_eq!(
        rc,
        Some(23),
        "ATTACK: an unterminated serial record was read as the output binding (exit {rc:?})"
    );
}

/// A record found INSIDE another line is not a record: the policy report
/// after kernel text binds nothing, 23.
#[test]
fn a_record_inside_another_line_is_no_record() {
    let s = "B263-BOOT 6.1.188\r\n[    0.1] noise B263-POLICY sha={P}\r\nB263-START\r\n\
             PSV-VERDICT-INIT sha256={V}\r\nB263-OUT stdout={D} exit=0\r\nB263-DONE\r\n";
    let rc = verify(s);
    assert_eq!(
        rc,
        Some(23),
        "ATTACK: a policy record inside another serial line was read as the policy report \
         (exit {rc:?})"
    );
}
