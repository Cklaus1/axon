//! One build/fixture uid per TEST THREAD (shared by `guest_build_env.rs` and `guest_build_env_guards.rs`).
//!
//! Amendment 111: a test that hardcodes a uid (4242, 4243, 4322, 65534) and starts a process as it, or
//! asks the controlled build to take it as the build uid, is visible to every other test and run on the
//! host: `begin` refuses a build uid that "already owns running processes" by reading the HOST /proc (a
//! private PID namespace does not hide a child from a host-namespace observer), so
//! `the_build_uid_lock_is_root_owned_and_begin_holds_it` failed whenever anything else on the machine
//! (a sibling binary, another agent's run) happened to run a uid-4242 process. Every uid a test
//! STARTS A PROCESS AS, or hands `begin`/`build_uid_lock`, comes from here.

/// The build uid THIS test uses. Every test owns a distinct, unused uid, claimed for the life of the
/// test thread, so the per-uid lock, the "already owns running processes" refusal (amendment 101) and the
/// post-step reaper (which SIGKILLs every process of the build uid) can never cross from one test to
/// another. They used to share 65534 (`nobody`): a host-namespace `begin` of one test saw (and refused)
/// the transient or detached build-uid processes of another test's step, which is visible from the host
/// /proc even from inside that test's PID namespace, so the suite failed a DIFFERENT 1..7 tests per run.
/// The claim is an exclusive flock on a root-owned file per candidate uid, so concurrent test binaries
/// (and other agents' runs) cannot pick the same uid either.
pub struct UidClaim {
    pub uid: u32,
    _lock: std::fs::File,
}

pub const UID_CLAIM_DIR: &str = "/var/lib/axon-gbe-test-uids";

pub fn owns_a_process(uid: u32) -> bool {
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return true;
    };
    for e in rd.flatten() {
        if !e
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        let Ok(tasks) = std::fs::read_dir(e.path().join("task")) else {
            continue;
        };
        for t in tasks.flatten() {
            if let Ok(st) = std::fs::read_to_string(t.path().join("status")) {
                if st.lines().find(|l| l.starts_with("Uid:")).is_some_and(|l| {
                    l.split_whitespace()
                        .skip(1)
                        .take(4)
                        .any(|x| x == uid.to_string())
                }) {
                    return true;
                }
            }
        }
    }
    false
}

impl UidClaim {
    pub fn new() -> UidClaim {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::io::AsRawFd;
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        std::fs::create_dir_all(UID_CLAIM_DIR).unwrap();
        std::fs::set_permissions(UID_CLAIM_DIR, std::fs::Permissions::from_mode(0o755)).unwrap();
        let span = 20_000u32;
        let start = std::process::id().wrapping_mul(2_654_435_761) % span;
        for _ in 0..span {
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let uid = 40_000 + (start + n) % span;
            let f = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(format!("{UID_CLAIM_DIR}/uid-{uid}"))
                .unwrap();
            if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                continue;
            }
            if owns_a_process(uid) {
                continue;
            }
            return UidClaim { uid, _lock: f };
        }
        panic!("setup: no unclaimed test build uid in 40000..60000");
    }
}

thread_local! {
    static TEST_UID: UidClaim = UidClaim::new();
}

/// This test's build uid (see `UidClaim`).
pub fn test_uid() -> u32 {
    TEST_UID.with(|c| c.uid)
}
