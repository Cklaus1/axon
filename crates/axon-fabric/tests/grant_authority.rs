//! D-016 (D-C2 / R5): the grant a request is admitted under is the REQUEST'S
//! grant, resolved from the operator's registry, and it is the grant the
//! executed check is bounded by. Every refusal asserts absence of effect:
//! no spawn, no launch record, and — for refusals before the journal — no
//! journal file at all.

mod common;
use common::*;

use axon_fabric::{submit, SubmitError};
use axon_loop_contracts::{ReceiptStatus, ReceiptVerification};
use serde_json::{json, Value};

/// A program whose one test needs `IO` at RUN time (`println`) but declares
/// nothing a static scan sees — so admission passes and only the EXECUTED
/// ceiling decides.
const PRINTS: &str = "\
@[test]
fn t_print() { println(\"hello\") }
";

fn with_grants(env: &Env, grants: &[(&str, &str, &str)]) -> axon_fabric::SubmitConfig {
    write_grant_registry(&env.grant_registry, grants);
    env.cfg(0)
}

fn journal_config(env: &Env, op: &str) -> Value {
    env.journal_text()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["kind"] == "intent" && v["intent"]["op"] == op)
        .map(|v| v["intent"]["config"].clone())
        .unwrap_or(Value::Null)
}

fn assert_untouched(env: &Env, what: &str) {
    assert_eq!(spawn_count(&env.spawns), 0, "{what}: nothing spawned");
    assert_eq!(env.launch_records(), 0, "{what}: no launch record");
    assert!(
        !env.journal.exists(),
        "{what}: refused before the journal was even opened"
    );
}

#[test]
fn an_unknown_or_unbound_or_edited_grant_is_refused_with_zero_effects() {
    let env = Env::new();

    let mut r = request(&env, "op-unknown", "t_ok");
    r["grant_ref"] = json!("grant:nobody-registered-this");
    let e = submit(&r.to_string(), &env.cfg(0)).unwrap_err();
    assert!(matches!(e, SubmitError::Unauthorized(_)), "{e}");
    assert!(e.to_string().contains("not in the grant registry"), "{e}");
    assert_untouched(&env, "unknown grant_ref");

    // The ref exists but is bound to a different principal.
    let mut r = request(&env, "op-principal", "t_ok");
    r["principal_ref"] = json!("principal:someone-else");
    let e = match submit(&r.to_string(), &env.cfg(0)) {
        Ok(s) => panic!(
            "ATTACK: a grant bound to another principal authorized this one: {:?}",
            s.receipt.status
        ),
        Err(e) => e,
    };
    assert!(matches!(e, SubmitError::Unauthorized(_)), "{e}");
    assert!(e.to_string().contains("bound to principal"), "{e}");
    assert_untouched(&env, "principal mismatch");

    // The grant file is widened after the registry pinned it.
    let cfg = env.cfg(0);
    let gfile = env
        .grant_registry
        .parent()
        .unwrap()
        .join("grant_test.axgrant");
    let widened = std::fs::read_to_string(&gfile)
        .unwrap()
        .replace("profile = \"restricted\"", "profile = \"developer\"");
    std::fs::write(&gfile, widened).unwrap();
    let e = match submit(&request(&env, "op-edited", "t_ok").to_string(), &cfg) {
        Ok(s) => panic!(
            "ATTACK: a grant file widened after the registry pinned it authorized a run: {:?}",
            s.receipt.status
        ),
        Err(e) => e,
    };
    assert!(matches!(e, SubmitError::Unauthorized(_)), "{e}");
    assert!(e.to_string().contains("the registry pins"), "{e}");
    assert_untouched(&env, "edited grant file");
}

#[test]
fn a_misspelled_profile_in_a_grant_is_refused_never_defaulted() {
    let env = Env::new();
    let cfg = with_grants(
        &env,
        &[(
            "grant:test",
            PRINCIPAL,
            "profile = \"restrcted\"\n[grant]\nmax_label = \"internal\"\n",
        )],
    );
    let e = submit(&request(&env, "op-typo", "t_ok").to_string(), &cfg).unwrap_err();
    assert!(matches!(e, SubmitError::Unauthorized(_)), "{e}");
    assert!(e.to_string().contains("unknown profile"), "{e}");
    assert_untouched(&env, "misspelled profile");
}

#[test]
fn the_all_zero_policy_digest_placeholder_is_refused_with_zero_effects() {
    let env = Env::new();
    let mut r = request(&env, "op-zero-policy", "t_ok");
    r["policy_digest"] = json!(format!("acf1:{}", "0".repeat(64)));
    let e = match submit(&r.to_string(), &env.cfg(0)) {
        Ok(s) => panic!(
            "ATTACK: a request governed by the all-zero placeholder policy was accepted: {:?}",
            s.receipt.status
        ),
        Err(e) => e,
    };
    assert!(matches!(e, SubmitError::Malformed(_)), "{e}");
    assert!(e.to_string().contains("placeholder"), "{e}");
    assert_untouched(&env, "zero policy digest");
}

/// The executed check runs under the ceiling the ADMITTED grant induces.
/// Before D-016 the ceiling was an unrelated operator flag (the tests set
/// `IO` whatever the grant), so a net-only grant still let the check print.
#[test]
fn the_executed_effect_ceiling_is_derived_from_the_admitted_grant() {
    let env = Env::new();
    std::fs::write(env.ws.join("f.ax"), PRINTS).unwrap();
    let net_only = "profile = \"restricted\"\n[grant]\nnet = [\"*\"]\nmax_label = \"internal\"\n\
                    [grant.budget]\ncost_micro = 1000\n";
    let cfg = with_grants(
        &env,
        &[
            ("grant:test", PRINCIPAL, GRANT_FS),
            ("grant:net", PRINCIPAL, net_only),
        ],
    );

    // fs grant ⇒ AXON_ALLOWED_EFFECTS=IO ⇒ println is allowed.
    let ok = submit(&request(&env, "op-io", "t_print").to_string(), &cfg).unwrap();
    assert_eq!(
        ok.receipt.verification,
        ReceiptVerification::Passed,
        "{ok:?}"
    );
    assert_eq!(
        journal_config(&env, "op-io")["grant"]["effect_ceiling"],
        "IO"
    );

    // net-only grant ⇒ AXON_ALLOWED_EFFECTS=Net,AI ⇒ println is refused at
    // run time by the interpreter.
    let mut r = request(&env, "op-net", "t_print");
    r["grant_ref"] = json!("grant:net");
    let s = submit(&r.to_string(), &cfg).unwrap();
    assert_eq!(s.receipt.status, ReceiptStatus::Completed);
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Failed,
        "the grant withholds IO, so the check may not print: {:?}",
        s.check_report
    );
    let cfgj = journal_config(&env, "op-net");
    assert_eq!(cfgj["grant"]["grant_ref"], "grant:net");
    assert_eq!(cfgj["grant"]["effect_ceiling"], "Net,AI");
}

/// A grant that withholds an effect the program statically declares is
/// denied by axon-os admission: a receipt, but no launch and no spawn.
#[test]
fn admission_uses_the_request_grant_and_denies_before_launch() {
    let env = Env::new();
    std::fs::write(
        env.ws.join("f.ax"),
        "@[test]\nfn t_w() { let _ = write_file(\"out.txt\", \"x\") }\n",
    )
    .unwrap();
    let net_only = "profile = \"restricted\"\n[grant]\nnet = [\"*\"]\nmax_label = \"internal\"\n\
                    [grant.budget]\ncost_micro = 1000\n";
    let cfg = with_grants(&env, &[("grant:test", PRINCIPAL, net_only)]);
    let s = submit(&request(&env, "op-deny", "t_w").to_string(), &cfg).unwrap();
    assert!(
        s.receipt.status == ReceiptStatus::Denied && spawn_count(&env.spawns) == 0,
        "ATTACK: axon-os admission refused a program writing files under a net-only grant, \
         and it ran anyway: {:?}",
        s.receipt.status
    );
    assert!(
        s.reason.as_deref().unwrap_or("").contains("fs_write"),
        "{s:?}"
    );
    assert_eq!(spawn_count(&env.spawns), 0, "nothing spawned");
    assert_eq!(env.launch_records(), 0, "no launch record");
    assert!(!env.ws.join("out.txt").exists());

    // The request may not spend more than its grant's budget. rows4b: the
    // grant's budget (500) is below the scope's (1000), so the request (600)
    // fits the scope and only the grant's cap refuses it.
    let env = Env::new();
    let half = GRANT_FS.replace("cost_micro = 1000", "cost_micro = 500");
    let cfg = with_grants(&env, &[("grant:test", PRINCIPAL, &half)]);
    let mut r = request(&env, "op-over", "t_ok");
    r["limits"]["max_cost_micro"] = json!(600);
    let s = submit(&r.to_string(), &cfg).unwrap();
    assert!(
        s.receipt.status == ReceiptStatus::Denied && spawn_count(&env.spawns) == 0,
        "ATTACK: a request allowed to spend more than its grant's budget ran: {:?}",
        s.receipt.status
    );
    assert!(s.reason.unwrap().contains("budget.cost_micro"));
    assert_eq!(spawn_count(&env.spawns), 0);
    assert_eq!(env.launch_records(), 0);
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(b))
}

/// An `axon-approval/1` token as axon-intent emits it.
fn token(program_src: &str, g: &axon_os::Grant) -> String {
    const U: char = '\u{1f}';
    let pd = format!("axsha256:{}", sha256_hex(program_src.as_bytes()));
    let gd = format!(
        "axsha256:{}",
        sha256_hex(axon_os::canonical_grant(g).as_bytes())
    );
    let td = format!(
        "axtok1:{}",
        sha256_hex(format!("{pd}{U}{gd}{U}alice{U}approved{U}low").as_bytes())
    );
    json!({"schema":"axon-approval/1","program_digest":pd,"grant_digest":gd,
           "approved_by":"alice","decision":"approved","risk":"low","token_digest":td})
    .to_string()
}

/// The grant's `require_approval` policy is honoured, with the axon-os table:
/// required+absent ⇒ denied; required+valid ⇒ runs; not required+INVALID ⇒
/// denied. Before D-016 admission hard-coded `require_approval: false`.
#[test]
fn the_grants_require_approval_policy_is_enforced() {
    let env = Env::new();
    let gated = format!("require_approval = true\n{GRANT_FS}");
    let cfg = with_grants(&env, &[("grant:test", PRINCIPAL, &gated)]);
    let gdir = env.grant_registry.parent().unwrap().to_path_buf();
    let tok_path = gdir.join("grant_test.approval");

    // required + absent
    let s = submit(&request(&env, "op-noapp", "t_ok").to_string(), &cfg).unwrap();
    assert_eq!(s.receipt.status, ReceiptStatus::Denied);
    assert!(
        s.reason
            .as_deref()
            .unwrap_or("")
            .contains("approval required"),
        "{s:?}"
    );
    assert_eq!(spawn_count(&env.spawns), 0, "nothing spawned");
    assert_eq!(env.launch_records(), 0, "no launch record");

    // required + valid (bound to the program bytes and the resolved grant)
    let g = cfg.grants.resolve("grant:test", PRINCIPAL).unwrap();
    let src = std::fs::read_to_string(env.ws.join("f.ax")).unwrap();
    std::fs::write(&tok_path, token(&src, g.grant())).unwrap();
    let s = submit(&request(&env, "op-app", "t_ok").to_string(), &cfg).unwrap();
    assert_eq!(s.receipt.verification, ReceiptVerification::Passed, "{s:?}");
    assert_eq!(
        journal_config(&env, "op-app")["grant"]["approval"],
        "verified_required"
    );

    // not required + INVALID token (signed over other program bytes)
    let env = Env::new();
    let cfg = env.cfg(0);
    let g = cfg.grants.resolve("grant:test", PRINCIPAL).unwrap();
    std::fs::write(
        env.grant_registry
            .parent()
            .unwrap()
            .join("grant_test.approval"),
        token("fn main() { 1 }", g.grant()),
    )
    .unwrap();
    let s = submit(&request(&env, "op-badtok", "t_ok").to_string(), &cfg).unwrap();
    assert_eq!(s.receipt.status, ReceiptStatus::Denied);
    assert!(s.reason.unwrap().contains("program was edited"));
    assert_eq!(spawn_count(&env.spawns), 0);
    assert_eq!(env.launch_records(), 0);
}

/// A reproducible (hermetic) grant cannot be honoured by any backend here, so
/// it is refused (journalled `unsupported`, never launched) rather than run
/// non-reproducibly. A path-scoped grant cannot be carried by the host
/// interpreter's coarse effect ceiling, so it is refused the same way.
#[test]
fn grants_no_backend_can_enforce_are_unsupported_not_weakened() {
    for (name, body) in [
        (
            "hermetic",
            "profile = \"hermetic\"\n[grant]\nmax_label = \"internal\"\n\
             [grant.budget]\ncost_micro = 1000\n",
        ),
        (
            "scoped",
            "profile = \"restricted\"\n[grant]\nfs_read = [\"./data/\"]\n\
             max_label = \"internal\"\n[grant.budget]\ncost_micro = 1000\n",
        ),
    ] {
        let env = Env::new();
        let cfg = with_grants(&env, &[("grant:test", PRINCIPAL, body)]);
        let s = submit(
            &request(&env, &format!("op-{name}"), "t_ok").to_string(),
            &cfg,
        )
        .unwrap();
        assert_eq!(
            s.receipt.status,
            ReceiptStatus::Unsupported,
            "{name}: {s:?}"
        );
        assert_eq!(spawn_count(&env.spawns), 0, "{name}: nothing spawned");
        assert_eq!(env.launch_records(), 0, "{name}: no launch record");
    }
}

/// An `axon-approval/1` token over (program, grant, decision), with its
/// `approved_by` replaced AFTER its own digest was taken when `edited`.
fn token_as(program_src: &str, g: &axon_os::Grant, decision: &str, edited: bool) -> String {
    const U: char = '\u{1f}';
    let pd = format!("axsha256:{}", sha256_hex(program_src.as_bytes()));
    let gd = format!(
        "axsha256:{}",
        sha256_hex(axon_os::canonical_grant(g).as_bytes())
    );
    let td = format!(
        "axtok1:{}",
        sha256_hex(format!("{pd}{U}{gd}{U}alice{U}{decision}{U}low").as_bytes())
    );
    let by = if edited { "mallory" } else { "alice" };
    json!({"schema":"axon-approval/1","program_digest":pd,"grant_digest":gd,
           "approved_by":by,"decision":decision,"risk":"low","token_digest":td})
    .to_string()
}

/// Amendment 75: each binding of an approval token, on the production route
/// (submit → supervisor_admits → axon-os `authorize`, the step every profile
/// takes before the profile split), each ALONE: under a grant that requires
/// sign-off, a token approving ANOTHER program, ANOTHER grant, a decision
/// other than `approved`, or metadata edited after its own digest admits
/// nothing. Control: the token over exactly this program and grant runs.
#[test]
fn an_approval_token_admits_only_what_it_approved() {
    let gated = format!("require_approval = true\n{GRANT_FS}");
    // (attack, token builder over the program source and the resolved grant)
    type Tok = fn(&str, &axon_os::Grant) -> String;
    let cases: [(&str, Tok); 5] = [
        ("control", |s, g| token_as(s, g, "approved", false)),
        ("for another program", |_, g| {
            token_as("fn main() { 1 }", g, "approved", false)
        }),
        ("for another grant", |s, g| {
            let mut other = g.clone();
            other.budget.cost_micro += 1;
            token_as(s, &other, "approved", false)
        }),
        ("whose decision is not `approved`", |s, g| {
            token_as(s, g, "rejected", false)
        }),
        ("whose metadata was edited after its digest", |s, g| {
            token_as(s, g, "approved", true)
        }),
    ];
    for (what, tok) in cases {
        let env = Env::new();
        let cfg = with_grants(&env, &[("grant:test", PRINCIPAL, &gated)]);
        let g = cfg.grants.resolve("grant:test", PRINCIPAL).unwrap();
        let src = std::fs::read_to_string(env.ws.join("f.ax")).unwrap();
        std::fs::write(
            env.grant_registry
                .parent()
                .unwrap()
                .join("grant_test.approval"),
            tok(&src, g.grant()),
        )
        .unwrap();
        let s = submit(&request(&env, "op-tok", "t_ok").to_string(), &cfg).unwrap();
        if what == "control" {
            assert_eq!(
                s.receipt.verification,
                ReceiptVerification::Passed,
                "control: the token over this program and grant: {s:?}"
            );
            continue;
        }
        assert!(
            s.receipt.status == ReceiptStatus::Denied && spawn_count(&env.spawns) == 0,
            "ATTACK: an approval token {what} admitted the job: {:?}",
            s.receipt.status
        );
        assert_eq!(env.launch_records(), 0, "{what}: no launch record");
    }
}
