//! NH-R1 `cortex repair --unattended` (`tasks/unattended-auth-spec.md` §5.2-§5.6):
//! the envelope verification of an unattended run, in the two phases the
//! spec fixes.
//!
//! - [`phase_a`] runs right after argument parsing, before the generator is
//!   built and before ANY child is spawned: flag allowlist, parse + schema,
//!   verify-key file, signature, time, the §5.2 limit comparisons, the sha256
//!   of `--axon` against the check digest, and the task (canonical paths, the
//!   symlink walk, the entry file read ONCE).
//! - [`phase_b`] runs after the pinned interpreter's static `axon caps` and
//!   before any child that executes workspace code: the pinned fence, the
//!   target's static grant, the import closure, the check digest, the
//!   enforcement gate (§6), and - last - consume.
//!
//! Every refusal is a `String` the caller prints before exiting 29
//! (`unauthorized`); nothing here writes to the workspace.

use std::path::{Path, PathBuf};

use axon_os::repair_envelope::{self as env, Effective, Requested, VerifiedEnvelope};

use crate::unattended::{self as un, Row};

/// The exit code of every unattended refusal (§5.3).
pub const EXIT_UNAUTHORIZED: i32 = 29;

/// The `argv_shape` of the only admitted check route (E7: the local
/// interpreter executor).
pub const ARGV_SHAPE: &str = "axon-test-local/1";

/// What the unattended command line gave, as parsed by `cortex repair`.
#[derive(Debug, Clone, Default)]
pub struct Cli {
    /// Every flag seen, in order (values not included).
    pub flags: Vec<String>,
    pub envelope: Option<PathBuf>,
    pub verify_key: Option<PathBuf>,
    pub spent_store: Option<PathBuf>,
    pub workspace: PathBuf,
    pub file: String,
    pub symbol: String,
    pub check: String,
    pub generator: String,
    pub axon: PathBuf,
    pub requested: Requested,
}

/// What phase A established.
#[derive(Debug)]
pub struct PhaseA {
    pub verified: VerifiedEnvelope,
    pub effective: Effective,
    /// The canonical workspace.
    pub workspace: PathBuf,
    /// The normalized workspace-relative entry file.
    pub file: String,
    /// The entry file's bytes, read once (the walk's `original`, §5.4).
    pub pristine: Vec<u8>,
    /// The canonical spent store.
    pub store: PathBuf,
    /// sha256 of the `--axon` bytes, taken before any spawn of it.
    pub checker_exe_sha256: String,
    /// The run lock (E5), held for the life of the process.
    pub lock: std::fs::File,
    pub euid: u32,
}

/// What phase B established.
#[derive(Debug)]
pub struct PhaseB {
    /// The fence with the envelope's scope pins (E1).
    pub fence: crate::fence::Fence,
    /// The enforcement-gate rows, every one admitted.
    pub rows: Vec<Row>,
}

/// §4: `check.digest = sha256(cl22{check_name, checker_exe_sha256,
/// argv_shape, check_suite_ref|null, fence_sha256, imports_digest})`.
/// The ONE definition, called by the minter's preview and by the runner.
pub fn check_digest(
    check_name: &str,
    checker_exe_sha256: &str,
    check_suite_ref: Option<&str>,
    fence_sha256: &str,
    imports_digest: &str,
) -> String {
    let input = check_digest_input(
        check_name,
        checker_exe_sha256,
        check_suite_ref,
        fence_sha256,
        imports_digest,
    );
    let bytes = axon_canonical::canonical_bytes(&input)
        .expect("the check digest input is strings and null only");
    crate::label::sha256_hex(&bytes)
}

/// The object [`check_digest`] hashes (the preview shows it).
pub fn check_digest_input(
    check_name: &str,
    checker_exe_sha256: &str,
    check_suite_ref: Option<&str>,
    fence_sha256: &str,
    imports_digest: &str,
) -> serde_json::Value {
    serde_json::json!({
        "argv_shape": ARGV_SHAPE,
        "check_name": check_name,
        "check_suite_ref": check_suite_ref,
        "checker_exe_sha256": checker_exe_sha256,
        "fence_sha256": fence_sha256,
        "imports_digest": imports_digest,
    })
}

/// The `imports_digest` of `workspace/file`'s import closure, by the pinned
/// interpreter (`axon imports --json`, schema `axon-imports/1`).
pub fn imports_digest(
    axon: &Path,
    workspace: &Path,
    file: &str,
    wall: std::time::Duration,
) -> Result<String, String> {
    let (text, code) = crate::runner::run_axon_imports(axon, &workspace.join(file), wall)
        .map_err(|e| format!("`axon imports` on {file} could not be run: {e}"))?;
    if code != Some(0) {
        return Err(format!(
            "`axon imports` on {file} exited {code:?}: {}",
            text.trim()
        ));
    }
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["schema"] != "axon-imports/1" {
            continue;
        }
        return match v["imports_digest"].as_str() {
            Some(d) if d.starts_with("axsha256:") => Ok(d.to_string()),
            _ => Err(format!(
                "`axon imports` on {file} printed no axsha256 digest"
            )),
        };
    }
    Err(format!(
        "`axon imports` on {file} printed no axon-imports/1 record"
    ))
}

/// The provider name and endpoint host the `ai:` generator will reach, as
/// `axon-ai` resolves them (§5.2 generator rule).
pub fn resolved_generator() -> (String, String) {
    let p = axon_ai::provider();
    let provider = match p {
        axon_ai::Provider::Anthropic => "anthropic",
        axon_ai::Provider::OpenAi => "openai",
    };
    (provider.to_string(), axon_ai::endpoint_host(p))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    std::fs::read(path)
        .map(|b| crate::label::sha256_hex(&b))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Phase A (§5.3 steps 1-8). `axon` is the resolved `--axon` path; nothing
/// is spawned here.
pub fn phase_a(cli: &Cli, axon: &Path, hard_grace_ms: u64) -> Result<PhaseA, String> {
    // 1. flag allowlist, env selectors, generator kind (E9).
    for f in &cli.flags {
        un::check_flag(f)?;
    }
    un::check_env(|k: &str| std::env::var_os(k))?;
    let model = un::check_generator(&cli.generator)?;
    if cli.symbol.is_empty() {
        return Err("--symbol is required with --unattended".into());
    }
    let need = |p: &Option<PathBuf>, flag: &str| -> Result<PathBuf, String> {
        p.clone()
            .ok_or_else(|| format!("{flag} is required with --unattended"))
    };
    let envelope_path = need(&cli.envelope, "--envelope")?;
    let key_path = need(&cli.verify_key, "--verify-key")?;
    let store = need(&cli.spent_store, "--spent-store")?;
    let workspace = std::fs::canonicalize(&cli.workspace)
        .map_err(|e| format!("--workspace {}: {e}", cli.workspace.display()))?;
    let store = std::fs::canonicalize(&store)
        .map_err(|e| format!("--spent-store {}: {e}", store.display()))?;
    // SAFETY: geteuid has no preconditions.
    let euid = unsafe { libc::geteuid() };
    // 2-4. parse + schema, the verify key's file, signature; 5 (part): time.
    let key = env::VerifyKey::load(&key_path, euid, std::slice::from_ref(&workspace))?;
    let text = std::fs::read_to_string(&envelope_path)
        .map_err(|e| format!("--envelope {}: {e}", envelope_path.display()))?;
    let now = now_unix();
    let verified = env::verify(&text, &key, now)?;
    let e = &verified.envelope;
    // 6. §5.2 limit comparisons and the generator.
    let effective = e.narrow(&cli.requested)?;
    // 5 (rest): the run ends before the envelope expires.
    e.fits_before_expiry(now, effective.deadline_ms, hard_grace_ms)?;
    let (provider, host) = resolved_generator();
    e.check_generator("ai", model, &provider, &host)?;
    // 8. the task: canonical workspace and file, the symlink walk, one read.
    let file = un::normalize_rel(&cli.file)?;
    if workspace.to_string_lossy() != e.task.workspace {
        return Err(format!(
            "workspace {} is not the envelope's {}",
            workspace.display(),
            e.task.workspace
        ));
    }
    if file != e.task.file {
        return Err(format!(
            "--file {file} is not the envelope's {}",
            e.task.file
        ));
    }
    if cli.symbol != e.task.symbol {
        return Err(format!(
            "--symbol {} is not the envelope's {}",
            cli.symbol, e.task.symbol
        ));
    }
    if cli.check != e.check.name {
        return Err(format!(
            "--check {} is not the envelope's {}",
            cli.check, e.check.name
        ));
    }
    un::no_symlink_on_path(&workspace, &file, true)?;
    for p in &effective.write_prefixes {
        un::no_symlink_at_or_under(&workspace, p)?;
    }
    for p in e.scopes.fs_read.iter().chain(&e.scopes.fs_write) {
        if let Ok(rel) = Path::new(p).strip_prefix(&workspace) {
            un::no_symlink_at_or_under(&workspace, &rel.to_string_lossy())?;
        }
    }
    if store.starts_with(&workspace) {
        return Err(format!(
            "the spent store {} lies inside the workspace: the repaired code could reach it",
            store.display()
        ));
    }
    let canonical_file = workspace.join(&file);
    let pristine =
        std::fs::read(&canonical_file).map_err(|e| format!("{}: {e}", canonical_file.display()))?;
    let found = crate::label::sha256_hex(&pristine);
    if found != e.task.pristine_sha256 {
        return Err(format!(
            "{file} (sha256 {found}) is not the file the envelope was minted for (sha256 {})",
            e.task.pristine_sha256
        ));
    }
    // 7. the check digest over the `--axon` bytes, before any spawn of it.
    let checker_exe_sha256 = sha256_file(axon)?;
    let want = check_digest(
        &e.check.name,
        &checker_exe_sha256,
        None,
        &e.capabilities.fence_sha256,
        &e.task.imports_digest,
    );
    if want != e.check.digest {
        return Err(format!(
            "check digest {want} (over --axon sha256 {checker_exe_sha256}) is not the \
             envelope's {}: a different checker, fence or import closure",
            e.check.digest
        ));
    }
    let lock = un::lock_file(&store, &canonical_file)?;
    Ok(PhaseA {
        verified,
        effective,
        workspace,
        file,
        pristine,
        store,
        checker_exe_sha256,
        lock,
        euid,
    })
}

/// Phase B (§5.3 steps 9-11): after the pinned interpreter's static `axon
/// caps`, before any child that executes workspace code. `fence` is the
/// pristine fence `derive_fence` built; `caps` the whole-file answer it was
/// built from.
pub fn phase_b(
    a: &PhaseA,
    axon: &Path,
    fence: &crate::fence::Fence,
    caps: &crate::fence::Caps,
    wall: std::time::Duration,
) -> Result<PhaseB, String> {
    let e = &a.verified.envelope;
    let pins = crate::fence::ScopePins {
        fs_read: e.scopes.fs_read.clone(),
        fs_write: e.scopes.fs_write.clone(),
        net: e.scopes.net.clone(),
    };
    let (rows, pinned) = gate_rows(fence, caps, &pins, &e.generator.kind);
    if let Some(bad) = rows.iter().find(|r| !r.admit) {
        return Err(format!("enforcement gate {}: {}", bad.id, bad.basis));
    }
    let pinned = pinned.expect("an admitted E1 row carries the pinned fence");
    // 9. pinned fence, target grant, import closure, check digest.
    let sha = pinned.sha256();
    if sha != e.capabilities.fence_sha256 {
        return Err(format!(
            "the pinned fence (sha256 {sha}) is not the envelope's {}",
            e.capabilities.fence_sha256
        ));
    }
    let grant = target_grant(axon, &a.workspace, &a.file, &e.task.symbol, wall)?;
    if grant != e.capabilities.target_grant {
        return Err(format!(
            "the static grant of `{}` ({grant}) is not the envelope's ({})",
            e.task.symbol, e.capabilities.target_grant
        ));
    }
    let imports = imports_digest(axon, &a.workspace, &a.file, wall)?;
    if imports != e.task.imports_digest {
        return Err(format!(
            "the import closure ({imports}) is not the envelope's ({})",
            e.task.imports_digest
        ));
    }
    let digest = check_digest(&e.check.name, &a.checker_exe_sha256, None, &sha, &imports);
    if digest != e.check.digest {
        return Err(format!(
            "check digest {digest} is not the envelope's {}",
            e.check.digest
        ));
    }
    fs_write_disjoint(&e.scopes.fs_write, &a.workspace, &a.file)?;
    // 11. consume: the last check, so a refused envelope stays unspent.
    env::consume(&a.store, a.euid, &a.verified, now_unix())?;
    Ok(PhaseB {
        fence: pinned,
        rows,
    })
}

/// The enforcement-gate rows (§6) over the pristine fence and its whole-file
/// `axon caps` answer, and the fence with the scope `pins` (E1) when that
/// row admits. The minter's preview and phase B both call this.
pub fn gate_rows(
    fence: &crate::fence::Fence,
    caps: &crate::fence::Caps,
    pins: &crate::fence::ScopePins,
    generator_kind: &str,
) -> (Vec<Row>, Option<crate::fence::Fence>) {
    let mut rows = vec![Row::admit(
        "E2",
        "local interpreter executor (no --fabric-*)",
    )];
    rows.push(Row::admit(
        "E7",
        "local interpreter executor (no --check-registry)",
    ));
    rows.push(match un::check_generator(&format!("{generator_kind}:m")) {
        Ok(_) => Row::admit("E9", format!("generator `{generator_kind}`")),
        Err(why) => Row::refuse("E9", why),
    });
    rows.extend(un::caps_rows(caps));
    let (row, pinned) = un::pin_row(fence, pins);
    rows.push(row);
    rows.push(Row::admit(
        "E1s",
        "no symlink on the task file, write prefixes or pins",
    ));
    (rows, pinned)
}

/// The static grant of `symbol` in the pristine `workspace/file`, as JSON
/// (`capabilities.target_grant`). The minter's preview and phase B both
/// call this.
pub fn target_grant(
    axon: &Path,
    workspace: &Path,
    file: &str,
    symbol: &str,
    wall: std::time::Duration,
) -> Result<serde_json::Value, String> {
    let caps = crate::fence::caps_of_file(axon, &workspace.join(file), Some(symbol), wall)
        .map_err(|why| format!("the static grant of `{symbol}` could not be derived: {why}"))?;
    Ok(crate::fence::StaticGrant::from_caps(symbol, &caps).to_json())
}

/// §5.4: the check child's FS_WRITE pins are disjoint from the task file and
/// the policy paths (a pin is an absolute path prefix).
pub fn fs_write_disjoint(fs_write: &[String], workspace: &Path, file: &str) -> Result<(), String> {
    let task = workspace.join(file);
    for pin in fs_write {
        let p = Path::new(pin);
        if task.starts_with(p) || p.starts_with(&task) {
            return Err(format!(
                "the check's fs_write pin {pin} covers the task file {file}"
            ));
        }
        if let Ok(rel) = p.strip_prefix(workspace) {
            let rel = rel.to_string_lossy();
            if crate::runner::touches_policy(&rel) {
                return Err(format!(
                    "the check's fs_write pin {pin} covers a policy path"
                ));
            }
        }
    }
    Ok(())
}

/// §5.4: the bytes about to be graded still are the pristine file and the
/// import closure is still the envelope's. Called after the witness run.
pub fn still_pristine(a: &PhaseA, axon: &Path, wall: std::time::Duration) -> Result<(), String> {
    let now = std::fs::read(a.workspace.join(&a.file)).map_err(|e| format!("{}: {e}", a.file))?;
    if now != a.pristine {
        return Err(format!(
            "{} changed after verification (sha256 {} -> {})",
            a.file,
            a.verified.envelope.task.pristine_sha256,
            crate::label::sha256_hex(&now)
        ));
    }
    let imports = imports_digest(axon, &a.workspace, &a.file, wall)?;
    if imports != a.verified.envelope.task.imports_digest {
        return Err(format!(
            "the import closure changed after verification ({} -> {imports})",
            a.verified.envelope.task.imports_digest
        ));
    }
    Ok(())
}

/// §5.6: the `authorization` object of an unattended `cortex-repair/1` record.
pub fn authorization_json(a: &PhaseA, b: &PhaseB) -> serde_json::Value {
    let e = &a.verified.envelope;
    serde_json::json!({
        "schema": e.schema,
        "envelope_id": e.envelope_id,
        "envelope_digest": a.verified.digest,
        "issued_by": e.issued_by,
        "expires_unix": e.expires_unix,
        "enforcement": b.rows.iter().map(Row::to_json).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_digest_is_sha256_of_the_canonical_input() {
        let d = check_digest("t", &"a".repeat(64), None, &"b".repeat(64), "axsha256:00");
        assert_eq!(d.len(), 64);
        let other = check_digest("u", &"a".repeat(64), None, &"b".repeat(64), "axsha256:00");
        assert_ne!(
            d, other,
            "ATTACK: NH-R1 the check name is not bound by the check digest"
        );
        let exe = check_digest("t", &"c".repeat(64), None, &"b".repeat(64), "axsha256:00");
        assert_ne!(
            d, exe,
            "ATTACK: NH-R1 the checker binary is not bound by the check digest"
        );
    }

    #[test]
    fn an_fs_write_pin_over_the_task_file_refuses() {
        let w = Path::new("/w");
        assert!(fs_write_disjoint(&["/w/out".into()], w, "f.ax").is_ok());
        assert!(
            fs_write_disjoint(&["/w".into()], w, "f.ax").is_err(),
            "ATTACK: NH-R1 a check fs_write pin covering the task file was admitted"
        );
        assert!(fs_write_disjoint(&["/w/scripts".into()], w, "f.ax").is_err());
    }
}
