//! C9 round 4c, ADMIT (amendment 76): the axon-os ADMISSION chain Fabric's
//! protected route runs (`submit` -> `supervisor_admits` -> `supervise_requiring`
//! -> `gate::admit`), attacked through `submit` itself. Each test is a program
//! or a grant with ONE defect and every other guard genuine; a refusal asserts
//! absence of effect (no spawn, no launch record). A panic that starts
//! `ATTACK:` is the attack getting through; the control is the same program
//! under a grant that allows it.

mod common;
use common::*;

use axon_fabric::submit;
use axon_loop_contracts::ReceiptStatus;

fn with_grant(env: &Env, body: &str) -> axon_fabric::SubmitConfig {
    write_grant_registry(&env.grant_registry, &[("grant:test", PRINCIPAL, body)]);
    env.cfg(0)
}

/// A grant that gives `axes` (and nothing else) at `max_label`.
fn grant(axes: &[&str], max_label: &str) -> String {
    let mut g = String::from("profile = \"restricted\"\n[grant]\n");
    for a in axes {
        g.push_str(&format!("{a} = [\"*\"]\n"));
    }
    g.push_str(&format!(
        "max_label = \"{max_label}\"\n[grant.budget]\ncost_micro = 1000\n"
    ));
    g
}

/// Submit `src` (the workspace's `f.ax`) under `grant_body`.
fn submit_under(env: &Env, op: &str, src: &str, grant_body: &str) -> axon_fabric::Submission {
    std::fs::write(env.ws.join("f.ax"), src).unwrap();
    let cfg = with_grant(env, grant_body);
    submit(&request(env, op, "t_x").to_string(), &cfg).unwrap()
}

/// The program must be DENIED by axon-os admission, naming `why`, with no
/// effect; ATTACK (`attack`) if it ran or was refused for another reason.
fn denied(env: &Env, s: &axon_fabric::Submission, why: &str, attack: &str) {
    if s.receipt.status != ReceiptStatus::Denied
        || spawn_count(&env.spawns) != 0
        || env.launch_records() != 0
    {
        panic!(
            "ATTACK: {attack}: {:?} ({:?}), {} spawns, {} launch records",
            s.receipt.status,
            s.reason,
            spawn_count(&env.spawns),
            env.launch_records()
        );
    }
    assert!(
        s.reason.as_deref().unwrap_or("").contains(why),
        "setup: denied for another reason (want {why:?}): {:?}",
        s.reason
    );
}

/// The control: the same program is not refused by axon-os admission.
fn admitted(s: &axon_fabric::Submission, what: &str) {
    assert!(
        !s.reason
            .as_deref()
            .unwrap_or("")
            .contains("axon-os supervisor refused"),
        "control ({what}): the grant allows it, admission must not refuse: {:?}",
        s.reason
    );
}

/// gate::admit: every effect axis the program declares must be in the grant,
/// axis by axis. One program per axis, under a grant that holds another axis
/// and withholds that one.
#[test]
fn a_program_using_an_axis_the_grant_withholds_is_denied_on_every_axis() {
    // (axis, a program using only it, a grant without it, a grant with it)
    let cases: [(&str, &str, &str, &str); 4] = [
        (
            "fs_read",
            "@[test]\nfn t_x() { let _ = read_file(\"in.txt\") }\n",
            "net",
            "fs_read",
        ),
        (
            "fs_write",
            "@[test]\nfn t_x() { let _ = write_file(\"out.txt\", \"x\") }\n",
            "net",
            "fs_write",
        ),
        (
            "net",
            "@[test]\nfn t_x() { let _ = http_get(\"http://example.invalid/\") }\n",
            "fs_read",
            "net",
        ),
        (
            "exec",
            "@[test]\nfn t_x() { let _ = exec(\"true\", []) }\n",
            "fs_read",
            "fs_read",
        ),
    ];
    for (axis, src, other, _own) in cases {
        let env = Env::new();
        let s = submit_under(
            &env,
            &format!("op-{axis}"),
            src,
            &grant(&[other], "internal"),
        );
        denied(
            &env,
            &s,
            &format!("program may perform {axis} but the grant withholds it"),
            &format!("a program using {axis} ran under a grant withholding {axis}"),
        );
    }
}

/// gate::admit: the declared confidentiality may not exceed the grant's
/// ceiling. The scan declares `internal`; a `public` grant is below it.
#[test]
fn a_program_above_the_grants_confidentiality_ceiling_is_denied() {
    let src = "@[test]\nfn t_x() { println(\"hello\") }\n";
    let env = Env::new();
    let s = submit_under(&env, "op-label-ok", src, &grant(&["fs_read"], "internal"));
    admitted(&s, "an internal ceiling holds internal data");
    let env = Env::new();
    let s = submit_under(&env, "op-label", src, &grant(&["fs_read"], "public"));
    denied(
        &env,
        &s,
        "above the grant ceiling",
        "a program handling internal data ran under a grant whose confidentiality ceiling is public",
    );
}

/// scan_effects / calls_name: the declared row is what a static scan sees, and
/// the scan must not be evaded by spacing or by an import it cannot read.
/// Every program writes a file under a grant that holds no fs_write.
#[test]
fn the_effects_scan_is_not_evaded_by_spacing_or_an_import() {
    let net_only = grant(&["net"], "internal");
    let cases: [(&str, &str, &str); 4] = [
        (
            "plain",
            "@[test]\nfn t_x() { let _ = write_file(\"out.txt\", \"x\") }\n",
            "a plain call write_file( was not seen by the effects scan",
        ),
        (
            "space",
            "@[test]\nfn t_x() { let _ = write_file (\"out.txt\", \"x\") }\n",
            "a spaced call write_file ( evaded the effects scan",
        ),
        (
            "newline",
            "@[test]\nfn t_x() { let _ = write_file\n(\"out.txt\", \"x\") }\n",
            "a call split over a newline write_file\\n( evaded the effects scan",
        ),
        (
            "import",
            "  mod helper\n@[test]\nfn t_x() { let _ = 1 }\n",
            "an indented `mod` import (effects in a file the scan cannot read) was declared effect-free",
        ),
    ];
    for (name, src, attack) in cases {
        let env = Env::new();
        let s = submit_under(&env, &format!("op-scan-{name}"), src, &net_only);
        denied(&env, &s, "the grant withholds it", attack);
        assert!(
            !env.ws.join("out.txt").exists(),
            "{name}: nothing was written"
        );
    }
}

/// The exemptions of `Grant::intersect`'s predicates (grant.rs `is_ancestor`,
/// `host_allows`, `host_matches`) rest on one fact: Fabric's admission
/// intersects the resolved grant WITH ITSELF (`manifest_for` clones the very
/// grant `supervise_requiring` is given as the supervisor's), so the effective
/// grant can never exceed the job's own. If a supervisor grant ever differs
/// from the manifest's, those predicates decide, and this test fails first.
#[test]
fn admission_intersects_the_resolved_grant_with_itself() {
    let env = Env::new();
    let cfg = env.cfg(0);
    let g = cfg.grants.resolve("grant:test", PRINCIPAL).unwrap();
    let m = g.manifest_for(std::path::Path::new("/p/f.ax"), "i".into());
    assert_eq!(
        m.grant,
        *g.grant(),
        "the manifest's grant is the supervisor's grant: the intersection is the grant with itself"
    );
}
