//! R6 L3 (`tasks/r6-ledger-spec.md` §2 C1, §5): `axon-intent audit-runs`,
//! driven as an operator would (subprocess). The ledger, the spent store and
//! the envelopes are made with the axon-os library under a TEST operator key
//! (derived from a fixed, public string; never a real secret).

use axon_os::repair_envelope::{self as re, Budget, MintKey, RepairEnvelope};
use axon_os::run_ledger::{self, Accepted, Body, SealKey, Started, Terminal, WriteEntry};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const OP_KEY: &[u8] = b"nhr r6 l3 test operator key, not a secret";

fn op_key_hex() -> String {
    OP_KEY.iter().map(|b| format!("{b:02x}")).collect()
}

fn me() -> u32 {
    // SAFETY: geteuid cannot fail.
    unsafe { libc::geteuid() }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn private_dir(p: &Path) {
    std::fs::create_dir_all(p).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).unwrap();
}

struct Out {
    stdout: String,
    stderr: String,
    code: i32,
}

fn audit_runs(args: &[&str], key: Option<String>) -> Out {
    let mut c = Command::new(env!("CARGO_BIN_EXE_axon-intent"));
    c.arg("audit-runs").args(args);
    c.env_remove("AXON_APPROVAL_KEY");
    if let Some(k) = key {
        c.env("AXON_APPROVAL_KEY", k);
    }
    let o = c.output().unwrap();
    Out {
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        code: o.status.code().unwrap_or(-1),
    }
}

struct Fx {
    dir: PathBuf,
    spent: PathBuf,
    ledger: PathBuf,
    envelopes: PathBuf,
    store_id: String,
    ledger_id: String,
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fx(name: &str) -> Fx {
    let dir = std::env::temp_dir().join(format!("axon-intent-c1-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    private_dir(&dir);
    let (spent, ledger, envelopes) = (dir.join("spent"), dir.join("runs"), dir.join("env"));
    private_dir(&envelopes);
    private_dir(&spent);
    private_dir(&ledger);
    let store_id = re::init_store(&spent, me()).unwrap();
    let ledger_id = run_ledger::init_ledger(&ledger, me()).unwrap();
    Fx {
        dir,
        spent,
        ledger,
        envelopes,
        store_id,
        ledger_id,
    }
}

fn budget() -> Budget {
    serde_json::from_value(json!({"max_proposals": 4, "deadline_ms": 60_000,
        "check_timeout_ms": 10_000, "candidates": 3, "max_steps": 8}))
    .unwrap()
}

impl Fx {
    fn args(&self) -> Vec<String> {
        vec![
            "--spent-store".into(),
            self.spent.display().to_string(),
            "--run-ledger".into(),
            self.ledger.display().to_string(),
            "--envelopes".into(),
            self.envelopes.display().to_string(),
        ]
    }

    fn audit(&self) -> Out {
        let a = self.args();
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        audit_runs(&a, Some(op_key_hex()))
    }

    /// Mint, sign, write, issue and consume one envelope (as the operator and
    /// phase B would), returning the verified envelope.
    fn envelope(&self, symbol: &str) -> re::VerifiedEnvelope {
        let id = re::new_id().unwrap();
        let t = now();
        let env: RepairEnvelope = serde_json::from_value(json!({
            "schema": re::ENVELOPE_SCHEMA,
            "envelope_id": id,
            "spent_store_id": self.store_id,
            "run_ledger_id": self.ledger_id,
            "issued_by": "operator",
            "issued_unix": t - 10,
            "expires_unix": t + 3_600,
            "task": {"workspace": "/srv/ws", "file": "src/main.axon", "symbol": symbol,
                     "pristine_sha256": "a".repeat(64),
                     "imports_digest": format!("axsha256:{}", "b".repeat(64))},
            "write_prefixes": ["src"],
            "scopes": {"fs_read": [], "fs_write": [], "net": []},
            "capabilities": {"fence_sha256": "c".repeat(64),
                             "target_grant": {"symbol": symbol}},
            "check": {"name": "test_parse", "digest": "d".repeat(64)},
            "budget": {"max_proposals": 4, "deadline_ms": 60_000, "check_timeout_ms": 10_000,
                       "candidates": 3, "max_steps": 8},
            "generator": {"kind": "ai", "model": "claude-x", "provider": "anthropic",
                          "endpoint_host": "api.anthropic.com"},
            "decision": re::DECISION,
            "sig": "",
        }))
        .unwrap();
        let mk = MintKey::derive(OP_KEY).unwrap();
        let text = mk.sign(&env).unwrap();
        std::fs::write(self.envelopes.join(format!("{id}.envelope")), &text).unwrap();
        let v = re::verify(&text, &mk.verify_key(), t).unwrap();
        re::issue(&self.spent, me(), &self.store_id, &id, t).unwrap();
        re::consume(&self.spent, me(), &v, t).unwrap();
        v
    }

    /// A complete run: started, then terminal, sealed under `key`.
    fn run(&self, v: &re::VerifiedEnvelope, key: &SealKey) {
        let before = [("src/main.axon".to_string(), "a".repeat(64))].into();
        let after = [("src/main.axon".to_string(), "e".repeat(64))].into();
        run_ledger::append(
            &self.ledger,
            me(),
            key,
            v,
            Body::Started(Started {
                enforcement: json!([{"id": "E1", "status": "admit", "basis": "pinned"}]),
                pinned_tree_before: before,
                budget: budget(),
            }),
            now(),
        )
        .unwrap();
        run_ledger::append(
            &self.ledger,
            me(),
            key,
            v,
            Body::Terminal(Terminal {
                outcome: "repaired".into(),
                exit_code: 0,
                stopped_by: None,
                accepted: Some(Accepted {
                    path: "src/main.axon".into(),
                    sha256: "e".repeat(64),
                }),
                accepted_refusal: None,
                writes: vec![WriteEntry {
                    site: "apply".into(),
                    path: "src/main.axon".into(),
                    before_sha256: Some("a".repeat(64)),
                    after_sha256: Some("e".repeat(64)),
                }],
                pinned_tree_after: after,
                episode_digest: None,
                budget: budget(),
            }),
            now(),
        )
        .unwrap();
    }

    fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
        let mut v = Vec::new();
        for d in [&self.ledger, &self.spent, &self.envelopes] {
            let mut names: Vec<PathBuf> = std::fs::read_dir(d)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            names.sort();
            for p in names {
                v.push((p.display().to_string(), std::fs::read(&p).unwrap()));
            }
        }
        v
    }
}

fn key() -> SealKey {
    SealKey::derive(OP_KEY).unwrap()
}

#[test]
fn a_clean_ledger_audits_clean_and_the_audit_writes_nothing() {
    let f = fx("clean");
    let v = f.envelope("parse");
    f.run(&v, &key());
    let before = f.snapshot();
    let o = f.audit();
    assert_eq!(o.code, 0, "stdout {} stderr {}", o.stdout, o.stderr);
    assert!(o.stdout.contains("clean: no findings"), "{}", o.stdout);
    assert!(o.stdout.contains(&v.envelope.envelope_id), "{}", o.stdout);
    assert!(o.stdout.contains("outcome repaired"), "{}", o.stdout);
    assert_eq!(
        f.snapshot(),
        before,
        "ATTACK: the read-only C1 audit changed the ledger, store or envelopes"
    );
}

#[test]
fn an_empty_ledger_and_store_audit_clean() {
    let f = fx("empty");
    let o = f.audit();
    assert_eq!(o.code, 0, "{} {}", o.stdout, o.stderr);
}

#[test]
fn a_consumed_envelope_with_no_record_is_a_finding() {
    let f = fx("truncated");
    let v = f.envelope("parse");
    f.run(&v, &key());
    let lost = f.envelope("other");
    let o = f.audit();
    assert_eq!(
        o.code, 1,
        "ATTACK: a consumed envelope with no run record audited clean: {}",
        o.stdout
    );
    assert!(
        o.stdout.contains("FINDING consumed-no-record")
            && o.stdout.contains(&lost.envelope.envelope_id),
        "{}",
        o.stdout
    );
}

#[test]
fn records_sealed_under_another_key_are_a_seal_break() {
    let f = fx("wrongkey");
    let v = f.envelope("parse");
    let other = SealKey::derive(b"a different operator key, also public").unwrap();
    f.run(&v, &other);
    let o = f.audit();
    assert_eq!(
        o.code, 1,
        "ATTACK: records sealed under another key audited clean: {}",
        o.stdout
    );
    assert!(o.stdout.contains("FINDING seal-break"), "{}", o.stdout);
}

#[test]
fn a_supplied_envelope_the_records_do_not_name_is_a_finding() {
    let f = fx("mismatch");
    let v = f.envelope("parse");
    f.run(&v, &key());
    // Replace the supplied envelope with another signed one under the same id.
    let p = f
        .envelopes
        .join(format!("{}.envelope", v.envelope.envelope_id));
    let mut env = v.envelope.clone();
    env.issued_by = "operator-b".into();
    let text = MintKey::derive(OP_KEY).unwrap().sign(&env).unwrap();
    std::fs::write(&p, text).unwrap();
    let o = f.audit();
    assert_eq!(
        o.code, 1,
        "ATTACK: records naming a different envelope digest audited clean: {}",
        o.stdout
    );
    assert!(
        o.stdout.contains("FINDING envelope-mismatch"),
        "{}",
        o.stdout
    );
}

#[test]
fn json_output_carries_runs_and_findings() {
    let f = fx("json");
    let v = f.envelope("parse");
    f.run(&v, &key());
    let mut a = f.args();
    a.push("--json".into());
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let o = audit_runs(&a, Some(op_key_hex()));
    assert_eq!(o.code, 0, "{}", o.stderr);
    let j: serde_json::Value = serde_json::from_str(o.stdout.trim()).unwrap();
    assert_eq!(j["schema"], "axon-intent-audit-runs/1");
    assert_eq!(j["runs"][0]["envelope_id"], json!(v.envelope.envelope_id));
    assert_eq!(j["runs"][0]["accepted"]["sha256"], json!("e".repeat(64)));
    assert_eq!(j["findings"], json!([]));
}

#[test]
fn usage_and_unreadable_inputs_exit_2() {
    let f = fx("usage");
    // No key: the seal cannot be checked, so there is no audit.
    let a = f.args();
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let o = audit_runs(&a, None);
    assert_eq!(
        o.code, 2,
        "ATTACK: an audit ran without the operator key: {}",
        o.stdout
    );
    assert!(o.stdout.is_empty(), "{}", o.stdout);
    // A missing directory.
    let o = audit_runs(
        &["--spent-store", "/nonexistent-c1", "--run-ledger", "/x"],
        Some(op_key_hex()),
    );
    assert_eq!(o.code, 2, "{}", o.stderr);
    // An unsigned file among the envelopes is refused, not skipped.
    std::fs::write(
        f.envelopes.join("junk.envelope"),
        "{\"schema\":\"axon-approval/4\"}",
    )
    .unwrap();
    let o = f.audit();
    assert_eq!(
        o.code, 2,
        "ATTACK: an unverifiable file in --envelopes was skipped: {}",
        o.stdout
    );
    // A ledger group/other can reach is refused.
    std::fs::remove_file(f.envelopes.join("junk.envelope")).unwrap();
    std::fs::set_permissions(&f.ledger, std::fs::Permissions::from_mode(0o770)).unwrap();
    let o = f.audit();
    assert_eq!(
        o.code, 2,
        "ATTACK: a ledger others can reach was audited: {}",
        o.stdout
    );
    let o = audit_runs(&["--help"], None);
    assert_eq!(o.code, 0);
    assert!(o.stdout.contains("audit-runs"), "{}", o.stdout);
}
