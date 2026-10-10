//! D — same-byte hash-and-exec (operator decision D, 2026-09-29;
//! `governance/specs/v022-psv-protocol.md` amendment 45).
//!
//! An authority program (the protected launcher, the privileged helper, the
//! preflight observer) used to be hashed BY PATH and then executed BY PATH.
//! Between the two, the path could name another object (a rename, a symlink
//! swap), so the bytes that were authenticated need not be the bytes that ran.
//!
//! Here the object is opened ONCE, with `O_NOFOLLOW`; its owner and mode are
//! checked on the open descriptor; the descriptor's bytes are hashed and
//! compared with the pin; and THAT descriptor is executed
//! (`execveat(fd, "", AT_EMPTY_PATH)`). A script is never executed through its
//! `#!` line (the kernel would pick an interpreter the pin never covered): its
//! interpreter must be pinned too, is itself executed from its verified
//! descriptor, and reads the script as `/dev/fd/N`, the SAME open file.
//!
//! "Unchanged between hash and exec": the descriptor's identity (device,
//! inode, size, mtime, ctime, mode, owner) is re-read in the child
//! immediately before `execveat`, and a read LEASE taken at open (when the
//! kernel grants one) is required to still be held. A lease cannot be taken
//! while anyone has the file open for writing, and any later open for writing
//! breaks it, so a writer present at hash time refuses the open and a writer
//! that appears later refuses the exec (or, after the run, marks it unknown).

use std::ffi::{CString, OsString};
use std::fs::File;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};

/// A pinned file: a path and the sha256 of the exact bytes allowed there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pinned {
    pub path: PathBuf,
    pub sha256: String,
}

/// Whether a read lease must be held (a root helper always gets one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lease {
    /// Refuse the object if the kernel will not grant a lease.
    Required,
    /// Take one when the kernel grants it (the owner or `CAP_LEASE`); without
    /// one, immutability rests on the owner/mode check alone.
    IfGranted,
}

/// The largest authority program hashed (a launcher script, a helper binary,
/// an interpreter). An unbounded read is a denial of service.
const MAX_BYTES: u64 = 256 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    dev: u64,
    ino: u64,
    size: i64,
    mtime: (i64, i64),
    ctime: (i64, i64),
    mode: u32,
    uid: u32,
}

fn identity(fd: RawFd) -> std::io::Result<Identity> {
    // SAFETY: fstat writes into a zeroed stat on a descriptor we own.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Identity {
        dev: st.st_dev,
        ino: st.st_ino,
        size: st.st_size,
        mtime: (st.st_mtime, st.st_mtime_nsec),
        ctime: (st.st_ctime, st.st_ctime_nsec),
        mode: st.st_mode,
        uid: st.st_uid,
    })
}

/// An authority program, opened once and verified on its descriptor.
#[derive(Debug)]
pub struct Verified {
    file: File,
    path: PathBuf,
    sha256: String,
    id: Identity,
    leased: bool,
    script: bool,
}

impl Verified {
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Whether a read lease guards the inode.
    pub fn leased(&self) -> bool {
        self.leased
    }
    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
    /// The inode is still the one hashed: same identity, and (if leased) no
    /// one has opened it for writing since.
    pub fn unchanged(&self) -> Result<(), String> {
        match check_unchanged(self.fd(), &self.id, self.leased) {
            0 => Ok(()),
            1 => Err(format!(
                "{} changed after it was hashed (inode, size, mtime, ctime, mode or owner)",
                self.path.display()
            )),
            _ => Err(format!(
                "{} was opened for writing after it was hashed (its read lease was broken)",
                self.path.display()
            )),
        }
    }
}

/// 0 unchanged, 1 identity differs, 2 lease broken. Async-signal-safe: it is
/// also the child's last check before `execveat`.
fn check_unchanged(fd: RawFd, id: &Identity, leased: bool) -> i32 {
    match identity(fd) {
        Ok(now) if now == *id => {}
        _ => return 1,
    }
    // SAFETY: F_GETLEASE on an open descriptor.
    if leased && unsafe { libc::fcntl(fd, libc::F_GETLEASE) } != libc::F_RDLCK {
        return 2;
    }
    0
}

/// A read lease on `fd`, as the kernel answers.
///
/// TEST-TRUST BUILDS ONLY (C9 round 4, EQUIVALENCE; M709): while
/// `/etc/axon/TEST-no-read-lease` exists, the answer is EINVAL, as on a
/// filesystem that grants no lease (9p, NFS). A root helper on a root-owned
/// file is always granted one, so without this a test could reach the
/// production helper's `Lease::Required` refusal only by changing the host
/// (`fs.leases-enable`). The branch, and the path, are not in a production
/// build: a production helper takes the kernel's answer, always (M709's test
/// runs the production helper with the switch present).
fn take_read_lease(fd: RawFd) -> std::io::Result<()> {
    #[cfg(feature = "test-trust-root")]
    if Path::new("/etc/axon/TEST-no-read-lease").exists() {
        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
    }
    // SAFETY: F_SETLEASE on a read-only descriptor we own.
    if unsafe { libc::fcntl(fd, libc::F_SETLEASE, libc::F_RDLCK) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Open `p` for execution: `O_NOFOLLOW`, a regular file, owned by `owner` or
/// by root (root can change anything anyway; any owner when `None`:
/// development only), not group- or other-writable, its descriptor's bytes
/// equal to the pin.
pub fn open_verified(p: &Pinned, owner: Option<u32>, lease: Lease) -> Result<Verified, String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let shown = p.path.display();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&p.path)
        .map_err(|e| {
            if e.raw_os_error() == Some(libc::ELOOP) {
                format!("{shown} is a symlink: an authority program is executed only from the file itself")
            } else {
                format!("{shown}: {e}")
            }
        })?;
    let fd = file.as_raw_fd();
    let id = identity(fd).map_err(|e| format!("{shown}: {e}"))?;
    if id.mode & libc::S_IFMT != libc::S_IFREG {
        return Err(format!("{shown} is not a regular file"));
    }
    if let Some(want) = owner {
        if id.uid != want && id.uid != 0 {
            return Err(format!(
                "{shown} is owned by uid {}, not {want}: whoever owns it can change the bytes \
                 after they are verified",
                id.uid
            ));
        }
    }
    if id.mode & 0o022 != 0 {
        return Err(format!(
            "{shown} is group- or other-writable (mode {:o}): another uid can change the bytes \
             after they are verified",
            id.mode & 0o7777
        ));
    }
    let leased = match take_read_lease(fd) {
        Ok(()) => {
            // Taking a lease makes this process the file's signal owner, and a
            // break would deliver SIGIO (default: terminate). No owner: the
            // break is observed by polling F_GETLEASE, never by a signal.
            // SAFETY: F_SETOWN on our descriptor.
            unsafe { libc::fcntl(fd, libc::F_SETOWN, 0) };
            true
        }
        Err(e) => {
            if e.raw_os_error() == Some(libc::EAGAIN) {
                return Err(format!(
                    "{shown} is open for writing by some process: its bytes can change after \
                     they are verified"
                ));
            }
            if lease == Lease::Required {
                return Err(format!(
                    "{shown}: no read lease ({e}); without one a later writer is not detected"
                ));
            }
            false
        }
    };
    if id.size < 0 || id.size as u64 > MAX_BYTES {
        return Err(format!("{shown} is larger than {MAX_BYTES} bytes"));
    }
    let mut bytes = Vec::with_capacity(id.size as usize);
    (&file)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{shown}: {e}"))?;
    // Tests only: act between the read and the verdict (the window the
    // post-hash check below exists for). Compiled out of every other build.
    #[cfg(test)]
    tests::after_read();
    let sha256 = crate::backend::sha256_hex(&bytes);
    let v = Verified {
        file,
        path: p.path.clone(),
        sha256,
        id,
        leased,
        script: bytes.starts_with(b"#!"),
    };
    // The bytes read are the inode's bytes only if it did not change meanwhile.
    v.unchanged()?;
    if v.sha256 != p.sha256 {
        return Err(format!(
            "{shown} has sha256 {}, not its pin {}",
            v.sha256, p.sha256
        ));
    }
    Ok(v)
}

/// The child's refusal when a verified object changed between hash and exec.
pub const CHANGED_ERRNO: i32 = libc::ETXTBSY;

/// argv/envp prepared before `fork`, so the child allocates nothing.
struct Exec {
    target: RawFd,
    argv: Vec<CString>,
    envp: Vec<CString>,
    inherit: Vec<RawFd>,
    checks: Vec<(RawFd, Identity, bool)>,
}

/// A `Command` that executes `program` from its verified descriptor — or,
/// when `interpreter` is given, executes the interpreter from ITS descriptor
/// with the program as `/dev/fd/N`. `inherit` descriptors stay open in the
/// child (their close-on-exec flag is cleared there only). The returned
/// command's own program, arguments and environment are NOT used: the child
/// runs exactly `argv` under `env`. Configure stdio on it and run it.
pub fn command(
    program: &Verified,
    interpreter: Option<&Verified>,
    args: &[OsString],
    env: &[(&str, &str)],
    inherit: &[RawFd],
) -> Result<std::process::Command, String> {
    let c = |s: &[u8]| CString::new(s).map_err(|_| "an argument holds a NUL byte".to_string());
    let mut argv = Vec::new();
    let (target, mut inherit_fds) = match interpreter {
        None => {
            if program.script {
                return Err(format!(
                    "{} is a script (#!) and no interpreter is pinned: the kernel would run the \
                     interpreter its first line names, which no pin covers",
                    program.path.display()
                ));
            }
            argv.push(c(program.path.as_os_str().as_bytes())?);
            (program.fd(), Vec::new())
        }
        Some(i) => {
            if i.script {
                return Err(format!(
                    "the interpreter {} is itself a script",
                    i.path.display()
                ));
            }
            argv.push(c(i.path.as_os_str().as_bytes())?);
            // The interpreter reads the program from the SAME open file.
            argv.push(c(format!("/dev/fd/{}", program.fd()).as_bytes())?);
            (i.fd(), vec![program.fd()])
        }
    };
    for a in args {
        argv.push(c(a.as_bytes())?);
    }
    let envp = env
        .iter()
        .map(|(k, v)| c(format!("{k}={v}").as_bytes()))
        .collect::<Result<Vec<_>, _>>()?;
    inherit_fds.extend_from_slice(inherit);
    let mut checks = vec![(program.fd(), program.id, program.leased)];
    if let Some(i) = interpreter {
        checks.push((i.fd(), i.id, i.leased));
    }
    let exec = Exec {
        target,
        argv,
        envp,
        inherit: inherit_fds,
        checks,
    };
    // Checked once more in the parent: a refusal here is a clear message,
    // the child's check below is the one nearest the exec.
    program.unchanged()?;
    if let Some(i) = interpreter {
        i.unchanged()?;
    }
    let mut cmd = std::process::Command::new("/nonexistent/axon-sealed-exec");
    // SAFETY: the closure calls only async-signal-safe functions (fstat,
    // fcntl, execveat) on memory prepared before fork.
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(move || exec.run());
    }
    Ok(cmd)
}

impl Exec {
    /// In the child, after fork: re-check every verified descriptor, keep the
    /// inherited ones open, and execute the verified object itself.
    fn run(&self) -> std::io::Result<()> {
        for (fd, id, leased) in &self.checks {
            if check_unchanged(*fd, id, *leased) != 0 {
                // Only an errno crosses back to the parent: ETXTBSY ("text
                // file busy"), the kernel's own word for a program being
                // written while it would run.
                return Err(std::io::Error::from_raw_os_error(CHANGED_ERRNO));
            }
        }
        for fd in &self.inherit {
            // SAFETY: clearing FD_CLOEXEC on a descriptor this process holds.
            if unsafe { libc::fcntl(*fd, libc::F_SETFD, 0) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        let mut argv: Vec<*const libc::c_char> = self.argv.iter().map(|a| a.as_ptr()).collect();
        argv.push(std::ptr::null());
        let mut envp: Vec<*const libc::c_char> = self.envp.iter().map(|a| a.as_ptr()).collect();
        envp.push(std::ptr::null());
        // SAFETY: execveat with NUL-terminated argv/envp; returns only on error.
        unsafe {
            libc::syscall(
                libc::SYS_execveat,
                self.target,
                c"".as_ptr(),
                argv.as_ptr(),
                envp.as_ptr(),
                libc::AT_EMPTY_PATH,
            );
        }
        Err(std::io::Error::last_os_error())
    }
}

// The closure owns the prepared argv/envp; nothing in it is shared.
unsafe impl Send for Exec {}
unsafe impl Sync for Exec {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn euid() -> u32 {
        unsafe { libc::geteuid() }
    }

    thread_local! {
        static AFTER_READ: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
            const { std::cell::RefCell::new(None) };
    }

    /// open_verified's test seam: runs (once) what the test set on this thread.
    pub(super) fn after_read() {
        if let Some(f) = AFTER_READ.with(|h| h.borrow_mut().take()) {
            f()
        }
    }

    /// Run `f` on a thread WITHOUT `CAP_LEASE` (capabilities are per thread,
    /// so the rest of the test process keeps it). As root, a file owned by
    /// another uid can then not be leased: the production helper's position
    /// for any object whose owner it is not, when it lacks the capability.
    fn without_cap_lease<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::spawn(move || {
            #[repr(C)]
            struct Hdr {
                version: u32,
                pid: i32,
            }
            #[repr(C)]
            #[derive(Clone, Copy, Default)]
            struct Data {
                effective: u32,
                permitted: u32,
                inheritable: u32,
            }
            const CAP_LEASE: u32 = 28;
            let mut h = Hdr {
                version: 0x2008_0522, // _LINUX_CAPABILITY_VERSION_3
                pid: 0,
            };
            let mut d = [Data::default(); 2];
            // SAFETY: capget/capset on this thread with a v3 header and two
            // data words, as the ABI requires.
            unsafe {
                assert_eq!(libc::syscall(libc::SYS_capget, &mut h, d.as_mut_ptr()), 0);
                d[0].effective &= !(1 << CAP_LEASE);
                d[0].permitted &= !(1 << CAP_LEASE);
                assert_eq!(libc::syscall(libc::SYS_capset, &mut h, d.as_ptr()), 0);
            }
            f()
        })
        .join()
        .unwrap()
    }

    /// Hand `p` to another uid (root only): what makes a lease unavailable
    /// to a root thread without `CAP_LEASE`.
    fn give_away(p: &Path) -> u32 {
        std::os::unix::fs::chown(p, Some(4242), None).unwrap();
        4242
    }

    fn write_exec(p: &Path, body: &str) -> Pinned {
        // Written by a SEPARATE process: a write fd held here would be
        // inherited by a sibling test thread's fork, and the read lease would
        // then (correctly) see a writer (a flake in the full suite, C9 r2).
        crate::test_exec::write_executable(p, body, 0o755);
        Pinned {
            path: p.to_path_buf(),
            sha256: crate::backend::sha256_hex(body.as_bytes()),
        }
    }

    fn bash() -> Pinned {
        let p = PathBuf::from("/bin/bash");
        Pinned {
            sha256: crate::backend::sha256_hex(&std::fs::read(&p).unwrap()),
            path: p,
        }
    }

    /// Run `script` (pinned as `pin`) under the pinned bash, with `between`
    /// run after the hash and after the command is built: the last moment
    /// before the fork, so only the child's own re-check stands between it
    /// and the exec. Returns what it printed.
    fn run_between(pin: &Pinned, between: impl FnOnce()) -> Result<String, String> {
        let prog = open_verified(pin, Some(euid()), Lease::IfGranted)?;
        let interp = open_verified(&bash(), None, Lease::IfGranted)?;
        let mut cmd = command(&prog, Some(&interp), &[], &[("PATH", "/usr/bin:/bin")], &[])?;
        between();
        let out = cmd
            .stdout(std::process::Stdio::piped())
            .output()
            .map_err(|e| format!("exec refused: {e}"))?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// D: a launcher swapped by a rename race between its hash and its exec.
    /// The directory holding it is renamed away and a new one, holding other
    /// bytes, takes its name: the PATH now names the swapped file, while the
    /// verified inode is untouched. The exec is of the verified descriptor,
    /// so the verified bytes run.
    #[test]
    fn a_launcher_swapped_by_rename_between_hash_and_exec_runs_the_verified_bytes() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("bin");
        std::fs::create_dir(&dir).unwrap();
        let p = dir.join("launcher.sh");
        let pin = write_exec(&p, "#!/bin/sh\necho genuine\n");
        let staged = d.path().join("staged");
        std::fs::create_dir(&staged).unwrap();
        write_exec(&staged.join("launcher.sh"), "#!/bin/sh\necho SWAPPED\n");
        let got = run_between(&pin, || {
            std::fs::rename(&dir, d.path().join("old")).unwrap();
            std::fs::rename(&staged, &dir).unwrap();
        })
        .unwrap();
        assert!(
            !got.contains("SWAPPED"),
            "ATTACK: a launcher swapped in by a rename race between its hash and its exec was \
             executed: {got:?}"
        );
        assert_eq!(got, "genuine\n");
    }

    /// D, for a BINARY (executed directly, no interpreter): the same rename
    /// race; the verified descriptor is what runs.
    #[test]
    fn a_binary_swapped_by_rename_between_hash_and_exec_runs_the_verified_bytes() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("bin");
        std::fs::create_dir(&dir).unwrap();
        let p = dir.join("helper");
        crate::test_exec::copy_executable("/bin/true", &p, 0o755);
        let pin = Pinned {
            sha256: crate::backend::sha256_hex(&std::fs::read(&p).unwrap()),
            path: p.clone(),
        };
        let staged = d.path().join("staged");
        std::fs::create_dir(&staged).unwrap();
        crate::test_exec::copy_executable("/bin/false", staged.join("helper"), 0o755);
        let v = open_verified(&pin, None, Lease::IfGranted).unwrap();
        let mut cmd = command(&v, None, &[], &[], &[]).unwrap();
        std::fs::rename(&dir, d.path().join("old")).unwrap();
        std::fs::rename(&staged, &dir).unwrap();
        let st = cmd.status().unwrap();
        assert!(
            st.success(),
            "ATTACK: a binary swapped in by a rename race between its hash and its exec was \
             executed (it exited {st:?}, as /bin/false does)"
        );
    }

    /// A FIFO serves different bytes to each reader: never an executable.
    #[test]
    fn a_fifo_at_the_pinned_path_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("launcher.sh");
        let c = CString::new(p.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o644) }, 0);
        let pin = Pinned {
            path: p,
            // What a writerless FIFO reads as: nothing.
            sha256: crate::backend::sha256_hex(b""),
        };
        let got = open_verified(&pin, None, Lease::IfGranted);
        let why = got.expect_err("ATTACK: a FIFO was verified for exec");
        assert!(why.contains("not a regular file"), "{why}");
    }

    /// The file itself renamed over: the verified inode's link count (so its
    /// ctime) changes, and the exec is refused; the swapped bytes never run.
    #[test]
    fn a_launcher_renamed_over_between_hash_and_exec_never_runs_the_swapped_bytes() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("launcher.sh");
        let pin = write_exec(&p, "#!/bin/sh\necho genuine\n");
        let evil = d.path().join("evil.sh");
        write_exec(&evil, "#!/bin/sh\necho SWAPPED\n");
        let got = run_between(&pin, || std::fs::rename(&evil, &p).unwrap());
        if let Ok(out) = &got {
            assert!(
                !out.contains("SWAPPED"),
                "ATTACK: a launcher renamed over between its hash and its exec was executed: \
                 {out:?}"
            );
        }
    }

    /// D: the inode itself written between hash and exec (its owner can
    /// write it). The write breaks the read lease, and the exec is refused.
    #[test]
    fn a_launcher_written_in_place_between_hash_and_exec_is_not_executed() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("launcher.sh");
        let pin = write_exec(&p, "#!/bin/sh\necho genuine\n");
        let got = run_between(&pin, || {
            // The writer is ANOTHER process (`dd`), so no write fd to the
            // launcher ever exists in this test process for a sibling test
            // thread's fork to inherit. O_NONBLOCK (`oflag=nonblock`): the
            // open starts the lease break and returns at once (EWOULDBLOCK,
            // `dd` then fails, which is ignored) instead of waiting out
            // lease-break-time; a holder without a lease would have let it
            // write. `conv=notrunc`: written in place, as an open for write is.
            let _ = std::process::Command::new("sh")
                .arg("-c")
                .arg("printf '#!/bin/sh\\necho REWRITTEN\\n' | dd of=\"$1\" oflag=nonblock conv=notrunc status=none")
                .arg("sh")
                .arg(&p)
                .stderr(std::process::Stdio::null())
                .status();
        });
        match got {
            Err(why) => assert!(why.contains("os error 26"), "{why}"),
            Ok(out) => panic!(
                "ATTACK: a writer opened the launcher between its hash and its exec, and the \
                 exec went ahead: {out:?}"
            ),
        }
    }

    /// D: a writer that already holds the file open when it is hashed can
    /// change it afterwards; no lease is granted, and the open is refused.
    #[test]
    fn a_launcher_open_for_writing_when_hashed_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("launcher.sh");
        let pin = write_exec(&p, "#!/bin/sh\necho genuine\n");
        // The writer is ANOTHER process: a write fd held in this test process
        // would be copied into any sibling test thread's fork, and would still
        // be open for the control below (a flake in the full suite, C9 r2).
        use std::io::BufRead;
        let mut writer = std::process::Command::new("sh")
            .arg("-c")
            .arg("exec 3>>\"$1\"; echo ready; exec sleep 60")
            .arg("sh")
            .arg(&p)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(writer.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(
            line.trim(),
            "ready",
            "setup: the writer holds the file open"
        );
        let got = open_verified(&pin, Some(euid()), Lease::IfGranted);
        let _ = writer.kill();
        let _ = writer.wait();
        let why = got.expect_err(
            "ATTACK: a launcher another process holds open for writing was verified for exec",
        );
        assert!(why.contains("open for writing"), "{why}");
        open_verified(&pin, Some(euid()), Lease::IfGranted).expect("control: no writer");
    }

    /// D: a symlink at the pinned path is refused, even to the pinned bytes.
    #[test]
    fn a_symlinked_launcher_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("real.sh");
        let pin = write_exec(&real, "#!/bin/sh\necho hi\n");
        let link = d.path().join("launcher.sh");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let got = open_verified(
            &Pinned {
                path: link,
                sha256: pin.sha256.clone(),
            },
            Some(euid()),
            Lease::IfGranted,
        );
        let why = got.expect_err("ATTACK: a symlinked launcher was verified for exec");
        assert!(why.contains("symlink"), "{why}");
        open_verified(&pin, Some(euid()), Lease::IfGranted).expect("control: the file itself");
    }

    /// D: a group- or other-writable launcher is refused (another uid could
    /// rewrite it after the hash).
    #[test]
    fn a_group_writable_launcher_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("launcher.sh");
        let pin = write_exec(&p, "#!/bin/sh\necho hi\n");
        for mode in [0o775, 0o757] {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
            let why = open_verified(&pin, Some(euid()), Lease::IfGranted).expect_err(&format!(
                "ATTACK: a launcher of mode {mode:o} (writable by another uid) was verified for exec"
            ));
            assert!(why.contains("group- or other-writable"), "{why}");
        }
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        open_verified(&pin, Some(euid()), Lease::IfGranted).expect("control: 0755");
    }

    /// D: an object owned by another uid than the operator is refused.
    #[test]
    fn a_launcher_owned_by_another_uid_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("launcher.sh");
        let pin = write_exec(&p, "#!/bin/sh\necho hi\n");
        // Root-owned is always accepted, so as root the file goes to another uid.
        let other = if euid() == 0 {
            std::os::unix::fs::chown(&p, Some(4242), None).unwrap();
            0
        } else {
            euid().wrapping_add(1)
        };
        let why = open_verified(&pin, Some(other), Lease::IfGranted).expect_err(
            "ATTACK: a launcher owned by a uid other than the operator's was verified for exec",
        );
        assert!(why.contains("is owned by uid"), "{why}");
    }

    /// D: a script is never run through its own `#!` line, whose interpreter
    /// no pin covers.
    #[test]
    fn a_script_without_a_pinned_interpreter_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("observer.sh");
        let pin = write_exec(&p, "#!/bin/sh\necho hi\n");
        let v = open_verified(&pin, Some(euid()), Lease::IfGranted).unwrap();
        let why = command(&v, None, &[], &[], &[]).expect_err(
            "ATTACK: a script was executed through the interpreter its #! line names, unpinned",
        );
        assert!(why.contains("no interpreter is pinned"), "{why}");
    }

    /// D, production (C9 round 3, harness): the privileged helper opens the
    /// launcher and its interpreter with `Lease::Required`. An object it cannot
    /// lease is refused: without the lease, a writer that opens the file after
    /// the hash is never detected, and the bytes that run need not be the
    /// bytes that were verified. Here the lease is unavailable because the
    /// file is another uid's and the thread lacks `CAP_LEASE` (as non-root:
    /// root's own bash, which no other uid may lease).
    #[test]
    fn an_authority_program_that_cannot_be_leased_is_refused_in_production() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("launcher.sh");
        let pin = write_exec(&p, "#!/bin/sh\necho hi\n");
        let try_open = |lease: Lease| {
            if euid() == 0 {
                let owner = give_away(&p);
                let pin = pin.clone();
                without_cap_lease(move || {
                    open_verified(&pin, Some(owner), lease).map(|v| v.leased())
                })
            } else {
                open_verified(&bash(), Some(0), lease).map(|v| v.leased())
            }
        };
        match try_open(Lease::Required) {
            Err(why) => assert!(why.contains("no read lease"), "{why}"),
            Ok(leased) => panic!(
                "ATTACK: an authority program that could not be leased (leased={leased}) was \
                 verified under Lease::Required: a writer after the hash would go undetected"
            ),
        }
        // Control: development mode takes it, with no lease.
        assert_eq!(try_open(Lease::IfGranted), Ok(false), "control");
    }

    /// D (C9 round 3, harness): the bytes hashed are the inode's bytes only
    /// if it did not change while they were read. Here the file grows between
    /// the read and the verdict (another process appends to it): the hash is
    /// of the ORIGINAL bytes and equals the pin, while the file now holds
    /// other bytes. For an object that is verified and not executed (the
    /// helper's kernel, rootfs, firecracker and jailer) no later check exists,
    /// so the post-hash check is the only one.
    #[test]
    fn a_file_changed_while_it_was_hashed_is_not_vouched_for() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("vmlinux");
        let pin = write_exec(&p, "#!/bin/sh\necho genuine\n");
        let owner = if euid() == 0 { give_away(&p) } else { euid() };
        let run = {
            let (p, pin) = (p.clone(), pin.clone());
            move || {
                AFTER_READ.with(|h| {
                    *h.borrow_mut() = Some(Box::new(move || {
                        // Another process, O_NONBLOCK: with no lease it
                        // appends; with one (non-root, the owner) the open
                        // breaks it and returns at once.
                        let _ = std::process::Command::new("sh")
                            .arg("-c")
                            .arg("printf 'echo appended\\n' | dd of=\"$1\" oflag=append,nonblock conv=notrunc status=none")
                            .arg("sh")
                            .arg(&p)
                            .stderr(std::process::Stdio::null())
                            .status();
                    }))
                });
                open_verified(&pin, Some(owner), Lease::IfGranted).map(|v| v.sha256().to_string())
            }
        };
        let got = if euid() == 0 {
            without_cap_lease(run)
        } else {
            run()
        };
        if let Ok(sha) = got {
            panic!(
                "ATTACK: a file that changed between its read and the verdict was vouched for \
                 as the pinned bytes ({sha}), which the inode no longer holds"
            );
        }
    }

    /// D (C9 round 3, harness): an authority program is read with a bound.
    /// A file over MAX_BYTES is refused before it is read; an unbounded read
    /// of a pinned path is a denial of service of the root helper.
    #[test]
    fn an_authority_program_over_the_size_bound_is_never_read() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("huge");
        // Sparse, and sized by another process (no write fd in this one).
        let st = std::process::Command::new("truncate")
            .arg("-s")
            // Amendment 98: the bound is pinned from outside (256 MiB + 1); sizing the file
            // from MAX_BYTES itself agreed with whatever the constant said (1 TiB left the
            // suite green).
            .arg("268435457")
            .arg(&p)
            .status()
            .unwrap();
        assert!(st.success(), "setup: truncate");
        let pin = Pinned {
            path: p,
            sha256: "0".repeat(64),
        };
        match open_verified(&pin, None, Lease::IfGranted) {
            Err(why) if why.contains("larger than") => {}
            other => panic!(
                "ATTACK: an authority program larger than MAX_BYTES ({MAX_BYTES}) was read and \
                 hashed whole: {other:?}"
            ),
        }
    }

    /// D (C9 round 3, harness): an INTERPRETER that is itself a `#!` script
    /// would run through its own first line, an interpreter no pin covers.
    /// Two guards stop it: the refusal in `command`, and the verified
    /// descriptor being close-on-exec (the kernel then cannot hand a script
    /// executed by descriptor to its `#!` interpreter: ENOENT). The attack is
    /// the unpinned interpreter line RUNNING.
    #[test]
    fn an_interpreter_that_is_a_script_never_runs_its_own_interpreter_line() {
        let d = tempfile::tempdir().unwrap();
        let prog = write_exec(&d.path().join("launcher.sh"), "#!/bin/sh\necho launcher\n");
        let interp = write_exec(
            &d.path().join("interp.sh"),
            "#!/bin/sh\necho UNPINNED-INTERPRETER-LINE-RAN\n",
        );
        let prog = open_verified(&prog, Some(euid()), Lease::IfGranted).unwrap();
        let interp = open_verified(&interp, Some(euid()), Lease::IfGranted).unwrap();
        if let Ok(mut cmd) = command(&prog, Some(&interp), &[], &[("PATH", "/usr/bin:/bin")], &[]) {
            let out = cmd.stdout(std::process::Stdio::piped()).output();
            let text = out
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            assert!(
                !text.contains("UNPINNED-INTERPRETER-LINE-RAN"),
                "ATTACK: an interpreter that is itself a script ran through its own #! line, an \
                 interpreter no pin covers: {text:?}"
            );
        }
    }

    /// Bytes that are not the pin are refused (control for every test above:
    /// the hash is of the descriptor).
    #[test]
    fn bytes_other_than_the_pin_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("launcher.sh");
        let mut pin = write_exec(&p, "#!/bin/sh\necho hi\n");
        pin.sha256 = "0".repeat(64);
        let why = open_verified(&pin, Some(euid()), Lease::IfGranted)
            .expect_err("ATTACK: bytes other than the pin were verified for exec");
        assert!(why.contains("not its pin"), "{why}");
    }
}
