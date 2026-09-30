//! PSV-7 (C9 round 1): the bytes an authority is verified over must be the
//! bytes the decision uses. The readiness verifier used to check one read of
//! the certification record and verify the operator signature over a SECOND
//! read, and to hash the trust preflight on one read and parse it on another;
//! the B263 qualification hashed the profile manifest and parsed a second
//! read. A repository writer who serves different bytes to the two reads — a
//! FIFO, or a rename between the two opens — made one genuine signature
//! certify anything.
//!
//! Each attack below starts from a GENUINE certification (PASS) and must
//! leave the component not PASS. Two ways of serving two reads:
//! * a FIFO in place of the file (refused now: not a regular file);
//! * other bytes served to the second read of a regular file: renamed into
//!   place while the reader is held opening a file it opens in between
//!   (`swap_while_gate_opens`), or, where it opens nothing in between,
//!   written in place while its second open is held (`replace_on_second_open`).
//!   Both hold the reader with fanotify FAN_OPEN_PERM, so the swap cannot lose
//!   a race (C9 round 2: the inotify rename that preceded them could, and M336
//!   survived a loaded run). Only reading once defeats either.

mod common;
mod readiness_fixture;
use common::*;
use readiness_fixture::*;

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Replace `path` with a FIFO that serves each of `reads` to one opener, in
/// order. The feeder opens without blocking (so it gives up, rather than
/// hanging the test, when nobody reads) and ignores a reader that goes away.
fn fifo_serving(path: &Path, reads: Vec<Vec<u8>>) -> std::thread::JoinHandle<usize> {
    use std::os::unix::fs::OpenOptionsExt;
    let _ = std::fs::remove_file(path);
    let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o644) }, 0, "mkfifo");
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let mut served = 0;
        for bytes in reads {
            let deadline = Instant::now() + Duration::from_secs(5);
            let f = loop {
                match std::fs::OpenOptions::new()
                    .write(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(&path)
                {
                    Ok(f) => break Some(f),
                    Err(_) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break None,
                }
            };
            let Some(mut f) = f else { break };
            let _ = f.write_all(&bytes);
            drop(f);
            served += 1;
            // Let the reader see EOF and close before the next open pairs.
            std::thread::sleep(Duration::from_millis(300));
        }
        served
    })
}

/// An open of `path` that the test holds (fanotify FAN_OPEN_PERM) until this
/// thread answers. `stop` ends the watch; closing the group allows every open
/// still pending or to come.
struct OpenGate {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: std::thread::JoinHandle<usize>,
}

impl OpenGate {
    /// Stop watching; the number of opens of the file that were seen.
    fn finish(self) -> usize {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.thread.join().unwrap()
    }
}

/// A fanotify group, OPEN_PERM on `path`, whose event descriptors are
/// read-write (`event_f_flags`), so the watcher can write the file through
/// the very descriptor of a held open without generating an event itself.
fn open_perm_group(path: &Path, event_flags: libc::c_int) -> libc::c_int {
    let fd = unsafe {
        libc::fanotify_init(
            libc::FAN_CLOEXEC | libc::FAN_CLASS_CONTENT,
            event_flags as libc::c_uint,
        )
    };
    assert!(
        fd >= 0,
        "fanotify_init: {}",
        std::io::Error::last_os_error()
    );
    let g = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    assert_eq!(
        unsafe {
            libc::fanotify_mark(
                fd,
                libc::FAN_MARK_ADD,
                libc::FAN_OPEN_PERM,
                libc::AT_FDCWD,
                g.as_ptr(),
            )
        },
        0,
        "fanotify_mark: {}",
        std::io::Error::last_os_error()
    );
    fd
}

/// Answer one held open: allow it, and close its event descriptor.
fn allow(group: libc::c_int, event_fd: libc::c_int) {
    let r = libc::fanotify_response {
        fd: event_fd,
        response: libc::FAN_ALLOW,
    };
    unsafe {
        libc::write(
            group,
            (&r as *const libc::fanotify_response).cast(),
            std::mem::size_of::<libc::fanotify_response>(),
        );
        libc::close(event_fd);
    }
}

/// Serve `first` at `path` to its first opener and `second` to every later
/// one: each later open is HELD (FAN_OPEN_PERM) while the file's bytes are
/// replaced in place through the held open's own descriptor, and only then
/// allowed. For a reader with no other open between its two reads of `path`
/// (so no gate for [`swap_while_gate_opens`]), this is the deterministic way
/// to serve two reads two contents: an inotify rename after the first close
/// raced the second open, and could lose it under load (M336, C9 round 2).
fn replace_on_second_open(path: &Path, first: &[u8], second: &[u8]) -> OpenGate {
    std::fs::write(path, first).unwrap();
    let fd = open_perm_group(path, libc::O_RDWR);
    let second = second.to_vec();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = stop.clone();
    let thread = std::thread::spawn(move || {
        let mut opens = 0usize;
        while !stopped.load(std::sync::atomic::Ordering::SeqCst) {
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut pfd, 1, 100) } <= 0 {
                continue;
            }
            let mut buf = [0u8; 4096];
            let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
            assert!(n > 0, "fanotify read");
            let mut off = 0usize;
            while off < n as usize {
                let m: libc::fanotify_event_metadata =
                    unsafe { std::ptr::read_unaligned(buf.as_ptr().add(off).cast()) };
                if m.fd >= 0 {
                    if opens >= 1 {
                        // The opener is held: the bytes it will read are
                        // replaced before its open returns.
                        unsafe {
                            assert_eq!(libc::ftruncate(m.fd, 0), 0, "ftruncate");
                            assert_eq!(
                                libc::pwrite(m.fd, second.as_ptr().cast(), second.len(), 0),
                                second.len() as isize,
                                "pwrite"
                            );
                        }
                    }
                    opens += 1;
                    allow(fd, m.fd);
                }
                off += m.event_len as usize;
            }
        }
        unsafe { libc::close(fd) };
        opens
    });
    OpenGate { stop, thread }
}

/// Serve `first` at `path`, and rename `second` into place while the reader
/// is BLOCKED opening `gate`, a file it opens after its first read of `path`
/// (fanotify FAN_OPEN_PERM: the open waits for this thread's answer, and the
/// answer comes only after the rename). An inotify rename after the first
/// close raced the reader's very next open and could lose under load: M336
/// survived the C9 round-2 two-shard run that way, its rename landing after
/// both reads. Returns whether the gate fired.
fn swap_while_gate_opens(
    path: &Path,
    first: &[u8],
    second: &[u8],
    gate: &Path,
) -> std::thread::JoinHandle<bool> {
    std::fs::write(path, first).unwrap();
    let staged: PathBuf = path.with_extension("swap");
    std::fs::write(&staged, second).unwrap();
    let fd = open_perm_group(gate, libc::O_RDONLY);
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut pfd, 1, 20_000) } > 0;
        if ready {
            let mut buf = [0u8; 4096];
            let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
            assert!(n > 0, "fanotify read");
            // The reader is held in open(gate) until the answer below.
            std::fs::rename(&staged, &path).unwrap();
            let mut off = 0usize;
            while off < n as usize {
                let m: libc::fanotify_event_metadata =
                    unsafe { std::ptr::read_unaligned(buf.as_ptr().add(off).cast()) };
                if m.fd >= 0 {
                    allow(fd, m.fd);
                }
                off += m.event_len as usize;
            }
        }
        // Closing the group allows every open still pending or to come.
        unsafe { libc::close(fd) };
        ready
    })
}

fn components(c: &Certified) -> Value {
    axon_fabric::readiness::protected_components(&c.repo, &c.trust)["components"].clone()
}

/// The agent's commit changes code outside governance/, so the genuine record
/// no longer applies; the forged one names the agent's commit and is unsigned
/// (its sidecar signature is the genuine one).
fn forged_for_agent_commit(c: &Certified) -> (Vec<u8>, Vec<u8>) {
    let genuine = std::fs::read(c.record()).unwrap();
    let (_before, agent) = c.commit_changed_code();
    c.refused("changed since the certified revision");
    let mut forged: Value = serde_json::from_slice(&genuine).unwrap();
    forged["axon_sha"] = json!(agent);
    let forged = serde_json::to_vec(&forged).unwrap();
    // Control: the forged record served on EVERY read does not verify.
    std::fs::write(c.record(), &forged).unwrap();
    c.refused("does not verify");
    (forged, genuine)
}

#[test]
fn a_record_served_twice_by_a_fifo_certifies_nothing() {
    let Some(c) = certified() else { return };
    let (forged, genuine) = forged_for_agent_commit(&c);
    let feeder = fifo_serving(&c.record(), vec![forged, genuine]);
    let v = c.verdict();
    feeder.join().unwrap();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: a FIFO served the checked record and the signed record as two reads, and \
         the operator's signature over the certified revision certified the agent's commit: {v}"
    );
    // Not WHICH layer refused it: `read_regular`'s regular-file check, and
    // reading the record ONCE (M335; a non-blocking open of a FIFO whose
    // writer has not yet connected also reads EOF), each refuse this alone.
    // The regular-file check is retired against them (four-cell record,
    // C9 round 2); its only unique effect is naming the FIFO in the reason.
}

#[test]
fn a_record_renamed_between_two_reads_certifies_nothing() {
    let Some(c) = certified() else { return };
    let (forged, genuine) = forged_for_agent_commit(&c);
    // The record is read first, then the PSV spec is
    // hashed long before the signature is checked: the rename lands while
    // the reader is held opening the spec.
    let swapped = swap_while_gate_opens(
        &c.record(),
        &forged,
        &genuine,
        &c.repo
            .join("governance/specs/v022-protected-suite-verdict.md"),
    );
    let v = c.verdict();
    assert!(swapped.join().unwrap(), "the PSV spec was never opened");
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: the genuine record was renamed in after the fields were checked on the \
         forged one, and the signature over the second read certified the agent's commit: {v}"
    );
    assert!(v.to_string().contains("does not verify"), "{v}");
}

/// One component's signature must not certify another: the reviewer's second
/// repro (protected_backend's record + signature, relabelled pci).
#[test]
fn one_components_signature_does_not_certify_another_through_a_fifo() {
    let Some(c) = certified() else { return };
    let genuine = std::fs::read(c.record()).unwrap();
    let genuine_sig = std::fs::read(sig_of(&c.record())).unwrap();
    let reg = c
        .repo
        .join("governance/cortex_gate_execution_registry.json");
    let mut r: Value = serde_json::from_slice(&std::fs::read(&reg).unwrap()).unwrap();
    for g in ["G03-r22-trial-isolation", "G03-r22-physical-isolation"] {
        r["gates"]
            .as_array_mut()
            .unwrap()
            .push(json!({"gate_id": g}));
    }
    std::fs::write(&reg, r.to_string()).unwrap();
    write(
        &c.repo
            .join("governance/proofs/v022-pci-microvm/CERTIFICATION.md"),
        "agent-written\n",
    );
    let pci = c
        .repo
        .join("governance/proofs/v022-protected/pci_on_protected_backend.json");
    let mut forged: Value = serde_json::from_slice(&genuine).unwrap();
    forged["component"] = json!("pci_on_protected_backend");
    let forged = serde_json::to_vec(&forged).unwrap();
    std::fs::write(sig_of(&pci), &genuine_sig).unwrap();
    // Control: as a plain file, the relabelled record does not verify.
    std::fs::write(&pci, &forged).unwrap();
    assert_ne!(components(&c)["pci_on_protected_backend"]["status"], "PASS");
    let feeder = fifo_serving(&pci, vec![forged, genuine]);
    let v = components(&c);
    feeder.join().unwrap();
    assert_eq!(v["protected_backend"]["status"], "PASS", "{v}");
    assert_ne!(
        v["pci_on_protected_backend"]["status"], "PASS",
        "ATTACK: protected_backend's signature certified pci_on_protected_backend, which the \
         operator never signed: {v}"
    );
}

/// The trust preflight the operator certified is a FAILING protected run;
/// the second read serves a passing one.
fn failing_preflight_certified(c: &Certified) -> (Vec<u8>, Vec<u8>) {
    let fail = json!({"schema": axon_fabric::readiness::TRUST_PREFLIGHT_SCHEMA,
                      "mode": "protected", "verdict": "FAIL"})
    .to_string()
    .into_bytes();
    let pass = std::fs::read(c.repo.join(PREFLIGHT)).unwrap();
    std::fs::write(c.repo.join(PREFLIGHT), &fail).unwrap();
    let evidence: Vec<String> =
        serde_json::from_slice::<Value>(&std::fs::read(c.record()).unwrap()).unwrap()["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.as_str().unwrap().to_string())
            .collect();
    let ev: Vec<&str> = evidence.iter().map(String::as_str).collect();
    let bundle = bundle_of(&c.repo, &ev);
    let pf = sha(&c.repo.join(PREFLIGHT));
    resign(c, &c.operator, |r| {
        r["trust_preflight_sha256"] = json!(pf);
        r["evidence_bundle_sha256"] = json!(bundle);
    });
    // Control: the failing preflight, read as it is, is refused.
    c.refused("not a passing protected-mode");
    (fail, pass)
}

#[test]
fn a_preflight_renamed_between_hash_and_parse_certifies_nothing() {
    let Some(c) = certified() else { return };
    let (fail, pass) = failing_preflight_certified(&c);
    // The evidence is read in the record's order (run-evidence, PREFLIGHT,
    // OBSERVATION, B263): the rename lands while the reader is held opening
    // the observation, after the preflight was read and hashed.
    let swapped = swap_while_gate_opens(
        &c.repo.join(PREFLIGHT),
        &fail,
        &pass,
        &c.repo.join(OBSERVATION),
    );
    let v = c.verdict();
    assert!(swapped.join().unwrap(), "the observation was never opened");
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: the certified (failing) preflight was hashed, a passing one was renamed in, \
         and the second read was parsed: {v}"
    );
    assert!(
        v.to_string().contains("not a passing protected-mode"),
        "{v}"
    );
}

#[test]
fn a_preflight_served_twice_by_a_fifo_certifies_nothing() {
    let Some(c) = certified() else { return };
    let (fail, pass) = failing_preflight_certified(&c);
    let feeder = fifo_serving(&c.repo.join(PREFLIGHT), vec![fail, pass]);
    let v = c.verdict();
    feeder.join().unwrap();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: a FIFO served the hashed preflight and the parsed preflight as two reads: {v}"
    );
}

/// Not an attack on its own now that every file is read once, but the rule
/// is that evidence is a regular file reached without a symlink.
#[test]
fn a_symlinked_record_or_signature_is_refused() {
    let Some(c) = certified() else { return };
    let real = c._d.path().join("elsewhere.json");
    std::fs::rename(c.record(), &real).unwrap();
    std::os::unix::fs::symlink(&real, c.record()).unwrap();
    c.refused("is a symlink");

    let Some(c) = certified() else { return };
    let real = c._d.path().join("elsewhere.sig");
    std::fs::rename(sig_of(&c.record()), &real).unwrap();
    std::os::unix::fs::symlink(&real, sig_of(&c.record())).unwrap();
    c.refused("is a symlink");
}

/// A signature FIFO with no writer must not hang the verifier.
#[test]
fn a_signature_fifo_with_no_writer_does_not_hang_readiness() {
    let Some(c) = certified() else { return };
    let sig = sig_of(&c.record());
    std::fs::remove_file(&sig).unwrap();
    let cs = std::ffi::CString::new(sig.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(cs.as_ptr(), 0o644) }, 0);
    let (tx, rx) = std::sync::mpsc::channel();
    let repo = c.repo.clone();
    let trust = c.trust.clone();
    std::thread::spawn(move || {
        let v = axon_fabric::readiness::protected_components(&repo, &trust);
        let _ = tx.send(v);
    });
    let v = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("ATTACK: readiness hung on a FIFO signature with no writer");
    // The non-blocking open (read_regular's O_NONBLOCK) is the ONLY guard of
    // the hang: the regular-file check runs after the open returns, so it
    // cannot stop an open that blocks. Which layer then refuses the FIFO is
    // not asserted (see a_record_served_twice_by_a_fifo_certifies_nothing).
    let pb = &v["components"]["protected_backend"];
    assert_ne!(pb["status"], "PASS", "{pb}");
}

// ── the B263 qualification: one read of the profile manifest ────────────────

const QUALIFIED_GUEST: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
const OTHER_GUEST: &str = "abababababababababababababababababababababababababababababababab";

#[test]
fn a_manifest_renamed_between_hash_and_parse_does_not_change_the_qualified_guest() {
    let d = tempfile::tempdir().unwrap();
    let manifest = d.path().join("manifest.json");
    let qualified = lx_manifest(QUALIFIED_GUEST);
    std::fs::write(&manifest, &qualified).unwrap();
    let issuer = Issuer::generate();
    let lx = qualified_linux_cfg(d.path(), &issuer, &good_evidence(&sha256_file(&manifest)));
    // Control: read as it is, the qualified guest is the manifest's.
    assert_eq!(
        lx.qualification().unwrap().guest_axon_sha256,
        QUALIFIED_GUEST
    );

    // The qualification reads the manifest, then its evidence record: the
    // rename lands while it is held opening the evidence.
    let swapped = swap_while_gate_opens(
        &manifest,
        qualified.as_bytes(),
        lx_manifest(OTHER_GUEST).as_bytes(),
        &lx.evidence,
    );
    let q = lx.qualification();
    assert!(swapped.join().unwrap(), "the evidence was never opened");
    let got = q.map(|q| q.guest_axon_sha256).unwrap_or_default();
    assert_ne!(
        got, OTHER_GUEST,
        "ATTACK: the qualified manifest was hashed, another was renamed in, and the guest the \
         launch would pin came from the second read"
    );
}

// ── psv::prepare: the manifest it pins the guest from is the qualified one ──

/// `psv::prepare` reads the profile manifest again, after the qualification
/// hashed it, to take the guest kernel, image and init digests it pins in the
/// launch manifest. That read is joined to the qualified digest: a manifest
/// changed after qualification (another kernel) prepares nothing. This join is
/// the only guard on the route: `prepare` takes the qualification as given.
#[test]
fn prepare_pins_the_guest_only_from_the_manifest_the_qualification_hashed() {
    use axon_workspace_recipe::{tree_version_ref, Quota};
    let env = Env::new();
    let d = env.dir.path();
    let manifest = d.join("manifest.json");
    std::fs::write(&manifest, full_lx_manifest(QUALIFIED_GUEST)).unwrap();
    let issuer = Issuer::generate();
    let lx = qualified_linux_cfg(d, &issuer, &good_evidence(&sha256_file(&manifest)));
    assert_eq!(lx.manifest, manifest);
    let q = lx.qualification().unwrap();

    let (cand, suite) = (d.join("in/candidate"), d.join("in/check"));
    std::fs::create_dir_all(&cand).unwrap();
    std::fs::create_dir_all(&suite).unwrap();
    std::fs::write(cand.join("f.ax"), "fn main() {}\n").unwrap();
    std::fs::write(suite.join("accept.ax"), "@[test] fn t_ok() {}\n").unwrap();
    let quota = Quota::default();
    let cand_ref = tree_version_ref(&cand, &quota).unwrap();
    let suite_ref = tree_version_ref(&suite, &quota).unwrap();
    let mut rq = request(&env, "op-prepare-join", "t_ok");
    rq["workspace_version_ref"] = json!(cand_ref);
    let rq: axon_loop_contracts::ComputeRequest = serde_json::from_value(rq).unwrap();
    let prepare = |job: &str| {
        axon_fabric::psv::prepare(
            &rq,
            &axon_fabric::psv::PrepareInputs {
                qualification: &q,
                profile_manifest: &lx.manifest,
                host: None,
                policy_json: "{}",
                suite_id: "acc",
                suite_version: &suite_ref,
                entry: "accept.ax",
                test: "t_ok",
                candidate_dir: &cand,
                suite_dir: &suite,
                job_dir: &d.join(job),
                observation_nonce: "none",
            },
        )
    };
    // Control: the qualified manifest prepares, pinning its kernel.
    let qualified_kernel = "1".repeat(64);
    let launch = prepare("job-control").unwrap();
    assert_eq!(launch.manifest.guest.kernel_sha256, qualified_kernel);

    // The manifest now names another kernel; the qualification is unchanged.
    let mut other: Value = serde_json::from_str(&full_lx_manifest(QUALIFIED_GUEST)).unwrap();
    other["artifacts"]["vmlinux"]["sha256"] = json!("9".repeat(64));
    std::fs::write(&lx.manifest, other.to_string()).unwrap();
    match prepare("job-attack") {
        Ok(l) => panic!(
            "ATTACK: prepare pinned guest kernel {} from a profile manifest the qualification \
             never hashed",
            l.manifest.guest.kernel_sha256
        ),
        Err(e) => assert!(e.contains("not the qualified manifest"), "{e}"),
    }
}

/// A69 (C9 round 2, PSV-5, class a): `psv::prepare` builds no launch manifest
/// naming a `*sha256` that is not a sha256 (`unknown`, as `verifier_identity()`
/// falls back to when its executable cannot be hashed; here a qualification's
/// launcher digest). The loop refuses such a manifest too (`check_bundle`);
/// Fabric must not launch one. Control: the qualified inputs prepare.
#[test]
fn prepare_builds_no_manifest_naming_a_digest_that_is_not_a_sha256() {
    use axon_workspace_recipe::{tree_version_ref, Quota};
    let env = Env::new();
    let d = env.dir.path();
    let manifest = d.join("manifest.json");
    std::fs::write(&manifest, full_lx_manifest(QUALIFIED_GUEST)).unwrap();
    let issuer = Issuer::generate();
    let lx = qualified_linux_cfg(d, &issuer, &good_evidence(&sha256_file(&manifest)));
    let q = lx.qualification().unwrap();
    let (cand, suite) = (d.join("in/candidate"), d.join("in/check"));
    std::fs::create_dir_all(&cand).unwrap();
    std::fs::create_dir_all(&suite).unwrap();
    std::fs::write(cand.join("f.ax"), "fn main() {}\n").unwrap();
    std::fs::write(suite.join("accept.ax"), "@[test] fn t_ok() {}\n").unwrap();
    let quota = Quota::default();
    let cand_ref = tree_version_ref(&cand, &quota).unwrap();
    let suite_ref = tree_version_ref(&suite, &quota).unwrap();
    let mut rq = request(&env, "op-prepare-digests", "t_ok");
    rq["workspace_version_ref"] = json!(cand_ref);
    let rq: axon_loop_contracts::ComputeRequest = serde_json::from_value(rq).unwrap();
    let prepare = |q: &axon_fabric::backend::LinuxQualification, job: &str| {
        axon_fabric::psv::prepare(
            &rq,
            &axon_fabric::psv::PrepareInputs {
                qualification: q,
                profile_manifest: &lx.manifest,
                host: None,
                policy_json: "{}",
                suite_id: "acc",
                suite_version: &suite_ref,
                entry: "accept.ax",
                test: "t_ok",
                candidate_dir: &cand,
                suite_dir: &suite,
                job_dir: &d.join(job),
                observation_nonce: "none",
            },
        )
    };
    prepare(&q, "job-control").expect("control: the qualified inputs prepare");
    for bad in ["unknown", "x", ""] {
        let mut q2 = q.clone();
        q2.launcher_sha256 = bad.into();
        match prepare(&q2, &format!("job-{}", bad.len())) {
            Ok(l) => panic!(
                "ATTACK: prepare built a launch manifest whose launcher_sha256 is {:?}",
                l.manifest.launcher_sha256
            ),
            Err(e) => assert!(e.contains("not a sha256"), "{bad:?}: {e}"),
        }
    }
}

// ── interpret_linux_result: the digest recorded is of the bytes interpreted ─

/// `interpret_linux_result` records `sha256-result-json:` as evidence and
/// decides the outcome from result.json. Both must come from ONE read: a
/// result.json renamed in after the first read closes (inotify) would
/// otherwise decide the outcome while the evidence names the other bytes.
/// Nothing is opened between the two reads, so the second OPEN is held
/// (fanotify) while the bytes are replaced in place: deterministic, where the
/// old inotify rename needed 96 MiB of padding to win its race.
#[test]
fn the_result_json_hashed_as_evidence_is_the_one_interpreted() {
    use axon_fabric::backend::{interpret_linux_result, LinuxOutcome};
    use sha2::{Digest, Sha256};
    let d = tempfile::tempdir().unwrap();
    let rj = d.path().join("result.json");
    let result = |status: &str, complete: bool| {
        json!({
            "schema": "axon-linux-microvm-result/1", "status": status,
            "admissible": complete, "output_bound": true, "workload_exit": 0,
            "outputs": {"stdout": {"sha256": "a".repeat(64)}},
            "cleanup": {"complete": complete, "left_behind": []},
        })
        .to_string()
    };
    // Hashed first: cleanup not confirmed, so Unknown.
    let first = result("vmm-died", false).into_bytes();
    let second = result("ok", true).into_bytes();
    let first_sha = format!("{:x}", Sha256::digest(&first));

    let gate = replace_on_second_open(&rj, &first, &second);
    let (outcome, why, evidence) = interpret_linux_result(Some(0), d.path(), &mut || Some(0));
    assert!(gate.finish() >= 1, "result.json was never read");
    let recorded = format!("sha256-result-json:{first_sha}");
    assert!(evidence.contains(&recorded), "{evidence:?}");
    assert!(
        !matches!(outcome, LinuxOutcome::Ok { .. }),
        "ATTACK: one result.json was hashed as evidence ({first_sha}) and another, written in \
         place, decided the outcome Ok ({why})"
    );
    assert!(
        matches!(outcome, LinuxOutcome::Unknown),
        "{outcome:?} {why}"
    );
}
