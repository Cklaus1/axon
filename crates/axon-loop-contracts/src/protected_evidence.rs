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

// ── B2 (review wf_d725935a-7ed): the JOINS, over the exact documents ────────

pub const PSV_EVIDENCE_SCHEMA: &str = "axon-psv-evidence/1";

/// `axon-psv-evidence/1`: the EXACT bytes Fabric launched and observed, as the
/// receipt's digests name them.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PsvEvidence {
    pub schema: String,
    /// The launch manifest's canonical bytes.
    pub launch_manifest: String,
    /// The observation's exact bytes, as the observer signed them.
    pub observation: String,
    /// Its detached OBSERVER-domain `axon-evidence-signature/2`.
    pub observation_signature: String,
}

fn one_ref<'a>(rc: &'a ExecutionReceipt, prefix: &str) -> Option<&'a str> {
    let mut it = rc
        .evidence_refs
        .iter()
        .filter_map(|e| e.as_str().strip_prefix(prefix));
    match (it.next(), it.next()) {
        (Some(v), None) => Some(v),
        _ => None,
    }
}

/// Everything [`check`] requires, PLUS the joins over the documents:
/// 1. the manifest's bytes are the receipt's `launch-manifest-sha256`, and a
///    well-formed protected manifest;
/// 2. the observation's bytes are the receipt's `preflight-observation-sha256`,
///    signed in the OBSERVER domain by a key the OPERATOR's observer root holds
///    (O2), naming that key;
/// 3. the observation joins the manifest field for field (its digest, nonce,
///    profile, revisions, launcher, host config, guest, verifier, registry,
///    policy);
/// 4. the manifest joins the request and receipt: operation, task, trial and
///    attempt; the candidate (the request's and receipt's WorkspaceVersion);
///    the suite (the receipt's `check-suite:` ref) and the test (the request's
///    argv); and the guest kernel/rootfs/axon/init and qualification digests the
///    receipt names.
pub fn check_bundle(
    req: &ComputeRequest,
    rc: &ExecutionReceipt,
    bundle: &str,
) -> Result<(), String> {
    use crate::operator_trust::{rooted_keys, verify_evidence_signature, TrustAuthority};
    check(req, rc)?;
    let b: PsvEvidence = serde_json::from_str(bundle)
        .map_err(|e| format!("the protected evidence bundle is malformed: {e}"))?;
    if b.schema != PSV_EVIDENCE_SCHEMA {
        return Err(format!(
            "the protected evidence bundle is not {PSV_EVIDENCE_SCHEMA}"
        ));
    }
    let want = |p: &str| one_ref(rc, p).ok_or_else(|| format!("no single {p} ref"));
    // 1. the manifest
    let m_sha = axon_psv::sha256_hex(b.launch_manifest.as_bytes());
    if m_sha != want("launch-manifest-sha256:")? {
        return Err(format!("the launch manifest is {m_sha}, not the receipt's"));
    }
    let m = axon_psv::LaunchManifest::verify(b.launch_manifest.as_bytes(), &m_sha)?;
    // 2. the observation, authenticated under the OPERATOR's observer root
    let o_sha = axon_psv::sha256_hex(b.observation.as_bytes());
    if o_sha != want("preflight-observation-sha256:")? {
        return Err(format!("the observation is {o_sha}, not the receipt's"));
    }
    let signer = verify_evidence_signature(
        "observation",
        b.observation.as_bytes(),
        &b.observation_signature,
        &rooted_keys(TrustAuthority::Observer)?,
        TrustAuthority::Observer,
    )?;
    let o: axon_psv::PreflightObservation = serde_json::from_str(&b.observation)
        .map_err(|e| format!("the observation is malformed: {e}"))?;
    if o.observer_key_id != signer {
        return Err(format!(
            "the observation claims observer {} but is signed by {signer}",
            o.observer_key_id
        ));
    }
    // 3. the observation joins the manifest
    o.joins(&m, &m_sha)?;
    // 4. the manifest joins the request and the receipt
    let pairs: [(&str, &str, &str); 12] = [
        ("operation_id", &m.operation_id, req.operation_id.as_str()),
        ("task_id", &m.task_id, req.task_id.as_str()),
        ("trial_id", &m.trial_id, req.trial_id.as_str()),
        ("attempt_id", &m.attempt_id, req.attempt_id.as_str()),
        (
            "candidate",
            &m.candidate.workspace_version,
            req.workspace_version_ref.as_str(),
        ),
        (
            "candidate (receipt)",
            &m.candidate.workspace_version,
            rc.input_workspace_ref.as_str(),
        ),
        (
            "test",
            &m.suite.test,
            req.argv.get(1).map(String::as_str).unwrap_or(""),
        ),
        (
            "guest kernel",
            &m.guest.kernel_sha256,
            want("guest-kernel-sha256:")?,
        ),
        (
            "guest rootfs",
            &m.guest.rootfs_sha256,
            want("guest-rootfs-sha256:")?,
        ),
        (
            "guest axon",
            &m.guest.axon_sha256,
            want("guest-axon-sha256:")?,
        ),
        (
            "guest init",
            &m.guest.init_sha256,
            want("guest-init-sha256:")?,
        ),
        (
            "qualification",
            &m.qualification_sha256,
            want("qualification-sha256:")?,
        ),
    ];
    for (what, manifest, other) in pairs {
        if manifest != other {
            return Err(format!(
                "the launch manifest's {what} is {manifest}, but the request/receipt names {other}"
            ));
        }
    }
    let suite_ref = format!("{}@{}#{}", m.suite.id, m.suite.version, m.suite.entry);
    if want("check-suite:")? != suite_ref {
        return Err(format!(
            "the launch manifest's suite {suite_ref} is not the receipt's check-suite"
        ));
    }
    if req.argv.first().map(String::as_str) != Some(&format!("check:{}", m.suite.id)) {
        return Err(format!(
            "the request ran {:?}, not the manifest's suite {}",
            req.argv.first(),
            m.suite.id
        ));
    }
    Ok(())
}
