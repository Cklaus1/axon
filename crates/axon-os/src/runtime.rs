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
#[cfg(test)]
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
use axon_jobctl::{join_within, termination_signal, wait_readable, Job, DRAIN_GRACE};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
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
                run_nonce()?
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
/// before EOF (G2c: see [`axon_jobctl::join_within`]); the capture then says so.
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
    /// Bytes of stdout read but not retained (the capture bound).
    stdout_omitted: u64,
    timed_out: bool,
    /// R27/R29: true if the process was killed because the kill file latch was
    /// tripped (by `axon-os kill` for R27, or by the R29 ComplianceMonitor).
    killed_by_latch: bool,
    /// G2d: the termination signal the supervisor received, if that is what
    /// ended the job (see [`axon_jobctl::forward_termination_signals`]).
    killed_by_signal: Option<i32>,
    /// GV-1: the signal that ended the child, when one did (the deadline
    /// kill, the latch kill, or a signal nobody here sent).
    exit_signal: Option<i32>,
    /// GV-1: what the child wrote to its outcome pipe (at most
    /// `2 * OUTCOME_RECORD_MAX` bytes kept; anything over one record's bound
    /// is malformed), `None` when the run had no outcome pipe.
    outcome: Option<String>,
}

/// GV-1: the fd the child finds its outcome pipe on (`AXON_OUTCOME_FD`).
pub const OUTCOME_FD: i32 = 3;
/// genviab-b3 B3-3: the fd the child finds the nonce pipe on
/// (`AXON_OUTCOME_NONCE_FD`).
pub const OUTCOME_NONCE_FD: i32 = 4;
/// GV-1: `axon-core`'s outcome variables (`env_registry`).
pub const OUTCOME_FD_ENV: &str = "AXON_OUTCOME_FD";
pub const OUTCOME_NONCE_FD_ENV: &str = "AXON_OUTCOME_NONCE_FD";
/// GV-1: one record line's bound, newline included (`PIPE_BUF`).
pub const OUTCOME_RECORD_MAX: usize = 4096;

/// GV-1: the outcome pipe of one run: the read end the supervisor drains and
/// the write end the child gets on [`OUTCOME_FD`]. Both are close-on-exec
/// here; only the child's `dup2` copy is not.
struct OutcomePipe {
    reader: std::io::PipeReader,
    writer: std::io::PipeWriter,
}

/// GV-1 / B3-3: arrange for `cmd`'s child to find `writer` on [`OUTCOME_FD`]
/// and `nonce_reader` (a pipe already holding the nonce and `\n`, its write
/// end closed) on [`OUTCOME_NONCE_FD`], and tell its interpreter so
/// (`AXON_OUTCOME_FD`, `AXON_OUTCOME_NONCE_FD`: numbers only). The
/// interpreter reads the nonce and closes fd 4, removes both variables before
/// the program runs, makes fd 3 close-on-exec and itself non-dumpable. The
/// nonce is never in the environment, whose kernel copy `/proc/self/environ`
/// a grant on `/proc/` reads (O-GV-06).
fn give_child_outcome_fd(
    cmd: &mut Command,
    writer: &std::io::PipeWriter,
    nonce_reader: &std::io::PipeReader,
) {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    let wfd = writer.as_raw_fd();
    let nfd = nonce_reader.as_raw_fd();
    cmd.env(OUTCOME_FD_ENV, OUTCOME_FD.to_string());
    cmd.env(OUTCOME_NONCE_FD_ENV, OUTCOME_NONCE_FD.to_string());
    // SAFETY: runs in the forked child before exec and calls only `fcntl`
    // and `dup2`, both async-signal-safe.
    unsafe {
        cmd.pre_exec(move || place_outcome_fds(wfd, nfd));
    }
}

/// B3A-4: put `wfd` on [`OUTCOME_FD`] and `nfd` on [`OUTCOME_NONCE_FD`]
/// whatever raw numbers they have. Both are first copied to fds >= 10
/// (close-on-exec), so neither `dup2` can close the other's source (`nfd`
/// being raw 3, or `wfd` raw 4); `dup2` then clears close-on-exec on 3 and
/// 4, and the copies close on exec. Only async-signal-safe calls: it runs
/// between fork and exec.
fn place_outcome_fds(wfd: i32, nfd: i32) -> std::io::Result<()> {
    // SAFETY: `fcntl`/`dup2` on integer fds take no pointers.
    unsafe {
        let w = libc::fcntl(wfd, libc::F_DUPFD_CLOEXEC, 10);
        if w == -1 {
            return Err(std::io::Error::last_os_error());
        }
        let n = libc::fcntl(nfd, libc::F_DUPFD_CLOEXEC, 10);
        if n == -1 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::dup2(w, OUTCOME_FD) == -1 || libc::dup2(n, OUTCOME_NONCE_FD) == -1 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

fn signal_name(sig: i32) -> String {
    match sig {
        libc::SIGINT => "SIGINT".into(),
        libc::SIGTERM => "SIGTERM".into(),
        libc::SIGHUP => "SIGHUP".into(),
        other => format!("signal {other}"),
    }
}

/// Drain one child pipe on its own thread (T25), bounded (B259) and stoppable.
/// Returns the capture and the count of bytes read but not retained.
fn spawn_drain<R>(
    pipe: Option<R>,
    max_capture: usize,
    stop: Arc<AtomicBool>,
) -> JoinHandle<(String, u64)>
where
    R: std::io::Read + AsRawFd + Send + 'static,
{
    std::thread::spawn(move || {
        pipe.map(|p| {
            let fd = p.as_raw_fd();
            read_bounded(p, max_capture, || wait_readable(fd, &stop))
        })
        .unwrap_or_default()
    })
}

/// Join the drains, waiting at most [`DRAIN_GRACE`] for their EOF, then stop
/// them: each returns what it read within one poll interval. In the order
/// given.
fn join_drains(handles: Vec<JoinHandle<(String, u64)>>, stop: &AtomicBool) -> Vec<(String, u64)> {
    join_within(DRAIN_GRACE, handles, stop)
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect()
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
    run_bounded_capped(cmd, timeout, kill_file, DEFAULT_MAX_CAPTURE_BYTES, None)
}

/// [`run_bounded`] with an explicit per-stream capture bound (B259) and,
/// when `stdin` is `Some`, those bytes fed to the child's stdin (then closed)
/// from a detached writer thread; `None` is `/dev/null`.
fn run_bounded_capped(
    cmd: &mut Command,
    timeout: Duration,
    kill_file: Option<&std::path::Path>,
    max_capture: usize,
    stdin: Option<Vec<u8>>,
) -> std::io::Result<ProcOutcome> {
    run_bounded_outcome(cmd, timeout, kill_file, max_capture, stdin, None)
}

/// [`run_bounded_capped`] with, when `outcome` is `Some`, the run's outcome
/// pipe (GV-1): the caller has already arranged for the child to receive the
/// write end on [`OUTCOME_FD`] (see [`AxonCoreRuntime::launch`]). This
/// process's copy of the write end is dropped right after the spawn, so the
/// read end reaches EOF when the child (and anything it handed the fd to)
/// is gone; the read end is drained like stdout.
fn run_bounded_outcome(
    cmd: &mut Command,
    timeout: Duration,
    kill_file: Option<&std::path::Path>,
    max_capture: usize,
    stdin: Option<Vec<u8>>,
    outcome: Option<OutcomePipe>,
) -> std::io::Result<ProcOutcome> {
    /// What ended the job's watch.
    enum End {
        Exited,
        TimedOut,
        Latch,
        Signal(i32),
    }

    // KILL THE JOB, NOT A PID.
    //
    // `Job::spawn` makes the child a process-group LEADER, so every
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
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut job = Job::spawn(cmd)?;
    // GV-1: this process's write end goes now, so the read end's EOF means
    // every holder of the child's copy is gone.
    let outcome_reader = outcome.map(|OutcomePipe { reader, writer }| {
        drop(writer);
        reader
    });

    let stdout_pipe = job.child_mut().stdout.take();
    let stderr_pipe = job.child_mut().stderr.take();
    // The writer is never joined: a child that never reads its stdin blocks
    // it only until the group dies (then the write fails with EPIPE).
    if let (Some(bytes), Some(mut pipe)) = (stdin, job.child_mut().stdin.take()) {
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = pipe.write_all(&bytes);
        });
    }

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
    // Twice one record's bound kept: more than one record's bound is
    // malformed whatever it holds, and the classifier sees that it was more.
    let has_outcome = outcome_reader.is_some();
    let outcome_h = spawn_drain(outcome_reader, 2 * OUTCOME_RECORD_MAX, Arc::clone(&stop));

    let start = Instant::now();
    let watched = loop {
        match job.has_exited() {
            Ok(true) => break Ok(End::Exited),
            Ok(false) => {}
            Err(e) => {
                job.kill();
                break Err(e);
            }
        }
        // G2d: normally the handler has already killed the group; this reaches
        // a job it could not (signalled before registration, registry full).
        if let Some(sig) = termination_signal() {
            job.kill();
            break Ok(End::Signal(sig));
        }
        if start.elapsed() >= timeout {
            job.kill();
            break Ok(End::TimedOut);
        }
        // R27/R29 poll-before-progress: check the kill file if set.
        if let Some(kf) = kill_file {
            if is_kill_file_tripped(kf) {
                job.kill();
                break Ok(End::Latch);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    // Frees the job's registry slot, THEN reaps (axon_jobctl::Job::reap).
    let reaped = job.reap();
    let mut drained = join_drains(vec![out_h, err_h, outcome_h], &stop).into_iter();
    let (stdout, stdout_omitted) = drained.next().unwrap_or_default();
    let (stderr, _) = drained.next().unwrap_or_default();
    let outcome = drained
        .next()
        .filter(|_| has_outcome)
        .map(|(record, _)| record);
    let end = watched?;
    let status = reaped?;
    let exit_signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    };

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
        stdout_omitted,
        timed_out: matches!(end, End::TimedOut),
        killed_by_latch: matches!(end, End::Latch),
        killed_by_signal,
        exit_signal,
        outcome,
    })
}

/// What a bounded child run ended with ([`run_bounded_job`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedRun {
    /// The exit status; `None` when the run was killed (its deadline, or a
    /// termination signal to this process).
    pub code: Option<i32>,
    /// Captured stdout (head and tail within the capture bound).
    pub stdout: String,
    /// Captured stderr (head and tail within the capture bound).
    pub stderr: String,
    /// Bytes of stdout read but not retained: non-zero means `stdout` is not
    /// the whole stream.
    pub stdout_omitted: u64,
    /// The deadline killed the job's process group.
    pub timed_out: bool,
    /// The termination signal this process received, if that ended the run.
    pub killed_by_signal: Option<i32>,
}

/// Run `cmd` as a process-group job ([`axon_jobctl::Job`]) under a wall-clock
/// `timeout`, feeding `stdin` (`None`: `/dev/null`) and capturing at most
/// `max_capture` bytes per stream. At the deadline the whole group is
/// SIGKILLed and reaped. The single bounded runner shared with the axon-intent
/// generator seam; environment, arguments and cwd are the caller's.
pub fn run_bounded_job(
    cmd: &mut Command,
    timeout: Duration,
    stdin: Option<Vec<u8>>,
    max_capture: usize,
) -> std::io::Result<BoundedRun> {
    let p = run_bounded_capped(cmd, timeout, None, max_capture, stdin)?;
    Ok(BoundedRun {
        code: p.code,
        stdout: p.stdout,
        stderr: p.stderr,
        stdout_omitted: p.stdout_omitted,
        timed_out: p.timed_out,
        killed_by_signal: p.killed_by_signal,
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

/// A program's declared effects for admission, from the parsed program and
/// the compiler's one admission-axes table
/// (`axon_core::builtins::builtin_admission_axes`, through
/// `axon_core::capabilities::program_admission_axes_in`): every builtin the
/// program — and every module its `use`s load from `search_dirs` — calls or
/// uses as a value, in any fn, method or module `let`; every axis when an
/// import does not load from `search_dirs` (none given: any non-`native`
/// `use`) or the walk reaches something it cannot classify.
///
/// IG0 (F12 a, b). This was a hand-written list of nine names scanned as text:
/// `append_file` (and every other file builtin but `read_file`/`write_file`)
/// was invisible, so an `append_file` program was admitted with "It may NOT:
/// write files" and then refused by the sandbox; and a whole-word `mod`
/// anywhere — the comment `// total mod 10` — read as an import and widened
/// the row to every effect. On the AST, comments and string contents can
/// neither add nor hide an effect.
///
/// The grant has no `env` axis: an `env_var` program needs `fs_read`, which
/// is what the runtime sandbox's single `IO` tag requires of it.
///
/// `Err` (with the parser's message) when the program does not parse — a
/// refusal, never "every effect".
pub fn try_scan_effects_in(source: &str, search_dirs: &[PathBuf]) -> Result<Decl, String> {
    let program = axon_core::parse_source(source).map_err(|e| e.to_string())?;
    Ok(declared_from_axes(
        axon_core::capabilities::program_admission_axes_in(&program, search_dirs),
    ))
}

/// [`try_scan_effects_in`] with no module directory: every non-`native`
/// `use` is every axis.
pub fn try_scan_effects(source: &str) -> Result<Decl, String> {
    try_scan_effects_in(source, &[])
}

/// The grant's four axes from the table's five: `env` rides on `fs_read`
/// (see [`try_scan_effects`]).
pub fn declared_from_axes(axes: axon_core::builtins::AdmissionAxes) -> Decl {
    Decl {
        row: EffectSet {
            fs_read: axes.fs_read || axes.env,
            fs_write: axes.fs_write,
            net: axes.net,
            exec: axes.exec,
        },
        max_label: Label::Internal,
    }
}

/// [`try_scan_effects`] for the supervisor's admission gate, which has no
/// refusal channel of its own here: a program that does not parse declares
/// EVERY effect (deny by default) — it is refused unless the grant holds every
/// axis, and then the interpreter refuses to compile it.
pub fn scan_effects(source: &str) -> Decl {
    scan_effects_in(source, &[])
}

/// [`scan_effects`] with `use` imports loaded from `search_dirs`.
pub fn scan_effects_in(source: &str, search_dirs: &[PathBuf]) -> Decl {
    try_scan_effects_in(source, search_dirs).unwrap_or(Decl {
        row: EffectSet {
            fs_read: true,
            fs_write: true,
            net: true,
            exec: true,
        },
        max_label: Label::Internal,
    })
}

/// The module directories a job's interpreter can load a `use` from, in its
/// order ([`axon_core::axon_search_dirs`] under [`job_child_command`]'s
/// environment): the forwarded `AXON_PATH` (relative entries against the
/// program's directory, the child's cwd), no `~/.axon/lib` (`HOME` is
/// cleared), then the interpreter binary's own library. A reproducible grant
/// does not forward `AXON_PATH`, and the scan is not told which grant runs,
/// so BOTH orders are returned; the caller unions what each loads.
fn job_module_dir_sets(axon_bin: &Path, program: &Path) -> Vec<Vec<PathBuf>> {
    let bin_lib: Vec<PathBuf> = axon_bin
        .parent()
        .map(|d| d.join("..").join("lib").join("axon"))
        .into_iter()
        .collect();
    let base = program.parent().unwrap_or(Path::new("."));
    let forwarded: Vec<PathBuf> = std::env::var_os("AXON_PATH")
        .map(|v| {
            std::env::split_paths(&v)
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| base.join(p))
                .collect()
        })
        .unwrap_or_default();
    if forwarded.is_empty() {
        return vec![bin_lib];
    }
    let mut restricted = forwarded;
    restricted.extend(bin_lib.iter().cloned());
    vec![restricted, bin_lib]
}

/// A per-run nonce: 16 bytes from the OS RNG, hex. It marks every outcome
/// record of the run (GV-1) and reaches the interpreter only on its nonce
/// pipe (B3-3). Not derived from the job's
/// seed or the clock - the job can read `AXON_SEED` from its own environment,
/// and a guessable nonce is forgeable by the very code whose completion it
/// attests. `Err` when the RNG cannot be read (the run is then not started).
fn run_nonce() -> std::io::Result<String> {
    use std::io::Read;
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// The interpreter's sandbox tags for `grant`, as the csv both the wrapper's
/// `sandbox_create_scoped` and the child's `AXON_ALLOWED_EFFECTS` carry.
///
/// `IO` is ALWAYS in the set: it is console output and `exit`, not a
/// capability admission grants. It used to be emitted only when
/// fs_read/fs_write/exec was granted, so a print-only job was admitted
/// under an empty grant and refused here. What else `IO` covers stays
/// narrowed by the interpreter: the filesystem by the allowlists (an empty
/// list is deny-all), process spawn by its own `Exec` tag, and `env_var` by
/// the fs_read allowlist (deny-all when empty), the axis admission projects
/// an environment read onto. `Time`/`Random` stay out (examples/jobs/evasion:
/// the runtime fence for effects admission cannot represent).
fn grant_sandbox_tags(grant: &Grant) -> String {
    let ceiling = grant.effect_set();
    let mut tags: Vec<&str> = vec!["IO"];
    if ceiling.net {
        tags.push("Net");
        tags.push("AI");
    }
    if ceiling.exec {
        tags.push("Exec");
    }
    tags.join(",")
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
fn wrap_in_sandbox(src: &str, grant: &Grant, budget: &Budget) -> String {
    let ceiling = grant.effect_set();
    let csv = grant_sandbox_tags(grant);
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
    // was indistinguishable from a sandbox violation. GV-1 moved "how did the
    // run end" to the interpreter's outcome record; genviab-b3 B3-3 moved
    // "did the job finish" there too. The wrapper used to print a nonce-bearing
    // completion marker after `sandbox_run` returned, but any marker it prints
    // is a literal in this source, which the job reads through
    // `/proc/self/root` (O-GV-06). The record's class says it instead: an
    // `exit` from this `main`, outside `sandbox_run`, is class `exit`; the
    // job's own `exit` (inside `sandbox_run`, or in a module-level `let`
    // before `main`) is `inner-exit`, which `classify` does not complete.
    //
    // The wrapper STATES the job's value with `exit(__r)` rather than returning
    // it as a tail expression. A value falling out of `main` is an answer and may
    // not claim a ledger code — it is remapped to 1 — which is the language-level
    // half of this same collision (`interp::returned_exit_status`). A supervisor
    // deliberately propagating a job's chosen status is exactly the case `exit`
    // exists for.
    // Only a job that DECLARES a value has one to propagate. A nullary
    // `fn main()` job leaves `__r` unit, and `exit(())` is a type error — which is
    // why the tail differs by job shape rather than being uniform.
    let propagate = if src.contains("fn main() ->") || src.contains("fn main () ->") {
        "    exit(__r)\n    0\n"
    } else {
        "    0\n"
    };
    format!(
        "{renamed}\n// \u{2500}\u{2500} axon-os runtime sandbox wrapper \u{2500}\u{2500}\nfn main() -> i64 {{\n    let __p = principal_root(\"job\", {net}, {fsw}, {exec}, {budget})\n    let __sb = sandbox_create_scoped(__p, \"{csv}\", \"{fsr_l}\", \"{fsw_l}\", \"{net_l}\")\n    let __r = sandbox_run(__sb, \"__job_entry\", 0)\n{propagate}}}\n",
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
    let out = run_bounded_capped(&mut cmd, timeout, None, DEFAULT_MAX_CAPTURE_BYTES, None)
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

impl Runtime for AxonCoreRuntime {
    /// Stated explicitly, not inherited from the default, so a change to the
    /// trait default cannot silently relabel the legacy adapter.
    fn isolation(&self) -> Isolation {
        Isolation::ProcessScoped
    }

    /// IG0: the job's `use`s are loaded from the directories its interpreter
    /// searches ([`job_module_dir_sets`]); a module the run can load is read,
    /// not assumed to do everything.
    fn declared_effects(&self, program: &ProgramSource) -> DeclaredEffects {
        job_module_dir_sets(&self.axon_bin, program.path)
            .iter()
            .map(|dirs| scan_effects_in(program.text, dirs).row)
            .fold(
                Decl {
                    row: EffectSet::default(),
                    max_label: Label::Internal,
                },
                |mut d, r| {
                    d.row.fs_read |= r.fs_read;
                    d.row.fs_write |= r.fs_write;
                    d.row.net |= r.net;
                    d.row.exec |= r.exec;
                    d
                },
            )
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
        let verdict = match self.launch(program, grant, budget, seed, required_imports, true) {
            Ok((proc, nonce)) => self.classify(&proc, &nonce),
            Err(verdict) => {
                return RunOutcome {
                    events: vec![],
                    verdict,
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

/// How a captured sandbox run ended ([`AxonCoreRuntime::run_sandboxed_capture`]).
/// Every arm is distinct, so a caller never reads prose to tell a program that
/// did not compile from one that returned 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxOutcome {
    /// The program did not compile, or `axon run` refused it before it
    /// started; nothing ran. Read only from the interpreter's outcome record
    /// (kind `compile`); `diagnostic` is its reason (the first error).
    CompileFailed { diagnostic: String },
    /// The process exited with `code` and the outcome record says the
    /// program ended itself: `main` returned (0 for a nullary `fn main()`),
    /// or it called `exit(code)`.
    Exited { code: i32 },
    /// The interpreter stopped the run (record kind `fault`): a sandbox
    /// refusal (`verdict` = `Denied { axis: "sandbox" }`), a goal budget, a
    /// refinement, a failed `@[verify]`, a halt, an AI-policy refusal, a
    /// resource ceiling, a panic. Also every run without one valid outcome
    /// record of its own (`Denied { axis: "sandbox" }`, reason `no outcome
    /// record: ...`: fail closed), and a run ended by a termination signal
    /// this process received. `code` is the raw process status.
    Faulted { code: Option<i32>, verdict: Verdict },
    /// The wall deadline killed the run's process group.
    TimedOut { after_ms: u64 },
    /// The run never started (staging or spawn failed): an infrastructure
    /// failure, not a property of the program.
    NotRun { reason: String },
}

/// A sandbox run with its output ([`AxonCoreRuntime::run_sandboxed_capture`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedRun {
    pub outcome: SandboxOutcome,
    /// The program's stdout without the wrapper's completion marker; head and
    /// tail within the runtime's capture bound.
    pub stdout: String,
    /// The interpreter's stderr (diagnostics and the program's `eprintln`),
    /// head and tail within the capture bound.
    pub stderr: String,
    /// Bytes of stdout read but not retained (non-zero: `stdout` is partial).
    pub stdout_omitted: u64,
}

impl AxonCoreRuntime {
    /// Run `program` in the sandbox [`Runtime::run_sandboxed`] uses (the
    /// wrapper, `grant`'s allowlists, the wall deadline, the process-group
    /// kill) and return how it ended with its captured output. For the
    /// intent-generation loop's examples (IG4): no import binding, no audit
    /// event, the kill file is not polled. The child's cwd is the program's
    /// directory.
    pub fn run_sandboxed_capture(
        &self,
        program: &ProgramSource,
        grant: &Grant,
        budget: &Budget,
        seed: u64,
    ) -> CapturedRun {
        let (proc, nonce) = match self.launch(program, grant, budget, seed, None, false) {
            Ok(p) => p,
            Err(v) => {
                return CapturedRun {
                    outcome: SandboxOutcome::NotRun {
                        reason: verdict_reason(&v),
                    },
                    stdout: String::new(),
                    stderr: String::new(),
                    stdout_omitted: 0,
                }
            }
        };
        let outcome = self.classify_capture(&proc, &nonce);
        CapturedRun {
            outcome,
            stdout: proc.stdout,
            stderr: proc.stderr,
            stdout_omitted: proc.stdout_omitted,
        }
    }

    /// How a capture run ended (GV-1). Decided in this order, and never from
    /// anything the program wrote to stdout or stderr:
    /// 1. the supervisor's own kill: the deadline (`TimedOut`) or a
    ///    termination signal this process received;
    /// 2. the interpreter's `axon-outcome/1` record on the outcome pipe
    ///    ([`read_outcome_record`]): `exit` is the program's own exit status,
    ///    `fault` the interpreter stopping it, `compile` nothing ran;
    /// 3. anything else - no record, two, a malformed one, another run's
    ///    nonce, a code that is not the exit status, death by a signal nobody
    ///    here sent - fails closed as a sandbox refusal.
    fn classify_capture(&self, proc: &ProcOutcome, nonce: &str) -> SandboxOutcome {
        if proc.timed_out {
            return SandboxOutcome::TimedOut {
                after_ms: self.timeout.as_millis() as u64,
            };
        }
        if let Some(sig) = proc.killed_by_signal {
            return SandboxOutcome::Faulted {
                code: proc.code,
                verdict: Verdict::Halted {
                    reason: format!("the run was killed by {}", signal_name(sig)),
                },
            };
        }
        match read_outcome_record(proc, nonce) {
            Ok(r) => match r.kind {
                RecordKind::Exit => SandboxOutcome::Exited { code: r.code },
                RecordKind::Fault => SandboxOutcome::Faulted {
                    code: Some(r.code),
                    verdict: fault_verdict(&r.class, r.reason),
                },
                RecordKind::Compile => SandboxOutcome::CompileFailed {
                    diagnostic: r.reason,
                },
            },
            Err(no_record) => SandboxOutcome::Faulted {
                code: proc.code,
                verdict: Verdict::Denied {
                    reason: no_record,
                    axis: "sandbox".into(),
                },
            },
        }
    }

    /// Stage the wrapper and run it: `Ok((outcome, nonce))`, or the verdict
    /// for a run that could not start. `poll_kill_file`: honour
    /// `AXON_KILL_FILE` (R27/R29). Every run gets an outcome pipe (GV-1) and
    /// is classified by the record the interpreter writes to it.
    fn launch(
        &self,
        program: &ProgramSource,
        grant: &Grant,
        budget: &Budget,
        seed: u64,
        required_imports: Option<&str>,
        poll_kill_file: bool,
    ) -> Result<(ProcOutcome, String), Verdict> {
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
        let nonce = run_nonce().map_err(|e| Verdict::Denied {
            reason: format!("cannot read the OS random source for the run nonce: {e}"),
            axis: "io".into(),
        })?;
        let wrapper_src = wrap_in_sandbox(program.text, grant, budget);
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
        let staging = StagingDir::create(&self.staging_root).map_err(|e| Verdict::Denied {
            reason: format!("cannot create private staging dir: {e}"),
            axis: "io".into(),
        })?;
        let wrapper_path = staging
            .write_new(&format!("{stem}.wrap.ax"), wrapper_src.as_bytes())
            .map_err(|_| Verdict::Denied {
                reason: "cannot stage sandbox wrapper".into(),
                axis: "io".into(),
            })?;
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
        // B3-7: the grant is also the interpreter's AMBIENT scope. The
        // wrapper opens the grant's sandbox in `main`, but every module-level
        // `let` initialiser of the job is evaluated before `main`; without
        // this it ran with the empty ambient scope of `env_clear`, i.e. with
        // no restriction at all. Same tags and allowlists as the wrapper's
        // `sandbox_create_scoped` (an empty list is deny-all), so the
        // wrapper's `sandbox_run` stays a narrowing of the ambient ceiling.
        // Set after `job_child_command`'s `env_clear`.
        cmd.env("AXON_ALLOWED_EFFECTS", grant_sandbox_tags(grant));
        cmd.env("AXON_ALLOWED_FS_READ", grant.fs_read.join(","));
        cmd.env("AXON_ALLOWED_FS_WRITE", grant.fs_write.join(","));
        cmd.env("AXON_ALLOWED_NET", grant.net.join(","));
        // GV-1: the outcome pipe. The interpreter writes one `axon-outcome/1`
        // record with this run's nonce to fd 3 before it exits; the program
        // owns stdout and stderr, so how the run ended is read from here.
        let (reader, writer) = std::io::pipe().map_err(|e| Verdict::Denied {
            reason: format!("cannot create the outcome pipe: {e}"),
            axis: "io".into(),
        })?;
        // B3-3: the nonce travels on its own pipe (fd 4), never in the
        // environment: the kernel's copy of that (`/proc/self/environ`) is
        // readable by a grant on `/proc/` (O-GV-06). The nonce and its `\n`
        // (at most 129 bytes, under `PIPE_BUF`) are written and the write end
        // closed before the spawn, so the interpreter reads to EOF.
        let nonce_reader = {
            use std::io::Write;
            let (nonce_reader, mut nonce_writer) =
                std::io::pipe().map_err(|e| Verdict::Denied {
                    reason: format!("cannot create the nonce pipe: {e}"),
                    axis: "io".into(),
                })?;
            nonce_writer
                .write_all(format!("{nonce}\n").as_bytes())
                .map_err(|e| Verdict::Denied {
                    reason: format!("cannot write the run nonce: {e}"),
                    axis: "io".into(),
                })?;
            nonce_reader
        };
        give_child_outcome_fd(&mut cmd, &writer, &nonce_reader);
        let pipe = Some(OutcomePipe { reader, writer });

        // R27/R29: if AXON_KILL_FILE is set, poll it during run_bounded.
        // R27 uses this for operator kill (`axon-os kill`); R29 uses it for the
        // compliance monitor. Both write `{"latch":"tripped"}` to stop the job.
        let kill_file_env = std::env::var_os("AXON_KILL_FILE")
            .filter(|_| poll_kill_file)
            .map(std::path::PathBuf::from);
        let kill_file = kill_file_env.as_deref();
        let proc_res = run_bounded_outcome(
            &mut cmd,
            self.timeout,
            kill_file,
            self.max_capture,
            None,
            pipe,
        );
        drop(staging); // removes the private dir and the wrapper in it
        let proc = proc_res.map_err(|e| Verdict::Denied {
            reason: format!("could not launch interpreter: {e}"),
            axis: "exec".into(),
        })?;
        Ok((proc, nonce))
    }

    /// Map a job run to a verdict (GV-1b). The supervisor's own kills decide
    /// first (a termination signal it received, the kill-file latch, the
    /// deadline); then the interpreter's `axon-outcome/1` record on the
    /// outcome pipe ([`read_outcome_record`]). Never stderr: the job owns it,
    /// and a completed job whose stderr merely NAMED a violation was sealed
    /// as a sandbox denial in the hash-chained record (O-GV-01).
    ///
    /// `exit` is a completion only when the record's class says the end was
    /// the wrapper's (B3-3): `return` (a nullary job returned) or `exit` (the
    /// wrapper's `main` propagated the job's value). Class `inner-exit` - a
    /// job that called `exit(n)` itself inside `sandbox_run`, or in a
    /// module-level `let` before `main` - stopped before the wrapper
    /// finished, which is not a completion. Nothing on stdout decides it: the
    /// job owns stdout and can read the wrapper's source. No valid record is
    /// `Denied` on the `runtime` axis.
    fn classify(&self, proc: &ProcOutcome, nonce: &str) -> Verdict {
        if let Some(sig) = proc.killed_by_signal {
            // G2d: the supervisor was told to stop and took the job with it.
            // `axon-os run` writes this record and then exits 128+signal.
            return Verdict::Halted {
                reason: format!(
                    "the supervisor received {} and killed the job's process group",
                    signal_name(sig)
                ),
            };
        }
        if proc.killed_by_latch {
            // Kill file tripped: R27 operator kill → Halted (exit 4).
            // R29 monitor kill → cmd_run overrides the final exit code to 12
            // via the `containment_violation` flag (checked after supervisor::run).
            return Verdict::Halted {
                reason: "kill-switch tripped by supervisor".into(),
            };
        }
        if proc.timed_out {
            return Verdict::Denied {
                reason: format!("timed out after {} ms", self.timeout.as_millis()),
                axis: "time".into(),
            };
        }
        let r = match read_outcome_record(proc, nonce) {
            Ok(r) => r,
            Err(no_record) => {
                return Verdict::Denied {
                    reason: no_record,
                    axis: "runtime".into(),
                }
            }
        };
        // The record's reason is the interpreter's own line; a job verdict
        // has always carried it without the `axon:` prefix.
        let reason = r
            .reason
            .strip_prefix("axon:")
            .map(str::trim)
            .unwrap_or(&r.reason)
            .to_string();
        match r.kind {
            RecordKind::Exit if r.class == "exit" || r.class == "return" => Verdict::Completed {
                value: r.code as i64,
            },
            RecordKind::Exit => Verdict::Denied {
                reason: format!(
                    "the job stopped itself with exit({}) before the sandbox wrapper finished",
                    r.code
                ),
                axis: "runtime".into(),
            },
            RecordKind::Fault => fault_verdict(&r.class, reason),
            // G1e: the loader refused the closure before executing anything -
            // a module changed since approval. An approval failure, not a
            // sandbox one.
            RecordKind::Compile if r.class == "imports-digest" => Verdict::Denied {
                reason: format!("imports changed since approval: {reason}"),
                axis: "approval".into(),
            },
            // AUDIT T24/T44: a job that did not compile executed nothing.
            RecordKind::Compile => Verdict::Malformed { reason },
        }
    }
}

/// The human-facing reason a verdict carries (for a run that never started).
fn verdict_reason(v: &Verdict) -> String {
    match v {
        Verdict::Denied { reason, .. }
        | Verdict::Halted { reason }
        | Verdict::Malformed { reason }
        | Verdict::RefineViolation { reason } => reason.clone(),
        other => format!("{other:?}"),
    }
}

/// The `kind` of an `axon-outcome/1` record (`axon_core::outcome::Kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordKind {
    Exit,
    Fault,
    Compile,
}

/// One valid `axon-outcome/1` record of this run.
#[derive(Debug)]
struct OutcomeRecord {
    kind: RecordKind,
    code: i32,
    class: String,
    reason: String,
}

/// The fault classes the interpreter states (`interp::run_program_inner`,
/// and `replay-divergence` from `axon run`'s end).
const FAULT_CLASSES: &[&str] = &[
    "verify",
    "halted",
    "ai-policy",
    "refine",
    "goal-budget",
    "sandbox",
    "panic",
    "multishot",
    "steps",
    "wall",
    "replay-divergence",
];

/// GV-1: the one record the interpreter wrote to this run's outcome pipe, or
/// why there is none (`Err` is the whole reason, `no outcome record: <which>
/// (exit <n>|signal <s>)`). Valid only as exactly one newline-terminated
/// `axon-outcome/1` JSON line within [`OUTCOME_RECORD_MAX`], carrying THIS
/// run's nonce, a known kind and class, and a `code` equal to the process
/// exit status. Death by a signal is never a valid end: the supervisor's own
/// kills are decided before this is asked.
fn read_outcome_record(proc: &ProcOutcome, nonce: &str) -> Result<OutcomeRecord, String> {
    let ended = match (proc.exit_signal, proc.code) {
        (Some(sig), _) => format!("signal {}", signal_name(sig)),
        (None, Some(c)) => format!("exit {c}"),
        (None, None) => "no exit status".to_string(),
    };
    let refuse = |which: &str| Err(format!("no outcome record: {which} ({ended})"));
    let Some(raw) = proc.outcome.as_deref() else {
        return refuse("the run had no outcome pipe");
    };
    if raw.is_empty() {
        return refuse("none written");
    }
    if raw.len() > OUTCOME_RECORD_MAX {
        return refuse("more than one record's bound written");
    }
    let Some(line) = raw.strip_suffix('\n') else {
        return refuse("truncated (no newline)");
    };
    if line.contains('\n') {
        return refuse("more than one record");
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return refuse("malformed");
    };
    if v.get("schema").and_then(|s| s.as_str()) != Some("axon-outcome/1") {
        return refuse("not axon-outcome/1");
    }
    if v.get("nonce").and_then(|s| s.as_str()) != Some(nonce) {
        return refuse("wrong nonce");
    }
    let kind = match v.get("kind").and_then(|s| s.as_str()) {
        Some("exit") => RecordKind::Exit,
        Some("fault") => RecordKind::Fault,
        Some("compile") => RecordKind::Compile,
        _ => return refuse("unknown kind"),
    };
    let Some(code) = v
        .get("code")
        .and_then(|c| c.as_i64())
        .and_then(|c| i32::try_from(c).ok())
    else {
        return refuse("malformed code");
    };
    let (Some(class), Some(reason)) = (
        v.get("class").and_then(|s| s.as_str()),
        v.get("reason").and_then(|s| s.as_str()),
    ) else {
        return refuse("malformed");
    };
    if kind == RecordKind::Fault && !FAULT_CLASSES.contains(&class) {
        return refuse("unknown fault class");
    }
    if proc.exit_signal.is_some() {
        return refuse("the interpreter died by a signal after writing it");
    }
    if proc.code != Some(code) {
        return refuse(&format!("its code {code} is not the exit status"));
    }
    Ok(OutcomeRecord {
        kind,
        code,
        class: class.to_string(),
        reason: reason.to_string(),
    })
}

/// The verdict for an interpreter fault of `class` (an [`OutcomeRecord`] of
/// kind `fault`), carrying `reason`.
fn fault_verdict(class: &str, reason: String) -> Verdict {
    match class {
        "halted" => Verdict::Halted { reason },
        "refine" => Verdict::RefineViolation { reason },
        "goal-budget" => Verdict::BudgetExhausted {
            axis: "budget".into(),
        },
        "replay-divergence" => Verdict::VerifyMismatch { detail: reason },
        "verify" | "ai-policy" | "sandbox" | "steps" | "wall" => Verdict::Denied {
            reason,
            axis: class.into(),
        },
        _ => Verdict::Denied {
            reason,
            axis: "runtime".into(),
        },
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

    /// B3A-4: the outcome writer and the nonce reader land on 3 and 4
    /// whatever raw fds they start on. The worst order - the writer on raw 4
    /// and the nonce reader on raw 3, where a plain `dup2(w, 3)` would close
    /// the nonce reader - is set up first, then [`place_outcome_fds`] runs.
    #[test]
    fn the_two_outcome_fds_survive_any_source_order() {
        use std::io::{Read, Write};
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let (mut reader, writer) = std::io::pipe().unwrap();
        let (nonce_reader, mut nonce_writer) = std::io::pipe().unwrap();
        nonce_writer.write_all(b"0123456789abcdef\n").unwrap();
        drop(nonce_writer);
        let (wfd, nfd) = (writer.as_raw_fd(), nonce_reader.as_raw_fd());
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "read -r n <&4; echo \"$n\" >&3"]);
        // SAFETY: only async-signal-safe `fcntl`/`dup2` between fork and exec.
        unsafe {
            cmd.pre_exec(move || {
                let w = libc::fcntl(wfd, libc::F_DUPFD_CLOEXEC, 20);
                let n = libc::fcntl(nfd, libc::F_DUPFD_CLOEXEC, 20);
                if w == -1
                    || n == -1
                    || libc::dup2(w, OUTCOME_NONCE_FD) == -1
                    || libc::dup2(n, OUTCOME_FD) == -1
                {
                    return Err(std::io::Error::last_os_error());
                }
                place_outcome_fds(OUTCOME_NONCE_FD, OUTCOME_FD)
            });
        }
        let status = cmd.status().unwrap();
        drop(writer);
        drop(nonce_reader);
        let mut got = String::new();
        reader.read_to_string(&mut got).unwrap();
        assert!(status.success(), "{status:?}");
        assert_eq!(
            got, "0123456789abcdef\n",
            "ATTACK: placing the outcome fds closed one of them"
        );
    }

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
        let out = run_bounded_capped(&mut cmd, Duration::from_secs(20), None, 4096, None).unwrap();
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
        // A module import this scan does not load (no module directory) puts
        // effects in a file it never reads, so the honest answer is the full
        // set, not "nothing found here".
        let m = scan_effects("mod util\nuse util.{go}\nfn main() { go() }").row;
        assert!(
            m.exec && m.net && m.fs_read && m.fs_write,
            "an unloaded `use` must deny-by-default to the full effect set"
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

    /// IG0 F12 (a): the scan reads the compiler's table, so every file
    /// builtin carries its own axis — `append_file` writes.
    #[test]
    fn scan_effects_reads_the_one_table_for_every_file_builtin() {
        let w = scan_effects("fn main() -> i64 {\n    match append_file(\"./out/a\", \"x\") { Ok(_) => 0, Err(_) => 1 }\n}\n").row;
        assert!(
            w.fs_write,
            "ATTACK: an append_file program scanned without fs_write"
        );
        assert!(!w.fs_read && !w.net && !w.exec);
        for (src, read, write) in [
            ("fn f() { file_exists(\"./a\") }", true, false),
            ("fn f() { dir_list(\"./a\") }", true, false),
            ("fn f() { dir_create(\"./a/\") }", false, true),
            ("fn f() { file_copy(\"./a\", \"./b\") }", true, true),
            ("fn f() { file_rename(\"./a\", \"./b\") }", false, true),
        ] {
            let r = scan_effects(src).row;
            assert_eq!((r.fs_read, r.fs_write), (read, write), "{src}");
        }
        // A builtin used as a VALUE is reached too.
        assert!(
            scan_effects("fn f() -> i64 { let w = write_file\n 0 }")
                .row
                .fs_write
        );
    }

    /// IG0 F12 (b): a comment or a string is never an import or a call.
    #[test]
    fn scan_effects_ignores_comments_and_strings() {
        let src =
            "// total mod 10\nfn main() -> i64 {\n    println(\"exec(x) mod y\")\n    7 % 10\n}\n";
        let r = scan_effects(src).row;
        assert!(
            !r.net && !r.exec && !r.fs_read && !r.fs_write,
            "ATTACK: a comment or string widened the scan: {r:?}"
        );
        // A program that does not parse is refused by `try_scan_effects`,
        // and `scan_effects` (the supervisor's) denies by default.
        assert!(try_scan_effects("fn main( {").is_err());
        let all = scan_effects("fn main( {").row;
        assert!(all.fs_read && all.fs_write && all.net && all.exec);
        // A `use` this scan cannot load (no module directory) is every effect.
        let u = scan_effects("use util\nfn main() -> i64 { 0 }\n").row;
        assert!(u.fs_read && u.fs_write && u.net && u.exec);
    }
}
