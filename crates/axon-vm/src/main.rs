//! axon-vm — Axon microVM launcher.
//!
//! Wraps Firecracker to run an Axon program in a hardware-isolated microVM with:
//!   • Capability-derived seccomp-BPF policy (from `.axmeta` manifest)
//!   • MMDS-delivered boot policy (principal, budget, allowed effects, BPF)
//!   • vsock host_await substrate for interactive programs
//!   • Principal registry (~/.config/axon/principals.toml)
//!
//! Commands:
//!   axon-vm run <program.ax>  [options]     Launch program in a microVM
//!   axon-vm principal add <name> [options]  Register a principal
//!   axon-vm principal list                  List principals

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, fs, process};

use base64::Engine as _;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

// ── CLI surface ───────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "axon-vm", about = "Run Axon programs in Firecracker microVMs")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run an Axon program in a microVM
    Run {
        /// Path to the .ax program to run
        program: PathBuf,

        /// Firecracker socket path (default: /tmp/axon-vm-<pid>.sock)
        #[arg(long)]
        fc_socket: Option<PathBuf>,

        /// Guest kernel image (default: dist/guest/vmlinuz)
        #[arg(long)]
        kernel: Option<PathBuf>,

        /// Guest initramfs (default: dist/guest/initramfs.cpio.gz)
        #[arg(long)]
        initrd: Option<PathBuf>,

        /// Guest memory in MiB (default: 128)
        #[arg(long, default_value = "128")]
        mem_mib: u64,

        /// Guest vCPU count (default: 1)
        #[arg(long, default_value = "1")]
        vcpus: u64,

        /// Principal name (from ~/.config/axon/principals.toml)
        #[arg(long)]
        principal: Option<String>,

        /// vsock port for host_await (default: 5000)
        #[arg(long, default_value = "5000")]
        vsock_port: u32,

        /// Emit JSON output (axon-vm-run/1 schema)
        #[arg(long)]
        json: bool,
    },

    /// Manage principals
    Principal {
        #[command(subcommand)]
        cmd: PrincipalCmd,
    },
}

#[derive(Subcommand)]
enum PrincipalCmd {
    /// Register a new principal
    Add {
        /// Principal name (must be unique)
        name: String,

        /// Token budget (default: 10000)
        #[arg(long, default_value = "10000")]
        budget_tokens: u64,

        /// Allowed effect rows, comma-separated (e.g. "AI,Net")
        #[arg(long, default_value = "AI")]
        allowed_effects: String,

        /// Memory limit in MiB (default: 128)
        #[arg(long, default_value = "128")]
        mem_mib: u64,

        /// CPU share percentage 0-100 (default: 50)
        #[arg(long, default_value = "50")]
        cpu_pct: u32,
    },
    /// List registered principals
    List,
}

// ── Data types ────────────────────────────────────────────────────────────────

/// Schema: axon-vm-mmds/1 — written to MMDS before VM boot.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct MmdsPayload {
    schema: String,
    run_id: String,
    principal: Option<String>,
    allowed_effects: Option<Vec<String>>,
    budget_tokens: Option<u64>,
    source_hash: Option<String>,
    seccomp_bpf_b64: Option<String>,
}

/// Schema: axon-manifest/1 — sidecar emitted by `axon build --emit-manifest`.
#[derive(Deserialize, Debug, Default)]
struct AxonManifest {
    schema: Option<String>,
    source: Option<String>,
    binary: Option<String>,
    effect_union: Option<Vec<String>>,
    syscall_hint: Option<Vec<String>>,
    risk: Option<String>,
    #[serde(default)]
    per_fn: Vec<serde_json::Value>,
}

/// Gap 7: Principal registry entry (~/.config/axon/principals.toml).
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Principal {
    name: String,
    budget_tokens: u64,
    allowed_effects: Vec<String>,
    mem_mib: u64,
    cpu_pct: u32,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct PrincipalRegistry {
    #[serde(default)]
    principals: Vec<Principal>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run {
            program,
            fc_socket,
            kernel,
            initrd,
            mem_mib,
            vcpus,
            principal,
            vsock_port,
            json,
        } => cmd_run(program, fc_socket, kernel, initrd, mem_mib, vcpus, principal, vsock_port, json),
        Cmd::Principal { cmd } => match cmd {
            PrincipalCmd::Add {
                name,
                budget_tokens,
                allowed_effects,
                mem_mib,
                cpu_pct,
            } => cmd_principal_add(name, budget_tokens, allowed_effects, mem_mib, cpu_pct),
            PrincipalCmd::List => cmd_principal_list(),
        },
    }
}

// ── cmd_run ───────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn cmd_run(
    program: PathBuf,
    fc_socket: Option<PathBuf>,
    kernel: Option<PathBuf>,
    initrd: Option<PathBuf>,
    mem_mib: u64,
    vcpus: u64,
    principal_name: Option<String>,
    vsock_port: u32,
    json_out: bool,
) {
    let start = Instant::now();

    // Locate guest image files.
    let kernel_path = kernel
        .unwrap_or_else(|| PathBuf::from("dist/guest/vmlinuz"));
    let initrd_path = initrd
        .unwrap_or_else(|| PathBuf::from("dist/guest/initramfs.cpio.gz"));

    for p in [&kernel_path, &initrd_path, &program] {
        if !p.exists() {
            eprintln!("axon-vm: file not found: {}", p.display());
            process::exit(1);
        }
    }

    // Generate a unique run-id.
    let run_id = format!(
        "vm-{}-{}",
        process::id(),
        start.elapsed().as_nanos()
    );

    // Compute source hash for auditability.
    let source_hash = sha256_file(&program);

    // Load the .axmeta manifest (emitted by `axon build --emit-manifest`).
    let manifest = load_manifest(&program);

    // Resolve the principal.
    let principal = principal_name
        .as_ref()
        .and_then(|n| load_principal(n));

    // Build the seccomp BPF policy from the manifest's syscall_hint.
    let seccomp_b64 = if let Some(ref hints) = manifest.syscall_hint {
        if hints.is_empty() {
            None
        } else {
            match bpf_allowlist(hints) {
                Ok(b64) => Some(b64),
                Err(e) => {
                    eprintln!("axon-vm: BPF generation failed: {e}");
                    None
                }
            }
        }
    } else {
        None
    };

    // Derive allowed effects: prefer manifest, fall back to principal, then open.
    let allowed_effects = manifest
        .effect_union
        .clone()
        .or_else(|| principal.as_ref().map(|p| p.allowed_effects.clone()));

    let budget_tokens = principal.as_ref().map(|p| p.budget_tokens);

    // Construct the MMDS payload.
    let mmds_payload = MmdsPayload {
        schema: "axon-vm-mmds/1".to_string(),
        run_id: run_id.clone(),
        principal: principal_name.clone(),
        allowed_effects,
        budget_tokens,
        source_hash: Some(source_hash),
        seccomp_bpf_b64: seccomp_b64,
    };

    // Choose or generate the Firecracker socket path.
    let socket_path = fc_socket.unwrap_or_else(|| {
        PathBuf::from(format!("/tmp/axon-vm-{}.sock", process::id()))
    });

    // Launch Firecracker, configure the VM, and run the program.
    let result = run_in_firecracker(
        &program,
        &kernel_path,
        &initrd_path,
        mem_mib,
        vcpus,
        vsock_port,
        &socket_path,
        &mmds_payload,
        principal.as_ref(),
    );

    let elapsed_ms = start.elapsed().as_millis();

    if json_out {
        let out = serde_json::json!({
            "schema": "axon-vm-run/1",
            "ok": result.is_ok(),
            "run_id": run_id,
            "exit_code": result.as_ref().map(|r| r.exit_code).unwrap_or(-1),
            "elapsed_ms": elapsed_ms,
            "error": result.as_ref().err().map(|e| e.to_string()),
            "principal": principal_name,
            "risk": manifest.risk,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        match result {
            Ok(r) => process::exit(r.exit_code),
            Err(e) => {
                eprintln!("axon-vm: {e}");
                process::exit(1);
            }
        }
    }
}

// ── Firecracker orchestration ─────────────────────────────────────────────────

struct RunResult {
    exit_code: i32,
}

fn run_in_firecracker(
    program: &Path,
    kernel: &Path,
    initrd: &Path,
    mem_mib: u64,
    vcpus: u64,
    vsock_port: u32,
    socket_path: &Path,
    mmds: &MmdsPayload,
    principal: Option<&Principal>,
) -> Result<RunResult, Box<dyn std::error::Error>> {
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

    // Wait for Firecracker to create its API socket (typically < 50ms).
    let api = wait_for_socket(socket_path, Duration::from_secs(5))?;

    // Configure boot source.
    fc_put(
        &api,
        "/boot-source",
        &serde_json::json!({
            "kernel_image_path": kernel.to_str().unwrap(),
            "boot_args": format!(
                "console=ttyS0 reboot=k panic=1 pci=off nomodules \
                 init=/init -- /init /usr/bin/axon run /axon/program.ax"
            ),
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

    // Enable MMDS V2 and bind it to the network interface.
    fc_put(
        &api,
        "/mmds/config",
        &serde_json::json!({
            "version": "V2",
            "network_interfaces": [],
        }),
    )?;

    // Write the axon policy payload to MMDS.
    let mmds_content = serde_json::json!({
        "latest": {
            "axon": mmds,
        }
    });
    fc_put(&api, "/mmds", &mmds_content)?;

    // Apply cgroup limits via jailer-style resource controls (if principal has limits).
    // In production use, Firecracker would be launched via jailer with uid/gid isolation.
    // Here we set balloon memory limits instead (available without jailer).
    if let Some(p) = principal {
        if p.mem_mib < mem_mib {
            fc_put(
                &api,
                "/balloon",
                &serde_json::json!({
                    "amount_mib": mem_mib - p.mem_mib,
                    "deflate_on_oom": true,
                }),
            )?;
        }
    }

    // Start a vsock relay thread to bridge vsock ↔ host_await callbacks.
    // For now we listen on the UDS path and echo any data (stub; real relay = Gap 6b).
    let vsock_uds = vsock_host_uds.clone();
    let _vsock_thread = std::thread::spawn(move || {
        vsock_relay_stub(&vsock_uds, vsock_port);
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
    fc_put(&api, "/actions", &serde_json::json!({"action_type": "InstanceStart"}))?;

    // Wait for Firecracker to exit and collect the exit code.
    let status = fc.wait()?;
    let exit_code = status.code().unwrap_or(1);

    // Clean up socket files.
    let _ = fs::remove_file(socket_path);
    let _ = fs::remove_file(&vsock_host_uds);

    Ok(RunResult { exit_code })
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
            return Ok(FcApi { socket_path: path.to_owned() });
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(format!("Firecracker socket not ready after {}s: {}", timeout.as_secs(), path.display()).into())
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

    let mut stream = UnixStream::connect(&api.socket_path)?;
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    // Read response — check for 2xx.
    let mut resp = Vec::new();
    stream.read_to_end(&mut resp)?;
    let resp_str = String::from_utf8_lossy(&resp);

    let status_line = resp_str.lines().next().unwrap_or("");
    // e.g. "HTTP/1.1 204 No Content"
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if status_code < 200 || status_code >= 300 {
        return Err(format!(
            "Firecracker API PUT {path} returned {status_line}\n{resp_str}"
        )
        .into());
    }

    Ok(())
}

// ── vsock relay (stub) ────────────────────────────────────────────────────────

/// Stub vsock relay: listens on the UDS path that Firecracker uses for vsock
/// connections (CID 2 = host). For each guest connection on vsock_port we
/// forward the request to the actual host handler (currently a no-op echo).
fn vsock_relay_stub(uds_path: &str, _vsock_port: u32) {
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
                std::thread::spawn(move || {
                    // Read 4-byte LE length + payload.
                    let mut lbuf = [0u8; 4];
                    if s.read_exact(&mut lbuf).is_err() { return; }
                    let len = u32::from_le_bytes(lbuf) as usize;
                    let mut buf = vec![0u8; len];
                    if s.read_exact(&mut buf).is_err() { return; }
                    let _req = String::from_utf8_lossy(&buf);

                    // Echo reply (stub — real relay would call host_await handler).
                    let reply = b"ok";
                    let rlen = (reply.len() as u32).to_le_bytes();
                    let _ = s.write_all(&rlen);
                    let _ = s.write_all(reply);
                });
            }
            Err(_) => break,
        }
    }
}

// ── BPF policy generation ─────────────────────────────────────────────────────

/// Generate a seccomp-BPF allowlist program from a list of syscall names.
/// Returns the base64-encoded BPF bytecode (sock_filter array, 8 bytes each).
///
/// Layout (N allowed syscalls):
///   [0]   LD  syscall_nr from seccomp_data.nr
///   [1..N] JEQ to ALLOW for each allowed syscall (fallthrough = next check)
///   [N+1] RET KILL_PROCESS (default deny)
///   [N+2] RET ALLOW
pub fn bpf_allowlist(syscall_names: &[String]) -> Result<String, Box<dyn std::error::Error>> {
    let mut syscall_nrs: Vec<u32> = Vec::with_capacity(syscall_names.len());
    for name in syscall_names {
        match SYSCALL_TABLE.iter().find(|(n, _)| *n == name.as_str()) {
            Some((_, nr)) => syscall_nrs.push(*nr),
            None => eprintln!("axon-vm: unknown syscall '{name}', skipping"),
        }
    }

    if syscall_nrs.is_empty() {
        return Err("no recognized syscall names".into());
    }

    let n = syscall_nrs.len();
    // Total instructions: 1 (LD) + N (JEQ) + 1 (RET KILL) + 1 (RET ALLOW)
    let n_insns = 1 + n + 2;
    let mut prog = Vec::with_capacity(n_insns * 8);

    // Encode a sock_filter as 8 LE bytes: [code_lo, code_hi, jt, jf, k0..k3]
    let encode = |buf: &mut Vec<u8>, code: u16, jt: u8, jf: u8, k: u32| {
        buf.extend_from_slice(&code.to_le_bytes());
        buf.push(jt);
        buf.push(jf);
        buf.extend_from_slice(&k.to_le_bytes());
    };

    // BPF opcodes
    const LD_W_ABS: u16 = 0x20; // load 32-bit word from absolute offset
    const JMP_JEQ_K: u16 = 0x15; // jump if equal to constant
    const RET_K: u16 = 0x06; // return constant

    // seccomp_data.nr offset = 0 (x86_64 ABI)
    encode(&mut prog, LD_W_ABS, 0, 0, 0);

    // JEQ instructions: if syscall_nr matches, jump forward to RET ALLOW.
    // The ALLOW instruction is at offset N+1 from insn 0 (= N - j from insn j).
    for (j, &nr) in syscall_nrs.iter().enumerate() {
        // jt = how many instructions to skip to reach ALLOW (index N+1 relative to this insn)
        let jt = (n - j) as u8; // j is 0-based; allow is at absolute index N+1
        encode(&mut prog, JMP_JEQ_K, jt, 0, nr);
    }

    // Default deny: KILL_PROCESS
    const KILL_PROCESS: u32 = 0x8000_0000;
    encode(&mut prog, RET_K, 0, 0, KILL_PROCESS);

    // ALLOW
    const ALLOW: u32 = 0x7fff_0000;
    encode(&mut prog, RET_K, 0, 0, ALLOW);

    Ok(base64::engine::general_purpose::STANDARD.encode(&prog))
}

// x86_64 Linux syscall table (most common; extend as needed).
static SYSCALL_TABLE: &[(&str, u32)] = &[
    ("read", 0), ("write", 1), ("open", 2), ("close", 3),
    ("stat", 4), ("fstat", 5), ("lstat", 6), ("poll", 7),
    ("lseek", 8), ("mmap", 9), ("mprotect", 10), ("munmap", 11),
    ("brk", 12), ("rt_sigaction", 13), ("rt_sigprocmask", 14),
    ("rt_sigreturn", 15), ("ioctl", 16), ("pread64", 17), ("pwrite64", 18),
    ("readv", 19), ("writev", 20), ("access", 21), ("pipe", 22),
    ("select", 23), ("sched_yield", 24), ("mremap", 25), ("msync", 26),
    ("mincore", 27), ("madvise", 28), ("dup", 32), ("dup2", 33),
    ("nanosleep", 35), ("getitimer", 36), ("alarm", 37), ("setitimer", 38),
    ("getpid", 39), ("sendfile", 40), ("socket", 41), ("connect", 42),
    ("accept", 43), ("sendto", 44), ("recvfrom", 45), ("sendmsg", 46),
    ("recvmsg", 47), ("shutdown", 48), ("bind", 49), ("listen", 50),
    ("getsockname", 51), ("getpeername", 52), ("socketpair", 53),
    ("setsockopt", 54), ("getsockopt", 55), ("clone", 56), ("fork", 57),
    ("vfork", 58), ("execve", 59), ("exit", 60), ("wait4", 61),
    ("kill", 62), ("uname", 63), ("fcntl", 72), ("fsync", 74),
    ("ftruncate", 77), ("getdents", 78), ("getcwd", 79), ("chdir", 80),
    ("rename", 82), ("mkdir", 83), ("rmdir", 84), ("creat", 85),
    ("link", 86), ("unlink", 87), ("symlink", 88), ("readlink", 89),
    ("chmod", 90), ("chown", 92), ("umask", 95), ("gettimeofday", 96),
    ("getrlimit", 97), ("getrusage", 98), ("sysinfo", 99), ("times", 100),
    ("getuid", 102), ("syslog", 103), ("getgid", 104), ("setuid", 105),
    ("setgid", 106), ("geteuid", 107), ("getegid", 108),
    ("setpgid", 109), ("getppid", 110), ("getpgrp", 111), ("setsid", 112),
    ("setreuid", 113), ("setregid", 114), ("getgroups", 115), ("setgroups", 116),
    ("setresuid", 117), ("getresuid", 118), ("setresgid", 119), ("getresgid", 120),
    ("getpgid", 121), ("setfsuid", 122), ("setfsgid", 123), ("getsid", 124),
    ("capget", 125), ("capset", 126), ("rt_sigsuspend", 130),
    ("sigaltstack", 131), ("utime", 132), ("mknod", 133), ("personality", 135),
    ("statfs", 137), ("fstatfs", 138), ("getpriority", 140),
    ("setpriority", 141), ("sched_setparam", 142), ("sched_getparam", 143),
    ("sched_setscheduler", 144), ("sched_getscheduler", 145),
    ("sched_get_priority_max", 146), ("sched_get_priority_min", 147),
    ("sched_rr_get_interval", 148), ("mlock", 149), ("munlock", 150),
    ("mlockall", 151), ("munlockall", 152), ("vhangup", 153),
    ("pivot_root", 155), ("prctl", 157), ("arch_prctl", 158),
    ("adjtimex", 159), ("setrlimit", 160), ("chroot", 161), ("sync", 162),
    ("acct", 163), ("settimeofday", 164), ("umount2", 166), ("swapon", 167),
    ("swapoff", 168), ("reboot", 169), ("sethostname", 170), ("setdomainname", 171),
    ("iopl", 172), ("ioperm", 173), ("init_module", 175), ("delete_module", 176),
    ("gettid", 186), ("readahead", 187), ("getxattr", 191), ("lgetxattr", 192),
    ("fgetxattr", 193), ("listxattr", 194), ("llistxattr", 195), ("flistxattr", 196),
    ("removexattr", 197), ("lremovexattr", 198), ("fremovexattr", 199),
    ("tkill", 200), ("time", 201), ("futex", 202), ("sched_setaffinity", 203),
    ("sched_getaffinity", 204), ("io_setup", 206), ("io_destroy", 207),
    ("io_getevents", 208), ("io_submit", 209), ("io_cancel", 210),
    ("lookup_dcookie", 212), ("epoll_create", 213), ("epoll_ctl_old", 214),
    ("epoll_wait_old", 215), ("remap_file_pages", 216), ("getdents64", 217),
    ("set_tid_address", 218), ("restart_syscall", 219), ("semtimedop", 220),
    ("fadvise64", 221), ("timer_create", 222), ("timer_settime", 223),
    ("timer_gettime", 224), ("timer_getoverrun", 225), ("timer_delete", 226),
    ("clock_settime", 227), ("clock_gettime", 228), ("clock_getres", 229),
    ("clock_nanosleep", 230), ("exit_group", 231), ("epoll_wait", 232),
    ("epoll_ctl", 233), ("tgkill", 234), ("utimes", 235),
    ("mbind", 237), ("set_mempolicy", 238), ("get_mempolicy", 239),
    ("mq_open", 240), ("mq_unlink", 241), ("mq_timedsend", 242),
    ("mq_timedreceive", 243), ("mq_notify", 244), ("mq_getsetattr", 245),
    ("waitid", 247), ("add_key", 248), ("request_key", 249), ("keyctl", 250),
    ("ioprio_set", 251), ("ioprio_get", 252), ("inotify_init", 253),
    ("inotify_add_watch", 254), ("inotify_rm_watch", 255), ("migrate_pages", 256),
    ("openat", 257), ("mkdirat", 258), ("mknodat", 259), ("fchownat", 260),
    ("futimesat", 261), ("newfstatat", 262), ("unlinkat", 263), ("renameat", 264),
    ("linkat", 265), ("symlinkat", 266), ("readlinkat", 267), ("fchmodat", 268),
    ("faccessat", 269), ("pselect6", 270), ("ppoll", 271), ("unshare", 272),
    ("set_robust_list", 273), ("get_robust_list", 274), ("splice", 275),
    ("tee", 276), ("sync_file_range", 277), ("vmsplice", 278),
    ("move_pages", 279), ("utimensat", 280), ("epoll_pwait", 281),
    ("signalfd", 282), ("timerfd_create", 283), ("eventfd", 284),
    ("fallocate", 285), ("timerfd_settime", 286), ("timerfd_gettime", 287),
    ("accept4", 288), ("signalfd4", 289), ("eventfd2", 290),
    ("epoll_create1", 291), ("dup3", 292), ("pipe2", 293),
    ("inotify_init1", 294), ("preadv", 295), ("pwritev", 296),
    ("rt_tgsigqueueinfo", 297), ("perf_event_open", 298), ("recvmmsg", 299),
    ("fanotify_init", 300), ("fanotify_mark", 301), ("prlimit64", 302),
    ("name_to_handle_at", 303), ("open_by_handle_at", 304), ("clock_adjtime", 305),
    ("syncfs", 306), ("sendmmsg", 307), ("setns", 308), ("getcpu", 309),
    ("process_vm_readv", 310), ("process_vm_writev", 311), ("kcmp", 312),
    ("finit_module", 313), ("sched_setattr", 314), ("sched_getattr", 315),
    ("renameat2", 316), ("seccomp", 317), ("getrandom", 318),
    ("memfd_create", 319), ("kexec_file_load", 320), ("bpf", 321),
    ("execveat", 322), ("userfaultfd", 323), ("membarrier", 324),
    ("mlock2", 325), ("copy_file_range", 326), ("preadv2", 327),
    ("pwritev2", 328), ("pkey_mprotect", 329), ("pkey_alloc", 330),
    ("pkey_free", 331), ("statx", 332), ("io_pgetevents", 333),
    ("rseq", 334),
];

// ── Manifest loading ──────────────────────────────────────────────────────────

fn load_manifest(program: &Path) -> AxonManifest {
    let meta_path = program.with_extension("axmeta");
    if !meta_path.exists() {
        return AxonManifest::default();
    }
    match fs::read_to_string(&meta_path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => AxonManifest::default(),
    }
}

// ── Helper: sha256 a file ─────────────────────────────────────────────────────

fn sha256_file(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path).unwrap_or_default();
    let hash = Sha256::digest(&bytes);
    format!("{hash:x}")
}

// ── Helper: find Firecracker binary ──────────────────────────────────────────

fn which_firecracker() -> Result<PathBuf, Box<dyn std::error::Error>> {
    for candidate in &["firecracker", "/usr/local/bin/firecracker", "/opt/firecracker/firecracker"] {
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

// ── Gap 7: Principal registry ─────────────────────────────────────────────────

fn registry_path() -> PathBuf {
    let base = env::var("AXON_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs_home()
                .map(|h| h.join(".config/axon"))
                .unwrap_or_else(|| PathBuf::from(".axon"))
        });
    base.join("principals.toml")
}

fn dirs_home() -> Option<PathBuf> {
    env::var("HOME").ok().map(PathBuf::from)
}

fn load_registry() -> PrincipalRegistry {
    let path = registry_path();
    if !path.exists() {
        return PrincipalRegistry::default();
    }
    let s = fs::read_to_string(&path).unwrap_or_default();
    toml::from_str(&s).unwrap_or_default()
}

fn save_registry(reg: &PrincipalRegistry) {
    let path = registry_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let s = toml::to_string_pretty(reg).expect("serialize registry");
    fs::write(&path, s).expect("write registry");
}

fn load_principal(name: &str) -> Option<Principal> {
    let reg = load_registry();
    reg.principals.into_iter().find(|p| p.name == name)
}

fn cmd_principal_add(
    name: String,
    budget_tokens: u64,
    allowed_effects: String,
    mem_mib: u64,
    cpu_pct: u32,
) {
    let mut reg = load_registry();
    if reg.principals.iter().any(|p| p.name == name) {
        eprintln!("axon-vm: principal '{name}' already exists");
        process::exit(1);
    }
    let effects: Vec<String> = allowed_effects
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    reg.principals.push(Principal {
        name: name.clone(),
        budget_tokens,
        allowed_effects: effects,
        mem_mib,
        cpu_pct,
    });
    save_registry(&reg);
    println!("principal '{name}' added ({} → {})", registry_path().display(), budget_tokens);
}

fn cmd_principal_list() {
    let reg = load_registry();
    if reg.principals.is_empty() {
        println!("no principals registered (axon-vm principal add <name>)");
        return;
    }
    println!("{:<20} {:>12}  {:>6}  effects", "name", "budget_tokens", "mem_mib");
    println!("{}", "-".repeat(60));
    for p in &reg.principals {
        println!(
            "{:<20} {:>12}  {:>6}  {}",
            p.name,
            p.budget_tokens,
            p.mem_mib,
            p.allowed_effects.join(",")
        );
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpf_encodes_allow_write_exit() {
        // Allow write(1) + exit_group(231) — minimal Axon pure program.
        let names: Vec<String> = vec!["write".into(), "exit_group".into()];
        let b64 = bpf_allowlist(&names).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        // 1 LD + 2 JEQ + 1 KILL + 1 ALLOW = 5 instructions × 8 bytes = 40
        assert_eq!(bytes.len(), 40);
    }

    #[test]
    fn bpf_first_insn_is_ld_abs() {
        let names: Vec<String> = vec!["read".into()];
        let b64 = bpf_allowlist(&names).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        // First 2 bytes = opcode (LD_W_ABS = 0x0020 LE)
        assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]), 0x0020);
    }

    #[test]
    fn bpf_last_insn_is_ret_allow() {
        let names: Vec<String> = vec!["write".into()];
        let b64 = bpf_allowlist(&names).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        let last_insn = &bytes[bytes.len() - 8..];
        // RET_K = 0x0006
        assert_eq!(u16::from_le_bytes([last_insn[0], last_insn[1]]), 0x0006);
        // k = ALLOW = 0x7fff_0000
        assert_eq!(u32::from_le_bytes([last_insn[4], last_insn[5], last_insn[6], last_insn[7]]), 0x7fff_0000);
    }

    #[test]
    fn bpf_kill_insn_before_allow() {
        let names: Vec<String> = vec!["write".into()];
        let b64 = bpf_allowlist(&names).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        // Second-to-last instruction = KILL
        let kill_insn = &bytes[bytes.len() - 16..bytes.len() - 8];
        assert_eq!(u32::from_le_bytes([kill_insn[4], kill_insn[5], kill_insn[6], kill_insn[7]]), 0x8000_0000);
    }

    #[test]
    fn principal_registry_roundtrip() {
        let reg = PrincipalRegistry {
            principals: vec![Principal {
                name: "test-agent".to_string(),
                budget_tokens: 5000,
                allowed_effects: vec!["AI".to_string()],
                mem_mib: 64,
                cpu_pct: 25,
            }],
        };
        let s = toml::to_string_pretty(&reg).unwrap();
        let back: PrincipalRegistry = toml::from_str(&s).unwrap();
        assert_eq!(back.principals[0].name, "test-agent");
        assert_eq!(back.principals[0].budget_tokens, 5000);
    }

    #[test]
    fn manifest_default_when_missing() {
        let m = load_manifest(Path::new("/nonexistent/program.ax"));
        assert!(m.syscall_hint.is_none());
        assert!(m.effect_union.is_none());
    }

    #[test]
    fn sha256_file_nonexistent_is_empty_hash() {
        // Empty input → well-known SHA256.
        let h = sha256_file(Path::new("/nonexistent_file_axon_vm_test"));
        // SHA256 of empty bytes
        assert_eq!(h, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }
}
