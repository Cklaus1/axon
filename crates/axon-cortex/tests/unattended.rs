#![cfg(feature = "unattended")]
//! NH-R1 `cortex repair --unattended` (`tasks/unattended-auth-spec.md` §7):
//! S5 replay, S6 out of scope, S8 no self-authority, S12 acceptance is not
//! approval, S13 TOCTOU. Every envelope here is minted in-process with the
//! SAME functions the runner verifies with (`axon_cortex::auth`), signed by a
//! test operator key; no model is called (the generator's endpoint is a
//! closed local port, so a run that gets past consume fails at its first
//! proposal, never earlier).

mod common;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;

use std::path::{Path, PathBuf};
use std::time::Duration;

use axon_cortex::auth;
use axon_cortex::fence::{caps_of_file, Fence, ScopePins};
use axon_os::repair_envelope::{
    self as env, Budget, Capabilities, Check, Generator, MintKey, RepairEnvelope, Scopes, Task,
};
use axon_os::run_ledger as rl;

const OP_KEY: &[u8] = b"nh-r1 cortex unattended test operator key";
const WALL: Duration = Duration::from_secs(60);

fn axon_bin() -> PathBuf {
    script_spawn::workspace_bin(
        "AXON_BIN",
        &[
            "build",
            "-p",
            "axon-core",
            "--no-default-features",
            "--bin",
            "axon",
        ],
        "axon",
    )
}

fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn sha(b: &[u8]) -> String {
    axon_cortex::label::sha256_hex(b)
}

/// A run's directories: the workspace (the broken fixture), and OUTSIDE it the
/// operator's verify key, spent store and envelope.
struct Setup {
    root: PathBuf,
    ws: PathBuf,
    key: PathBuf,
    store: PathBuf,
    /// R6 §4: the run ledger and the run-record seal key file.
    ledger: PathBuf,
    seal: PathBuf,
    envelope: PathBuf,
    axon: PathBuf,
    /// Security review F1: the host opt-in file's content, copied to
    /// `/etc/axon/unattended-repair` inside the run's private mount namespace.
    opt_in: PathBuf,
}

fn private_write(path: &Path, bytes: &[u8]) {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    f.write_all(bytes).unwrap();
}

/// A private (0700) directory initialised as an empty spent store.
fn new_store(dir: &Path) {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(dir).unwrap();
    env::init_store(dir, euid()).unwrap();
}

fn setup(name: &str) -> Setup {
    let root = std::env::temp_dir().join(format!("cortex_unatt_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("ws")).unwrap();
    let ws = std::fs::canonicalize(root.join("ws")).unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/broken.ax");
    std::fs::copy(src, ws.join("broken.ax")).unwrap();
    let ops = root.join("ops");
    std::fs::create_dir_all(&ops).unwrap();
    let key = ops.join("verify.key");
    let vk = MintKey::derive(OP_KEY).unwrap().verify_key().to_hex();
    private_write(&key, format!("{vk}\n").as_bytes());
    let store = ops.join("spent");
    new_store(&store);
    let ledger = ops.join("ledger");
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&ledger)
            .unwrap();
    }
    rl::init_ledger(&ledger, euid()).unwrap();
    let seal = ops.join("run-record.key");
    let sk = rl::SealKey::derive(OP_KEY).unwrap().to_hex();
    private_write(&seal, format!("{sk}\n").as_bytes());
    let opt_in = ops.join("opt-in");
    std::fs::write(
        &opt_in,
        "decision: governance/decisions/nh-r1-test-enable.md\n",
    )
    .unwrap();
    Setup {
        envelope: ops.join("envelope.json"),
        root,
        ws,
        key,
        store,
        ledger,
        seal,
        axon: axon_bin(),
        opt_in,
    }
}

/// The envelope the minter would issue for `s` (the same functions the
/// runner calls), before any test's change.
fn envelope_for(s: &Setup) -> RepairEnvelope {
    envelope_pinned(s, ScopePins::default())
}

/// [`envelope_for`] with the scope `pins` signed in (E1).
fn envelope_pinned(s: &Setup, pins: ScopePins) -> RepairEnvelope {
    let file = s.ws.join("broken.ax");
    let caps = caps_of_file(&s.axon, &file, None, WALL).unwrap();
    let fence = Fence::from_pristine(&caps);
    let (rows, pinned) = auth::gate_rows(&fence, &caps, &pins, "ai");
    assert!(rows.iter().all(|r| r.admit), "{rows:?}");
    let fence_sha = pinned.unwrap().sha256();
    let ScopePins {
        fs_read,
        fs_write,
        net,
    } = pins;
    let imports = auth::imports_digest(&s.axon, &s.ws, "broken.ax", WALL).unwrap();
    let checker = sha(&std::fs::read(&s.axon).unwrap());
    let issued = now();
    RepairEnvelope {
        schema: env::ENVELOPE_SCHEMA.into(),
        envelope_id: env::new_id().unwrap(),
        spent_store_id: env::store_id(&s.store, euid()).unwrap(),
        run_ledger_id: rl::ledger_id(&s.ledger, euid()).unwrap(),
        issued_by: "op:test".into(),
        issued_unix: issued,
        expires_unix: issued + 600,
        task: Task {
            workspace: s.ws.to_string_lossy().into_owned(),
            file: "broken.ax".into(),
            symbol: "double".into(),
            pristine_sha256: sha(&std::fs::read(&file).unwrap()),
            imports_digest: imports.clone(),
        },
        write_prefixes: vec!["broken.ax".into()],
        scopes: Scopes {
            fs_read,
            fs_write,
            net,
        },
        capabilities: Capabilities {
            fence_sha256: fence_sha.clone(),
            target_grant: auth::target_grant(&s.axon, &s.ws, "broken.ax", "double", WALL).unwrap(),
        },
        check: Check {
            name: "hidden_completion".into(),
            digest: auth::check_digest("hidden_completion", &checker, None, &fence_sha, &imports),
        },
        budget: Budget {
            max_proposals: 1,
            deadline_ms: 60_000,
            check_timeout_ms: 30_000,
            candidates: 1,
            max_steps: 2,
        },
        generator: Generator {
            kind: "ai".into(),
            model: "m".into(),
            provider: "anthropic".into(),
            endpoint_host: "127.0.0.1".into(),
        },
        decision: env::DECISION.into(),
        sig: String::new(),
    }
}

/// Sign `e`, record it as issued in `s.store`, and write it to `s.envelope`.
fn mint(s: &Setup, e: &RepairEnvelope) -> String {
    let text = MintKey::derive(OP_KEY).unwrap().sign(e).unwrap();
    issue(s, e, text)
}

/// As `mint`, but signed without `MintKey::sign`'s intrinsic checks: the same
/// seed, domain and canonical form as axon-os, for an envelope the minter
/// refuses (a relative pin, security F4) that an attacker holding the key
/// could still sign.
fn mint_unvalidated(s: &Setup, e: &RepairEnvelope) -> String {
    use ring::signature::Ed25519KeyPair;
    let mut v = serde_json::to_value(e).unwrap();
    v.as_object_mut().unwrap().remove("sig");
    let mut msg = env::ENVELOPE_SIG_DOMAIN.as_bytes().to_vec();
    msg.push(0);
    msg.extend_from_slice(&axon_canonical::canonical_bytes(&v).unwrap());
    let seed = axon_attest::hmac_sha256(OP_KEY, env::ENVELOPE_SEED_DOMAIN.as_bytes());
    let kp = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
    let sig = format!("{}{}", env::SIG_PREFIX, hex::encode(kp.sign(&msg).as_ref()));
    v.as_object_mut()
        .unwrap()
        .insert("sig".into(), serde_json::Value::String(sig));
    let text = String::from_utf8(axon_canonical::canonical_bytes(&v).unwrap()).unwrap();
    issue(s, e, text)
}

fn issue(s: &Setup, e: &RepairEnvelope, text: String) -> String {
    env::issue(
        &s.store,
        euid(),
        &e.spent_store_id,
        &e.envelope_id,
        e.issued_unix,
    )
    .unwrap();
    let _ = std::fs::remove_file(&s.envelope);
    std::fs::write(&s.envelope, &text).unwrap();
    text
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// How the host opt-in file (security review F1) looks for one run: its
/// content file (`None`: no opt-in file), mode and owner, and the tmpfs
/// options of its directory `/etc/axon` (security re-review N2).
struct OptIn<'a> {
    src: Option<&'a Path>,
    mode: &'a str,
    owner: &'a str,
    dir: &'a str,
}

impl Setup {
    /// The opt-in a host that agreed has: owned by the running uid (root
    /// here), mode 0600, naming the decision record, in a root-owned 0755
    /// directory.
    fn opted_in(&self) -> OptIn<'_> {
        OptIn {
            src: Some(&self.opt_in),
            mode: "0600",
            owner: "0",
            dir: "mode=0755",
        }
    }
}

/// `cortex repair --unattended` over `s`, with `extra` arguments and `envs`
/// on top of a clean environment.
fn run_with(s: &Setup, axon: &Path, extra: &[&str], envs: &[(&str, &str)]) -> Out {
    run_full(s, axon, extra, envs, &s.opted_in(), None)
}

/// [`run_with`], with the host opt-in `opt` and the working directory `cwd`.
/// The run is in a private mount namespace whose `/etc/axon` is an empty
/// tmpfs holding only the opt-in file `opt` describes: the host's `/etc` is
/// never written or read (the freeze tests' pattern; needs root).
fn run_full(
    s: &Setup,
    axon: &Path,
    extra: &[&str],
    envs: &[(&str, &str)],
    opt: &OptIn,
    cwd: Option<&Path>,
) -> Out {
    run_unshared(s, axon, extra, envs, opt, cwd, None)
}

/// [`run_full`]; with `userns` set, inside a user namespace (`unshare --user
/// --map-root-user`) running the cortex binary copied to `userns` (the
/// namespace's root cannot reach a build directory under a 0750 home).
fn run_unshared(
    s: &Setup,
    axon: &Path,
    extra: &[&str],
    envs: &[(&str, &str)],
    opt: &OptIn,
    cwd: Option<&Path>,
    userns: Option<&Path>,
) -> Out {
    assert!(
        euid() == 0 && Path::new("/etc/axon").is_dir(),
        "setup: the unattended tests need root and an /etc/axon mount point (a private mount \
         namespace holds the host opt-in file; /etc is never written)"
    );
    let none = s.root.join("no-opt-in");
    let mut c = std::process::Command::new("unshare");
    if userns.is_some() {
        c.args(["--user", "--map-root-user"]);
    }
    c.args(["-m", "--propagation", "private", "sh", "-c"])
        .arg(
            "set -e; mount -t tmpfs -o \"$4\" tmpfs /etc/axon; o=/etc/axon/unattended-repair; \
             if [ -d \"$1\" ]; then mkdir -m 0700 \"$o\"; \
             elif [ -e \"$1\" ]; then cp \"$1\" \"$o\"; chown \"$3\" \"$o\"; chmod \"$2\" \"$o\"; fi; \
             shift 4; exec \"$@\"",
        )
        .arg("sh")
        .arg(opt.src.unwrap_or(&none))
        .args([opt.mode, opt.owner, opt.dir])
        .arg(userns.unwrap_or(Path::new(env!("CARGO_BIN_EXE_cortex"))));
    c.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("AXON_AI_PROVIDER", "anthropic")
        // https (security re-review 2 N1); nothing listens on port 9, so an
        // admitted run fails at its first proposal, never reaching a model.
        .env("AXON_AI_BASE_URL", "https://127.0.0.1:9")
        .env("AXON_AI_API_KEY", "test-not-a-key")
        .args(["repair", "--unattended", "--workspace"])
        .arg(&s.ws)
        .args(["--file", "broken.ax", "--symbol", "double"])
        .args([
            "--check",
            "hidden_completion",
            "--generator",
            "ai:m",
            "--axon",
        ])
        .arg(axon)
        .arg("--envelope")
        .arg(&s.envelope)
        .arg("--verify-key")
        .arg(&s.key)
        .arg("--spent-store")
        .arg(&s.store)
        .arg("--run-ledger")
        .arg(&s.ledger)
        .arg("--run-record-key")
        .arg(&s.seal)
        .arg("--json")
        .args(extra);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    for (k, v) in envs {
        c.env(k, v);
    }
    let o = c.output().expect("run cortex");
    Out {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

fn run(s: &Setup, extra: &[&str]) -> Out {
    run_with(s, &s.axon, extra, &[])
}

/// The spent store's records, sorted (`store-id` excluded).
fn records(s: &Setup) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(&s.store)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".issued") || n.ends_with(".used"))
        .collect();
    v.sort();
    v
}

/// A refused run: exit 29, the reason on stderr, the file untouched, the
/// envelope still `.issued` (never spent by a refusal).
fn refused(s: &Setup, o: &Out, e: &RepairEnvelope, why: &str, attack: &str) {
    let fixture = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/broken.ax"));
    refused_over(s, o, e, why, attack, &fixture.unwrap());
}

/// [`refused`] for a run whose `broken.ax` held `pristine` (not the fixture).
fn refused_over(s: &Setup, o: &Out, e: &RepairEnvelope, why: &str, attack: &str, pristine: &[u8]) {
    assert_eq!(o.code, 29, "{attack}: {}{}", o.stdout, o.stderr);
    assert!(
        o.stderr.contains(why),
        "{attack}: stderr {:?} lacks {why:?}",
        o.stderr
    );
    let found = std::fs::read(s.ws.join("broken.ax")).unwrap();
    assert_eq!(found, pristine, "{attack}: the file changed");
    assert_eq!(
        records(s),
        vec![format!("{}.issued", e.envelope_id)],
        "{attack}: a refused envelope was spent"
    );
}

/// `broken.ax` with one `read_file` and one `write_file`, so `fs_read` and
/// `fs_write` pins are used kinds (E1 refuses a pin on an unused kind).
/// Returns the new pristine bytes.
fn use_fs(s: &Setup) -> Vec<u8> {
    let p = s.ws.join("broken.ax");
    let mut src = std::fs::read_to_string(&p).unwrap();
    src.push_str(
        "\nfn touch() -> i64 {\n    let _ = write_file(\"/x\", \"y\")\n    match read_file(\"/x\") {\n        \
         Ok(_) => 1,\n        Err(_) => 0,\n    }\n}\n",
    );
    std::fs::write(&p, &src).unwrap();
    src.into_bytes()
}

/// A run that got past consume: never 29, and the envelope is `.used`.
fn spent(s: &Setup, o: &Out, e: &RepairEnvelope) {
    assert_ne!(
        o.code, 29,
        "an admitted run was refused: {}{}",
        o.stdout, o.stderr
    );
    assert_eq!(
        records(s),
        vec![format!("{}.used", e.envelope_id)],
        "ATTACK: NH-R1 S5 an admitted run did not spend its envelope"
    );
}

#[test]
fn s8_an_admitted_run_spends_once_and_reports_its_authorization() {
    let s = setup("admit");
    let e = envelope_for(&s);
    let text = mint(&s, &e);
    let o = run(&s, &[]);
    spent(&s, &o, &e);
    // S8: the envelope file is byte-identical after the run.
    assert_eq!(std::fs::read_to_string(&s.envelope).unwrap(), text);
    let rec: serde_json::Value = serde_json::from_str(o.stdout.trim()).expect(&o.stdout);
    let a = &rec["authorization"];
    assert_eq!(a["schema"], env::ENVELOPE_SCHEMA);
    assert_eq!(a["envelope_id"], e.envelope_id.as_str());
    assert_eq!(a["issued_by"], "op:test");
    // Security F1: the record names the host opt-in's decision record.
    assert_eq!(
        a["host_opt_in"]["decision"], "governance/decisions/nh-r1-test-enable.md",
        "{a}"
    );
    let rows = a["enforcement"].as_array().unwrap();
    assert!(rows.iter().all(|r| r["status"] == "admit"), "{rows:?}");
    // S12 (§5.7): acceptance is not approval - no `/3` token, no
    // `.approved` record anywhere the run could write.
    assert!(!o.stdout.contains("axon-approval/3"), "{}", o.stdout);
    for dir in [&s.ws, &s.store] {
        for f in std::fs::read_dir(dir).unwrap() {
            let n = f.unwrap().file_name().to_string_lossy().into_owned();
            assert!(!n.ends_with(".approved"), "S12: {n} was written");
        }
    }
    // R6 L2: one started and one terminal record, sealed, chained, naming
    // this envelope; the terminal record carries the exit code and the
    // E5 journal (the walk's restore of broken.ax among its writes).
    let ledger = run_records(&s);
    let kinds: Vec<String> = ledger.iter().map(|r| format!("{:?}", r.kind)).collect();
    assert_eq!(
        kinds,
        ["Started", "Terminal"],
        "ATTACK: R6 L2 an admitted run did not write its started and terminal records"
    );
    assert!(ledger.iter().all(|r| r.envelope_id == e.envelope_id));
    assert!(ledger[0]
        .pinned_tree_before
        .as_ref()
        .unwrap()
        .contains_key("broken.ax"));
    assert_eq!(
        ledger[1].exit_code,
        Some(i64::from(o.code)),
        "ATTACK: R6 L2 the terminal record names another exit code\n{}",
        o.stderr
    );
    let _ = std::fs::remove_dir_all(&s.root);
}

/// Every record in `s`'s run ledger, seal-checked.
fn run_records(s: &Setup) -> Vec<rl::RunRecord> {
    let key = rl::SealKey::derive(OP_KEY).unwrap();
    std::fs::read_to_string(s.ledger.join(rl::LEDGER_FILE))
        .unwrap()
        .lines()
        .map(|l| rl::parse_record(l, &key).expect(l).0)
        .collect()
}

#[test]
fn r6_a_refusal_before_consume_writes_no_record() {
    let s = setup("norecord");
    let mut e = envelope_for(&s);
    e.task.symbol = "other".into();
    mint(&s, &e);
    let o = run(&s, &[]);
    assert_eq!(o.code, 29, "{}", o.stderr);
    assert!(
        run_records(&s).is_empty(),
        "a refused run wrote a run record"
    );
    let _ = std::fs::remove_dir_all(&s.root);
}

#[test]
fn s5_a_second_use_is_refused() {
    let s = setup("replay");
    let e = envelope_for(&s);
    mint(&s, &e);
    spent(&s, &run(&s, &[]), &e);
    let o = run(&s, &[]);
    assert_eq!(
        o.code, 29,
        "ATTACK: NH-R1 S5 a spent envelope was used twice\n{}",
        o.stderr
    );
    assert_eq!(records(&s), vec![format!("{}.used", e.envelope_id)]);
    let _ = std::fs::remove_dir_all(&s.root);
}

#[test]
fn s5_a_fresh_empty_store_or_a_foreign_store_refuses() {
    let s = setup("fresh");
    let e = envelope_for(&s);
    mint(&s, &e);
    // A fresh store of its own id: the envelope names another one.
    let fresh = s.root.join("ops/fresh");
    new_store(&fresh);
    let mut c = Setup {
        store: fresh,
        ..setup_clone(&s)
    };
    let o = run(&c, &[]);
    assert_eq!(
        o.code, 29,
        "ATTACK: NH-R1 S5 a fresh store admitted an envelope\n{}",
        o.stderr
    );
    assert!(records(&c).is_empty());
    // The same store id copied over (a foreign store claiming the id), but
    // without the `.issued` record: never issued there, never spent.
    let foreign = s.root.join("ops/foreign");
    new_store(&foreign);
    std::fs::remove_file(foreign.join(env::STORE_ID_FILE)).unwrap();
    std::fs::copy(
        s.store.join(env::STORE_ID_FILE),
        foreign.join(env::STORE_ID_FILE),
    )
    .unwrap();
    c.store = foreign;
    let o = run(&c, &[]);
    assert_eq!(
        o.code, 29,
        "ATTACK: NH-R1 S5 an envelope never issued into this store was spent\n{}",
        o.stderr
    );
    assert!(records(&c).is_empty());
    assert_eq!(records(&s), vec![format!("{}.issued", e.envelope_id)]);
    let _ = std::fs::remove_dir_all(&s.root);
}

fn setup_clone(s: &Setup) -> Setup {
    Setup {
        root: s.root.clone(),
        ws: s.ws.clone(),
        key: s.key.clone(),
        store: s.store.clone(),
        ledger: s.ledger.clone(),
        seal: s.seal.clone(),
        envelope: s.envelope.clone(),
        axon: s.axon.clone(),
        opt_in: s.opt_in.clone(),
    }
}

/// `s` with `broken.ax` importing module `helpers` (and using the fs, so an
/// fs pin is a used kind). The interpreter runs from a copy at
/// `ops/bin/axon`, so it finds the module at `ops/lib/axon/helpers.ax`
/// (binary-relative: a check child's environment is cleared). Returns the
/// setup, the module's canonical path and the new pristine bytes.
fn with_module(s: &Setup) -> (Setup, PathBuf, Vec<u8>) {
    let bin = s.root.join("ops/bin");
    let lib = s.root.join("ops/lib/axon");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&lib).unwrap();
    let axon = bin.join("axon");
    common::write_executable(&axon, std::fs::read(&s.axon).unwrap(), 0o755);
    let module = lib.join("helpers.ax");
    std::fs::write(&module, "fn inc(n: i64) -> i64 { n + 1 }\n").unwrap();
    let module = std::fs::canonicalize(&module).unwrap();
    let tail = use_fs(s);
    let mut src = b"mod helpers\nuse helpers.{inc}\n".to_vec();
    src.extend_from_slice(&tail);
    std::fs::write(s.ws.join("broken.ax"), &src).unwrap();
    let s = Setup {
        axon: std::fs::canonicalize(&axon).unwrap(),
        ..setup_clone(s)
    };
    (s, module, src)
}

/// Security review F5: the check child's `fs_write` pins are disjoint from
/// the modules of the task's import closure, not only from the task file.
/// A pin over `lib/` let the check child rewrite an imported module, and
/// every later verdict was graded against the tampered import.
#[test]
fn s6_an_fs_write_pin_over_an_imported_module_is_refused() {
    let s = setup("s6_modpin");
    let (s, module, pristine) = with_module(&s);
    let scratch = s.root.join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let scratch = std::fs::canonicalize(&scratch).unwrap();
    let e = envelope_pinned(
        &s,
        ScopePins {
            fs_read: vec![scratch.to_string_lossy().into_owned()],
            fs_write: vec![module.parent().unwrap().to_string_lossy().into_owned()],
            net: vec![],
        },
    );
    mint(&s, &e);
    let axon = s.axon.clone();
    let o = run_with(&s, &axon, &[], &[]);
    refused_over(
        &s,
        &o,
        &e,
        "covers the imported module",
        "ATTACK: NH-R1 security F5 an fs_write pin over an imported module ran",
        &pristine,
    );
    let _ = std::fs::remove_dir_all(&s.root);
}

/// Security review F5: the import closure is re-hashed before EVERY check
/// dispatch, not only once after the witness run. Here the check child
/// rewrites an imported module during the first check (the fence-conflict
/// run); the next dispatch must refuse (exit 29) instead of running a check
/// on a closure nobody verified.
#[test]
fn s13_an_import_changed_by_a_check_stops_the_next_dispatch() {
    let s = setup("s13_import");
    let (s, module, pristine) = with_module(&s);
    let wrapper = s.root.join("ops/axon-import-rewriter");
    let log = s.root.join("ops/test-runs");
    common::write_executable(
        &wrapper,
        format!(
            "#!/bin/sh\nif [ \"$1\" = test ]; then echo run >> '{l}'; [ -s '{l}.done' ] || {{ echo x > '{l}.done'; printf '// changed\\n' >> '{m}'; }}; fi\nexec '{a}' \"$@\"\n",
            l = log.display(),
            m = module.display(),
            a = s.axon.display()
        ),
        0o755,
    );
    let s = Setup {
        axon: wrapper.clone(),
        ..setup_clone(&s)
    };
    let scratch = s.root.join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let scratch = std::fs::canonicalize(&scratch)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let e = envelope_pinned(
        &s,
        ScopePins {
            fs_read: vec![scratch.clone()],
            fs_write: vec![scratch],
            net: vec![],
        },
    );
    mint(&s, &e);
    let o = run_with(&s, &wrapper, &[], &[]);
    let runs = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(
        (o.code, runs.lines().count()),
        (29, 1),
        "ATTACK: NH-R1 security F5 a check ran on an import closure changed by an earlier check\n{}{}",
        o.stdout,
        o.stderr
    );
    assert!(
        o.stderr
            .contains("the import closure changed before a check dispatch"),
        "{}",
        o.stderr
    );
    assert_eq!(std::fs::read(s.ws.join("broken.ax")).unwrap(), pristine);
    let _ = std::fs::remove_dir_all(&s.root);
}

#[test]
fn s5_of_concurrent_runs_exactly_one_proceeds() {
    let s = setup("race");
    let e = envelope_for(&s);
    mint(&s, &e);
    let codes: Vec<i32> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..6).map(|_| sc.spawn(|| run(&s, &[]).code)).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let admitted = codes.iter().filter(|c| **c != 29).count();
    assert_eq!(
        admitted, 1,
        "ATTACK: NH-R1 S5 concurrent runs spent one envelope {admitted} times: {codes:?}"
    );
    assert_eq!(records(&s), vec![format!("{}.used", e.envelope_id)]);
    let _ = std::fs::remove_dir_all(&s.root);
}

#[test]
fn s6_an_envelope_for_anything_else_is_refused() {
    type Change = fn(&mut RepairEnvelope);
    let cases: &[(&str, Change, &str)] = &[
        (
            "workspace",
            |e| e.task.workspace.push_str("-other"),
            "workspace",
        ),
        ("file", |e| e.task.file = "other.ax".into(), "--file"),
        ("symbol", |e| e.task.symbol = "main".into(), "--symbol"),
        (
            "pristine sha",
            |e| e.task.pristine_sha256 = "0".repeat(64),
            "not the file the envelope was minted for",
        ),
        (
            "imports digest",
            |e| e.task.imports_digest = format!("axsha256:{}", "1".repeat(64)),
            "check digest",
        ),
        (
            "model",
            |e| e.generator.model = "other".into(),
            "generator model",
        ),
        (
            "provider",
            |e| e.generator.provider = "openai".into(),
            "generator provider",
        ),
        (
            "endpoint",
            |e| e.generator.endpoint_host = "api.example.com".into(),
            "generator endpoint host",
        ),
        (
            "fence",
            |e| e.capabilities.fence_sha256 = "2".repeat(64),
            "check digest",
        ),
        (
            "check digest",
            |e| e.check.digest = "3".repeat(64),
            "check digest",
        ),
        (
            "target grant",
            |e| e.capabilities.target_grant = serde_json::json!({"symbol": "double"}),
            "static grant",
        ),
    ];
    for (what, change, why) in cases {
        let s = setup(&format!("s6_{}", what.replace(' ', "_")));
        let mut e = envelope_for(&s);
        change(&mut e);
        mint(&s, &e);
        let o = run(&s, &[]);
        refused(
            &s,
            &o,
            &e,
            why,
            &format!("ATTACK: NH-R1 S6 an envelope with another {what} was admitted"),
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

#[test]
fn s6_a_wider_command_line_is_refused() {
    let cases: &[(&str, &[&str], &str)] = &[
        ("write prefix", &["--write-prefix", "."], "--write-prefix"),
        ("budget", &["--budget", "3"], "exceeds the envelope's"),
        (
            "max proposals",
            &["--max-proposals", "2"],
            "exceeds the envelope's",
        ),
        (
            "deadline",
            &["--deadline-ms", "60001"],
            "exceeds the envelope's",
        ),
        (
            "candidates",
            &["--candidates", "2"],
            "exceeds the envelope's",
        ),
        (
            "check timeout",
            &["--check-timeout-ms", "30001"],
            "exceeds the envelope's",
        ),
        ("fabric", &["--fabric-store", "/x"], "E2"),
        ("check registry", &["--check-registry", "/x"], "E7"),
        ("ranker", &["--ranker", "spectrum"], "--ranker"),
        ("audit", &["--audit", "off"], "--audit"),
        ("label log", &["--label-log", "/x"], "--label-log"),
        ("principal", &["--principal", "p"], "--principal"),
    ];
    for (what, extra, why) in cases {
        let s = setup(&format!("s6w_{}", what.replace(' ', "_")));
        let e = envelope_for(&s);
        mint(&s, &e);
        let o = run(&s, extra);
        refused(
            &s,
            &o,
            &e,
            why,
            &format!("ATTACK: NH-R1 S6 a wider {what} was admitted"),
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

#[test]
fn s6_a_cortex_env_selector_is_refused() {
    for var in axon_cortex::unattended::REFUSED_ENV {
        let s = setup(&format!("s6e_{var}"));
        let e = envelope_for(&s);
        mint(&s, &e);
        let o = run_with(&s, &s.axon, &[], &[(var, "x")]);
        refused(
            &s,
            &o,
            &e,
            var,
            &format!("ATTACK: NH-R1 S6 the env selector {var} steered an unattended run"),
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

/// Security review F2: a `.env` cortex would load (in its working directory,
/// or named by `AXON_DOTENV`) steers nothing. It sets `CORTEX_LABEL_LOG` -
/// a §5.2 selector `check_env` refuses when it is in the real environment -
/// and the run must not append a label row (a write outside the pins, the
/// E5 helper and the journal) there. It also sets `AXON_AI_MOCK`, which no
/// later guard reads: loaded, the generator would be answered by the mock
/// stub instead of the endpoint the envelope pins (here a closed port, so
/// every call must fail as unavailable).
#[test]
fn s6_a_dotenv_steers_nothing() {
    for how in ["cwd", "AXON_DOTENV"] {
        let s = setup(&format!("s6_dotenv_{how}"));
        let e = envelope_for(&s);
        mint(&s, &e);
        let log = s.root.join("label.log");
        let dotenv = format!("CORTEX_LABEL_LOG={}\nAXON_AI_MOCK=1\n", log.display());
        let named = s.root.join("ops/steer.env");
        std::fs::write(&named, &dotenv).unwrap();
        let o = if how == "cwd" {
            std::fs::write(s.ws.join(".env"), &dotenv).unwrap();
            run_full(&s, &s.axon, &[], &[], &s.opted_in(), Some(&s.ws))
        } else {
            run_with(
                &s,
                &s.axon,
                &[],
                &[("AXON_DOTENV", named.to_str().unwrap())],
            )
        };
        assert!(
            !log.exists(),
            "ATTACK: NH-R1 security F2 a .env ({how}) steered an unattended run's label log\n{}{}",
            o.stdout,
            o.stderr
        );
        assert!(
            !o.stdout.contains("\"kind\":\"proposed\""),
            "ATTACK: NH-R1 security F2 a .env ({how}) answered an unattended run's generator \
             (AXON_AI_MOCK) instead of the pinned endpoint\n{}{}",
            o.stdout,
            o.stderr
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

/// Security build: a model-steering variable in the REAL environment is
/// refused like the §5.2 selectors. `AXON_AI_MOCK` would have the in-process
/// generator answered by the mock stub instead of the endpoint the envelope
/// pins (here a closed port, so every call must fail as unavailable);
/// `AXON_AI_REPLAY` would serve `ai_complete` answers from a cache file.
#[test]
fn s6_a_real_env_model_override_is_refused() {
    for var in ["AXON_AI_MOCK", "AXON_AI_REPLAY"] {
        let s = setup(&format!("s6_aienv_{var}"));
        let e = envelope_for(&s);
        mint(&s, &e);
        let cache = s.root.join("ops/replay.jsonl");
        std::fs::write(&cache, "").unwrap();
        let value = if var == "AXON_AI_MOCK" {
            "1"
        } else {
            cache.to_str().unwrap()
        };
        let o = run_with(&s, &s.axon, &[], &[(var, value)]);
        assert!(
            !o.stdout.contains("\"kind\":\"proposed\""),
            "ATTACK: NH-R1 security build {var} in the real environment answered an \
             unattended run's generator instead of the pinned endpoint\n{}{}",
            o.stdout,
            o.stderr
        );
        refused(
            &s,
            &o,
            &e,
            var,
            &format!("ATTACK: NH-R1 security build {var} in the real environment ran unattended"),
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

/// A local listener that records the first line of every request reaching
/// it, and whether it carried an `x-api-key` header, answering 502.
struct Recorder {
    addr: std::net::SocketAddr,
    got: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl Recorder {
    fn start() -> Recorder {
        use std::io::{Read as _, Write as _};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let got: Arc<Mutex<Vec<String>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (got2, stop2) = (Arc::clone(&got), Arc::clone(&stop));
        let thread = std::thread::spawn(move || {
            while !stop2.load(Ordering::SeqCst) {
                let Ok((mut c, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                };
                c.set_nonblocking(false).unwrap();
                c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut buf = vec![0u8; 8192];
                let n = c.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let line = head.lines().next().unwrap_or("").to_string();
                let keyed = head.to_ascii_lowercase().contains("x-api-key");
                got2.lock()
                    .unwrap()
                    .push(format!("{line} (x-api-key: {keyed})"));
                let _ = c.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n");
            }
        });
        Recorder {
            addr,
            got,
            stop,
            thread,
        }
    }

    /// Stop the listener; what reached it.
    fn finish(self) -> Vec<String> {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.thread.join().unwrap();
        let got = self.got.lock().unwrap().clone();
        got
    }
}

/// Security re-review N1: a proxy variable in the real environment is
/// refused. The envelope pins the generator's endpoint HOST; with
/// `AXON_AI_BASE_URL` on that host and a proxy set, the model call - with the
/// API key - went to whoever runs the proxy, which could answer with any
/// patch. Here the proxy is a local listener that records what reaches it
/// and the key is a dummy one.
#[test]
fn s6_a_proxy_variable_is_refused() {
    let cases = [
        ("HTTP_PROXY", "http"),
        ("http_proxy", "http"),
        ("ALL_PROXY", "http"),
        ("all_proxy", "http"),
        ("HTTPS_PROXY", "https"),
        ("https_proxy", "https"),
    ];
    for (var, scheme) in cases {
        let rec = Recorder::start();
        let proxy = format!("http://{}", rec.addr);
        let s = setup(&format!("s6_proxy_{var}"));
        let mut e = envelope_for(&s);
        e.generator.endpoint_host = "api.anthropic.com".into();
        mint(&s, &e);
        let base = format!("{scheme}://api.anthropic.com");
        let o = run_with(
            &s,
            &s.axon,
            &[],
            &[("AXON_AI_BASE_URL", &base), (var, &proxy)],
        );
        let got = rec.finish();
        assert!(
            got.is_empty(),
            "ATTACK: NH-R1 security N1 {var} sent an unattended run's model call to a proxy the \
             envelope never pinned: {got:?}\n{}{}",
            o.stdout,
            o.stderr
        );
        refused(
            &s,
            &o,
            &e,
            var,
            &format!("ATTACK: NH-R1 security N1 {var} in the real environment ran unattended"),
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

/// Security re-review 2 N1: the envelope pins the endpoint HOST, not the
/// scheme, so `AXON_AI_BASE_URL=http://<pinned host>` sent the model call
/// and its API key in plaintext to anyone on the path. An unattended run
/// refuses a base URL that is not `https://`. Here the pinned host is
/// `127.0.0.1`, a plain-HTTP listener on it records what reaches it, and the
/// key is a dummy one: the listener must get nothing.
#[test]
fn s6_a_plain_http_endpoint_is_refused() {
    for var in ["AXON_AI_BASE_URL", "ANTHROPIC_BASE_URL"] {
        let rec = Recorder::start();
        let s = setup(&format!("s6_plainhttp_{var}"));
        let e = envelope_for(&s);
        assert_eq!(e.generator.endpoint_host, "127.0.0.1");
        mint(&s, &e);
        let base = format!("http://{}", rec.addr);
        let o = run_with(
            &s,
            &s.axon,
            &[],
            &[
                (
                    "AXON_AI_BASE_URL",
                    if var == "AXON_AI_BASE_URL" {
                        base.as_str()
                    } else {
                        ""
                    },
                ),
                ("ANTHROPIC_BASE_URL", base.as_str()),
            ],
        );
        let got = rec.finish();
        assert!(
            got.is_empty(),
            "ATTACK: NH-R1 security N1 {var}=http:// sent an unattended run's model call in \
             plaintext: {got:?}\n{}{}",
            o.stdout,
            o.stderr
        );
        refused(
            &s,
            &o,
            &e,
            "https",
            &format!("ATTACK: NH-R1 security N1 {var}=http:// ran unattended"),
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

/// Security review F4: a relative `fs_read` / `fs_write` pin is refused. The
/// check child's cwd is the workspace, so `out` resolves to `ws/out` - here a
/// symlink to the operator's key directory - and a relative pin passed the
/// symlink walk and the policy check, which only saw absolute pins.
#[test]
fn s6_a_relative_fs_pin_is_refused() {
    for kind in ["fs_read", "fs_write"] {
        let s = setup(&format!("s6_relpin_{kind}"));
        let pristine = use_fs(&s);
        std::os::unix::fs::symlink(s.root.join("ops"), s.ws.join("out")).unwrap();
        let scratch = s.root.join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        let scratch = std::fs::canonicalize(scratch)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let (fs_read, fs_write) = if kind == "fs_read" {
            (vec!["out".to_string()], vec![scratch])
        } else {
            (vec![scratch], vec!["out".to_string()])
        };
        let e = envelope_pinned(
            &s,
            ScopePins {
                fs_read,
                fs_write,
                net: vec![],
            },
        );
        // The minter refuses a relative pin (axon-os validate); sign around it.
        mint_unvalidated(&s, &e);
        let o = run(&s, &[]);
        refused_over(
            &s,
            &o,
            &e,
            &format!("scopes.{kind} entry \"out\" is relative"),
            &format!(
                "ATTACK: NH-R1 security F4 a relative {kind} pin through a workspace symlink ran"
            ),
            &pristine,
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

/// Security review F6: no `fs_read` / `fs_write` pin reaches the spent store
/// or a key file - at, above or below it. Over the store the check child
/// could rename `.used` back to `.issued` (replay); over the verify key,
/// overwrite it (self-mint); over the seal key, read it (forge records).
#[test]
fn s6_a_pin_over_the_store_or_a_key_is_refused() {
    type Case = (
        &'static str,
        &'static str,
        fn(&Setup) -> PathBuf,
        &'static str,
    );
    let cases: [Case; 6] = [
        (
            "fs_write",
            "the spent store",
            |s| s.store.clone(),
            "the spent store",
        ),
        (
            "fs_write",
            "a directory under the spent store",
            |s| s.store.join("sub"),
            "the spent store",
        ),
        (
            "fs_write",
            "the verify key",
            |s| s.key.clone(),
            "the verify key",
        ),
        (
            "fs_read",
            "the run-record seal key",
            |s| s.seal.clone(),
            "the run-record seal key",
        ),
        (
            "fs_read",
            "the operator directory above both keys",
            |s| s.root.join("ops"),
            "the spent store",
        ),
        (
            "fs_write",
            "a directory under the run ledger",
            |s| s.ledger.join("sub"),
            "the run ledger",
        ),
    ];
    for (i, (kind, what, pin, why)) in cases.into_iter().enumerate() {
        let s = setup(&format!("s6_keypin_{i}"));
        let pristine = use_fs(&s);
        let scratch = s.root.join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        let canon = |p: PathBuf| {
            let parent = std::fs::canonicalize(p.parent().unwrap()).unwrap();
            parent
                .join(p.file_name().unwrap())
                .to_string_lossy()
                .into_owned()
        };
        let (pin, scratch) = (canon(pin(&s)), canon(scratch));
        let (fs_read, fs_write) = if kind == "fs_read" {
            (vec![pin], vec![scratch])
        } else {
            (vec![scratch], vec![pin])
        };
        let e = envelope_pinned(
            &s,
            ScopePins {
                fs_read,
                fs_write,
                net: vec![],
            },
        );
        mint(&s, &e);
        let o = run(&s, &[]);
        refused_over(
            &s,
            &o,
            &e,
            &format!("{why} "),
            &format!("ATTACK: NH-R1 security F6 an {kind} pin over {what} ran"),
            &pristine,
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

/// Security re-review N4: a pin outside the workspace that reaches the run
/// ledger or the spent store through a symlink - one under the pin, or the
/// pin itself - is refused. The interpreter matches a pin as a path prefix
/// and the OS follows the link, so a symlink made after minting under a
/// write pin handed the check child the ledger (forge or drop records) or
/// the store (rename `.used` back to `.issued`); phase A compared the
/// canonical ledger and store with the pin strings and walked only the pins
/// inside the workspace for symlinks.
#[test]
fn s6_a_pin_reaching_the_ledger_or_store_through_a_symlink_is_refused() {
    type Case = (&'static str, fn(&Setup, &Path) -> PathBuf);
    let cases: [Case; 3] = [
        (
            "a symlink to the run ledger under the write pin",
            |s, scratch| {
                std::os::unix::fs::symlink(&s.ledger, scratch.join("l")).unwrap();
                scratch.to_path_buf()
            },
        ),
        (
            "a symlink to the spent store under the write pin",
            |s, scratch| {
                std::os::unix::fs::symlink(&s.store, scratch.join("s")).unwrap();
                scratch.to_path_buf()
            },
        ),
        (
            "a write pin that is a symlink to the operator directory",
            |s, scratch| {
                let alias = scratch.join("alias");
                std::os::unix::fs::symlink(s.root.join("ops"), &alias).unwrap();
                alias
            },
        ),
    ];
    for (i, (what, make)) in cases.into_iter().enumerate() {
        let s = setup(&format!("s6_n4_{i}"));
        let pristine = use_fs(&s);
        let scratch = s.root.join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        let scratch = std::fs::canonicalize(scratch).unwrap();
        let read = s.root.join("read");
        std::fs::create_dir_all(&read).unwrap();
        let read = std::fs::canonicalize(read).unwrap();
        let e = envelope_pinned(
            &s,
            ScopePins {
                fs_read: vec![read.to_string_lossy().into_owned()],
                fs_write: vec![make(&s, &scratch).to_string_lossy().into_owned()],
                net: vec![],
            },
        );
        mint(&s, &e);
        let o = run(&s, &[]);
        refused_over(
            &s,
            &o,
            &e,
            "is a symlink",
            &format!("ATTACK: NH-R1 security N4 {what} ran"),
            &pristine,
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

/// Security review F7: the interpreter the check digest binds is the one
/// that runs. A bare `--axon axon` was hashed from the cwd (here the genuine
/// interpreter) and then resolved through `PATH` (here another one) for
/// `axon caps` and every check. Resolved once, the PATH binary's sha256 is
/// not the envelope's: refused before it runs.
#[test]
fn s6_a_bare_axon_runs_the_binary_that_was_hashed() {
    let s = setup("s6_bare_axon");
    let e = envelope_for(&s);
    mint(&s, &e);
    let cwd = s.root.join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    std::os::unix::fs::symlink(&s.axon, cwd.join("axon")).unwrap();
    let bin = s.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let mark = s.root.join("ops/path-axon-ran");
    common::write_executable(
        &bin.join("axon"),
        format!(
            "#!/bin/sh\ntouch '{}'\nexec '{}' \"$@\"\n",
            mark.display(),
            s.axon.display()
        ),
        0o755,
    );
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let o = run_full(
        &s,
        Path::new("axon"),
        &[],
        &[("PATH", &path)],
        &s.opted_in(),
        Some(&cwd),
    );
    assert!(
        !mark.exists(),
        "ATTACK: NH-R1 security F7 a PATH interpreter other than the hashed one ran\n{}{}",
        o.stdout,
        o.stderr
    );
    refused(
        &s,
        &o,
        &e,
        "is not the envelope's",
        "ATTACK: NH-R1 security F7 a PATH interpreter other than the hashed one got past phase A",
    );
    let _ = std::fs::remove_dir_all(&s.root);
}

/// Security review F1 / §10 Q3: a host runs unattended only after it opted
/// in. Without `/etc/axon/unattended-repair` - or with one another uid owns,
/// one group/other can read or write, a directory in its place, one naming
/// no decision record, or one in an `/etc/axon` another user owns or can
/// write (security re-review N2) - phase A refuses (exit 29) and nothing is
/// spent. A feature build copied to another host does not run.
#[test]
fn s14_a_host_that_did_not_opt_in_refuses() {
    let s = setup("s14_optin");
    let content = |name: &str, text: &str| {
        let p = s.root.join("ops").join(name);
        std::fs::write(&p, text).unwrap();
        p
    };
    let no_decision = content("opt-in-none", "# this host agrees\n");
    let other_line = content("opt-in-other", "enabled\ndecision: d.md\n");
    let two = content("opt-in-two", "decision: a.md\ndecision: b.md\n");
    let dir = s.root.join("ops/opt-in-dir");
    std::fs::create_dir_all(&dir).unwrap();
    fn file(src: &Path) -> OptIn<'_> {
        OptIn {
            src: Some(src),
            mode: "0600",
            owner: "0",
            dir: "mode=0755",
        }
    }
    let cases: [(&str, OptIn); 9] = [
        (
            "no opt-in file",
            OptIn {
                src: None,
                ..file(&s.opt_in)
            },
        ),
        (
            "an opt-in of mode 0640",
            OptIn {
                mode: "0640",
                ..file(&s.opt_in)
            },
        ),
        (
            "an opt-in owned by uid 1",
            OptIn {
                owner: "1",
                ..file(&s.opt_in)
            },
        ),
        (
            "a directory as its opt-in",
            OptIn {
                mode: "0700",
                ..file(&dir)
            },
        ),
        ("an opt-in naming no decision record", file(&no_decision)),
        (
            "an opt-in holding a line that is not its decision",
            file(&other_line),
        ),
        ("an opt-in naming two decision records", file(&two)),
        (
            "an opt-in in an /etc/axon other users can write",
            OptIn {
                dir: "mode=0777",
                ..file(&s.opt_in)
            },
        ),
        (
            "an opt-in in an /etc/axon uid 1 owns",
            OptIn {
                dir: "mode=0755,uid=1",
                ..file(&s.opt_in)
            },
        ),
    ];
    for (what, opt) in cases {
        let e = envelope_for(&s);
        for r in records(&s) {
            std::fs::remove_file(s.store.join(r)).unwrap();
        }
        mint(&s, &e);
        let o = run_full(&s, &s.axon, &[], &[], &opt, None);
        refused(
            &s,
            &o,
            &e,
            "this host has not opted in to unattended repair",
            &format!("ATTACK: NH-R1 security F1 a host with {what} ran unattended"),
        );
    }
    let _ = std::fs::remove_dir_all(&s.root);
}

/// Security re-review 2 N7: inside a user namespace with a uid mapping,
/// files the caller owns can appear root-owned, so the host opt-in's
/// ownership checks prove nothing there. A run under `unshare --user
/// --map-root-user` - where the opt-in, owned by the namespace's uid 0,
/// would otherwise pass - refuses with exit 29, writing nothing.
#[test]
fn s14_a_run_in_a_user_namespace_refuses() {
    let probe = std::process::Command::new("unshare")
        .args(["--user", "--map-root-user", "true"])
        .status();
    if !probe.is_ok_and(|st| st.success()) {
        println!("SKIP s14_a_run_in_a_user_namespace_refuses: user namespaces are unavailable");
        return;
    }
    let s = setup("s14_userns");
    let e = envelope_for(&s);
    mint(&s, &e);
    let bin = s.root.join("ops/cortex-userns");
    std::fs::copy(env!("CARGO_BIN_EXE_cortex"), &bin).unwrap();
    let o = run_unshared(&s, &s.axon, &[], &[], &s.opted_in(), None, Some(&bin));
    refused(
        &s,
        &o,
        &e,
        "only in the initial user namespace",
        "ATTACK: NH-R1 security N7 a run in a user namespace passed the host opt-in",
    );
    let _ = std::fs::remove_dir_all(&s.root);
}

#[test]
fn s6_another_checker_binary_is_refused() {
    let s = setup("s6_checker");
    let e = envelope_for(&s);
    mint(&s, &e);
    let other = s.root.join("ops/axon-other");
    common::write_executable(&other, b"#!/bin/sh\nexit 0\n", 0o755);
    let o = run_with(&s, &other, &[], &[]);
    refused(
        &s,
        &o,
        &e,
        "check digest",
        "ATTACK: NH-R1 S6 a changed checker binary was admitted",
    );
    let _ = std::fs::remove_dir_all(&s.root);
}

#[test]
fn s8_a_bad_signature_never_runs_the_checker() {
    let s = setup("s8_canary");
    let canary_mark = s.root.join("ops/canary-ran");
    let canary = s.root.join("ops/axon-canary");
    common::write_executable(
        &canary,
        format!("#!/bin/sh\ntouch '{}'\nexit 0\n", canary_mark.display()),
        0o755,
    );
    let e = envelope_for(&s);
    let text = mint(&s, &e);
    // Flip one hex digit of the signature.
    let i = text.find(env::SIG_PREFIX).unwrap() + env::SIG_PREFIX.len();
    let mut bad = text.into_bytes();
    bad[i] = if bad[i] == b'0' { b'1' } else { b'0' };
    std::fs::write(&s.envelope, &bad).unwrap();
    let o = run_with(&s, &canary, &[], &[]);
    refused(
        &s,
        &o,
        &e,
        "signature",
        "ATTACK: NH-R1 S8 a bad-signature envelope was admitted",
    );
    assert!(
        !canary_mark.exists(),
        "ATTACK: NH-R1 S8 the canary --axon ran before verification"
    );
    assert_eq!(
        std::fs::read(&s.envelope).unwrap(),
        bad,
        "the envelope file changed"
    );
    let _ = std::fs::remove_dir_all(&s.root);
}

/// §5.4: a check child that rewrites the entry file on its first `test` run
/// (the fence-conflict run) and then runs the real interpreter: the
/// pristine-test TOCTOU. The envelope pins THIS wrapper's sha, so only the
/// rewrite is out of order. Outside the target's body the next dispatch
/// refuses (security review F5: no child runs on bytes nobody verified);
/// inside it, the full compare right after the witness run does - both name
/// both digests.
#[test]
fn s13_a_checker_that_rewrites_the_entry_file_during_the_witness_run_refuses() {
    let cases = [
        (
            "outside",
            "printf '// changed\\n' >> '{f}'",
            "broken.ax changed outside `double` before a check dispatch (sha256 ",
        ),
        (
            "body",
            "sed -i 's/n + 2/n + 3/' '{f}'",
            "broken.ax changed after verification (sha256 ",
        ),
    ];
    for (name, edit, why) in cases {
        let s = setup(&format!("s13_{name}"));
        let wrapper = s.root.join("ops/axon-rewriter");
        let mark = s.root.join("ops/rewrote");
        let edit = edit.replace("{f}", &s.ws.join("broken.ax").display().to_string());
        common::write_executable(
            &wrapper,
            format!(
                "#!/bin/sh\nif [ \"$1\" = test ] && [ ! -e '{m}' ]; then touch '{m}'; {edit}; fi\nexec '{a}' \"$@\"\n",
                m = mark.display(),
                a = s.axon.display()
            ),
            0o755,
        );
        let s = Setup {
            axon: wrapper.clone(),
            ..setup_clone(&s)
        };
        let e = envelope_for(&s);
        mint(&s, &e);
        let o = run_with(&s, &wrapper, &[], &[]);
        assert!(mark.exists(), "{name}: the first check never ran");
        assert_eq!(
            o.code, 29,
            "ATTACK: NH-R1 S13 a file changed during the witness run was graded ({name})\n{}{}",
            o.stdout, o.stderr
        );
        assert!(
            o.stderr.contains(why),
            "ATTACK: NH-R1 S13 a file changed during the witness run was graded ({name})\n{}",
            o.stderr
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

/// Security review F8: the hard deadline covers every child after phase B.
/// The witness run (and the fence-conflict run before it) executes
/// workspace code; armed only after them, a check that stalls there ran for
/// the whole `check_timeout`, far past `deadline + HARD_GRACE`. Here the
/// witness check sleeps 20 s under a 4 s deadline: the run must exit 20
/// (budget exhausted) by the deadline plus the grace, the file as found.
#[test]
fn s10_a_witness_run_stalled_past_the_deadline_is_stopped_by_the_grace() {
    let s = setup("s10_witness");
    let wrapper = s.root.join("ops/axon-sleeper");
    common::write_executable(
        &wrapper,
        format!(
            "#!/bin/sh\nif [ \"$1\" = test ]; then sleep 20; fi\nexec '{a}' \"$@\"\n",
            a = s.axon.display()
        ),
        0o755,
    );
    let s = Setup {
        axon: wrapper.clone(),
        ..setup_clone(&s)
    };
    let mut e = envelope_for(&s);
    e.budget.deadline_ms = 4_000;
    mint(&s, &e);
    let t0 = std::time::Instant::now();
    let o = run_with(&s, &wrapper, &[], &[]);
    let took = t0.elapsed();
    // Process start-up (the namespace wrapper, phase A/B's static children)
    // comes before the deadline's clock and on top of it.
    let bound = Duration::from_millis(4_000)
        + axon_cortex::watchdog::HARD_GRACE
        + Duration::from_millis(1_500);
    assert!(
        o.code == axon_cortex::watchdog::EXIT_DEADLINE && took <= bound,
        "ATTACK: NH-R1 security F8 a check stalled before the watchdog was armed outlived the \
         deadline (exit {}, {took:?} > {bound:?})\n{}{}",
        o.code,
        o.stdout,
        o.stderr
    );
    assert_eq!(
        std::fs::read(s.ws.join("broken.ax")).unwrap(),
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/broken.ax")).unwrap(),
        "the stopped run left the file changed"
    );
    let _ = std::fs::remove_dir_all(&s.root);
}

/// Security review F8: before the watchdog is armed, an unattended run's
/// static children (`axon caps`, `axon imports`) are each bounded by the
/// time left before the deadline, not by `check_timeout` alone. Here every
/// `caps` after the mint sleeps 20 s under a 4 s deadline: the run must end
/// by the deadline plus the grace, refused before consume (envelope unspent).
#[test]
fn s10_a_static_child_stalled_before_phase_b_is_bounded_by_the_deadline() {
    let s = setup("s10_static");
    let wrapper = s.root.join("ops/axon-caps-sleeper");
    let mark = s.root.join("ops/minted");
    common::write_executable(
        &wrapper,
        format!(
            "#!/bin/sh\nif [ \"$1\" = caps ] && [ -e '{m}' ]; then sleep 20; fi\nexec '{a}' \"$@\"\n",
            m = mark.display(),
            a = s.axon.display()
        ),
        0o755,
    );
    let s = Setup {
        axon: wrapper.clone(),
        ..setup_clone(&s)
    };
    let mut e = envelope_for(&s);
    e.budget.deadline_ms = 4_000;
    mint(&s, &e);
    std::fs::write(&mark, b"").unwrap();
    let t0 = std::time::Instant::now();
    let o = run_with(&s, &wrapper, &[], &[]);
    let took = t0.elapsed();
    let bound = Duration::from_millis(4_000)
        + axon_cortex::watchdog::HARD_GRACE
        + Duration::from_millis(1_500);
    assert!(
        took <= bound,
        "ATTACK: NH-R1 security F8 a static child outlived the deadline before the watchdog was \
         armed (exit {}, {took:?} > {bound:?})\n{}{}",
        o.code,
        o.stdout,
        o.stderr
    );
    // `derive_fence`'s own `axon caps` is the child cut: blocked (22),
    // before consume.
    assert_eq!(o.code, 22, "{}{}", o.stdout, o.stderr);
    assert!(o.stderr.contains("wall-clock limit"), "{}", o.stderr);
    assert_eq!(records(&s), vec![format!("{}.issued", e.envelope_id)]);
    let _ = std::fs::remove_dir_all(&s.root);
}
