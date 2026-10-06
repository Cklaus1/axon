//! axon-guest-init — static PID-1 supervisor for Axon microVM guests.
//!
//! Boot sequence:
//!   1. Re-seed entropy from virtio-rng (/dev/urandom)
//!   2. Read the capability policy (schema axon-vm-mmds/1) and REFUSE to start
//!      the guest unless it constrains something (see "Policy channels").
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
//!   axon-guest-init /usr/bin/axon run /axon/program.ax
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
    /// Only the three ENFORCED mechanisms count. `principal`, `run_id` and
    /// `source_hash` are LABELS — the module header above is explicit that they
    /// "grant and withhold nothing" — so a payload carrying only labels is
    /// exactly as unpoliced as `{}` and must not be rescued by them.
    fn constrains_anything(&self) -> bool {
        self.allowed_effects.is_some()
            || self.budget_tokens.is_some()
            || self.seccomp_bpf_b64.is_some()
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
            if p.seccomp_bpf_b64.is_none() {
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
/// The only schema the cmdline channel accepts.
const POLICY_SCHEMA: &str = "axon-vm-mmds/1";
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
    /// `schema` absent or not `axon-vm-mmds/1`.
    WrongSchema(Option<String>),
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
                "cmdline policy CONSTRAINS NOTHING: no allowed_effects, budget_tokens or \
                 seccomp_bpf_b64"
            ),
            LabelsOnly => write!(
                f,
                "cmdline policy is LABELS ONLY: principal/run_id/source_hash grant and \
                 withhold nothing, and no enforced field is present"
            ),
            WrongSchema(s) => write!(
                f,
                "cmdline policy has WRONG SCHEMA: expected `{POLICY_SCHEMA}`, got {s:?}"
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
    if payload.schema.as_deref() != Some(POLICY_SCHEMA) {
        return Err(CmdlinePolicyError::WrongSchema(payload.schema));
    }
    Ok(Some(payload))
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
            Ok(Some(p)) if p.constrains_anything() => Ok((p, PolicySource::Mmds)),
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
            cmdline_err(r#"{"schema":"axon-vm-mmds/2","allowed_effects":["IO"]}"#),
            CmdlinePolicyError::WrongSchema(Some("axon-vm-mmds/2".to_string()))
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
}
