//! axon-guest-init — static PID-1 supervisor for Axon microVM guests.
//!
//! Boot sequence:
//!   1. Re-seed entropy from virtio-rng (/dev/urandom)
//!   2. Read capability policy from MMDS at 169.254.169.254 (schema axon-vm-mmds/1).
//!      If no policy can be read, REFUSE to start the guest — an absent policy is
//!      not a permissive one, and this binary exists to install the sandbox.
//!      `AXON_GUEST_ALLOW_NO_POLICY=1` opts into the old unpoliced behaviour
//!      (development only) and says so loudly on every boot.
//!   3. Fork: parent becomes PID-1 supervisor; child applies seccomp then execs Axon
//!   4. Supervisor loop: reap zombies, forward SIGTERM/SIGINT, exit with child's code
//!
//! Invocation:
//!   axon-guest-init <binary> [args...]
//!   axon-guest-init /usr/bin/axon run /axon/program.ax
//!
//! Environment variables exported to child (from MMDS payload):
//!   AXON_PRINCIPAL, AXON_BUDGET_TOKENS, AXON_RUN_ID, AXON_ALLOWED_EFFECTS,
//!   AXON_SOURCE_HASH
//!
//! Two of those are the guest's ENFORCED policy, not just labels:
//! AXON_ALLOWED_EFFECTS is the run's effect ceiling (SandboxViolation, exit 8)
//! and AXON_BUDGET_TOKENS its AI token cap (E1303, exit 5). Both were exported
//! here and read by nothing for as long as they have existed, so a policy that
//! capped tokens or restricted effects produced a guest that did neither and
//! said nothing about it. The other three are LABELS, deliberately: AXON_PRINCIPAL
//! is audit attribution, AXON_RUN_ID correlates records, and AXON_SOURCE_HASH
//! carries the approved digest. None of the three grants or withholds anything,
//! and nothing in the workspace reads them -- in particular the guest does NOT
//! today check the loaded image against AXON_SOURCE_HASH (R36 S2 owns that; see
//! its (i) clause). Do not read this list as a policy that is enforced.

use std::ffi::CString;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;
use std::{env, process};

use base64::Engine as _;
use serde::Deserialize;

// ── MMDS payload ──────────────────────────────────────────────────────────────

/// Schema: axon-vm-mmds/1
/// Written by axon-vm launcher before InstanceStart; read by us at boot.
impl MmdsPayload {
    /// Does this payload actually CONSTRAIN anything?
    ///
    /// `policy.is_some()` answered a SYNTAX question — did a body deserialize —
    /// and was used to answer a SEMANTIC one: is a security policy active.
    /// Every field is `Option`, so `{}` parses to an all-`None` payload, which
    /// read as "policy loaded" and took the `Apply` branch: no refusal, no
    /// "NO ceiling" warning, no env var set, and `apply_seccomp` never called.
    /// The guest booted with no effect ceiling, no token cap and no seccomp
    /// while the boot log said a policy had been applied.
    ///
    /// REPRODUCED at parse level: `{}`, an all-null body, and an
    /// unknown-fields-only body all deserialize to `Some(all None)`.
    ///
    /// Only the three ENFORCED mechanisms count. `principal`, `run_id` and
    /// `source_hash` are LABELS — the module header above is explicit that they
    /// "grant and withhold nothing" — so a payload carrying only labels is
    /// exactly as unpoliced as `{}` and must not be rescued by them.
    fn constrains_anything(&self) -> bool {
        self.allowed_effects.is_some()
            || self.budget_tokens.is_some()
            || self.seccomp_bpf_b64.is_some()
    }
}

#[derive(Deserialize, Debug)]
struct MmdsPayload {
    principal: Option<String>,
    allowed_effects: Option<Vec<String>>,
    budget_tokens: Option<u64>,
    source_hash: Option<String>,
    seccomp_bpf_b64: Option<String>,
    run_id: Option<String>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    // Default target when invoked bare: /usr/bin/axon run /axon/program.ax
    let (binary, exec_args): (String, Vec<String>) = if args.len() >= 2 {
        (args[1].clone(), args[1..].to_vec())
    } else {
        let bin = "/usr/bin/axon".to_string();
        (
            bin.clone(),
            vec![bin, "run".to_string(), "/axon/program.ax".to_string()],
        )
    };

    // 1. Re-seed entropy before any crypto-adjacent work.
    reseed_entropy();

    // 2. Read policy from MMDS, and REFUSE to exec if there isn't one.
    //
    //    This used to soft-fail: an unreachable metadata service logged one
    //    line that read like a note and handed `None` down, and `child_main`
    //    then skipped the whole policy block — no effect ceiling, no token cap,
    //    no seccomp — and exec'd the guest anyway. The sandbox this binary
    //    exists to install was simply absent, and the only evidence was a line
    //    saying "running without policy" among the boot messages.
    //
    //    The soft-fail was justified as "running outside axon-vm". Nothing in
    //    the workspace runs it that way: it is `/sbin/init` in
    //    `axon-rootfs.ext4`, PID 1 of a microVM, and no script or test invokes
    //    it directly. So the case being preserved had no caller, while the case
    //    being broken — a VM that boots but cannot reach MMDS — is exactly when
    //    a capability policy matters.
    //
    //    The developer case is still reachable, but must now be ASKED for.
    let allow_unpoliced = env::var("AXON_GUEST_ALLOW_NO_POLICY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let policy = match read_mmds() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[axon-guest-init] MMDS unavailable ({e})");
            None
        }
    };
    // CONTENT, not `is_some()`. A body that parsed but constrains nothing is
    // not a policy — see `MmdsPayload::constrains_anything`.
    let have_policy = policy.as_ref().is_some_and(|p| p.constrains_anything());
    match policy_decision(have_policy, allow_unpoliced) {
        PolicyDecision::Apply | PolicyDecision::ProceedUnpoliced => {
            // A policy can be ACTIVE and still leave a mechanism off. Say which
            // one, per mechanism, rather than treating "a policy loaded" as a
            // blanket assurance — "policy applied" beside an absent seccomp
            // filter is the same overclaim in miniature as `{}` reading as a
            // policy at all.
            if let Some(p) = policy.as_ref().filter(|_| have_policy) {
                if p.allowed_effects.is_none() {
                    eprintln!(
                        "[axon-guest-init] WARNING: policy loaded with NO effect ceiling                          (allowed_effects omitted) — the guest runs unrestricted on that axis"
                    );
                }
                if p.seccomp_bpf_b64.is_none() {
                    eprintln!(
                        "[axon-guest-init] WARNING: policy loaded with NO seccomp filter                          (seccomp_bpf_b64 omitted) — defence in depth is absent"
                    );
                }
                if p.budget_tokens.is_none() {
                    eprintln!(
                        "[axon-guest-init] WARNING: policy loaded with NO token cap                          (budget_tokens omitted) — AI spend is uncapped"
                    );
                }
            }
        }
        PolicyDecision::Refuse => {
            eprintln!(
                "[axon-guest-init] REFUSING to start the guest: no capability policy was \
                 loaded, so there would be no effect ceiling, no token cap and no seccomp \
                 filter. Set AXON_GUEST_ALLOW_NO_POLICY=1 to run unpoliced on purpose \
                 (development only)."
            );
            process::exit(1);
        }
    }
    if policy.is_none() {
        eprintln!(
            "[axon-guest-init] WARNING: AXON_GUEST_ALLOW_NO_POLICY is set — the guest is \
             running with NO effect ceiling, NO token cap and NO seccomp filter."
        );
    }

    // 3. Fork.
    let child_pid = unsafe { libc::fork() };
    match child_pid {
        -1 => {
            eprintln!(
                "[axon-guest-init] fork failed: {}",
                std::io::Error::last_os_error()
            );
            process::exit(1);
        }
        0 => child_main(policy, &binary, &exec_args),
        pid => supervisor_main(pid),
    }
}

/// What to do about the policy we did (or did not) load.
///
/// A value rather than inline control flow so it can be tested: this crate is a
/// PID-1 binary whose main path ends in `exec`, and it had no tests at all — so
/// the rule that decides whether an unpoliced guest may start was expressed
/// only in code that cannot be run from a test.
#[derive(Debug, PartialEq, Eq)]
enum PolicyDecision {
    /// A policy was loaded; apply it.
    Apply,
    /// No policy, and the operator explicitly allowed running without one.
    ProceedUnpoliced,
    /// No policy and no explicit override — do not start the guest.
    Refuse,
}

fn policy_decision(have_policy: bool, allow_unpoliced: bool) -> PolicyDecision {
    match (have_policy, allow_unpoliced) {
        (true, _) => PolicyDecision::Apply,
        (false, true) => PolicyDecision::ProceedUnpoliced,
        // Fail closed: an absent policy is not a permissive one.
        (false, false) => PolicyDecision::Refuse,
    }
}

// ── Child: apply policy then exec ─────────────────────────────────────────────

fn child_main(policy: Option<MmdsPayload>, binary: &str, exec_args: &[String]) -> ! {
    if let Some(p) = &policy {
        // Export identity to the Axon runtime (provenance log, ai_complete tier).
        if let Some(principal) = &p.principal {
            env::set_var("AXON_PRINCIPAL", principal);
        }
        if let Some(tokens) = p.budget_tokens {
            env::set_var("AXON_BUDGET_TOKENS", tokens.to_string());
        }
        if let Some(run_id) = &p.run_id {
            env::set_var("AXON_RUN_ID", run_id);
        }
        if let Some(effects) = &p.allowed_effects {
            env::set_var("AXON_ALLOWED_EFFECTS", effects.join(","));
        }
        if let Some(hash) = &p.source_hash {
            env::set_var("AXON_SOURCE_HASH", hash);
        }

        // Apply seccomp AFTER setting env vars (set_var would be blocked after).
        if let Some(bpf_b64) = &p.seccomp_bpf_b64 {
            if let Err(e) = apply_seccomp(bpf_b64) {
                eprintln!("[axon-guest-init] seccomp apply failed: {e}");
                process::exit(1);
            }
        }
    }

    exec_process(binary, exec_args)
}

// ── Entropy re-seed ───────────────────────────────────────────────────────────

fn reseed_entropy() {
    // The VM may have restored from a snapshot (repeating the RNG pool).
    // Reading from /dev/urandom triggers mixing of the virtio-rng hardware
    // entropy source into the kernel pool.
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let mut buf = [0u8; 64];
        let _ = f.read(&mut buf);
        // Drop buf immediately — we only care about the side-effect.
    }
}

// ── MMDS client ───────────────────────────────────────────────────────────────

const MMDS_ADDR: &str = "169.254.169.254:80";
const MMDS_TIMEOUT: Duration = Duration::from_secs(3);

fn read_mmds() -> Result<Option<MmdsPayload>, String> {
    // Step 1: obtain a V2 session token.
    let token = mmds_get_token()?;

    // Step 2: GET /latest/axon with the session token.
    let body = mmds_get("/latest/axon", &token)?;

    let body = body.trim();
    if body.is_empty() || body == "null" {
        return Ok(None);
    }

    // The launcher writes the full payload under /latest/axon in the MMDS
    // store, so the response body IS the MmdsPayload JSON.
    // MALFORMED, not unavailable. Both fail closed, but the call site logged
    // every Err as "MMDS unavailable", so a launcher writing bad JSON and a
    // launcher that cannot be reached produced the same message and sent the
    // operator looking at the network.
    let payload: MmdsPayload = serde_json::from_str(body)
        .map_err(|e| format!("MMDS policy is MALFORMED (not unreachable): {e}"))?;

    Ok(Some(payload))
}

fn mmds_get_token() -> Result<String, String> {
    let mut stream = tcp_connect()?;
    // PUT /latest/api/token with the desired TTL in a header.
    // Body is empty; Content-Length: 0 is required by MMDS V2.
    let req = "PUT /latest/api/token HTTP/1.0\r\n\
               X-metadata-token-ttl-seconds: 60\r\n\
               Content-Length: 0\r\n\r\n";
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("write token request: {e}"))?;

    let body = http_response_body(stream)?;
    let token = body.trim().to_string();
    if token.is_empty() {
        return Err("MMDS returned empty token".to_string());
    }
    Ok(token)
}

fn mmds_get(path: &str, token: &str) -> Result<String, String> {
    let mut stream = tcp_connect()?;
    let req = format!(
        "GET {path} HTTP/1.0\r\nX-metadata-token: {token}\r\nAccept: application/json\r\n\r\n"
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("write GET request: {e}"))?;
    http_response_body(stream)
}

fn tcp_connect() -> Result<TcpStream, String> {
    let addr: std::net::SocketAddr = MMDS_ADDR
        .parse()
        .map_err(|e| format!("parse MMDS addr: {e}"))?;
    let stream = TcpStream::connect_timeout(&addr, MMDS_TIMEOUT)
        .map_err(|e| format!("connect {MMDS_ADDR}: {e}"))?;
    stream
        .set_read_timeout(Some(MMDS_TIMEOUT))
        .map_err(|e| format!("set_read_timeout: {e}"))?;
    Ok(stream)
}

/// Read an HTTP/1.0 response and return its body (everything after the blank line).
fn http_response_body(mut stream: TcpStream) -> Result<String, String> {
    let mut raw = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&tmp[..n]),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                break
            }
            Err(e) => return Err(format!("read response: {e}")),
        }
    }
    let text = String::from_utf8_lossy(&raw);
    // Strip status line + headers; body starts after the first blank line.
    if let Some(pos) = text.find("\r\n\r\n") {
        Ok(text[pos + 4..].to_string())
    } else if let Some(pos) = text.find("\n\n") {
        Ok(text[pos + 2..].to_string())
    } else {
        // No headers found — treat the whole response as body (shouldn't happen).
        Ok(text.to_string())
    }
}

// ── Seccomp ───────────────────────────────────────────────────────────────────

/// Apply a pre-compiled seccomp-bpf program to the calling thread.
///
/// The BPF bytecode was generated by the Axon compiler from the program's
/// effect-row annotations and base64-encoded into the MMDS payload by the
/// axon-vm launcher. Each BPF instruction is 8 bytes (sock_filter layout:
/// u16 code, u8 jt, u8 jf, u32 k).
fn apply_seccomp(bpf_b64: &str) -> Result<(), String> {
    let bpf_bytes = base64::engine::general_purpose::STANDARD
        .decode(bpf_b64)
        .map_err(|e| format!("base64 decode BPF: {e}"))?;

    if bpf_bytes.is_empty() {
        return Err("BPF bytecode is empty".to_string());
    }
    if bpf_bytes.len() % 8 != 0 {
        return Err(format!(
            "BPF bytecode length {} is not a multiple of 8 (sock_filter size)",
            bpf_bytes.len()
        ));
    }
    let n_insns = (bpf_bytes.len() / 8) as u16;

    unsafe {
        // PR_SET_NO_NEW_PRIVS: mandatory prerequisite for installing a seccomp
        // filter without CAP_SYS_ADMIN. Irreversible for this process and all
        // descendants — that is intentional.
        let r = libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1usize, 0usize, 0usize, 0usize);
        if r != 0 {
            return Err(format!(
                "PR_SET_NO_NEW_PRIVS failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        // Install the BPF filter. The sock_fprog struct holds a pointer to
        // the instructions — bpf_bytes must remain live for the duration of
        // this call (it is, since it's on our stack).
        let prog = libc::sock_fprog {
            len: n_insns,
            filter: bpf_bytes.as_ptr() as *mut libc::sock_filter,
        };
        let r = libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER as libc::c_ulong,
            &prog as *const libc::sock_fprog as libc::c_ulong,
            0usize,
            0usize,
        );
        if r != 0 {
            return Err(format!(
                "PR_SET_SECCOMP failed: {}",
                std::io::Error::last_os_error()
            ));
        }
    }

    eprintln!("[axon-guest-init] seccomp applied ({n_insns} instructions)");
    Ok(())
}

// ── exec ──────────────────────────────────────────────────────────────────────

fn exec_process(binary: &str, args: &[String]) -> ! {
    let c_binary = CString::new(binary).unwrap_or_else(|_| {
        eprintln!("[axon-guest-init] binary path contains null byte");
        process::exit(1);
    });
    let c_args: Vec<CString> = args
        .iter()
        .map(|a| {
            CString::new(a.as_str()).unwrap_or_else(|_| {
                eprintln!("[axon-guest-init] argument contains null byte: {a:?}");
                process::exit(1);
            })
        })
        .collect();
    let mut ptrs: Vec<*const libc::c_char> = c_args.iter().map(|a| a.as_ptr()).collect();
    ptrs.push(std::ptr::null());

    unsafe { libc::execvp(c_binary.as_ptr(), ptrs.as_ptr()) };

    // Only reached if execvp fails.
    eprintln!(
        "[axon-guest-init] execvp({binary:?}) failed: {}",
        std::io::Error::last_os_error()
    );
    process::exit(127);
}

// ── PID-1 supervisor ──────────────────────────────────────────────────────────

// Signal handlers need to know which PID to forward to.
static CHILD_PID: AtomicI32 = AtomicI32::new(0);

extern "C" fn forward_signal(sig: libc::c_int) {
    let pid = CHILD_PID.load(Ordering::Relaxed);
    if pid > 0 {
        unsafe { libc::kill(pid, sig) };
    }
}

fn supervisor_main(first_child: libc::pid_t) -> ! {
    CHILD_PID.store(first_child, Ordering::Relaxed);

    unsafe {
        // As PID 1, signals whose default action is "ignore" stay ignored unless
        // we install handlers. Install forwarding handlers for the two most
        // common termination signals so they propagate to the Axon child.
        libc::signal(
            libc::SIGTERM,
            forward_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGINT,
            forward_signal as *const () as libc::sighandler_t,
        );
    }

    let mut exit_code: i32 = 0;

    loop {
        let mut status: libc::c_int = 0;
        // Block until any child changes state. EINTR (signal interrupted
        // the wait) just loops again.
        let pid = unsafe { libc::waitpid(-1, &mut status, 0) };

        if pid < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue; // EINTR — signal delivered, loop
            }
            // ECHILD: no more children. Exit with whatever code we captured.
            break;
        }

        if pid == 0 {
            continue;
        }

        // Determine the exit code if this is our main child.
        if pid == first_child {
            if libc::WIFEXITED(status) {
                exit_code = libc::WEXITSTATUS(status);
            } else if libc::WIFSIGNALED(status) {
                // Shell convention: 128 + signal number.
                exit_code = 128 + libc::WTERMSIG(status);
            }
            // Drain any remaining zombie grandchildren before exiting.
            loop {
                let r = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
                if r <= 0 {
                    break;
                }
            }
            break;
        }
        // Else: zombie grandchild reaped — continue supervising.
    }

    process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::{policy_decision, MmdsPayload, PolicyDecision};

    /// The defect: an unreachable MMDS produced a guest with no effect ceiling,
    /// no token cap and no seccomp, started anyway, announced by a single line
    /// that read like a note.
    #[test]
    fn no_policy_refuses_to_start_the_guest() {
        assert_eq!(policy_decision(false, false), PolicyDecision::Refuse);
    }

    /// The developer case the old soft-fail was justified by. It survives, but
    /// has to be asked for — which is the whole difference.
    #[test]
    fn no_policy_with_an_explicit_override_proceeds() {
        assert_eq!(
            policy_decision(false, true),
            PolicyDecision::ProceedUnpoliced
        );
    }

    /// A loaded policy is applied regardless of the override, so setting the
    /// escape hatch cannot accidentally DISABLE a policy that was read.
    #[test]
    fn a_loaded_policy_is_applied_and_the_override_cannot_discard_it() {
        assert_eq!(policy_decision(true, false), PolicyDecision::Apply);
        assert_eq!(policy_decision(true, true), PolicyDecision::Apply);
    }

    /// A body that PARSES is not a policy that CONSTRAINS.
    ///
    /// REPRODUCED before the fix: `{}` deserializes to an all-`None`
    /// `MmdsPayload`, `policy.is_some()` read that as "policy loaded", and the
    /// Apply branch ran — no refusal, no warning, no env var set, and
    /// `apply_seccomp` never called. The guest booted with no effect ceiling,
    /// no token cap and no seccomp while the record showed a policied run.
    #[test]
    fn a_payload_that_constrains_nothing_is_not_a_policy() {
        for body in [
            "{}",
            r#"{"principal":null,"allowed_effects":null}"#,
            r#"{"bogus":123}"#,
        ] {
            let p: MmdsPayload =
                serde_json::from_str(body).unwrap_or_else(|e| panic!("{body} should parse: {e}"));
            assert!(
                !p.constrains_anything(),
                "{body} parsed to a payload that claims to constrain something"
            );
            assert_eq!(
                policy_decision(p.constrains_anything(), false),
                PolicyDecision::Refuse,
                "{body} must fail closed"
            );
        }
    }

    /// Labels must not rescue an empty policy.
    ///
    /// `principal`, `run_id` and `source_hash` grant and withhold nothing — the
    /// module header says so. A payload carrying only labels is exactly as
    /// unpoliced as `{}`, and counting them would let a launcher disarm every
    /// enforced mechanism while still looking configured.
    #[test]
    fn a_labels_only_payload_does_not_count_as_policy() {
        let body = r#"{"principal":"alice","run_id":"r1","source_hash":"abc"}"#;
        let p: MmdsPayload = serde_json::from_str(body).expect("parses");
        assert!(
            !p.constrains_anything(),
            "labels are not policy: they grant and withhold nothing"
        );
    }

    /// Control: the fix must not refuse everything. ONE enforced field is a
    /// policy.
    #[test]
    fn a_payload_with_one_enforced_field_is_a_policy() {
        for body in [
            r#"{"allowed_effects":["IO"]}"#,
            r#"{"budget_tokens":100}"#,
            r#"{"seccomp_bpf_b64":"AAAA"}"#,
            r#"{"allowed_effects":[]}"#,
        ] {
            let p: MmdsPayload = serde_json::from_str(body).expect("parses");
            assert!(
                p.constrains_anything(),
                "{body} states an enforced mechanism and must count as a policy"
            );
            assert_eq!(
                policy_decision(p.constrains_anything(), false),
                PolicyDecision::Apply
            );
        }
    }

    /// An explicitly EMPTY effect list is a policy — the tightest one. It must
    /// not be confused with an omitted field, which states nothing. This is the
    /// same "unset is not empty" distinction the effect ceiling itself makes.
    #[test]
    fn an_explicitly_empty_effect_list_is_not_an_absent_one() {
        let empty: MmdsPayload = serde_json::from_str(r#"{"allowed_effects":[]}"#).unwrap();
        let absent: MmdsPayload = serde_json::from_str("{}").unwrap();
        assert!(empty.constrains_anything(), "deny-all is a policy");
        assert!(!absent.constrains_anything(), "omitted is not a policy");
    }

    /// Malformed JSON fails closed AND says it was malformed.
    #[test]
    fn malformed_json_is_reported_as_malformed_not_unavailable() {
        let e = serde_json::from_str::<MmdsPayload>("{not json")
            .map_err(|e| format!("MMDS policy is MALFORMED (not unreachable): {e}"))
            .unwrap_err();
        assert!(
            e.contains("MALFORMED"),
            "a launcher writing bad JSON must not be reported as an unreachable \
             metadata service: {e}"
        );
    }
}
