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
