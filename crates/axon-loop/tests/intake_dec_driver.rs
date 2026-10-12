//! The interop gate's POLICY PRODUCER: the real DEC path (`decide_builtin` →
//! `to_policy_envelope`), not a hand-written envelope. Ignored by default; the
//! gate runs it with its inputs in the environment:
//!
//! ```text
//! INTAKE_DEC_OUT=policy.json INTAKE_DEC_CANDIDATES=bash,edit,grep,read,...
//! INTAKE_DEC_TENANT=t INTAKE_DEC_FAMILY=f INTAKE_DEC_POLICY_ID=pol-x
//! INTAKE_DEC_PROVIDER=deterministic-frequency INTAKE_DEC_LIMIT=2
//! INTAKE_DEC_USAGE=read:5,grep:3
//! cargo test -p axon-loop --test intake_dec_driver -- --ignored --exact produce_policy
//! ```
//!
//! `candidate_set_ref` is `cl22:` over the SORTED candidate names — the rule
//! MiCode's `PolicyScope::candidate_set_ref` uses, recomputed here rather than
//! copied from MiCode's acknowledgement, so a divergence between the two
//! digest implementations fails the gate.

use axon_loop_contracts::{canonical_json, digest_value, CandidateId, PolicyId, Scope};
use axon_reflex::shortlist::{
    decide_builtin, to_policy_envelope, DecisionRequest, DiscoveryEvidence, ProviderId,
    TaskFeatures,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn var(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} must be set"))
}

#[test]
#[ignore = "driven by scripts/loop_interop_gate.sh"]
fn produce_policy() {
    let names: Vec<String> = var("INTAKE_DEC_CANDIDATES")
        .split(',')
        .map(str::to_string)
        .collect();
    let eligible: BTreeSet<CandidateId> = names
        .iter()
        .map(|n| CandidateId::new(n.as_str()).expect("candidate id"))
        .collect();
    let sorted: Vec<&str> = eligible.iter().map(CandidateId::as_str).collect();
    let candidate_set_ref = digest_value(&json!(sorted)).expect("digest");
    let scope: Scope = serde_json::from_value(json!({
        "tenant_id": var("INTAKE_DEC_TENANT"), "task_family": var("INTAKE_DEC_FAMILY")
    }))
    .expect("scope");
    // Fixed pilot controls: the model route and budget this trial runs under.
    let controls_ref =
        digest_value(&json!({"controls": "loop-interop-gate", "model": var("INTAKE_DEC_MODEL")}))
            .expect("digest");
    let mut usage = BTreeMap::new();
    for kv in var("INTAKE_DEC_USAGE").split(',').filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once(':').expect("name:count");
        usage.insert(CandidateId::new(k).expect("id"), v.parse().expect("count"));
    }
    let evidence = vec![DiscoveryEvidence {
        evidence_ref: digest_value(&json!({"interop-gate-usage": var("INTAKE_DEC_USAGE")}))
            .expect("digest"),
        usage_counts: usage,
    }];
    let request = DecisionRequest::new(
        scope,
        eligible,
        candidate_set_ref,
        controls_ref,
        TaskFeatures::new(BTreeMap::new()).expect("features"),
        ProviderId::new(var("INTAKE_DEC_PROVIDER")).expect("provider"),
    )
    .expect("request")
    .with_limit(var("INTAKE_DEC_LIMIT").parse().expect("limit"))
    .expect("limit");
    let decision = decide_builtin(&request, &evidence);
    let env = to_policy_envelope(
        &decision,
        &request,
        PolicyId::new(var("INTAKE_DEC_POLICY_ID")).expect("policy id"),
        axon_loop::null_policy_ref(),
    )
    .expect("DEC issued a shortlist");
    std::fs::write(
        var("INTAKE_DEC_OUT"),
        canonical_json(&env).expect("canonical"),
    )
    .expect("write");
}
