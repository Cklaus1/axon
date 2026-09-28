//! `axon-fabric` — the Fabric submit CLI. JSON in, JSON out.
//!
//! ```text
//! axon-fabric submit --request FILE|- --journal FILE --check-registry FILE
//!                    --grant-registry FILE
//!                    --store DIR --tenant T --family F --expected-epoch N
//!                    [--workspace DIR] [--state DIR (default <journal>.state)]
//!                    [--budget-micro N] [--budget-exec-ms N]
//!                    [--linux-launcher SH --linux-manifest JSON
//!                     --linux-evidence JSON [--linux-artifacts DIR]
//!                     [--linux-evidence-sig SIG] [--linux-waivers JSON]
//!                     [--linux-evidence-max-age-s N]
//!                     --linux-out-root DIR]
//!
//! The Linux profile is eligible only for an issuer-signed evidence record
//! (`<evidence>.sig` unless `--linux-evidence-sig`), verified against the
//! Ed25519 public keys in the OPERATOR's trust root,
//! `/etc/axon/trust/qualification/` (root-owned, not group/other
//! writable, no symlinks; a caller cannot choose it), no older than the max
//! age (default 30 days).
//! axon-fabric workspace-import --state DIR --tenant T --root DIR
//! axon-fabric verify-evidence --record FILE --issuers DIR [--signature FILE (default <record>.sig)]
//! axon-fabric verify-readiness --repo DIR
//!
//! `verify-readiness` is the AUTHORITATIVE verdict for the three protected
//! readiness components (governance/specs/v022-protected-suite-verdict.md):
//! the repository is read as evidence only, and authority comes solely from
//! the operator's `/etc/axon/trust/qualification/` (no flag chooses it). The
//! output names the build (`production` / `test-trust`); readiness accepts
//! only a production build, installed and pinned by the operator.
//!
//! `verify-evidence` checks an operator-signed evidence document (the v0.22
//! protected-host certification record) with the SAME rules as the B263
//! qualification record: exit 0 and `{"verified":true,"issuer":…}` only when
//! a detached `axon-evidence-signature/1` over the record's exact bytes
//! verifies under a key in `--issuers`; otherwise a refusal (exit 4).
//! axon-fabric status --journal FILE --op ID --grant-registry FILE --principal P --grant-ref G
//! axon-fabric cancel --journal FILE --op ID --reason TEXT --grant-registry FILE --principal P --grant-ref G
//!
//! `status` and `cancel` act only for the authority that SUBMITTED the op:
//! the grant must resolve for the principal in the operator's grant registry
//! (as for `submit`) and be the principal|grant the journal recorded for the
//! op. Holding a journal path or an operation id confers nothing
//! (G03-r22-authority-intersection); anything else is `unauthorized` (exit 7)
//! and nothing is written.
//! ```
//!
//! Output on success: `{"schema":"axon-fabric-submit/1", "receipt": <acf-execution-receipt/1>,
//! "check_report": …, "replayed": bool, "backend": …, "reason": …}`, exit 0 —
//! whatever the receipt status (a receipt is an answer, including
//! `unsupported`, `denied`, `failed`). A refusal that produced no receipt is
//! `{"schema":"axon-fabric-refusal/1","kind":…,"reason":…}` with a nonzero
//! exit (2 io/journal, 3 malformed, 4 unregistered, 5 conflict, 6 stale epoch,
//! 7 unauthorized: `grant_ref` not resolvable for `principal_ref`).
//!
//! SIGNED RECEIPTS (G01-r22-independent-issuer). The operator's check registry
//! may carry a `"signer": {"issuer_ref", "key_path", "public_key"}` block: the
//! verifier identity this Fabric issues receipts as, its PKCS#8 Ed25519 key,
//! and that key's public half, pinned. There is no per-call flag: a caller
//! cannot choose an issuer name or supply a key. The signer is refused before
//! any work unless the key file is a regular file owned by this uid and
//! readable by no one else, and derives exactly the pinned public key. After
//! the receipt is FINAL, Fabric signs an `acf-receipt-attestation/2` binding
//! issuer, key id, request and receipt digests and the receipt's identity —
//! never for a replay (the journal is the caller's to name, so a replayed
//! receipt may be one the caller wrote), and only if the check workload could
//! not have reached the key: the admitted
//! grant gives it no effect at all (the local interpreter cannot path-scope a
//! read, so any IO would reach the key file), or it ran in the protected
//! microVM. Otherwise `"receipt_attestation"` is `null` with
//! `"attestation_withheld"` saying why. `axon-fabric keygen --out PATH`
//! provisions a key (0600, never over an existing file) and prints the public
//! key to pin here and to register in the loop store's `verifier_keys`.
//!
//! Every executable comes from the `--check-registry` file (path + sha256),
//! never from the request. Every grant comes from the `--grant-registry` file
//! (`axon-fabric-grant-registry/1`, grant files pinned by sha256); the
//! interpreter effect ceiling is DERIVED from the resolved grant — there is no
//! flag that sets or removes it.

use std::io::Read;
use std::path::PathBuf;

use axon_fabric::backend::{LinuxProfileConfig, QualificationTrust, DEFAULT_EVIDENCE_MAX_AGE_S};
use axon_fabric::submit::{scope, EpochSource, SubmitConfig};
use axon_fabric::{Journal, ResourceVector};
use axon_loop_contracts::{AuthorityEpoch, OperationId};
use serde_json::json;

fn refuse(kind: &str, reason: &str, code: i32) -> ! {
    println!(
        "{}",
        json!({"schema": "axon-fabric-refusal/1", "kind": kind, "reason": reason})
    );
    std::process::exit(code)
}

struct Args(Vec<String>);

impl Args {
    fn opt(&self, flag: &str) -> Option<String> {
        self.0
            .iter()
            .position(|a| a == flag)
            .and_then(|i| self.0.get(i + 1).cloned())
    }
    fn req(&self, flag: &str) -> String {
        self.opt(flag)
            .unwrap_or_else(|| refuse("usage", &format!("{flag} is required"), 2))
    }
    fn num(&self, flag: &str, default: u64) -> u64 {
        match self.opt(flag) {
            None => default,
            Some(v) => v
                .parse()
                .unwrap_or_else(|_| refuse("usage", &format!("{flag} must be a number"), 2)),
        }
    }
}

fn main() {
    let mut argv = std::env::args().skip(1);
    let cmd = argv.next().unwrap_or_default();
    let a = Args(argv.collect());
    match cmd.as_str() {
        "submit" => submit(&a),
        "status" => status(&a),
        "cancel" => cancel(&a),
        "workspace-import" => workspace_import(&a),
        "keygen" => keygen(&a),
        "verify-evidence" => verify_evidence(&a),
        "verify-readiness" => verify_readiness(&a),
        _ => refuse(
            "usage",
            "usage: axon-fabric submit|status|cancel … (see --help in the source header)",
            2,
        ),
    }
}

/// The operator-configured signer in the check registry, if any. Every defect
/// refuses as `unregistered` (exit 4) before any work.
fn signer(registry: &std::path::Path) -> Option<(axon_loop_contracts::OpaqueRef, Vec<u8>)> {
    let bad = |why: String| -> ! { refuse("unregistered", &format!("registry signer: {why}"), 4) };
    let text = std::fs::read_to_string(registry).unwrap_or_else(|e| bad(e.to_string()));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| bad(e.to_string()));
    let sg = v.get("signer")?;
    let obj = sg
        .as_object()
        .unwrap_or_else(|| bad("not an object".into()));
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    if keys != ["issuer_ref", "key_path", "public_key"] {
        bad(format!(
            "must be exactly issuer_ref, key_path, public_key; has {keys:?}"
        ));
    }
    let field = |k: &str| {
        obj[k]
            .as_str()
            .unwrap_or_else(|| bad(format!("{k} is not a string")))
    };
    let id = axon_loop_contracts::OpaqueRef::new(field("issuer_ref"))
        .unwrap_or_else(|e| bad(format!("issuer_ref: {e}")));
    let mut path = PathBuf::from(field("key_path"));
    if path.is_relative() {
        path = registry
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join(path);
    }
    let meta = std::fs::symlink_metadata(&path)
        .unwrap_or_else(|e| bad(format!("key {}: {e}", path.display())));
    {
        use std::os::unix::fs::MetadataExt;
        if !meta.file_type().is_file() {
            bad(format!("key {} is not a regular file", path.display()));
        }
        // SAFETY: geteuid has no preconditions and cannot fail.
        let euid = unsafe { libc::geteuid() };
        if meta.uid() != euid || meta.mode() & 0o077 != 0 {
            bad(format!(
                "key {} must be owned by this uid and readable by no one else (mode {:o})",
                path.display(),
                meta.mode() & 0o777
            ));
        }
    }
    let key = std::fs::read(&path).unwrap_or_else(|e| bad(format!("key {}: {e}", path.display())));
    let pk = axon_loop_contracts::attestation::public_key_of(&key)
        .unwrap_or_else(|e| bad(format!("key {}: {e}", path.display())));
    if pk != field("public_key") {
        bad(format!(
            "key {} does not derive the pinned public_key",
            path.display()
        ));
    }
    Some((id, key))
}

fn verify_evidence(a: &Args) {
    let record = PathBuf::from(a.req("--record"));
    let issuers = PathBuf::from(a.req("--issuers"));
    let sig = a.opt("--signature").map(PathBuf::from).unwrap_or_else(|| {
        let mut s = record.as_os_str().to_owned();
        s.push(".sig");
        PathBuf::from(s)
    });
    match axon_fabric::backend::verify_operator_evidence(&record, &sig, &issuers) {
        Ok(issuer) => println!(
            "{}",
            serde_json::json!({"schema":"axon-fabric-verify-evidence/1","verified":true,"issuer":issuer})
        ),
        Err(e) => refuse("unregistered", &e, 4),
    }
}

/// The AUTHORITATIVE protected-readiness verdict for `--repo` (see
/// `axon_fabric::readiness`). Authority is the operator's root only: there is
/// no flag to choose it.
fn verify_readiness(a: &Args) {
    let repo = PathBuf::from(a.req("--repo"));
    let v = axon_fabric::readiness::protected_components(
        &repo,
        &axon_fabric::readiness::ReadinessTrust::operator(),
    );
    println!("{v}");
}

fn keygen(a: &Args) {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let out = PathBuf::from(a.req("--out"));
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .unwrap_or_else(|_| refuse("io", "key generation failed", 2));
    let pk = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .unwrap_or_else(|_| refuse("io", "generated key does not load", 2))
        .public_key()
        .as_ref()
        .to_vec();
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&out)
            .unwrap_or_else(|e| refuse("io", &format!("{}: {e}", out.display()), 2));
        f.write_all(pkcs8.as_ref())
            .unwrap_or_else(|e| refuse("io", &e.to_string(), 2));
    }
    println!(
        "{}",
        json!({
            "schema": "axon-fabric-issuer-key/1",
            "private_key": out,
            "public_key": pk.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "fingerprint": axon_loop_contracts::attestation::key_fingerprint(&pk),
        })
    );
}

fn submit(a: &Args) {
    // The protected profile's trust root is the OPERATOR's (/etc/axon/trust),
    // never the caller's and never a repository directory: whoever submits
    // cannot choose which issuers qualify it. Refused before anything else.
    if a.opt("--linux-trusted-issuers").is_some() {
        refuse(
            "usage",
            "--linux-trusted-issuers is not accepted: the protected profile's issuers are the \
             operator's, in /etc/axon/trust/qualification/",
            2,
        );
    }
    let req_src = a.req("--request");
    let text = if req_src == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .unwrap_or_else(|e| refuse("io", &e.to_string(), 2));
        s
    } else {
        std::fs::read_to_string(&req_src).unwrap_or_else(|e| refuse("io", &e.to_string(), 2))
    };
    let registry_path = PathBuf::from(a.req("--check-registry"));
    let registry = axon_cortex::runner::CheckRegistry::load(&registry_path)
        .unwrap_or_else(|e| refuse("unregistered", &e, 4));
    let issuer = signer(&registry_path);
    let grants = axon_fabric::GrantRegistry::load(&PathBuf::from(a.req("--grant-registry")))
        .unwrap_or_else(|e| refuse("unauthorized", &e, 7));
    let sc =
        scope(&a.req("--tenant"), &a.req("--family")).unwrap_or_else(|e| refuse("usage", &e, 2));
    let expected = AuthorityEpoch::new(a.num("--expected-epoch", u64::MAX))
        .unwrap_or_else(|e| refuse("usage", &format!("--expected-epoch: {e}"), 2));
    let linux = a.opt("--linux-launcher").map(|l| {
        let manifest = PathBuf::from(a.req("--linux-manifest"));
        let mut trust = QualificationTrust::operator();
        trust.max_age_s = a.num("--linux-evidence-max-age-s", DEFAULT_EVIDENCE_MAX_AGE_S);
        LinuxProfileConfig {
            launcher: PathBuf::from(l),
            manifest,
            artifacts_dir: a.opt("--linux-artifacts").map(PathBuf::from),
            evidence: PathBuf::from(a.req("--linux-evidence")),
            evidence_signature: a.opt("--linux-evidence-sig").map(PathBuf::from),
            waivers: a.opt("--linux-waivers").map(PathBuf::from),
            trust,
            out_root: PathBuf::from(a.req("--linux-out-root")),
        }
    });
    let cfg = SubmitConfig {
        journal: PathBuf::from(a.req("--journal")),
        registry,
        epoch: EpochSource::LoopStore {
            store: PathBuf::from(a.req("--store")),
            scope: sc,
        },
        expected_epoch: expected,
        workspace: PathBuf::from(a.opt("--workspace").unwrap_or_else(|| ".".into())),
        state_dir: state_dir(a),
        budget: ResourceVector {
            model_micro_usd: a.num("--budget-micro", 1_000_000),
            exec_ms: a.num("--budget-exec-ms", 3_600_000),
            verify_ms: a.num("--budget-verify-ms", 3_600_000),
            retries: a.num("--budget-retries", 1_000),
        },
        grants,
        linux,
        pre_launch_hook: None,
        fault_hook: None,
    };
    match axon_fabric::submit(&text, &cfg) {
        Ok(s) => {
            // The receipt is final here. Sign it only if the workload that
            // produced it could not have read the signing key.
            let (attestation, withheld) = match issuer {
                None => (None, None),
                Some((id, key)) => {
                    let req: axon_loop_contracts::ComputeRequest =
                        axon_loop_contracts::parse(&text)
                            .unwrap_or_else(|e| refuse("malformed", &e.to_string(), 3));
                    match axon_fabric::signing::attestation_decision(
                        &req,
                        s.replayed,
                        s.ran_under.as_ref(),
                    ) {
                        Ok(()) => {
                            let att = axon_loop_contracts::attestation::sign(
                                &key,
                                &id,
                                &req,
                                &s.receipt,
                                // Fabric's own clock: the trusted anchor EVL
                                // compares with the plan's freeze.
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0),
                            )
                            .unwrap_or_else(|e| refuse("io", &e, 2));
                            (Some(att), None)
                        }
                        Err(why) => (None, Some(why)),
                    }
                }
            };
            println!(
                "{}",
                json!({
                    "schema": "axon-fabric-submit/1",
                    "receipt": s.receipt,
                    "check_report": s.check_report,
                    "replayed": s.replayed,
                    "backend": s.backend,
                    "reason": s.reason,
                    "receipt_attestation": attestation,
                    "attestation_withheld": withheld,
                })
            )
        }
        Err(e) => refuse(e.kind(), &e.to_string(), e.exit_code()),
    }
}

/// `--state DIR`, default `<journal>.state` beside the journal.
fn state_dir(a: &Args) -> PathBuf {
    a.opt("--state").map(PathBuf::from).unwrap_or_else(|| {
        let mut s = PathBuf::from(a.req("--journal")).into_os_string();
        s.push(".state");
        PathBuf::from(s)
    })
}

/// `axon-fabric workspace-import --state DIR --tenant T --root DIR`: import a tree into
/// the WorkspaceVersion store and print its reference (and omissions).
fn workspace_import(a: &Args) {
    let tenant = axon_loop_contracts::TenantId::new(a.req("--tenant"))
        .unwrap_or_else(|e| refuse("usage", &format!("--tenant: {e}"), 2));
    let store =
        axon_fabric::workspace::WorkspaceStore::open(&PathBuf::from(a.req("--state")), &tenant)
            .unwrap_or_else(|e| refuse("workspace", &e.to_string(), 2));
    let tree = axon_fabric::workspace::WorkspaceTree::import_dir(
        &PathBuf::from(a.req("--root")),
        &axon_fabric::workspace::Quota::default(),
    )
    .unwrap_or_else(|e| refuse(e.class(), &e.to_string(), 3));
    let r = store
        .publish(&tree)
        .unwrap_or_else(|e| refuse("workspace", &e.to_string(), 2));
    println!(
        "{}",
        json!({"schema": "axon-fabric-workspace-import/1", "workspace_version_ref": r,
               "entries": tree.entries().len(), "omissions": tree.omissions()})
    );
}

fn open(a: &Args) -> (Journal, OperationId) {
    let op = OperationId::new(a.req("--op")).unwrap_or_else(|e| refuse("usage", &e.to_string(), 2));
    let (j, _) = Journal::open(PathBuf::from(a.req("--journal")))
        .unwrap_or_else(|e| refuse("journal", &e.to_string(), 2));
    (j, op)
}

/// G03-r22-authority-intersection: the caller must present the authority that
/// submitted `op` — a grant the operator's registry resolves for the principal,
/// equal to the op's recorded `principal|grant`. Refused before anything is
/// written or disclosed beyond the op's existence.
fn authorize(a: &Args, j: &Journal, op: &OperationId) {
    let grants = axon_fabric::GrantRegistry::load(&PathBuf::from(a.req("--grant-registry")))
        .unwrap_or_else(|e| refuse("unauthorized", &e, 7));
    let (principal, grant) = (a.req("--principal"), a.req("--grant-ref"));
    grants
        .resolve(&grant, &principal)
        .unwrap_or_else(|e| refuse("unauthorized", &e, 7));
    let Some(v) = j.view(op) else {
        refuse("unknown_op", &format!("no operation {op}"), 5)
    };
    if v.intent.authority_ref != format!("{principal}|{grant}") {
        refuse(
            "unauthorized",
            &format!("operation {op} was not submitted under {principal}|{grant}"),
            7,
        )
    }
}

fn status(a: &Args) {
    let (j, op) = open(a);
    authorize(a, &j, &op);
    print_status(&j, &op);
}

fn print_status(j: &Journal, op: &OperationId) {
    match j.view(op) {
        None => refuse("unknown_op", &format!("no operation {op}"), 5),
        Some(v) => println!(
            "{}",
            json!({
                "schema": "axon-fabric-status/1",
                "operation_id": op,
                "state": v.state,
                "launched": v.launched,
                "billing": v.billing,
                "reason": v.reason,
                "outcome": v.outcome,
                "scope_usage": j.scope_usage(&v.intent.scope).ok().map(|u| json!({
                    "held": u.held, "liability": u.liability, "charged": u.charged,
                    "ceiling": u.ceiling,
                })),
            })
        ),
    }
}

/// Cancel. A never-launched op is RELEASED; a launched one keeps its whole
/// reservation as unresolved liability (cancel acknowledgement is not
/// cleanup, and no cost evidence exists).
fn cancel(a: &Args) {
    let (j, op) = open(a);
    authorize(a, &j, &op);
    let reason = a.req("--reason");
    let Some(v) = j.view(&op) else {
        refuse("unknown_op", &format!("no operation {op}"), 5)
    };
    let billing = v.launched.then_some(axon_fabric::Billing::Unknown);
    match j.cancel(&op, &reason, billing) {
        Ok(()) => print_status(&j, &op),
        Err(e) => refuse("journal", &e.to_string(), 5),
    }
}
