//! NH-R1 (`tasks/unattended-auth-spec.md`): the checks of an unattended
//! `cortex repair` that need no envelope crypto — the flag allowlist (§5.2),
//! path normalization and containment, the symlink walk (E1s), the
//! enforcement-gate rows over the pristine capabilities (E4, E9, E11), the
//! run lock (E5) and the E3 watchdog.
//!
//! Every check here REFUSES on doubt: a route whose limit cannot be enforced
//! is refused, never admitted on trust (§6).

use std::path::{Component, Path, PathBuf};

/// §5.2: the only flags `cortex repair --unattended` accepts. Every other
/// flag — `--fabric-*` (E2), `--check-registry` (E7), `--ranker`, `--audit`,
/// `--label-log`, `--principal`, … — refuses (exit 29).
pub const ALLOWED_FLAGS: &[&str] = &[
    "--workspace",
    "--file",
    "--symbol",
    "--check",
    "--generator",
    "--write-prefix",
    "--max-proposals",
    "--deadline-ms",
    "--check-timeout-ms",
    "--candidates",
    "--budget",
    "--axon",
    "--json",
    "--unattended",
    "--envelope",
    "--verify-key",
    "--spent-store",
    // R6 §4: the run ledger sink and the run-record seal key.
    "--run-ledger",
    "--run-record-key",
];

/// §5.2: environment selectors that steer a run outside the envelope; set,
/// they refuse.
pub const REFUSED_ENV: &[&str] = &[
    crate::rank::RANKER_ENV,
    crate::audit::AUDIT_ENV,
    crate::label::LABEL_ENV,
    "CORTEX_MODEL",
    "AXON_CORTEX_GENERATOR_TIMEOUT_MS",
];

/// One enforcement-gate row (§5.6 `enforcement`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: &'static str,
    pub admit: bool,
    pub basis: String,
}

impl Row {
    pub(crate) fn admit(id: &'static str, basis: impl Into<String>) -> Row {
        Row {
            id,
            admit: true,
            basis: basis.into(),
        }
    }
    pub(crate) fn refuse(id: &'static str, basis: impl Into<String>) -> Row {
        Row {
            id,
            admit: false,
            basis: basis.into(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "status": if self.admit { "admit" } else { "refuse" },
            "basis": self.basis,
        })
    }
}

/// §5.2 row 1: every flag on the command line (the arguments that start
/// with `--`; values are skipped by the caller) must be allowlisted.
pub fn check_flag(flag: &str) -> Result<(), String> {
    if ALLOWED_FLAGS.contains(&flag) {
        return Ok(());
    }
    let why = if flag.starts_with("--fabric-") {
        " (E2: the Fabric executor does not take the fence)"
    } else if flag == "--check-registry" {
        " (E7: only the local interpreter executor is admitted)"
    } else {
        ""
    };
    Err(format!("`{flag}` is not accepted with --unattended{why}"))
}

/// §5.2: the first refused environment selector that is set, if any.
pub fn check_env(get: impl Fn(&str) -> Option<std::ffi::OsString>) -> Result<(), String> {
    match REFUSED_ENV.iter().find(|v| get(v).is_some()) {
        Some(v) => Err(format!("`{v}` is set; it is refused with --unattended")),
        None => Ok(()),
    }
}

/// E9: only an `ai:MODEL` generator. A `cmd:` generator is an arbitrary
/// same-uid process holding the run's authority; `literal:` and `none`
/// propose nothing a model chose and have no place in an unattended run.
pub fn check_generator(spec: &str) -> Result<&str, String> {
    match spec.strip_prefix("ai:") {
        Some(m) if !m.is_empty() => Ok(m),
        _ => Err(format!(
            "E9: --generator `{spec}` refused: only `ai:MODEL` is admitted unattended"
        )),
    }
}

/// A workspace-relative path in one canonical spelling: `/`-separated
/// normal components, no `.`, no `..`, not absolute, not empty, no `*` or
/// `,` (a scope list separator).
pub fn normalize_rel(p: &str) -> Result<String, String> {
    if p.contains(',') || p.contains('*') {
        return Err(format!("`{p}` holds `,` or `*`"));
    }
    let mut parts = Vec::new();
    for c in Path::new(p).components() {
        match c {
            Component::Normal(s) => parts.push(
                s.to_str()
                    .ok_or_else(|| format!("`{p}` is not UTF-8"))?
                    .to_string(),
            ),
            Component::CurDir => {}
            Component::ParentDir => return Err(format!("`{p}` holds `..`")),
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("`{p}` is not workspace-relative"))
            }
        }
    }
    if parts.is_empty() {
        return Err(format!("`{p}` names no path"));
    }
    Ok(parts.join("/"))
}

/// A net pin: one host glob, lowercase, no separators, not `*` alone.
pub fn normalize_host(h: &str) -> Result<String, String> {
    let h = h.trim().to_ascii_lowercase();
    if h.is_empty() || h == "*" || h.contains(',') || h.chars().any(char::is_whitespace) {
        return Err(format!("`{h}` is not a single host"));
    }
    Ok(h)
}

/// Component-wise containment of normalized paths: `a/b` is within `a`, `ab`
/// is not.
pub fn within(child: &str, parent: &str) -> bool {
    let c: Vec<&str> = child.split('/').collect();
    let p: Vec<&str> = parent.split('/').collect();
    c.len() >= p.len() && c[..p.len()] == p[..]
}

/// §5.2: every CLI write prefix lies within some envelope write prefix.
pub fn prefixes_within(cli: &[String], envelope: &[String]) -> Result<(), String> {
    for c in cli {
        let n = normalize_rel(c)?;
        if !envelope.iter().any(|e| within(&n, e)) {
            return Err(format!(
                "--write-prefix `{c}` is wider than the envelope's write_prefixes"
            ));
        }
    }
    Ok(())
}

/// E1s (F2): no symlink at `rel` or at any ancestor of it inside `workspace`
/// (lstat walk). `rel` must exist only when `must_exist`.
pub fn no_symlink_on_path(workspace: &Path, rel: &str, must_exist: bool) -> Result<(), String> {
    let n = normalize_rel(rel)?;
    let mut at = workspace.to_path_buf();
    for part in n.split('/') {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!("E1s: `{}` is a symlink", at.display()))
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !must_exist => return Ok(()),
            Err(e) => return Err(format!("E1s: `{}` cannot be examined: {e}", at.display())),
        }
    }
    Ok(())
}

/// E1s (F2): no symlink at `rel`, at any ancestor of it, or ANYWHERE under
/// it (a write prefix or scope pin covers its whole subtree).
pub fn no_symlink_at_or_under(workspace: &Path, rel: &str) -> Result<(), String> {
    no_symlink_on_path(workspace, rel, false)?;
    let root = workspace.join(normalize_rel(rel)?);
    let mut stack: Vec<PathBuf> = match std::fs::symlink_metadata(&root) {
        Ok(m) if m.is_dir() => vec![root],
        _ => return Ok(()),
    };
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| format!("E1s: `{}` cannot be listed: {e}", dir.display()))?;
        for e in entries {
            let e = e.map_err(|e| format!("E1s: `{}` cannot be listed: {e}", dir.display()))?;
            let t = e
                .file_type()
                .map_err(|err| format!("E1s: `{}`: {err}", e.path().display()))?;
            if t.is_symlink() {
                return Err(format!("E1s: `{}` is a symlink", e.path().display()));
            }
            if t.is_dir() {
                stack.push(e.path());
            }
        }
    }
    Ok(())
}

/// The gate rows read from the pristine `axon caps` answer: E4 (native
/// modules) and E11 (process spawn). Unknown (the walker could not classify
/// everything) refuses both: a limit nobody can see is not enforced.
pub fn caps_rows(caps: &crate::fence::Caps) -> Vec<Row> {
    let mut rows = Vec::new();
    if caps.unknown {
        rows.push(Row::refuse(
            "E4",
            "the pristine capabilities are unknown (`axon caps` could not classify everything)",
        ));
        rows.push(Row::refuse("E11", "the pristine capabilities are unknown"));
        return rows;
    }
    match caps.capabilities.iter().find(|c| c.starts_with("native:")) {
        Some(n) => rows.push(Row::refuse(
            "E4",
            format!(
                "the pristine file reaches the native module `{n}`, whose calls carry no scope"
            ),
        )),
        None => rows.push(Row::admit("E4", "no native: capability")),
    }
    let exec_cap = caps.capabilities.iter().any(|c| c == "exec");
    let exec_effect = caps.effects.iter().any(|e| e == "Exec");
    if exec_cap || exec_effect {
        rows.push(Row::refuse(
            "E11",
            "the pristine file reaches `exec` (a process spawn no scope confines)",
        ));
    } else {
        rows.push(Row::admit("E11", "no exec capability or Exec effect"));
    }
    // Security review F11 / §10 Q2: the durable store writes a file under the
    // cache dir whose path comes from a store KEY, so no scope pin can name
    // it; refused by its own row, not left to the child's missing `HOME`.
    if caps.capabilities.iter().any(|c| c == "dstore") {
        rows.push(Row::refuse(
            "E12",
            "the pristine file reaches the durable store (`dstore`), a file no scope pin can name",
        ));
    } else {
        rows.push(Row::admit("E12", "no dstore capability"));
    }
    rows
}

/// E1: the fence with the envelope's pins, as a gate row.
pub fn pin_row(
    fence: &crate::fence::Fence,
    pins: &crate::fence::ScopePins,
) -> (Row, Option<crate::fence::Fence>) {
    match fence.clone().with_scope_pins(pins) {
        Ok(f) => (
            Row::admit(
                "E1",
                format!("used kinds pinned; fence_sha256 {}", f.sha256()),
            ),
            Some(f),
        ),
        Err(why) => (Row::refuse("E1", why), None),
    }
}

/// Security review F4: every `fs_read` / `fs_write` pin is an absolute path.
/// The interpreter matches a pin as a plain path prefix and the check child's
/// cwd is the workspace, so a relative pin resolves inside it - and the
/// symlink walk and the policy check only see pins under the absolute
/// workspace, so a relative one (`out -> /home/op/keys`) passed both.
pub fn absolute_fs_pins(fs_read: &[String], fs_write: &[String]) -> Result<(), String> {
    for (kind, pins) in [("fs_read", fs_read), ("fs_write", fs_write)] {
        if let Some(p) = pins.iter().find(|p| !p.starts_with('/')) {
            return Err(format!(
                "scopes.{kind} entry {p:?} is relative: a pin must be absolute (a relative pin \
                 resolves against the check child's cwd and escapes the symlink walk and policy check)"
            ));
        }
    }
    Ok(())
}

/// Security review F6: `path` (the canonical spent store or a key file) and
/// every `fs_read` / `fs_write` pin are disjoint - neither lies at or under
/// the other. A pin over the store lets the check child rename `.used` back
/// to `.issued` (a replay); over the verify key, overwrite it (self-mint);
/// over the seal key, read it (forge run records).
pub fn outside_fs_pins(
    what: &str,
    path: &Path,
    fs_read: &[String],
    fs_write: &[String],
) -> Result<(), String> {
    for (kind, pins) in [("fs_read", fs_read), ("fs_write", fs_write)] {
        let overlaps = |p: &&String| {
            let p = Path::new(p.as_str());
            path.starts_with(p) || p.starts_with(path)
        };
        if let Some(p) = pins.iter().find(overlaps) {
            return Err(format!(
                "{what} {} overlaps the scopes.{kind} pin {p}: the check child could reach it",
                path.display()
            ));
        }
    }
    Ok(())
}

/// E5 / §6: the exclusive run lock, keyed by the canonical file path, held
/// in the spent store for the life of the process. A second unattended run
/// on the same file is refused (exit 29), never queued.
#[cfg(unix)]
pub fn lock_file(store: &Path, canonical_file: &Path) -> Result<std::fs::File, String> {
    use std::os::unix::io::AsRawFd;
    let key = crate::label::sha256_hex(canonical_file.as_os_str().as_encoded_bytes());
    let path = store.join(format!("run-{key}.lock"));
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| format!("the run lock {} cannot be opened: {e}", path.display()))?;
    // SAFETY: flock on an fd this function owns.
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(format!(
            "another unattended run holds {} (the run lock for {})",
            path.display(),
            canonical_file.display()
        ));
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allowlist_names_e2_and_e7() {
        assert!(check_flag("--symbol").is_ok());
        for (flag, row) in [
            ("--fabric-store", "E2"),
            ("--check-registry", "E7"),
            ("--ranker", ""),
            ("--principal", ""),
        ] {
            assert!(
                check_flag(flag).is_err_and(|why| why.contains(row)),
                "ATTACK: a non-allowlisted flag {flag} was accepted unattended"
            );
        }
    }

    #[test]
    fn env_selectors_refuse() {
        assert!(check_env(|_| None).is_ok());
        for var in REFUSED_ENV {
            let got = check_env(|v| (v == *var).then(|| "x".into()));
            assert!(
                got.is_err_and(|why| why.contains(var)),
                "ATTACK: the env selector {var} steered an unattended run"
            );
        }
    }

    #[test]
    fn only_ai_generators() {
        assert_eq!(check_generator("ai:m"), Ok("m"));
        for g in ["cmd:/bin/sh", "literal:x", "none", "ai:"] {
            assert!(
                check_generator(g).is_err_and(|why| why.contains("E9")),
                "ATTACK: E9 the generator {g} was admitted unattended"
            );
        }
    }

    #[test]
    fn normalization_and_containment() {
        assert_eq!(normalize_rel("./a//b/"), Ok("a/b".into()));
        for bad in ["", ".", "/a", "a/../b", "..", "a,b", "*", "a/*"] {
            assert!(
                normalize_rel(bad).is_err(),
                "ATTACK: the path {bad:?} normalized to a workspace path"
            );
        }
        assert!(within("a/b", "a"));
        assert!(within("a", "a"));
        assert!(
            !within("ab", "a") && !within("a", "a/b"),
            "ATTACK: containment is not component-wise"
        );
        assert_eq!(
            normalize_host(" API.example.com"),
            Ok("api.example.com".into())
        );
        for bad in ["", "*", "a,b", "a b"] {
            assert!(
                normalize_host(bad).is_err(),
                "ATTACK: the host pin {bad:?} normalized to a single host"
            );
        }
        assert!(prefixes_within(&["a/b".into()], &["a".into()]).is_ok());
        for wider in ["ab", "b", "a/../b"] {
            assert!(
                prefixes_within(&[wider.into()], &["a".into()]).is_err(),
                "ATTACK: the write prefix {wider} wider than the envelope's was accepted"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_at_on_or_under_refuse() {
        let d = std::env::temp_dir().join(format!("cortex-e1s-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src/deep")).unwrap();
        std::fs::write(d.join("src/f.ax"), "").unwrap();
        assert!(no_symlink_on_path(&d, "src/f.ax", true).is_ok());
        assert!(no_symlink_at_or_under(&d, "src").is_ok());
        std::os::unix::fs::symlink("/etc", d.join("src/deep/out")).unwrap();
        assert!(
            no_symlink_at_or_under(&d, "src").is_err_and(|why| why.contains("E1s")),
            "ATTACK: E1s a symlink under a write prefix was admitted"
        );
        std::os::unix::fs::symlink(d.join("src"), d.join("alias")).unwrap();
        std::os::unix::fs::symlink(d.join("src/f.ax"), d.join("src/link.ax")).unwrap();
        for linked in ["alias/f.ax", "src/link.ax"] {
            assert!(
                no_symlink_on_path(&d, linked, true).is_err_and(|why| why.contains("E1s")),
                "ATTACK: E1s the symlinked task file {linked} was admitted"
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    fn caps(capabilities: &[&str], effects: &[&str], unknown: bool) -> crate::fence::Caps {
        crate::fence::Caps {
            capabilities: capabilities.iter().map(|s| s.to_string()).collect(),
            effects: effects.iter().map(|s| s.to_string()).collect(),
            unknown,
        }
    }

    fn admits(rows: &[Row], id: &str) -> bool {
        rows.iter().find(|r| r.id == id).unwrap().admit
    }

    #[test]
    fn e4_e11_rows() {
        let ok = caps_rows(&caps(&["fs:read"], &["FileRead"], false));
        assert!(ok.iter().all(|r| r.admit), "{ok:?}");
        assert!(
            !admits(&caps_rows(&caps(&["native:gfx"], &[], false)), "E4"),
            "ATTACK: E4 a native:gfx file was admitted"
        );
        assert!(
            !admits(&caps_rows(&caps(&["exec"], &[], false)), "E11"),
            "ATTACK: E11 an exec-capability file was admitted"
        );
        assert!(
            !admits(&caps_rows(&caps(&[], &["Exec"], false)), "E11"),
            "ATTACK: E11 an Exec-effect file was admitted"
        );
        assert!(
            caps_rows(&caps(&[], &[], true)).iter().all(|r| !r.admit),
            "ATTACK: a file of unknown capabilities was admitted"
        );
    }

    /// Security review F11 / §10 Q2: a file reaching the durable store is
    /// refused by its own gate row, not left to the child's missing `HOME`.
    #[test]
    fn e12_a_dstore_file_is_refused() {
        let rows = caps_rows(&caps(&["dstore"], &[], false));
        assert!(
            rows.iter().any(|r| r.id == "E12"),
            "ATTACK: NH-R1 security F11 a dstore file had no gate row: {rows:?}"
        );
        assert!(
            !admits(&rows, "E12"),
            "ATTACK: NH-R1 security F11 a dstore file was admitted: {rows:?}"
        );
        assert!(admits(&caps_rows(&caps(&["fs:read"], &[], false)), "E12"));
    }

    #[test]
    fn e1_pins_used_kinds_only() {
        use crate::fence::{Fence, ScopePins};
        let fence = Fence::from_pristine(&caps(&["fs:read", "net"], &["FileRead", "Net"], false));
        // Without pins a used kind stays unset (unrestricted): refused.
        let (row, f) = pin_row(&fence, &ScopePins::default());
        assert!(
            !row.admit && f.is_none(),
            "ATTACK: E1 a used kind with no pin ran unrestricted: {row:?}"
        );
        let pins = ScopePins {
            fs_read: vec!["data".into()],
            fs_write: vec![],
            net: vec!["api.example.com".into()],
        };
        let (row, f) = pin_row(&fence, &pins);
        assert!(row.admit, "{row:?}");
        let env = f.unwrap().env;
        assert_eq!(env["AXON_ALLOWED_FS_READ"], "data");
        assert_eq!(env["AXON_ALLOWED_NET"], "api.example.com");
        assert_eq!(env["AXON_ALLOWED_FS_WRITE"], "");
        // A pin on an unused kind would widen a deny-all.
        let wider = ScopePins {
            fs_write: vec!["out".into()],
            ..pins.clone()
        };
        assert!(
            !pin_row(&fence, &wider).0.admit,
            "ATTACK: E1 a pin widened the deny-all of an unused kind"
        );
        let star = ScopePins {
            net: vec!["*".into()],
            ..pins.clone()
        };
        assert!(
            !pin_row(&fence, &star).0.admit,
            "ATTACK: E1 a wildcard pin was admitted"
        );
        assert!(
            !pin_row(&fence.clone().not_applied(), &pins).0.admit,
            "ATTACK: E1 a fence that is not applied was pinned"
        );
        let unknown = Fence::from_pristine(&caps(&[], &[], true));
        assert!(
            !pin_row(&unknown, &ScopePins::default()).0.admit,
            "ATTACK: E1 a fence with no axon caps basis was pinned"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_run_lock_is_exclusive() {
        let d = std::env::temp_dir().join(format!("cortex-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let held = lock_file(&d, Path::new("/w/f.ax")).unwrap();
        assert!(
            lock_file(&d, Path::new("/w/f.ax")).is_err(),
            "ATTACK: a second unattended run took the lock of a file in use"
        );
        assert!(lock_file(&d, Path::new("/w/g.ax")).is_ok());
        drop(held);
        assert!(lock_file(&d, Path::new("/w/f.ax")).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }
}
