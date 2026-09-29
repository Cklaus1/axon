//! M3 — the preflight observation (`governance/specs/v022-psv-protocol.md` §7,
//! ADR-002).
//!
//! Before a protected launch the Fabric:
//! 1. takes a fresh nonce from the CUSTODIAN's store and puts it in the launch
//!    manifest;
//! 2. asks the operator-pinned OBSERVER program to observe that launch (the
//!    observer signs with its own key, observer domain);
//! 3. verifies the observation and CONSUMES the nonce;
//! 4. launches only if every step held.
//!
//! Step 3 means all of these hold:
//! * the signature verifies under a key in the OBSERVER trust root — not the
//!   qualification root, not the repository;
//! * the observation is of THIS launch manifest, field for field;
//! * it names the key that signed it;
//! * it is fresh and of this epoch;
//! * the nonce was issued by the store and never used before.
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
/// to `.used`, atomically, so a nonce authorizes at most one launch.
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

    /// Consume `nonce`: it must have been issued here, for `epoch`, no longer
    /// than `max_age_s` ago, and never consumed. Exactly one caller wins.
    pub fn consume(
        &self,
        nonce: &str,
        epoch: u64,
        clock: &Clock,
        max_age_s: u64,
    ) -> Result<(), String> {
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
        let age = clock.now_unix() - rec["issued_unix"].as_i64().unwrap_or(i64::MIN / 2);
        if age < 0 || age as u64 > max_age_s {
            return Err(format!("nonce {nonce} is {age}s old (max {max_age_s}s)"));
        }
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
    pub trust: ObserverTrust,
    pub nonces: NonceStore,
    pub max_age_s: u64,
    pub clock: Clock,
}

/// Obtain and verify the observation of `m` (whose manifest bytes are at
/// `manifest_file`), consuming its nonce. `work` is a new directory.
pub fn observe(
    cfg: &ObserverConfig,
    m: &LaunchManifest,
    manifest_digest: &str,
    manifest_file: &Path,
    epoch: u64,
    work: &Path,
) -> Result<VerifiedObservation, String> {
    // The observer program is an authority: exactly its pinned bytes.
    let got = crate::backend::sha256_file(&cfg.command)?;
    if got != cfg.command_sha256 {
        return Err(format!(
            "observer {} has sha256 {got}, not its pin {}",
            cfg.command.display(),
            cfg.command_sha256
        ));
    }
    std::fs::create_dir(work).map_err(|e| format!("observation dir: {e}"))?;
    let status = std::process::Command::new(&cfg.command)
        .arg("--manifest")
        .arg(manifest_file)
        .arg("--out")
        .arg(work)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("observer did not run: {e}"))?;
    if !status.success() {
        return Err(format!("observer exited {:?}", status.code()));
    }
    let rec = work.join("observation.json");
    if cfg.trust.operator_owned {
        crate::backend::check_operator_owned(&cfg.trust.dir)?;
    }
    // The roots as they are NOW, not as they were when the host config
    // loaded: a key added to another root since then is refused here.
    cfg.trust.check_separation()?;
    // ONE read: the bytes whose signature is verified are the bytes parsed,
    // joined and digested (review wf_d725935a-7ed).
    let bytes = crate::backend::read_regular(&rec).map_err(|e| format!("observation: {e}"))?;
    // The signature is read once too, and the SAME text goes into the bundle.
    let signature =
        crate::backend::read_signature("observation", &work.join("observation.json.sig"))?;
    // The OBSERVER domain, under the OBSERVER root (RULE:authority-domain).
    let signer = crate::backend::verify_operator_evidence_signed(
        "observation",
        &bytes,
        &signature,
        &cfg.trust.dir,
        TrustAuthority::Observer,
    )?;
    let o: PreflightObservation =
        serde_json::from_slice(&bytes).map_err(|e| format!("observation is malformed: {e}"))?;
    if o.observer_key_id != signer {
        return Err(format!(
            "observation claims observer {} but is signed by {signer}",
            o.observer_key_id
        ));
    }
    o.joins(m, manifest_digest)?;
    if o.epoch != epoch {
        return Err(format!("observation is for epoch {}, not {epoch}", o.epoch));
    }
    let at = crate::backend::parse_utc(&o.observed_at)
        .ok_or_else(|| format!("observed_at {:?} is not a UTC timestamp", o.observed_at))?;
    let age = cfg.clock.now_unix() - at;
    if age < 0 || age as u64 > cfg.max_age_s {
        return Err(format!(
            "observation is {age}s old (max {}s)",
            cfg.max_age_s
        ));
    }
    // Last: consume. Only an observation that is otherwise acceptable spends
    // its nonce, and it can be spent once.
    cfg.nonces
        .consume(&o.nonce, epoch, &cfg.clock, cfg.max_age_s)?;
    Ok(VerifiedObservation {
        sha256: axon_psv::sha256_hex(&bytes),
        bytes,
        signature,
    })
}
