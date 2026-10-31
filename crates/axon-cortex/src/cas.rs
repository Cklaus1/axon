//! NH-R1 E5 (O-S1-13): the ONE compare-and-swap helper every workspace write
//! and delete of `cortex repair` goes through.
//!
//! A plain `std::fs::write` overwrote whatever was there: an edit another
//! actor made while a run was in flight was lost silently. Every write here:
//!
//! 1. writes a temp file in the target's directory (the target's mode kept);
//! 2. re-hashes the target against the last digest this process wrote or
//!    read for it ([`Ledger::observe`]); a mismatch writes nothing;
//! 3. EXCHANGES the temp file and the target (`renameat2(RENAME_EXCHANGE)`;
//!    `RENAME_NOREPLACE` when the target is expected absent), so the target's
//!    old inode is not unlinked but moved to the temp name;
//! 4. re-hashes that displaced inode against step 2's digest. POSIX has no
//!    atomic compare-and-rename, so a writer can still change the target
//!    between 2 and 3; such an edit is now NEVER lost and never silent: the
//!    displaced file is kept beside the target (`.cortex-cas-displaced-*`)
//!    and the write is reported ([`CasError::Displaced`]) with both digests
//!    and the kept path - the caller blocks (exit 22). (Security review F3:
//!    the plain rename and the re-hash of the renamed file in steps 4-5 of
//!    the earlier helper missed exactly this window.)
//! 5. checks that the target is the inode it placed, `(dev, ino)`, and
//!    re-hashes it against the bytes it wrote: a write AFTER the exchange is
//!    reported with both digests, never claimed as written.
//!
//! What remains: a writer that keeps an fd open on the old inode and writes
//! through it after step 4's re-hash writes into an inode no longer linked
//! anywhere (step 4 matched, so it was unlinked): that write is lost
//! unreported; only a writer through the path is covered. Every
//! read here opens with `O_NOFOLLOW | O_NONBLOCK` and refuses anything but a
//! regular file (F8), so a FIFO or a symlink in the target's place cannot
//! stall a read under the run's write lock.
//!
//! A delete renames the target to a temp name first, re-hashes what it moved
//! and only then unlinks it; a mismatch renames it back (F9).
//!
//! Every write and delete is journalled (`{site, path, before, after}`, hex
//! sha256), the record R6's run ledger carries. One process-wide [`Ledger`]
//! behind one mutex: it is also the run's write lock, which the NH-R1 E3
//! watchdog takes before its restore, so no write of the main thread can
//! interleave with it.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};

/// The nine write/delete sites of `cortex repair` (NH-R1 §6 E5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    /// A candidate body spliced into the file.
    Apply,
    /// A patch that broke the build, reverted.
    CompileRevert,
    /// The visible copy (hidden check cut out), created and removed.
    Visible,
    /// A rejected candidate undone to the bytes it started from.
    Restore,
    /// The found bytes put back before the next candidate.
    BetweenAttempts,
    /// The found bytes put back by a run that keeps no patch.
    ExitRestore,
    /// An exit 27's adjudicated bytes put back.
    ExitPutBack,
    /// R5's settle: the found bytes put back when the accepted state is refused.
    Settle,
    /// A stale visible copy of a dead run, deleted.
    Sweep,
}

impl Site {
    /// The journal's name for the site.
    pub fn label(self) -> &'static str {
        match self {
            Site::Apply => "apply",
            Site::CompileRevert => "compile_revert",
            Site::Visible => "visible",
            Site::Restore => "restore",
            Site::BetweenAttempts => "between_attempts",
            Site::ExitRestore => "exit_restore",
            Site::ExitPutBack => "exit_put_back",
            Site::Settle => "settle",
            Site::Sweep => "sweep",
        }
    }
}

/// One journalled write or delete. `None`: the file did not exist (before a
/// create, after a delete).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteRecord {
    pub site: Site,
    pub path: PathBuf,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
}

/// Why a write or delete did not happen, or happened and was then lost.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CasError {
    /// Nothing was read or written at `path` by this process: there is no
    /// digest to compare against, so nothing is written.
    Unknown { path: PathBuf },
    /// Step 2: the target is not what this process last wrote or read.
    Foreign {
        path: PathBuf,
        expected: Option<String>,
        found: Option<String>,
    },
    /// Step 5: the target is no longer the inode placed.
    Replaced { path: PathBuf },
    /// Step 5: the target does not hold the bytes just written.
    Lost {
        path: PathBuf,
        wrote: String,
        found: Option<String>,
    },
    /// Step 4 (security review F3): the inode the exchange displaced is not
    /// what step 2 hashed, so a writer got in between. This run's bytes are
    /// in place and journalled; the displaced file is kept at `kept`, never
    /// deleted. For a delete (F9): the moved file could not be renamed back
    /// and is kept at `kept`.
    Displaced {
        path: PathBuf,
        expected: Option<String>,
        displaced: Option<String>,
        kept: PathBuf,
    },
    /// The filesystem refused (the io error's text).
    Io(String),
}

fn shown(d: &Option<String>) -> &str {
    d.as_deref().unwrap_or("(absent)")
}

impl std::fmt::Display for CasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CasError::Unknown { path } => write!(
                f,
                "{}: no digest of it was read or written by this run; not written",
                path.display()
            ),
            CasError::Foreign {
                path,
                expected,
                found,
            } => write!(
                f,
                "{} changed by another writer (expected sha256 {}, found sha256 {}); not written",
                path.display(),
                shown(expected),
                shown(found)
            ),
            CasError::Replaced { path } => write!(
                f,
                "{} was replaced by another writer right after this run's write",
                path.display()
            ),
            CasError::Lost { path, wrote, found } => write!(
                f,
                "{} was overwritten during this run's write: wrote sha256 {wrote}, found \
                 sha256 {}",
                path.display(),
                shown(found)
            ),
            CasError::Displaced {
                path,
                expected,
                displaced,
                kept,
            } => write!(
                f,
                "{} was edited by another writer during this run's write (expected sha256 {}, \
                 the edited file holds sha256 {}); the edited file is kept at {}",
                path.display(),
                shown(expected),
                shown(displaced),
                kept.display()
            ),
            CasError::Io(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for CasError {}

/// The process's last-known digest per path and its write journal.
#[derive(Default, Debug)]
pub struct Ledger {
    known: HashMap<PathBuf, Option<String>>,
    journal: Vec<WriteRecord>,
}

static LEDGER: LazyLock<Mutex<Ledger>> = LazyLock::new(Default::default);

/// Set by the E3 watchdog when the deadline passes: from then on no thread
/// but the watchdog takes the write lock.
static FROZEN: AtomicBool = AtomicBool::new(false);

/// The run's write lock and ledger. A panic while it was held does not lose
/// the ledger: the journal is still the record of what was written.
///
/// Once the E3 watchdog has fired ([`freeze`]) this never returns: the
/// calling thread would write after the hard deadline, and an unfair mutex
/// re-taken in a loop could starve the watchdog's restore past its grace.
pub fn lock() -> MutexGuard<'static, Ledger> {
    if FROZEN.load(Ordering::SeqCst) {
        loop {
            std::thread::park();
        }
    }
    LEDGER.lock().unwrap_or_else(|p| p.into_inner())
}

/// NH-R1 E3: stop every later [`lock`] and take the lock for the watchdog,
/// waiting only for the write in progress (if any) to finish.
pub fn freeze() -> MutexGuard<'static, Ledger> {
    FROZEN.store(true, Ordering::SeqCst);
    LEDGER.lock().unwrap_or_else(|p| p.into_inner())
}

/// [`Ledger::observe`] under the lock.
pub fn observe(path: &Path, bytes: &[u8]) {
    lock().observe(path, bytes)
}

/// [`Ledger::write`] under the lock.
pub fn write(site: Site, path: &Path, bytes: &[u8]) -> Result<(), CasError> {
    lock().write(site, path, bytes)
}

/// [`Ledger::create`] under the lock.
pub fn create(site: Site, path: &Path, bytes: &[u8]) -> Result<(), CasError> {
    lock().create(site, path, bytes)
}

/// [`Ledger::remove`] under the lock.
pub fn remove(site: Site, path: &Path) -> Result<(), CasError> {
    lock().remove(site, path)
}

/// A copy of the journal so far.
pub fn journal() -> Vec<WriteRecord> {
    lock().journal.clone()
}

fn sha(bytes: &[u8]) -> String {
    crate::label::sha256_hex(bytes)
}

// A test's writer inside the residual window (between the re-hash and the
// rename of a write; between the re-hash and the unlink of a delete).
#[cfg(test)]
thread_local! {
    static IN_WINDOW: std::cell::RefCell<Option<Box<dyn FnMut(&Path)>>> =
        std::cell::RefCell::new(None);
}
#[cfg(test)]
fn in_window(path: &Path) {
    IN_WINDOW.with(|h| {
        if let Some(f) = h.borrow_mut().as_mut() {
            f(path)
        }
    })
}
#[cfg(not(test))]
fn in_window(_path: &Path) {}

/// The bytes of the regular file at `path`, read without following a
/// symlink and without blocking on a FIFO (security review F8: the E3
/// watchdog's restore reads under the run's write lock, and a FIFO or a
/// link to `/dev/zero` in the target's place stalled it past its grace).
fn read_regular(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let m = f.metadata()?;
    if !m.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    let mut b = Vec::with_capacity(m.len() as usize);
    f.read_to_end(&mut b)?;
    Ok(b)
}

/// The digest of the file at `path` now; `None` when it does not exist.
fn digest_now(path: &Path) -> Result<Option<String>, CasError> {
    match read_regular(path) {
        Ok(b) => Ok(Some(sha(&b))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CasError::Io(format!("{}: {e}", path.display()))),
    }
}

/// `renameat2(from, to, flags)` (`RENAME_EXCHANGE` / `RENAME_NOREPLACE`; a
/// filesystem without them refuses, so the write fails closed).
fn rename2(from: &Path, to: &Path, flags: libc::c_uint) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let a = std::ffi::CString::new(from.as_os_str().as_bytes())?;
    let b = std::ffi::CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both are valid NUL-terminated paths; AT_FDCWD resolves them as given.
    let rc = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            flags,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// A fresh name beside `path` to keep a displaced file under.
fn kept_name(dir: &Path, name: &std::ffi::OsStr) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    dir.join(format!(
        ".cortex-cas-displaced-{}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst),
        name.to_string_lossy()
    ))
}

#[cfg(unix)]
fn file_id(m: &std::fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (m.dev(), m.ino())
}

impl Ledger {
    /// Record `bytes` as what this process read at `path`: the digest the
    /// next write or delete there is compared against.
    pub fn observe(&mut self, path: &Path, bytes: &[u8]) {
        self.known.insert(path.to_path_buf(), Some(sha(bytes)));
    }

    /// Every write and delete so far, in order.
    pub fn journal(&self) -> &[WriteRecord] {
        &self.journal
    }

    /// Replace the file at `path` with `bytes`, steps 1-5 of the module doc.
    pub fn write(&mut self, site: Site, path: &Path, bytes: &[u8]) -> Result<(), CasError> {
        let expected = self
            .known
            .get(path)
            .cloned()
            .ok_or_else(|| CasError::Unknown {
                path: path.to_path_buf(),
            })?;
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(CasError::Io(format!("{} names no file", path.display())));
        };
        let tmp = dir.join(format!(
            ".cortex-cas-{}-{}",
            std::process::id(),
            name.to_string_lossy()
        ));
        // 1. The temp file, beside the target so the rename stays on one
        // filesystem. A name of this process left by an earlier failed write
        // is this process's own.
        let _ = std::fs::remove_file(&tmp);
        let written = (|| -> std::io::Result<(u64, u64)> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            f.write_all(bytes)?;
            if let Ok(m) = std::fs::metadata(path) {
                f.set_permissions(m.permissions())?;
            }
            Ok(file_id(&f.metadata()?))
        })();
        let tmp_id = match written {
            Ok(id) => id,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(CasError::Io(e.to_string()));
            }
        };
        // 2. The target is still what this run last wrote or read.
        let found = match digest_now(path) {
            Ok(d) => d,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        };
        if found != expected {
            let _ = std::fs::remove_file(&tmp);
            return Err(CasError::Foreign {
                path: path.to_path_buf(),
                expected,
                found,
            });
        }
        in_window(path);
        // 3. The exchange: the target's old inode moves to the temp name.
        // Expected absent: an exclusive rename, so nothing created meanwhile
        // is clobbered.
        let flags = if found.is_some() {
            libc::RENAME_EXCHANGE
        } else {
            libc::RENAME_NOREPLACE
        };
        if let Err(e) = rename2(&tmp, path, flags) {
            let _ = std::fs::remove_file(&tmp);
            return Err(match e.raw_os_error() {
                Some(libc::ENOENT) | Some(libc::EEXIST) => CasError::Foreign {
                    path: path.to_path_buf(),
                    expected,
                    found: digest_now(path).unwrap_or(None),
                },
                _ => CasError::Io(format!("{}: {e}", path.display())),
            });
        }
        let after = sha(bytes);
        self.known.insert(path.to_path_buf(), Some(after.clone()));
        self.journal.push(WriteRecord {
            site,
            path: path.to_path_buf(),
            before_sha256: found.clone(),
            after_sha256: Some(after.clone()),
        });
        // 4. The displaced inode is what step 2 hashed, or it is kept.
        if found.is_some() {
            let displaced = read_regular(&tmp).ok().map(|b| sha(&b));
            if displaced != found {
                let kept = kept_name(dir, name);
                let kept = match rename2(&tmp, &kept, libc::RENAME_NOREPLACE) {
                    Ok(()) => kept,
                    Err(_) => tmp,
                };
                return Err(CasError::Displaced {
                    path: path.to_path_buf(),
                    expected: found,
                    displaced,
                    kept,
                });
            }
            let _ = std::fs::remove_file(&tmp);
        }
        self.verify_placed(path, tmp_id, after)
    }

    /// Create `path`, which must not exist, holding `bytes` (the visible
    /// copy). Creation is exclusive, so step 2's "expected absent" and step
    /// 3 are one atomic open; steps 4 and 5 as for [`Ledger::write`].
    pub fn create(&mut self, site: Site, path: &Path, bytes: &[u8]) -> Result<(), CasError> {
        let mut f = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(CasError::Foreign {
                    path: path.to_path_buf(),
                    expected: None,
                    found: digest_now(path).unwrap_or(None),
                })
            }
            Err(e) => return Err(CasError::Io(e.to_string())),
        };
        let after = sha(bytes);
        self.known.insert(path.to_path_buf(), Some(after.clone()));
        self.journal.push(WriteRecord {
            site,
            path: path.to_path_buf(),
            before_sha256: None,
            after_sha256: Some(after.clone()),
        });
        let id = f
            .write_all(bytes)
            .and_then(|()| f.metadata())
            .map_err(|e| CasError::Io(e.to_string()))?;
        drop(f);
        self.verify_placed(path, file_id(&id), after)
    }

    /// Delete `path`, which must still be what this process last wrote or
    /// read. The target is re-hashed first; a mismatch deletes nothing. Then
    /// it is renamed to a temp name and what was moved is re-hashed before
    /// the unlink: an edit made in between is renamed back and reported,
    /// never deleted (security review F9).
    pub fn remove(&mut self, site: Site, path: &Path) -> Result<(), CasError> {
        let expected = self
            .known
            .get(path)
            .cloned()
            .ok_or_else(|| CasError::Unknown {
                path: path.to_path_buf(),
            })?;
        let found = digest_now(path)?;
        if found != expected {
            return Err(CasError::Foreign {
                path: path.to_path_buf(),
                expected,
                found,
            });
        }
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(CasError::Io(format!("{} names no file to delete", path.display())));
        };
        let tmp = dir.join(format!(
            ".cortex-cas-rm-{}-{}",
            std::process::id(),
            name.to_string_lossy()
        ));
        let _ = std::fs::remove_file(&tmp);
        in_window(path);
        rename2(path, &tmp, libc::RENAME_NOREPLACE)
            .map_err(|e| CasError::Io(format!("{}: {e}", path.display())))?;
        let moved = read_regular(&tmp).ok().map(|b| sha(&b));
        if moved != found {
            return Err(match rename2(&tmp, path, libc::RENAME_NOREPLACE) {
                Ok(()) => CasError::Foreign {
                    path: path.to_path_buf(),
                    expected,
                    found: moved,
                },
                Err(_) => CasError::Displaced {
                    path: path.to_path_buf(),
                    expected,
                    displaced: moved,
                    kept: tmp,
                },
            });
        }
        std::fs::remove_file(&tmp).map_err(|e| CasError::Io(format!("{}: {e}", path.display())))?;
        self.known.insert(path.to_path_buf(), None);
        self.journal.push(WriteRecord {
            site,
            path: path.to_path_buf(),
            before_sha256: found,
            after_sha256: None,
        });
        Ok(())
    }

    /// Steps 4 and 5: the inode placed is still there, holding `after`.
    fn verify_placed(&self, path: &Path, id: (u64, u64), after: String) -> Result<(), CasError> {
        match std::fs::symlink_metadata(path) {
            Ok(m) if file_id(&m) == id => {}
            _ => {
                return Err(CasError::Replaced {
                    path: path.to_path_buf(),
                })
            }
        }
        let found = digest_now(path)?;
        if found.as_deref() != Some(after.as_str()) {
            return Err(CasError::Lost {
                path: path.to_path_buf(),
                wrote: after,
                found,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cortex-cas-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_over_what_was_read_journals_before_and_after() {
        let d = dir("ok");
        let p = d.join("f.ax");
        std::fs::write(&p, b"one").unwrap();
        let mut l = Ledger::default();
        l.observe(&p, b"one");
        l.write(Site::Apply, &p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        assert_eq!(
            l.journal(),
            &[WriteRecord {
                site: Site::Apply,
                path: p.clone(),
                before_sha256: Some(sha(b"one")),
                after_sha256: Some(sha(b"two")),
            }]
        );
        // The next write compares against what this one wrote.
        l.write(Site::Restore, &p, b"one").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"one");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// ATTACK: E5 step 2 - a foreign edit after the last read is never overwritten.
    #[test]
    fn a_foreign_edit_is_not_overwritten_and_both_digests_are_named() {
        let d = dir("foreign");
        let p = d.join("f.ax");
        std::fs::write(&p, b"one").unwrap();
        let mut l = Ledger::default();
        l.observe(&p, b"one");
        std::fs::write(&p, b"theirs").unwrap();
        let got = l.write(Site::Restore, &p, b"mine");
        assert_eq!(
            std::fs::read(&p).unwrap(),
            b"theirs",
            "ATTACK: E5 a foreign edit was overwritten"
        );
        let shown = got.unwrap_err().to_string();
        assert!(
            shown.contains(&sha(b"one")) && shown.contains(&sha(b"theirs")),
            "{shown}"
        );
        assert!(l.journal().is_empty());
        let leftovers: Vec<_> = std::fs::read_dir(&d).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "the temp file is removed");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn nothing_read_nothing_written() {
        let d = dir("unknown");
        let p = d.join("f.ax");
        std::fs::write(&p, b"one").unwrap();
        let mut l = Ledger::default();
        assert!(
            matches!(
                l.write(Site::Apply, &p, b"two"),
                Err(CasError::Unknown { .. })
            ),
            "ATTACK: E5 a file this run never read was written"
        );
        assert_eq!(std::fs::read(&p).unwrap(), b"one");
        // Absent and never seen: not created either.
        let q = d.join("g.ax");
        assert!(
            matches!(
                l.write(Site::Apply, &q, b"two"),
                Err(CasError::Unknown { .. })
            ),
            "ATTACK: E5 a file this run never read was written"
        );
        assert!(!q.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn the_mode_survives_the_rename() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("mode");
        let p = d.join("f.ax");
        std::fs::write(&p, b"one").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640)).unwrap();
        let mut l = Ledger::default();
        l.observe(&p, b"one");
        l.write(Site::Apply, &p, b"two").unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640, "ATTACK: E5 the rename widened the file's mode");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// ATTACK: E5 step 5 - bytes changed after the rename are reported, not claimed.
    #[test]
    fn a_writer_inside_the_window_is_reported_with_both_digests() {
        let d = dir("lost");
        let p = d.join("f.ax");
        std::fs::write(&p, b"mine").unwrap();
        let l = Ledger::default();
        let id = file_id(&std::fs::metadata(&p).unwrap());
        // The rename happened; another writer then rewrote the same inode.
        std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&p)
            .unwrap()
            .write_all(b"theirs")
            .unwrap();
        let e = l.verify_placed(&p, id, sha(b"mine"));
        let shown = format!(
            "{e:?} {}",
            e.as_ref().err().map(|e| e.to_string()).unwrap_or_default()
        );
        assert!(
            matches!(e, Err(CasError::Lost { .. }))
                && shown.contains(&sha(b"mine"))
                && shown.contains(&sha(b"theirs")),
            "ATTACK: E5 bytes another writer put in the window were claimed as written: {shown}"
        );
        // A different inode in its place is named as a replacement.
        std::fs::remove_file(&p).unwrap();
        std::fs::write(&p, b"mine").unwrap();
        let moved = (id.0, id.1.wrapping_add(1));
        let same = file_id(&std::fs::metadata(&p).unwrap()) == moved;
        if !same {
            assert!(
                matches!(
                    l.verify_placed(&p, moved, sha(b"mine")),
                    Err(CasError::Replaced { .. })
                ),
                "ATTACK: E5 another inode in place of the write was claimed as written"
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// ATTACK: E5 create - a visible copy never clobbers an existing file.
    #[test]
    fn create_never_clobbers() {
        let d = dir("create");
        let p = d.join(".cortex-visible-1-f.ax");
        std::fs::write(&p, b"theirs").unwrap();
        let mut l = Ledger::default();
        assert!(
            matches!(
                l.create(Site::Visible, &p, b"mine"),
                Err(CasError::Foreign { .. })
            ) && std::fs::read(&p).unwrap() == b"theirs",
            "ATTACK: E5 a visible copy clobbered an existing file"
        );
        std::fs::remove_file(&p).unwrap();
        l.create(Site::Visible, &p, b"mine").unwrap();
        l.remove(Site::Visible, &p).unwrap();
        assert!(!p.exists());
        let sites: Vec<_> = l
            .journal()
            .iter()
            .map(|w| w.after_sha256.is_some())
            .collect();
        assert_eq!(sites, [true, false]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn remove_refuses_bytes_it_did_not_see() {
        let d = dir("remove");
        let p = d.join("f.ax");
        std::fs::write(&p, b"one").unwrap();
        let mut l = Ledger::default();
        l.observe(&p, b"one");
        std::fs::write(&p, b"theirs").unwrap();
        assert!(
            matches!(l.remove(Site::Sweep, &p), Err(CasError::Foreign { .. })) && p.exists(),
            "ATTACK: E5 a delete removed bytes this run never saw"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Another writer rewrites `path`'s inode in place inside the window.
    fn rewrite_in_window(bytes: &'static [u8]) {
        IN_WINDOW.with(|h| {
            *h.borrow_mut() = Some(Box::new(move |p: &Path| {
                std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(p)
                    .unwrap()
                    .write_all(bytes)
                    .unwrap();
            }))
        });
    }

    fn close_window() {
        IN_WINDOW.with(|h| *h.borrow_mut() = None);
    }

    /// Every file in `d` but `keep`, by contents.
    fn others(d: &Path, keep: &Path) -> Vec<Vec<u8>> {
        std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p != keep)
            .map(|p| std::fs::read(p).unwrap())
            .collect()
    }

    /// Security review F3: an edit between step 2's re-hash and the rename is
    /// kept (the displaced inode) and reported with both digests.
    #[test]
    fn a_writer_between_the_rehash_and_the_rename_is_kept_and_reported() {
        let d = dir("exchange");
        let p = d.join("f.ax");
        std::fs::write(&p, b"one").unwrap();
        let mut l = Ledger::default();
        l.observe(&p, b"one");
        rewrite_in_window(b"theirs");
        let got = l.write(Site::Apply, &p, b"mine");
        close_window();
        let shown = format!(
            "{got:?} {}",
            got.as_ref().err().map(|e| e.to_string()).unwrap_or_default()
        );
        let kept = others(&d, &p);
        assert!(
            matches!(got, Err(CasError::Displaced { .. }))
                && shown.contains(&sha(b"one"))
                && shown.contains(&sha(b"theirs"))
                && kept == [b"theirs".to_vec()],
            "ATTACK: E5 an edit between the re-hash and the rename was overwritten silently: \
             {shown}; kept {kept:?}"
        );
        // The run's bytes are in place and journalled: the report says so.
        assert_eq!(std::fs::read(&p).unwrap(), b"mine");
        assert_eq!(l.journal().len(), 1);
        // A later write never deletes the kept file.
        l.write(Site::Restore, &p, b"one").unwrap();
        assert_eq!(others(&d, &p), [b"theirs".to_vec()]);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Security review F3: a write over a target expected ABSENT never
    /// clobbers a file created in the window.
    #[test]
    fn a_file_created_in_the_window_is_not_clobbered() {
        let d = dir("noreplace");
        let p = d.join("f.ax");
        std::fs::write(&p, b"one").unwrap();
        let mut l = Ledger::default();
        l.observe(&p, b"one");
        l.remove(Site::Sweep, &p).unwrap();
        IN_WINDOW.with(|h| {
            *h.borrow_mut() = Some(Box::new(|p: &Path| std::fs::write(p, b"theirs").unwrap()))
        });
        let got = l.write(Site::Apply, &p, b"mine");
        close_window();
        assert!(
            matches!(got, Err(CasError::Foreign { .. }))
                && std::fs::read(&p).unwrap() == b"theirs",
            "ATTACK: E5 a file created between the re-hash and the rename was clobbered: {got:?}"
        );
        assert!(others(&d, &p).is_empty(), "the temp file is removed");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Security review F9: an edit between a delete's re-hash and its unlink
    /// is never deleted.
    #[test]
    fn a_writer_between_the_rehash_and_the_unlink_is_not_deleted() {
        let d = dir("rmwindow");
        let p = d.join("f.ax");
        std::fs::write(&p, b"one").unwrap();
        let mut l = Ledger::default();
        l.observe(&p, b"one");
        rewrite_in_window(b"theirs");
        let got = l.remove(Site::Sweep, &p);
        close_window();
        let shown = format!(
            "{got:?} {}",
            got.as_ref().err().map(|e| e.to_string()).unwrap_or_default()
        );
        assert!(
            std::fs::read(&p).ok().as_deref() == Some(b"theirs".as_slice())
                && shown.contains(&sha(b"one"))
                && shown.contains(&sha(b"theirs")),
            "ATTACK: E5 a delete removed an edit made between its re-hash and its unlink: {shown}"
        );
        assert!(l.journal().is_empty());
        assert!(others(&d, &p).is_empty(), "the temp name is renamed back");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Security review F8: a FIFO in the target's place never stalls a read
    /// (the E3 watchdog's restore reads under the write lock).
    #[cfg(unix)]
    #[test]
    fn a_fifo_in_the_targets_place_never_stalls_a_read() {
        use std::os::unix::ffi::OsStrExt;
        let d = dir("fifo");
        let p = d.join("f.ax");
        let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let q = p.clone();
        std::thread::spawn(move || {
            let mut l = Ledger::default();
            l.observe(&q, b"one");
            let _ = tx.send(l.write(Site::ExitRestore, &q, b"one"));
        });
        let got = rx.recv_timeout(std::time::Duration::from_secs(5));
        if got.is_err() {
            // Release the stalled reader: a writer end, opened and closed.
            // SAFETY: as above.
            let fd = unsafe { libc::open(c.as_ptr(), libc::O_WRONLY | libc::O_NONBLOCK) };
            if fd >= 0 {
                // SAFETY: fd was just opened here.
                unsafe { libc::close(fd) };
            }
        }
        assert!(
            matches!(got, Ok(Err(CasError::Io(_)))),
            "ATTACK: E3 a FIFO in the target's place stalled a CAS read: {got:?}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
