//! Whole-job cancellation, once per process.
//!
//! A supervisor that runs a child which can have descendants must kill the
//! JOB, not a pid: a `#!/bin/sh` script killed with `Child::kill` leaves the
//! `sleep` it started running, re-parented to init. [`Job::spawn`] makes the
//! child a process-group leader, so every descendant inherits its group and one
//! `killpg` ([`Job::kill`]) reaches all of them.
//!
//! The same move takes the job out of the supervisor's process group, and so
//! out of the terminal's foreground group: Ctrl-C no longer reaches it, and a
//! SIGTERM to the supervisor never did. [`forward_termination_signals`]
//! restores that reach: SIGINT, SIGTERM and SIGHUP to the supervisor become
//! SIGKILL to every registered job's group.
//!
//! # One copy per process
//!
//! The registry of running jobs, the termination flag and the signal handler
//! are process-global. Cargo builds one copy of this crate per binary however
//! many crates in it use [`Job`], so they exist once — which is why this is a
//! crate and not a module in each caller (a second copy would install a second
//! handler over the first, each knowing only its own registry).
//!
//! # Who installs the handler
//!
//! Library code never does. Only a binary's `main` calls
//! [`forward_termination_signals`], with the [`OnSignal`] mode that binary's exit
//! semantics need (`axon-os`: [`OnSignal::Continue`]; `cortex` and
//! `axon-intent`: [`OnSignal::Die`]). A binary that spawns a [`Job`] and wants
//! signals to reach it must make that call in its `main`; without it a [`Job`]
//! is still killed as a group at its caller's deadline, but a signal to the
//! process leaves it running. [`Job::spawn`]'s blocked-signal window and the
//! child's disposition reset are unconditional, so they are correct whether or
//! not a handler is installed.
//!
//! Current callers (`grep -rn axon_jobctl crates/*/src`): axon-os's legacy
//! adapter (`run_bounded_capped`, `cmd_run`), axon-intent's synthesizer and
//! `main`, and axon-cortex's `CommandGenerator` and `cortex` `main`.

use std::os::fd::RawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// ── Signal forwarding (G2d) ─────────────────────────────────────────────────

/// The signals forwarded to every running job, as SIGKILL to its group.
const TERMINATION_SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

/// The process groups of the jobs running now, read by the signal handler.
/// Fixed-size and lock-free because a handler may neither allocate nor lock;
/// `0` is a free slot. Several jobs can be in flight at once (the test suites
/// run them on parallel threads). A job that finds every slot taken runs
/// unregistered: the handler cannot reach it, and a caller's poll loop kills it
/// on its next pass instead (it checks [`termination_signal`]).
static JOB_GROUPS: [AtomicI32; 64] = [const { AtomicI32::new(0) }; 64];

/// The first termination signal received (0: none).
static TERMINATION: AtomicI32 = AtomicI32::new(0);

/// What the handler does once the jobs are killed. One process-global value,
/// set by the first [`forward_termination_signals`] (0: no handler installed).
static MODE: AtomicU8 = AtomicU8::new(0);
const MODE_CONTINUE: u8 = 1;
const MODE_DIE: u8 = 2;

/// What a forwarded termination signal does to the supervisor itself, after it
/// has SIGKILLed every registered job group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnSignal {
    /// The supervisor keeps running: the signal only sets the flag
    /// [`termination_signal`] reads. For a process that owns its signal
    /// handling — `axon-os run` lets its poll loop observe the job's death,
    /// writes the run record, and only then exits 128+signal.
    Continue,
    /// The supervisor then dies by the same signal, as it would have without a
    /// handler: the handler resets the signal's disposition to the default and
    /// raises it. For a process with no record to write and nothing to resume
    /// (`cortex`, `axon-intent`); a [`Continue`](OnSignal::Continue) flag would
    /// be read by no poll loop and swallow Ctrl-C.
    Die,
}

impl OnSignal {
    fn code(self) -> u8 {
        match self {
            OnSignal::Continue => MODE_CONTINUE,
            OnSignal::Die => MODE_DIE,
        }
    }
}

/// The signal handler. Async-signal-safe calls only: atomics, `killpg`,
/// `sigaction`, `raise`, and errno, which it saves and restores (a handler must
/// not clobber the errno of the code it interrupted).
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
            // SAFETY: plain syscall on a pgid registered by `Job::spawn`.
            unsafe { libc::killpg(pgid, libc::SIGKILL) };
        }
    }
    if MODE.load(Ordering::SeqCst) == MODE_DIE {
        // Every termination signal is blocked while this handler runs (its
        // `sa_mask`), so the raised signal stays pending until the handler
        // returns and is then delivered under the default disposition.
        // SAFETY: `sigaction` and `raise` are async-signal-safe.
        unsafe {
            set_disposition(sig, libc::SIG_DFL);
            libc::raise(sig);
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

/// Forward SIGINT, SIGTERM and SIGHUP to every running [`Job`], as SIGKILL to
/// its process group, instead of letting them kill the supervisor alone; then
/// do what `mode` says.
///
/// Call it from a binary's `main`, never from library code (see the crate
/// docs). A signal already IGNORED stays ignored, so `nohup <binary>` still
/// outlives its terminal.
///
/// The first call sets the mode for the process. A later call with the same
/// mode is a no-op; a later call with a DIFFERENT mode panics: one handler
/// serves the whole process, and a silent last-wins would let one caller's
/// choice change another's exit semantics.
pub fn forward_termination_signals(mode: OnSignal) {
    match MODE.compare_exchange(0, mode.code(), Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => {}
        Err(prev) if prev == mode.code() => return,
        Err(_) => panic!(
            "forward_termination_signals({mode:?}): this process already forwards \
             termination signals in the other mode; one binary must choose one"
        ),
    }
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

// ── The job ─────────────────────────────────────────────────────────────────

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

/// A child running as the leader of its own process group, registered with
/// the termination handler until it is reaped.
///
/// Dropping a `Job` that was not [`reap`](Job::reap)ed kills its group, frees
/// its slot and then reaps it, so an early return cannot leak a registered,
/// running job.
pub struct Job {
    child: Child,
    slot: Option<JobGroupSlot>,
    reaped: bool,
}

impl Job {
    /// Spawn `cmd` as a process-group leader and register its group.
    ///
    /// The termination signals are blocked in this thread across the spawn
    /// and the registration, so the handler cannot run HERE between the fork
    /// and the registration. (Another thread can take the signal in that
    /// window; a caller's poll loop that checks [`termination_signal`] covers
    /// it.)
    ///
    /// The child inherits the blocked mask — std no longer resets the mask on
    /// spawn — so its `pre_exec` restores the mask this thread had, after
    /// resetting any disposition that is our handler to the default: the
    /// forked copy would otherwise run it, against its copy of the registry,
    /// between the unblock and the exec.
    ///
    /// Stdio, environment and arguments are the caller's: set them on `cmd`
    /// first.
    pub fn spawn(cmd: &mut Command) -> std::io::Result<Job> {
        // KILL THE JOB, NOT A PID: as a group leader, every descendant the
        // child spawns inherits its group, and one `killpg` reaches them all.
        cmd.process_group(0);
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
            Job {
                child,
                slot: Some(slot),
                reaped: false,
            }
        });
        // SAFETY: as above.
        unsafe { swap_thread_mask(libc::SIG_SETMASK, &restore) };
        spawned
    }

    /// The leader's pid, which is also the job's process-group id.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// The leader's `Child`, for its pipes. Never wait on it directly: the
    /// slot must be freed first ([`Job::reap`]).
    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    /// SIGKILL the job's whole process GROUP. Does not reap: [`Job::reap`]
    /// frees the registry slot first.
    ///
    /// Falls back to killing the leader alone if the group signal fails, so a
    /// platform or permission quirk degrades to a direct-child kill rather than
    /// to no kill at all. The leader's pid IS the group id.
    ///
    /// `killpg(2)` DIRECTLY — never an external `kill -KILL -<pid>`.
    ///
    /// That form was in axon-intent's synthesizer, and it took down whole
    /// machines. procps `kill` parses `-<pid>` as ANOTHER OPTION, not a
    /// process group: it reads the leading digit(s) of it as a SIGNAL NUMBER
    /// and then uses that as the pid. Traced with strace on this host:
    ///
    /// ```text
    ///     kill -CONT -5881   ->  kill(-5, SIGCONT)
    ///     kill -CONT -6032   ->  kill(-6, SIGCONT)
    ///     kill -CONT -12345  ->  kill(-1, SIGCONT)      <- EVERY process
    ///     kill -CONT -- -5881 -> kill(-5881, SIGCONT)   <- what was meant
    /// ```
    ///
    /// So a timed-out child with pid 5881 sent SIGKILL to process GROUP 5, and
    /// many pids sent it to `-1` — every process the caller may signal, which
    /// as root is all of them. That is what happened at 2026-09-25 11:39:18:
    /// the journal records cron, dbus, journald, sshd, docker, tailscaled and
    /// seven more services "code=killed, status=9/KILL" in the same second the
    /// gate reached that test, with 105 GB of memory free and no OOM. It
    /// happened again when the test was run on its own, and closed every WSL
    /// window.
    ///
    /// The syscall takes the group id as a typed integer, so there is nothing
    /// to misparse.
    pub fn kill(&mut self) {
        let pid = self.child.id() as i32;
        // SAFETY: `killpg` takes a pgid and a signal and returns an int; the
        // child is a group leader so `pid` is its pgid. A failure (ESRCH: the
        // group is already gone) is not actionable here.
        let rc = unsafe { libc::killpg(pid, libc::SIGKILL) };
        if rc != 0 {
            let _ = self.child.kill();
        }
    }

    /// Has the job's leader exited? Asked WITHOUT reaping it (`WNOWAIT`), so
    /// its registry slot can be freed before the reap ([`Job::reap`]).
    pub fn has_exited(&self) -> std::io::Result<bool> {
        // SAFETY: a zeroed siginfo_t is valid; waitid fills it for a child of ours.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let rc = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
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

    /// Free the job's registry slot, THEN wait for its leader (see
    /// [`JobGroupSlot`]). Blocks until the leader exits; descendants that are
    /// still running are not waited for.
    pub fn reap(mut self) -> std::io::Result<ExitStatus> {
        self.slot = None;
        self.reaped = true;
        self.child.wait()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if !self.reaped {
            self.kill();
            self.slot = None;
            let _ = self.child.wait();
        }
    }
}

// ── Time-bounded drains (G2c) ───────────────────────────────────────────────

/// How long a caller lets its drains run on once the job has been reaped.
/// Normally no time is spent here: every writer is gone, so the pipes are
/// already at EOF. A descendant that left the job's group (`setsid`), or one
/// left running after a normal exit, can keep a pipe open; it used to block
/// the join, and so the supervisor, until it chose to exit.
pub const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// How long [`join_within`] waits for a stopped thread to notice `stop`. A
/// drain that polls with [`wait_readable`] returns within one poll interval
/// (50 ms); a thread that does not poll (a blocked writer) is detached.
const STOP_GRACE: Duration = Duration::from_secs(1);

/// Wait until `fd` is readable or at EOF (true), or `stop` is set (false).
pub fn wait_readable(fd: RawFd, stop: &AtomicBool) -> bool {
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

/// Join `handles`, waiting at most `grace` for all of them to finish, then set
/// `stop` and give them [`STOP_GRACE`] more. A handle that finished is joined
/// (`Some`, or `None` if its thread panicked); one that has not is DETACHED
/// (`None`) — it can no longer hold the caller up.
///
/// Drains should poll with [`wait_readable`] on the same `stop`, so they
/// return what they read within one poll interval of it.
pub fn join_within<T>(
    grace: Duration,
    handles: Vec<JoinHandle<T>>,
    stop: &AtomicBool,
) -> Vec<Option<T>> {
    let all_done = |hs: &[JoinHandle<T>]| hs.iter().all(|h| h.is_finished());
    let deadline = Instant::now() + grace;
    while !all_done(&handles) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    stop.store(true, Ordering::Release);
    let deadline = Instant::now() + STOP_GRACE;
    while !all_done(&handles) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    handles
        .into_iter()
        .map(|h| {
            if h.is_finished() {
                h.join().ok()
            } else {
                None // dropping a JoinHandle detaches its thread
            }
        })
        .collect()
}

#[cfg(test)]
fn registered(pgid: i32) -> bool {
    JOB_GROUPS.iter().any(|s| s.load(Ordering::SeqCst) == pgid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::process::Stdio;

    /// Selects the re-executed copy of this test binary's role. Test-only, so
    /// not `AXON_*`: the env registry (axon-core `env_registry.rs`) lists the
    /// controls a shipped binary reads, and no shipped binary reads these.
    const ROLE: &str = "JOBCTL_TEST_ROLE";
    const DIR: &str = "JOBCTL_TEST_DIR";

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("axon-jobctl-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A live process whose command line is exactly `argv`, read from a pid
    /// file. `/proc/<pid>/cmdline` (not `kill(pid, 0)`): a zombie and a reused
    /// pid both read as gone. Dropping it kills whatever is left.
    struct Tracked {
        pid: i32,
        cmdline: Vec<u8>,
    }

    impl Tracked {
        fn wait_alive(pid_file: &Path, argv: &[&str]) -> Tracked {
            let cmdline: Vec<u8> = argv
                .iter()
                .flat_map(|a| a.bytes().chain(std::iter::once(0)))
                .collect();
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let pid = std::fs::read_to_string(pid_file)
                    .ok()
                    .and_then(|s| s.trim().parse::<i32>().ok());
                if let Some(pid) = pid {
                    let t = Tracked {
                        pid,
                        cmdline: cmdline.clone(),
                    };
                    if t.alive() {
                        return t;
                    }
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
            std::fs::read(format!("/proc/{}/cmdline", self.pid)).is_ok_and(|c| c == self.cmdline)
        }

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

    impl Drop for Tracked {
        fn drop(&mut self) {
            if self.alive() {
                // SAFETY: plain syscall; the pid was just checked to be ours.
                unsafe { libc::kill(self.pid, libc::SIGKILL) };
            }
        }
    }

    /// A shell job that starts `sleep <gc>` in the background (stdio on
    /// /dev/null, so no pipe of the test is held) and then becomes
    /// `sleep <leader>`, writing both pids into `d`.
    fn shell_job(d: &Path, leader: &str, gc: &str) -> Command {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg(format!(
                "sleep {gc} </dev/null >/dev/null 2>&1 & echo $! > '{}'; echo $$ > '{}'; \
                 exec sleep {leader}",
                d.join("grandchild.pid").display(),
                d.join("leader.pid").display()
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd
    }

    #[test]
    fn a_killed_job_takes_its_grandchild_and_spares_a_bystander() {
        let d = tmp("grandchild");
        // A bystander in the TEST's own group: only the job's group may die.
        let mut bystander = Command::new("sleep")
            .arg("4732")
            .spawn()
            .expect("spawn bystander");
        let mut job = Job::spawn(&mut shell_job(&d, "4731", "4730")).expect("spawn job");
        let leader = Tracked::wait_alive(&d.join("leader.pid"), &["sleep", "4731"]);
        let grandchild = Tracked::wait_alive(&d.join("grandchild.pid"), &["sleep", "4730"]);
        assert!(registered(job.id() as i32));

        job.kill();
        let status = job.reap().expect("reap");
        let bystander_state = bystander.try_wait().expect("poll bystander");
        let _ = bystander.kill();
        let _ = bystander.wait();

        assert_eq!(status.signal(), Some(libc::SIGKILL), "{status:?}");
        assert!(leader.gone_within(Duration::from_secs(5)));
        assert!(
            grandchild.gone_within(Duration::from_secs(5)),
            "ATTACK: the killed job's grandchild (pid {}) survived",
            grandchild.pid
        );
        assert!(
            bystander_state.is_none(),
            "ATTACK: the kill escaped the job's group ({bystander_state:?})"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reap_frees_the_slot_before_it_waits() {
        let mut cmd = Command::new("sleep");
        cmd.arg("2");
        let job = Job::spawn(&mut cmd).expect("spawn");
        let pgid = job.id() as i32;
        assert!(registered(pgid), "a spawned job is registered");
        let reaper = std::thread::spawn(move || job.reap());
        std::thread::sleep(Duration::from_millis(300));
        assert!(!reaper.is_finished(), "the leader is still running");
        assert!(
            !registered(pgid),
            "the slot must be free while the reap still waits"
        );
        assert!(reaper.join().unwrap().unwrap().success());
    }

    #[test]
    fn dropping_an_unreaped_job_kills_and_reaps_it() {
        let d = tmp("drop");
        let job = Job::spawn(&mut shell_job(&d, "4734", "4733")).expect("spawn job");
        let leader = Tracked::wait_alive(&d.join("leader.pid"), &["sleep", "4734"]);
        let grandchild = Tracked::wait_alive(&d.join("grandchild.pid"), &["sleep", "4733"]);
        let pgid = job.id() as i32;
        drop(job);
        assert!(!registered(pgid));
        // Reaped by the drop: a zombie would still have a /proc entry with an
        // empty cmdline; a reaped pid has none of ours.
        assert!(leader.gone_within(Duration::from_secs(5)));
        assert!(grandchild.gone_within(Duration::from_secs(5)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn join_within_detaches_a_thread_that_will_not_stop() {
        let stop = AtomicBool::new(false);
        let quick = std::thread::spawn(|| 1);
        let stuck = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(30));
            2
        });
        let began = Instant::now();
        let joined = join_within(Duration::from_millis(100), vec![quick, stuck], &stop);
        assert_eq!(joined, vec![Some(1), None]);
        assert!(stop.load(Ordering::Acquire));
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "{:?}",
            began.elapsed()
        );
    }

    /// Run `test` (this binary's test of that name) again in a child process,
    /// in its own process group, with `role` selected.
    fn reexec(test: &str, role: &str, d: &Path) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(ROLE, role)
            .env(DIR, d)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .process_group(0)
            .spawn()
            .expect("re-exec the test binary")
    }

    fn exit_within(child: &mut Child, d: Duration) -> ExitStatus {
        let deadline = Instant::now() + d;
        loop {
            if let Some(st) = child.try_wait().unwrap() {
                return st;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the re-executed test did not exit within {d:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn die_kills_the_jobs_and_then_dies_by_the_signal() {
        if std::env::var(ROLE).as_deref() == Ok("die") {
            let d = PathBuf::from(std::env::var(DIR).unwrap());
            forward_termination_signals(OnSignal::Die);
            let _job = Job::spawn(&mut shell_job(&d, "4736", "4735")).expect("spawn job");
            loop {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        let d = tmp("die");
        let mut sup = reexec(
            "tests::die_kills_the_jobs_and_then_dies_by_the_signal",
            "die",
            &d,
        );
        let leader = Tracked::wait_alive(&d.join("leader.pid"), &["sleep", "4736"]);
        let grandchild = Tracked::wait_alive(&d.join("grandchild.pid"), &["sleep", "4735"]);
        // SAFETY: plain syscall to a child this test spawned and has not reaped.
        assert_eq!(unsafe { libc::kill(sup.id() as i32, libc::SIGTERM) }, 0);
        let status = exit_within(&mut sup, Duration::from_secs(20));
        assert_eq!(
            status.signal(),
            Some(libc::SIGTERM),
            "ATTACK: under OnSignal::Die the supervisor must die by the signal ({status:?})"
        );
        for (who, p) in [("leader", &leader), ("grandchild", &grandchild)] {
            assert!(
                p.gone_within(Duration::from_secs(5)),
                "ATTACK: the job's {who} (pid {}) outlived the supervisor's SIGTERM",
                p.pid
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_second_mode_panics_and_the_same_mode_is_a_no_op() {
        if std::env::var(ROLE).as_deref() == Ok("modes") {
            forward_termination_signals(OnSignal::Continue);
            forward_termination_signals(OnSignal::Continue);
            let other = std::panic::catch_unwind(|| forward_termination_signals(OnSignal::Die));
            std::process::exit(if other.is_err() { 0 } else { 3 });
        }
        let d = tmp("modes");
        let mut child = reexec(
            "tests::a_second_mode_panics_and_the_same_mode_is_a_no_op",
            "modes",
            &d,
        );
        let status = exit_within(&mut child, Duration::from_secs(20));
        assert_eq!(
            status.code(),
            Some(0),
            "a different second mode must panic, the same one must not ({status:?})"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
