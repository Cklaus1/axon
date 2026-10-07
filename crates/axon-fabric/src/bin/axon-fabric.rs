//! `axon-fabric` — the Fabric submit CLI. JSON in, JSON out.
//!
//! ```text
//! axon-fabric submit --request FILE|- --journal FILE --check-registry FILE
//!                    --grant-registry FILE
//!                    --store DIR --tenant T --family F --expected-epoch N
//!                    [--workspace DIR] [--state DIR (default <journal>.state)]
//!                    [--budget-micro N] [--budget-exec-ms N]
//!                    [--linux-launcher SH --linux-manifest JSON
//!                     --linux-evidence JSON [--linux-artifacts DIR]
//!                     [--linux-evidence-sig SIG] [--linux-waivers JSON]
//!                     [--linux-evidence-max-age-s N]
//!                     --linux-out-root DIR]
//!
//! The Linux profile is eligible only for an issuer-signed evidence record
//! (`<evidence>.sig` unless `--linux-evidence-sig`), verified against the
//! Ed25519 public keys in the OPERATOR's trust root,
//! `/etc/axon/trust/qualification/` (root-owned, not group/other
//! writable, no symlinks; a caller cannot choose it), no older than the max
//! age (default 30 days).
//! axon-fabric workspace-import --state DIR --tenant T --root DIR
//! axon-fabric verify-evidence --record FILE --issuers DIR --authority A [--signature FILE]
//! axon-fabric sign-evidence --record FILE --key PKCS8 --authority A   (OPERATOR, with the operator's key)
//! axon-fabric verifier-manifest   (OPERATOR: the installed binary describes itself → verifier.json)
//! axon-fabric protected-host-paths [--config FILE] [--launcher-config FILE] [--custodian-config FILE]   (the trust preflight's probe list)
//!
//! `protected-host-paths` prints every path a protected-host config pins, one
//! `KIND<TAB>PATH` line each (`operator-file`, `operator-dir`, `signing-key`,
//! `service-dir`, `privileged-helper`), then the paths the privileged
//! helper's own config pins (`--launcher-config`, default
//! /etc/axon/protected-launcher.json) and one `helper-fabric-uid<TAB>UID`
//! line, then the custodian's own config and store (`--custodian-config`,
//! default /etc/axon/custodian.json; `custodian-store`) and its
//! `custodian-uid`, `custodian-fabric-uid` and `custodian-launcher-uid`
//! lines (amendment 50): the list `ProtectedHost::load` ownership-walks, so the
//! trust preflight probes exactly those. It verifies no pin and authorizes
//! nothing; `--config` defaults to /etc/axon/protected-host.json.
//!
//! Evidence signatures are `axon-evidence-signature/2`: domain-separated by
//! AUTHORITY (qualification | observer | verifier | admission). A signature
//! for one authority never verifies as another.
//! axon-fabric verify-readiness --repo DIR
//!
//! `verify-readiness` is the AUTHORITATIVE verdict for the three protected
//! readiness components (governance/specs/v022-protected-suite-verdict.md):
//! the repository is read as evidence only, and authority comes solely from
//! the operator's `/etc/axon/trust/qualification/` (no flag chooses it). The
//! output names the build (`production` / `test-trust`); readiness accepts
//! only a production build, installed and pinned by the operator.
//!
//! `verify-evidence` checks an operator-signed evidence document (the v0.22
//! protected-host certification record) with the SAME rules as the B263
//! qualification record: exit 0 and `{"verified":true,"issuer":…}` only when
//! a detached `axon-evidence-signature/2` (authority-domain separated) over the record's exact bytes
//! verifies under a key in `--issuers`; otherwise a refusal (exit 4).
//! axon-fabric status --journal FILE --op ID --grant-registry FILE --principal P --grant-ref G
//! axon-fabric cancel --journal FILE --op ID --reason TEXT --grant-registry FILE --principal P --grant-ref G
//!
//! `status` and `cancel` act only for the authority that SUBMITTED the op:
//! the grant must resolve for the principal in the operator's grant registry
//! (as for `submit`) and be the principal|grant the journal recorded for the
//! op. Holding a journal path or an operation id confers nothing
//! (G03-r22-authority-intersection); anything else is `unauthorized` (exit 7)
//! and nothing is written — the journal is replayed without any write, and
//! only the authorized op's scope is reconciled, after authorization.
//!
//! GRANT REGISTRY (D1). On a PROTECTED host the grant registry is the one the
//! operator's host config pins (`grant_registry`, path + sha256, every grant
//! file operator-owned) and nothing else: `--grant-registry` is REFUSED on
//! every route (exit 2) before anything is read or written, and a host config
//! that pins none authorizes nothing. In development (no protected host) the
//! caller names it; its sha256 is recorded in each op's intent
//! (`config.grant.registry_sha256`), and `status`/`cancel` accept only the
//! registry bytes that authorized the op.
//! ```
//!
//! Output on success: `{"schema":"axon-fabric-submit/1", "receipt": <acf-execution-receipt/1>,
//! "check_report": …, "replayed": bool, "backend": …, "reason": …}`, exit 0 —
//! whatever the receipt status (a receipt is an answer, including
//! `unsupported`, `denied`, `failed`). A refusal that produced no receipt is
//! `{"schema":"axon-fabric-refusal/1","kind":…,"reason":…}` with a nonzero
//! exit (2 io/journal, 3 malformed, 4 unregistered, 5 conflict, 6 stale epoch,
//! 7 unauthorized: `grant_ref` not resolvable for `principal_ref`).
//!
//! SIGNED RECEIPTS (G01-r22-independent-issuer). The operator's check registry
//! may carry a `"signer": {"issuer_ref", "key_path", "public_key"}` block: the
//! verifier identity this Fabric issues receipts as, its PKCS#8 Ed25519 key,
//! and that key's public half, pinned. There is no per-call flag: a caller
//! cannot choose an issuer name or supply a key. The signer is refused before
//! any work unless the key file is a regular file owned by this uid and
//! readable by no one else, and derives exactly the pinned public key. After
//! the receipt is FINAL, Fabric signs an `acf-receipt-attestation/2` binding
//! issuer, key id, request and receipt digests and the receipt's identity —
//! never for a replay (the journal is the caller's to name, so a replayed
//! receipt may be one the caller wrote), and only if the check workload could
//! not have reached the key: the admitted
//! grant gives it no effect at all (the local interpreter cannot path-scope a
//! read, so any IO would reach the key file), or it ran in the protected
//! microVM. Otherwise `"receipt_attestation"` is `null` with
//! `"attestation_withheld"` saying why. `axon-fabric keygen --out PATH`
//! provisions a key (0400 — owner read-only, the mode the signing-key loader
//! requires; it refuses anything wider, 0600 included — never over an
//! existing file) and prints the public
//! key to pin here and to register in the loop store's `verifier_keys`.
//!
//! Every executable comes from the `--check-registry` file (path + sha256),
//! never from the request. Every grant comes from the `--grant-registry` file
//! (`axon-fabric-grant-registry/1`, grant files pinned by sha256); the
//! interpreter effect ceiling is DERIVED from the resolved grant — there is no
//! flag that sets or removes it.

use std::io::Read;
use std::path::PathBuf;

#[cfg(feature = "test-trust-root")]
use axon_fabric::backend::QualificationTrust;
use axon_fabric::submit::{scope, EpochSource, SubmitConfig};
use axon_fabric::{Journal, ResourceVector};
use axon_loop_contracts::{AuthorityEpoch, OperationId};
use serde_json::json;

fn refuse(kind: &str, reason: &str, code: i32) -> ! {
    println!(
        "{}",
        json!({"schema": "axon-fabric-refusal/1", "kind": kind, "reason": reason})
    );
    std::process::exit(code)
}

struct Args(Vec<String>);

impl Args {
    fn opt(&self, flag: &str) -> Option<String> {
        self.0
            .iter()
            .position(|a| a == flag)
            .and_then(|i| self.0.get(i + 1).cloned())
    }
    /// The flag is present at all (with or without a value).
    fn has(&self, flag: &str) -> bool {
        self.0.iter().any(|a| a == flag)
    }
    fn req(&self, flag: &str) -> String {
        self.opt(flag)
            .unwrap_or_else(|| refuse("usage", &format!("{flag} is required"), 2))
    }
    fn num(&self, flag: &str, default: u64) -> u64 {
        match self.opt(flag) {
            None => default,
            Some(v) => v
                .parse()
                .unwrap_or_else(|_| refuse("usage", &format!("{flag} must be a number"), 2)),
        }
    }
}

fn main() {
    let mut argv = std::env::args().skip(1);
    let cmd = argv.next().unwrap_or_default();
    let a = Args(argv.collect());
    match cmd.as_str() {
        "submit" => submit(&a),
        "status" => status(&a),
        "cancel" => cancel(&a),
        "workspace-import" => workspace_import(&a),
        "keygen" => keygen(&a),
        "verify-evidence" => verify_evidence(&a),
        "verify-readiness" => verify_readiness(&a),
        "sign-evidence" => sign_evidence(&a),
        "verifier-manifest" => verifier_manifest(),
        "protected-host-paths" => protected_host_paths(&a),
        #[cfg(feature = "test-trust-root")]
        "__psv-host-guest" => psv_host_guest(),
        _ => refuse(
            "usage",
            "usage: axon-fabric submit|status|cancel … (see --help in the source header)",
            2,
        ),
    }
}

/// The operator-configured signer in the check registry, if any. Every defect
/// refuses as `unregistered` (exit 4) before any work.
fn signer(registry: &std::path::Path) -> Option<(axon_loop_contracts::OpaqueRef, Vec<u8>)> {
    let bad = |why: String| -> ! { refuse("unregistered", &format!("registry signer: {why}"), 4) };
    let text = std::fs::read_to_string(registry).unwrap_or_else(|e| bad(e.to_string()));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| bad(e.to_string()));
    let sg = v.get("signer")?;
    Some(signer_from(sg, registry, "registry signer"))
}

/// The protected host's signer: named by the OPERATOR's host config, never by
/// a registry file (O1).
fn host_signer(
    h: &axon_fabric::protected_host::ProtectedHost,
) -> (axon_loop_contracts::OpaqueRef, Vec<u8>) {
    let sg = serde_json::json!({
        "issuer_ref": h.signer.issuer_ref,
        "key_path": h.signer.key_path,
        "public_key": h.signer.public_key,
    });
    signer_from(&sg, std::path::Path::new("/"), "protected-host signer")
}

fn signer_from(
    sg: &serde_json::Value,
    registry: &std::path::Path,
    what: &str,
) -> (axon_loop_contracts::OpaqueRef, Vec<u8>) {
    let bad = |why: String| -> ! { refuse("unregistered", &format!("{what}: {why}"), 4) };
    let obj = sg
        .as_object()
        .unwrap_or_else(|| bad("not an object".into()));
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    if keys != ["issuer_ref", "key_path", "public_key"] {
        bad(format!(
            "must be exactly issuer_ref, key_path, public_key; has {keys:?}"
        ));
    }
    let field = |k: &str| {
        obj[k]
            .as_str()
            .unwrap_or_else(|| bad(format!("{k} is not a string")))
    };
    let id = axon_loop_contracts::OpaqueRef::new(field("issuer_ref"))
        .unwrap_or_else(|e| bad(format!("issuer_ref: {e}")));
    let mut path = PathBuf::from(field("key_path"));
    if path.is_relative() {
        path = registry
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join(path);
    }
    // ONE open, never through a symlink: the file whose owner and mode are
    // checked is the file whose bytes are read (a path stat'd and then read
    // again can be two files).
    let mut f = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(&path)
            .unwrap_or_else(|e| {
                if e.raw_os_error() == Some(libc::ELOOP) {
                    bad(format!(
                        "key {} is not a regular file (a symlink)",
                        path.display()
                    ))
                }
                bad(format!("key {}: {e}", path.display()))
            })
    };
    let meta = f
        .metadata()
        .unwrap_or_else(|e| bad(format!("key {}: {e}", path.display())));
    {
        use std::os::unix::fs::MetadataExt;
        if !meta.file_type().is_file() {
            bad(format!("key {} is not a regular file", path.display()));
        }
        // SAFETY: geteuid has no preconditions and cannot fail.
        let euid = unsafe { libc::geteuid() };
        // Spec §2 rule 1: mode 0400. Readable by no one else, and writable by
        // no one — not even its owner, the service that uses it.
        if meta.uid() != euid || meta.mode() & 0o277 != 0 {
            bad(format!(
                "key {} must be owned by this uid, readable by no one else and writable by no one \
                 (mode 0400; it is {:o})",
                path.display(),
                meta.mode() & 0o777
            ));
        }
    }
    let mut key = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut key)
        .unwrap_or_else(|e| bad(format!("key {}: {e}", path.display())));
    let pk = axon_loop_contracts::attestation::public_key_of(&key)
        .unwrap_or_else(|e| bad(format!("key {}: {e}", path.display())));
    if pk != field("public_key") {
        bad(format!(
            "key {} does not derive the pinned public_key",
            path.display()
        ));
    }
    (id, key)
}

fn verify_evidence(a: &Args) {
    let record = PathBuf::from(a.req("--record"));
    let issuers = PathBuf::from(a.req("--issuers"));
    let authority = authority_flag(a);
    let sig = a.opt("--signature").map(PathBuf::from).unwrap_or_else(|| {
        let mut s = record.as_os_str().to_owned();
        s.push(".sig");
        PathBuf::from(s)
    });
    // `--issuers` is the CALLER's choice, so "verified" alone says only that
    // some key in a directory the caller named signed these bytes. The output
    // names that root and this build, and is authoritative only when the root
    // is the operator's own for this authority, passes the operator-ownership
    // walk, and this is a production build (review FIELD-ORIGIN, C9 round 1).
    let operator_root = authority.operator_dir();
    let authority_of_answer = axon_fabric::backend::verify_evidence_authority(
        axon_fabric::backend::TEST_TRUST_BUILD,
        &issuers,
        &operator_root,
        axon_fabric::backend::check_operator_owned(&issuers),
    );
    let authoritative = authority_of_answer.is_ok();
    match axon_fabric::backend::verify_operator_evidence(&record, &sig, &issuers, authority) {
        Ok(issuer) => println!(
            "{}",
            serde_json::json!({
                "schema": "axon-fabric-verify-evidence/1",
                "verified": true,
                "issuer": issuer,
                "authority": authority.dir_name(),
                "trust_root": issuers,
                "operator_root": operator_root,
                "authoritative": authoritative,
                "non_authoritative_because": authority_of_answer.err(),
                "build": if axon_fabric::backend::TEST_TRUST_BUILD { "test-trust" } else { "production" },
                "verifier": axon_fabric::readiness::verifier_identity(),
            })
        ),
        Err(e) => refuse("unregistered", &e, 4),
    }
}

/// The protected host's configuration: the operator's file, or — in a
/// test-trust build ONLY — `--protected-host-config FILE`, whose qualification
/// trust is `--protected-host-issuers DIR` (ownership unchecked). A production
/// build refuses both flags: nothing the caller passes configures protection.
fn protected_host(a: &Args) -> Option<axon_fabric::protected_host::ProtectedHost> {
    use axon_fabric::protected_host::ProtectedHost;
    let test_cfg = a.opt("--protected-host-config");
    if test_cfg.is_some() || a.opt("--protected-host-issuers").is_some() {
        if !axon_fabric::backend::TEST_TRUST_BUILD {
            refuse(
                "usage",
                "--protected-host-config is a test-trust-build flag; a production build reads only \
                 /etc/axon/protected-host.json",
                2,
            );
        }
        #[cfg(feature = "test-trust-root")]
        {
            let cfg = PathBuf::from(test_cfg.unwrap_or_else(|| a.req("--protected-host-config")));
            let mut trust = QualificationTrust::for_manifest(&cfg);
            trust.issuers_dir = PathBuf::from(a.req("--protected-host-issuers"));
            return Some(
                ProtectedHost::for_test(&cfg, None, trust)
                    .unwrap_or_else(|e| refuse("unregistered", &format!("protected host: {e}"), 4)),
            );
        }
    }
    ProtectedHost::operator()
        .unwrap_or_else(|e| refuse("unregistered", &format!("protected host: {e}"), 4))
}

fn authority_flag(a: &Args) -> axon_fabric::backend::TrustAuthority {
    let v = a.req("--authority");
    axon_fabric::backend::TrustAuthority::parse(&v).unwrap_or_else(|| {
        // Derived from the one list of authorities, so a new one is never
        // missing here (`monitor` was).
        let names: Vec<&str> = axon_fabric::backend::TrustAuthority::ALL
            .iter()
            .map(|a| a.dir_name())
            .collect();
        refuse(
            "usage",
            &format!("--authority must be one of: {}", names.join(", ")),
            2,
        )
    })
}

/// OPERATOR tool: sign a record for ONE authority with the operator's own key
/// (PKCS#8 from `axon-fabric keygen`), writing `<record>.sig`. Run where the
/// key lives; no agent or protected service ever holds it (ADR-001 D5/D6).
fn sign_evidence(a: &Args) {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let record = PathBuf::from(a.req("--record"));
    let authority = authority_flag(a);
    let key = std::fs::read(a.req("--key")).unwrap_or_else(|e| refuse("io", &e.to_string(), 2));
    let kp = Ed25519KeyPair::from_pkcs8(&key)
        .unwrap_or_else(|_| refuse("usage", "--key is not an Ed25519 PKCS#8 key", 2));
    let bytes = std::fs::read(&record).unwrap_or_else(|e| refuse("io", &e.to_string(), 2));
    let msg = axon_fabric::backend::evidence_signing_message(authority, &bytes);
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let sig = serde_json::json!({
        "schema": axon_fabric::backend::EVIDENCE_SIGNATURE_SCHEMA, "alg": "ed25519",
        "domain": authority.dir_name(), "public_key": hex(kp.public_key().as_ref()),
        "signature": hex(kp.sign(&msg).as_ref()),
    });
    let mut out = record.as_os_str().to_owned();
    out.push(".sig");
    std::fs::write(&out, sig.to_string()).unwrap_or_else(|e| refuse("io", &e.to_string(), 2));
    println!(
        "{}",
        serde_json::json!({"schema":"axon-fabric-sign-evidence/1","signature":PathBuf::from(out),
                           "authority":authority.dir_name()})
    );
}

/// TEST-TRUST BUILDS ONLY: a stand-in for `fc_linux_profile.sh`'s PSV mode that
/// runs the REAL trusted runner (`axon_psv::runner`) and the real interpreter
/// on the host, then returns the launcher's result shape. It lets the Fabric's
/// PSV dispatch be tested end to end without KVM, and `--tamper MODE` applies
/// ONE forgery to what comes back, so each test shows which check refuses
/// it. A production build does not contain it (it is a guest emulator, never
/// an authority).
#[cfg(feature = "test-trust-root")]
fn psv_host_guest() {
    use axon_psv::runner::{run, RunnerConfig};
    let args: Vec<String> = std::env::args().skip(2).collect();
    if let Some(i) = args.iter().position(|a| a == "--verify-result") {
        // Record what environment the verify step was given (the Fabric
        // must clear it: review wf_d725935a-7ed).
        if let Some(dir) = args.get(i + 1) {
            let leaked = std::env::var_os("PSV_VERIFY_ENV_PROBE").is_some();
            let _ = std::fs::write(
                PathBuf::from(dir).join("verify-env-leaked"),
                if leaked { "yes" } else { "no" },
            );
            // …and whether the per-attempt secret was still on disk then.
            let secret = std::fs::read_to_string(PathBuf::from(dir).join("job-secret-path"))
                .map(|p| std::path::Path::new(p.trim()).exists())
                .unwrap_or(false);
            let _ = std::fs::write(
                PathBuf::from(dir).join("verify-secret-present"),
                if secret { "yes" } else { "no" },
            );
        }
        std::process::exit(0);
    }
    let get = |n: &str| {
        args.iter()
            .position(|a| a == n)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let need = |n: &str| get(n).unwrap_or_else(|| panic!("__psv-host-guest: {n} required"));
    let out = PathBuf::from(need("--out"));
    let job = PathBuf::from(need("--psv-job"));
    let sha = need("--psv-manifest-sha");
    let tamper = get("--tamper").unwrap_or_default();
    let od = out.join("out");
    std::fs::create_dir_all(&od).unwrap();
    std::fs::write(
        out.join("job-secret-path"),
        job.join("completion-secret").to_string_lossy().as_bytes(),
    )
    .unwrap();
    let write_result = |status: &str, code: i32| {
        let stdout = od.join("stdout");
        if !stdout.exists() {
            std::fs::write(&stdout, "PSV-VERDICT sha256=stand-in\n").unwrap();
        }
        let s = axon_psv::sha256_hex(&std::fs::read(&stdout).unwrap());
        let r = serde_json::json!({
            "schema": "axon-linux-microvm-result/1", "status": status,
            "admissible": code == 0, "output_bound": true,
            // The RUNNER's exit (a verdict was written); the test's own exit is
            // inside the verdict.
            "workload_exit": if code == 0 { Some(0) } else { None },
            "outputs": {"stdout": {"sha256": s}},
            "cleanup": {"complete": true, "left_behind": []},
            "psv": {"launch_manifest_sha256": sha, "bound": code == 0,
                    "verdict_sha256": std::fs::read(od.join("verdict.bound"))
                        .or_else(|_| std::fs::read(od.join("verdict.json"))).ok()
                        .map(|b| axon_psv::sha256_hex(&b))},
        });
        std::fs::write(out.join("result.json"), r.to_string()).unwrap();
        std::process::exit(code);
    };
    if tamper == "vmm-died" {
        write_result("vmm-died", 21);
    }
    if tamper == "suite-changed" {
        // The OPERATOR suite changes under the guest: the runner must refuse.
        std::fs::write(
            PathBuf::from(need("--psv-suite")).join("planted.ax"),
            "// not the registered suite\n",
        )
        .unwrap();
    }
    if tamper == "candidate-changed" {
        // The candidate changes under the guest: the runner must refuse.
        std::fs::write(
            PathBuf::from(need("--psv-candidate")).join("f.ax"),
            "fn double(n: i64) -> i64 { 42 }\n",
        )
        .unwrap();
    }
    // The guest is booted under the `--policy` bytes (the launcher embeds them
    // on the kernel cmdline, where the runner reads them). `policy`: a
    // launcher that embedded ANOTHER policy than the one it was given
    // (PSV-6, A87: the runner must refuse it).
    let mut policy = std::fs::read(need("--policy")).ok();
    if tamper == "policy" {
        policy = Some(
            br#"{"schema":"axon-vm-mmds/1","allowed_effects":["AI","Exec","IO","Net","Time"]}"#
                .to_vec(),
        );
    }
    let v = run(&RunnerConfig {
        manifest: job.join("launch-manifest.json"),
        secret: job.join("completion-secret"),
        candidate: PathBuf::from(need("--psv-candidate")),
        suite: PathBuf::from(need("--psv-suite")),
        out: od.clone(),
        axon: PathBuf::from(need("--axon")),
        runner_exe: PathBuf::from(need("--axon")),
        expected_manifest_sha256: sha.clone(),
        drop: None,
        policy,
    });
    let mut v = serde_json::to_value(&v).unwrap();
    let rehash = |od: &std::path::Path, v: &mut serde_json::Value| {
        let b = std::fs::read(od.join("test-stdout")).unwrap_or_default();
        v["stdout_sha256"] = serde_json::json!(axon_psv::sha256_hex(&b));
    };
    match tamper.as_str() {
        "" => {}
        // The guest CLAIMS a pass for a run whose output says otherwise.
        "claim-pass" => v["status"] = serde_json::json!("passed"),
        // Output changed after the verdict named it.
        "stdout" => {
            let mut b = std::fs::read(od.join("test-stdout")).unwrap();
            b.extend_from_slice(b"{\"name\":\"x\",\"status\":\"ok\"}\n");
            std::fs::write(od.join("test-stdout"), b).unwrap();
        }
        // A CONSISTENT forgery (output + verdict agree) by someone without K.
        "forge" => {
            let t = v["test"].as_str().unwrap().to_string();
            std::fs::write(
                od.join("test-stdout"),
                format!(
                    "{{\"name\":\"{t}\",\"status\":\"ok\",\"duration_ms\":0,\"completion\":\"{}\"}}\n\
                     {{\"type\":\"summary\",\"total\":1,\"passed\":1,\"failed\":0,\"skipped\":0,\"duration_ms\":0}}\n",
                    "ab".repeat(32)
                ),
            )
            .unwrap();
            rehash(&od, &mut v);
            v["status"] = serde_json::json!("passed");
            v["exit_code"] = serde_json::json!(0);
        }
        "other-manifest" => v["launch_manifest_sha256"] = serde_json::json!("0".repeat(64)),
        "inputs" => v["inputs"]["match"] = serde_json::json!(false),
        // A genuine run whose verdict names another policy than the
        // manifest's (PSV-6, A87: Fabric's join must refuse it).
        "verdict-policy" => v["policy_sha256"] = serde_json::json!("e".repeat(64)),
        // A previous attempt's genuine output, re-labelled for this launch.
        "replay" => {
            let from = PathBuf::from(need("--replay-from"));
            std::fs::copy(from.join("out/test-stdout"), od.join("test-stdout")).unwrap();
            rehash(&od, &mut v);
            v["status"] = serde_json::json!("passed");
            v["exit_code"] = serde_json::json!(0);
        }
        // The run's exit, reported non-zero after a genuine pass.
        "exit" => v["exit_code"] = serde_json::json!(3),
        // The interpreter died after candidate output: a LONE failure line
        // with no keyed token, and a failing exit.
        "forge-fail" => {
            let t = v["test"].as_str().unwrap().to_string();
            std::fs::write(
                od.join("test-stdout"),
                format!("{{\"name\":\"{t}\",\"status\":\"failed\",\"duration_ms\":0,\"message\":\"x\"}}\n{{\"type\":\"summary\",\"total\":1,\"passed\":0,\"failed\":1,\"skipped\":0,\"duration_ms\":0}}\n"),
            )
            .unwrap();
            rehash(&od, &mut v);
            v["status"] = serde_json::json!("failed");
            v["exit_code"] = serde_json::json!(1);
        }
        // A failure, reported with a clean exit.
        "exit0" => v["exit_code"] = serde_json::json!(0),
        // A GENUINE keyed pass whose output then lost its summary line (the
        // run did not complete: amendment 75), consistently rehashed.
        "no-summary" => {
            let b = std::fs::read_to_string(od.join("test-stdout")).unwrap();
            let kept: String = b
                .lines()
                .filter(|l| !l.contains("\"type\":\"summary\""))
                .map(|l| format!("{l}\n"))
                .collect();
            assert_ne!(
                kept, b,
                "__psv-host-guest: the run printed no summary to drop"
            );
            std::fs::write(od.join("test-stdout"), kept).unwrap();
            rehash(&od, &mut v);
        }
        // The guest REFUSED, but everything else is a genuine pass (keyed
        // output, its digest, exit 0, this launch's inputs and test): the
        // guest's refusal alone must stand (M180).
        "refused-after-pass" => {
            v["status"] = serde_json::json!("refused");
            v["refusal"] = serde_json::json!("the runner refused after the run");
        }
        "candidate-changed"
        | "policy"
        | "suite-changed"
        | "unbound"
        | "swap-after"
        | "cleanup-incomplete"
        | "runner-died-after-verdict" => {}
        other => panic!("__psv-host-guest: unknown tamper {other}"),
    }
    std::fs::write(od.join("verdict.json"), axon_psv::canonical_json(&v)).unwrap();
    if tamper == "swap-after" {
        // The launcher bound the GENUINE verdict; the extracted file is then
        // replaced by one that needs no key (a failure).
        std::fs::rename(od.join("verdict.json"), od.join("verdict.bound")).unwrap();
        v["status"] = serde_json::json!("failed");
        std::fs::write(od.join("verdict.json"), axon_psv::canonical_json(&v)).unwrap();
    }
    if tamper == "unbound" {
        // A GENUINE verdict, from a launch the launcher did not bind (27).
        write_result("verdict-unbound", 27);
    }
    // A GENUINE, bound verdict from a launch that did not end admissibly: the
    // jail could not be cleaned up (24), or the runner died after writing the
    // verdict (10, workload exit 101: e.g. a println to a full /out). Either
    // way there is no verdict, whatever `derive` sees (PSV-4/PSV-5, C9 dev
    // review round 1).
    let inadmissible = match tamper.as_str() {
        "cleanup-incomplete" => Some(("cleanup-incomplete", 24, false, 0)),
        "runner-died-after-verdict" => Some(("workload-failed", 10, true, 101)),
        _ => None,
    };
    if let Some((status, code, cleaned, workload_exit)) = inadmissible {
        let stdout = od.join("stdout");
        if !stdout.exists() {
            std::fs::write(&stdout, "PSV-VERDICT sha256=stand-in\n").unwrap();
        }
        let s = axon_psv::sha256_hex(&std::fs::read(&stdout).unwrap());
        let verdict = axon_psv::sha256_hex(&std::fs::read(od.join("verdict.json")).unwrap());
        let r = serde_json::json!({
            "schema": "axon-linux-microvm-result/1", "status": status,
            "admissible": false, "output_bound": true, "workload_exit": workload_exit,
            "outputs": {"stdout": {"sha256": s}},
            "cleanup": {"complete": cleaned,
                        "left_behind": if cleaned { vec![] } else { vec!["tap0"] }},
            "psv": {"launch_manifest_sha256": sha, "bound": true, "verdict_sha256": verdict},
        });
        std::fs::write(out.join("result.json"), r.to_string()).unwrap();
        std::process::exit(code);
    }
    write_result("ok", 0);
}

/// OPERATOR tool: print the `axon-verifier-manifest/1` describing THIS binary
/// (its absolute path, sha256 and build provenance, and the trust roots it
/// decides over). Run the INSTALLED binary; the operator reviews the output and
/// installs it as `/etc/axon/trust/verifier.json`. The binary describes itself,
/// so there is no hand-copied digest to get wrong.
fn verifier_manifest() {
    let mut m = axon_fabric::readiness::verifier_identity();
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .unwrap_or_else(|e| refuse("io", &e.to_string(), 2));
    let roots: serde_json::Map<String, serde_json::Value> =
        axon_fabric::backend::TrustAuthority::ALL
            .iter()
            .map(|a| {
                (
                    a.dir_name().to_string(),
                    serde_json::json!(a.operator_dir()),
                )
            })
            .collect();
    m["schema"] = serde_json::json!("axon-verifier-manifest/1");
    m["path"] = serde_json::json!(exe);
    m["trust_roots"] = serde_json::Value::Object(roots);
    // Which launch routes this build lets attest a protected verdict: the
    // very function psv_receipt calls (PSV-4, C9 round 3). A production
    // verifier says false for a test-trust helper; no test build can show it.
    use axon_fabric::backend::{attests_protected, LaunchRoute};
    m["launch_routes_attesting_protected"] = serde_json::json!({
        "direct": attests_protected(LaunchRoute::Direct),
        "privileged_production_helper":
            attests_protected(LaunchRoute::Privileged { test_build: false }),
        "privileged_test_trust_helper":
            attests_protected(LaunchRoute::Privileged { test_build: true }),
    });
    println!("{}", serde_json::to_string_pretty(&m).unwrap());
}

/// Every path the protected-host config pins, for the trust preflight.
fn protected_host_paths(a: &Args) {
    let cfg = a
        .opt("--config")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(axon_fabric::protected_host::PROTECTED_HOST_CONFIG));
    let mut paths = axon_fabric::protected_host::pinned_paths(&cfg)
        .unwrap_or_else(|e| refuse("unregistered", &e, 4));
    // A: the privileged helper's own operator config and what it pins. The
    // helper reads only its fixed path; `--launcher-config` names a dev
    // fixture's (it lists paths and authorizes nothing).
    let helper_cfg = a
        .opt("--launcher-config")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(axon_fabric::privileged_launcher::CONFIG_PATH));
    let (fabric_uid, pin, helper_paths) =
        axon_fabric::protected_host::helper_pinned_paths(&helper_cfg)
            .unwrap_or_else(|e| refuse("unregistered", &e, 4));
    for hp in helper_paths {
        if !paths.contains(&hp) {
            paths.push(hp);
        }
    }
    // Amendment 50: the custodian's own operator config and its store. It is
    // read by the custodian, not by `load`; `--custodian-config` names a dev
    // fixture's.
    let cust_cfg = a
        .opt("--custodian-config")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(axon_fabric::custodian::CONFIG_PATH));
    let (cust, cust_paths) = axon_fabric::protected_host::custodian_pinned_paths(&cust_cfg)
        .unwrap_or_else(|e| refuse("unregistered", &e, 4));
    for cp in cust_paths {
        if !paths.contains(&cp) {
            paths.push(cp);
        }
    }
    // The uid the helper admits: the preflight holds it to its Fabric actor.
    println!("helper-fabric-uid\t{fabric_uid}");
    // Amendment 79: the Fabric program the helper pins: the preflight holds
    // it to the installed one.
    println!("helper-fabric-path\t{}", pin.path.display());
    println!("helper-fabric-sha256\t{}", pin.sha256);
    // The custodian's three roles: the preflight holds them to its actors and
    // requires the custodian to be neither the Fabric nor root.
    println!("custodian-uid\t{}", cust.custodian_uid);
    println!("custodian-fabric-uid\t{}", cust.fabric_uid);
    println!("custodian-launcher-uid\t{}", cust.launcher_uid);
    // Amendment 79: the uid the custodian answers `check` for: the observer's.
    println!(
        "custodian-observer-uid\t{}",
        cust.observer_uid.map_or("-".to_string(), |u| u.to_string())
    );
    for (kind, p) in paths {
        let s = p.to_string_lossy();
        if s.contains(['\t', '\n']) || s != p.as_os_str().to_str().unwrap_or_default() {
            refuse(
                "unregistered",
                &format!("pinned path {s:?} is not a plain UTF-8 path without tabs or newlines"),
                4,
            );
        }
        println!("{}\t{s}", kind.as_str());
    }
}

/// The AUTHORITATIVE protected-readiness verdict for `--repo` (see
/// `axon_fabric::readiness`). Authority is the operator's root only: there is
/// no flag to choose it.
fn verify_readiness(a: &Args) {
    let repo = PathBuf::from(a.req("--repo"));
    let v = axon_fabric::readiness::protected_components(
        &repo,
        &axon_fabric::readiness::ReadinessTrust::operator(),
    );
    println!("{v}");
}

fn keygen(a: &Args) {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let out = PathBuf::from(a.req("--out"));
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .unwrap_or_else(|_| refuse("io", "key generation failed", 2));
    let pk = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .unwrap_or_else(|_| refuse("io", "generated key does not load", 2))
        .public_key()
        .as_ref()
        .to_vec();
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o400)
            .open(&out)
            .unwrap_or_else(|e| refuse("io", &format!("{}: {e}", out.display()), 2));
        f.write_all(pkcs8.as_ref())
            .unwrap_or_else(|e| refuse("io", &e.to_string(), 2));
    }
    println!(
        "{}",
        json!({
            "schema": "axon-fabric-issuer-key/1",
            "private_key": out,
            "public_key": pk.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "fingerprint": axon_loop_contracts::attestation::key_fingerprint(&pk),
        })
    );
}

/// D1: the grant registry for this call. On a protected host it is the
/// operator's pinned one and ONLY that: a caller `--grant-registry` is refused
/// (never silently ignored) before anything is read or written. Off a
/// protected host (development) the caller names it.
fn grant_registry(
    a: &Args,
    host: Option<&axon_fabric::protected_host::ProtectedHost>,
) -> axon_fabric::GrantRegistry {
    match host {
        Some(h) => {
            if a.has("--grant-registry") {
                refuse_caller_grant_registry();
            }
            h.grants()
                .unwrap_or_else(|e| refuse("unauthorized", &format!("protected host: {e}"), 7))
        }
        None => axon_fabric::GrantRegistry::load(&PathBuf::from(a.req("--grant-registry")))
            .unwrap_or_else(|e| refuse("unauthorized", &e, 7)),
    }
}

fn refuse_caller_grant_registry() -> ! {
    refuse(
        "usage",
        &format!(
            "--grant-registry is not accepted on a protected host: grants come only from the \
             operator's grant_registry pinned in {}",
            axon_fabric::protected_host::PROTECTED_HOST_CONFIG
        ),
        2,
    )
}

fn submit(a: &Args) {
    // The protected profile's trust root is the OPERATOR's (/etc/axon/trust),
    // never the caller's and never a repository directory: whoever submits
    // cannot choose which issuers qualify it. Refused before anything else.
    for flag in axon_fabric::protected_host::REFUSED_CALLER_FLAGS {
        if a.opt(flag).is_some() {
            refuse(
                "usage",
                &format!(
                    "{flag} is not accepted: the protected profile is configured only by the \
                     operator, in {} (issuers in /etc/axon/trust/qualification/)",
                    axon_fabric::protected_host::PROTECTED_HOST_CONFIG
                ),
                2,
            );
        }
    }
    let host = protected_host(a);
    if host.is_some() && a.opt("--check-registry").is_some() {
        refuse(
            "usage",
            "--check-registry is not accepted on a protected host: suites come from the \
             operator's registry in the protected-host config; a request names a suite by id",
            2,
        );
    }
    // D1: a caller grant registry is refused on a protected host before the
    // request is even read ([`grant_registry`] refuses it again at load).
    if host.is_some() && a.has("--grant-registry") {
        refuse_caller_grant_registry();
    }
    let req_src = a.req("--request");
    let text = if req_src == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .unwrap_or_else(|e| refuse("io", &e.to_string(), 2));
        s
    } else {
        std::fs::read_to_string(&req_src).unwrap_or_else(|e| refuse("io", &e.to_string(), 2))
    };
    let registry_path = match &host {
        Some(h) => h.suite_registry.clone(),
        None => PathBuf::from(a.req("--check-registry")),
    };
    let registry = axon_cortex::runner::CheckRegistry::load(&registry_path)
        .unwrap_or_else(|e| refuse("unregistered", &e, 4));
    let issuer = match &host {
        Some(h) => Some(host_signer(h)),
        None => signer(&registry_path),
    };
    let grants = grant_registry(a, host.as_ref());
    let sc =
        scope(&a.req("--tenant"), &a.req("--family")).unwrap_or_else(|e| refuse("usage", &e, 2));
    let expected = AuthorityEpoch::new(a.num("--expected-epoch", u64::MAX))
        .unwrap_or_else(|e| refuse("usage", &format!("--expected-epoch: {e}"), 2));
    let protected_host = host.as_ref().map(|h| axon_fabric::psv::HostIdentity {
        config_sha256: h.config_sha256.clone(),
        suite_registry_sha256: h.suite_registry_sha256.clone(),
        grant_registry_sha256: h.grant_registry.as_ref().map(|(_, pin)| pin.clone()),
    });
    let observer = host.as_ref().and_then(|h| h.observer.clone());
    // A82: on a protected host the launch-time epoch is read from the store
    // the operator pinned, never a caller's --store.
    let store = match &host {
        Some(h) => h
            .authority_store(a.opt("--store").map(PathBuf::from).as_deref())
            .unwrap_or_else(|e| refuse("unregistered", &e, 4)),
        None => PathBuf::from(a.req("--store")),
    };
    let linux = host.map(|h| h.linux);
    let cfg = SubmitConfig {
        journal: PathBuf::from(a.req("--journal")),
        registry,
        epoch: EpochSource::LoopStore { store, scope: sc },
        expected_epoch: expected,
        workspace: PathBuf::from(a.opt("--workspace").unwrap_or_else(|| ".".into())),
        state_dir: state_dir(a),
        budget: ResourceVector {
            model_micro_usd: a.num("--budget-micro", 1_000_000),
            exec_ms: a.num("--budget-exec-ms", 3_600_000),
            verify_ms: a.num("--budget-verify-ms", 3_600_000),
            retries: a.num("--budget-retries", 1_000),
        },
        grants,
        linux,
        protected_host,
        observer,
        pre_launch_hook: None,
        fault_hook: None,
    };
    match axon_fabric::submit(&text, &cfg) {
        Ok(s) => {
            // The receipt is final here. Sign it only if the workload that
            // produced it could not have read the signing key.
            // PSV-7: an EXECUTION on the protected profile is attested too, in
            // its own domain, so the loop need not take its backend on trust.
            let execution_attestation = issuer.as_ref().and_then(|(id, key)| {
                let req: axon_loop_contracts::ComputeRequest =
                    axon_loop_contracts::parse(&text).ok()?;
                axon_fabric::signing::execution_attestation_decision(
                    &req,
                    &s.receipt,
                    s.replayed,
                    s.ran_under.as_ref(),
                )
                .ok()?;
                let doc = axon_loop_contracts::attestation::execution_document(&req, &s.receipt)
                    .unwrap_or_else(|e| refuse("io", &e.to_string(), 2));
                Some(
                    axon_loop_contracts::attestation::sign_document(
                        key,
                        axon_loop_contracts::attestation::EXECUTION_DOMAIN,
                        id,
                        &doc,
                    )
                    .unwrap_or_else(|e| refuse("io", &e, 2)),
                )
            });
            let (attestation, withheld) = match issuer {
                None => (None, None),
                Some((id, key)) => {
                    let req: axon_loop_contracts::ComputeRequest =
                        axon_loop_contracts::parse(&text)
                            .unwrap_or_else(|e| refuse("malformed", &e.to_string(), 3));
                    match axon_fabric::signing::attestation_decision(
                        &req,
                        s.replayed,
                        s.ran_under.as_ref(),
                    ) {
                        Ok(()) => {
                            let att = axon_loop_contracts::attestation::sign(
                                &key,
                                &id,
                                &req,
                                &s.receipt,
                                // Fabric's own clock: the trusted anchor EVL
                                // compares with the plan's freeze.
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0),
                            )
                            .unwrap_or_else(|e| refuse("io", &e, 2));
                            (Some(att), None)
                        }
                        Err(why) => (None, Some(why)),
                    }
                }
            };
            let mut out = json!({
                "schema": "axon-fabric-submit/1",
                "receipt": s.receipt,
                "check_report": s.check_report,
                "replayed": s.replayed,
                "backend": s.backend,
                "reason": s.reason,
                "receipt_attestation": attestation,
                "attestation_withheld": withheld,
            });
            // B2: only a PROTECTED verdict carries its axon-psv-evidence/1
            // bundle; every other output keeps its bytes.
            if let Some(b) = s.psv_evidence {
                out["psv_evidence"] = b;
            }
            if let Some(a) = execution_attestation {
                out["execution_attestation"] = a;
            }
            println!("{out}")
        }
        Err(e) => refuse(e.kind(), &e.to_string(), e.exit_code()),
    }
}

/// `--state DIR`, default `<journal>.state` beside the journal.
fn state_dir(a: &Args) -> PathBuf {
    a.opt("--state").map(PathBuf::from).unwrap_or_else(|| {
        let mut s = PathBuf::from(a.req("--journal")).into_os_string();
        s.push(".state");
        PathBuf::from(s)
    })
}

/// `axon-fabric workspace-import --state DIR --tenant T --root DIR`: import a tree into
/// the WorkspaceVersion store and print its reference (and omissions).
fn workspace_import(a: &Args) {
    let tenant = axon_loop_contracts::TenantId::new(a.req("--tenant"))
        .unwrap_or_else(|e| refuse("usage", &format!("--tenant: {e}"), 2));
    let store =
        axon_fabric::workspace::WorkspaceStore::open(&PathBuf::from(a.req("--state")), &tenant)
            .unwrap_or_else(|e| refuse("workspace", &e.to_string(), 2));
    let tree = axon_fabric::workspace::WorkspaceTree::import_dir(
        &PathBuf::from(a.req("--root")),
        &axon_fabric::workspace::Quota::default(),
    )
    .unwrap_or_else(|e| refuse(e.class(), &e.to_string(), 3));
    let r = store
        .publish(&tree)
        .unwrap_or_else(|e| refuse("workspace", &e.to_string(), 2));
    println!(
        "{}",
        json!({"schema": "axon-fabric-workspace-import/1", "workspace_version_ref": r,
               "entries": tree.entries().len(), "omissions": tree.omissions()})
    );
}

/// G03-r22-authority-intersection + D1: the caller must present the authority
/// that submitted `op` — a grant the registry resolves for the principal,
/// equal to the op's recorded `principal|grant`. The registry is the
/// operator's on a protected host ([`grant_registry`]); in development it must
/// be the very registry (by sha256) that authorized the op. Every check runs
/// BEFORE any write: the grant is resolved before the journal is touched, the
/// op is looked up in a replay that writes nothing, and only then is the
/// authorized op's scope — and no other — reconciled.
fn authorized(a: &Args) -> (Journal, OperationId) {
    let host = protected_host(a);
    let grants = grant_registry(a, host.as_ref());
    let (principal, grant) = (a.req("--principal"), a.req("--grant-ref"));
    grants
        .resolve(&grant, &principal)
        .unwrap_or_else(|e| refuse("unauthorized", &e, 7));
    let op = OperationId::new(a.req("--op")).unwrap_or_else(|e| refuse("usage", &e.to_string(), 2));
    let j = Journal::open_unreconciled(PathBuf::from(a.req("--journal")))
        .unwrap_or_else(|e| refuse("journal", &e.to_string(), 2))
        .unwrap_or_else(|| refuse("unknown_op", &format!("no operation {op}"), 5));
    let Some(v) = j.view(&op) else {
        refuse("unknown_op", &format!("no operation {op}"), 5)
    };
    if v.intent.authority_ref != format!("{principal}|{grant}") {
        refuse(
            "unauthorized",
            &format!("operation {op} was not submitted under {principal}|{grant}"),
            7,
        )
    }
    if host.is_none() {
        let recorded = v.intent.config["grant"]["registry_sha256"].as_str();
        if recorded != Some(grants.sha256()) {
            refuse(
                "unauthorized",
                &format!(
                    "operation {op} was authorized under grant registry sha256 {}; this one is \
                     sha256 {}",
                    recorded.unwrap_or("(none recorded)"),
                    grants.sha256()
                ),
                7,
            )
        }
    }
    j.reconcile_scope(&v.intent.scope)
        .unwrap_or_else(|e| refuse("journal", &e.to_string(), 2));
    (j, op)
}

fn status(a: &Args) {
    let (j, op) = authorized(a);
    print_status(&j, &op);
}

fn print_status(j: &Journal, op: &OperationId) {
    match j.view(op) {
        None => refuse("unknown_op", &format!("no operation {op}"), 5),
        Some(v) => println!(
            "{}",
            json!({
                "schema": "axon-fabric-status/1",
                "operation_id": op,
                "state": v.state,
                "launched": v.launched,
                "billing": v.billing,
                "reason": v.reason,
                "outcome": v.outcome,
                "scope_usage": j.scope_usage(&v.intent.scope).ok().map(|u| json!({
                    "held": u.held, "liability": u.liability, "charged": u.charged,
                    "ceiling": u.ceiling,
                })),
            })
        ),
    }
}

/// Cancel. A never-launched op is RELEASED; a launched one keeps its whole
/// reservation as unresolved liability (cancel acknowledgement is not
/// cleanup, and no cost evidence exists).
fn cancel(a: &Args) {
    let (j, op) = authorized(a);
    let reason = a.req("--reason");
    let Some(v) = j.view(&op) else {
        refuse("unknown_op", &format!("no operation {op}"), 5)
    };
    let billing = v.launched.then_some(axon_fabric::Billing::Unknown);
    match j.cancel(&op, &reason, billing) {
        Ok(()) => print_status(&j, &op),
        Err(e) => refuse("journal", &e.to_string(), 5),
    }
}
