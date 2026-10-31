//! NH-R1 S10 / E3: the hard deadline. A run stalled in a model call, in a
//! sleeping check, or in the middle of its own writes is stopped at its
//! deadline: the running check's process group is killed, the found bytes
//! are restored through the CAS helper, a verdict is printed and the process
//! exits 20 - all within `HARD_GRACE` of the deadline.
//!
//! Each case runs in a CHILD process (this test binary re-run on the hidden
//! `e3_child` test), because the watchdog ends its process with `_exit`.

use std::io::Read as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axon_cortex::cas::{self, Site};
use axon_cortex::watchdog;

const FOUND: &[u8] = b"fn double(n: i64) -> i64 {\n    n + 1\n}\n";
const PATCHED: &[u8] = b"fn double(n: i64) -> i64 {\n    loop {}\n}\n";
const CHILD_ENV: &str = "CORTEX_E3_CHILD_DIR";
const CASE_ENV: &str = "CORTEX_E3_CHILD_CASE";
/// The deadline, from the moment the child arms the watchdog.
const DEADLINE: Duration = Duration::from_millis(800);

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

/// The child: a run in flight when its deadline passes. Not a test of its
/// own: without the env var it returns at once.
#[test]
fn e3_child() {
    let Some(dir) = std::env::var_os(CHILD_ENV).map(PathBuf::from) else {
        return;
    };
    let case = std::env::var(CASE_ENV).unwrap();
    let file = dir.join("f.ax");
    // The run read the file and applied a candidate: the bytes on disk are
    // the run's own, so the restore puts the found bytes back.
    cas::observe(&file, FOUND);
    cas::write(Site::Apply, &file, PATCHED).unwrap();
    // A running check: a process group the watchdog must kill.
    let mut sleeper = Command::new("sleep");
    sleeper
        .arg("600")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let job = axon_jobctl::Job::spawn(&mut sleeper).unwrap();
    std::fs::write(dir.join("check.pid"), job.id().to_string()).unwrap();
    let restore_to = file.clone();
    let deadline = Instant::now() + DEADLINE;
    std::fs::write(
        dir.join("deadline"),
        (epoch_ms() + DEADLINE.as_millis()).to_string(),
    )
    .unwrap();
    let _armed = watchdog::arm(deadline, move |ledger| {
        let restored = ledger
            .write(Site::ExitRestore, &restore_to, FOUND)
            .map(|()| "restored".to_string())
            .unwrap_or_else(|e| e.to_string());
        format!("{{\"outcome\":\"hard_deadline\",\"restore\":\"{restored}\"}}\n")
    });
    match case.as_str() {
        // An `ai:` call to an endpoint that accepts and never answers.
        "model" => {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let mut c = std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap();
            let _held = l.accept().unwrap();
            let mut b = [0u8; 1];
            let _ = c.read(&mut b);
        }
        // The main thread waits on the sleeping check, as `run_limited`
        // does: polling it unreaped, so its group stays registered.
        "check" => {
            while !job.has_exited().unwrap() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        // The main thread is writing, through the CAS helper, when the
        // deadline passes: the watchdog waits for the write lock.
        "writing" => loop {
            let mut l = cas::lock();
            let body: Vec<u8> = PATCHED.iter().cycle().take(256 * 1024).copied().collect();
            if l.write(Site::Apply, &file, &body).is_err() {
                break;
            }
            let _ = l.write(Site::Apply, &file, PATCHED);
        },
        other => panic!("unknown case {other}"),
    }
    // Reaching here means nothing stopped the run.
    std::thread::sleep(Duration::from_secs(600));
}

struct Ended {
    code: Option<i32>,
    stdout: String,
    /// Exit time minus the deadline, ms (negative: before it).
    overshoot_ms: i128,
    file: Vec<u8>,
    check_pid: i32,
}

fn run_case(case: &str, n: usize) -> Ended {
    let dir = std::env::temp_dir().join(format!("cortex-e3-{case}-{n}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("f.ax"), FOUND).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "e3_child",
            "--nocapture",
            "--test-threads=1",
            "--quiet",
        ])
        .env(CHILD_ENV, &dir)
        .env(CASE_ENV, case)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    // A watchdog that never fires leaves the child sleeping: cut it here.
    let cap = Instant::now() + DEADLINE + Duration::from_secs(20);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break Some(s);
        }
        if Instant::now() > cap {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let exited = epoch_ms() as i128;
    let stdout = reader.join().unwrap();
    let deadline: i128 = std::fs::read_to_string(dir.join("deadline"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let check_pid: i32 = std::fs::read_to_string(dir.join("check.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let file = std::fs::read(dir.join("f.ax")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    Ended {
        code: status.and_then(|s| s.code()),
        stdout,
        overshoot_ms: exited - deadline,
        file,
        check_pid,
    }
}

/// The check's process is gone (reaped by init once its parent exited).
fn gone(pid: i32) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        // SAFETY: signal 0 only checks for the pid's existence.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        let zombie = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map(|s| s.contains(") Z "))
            .unwrap_or(false);
        if !alive || zombie {
            return true;
        }
        if Instant::now() > until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_stopped(case: &str, e: &Ended) {
    assert_eq!(
        e.code,
        Some(watchdog::EXIT_DEADLINE),
        "ATTACK: E3 a {case}-stalled run outlived its deadline (stdout {:?})",
        e.stdout
    );
    assert!(
        e.stdout.contains("\"outcome\":\"hard_deadline\"") && e.stdout.contains("restored"),
        "ATTACK: E3 a {case}-stalled run printed no verdict: {:?}",
        e.stdout
    );
    assert_eq!(
        e.file, FOUND,
        "ATTACK: E3 a {case}-stalled run left the patched bytes in place"
    );
    assert!(
        gone(e.check_pid),
        "ATTACK: E3 the running check {} survived the deadline",
        e.check_pid
    );
    assert!(
        e.overshoot_ms <= watchdog::HARD_GRACE.as_millis() as i128,
        "ATTACK: E3 a {case}-stalled run exited {} ms after its deadline, past HARD_GRACE {:?}",
        e.overshoot_ms,
        watchdog::HARD_GRACE
    );
}

/// S10 E3: a stalled model call, a sleeping check, a run mid-write.
#[test]
fn s10_e3_a_stalled_run_is_stopped_restored_and_reported_by_the_grace() {
    for case in ["model", "check", "writing"] {
        let e = run_case(case, 0);
        assert_stopped(case, &e);
    }
}

/// A run that finished first disarms the watchdog, which then never fires
/// (firing would end this whole test process with exit 20).
#[test]
fn a_disarmed_watchdog_never_fires() {
    let armed = watchdog::arm(Instant::now() + Duration::from_millis(200), |_| {
        "ATTACK: E3 a disarmed watchdog fired\n".to_string()
    });
    armed.disarm();
    std::thread::sleep(Duration::from_millis(500));
}

/// S10's measurement of `HARD_GRACE` (§10 Q4): the overshoot past the
/// deadline (lock wait + kill + restore + report + exit) over
/// `CORTEX_E3_SAMPLES` runs of each case, under the host's load as it is.
/// Run on demand (`--ignored`); its output is recorded with S10.
#[test]
#[ignore]
fn s10_e3_measure_hard_grace() {
    let n: usize = std::env::var("CORTEX_E3_SAMPLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let load = || std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let before = load();
    let mut all = Vec::new();
    for case in ["model", "check", "writing"] {
        let mut v = Vec::new();
        for i in 0..n {
            let e = run_case(case, i + 1);
            assert_stopped(case, &e);
            v.push(e.overshoot_ms);
        }
        v.sort();
        println!(
            "case {case}: n={n} min={} p50={} max={} ms",
            v[0],
            v[v.len() / 2],
            v[v.len() - 1]
        );
        all.extend(v);
    }
    all.sort();
    let p99 = all[((all.len() * 99).div_ceil(100)).saturating_sub(1)];
    println!(
        "all: n={} p50={} p99={} max={} ms; loadavg before: {} after: {}; nproc {}",
        all.len(),
        all[all.len() / 2],
        p99,
        all[all.len() - 1],
        before.trim(),
        load().trim(),
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0)
    );
}
