//! The Firecracker launch path, extracted from `main.rs` into the library (B262).
//!
//! # What this backend is — and is not
//!
//! [`BACKEND_PROFILE`] is the truthful label for everything in this module. It
//! is Firecracker on KVM, launched DIRECTLY (no `jailer`: no chroot, no
//! dedicated uid/gid, no cgroup placement — a principal's memory cap is a
//! balloon), booting the CUSTOM bare-metal `axon-guest-kernel`. It is not a
//! Linux guest and not a general native execution environment, so it can never
//! satisfy a "protected Linux microVM" requirement (B263, blocked).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs, process};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// A tested engine/enclosure/guest/OS/architecture combination. Deliberately a
/// flat description rather than a tier: consumers match on the fields they
/// require (G13-r22-guest-truth).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BackendProfile {
    /// Stable id, e.g. `axon-metal-fc-nojailer`.
    pub id: &'static str,
    /// The VMM.
    pub engine: &'static str,
    /// The host-side confinement around the VMM process.
    pub enclosure: &'static str,
    /// What boots inside the VM.
    pub guest: &'static str,
    /// The guest operating system, if any. `None` for the Axon kernel.
    pub guest_os: Option<&'static str>,
    pub arch: &'static str,
    /// Whether the VMM runs under Firecracker's `jailer`.
    pub jailer: bool,
    /// Whether the guest is Linux. Never true for this backend.
    pub linux_guest: bool,
    /// Whether this combination has been physically QUALIFIED (host boundary,
    /// egress denial, crash cleanup … — B263). Nothing here sets it.
    pub qualified_protected: bool,
}

/// The one backend this crate can launch today.
pub const BACKEND_PROFILE: BackendProfile = BackendProfile {
    id: "axon-metal-fc-nojailer",
    engine: "firecracker",
    enclosure: "none: firecracker spawned directly by axon-vm (no jailer chroot/uid/gid/cgroup); memory cap via balloon only",
    guest: "axon-guest-kernel (custom bare-metal Axon kernel)",
    guest_os: None,
    arch: "x86_64",
    jailer: false,
    linux_guest: false,
    qualified_protected: false,
};

impl BackendProfile {
    /// Can this backend satisfy a request that requires a protected Linux
    /// microVM? For every profile this crate ships the answer is no.
    pub fn satisfies_protected_linux_microvm(&self) -> bool {
        self.linux_guest && self.jailer && self.qualified_protected
    }
}

impl RunResult {
    /// `ok` describes the GUEST, not the launcher: only a definite clean exit 0
    /// is ok (P7-KRN-04). This is the mapping `axon-vm run` prints.
    pub fn ok(&self) -> bool {
        matches!(self.outcome, GuestOutcome::Exited(_)) && self.exit_code == 0
    }
}

/// Schema: axon-vm-mmds/1 — written to MMDS before VM boot.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MmdsPayload {
    pub schema: String,
    pub run_id: String,
    pub principal: Option<String>,
    pub allowed_effects: Option<Vec<String>>,
    pub budget_tokens: Option<u64>,
    pub source_hash: Option<String>,
    pub seccomp_bpf_b64: Option<String>,
}

// ── Firecracker orchestration ─────────────────────────────────────────────────

/// What the GUEST did — as distinct from whether the launcher successfully drove
/// the Firecracker API, which is all `Result::is_ok` on the launch ever told us
/// (P7-KRN-04). A run whose guest never reported anything is not a success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestOutcome {
    /// The guest signalled a policy violation on the serial console (`-VIOLATION8`).
    Violation,
    /// The guest signalled a panic (`-PANIC<n>`).
    Panic(i32),
    /// The guest announced a clean exit (`-EXIT<n>`), or Firecracker exited on its
    /// own and the guest made no distress signal.
    Exited(i32),
    /// The deadline passed with no guest signal and no Firecracker exit.
    Timeout,
}

impl GuestOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            GuestOutcome::Violation => "violation",
            GuestOutcome::Panic(_) => "panic",
            GuestOutcome::Exited(_) => "exited",
            GuestOutcome::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunResult {
    pub exit_code: i32,
    pub outcome: GuestOutcome,
}

/// Serial-console sentinels the guest kernel writes immediately before halting.
/// `axon-vm` had no parser for these at all: the guest correctly announced
/// "policy violation, exit 8" on COM1 and the host reported `ok:true` (P7-KRN-04).
pub fn parse_guest_sentinel(line: &str) -> Option<GuestOutcome> {
    // The guest prefixes with an ANSI erase-line; match on the tail.
    let l = line.trim_end();
    if l.ends_with("-VIOLATION8") {
        return Some(GuestOutcome::Violation);
    }
    if let Some(idx) = l.rfind("-PANIC") {
        if let Ok(code) = l[idx + "-PANIC".len()..].trim().parse::<i32>() {
            return Some(GuestOutcome::Panic(code));
        }
    }
    if let Some(idx) = l.rfind("-EXIT") {
        if let Ok(code) = l[idx + "-EXIT".len()..].trim().parse::<i32>() {
            return Some(GuestOutcome::Exited(code));
        }
    }
    None
}

/// Everything one Firecracker launch needs. This replaces the pre-B262 9-argument
/// `run_in_firecracker` signature (which carried a clippy `too_many_arguments`
/// allow). `principal_mem_mib` is the only field of the CLI's `Principal` the
/// launch path ever read, so the library takes just that rather than the
/// CLI-private registry type.
#[derive(Debug, Clone, Copy)]
pub struct LaunchSpec<'a> {
    pub program: &'a Path,
    pub kernel: &'a Path,
    pub initrd: &'a Path,
    pub mem_mib: u64,
    pub vcpus: u64,
    pub vsock_port: u32,
    pub socket_path: &'a Path,
    pub mmds: &'a MmdsPayload,
    /// The principal's memory cap, if any. Enforced with a BALLOON, not a cgroup
    /// (there is no jailer) — see [`BACKEND_PROFILE`].
    pub principal_mem_mib: Option<u64>,
}

/// Launch Firecracker, configure the VM, run the program and report what the
/// GUEST did. Moved verbatim from `main.rs::run_in_firecracker` (B262); the only
/// change is the argument packaging above.
pub fn run_in_firecracker(spec: &LaunchSpec<'_>) -> Result<RunResult, Box<dyn std::error::Error>> {
    let LaunchSpec {
        program,
        kernel,
        initrd,
        mem_mib,
        vcpus,
        vsock_port,
        socket_path,
        mmds,
        principal_mem_mib,
    } = *spec;
    // Check Firecracker is installed.
    let fc_bin = which_firecracker()?;

    // Spawn Firecracker.
    let mut fc = Command::new(&fc_bin)
        .arg("--api-sock")
        .arg(socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Drain Firecracker's stdout/stderr (the guest serial console + FC logs) to our
    // stderr on background threads. Without this the piped buffers fill and the guest
    // BLOCKS — a deadlock, since `fc.wait()` can't return until FC exits and FC can't
    // make progress while its stdout pipe is full. Draining also surfaces the guest
    // boot log (set AXON_VM_QUIET=1 to suppress).
    //
    // The stdout drain also WATCHES for the guest's exit sentinels. The guest kernel
    // writes `-VIOLATION8` to COM1 and then attempts an ACPI S5 power-off — which
    // Firecracker does not implement, so the guest spins in `hlt` and the run would
    // otherwise be reported as a 124 timeout with `ok:true` (P7-KRN-04). Reading the
    // sentinel means the guest's own verdict decides the outcome, independent of
    // whether it manages to power the machine off.
    let quiet = env::var("AXON_VM_QUIET").map(|v| v == "1").unwrap_or(false);
    let signal: Arc<Mutex<Option<GuestOutcome>>> = Arc::new(Mutex::new(None));
    let mut drains = Vec::new();
    if let Some(out) = fc.stdout.take() {
        let sig = Arc::clone(&signal);
        drains.push(std::thread::spawn(move || {
            drain_to_stderr(out, "guest", quiet, Some(sig))
        }));
    }
    if let Some(err) = fc.stderr.take() {
        drains.push(std::thread::spawn(move || {
            drain_to_stderr(err, "fc", quiet, None)
        }));
    }

    // Wait for Firecracker to create its API socket (typically < 50ms; a fixed 5s margin
    // was found flaky under heavy host CPU contention — R30's own acceptance gate observed
    // acc_a1/acc_a4 failing at this exact R26_ATTESTATION stage under concurrent load, isolated
    // reruns always passing clean, "root cause not chased further" per REQUIREMENTS.md — a
    // starved Firecracker process spawn can plausibly take longer than 5s to even get scheduled.
    // Tunable via AXON_VM_SOCKET_TIMEOUT_SECS (default 5), mirroring AXON_VM_TIMEOUT_SECS below.
    let socket_timeout_secs: u64 = env::var("AXON_VM_SOCKET_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let api = wait_for_socket(socket_path, Duration::from_secs(socket_timeout_secs))?;

    // Configure boot source.
    // The policy is embedded in the cmdline as base64-JSON so the guest kernel
    // can read it without a virtio-net driver (K2 cmdline-reader path).
    let boot_args = embed_policy_in_cmdline(
        "console=ttyS0 reboot=k panic=1 pci=off nomodules \
         init=/init -- /init /usr/bin/axon run /axon/program.ax",
        mmds,
    );
    fc_put(
        &api,
        "/boot-source",
        &serde_json::json!({
            "kernel_image_path": kernel.to_str().unwrap(),
            "boot_args": boot_args,
            "initrd_path": initrd.to_str().unwrap(),
        }),
    )?;

    // Configure machine.
    fc_put(
        &api,
        "/machine-config",
        &serde_json::json!({
            "vcpu_count": vcpus,
            "mem_size_mib": mem_mib,
        }),
    )?;

    // Configure vsock device so the guest can use host_await.
    // uds_path is the host-side Unix socket; the guest connects via CID 2.
    let vsock_host_uds = format!("/tmp/axon-vm-vsock-{}.sock", process::id());
    fc_put(
        &api,
        "/vsock",
        &serde_json::json!({
            "guest_cid": 3,
            "uds_path": vsock_host_uds,
        }),
    )?;

    // MMDS is a SECONDARY policy channel and requires a network interface to bind to.
    // This launcher delivers the policy via the kernel cmdline (`axon.policy=<base64>`,
    // the K2 cmdline-reader path) and configures no NIC, so MMDS V2 config with an empty
    // `network_interfaces` is rejected (400) — correctly. Make it best-effort: try it for
    // hosts that do add a NIC, but never fail the run, since the cmdline already carries
    // the policy. (Was a hard `?` that aborted every run at /mmds/config.)
    if let Err(e) = fc_put(
        &api,
        "/mmds/config",
        &serde_json::json!({
            "version": "V2",
            "network_interfaces": [],
        }),
    ) {
        eprintln!("axon-vm: MMDS config skipped ({e}); policy is delivered via the kernel cmdline");
    } else {
        // Only write the payload if MMDS config succeeded.
        let mmds_content = serde_json::json!({ "latest": { "axon": mmds } });
        if let Err(e) = fc_put(&api, "/mmds", &mmds_content) {
            eprintln!("axon-vm: MMDS payload write skipped ({e})");
        }
    }

    // Apply cgroup limits via jailer-style resource controls (if principal has limits).
    // In production use, Firecracker would be launched via jailer with uid/gid isolation.
    // Here we set balloon memory limits instead (available without jailer).
    if let Some(p_mem_mib) = principal_mem_mib {
        if p_mem_mib < mem_mib {
            fc_put(
                &api,
                "/balloon",
                &serde_json::json!({
                    "amount_mib": mem_mib - p_mem_mib,
                    "deflate_on_oom": true,
                }),
            )?;
        }
    }

    // Start a vsock relay thread to bridge vsock ↔ host_await callbacks.
    // Uses EchoHandler by default; plug in a custom HostAwaitHandler to forward
    // requests to a real host process (e.g. a stdin/stdout bridge).
    let vsock_uds = vsock_host_uds.clone();
    let handler: Arc<dyn HostAwaitHandler> = Arc::new(EchoHandler);
    let _vsock_thread = std::thread::spawn(move || {
        vsock_relay(&vsock_uds, vsock_port, handler);
    });

    // Copy the .ax program into a tmpfs-backed guest path.
    // For real deployments this would be a read-only virtio-blk device.
    // We pass it via a read-only drive.
    let prog_abs = program.canonicalize()?;
    fc_put(
        &api,
        "/drives/program",
        &serde_json::json!({
            "drive_id": "program",
            "path_on_host": prog_abs.to_str().unwrap(),
            "is_root_device": false,
            "is_read_only": true,
        }),
    )?;

    // Start the VM.
    fc_put(
        &api,
        "/actions",
        &serde_json::json!({"action_type": "InstanceStart"}),
    )?;

    // Bounded wait: the guest should run the program and power off (`reboot=k panic=1`
    // turns a finished/paniced guest into a Firecracker exit). A guest that never powers
    // off must NOT hang the host — kill it after the deadline and report. Tunable via
    // AXON_VM_TIMEOUT_SECS (default 45).
    let timeout_secs: u64 = env::var("AXON_VM_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(45);
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    // A guest that has announced its verdict on the serial console gets a short
    // grace period to power itself off, then is reaped. Its own verdict stands
    // either way — a guest that says "policy violation" and then fails to shut
    // down has still refused the operation, and reporting that as a timeout (or,
    // before this, as ok:true) inverts the security-relevant result.
    let sentinel_grace = Duration::from_secs(2);
    let mut sentinel_seen_at: Option<Instant> = None;
    let (exit_code, outcome) = loop {
        if let Some(status) = fc.try_wait()? {
            // Firecracker exited. A sentinel, if any, is the more specific answer.
            let sig = *signal.lock().unwrap();
            break match sig {
                Some(GuestOutcome::Violation) => (8, GuestOutcome::Violation),
                Some(GuestOutcome::Panic(c)) => (c, GuestOutcome::Panic(c)),
                Some(GuestOutcome::Exited(c)) => (c, GuestOutcome::Exited(c)),
                _ => {
                    let c = status.code().unwrap_or(1);
                    (c, GuestOutcome::Exited(c))
                }
            };
        }

        let sig = *signal.lock().unwrap();
        if let Some(o) = sig {
            let since = *sentinel_seen_at.get_or_insert_with(Instant::now);
            if since.elapsed() >= sentinel_grace {
                let _ = fc.kill();
                let _ = fc.wait();
                eprintln!(
                    "axon-vm: guest signalled {} but did not power off — reaped. \
                     (The guest's ACPI S5 write is a no-op under Firecracker; the \
                     guest verdict is authoritative.)",
                    o.as_str()
                );
                break match o {
                    GuestOutcome::Violation => (8, o),
                    GuestOutcome::Panic(c) => (c, o),
                    GuestOutcome::Exited(c) => (c, o),
                    other => (0, other),
                };
            }
        }

        if Instant::now() >= deadline {
            let _ = fc.kill();
            let _ = fc.wait();
            eprintln!(
                "axon-vm: guest did not power off within {timeout_secs}s — killed. \
                 See the guest log above; the guest image's init must run the program \
                 and then poweroff/reboot for the VM to exit."
            );
            break (124, GuestOutcome::Timeout);
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    for d in drains {
        let _ = d.join();
    }

    // Clean up socket files.
    let _ = fs::remove_file(socket_path);
    let _ = fs::remove_file(&vsock_host_uds);

    Ok(RunResult { exit_code, outcome })
}

/// Read a single HTTP/1.1 response from a (keep-alive) stream without relying on the
/// connection closing: read until the header terminator `\r\n\r\n`, then read exactly
/// `Content-Length` more bytes if present. Firecracker replies `204 No Content` (no body)
/// to a successful PUT and keeps the socket open, so reading to EOF would deadlock.
fn read_http_response(stream: &mut UnixStream) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut resp = Vec::new();
    let mut buf = [0u8; 1024];
    let header_end = loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(resp); // connection closed before full headers
        }
        resp.extend_from_slice(&buf[..n]);
        if let Some(pos) = resp.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    // Parse Content-Length (case-insensitive) from the headers.
    let headers = String::from_utf8_lossy(&resp[..header_end]).to_ascii_lowercase();
    let content_len: usize = headers
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    // Read the remaining body bytes, if any.
    while resp.len() < header_end + content_len {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        resp.extend_from_slice(&buf[..n]);
    }
    Ok(resp)
}

/// Copy a child stream (Firecracker stdout = guest serial console, or stderr = FC log)
/// line-by-line to our stderr with a tag. Prevents the piped-buffer deadlock and surfaces
/// the guest boot log. `quiet` suppresses the echo but still drains.
/// When `signal` is supplied (the guest serial console), each line is also scanned
/// for a guest exit sentinel; the FIRST one seen wins and is recorded for the
/// launcher's wait loop.
fn drain_to_stderr<R: std::io::Read + Send + 'static>(
    r: R,
    tag: &'static str,
    quiet: bool,
    signal: Option<Arc<Mutex<Option<GuestOutcome>>>>,
) {
    use std::io::BufRead;
    let reader = std::io::BufReader::new(r);
    for line in reader.lines() {
        match line {
            Ok(l) => {
                if let (Some(sig), Some(outcome)) = (&signal, parse_guest_sentinel(&l)) {
                    let mut g = sig.lock().unwrap();
                    if g.is_none() {
                        *g = Some(outcome);
                    }
                }
                if !quiet {
                    eprintln!("[{tag}] {l}");
                }
            }
            Err(_) => break,
        }
    }
}

// ── Firecracker API client (raw HTTP/1.1 over Unix socket) ────────────────────

/// Represents a connection to the Firecracker API socket.
struct FcApi {
    socket_path: PathBuf,
}

fn wait_for_socket(path: &Path, timeout: Duration) -> Result<FcApi, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return Ok(FcApi {
                socket_path: path.to_owned(),
            });
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(format!(
        "Firecracker socket not ready after {}s: {}",
        timeout.as_secs(),
        path.display()
    )
    .into())
}

/// PUT a JSON body to a Firecracker API endpoint.
fn fc_put(
    api: &FcApi,
    path: &str,
    body: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let body_str = serde_json::to_string(body)?;
    let request = format!(
        "PUT {path} HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Accept: */*\r\n\
         Connection: close\r\n\
         \r\n{body_str}",
        body_str.len()
    );

    let dbg = env::var("AXON_VM_DEBUG").map(|v| v == "1").unwrap_or(false);
    if dbg {
        eprintln!("[axon-vm] → PUT {path}");
    }
    let mut stream = UnixStream::connect(&api.socket_path)?;
    // Firecracker's API server keeps the connection open (it ignores our
    // `Connection: close`), so reading until EOF (`read_to_end`) HANGS FOREVER on the
    // very first request. Read only up to the end of the HTTP headers, then the
    // Content-Length body if any. A read timeout is a backstop against a wedged socket.
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let resp = read_http_response(&mut stream)?;
    if dbg {
        eprintln!("[axon-vm] ← {path} ({} bytes)", resp.len());
    }
    let resp_str = String::from_utf8_lossy(&resp);

    let status_line = resp_str.lines().next().unwrap_or("");
    // e.g. "HTTP/1.1 204 No Content"
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if !(200..300).contains(&status_code) {
        return Err(
            format!("Firecracker API PUT {path} returned {status_line}\n{resp_str}").into(),
        );
    }

    Ok(())
}

// ── HostAwaitHandler trait ────────────────────────────────────────────────────

/// Trait for handling `host_await` requests forwarded from guest Axon programs
/// over vsock.
///
/// Implement this trait to plug in a custom relay. For example, a stdio relay
/// would forward each request to the host process's stdin and return the reply
/// from stdout — the same mechanism as `run_suspendable_stdio` uses on the
/// plain interpreter path. By default `EchoHandler` is wired in, which echoes
/// each request back unchanged (useful for smoke-testing the vsock plumbing
/// and as a starting point for custom handlers).
///
/// # `--host-await-echo` note
///
/// The default `EchoHandler` is the equivalent of running axon-vm with a
/// hypothetical `--host-await-echo` flag: every `host_await` call in the
/// guest receives its own request payload as the reply. To replace it, wrap
/// your handler in `Arc::new(...)` and pass it to `vsock_relay` directly.
pub trait HostAwaitHandler: Send + Sync {
    /// Process a UTF-8 request payload received from the guest.
    ///
    /// Return `Some(reply)` to send the reply string back, or `None` to write
    /// a zero-length frame (the EOF sentinel that signals the guest the
    /// connection is closing).
    fn handle(&self, request: &str) -> Option<String>;
}

/// Default handler: echoes the request back as the reply, unchanged.
///
/// Useful for smoke-testing the vsock plumbing without a real `host_await`
/// implementation. Replace with a handler that forwards to your host process
/// when interactive behavior is required.
pub struct EchoHandler;

impl HostAwaitHandler for EchoHandler {
    fn handle(&self, request: &str) -> Option<String> {
        Some(request.to_string())
    }
}

// ── vsock relay ───────────────────────────────────────────────────────────────

/// vsock relay: listens on the host-side UDS path that Firecracker maps as
/// CID 2 (host). For each guest connection on `vsock_port`, reads a
/// length-prefixed request, calls `handler`, and writes back a
/// length-prefixed reply.
///
/// # Protocol
///
/// Each frame is: 4-byte little-endian u32 length, followed by `length` bytes
/// of UTF-8 payload. A reply frame with length=0 is the EOF sentinel
/// (returned when `handler.handle` returns `None`).
///
/// # Concurrency
///
/// Each accepted connection is dispatched to a new thread so concurrent guest
/// `host_await` calls do not block one another.
fn vsock_relay(uds_path: &str, _vsock_port: u32, handler: Arc<dyn HostAwaitHandler>) {
    use std::os::unix::net::UnixListener;

    // Firecracker requires the UDS path to not exist yet.
    let _ = fs::remove_file(uds_path);
    let listener = match UnixListener::bind(uds_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("axon-vm: vsock relay bind failed: {e}");
            return;
        }
    };

    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                let handler = Arc::clone(&handler);
                std::thread::spawn(move || {
                    // Read 4-byte LE length + payload.
                    let mut lbuf = [0u8; 4];
                    if s.read_exact(&mut lbuf).is_err() {
                        return;
                    }
                    let len = u32::from_le_bytes(lbuf) as usize;
                    let mut buf = vec![0u8; len];
                    if s.read_exact(&mut buf).is_err() {
                        return;
                    }
                    let req = String::from_utf8_lossy(&buf);

                    // Dispatch to the handler and write back the reply.
                    match handler.handle(&req) {
                        Some(reply) => {
                            let rlen = (reply.len() as u32).to_le_bytes();
                            let _ = s.write_all(&rlen);
                            let _ = s.write_all(reply.as_bytes());
                        }
                        None => {
                            // EOF sentinel: length=0, no payload.
                            let _ = s.write_all(&0u32.to_le_bytes());
                        }
                    }
                });
            }
            Err(_) => break,
        }
    }
}

// ── Helper: find Firecracker binary ──────────────────────────────────────────

pub fn which_firecracker() -> Result<PathBuf, Box<dyn std::error::Error>> {
    for candidate in &[
        "firecracker",
        "/usr/local/bin/firecracker",
        "/opt/firecracker/firecracker",
    ] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
        // Try PATH lookup.
        if let Ok(out) = Command::new("which").arg(candidate).output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Ok(PathBuf::from(s));
                }
            }
        }
    }
    Err("firecracker not found in PATH or /usr/local/bin; install from github.com/firecracker-microvm/firecracker".into())
}

// ── Policy-in-cmdline embedding ───────────────────────────────────────────────

/// Append `axon.policy=<base64-json>` to `base_cmdline` so the guest kernel can
/// read the boot policy from the Linux cmdline without a virtio-net driver.
pub fn embed_policy_in_cmdline(base_cmdline: &str, mmds: &MmdsPayload) -> String {
    let json = serde_json::to_string(mmds).unwrap_or_default();
    let b64 = base64::engine::general_purpose::STANDARD.encode(json.as_bytes());
    format!("{base_cmdline} axon.policy={b64}")
}
