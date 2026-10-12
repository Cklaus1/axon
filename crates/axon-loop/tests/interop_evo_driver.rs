//! The interop gate's EVO REQUEST producer (section 13, G01-r22-unknown-outcome).
//! A plan's candidate must be a bounded EVO mutation of its incumbent, and EVO
//! learns only from a learning-eligible discovery episode (completed,
//! independently verified, FINAL usage). A real MiCode episode reports
//! estimated usage and a mechanism-test role, so it is never one: this driver
//! re-scopes the fixture discovery episode to the gate's incumbent and writes
//! the request. The gate then runs the REAL `axon-loop evo propose` on it.
//! Ignored by default; the gate runs it with its inputs in the environment:
//!
//! ```text
//! EVO_INCUMBENT=active-policy.json EVO_ELIGIBLE=bash,edit,grep,read,...
//! EVO_VERIFIER=gate:independent-verifier EVO_PROPOSER=agent:gate-proposer
//! EVO_POLICY_ID=pol-evo-1 EVO_OUT=evo-request.json
//! cargo test -p axon-loop --test interop_evo_driver -- --ignored --exact produce_request
//! ```

mod common;
use axon_loop_contracts::{digest, parse, PolicyEnvelope};
use common::*;
use serde_json::json;

fn var(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} must be set"))
}

#[test]
#[ignore = "driven by scripts/loop_interop_gate.sh"]
fn produce_request() {
    let inc: PolicyEnvelope =
        parse(&std::fs::read_to_string(var("EVO_INCUMBENT")).unwrap()).unwrap();
    let mut ep = discovery_episode(&inc, "gate-discovery-1");
    ep["scope"] = json!(inc.scope);
    ep["candidate_set_ref"] = json!(inc.candidate_set_ref);
    ep["policy_ref"] = json!(digest(&inc).unwrap());
    ep["verification"]["issuer_ref"] = json!(var("EVO_VERIFIER"));
    let mut eligible: Vec<String> = var("EVO_ELIGIBLE").split(',').map(str::to_string).collect();
    eligible.sort();
    let req = json!({
        "schema": "axon.loop.evo-request/1",
        "incumbent": inc,
        "eligible": eligible,
        "episodes": [ep],
        "seed": 1,
        "new_policy_id": var("EVO_POLICY_ID"),
        "proposer_ref": var("EVO_PROPOSER"),
        "rationale": "interop gate: a one-edit shortlist mutation to evaluate"
    });
    std::fs::write(var("EVO_OUT"), req.to_string()).unwrap();
}
