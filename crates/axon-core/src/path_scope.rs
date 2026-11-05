//! C-1 (O-GV-08): the RESOLVED half of the fs path scope.
//!
//! The scope (`AXON_ALLOWED_FS_READ/WRITE`, the `axon-ceiling/1` `fs_*` keys,
//! `sandbox_create_scoped`) used to test only the path TEXT
//! (`capabilities::path_is_under`). The host then opened the path and
//! followed every link, so under `./data/` a `./data/link -> /etc/hostname`
//! read the host's file, `/proc/self/root/...` under a `/proc/` grant reached
//! every file, and a `./out/odir -> /elsewhere` link wrote outside `./out/`.
//!
//! The lexical test still runs first and is unchanged; this module adds the
//! second question, asked of the operating system: where does the path really
//! lead? It can only refuse more, never admit more. On Linux the kernel
//! resolves (`openat2` with `RESOLVE_NO_MAGICLINKS`, the real path read back
//! from `/proc/self/fd/N`); elsewhere on unix, and when `openat2` is
//! unavailable (kernel < 5.6, a seccomp filter), a userspace lstat/readlink
//! walk does it. wasm keeps the lexical test (no `cfg(unix)`).
//!
//! Residual, documented: a concurrent same-uid process can swap a component
//! between this check and the host's open (O-C-01); hard links and bind
//! mounts inside a grant are not detected.

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

/// Where a path leads once its links are followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolution {
    /// The real path the op would reach: absolute, with a missing tail joined
    /// on unresolved (none of it exists, and it holds no `..`).
    Real(PathBuf),
    /// Resolution crosses a `/proc` magic link (`/proc/*/root`, `cwd`, `exe`,
    /// `fd/*`, `map_files/*`, and `/dev/fd` through them), or a link whose
    /// text is not a path (`pipe:[N]`). Always a violation.
    MagicLink,
    /// The path cannot be traversed (a real link loop, a NUL byte): the op
    /// fails the same way and returns its own `Err`.
    Opaque,
}

/// The kernel's symlink budget (`MAXSYMLINKS`), used for the splices this
/// module performs itself.
const MAX_SPLICES: usize = 40;

#[cfg(target_os = "linux")]
const ELOOP: i32 = libc::ELOOP;
#[cfg(any(target_os = "solaris", target_os = "illumos"))]
const ELOOP: i32 = 90;
// Darwin and the BSDs.
#[cfg(not(any(target_os = "linux", target_os = "solaris", target_os = "illumos")))]
const ELOOP: i32 = 62;

/// Resolve `path` (relative to the working directory) the way the op will.
/// With `follow_final == false` (`file_rename`, `dir_create`: rename(2) and
/// mkdir(2) do not follow the final name) the parent is resolved and the
/// final name is appended unresolved.
pub(crate) fn resolve_for_scope(path: &str, follow_final: bool) -> Resolution {
    if !follow_final {
        if let Some((parent, name)) = split_final(path) {
            return match resolve_for_scope(parent, true) {
                Resolution::Real(p) => Resolution::Real(p.join(name)),
                other => other,
            };
        }
    }
    #[cfg(target_os = "linux")]
    if let Some(r) = kernel::resolve(path) {
        return r;
    }
    fallback_walk(path)
}

/// Is the resolved path `real` under one of the pinned `roots`? By
/// COMPONENT (CBA-1): a root's trailing `/` is removed (`/` stays `/`), and
/// `real` must equal the root or continue it with a `/`. `path_is_under` is
/// not used: for a prefix ending in `/` it asks `starts_with(prefix)`, and a
/// resolved path never ends in `/`, so the grant root itself
/// (`dir_list("./data/")` under `./data/`) would be refused.
///
/// A relative `real` never matches: it is not a place on the filesystem.
pub(crate) fn admits(roots: &[PathBuf], real: &Path) -> bool {
    let r = real.as_os_str().as_bytes();
    if r.first() != Some(&b'/') {
        return false;
    }
    roots.iter().any(|t| {
        let t = t.as_os_str().as_bytes();
        let t = if t == b"/" {
            t
        } else {
            t.strip_suffix(b"/").unwrap_or(t)
        };
        t == b"/" || r == t || (r.starts_with(t) && r.get(t.len()) == Some(&b'/'))
    })
}

/// `(parent, final name)` of a path whose final name is a real name;
/// `None` for `/`, `.`, a path ending in `.`/`..`, or the empty path.
fn split_final(path: &str) -> Option<(&str, &str)> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let (parent, name) = match trimmed.rfind('/') {
        Some(i) => (&trimmed[..i], &trimmed[i + 1..]),
        None => (".", trimmed),
    };
    if name == "." || name == ".." {
        return None;
    }
    let parent = match parent {
        "" if trimmed.starts_with('/') => "/",
        "" => ".",
        p => p,
    };
    Some((parent, name))
}

/// The path's components, `.` and empty ones dropped (`..` kept).
fn components(path: &[u8]) -> (bool, Vec<&[u8]>) {
    let comps = path
        .split(|b| *b == b'/')
        .filter(|c| !c.is_empty() && *c != b".")
        .collect();
    (path.first() == Some(&b'/'), comps)
}

/// The fallback budget ran out. The OS decides (CBA-6): its own ELOOP means
/// the op fails the same way (`Opaque`); anything else means it resolved a
/// chain this module could not verify, which is a violation. An empty path
/// is not absolute, so it matches no root.
fn exhausted(path: &[u8]) -> Resolution {
    match std::fs::metadata(OsStr::from_bytes(path)) {
        Err(e) if e.raw_os_error() == Some(ELOOP) => Resolution::Opaque,
        _ => Resolution::Real(PathBuf::new()),
    }
}

/// A link text that names no path: an fd magic link's `pipe:[N]`,
/// `socket:[N]` or `anon_inode:...`.
fn not_a_path(target: &[u8]) -> bool {
    target.first() != Some(&b'/')
        && (target.starts_with(b"anon_inode:")
            || (target.ends_with(b"]") && target.windows(2).any(|w| w == b":[")))
}

/// The userspace walk: lstat each component, splice every symlink's text in
/// its place (at most [`MAX_SPLICES`]), stop at the first component that does
/// not exist or cannot be traversed. Follows the final component.
pub(crate) fn fallback_walk(path: &str) -> Resolution {
    let (abs, comps) = components(path.as_bytes());
    let mut base = if abs {
        PathBuf::from("/")
    } else {
        match std::env::current_dir() {
            Ok(d) => d,
            Err(_) => return Resolution::Opaque,
        }
    };
    let mut queue: VecDeque<OsString> = comps
        .into_iter()
        .map(|c| OsStr::from_bytes(c).to_os_string())
        .collect();
    let mut splices = 0;
    while let Some(c) = queue.pop_front() {
        if c.as_bytes() == b".." {
            // `base` is a real path, so its parent is the real parent.
            base.pop();
            continue;
        }
        let next = base.join(&c);
        match std::fs::symlink_metadata(&next) {
            Ok(m) if m.file_type().is_symlink() => {
                splices += 1;
                if splices > MAX_SPLICES {
                    return exhausted(path.as_bytes());
                }
                let Ok(target) = std::fs::read_link(&next) else {
                    return Resolution::Opaque;
                };
                let target = target.into_os_string().into_vec();
                if not_a_path(&target) {
                    return Resolution::MagicLink;
                }
                let (t_abs, t_comps) = components(&target);
                if t_abs {
                    base = PathBuf::from("/");
                }
                for tc in t_comps.into_iter().rev() {
                    queue.push_front(OsStr::from_bytes(tc).to_os_string());
                }
            }
            Ok(_) => base = next,
            Err(_) => {
                // Missing or untraversable: nothing below it exists for the
                // op either.
                queue.push_front(c);
                if queue.iter().any(|c| c.as_bytes() == b"..") {
                    return Resolution::Opaque;
                }
                return Resolution::Real(queue.iter().fold(base, |p, c| p.join(c)));
            }
        }
    }
    Resolution::Real(base)
}

#[cfg(target_os = "linux")]
mod kernel {
    use super::{components, exhausted, Resolution, MAX_SPLICES};
    use std::ffi::{CString, OsStr};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    /// One `openat2` probe.
    enum Open {
        Real(PathBuf),
        Errno(i32),
        /// `openat2` or `/proc/self/fd` is not available: walk instead.
        Unavailable,
    }

    fn open_real(path: &[u8], no_magic: bool) -> Open {
        let Ok(c) = CString::new(path) else {
            return Open::Errno(libc::EINVAL);
        };
        // SAFETY: `open_how` is plain integers; all-zero is its documented
        // default (no flags, no resolve restrictions).
        let mut how: libc::open_how = unsafe { std::mem::zeroed() };
        how.flags = (libc::O_PATH | libc::O_CLOEXEC) as u64;
        how.resolve = if no_magic {
            libc::RESOLVE_NO_MAGICLINKS
        } else {
            0
        };
        // SAFETY: `c` is NUL-terminated and `how` outlives the call; the
        // size passed is the struct's own.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                libc::AT_FDCWD,
                c.as_ptr(),
                &how as *const libc::open_how,
                std::mem::size_of::<libc::open_how>(),
            )
        };
        if fd < 0 {
            return match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ENOSYS) | Some(libc::EPERM) | None => Open::Unavailable,
                Some(e) => Open::Errno(e),
            };
        }
        let fd = fd as i32;
        let real = std::fs::read_link(format!("/proc/self/fd/{fd}"));
        // SAFETY: `fd` was returned by the kernel above and is closed once.
        unsafe { libc::close(fd) };
        match real {
            Ok(p) => Open::Real(p),
            Err(_) => Open::Unavailable,
        }
    }

    /// The ELOOP rule (CBA-4): retry the same string without
    /// `RESOLVE_NO_MAGICLINKS`. Another ELOOP is a real link loop, which the
    /// op hits too (`Opaque`); anything else (success, ENOENT, ENOTDIR,
    /// EACCES) means a magic link was crossed, so
    /// `/proc/self/root/elsewhere/new.txt` is a violation, not a new file.
    fn eloop_retry(path: &[u8]) -> Resolution {
        match open_real(path, false) {
            Open::Errno(libc::ELOOP) => Resolution::Opaque,
            Open::Errno(_) => Resolution::MagicLink,
            Open::Real(_) | Open::Unavailable => Resolution::MagicLink,
        }
    }

    fn join_prefix(abs: bool, comps: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        if abs {
            out.push(b'/');
        } else if comps.is_empty() {
            out.push(b'.');
        }
        for (i, c) in comps.iter().enumerate() {
            if i > 0 {
                out.push(b'/');
            }
            out.extend_from_slice(c);
        }
        out
    }

    fn is_symlink(p: &Path) -> bool {
        std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
    }

    /// `None`: the kernel resolver is unavailable; the caller walks instead.
    pub(super) fn resolve(path: &str) -> Option<Resolution> {
        let mut cur: Vec<u8> = path.as_bytes().to_vec();
        let mut splices = 0;
        loop {
            match open_real(&cur, true) {
                Open::Real(r) => return Some(Resolution::Real(r)),
                Open::Unavailable => return None,
                Open::Errno(libc::ELOOP) => return Some(eloop_retry(&cur)),
                // A missing tail: the longest existing prefix is found below.
                Open::Errno(libc::ENOENT) => {}
                // EACCES, ENOTDIR: the same walk; the op cannot get further.
                Open::Errno(_) => {}
            }
            let (abs, comps) = components(&cur);
            // The longest prefix that resolves (an EACCES/ENOTDIR prefix does
            // not; the op cannot traverse it either).
            let mut found = None;
            for k in (0..comps.len()).rev() {
                let prefix = join_prefix(abs, &comps[..k]);
                match open_real(&prefix, true) {
                    Open::Real(r) => {
                        found = Some((k, r));
                        break;
                    }
                    Open::Unavailable => return None,
                    Open::Errno(libc::ELOOP) => return Some(eloop_retry(&prefix)),
                    Open::Errno(_) => continue,
                }
            }
            let Some((k, base)) = found else {
                return Some(Resolution::Opaque);
            };
            let rest = &comps[k..];
            let next = base.join(OsStr::from_bytes(rest[0]));
            // The prefix ending in `next` did not resolve, so a symlink there
            // dangles (or leads somewhere untraversable): splice its text in
            // its place and resolve again, as the kernel would on create.
            if is_symlink(&next) {
                splices += 1;
                if splices > MAX_SPLICES {
                    return Some(exhausted(path.as_bytes()));
                }
                let Ok(target) = std::fs::read_link(&next) else {
                    return Some(Resolution::Opaque);
                };
                let target = target.into_os_string().into_vec();
                let mut spliced = if target.first() == Some(&b'/') {
                    target
                } else {
                    let mut b = base.into_os_string().into_vec();
                    b.push(b'/');
                    b.extend_from_slice(&target);
                    b
                };
                for c in &rest[1..] {
                    spliced.push(b'/');
                    spliced.extend_from_slice(c);
                }
                cur = spliced;
                continue;
            }
            if rest.contains(&&b".."[..]) {
                return Some(Resolution::Opaque);
            }
            let real = rest.iter().fold(base, |p, c| p.join(OsStr::from_bytes(c)));
            return Some(Resolution::Real(real));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("axon_pscope_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // Canonical, so a symlinked temp dir does not move the expectation.
        d.canonicalize().unwrap()
    }

    #[test]
    fn a_non_absolute_resolution_never_matches() {
        let roots = [PathBuf::from("data/")];
        assert!(
            !admits(&roots, Path::new("data/x.txt")),
            "ATTACK: a non-path link text matched a relative grant"
        );
        assert!(
            !admits(&[PathBuf::from("/")], Path::new("pipe:[7]")),
            "ATTACK: a non-path link text matched a relative grant"
        );
        assert!(admits(&[PathBuf::from("/w/data/")], Path::new("/w/data/x")));
        assert!(admits(&[PathBuf::from("/w/data/")], Path::new("/w/data")));
        assert!(!admits(
            &[PathBuf::from("/w/data")],
            Path::new("/w/database")
        ));
    }

    #[test]
    fn fallback_walk_splices_a_dangling_link() {
        let d = tmp("splice");
        std::fs::create_dir_all(d.join("out")).unwrap();
        std::fs::create_dir_all(d.join("outside")).unwrap();
        std::os::unix::fs::symlink("../outside/new.txt", d.join("out/w")).unwrap();
        let p = d.join("out/w");
        assert_eq!(
            fallback_walk(p.to_str().unwrap()),
            Resolution::Real(d.join("outside/new.txt")),
            "ATTACK: the fallback walk did not follow a dangling link outside the grant"
        );
        // An in-prefix dangling link stays in the prefix.
        std::os::unix::fs::symlink("newfile.txt", d.join("out/in")).unwrap();
        assert_eq!(
            fallback_walk(d.join("out/in").to_str().unwrap()),
            Resolution::Real(d.join("out/newfile.txt"))
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fallback_walk_reports_pipe_links_as_magic() {
        use std::os::fd::AsRawFd;
        let (r, _w) = std::io::pipe().unwrap();
        let p = format!("/proc/self/fd/{}", r.as_raw_fd());
        assert_eq!(
            fallback_walk(&p),
            Resolution::MagicLink,
            "ATTACK: a pipe's fd link was read as a path"
        );
    }

    #[test]
    fn fallback_budget_exhaustion_on_a_real_loop_is_opaque() {
        let d = tmp("loop");
        std::os::unix::fs::symlink("loop", d.join("loop")).unwrap();
        assert_eq!(
            fallback_walk(d.join("loop").to_str().unwrap()),
            Resolution::Opaque,
            "FALSE REFUSAL: a real link loop was a violation, not the op's own Err"
        );
        // A chain longer than the budget that the OS still resolves is a
        // violation (an empty, non-absolute path matches no root).
        std::fs::write(d.join("end.txt"), "e").unwrap();
        let mut prev = String::from("end.txt");
        for i in 0..45 {
            let name = format!("l{i}");
            std::os::unix::fs::symlink(&prev, d.join(&name)).unwrap();
            prev = name;
        }
        let r = fallback_walk(d.join(&prev).to_str().unwrap());
        let _ = std::fs::remove_dir_all(&d);
        // The kernel stops at 40 links too, so this chain is ELOOP for the
        // OS as well: Opaque.
        assert_eq!(r, Resolution::Opaque);
    }

    #[test]
    fn split_final_keeps_the_parent() {
        assert_eq!(split_final("./out/"), Some((".", "out")));
        assert_eq!(split_final("./out/a/b"), Some(("./out/a", "b")));
        assert_eq!(split_final("/a"), Some(("/", "a")));
        assert_eq!(split_final("a"), Some((".", "a")));
        assert_eq!(split_final("/"), None);
        assert_eq!(split_final("./out/.."), None);
    }
}
