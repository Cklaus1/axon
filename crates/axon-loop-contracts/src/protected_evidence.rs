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

/// `/2` adds the guest verdict (C9, PSV-5): `/1` carried nothing the receipt's
/// `guest-verdict-sha256` could be joined to.
pub const PSV_EVIDENCE_SCHEMA: &str = "axon-psv-evidence/2";

/// `axon-psv-evidence/2`: the EXACT bytes Fabric launched, observed and
/// derived its verdict from, as the receipt's digests name them.
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
    /// The guest verdict's exact bytes (`axon-guest-verdict/1`), the ones the
    /// launcher bound and Fabric derived the receipt's verification from.
    pub guest_verdict: String,
}

/// Exactly a sha256 as the stack writes one: 64 LOWERCASE hex characters.
pub fn is_sha256_hex(d: &str) -> bool {
    d.len() == 64 && d.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Every `*sha256` field of a launch manifest, by path, with its value (`""`
/// for a non-string), found by walking the manifest's OWN serialization, so a
/// field added later is covered too. Shared by the loop's check and Fabric's
/// `psv::prepare`, so the producer and the consumer judge the same fields.
pub fn manifest_digest_fields(
    m: &axon_psv::LaunchManifest,
) -> Result<Vec<(String, String)>, String> {
    fn walk(path: &str, v: &serde_json::Value, out: &mut Vec<(String, String)>) {
        if let serde_json::Value::Object(o) = v {
            for (k, x) in o {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                if k.ends_with("sha256") {
                    out.push((p.clone(), x.as_str().unwrap_or_default().to_string()));
                }
                walk(&p, x, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(
        "",
        &serde_json::to_value(m).map_err(|e| format!("the launch manifest: {e}"))?,
        &mut out,
    );
    Ok(out)
}

/// A protected manifest names every digest it carries (C9 round 1, PSV-7;
/// round 2, PSV-5). Fabric's `psv::prepare` writes an all-zero sha256 where it
/// has nothing to name (a library launch with no operator host config:
/// `host_config_sha256` and `suite.registry_sha256`), and the observation joins
/// it field for field, so the joins alone accept "no operator host" as a
/// value — and equally `""`, `unknown` (`verifier_identity()`'s fallback) or
/// any other string both documents agree on. Every `*sha256` field must be a
/// sha256 ([`is_sha256_hex`]) and not that placeholder.
fn names_every_digest(m: &axon_psv::LaunchManifest) -> Result<(), String> {
    for (p, d) in manifest_digest_fields(m)? {
        if !is_sha256_hex(&d) {
            return Err(format!(
                "the launch manifest's {p} is {d:?}, not a sha256 (64 lowercase hex): a protected \
                 launch names every digest it carries, so it is not protected evidence"
            ));
        }
        if d.bytes().all(|b| b == b'0') {
            return Err(format!(
                "the launch manifest's {p} is all zeros: a protected launch names no such digest \
                 (no operator host), so it is not protected evidence"
            ));
        }
    }
    Ok(())
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
///    (O2), naming that key, and registered by the store for exactly ONE of the
///    trusted `observers` (the store narrows the root, never widens it) — that
///    observer and key are returned, for intake and EVL to record and admission
///    to join (C9 round 2, PSV-5);
/// 3. the observation joins the manifest field for field (its digest, nonce,
///    profile, revisions, launcher, host config, guest, verifier, registry,
///    policy), and the observation's epoch and the manifest's authority (epoch,
///    tenant, family) are the loop's own `epoch` and `scope`;
/// 4. the manifest joins the request and receipt: operation, task, trial and
///    attempt; the candidate (the request's and receipt's WorkspaceVersion);
///    the suite (the receipt's `check-suite:` ref) and the test (the request's
///    argv); and the guest kernel/rootfs/axon/init and qualification digests the
///    receipt names;
/// 5. the guest verdict's bytes are the receipt's `guest-verdict-sha256`, name
///    this manifest, its test and its inputs, and claim the outcome the receipt
///    counts (Passed or Failed; a protected receipt carries no other).
pub fn check_bundle(
    req: &ComputeRequest,
    rc: &ExecutionReceipt,
    bundle: &str,
    epoch: u64,
    scope: &crate::Scope,
    observers: &std::collections::BTreeMap<crate::OpaqueRef, String>,
) -> Result<ObservationSigner, String> {
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
    names_every_digest(&m)?;
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
    // …by an observer the STORE trusts, under the key it registered for that
    // observer: the store narrows the operator root, never widens it, exactly
    // as for the verifier (O2). Any other key the root holds is some other
    // identity's, and the observation is attributed to no one (C9 round 2,
    // PSV-5, class c). Exactly one: two observers sharing a key cannot say
    // which of them observed.
    let by: Vec<&crate::OpaqueRef> = observers
        .iter()
        .filter(|(_, k)| crate::attestation::key_id_of_hex(k).as_deref() == Some(signer.as_str()))
        .map(|(who, _)| who)
        .collect();
    let observer = match by.as_slice() {
        [one] => (*one).clone(),
        [] => {
            return Err(format!(
                "the observation is signed by {signer}, which is no observer the store trusts \
                 (a key the operator root holds is authority only for the identity it is \
                 registered to)"
            ))
        }
        _ => {
            return Err(format!(
                "the observation is signed by {signer}, which {} trusted observers share: it is \
                 attributed to none of them",
                by.len()
            ))
        }
    };
    // 3. the observation joins the manifest
    o.joins(&m, &m_sha)?;
    // …and was made under the trial's authority epoch, which the loop joins
    // to its OWN scope pointer. A caller-chosen authority store, or an epoch
    // that moved while the observer ran, is another epoch (dev review round
    // wf_336353cb-a2b, PSV-6).
    if o.epoch != epoch {
        return Err(format!(
            "the observation is for authority epoch {}, not the trial's epoch {epoch}",
            o.epoch
        ));
    }
    // …and the LAUNCH was made under it, in this scope: the manifest names the
    // authority epoch and scope it was launched under, the observation's
    // intended-manifest digest covers them, and they are joined here to the
    // loop's OWN scope pointer (`epoch`, `scope`: the episode's, which intake
    // binds to the pointer). Before, the launch-time epoch was the caller's
    // `--expected-epoch` against a caller-named store, and the observer could
    // only echo it (C9 round 3, PSV-6; A82).
    if m.authority.epoch != epoch {
        return Err(format!(
            "the launch manifest is for authority epoch {}, not the trial's epoch {epoch}",
            m.authority.epoch
        ));
    }
    if m.authority.tenant_id != scope.tenant_id.as_str() {
        return Err(format!(
            "the launch manifest is for tenant {}, not the trial's tenant {}",
            m.authority.tenant_id, scope.tenant_id
        ));
    }
    if m.authority.task_family != scope.task_family.as_str() {
        return Err(format!(
            "the launch manifest is for task family {}, not the trial's family {}",
            m.authority.task_family, scope.task_family
        ));
    }
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
    // The suite, field by field through the ONE parser: never the manifest's
    // fields formatted into a string and compared, which joined a manifest
    // naming (version `V#x`, entry `accept.ax`) to a receipt the parser reads
    // as (version `V`, entry `x#accept.ax`) (C9 round 3, PSV-5; A81).
    let receipt_suite = format!("check-suite:{}", want("check-suite:")?);
    let (sid, sver, sentry) = crate::suite::parse_check_suite_ref(&receipt_suite)
        .map_err(|e| format!("the receipt's check-suite is not one suite: {e}"))?;
    let manifest_suite = (
        m.suite.id.as_str(),
        m.suite.version.as_str(),
        m.suite.entry.as_str(),
    );
    if manifest_suite != (sid, sver, sentry) {
        return Err(format!(
            "the launch manifest's suite (id {:?}, version {:?}, entry {:?}) is not the \
             receipt's check-suite (id {sid:?}, version {sver:?}, entry {sentry:?})",
            m.suite.id, m.suite.version, m.suite.entry
        ));
    }
    if req.argv.first().and_then(|a| a.strip_prefix("check:")) != Some(sid) {
        return Err(format!(
            "the request ran {:?}, not the manifest's suite {}",
            req.argv.first(),
            m.suite.id
        ));
    }
    // 5. the guest verdict (C9, PSV-5): the receipt's `guest-verdict-sha256`
    // names these bytes, and they are this launch's verdict for the outcome the
    // loop counts.
    let v_sha = axon_psv::sha256_hex(b.guest_verdict.as_bytes());
    if v_sha != want("guest-verdict-sha256:")? {
        return Err(format!(
            "the guest verdict's bytes are {v_sha}, not the receipt's guest-verdict-sha256"
        ));
    }
    let v: axon_psv::GuestVerdict = serde_json::from_str(&b.guest_verdict)
        .map_err(|e| format!("the guest verdict is malformed: {e}"))?;
    if v.schema != axon_psv::GUEST_VERDICT_SCHEMA {
        return Err(format!(
            "the guest verdict is {}, not {}",
            v.schema,
            axon_psv::GUEST_VERDICT_SCHEMA
        ));
    }
    if v.launch_manifest_sha256 != m_sha {
        return Err(format!(
            "the guest verdict is for launch manifest {}, not the bundle's {m_sha}",
            v.launch_manifest_sha256
        ));
    }
    if v.test != m.suite.test {
        return Err(format!(
            "the guest verdict's test is {}, not the manifest's {}",
            v.test, m.suite.test
        ));
    }
    if !(v.inputs.matches
        && v.inputs.candidate_tree_digest == m.candidate.tree_digest
        && v.inputs.suite_tree_digest == m.suite.tree_digest)
    {
        return Err("the guest verdict's inputs are not the manifest's".into());
    }
    use crate::ReceiptVerification as RV;
    use axon_psv::GuestStatus as GS;
    match (&rc.verification, v.status) {
        (RV::Passed, GS::Passed) | (RV::Failed, GS::Failed) => {}
        (counted, claimed) => {
            return Err(format!(
                "the guest verdict claims {claimed:?}, but the receipt counts {counted:?}"
            ))
        }
    }
    Ok(ObservationSigner {
        observer_ref: observer,
        key_id: signer,
    })
}

/// Who signed a protected verdict's preflight observation: a store-trusted
/// observer and the key it verified under. Intake and EVL record it, and
/// admission joins the record to the signer it re-verifies (C9 round 2,
/// PSV-5), as it does the context's `context_signed_by` (A62).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationSigner {
    pub observer_ref: crate::OpaqueRef,
    /// `ed25519:<16 hex>`.
    pub key_id: String,
}
