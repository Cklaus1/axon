//! No job reaches supervisor execution without an authorization result.
//!
//! REPRODUCED: `cmd_run` verified the `.approval` token and refused with exit 8;
//! `cmd_replay` reached `supervisor::run` by a different route and performed no
//! approval check at all. Same manifest — `run` refused with no side effect,
//! `replay` exited 0 and the side-effect file's mtime advanced. The public
//! `axon_os::supervise` re-export was a third route with no gate whatsoever.
//!
//! The fix is not a check added to `cmd_replay`. Authorization moved INTO
//! `supervisor::run`, the point every execution path converges on — and which
//! already hosted the one check (`gate::admit`) no caller could skip. A check
//! in a caller is opt-in per call site; three call sites existed and two had
//! forgotten it.

use axon_os::approval::{authorize, ApprovalStatus};
use axon_os::manifest::parse;
use std::path::Path;

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn manifest_src(require: bool) -> String {
    format!(
        "program = \"p.ax\"\nintent = \"t\"\nseed = 1\nrequire_approval = {require}\n\
         [grant]\nfs_read = [\"./\"]\nfs_write = [\"./out/\"]\nnet = []\n\
         exec = \"none\"\nmax_label = \"internal\"\n\
         [grant.budget]\ncalls = 1\ntokens = 1\ncost_micro = 0\n"
    )
}

#[test]
fn a_job_requiring_approval_without_a_token_is_refused() {
    let m = parse(&manifest_src(true), Path::new(".")).expect("parse");
    let err = authorize(Path::new("/nonexistent/job.axjob"), &m)
        .expect_err("a job requiring approval with no token must be refused");
    assert!(
        err.contains("approval required but missing"),
        "the refusal must say why: {err}"
    );
}

/// Control: the gate must not refuse everything.
#[test]
fn a_job_not_requiring_approval_authorizes_as_not_required() {
    let m = parse(&manifest_src(false), Path::new(".")).expect("parse");
    let status = authorize(Path::new("/nonexistent/job.axjob"), &m)
        .expect("a job that does not require approval must authorize");
    assert_eq!(status, ApprovalStatus::NotRequired);
}

/// The authorization decision must be REACHABLE from the archived record.
///
/// `RunRecord` previously had no approval field, so an authorized run and an
/// unauthorized one archived identically — a reviewer could not tell "signed
/// off" from "nobody checked". An older record without the field reads as
/// `unknown`, never as approved: an absent statement is not a positive one.
#[test]
fn the_run_record_carries_the_authorization_decision() {
    // Built by SERIALIZING a real record and removing the field, rather than
    // hand-writing JSON — a hand-written fixture guesses at the Verdict
    // encoding and fails for its own reasons, which says nothing about the
    // property under test.
    let real = axon_os::build_record(
        "r",
        &parse(&manifest_src(false), Path::new(".")).expect("parse"),
        1,
        &[],
        axon_os::Verdict::Completed { value: 0 },
    );
    let mut v: serde_json::Value = serde_json::to_value(&real).expect("a record must serialize");
    v.as_object_mut().expect("object").remove("approval");
    let json = v.to_string();
    let rec: axon_os::RunRecord =
        serde_json::from_str(&json).expect("a pre-field record must still load");
    assert_eq!(
        rec.approval, "unknown",
        "a record written before this field must read as unknown, not as approved"
    );
}

// ── The approval token must survive archival, and a denial must say so ───────

/// A run's approval token has to be archived with the job it approved.
///
/// REPRODUCED: `cmd_run` writes `<run_id>.axjob` into the store and stops
/// there. `cmd_replay` re-reads that archived manifest and, by design, looks
/// for `<run_id>.approval` BESIDE IT — a comment in cli.rs says exactly that:
/// "the token must travel with the archived job". Nothing ever put it there.
///
/// So every approval-required job is unreplayable, and worse, it fails as the
/// wrong thing: `authorize` returns "approval required but missing", the
/// supervisor records a denial, and `cmd_replay` maps that to exit 11 —
/// TAMPER. A reviewer reading that sees "this record was altered", when the
/// truth is "the operator's own archive is missing a file it never wrote".
/// Confusing a refusal with evidence of tampering is the more expensive error.
#[test]
fn the_approval_token_is_archived_beside_the_job_it_approved() {
    use std::process::Command;

    let d = std::env::temp_dir().join(format!("axon_os_arch_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let prog = d.join("p.ax");
    // A pure program: the job grants no IO, so a printing main would be
    // refused on the sandbox axis and never reach the archival question.
    std::fs::write(&prog, "fn main() { let _ = 1 + 1 }\n").unwrap();

    let job = d.join("j.axjob");
    std::fs::write(
        &job,
        format!(
            "program = \"{}\"\nintent = \"t\"\nseed = 1\nrequire_approval = true\n\
             [grant]\nfs_read = []\nfs_write = []\nnet = []\nexec = \"none\"\n\
             max_label = \"internal\"\n[grant.budget]\ncalls = 1\ntokens = 1\ncost_micro = 0\n",
            prog.display()
        ),
    )
    .unwrap();

    // Mint a token that verifies against this exact program + grant.
    let m =
        axon_os::manifest::parse(&std::fs::read_to_string(&job).unwrap(), &d).expect("manifest");
    let src = std::fs::read_to_string(&prog).unwrap();
    let unit = "\u{1f}";
    let pd = format!("axsha256:{}", sha256_hex(src.as_bytes()));
    let gd = format!(
        "axsha256:{}",
        sha256_hex(axon_os::approval::canonical_grant(&m.grant).as_bytes())
    );
    let canon = format!("{pd}{unit}{gd}{unit}auditor{unit}approved{unit}low");
    let td = format!("axtok1:{}", sha256_hex(canon.as_bytes()));
    std::fs::write(
        job.with_extension("approval"),
        format!(
            "{{\"schema\":\"axon-approval/1\",\"program_digest\":\"{pd}\",\
             \"grant_digest\":\"{gd}\",\"approved_by\":\"auditor\",\
             \"decision\":\"approved\",\"risk\":\"low\",\"token_digest\":\"{td}\"}}"
        ),
    )
    .unwrap();

    let store = d.join("store");
    let bin = env!("CARGO_BIN_EXE_axon-os");
    // The interpreter is a separate binary. Archival does not depend on it —
    // `cmd_run` writes the archive whatever the verdict — so the assertion
    // that matters runs either way, and only the replay leg needs it.
    let interp = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|r| r.join("target/debug/axon"));
    let mut cmd = Command::new(bin);
    cmd.args(["run"])
        .arg(&job)
        .arg("--run-id")
        .arg("r1")
        .arg("--out")
        .arg(&store);
    let have_interp = match &interp {
        Some(p) if p.exists() => {
            cmd.env("AXON_BIN", p);
            true
        }
        _ => false,
    };
    let run = cmd.output().unwrap();
    let run_txt = String::from_utf8_lossy(&run.stdout).to_string();
    assert!(
        run_txt.contains("required and verified"),
        "premise: the token must verify, or this tests the wrong refusal: {run_txt}"
    );

    // The archived job is there. The token that authorized it must be too.
    assert!(store.join("r1.axjob").exists(), "the job was not archived");
    assert!(
        store.join("r1.approval").exists(),
        "the approval token was not archived beside the job it approved, so \
         replay cannot find it"
    );

    // And the consequence, checked directly rather than inferred: replay of an
    // APPROVED job must not report tamper.
    if !have_interp {
        // Not a silent skip: say which leg did not run and why, so a green
        // result cannot be mistaken for a complete one.
        eprintln!(
            "replay_authorization: the REPLAY leg did not run — no interpreter at {:?}. \
             The archival assertions above did run.",
            interp
        );
        let _ = std::fs::remove_dir_all(&d);
        return;
    }
    assert_eq!(
        run.status.code(),
        Some(0),
        "the approved run must succeed before replay means anything: {run_txt}"
    );
    let rep = Command::new(bin)
        .args(["replay", "r1", "--store"])
        .arg(&store)
        .env("AXON_BIN", interp.as_ref().unwrap())
        .output()
        .unwrap();
    let rep_txt = String::from_utf8_lossy(&rep.stdout).to_string();
    assert_ne!(
        rep.status.code(),
        Some(11),
        "replay of an APPROVED job reported tamper/divergence, which is a \
         false accusation — the token was simply never archived: {rep_txt}"
    );
    assert_eq!(rep.status.code(), Some(0), "replay failed: {rep_txt}");

    let _ = std::fs::remove_dir_all(&d);
}

/// A run refused FOR LACK OF APPROVAL must not record "approval: not_required".
///
/// The denial branch stamped `ApprovalStatus::NotRequired`, so the record of a
/// job that was refused because its sign-off was missing or invalid says the
/// job did not need sign-off. That is the absent-vs-verified collapse the
/// field exists to close, inverted: the record contradicts the very verdict it
/// carries.
#[test]
fn a_run_denied_on_approval_records_that_it_was_denied() {
    use axon_os::record::RunRecord;
    let m = parse(&manifest_src(true), Path::new(".")).expect("parse");
    let rt = axon_os::runtime::AxonCoreRuntime::from_env();
    let rec: RunRecord = axon_os::supervisor::run(
        &m,
        Path::new("/nonexistent/job.axjob"),
        // The supervisor grant is irrelevant here: authorization is step 0 and
        // returns before any intersect, which is the point of moving it there.
        &m.grant.clone(),
        "rid",
        &rt,
    );
    assert!(
        matches!(&rec.verdict, axon_os::verdict::Verdict::Denied { axis, .. } if axis == "approval"),
        "premise: this run must be denied on the approval axis"
    );
    assert_ne!(
        rec.approval, "not_required",
        "a run refused because approval was MISSING recorded that approval was \
         not required — the record contradicts its own verdict"
    );
    assert_eq!(rec.approval, "denied");
}
