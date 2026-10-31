//! NH-R1 A2 (`tasks/unattended-auth-spec.md` §5.1): the operator's side of
//! the repair envelope.
//!
//! * `authorize-repair --export-verify-key PATH`: the runner's public key,
//!   written once, owner-only. The operator key never leaves this process.
//! * `authorize-repair --init-spent-store DIR`: an empty private spent store
//!   and its `store-id`.
//! * `authorize-repair --init-run-ledger DIR`: an empty private R6 run ledger
//!   and its `ledger-id` (`tasks/r6-ledger-spec.md` §4).
//! * `authorize-repair --export-run-record-key PATH`: the runner's R6
//!   run-record subkey (it seals records; it derives nothing else), written
//!   once, owner-only.
//! * `authorize-repair … --by OP --ttl-s T --out PATH`: mint one envelope.
//!   1. exec `cortex grant --envelope-preview` (the bindings are computed by
//!      cortex, with the functions its runner verifies with);
//!   2. refuse unless every gate row is exactly `admit` (and there is one);
//!   3. record the sha256 of the cortex binary it invoked (`<out>.mint`);
//!   4. write `<store>/<envelope_id>.issued`, binding `--run-ledger`'s id;
//!   5. write the envelope with `create_new`.
//!
//! The preview is JUDGED, not trusted: strict JSON (duplicate keys, floats,
//! unknown fields refused), its check digest recomputed from its own input,
//! the symbol and check it answers compared with the ones asked for. Minting
//! authorizes nothing by itself: an unattended run still refuses by default
//! until the §9 enabling decision and the host's opt-in.

use axon_os::repair_envelope::{
    self, Budget, Capabilities, Check, Generator, MintKey, RepairEnvelope, Scopes, Task,
};
use axon_os::run_ledger;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The preview document `cortex grant --envelope-preview` prints.
pub const PREVIEW_SCHEMA: &str = "cortex-envelope-preview/1";
/// The check-digest argv shape (§4.1): the local interpreter executor only.
pub const ARGV_SHAPE: &str = "axon-test-local/1";
/// The minter's record next to the envelope (`<out>.mint`).
pub const MINT_RECORD_SCHEMA: &str = "axon-intent-mint/1";

/// The task half of the preview: what cortex resolved the operator's
/// `--workspace/--file/--symbol` to.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewTask {
    pub workspace: String,
    pub file: String,
    pub symbol: String,
    pub pristine_sha256: String,
}

/// One enforcement-gate row as cortex reports it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateRow {
    pub id: String,
    pub status: String,
    pub basis: String,
}

/// `cortex-envelope-preview/1`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub schema: String,
    pub task: PreviewTask,
    pub imports_digest: String,
    pub check_name: String,
    pub check_digest_input: Value,
    pub check_digest: String,
    pub write_prefixes: Vec<String>,
    pub scopes: Scopes,
    pub fence: Value,
    pub fence_sha256: String,
    pub target_grant: Value,
    pub generator: Generator,
    pub gate: Vec<GateRow>,
}

/// Why a mint was refused: `Gate` is a refusal on the merits (exit 8, like
/// a not-admissible job), `Bad` everything else (exit 2).
#[derive(Debug, Clone, PartialEq)]
pub enum Refused {
    Gate(String),
    Bad(String),
}

impl Refused {
    fn bad(s: impl Into<String>) -> Refused {
        Refused::Bad(s.into())
    }
}

fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Strict parse of the preview: the cl22 profile's rules, then the typed
/// shape with every field required and no unknown one.
pub fn parse_preview(text: &str) -> Result<Preview, String> {
    let v = axon_canonical::parse_value(text.trim_end_matches('\n'))
        .map_err(|e| format!("cortex preview: {e}"))?;
    let p: Preview = serde_json::from_value(v).map_err(|e| format!("cortex preview: {e}"))?;
    if p.schema != PREVIEW_SCHEMA {
        return Err(format!(
            "cortex preview: schema is {:?}, not {PREVIEW_SCHEMA}",
            p.schema
        ));
    }
    Ok(p)
}

/// The preview's check digest is the sha256 of its own input's cl22 bytes,
/// and that input names the preview's check, fence and import closure. A
/// preview that disagrees with itself binds nothing.
pub fn check_consistent(p: &Preview) -> Result<(), String> {
    let input = &p.check_digest_input;
    let bytes = axon_canonical::canonical_bytes(input)
        .map_err(|e| format!("cortex preview: check digest input: {e}"))?;
    if sha256_hex(&bytes) != p.check_digest {
        return Err("cortex preview: the check digest is not the sha256 of its input".into());
    }
    for (field, want) in [
        ("argv_shape", ARGV_SHAPE),
        ("check_name", p.check_name.as_str()),
        ("fence_sha256", p.fence_sha256.as_str()),
        ("imports_digest", p.imports_digest.as_str()),
    ] {
        if input.get(field).and_then(Value::as_str) != Some(want) {
            return Err(format!(
                "cortex preview: the check digest input's {field} is not the preview's ({want:?})"
            ));
        }
    }
    Ok(())
}

/// Step 2: every gate row is exactly `admit`, and there is at least one.
pub fn gate_admits(rows: &[GateRow]) -> Result<(), String> {
    if rows.is_empty() {
        return Err(
            "the preview has no gate rows: nothing was checked, so nothing is admitted".into(),
        );
    }
    let refused: Vec<String> = rows
        .iter()
        .filter(|r| r.status != "admit")
        .map(|r| format!("{} ({}: {})", r.id, r.status, r.basis))
        .collect();
    if refused.is_empty() {
        Ok(())
    } else {
        Err(format!("the gate refuses: {}", refused.join("; ")))
    }
}

/// What the operator asked for, beyond what the preview computes.
#[derive(Debug, Clone, PartialEq)]
pub struct MintRequest {
    pub symbol: String,
    pub check: String,
    pub issued_by: String,
    pub ttl_s: u64,
    pub budget: Budget,
}

/// Judge the preview against the request and build the unsigned envelope.
pub fn build_envelope(
    p: &Preview,
    req: &MintRequest,
    envelope_id: String,
    spent_store_id: String,
    run_ledger_id: String,
    now_unix: u64,
) -> Result<RepairEnvelope, Refused> {
    check_consistent(p).map_err(Refused::Bad)?;
    if p.task.symbol != req.symbol {
        return Err(Refused::Bad(format!(
            "cortex preview: answers symbol {:?}, not the requested {:?}",
            p.task.symbol, req.symbol
        )));
    }
    if p.check_name != req.check {
        return Err(Refused::Bad(format!(
            "cortex preview: answers check {:?}, not the requested {:?}",
            p.check_name, req.check
        )));
    }
    gate_admits(&p.gate).map_err(Refused::Gate)?;
    if req.ttl_s == 0 {
        return Err(Refused::bad("--ttl-s must be at least 1"));
    }
    let env = RepairEnvelope {
        schema: repair_envelope::ENVELOPE_SCHEMA.into(),
        envelope_id,
        spent_store_id,
        run_ledger_id,
        issued_by: req.issued_by.clone(),
        issued_unix: now_unix,
        expires_unix: now_unix.saturating_add(req.ttl_s),
        task: Task {
            workspace: p.task.workspace.clone(),
            file: p.task.file.clone(),
            symbol: p.task.symbol.clone(),
            pristine_sha256: p.task.pristine_sha256.clone(),
            imports_digest: p.imports_digest.clone(),
        },
        write_prefixes: p.write_prefixes.clone(),
        scopes: p.scopes.clone(),
        capabilities: Capabilities {
            fence_sha256: p.fence_sha256.clone(),
            target_grant: p.target_grant.clone(),
        },
        check: Check {
            name: p.check_name.clone(),
            digest: p.check_digest.clone(),
        },
        budget: req.budget.clone(),
        generator: p.generator.clone(),
        decision: repair_envelope::DECISION.into(),
        sig: String::new(),
    };
    repair_envelope::validate(&env).map_err(Refused::Bad)?;
    Ok(env)
}

// ── the CLI ──────────────────────────────────────────────────────────────────

pub const HELP: &str = "authorize-repair — mint one axon-approval/4 repair envelope (NH-R1 §5.1)
    authorize-repair --export-verify-key PATH
    authorize-repair --init-spent-store DIR
    authorize-repair --init-run-ledger DIR
    authorize-repair --export-run-record-key PATH
    authorize-repair --cortex BIN --workspace W --file F --symbol S --check C
        --write-prefix P... [--read-prefix R...] [--fs-write-scope X...] [--net-host H...]
        --axon A --generator ai:M --spent-store DIR --run-ledger DIR
        --verify-key K --run-record-key R --by OP --ttl-s T
        --max-proposals N --deadline-ms N --check-timeout-ms N --candidates N --max-steps N
        --out PATH
  --verify-key and --run-record-key name the runner's key files: no check pin
  may reach them, the spent store or the run ledger (security F6).
  Needs AXON_APPROVAL_KEY (hex) to export or mint. Exit 0 minted, 2 refused or
  malformed, 8 a gate row refuses. An envelope authorizes nothing by itself:
  `cortex repair --unattended` refuses by default (§9).";

fn euid() -> u32 {
    // SAFETY: geteuid cannot fail.
    unsafe { libc::geteuid() }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn fail(code: u8, msg: &str) -> ExitCode {
    eprintln!("axon-intent authorize-repair: {msg}");
    ExitCode::from(code)
}

/// Write `bytes` to a new owner-only file; an existing path is refused.
fn write_new(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(p)?;
    f.write_all(bytes)?;
    f.sync_all()
}

pub fn run(rest: &[&str]) -> ExitCode {
    match rest {
        ["--help"] => {
            println!("axon-intent {HELP}");
            ExitCode::from(0)
        }
        ["--export-verify-key", path] => export_verify_key(Path::new(path)),
        ["--init-spent-store", dir] => init_spent_store(Path::new(dir)),
        ["--init-run-ledger", dir] => init_run_ledger(Path::new(dir)),
        ["--export-run-record-key", path] => export_run_record_key(Path::new(path)),
        _ => match parse_args(rest) {
            Ok(a) => mint(&a),
            Err(e) => fail(2, &e),
        },
    }
}

fn export_verify_key(path: &Path) -> ExitCode {
    let key = match MintKey::from_env() {
        Ok(k) => k,
        Err(e) => return fail(2, &e),
    };
    let hex = key.verify_key().to_hex();
    if let Err(e) = write_new(path, format!("{hex}\n").as_bytes()) {
        return fail(2, &format!("cannot write {}: {e}", path.display()));
    }
    println!("verify key {hex}\n  Wrote {}", path.display());
    ExitCode::from(0)
}

fn export_run_record_key(path: &Path) -> ExitCode {
    let key = match run_ledger::SealKey::from_env() {
        Ok(k) => k,
        Err(e) => return fail(2, &e),
    };
    if let Err(e) = write_new(path, format!("{}\n", key.to_hex()).as_bytes()) {
        return fail(2, &format!("cannot write {}: {e}", path.display()));
    }
    println!("run-record key\n  Wrote {}", path.display());
    ExitCode::from(0)
}

/// Create `dir` owner-only if absent (an existing one is then checked by `init`).
fn init_private_dir(
    dir: &Path,
    init: fn(&Path, u32) -> Result<String, String>,
    what: &str,
) -> ExitCode {
    use std::os::unix::fs::DirBuilderExt;
    if let Err(e) = std::fs::DirBuilder::new().mode(0o700).create(dir) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            return fail(2, &format!("cannot create {}: {e}", dir.display()));
        }
    }
    match init(dir, euid()) {
        Ok(id) => {
            println!("{what} {} id {id}", dir.display());
            ExitCode::from(0)
        }
        Err(e) => fail(2, &e),
    }
}

fn init_spent_store(dir: &Path) -> ExitCode {
    init_private_dir(dir, repair_envelope::init_store, "spent store")
}

fn init_run_ledger(dir: &Path) -> ExitCode {
    init_private_dir(dir, run_ledger::init_ledger, "run ledger")
}

/// The parsed mint command line.
#[derive(Debug, Default)]
struct Args {
    cortex: Option<PathBuf>,
    /// `--workspace … --generator`, forwarded to the preview in order.
    preview: Vec<String>,
    symbol: Option<String>,
    check: Option<String>,
    spent_store: Option<PathBuf>,
    run_ledger: Option<PathBuf>,
    verify_key: Option<PathBuf>,
    run_record_key: Option<PathBuf>,
    by: Option<String>,
    ttl_s: Option<u64>,
    budget: [Option<u64>; 5],
    out: Option<PathBuf>,
}

const BUDGET_FLAGS: [&str; 5] = [
    "--max-proposals",
    "--deadline-ms",
    "--check-timeout-ms",
    "--candidates",
    "--max-steps",
];

fn parse_args(rest: &[&str]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut i = 0;
    while i < rest.len() {
        let flag = rest[i];
        let Some(&val) = rest.get(i + 1) else {
            return Err(format!("`{flag}` needs a value (see --help)"));
        };
        let num = || -> Result<u64, String> {
            val.parse::<u64>()
                .map_err(|_| format!("{flag} must be a whole number, got `{val}`"))
        };
        match flag {
            "--cortex" => a.cortex = Some(PathBuf::from(val)),
            "--workspace" | "--file" | "--write-prefix" | "--read-prefix" | "--fs-write-scope"
            | "--net-host" | "--axon" | "--generator" => {}
            "--symbol" => a.symbol = Some(val.to_string()),
            "--check" => a.check = Some(val.to_string()),
            "--spent-store" => a.spent_store = Some(PathBuf::from(val)),
            "--run-ledger" => a.run_ledger = Some(PathBuf::from(val)),
            "--verify-key" => a.verify_key = Some(PathBuf::from(val)),
            "--run-record-key" => a.run_record_key = Some(PathBuf::from(val)),
            "--by" => a.by = Some(val.to_string()),
            "--ttl-s" => a.ttl_s = Some(num()?),
            "--out" => a.out = Some(PathBuf::from(val)),
            f => match BUDGET_FLAGS.iter().position(|b| *b == f) {
                Some(k) => a.budget[k] = Some(num()?),
                None => return Err(format!("unknown argument `{f}` (see --help)")),
            },
        }
        if matches!(
            flag,
            "--workspace"
                | "--file"
                | "--symbol"
                | "--check"
                | "--write-prefix"
                | "--read-prefix"
                | "--fs-write-scope"
                | "--net-host"
                | "--axon"
                | "--generator"
        ) {
            a.preview.push(flag.to_string());
            a.preview.push(val.to_string());
        }
        i += 2;
    }
    for (name, missing) in [
        ("--cortex", a.cortex.is_none()),
        ("--symbol", a.symbol.is_none()),
        ("--check", a.check.is_none()),
        ("--spent-store", a.spent_store.is_none()),
        ("--run-ledger", a.run_ledger.is_none()),
        ("--verify-key", a.verify_key.is_none()),
        ("--run-record-key", a.run_record_key.is_none()),
        ("--by", a.by.is_none()),
        ("--ttl-s", a.ttl_s.is_none()),
        ("--out", a.out.is_none()),
    ] {
        if missing {
            return Err(format!("{name} is required (see --help)"));
        }
    }
    for (k, b) in BUDGET_FLAGS.iter().enumerate() {
        if a.budget[k].is_none() {
            return Err(format!("{b} is required (see --help)"));
        }
    }
    Ok(a)
}

fn mint(a: &Args) -> ExitCode {
    // Every check that needs no preview runs before the preview does.
    let key = match MintKey::from_env() {
        Ok(k) => k,
        Err(e) => return fail(2, &e),
    };
    let (Some(cortex), Some(store), Some(ledger), Some(vkey), Some(rkey), Some(out)) = (
        &a.cortex,
        &a.spent_store,
        &a.run_ledger,
        &a.verify_key,
        &a.run_record_key,
        &a.out,
    ) else {
        return fail(
            2,
            "missing --cortex, --spent-store, --run-ledger, --verify-key, --run-record-key or --out",
        );
    };
    let mint_path = PathBuf::from(format!("{}.mint", out.display()));
    for p in [out, &mint_path] {
        if std::fs::symlink_metadata(p).is_ok() {
            return fail(
                2,
                &format!("{} exists: an envelope is never overwritten", p.display()),
            );
        }
    }
    let me = euid();
    let store_id = match repair_envelope::store_id(store, me) {
        Ok(id) => id,
        Err(e) => return fail(2, &e),
    };
    let ledger_id = match run_ledger::ledger_id(ledger, me) {
        Ok(id) => id,
        Err(e) => return fail(2, &e),
    };
    // Security F6: what the check pins may never reach, canonical.
    let mut guarded: Vec<(&str, PathBuf, bool)> = Vec::new();
    for (what, flag, p, read_too) in [
        ("spent store", "--spent-store", store, true),
        ("run ledger", "--run-ledger", ledger, false),
        ("verify key", "--verify-key", vkey, true),
        ("run-record key", "--run-record-key", rkey, true),
    ] {
        match std::fs::canonicalize(p) {
            Ok(c) => guarded.push((what, c, read_too)),
            Err(e) => return fail(2, &format!("{flag} {}: {e}", p.display())),
        }
    }
    // Step 1: the preview, from the exact binary whose bytes are recorded.
    let cortex = match std::fs::canonicalize(cortex) {
        Ok(p) => p,
        Err(e) => return fail(2, &format!("--cortex {}: {e}", cortex.display())),
    };
    let hash = |p: &Path| std::fs::read(p).map(|b| sha256_hex(&b));
    let before = match hash(&cortex) {
        Ok(h) => h,
        Err(e) => return fail(2, &format!("cannot read {}: {e}", cortex.display())),
    };
    // Security F12: the preview needs no operator key; its child never sees it.
    let run = std::process::Command::new(&cortex)
        .args(["grant", "--envelope-preview"])
        .args(&a.preview)
        .env_remove(axon_os::approval::APPROVAL_KEY_ENV)
        .stdin(std::process::Stdio::null())
        .output();
    let run = match run {
        Ok(o) => o,
        Err(e) => return fail(2, &format!("cannot run {}: {e}", cortex.display())),
    };
    if !run.status.success() {
        return fail(
            2,
            &format!(
                "the cortex preview failed ({}): {}",
                match run.status.code() {
                    Some(c) => format!("exit {c}"),
                    None => "killed by a signal".into(),
                },
                String::from_utf8_lossy(&run.stderr).trim_end()
            ),
        );
    }
    match hash(&cortex) {
        Ok(after) if after == before => {}
        _ => {
            return fail(
                2,
                &format!("{} changed while its preview ran", cortex.display()),
            )
        }
    }
    let text = match String::from_utf8(run.stdout) {
        Ok(t) => t,
        Err(_) => return fail(2, "cortex preview: output is not UTF-8"),
    };
    let preview = match parse_preview(&text) {
        Ok(p) => p,
        Err(e) => return fail(2, &e),
    };
    let envelope_id = match repair_envelope::new_id() {
        Ok(id) => id,
        Err(e) => return fail(2, &e),
    };
    let [Some(max_proposals), Some(deadline_ms), Some(check_timeout_ms), Some(candidates), Some(max_steps)] =
        a.budget
    else {
        return fail(2, "a budget flag is missing");
    };
    let req = MintRequest {
        symbol: a.symbol.clone().unwrap_or_default(),
        check: a.check.clone().unwrap_or_default(),
        issued_by: a.by.clone().unwrap_or_default(),
        ttl_s: a.ttl_s.unwrap_or(0),
        budget: Budget {
            max_proposals,
            deadline_ms,
            check_timeout_ms,
            candidates,
            max_steps,
        },
    };
    let now = now_unix();
    // Step 2 (and the preview's own consistency) inside build_envelope.
    let env = match build_envelope(
        &preview,
        &req,
        envelope_id,
        store_id.clone(),
        ledger_id,
        now,
    ) {
        Ok(e) => e,
        Err(Refused::Gate(e)) => return fail(8, &e),
        Err(Refused::Bad(e)) => return fail(2, &e),
    };
    for (what, path, read_too) in &guarded {
        if let Err(e) = repair_envelope::refuse_pin_overlap(&env.scopes, what, path, *read_too) {
            return fail(2, &e);
        }
    }
    let json = match key.sign(&env) {
        Ok(j) => j,
        Err(e) => return fail(2, &e),
    };
    let verified = match repair_envelope::verify(&json, &key.verify_key(), now) {
        Ok(v) => v,
        Err(e) => return fail(2, &format!("the minted envelope does not verify: {e}")),
    };
    // Step 3: what minted it.
    let record = json!({
        "schema": MINT_RECORD_SCHEMA,
        "envelope_id": env.envelope_id,
        "envelope_digest": verified.digest,
        "issued_by": env.issued_by,
        "issued_unix": env.issued_unix,
        "expires_unix": env.expires_unix,
        "cortex_path": cortex.display().to_string(),
        "cortex_sha256": before,
        "fence": preview.fence,
        "gate": preview.gate.iter().map(|r| json!({"id": r.id, "status": r.status, "basis": r.basis})).collect::<Vec<_>>(),
    });
    if let Err(e) = write_new(&mint_path, format!("{record}\n").as_bytes()) {
        return fail(2, &format!("cannot write {}: {e}", mint_path.display()));
    }
    // Step 4, then step 5.
    if let Err(e) = repair_envelope::issue(store, me, &store_id, &env.envelope_id, now) {
        return fail(2, &e);
    }
    if let Err(e) = write_new(out, format!("{json}\n").as_bytes()) {
        return fail(
            2,
            &format!(
                "envelope {} was issued but {} cannot be written: {e}",
                env.envelope_id,
                out.display()
            ),
        );
    }
    println!(
        "\u{2713} repair envelope {} for {}:{} (check {}) by {}, expires_unix {}",
        env.envelope_id,
        env.task.file,
        env.task.symbol,
        env.check.name,
        env.issued_by,
        env.expires_unix
    );
    println!("  envelope_digest {}", verified.digest);
    println!("  cortex {} sha256 {before}", cortex.display());
    println!("  Wrote {} and {}", out.display(), mint_path.display());
    println!(
        "  This authorizes nothing by itself: `cortex repair --unattended` refuses until it is enabled (NH-R1 §9)."
    );
    ExitCode::from(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, status: &str) -> GateRow {
        GateRow {
            id: id.into(),
            status: status.into(),
            basis: "b".into(),
        }
    }

    #[test]
    fn the_gate_admits_only_all_admit_rows() {
        assert_eq!(
            gate_admits(&[row("E1", "admit"), row("E4", "admit")]),
            Ok(())
        );
        let e = gate_admits(&[]).expect_err("ATTACK: an empty gate admitted");
        assert!(e.contains("no gate rows"), "{e}");
        for bad in ["refuse", "Admit", "admit ", ""] {
            let e = gate_admits(&[row("E1", "admit"), row("E11", bad)])
                .expect_err("ATTACK: a gate row that is not exactly admit was admitted");
            assert!(e.contains("E11"), "{e}");
        }
    }

    #[test]
    fn the_preview_schema_is_checked() {
        let e = parse_preview(r#"{"schema":"cortex-grant/1"}"#).unwrap_err();
        assert!(e.starts_with("cortex preview:"), "{e}");
        let e = parse_preview(r#"{"a":1,"a":2}"#).unwrap_err();
        assert!(e.contains("duplicate"), "{e}");
    }
}
