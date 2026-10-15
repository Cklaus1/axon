//! The approval key on the Fabric route. A binary of its own: the supervisor
//! reads `AXON_APPROVAL_KEY` from the process environment, and the tests in
//! `grant_authority.rs` clear it to mint unkeyed tokens, so setting it there
//! would race them. Here it is set once, for the whole process.

mod common;
use common::*;

use axon_fabric::submit;
use axon_loop_contracts::ReceiptStatus;
use serde_json::json;

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(b))
}

/// The spec's `/1` rule, keyed half: with `AXON_APPROVAL_KEY` set a legacy
/// `axon-approval/1` token (unkeyed `axtok1:`, no import binding) is refused,
/// even for a program that imports nothing (the case a keyless verifier
/// admits: `grant_authority::a_keyless_legacy_token_is_admitted_for_an_import_free_program`).
#[test]
fn a_legacy_token_is_refused_under_a_key() {
    // A fixed test key (16 bytes, hex), not a credential.
    std::env::set_var(
        axon_os::APPROVAL_KEY_ENV,
        "000102030405060708090a0b0c0d0e0f",
    );
    let gated = format!("require_approval = true\n{GRANT_FS}");
    let env = Env::new();
    write_grant_registry(&env.grant_registry, &[("grant:test", PRINCIPAL, &gated)]);
    let cfg = env.cfg(0);
    let g = cfg.grants.resolve("grant:test", PRINCIPAL).unwrap();
    let src = std::fs::read_to_string(env.ws.join("f.ax")).unwrap();
    const U: char = '\u{1f}';
    let pd = format!("axsha256:{}", sha256_hex(src.as_bytes()));
    let gd = format!(
        "axsha256:{}",
        sha256_hex(axon_os::canonical_grant(g.grant()).as_bytes())
    );
    let td = format!(
        "axtok1:{}",
        sha256_hex(format!("{pd}{U}{gd}{U}alice{U}approved{U}low").as_bytes())
    );
    let tok = json!({"schema":"axon-approval/1","program_digest":pd,"grant_digest":gd,
                     "approved_by":"alice","decision":"approved","risk":"low","token_digest":td});
    std::fs::write(
        env.grant_registry
            .parent()
            .unwrap()
            .join("grant_test.approval"),
        tok.to_string(),
    )
    .unwrap();
    let s = submit(&request(&env, "op-keyed", "t_ok").to_string(), &cfg).unwrap();
    assert!(
        s.receipt.status == ReceiptStatus::Denied && spawn_count(&env.spawns) == 0,
        "ATTACK: a legacy /1 token was admitted under a key: {s:?}"
    );
    assert!(
        s.reason
            .as_deref()
            .unwrap_or("")
            .contains("predates import binding"),
        "the refusal says re-approve: {s:?}"
    );
    assert_eq!(env.launch_records(), 0, "no launch record");
}
