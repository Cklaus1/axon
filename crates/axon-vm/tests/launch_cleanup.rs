//! ACF-G22: a launch that fails AFTER Firecracker was spawned must not leave the
//! child running or its sockets on disk.
//!
//! No real Firecracker, no KVM: the `FirecrackerBin` in the `LaunchSpec` is a
//! stub shell script that records its pid and then misbehaves in a chosen way.
//! After the `Err`, the stub's pid must be GONE — `kill(pid, 0)` failing with
//! ESRCH, which also proves the child was reaped (a zombie still answers) — and
//! neither the API socket nor the vsock socket may remain.

#![cfg(target_os = "linux")]

mod common;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axon_vm::firecracker::vsock_uds_path;
use axon_vm::{
    admit, run_in_firecracker, AdmitRequest, AdmittedLaunch, FirecrackerBin, KernelPin, LaunchSpec,
};

struct Env {
    dir: tempfile::TempDir,
    admitted: AdmittedLaunch,
    program: PathBuf,
    initrd: PathBuf,
    sock: PathBuf,
    pidfile: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let kernel = dir.path().join("vmlinuz");
    std::fs::write(&kernel, b"stub-kernel").unwrap();
    let digest = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(b"stub-kernel"))
    };
    let program = dir.path().join("p.ax");
    std::fs::write(&program, "fn main() {}\n").unwrap();
    let initrd = dir.path().join("initrd");
    std::fs::write(&initrd, "x").unwrap();
    let effects = vec!["IO".to_string()];
    let admitted = admit(&AdmitRequest {
        run_id: "cleanup-test",
        kernel: &kernel,
        principal: None,
        manifest_effects: Some(&effects),
        principal_effects: None,
        effects_override: None,
        budget_tokens: None,
        source_hash: None,
        seccomp_bpf_b64: None,
        kernel_pin: KernelPin::Verify {
            expect_digest: Some(&digest),
            baseline: &dir.path().join("no-baseline"),
        },
        extended_tcb: None,
    })
    .expect("pinned stub kernel is admitted");
    Env {
        sock: dir.path().join("fc.sock"),
        pidfile: dir.path().join("stub.pid"),
        admitted,
        program,
        initrd,
        dir,
    }
}

/// Write an executable stub. It records `$$` first; every body `exec`s, so the
/// recorded pid IS the process the launcher spawned.
fn stub(e: &Env, body: &str) -> FirecrackerBin {
    let p = e.dir.path().join("firecracker");
    common::write_executable(
        &p,
        format!("#!/bin/sh\necho $$ > '{}'\n{body}\n", e.pidfile.display()),
        0o755,
    );
    FirecrackerBin::at(&p).unwrap()
}

fn launch(e: &Env, fc: &FirecrackerBin) -> Result<axon_vm::RunResult, String> {
    std::env::set_var("AXON_VM_QUIET", "1");
    run_in_firecracker(&LaunchSpec {
        admitted: &e.admitted,
        firecracker: fc,
        program: &e.program,
        initrd: &e.initrd,
        mem_mib: 128,
        vcpus: 1,
        vsock_port: 5000,
        socket_path: &e.sock,
        principal_mem_mib: None,
        socket_timeout: Duration::from_secs(2),
    })
    .map_err(|e| e.to_string())
}

fn stub_pid(e: &Env) -> libc::pid_t {
    std::fs::read_to_string(&e.pidfile)
        .expect("the stub ran and recorded its pid")
        .trim()
        .parse()
        .unwrap()
}

/// The absence assertions shared by every case.
fn assert_cleaned_up(e: &Env) {
    let pid = stub_pid(e);
    let rc = unsafe { libc::kill(pid, 0) };
    let errno = std::io::Error::last_os_error().raw_os_error();
    assert!(
        rc == -1 && errno == Some(libc::ESRCH),
        "stub firecracker pid {pid} still exists after the launch failed \
         (kill(pid,0) rc={rc} errno={errno:?}) — the child leaked or was not reaped"
    );
    assert!(
        !e.sock.exists(),
        "API socket left behind: {}",
        e.sock.display()
    );
    let v = vsock_uds_path(&e.sock);
    assert!(!v.exists(), "vsock socket left behind: {}", v.display());
}

/// A python UNIX-socket server standing in for the Firecracker API. It answers
/// 204 to every PUT except the one to `fail_path`, which gets a 400, and logs
/// each request line + body to `<sock>.log`.
fn api_stub_body(fail_path: &str) -> String {
    let py = format!(
        r#"
import socket, sys
p = sys.argv[2]
log = open(p + ".log", "a")
s = socket.socket(socket.AF_UNIX)
s.bind(p)
s.listen(8)
while True:
    c, _ = s.accept()
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = c.recv(65536)
        if not chunk:
            break
        data += chunk
    head, _, body = data.partition(b"\r\n\r\n")
    n = 0
    for line in head.split(b"\r\n"):
        if line.lower().startswith(b"content-length:"):
            n = int(line.split(b":")[1])
    while len(body) < n:
        body += c.recv(65536)
    path = head.split(b" ")[1].decode()
    log.write(path + " " + body.decode() + "\n")
    log.flush()
    if path == "{fail_path}":
        c.sendall(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
    else:
        c.sendall(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
    c.close()
"#
    );
    format!("exec python3 -c '{py}' \"$@\"")
}

/// (1) Firecracker starts but never creates its API socket → the socket wait
/// times out. Before the guard, `sleep 60` was left running.
#[test]
fn child_is_killed_and_reaped_when_the_api_socket_never_appears() {
    let e = env();
    let fc = stub(&e, "exec sleep 60");
    let err = launch(&e, &fc).expect_err("no API socket must fail the launch");
    assert!(err.contains("socket not ready"), "wrong failure: {err}");
    assert_cleaned_up(&e);
}

/// (2) The API socket appears and the FIRST PUT is refused with HTTP 400.
/// Before the guard, the stub and its socket both survived the `?`.
#[test]
fn child_and_sockets_are_removed_when_the_first_put_is_refused() {
    let e = env();
    let fc = stub(&e, &api_stub_body("/boot-source"));
    let err = launch(&e, &fc).expect_err("a 400 must fail the launch");
    assert!(
        err.contains("PUT /boot-source returned HTTP/1.1 400"),
        "wrong failure: {err}"
    );
    assert_cleaned_up(&e);
}

/// (3) A late failure, after the vsock relay has bound its socket. The vsock
/// UDS is keyed on the API socket path (not on the pid, where two launches in
/// one process collided) and is removed along with everything else.
#[test]
fn vsock_socket_is_keyed_on_the_api_socket_and_removed_on_a_late_failure() {
    let e = env();
    let fc = stub(&e, &api_stub_body("/drives/program"));
    let err = launch(&e, &fc).expect_err("a 400 must fail the launch");
    assert!(
        err.contains("PUT /drives/program returned HTTP/1.1 400"),
        "wrong failure: {err}"
    );
    let log = std::fs::read_to_string(format!("{}.log", e.sock.display())).unwrap();
    let vsock_line = log
        .lines()
        .find(|l| l.starts_with("/vsock "))
        .expect("the /vsock PUT reached the stub");
    let want = vsock_uds_path(&e.sock);
    assert!(
        vsock_line.contains(&format!("\"uds_path\":\"{}\"", want.display())),
        "vsock uds_path must be derived from the API socket: {vsock_line}"
    );
    assert!(!vsock_line.contains("axon-vm-vsock-"), "{vsock_line}");
    assert_cleaned_up(&e);
}

#[test]
fn vsock_paths_of_two_launches_differ() {
    assert_ne!(
        vsock_uds_path(Path::new("/tmp/a/fc.sock")),
        vsock_uds_path(Path::new("/tmp/b/fc.sock"))
    );
}
