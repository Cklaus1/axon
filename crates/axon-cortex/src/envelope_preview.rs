//! NH-R1 §5.1: `cortex grant --envelope-preview`, the minter's source for
//! every value an `axon-approval/4` repair envelope pins.
//!
//! The operator's `axon-intent authorize-repair` execs this and signs what it
//! prints. Each value is computed with the SAME function the unattended runner
//! calls when it verifies the envelope ([`crate::auth::phase_a`] /
//! [`crate::auth::phase_b`]): the pinned fence and its sha256 and the gate
//! rows ([`auth::gate_rows`]), the target's static grant
//! ([`auth::target_grant`]), the import closure ([`auth::imports_digest`]) and
//! the check digest ([`auth::check_digest`] over the `--axon` bytes). A value
//! derived twice by two definitions could drift; one definition cannot.
//!
//! The preview runs nothing of the workspace: `axon caps` and `axon imports`
//! are static. It refuses (an `Err`, never a partial document) the inputs
//! phase A would refuse before any gate row is reached: a non-normalized
//! path, a symlink on the task file, a write prefix or a workspace pin, a
//! check `fs_write` pin over the task file.

use crate::auth;
use crate::unattended as un;
use std::path::{Path, PathBuf};

/// `cortex-envelope-preview/1`: the preview's schema id (axon-intent's
/// `PREVIEW_SCHEMA` reads it).
pub const PREVIEW_SCHEMA: &str = "cortex-envelope-preview/1";

/// The operator's request, as `cortex grant --envelope-preview` parsed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub workspace: PathBuf,
    pub file: String,
    pub symbol: String,
    pub check: String,
    pub write_prefixes: Vec<String>,
    pub pins: crate::fence::ScopePins,
    /// `ai:MODEL` (E9; anything else is a refused gate row).
    pub generator: String,
}

/// The preview document, or why it cannot be produced.
pub fn envelope_preview(
    axon: &Path,
    r: &Request,
    wall: std::time::Duration,
) -> Result<serde_json::Value, String> {
    if r.symbol.is_empty() {
        return Err("--symbol is required with --envelope-preview".into());
    }
    if r.check.is_empty() {
        return Err("--check is required with --envelope-preview".into());
    }
    if r.write_prefixes.is_empty() {
        return Err("at least one --write-prefix is required with --envelope-preview".into());
    }
    let workspace = std::fs::canonicalize(&r.workspace)
        .map_err(|e| format!("--workspace {}: {e}", r.workspace.display()))?;
    let ws = workspace
        .to_str()
        .ok_or_else(|| format!("--workspace {} is not UTF-8", workspace.display()))?
        .to_string();
    let file = un::normalize_rel(&r.file)?;
    let write_prefixes = r
        .write_prefixes
        .iter()
        .map(|p| un::normalize_rel(p))
        .collect::<Result<Vec<_>, _>>()?;
    // Phase A step 8's symlink walk, over the same paths.
    un::no_symlink_on_path(&workspace, &file, true)?;
    for p in &write_prefixes {
        un::no_symlink_at_or_under(&workspace, p)?;
    }
    for p in r.pins.fs_read.iter().chain(&r.pins.fs_write) {
        if let Ok(rel) = Path::new(p).strip_prefix(&workspace) {
            un::no_symlink_at_or_under(&workspace, &rel.to_string_lossy())?;
        }
    }
    auth::fs_write_disjoint(&r.pins.fs_write, &workspace, &file)?;
    let canonical_file = workspace.join(&file);
    let pristine =
        std::fs::read(&canonical_file).map_err(|e| format!("{}: {e}", canonical_file.display()))?;
    // The gate (§6) over the pristine whole-file `axon caps`, as phase B.
    let caps = crate::fence::caps_of_file(axon, &canonical_file, None, wall)
        .map_err(|why| format!("`axon caps` on {file}: {why}"))?;
    let fence = crate::fence::Fence::from_pristine(&caps);
    let (kind, model) = r.generator.split_once(':').unwrap_or((&r.generator, ""));
    let (rows, pinned) = auth::gate_rows(&fence, &caps, &r.pins, kind);
    let fence = pinned.unwrap_or(fence);
    let fence_sha256 = fence.sha256();
    let target_grant = auth::target_grant(axon, &workspace, &file, &r.symbol, wall)?;
    let imports_digest = auth::imports_digest(axon, &workspace, &file, wall)?;
    // Phase A step 7: the checker is the `--axon` bytes.
    let checker = std::fs::read(axon).map_err(|e| format!("--axon {}: {e}", axon.display()))?;
    let checker_exe_sha256 = crate::label::sha256_hex(&checker);
    let check_digest_input = auth::check_digest_input(
        &r.check,
        &checker_exe_sha256,
        None,
        &fence_sha256,
        &imports_digest,
    );
    let check_digest = auth::check_digest(
        &r.check,
        &checker_exe_sha256,
        None,
        &fence_sha256,
        &imports_digest,
    );
    let (provider, endpoint_host) = auth::resolved_generator();
    Ok(serde_json::json!({
        "schema": PREVIEW_SCHEMA,
        "task": {
            "workspace": ws,
            "file": file,
            "symbol": r.symbol,
            "pristine_sha256": crate::label::sha256_hex(&pristine),
        },
        "imports_digest": imports_digest,
        "check_name": r.check,
        "check_digest_input": check_digest_input,
        "check_digest": check_digest,
        "write_prefixes": write_prefixes,
        "scopes": {
            "fs_read": r.pins.fs_read,
            "fs_write": r.pins.fs_write,
            "net": r.pins.net,
        },
        "fence": fence.to_json(&[]),
        "fence_sha256": fence_sha256,
        "target_grant": target_grant,
        "generator": {
            "kind": kind,
            "model": model,
            "provider": provider,
            "endpoint_host": endpoint_host,
        },
        "gate": rows.iter().map(un::Row::to_json).collect::<Vec<_>>(),
    }))
}
