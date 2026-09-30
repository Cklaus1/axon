//! D1 — the grant registry is the authority source, so WHO supplies it is the
//! trust boundary. Before this fix every `axon-fabric` route took it from the
//! caller (`--grant-registry`), protected host included, so a registry the
//! caller wrote authorized `submit`, `status` and `cancel` for any
//! principal|grant, and decided the guest effect policy of a protected launch.
//! And `status`/`cancel` reconciled every scope of the journal before asking
//! who the caller was.
//!
//! Now:
//! * on a PROTECTED host the registry is the operator's, pinned by the host
//!   config (path + sha256, grant files operator-owned); a caller
//!   `--grant-registry` is REFUSED on every route before any effect, and the
//!   library refuses a registry that is not the pin (whoever built the config);
//! * in DEVELOPMENT the caller names it, its sha256 is recorded in the op's
//!   intent, and `status`/`cancel` accept only the registry that authorized it;
//! * `status`/`cancel` authorize BEFORE any journal write, and reconcile only
//!   the authorized op's scope.
//!
//! Every refusal asserts its REASON and the absence of effect.

mod common;
use common::*;

use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use axon_fabric::protected_host::ProtectedHost;

const GUEST: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

const GRANT_DENY: &str = "\
profile = \"restricted\"
[grant]
max_label = \"internal\"
[grant.budget]
cost_micro = 1000
";

/// `YYYY-MM-DDTHH:MM:SSZ` for Unix seconds `t` (civil-from-days).
fn utc(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn fabric() -> Command {
    Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
}

fn run(c: &mut Command) -> (i32, String) {
    let o = c.output().unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

/// A caller-written registry in a directory the caller owns, binding
/// `grant_ref` → `body` for `principal`.
fn forged_registry(env: &Env, principal: &str, grant_ref: &str, body: &str) -> PathBuf {
    let d = env.dir.path().join("attacker");
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("grants.json");
    write_grant_registry(&p, &[(grant_ref, principal, body)]);
    p
}

/// A complete protected host (test trust, stand-in launcher that records the
/// delivered guest policy), fresh x1-PASS qualification against the REAL clock
/// (the CLI's), and an operator grant registry under `host/grants/`.
struct Host {
    env: Env,
    root: PathBuf,
    candidate: axon_loop_contracts::Acf1Ref,
    _custodian: TestCustodian,
}

impl Host {
    fn new() -> Host {
        let env = Env::new();
        let root = env.dir.path().join("host");
        for d in ["dist", "keys", "grants", "runs"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // The service's own private leaf (A56).
        std::fs::set_permissions(root.join("runs"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        let issuer = Issuer::generate();
        // A: the launch goes through the (test-trust) privileged helper,
        // which re-verifies the artifacts and engine the manifest pins.
        let inputs = helper_inputs(&root);
        std::fs::write(
            root.join("manifest.json"),
            inputs.pin_manifest(&full_lx_manifest(GUEST)),
        )
        .unwrap();
        let mut ev = good_evidence(&sha256_file(&root.join("manifest.json")));
        inputs.pin_evidence(&mut ev);
        ev["assertions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": axon_fabric::backend::X1_GUEST_POLICY_CHANNEL, "status": "PASS"}));
        ev["counts"] = json!({"total": 4, "PASS": 4, "FAIL": 0, "BLOCKED": 0});
        ev["start"] = json!(utc(now() - 3630));
        ev["end"] = json!(utc(now() - 3600));
        qualified_linux_cfg(&root, &issuer, &ev);
        let launcher = stand_in_launcher(&env, 0, true, true, 0);
        copy_executable(&launcher, root.join("launcher.sh"), 0o755);
        // The helper binary and the observer's interpreter, copied in: a host
        // whose ownership is walked from its own base pins nothing outside it.
        copy_executable(helper_pin().path, root.join("protected-launcher"), 0o755);
        copy_executable("/bin/bash", root.join("bash"), 0o755);
        write_helper_config(
            &root,
            &inputs,
            &root.join("launcher.sh"),
            &root.join("manifest.json"),
            &root.join("runs"),
            "protected-launcher.json",
        );
        // The operator suite `acc` (the protected profile runs only an
        // operator-suite check, PSV-6 / A54) and the candidate, in the store
        // the CLI reads (`<journal>.state`).
        let candidate = psv_suite(&env);
        std::fs::copy(&env.registry, root.join("registry.json")).unwrap();
        let mut state = env.journal.clone().into_os_string();
        state.push(".state");
        axon_fabric::workspace::WorkspaceStore::open(Path::new(&state), &scope().tenant_id)
            .unwrap()
            .import_dir(&env.ws, &axon_fabric::workspace::Quota::default())
            .unwrap();
        // The preflight observer: a protected host launches nothing without
        // one. Its key is in the observer root beside the issuers.
        // The privileged helper verifies the same observation under its own
        // configured root (amendment 50).
        let key = observer_key(
            &root,
            "obs",
            &[&root.join("observer"), &observer_root(&root)],
        );
        copy_executable(
            observer_script(&root, "", &key, "observer"),
            root.join("observer.sh"),
            0o755,
        );
        let rng = ring::rand::SystemRandom::new();
        let pk8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        std::fs::write(root.join("keys/attest.pk8"), pk8.as_ref()).unwrap();
        std::fs::set_permissions(
            root.join("keys/attest.pk8"),
            std::fs::Permissions::from_mode(0o400),
        )
        .unwrap();
        // The OPERATOR's grants: principal:test may run deny-all only.
        write_grant_registry(
            &root.join("grants/grants.json"),
            &[
                ("grant:test", PRINCIPAL, GRANT_DENY),
                ("grant:x", PRINCIPAL, GRANT_DENY),
            ],
        );
        // Amendment 50: the custodian that issues the nonce and that the
        // helper spends it through.
        let custodian = start_custodian(&root);
        let h = Host {
            env,
            root,
            candidate,
            _custodian: custodian,
        };
        h.write_config();
        h
    }
    fn p(&self, n: &str) -> PathBuf {
        self.root.join(n)
    }
    fn config(&self) -> PathBuf {
        self.p("protected-host.json")
    }
    /// The conforming config, pinning the operator grant registry.
    fn write_config(&self) {
        self.write_config_with(true)
    }
    fn write_config_with(&self, grant_registry: bool) {
        let pin = |n: &str| json!({"path": self.p(n), "sha256": sha256_file(&self.p(n))});
        let mut v = json!({
            "schema": "axon-protected-host/1",
            "launcher": pin("launcher.sh"),
            "privileged_launcher": {"path": self.p("protected-launcher"),
                                    "sha256": sha256_file(&self.p("protected-launcher")),
                                    "test_config": self.p("protected-launcher.json")},
            "profile_manifest": pin("manifest.json"),
            "artifacts_dir": self.p("dist"),
            "qualification": {"record": self.p("evidence.json"), "signature": null,
                              "waivers": null, "max_age_s": 2_592_000},
            "suite_registry": pin("registry.json"),
            "signer": {"issuer_ref": "verifier:fabric",
                       "public_key": axon_loop_contracts::attestation::public_key_of(
                           &std::fs::read(self.p("keys/attest.pk8")).unwrap()).unwrap(),
                       "key_path": self.p("keys/attest.pk8")},
            "out_root": self.p("runs"),
            "observer": {"command": pin("observer.sh"), "interpreter": pin("bash"),
                         "custodian": {"socket": custodian_socket(&self.root),
                                       "uid": unsafe { libc::geteuid() }},
                         "max_age_s": 300},
        });
        if grant_registry {
            v["grant_registry"] = pin("grants/grants.json");
        }
        std::fs::write(self.config(), v.to_string()).unwrap();
    }
    fn host_args<'a>(&self, c: &'a mut Command) -> &'a mut Command {
        c.arg("--protected-host-config")
            .arg(self.config())
            .arg("--protected-host-issuers")
            .arg(self.p("trusted_issuers"))
    }
    /// An operator-suite check: the only job the protected profile runs
    /// (PSV-6, A54).
    fn linux_request(&self, op: &str, grant: &str) -> Value {
        let mut r = request(&self.env, op, "t_psv_ok");
        as_protected_check(&mut r, &self.candidate, GUEST);
        r["grant_ref"] = json!(grant);
        r
    }
    fn submit(&self, req: &Value, grant_registry: Option<&Path>) -> (i32, String) {
        let rp = self.env.dir.path().join("req.json");
        std::fs::write(&rp, req.to_string()).unwrap();
        let mut c = fabric();
        c.arg("submit").arg("--request").arg(&rp);
        self.host_args(&mut c);
        c.arg("--journal")
            .arg(&self.env.journal)
            .arg("--store")
            .arg(&self.env.store)
            .args(["--tenant", "tenant-t", "--family", "family-f"])
            .args(["--expected-epoch", "0"])
            .arg("--workspace")
            .arg(&self.env.ws);
        if let Some(g) = grant_registry {
            c.arg("--grant-registry").arg(g);
        }
        run(&mut c)
    }
}

/// An op recorded directly in the journal under `authority`.
fn record_op(
    env: &Env,
    id: &str,
    authority: &str,
    scope: axon_loop_contracts::Scope,
    launched: bool,
    registry: &Path,
) {
    let (j, _) = axon_fabric::Journal::open(&env.journal).unwrap();
    j.declare_budget(&scope, env.cfg(0).budget).unwrap();
    let op = axon_loop_contracts::OperationId::new(id).unwrap();
    j.begin(axon_fabric::Intent {
        op: op.clone(),
        task_id: axon_loop_contracts::TaskId::new("task-1").unwrap(),
        trial_id: axon_loop_contracts::TrialId::new("trial-1").unwrap(),
        attempt_id: axon_loop_contracts::AttemptId::new("attempt-1").unwrap(),
        input_digest: axon_loop_contracts::Ref::new(format!("cl22:{}", "1".repeat(64))).unwrap(),
        // As `submit` records it: which registry authorized the op.
        config: json!({"grant": {"registry_sha256": sha256_file(registry)}}),
        authority_ref: authority.to_string(),
        authority_epoch: axon_loop_contracts::AuthorityEpoch::new(0).unwrap(),
        scope,
        reservation: axon_fabric::ResourceVector {
            model_micro_usd: 10,
            exec_ms: 10,
            verify_ms: 0,
            retries: 0,
        },
        expected_version: 0,
    })
    .unwrap();
    j.reserve(&op).unwrap();
    if launched {
        j.mark_launched(&op).unwrap();
    }
}

fn status_cancel(
    env: &Env,
    verb: &str,
    op: &str,
    principal: &str,
    grant: &str,
    grant_registry: Option<&Path>,
    host: Option<&Host>,
) -> (i32, String) {
    let mut c = fabric();
    c.arg(verb)
        .arg("--journal")
        .arg(&env.journal)
        .args(["--op", op]);
    if verb == "cancel" {
        c.args(["--reason", "forged"]);
    }
    if let Some(g) = grant_registry {
        c.arg("--grant-registry").arg(g);
    }
    c.args(["--principal", principal, "--grant-ref", grant]);
    if let Some(h) = host {
        h.host_args(&mut c);
    }
    run(&mut c)
}

fn refusal(out: &str) -> Value {
    serde_json::from_str(out.trim()).unwrap_or(Value::Null)
}

/// Exit `code`, an `axon-fabric-refusal/1` of `kind`, whose reason contains
/// `why`.
fn assert_refused(what: &str, (code, out): &(i32, String), want: i32, kind: &str, why: &str) {
    let r = refusal(out);
    assert_eq!(*code, want, "{what}: {out}");
    assert_eq!(r["schema"], "axon-fabric-refusal/1", "{what}: {out}");
    assert_eq!(r["kind"], kind, "{what}: {out}");
    assert!(
        r["reason"].as_str().unwrap_or("").contains(why),
        "{what}: expected {why:?} in {out}"
    );
}

const CALLER_REGISTRY_REFUSED: &str = "--grant-registry is not accepted on a protected host";

fn launches(h: &Host) -> usize {
    std::fs::read_to_string(h.p("runs/launches"))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// (a) DEVELOPMENT: the caller names the registry, so a registry it wrote
/// could claim any principal|grant. The op records the sha256 of the registry
/// that authorized it; `status`/`cancel` accept only that registry. Nothing is
/// disclosed or written for a forged one.
#[test]
fn development_status_and_cancel_accept_only_the_registry_that_authorized_the_op() {
    let env = Env::new();
    let reg_sha = sha256_file(&env.grant_registry);
    // A completed op, and a reserved one (the submit stops before launch):
    // both recorded by the REAL submit path.
    let done = axon_fabric::submit(&request(&env, "op-a", "t_ok").to_string(), &env.cfg(0));
    assert!(done.is_ok(), "{done:?}");
    let mut cfg = env.cfg(0);
    cfg.pre_launch_hook = Some(|_| panic!("stop before launch"));
    let r = std::panic::catch_unwind(|| {
        axon_fabric::submit(&request(&env, "op-a2", "t_ok").to_string(), &cfg)
    });
    assert!(r.is_err(), "the hook stops the submit before launch");
    // Auditable: each intent names the registry that authorized it.
    for op in ["op-a", "op-a2"] {
        let intent = env
            .journal_text()
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|v| v["kind"] == "intent" && v["intent"]["op"] == op)
            .unwrap();
        assert_eq!(
            intent["intent"]["config"]["grant"]["registry_sha256"], reg_sha,
            "{op}"
        );
    }

    // The attacker's registry binds the same principal|grant to a grant of
    // its choosing.
    let forged = forged_registry(&env, PRINCIPAL, "grant:test", GRANT_OPEN);
    let before = std::fs::read(&env.journal).unwrap();
    let why = format!("was authorized under grant registry sha256 {reg_sha}");
    let st = status_cancel(
        &env,
        "status",
        "op-a",
        PRINCIPAL,
        "grant:test",
        Some(&forged),
        None,
    );
    assert_refused("forged status", &st, 7, "unauthorized", &why);
    assert!(!st.1.contains("scope_usage"), "status disclosed: {}", st.1);
    let ca = status_cancel(
        &env,
        "cancel",
        "op-a2",
        PRINCIPAL,
        "grant:test",
        Some(&forged),
        None,
    );
    assert_refused("forged cancel", &ca, 7, "unauthorized", &why);
    assert_eq!(
        std::fs::read(&env.journal).unwrap(),
        before,
        "a refusal wrote"
    );

    // Control: the registry that authorized the op still works.
    let (c, out) = status_cancel(
        &env,
        "status",
        "op-a",
        PRINCIPAL,
        "grant:test",
        Some(&env.grant_registry),
        None,
    );
    assert_eq!(c, 0, "{out}");
    assert_eq!(refusal(&out)["state"], "completed");
    let (c, out) = status_cancel(
        &env,
        "cancel",
        "op-a2",
        PRINCIPAL,
        "grant:test",
        Some(&env.grant_registry),
        None,
    );
    assert_eq!(c, 0, "{out}");
    assert_eq!(refusal(&out)["state"], "cancelled");
}

/// (b) PROTECTED host: a caller `--grant-registry` is refused on submit,
/// status and cancel — by name, before anything is read or written — and
/// authority is decided by the operator's pinned registry alone.
#[test]
fn a_protected_host_refuses_a_caller_grant_registry_on_every_route() {
    let h = Host::new();
    let op_reg = h.p("grants/grants.json");
    // An op under an authority the OPERATOR never granted, and one it did.
    record_op(
        &h.env,
        "op-b",
        "principal:intruder|grant:root",
        scope(),
        false,
        &op_reg,
    );
    record_op(
        &h.env,
        "op-ok",
        &format!("{PRINCIPAL}|grant:test"),
        scope(),
        false,
        &op_reg,
    );
    let forged = forged_registry(&h.env, "principal:intruder", "grant:root", GRANT_OPEN);
    let before = std::fs::read(&h.env.journal).unwrap();
    for verb in ["status", "cancel"] {
        let r = status_cancel(
            &h.env,
            verb,
            "op-b",
            "principal:intruder",
            "grant:root",
            Some(&forged),
            Some(&h),
        );
        if r.0 == 0 {
            panic!(
                "ATTACK: {verb}: a caller grant registry was honoured on a protected host: {}",
                r.1
            );
        }
        // Two layers keep the caller's registry out, each on its own: the
        // flag is refused by name (M273), and the protected arm resolves from
        // the operator's pinned registry whatever the caller names (M401).
        // Which one refuses is not the property (four-cell,
        // EQUIV_RECORD["M273"]).
        if r.0 == 2 {
            assert_refused(verb, &r, 2, "usage", CALLER_REGISTRY_REFUSED);
        } else {
            assert_refused(verb, &r, 7, "unauthorized", "not in the grant registry");
        }
        // Naming the OPERATOR's own file: refused by name, or (were the
        // by-name refusal gone) served from the pinned registry, which is
        // that same file. Either way the operator's registry decides. Asked
        // with `status` only, which writes nothing either way.
        let r = status_cancel(
            &h.env,
            "status",
            "op-ok",
            PRINCIPAL,
            "grant:test",
            Some(&op_reg),
            Some(&h),
        );
        if r.0 != 0 {
            assert_refused(verb, &r, 2, "usage", CALLER_REGISTRY_REFUSED);
        }
        // Without the flag, the operator's registry decides: not granted.
        let r = status_cancel(
            &h.env,
            verb,
            "op-b",
            "principal:intruder",
            "grant:root",
            None,
            Some(&h),
        );
        assert_refused(verb, &r, 7, "unauthorized", "not in the grant registry");
    }
    assert_eq!(
        std::fs::read(&h.env.journal).unwrap(),
        before,
        "a refusal wrote"
    );

    // Control: the operator-granted authority is served from the pinned registry.
    let (c, out) = status_cancel(
        &h.env,
        "status",
        "op-ok",
        PRINCIPAL,
        "grant:test",
        None,
        Some(&h),
    );
    assert_eq!(c, 0, "{out}");
    assert_eq!(refusal(&out)["state"], "reserved");

    // submit: refused before the request is even read…
    let mut c = fabric();
    c.args(["submit", "--request", "/nonexistent/request.json"]);
    h.host_args(&mut c).arg("--grant-registry").arg(&forged);
    assert_refused(
        "submit (unread request)",
        &run(&mut c),
        2,
        "usage",
        CALLER_REGISTRY_REFUSED,
    );
    // …and before the journal is opened or anything launched.
    let h = Host::new();
    let forged = forged_registry(&h.env, PRINCIPAL, "grant:root", GRANT_OPEN);
    let r = h.submit(&h.linux_request("op-b-submit", "grant:root"), Some(&forged));
    assert_refused("submit", &r, 2, "usage", CALLER_REGISTRY_REFUSED);
    assert!(!h.env.journal.exists(), "the journal was opened");
    assert_eq!(launches(&h), 0);
    let r = h.submit(&h.linux_request("op-b-submit", "grant:root"), None);
    assert_refused("submit", &r, 7, "unauthorized", "not in the grant registry");
    assert!(!h.env.journal.exists(), "the journal was opened");
}

/// (c) PROTECTED host: the guest effect policy is derived from the grant, so
/// it comes from the operator's registry — never the caller's. The operator's
/// `grant:x` is deny-all; the caller's says everything.
#[test]
fn on_a_protected_host_the_guest_policy_comes_from_the_operators_registry() {
    let h = Host::new();
    let forged = forged_registry(&h.env, PRINCIPAL, "grant:x", GRANT_OPEN);
    let r = h.submit(&h.linux_request("op-c", "grant:x"), Some(&forged));
    assert_refused("caller registry", &r, 2, "usage", CALLER_REGISTRY_REFUSED);
    assert_eq!(launches(&h), 0);
    assert!(!h.p("runs/delivered-policy.json").exists());

    let (c, out) = h.submit(&h.linux_request("op-c", "grant:x"), None);
    assert_eq!(c, 0, "{out}");
    assert_eq!(refusal(&out)["receipt"]["status"], "completed", "{out}");
    assert_eq!(launches(&h), 1);
    let pol: Value =
        serde_json::from_str(&std::fs::read_to_string(h.p("runs/delivered-policy.json")).unwrap())
            .unwrap();
    assert_eq!(
        pol["allowed_effects"],
        json!([]),
        "the operator's deny-all: {pol}"
    );
    let intent = h
        .env
        .journal_text()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["kind"] == "intent")
        .unwrap();
    assert_eq!(
        intent["intent"]["config"]["grant"]["registry_sha256"],
        sha256_file(&h.p("grants/grants.json"))
    );
}

/// The LIBRARY boundary, independent of the CLI: with a protected host
/// configured, a registry that is not the operator's pin — or a host that pins
/// none — authorizes nothing, and so decides no guest policy.
#[test]
fn submit_refuses_any_registry_but_the_protected_hosts_pin() {
    let h = Host::new();
    let forged = forged_registry(&h.env, PRINCIPAL, "grant:x", GRANT_OPEN);
    let ph = ProtectedHost::for_test(&h.config(), None, {
        let mut t = axon_fabric::backend::QualificationTrust::for_manifest(&h.p("manifest.json"));
        t.issuers_dir = h.p("trusted_issuers");
        t
    })
    .unwrap();
    let pin = ph.grant_registry.as_ref().unwrap().1.clone();
    let mut cfg = h.env.cfg(0);
    cfg.linux = Some(ph.linux.clone());
    cfg.observer = ph.observer.clone();
    cfg.grants = axon_fabric::GrantRegistry::load(&forged).unwrap();
    let identity = |pin: Option<String>| axon_fabric::psv::HostIdentity {
        config_sha256: ph.config_sha256.clone(),
        suite_registry_sha256: ph.suite_registry_sha256.clone(),
        grant_registry_sha256: pin,
    };
    let req = h.linux_request("op-lib", "grant:x").to_string();

    cfg.protected_host = Some(identity(Some(pin.clone())));
    let e = axon_fabric::submit(&req, &cfg).unwrap_err();
    assert_eq!(e.kind(), "unauthorized", "{e}");
    assert!(
        e.to_string().contains(&format!(
            "the grant registry is the operator's (sha256 {pin})"
        )),
        "{e}"
    );
    cfg.protected_host = Some(identity(None));
    let e = axon_fabric::submit(&req, &cfg).unwrap_err();
    assert_eq!(e.kind(), "unauthorized", "{e}");
    assert!(e.to_string().contains("pins no grant_registry"), "{e}");
    assert!(!h.env.journal.exists(), "the journal was opened");
    assert_eq!(launches(&h), 0);
    assert!(!h.p("runs/delivered-policy.json").exists());

    // Control: the operator's registry, at its pin, is admitted.
    cfg.grants = ph.grants().unwrap();
    cfg.protected_host = Some(identity(Some(pin)));
    let s = axon_fabric::submit(&req, &cfg).unwrap();
    assert_eq!(s.backend, Some("linux-microvm-protected"), "{:?}", s.reason);
}

/// A host config that pins NO grant registry authorizes nothing, on every
/// route.
#[test]
fn a_protected_host_without_a_pinned_grant_registry_authorizes_nothing() {
    let h = Host::new();
    record_op(
        &h.env,
        "op-n",
        &format!("{PRINCIPAL}|grant:test"),
        scope(),
        false,
        &h.p("grants/grants.json"),
    );
    h.write_config_with(false);
    let before = std::fs::read(&h.env.journal).unwrap();
    for verb in ["status", "cancel"] {
        let r = status_cancel(
            &h.env,
            verb,
            "op-n",
            PRINCIPAL,
            "grant:test",
            None,
            Some(&h),
        );
        assert_refused(verb, &r, 7, "unauthorized", "pins no grant_registry");
    }
    assert_eq!(std::fs::read(&h.env.journal).unwrap(), before);
    let h = Host::new();
    h.write_config_with(false);
    let r = h.submit(&h.linux_request("op-n", "grant:test"), None);
    assert_refused("submit", &r, 7, "unauthorized", "pins no grant_registry");
    assert!(!h.env.journal.exists());
}

/// The pinned registry: its bytes at load, and again at every use.
#[test]
fn the_operator_grant_registry_is_used_only_at_its_pin() {
    let h = Host::new();
    let trust = || {
        let mut t = axon_fabric::backend::QualificationTrust::for_manifest(&h.p("manifest.json"));
        t.issuers_dir = h.p("trusted_issuers");
        t
    };
    let ph = ProtectedHost::for_test(&h.config(), None, trust()).unwrap();
    assert!(ph.grants().is_ok());
    // Changed after the host loaded: every later use refuses.
    write_grant_registry(
        &h.p("grants/grants.json"),
        &[("grant:x", PRINCIPAL, GRANT_OPEN)],
    );
    let e = ph.grants().unwrap_err();
    assert!(e.contains("not its pin"), "{e}");
    // …and so does loading the host.
    let e = ProtectedHost::for_test(&h.config(), None, trust()).unwrap_err();
    assert!(
        e.contains("grant_registry") && e.contains("not its pin"),
        "{e}"
    );
}

/// Operator ownership of the pinned registry AND every grant file it names
/// (a caller who can write a grant file, or drop an `.approval` beside it,
/// holds authority). Needs root to create root-owned fixtures.
#[test]
fn every_grant_file_must_be_operator_owned() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("skipped: needs root to create root-owned fixtures");
        return;
    }
    let h = Host::new();
    let base = h.env.dir.path();
    for d in [
        base.to_path_buf(),
        h.root.clone(),
        h.p("grants"),
        h.p("dist"),
        h.p("keys"),
    ] {
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for e in std::fs::read_dir(&h.root)
        .unwrap()
        .chain(std::fs::read_dir(h.p("grants")).unwrap())
    {
        let p = e.unwrap().path();
        if p.is_file() && p != h.p("keys/attest.pk8") {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
    }
    let load = || {
        let mut t = axon_fabric::backend::QualificationTrust::for_manifest(&h.p("manifest.json"));
        t.issuers_dir = h.p("trusted_issuers");
        ProtectedHost::for_test(&h.config(), Some(base), t)
    };
    load().expect("root-owned, 0755/0644");
    let gfile = h.p("grants/grant_x.axgrant");
    std::os::unix::fs::chown(&gfile, Some(1000), None).unwrap();
    let e = load().unwrap_err();
    assert!(
        e.contains("not root") && e.contains("grant_x.axgrant"),
        "{e}"
    );
    std::os::unix::fs::chown(&gfile, Some(0), None).unwrap();
    std::fs::set_permissions(h.p("grants"), std::fs::Permissions::from_mode(0o777)).unwrap();
    let e = load().unwrap_err();
    assert!(e.contains("writable"), "{e}");
    std::fs::set_permissions(h.p("grants"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::chown(h.p("grants/grants.json"), Some(1000), None).unwrap();
    assert!(load().unwrap_err().contains("not root"));
}

/// `status`/`cancel` authorize BEFORE any journal write, and then reconcile
/// only the authorized op's scope. Another tenant's launched-but-unfinished
/// op is left exactly as recorded (a full open still reconciles it).
#[test]
fn status_and_cancel_authorize_before_any_write_and_reconcile_only_their_scope() {
    let env = Env::new();
    let other = axon_fabric::submit::scope("tenant-other", "family-other").unwrap();
    let reg = env.grant_registry.clone();
    record_op(
        &env,
        "op-d",
        &format!("{PRINCIPAL}|grant:test"),
        scope(),
        false,
        &reg,
    );
    record_op(
        &env,
        "op-victim",
        "principal:victim|grant:v",
        other.clone(),
        true,
        &reg,
    );
    let before = std::fs::read(&env.journal).unwrap();
    for verb in ["status", "cancel"] {
        let r = status_cancel(
            &env,
            verb,
            "op-d",
            "principal:intruder",
            "grant:test",
            Some(&reg),
            None,
        );
        assert_refused(verb, &r, 7, "unauthorized", "bound to principal");
        let r = status_cancel(
            &env,
            verb,
            "op-d",
            PRINCIPAL,
            "grant:open",
            Some(&reg),
            None,
        );
        assert_refused(verb, &r, 7, "unauthorized", "was not submitted under");
    }
    assert_eq!(
        std::fs::read(&env.journal).unwrap(),
        before,
        "an unauthorized call wrote"
    );
    // A journal that does not exist is not created by an unauthorized call.
    let missing = env.dir.path().join("none.journal");
    let o = fabric()
        .args(["status", "--journal"])
        .arg(&missing)
        .args(["--op", "op-d", "--grant-registry"])
        .arg(&reg)
        .args(["--principal", PRINCIPAL, "--grant-ref", "grant:test"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(5));
    assert!(!missing.exists(), "status created a journal");

    // Authorized: served, and only its own scope reconciled.
    let (c, out) = status_cancel(
        &env,
        "status",
        "op-d",
        PRINCIPAL,
        "grant:test",
        Some(&reg),
        None,
    );
    assert_eq!(c, 0, "{out}");
    assert!(
        !env.journal_text().contains("outcome_unknown"),
        "another scope's op was reconciled by an authorized status"
    );
    let (j, rep) = axon_fabric::Journal::open(&env.journal).unwrap();
    assert_eq!(
        rep.reconciled_unknown,
        vec![axon_loop_contracts::OperationId::new("op-victim").unwrap()]
    );
    drop(j);
}

/// A journal replayed for authorization has not been recovered; it refuses
/// every write until its scope is reconciled (a torn tail is never appended
/// after).
#[test]
fn an_unreconciled_journal_refuses_writes_over_a_torn_tail() {
    let env = Env::new();
    record_op(
        &env,
        "op-t",
        &format!("{PRINCIPAL}|grant:test"),
        scope(),
        false,
        &env.grant_registry,
    );
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&env.journal)
            .unwrap();
        f.write_all(b"{\"seq\":99,\"kind\":\"tor").unwrap();
    }
    let before = std::fs::read(&env.journal).unwrap();
    let j = axon_fabric::Journal::open_unreconciled(&env.journal)
        .unwrap()
        .unwrap();
    let op = axon_loop_contracts::OperationId::new("op-t").unwrap();
    let e = j.cancel(&op, "x", None).unwrap_err();
    assert!(e.to_string().contains("unrecovered torn tail"), "{e}");
    assert_eq!(
        std::fs::read(&env.journal).unwrap(),
        before,
        "a write landed"
    );
    j.reconcile_scope(&scope()).unwrap();
    j.cancel(&op, "x", None).unwrap();
    assert_eq!(j.view(&op).unwrap().state, axon_fabric::OpState::Cancelled);
}

/// Spec §2 rule 1: the protected signer key is mode 0400. On a host where
/// everything else authorizes the run (a pinned operator grant registry), an
/// owner-WRITABLE key (0600) must still refuse before anything launches: the
/// service that holds the key must not be able to replace it.
#[test]
fn an_owner_writable_signer_key_signs_nothing() {
    let h = Host::new();
    // Control: at 0400 the same request completes.
    let (c, out) = h.submit(&h.linux_request("op-k0", "grant:x"), None);
    assert_eq!(c, 0, "{out}");
    assert_eq!(launches(&h), 1);
    std::fs::set_permissions(
        h.p("keys/attest.pk8"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let r = h.submit(&h.linux_request("op-k1", "grant:x"), None);
    assert!(
        r.0 != 0 && launches(&h) == 1,
        "ATTACK: an owner-writable (0600) signing key signed a protected run: {}",
        r.1
    );
    assert_refused("0600 key", &r, 4, "unregistered", "writable by no one");
}

/// The signer key is opened ONCE, never through a symlink (C9 round 2,
/// harness): the directory the operator-ownership check walks (M146) is the
/// directory whose file is read. A `key_path` that is a symlink to the same
/// 0400 key held elsewhere passes every other check (the key file is the
/// service's, 0400, and derives its pin), so only `O_NOFOLLOW` refuses it.
/// Control: the key in place signs.
#[test]
fn a_symlinked_signer_key_signs_nothing() {
    let h = Host::new();
    let (c, out) = h.submit(&h.linux_request("op-s0", "grant:x"), None);
    assert_eq!(c, 0, "{out}");
    assert_eq!(launches(&h), 1);
    let key = h.p("keys/attest.pk8");
    let away = h.p("elsewhere/attest.pk8");
    std::fs::create_dir_all(away.parent().unwrap()).unwrap();
    std::fs::rename(&key, &away).unwrap();
    std::os::unix::fs::symlink(&away, &key).unwrap();
    let r = h.submit(&h.linux_request("op-s1", "grant:x"), None);
    assert!(
        r.0 != 0 && launches(&h) == 1,
        "ATTACK: a signer key reached through a symlink out of the operator-owned key \
         directory was accepted and a protected run launched: {}",
        r.1
    );
    assert_refused("symlinked key", &r, 4, "unregistered", "a symlink");
}
