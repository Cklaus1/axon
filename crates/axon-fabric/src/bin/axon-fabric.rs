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
//!                     [--linux-trusted-issuers DIR]
//!                     [--linux-evidence-max-age-s N]
//!                     --linux-out-root DIR]
//!
//! The Linux profile is eligible only for an issuer-signed evidence record
//! (`<evidence>.sig` unless `--linux-evidence-sig`), verified against the
//! Ed25519 public keys in `--linux-trusted-issuers` (default: the manifest's
//! sibling `trusted_issuers/`), no older than the max age (default 30 days).
//! axon-fabric workspace-import --state DIR --root DIR
//! axon-fabric status --journal FILE --op ID
//! axon-fabric cancel --journal FILE --op ID --reason TEXT
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
        _ => refuse(
            "usage",
            "usage: axon-fabric submit|status|cancel … (see --help in the source header)",
            2,
        ),
    }
}

fn submit(a: &Args) {
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
    let registry =
        axon_cortex::runner::CheckRegistry::load(&PathBuf::from(a.req("--check-registry")))
            .unwrap_or_else(|e| refuse("unregistered", &e, 4));
    let grants = axon_fabric::GrantRegistry::load(&PathBuf::from(a.req("--grant-registry")))
        .unwrap_or_else(|e| refuse("unauthorized", &e, 7));
    let sc =
        scope(&a.req("--tenant"), &a.req("--family")).unwrap_or_else(|e| refuse("usage", &e, 2));
    let expected = AuthorityEpoch::new(a.num("--expected-epoch", u64::MAX))
        .unwrap_or_else(|e| refuse("usage", &format!("--expected-epoch: {e}"), 2));
    let linux = a.opt("--linux-launcher").map(|l| {
        let manifest = PathBuf::from(a.req("--linux-manifest"));
        let mut trust = QualificationTrust::for_manifest(&manifest);
        if let Some(d) = a.opt("--linux-trusted-issuers") {
            trust.issuers_dir = PathBuf::from(d);
        }
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
    };
    match axon_fabric::submit(&text, &cfg) {
        Ok(s) => println!(
            "{}",
            json!({
                "schema": "axon-fabric-submit/1",
                "receipt": s.receipt,
                "check_report": s.check_report,
                "replayed": s.replayed,
                "backend": s.backend,
                "reason": s.reason,
            })
        ),
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

/// `axon-fabric workspace-import --state DIR --root DIR`: import a tree into
/// the WorkspaceVersion store and print its reference (and omissions).
fn workspace_import(a: &Args) {
    let store = axon_fabric::workspace::WorkspaceStore::open(&PathBuf::from(a.req("--state")))
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

fn status(a: &Args) {
    let (j, op) = open(a);
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
