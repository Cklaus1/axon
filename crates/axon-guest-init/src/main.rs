//! axon-guest-init — static PID-1 supervisor for Axon microVM guests.
//!
//! Boot sequence:
//!   1. Re-seed entropy from virtio-rng (/dev/urandom)
//!   2. Read the capability policy (schema axon-vm-mmds/1, or /2 with the `fs`
//!      axis) and REFUSE to start the guest unless it constrains something
//!      (see "Policy channels").
//!   3. Fork: parent becomes PID-1 supervisor; child applies seccomp then execs Axon
//!   4. Supervisor loop: reap zombies, forward SIGTERM/SIGINT, exit with child's code
//!
//! Policy channels, in order of precedence:
//!
//! * The KERNEL CMDLINE (`/proc/cmdline`): one word `axon.policy=<standard
//!   padded base64 of the JSON payload>` — the encoding
//!   `axon_vm::firecracker::embed_policy_in_cmdline` writes. This is the only
//!   channel a NIC-less guest (the B263 profile) has. When the word is present
//!   it WINS: MMDS is not consulted, and a malformed cmdline policy is a
//!   refusal, never a fall-through to a second channel.
//! * MMDS at 169.254.169.254, only when the cmdline carries no policy word.
//!
//! If neither yields a policy that CONSTRAINS something the guest is refused —
//! an absent policy is not a permissive one, and this binary exists to install
//! the sandbox. The old `AXON_GUEST_ALLOW_NO_POLICY=1` escape exists ONLY in
//! builds with the non-default cargo feature `dev-allow-no-policy`. It is not a
//! runtime flag in a default build because Linux copies unrecognised
//! `NAME=value` cmdline words into init's ENVIRONMENT: whoever can append a
//! word to the cmdline could otherwise switch the sandbox off (ACF-G25: fail
//! closed "without development bypass").
//!
//! Invocation:
//!   axon-guest-init <binary> [args...]
//!   axon-guest-init --source-fd N=PATH -- <binary> [args...]
//!   axon-guest-init        (bare: --source-fd 3=/axon/program.ax --
//!                           /usr/bin/axon run --source-fd 3 /axon/program.ax)
//!
//! `--source-fd N=PATH` (guest-fs GF-2, load before gate): the forked child
//! opens PATH (O_NOFOLLOW, a regular file) BEFORE it applies any seccomp filter
//! and leaves it open on fd N (N >= 3) across the exec, so the workload
//! (`axon run --source-fd N PATH`) never opens its own program and a filter
//! withholding file syscalls can cover it from its first instruction. The argv
//! after `--` is passed through untouched; the caller states the fd.
//!
//! Environment variables exported to child (from the policy payload):
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
//!
//! The `fs` axis (guest-fs GF-3, schema `axon-vm-mmds/2` only): `"fs": "none"`
//! makes the child export `AXON_ALLOWED_FS_READ=` and `AXON_ALLOWED_FS_WRITE=`
//! (empty: every Axon-level file read and write is refused, exit 8) and install
//! the COMPILED-IN substrate filter (`fs_none_filter`) after any policy
//! `seccomp_bpf_b64`: path-taking file syscalls fail with EACCES, the
//! interpreter's substrate is allowed, every other syscall and any non-x86_64
//! syscall ABI kills the process. It then prints `B263-FS none insns=<n>` on
//! the serial console, so the host can tell that the axis was applied. A `/1`
//! policy carrying `fs` is refused: an image that predates `/2` refuses the
//! schema, so no guest silently ignores the axis.

use std::ffi::CString;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
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
    /// Only the four ENFORCED mechanisms count (`fs` since guest-fs GF-3; its
    /// value is held to `"none"` by `check_fs_axis`). `principal`, `run_id` and
    /// `source_hash` are LABELS — the module header above is explicit that they
    /// "grant and withhold nothing" — so a payload carrying only labels is
    /// exactly as unpoliced as `{}` and must not be rescued by them.
    fn constrains_anything(&self) -> bool {
        self.allowed_effects.is_some()
            || self.budget_tokens.is_some()
            || self.seccomp_bpf_b64.is_some()
            || self.fs.is_some()
    }

    /// Does it carry any of the LABEL fields? Used only to tell a labels-only
    /// payload apart from an empty one in the refusal message — neither is a
    /// policy.
    fn has_labels(&self) -> bool {
        self.principal.is_some() || self.run_id.is_some() || self.source_hash.is_some()
    }
}

#[derive(Deserialize, Debug)]
struct MmdsPayload {
    /// `axon-vm-mmds/1` when written by `axon-vm`. REQUIRED on the cmdline
    /// channel (see `parse_cmdline_policy`); tolerated-if-absent on MMDS, whose
    /// historic payloads never carried it.
    schema: Option<String>,
    principal: Option<String>,
    allowed_effects: Option<Vec<String>>,
    budget_tokens: Option<u64>,
    source_hash: Option<String>,
    seccomp_bpf_b64: Option<String>,
    run_id: Option<String>,
    /// guest-fs GF-3: `"none"` withholds the filesystem (schema `/2` only;
    /// `check_fs_axis`). Absent = today's behaviour.
    fs: Option<String>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    #[cfg(feature = "host-fs-mirror")]
    if args.get(1).map(String::as_str) == Some("--host-fs-mirror") {
        host_fs_mirror(&args);
    }
    let (source, binary, exec_args) = match parse_args(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[axon-guest-init] {e}");
            process::exit(1);
        }
    };

    // 1. Re-seed entropy before any crypto-adjacent work.
    reseed_entropy();

    // 2. Read the policy, and REFUSE to exec if there isn't one.
    //
    //    This used to soft-fail: an unreachable metadata service logged one
    //    line that read like a note and handed `None` down, and `child_main`
    //    then skipped the whole policy block — no effect ceiling, no token cap,
    //    no seccomp — and exec'd the guest anyway.
    //
    //    The kernel cmdline is read FIRST and wins: the B263 profile has no NIC,
    //    so MMDS cannot work there at all, and a policy the host put on the
    //    cmdline must not be overridable by whatever answers on the network.
    let allow_unpoliced = allow_unpoliced();
    let loaded = load_policy(read_cmdline_policy(Path::new(CMDLINE_PATH)), read_mmds);
    let have_policy = loaded.is_ok();
    let policy = match policy_decision(have_policy, allow_unpoliced) {
        PolicyDecision::Apply => {
            let (p, source) = loaded.expect("have_policy implies Ok");
            eprintln!("[axon-guest-init] policy loaded from {}", source.describe());
            // A policy can be ACTIVE and still leave a mechanism off. Say which
            // one, per mechanism, rather than treating "a policy loaded" as a
            // blanket assurance — "policy applied" beside an absent seccomp
            // filter is the same overclaim in miniature as `{}` reading as a
            // policy at all.
            if p.allowed_effects.is_none() {
                eprintln!(
                    "[axon-guest-init] WARNING: policy loaded with NO effect ceiling \
                     (allowed_effects omitted) — the guest runs unrestricted on that axis"
                );
            }
            if p.seccomp_bpf_b64.is_none() && p.fs.is_none() {
                eprintln!(
                    "[axon-guest-init] WARNING: policy loaded with NO seccomp filter \
                     (seccomp_bpf_b64 omitted) — defence in depth is absent"
                );
            }
            if p.budget_tokens.is_none() {
                eprintln!(
                    "[axon-guest-init] WARNING: policy loaded with NO token cap \
                     (budget_tokens omitted) — AI spend is uncapped"
                );
            }
            Some(p)
        }
        PolicyDecision::ProceedUnpoliced => {
            eprintln!(
                // The variable's NAME is deliberately not spelled here: it must
                // appear in the binary only when the bypass is compiled in, so
                // `strings` on an image's init is a check (build-guest-image.sh
                // and tests/no_bypass_in_default_build.rs both make it).
                "[axon-guest-init] WARNING: the development no-policy BYPASS is \
                 active (dev-allow-no-policy build) — the guest is running with NO effect \
                 ceiling, NO token cap and NO seccomp filter ({}).",
                loaded.err().unwrap_or_default()
            );
            None
        }
        PolicyDecision::Refuse => {
            eprintln!(
                "[axon-guest-init] REFUSING to start the guest: {}. With no capability \
                 policy there would be no effect ceiling, no token cap and no seccomp \
                 filter.",
                loaded.err().unwrap_or_default()
            );
            process::exit(1);
        }
    };

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
        0 => child_main(
            policy,
            &binary,
            &exec_args,
            source.as_ref(),
            Path::new(SERIAL_CONSOLE),
        ),
        pid => supervisor_main(pid),
    }
}

/// guest-fs GF-3 corpus differential (`scripts/guest_fs_corpus.sh`), on the
/// HOST: run the guest child's exact path - load before gate, empty path
/// scopes, the compiled-in `fs:none` filter, exec - under the policy
/// `{"schema":"axon-vm-mmds/2","fs":"none"}`, with no fork, no supervisor and
/// the `B263-FS` report on stderr. `axon-guest-init --host-fs-mirror
/// --source-fd N=PATH -- <binary> [args...]`. Compiled only with the
/// non-default feature `host-fs-mirror`; no image carries it (it reads no
/// policy). `tests/no_bypass_in_default_build.rs` checks the default build.
#[cfg(feature = "host-fs-mirror")]
fn host_fs_mirror(args: &[String]) -> ! {
    let mut rest = vec![args[0].clone()];
    rest.extend_from_slice(&args[2..]);
    let (source, binary, exec_args) = match parse_args(&rest) {
        Ok((Some(s), b, a)) => (s, b, a),
        Ok((None, _, _)) => {
            eprintln!("[axon-guest-init] --host-fs-mirror needs --source-fd N=PATH");
            process::exit(1);
        }
        Err(e) => {
            eprintln!("[axon-guest-init] --host-fs-mirror argv: {e}");
            process::exit(1);
        }
    };
    let policy = parse_policy_json_strict(r#"{"schema":"axon-vm-mmds/2","fs":"none"}"#)
        .and_then(|p| check_fs_axis(&p).map(|()| p))
        .unwrap_or_else(|e| {
            eprintln!("[axon-guest-init] --host-fs-mirror policy: {e}");
            process::exit(1);
        });
    child_main(
        Some(policy),
        &binary,
        &exec_args,
        Some(&source),
        Path::new("/dev/stderr"),
    )
}

/// The program to open before the gate: `--source-fd N=PATH`.
#[derive(Debug, PartialEq, Eq)]
struct SourceFd {
    fd: libc::c_int,
    path: String,
}

/// argv → (program to load before the gate, binary, the binary's argv).
///
/// * `axon-guest-init <binary> [args...]`: today's form, no source fd.
/// * `axon-guest-init --source-fd N=PATH -- <binary> [args...]`: N >= 3 (fds
///   0-2 are the workload's own streams), PATH non-empty, `--` and a binary
///   required. The binary's argv is passed through untouched.
/// * bare: `--source-fd 3=/axon/program.ax -- /usr/bin/axon run --source-fd 3
///   /axon/program.ax`.
fn parse_args(args: &[String]) -> Result<(Option<SourceFd>, String, Vec<String>), String> {
    match args.get(1).map(String::as_str) {
        None => {
            let argv: Vec<String> = ["/usr/bin/axon", "run", "--source-fd", "3"]
                .iter()
                .map(|s| s.to_string())
                .chain(["/axon/program.ax".to_string()])
                .collect();
            let source = SourceFd {
                fd: 3,
                path: argv[4].clone(),
            };
            return Ok((Some(source), argv[0].clone(), argv));
        }
        Some("--source-fd") => {}
        Some(binary) => return Ok((None, binary.to_string(), args[1..].to_vec())),
    }
    // `--source-fd N=PATH -- <binary> ...`: all four parts, PATH non-empty.
    let source = match (
        args.get(2).and_then(|s| s.split_once('=')),
        args.get(3).map(String::as_str),
        args.get(4),
    ) {
        (Some((n, path)), Some("--"), Some(_)) if !path.is_empty() => {
            n.parse::<libc::c_int>().ok().map(|fd| SourceFd {
                fd,
                path: path.to_string(),
            })
        }
        _ => None,
    };
    match source {
        Some(s) if s.fd >= 3 => Ok((Some(s), args[4].clone(), args[4..].to_vec())),
        _ => Err(format!(
            "usage: axon-guest-init --source-fd N=PATH -- <binary> [args...] \
             (N >= 3: stdin, stdout and stderr cannot carry the program): {:?}",
            &args[1..]
        )),
    }
}

/// Open `path` and leave it on exactly fd `n`, inheritable across `execve`
/// (guest-fs GF-2). Run in the forked child BEFORE any seccomp filter: this is
/// the workload's only open of its program, and it happens outside the gate.
///
/// `O_NOFOLLOW` (a symlink is refused, not followed), a regular file only.
/// std opens with `O_CLOEXEC` and the fd is moved to `n` with
/// `dup3(.., O_CLOEXEC)`, so no fd but `n` can leak into the workload;
/// `FD_CLOEXEC` is then cleared on `n` alone. Returns the fd `open` returned
/// (already closed unless it was `n`).
fn open_source_before_gate(n: libc::c_int, path: &str) -> Result<libc::c_int, String> {
    use std::os::fd::IntoRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| e.to_string())?;
    let regular = f.metadata().map_err(|e| e.to_string())?.is_file();
    if !regular {
        return Err("not a regular file".to_string());
    }
    let fd = f.into_raw_fd();
    // SAFETY: dup3 / close / fcntl on fds this fn owns.
    unsafe {
        if fd != n {
            let moved = libc::dup3(fd, n, libc::O_CLOEXEC);
            let e = std::io::Error::last_os_error();
            libc::close(fd);
            if moved != n {
                return Err(format!("dup3 to fd {n}: {e}"));
            }
        }
        let flags = libc::fcntl(n, libc::F_GETFD);
        if flags < 0 || libc::fcntl(n, libc::F_SETFD, flags & !libc::FD_CLOEXEC) != 0 {
            return Err(format!(
                "make fd {n} inheritable: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(fd)
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

/// Is the development bypass compiled in AND asked for?
///
/// In a default build this is the constant `false`: the env var is not even
/// read. Linux passes unrecognised `NAME=value` cmdline words into init's
/// environment, so a runtime flag would be reachable by anyone who can append
/// one word to the kernel cmdline. The bypass therefore exists only behind the
/// non-default cargo feature `dev-allow-no-policy`.
#[cfg(feature = "dev-allow-no-policy")]
fn allow_unpoliced() -> bool {
    env::var("AXON_GUEST_ALLOW_NO_POLICY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[cfg(not(feature = "dev-allow-no-policy"))]
fn allow_unpoliced() -> bool {
    false
}

// ── Kernel-cmdline policy channel ─────────────────────────────────────────────

const CMDLINE_PATH: &str = "/proc/cmdline";
/// The one cmdline word that carries the policy.
const CMDLINE_POLICY_KEY: &str = "axon.policy=";
/// The schema the cmdline channel accepts for a policy without the `fs` axis.
const POLICY_SCHEMA: &str = "axon-vm-mmds/1";
/// guest-fs GF-3: `/1` plus the `fs` axis. A policy carrying `fs` must say it,
/// so an image that predates the axis refuses the schema instead of ignoring it.
const POLICY_SCHEMA_FS: &str = "axon-vm-mmds/2";
/// x86 `COMMAND_LINE_SIZE`. The kernel keeps at most `COMMAND_LINE_SIZE - 1`
/// bytes and silently TRUNCATES the rest, so a cmdline that reaches that
/// length may have lost the tail of the policy. Refuse rather than guess; the
/// host must keep the whole cmdline (policy word included) at or below
/// `CMDLINE_MAX_SAFE` bytes.
const X86_COMMAND_LINE_SIZE: usize = 2048;
const CMDLINE_MAX_SAFE: usize = X86_COMMAND_LINE_SIZE - 2;

/// Why a cmdline policy was refused. Each variant is a DISTINCT failure so
/// the boot log says which one — a launcher bug and a truncation need
/// different remedies.
#[derive(Debug, PartialEq, Eq)]
enum CmdlinePolicyError {
    /// `/proc/cmdline` could not be read, so whether the host sent a policy
    /// is unknown.
    Unreadable(String),
    /// The cmdline is long enough that the kernel may have truncated it.
    PossiblyTruncated(usize),
    /// `axon.policy=` appears more than once — which one the host meant is
    /// ambiguous, so neither is used.
    RepeatedWord,
    /// `axon.policy=` with nothing after it.
    EmptyValue,
    /// The value is not standard padded base64.
    BadBase64(String),
    /// The decoded bytes are not a JSON object of the payload schema.
    BadJson(String),
    /// A JSON object key appears twice (compared as DECODED, so `"a"` and
    /// `"\u0061"` are the same key). serde_json would silently keep the last.
    DuplicateKey(String),
    /// Parsed, but constrains nothing and carries nothing (`{}`).
    ConstrainsNothing,
    /// Parsed, carries labels (principal/run_id/source_hash) but no enforced
    /// mechanism. Labels grant and withhold nothing.
    LabelsOnly,
    /// `schema` absent or neither `axon-vm-mmds/1` nor `axon-vm-mmds/2`.
    WrongSchema(Option<String>),
    /// guest-fs GF-3: an `fs` key under a schema other than `axon-vm-mmds/2`.
    FsNeedsSchemaV2(Option<String>),
    /// guest-fs GF-3: an `fs` value other than `"none"`.
    BadFsValue(String),
}

impl std::fmt::Display for CmdlinePolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use CmdlinePolicyError::*;
        match self {
            Unreadable(e) => write!(f, "cmdline policy UNREADABLE: {CMDLINE_PATH}: {e}"),
            PossiblyTruncated(n) => write!(
                f,
                "cmdline policy POSSIBLY TRUNCATED: the kernel cmdline is {n} bytes, at or \
                 over the {CMDLINE_MAX_SAFE}-byte safe limit (x86 COMMAND_LINE_SIZE \
                 {X86_COMMAND_LINE_SIZE}); the kernel drops the tail silently"
            ),
            RepeatedWord => write!(
                f,
                "cmdline policy AMBIGUOUS: `axon.policy=` appears more than once"
            ),
            EmptyValue => write!(f, "cmdline policy EMPTY: `axon.policy=` carries no value"),
            BadBase64(e) => write!(f, "cmdline policy MALFORMED BASE64: {e}"),
            BadJson(e) => write!(f, "cmdline policy MALFORMED JSON: {e}"),
            DuplicateKey(k) => write!(f, "cmdline policy has DUPLICATE KEY `{k}`"),
            ConstrainsNothing => write!(
                f,
                "cmdline policy CONSTRAINS NOTHING: no allowed_effects, budget_tokens, \
                 seccomp_bpf_b64 or fs"
            ),
            LabelsOnly => write!(
                f,
                "cmdline policy is LABELS ONLY: principal/run_id/source_hash grant and \
                 withhold nothing, and no enforced field is present"
            ),
            WrongSchema(s) => write!(
                f,
                "cmdline policy has WRONG SCHEMA: expected `{POLICY_SCHEMA}` or \
                 `{POLICY_SCHEMA_FS}`, got {s:?}"
            ),
            FsNeedsSchemaV2(s) => write!(
                f,
                "policy carries the `fs` axis under schema {s:?}: `fs` is valid only \
                 under `{POLICY_SCHEMA_FS}`"
            ),
            BadFsValue(v) => write!(
                f,
                "policy `fs` is {v:?}: the only value is \"none\" (path scope is not \
                 projected into the guest)"
            ),
        }
    }
}

/// Read and parse the cmdline policy from `path` (`/proc/cmdline` in the
/// guest; injectable so tests do not depend on the host's cmdline).
///
/// `Ok(None)` means the cmdline carries NO `axon.policy=` word at all — the
/// only case in which a second channel may be consulted.
fn read_cmdline_policy(path: &Path) -> Result<Option<MmdsPayload>, CmdlinePolicyError> {
    let raw = std::fs::read(path).map_err(|e| CmdlinePolicyError::Unreadable(e.to_string()))?;
    let text = String::from_utf8(raw)
        .map_err(|e| CmdlinePolicyError::Unreadable(format!("not UTF-8: {e}")))?;
    parse_cmdline_policy(&text)
}

/// Parse a kernel cmdline. Every whitespace-separated word is scanned,
/// including those after `--` (axon-vm appends the policy word after the init
/// argv separator; `/proc/cmdline` shows the whole line either way).
fn parse_cmdline_policy(cmdline: &str) -> Result<Option<MmdsPayload>, CmdlinePolicyError> {
    let cmdline = cmdline.trim_end_matches('\n');
    if cmdline.len() > CMDLINE_MAX_SAFE {
        return Err(CmdlinePolicyError::PossiblyTruncated(cmdline.len()));
    }
    let mut values = cmdline
        .split_ascii_whitespace()
        .filter_map(|w| w.strip_prefix(CMDLINE_POLICY_KEY));
    let Some(b64) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(CmdlinePolicyError::RepeatedWord);
    }
    if b64.is_empty() {
        return Err(CmdlinePolicyError::EmptyValue);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| CmdlinePolicyError::BadBase64(e.to_string()))?;
    let json =
        std::str::from_utf8(&bytes).map_err(|e| CmdlinePolicyError::BadJson(e.to_string()))?;
    let payload = parse_policy_json_strict(json)?;
    if !payload.constrains_anything() {
        return Err(if payload.has_labels() {
            CmdlinePolicyError::LabelsOnly
        } else {
            CmdlinePolicyError::ConstrainsNothing
        });
    }
    if payload.schema.as_deref() == Some(POLICY_SCHEMA_FS) {
        check_fs_axis(&payload)?;
        return Ok(Some(payload));
    }
    if payload.schema.as_deref() != Some(POLICY_SCHEMA) {
        return Err(CmdlinePolicyError::WrongSchema(payload.schema));
    }
    check_fs_axis(&payload)?;
    Ok(Some(payload))
}

/// guest-fs GF-3: the `fs` axis is valid only under `axon-vm-mmds/2`, and its
/// only value is `"none"`. Both channels hold a payload to this.
fn check_fs_axis(p: &MmdsPayload) -> Result<(), CmdlinePolicyError> {
    let Some(v) = &p.fs else {
        return Ok(());
    };
    if p.schema.as_deref() != Some(POLICY_SCHEMA_FS) {
        return Err(CmdlinePolicyError::FsNeedsSchemaV2(p.schema.clone()));
    }
    if v != "none" {
        return Err(CmdlinePolicyError::BadFsValue(v.clone()));
    }
    Ok(())
}

/// Parse the payload JSON refusing duplicate object keys at any depth.
///
/// The same approach as `axon_cortex::parse_strict` (not depended on: this is
/// a static PID-1 binary and pulls in nothing it does not need): keys are
/// compared as the real parser DECODES them, so an escaped spelling of a key
/// is still the same key.
fn parse_policy_json_strict(json: &str) -> Result<MmdsPayload, CmdlinePolicyError> {
    let StrictValue(v) = serde_json::from_str(json).map_err(|e| {
        let m = e.to_string();
        match m.strip_prefix("duplicate key: ") {
            Some(k) => CmdlinePolicyError::DuplicateKey(
                k.split(" at line").next().unwrap_or(k).to_string(),
            ),
            None => CmdlinePolicyError::BadJson(m),
        }
    })?;
    if !v.is_object() {
        return Err(CmdlinePolicyError::BadJson(
            "top-level value is not a JSON object".to_string(),
        ));
    }
    serde_json::from_value(v).map_err(|e| CmdlinePolicyError::BadJson(e.to_string()))
}

/// A JSON value whose deserialization fails on a repeated object key.
struct StrictValue(serde_json::Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = serde_json::Value;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("any JSON value, with no repeated object key")
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(serde_json::Value::Null)
            }
            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
                Ok(v.into())
            }
            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
                Ok(v.into())
            }
            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
                Ok(v.into())
            }
            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
                Ok(serde_json::Number::from_f64(v)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null))
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
                Ok(v.into())
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some(StrictValue(v)) = a.next_element()? {
                    out.push(v);
                }
                Ok(serde_json::Value::Array(out))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    let StrictValue(v) = a.next_value()?;
                    if out.contains_key(&k) {
                        return Err(serde::de::Error::custom(format!("duplicate key: {k}")));
                    }
                    out.insert(k, v);
                }
                Ok(serde_json::Value::Object(out))
            }
        }
        d.deserialize_any(V).map(StrictValue)
    }
}

/// Which channel supplied the policy.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum PolicySource {
    Cmdline,
    Mmds,
}

impl PolicySource {
    fn describe(self) -> &'static str {
        match self {
            PolicySource::Cmdline => "the kernel cmdline (axon.policy=)",
            PolicySource::Mmds => "MMDS",
        }
    }
}

/// Combine the two channels. The cmdline WINS: if it carries a policy word,
/// its verdict (a policy, or a refusal) is final and `mmds` is never called.
/// Only a cmdline with no `axon.policy=` word at all falls back to MMDS.
/// Every `Err` is a reason to refuse, worded for the boot log.
fn load_policy(
    cmdline: Result<Option<MmdsPayload>, CmdlinePolicyError>,
    mmds: impl FnOnce() -> Result<Option<MmdsPayload>, String>,
) -> Result<(MmdsPayload, PolicySource), String> {
    match cmdline {
        Err(e) => Err(e.to_string()),
        Ok(Some(p)) => Ok((p, PolicySource::Cmdline)),
        Ok(None) => match mmds() {
            // CONTENT, not presence. A body that parsed but constrains nothing
            // is not a policy — see `MmdsPayload::constrains_anything`.
            // guest-fs GF-3: the `fs` axis is held to the same rule as on the
            // cmdline (schema `/2`, value `"none"`).
            Ok(Some(p)) if p.constrains_anything() => match check_fs_axis(&p) {
                Ok(()) => Ok((p, PolicySource::Mmds)),
                Err(e) => Err(format!("MMDS {e}")),
            },
            Ok(Some(_)) => Err(
                "policy ABSENT: no `axon.policy=` on the kernel cmdline, and \
                                the MMDS payload constrains nothing"
                    .to_string(),
            ),
            Ok(None) => Err(
                "policy ABSENT: no `axon.policy=` on the kernel cmdline, and \
                             MMDS returned an empty body"
                    .to_string(),
            ),
            Err(e) => Err(format!(
                "policy ABSENT: no `axon.policy=` on the kernel cmdline, and MMDS \
                 failed ({e})"
            )),
        },
    }
}

// ── Child: apply policy then exec ─────────────────────────────────────────────

fn child_main(
    policy: Option<MmdsPayload>,
    binary: &str,
    exec_args: &[String],
    source: Option<&SourceFd>,
    fs_report: &Path,
) -> ! {
    // guest-fs GF-2: load before gate. The program is opened here, before the
    // env export and before `apply_seccomp`, so the workload's first file
    // syscall is never its own source.
    if let Some(s) = source {
        if let Err(e) = open_source_before_gate(s.fd, &s.path) {
            eprintln!("[axon-guest-init] cannot open program {}: {e}", s.path);
            process::exit(1);
        }
    }
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
        // guest-fs GF-3: `"fs": "none"` (check_fs_axis admitted no other
        // value). The path scopes are emptied here, and the report stream (the
        // serial console in the guest) is opened now, before any filter, to
        // report the axis once it is on.
        let fs_console = if p.fs.is_some() {
            export_fs_none_env();
            match open_console(fs_report) {
                Ok(c) => Some(c),
                Err(e) => {
                    eprintln!(
                        "[axon-guest-init] cannot open {} for the fs axis: {e}",
                        fs_report.display()
                    );
                    process::exit(1);
                }
            }
        } else {
            None
        };

        // Apply seccomp AFTER setting env vars (set_var would be blocked after).
        if let Some(bpf_b64) = &p.seccomp_bpf_b64 {
            if let Err(e) = apply_seccomp(bpf_b64) {
                eprintln!("[axon-guest-init] seccomp apply failed: {e}");
                process::exit(1);
            }
        }
        // The fs filter goes on LAST: it withholds `prctl`, so no filter can
        // be installed after it. Stacked filters take the strictest result.
        if let Some(console) = fs_console {
            if let Err(e) = apply_fs_none(console) {
                eprintln!("[axon-guest-init] fs axis apply failed: {e}");
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
    install_bpf(&bpf_bytes, n_insns)?;
    eprintln!("[axon-guest-init] seccomp applied ({n_insns} instructions)");
    Ok(())
}

/// Install `n_insns` sock_filter instructions from `bpf_bytes` on the calling
/// thread, after setting no-new-privs. Shared by the policy's own program
/// (`apply_seccomp`) and the compiled-in `fs` axis filter (`apply_fs_none`).
fn install_bpf(bpf_bytes: &[u8], n_insns: u16) -> Result<(), String> {
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
    Ok(())
}

// ── guest-fs GF-3: the `fs` axis ──────────────────────────────────────────────

/// The serial console, where `B263-FS` is reported for the host to bind.
const SERIAL_CONSOLE: &str = "/dev/console";

/// x86_64 syscalls the interpreter substrate needs under `"fs": "none"`:
/// memory, threads, signals, fd-relative I/O, time, `getrandom`, and `execve`
/// (the filter is installed BEFORE this child execs the workload). Not here,
/// so KILLED: `prctl` (no further filter can be installed or relaxed), every
/// socket call, `ptrace`, `mount`, ... Numbers, not names: the arch check
/// pins the table to x86_64 whatever target this binary was built for.
const FS_NONE_ALLOW: [u32; 41] = [
    0,   // read
    1,   // write
    3,   // close
    5,   // fstat
    7,   // poll
    8,   // lseek
    9,   // mmap
    10,  // mprotect
    11,  // munmap
    12,  // brk
    13,  // rt_sigaction
    14,  // rt_sigprocmask
    15,  // rt_sigreturn
    16,  // ioctl
    17,  // pread64
    19,  // readv
    20,  // writev
    24,  // sched_yield
    25,  // mremap
    28,  // madvise
    35,  // nanosleep
    39,  // getpid
    56,  // clone
    59,  // execve
    60,  // exit
    72,  // fcntl
    131, // sigaltstack
    158, // arch_prctl
    186, // gettid
    202, // futex
    204, // sched_getaffinity
    218, // set_tid_address
    228, // clock_gettime
    230, // clock_nanosleep
    231, // exit_group
    234, // tgkill
    273, // set_robust_list
    302, // prlimit64
    318, // getrandom
    324, // membarrier
    334, // rseq
];

/// x86_64 path-taking filesystem syscalls: refused with EACCES (not killed),
/// so the interpreter's own probes fail like a denied open and an Axon-level
/// read reaches the runtime's refusal (exit 8) instead of a SIGSYS.
const FS_NONE_EACCES: [u32; 28] = [
    2,   // open (musl opens with `open`)
    4,   // stat
    6,   // lstat
    21,  // access
    79,  // getcwd
    80,  // chdir
    82,  // rename
    83,  // mkdir
    84,  // rmdir
    85,  // creat
    86,  // link
    87,  // unlink
    88,  // symlink
    89,  // readlink
    90,  // chmod
    92,  // chown
    217, // getdents64
    257, // openat
    258, // mkdirat
    262, // newfstatat
    263, // unlinkat
    264, // renameat
    267, // readlinkat
    269, // faccessat
    316, // renameat2
    332, // statx
    437, // openat2
    439, // faccessat2
];

const AUDIT_ARCH_X86_64: u32 = 0xC000_003E;
const SECCOMP_RET_ALLOW: u32 = 0x7FFF_0000;
const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
const SECCOMP_RET_ERRNO_EACCES: u32 = 0x0005_0000 | libc::EACCES as u32;
const BPF_LD_W_ABS: u16 = 0x20;
const BPF_JEQ_K: u16 = 0x15;
const BPF_RET_K: u16 = 0x06;

fn bpf_ins(out: &mut Vec<u8>, code: u16, jt: u8, jf: u8, k: u32) {
    out.extend_from_slice(&code.to_le_bytes());
    out.push(jt);
    out.push(jf);
    out.extend_from_slice(&k.to_le_bytes());
}

/// The compiled-in `"fs": "none"` filter. Layout: arch check (any ABI but
/// x86_64 is KILLED), load the syscall number, one JEQ per ALLOW and per
/// EACCES entry, then RET KILL (default), RET EACCES, RET ALLOW. An x32
/// number (bit 30 set) matches no entry and is KILLED by the default.
fn fs_none_filter() -> Vec<u8> {
    let (n_a, n_f) = (FS_NONE_ALLOW.len(), FS_NONE_EACCES.len());
    let mut p = Vec::with_capacity((n_a + n_f + 7) * 8);
    bpf_ins(&mut p, BPF_LD_W_ABS, 0, 0, 4); // seccomp_data.arch
    bpf_ins(&mut p, BPF_JEQ_K, 1, 0, AUDIT_ARCH_X86_64);
    bpf_ins(&mut p, BPF_RET_K, 0, 0, SECCOMP_RET_KILL_PROCESS);
    bpf_ins(&mut p, BPF_LD_W_ABS, 0, 0, 0); // seccomp_data.nr
    for (i, nr) in FS_NONE_ALLOW.iter().enumerate() {
        let to_allow = (n_a - 1 - i) + n_f + 2;
        bpf_ins(&mut p, BPF_JEQ_K, to_allow as u8, 0, *nr);
    }
    for (i, nr) in FS_NONE_EACCES.iter().enumerate() {
        let to_eacces = (n_f - 1 - i) + 1;
        bpf_ins(&mut p, BPF_JEQ_K, to_eacces as u8, 0, *nr);
    }
    bpf_ins(&mut p, BPF_RET_K, 0, 0, SECCOMP_RET_KILL_PROCESS);
    bpf_ins(&mut p, BPF_RET_K, 0, 0, SECCOMP_RET_ERRNO_EACCES);
    bpf_ins(&mut p, BPF_RET_K, 0, 0, SECCOMP_RET_ALLOW);
    p
}

/// `"fs": "none"` (a): empty Axon-level path scopes. Empty means every
/// interpreter read and write is refused (exit 8) before any syscall is made.
fn export_fs_none_env() {
    env::set_var("AXON_ALLOWED_FS_READ", "");
    env::set_var("AXON_ALLOWED_FS_WRITE", "");
}

/// Open the serial console for the `B263-FS` report. Close-on-exec (std's
/// default), so the workload never inherits it.
fn open_console(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open(path)
}

/// `"fs": "none"` (b) + (c): install the compiled-in filter on top of any the
/// policy carried (stacked filters take the strictest result), then report
/// `B263-FS none insns=<n>` on `report` - a `write` on an fd opened before the
/// filter, which the filter allows.
fn apply_fs_none(mut report: std::fs::File) -> Result<(), String> {
    let filter = fs_none_filter();
    let n_insns = (filter.len() / 8) as u16;
    install_bpf(&filter, n_insns)?;
    writeln!(report, "B263-FS none insns={n_insns}")
        .map_err(|e| format!("cannot report B263-FS: {e}"))?;
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
    use super::{
        allow_unpoliced, load_policy, parse_cmdline_policy, policy_decision, read_cmdline_policy,
        CmdlinePolicyError, MmdsPayload, PolicyDecision, PolicySource, CMDLINE_MAX_SAFE,
    };
    use base64::Engine as _;
    use std::path::PathBuf;

    const VALID: &str = r#"{"schema":"axon-vm-mmds/1","run_id":"r1","principal":"alice","allowed_effects":["IO"],"budget_tokens":100,"source_hash":null,"seccomp_bpf_b64":null}"#;

    fn b64(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
    }

    /// The exact shape axon-vm's launcher boots with.
    fn cmdline_with(policy_word: &str) -> String {
        format!(
            "console=ttyS0 reboot=k panic=1 pci=off nomodules init=/init -- /init \
             /usr/bin/axon run /axon/program.ax {policy_word}\n"
        )
    }

    fn write_tmp(name: &str, body: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "axon-guest-init-test-{}-{name}",
            std::process::id()
        ));
        std::fs::write(&p, body).unwrap();
        p
    }

    fn mmds_must_not_be_called() -> Result<Option<MmdsPayload>, String> {
        panic!("MMDS consulted although the cmdline carried a policy word")
    }

    fn cmdline_err(policy_json: &str) -> CmdlinePolicyError {
        parse_cmdline_policy(&cmdline_with(&format!("axon.policy={}", b64(policy_json))))
            .expect_err("must refuse")
    }

    #[test]
    fn a_valid_cmdline_policy_is_read_from_the_file_and_used() {
        let path = write_tmp(
            "valid",
            &cmdline_with(&format!("axon.policy={}", b64(VALID))),
        );
        let got = read_cmdline_policy(&path).expect("valid").expect("present");
        std::fs::remove_file(&path).ok();
        assert_eq!(got.allowed_effects, Some(vec!["IO".to_string()]));
        assert_eq!(got.budget_tokens, Some(100));
        let (p, src) = load_policy(Ok(Some(got)), mmds_must_not_be_called).expect("applies");
        assert_eq!(src, PolicySource::Cmdline);
        assert!(p.constrains_anything());
    }

    /// The cmdline wins: a present cmdline policy means MMDS is never asked,
    /// and a BAD cmdline policy is a refusal, not a fall-through to MMDS.
    #[test]
    fn the_cmdline_policy_beats_mmds() {
        let cmd = parse_cmdline_policy(&cmdline_with(&format!("axon.policy={}", b64(VALID))));
        let (p, src) = load_policy(cmd, || {
            Ok(Some(
                serde_json::from_str(r#"{"allowed_effects":["IO","Net","Exec"]}"#).unwrap(),
            ))
        })
        .unwrap();
        assert_eq!(src, PolicySource::Cmdline);
        assert_eq!(p.allowed_effects, Some(vec!["IO".to_string()]));

        let bad = parse_cmdline_policy(&cmdline_with("axon.policy=!!!"));
        let e = load_policy(bad, mmds_must_not_be_called).unwrap_err();
        assert!(e.contains("MALFORMED BASE64"), "{e}");
    }

    /// No word at all is the only case that falls back; with MMDS also empty
    /// or unreachable the result is a refusal naming ABSENT.
    #[test]
    fn an_absent_cmdline_policy_with_no_mmds_refuses() {
        let path = write_tmp("absent", &cmdline_with(""));
        let cmd = read_cmdline_policy(&path);
        std::fs::remove_file(&path).ok();
        assert!(matches!(cmd, Ok(None)));
        for mmds in [
            Err("connect 169.254.169.254:80: Network is unreachable".to_string()),
            Ok(None),
            Ok(Some(serde_json::from_str::<MmdsPayload>("{}").unwrap())),
        ] {
            let e = load_policy(parse_cmdline_policy(&cmdline_with("")), || mmds).unwrap_err();
            assert!(e.contains("ABSENT"), "{e}");
            assert_eq!(
                policy_decision(false, allow_unpoliced()),
                PolicyDecision::Refuse
            );
        }
    }

    #[test]
    fn an_unreadable_cmdline_refuses_rather_than_falling_back() {
        let e = read_cmdline_policy(std::path::Path::new("/nonexistent/axon/cmdline"))
            .expect_err("unreadable is not absent");
        assert!(matches!(e, CmdlinePolicyError::Unreadable(_)));
        assert!(load_policy(Err(e), mmds_must_not_be_called).is_err());
    }

    #[test]
    fn an_empty_object_is_refused_as_constraining_nothing() {
        assert_eq!(cmdline_err("{}"), CmdlinePolicyError::ConstrainsNothing);
        assert_eq!(
            cmdline_err(r#"{"schema":"axon-vm-mmds/1","allowed_effects":null}"#),
            CmdlinePolicyError::ConstrainsNothing
        );
    }

    #[test]
    fn a_labels_only_cmdline_policy_is_refused_as_labels_only() {
        assert_eq!(
            cmdline_err(
                r#"{"schema":"axon-vm-mmds/1","run_id":"r1","principal":"alice","source_hash":"abc"}"#
            ),
            CmdlinePolicyError::LabelsOnly
        );
    }

    #[test]
    fn malformed_base64_is_refused_as_malformed_base64() {
        for word in ["axon.policy=!!!", "axon.policy=e30=e30=", "axon.policy=e30"] {
            let e = parse_cmdline_policy(&cmdline_with(word)).expect_err(word);
            assert!(
                matches!(e, CmdlinePolicyError::BadBase64(_)),
                "{word}: {e:?}"
            );
        }
        assert_eq!(
            parse_cmdline_policy(&cmdline_with("axon.policy=")).unwrap_err(),
            CmdlinePolicyError::EmptyValue
        );
    }

    #[test]
    fn malformed_json_is_refused_as_malformed_json() {
        for body in ["{not json", "[1,2]", "\"x\"", r#"{"budget_tokens":"lots"}"#] {
            let e = cmdline_err(body);
            assert!(matches!(e, CmdlinePolicyError::BadJson(_)), "{body}: {e:?}");
        }
    }

    /// serde_json keeps the LAST of two equal keys. A policy whose first
    /// `allowed_effects` a reviewer reads and whose second the guest applies
    /// is refused — including when the repeat is spelled with an escape.
    #[test]
    fn a_duplicate_key_is_refused_as_a_duplicate_key() {
        for body in [
            r#"{"schema":"axon-vm-mmds/1","allowed_effects":[],"allowed_effects":["Exec"]}"#,
            r#"{"schema":"axon-vm-mmds/1","allowed_effects":[],"allowed_effect\u0073":["Exec"]}"#,
        ] {
            assert_eq!(
                cmdline_err(body),
                CmdlinePolicyError::DuplicateKey("allowed_effects".to_string()),
                "{body}"
            );
        }
    }

    #[test]
    fn a_repeated_policy_word_is_refused() {
        let w = format!("axon.policy={} axon.policy={}", b64(VALID), b64(VALID));
        assert_eq!(
            parse_cmdline_policy(&cmdline_with(&w)).unwrap_err(),
            CmdlinePolicyError::RepeatedWord
        );
    }

    #[test]
    fn a_missing_or_wrong_schema_is_refused() {
        assert_eq!(
            cmdline_err(r#"{"allowed_effects":["IO"]}"#),
            CmdlinePolicyError::WrongSchema(None)
        );
        assert_eq!(
            cmdline_err(r#"{"schema":"axon-vm-mmds/3","allowed_effects":["IO"]}"#),
            CmdlinePolicyError::WrongSchema(Some("axon-vm-mmds/3".to_string()))
        );
    }

    /// The kernel silently drops cmdline bytes past COMMAND_LINE_SIZE-1. A
    /// cmdline that reaches the safe limit is refused even when the tail still
    /// happens to parse; one just under it is accepted.
    #[test]
    fn a_cmdline_at_the_x86_limit_is_refused_as_possibly_truncated() {
        let word = format!("axon.policy={}", b64(VALID));
        let base = cmdline_with(&word);
        let base = base.trim_end();
        let pad_to = |n: usize| format!("x{} {base}", "x".repeat(n - base.len() - 2));
        let at = pad_to(CMDLINE_MAX_SAFE + 1);
        assert_eq!(at.len(), CMDLINE_MAX_SAFE + 1);
        assert!(matches!(
            parse_cmdline_policy(&at),
            Err(CmdlinePolicyError::PossiblyTruncated(_))
        ));
        let under = pad_to(CMDLINE_MAX_SAFE);
        assert!(parse_cmdline_policy(&under).unwrap().is_some());
    }

    /// Every refusal above is DISTINCT: the boot log names which one.
    #[test]
    fn the_refusals_are_distinct_messages() {
        let msgs: std::collections::HashSet<String> = [
            cmdline_err("{}"),
            cmdline_err(r#"{"principal":"a"}"#),
            parse_cmdline_policy(&cmdline_with("axon.policy=!!!")).unwrap_err(),
            cmdline_err("{not json"),
            cmdline_err(r#"{"budget_tokens":1,"budget_tokens":2}"#),
        ]
        .iter()
        .map(|e| e.to_string().split(':').next().unwrap().to_string())
        .collect();
        assert_eq!(msgs.len(), 5, "{msgs:?}");
    }

    /// A default build carries no bypass: the env var is not even read.
    ///
    /// Deliberately NOT `cfg`-gated on the feature: if `dev-allow-no-policy`
    /// is ever made a default (or enabled in the build under test) this must
    /// go RED, not silently compile out. A `--features dev-allow-no-policy`
    /// test run therefore fails here by design — that build IS the bypass.
    #[test]
    fn the_default_build_has_no_runtime_bypass() {
        std::env::set_var("AXON_GUEST_ALLOW_NO_POLICY", "1");
        let allowed = allow_unpoliced();
        std::env::remove_var("AXON_GUEST_ALLOW_NO_POLICY");
        assert!(
            !allowed,
            "a default build must not honour AXON_GUEST_ALLOW_NO_POLICY"
        );
    }

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

    // ── C9 round 7, EQGATE3 (amendment 91): the guest's process hardening ────
    //
    // `apply_seccomp` and the PID-1 supervisor are libc calls that build no
    // `Err` the gate could see; each was removable alone with every suite green
    // (no host test ran them). They run here in a FORKED child, so the test
    // process itself is never made no-new-privs, filtered or a supervisor, and
    // the parent bounds the child with a wall clock.

    /// C9 round 7 (eqgate3): run `test` again ALONE in a child of this test
    /// binary and say whether THIS process is that child. The tests below FORK,
    /// and a fork of a multi-threaded test process can deadlock on an allocator
    /// lock another test thread held; in a process that runs only this test the
    /// other thread is idle. (`harden`'s test in axon-fabric hung a suite run
    /// that way.) Usage: `if !alone("tests::name") { return; }` first.
    fn alone(test: &str) -> bool {
        if std::env::var("AXON_EQ_ALONE").as_deref() == Ok(test) {
            return true;
        }
        let o = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--test-threads=1", "--nocapture"])
            .env("AXON_EQ_ALONE", test)
            .output()
            .unwrap();
        assert!(
            o.status.success() && String::from_utf8_lossy(&o.stdout).contains("1 passed"),
            "{test} failed when run alone:\n{}\n{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        false
    }

    /// Run `body` in a forked child and return its exit status, or None when it
    /// did not finish in `secs` (it is then killed).
    fn forked(secs: u64, body: impl FnOnce() -> i32) -> Option<libc::c_int> {
        // SAFETY: fork; the child runs `body` and `_exit`s.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            let code = body();
            unsafe { libc::_exit(code) };
        }
        assert!(pid > 0, "setup: fork");
        let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        loop {
            let mut status = 0;
            // SAFETY: waitpid on our own child.
            let r = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if r == pid {
                return Some(status);
            }
            if std::time::Instant::now() >= end {
                unsafe { libc::kill(pid, libc::SIGKILL) };
                unsafe { libc::waitpid(pid, &mut status, 0) };
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_seccomp_filter_is_installed_with_no_new_privs() {
        if !alone("tests::a_seccomp_filter_is_installed_with_no_new_privs") {
            return;
        }
        // One instruction: BPF_RET | BPF_K, SECCOMP_RET_ALLOW.
        let allow_all =
            base64::engine::general_purpose::STANDARD.encode([0x06u8, 0, 0, 0, 0, 0, 0xff, 0x7f]);
        let st = forked(10, || unsafe {
            if super::apply_seccomp(&allow_all).is_err() {
                return 30;
            }
            if libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) != 1 {
                return 31;
            }
            if libc::prctl(libc::PR_GET_SECCOMP, 0, 0, 0, 0) != 2 {
                return 32;
            }
            0
        })
        .expect("setup: the child finished");
        assert!(libc::WIFEXITED(st), "setup: the child exited normally");
        match libc::WEXITSTATUS(st) {
            0 => {}
            30 => panic!("setup: apply_seccomp refused an allow-all filter"),
            31 => panic!(
                "ATTACK: apply_seccomp left the process able to gain privileges (PR_GET_NO_NEW_PRIVS != 1)"
            ),
            32 => panic!(
                "ATTACK: apply_seccomp installed no filter (PR_GET_SECCOMP != 2, filter mode)"
            ),
            c => panic!("setup: unexpected child status {c}"),
        }
    }

    #[test]
    fn the_supervisor_forwards_term_and_int_to_its_child() {
        if !alone("tests::the_supervisor_forwards_term_and_int_to_its_child") {
            return;
        }
        for sig in [libc::SIGTERM, libc::SIGINT] {
            let sleep = std::ffi::CString::new("sleep").unwrap();
            let arg = std::ffi::CString::new("30").unwrap();
            let argv = [sleep.as_ptr(), arg.as_ptr(), std::ptr::null()];
            // The supervisor, as a forked child of the test, supervising a
            // `sleep` grandchild of its own. Its pid is shared through a pipe.
            let mut fds = [0i32; 2];
            // SAFETY: pipe into a two-int array.
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "setup: pipe");
            // SAFETY: fork; the child never returns.
            let sup = unsafe { libc::fork() };
            if sup == 0 {
                // SAFETY: fork, exec and write to a pipe we own.
                let g = unsafe {
                    let g = libc::fork();
                    if g == 0 {
                        libc::execvp(sleep.as_ptr(), argv.as_ptr());
                        libc::_exit(127);
                    }
                    let b = g.to_ne_bytes();
                    libc::write(fds[1], b.as_ptr().cast(), b.len());
                    g
                };
                super::supervisor_main(g);
            }
            assert!(sup > 0, "setup: fork");
            // The supervisor must supervise ITS grandchild: read its pid.
            let mut b = [0u8; 4];
            // SAFETY: read from our pipe.
            let n = unsafe { libc::read(fds[0], b.as_mut_ptr().cast(), 4) };
            assert_eq!(n, 4, "setup: the supervisor reported its child");
            let grandchild = i32::from_ne_bytes(b);
            std::thread::sleep(std::time::Duration::from_millis(300));
            // SAFETY: signal our own children.
            unsafe { libc::kill(sup, sig) };
            let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut status = 0;
            let mut done = false;
            while std::time::Instant::now() < end {
                // SAFETY: waitpid on our own child.
                if unsafe { libc::waitpid(sup, &mut status, libc::WNOHANG) } == sup {
                    done = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            if !done {
                unsafe { libc::kill(sup, libc::SIGKILL) };
                unsafe { libc::kill(grandchild, libc::SIGKILL) };
                unsafe { libc::waitpid(sup, &mut status, 0) };
            }
            assert!(
                done,
                "ATTACK: the supervisor did not forward signal {sig} to its child: it was still \
                 waiting 5 s later"
            );
            assert!(
                libc::WIFEXITED(status),
                "ATTACK: the supervisor died of signal {sig} itself (no forwarding handler is \
                 installed): {status:#x}"
            );
            assert_eq!(
                libc::WEXITSTATUS(status),
                128 + sig,
                "the supervisor reports its child's death by signal {sig} (128 + signal)"
            );
        }
    }

    // ── guest-fs GF-2: load before gate ─────────────────────────────────────

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    /// `--source-fd N=PATH -- <binary> <args...>` names the program to open
    /// before the gate and passes the binary's argv through untouched,
    /// trailing program args included. The plain form opens nothing; the bare
    /// form opens the default program on fd 3.
    #[test]
    fn source_fd_arg_parses() {
        let (src, bin, args) = super::parse_args(&argv(&[
            "axon-guest-init",
            "--source-fd",
            "3=/work/job/program.ax",
            "--",
            "/usr/bin/axon",
            "run",
            "--source-fd",
            "3",
            "/work/job/program.ax",
            "a",
            "--b",
        ]))
        .unwrap();
        assert_eq!(
            src,
            Some(super::SourceFd {
                fd: 3,
                path: "/work/job/program.ax".into()
            }),
            "ATTACK: --source-fd did not name the program to load before the gate"
        );
        assert_eq!(bin, "/usr/bin/axon");
        assert_eq!(
            args,
            argv(&[
                "/usr/bin/axon",
                "run",
                "--source-fd",
                "3",
                "/work/job/program.ax",
                "a",
                "--b"
            ]),
            "the binary's argv must pass through untouched"
        );

        let (src, bin, args) =
            super::parse_args(&argv(&["axon-guest-init", "/bin/true", "x"])).unwrap();
        assert_eq!(
            (src, bin, args),
            (None, "/bin/true".into(), argv(&["/bin/true", "x"]))
        );

        let (src, _, args) = super::parse_args(&argv(&["axon-guest-init"])).unwrap();
        assert_eq!(
            src,
            Some(super::SourceFd {
                fd: 3,
                path: "/axon/program.ax".into()
            }),
            "ATTACK: the bare default does not load its program before the gate"
        );
        assert_eq!(
            args,
            argv(&[
                "/usr/bin/axon",
                "run",
                "--source-fd",
                "3",
                "/axon/program.ax"
            ])
        );

        for bad in [
            &["axon-guest-init", "--source-fd"][..],
            &["axon-guest-init", "--source-fd", "3"],
            &["axon-guest-init", "--source-fd", "x=/p", "--", "/bin/true"],
            &["axon-guest-init", "--source-fd", "3=", "--", "/bin/true"],
            &["axon-guest-init", "--source-fd", "3=/p", "/bin/true"],
            &["axon-guest-init", "--source-fd", "3=/p", "--"],
        ] {
            assert!(
                super::parse_args(&argv(bad)).is_err(),
                "a malformed --source-fd was accepted: {bad:?}"
            );
        }
    }

    /// fds 0-2 are the workload's own streams; the program never goes there.
    #[test]
    fn source_fd_rejects_std_fd() {
        for n in ["0", "1", "2", "-1"] {
            let e = super::parse_args(&argv(&[
                "axon-guest-init",
                "--source-fd",
                &format!("{n}=/work/job/program.ax"),
                "--",
                "/usr/bin/axon",
            ]));
            assert!(
                e.is_err(),
                "ATTACK: --source-fd {n} was accepted: the program would replace a std stream"
            );
        }
    }

    /// The opened program sits on exactly fd N, readable, inheritable across
    /// exec (FD_CLOEXEC clear), and the descriptor `open` returned is closed.
    /// A symlink and a directory are refused. Run alone: it installs fd N in
    /// the test process.
    #[test]
    fn open_source_before_gate_dups_to_n() {
        if !alone("tests::open_source_before_gate_dups_to_n") {
            return;
        }
        const N: libc::c_int = 417;
        let p = write_tmp("gf2-src.ax", "fn main() -> i64 { 0 }\n");
        let got = super::open_source_before_gate(N, p.to_str().unwrap()).unwrap();
        // SAFETY: fcntl / read / close on fds this test owns.
        unsafe {
            let flags = libc::fcntl(N, libc::F_GETFD);
            assert!(flags >= 0, "ATTACK: fd {N} is not open after the load");
            assert_eq!(
                flags & libc::FD_CLOEXEC,
                0,
                "ATTACK: fd {N} is close-on-exec: the workload would not inherit its program"
            );
            assert_ne!(got, N, "setup: open already returned {N}");
            assert_eq!(
                libc::fcntl(got, libc::F_GETFD),
                -1,
                "ATTACK: the fd open returned ({got}) leaked into the workload"
            );
            let mut buf = [0u8; 64];
            let r = libc::read(N, buf.as_mut_ptr().cast(), buf.len());
            assert_eq!(&buf[..r.max(0) as usize], b"fn main() -> i64 { 0 }\n");
            libc::close(N);
        }
        let link = p.with_extension("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&p, &link).unwrap();
        assert!(
            super::open_source_before_gate(N, link.to_str().unwrap()).is_err(),
            "ATTACK: a symlinked program was followed"
        );
        assert!(
            super::open_source_before_gate(N, std::env::temp_dir().to_str().unwrap()).is_err(),
            "ATTACK: a directory was accepted as the program"
        );
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_file(&p);
    }

    // ── guest-fs GF-3: the `fs` axis ──────────────────────────────────────────

    /// `fs` is valid only under `axon-vm-mmds/2`, its one value is `"none"`,
    /// and `/2` alone (no `fs`) is still a policy. MMDS is held to the same.
    #[test]
    fn fs_key_requires_schema_v2() {
        assert_eq!(
            cmdline_err(r#"{"schema":"axon-vm-mmds/1","allowed_effects":["IO"],"fs":"none"}"#),
            CmdlinePolicyError::FsNeedsSchemaV2(Some("axon-vm-mmds/1".to_string())),
            "ATTACK: a /1 policy carrying fs was accepted"
        );
        assert_eq!(
            cmdline_err(r#"{"schema":"axon-vm-mmds/2","allowed_effects":["IO"],"fs":"read"}"#),
            CmdlinePolicyError::BadFsValue("read".to_string()),
            "ATTACK: an fs value other than none was accepted"
        );
        assert!(matches!(
            cmdline_err(r#"{"schema":"axon-vm-mmds/3","fs":"none"}"#),
            CmdlinePolicyError::WrongSchema(_)
        ));
        let ok = |j: &str| {
            parse_cmdline_policy(&cmdline_with(&format!("axon.policy={}", b64(j))))
                .expect("accepted")
                .expect("present")
        };
        let p = ok(r#"{"schema":"axon-vm-mmds/2","fs":"none"}"#);
        assert_eq!(p.fs.as_deref(), Some("none"), "fs alone is a policy");
        let p = ok(r#"{"schema":"axon-vm-mmds/2","allowed_effects":["IO"]}"#);
        assert_eq!(p.fs, None);
        let mmds = || {
            Ok(Some(
                serde_json::from_str::<MmdsPayload>(
                    r#"{"schema":"axon-vm-mmds/1","allowed_effects":["IO"],"fs":"none"}"#,
                )
                .unwrap(),
            ))
        };
        assert!(
            load_policy(Ok(None), mmds).is_err(),
            "ATTACK: an MMDS /1 policy carrying fs was accepted"
        );
    }

    /// The empty path scopes: both variables PRESENT and empty (empty denies
    /// every read / write; absent would be unrestricted).
    #[test]
    fn fs_none_exports_empty_path_scopes() {
        if !alone("tests::fs_none_exports_empty_path_scopes") {
            return;
        }
        std::env::set_var("AXON_ALLOWED_FS_READ", "/");
        std::env::set_var("AXON_ALLOWED_FS_WRITE", "/");
        super::export_fs_none_env();
        assert_eq!(
            std::env::var("AXON_ALLOWED_FS_READ").as_deref(),
            Ok(""),
            "ATTACK: fs none left the read path scope open"
        );
        assert_eq!(
            std::env::var("AXON_ALLOWED_FS_WRITE").as_deref(),
            Ok(""),
            "ATTACK: fs none left the write path scope open"
        );
    }

    /// Step a classic-BPF program over one `seccomp_data` (nr, arch). Only the
    /// three opcodes `fs_none_filter` emits are understood.
    fn run_bpf(prog: &[u8], arch: u32, nr: u32) -> u32 {
        let (mut pc, mut acc) = (0usize, 0u32);
        loop {
            let i = &prog[pc * 8..pc * 8 + 8];
            let code = u16::from_le_bytes([i[0], i[1]]);
            let (jt, jf) = (i[2] as usize, i[3] as usize);
            let k = u32::from_le_bytes([i[4], i[5], i[6], i[7]]);
            pc += 1;
            match code {
                0x20 => {
                    acc = match k {
                        0 => nr,
                        4 => arch,
                        _ => panic!("unexpected load offset {k}"),
                    }
                }
                0x15 => pc += if acc == k { jt } else { jf },
                0x06 => return k,
                _ => panic!("unexpected opcode {code:#x}"),
            }
        }
    }

    /// Any ABI but x86_64 (i386, and x32 numbers on x86_64) is KILLED; the
    /// tables decide the rest; anything unlisted (`prctl`) is KILLED.
    #[test]
    fn fs_none_filter_kills_non_x86_64_arch() {
        let p = super::fs_none_filter();
        const I386: u32 = 0x4000_0003;
        const KILL: u32 = super::SECCOMP_RET_KILL_PROCESS;
        for nr in [0, 1, 257, 2] {
            assert_eq!(
                run_bpf(&p, I386, nr),
                KILL,
                "ATTACK: i386 syscall {nr} got through the arch check"
            );
        }
        assert_eq!(
            run_bpf(&p, super::AUDIT_ARCH_X86_64, 0x4000_0000),
            KILL,
            "x32 read"
        );
        assert_eq!(run_bpf(&p, super::AUDIT_ARCH_X86_64, 157), KILL, "prctl");
        for nr in super::FS_NONE_ALLOW {
            assert_eq!(
                run_bpf(&p, super::AUDIT_ARCH_X86_64, nr),
                super::SECCOMP_RET_ALLOW
            );
        }
        for nr in super::FS_NONE_EACCES {
            assert_eq!(
                run_bpf(&p, super::AUDIT_ARCH_X86_64, nr),
                super::SECCOMP_RET_ERRNO_EACCES,
                "ATTACK: path syscall {nr} was not refused with EACCES"
            );
        }
        assert_eq!(p.len() / 8, 76);
    }

    /// In a forked child: install the fs filter, then `open` is EACCES while
    /// `write` and `getrandom` work, the `B263-FS` report is written, and a
    /// later `prctl` (another filter) is KILLED.
    #[test]
    fn fs_none_filter_denies_path_syscalls() {
        if !alone("tests::fs_none_filter_denies_path_syscalls") {
            return;
        }
        let target = write_tmp("gf3-target", "secret\n");
        let report = write_tmp("gf3-report", "");
        let c_target = std::ffi::CString::new(target.to_str().unwrap()).unwrap();
        let st = forked(10, || {
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open(&report)
                .unwrap();
            if super::apply_fs_none(f).is_err() {
                return 10;
            }
            // SAFETY: plain syscalls on a NUL-terminated path and local buffers.
            unsafe {
                let fd = libc::open(c_target.as_ptr(), libc::O_RDONLY);
                if fd != -1 {
                    return 11;
                }
                if *libc::__errno_location() != libc::EACCES {
                    return 12;
                }
                if libc::write(1, b"".as_ptr().cast(), 0) != 0 {
                    return 13;
                }
                let mut buf = [0u8; 8];
                if libc::getrandom(buf.as_mut_ptr().cast(), 8, 0) != 8 {
                    return 14;
                }
            }
            0
        })
        .expect("the filtered child hung");
        assert!(
            libc::WIFEXITED(st) && libc::WEXITSTATUS(st) == 0,
            "ATTACK: under fs none a path syscall was not refused with EACCES \
             (or the substrate was withheld): status {st:#x}"
        );
        assert_eq!(
            std::fs::read_to_string(&report).unwrap(),
            "B263-FS none insns=76\n"
        );
        let st = forked(10, || {
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open(&report)
                .unwrap();
            if super::apply_fs_none(f).is_err() {
                return 10;
            }
            // SAFETY: prctl with no pointer arguments.
            unsafe { libc::prctl(libc::PR_GET_SECCOMP, 0, 0, 0, 0) };
            0
        })
        .expect("the filtered child hung");
        assert!(
            libc::WIFSIGNALED(st) && libc::WTERMSIG(st) == libc::SIGSYS,
            "ATTACK: prctl ran under the fs filter: status {st:#x}"
        );
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_file(&report);
    }
}
