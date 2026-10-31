//! R6 L3 (`tasks/r6-ledger-spec.md` §2 C1, §5): `axon-intent audit-runs`,
//! the read-only accountability audit of unattended repair runs.
//!
//! It verifies the run ledger's chain, every seal and the head, checks each
//! record's `envelope_digest` against the supplied envelopes, and joins the
//! records against the spent store ([`axon_os::run_ledger::audit`]). It
//! writes nothing. Exit 0 clean, 1 findings, 2 usage or an unreadable input.
//!
//! The run-record key is derived from `AXON_APPROVAL_KEY` (the operator runs
//! this audit); supplied envelopes are verified under the verify key derived
//! from the same operator key.

use axon_os::repair_envelope::{self, MintKey};
use axon_os::run_ledger::{self, Audit, Kind, RunRecord, SealKey};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub const HELP: &str = "audit-runs — the C1 audit of unattended repair runs (R6 §5); read-only
    audit-runs --spent-store DIR --run-ledger DIR [--envelopes DIR] [--json]
  Verifies the run ledger's chain, seals and head, every record's envelope
  digest against the envelopes in --envelopes (every axon-approval/4 file there;
  axon-intent-mint/1 records are skipped), and joins the records with the spent
  store. Needs AXON_APPROVAL_KEY (hex). Exit 0 clean, 1 findings, 2 usage or
  an unreadable input.";

/// The mint record schema `authorize-repair` writes next to an envelope.
const MINT_RECORD_SCHEMA: &str = crate::authorize::MINT_RECORD_SCHEMA;

#[derive(Debug, Default)]
struct Args {
    spent: Option<PathBuf>,
    ledger: Option<PathBuf>,
    envelopes: Option<PathBuf>,
    json: bool,
}

fn parse_args(rest: &[&str]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut i = 0;
    while i < rest.len() {
        let flag = rest[i];
        if flag == "--json" {
            a.json = true;
            i += 1;
            continue;
        }
        let Some(&val) = rest.get(i + 1) else {
            return Err(format!("`{flag}` needs a value (see --help)"));
        };
        let slot = match flag {
            "--spent-store" => &mut a.spent,
            "--run-ledger" => &mut a.ledger,
            "--envelopes" => &mut a.envelopes,
            f => return Err(format!("unknown argument `{f}` (see --help)")),
        };
        if slot.replace(PathBuf::from(val)).is_some() {
            return Err(format!("{flag} is given twice"));
        }
        i += 2;
    }
    if a.spent.is_none() {
        return Err("--spent-store is required (see --help)".into());
    }
    if a.ledger.is_none() {
        return Err("--run-ledger is required (see --help)".into());
    }
    Ok(a)
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("axon-intent audit-runs: {msg}");
    ExitCode::from(2)
}

fn euid() -> u32 {
    // SAFETY: geteuid cannot fail.
    unsafe { libc::geteuid() }
}

/// The supplied envelopes, `envelope_id -> digest`: each verified under the
/// operator's verify key, at its own issue time (an audit after expiry still
/// checks the signature and the intrinsic rules).
pub fn load_envelopes(
    dir: &Path,
    vk: &repair_envelope::VerifyKey,
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("--envelopes {}: {e}", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("--envelopes {}: {e}", dir.display()))?;
    entries.sort();
    for p in entries {
        let m = std::fs::symlink_metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        if !m.is_file() {
            return Err(format!(
                "{} is not a regular file (a symlink is never followed)",
                p.display()
            ));
        }
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let text = text.trim_end_matches('\n');
        let v = axon_canonical::parse_value(text).map_err(|e| format!("{}: {e}", p.display()))?;
        let schema = v.get("schema").and_then(Value::as_str).unwrap_or("");
        if schema == MINT_RECORD_SCHEMA {
            continue;
        }
        if schema != repair_envelope::ENVELOPE_SCHEMA {
            return Err(format!(
                "{} is neither an {} envelope nor an {MINT_RECORD_SCHEMA} record",
                p.display(),
                repair_envelope::ENVELOPE_SCHEMA
            ));
        }
        let at = v.get("issued_unix").and_then(Value::as_u64).unwrap_or(0);
        let ve = repair_envelope::verify(text, vk, at)
            .map_err(|e| format!("{}: not an envelope this operator signed: {e}", p.display()))?;
        if let Some(old) = out.insert(ve.envelope.envelope_id.clone(), ve.digest.clone()) {
            if old != ve.digest {
                return Err(format!(
                    "--envelopes holds two different envelopes with id {}",
                    ve.envelope.envelope_id
                ));
            }
        }
    }
    Ok(out)
}

fn run_summary(id: &str, recs: &[RunRecord]) -> Value {
    let started = recs.iter().find(|r| r.kind == Kind::Started);
    let terminal = recs.iter().find(|r| r.kind == Kind::Terminal);
    json!({
        "envelope_id": id,
        "envelope_digest": recs.first().map(|r| r.envelope_digest.clone()),
        "task": recs.first().map(|r| json!({"file": r.task.file, "symbol": r.task.symbol})),
        "started_seq": started.map(|r| r.seq),
        "terminal_seq": terminal.map(|r| r.seq),
        "outcome": terminal.and_then(|r| r.outcome.clone()),
        "exit_code": terminal.and_then(|r| r.exit_code),
        "stopped_by": terminal.and_then(|r| r.stopped_by.clone()),
        "accepted": terminal.and_then(|r| r.accepted.as_ref())
            .map(|a| json!({"path": a.path, "sha256": a.sha256})),
        "accepted_refusal": terminal.and_then(|r| r.accepted_refusal.clone()),
        "writes": terminal.and_then(|r| r.writes.as_ref()).map(|w| w.len()),
    })
}

fn print(a: &Audit, ledger: &Path, spent: &Path, as_json: bool) {
    let runs: Vec<Value> = a.runs.iter().map(|(id, r)| run_summary(id, r)).collect();
    if as_json {
        println!(
            "{}",
            json!({
                "schema": "axon-intent-audit-runs/1",
                "runs": runs,
                "consumed": a.consumed,
                "findings": a.findings,
            })
        );
        return;
    }
    let records: usize = a.runs.values().map(Vec::len).sum();
    println!(
        "audit-runs: ledger {} ({records} verified record(s), {} run(s)); spent store {} ({} consumed)",
        ledger.display(),
        a.runs.len(),
        spent.display(),
        a.consumed.len()
    );
    for r in &runs {
        let opt = |k: &str| match &r[k] {
            Value::Null => "-".to_string(),
            Value::String(s) => s.clone(),
            v => v.to_string(),
        };
        println!(
            "  run {}: started {} terminal {} outcome {} exit {} accepted {} writes {}",
            opt("envelope_id"),
            opt("started_seq"),
            opt("terminal_seq"),
            opt("outcome"),
            opt("exit_code"),
            match &r["accepted"] {
                Value::Null => opt("accepted_refusal"),
                v => format!(
                    "{} {}",
                    v["path"].as_str().unwrap_or(""),
                    v["sha256"].as_str().unwrap_or("")
                ),
            },
            opt("writes")
        );
    }
    for f in &a.findings {
        println!(
            "  FINDING {}{}{}: {}",
            f.finding,
            f.seq.map(|s| format!(" seq {s}")).unwrap_or_default(),
            f.envelope_id
                .as_deref()
                .map(|e| format!(" envelope {e}"))
                .unwrap_or_default(),
            f.detail
        );
    }
    if a.findings.is_empty() {
        println!("  clean: no findings");
    } else {
        println!("  {} finding(s)", a.findings.len());
    }
}

pub fn run(rest: &[&str]) -> ExitCode {
    if rest == ["--help"] {
        println!("axon-intent {HELP}");
        return ExitCode::from(0);
    }
    let a = match parse_args(rest) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let (Some(spent), Some(ledger)) = (&a.spent, &a.ledger) else {
        return fail("--spent-store and --run-ledger are required");
    };
    let key = match SealKey::from_env() {
        Ok(k) => k,
        Err(e) => return fail(&e),
    };
    let envelopes = match &a.envelopes {
        None => BTreeMap::new(),
        Some(dir) => {
            let vk = match MintKey::from_env() {
                Ok(k) => k.verify_key(),
                Err(e) => return fail(&e),
            };
            match load_envelopes(dir, &vk) {
                Ok(m) => m,
                Err(e) => return fail(&e),
            }
        }
    };
    let audit = match run_ledger::audit(ledger, spent, euid(), &key, &envelopes) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    print(&audit, ledger, spent, a.json);
    ExitCode::from(if audit.findings.is_empty() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_runs_needs_both_directories() {
        assert!(parse_args(&["--spent-store", "s"]).is_err());
        assert!(parse_args(&["--run-ledger", "l"]).is_err());
        assert!(parse_args(&["--spent-store", "s", "--run-ledger", "l", "--bogus", "x"]).is_err());
        assert!(
            parse_args(&[
                "--spent-store",
                "s",
                "--spent-store",
                "t",
                "--run-ledger",
                "l"
            ])
            .is_err(),
            "ATTACK: a repeated --spent-store was accepted"
        );
        let a = parse_args(&["--run-ledger", "l", "--spent-store", "s", "--json"]).unwrap();
        assert!(a.json && a.envelopes.is_none());
    }
}
