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
use axon_os::run_ledger as rl;

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
    /// R6 §4: the run ledger directory and the run-record seal key file.
    pub run_ledger: Option<PathBuf>,
    pub run_record_key: Option<PathBuf>,
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
    /// The canonical run ledger (R6 §4), its id checked against the envelope.
    pub ledger: PathBuf,
    /// The run-record seal key (R6 §4): it seals records, nothing else.
    pub seal: rl::SealKey,
    /// `--axon` resolved ONCE (`runner::resolve_executable`: a bare name is
    /// a PATH lookup), and the sha256 of that file's bytes, taken before any
    /// spawn of it. The registry pins exactly this pair (security F7).
    pub checker_exe: PathBuf,
    pub checker_exe_sha256: String,
    /// The run lock (E5), held for the life of the process.
    pub lock: std::fs::File,
    pub euid: u32,
    /// The decision record this host's opt-in file names (security F1).
    pub decision: String,
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
    imports_closure(axon, workspace, file, wall).map(|(digest, _)| digest)
}

/// [`imports_digest`], and the canonical path of every module file in the
/// closure (security review F5: no `fs_write` pin may reach one). A module
/// whose path does not resolve refuses: an unresolved path cannot be shown
/// disjoint from the pins.
pub fn imports_closure(
    axon: &Path,
    workspace: &Path,
    file: &str,
    wall: std::time::Duration,
) -> Result<(String, Vec<PathBuf>), String> {
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
        let digest = match v["imports_digest"].as_str() {
            Some(d) if d.starts_with("axsha256:") => d.to_string(),
            _ => {
                return Err(format!(
                    "`axon imports` on {file} printed no axsha256 digest"
                ))
            }
        };
        let Some(modules) = v["modules"].as_array() else {
            return Err(format!("`axon imports` on {file} printed no module list"));
        };
        let mut paths = Vec::with_capacity(modules.len());
        for m in modules {
            let Some(p) = m["path"].as_str() else {
                return Err(format!(
                    "`axon imports` on {file} printed a module with no path"
                ));
            };
            paths.push(
                std::fs::canonicalize(p)
                    .map_err(|e| format!("the imported module {p} cannot be resolved: {e}"))?,
            );
        }
        return Ok((digest, paths));
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

/// Whether an id map (`/proc/self/uid_map` / `gid_map` content) is the
/// initial user namespace's: exactly one mapping, `0 0 4294967295`
/// (whitespace-normalized).
pub fn is_initial_id_map(map: &str) -> bool {
    let mut lines = map.lines().filter(|l| !l.trim().is_empty());
    let one = lines
        .next()
        .map(|l| l.split_whitespace().collect::<Vec<_>>() == ["0", "0", "4294967295"]);
    one == Some(true) && lines.next().is_none()
}

/// [`is_initial_id_map`] of the file at `path`; an unreadable one is not.
fn id_map_file_is_initial(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|m| is_initial_id_map(&m))
}

/// Security re-review 2 N7: the host opt-in's ownership checks (root-owned,
/// no group/other write) mean something only in the initial user
/// namespace; inside one with a uid mapping any file can appear root-owned.
/// Refuses unless both `/proc/self/uid_map` and `gid_map` are the initial
/// namespace's identity map.
pub fn initial_user_namespace() -> Result<(), String> {
    for map in ["/proc/self/uid_map", "/proc/self/gid_map"] {
        if !id_map_file_is_initial(Path::new(map)) {
            return Err(format!(
                "--unattended runs only in the initial user namespace: {map} is not the \
                 identity map `0 0 4294967295` (or is unreadable), so the host opt-in's \
                 ownership checks would prove nothing"
            ));
        }
    }
    Ok(())
}

/// Security review F1 / §10 Q3: the host opt-in file. A human records the
/// enabling decision; each host that agrees then holds this file. A build
/// with feature `unattended` copied to a host without it does not run.
pub const OPT_IN_PATH: &str = "/etc/axon/unattended-repair";

/// The decision record the host opt-in file at `path` names: a regular file
/// (never followed through a symlink, never a FIFO), owned by `euid`, that
/// group and other can neither read nor write (`mode & 0o077 == 0`), whose
/// one non-comment line is `decision: <the decision record>`, in directories
/// only root controls ([`open_root_held`], security re-review N2).
pub fn host_opt_in(path: &Path, euid: u32) -> Result<String, String> {
    use std::io::Read as _;
    use std::os::unix::fs::MetadataExt;
    let refuse = |why: String| {
        format!(
            "this host has not opted in to unattended repair: {why}. After a human records \
             the enabling decision (§10 Q3), the host's owner writes {} - a regular file \
             owned by uid {euid}, mode 0600, holding one line `decision: <the decision record>`, \
             in root-owned directories no other user can write",
            path.display()
        )
    };
    let mut f = open_root_held(path).map_err(refuse)?;
    let m = f
        .metadata()
        .map_err(|e| refuse(format!("{}: {e}", path.display())))?;
    if !m.file_type().is_file() {
        return Err(refuse(format!("{} is not a regular file", path.display())));
    }
    if m.uid() != euid {
        return Err(refuse(format!(
            "{} is owned by uid {}",
            path.display(),
            m.uid()
        )));
    }
    if m.mode() & 0o077 != 0 {
        return Err(refuse(format!(
            "{} has mode {:04o}: group or other can reach it",
            path.display(),
            m.mode() & 0o7777
        )));
    }
    let mut text = String::new();
    f.by_ref()
        .take(4096)
        .read_to_string(&mut text)
        .map_err(|e| refuse(format!("{}: {e}", path.display())))?;
    let mut decision = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(d) = line
            .strip_prefix("decision:")
            .map(str::trim)
            .filter(|d| !d.is_empty())
        else {
            return Err(refuse(format!(
                "{} holds `{line}`, which is not a `decision: <record>` line",
                path.display()
            )));
        };
        if decision.replace(d.to_string()).is_some() {
            return Err(refuse(format!(
                "{} names more than one decision record",
                path.display()
            )));
        }
    }
    decision.ok_or_else(|| refuse(format!("{} names no decision record", path.display())))
}

/// Security re-review N2: open `path` from `/` one component at a time with
/// `O_NOFOLLOW` on EVERY component (`open(2)`'s flag covers only the last),
/// refusing an ancestor that is not root-owned or that group or other can
/// write: whoever can write a directory on the path can put an opt-in there,
/// so "owned by euid" alone let the gated uid opt itself in. A root-owned
/// sticky directory (`/tmp`) is accepted above the parent - its other users
/// cannot rename the root-owned entry below it - but never as the parent,
/// where any user could create the file.
fn open_root_held(path: &Path) -> Result<std::fs::File, String> {
    use std::ffi::{CString, OsStr};
    use std::os::fd::{AsRawFd as _, FromRawFd as _};
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::MetadataExt as _;
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(p) => parts.push(p),
            _ => return Err(format!("{} is not a plain absolute path", path.display())),
        }
    }
    let Some((last, dirs)) = parts.split_last().filter(|_| path.is_absolute()) else {
        return Err(format!("{} is not a plain absolute path", path.display()));
    };
    let open_at = |dir: Option<&std::fs::File>, name: &OsStr, flags: i32, at: &Path| {
        let c = CString::new(name.as_bytes()).map_err(|_| format!("{}: NUL", at.display()))?;
        let dirfd = dir.map_or(libc::AT_FDCWD, |d| d.as_raw_fd());
        // SAFETY: `c` is NUL-terminated and `dirfd` is AT_FDCWD or an open fd.
        let fd = unsafe { libc::openat(dirfd, c.as_ptr(), flags | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(format!(
                "{}: {}",
                at.display(),
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: `fd` was just opened and nothing else owns it.
        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    };
    let held = |d: &std::fs::File, at: &Path, parent: bool| -> Result<(), String> {
        let m = d.metadata().map_err(|e| format!("{}: {e}", at.display()))?;
        if m.uid() != 0 {
            return Err(format!(
                "{} is owned by uid {}, not root: that user can replace what it holds",
                at.display(),
                m.uid()
            ));
        }
        let others_write = m.mode() & 0o022 != 0;
        let sticky = m.mode() & 0o1000 != 0;
        if others_write && (parent || !sticky) {
            return Err(format!(
                "{} has mode {:04o}: a user other than root can put a file there",
                at.display(),
                m.mode() & 0o7777
            ));
        }
        Ok(())
    };
    let dir_flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW;
    let mut at = PathBuf::from("/");
    let mut dir = open_at(None, OsStr::new("/"), dir_flags, &at)?;
    held(&dir, &at, dirs.is_empty())?;
    for (i, name) in dirs.iter().enumerate() {
        at.push(name);
        dir = open_at(Some(&dir), name, dir_flags, &at)?;
        held(&dir, &at, i + 1 == dirs.len())?;
    }
    open_at(
        Some(&dir),
        last,
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        path,
    )
}

/// Phase A (§5.3 steps 1-8). `axon` is the resolved `--axon` path; nothing
/// is spawned here.
pub fn phase_a(cli: &Cli, axon: &Path, hard_grace_ms: u64) -> Result<PhaseA, String> {
    // 0. the run is in the initial user namespace (security re-review 2
    // N7) and the host opted in (security F1), before anything else is read.
    initial_user_namespace()?;
    // SAFETY: geteuid has no preconditions.
    let euid = unsafe { libc::geteuid() };
    let decision = host_opt_in(Path::new(OPT_IN_PATH), euid)?;
    // 1. flag allowlist, env selectors, generator kind (E9). The selectors
    // are refused in the real environment, and no `.env` is ever loaded
    // (security F2): one in the workspace or named by `AXON_DOTENV` would
    // set what this check never saw.
    axon_ai::forbid_dotenv().map_err(|why| format!("--unattended runs without a .env: {why}"))?;
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
    let ledger = need(&cli.run_ledger, "--run-ledger")?;
    let seal_path = need(&cli.run_record_key, "--run-record-key")?;
    let workspace = std::fs::canonicalize(&cli.workspace)
        .map_err(|e| format!("--workspace {}: {e}", cli.workspace.display()))?;
    let store = std::fs::canonicalize(&store)
        .map_err(|e| format!("--spent-store {}: {e}", store.display()))?;
    let ledger = std::fs::canonicalize(&ledger)
        .map_err(|e| format!("--run-ledger {}: {e}", ledger.display()))?;
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
    // Security re-review 2 N1: the envelope pins the host, not the scheme;
    // a plain-HTTP base URL on the pinned host sent the key in the clear.
    if !axon_ai::endpoint_is_https(axon_ai::provider()) {
        return Err(
            "the AI endpoint (AXON_AI_BASE_URL / ANTHROPIC_BASE_URL) is not https://: a \
             plaintext call would carry the request and its API key in the clear"
                .into(),
        );
    }
    // Security F4 / F6: every fs pin is absolute, and none reaches the spent
    // store or a key file (the ledger is checked with the workspace below).
    un::absolute_fs_pins(&e.scopes.fs_read, &e.scopes.fs_write)?;
    for (what, p) in [
        ("the spent store", &store),
        ("the verify key", &key_path),
        ("the run-record seal key", &seal_path),
    ] {
        let p = std::fs::canonicalize(p).map_err(|err| format!("{what} {}: {err}", p.display()))?;
        un::outside_fs_pins(what, &p, &e.scopes.fs_read, &e.scopes.fs_write)?;
    }
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
    // Security re-review N4: every pin, inside the workspace or not.
    un::no_symlink_in_fs_pins(&e.scopes.fs_read, &e.scopes.fs_write)?;
    if store.starts_with(&workspace) {
        return Err(format!(
            "the spent store {} lies inside the workspace: the repaired code could reach it",
            store.display()
        ));
    }
    // R6 §4: the run ledger is checked like the spent store, disjoint from
    // the workspace and every write scope (canonical path, `outside_fs_pins`
    // as for the store and keys), and is the one the envelope names.
    if ledger.starts_with(&workspace) {
        return Err(format!(
            "the run ledger {} lies inside the workspace: the repaired code could reach it",
            ledger.display()
        ));
    }
    un::outside_fs_pins("the run ledger", &ledger, &[], &e.scopes.fs_write)?;
    let ledger_id = rl::ledger_id(&ledger, euid)?;
    if ledger_id != e.run_ledger_id {
        return Err(format!(
            "run ledger {} has id {ledger_id}, but the envelope records into {}",
            ledger.display(),
            e.run_ledger_id
        ));
    }
    let seal = rl::SealKey::load(&seal_path, euid, std::slice::from_ref(&workspace))?;
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
    // 7. the check digest over the `--axon` bytes, before any spawn of it:
    // the file `--axon` resolves to (a bare name through PATH, as the spawn
    // would), so the bytes hashed are the bytes that run (security F7).
    let checker_exe = crate::runner::resolve_executable(axon)
        .map_err(|why| format!("--axon {}: {why}", axon.display()))?;
    let checker_exe_sha256 = sha256_file(&checker_exe)?;
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
        ledger,
        seal,
        checker_exe,
        checker_exe_sha256,
        lock,
        euid,
        decision,
    })
}

/// Phase B (§5.3 steps 9-11): after the pinned interpreter's static `axon
/// caps`, before any child that executes workspace code. `fence` is the
/// pristine fence `derive_fence` built; `caps` the whole-file answer it was
/// built from. `wall` is asked once per child for its bound (E3, security
/// review F8: min(check_timeout, the time left)).
pub fn phase_b(
    a: &PhaseA,
    axon: &Path,
    fence: &crate::fence::Fence,
    caps: &crate::fence::Caps,
    wall: &dyn Fn() -> std::time::Duration,
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
    let grant = target_grant(axon, &a.workspace, &a.file, &e.task.symbol, wall())?;
    if grant != e.capabilities.target_grant {
        return Err(format!(
            "the static grant of `{}` ({grant}) is not the envelope's ({})",
            e.task.symbol, e.capabilities.target_grant
        ));
    }
    let (imports, modules) = imports_closure(axon, &a.workspace, &a.file, wall())?;
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
    fs_write_disjoint(&e.scopes.fs_write, &a.workspace, &a.file, &modules)?;
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

/// §5.4: the check child's FS_WRITE pins are disjoint from the task file, the
/// modules of its import closure (`imports`, canonical paths; security review
/// F5) and the policy paths (a pin is an absolute path prefix).
pub fn fs_write_disjoint(
    fs_write: &[String],
    workspace: &Path,
    file: &str,
    imports: &[PathBuf],
) -> Result<(), String> {
    let task = workspace.join(file);
    for pin in fs_write {
        let p = Path::new(pin);
        if task.starts_with(p) || p.starts_with(&task) {
            return Err(format!(
                "the check's fs_write pin {pin} covers the task file {file}"
            ));
        }
        // The module paths are canonical; the pin is compared as given and
        // resolved (a symlinked pin directory reaches the same files).
        let resolved = std::fs::canonicalize(p).ok();
        let covers =
            |m: &PathBuf| m.starts_with(p) || resolved.as_ref().is_some_and(|r| m.starts_with(r));
        if let Some(m) = imports.iter().find(|m| covers(m)) {
            return Err(format!(
                "the check's fs_write pin {pin} covers the imported module {}",
                m.display()
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

/// §5.4 / security review F5: what is re-checked before EVERY check dispatch
/// of an unattended run (the walk's included, not only the witness run's):
/// the task file outside the target's body is the pristine file, and the
/// import closure is still the envelope's. A check child that rewrote an
/// imported module or the code around the target would otherwise have every
/// later verdict graded against its own edit.
pub struct DispatchGuard {
    axon: PathBuf,
    workspace: PathBuf,
    file: String,
    symbol: String,
    pristine: Vec<u8>,
    /// The target's body in `pristine` (`runner::symbol_body`): a candidate
    /// replaces exactly this span, so everything around it is the pristine
    /// bytes.
    body: (usize, usize),
    imports_digest: String,
    wall: std::time::Duration,
}

impl DispatchGuard {
    pub fn new(a: &PhaseA, axon: &Path, wall: std::time::Duration) -> Result<Self, String> {
        let symbol = a.verified.envelope.task.symbol.clone();
        let body = std::str::from_utf8(&a.pristine)
            .ok()
            .and_then(|src| crate::runner::symbol_body(src, &symbol))
            .ok_or_else(|| format!("`{symbol}` has no body in the pristine {}", a.file))?;
        Ok(DispatchGuard {
            axon: axon.to_path_buf(),
            workspace: a.workspace.clone(),
            file: a.file.clone(),
            symbol,
            pristine: a.pristine.clone(),
            body,
            imports_digest: a.verified.envelope.task.imports_digest.clone(),
            wall,
        })
    }

    /// `Ok` when the next check may be dispatched; otherwise why not.
    pub fn check(&self) -> Result<(), String> {
        let now = std::fs::read(self.workspace.join(&self.file))
            .map_err(|e| format!("{}: {e}", self.file))?;
        let (start, end) = self.body;
        let (head, tail) = (&self.pristine[..start], &self.pristine[end..]);
        if now.len() < head.len() + tail.len() || !now.starts_with(head) || !now.ends_with(tail) {
            return Err(format!(
                "{} changed outside `{}` before a check dispatch (sha256 {} -> {})",
                self.file,
                self.symbol,
                crate::label::sha256_hex(&self.pristine),
                crate::label::sha256_hex(&now)
            ));
        }
        let imports = imports_digest(&self.axon, &self.workspace, &self.file, self.wall)?;
        if imports != self.imports_digest {
            return Err(format!(
                "the import closure changed before a check dispatch ({} -> {imports})",
                self.imports_digest
            ));
        }
        Ok(())
    }
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
        "host_opt_in": {"path": OPT_IN_PATH, "decision": a.decision},
        "enforcement": b.rows.iter().map(Row::to_json).collect::<Vec<_>>(),
    })
}

/// R6 §3 `pinned_tree_before` / `_after`: every file under the run's write
/// prefixes (workspace-relative) and its `scopes.fs_write` pins (absolute),
/// keyed like the E5 journal's paths (workspace-relative where possible),
/// valued by the hex sha256 of its bytes. A symlink is never followed: its
/// value is the sha256 of `link:<target>`. A check child's write shows as a
/// difference that the journal does not explain.
fn tree(workspace: &Path, roots: &[PathBuf]) -> Result<rl::Tree, String> {
    let mut out = rl::Tree::new();
    let mut todo: Vec<PathBuf> = roots.to_vec();
    while let Some(p) = todo.pop() {
        let key = rel(workspace, &p);
        let err = |e: std::io::Error| format!("pinned tree: {key}: {e}");
        let value = match std::fs::symlink_metadata(&p) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(err(e)),
            Ok(m) if m.file_type().is_symlink() => {
                let t = std::fs::read_link(&p).map_err(err)?;
                crate::label::sha256_hex(format!("link:{}", t.to_string_lossy()).as_bytes())
            }
            Ok(m) if m.is_dir() => {
                for e in std::fs::read_dir(&p).map_err(err)? {
                    todo.push(e.map_err(err)?.path());
                }
                continue;
            }
            Ok(_) => crate::label::sha256_hex(&std::fs::read(&p).map_err(err)?),
        };
        out.insert(key, value);
    }
    Ok(out)
}

/// `p` relative to the workspace when under it, else as given.
fn rel(workspace: &Path, p: &Path) -> String {
    p.strip_prefix(workspace)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

/// What the terminal record needs from the run, set once its started record
/// is written. Taken exactly once: by the main thread's exit or by the E3
/// watchdog, whichever comes first.
struct Recorder {
    dir: PathBuf,
    euid: u32,
    key: rl::SealKey,
    verified: VerifiedEnvelope,
    workspace: PathBuf,
    roots: Vec<PathBuf>,
    budget: env::Budget,
}

static RECORDER: std::sync::Mutex<Option<Recorder>> = std::sync::Mutex::new(None);

/// R6 §3 `started`: after consume, before any untrusted code runs. A failed
/// append marks the spent envelope `.unrecorded` and refuses the run (the
/// caller exits 29; nothing ran).
pub fn record_started(a: &PhaseA, b: &PhaseB) -> Result<(), String> {
    let e = &a.verified.envelope;
    let roots: Vec<PathBuf> = a
        .effective
        .write_prefixes
        .iter()
        .map(|p| a.workspace.join(p))
        .chain(e.scopes.fs_write.iter().map(PathBuf::from))
        .collect();
    let f = &a.effective;
    let budget = env::Budget {
        max_proposals: f.max_proposals,
        deadline_ms: f.deadline_ms,
        check_timeout_ms: f.check_timeout_ms,
        candidates: f.candidates,
        max_steps: f.max_steps,
    };
    let appended = tree(&a.workspace, &roots).and_then(|before| {
        let body = rl::Body::Started(rl::Started {
            enforcement: serde_json::Value::Array(b.rows.iter().map(Row::to_json).collect()),
            pinned_tree_before: before,
            budget: budget.clone(),
        });
        rl::append(&a.ledger, a.euid, &a.seal, &a.verified, body, now_unix())
    });
    match appended {
        Ok(_) => {
            *RECORDER.lock().unwrap_or_else(|p| p.into_inner()) = Some(Recorder {
                dir: a.ledger.clone(),
                euid: a.euid,
                key: a.seal.clone(),
                verified: a.verified.clone(),
                workspace: a.workspace.clone(),
                roots,
                budget,
            });
            Ok(())
        }
        Err(why) => Err(
            match rl::mark_unrecorded(&a.store, a.euid, &e.envelope_id, &why, now_unix()) {
                Ok(()) => format!("the run ledger's started record could not be written: {why}"),
                Err(m) => format!(
                    "the run ledger's started record could not be written: {why}; \
                     and the spent store could not mark it: {m}"
                ),
            },
        ),
    }
}

/// How a run that wrote a started record ended.
pub struct Ending {
    pub exit_code: i32,
    pub outcome: String,
    pub stopped_by: Option<String>,
    /// `(path, graded sha256)`: exit 0 and 27 only.
    pub accepted: Option<(String, String)>,
    pub episode_digest: Option<String>,
}

/// R6 §3 `terminal`, over the E5 journal `writes`. A no-op when no started
/// record was written (attended runs; refusals before consume) or the
/// terminal record already was. A failed append is one stderr line; the
/// exit code and verdict are unchanged.
pub fn record_terminal(end: Ending, writes: &[crate::cas::WriteRecord]) {
    let taken = RECORDER.lock().unwrap_or_else(|p| p.into_inner()).take();
    let Some(r) = taken else {
        return;
    };
    let writes = writes
        .iter()
        .map(|w| rl::WriteEntry {
            site: w.site.label().to_string(),
            path: rel(&r.workspace, &w.path),
            before_sha256: w.before_sha256.clone(),
            after_sha256: w.after_sha256.clone(),
        })
        .collect();
    let appended = tree(&r.workspace, &r.roots).and_then(|after| {
        let body = rl::Body::Terminal(rl::Terminal {
            outcome: end.outcome,
            exit_code: i64::from(end.exit_code),
            stopped_by: end.stopped_by,
            accepted: end
                .accepted
                .map(|(path, sha256)| rl::Accepted { path, sha256 }),
            accepted_refusal: None,
            writes,
            pinned_tree_after: after,
            episode_digest: end.episode_digest,
            budget: r.budget,
        });
        rl::append(&r.dir, r.euid, &r.key, &r.verified, body, now_unix())
    });
    if let Err(e) = appended {
        eprintln!(
            "run ledger {}: the terminal record could not be written ({e}); \
             the verdict and exit code are unaffected",
            r.dir.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pinned_tree_names_every_file_under_a_root() {
        let d = std::env::temp_dir().join(format!("cortex-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("w/sub")).unwrap();
        std::fs::write(d.join("w/sub/a"), b"1").unwrap();
        let t = || tree(&d, &[d.join("w"), d.join("gone")]).unwrap();
        let before = t();
        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            ["w/sub/a"],
            "ATTACK: R6 a file under a pinned root is not in the pinned tree"
        );
        std::fs::write(d.join("w/sub/b"), b"").unwrap();
        let added = t();
        assert_ne!(
            before, added,
            "ATTACK: R6 a file a check child created is not in the pinned tree"
        );
        std::fs::write(d.join("w/sub/a"), b"2").unwrap();
        assert_ne!(
            added,
            t(),
            "ATTACK: R6 a file a check child rewrote is not in the pinned tree"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

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
        assert!(fs_write_disjoint(&["/w/out".into()], w, "f.ax", &[]).is_ok());
        assert!(
            fs_write_disjoint(&["/w".into()], w, "f.ax", &[]).is_err(),
            "ATTACK: NH-R1 a check fs_write pin covering the task file was admitted"
        );
        assert!(fs_write_disjoint(&["/w/scripts".into()], w, "f.ax", &[]).is_err());
    }

    /// Security review F5: a pin over a module of the task's import closure
    /// refuses, as given and through a symlinked pin directory.
    #[test]
    fn an_fs_write_pin_over_an_imported_module_refuses() {
        let d = std::env::temp_dir().join(format!("cortex-auth-f5-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("lib")).unwrap();
        std::fs::write(
            d.join("lib/helpers.ax"),
            "fn inc(n: i64) -> i64 { n + 1 }\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(d.join("lib"), d.join("alias")).unwrap();
        let d = std::fs::canonicalize(&d).unwrap();
        let module = d.join("lib/helpers.ax");
        let w = Path::new("/w");
        let pin = |p: &Path| vec![p.to_string_lossy().into_owned()];
        let modules = std::slice::from_ref(&module);
        assert!(fs_write_disjoint(&pin(&d.join("out")), w, "f.ax", modules).is_ok());
        for p in [d.join("lib"), d.clone(), module.clone(), d.join("alias")] {
            let got = fs_write_disjoint(&pin(&p), w, "f.ax", modules);
            assert!(
                got.as_ref()
                    .is_err_and(|why| why.contains("covers the imported module")),
                "ATTACK: NH-R1 security F5 an fs_write pin over an imported module was admitted: \
                 {} {got:?}",
                p.display()
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Security re-review 2 N7: only the initial user namespace's identity
    /// map admits; a namespace's map, several lines, an empty or an
    /// unreadable map refuse.
    #[test]
    fn only_the_initial_user_namespace_map_admits() {
        assert!(is_initial_id_map("         0          0 4294967295\n"));
        assert!(is_initial_id_map("0 0 4294967295"));
        for bad in [
            "0 1000 1\n",
            "0 0 1\n",
            "0 0 4294967295\n1 1 1\n",
            "0 0 4294967295\n0 0 4294967295\n",
            "",
            "\n",
            "0 0 4294967295 7\n",
        ] {
            assert!(
                !is_initial_id_map(bad),
                "ATTACK: NH-R1 security N7 the id map {bad:?} was admitted as the initial user \
                 namespace's"
            );
        }
        assert!(
            !id_map_file_is_initial(Path::new("/nonexistent/cortex-n7/uid_map")),
            "ATTACK: NH-R1 security N7 an unreadable id map was admitted"
        );
    }

    /// Security re-review N2: the opt-in file's directories are part of the
    /// gate. A directory another user can write (or does own), or one
    /// reached through a symlink, lets that user put an opt-in there, so each
    /// refuses; a root-owned 0755 directory admits. The path is injected
    /// here; `phase_a` only ever passes [`OPT_IN_PATH`].
    #[test]
    fn an_opt_in_in_a_directory_another_user_controls_refuses() {
        use std::os::unix::fs::PermissionsExt as _;
        // SAFETY: geteuid has no preconditions.
        let euid = unsafe { libc::geteuid() };
        let d = std::env::temp_dir().join(format!("cortex-auth-n2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mode = |p: &Path, m: u32| {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap()
        };
        let opt_in = |dir: &Path| {
            std::fs::create_dir_all(dir).unwrap();
            mode(dir, 0o755);
            let f = dir.join("unattended-repair");
            std::fs::write(&f, "decision: governance/decisions/x.md\n").unwrap();
            mode(&f, 0o600);
            f
        };
        std::fs::create_dir_all(&d).unwrap();
        mode(&d, 0o755);
        let good = opt_in(&d.join("good"));
        assert_eq!(
            host_opt_in(&good, euid).as_deref(),
            Ok("governance/decisions/x.md")
        );
        let open = opt_in(&d.join("open"));
        mode(&d.join("open"), 0o777);
        let sticky = opt_in(&d.join("sticky"));
        mode(&d.join("sticky"), 0o1777);
        let owned = opt_in(&d.join("owned"));
        std::os::unix::fs::chown(d.join("owned"), Some(1), None).unwrap();
        let deep = opt_in(&d.join("upper/inner"));
        mode(&d.join("upper"), 0o777);
        opt_in(&d.join("real"));
        std::os::unix::fs::symlink(d.join("real"), d.join("link")).unwrap();
        let linked = d.join("link/unattended-repair");
        for (what, p) in [
            ("a parent other users can write", &open),
            ("a sticky parent other users can create in", &sticky),
            ("a parent owned by another uid", &owned),
            ("an ancestor other users can write", &deep),
            ("a parent reached through a symlink", &linked),
        ] {
            let got = host_opt_in(p, euid);
            assert!(
                got.as_ref()
                    .is_err_and(|why| why.contains("has not opted in")),
                "ATTACK: NH-R1 security N2 an opt-in in {what} was admitted: {got:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
