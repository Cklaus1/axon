//! B262 / G13-r22-vm-cli-parity: the `axon-vm` CLI must behave byte-for-byte the
//! same after the Firecracker launch path moved behind a library target.
//!
//! The goldens under `tests/cli_parity/` were captured from the binary BEFORE the
//! extraction (commit 4cceb89, `AXON_VM_PARITY_BLESS=1 cargo test -p axon-vm
//! --test cli_parity`). Every case records exit code, stdout and stderr. The only
//! normalisation is of values that differ between two runs of the SAME binary:
//! the per-run temp directory, process ids inside run ids / socket names, and
//! `elapsed_ms`. Nothing about wording, ordering or exit codes is normalised away.
//!
//! Re-blessing is a deliberate act: a diff here means the CLI changed.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_axon-vm"))
}

/// Same convention as `crates/axon-core/tests/cli_run.rs::note_harness_skip`:
/// a skipped case is appended to the workspace `target/harness-skips.log`
/// (reported by `scripts/gate.sh`'s coverage notice) and is FATAL under
/// `AXON_HARNESS_STRICT=1`. The >=25 case-count floor guards a zero-case run;
/// this makes the one host-dependent case legible as a non-result too.
fn note_skip(what: &str) {
    let log = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/harness-skips.log");
    if let Some(d) = log.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        use std::io::Write;
        let _ = writeln!(f, "{what}");
    }
}

fn strict_skips_are_fatal(skipped: &[String]) {
    if !skipped.is_empty() && std::env::var("AXON_HARNESS_STRICT").as_deref() == Ok("1") {
        panic!(
            "SKIPPED under AXON_HARNESS_STRICT=1:\n  {}\nThese cases measured NOTHING.",
            skipped.join("\n  ")
        );
    }
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cli_parity")
}

/// Replace every `<prefix><digits>` with `<prefix>N`.
fn norm_digits_after(s: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(prefix) {
        out.push_str(&rest[..i + prefix.len()]);
        rest = &rest[i + prefix.len()..];
        let n = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
        if n > 0 {
            out.push('N');
            rest = &rest[n..];
        }
    }
    out.push_str(rest);
    out
}

fn normalise(s: &str, tmp: &Path) -> String {
    let mut s = s.replace(tmp.to_str().unwrap(), "$TMP");
    for p in [
        "vm-",
        "\"elapsed_ms\": ",
        "axon-vm-vsock-",
        "axon-vm-",
        "run_id\": \"vm-N-",
    ] {
        s = norm_digits_after(&s, p);
    }
    s
}

struct Case {
    name: &'static str,
    args: Vec<String>,
    env: Vec<(&'static str, String)>,
    /// Files to create in the temp dir before running (relative path, contents).
    files: Vec<(&'static str, &'static str)>,
}

fn case(name: &'static str, args: &[&str]) -> Case {
    Case {
        name,
        args: args.iter().map(|s| s.to_string()).collect(),
        env: vec![],
        files: vec![],
    }
}

fn run_case(c: &Case) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let t = tmp.path();
    for (rel, body) in &c.files {
        let p = t.join(rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(p, body).unwrap();
    }
    let mut cmd = Command::new(bin());
    cmd.current_dir(t)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", t)
        .env("AXON_CONFIG_DIR", t.join("cfg"))
        .env("AXON_VM_QUIET", "1");
    for (k, v) in &c.env {
        cmd.env(k, v.replace("$TMP", t.to_str().unwrap()));
    }
    for a in &c.args {
        cmd.arg(a.replace("$TMP", t.to_str().unwrap()));
    }
    let out = cmd.output().expect("spawn axon-vm");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    format!(
        "exit: {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}\n",
        out.status.code(),
        normalise(&stdout, t),
        normalise(&stderr, t)
    )
}

fn cases() -> Vec<Case> {
    let mut v = vec![
        case("help", &["--help"]),
        case("no_args", &[]),
        case("bogus_subcommand", &["bogus"]),
        case("run_help", &["run", "--help"]),
        case("attest_help", &["attest", "--help"]),
        case("principal_help", &["principal", "--help"]),
        case("principal_add_help", &["principal", "add", "--help"]),
        case("chain_help", &["chain", "--help"]),
        case("chain_stamp_help", &["chain", "stamp", "--help"]),
        case("chain_verify_help", &["chain", "verify", "--help"]),
        case("chain_show_help", &["chain", "show", "--help"]),
        case("chain_export_help", &["chain", "export", "--help"]),
        case(
            "chain_verify_export_help",
            &["chain", "verify-export", "--help"],
        ),
        case("quorum_help", &["quorum", "--help"]),
        case("quorum_propose_help", &["quorum", "propose", "--help"]),
        case("quorum_vote_help", &["quorum", "vote", "--help"]),
        case("quorum_check_help", &["quorum", "check", "--help"]),
        case("principal_list_empty", &["principal", "list"]),
        case("run_missing_program", &["run", "$TMP/nope.ax"]),
        case(
            "run_missing_kernel",
            &[
                "run",
                "$TMP/p.ax",
                "--kernel",
                "$TMP/nok",
                "--initrd",
                "$TMP/i",
            ],
        ),
        case(
            "quorum_check_empty_dir",
            &[
                "quorum",
                "check",
                "--responses-dir",
                "$TMP/votes",
                "--n",
                "3",
                "--run-id",
                "r1",
            ],
        ),
    ];
    let with_files = |mut c: Case| {
        c.files = vec![
            ("p.ax", "fn main() { println(\"hi\") }\n"),
            ("k", "not a kernel"),
            ("i", "not an initrd"),
            ("votes/.keep", ""),
        ];
        c
    };
    v = v.into_iter().map(with_files).collect();

    // No grant from any of the three sources → exit 2, before attestation.
    v.push(with_files(case(
        "run_no_effect_grant",
        &[
            "run",
            "$TMP/p.ax",
            "--kernel",
            "$TMP/k",
            "--initrd",
            "$TMP/i",
        ],
    )));
    v.push(with_files(case(
        "run_no_effect_grant_json",
        &[
            "run",
            "$TMP/p.ax",
            "--kernel",
            "$TMP/k",
            "--initrd",
            "$TMP/i",
            "--json",
        ],
    )));
    // Grant present, no pinned baseline → R26 refusal (exit 10), VM never launched.
    let mut c = with_files(case(
        "run_attest_no_pin",
        &[
            "run",
            "$TMP/p.ax",
            "--kernel",
            "$TMP/k",
            "--initrd",
            "$TMP/i",
        ],
    ));
    c.env.push(("AXON_VM_ALLOWED_EFFECTS", "IO".into()));
    v.push(c);
    let mut c = with_files(case(
        "run_attest_wrong_digest_json",
        &[
            "run",
            "$TMP/p.ax",
            "--kernel",
            "$TMP/k",
            "--initrd",
            "$TMP/i",
            "--json",
            "--expect-digest",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ],
    ));
    c.env.push(("AXON_VM_ALLOWED_EFFECTS", "IO".into()));
    v.push(c);
    // Unparseable manifest → exit 2.
    let mut c = with_files(case(
        "run_bad_manifest",
        &[
            "run",
            "$TMP/p.ax",
            "--kernel",
            "$TMP/k",
            "--initrd",
            "$TMP/i",
        ],
    ));
    c.files.push(("p.axmeta", "{ not json"));
    v.push(c);
    // Override widening the manifest → exit 2.
    let mut c = with_files(case(
        "run_override_widens",
        &[
            "run",
            "$TMP/p.ax",
            "--kernel",
            "$TMP/k",
            "--initrd",
            "$TMP/i",
        ],
    ));
    c.files
        .push(("p.axmeta", "{\"effect_union\":[\"IO\"],\"risk\":0}"));
    c.env.push(("AXON_VM_ALLOWED_EFFECTS", "IO,Net".into()));
    v.push(c);
    // The launch path itself: an absent firecracker binary (PATH emptied) is a
    // launcher failure reported the same way before and after extraction.
    let mut c = with_files(case(
        "run_launch_no_firecracker_json",
        &[
            "run",
            "$TMP/p.ax",
            "--kernel",
            "$TMP/k",
            "--initrd",
            "$TMP/i",
            "--no-attest",
            "--json",
        ],
    ));
    c.env.push(("AXON_VM_ALLOWED_EFFECTS", "IO".into()));
    c.env.push(("PATH", "$TMP/emptybin".into()));
    v.push(c);
    v
}

#[test]
fn axon_vm_cli_matches_pre_extraction_goldens() {
    let bless = std::env::var("AXON_VM_PARITY_BLESS").as_deref() == Ok("1");
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let mut failures = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let all = cases();
    for c in &all {
        // The no-firecracker case relies on firecracker NOT being at a fixed
        // absolute path the lookup also probes; skip only that case when it is.
        if c.name == "run_launch_no_firecracker_json"
            && (Path::new("/usr/local/bin/firecracker").exists()
                || Path::new("/opt/firecracker/firecracker").exists())
        {
            let what = format!(
                "axon-vm cli_parity case {} (firecracker present at a fixed path, \
                 so the no-firecracker golden cannot be reproduced on this host)",
                c.name
            );
            eprintln!("cli_parity: SKIPPED — {what}");
            skipped.push(what);
            continue;
        }
        let got = run_case(c);
        let path = dir.join(format!("{}.golden", c.name));
        if bless {
            std::fs::write(&path, &got).unwrap();
            continue;
        }
        let want = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("missing golden {}: {e}", path.display()));
        if want != got {
            failures.push(format!(
                "== {} ==\n--- want\n{want}\n--- got\n{got}",
                c.name
            ));
        }
    }
    assert!(all.len() >= 25, "parity suite shrank to {}", all.len());
    // Recorded before the drift assertion, escalated after it: a strict-mode
    // skip panic must not mask a real drift in the cases that DID run.
    for w in &skipped {
        note_skip(w);
    }
    assert!(
        failures.is_empty(),
        "{} CLI parity case(s) drifted:\n{}",
        failures.len(),
        failures.join("\n")
    );
    strict_skips_are_fatal(&skipped);
}
