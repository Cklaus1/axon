//! M4 — what a receipt must carry to be PROTECTED evidence
//! (`governance/specs/v022-psv-protocol.md` §9).
//!
//! A pure check over one verification request and its receipt, shared by
//! intake and EVL. It never reads a file. The receipt counts as protected only
//! if every one of these holds:
//! * its ONE evidence class is `protected` (not `guest-unobserved`, not
//!   `development`, not two classes);
//! * it came from a protected backend;
//! * it names, exactly once each and well-formed, what the Fabric verified
//!   before and after the launch — the launch manifest, the preflight
//!   observation, the guest verdict, and the guest kernel, rootfs and
//!   interpreter;
//! * the interpreter that ran in the guest is the one the request pinned (the
//!   request's `executable_digest` is `acf1` over that exact sha256).
//!
//! Authentication (the signature under an operator-rooted verifier key, O2)
//! and the suite/test/identity joins are intake's `verify_check_evidence` and
//! `check_pins`. This adds only what makes a verdict PROTECTED.

use crate::{ComputeRequest, ExecutionReceipt};

pub const PROTECTED_CLASS_REF: &str = "evidence-class:protected";

/// The references a protected receipt must carry exactly once, each a sha256.
pub const REQUIRED_DIGEST_REFS: [&str; 8] = [
    "launch-manifest-sha256:",
    "preflight-observation-sha256:",
    "guest-verdict-sha256:",
    "guest-kernel-sha256:",
    "guest-rootfs-sha256:",
    "guest-axon-sha256:",
    "guest-init-sha256:",
    "qualification-sha256:",
];

/// The request's `executable_digest` for executable `id` whose bytes hash to
/// `sha256` (the Fabric's `acf1` identity; shared so every side derives it the
/// same way).
pub fn executable_digest(id: &str, sha256: &str) -> String {
    axon_cortex::runner::fabric_executable_digest(id, sha256)
}

/// Whether the receipt CLAIMS to be protected evidence (any class ref
/// `protected`). A claim is then held to [`check`].
pub fn claims_protected(rc: &ExecutionReceipt) -> bool {
    rc.evidence_refs
        .iter()
        .any(|e| e.as_str() == PROTECTED_CLASS_REF)
}

/// `Ok` iff `rc` (for `req`) is protected evidence; `Err` names the first
/// reason it is not.
pub fn check(req: &ComputeRequest, rc: &ExecutionReceipt) -> Result<(), String> {
    let refs: Vec<&str> = rc.evidence_refs.iter().map(|e| e.as_str()).collect();
    let classes: Vec<&str> = refs
        .iter()
        .filter_map(|e| e.strip_prefix("evidence-class:"))
        .collect();
    match classes.as_slice() {
        ["protected"] => {}
        [] => return Err("the receipt states no evidence class: not protected evidence".into()),
        [one] => {
            return Err(format!(
                "the receipt's evidence class is {one}, not protected (only a guest verdict with a \
                 verified preflight observation is)"
            ))
        }
        many => {
            return Err(format!(
                "the receipt states {} evidence classes",
                many.len()
            ))
        }
    }
    if !crate::PROTECTED_PROFILES.contains(&rc.backend_profile_ref.as_str()) {
        return Err(format!(
            "a protected-class receipt from backend {}, which is not a protected profile",
            rc.backend_profile_ref
        ));
    }
    let mut digests = std::collections::BTreeMap::new();
    for prefix in REQUIRED_DIGEST_REFS {
        let found: Vec<&str> = refs.iter().filter_map(|e| e.strip_prefix(prefix)).collect();
        match found.as_slice() {
            [d] if d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()) => {
                digests.insert(prefix, *d);
            }
            [] => return Err(format!("a protected receipt names no {prefix}…")),
            [d] => return Err(format!("{prefix}{d} is not a sha256")),
            _ => {
                return Err(format!(
                    "a protected receipt names {prefix}… more than once"
                ))
            }
        }
    }
    // The interpreter the guest ran is the one the request pinned.
    let axon = digests["guest-axon-sha256:"];
    let want = executable_digest(req.registered_executable_ref.as_str(), axon);
    if req.executable_digest.as_str() != want {
        return Err(format!(
            "the guest interpreter {axon} is not the one the request pinned ({})",
            req.executable_digest
        ));
    }
    Ok(())
}
