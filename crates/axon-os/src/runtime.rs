//! R21 §4.7 + §5.1 — the `Runtime` seam: the firewall between the pure
//! supervisor and the outside world (the model / the interpreter / the OS).
//!
//! The supervisor is generic over `Runtime`, so S1–S5 are testable with a
//! `MockRuntime` and never touch `axon-core`. The real `AxonCoreRuntime` (the
//! only impure module) lands in S6.

use crate::gate::DeclaredEffects;
use crate::grant::{Budget, EffectSet, Grant};
use crate::record::RawEvent;
use crate::verdict::Verdict;
use std::os::unix::process::CommandExt;
use std::path::Path;

/// An opaque handle to a minted Principal in the runtime's registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrincipalHandle(pub usize);

/// The result of running a program inside the sandbox: the observed
/// capability-bearing actions and the sealing verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub events: Vec<RawEvent>,
    pub verdict: Verdict,
}

/// What kind of confinement a runtime actually provides (B259).
///
/// A label, not a tier: it says what the runtime IS so a request that needs
/// something stronger can be refused rather than silently downgraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    /// A child process of the supervisor on the same host, same uid, same
    /// kernel. Capability limits are enforced by the Axon interpreter's
    /// sandbox (`sandbox_run`), not by any OS/VM boundary. The label is
    /// `process_scoped`.
    ProcessScoped,
    /// A KVM microVM that has NOT been physically qualified: Firecracker
    /// launched without jailer, booting the custom Axon guest kernel (not
    /// Linux). Hardware virtualisation is present; the host-side enclosure
    /// and the guest are not what a protected profile requires (B263).
    KvmMicroVmUnqualified,
    /// The B263 `linux-microvm-protected` profile: Firecracker under jailer,
    /// pinned Linux guest, empty netns, host cgroup limits. axon-os takes this
    /// label from the runtime that states it; the runtime is responsible for
    /// stating it only when its pinned artifacts match the qualification
    /// evidence (axon-fabric checks manifest sha256 vs the evidence record).
    LinuxMicroVmProtected,
}

impl Isolation {
    pub fn label(self) -> &'static str {
        match self {
            Isolation::ProcessScoped => "process_scoped",
            Isolation::KvmMicroVmUnqualified => "kvm_microvm_unqualified",
            Isolation::LinuxMicroVmProtected => "linux_microvm_protected",
        }
    }
}

/// What confinement a request REQUIRES.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IsolationRequirement {
    /// Any runtime will do (every existing caller: the legacy semantics).
    #[default]
    Any,
    /// Hardware isolation that has been QUALIFIED. Only
    /// [`Isolation::LinuxMicroVmProtected`] satisfies it: a KVM microVM that
    /// was never physically qualified (no jailer, unpinned guest — B263) has
    /// hardware virtualisation present but no evidence that the enclosure
    /// holds, so it is NOT accepted as hardware isolation (D-020 / B259).
    HardwareIsolated,
    /// A QUALIFIED protected Linux microVM. Only
    /// [`Isolation::LinuxMicroVmProtected`] satisfies it; nothing is ever
    /// routed to a subprocess or an unqualified VM as a fallback
    /// (G13-r22-no-weak-fallback).
    MicroVm,
}

impl IsolationRequirement {
    pub fn satisfied_by(self, iso: Isolation) -> bool {
        match (self, iso) {
            (IsolationRequirement::Any, _) => true,
            // Enumerated exhaustively, with NO wildcard: a new isolation
            // level must be classified here explicitly or the crate does not
            // compile — it can never silently satisfy a hardware requirement.
            (IsolationRequirement::HardwareIsolated, Isolation::ProcessScoped) => false,
            (IsolationRequirement::HardwareIsolated, Isolation::KvmMicroVmUnqualified) => false,
            (IsolationRequirement::HardwareIsolated, Isolation::LinuxMicroVmProtected) => true,
            (IsolationRequirement::MicroVm, Isolation::ProcessScoped) => false,
            (IsolationRequirement::MicroVm, Isolation::KvmMicroVmUnqualified) => false,
            (IsolationRequirement::MicroVm, Isolation::LinuxMicroVmProtected) => true,
        }
    }
}

/// The entry program of a supervised run, read ONCE (AR1). `text` is what
/// `authorize` verified the token against, what the static scan judges, and
/// what the sandbox stages: no runtime re-reads the entry. `path` names the
/// entry (the wrapper's file stem, the child's cwd); it is never read again.
#[derive(Debug, Clone, Copy)]
pub struct ProgramSource<'a> {
    pub path: &'a Path,
    pub text: &'a str,
}

/// The seam. Every method that touches the model/interpreter/OS lives here.
pub trait Runtime {
    /// What confinement this runtime provides. Defaults to the WEAKEST label,
    /// so a runtime that forgets to say can only under-claim.
    fn isolation(&self) -> Isolation {
        Isolation::ProcessScoped
    }

    /// Read the entry program: the supervisor's one read per run (AR1).
    /// `Err` names why it cannot be read; the supervisor refuses the run on
    /// the `io` axis.
    fn load_program(&self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|e| e.to_string())
    }

    /// The effect row the program text declares it may perform. An absent
    /// declaration MUST map to `DeclaredEffects::unknown()` (deny-by-default).
    fn declared_effects(&self, program: &ProgramSource) -> DeclaredEffects;

    /// Mint a Principal holding exactly `grant`. Attenuation (no authority the
    /// supervisor lacks) is guaranteed by the caller passing the effective
    /// grant `J ∩ S`; the runtime mints to that, no more.
    fn mint_principal(&self, grant: &Grant) -> PrincipalHandle;

    /// Run `program.text` as `principal` inside a sandbox enforcing `ceiling` +
    /// `budget` with a fixed `seed`. Returns the observed events and the verdict
    /// (mapping any runtime over-reach to Denied/BudgetExhausted/RefineViolation).
    /// AUDIT T3: takes the full `Grant`, not just its induced `EffectSet`.
    /// `effect_set()` reduces the grant to four booleans, discarding the path
    /// prefixes and host allowlists — so the ceiling could only ever express
    /// "may write SOMEWHERE", never "may write ./out/". The scoped runtime
    /// needs the allowlists themselves.
    ///
    /// G1e: `required_imports` is the `imports_digest` the approval token
    /// bound. The process that loads the program's modules must refuse to run
    /// a different closure; a runtime that cannot make it do so must refuse
    /// the run rather than ignore the binding.
    ///
    /// AR1: the runtime executes `program.text`, the bytes the approval
    /// verified, and never re-reads `program.path`.
    fn run_sandboxed(
        &self,
        program: &ProgramSource,
        principal: &PrincipalHandle,
        grant: &Grant,
        budget: &Budget,
        seed: u64,
        required_imports: Option<&str>,
    ) -> RunOutcome;

    /// The `imports_digest` of `program`'s import closure as THIS runtime's
    /// interpreter would load it under `grant` (`axon imports --json` in the
    /// job's environment). Used to admit a legacy `axon-approval/1` token,
    /// which binds no closure, only for an import-free program. `Err` when it
    /// cannot be determined — which refuses such a token.
    fn imports_digest(&self, program: &Path, grant: &Grant) -> Result<String, String>;
}

// ── S6: the real runtime — hermetic, isolated subprocess execution (A4) ──────

use crate::gate::DeclaredEffects as Decl;
use crate::grant::Label;
use crate::record::RawEvent as RE;
use std::os::fd::{AsRawFd, RawFd};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The real `Runtime`: runs programs by invoking the canonical `axon`
/// interpreter in a fresh, time-bounded subprocess (R21 §4.4). The only impure
/// module. Capability enforcement is layered: the supervisor's static gate
/// (S3) denies a program whose declared effects exceed the grant BEFORE this
/// runs; here we additionally bound execution in time and map the interpreter's
/// fail-closed exit codes (6/7/8) back to verdicts.
///
/// B259: this is the LEGACY PROCESS ADAPTER and says so — its
/// [`Runtime::isolation`] is `process_scoped`, so a request requiring a
/// microVM is refused before it gets here. Its wrapper is staged in a private
/// per-run `0700` directory (not a guessable name in shared `/tmp`), and what
/// it captures from the child is bounded ([`DEFAULT_MAX_CAPTURE_BYTES`] per
/// stream, head + tail kept).
pub struct AxonCoreRuntime {
    axon_bin: PathBuf,
    timeout: Duration,
    /// Parent of the per-run private staging directories.
    staging_root: PathBuf,
    /// Per-stream capture bound (bytes).
    max_capture: usize,
}

/// Default per-stream bound on captured child output: 1 MiB.
pub const DEFAULT_MAX_CAPTURE_BYTES: usize = 1 << 20;

impl AxonCoreRuntime {
    /// Override where private per-run staging directories are created
    /// (default: the system temp dir). Each run still gets its own fresh
    /// `0700` directory underneath.
    pub fn with_staging_root(mut self, root: PathBuf) -> Self {
        self.staging_root = root;
        self
    }

    /// Override the per-stream capture bound.
    pub fn with_max_capture(mut self, bytes: usize) -> Self {
        self.max_capture = bytes.max(2);
        self
    }

    /// Resolve the canonical entrypoint from `AXON_BIN` (an absolute path, not
    /// an ambient PATH search) or a sensible default, and the timeout from
    /// `AXON_OS_TIMEOUT_MS` (default 30s).
    pub fn from_env() -> Self {
        let axon_bin = std::env::var_os("AXON_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/debug/axon"));
        let ms = std::env::var("AXON_OS_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30_000);
        Self::with_bin_and_timeout(axon_bin, Duration::from_millis(ms))
    }

    pub fn with_bin_and_timeout(axon_bin: PathBuf, timeout: Duration) -> Self {
        AxonCoreRuntime {
            axon_bin: absolutize(axon_bin),
            timeout,
            staging_root: std::env::temp_dir(),
            max_capture: DEFAULT_MAX_CAPTURE_BYTES,
        }
    }
}

/// A private, per-run staging directory: created fresh with mode `0700`
/// (`create`, not `create_all`, so a pre-existing path — e.g. a symlink an
/// attacker planted under a predictable name — is an error, not reused), and
/// removed with everything in it on drop.
///
/// Replaces `$TMPDIR/axon-os-wrap-<pid>-<stem>.ax`, a predictable name in a
/// shared directory that any local user could pre-create or read.
pub(crate) struct StagingDir {
    path: PathBuf,
}

impl StagingDir {
    pub(crate) fn create(root: &Path) -> std::io::Result<StagingDir> {
        use std::os::unix::fs::DirBuilderExt;
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut last = None;
        for attempt in 0..8u32 {
            let path = root.join(format!(
                "axon-os-run-{}-{}-{n}-{attempt}",
                std::process::id(),
                done_nonce()
            ));
            match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(StagingDir { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("staging dir collision")))
    }

    /// Write a new file inside the directory (`create_new`: never follows or
    /// overwrites an existing entry).
    pub(crate) fn write_new(&self, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let p = self.path.join(name);
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&p)?;
        f.write_all(bytes)?;
        Ok(p)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Bounded capture of one child stream: keeps the first `cap/2` and the last
/// `cap/2` bytes and counts what was dropped. The whole stream is still READ
/// (so the child never blocks on a full pipe — T25), only not retained.
///
/// Head AND tail, not just head: the wrapper's completion marker is the LAST
/// line of stdout, and a job that printed more than the bound before it would
/// otherwise lose the marker and be sealed as not-completed.
///
/// `readable` is asked before every read and returns false to stop reading
/// before EOF (G2c: see [`join_drains`]); the capture then says so.
fn read_bounded<R: std::io::Read>(
    mut r: R,
    cap: usize,
    mut readable: impl FnMut() -> bool,
) -> (String, u64) {
    let half = (cap / 2).max(1);
    let mut head: Vec<u8> = Vec::new();
    let mut tail: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
    let mut dropped: u64 = 0;
    let mut stopped = false;
    let mut buf = [0u8; 8192];
    loop {
        if !readable() {
            stopped = true;
            break;
        }
        let n = match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let mut chunk = &buf[..n];
        if head.len() < half {
            let take = (half - head.len()).min(chunk.len());
            head.extend_from_slice(&chunk[..take]);
            chunk = &chunk[take..];
        }
        for &b in chunk {
            if tail.len() == half {
                tail.pop_front();
                dropped += 1;
            }
            tail.push_back(b);
        }
    }
    let mut out = String::from_utf8_lossy(&head).into_owned();
    if dropped > 0 {
        out.push_str(&format!(
            "\n[axon-os: {dropped} bytes of output omitted (capture bound {cap})]\n"
        ));
    }
    let tail: Vec<u8> = tail.into_iter().collect();
    out.push_str(&String::from_utf8_lossy(&tail));
    if stopped {
        out.push_str(&format!(
            "\n[axon-os: capture stopped {} ms after the job ended: a process outside \
             the job's process group still holds this stream open]\n",
            DRAIN_GRACE.as_millis()
        ));
    }
    (out, dropped)
}

/// Resolve a possibly-relative entrypoint to an absolute path NOW (against the
/// current dir), so a later `current_dir` change on the child can't break the
/// interpreter lookup (hermetic — no ambient PATH search at spawn time).
fn absolutize(p: PathBuf) -> PathBuf {
    if p.is_absolute() {
        return p;
    }
    std::fs::canonicalize(&p)
        .unwrap_or_else(|_| std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p))
}

/// Outcome of a bounded subprocess: the exit code (None ⇒ killed by timeout
/// or by a signal to the supervisor).
struct ProcOutcome {
    code: Option<i32>,
    /// AUDIT T44: stdout was drained (T25) but DISCARDED. It carries the
    /// wrapper's result sentinel, which is what lets the verdict be decided by
    /// the exit code instead of by matching prose in stderr.
    stdout: String,
    stderr: String,
    timed_out: bool,
    /// R27/R29: true if the process was killed because the kill file latch was
    /// tripped (by `axon-os kill` for R27, or by the R29 ComplianceMonitor).
    killed_by_latch: bool,
    /// G2d: the termination signal the supervisor received, if that is what
    /// ended the job (see [`forward_termination_signals`]).
    killed_by_signal: Option<i32>,
}

// ── G2d: a signal to the supervisor reaches the job ─────────────────────────
//
// `process_group(0)` (below) takes the job OUT of the supervisor's process
// group, which is what lets one `killpg` reach all of it — and also what keeps
// the terminal's Ctrl-C (SIGINT to the FOREGROUND group) from reaching it.
// Before this, Ctrl-C, SIGTERM or SIGHUP to `axon-os run` killed the supervisor
// and left the job running, re-parented to init, with nothing left to kill it
// at its timeout or on its latch, and no record of the run.

/// The signals forwarded to every running job, as SIGKILL to its group.
const TERMINATION_SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

/// The process groups of the jobs running now, read by the signal handler.
/// Fixed-size and lock-free because a handler may neither allocate nor lock;
/// `0` is a free slot. Several `run_bounded_capped` calls can be in flight at
/// once (the test suites run them on parallel threads). A job that finds every
/// slot taken runs unregistered: the handler cannot reach it, and the poll loop
/// kills it on its next pass instead (it checks [`termination_signal`]).
static JOB_GROUPS: [AtomicI32; 64] = [const { AtomicI32::new(0) }; 64];

/// The first termination signal received (0: none).
static TERMINATION: AtomicI32 = AtomicI32::new(0);

/// The signal handler. Async-signal-safe calls only: atomics, `killpg`, and
/// errno, which it saves and restores (a handler must not clobber the errno of
/// the code it interrupted).
extern "C" fn on_termination_signal(sig: libc::c_int) {
    // SAFETY: `__errno_location` returns this thread's errno slot.
    let errno = unsafe { *libc::__errno_location() };
    // The flag FIRST: the poll loop that sees the job die must see why.
    let _ = TERMINATION.compare_exchange(0, sig, Ordering::SeqCst, Ordering::SeqCst);
    for slot in &JOB_GROUPS {
        let pgid = slot.load(Ordering::SeqCst);
        // `killpg(0)` is the SUPERVISOR's own group and 1 is init's: a free
        // slot (0) must never reach the call.
        if pgid > 1 {
            // SAFETY: plain syscall on a pgid registered by `spawn_registered`.
            unsafe { libc::killpg(pgid, libc::SIGKILL) };
        }
    }
    // SAFETY: as above.
    unsafe { *libc::__errno_location() = errno };
}

fn handler_address() -> libc::sighandler_t {
    on_termination_signal as extern "C" fn(libc::c_int) as libc::sighandler_t
}

/// The current disposition of `sig` (None if it cannot be read).
///
/// # Safety
/// Async-signal-safe (`sigaction` only); callable in a forked child.
unsafe fn disposition(sig: libc::c_int) -> Option<libc::sighandler_t> {
    let mut cur: libc::sigaction = std::mem::zeroed();
    (libc::sigaction(sig, std::ptr::null(), &mut cur) == 0).then_some(cur.sa_sigaction)
}

/// Set `sig`'s disposition to `handler`, with every termination signal blocked
/// while it runs and interrupted syscalls restarted.
///
/// # Safety
/// Async-signal-safe (`sigaction` only); callable in a forked child.
unsafe fn set_disposition(sig: libc::c_int, handler: libc::sighandler_t) {
    let mut act: libc::sigaction = std::mem::zeroed();
    act.sa_sigaction = handler;
    act.sa_flags = libc::SA_RESTART;
    act.sa_mask = termination_mask();
    libc::sigaction(sig, &act, std::ptr::null_mut());
}

/// The set of [`TERMINATION_SIGNALS`].
fn termination_mask() -> libc::sigset_t {
    // SAFETY: `sigemptyset` initialises the set before `sigaddset` reads it.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for sig in TERMINATION_SIGNALS {
            libc::sigaddset(&mut set, sig);
        }
        set
    }
}

/// Set this thread's signal mask (`how`: `SIG_BLOCK` / `SIG_SETMASK`) and
/// return the previous one.
///
/// # Safety
/// Async-signal-safe (`pthread_sigmask` only); callable in a forked child.
unsafe fn swap_thread_mask(how: libc::c_int, set: &libc::sigset_t) -> libc::sigset_t {
    let mut old: libc::sigset_t = std::mem::zeroed();
    libc::pthread_sigmask(how, set, &mut old);
    old
}

/// G2d: forward SIGINT, SIGTERM and SIGHUP to every running job, as SIGKILL
/// to its process group, instead of letting them kill the supervisor alone.
///
/// For a process that owns its signal handling — `axon-os run`, which calls
/// this before the job starts, lets the poll loop observe the job's death,
/// writes the run record, and only then exits 128+signal
/// ([`termination_signal`]). A signal already IGNORED stays ignored, so
/// `nohup axon-os run` still outlives its terminal.
pub fn forward_termination_signals() {
    for sig in TERMINATION_SIGNALS {
        // SAFETY: reads and sets this process's dispositions; the handler is
        // async-signal-safe.
        unsafe {
            if disposition(sig) == Some(libc::SIG_IGN) {
                continue;
            }
            set_disposition(sig, handler_address());
        }
    }
}

/// The termination signal this process has received since
/// [`forward_termination_signals`], if any (the first, if several).
pub fn termination_signal() -> Option<i32> {
    match TERMINATION.load(Ordering::SeqCst) {
        0 => None,
        sig => Some(sig),
    }
}

fn signal_name(sig: i32) -> String {
    match sig {
        libc::SIGINT => "SIGINT".into(),
        libc::SIGTERM => "SIGTERM".into(),
        libc::SIGHUP => "SIGHUP".into(),
        other => format!("signal {other}"),
    }
}

/// A job's slot in [`JOB_GROUPS`]; dropping it frees the slot.
///
/// Dropped BEFORE the job is reaped: while its leader is an unreaped zombie
/// the pid cannot be reused, so the handler can never signal a group that the
/// kernel has since given to an unrelated process.
struct JobGroupSlot(Option<&'static AtomicI32>);

impl JobGroupSlot {
    fn register(pgid: i32) -> JobGroupSlot {
        JobGroupSlot(JOB_GROUPS.iter().find(|s| {
            s.compare_exchange(0, pgid, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        }))
    }
}

impl Drop for JobGroupSlot {
    fn drop(&mut self) {
        if let Some(slot) = self.0 {
            slot.store(0, Ordering::SeqCst);
        }
    }
}

/// Spawn `cmd` and register its process group, with the termination signals
/// blocked in this thread across both, so the handler cannot run HERE between
/// the fork and the registration. (Another thread can take the signal in that
/// window; the poll loop's check of [`termination_signal`] covers it.)
///
/// The child inherits the blocked mask — std no longer resets the mask on
/// spawn — so its `pre_exec` restores the mask this thread had, after
/// resetting any disposition that is our handler to the default: the forked
/// copy would otherwise run it, against its copy of the registry, between the
/// unblock and the exec.
fn spawn_registered(cmd: &mut Command) -> std::io::Result<(Child, JobGroupSlot)> {
    // SAFETY: changes only this thread's mask; restored below on every path.
    let restore = unsafe { swap_thread_mask(libc::SIG_BLOCK, &termination_mask()) };
    let handler = handler_address();
    // SAFETY: the hook runs in the forked child before exec and calls only
    // `sigaction` and `pthread_sigmask`, both async-signal-safe.
    unsafe {
        cmd.pre_exec(move || {
            for sig in TERMINATION_SIGNALS {
                if disposition(sig) == Some(handler) {
                    set_disposition(sig, libc::SIG_DFL);
                }
            }
            swap_thread_mask(libc::SIG_SETMASK, &restore);
            Ok(())
        });
    }
    let spawned = cmd.spawn().map(|child| {
        let slot = JobGroupSlot::register(child.id() as i32);
        (child, slot)
    });
    // SAFETY: as above.
    unsafe { swap_thread_mask(libc::SIG_SETMASK, &restore) };
    spawned
}

/// Has the job's leader exited? Asked WITHOUT reaping it (`WNOWAIT`), so its
/// registry slot can be freed before the reap ([`JobGroupSlot`]).
fn job_has_exited(child: &Child) -> std::io::Result<bool> {
    // SAFETY: a zeroed siginfo_t is valid; waitid fills it for a child of ours.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id(),
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if rc != 0 {
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(e);
    }
    // WNOHANG with nothing to report leaves si_pid zero (the struct was zeroed).
    // SAFETY: si_pid is valid to read for a SIGCHLD-shaped siginfo.
    Ok(unsafe { info.si_pid() } != 0)
}

// ── G2c: the drains end even if the pipes do not ────────────────────────────

/// How long the drains may run on once the job has been reaped. Normally no
/// time is spent here: every writer is gone, so both pipes are already at EOF.
/// A descendant that left the job's group (`setsid`) and kept stdout or stderr
/// open is the exception — after a kill and after a normal exit alike — and it
/// used to block the join, and so the supervisor, until it chose to exit.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Wait until `fd` is readable or at EOF (true), or `stop` is set (false).
fn wait_readable(fd: RawFd, stop: &AtomicBool) -> bool {
    loop {
        if stop.load(Ordering::Acquire) {
            return false;
        }
        let mut p = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd for a descriptor the calling drain owns.
        let n = unsafe { libc::poll(&mut p, 1, 50) };
        if n > 0 {
            return true; // data, EOF or an error: the read says which
        }
        if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return true; // poll itself failed: let the read fail and end the drain
        }
    }
}

/// Drain one child pipe on its own thread (T25), bounded (B259) and stoppable.
fn spawn_drain<R>(pipe: Option<R>, max_capture: usize, stop: Arc<AtomicBool>) -> JoinHandle<String>
where
    R: std::io::Read + AsRawFd + Send + 'static,
{
    std::thread::spawn(move || {
        pipe.map(|p| {
            let fd = p.as_raw_fd();
            read_bounded(p, max_capture, || wait_readable(fd, &stop)).0
        })
        .unwrap_or_default()
    })
}

/// Join both drains, waiting at most [`DRAIN_GRACE`] for their EOF, then stop
/// them: each returns what it read within one poll interval.
fn join_drains(
    out_h: JoinHandle<String>,
    err_h: JoinHandle<String>,
    stop: &AtomicBool,
) -> (String, String) {
    let deadline = Instant::now() + DRAIN_GRACE;
    while !(out_h.is_finished() && err_h.is_finished()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    stop.store(true, Ordering::Release);
    (
        out_h.join().unwrap_or_default(),
        err_h.join().unwrap_or_default(),
    )
}

/// Run a command with a hard wall-clock timeout. On expiry the child is killed
/// and reaped (no leaked handle/zombie). Hermetic: a minimal explicit env.
/// stdout + stderr are captured (the interpreter prints fault diagnostics to
/// stderr, which is how we distinguish a FAULT exit from a program that simply
/// `return`s an integer — the two would otherwise collide on the exit code).
///
/// `kill_file`: if `Some(path)`, poll the file every 100 ms (R27/R29 kill-switch).
/// When the file contains `"latch":"tripped"`, SIGKILL the child immediately.
/// Returns `killed_by_latch = true` (→ `Verdict::Halted`, exit 4 for R27;
/// the R29 monitor overrides to exit 12 via `containment_violation` in cmd_run).
#[cfg(test)]
fn run_bounded(
    cmd: &mut Command,
    timeout: Duration,
    kill_file: Option<&std::path::Path>,
) -> std::io::Result<ProcOutcome> {
    run_bounded_capped(cmd, timeout, kill_file, DEFAULT_MAX_CAPTURE_BYTES)
}

/// [`run_bounded`] with an explicit per-stream capture bound (B259).
fn run_bounded_capped(
    cmd: &mut Command,
    timeout: Duration,
    kill_file: Option<&std::path::Path>,
    max_capture: usize,
) -> std::io::Result<ProcOutcome> {
    /// SIGKILL the child's whole process GROUP. Does not reap: the caller
    /// frees the job's registry slot first (G2d).
    ///
    /// Falls back to killing the child alone if the group signal fails, so a
    /// platform or permission quirk degrades to the previous behaviour rather
    /// than to no kill at all. The child was spawned as a group leader, so its
    /// pid IS the group id.
    fn kill_group(child: &mut std::process::Child) {
        let pid = child.id() as i32;
        // SAFETY: `killpg` takes a pgid and a signal and returns an int; the
        // child is a group leader so `pid` is its pgid. A failure (ESRCH: the
        // group is already gone) is not actionable here.
        let rc = unsafe { libc::killpg(pid, libc::SIGKILL) };
        if rc != 0 {
            let _ = child.kill();
        }
    }

    /// What ended the job's watch.
    enum End {
        Exited,
        TimedOut,
        Latch,
        Signal(i32),
    }

    // KILL THE JOB, NOT A PID.
    //
    // `process_group(0)` makes the child a process-group LEADER, so every
    // descendant it spawns inherits that group and one `killpg` reaches all of
    // them. Without it, `child.kill()` signals the direct child only.
    //
    // Measured before this change, through the real supervisor: a job that
    // execs `sh -c "nohup sleep 4002 &"` and then loops is killed at the
    // timeout, and the grandchild SURVIVES (0 such processes before the run,
    // 1 after). Under the `developer` profile default `exec: any` that is the
    // ordinary case, and BOTH the operator kill switch (R27) and the compliance
    // monitor (R29) terminate through this path — so neither stopped a job that
    // had spawned a subprocess.
    let cmd = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let (mut child, slot) = spawn_registered(cmd)?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    // AUDIT T25 (finding OSK-P4-H3 / O019). These handles were taken here but
    // not READ until after the try_wait/timeout loop below, so a child emitting
    // more than a pipe buffer (~64 KiB) blocked writing while the parent blocked
    // waiting for it to exit — a deadlock broken only by the wall-clock timeout.
    // Observed: a 20,000-line program that runs in 0.09s standalone was sealed
    // as `Denied{axis:"time"}` after 30s, blaming the job for being slow when it
    // was fast.
    //
    // Drain both pipes on their own threads, concurrently with the wait. The
    // timeout and kill-file polling below are untouched; only the reads move.
    //
    // B259: and BOUNDED. `read_to_string` retained everything a job printed,
    // so a job could make the supervisor allocate without limit. The pipes are
    // still drained to EOF; only `max_capture` bytes per stream are kept.
    //
    // G2c: and STOPPABLE, so the join below cannot wait on a pipe forever.
    let stop = Arc::new(AtomicBool::new(false));
    let out_h = spawn_drain(stdout_pipe, max_capture, Arc::clone(&stop));
    let err_h = spawn_drain(stderr_pipe, max_capture, Arc::clone(&stop));

    let start = Instant::now();
    let watched = loop {
        match job_has_exited(&child) {
            Ok(true) => break Ok(End::Exited),
            Ok(false) => {}
            Err(e) => {
                kill_group(&mut child);
                break Err(e);
            }
        }
        // G2d: normally the handler has already killed the group; this reaches
        // a job it could not (signalled before registration, registry full).
        if let Some(sig) = termination_signal() {
            kill_group(&mut child);
            break Ok(End::Signal(sig));
        }
        if start.elapsed() >= timeout {
            kill_group(&mut child);
            break Ok(End::TimedOut);
        }
        // R27/R29 poll-before-progress: check the kill file if set.
        if let Some(kf) = kill_file {
            if is_kill_file_tripped(kf) {
                kill_group(&mut child);
                break Ok(End::Latch);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    // Free the slot, THEN reap (see `JobGroupSlot`).
    drop(slot);
    let reaped = child.wait();
    let (stdout, stderr) = join_drains(out_h, err_h, &stop);
    let end = watched?;
    let status = reaped?;

    let killed_by_signal = match end {
        End::Signal(sig) => Some(sig),
        // The handler killed the group before the loop looked. A job that had
        // exited on its own keeps its own outcome.
        End::Exited if status.code().is_none() => termination_signal(),
        _ => None,
    };
    let code = match end {
        End::Exited => Some(status.code().unwrap_or(-1)),
        End::Latch => Some(4), // HALTED_EXIT_CODE (R27); R29 overrides to 12 in cmd_run
        End::TimedOut | End::Signal(_) => None,
    };
    Ok(ProcOutcome {
        code,
        stdout,
        stderr,
        timed_out: matches!(end, End::TimedOut),
        killed_by_latch: matches!(end, End::Latch),
        killed_by_signal,
    })
}

/// R27/R29: read the kill file and return true if the latch is tripped.
///
/// Same rule as `killchan::FileKillChannel::poll` — the finding (OSK-P4-H7)
/// names both readers, and two readers of one channel disagreeing is how the
/// contradiction got there in the first place:
///
///   - `NotFound`                 → not tripped (a valid steady state)
///   - any other read failure     → TRIPPED (the state cannot be determined)
fn is_kill_file_tripped(path: &std::path::Path) -> bool {
    match std::fs::read_to_string(path) {
        Ok(s) => s.contains("\"latch\":\"tripped\"") || s.contains("\"latch\": \"tripped\""),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    }
}

/// Extract the first `axon:` FAULT line from stderr (the human-facing reason),
/// skipping informational lines (SMT discharge summaries, the mint
/// certificate-checked notice) that aren't faults; fall back to `default`.
/// (`axon run` no longer prints its run-id by default, and the job runs under
/// `env_clear` without `AXON_RECORD`, so there is no run-id line to skip.)
/// Did the sandbox wrapper run to completion? (AUDIT T44.) True only if stdout
/// carries the exact marker for THIS run — the nonce makes it unforgeable by the
/// job, which never sees the generated wrapper.
fn ran_to_completion(stdout: &str, nonce: &str) -> bool {
    let expect = format!("{DONE_SENTINEL}{nonce}");
    stdout.lines().any(|l| l.trim() == expect)
}

fn first_axon_line(stderr: &str, default: &str) -> String {
    let is_info = |l: &str| l.starts_with("axon: SMT") || l.starts_with("axon: mint");
    stderr
        .lines()
        .find(|l| l.starts_with("axon:") && !is_info(l))
        .map(|l| l.trim_start_matches("axon:").trim().to_string())
        .unwrap_or_else(|| default.to_string())
}

/// Over-approximate a program's declared effects by scanning its source for
/// capability-bearing builtins (deny-by-default: a read error or any ambiguity
/// yields the FULL set). A fully sound extractor uses `axon ast review`; this
/// conservative scanner is sound *as a lower bound made safe by over-approx*.
/// AUDIT T54 (finding P4-OS-03). Made `pub` so `axon-intent` can call THIS
/// implementation instead of keeping its own. There were two copies of this
/// security scanner, and T4's whitespace-evasion fix landed in only one — the
/// `axon-intent` copy still read `source.contains("exec(")`, which
/// `exec (name, args)` evades, and additionally omitted `env_var` and the whole
/// `goal_run*` family and had no `mod`-import handling at all. Duplicated
/// security logic does not stay in sync; the fix is to have one copy.
pub fn scan_effects(source: &str) -> Decl {
    // AUDIT T4 (finding OSK-P4-H1). The previous implementation tested
    // `source.contains("exec(")`, which `exec (name, args)` — one space — parses
    // identically and evades. That is an UNDER-approximation, the opposite of
    // this function's documented deny-by-default contract, and it mattered
    // because runtime collapsed fs and exec into one bucket, making this scan
    // the sole exec control. (T8 has since given exec its own runtime effect, so
    // this is no longer load-bearing — but a gate whose result is shown to a
    // human as an approval assertion must not state falsehoods either way.)
    //
    // A module import means the effects live in a file we are not reading, so
    // the honest answer is the full set, not "none found here".
    if calls_name(source, "mod") || source.contains("\nmod ") || source.starts_with("mod ") {
        return Decl {
            row: EffectSet {
                fs_read: true,
                fs_write: true,
                net: true,
                exec: true,
            },
            max_label: Label::Internal,
        };
    }
    let any = |names: &[&str]| names.iter().any(|m| calls_name(source, m));
    let net = any(&[
        "http_get",
        "http_post",
        "http_sse",
        "http_sse_post",
        "ai_complete",
        "ai_extract_i64",
        "ai_extract_f64",
        "ai_extract_str",
        "ai_extract_bool",
        "ai_extract_uncertain_i64",
        "ai_extract_uncertain_f64",
        // The goal_run family re-calls @[adaptive] metrics, which may ai_complete.
        "goal_run",
        "goal_run_constrained",
        "goal_run_categorical",
        "goal_run_random",
        "goal_run_multistart",
        "goal_continue",
        "goal_eval",
    ]);
    let fs_read = any(&["read_file", "read_line", "env_var"]);
    let fs_write = any(&["write_file"]);
    let exec = any(&["exec", "spawn_proc"]);
    Decl {
        row: EffectSet {
            fs_read,
            fs_write,
            net,
            exec,
        },
        max_label: Label::Internal,
    }
}

/// True if `source` appears to CALL `name` — the identifier on a word boundary,
/// followed by optional whitespace and `(`. Deliberately over-approximates:
/// occurrences inside comments or string literals count, because a false
/// positive only narrows the grant (safe) while a false negative widens it.
fn calls_name(source: &str, name: &str) -> bool {
    let bytes = source.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut from = 0usize;
    while let Some(rel) = source[from..].find(name) {
        let start = from + rel;
        let end = start + name.len();
        from = start + 1;
        // Must not be a suffix of a longer identifier (e.g. `my_exec`),
        // and must not be a prefix of one (e.g. `exec_report`).
        if start > 0 && is_word(bytes[start - 1]) {
            continue;
        }
        let mut i = end;
        if i < bytes.len() && is_word(bytes[i]) {
            continue;
        }
        // `mod foo` is a declaration, not a call — accept a bare keyword.
        if name == "mod" {
            return true;
        }
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'(' {
            return true;
        }
    }
    false
}

/// Wrap a user program so it runs inside a RUNTIME sandbox enforcing `ceiling`.
/// Renames the user's `fn main` to `__job_entry` and appends an orchestrator
/// `main` that mints a principal and `sandbox_run`s the entry under the effect
/// ceiling. The interpreter then enforces the AI/Net/IO ceiling at builtin
/// dispatch (SandboxViolation → exit 8) — sound against renaming/indirection,
/// unlike the static source scan. The grant axes map to the interpreter's
/// coarse sandbox tags: net→{Net,AI} (model calls are AI+Net); any of
/// fs_read/fs_write/exec→IO (the sandbox's single IO bucket — finer fs-vs-exec
/// distinctions are enforced only by the static gate, a documented coarseness).
///
/// AUDIT T8: that coarseness was not merely imprecise, it was unsound for `exec`.
/// Collapsing exec into the same `IO` bucket as fs meant any job granted
/// `fs_read` or `fs_write` was granted arbitrary process spawn at runtime, and
/// the ONLY thing separating `exec: "none"` from spawn was the static source
/// scan — which a single space (`exec (`) defeats. The interpreter now requires
/// an explicit `Exec` tag for process spawning, so exec is emitted here only
/// when the grant actually carries it.
/// Prefix of the completion marker the sandbox wrapper prints (AUDIT T44).
/// Carries a per-run nonce, so a job cannot forge it: the nonce lives only in
/// the generated wrapper, which is staged outside any granted fs_read prefix.
const DONE_SENTINEL: &str = "__axon_os_done:";

/// A per-run nonce for the completion marker. Not derived from the job's seed —
/// the job can read `AXON_SEED` from its own environment, so a seed-derived
/// marker would be forgeable by the very code whose completion it attests.
fn done_nonce() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", t, std::process::id())
}

fn wrap_in_sandbox(src: &str, grant: &Grant, budget: &Budget, nonce: &str) -> String {
    let ceiling = grant.effect_set();
    let mut tags: Vec<&str> = Vec::new();
    if ceiling.net {
        tags.push("Net");
        tags.push("AI");
    }
    if ceiling.fs_read || ceiling.fs_write || ceiling.exec {
        tags.push("IO");
    }
    if ceiling.exec {
        tags.push("Exec");
    }
    let csv = tags.join(",");
    // AUDIT T3: pass the ALLOWLISTS through, not just the booleans they induce.
    // `axon-os explain` renders "This program MAY: write ./out/" to the human
    // approving the run; before this the prefixes were dropped here and the
    // runtime enforced only "may write somewhere", so that sentence was false.
    let esc = |v: &[String]| {
        v.iter()
            .map(|x| x.replace('\\', "\\\\").replace('"', "\\\""))
            .collect::<Vec<_>>()
            .join(",")
    };
    let fs_read_csv = esc(&grant.fs_read);
    let fs_write_csv = esc(&grant.fs_write);
    let net_csv = esc(&grant.net);
    // `sandbox_run(sb, fn, arg)` calls `fn(arg)`, so the entry must take one i64.
    // Axon `main` is nullary — inject an unused i64 param when renaming.
    let renamed = src
        .replace("fn main()", "fn __job_entry(_axon_arg: i64)")
        .replace("fn main ()", "fn __job_entry(_axon_arg: i64)");
    // AUDIT T44 (OSK-P4-H2 / P4-OS-21). `main` returns the job's value and
    // `axon run` propagates it as the PROCESS EXIT CODE, so a job returning 8
    // was indistinguishable from a sandbox violation. That collision is why the
    // verdict was inferred from stderr PROSE — and the prose drifted (see the
    // classifier). Emit a completion MARKER after the job returns instead: its
    // presence says "the job ran to completion", so the exit code can then be
    // read as main's return value, and its ABSENCE says the run stopped short,
    // so the exit code is a fault. Same shape as the T33 guest-kernel sentinel.
    //
    // The marker carries the job's value only implicitly (via the exit code) —
    // printing `to_str(__v)` would panic for the common `fn main()` job, whose
    // renamed entry returns unit.
    //
    // The wrapper STATES the job's value with `exit(__r)` rather than returning
    // it as a tail expression. A value falling out of `main` is an answer and may
    // not claim a ledger code — it is remapped to 1 — which is the language-level
    // half of this same collision (`interp::returned_exit_status`). A supervisor
    // deliberately propagating a job's chosen status is exactly the case `exit`
    // exists for, and the marker is still printed BEFORE it, so "did it finish?"
    // is still answered independently of the number.
    // Only a job that DECLARES a value has one to propagate. A nullary
    // `fn main()` job leaves `__r` unit, and `exit(())` is a type error — which is
    // why the tail differs by job shape rather than being uniform.
    let propagate = if src.contains("fn main() ->") || src.contains("fn main () ->") {
        "    exit(__r)\n    0\n"
    } else {
        "    0\n"
    };
    format!(
        "{renamed}\n// \u{2500}\u{2500} axon-os runtime sandbox wrapper \u{2500}\u{2500}\nfn main() -> i64 {{\n    let __p = principal_root(\"job\", {net}, {fsw}, {exec}, {budget})\n    let __sb = sandbox_create_scoped(__p, \"{csv}\", \"{fsr_l}\", \"{fsw_l}\", \"{net_l}\")\n    let __r = sandbox_run(__sb, \"__job_entry\", 0)\n    println(\"{sentinel}{nonce}\")\n{propagate}}}\n",
        sentinel = DONE_SENTINEL,
        nonce = nonce,
        net = ceiling.net,
        fsw = ceiling.fs_write,
        exec = ceiling.exec,
        budget = budget.calls.max(0),
        fsr_l = fs_read_csv,
        fsw_l = fs_write_csv,
        net_l = net_csv,
    )
}

/// G1d/G1e: the variable a job's interpreter compares its loaded import
/// closure against (axon-core `env_registry`).
pub const REQUIRE_IMPORTS_DIGEST_ENV: &str = "AXON_REQUIRE_IMPORTS_DIGEST";

/// The environment SHAPE of every interpreter child axon-os starts for a job:
/// `env_clear`, then `PATH`, then either the operator's forwarded AXON_*
/// controls (restricted) or the virtual clock (hermetic), with the program's
/// directory as cwd. The caller adds the verb and its arguments.
///
/// ONE definition, shared by the run ([`AxonCoreRuntime::run_sandboxed`]) and
/// by every question asked about the program ahead of it — notably the import
/// closure `axon-intent approve` binds into the token
/// ([`query_imports_digest`]). An approver resolving modules under a
/// different `AXON_PATH`, `HOME` or cwd than the run would bind a closure the
/// run never loads.
pub fn job_child_command(axon_bin: &Path, program: &Path, grant: &Grant) -> Command {
    let mut cmd = Command::new(axon_bin);
    cmd.env_clear();
    if let Some(p) = std::env::var_os("PATH") {
        cmd.env("PATH", p); // cc/linker discovery for the interpreter
    }
    // AUDIT T23 (findings OSK-P4-H4 / O017 / O018). `env_clear` is correct
    // and stays — but the allowlist omitted every AXON_* control an operator
    // legitimately sets, so they were silently dropped:
    //
    //   AXON_AUDIT_LEDGER — the R28 capability-audit ledger was NEVER
    //     written for a job run under the supervisor: exactly the execution
    //     path where an operator most wants that trail.
    //   AXON_AI_MOCK / AXON_AI_REPLAY — the deterministic stub and the
    //     record/replay cache were inert here, so a job fell through to the
    //     live path (observed: it then hit the AI-policy refusal, exit 5),
    //     making replay-reproducibility non-functional through axon-os,
    //     which is where it is meant to hold.
    //
    // Silent in every case: the operator sets the variable, the run
    // succeeds, and the artifact simply never appears.
    // A REPRODUCIBLE run (profile = "hermetic") is the one case where this
    // forwarding must NOT happen. Each of these can change what the run
    // does — a replay cache, a ledger path, a module search path — so a job
    // that must give the same bytes on two machines cannot inherit them
    // from whichever shell launched it.
    //
    // This, plus the virtual clock below, is the entire difference between
    // `hermetic` and `restricted`. They previously had byte-identical
    // grants, which is two names for one behaviour.
    if !grant.reproducible {
        for key in [
            "AXON_AUDIT_LEDGER",
            "AXON_AI_MOCK",
            "AXON_AI_REPLAY",
            "AXON_PATH",
            "AXON_MAX_DEPTH",
        ] {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
        }
    } else {
        // Deterministic virtual clock: `now_ms()` becomes a function of the
        // run rather than of when it happened, and `sleep_ms(n)` advances it
        // without really sleeping. Without this, a hermetic run reading the
        // clock produces different bytes every time — which is precisely
        // what the profile promises it will not do.
        cmd.env(
            "AXON_CLOCK",
            crate::profile::Profile::Hermetic.virtual_clock().unwrap(),
        );
    }
    // Relative paths in the program resolve against the job's directory, so
    // an example runs the same wherever it is invoked from (hermetic).
    if let Some(dir) = program.parent() {
        if !dir.as_os_str().is_empty() {
            cmd.current_dir(dir);
        }
    }
    cmd
}

/// `axon imports <program> --json` in the job's environment
/// ([`job_child_command`]): the `imports_digest` of the closure a run of
/// `program` under `grant` would load. `Err` when an import does not load
/// (exit 2), the interpreter cannot start, or its answer is not
/// `axon-imports/1`.
pub fn query_imports_digest(
    axon_bin: &Path,
    program: &Path,
    grant: &Grant,
    timeout: Duration,
) -> Result<String, String> {
    // Absolute: the child's cwd is the program's directory.
    let program = absolutize(program.to_path_buf());
    let mut cmd = job_child_command(axon_bin, &program, grant);
    cmd.arg("imports").arg(&program).arg("--json");
    let out = run_bounded_capped(&mut cmd, timeout, None, DEFAULT_MAX_CAPTURE_BYTES)
        .map_err(|e| format!("cannot run `{} imports`: {e}", axon_bin.display()))?;
    if out.timed_out || out.code != Some(0) {
        let why = out
            .stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no diagnostic")
            .trim()
            .to_string();
        return Err(format!(
            "`axon imports {}` failed ({}): {why}",
            program.display(),
            if out.timed_out {
                "timed out".to_string()
            } else {
                format!("exit {:?}", out.code)
            }
        ));
    }
    let v: serde_json::Value = serde_json::from_str(out.stdout.trim())
        .map_err(|e| format!("`axon imports --json` printed no JSON: {e}"))?;
    if v["schema"] != "axon-imports/1" {
        return Err(format!(
            "`axon imports --json` answered schema {}, not axon-imports/1",
            v["schema"]
        ));
    }
    match v["imports_digest"].as_str() {
        Some(d) if d.starts_with("axsha256:") => Ok(d.to_string()),
        _ => Err("`axon imports --json` printed no axsha256: imports_digest".into()),
    }
}

/// The interpreter's G1d refusal line, when the run stopped on it: exit 8, the
/// wrapper never completed, and stderr carries
/// `error: AXON_REQUIRE_IMPORTS_DIGEST: …`.
fn imports_refusal(stderr: &str, code: Option<i32>, stdout: &str, nonce: &str) -> Option<String> {
    if code != Some(8) || ran_to_completion(stdout, nonce) {
        return None;
    }
    let prefix = format!("error: {REQUIRE_IMPORTS_DIGEST_ENV}:");
    stderr
        .lines()
        .find(|l| l.starts_with(&prefix))
        .map(|l| l.trim().to_string())
}

impl Runtime for AxonCoreRuntime {
    /// Stated explicitly, not inherited from the default, so a change to the
    /// trait default cannot silently relabel the legacy adapter.
    fn isolation(&self) -> Isolation {
        Isolation::ProcessScoped
    }

    fn declared_effects(&self, program: &ProgramSource) -> DeclaredEffects {
        scan_effects(program.text)
    }

    fn mint_principal(&self, _grant: &Grant) -> PrincipalHandle {
        // Attenuation is enforced by the supervisor passing the EFFECTIVE grant
        // (J ∩ S, R20-proven ⊆) to the gate before we run; the handle is opaque.
        PrincipalHandle(0)
    }

    fn imports_digest(&self, program: &Path, grant: &Grant) -> Result<String, String> {
        query_imports_digest(&self.axon_bin, program, grant, self.timeout)
    }

    fn run_sandboxed(
        &self,
        program: &ProgramSource,
        _principal: &PrincipalHandle,
        grant: &Grant,
        budget: &Budget,
        seed: u64,
        required_imports: Option<&str>,
    ) -> RunOutcome {
        let ceiling = grant.effect_set();
        // RUNTIME ENFORCEMENT (the sound fence). Rather than running the program
        // raw, wrap it: mint a principal + `sandbox_run` it inside an effect
        // ceiling. The interpreter then refuses ANY builtin whose effect row
        // (AI/Net/IO) exceeds the ceiling — SandboxViolation, exit 8 — at the
        // builtin-dispatch level, so an effect cannot be hidden by renaming or
        // indirection the way the static-gate source scan could be fooled. The
        // static gate is a best-effort PRE-check; THIS is what actually contains.
        //
        // AR1: the text staged is the text the approval verified and the static
        // scan judged; the entry file is not read again.
        let nonce = done_nonce();
        let wrapper_src = wrap_in_sandbox(program.text, grant, budget, &nonce);
        // B259: a PRIVATE per-run 0700 directory, removed on every exit path
        // when `staging` drops. The wrapper name inside it is fixed; its
        // uniqueness comes from the directory.
        let stem = program
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| {
                s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
            .unwrap_or("job");
        let staging = match StagingDir::create(&self.staging_root) {
            Ok(d) => d,
            Err(e) => {
                return RunOutcome {
                    events: vec![],
                    verdict: Verdict::Denied {
                        reason: format!("cannot create private staging dir: {e}"),
                        axis: "io".into(),
                    },
                }
            }
        };
        let wrapper_path =
            match staging.write_new(&format!("{stem}.wrap.ax"), wrapper_src.as_bytes()) {
                Ok(p) => p,
                Err(_) => {
                    return RunOutcome {
                        events: vec![],
                        verdict: Verdict::Denied {
                            reason: "cannot stage sandbox wrapper".into(),
                            axis: "io".into(),
                        },
                    };
                }
            };
        debug_assert!(wrapper_path.starts_with(staging.path()));

        let mut cmd = job_child_command(&self.axon_bin, program.path, grant);
        cmd.arg("run").arg(&wrapper_path);
        cmd.env("AXON_SEED", seed.to_string());
        // G1e: the closure the approval bound. Set AFTER the helper's
        // `env_clear`, whichever reproducible branch it took, so the child's
        // loader compares what it actually loaded and refuses (exit 8, nothing
        // executed) when a module changed since approval. The interpreter
        // removes it from its own environment after the check.
        if let Some(digest) = required_imports {
            cmd.env(REQUIRE_IMPORTS_DIGEST_ENV, digest);
        }

        // R27/R29: if AXON_KILL_FILE is set, poll it during run_bounded.
        // R27 uses this for operator kill (`axon-os kill`); R29 uses it for the
        // compliance monitor. Both write `{"latch":"tripped"}` to stop the job.
        let kill_file_env = std::env::var_os("AXON_KILL_FILE").map(std::path::PathBuf::from);
        let kill_file = kill_file_env.as_deref();
        let proc_res = run_bounded_capped(&mut cmd, self.timeout, kill_file, self.max_capture);
        drop(staging); // removes the private dir and the wrapper in it
        let proc = match proc_res {
            Ok(p) => p,
            Err(e) => {
                return RunOutcome {
                    events: vec![],
                    verdict: Verdict::Denied {
                        reason: format!("could not launch interpreter: {e}"),
                        axis: "exec".into(),
                    },
                }
            }
        };

        // Map the run to a verdict. Faults are detected by the interpreter's
        // STDERR diagnostics, NOT by the raw exit code — because `axon run`
        // propagates `main`'s integer return value as the exit code, which would
        // otherwise collide with the carved fault codes (a program returning 7
        // is NOT budget exhaustion). A clean run ⇒ Completed{value = exit code}.
        let err = &proc.stderr;
        let verdict = if let Some(sig) = proc.killed_by_signal {
            // G2d: the supervisor was told to stop and took the job with it.
            // `axon-os run` writes this record and then exits 128+signal.
            Verdict::Halted {
                reason: format!(
                    "the supervisor received {} and killed the job's process group",
                    signal_name(sig)
                ),
            }
        } else if proc.killed_by_latch {
            // Kill file tripped: R27 operator kill → Halted (exit 4).
            // R29 monitor kill → cmd_run overrides the final exit code to 12
            // via the `containment_violation` flag (checked after supervisor::run).
            Verdict::Halted {
                reason: "kill-switch tripped by supervisor".into(),
            }
        } else if proc.timed_out {
            Verdict::Denied {
                reason: format!("timed out after {} ms", self.timeout.as_millis()),
                axis: "time".into(),
            }
        } else if let Some(line) = imports_refusal(err, proc.code, &proc.stdout, &nonce) {
            // G1e: the loader refused the closure before executing anything —
            // a module changed since approval. An approval failure, not a
            // sandbox one.
            Verdict::Denied {
                reason: format!("imports changed since approval: {line}"),
                axis: "approval".into(),
            }
        } else if err.contains("sandbox violation")
            || err.contains("SandboxViolation")
            || err.contains("not permitted by @[contained]")
            || err.contains("capability")
        {
            Verdict::Denied {
                reason: first_axon_line(err, "runtime capability/sandbox violation"),
                axis: "sandbox".into(),
            }
        } else if err.contains("budget") && (err.contains("exhaust") || err.contains("exceeded")) {
            Verdict::BudgetExhausted {
                axis: "budget".into(),
            }
        } else if err.contains("refinement violated") || err.contains("REFINE") {
            Verdict::RefineViolation {
                reason: first_axon_line(err, "refinement contract violated"),
            }
        } else if err.contains("axon: panic") {
            Verdict::Denied {
                reason: first_axon_line(err, "interpreter panic"),
                axis: "runtime".into(),
            }
        } else if err.contains("ai policy") || err.contains("E1300") {
            // AUDIT T24 (O016/O018): observed sealing as Completed{value:5}.
            // Exit 5 is AI_POLICY_EXIT_CODE — a CARVED fault code — so the
            // record stored a fault code in the `value` field of a SUCCESS
            // verdict, and a reader could not tell "returned 5" from "refused
            // by AI policy".
            Verdict::Denied {
                reason: first_axon_line(err, "AI policy refused the call"),
                axis: "ai-policy".into(),
            }
        } else if err.contains("parse error") || err.contains("type error") {
            // AUDIT T24 (O016): a program that does not compile executes ZERO
            // statements, yet sealed as `✓ completed (value=2)` because the
            // interpreter reports it as `error: parse error: ...` — no `axon:`
            // fault line, so none of the arms above matched and it fell through
            // to Completed. The tamper-evident record then attested success for
            // a job that never ran.
            Verdict::Malformed {
                reason: first_axon_line(err, "program failed to compile"),
            }
        } else {
            // AUDIT T44 (OSK-P4-H2 / P4-OS-21). This arm used to be
            // `Completed { value: exit_code }` unconditionally, so ANY fault
            // whose wording the chain above did not match sealed as a success.
            // That is not hypothetical drift — it was live. T24 added the
            // `parse error`/`type error` arm above, but `axon run` reports TYPE
            // errors as JSON diagnostics (`{"schema":"axon-diag/1",…}`) and only
            // SYNTAX errors as prose, so:
            //
            //   syntax error in a job -> "⚠ DENIED: program failed to compile", exit 8
            //   type   error in a job -> "✓ completed (value=2)",                exit 0
            //
            // Same class of job, opposite records, and the wrong one is the
            // silent one. A record that attests success for a job which executed
            // zero statements is an attestation-integrity failure — the record
            // lies, and it is hash-chained, so it lies durably.
            //
            // The exit code now decides. It is unambiguous because the wrapper
            // returns 0 and reports the job's value via RESULT_SENTINEL
            // (see `wrap_in_sandbox`) — the collision that forced the stderr
            // heuristic in the first place is gone. stderr is still read, but
            // only to phrase the human-readable `reason`.
            if ran_to_completion(&proc.stdout, &nonce) {
                // The wrapper's tail ran, so the job returned normally and the
                // exit code IS its return value — including values that collide
                // with carved fault codes, which is exactly what the old stderr
                // heuristic could not express.
                Verdict::Completed {
                    value: proc.code.unwrap_or(0) as i64,
                }
            } else {
                match proc.code.unwrap_or(-1) {
                    // Exit 0 with no completion marker means the wrapper's own tail
                    // never ran. Something stopped the job short. Refusing to seal
                    // that as a success is the whole point of this arm.
                    0 => Verdict::Denied {
                        reason: first_axon_line(
                            err,
                            "run produced no completion marker — the sandbox wrapper did not \
                         finish, so the job's outcome is unknown",
                        ),
                        axis: "runtime".into(),
                    },
                    2 => Verdict::Malformed {
                        reason: first_axon_line(err, "program failed to compile"),
                    },
                    3 => Verdict::Denied {
                        reason: first_axon_line(err, "@[verify] postcondition failed"),
                        axis: "verify".into(),
                    },
                    4 => Verdict::Halted {
                        reason: first_axon_line(err, "corrigibility kill-switch tripped"),
                    },
                    5 => Verdict::Denied {
                        reason: first_axon_line(err, "AI policy refused the call"),
                        axis: "ai-policy".into(),
                    },
                    6 => Verdict::RefineViolation {
                        reason: first_axon_line(err, "refinement contract violated"),
                    },
                    7 => Verdict::BudgetExhausted {
                        axis: "budget".into(),
                    },
                    8 => Verdict::Denied {
                        reason: first_axon_line(err, "runtime capability/sandbox violation"),
                        axis: "sandbox".into(),
                    },
                    // Any other non-zero exit is a fault we have no name for. It is
                    // NOT a completion. Naming it honestly beats sealing a lie.
                    other => Verdict::Denied {
                        reason: first_axon_line(
                            err,
                            &format!("interpreter exited {other} with no recognised diagnostic"),
                        ),
                        axis: "runtime".into(),
                    },
                }
            }
        };

        // A single audit event recording the bounded run under the ceiling.
        let events = vec![RE::new(
            "run",
            &program.path.display().to_string(),
            ceiling,
            "internal",
        )];
        RunOutcome { events, verdict }
    }
}

/// A configurable in-memory `Runtime` for testing the supervisor with no I/O.
#[cfg(any(test, feature = "mock"))]
pub struct MockRuntime {
    pub declared: DeclaredEffects,
    pub outcome: RunOutcome,
    pub mint_calls: std::cell::Cell<usize>,
    pub run_calls: std::cell::Cell<usize>,
    /// What [`Runtime::imports_digest`] answers (default: the empty closure).
    pub imports: Result<String, String>,
    /// The `required_imports` the last `run_sandboxed` call was handed.
    pub last_required_imports: std::cell::RefCell<Option<String>>,
    /// What [`Runtime::load_program`] answers, whatever the path (default: a
    /// trivial program), so a supervisor test needs no file on disk.
    pub source: Result<String, String>,
}

#[cfg(any(test, feature = "mock"))]
impl MockRuntime {
    pub fn new(declared: DeclaredEffects, outcome: RunOutcome) -> Self {
        MockRuntime {
            declared,
            outcome,
            mint_calls: std::cell::Cell::new(0),
            run_calls: std::cell::Cell::new(0),
            imports: Ok(crate::approval::EMPTY_IMPORTS_DIGEST.to_string()),
            last_required_imports: std::cell::RefCell::new(None),
            source: Ok("fn main() -> i64 { 0 }\n".to_string()),
        }
    }
}

#[cfg(any(test, feature = "mock"))]
impl Runtime for MockRuntime {
    fn load_program(&self, _path: &Path) -> Result<String, String> {
        self.source.clone()
    }
    fn declared_effects(&self, _program: &ProgramSource) -> DeclaredEffects {
        self.declared
    }
    fn mint_principal(&self, _grant: &Grant) -> PrincipalHandle {
        self.mint_calls.set(self.mint_calls.get() + 1);
        PrincipalHandle(0)
    }
    fn imports_digest(&self, _program: &Path, _grant: &Grant) -> Result<String, String> {
        self.imports.clone()
    }
    fn run_sandboxed(
        &self,
        _program: &ProgramSource,
        _principal: &PrincipalHandle,
        _grant: &Grant,
        _budget: &Budget,
        _seed: u64,
        required_imports: Option<&str>,
    ) -> RunOutcome {
        self.run_calls.set(self.run_calls.get() + 1);
        *self.last_required_imports.borrow_mut() = required_imports.map(str::to_string);
        self.outcome.clone()
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;

    #[test]
    fn run_bounded_kills_a_runaway_at_the_timeout() {
        // A `sleep 5` under a 150 ms timeout must be killed (timed_out), and the
        // call must return promptly (no leaked child, no 5 s hang).
        let start = std::time::Instant::now();
        let mut cmd = Command::new("sleep");
        cmd.arg("5");
        let out =
            run_bounded(&mut cmd, Duration::from_millis(150), None).expect("spawn sleep (POSIX)");
        assert!(out.timed_out, "runaway must be killed at the timeout");
        assert!(out.code.is_none());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "must return promptly after the kill, not wait out the sleep"
        );
    }

    // ── G2a: a kill reaches the JOB, every process in it, and nothing else ──

    /// A process a test started, known by pid AND command line: a pid the
    /// kernel has reused is never taken for it, nor killed by the cleanup. A
    /// zombie's cmdline reads empty, so killed-but-unreaped counts as gone.
    /// Dropping it SIGKILLs the process if it is still there, so a failing
    /// test leaks nothing.
    struct Tracked {
        pid: i32,
        cmdline: Vec<u8>,
    }

    impl Tracked {
        /// Wait (at most 20 s) until `pid_file` names a live process running
        /// exactly `argv`.
        fn wait_alive(pid_file: &Path, argv: &[&str]) -> Tracked {
            let mut cmdline = Vec::new();
            for a in argv {
                cmdline.extend_from_slice(a.as_bytes());
                cmdline.push(0);
            }
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let pid = std::fs::read_to_string(pid_file)
                    .ok()
                    .and_then(|s| s.trim().parse::<i32>().ok());
                if let Some(pid) = pid.filter(|&p| runs(p, &cmdline)) {
                    return Tracked { pid, cmdline };
                }
                assert!(
                    Instant::now() < deadline,
                    "{} never named a live `{}`",
                    pid_file.display(),
                    argv.join(" ")
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn alive(&self) -> bool {
            runs(self.pid, &self.cmdline)
        }

        /// Gone within `d` (a SIGKILL is delivered asynchronously).
        fn gone_within(&self, d: Duration) -> bool {
            let deadline = Instant::now() + d;
            while self.alive() {
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            true
        }
    }

    /// Is `pid` a live process whose command line is exactly `cmdline`?
    fn runs(pid: i32, cmdline: &[u8]) -> bool {
        std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c == cmdline)
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            if self.alive() {
                // SAFETY: plain syscall; the pid was just checked to be ours.
                unsafe { libc::kill(self.pid, libc::SIGKILL) };
            }
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("axon-os-g2a-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A shell job that starts a grandchild `sleep <secs>` and waits on it.
    /// The grandchild's stdio is /dev/null, so it holds none of the job's
    /// pipes: what is tested is the kill, not the drain.
    fn job_with_grandchild(pid_file: &Path, secs: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(format!(
            "sleep {secs} </dev/null >/dev/null 2>&1 & echo $! > '{}'; wait",
            pid_file.display()
        ));
        cmd
    }

    /// The timeout kill takes the job's grandchild with it. Before 821cf767 it
    /// signalled the direct child only, and the grandchild outlived the run.
    #[test]
    fn the_timeout_kill_reaches_the_jobs_grandchild() {
        let d = scratch("timeout");
        let pid_file = d.join("grandchild.pid");
        let mut cmd = job_with_grandchild(&pid_file, "3621");
        let run = std::thread::spawn(move || run_bounded(&mut cmd, Duration::from_secs(3), None));
        let grandchild = Tracked::wait_alive(&pid_file, &["sleep", "3621"]);
        let out = run.join().unwrap().expect("spawn sh");
        assert!(
            out.timed_out,
            "the job itself must be killed at the timeout"
        );
        assert!(
            grandchild.gone_within(Duration::from_secs(5)),
            "ATTACK: the job's grandchild (pid {}) outlived the timeout kill",
            grandchild.pid
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The kill-latch path (R27 operator kill, R29 monitor) takes the job's
    /// grandchild with it too.
    #[test]
    fn the_latch_kill_reaches_the_jobs_grandchild() {
        let d = scratch("latch");
        let pid_file = d.join("grandchild.pid");
        let latch = d.join("run.kill");
        std::fs::write(&latch, r#"{"latch":"clear"}"#).unwrap();
        let mut cmd = job_with_grandchild(&pid_file, "3622");
        let polled = latch.clone();
        let run = std::thread::spawn(move || {
            run_bounded(&mut cmd, Duration::from_secs(60), Some(&polled))
        });
        let grandchild = Tracked::wait_alive(&pid_file, &["sleep", "3622"]);
        std::fs::write(&latch, r#"{"latch":"tripped","reason":"g2a"}"#).unwrap();
        let out = run.join().unwrap().expect("spawn sh");
        assert!(
            out.killed_by_latch,
            "the job itself must be killed by the latch"
        );
        assert!(
            grandchild.gone_within(Duration::from_secs(5)),
            "ATTACK: the job's grandchild (pid {}) outlived the latch kill",
            grandchild.pid
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The group kill reaches ONLY the job's group: a process in another
    /// group survives it (the `kill(-1)` class of bug, see axon-intent
    /// synth.rs `the_timeout_kill_reaches_only_the_childs_own_group`).
    #[test]
    fn the_timeout_kill_spares_a_bystander_in_another_group() {
        let mut bystander = Command::new("sleep");
        bystander.arg("30").process_group(0);
        let mut bystander = bystander.spawn().expect("spawn bystander");
        let mut cmd = Command::new("sleep");
        cmd.arg("3623");
        let out = run_bounded(&mut cmd, Duration::from_millis(300), None).expect("spawn sleep");
        assert!(out.timed_out);
        // `try_wait`, not `kill(pid, 0)`: an unreaped SIGKILLed child is a
        // zombie, and kill(pid, 0) succeeds on a zombie.
        let state = bystander.try_wait().expect("poll bystander");
        let _ = bystander.kill();
        let _ = bystander.wait();
        assert!(
            state.is_none(),
            "ATTACK: a process in an UNRELATED group was killed by the job's \
             timeout kill ({state:?})"
        );
    }

    #[test]
    fn read_bounded_keeps_head_and_tail_and_counts_the_rest() {
        let data: Vec<u8> = (0..10_000u32).map(|i| b'a' + (i % 26) as u8).collect();
        let (s, dropped) = read_bounded(&data[..], 100, || true);
        assert_eq!(dropped, 10_000 - 100);
        assert!(s.starts_with(std::str::from_utf8(&data[..50]).unwrap()));
        assert!(s.ends_with(std::str::from_utf8(&data[10_000 - 50..]).unwrap()));
        assert!(s.contains("9900 bytes of output omitted"));
        // Under the bound: byte-identical, no annotation.
        let (s, dropped) = read_bounded(&b"hello\n"[..], 100, || true);
        assert_eq!((s.as_str(), dropped), ("hello\n", 0));
    }

    #[test]
    fn run_bounded_capped_drains_a_flood_without_retaining_it() {
        // 8 MiB of output, 4 KiB bound: must neither deadlock (T25) nor retain.
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("head -c 8388608 /dev/zero | tr '\\0' x; echo; echo LAST");
        let out = run_bounded_capped(&mut cmd, Duration::from_secs(20), None, 4096).unwrap();
        assert_eq!(out.code, Some(0));
        assert!(out.stdout.len() < 4096 + 200, "{}", out.stdout.len());
        assert!(out.stdout.trim_end().ends_with("LAST"));
    }

    #[test]
    fn staging_dir_is_private_fresh_and_removed() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("axon-os-stg-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let d = StagingDir::create(&root).unwrap();
        let mode = std::fs::metadata(d.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let f = d.write_new("w.ax", b"x").unwrap();
        assert_eq!(
            std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(d.write_new("w.ax", b"y").is_err(), "never overwrites");
        let p = d.path().to_path_buf();
        let d2 = StagingDir::create(&root).unwrap();
        assert_ne!(d2.path(), p, "each run gets its own directory");
        drop(d);
        assert!(!p.exists());
        drop(d2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_the_protected_linux_profile_satisfies_a_microvm_requirement() {
        use IsolationRequirement as R;
        let all = [
            Isolation::ProcessScoped,
            Isolation::KvmMicroVmUnqualified,
            Isolation::LinuxMicroVmProtected,
        ];
        for iso in all {
            assert!(R::Any.satisfied_by(iso));
            assert_eq!(
                R::MicroVm.satisfied_by(iso),
                iso == Isolation::LinuxMicroVmProtected,
                "{iso:?}"
            );
            // D-020: an UNQUALIFIED KVM microVM used to satisfy
            // HardwareIsolated (`(HardwareIsolated, _) => true`). Hardware
            // virtualisation being present is not qualified isolation, so
            // only the qualified protected profile satisfies it now.
            assert_eq!(
                R::HardwareIsolated.satisfied_by(iso),
                iso == Isolation::LinuxMicroVmProtected,
                "{iso:?}"
            );
        }
    }

    #[test]
    fn the_legacy_adapter_is_labelled_process_scoped() {
        let rt = AxonCoreRuntime::with_bin_and_timeout("/bin/false".into(), Duration::from_secs(1));
        assert_eq!(rt.isolation(), Isolation::ProcessScoped);
        assert_eq!(rt.isolation().label(), "process_scoped");
        assert!(!IsolationRequirement::MicroVm.satisfied_by(rt.isolation()));
        assert!(IsolationRequirement::Any.satisfied_by(rt.isolation()));
    }

    #[test]
    fn run_bounded_captures_a_fast_command() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo 41");
        let out = run_bounded(&mut cmd, Duration::from_secs(5), None).expect("spawn sh");
        assert!(!out.timed_out);
        assert_eq!(out.code, Some(0));
    }

    #[test]
    fn run_bounded_trips_latch_via_kill_file() {
        // R27/R29: when the kill file is tripped mid-run, the subprocess is killed
        // with killed_by_latch=true.
        let dir = std::env::temp_dir().join(format!("axon-r27r29-rt-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let kill_path = dir.join("latch.json");
        // Start clear.
        std::fs::write(&kill_path, r#"{"latch":"clear"}"#).unwrap();
        // Run a long sleep.
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        let kill_path_clone = kill_path.clone();
        // Trip the latch from a background thread after 100ms.
        let _handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let _ = std::fs::write(&kill_path_clone, r#"{"latch":"tripped","reason":"test"}"#);
        });
        let start = std::time::Instant::now();
        let out = run_bounded(&mut cmd, Duration::from_secs(10), Some(&kill_path)).unwrap();
        assert!(out.killed_by_latch, "must be killed by latch");
        assert_eq!(out.code, Some(4), "killed_by_latch returns code 4");
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "must return quickly after latch trip"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_effects_is_not_evaded_by_whitespace_or_module_imports() {
        // AUDIT T4 (OSK-P4-H1). `source.contains("exec(")` was defeated by a
        // single space, so a job declaring exec:"none" scanned clean while
        // calling exec. The scan's result is rendered to a human as an approval
        // assertion, so an under-approximation makes the UI state a falsehood.
        assert!(
            scan_effects("fn f() { exec(\"id\", []) }").row.exec,
            "baseline: exec( must be detected"
        );
        for evasion in [
            "fn f() { exec (\"id\", []) }",
            "fn f() { exec\t(\"id\", []) }",
            "fn f() { exec\n        (\"id\", []) }",
        ] {
            assert!(
                scan_effects(evasion).row.exec,
                "whitespace before `(` must not evade the exec scan: {evasion}"
            );
        }
        // Word boundaries: a longer identifier must NOT trip the exec axis.
        assert!(
            !scan_effects("fn f() { my_exec(1) }").row.exec,
            "`my_exec` is not `exec`"
        );
        assert!(
            !scan_effects("fn f() { exec_report(1) }").row.exec,
            "`exec_report` is not `exec`"
        );
        // A module import puts effects in a file this scanner never reads, so
        // the honest answer is the full set, not "nothing found here".
        let m = scan_effects("mod util\nfn main() { util.go() }").row;
        assert!(
            m.exec && m.net && m.fs_read && m.fs_write,
            "a mod import must deny-by-default to the full effect set"
        );
        // Previously-invisible capability builtins are now classified.
        assert!(scan_effects("fn f() { env_var(\"X\") }").row.fs_read);
        assert!(scan_effects("fn f() { goal_run(\"g\", 1) }").row.net);
        let sse = scan_effects("fn f() { http_sse_post(\"u\", \"b\") }").row;
        assert!(sse.net);
        // A pure program stays pure.
        let pure = scan_effects("fn add(a: i64, b: i64) -> i64 { a + b }").row;
        assert!(!pure.exec && !pure.net && !pure.fs_read && !pure.fs_write);
    }
}
