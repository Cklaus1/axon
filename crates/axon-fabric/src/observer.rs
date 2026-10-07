//! M3 — the preflight observation (`governance/specs/v022-psv-protocol.md` §7,
//! ADR-002).
//!
//! Before a protected launch the Fabric:
//! 1. is issued a fresh nonce by the CUSTODIAN (a separate uid; amendment 50,
//!    [`crate::custodian`]) and puts it in the launch manifest;
//! 2. asks the operator-pinned OBSERVER program to observe that launch (the
//!    observer signs with its own key, observer domain);
//! 3. verifies the observation (an early check: nothing reaches the root
//!    helper on an observation Fabric can already refuse);
//! 4. hands the observation to the privileged launcher, which verifies it
//!    AGAIN by the same [`verify_observation`], and spends the nonce through
//!    the custodian at the root boundary: one nonce, one launch.
//!
//! Fabric never spends a nonce. A spend by the principal the nonce constrains
//! proves nothing to the root boundary, and a Fabric spend first would leave
//! the helper nothing to spend (amendment 50).
//!
//! Step 3 means all of these hold:
//! * the signature verifies under a key in the OBSERVER trust root — not the
//!   qualification root, not the repository;
//! * the observation is of THIS launch manifest, field for field;
//! * it names the key that signed it;
//! * it is fresh and of this epoch.
//!
//! The helper adds: the nonce was issued by the custodian, for the observed
//! epoch, and never spent before.
//!
//! Any failure refuses the launch: nothing runs.
//!
//! The Fabric holds no observer key and cannot mint an observation. What the
//! observer MEASURES is the observer's (operator) responsibility; the dev
//! stand-in used in tests copies the manifest's facts and proves only the
//! protocol, never the measurement.

use crate::backend::{Clock, TrustAuthority};
use crate::psv::VerifiedObservation;
use axon_psv::{LaunchManifest, PreflightObservation};
use std::path::{Path, PathBuf};

pub const DEFAULT_OBSERVATION_MAX_AGE_S: u64 = 300;

/// The custodian's nonce store: one file per issued nonce; consuming renames it
/// to `.used`, atomically, so a nonce authorizes at most one launch. On a
/// protected host only the custodian ([`crate::custodian::Server`]) opens it,
/// as its own uid.
#[derive(Debug, Clone)]
pub struct NonceStore {
    pub dir: PathBuf,
}

impl NonceStore {
    /// Issue a fresh 128-bit nonce for `epoch`.
    pub fn issue(&self, epoch: u64, clock: &Clock) -> Result<String, String> {
        let mut b = [0u8; 16];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut b)
            .map_err(|_| "no system randomness for a nonce".to_string())?;
        let nonce: String = b.iter().map(|x| format!("{x:02x}")).collect();
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("nonce store: {e}"))?;
        let rec = serde_json::json!({"epoch": epoch, "issued_unix": clock.now_unix()});
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.dir.join(format!("{nonce}.issued")))
            .and_then(|mut f| std::io::Write::write_all(&mut f, rec.to_string().as_bytes()))
            .map_err(|e| format!("nonce store: {e}"))?;
        Ok(nonce)
    }

    /// [`Self::issue`] under the custodian's bounds (amendment 79): records older
    /// than `max_age_s` are dropped first (an expired nonce is refused by
    /// [`Self::consume`] whether its record exists or not), and no more than
    /// `max_outstanding` issued-and-unspent nonces are held. The caller may be the
    /// principal the nonces constrain, so an unbounded store is a disk it fills.
    pub fn issue_bounded(
        &self,
        epoch: u64,
        clock: &Clock,
        max_age_s: u64,
        max_outstanding: usize,
    ) -> Result<String, String> {
        let outstanding = self.prune(clock, max_age_s)?;
        if outstanding >= max_outstanding {
            return Err(format!(
                "{outstanding} nonces are outstanding (at most {max_outstanding}): none is \
                 issued until some are spent or expire"
            ));
        }
        self.issue(epoch, clock)
    }

    /// Remove every record whose nonce can no longer be spent: an `.issued` older
    /// than `max_age_s`, a `.used` spent longer ago than that. Returns how many
    /// `.issued` records remain. A record that does not parse (a crash between its
    /// creation and its write) ages by its file's mtime.
    pub fn prune(&self, clock: &Clock, max_age_s: u64) -> Result<usize, String> {
        let now = clock.now_unix();
        let mut outstanding = 0usize;
        let Ok(dir) = std::fs::read_dir(&self.dir) else {
            return Ok(0);
        };
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let issued = name.ends_with(".issued");
            if !issued && !name.ends_with(".used") {
                continue;
            }
            let rec: Option<serde_json::Value> = std::fs::read(e.path())
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
            let at = rec
                .as_ref()
                .and_then(|r| {
                    r["spent_unix"]
                        .as_i64()
                        .or_else(|| r["issued_unix"].as_i64())
                })
                .or_else(|| {
                    e.metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                })
                .unwrap_or(i64::MIN / 2);
            if now - at > max_age_s as i64 {
                let _ = std::fs::remove_file(e.path());
            } else if issued {
                outstanding += 1;
            }
        }
        Ok(outstanding)
    }

    /// The record of `nonce` if it is outstanding for `epoch`: issued here, no
    /// longer than `max_age_s` ago, never consumed. Returns the record's paths and
    /// the unix time at which it expires.
    fn outstanding(
        &self,
        nonce: &str,
        epoch: u64,
        clock: &Clock,
        max_age_s: u64,
    ) -> Result<(PathBuf, PathBuf, i64), String> {
        if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("nonce {nonce:?} is not one this custodian issues"));
        }
        let issued = self.dir.join(format!("{nonce}.issued"));
        let used = self.dir.join(format!("{nonce}.used"));
        let rec: serde_json::Value = match std::fs::read(&issued) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| format!("nonce record: {e}"))?,
            Err(_) if used.exists() => return Err(format!("nonce {nonce} was already used")),
            Err(_) => return Err(format!("nonce {nonce} was never issued by this custodian")),
        };
        if rec["epoch"].as_u64() != Some(epoch) {
            return Err(format!(
                "nonce {nonce} was issued for epoch {}, not {epoch}",
                rec["epoch"]
            ));
        }
        let issued_unix = rec["issued_unix"].as_i64().unwrap_or(i64::MIN / 2);
        let age = clock.now_unix() - issued_unix;
        if age < 0 || age as u64 > max_age_s {
            return Err(format!("nonce {nonce} is {age}s old (max {max_age_s}s)"));
        }
        Ok((issued, used, issued_unix + max_age_s as i64))
    }

    /// Amendment 79: whether `nonce` is outstanding for `epoch`, without
    /// consuming it. Returns the unix time at which it expires.
    pub fn check(
        &self,
        nonce: &str,
        epoch: u64,
        clock: &Clock,
        max_age_s: u64,
    ) -> Result<i64, String> {
        self.outstanding(nonce, epoch, clock, max_age_s)
            .map(|(_, _, expires)| expires)
    }

    /// Consume `nonce`: it must have been issued here, for `epoch`, no longer
    /// than `max_age_s` ago, and never consumed. Exactly one caller wins.
    pub fn consume(
        &self,
        nonce: &str,
        epoch: u64,
        clock: &Clock,
        max_age_s: u64,
    ) -> Result<(), String> {
        let (issued, used, _) = self.outstanding(nonce, epoch, clock, max_age_s)?;
        // The atomic step: whoever renames it consumed it.
        std::fs::rename(&issued, &used).map_err(|_| format!("nonce {nonce} was already used"))
    }
}

/// Where observer keys are trusted.
#[derive(Debug, Clone)]
pub struct ObserverTrust {
    pub dir: PathBuf,
    pub operator_owned: bool,
    /// ADR-002 key-role separation: the OTHER operator authority roots. A key
    /// in the observer root that is also in one of these is refused.
    pub separate_from: Vec<(TrustAuthority, PathBuf)>,
    /// The protected host's attestation signer (`signer.public_key`, hex).
    /// Fabric holds its private half, so the observer root must not hold it.
    pub host_signer_public_key: Option<String>,
}

impl ObserverTrust {
    /// Production: `/etc/axon/trust/observer`, operator-owned, kept separate
    /// from every other root under `/etc/axon/trust`.
    pub fn operator() -> ObserverTrust {
        ObserverTrust {
            dir: TrustAuthority::Observer.operator_dir(),
            operator_owned: true,
            separate_from: crate::backend::sibling_roots(
                TrustAuthority::Observer,
                &TrustAuthority::Observer.operator_dir(),
            ),
            host_signer_public_key: None,
        }
    }
    /// TESTS ONLY. The other roots are `dir`'s siblings, named by authority.
    #[cfg(any(test, feature = "test-trust-root"))]
    pub fn for_test(dir: &Path) -> ObserverTrust {
        ObserverTrust {
            dir: dir.to_path_buf(),
            operator_owned: false,
            separate_from: crate::backend::sibling_roots(TrustAuthority::Observer, dir),
            host_signer_public_key: None,
        }
    }

    /// ADR-002 key-role separation, over the roots THEMSELVES (not a store's
    /// copy of them): no key in the observer root is the host signer's public
    /// key or a key of another operator authority. Fabric holds the signer's
    /// private key and signs any domain (`sign-evidence --authority
    /// observer`), and a key in two roots is valid for both; either way Fabric
    /// could mint an observation that verifies (PSV-6, C9 dev review round 1;
    /// A57). Checked when the host config loads AND at every observation.
    ///
    /// ONE implementation with every other Fabric trust read
    /// ([`crate::backend::exclusive_root_keys`]): an unreadable other root
    /// refuses, never reads as holding no key (C9 round 2; A67).
    pub fn check_separation(&self) -> Result<(), String> {
        crate::backend::exclusive_root_keys(
            TrustAuthority::Observer,
            &self.dir,
            &self.separate_from,
            self.operator_owned.then_some(Path::new("/")),
            self.host_signer_public_key.as_deref(),
        )
        .map(drop)
    }
}

/// The observer, as the operator configured it (O1 host config `observer`).
#[derive(Debug, Clone)]
pub struct ObserverConfig {
    /// The operator-installed observer program: `PROGRAM --manifest FILE
    /// --out DIR` writes `DIR/observation.json` + `.sig`.
    pub command: PathBuf,
    pub command_sha256: String,
    /// D: the interpreter a SCRIPT observer runs under, pinned; the script is
    /// handed to it as `/dev/fd/N` of the verified descriptor. `None`: the
    /// command must be a binary (a `#!` file is refused, never run through
    /// its unpinned first line).
    pub interpreter: Option<crate::sealed_exec::Pinned>,
    /// D: the uid that must own the command and its interpreter (root on a
    /// protected host). `None`: any owner (tests without an operator only).
    pub exec_owner: Option<u32>,
    pub trust: ObserverTrust,
    /// Amendment 50: who issues the nonce. Fabric only asks.
    pub custodian: crate::custodian::Custodian,
    pub max_age_s: u64,
    pub clock: Clock,
    /// Amendment 68 (decision G1 = A): `Some` on a protected host. The
    /// observation is then asked of the operator's observer SERVICE through
    /// the privileged helper's `--observe` relay (this route), and the
    /// in-uid `command` above is not used. A production host config naming a
    /// `command` is refused (`ProtectedHost::load`): an observer run as the
    /// Fabric uid signs with a key the Fabric uid can read.
    pub relay: Option<crate::backend::PrivilegedRoute>,
}

/// What an observation is verified against. ONE implementation
/// ([`verify_observation`]) for Fabric's early check and the privileged
/// launcher's check at the root boundary (amendment 50).
#[derive(Debug, Clone)]
pub struct ObservationRules {
    pub trust: ObserverTrust,
    pub max_age_s: u64,
    pub clock: Clock,
}

/// Verify an observation (`bytes`, its detached `signature`) of the launch
/// manifest `m` whose digest is `manifest_digest`:
/// * the observer root is the operator's (walked, when it is) and shares no
///   key with another role;
/// * the signature verifies under a key in the OBSERVER root, observer domain;
/// * the observation names the key that signed it;
/// * it joins `m` field for field and is of `manifest_digest`;
/// * it is fresh.
///
/// The epoch is the caller's to join (Fabric: its expected epoch; the helper:
/// the custodian's issuing epoch, at the spend).
pub fn verify_observation(
    cfg: &ObservationRules,
    bytes: &[u8],
    signature: &str,
    m: &LaunchManifest,
    manifest_digest: &str,
) -> Result<PreflightObservation, String> {
    if cfg.trust.operator_owned {
        crate::backend::check_operator_owned(&cfg.trust.dir)?;
    }
    // The roots as they are NOW, not as they were when the host config
    // loaded: a key added to another root since then is refused here.
    cfg.trust.check_separation()?;
    // The OBSERVER domain, under the OBSERVER root (RULE:authority-domain).
    let signer = crate::backend::verify_operator_evidence_signed(
        "observation",
        bytes,
        signature,
        &cfg.trust.dir,
        TrustAuthority::Observer,
    )?;
    let o: PreflightObservation =
        serde_json::from_slice(bytes).map_err(|e| format!("observation is malformed: {e}"))?;
    if o.observer_key_id != signer {
        return Err(format!(
            "observation claims observer {} but is signed by {signer}",
            o.observer_key_id
        ));
    }
    o.joins(m, manifest_digest)?;
    let at = crate::backend::parse_utc(&o.observed_at)
        .ok_or_else(|| format!("observed_at {:?} is not a UTC timestamp", o.observed_at))?;
    let age = cfg.clock.now_unix() - at;
    if age < 0 || age as u64 > cfg.max_age_s {
        return Err(format!(
            "observation is {age}s old (max {}s)",
            cfg.max_age_s
        ));
    }
    Ok(o)
}

/// Obtain and verify the observation of `m` (whose manifest bytes are at
/// `manifest_file`). `work` is a new directory. The nonce is NOT spent here:
/// the privileged launcher spends it through the custodian (amendment 50).
pub fn observe(
    cfg: &ObserverConfig,
    m: &LaunchManifest,
    manifest_digest: &str,
    manifest_file: &Path,
    epoch: u64,
    work: &Path,
) -> Result<VerifiedObservation, String> {
    let (bytes, signature) = match &cfg.relay {
        Some(h) => relayed(h, manifest_file)?,
        None => run_program(cfg, manifest_file, work)?,
    };
    let o = verify_observation(
        &ObservationRules {
            trust: cfg.trust.clone(),
            max_age_s: cfg.max_age_s,
            clock: cfg.clock,
        },
        &bytes,
        &signature,
        m,
        manifest_digest,
    )?;
    if o.epoch != epoch {
        return Err(format!("observation is for epoch {}, not {epoch}", o.epoch));
    }
    Ok(VerifiedObservation {
        sha256: axon_psv::sha256_hex(&bytes),
        bytes,
        signature,
    })
}

/// Amendment 68: the observation, from the operator's observer service
/// through the privileged helper's `--observe` relay (executed from its
/// verified descriptor, as for a launch). The helper authenticates the
/// observer program and measures this running Fabric for it; Fabric holds no
/// observer key and never talks to the observer itself. Returns the exact
/// observation bytes and their signature, which [`observe`] verifies.
fn relayed(
    h: &crate::backend::PrivilegedRoute,
    manifest_file: &Path,
) -> Result<(Vec<u8>, String), String> {
    use crate::privileged_launcher as pl;
    use std::io::{Read, Write};
    let manifest =
        crate::backend::read_regular(manifest_file).map_err(|e| format!("launch manifest: {e}"))?;
    let helper = crate::sealed_exec::open_verified(
        &h.helper,
        Some(h.owner),
        crate::sealed_exec::Lease::IfGranted,
    )
    .map_err(|e| format!("privileged launcher {e}"))?;
    let mut args: Vec<std::ffi::OsString> = vec!["--observe".into()];
    if let Some(t) = &h.test_config {
        args.push("--test-config".into());
        args.push(t.clone().into_os_string());
    }
    let mut child = crate::sealed_exec::command(
        &helper,
        None,
        &args,
        &[("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")],
        &[],
    )
    .and_then(|mut c| {
        c.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())
    })
    .map_err(|e| format!("privileged launcher did not start: {e}"))?;
    let body = serde_json::to_vec(&pl::ObserveRequest {
        schema: pl::OBSERVE_REQUEST_SCHEMA.into(),
        manifest: String::from_utf8_lossy(&manifest).into_owned(),
    })
    .map_err(|e| e.to_string())?;
    if let Some(mut i) = child.stdin.take() {
        let _ = i.write_all(&body);
    }
    let mut text = Vec::new();
    if let Some(o) = child.stdout.take() {
        let _ = o
            .take(crate::observer_service::MAX_REPLY + 4096)
            .read_to_end(&mut text);
    }
    let status = child.wait().ok().and_then(|s| s.code());
    let r: pl::ObserveReport = serde_json::from_slice(&text)
        .map_err(|e| format!("the privileged launcher (exit {status:?}) gave no report: {e}"))?;
    if r.schema != pl::OBSERVE_REPORT_SCHEMA {
        return Err(format!(
            "the privileged launcher's report is not {}",
            pl::OBSERVE_REPORT_SCHEMA
        ));
    }
    match (r.ok, r.observation, r.observation_signature) {
        (true, Some(o), Some(sig)) => Ok((o.into_bytes(), sig)),
        _ => Err(format!(
            "the privileged launcher relayed no observation: {}",
            r.error.unwrap_or_default()
        )),
    }
}

/// The in-uid observer PROGRAM: test-trust builds only (a production host
/// config naming one is refused at load, amendment 68). Returns the exact
/// observation bytes and their signature.
fn run_program(
    cfg: &ObserverConfig,
    manifest_file: &Path,
    work: &Path,
) -> Result<(Vec<u8>, String), String> {
    // The observer program is an authority: exactly its pinned bytes, and
    // (D) those bytes are the ones executed: hashed on the open descriptor,
    // which is then executed itself (a script through its pinned interpreter,
    // reading the same descriptor).
    use crate::sealed_exec::{self, Lease, Pinned};
    let program = sealed_exec::open_verified(
        &Pinned {
            path: cfg.command.clone(),
            sha256: cfg.command_sha256.clone(),
        },
        cfg.exec_owner,
        Lease::IfGranted,
    )
    .map_err(|e| format!("observer {e}"))?;
    let interpreter = match &cfg.interpreter {
        Some(p) => Some(
            sealed_exec::open_verified(p, cfg.exec_owner, Lease::IfGranted)
                .map_err(|e| format!("observer interpreter {e}"))?,
        ),
        None => None,
    };
    std::fs::create_dir(work).map_err(|e| format!("observation dir: {e}"))?;
    let status = sealed_exec::command(
        &program,
        interpreter.as_ref(),
        &[
            "--manifest".into(),
            manifest_file.as_os_str().to_os_string(),
            "--out".into(),
            work.as_os_str().to_os_string(),
        ],
        &[("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")],
        &[],
    )
    .map_err(|e| format!("observer did not run: {e}"))?
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .status()
    .map_err(|e| format!("observer did not run: {e}"))?;
    if !status.success() {
        return Err(format!("observer exited {:?}", status.code()));
    }
    let rec = work.join("observation.json");
    // ONE read: the bytes whose signature is verified are the bytes parsed,
    // joined and digested (review wf_d725935a-7ed).
    let bytes = crate::backend::read_regular(&rec).map_err(|e| format!("observation: {e}"))?;
    // The signature is read once too, and the SAME text goes into the bundle
    // and to the privileged launcher.
    let signature =
        crate::backend::read_signature("observation", &work.join("observation.json.sig"))?;
    Ok((bytes, signature))
}
