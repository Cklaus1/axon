//! B282 / G03-r22-joint-bypass, Fabric routes the survey found unattacked:
//!
//! * a MALICIOUS REPOSITORY that ships its own check and grant registries —
//!   authority comes only from the operator's files, never the workspace;
//! * the ATTACH route (`Journal::record_outcome`) — an outcome is attached once,
//!   to a terminal op, before any write; a second or premature attach is
//!   refused and writes nothing;
//! * an argv file that escapes the workspace.
//!
//! The legacy single-file fallback's time-of-check/time-of-use swap is
//! `workspace.rs::a_legacy_single_file_swapped_before_launch_never_yields_a_verdict_on_the_original`.

mod common;
use common::*;

use axon_fabric::{submit, Journal};
use serde_json::json;

/// The workspace ships a check registry and grant registry of its own, naming a
/// hostile executable and an all-powerful grant. A request that names them is
/// refused as unregistered/unauthorized; nothing is journalled or spawned.
#[test]
fn a_repository_cannot_supply_its_own_registries() {
    let env = Env::new();
    let evil = env.ws.join("evil.sh");
    std::fs::write(&evil, "#!/bin/sh\ntouch \"$(dirname \"$0\")/PWNED\"\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&evil, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(
        env.ws.join("registry.json"),
        json!({"schema":"cortex-check-registry/1","executors":[
            {"id":"repo-evil","path":evil,"sha256":sha256_file(&evil)}]})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        env.ws.join("everything.axgrant"),
        "profile = \"developer\"\n[grant]\nmax_label = \"internal\"\n",
    )
    .unwrap();
    std::fs::write(
        env.ws.join("grants.json"),
        json!({"schema":"axon-fabric-grant-registry/1","grants":[
            {"grant_ref":"grant:repo","principal_ref":PRINCIPAL,"path":"everything.axgrant",
             "sha256":sha256_file(&env.ws.join("everything.axgrant"))}]})
        .to_string(),
    )
    .unwrap();

    let mut exe = request(&env, "op-repo-exe", "t_ok");
    exe["registered_executable_ref"] = json!("repo-evil");
    exe["executable_digest"] = json!(axon_cortex::runner::fabric_executable_digest(
        "repo-evil",
        &sha256_file(&evil)
    ));
    let e = submit(&exe.to_string(), &env.cfg(0)).unwrap_err();
    assert_eq!(e.kind(), "unregistered", "{e}");

    let mut grant = request(&env, "op-repo-grant", "t_ok");
    grant["grant_ref"] = json!("grant:repo");
    let e = submit(&grant.to_string(), &env.cfg(0)).unwrap_err();
    assert_eq!(e.kind(), "unauthorized", "{e}");

    assert!(
        !env.ws.join("PWNED").exists(),
        "the repository's executable ran"
    );
    assert_eq!(spawn_count(&env.spawns), 0);
    assert!(
        !env.journal_text().contains("op-repo-"),
        "a refused request was journalled"
    );
}

/// The attach route: a finished op's outcome is attached once. A second,
/// DIFFERENT outcome (which every later replay would return) is refused before
/// any byte is written; so is an outcome on an op that has not finished.
#[test]
fn an_outcome_cannot_be_reattached_or_attached_early() {
    let env = Env::new();
    let req = request(&env, "op-attach", "t_ok");
    let first = submit(&req.to_string(), &env.cfg(0)).unwrap();
    let (j, _) = Journal::open(&env.journal).unwrap();
    let op = axon_loop_contracts::OperationId::new("op-attach").unwrap();
    let before = std::fs::read(&env.journal).unwrap();
    let forged = json!({"receipt": {"forged": true, "verification": "passed"}});
    assert!(
        j.record_outcome(&op, forged).is_err(),
        "a second outcome was attached"
    );
    assert_eq!(
        std::fs::read(&env.journal).unwrap(),
        before,
        "the refusal wrote bytes"
    );
    drop(j);
    // Every later replay still returns the ORIGINAL receipt.
    let again = submit(&req.to_string(), &env.cfg(0)).unwrap();
    assert!(again.replayed);
    assert_eq!(again.receipt, first.receipt);

    // Early attach: an op that is only reserved.
    let mut cfg = env.cfg(0);
    cfg.pre_launch_hook = Some(|_| panic!("stop before launch"));
    let early = request(&env, "op-early", "t_ok");
    let r = std::panic::catch_unwind(|| submit(&early.to_string(), &cfg));
    assert!(r.is_err(), "the hook stops the submit before launch");
    let (j, _) = Journal::open(&env.journal).unwrap();
    let op = axon_loop_contracts::OperationId::new("op-early").unwrap();
    let before = std::fs::read(&env.journal).unwrap();
    assert!(j.record_outcome(&op, json!({"receipt": {}})).is_err());
    assert_eq!(std::fs::read(&env.journal).unwrap(), before);
}

/// The argv file is resolved under the workspace; a path that escapes it is
/// refused before anything is journalled or run.
#[test]
fn an_argv_file_outside_the_workspace_is_refused() {
    let env = Env::new();
    for (i, bad) in ["../f.ax", "/etc/passwd", "sub/../../f.ax"]
        .iter()
        .enumerate()
    {
        let mut r = request(&env, &format!("op-escape-{i}"), "t_ok");
        r["argv"] = json!([bad, "t_ok"]);
        assert!(
            submit(&r.to_string(), &env.cfg(0)).is_err(),
            "{bad} was accepted"
        );
    }
    assert_eq!(spawn_count(&env.spawns), 0);
    assert_eq!(env.launch_records(), 0);
}

/// G26-r22-speculation-disabled: there is no speculative-effect path, and a
/// request cannot ask for one — the request schema is closed, so any attempt
/// to mark work speculative, pre-dispatch an alternative, or run a second
/// dispatch authority is refused before anything is journalled or run.
#[test]
fn a_request_cannot_ask_for_speculative_dispatch() {
    let env = Env::new();
    for (i, (field, value)) in [
        ("speculative", json!(true)),
        ("speculate_alternatives", json!(2)),
        ("dispatch_authority", json!("speculation-engine")),
    ]
    .into_iter()
    .enumerate()
    {
        let mut r = request(&env, &format!("op-spec-{i}"), "t_ok");
        r[field] = value;
        let e = submit(&r.to_string(), &env.cfg(0)).unwrap_err();
        assert_eq!(e.kind(), "malformed", "{field}: {e}");
    }
    assert!(
        !env.journal_text().contains("op-spec-"),
        "a speculative request was journalled"
    );
    assert_eq!(spawn_count(&env.spawns), 0);
}
