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
    std::fs::write(&opt_in, "decision: governance/decisions/nh-r1-test-enable.md\n").unwrap();
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
    let file = s.ws.join("broken.ax");
    let caps = caps_of_file(&s.axon, &file, None, WALL).unwrap();
    let fence = Fence::from_pristine(&caps);
    let pins = ScopePins::default();
    let (rows, pinned) = auth::gate_rows(&fence, &caps, &pins, "ai");
    assert!(rows.iter().all(|r| r.admit), "{rows:?}");
    let fence_sha = pinned.unwrap().sha256();
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
            fs_read: vec![],
            fs_write: vec![],
            net: vec![],
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
/// content file (`None`: no opt-in file), mode and owner.
struct OptIn<'a> {
    src: Option<&'a Path>,
    mode: &'a str,
    owner: &'a str,
}

impl Setup {
    /// The opt-in a host that agreed has: owned by the running uid (root
    /// here), mode 0600, naming the decision record.
    fn opted_in(&self) -> OptIn<'_> {
        OptIn {
            src: Some(&self.opt_in),
            mode: "0600",
            owner: "0",
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
    assert!(
        euid() == 0 && Path::new("/etc/axon").is_dir(),
        "setup: the unattended tests need root and an /etc/axon mount point (a private mount \
         namespace holds the host opt-in file; /etc is never written)"
    );
    let none = s.root.join("no-opt-in");
    let mut c = std::process::Command::new("unshare");
    c.args(["-m", "--propagation", "private", "sh", "-c"])
        .arg(
            "set -e; mount -t tmpfs -o mode=0755 tmpfs /etc/axon; o=/etc/axon/unattended-repair; \
             if [ -d \"$1\" ]; then mkdir -m 0700 \"$o\"; \
             elif [ -e \"$1\" ]; then cp \"$1\" \"$o\"; chown \"$3\" \"$o\"; chmod \"$2\" \"$o\"; fi; \
             shift 3; exec \"$@\"",
        )
        .arg("sh")
        .arg(opt.src.unwrap_or(&none))
        .args([opt.mode, opt.owner])
        .arg(env!("CARGO_BIN_EXE_cortex"));
    c.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("AXON_AI_PROVIDER", "anthropic")
        .env("AXON_AI_BASE_URL", "http://127.0.0.1:9")
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
    assert_eq!(o.code, 29, "{attack}: {}{}", o.stdout, o.stderr);
    assert!(
        o.stderr.contains(why),
        "{attack}: stderr {:?} lacks {why:?}",
        o.stderr
    );
    let found = std::fs::read(s.ws.join("broken.ax")).unwrap();
    let fixture = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/broken.ax"));
    assert_eq!(found, fixture.unwrap(), "{attack}: the file changed");
    assert_eq!(
        records(s),
        vec![format!("{}.issued", e.envelope_id)],
        "{attack}: a refused envelope was spent"
    );
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
/// E5 helper and the journal) there.
#[test]
fn s6_a_dotenv_steers_nothing() {
    for how in ["cwd", "AXON_DOTENV"] {
        let s = setup(&format!("s6_dotenv_{how}"));
        let e = envelope_for(&s);
        mint(&s, &e);
        let log = s.root.join("label.log");
        let dotenv = format!("CORTEX_LABEL_LOG={}\n", log.display());
        let named = s.root.join("ops/steer.env");
        std::fs::write(&named, &dotenv).unwrap();
        let o = if how == "cwd" {
            std::fs::write(s.ws.join(".env"), &dotenv).unwrap();
            run_full(&s, &s.axon, &[], &[], &s.opted_in(), Some(&s.ws))
        } else {
            run_with(&s, &s.axon, &[], &[("AXON_DOTENV", named.to_str().unwrap())])
        };
        assert!(
            !log.exists(),
            "ATTACK: NH-R1 security F2 a .env ({how}) steered an unattended run's label log\n{}{}",
            o.stdout,
            o.stderr
        );
        let _ = std::fs::remove_dir_all(&s.root);
    }
}

/// Security review F1 / §10 Q3: a host runs unattended only after it opted
/// in. Without `/etc/axon/unattended-repair` - or with one another uid owns,
/// one group/other can read or write, a directory in its place, or one
/// naming no decision record - phase A refuses (exit 29) and nothing is
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
        OptIn { src: Some(src), mode: "0600", owner: "0" }
    }
    let cases: [(&str, OptIn); 7] = [
        ("no opt-in file", OptIn { src: None, mode: "0600", owner: "0" }),
        ("an opt-in of mode 0640", OptIn { src: Some(&s.opt_in), mode: "0640", owner: "0" }),
        ("an opt-in owned by uid 1", OptIn { src: Some(&s.opt_in), mode: "0600", owner: "1" }),
        ("a directory as its opt-in", OptIn { src: Some(&dir), mode: "0700", owner: "0" }),
        ("an opt-in naming no decision record", file(&no_decision)),
        ("an opt-in holding a line that is not its decision", file(&other_line)),
        ("an opt-in naming two decision records", file(&two)),
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

#[test]
fn s13_a_checker_that_rewrites_the_entry_file_during_the_witness_run_refuses() {
    let s = setup("s13");
    // The check child rewrites the entry file on its first `test` run and
    // then runs the real interpreter: the pristine-test TOCTOU (§5.4). The
    // envelope pins THIS wrapper's sha, so only the rewrite is out of order.
    let wrapper = s.root.join("ops/axon-rewriter");
    let mark = s.root.join("ops/rewrote");
    common::write_executable(
        &wrapper,
        format!(
            "#!/bin/sh\nif [ \"$1\" = test ] && [ ! -e '{m}' ]; then touch '{m}'; printf '// changed\\n' >> '{f}'; fi\nexec '{a}' \"$@\"\n",
            m = mark.display(),
            f = s.ws.join("broken.ax").display(),
            a = s.axon.display()
        ),
        0o755,
    );
    let real = s.axon.clone();
    let s = Setup {
        axon: wrapper.clone(),
        ..setup_clone(&s)
    };
    let e = envelope_for(&s);
    mint(&s, &e);
    let o = run_with(&s, &wrapper, &[], &[]);
    let _ = real;
    assert!(mark.exists(), "the witness run never ran");
    assert_eq!(
        o.code, 29,
        "ATTACK: NH-R1 S13 a file changed during the witness run was graded\n{}{}",
        o.stdout, o.stderr
    );
    // The check right after the witness run (it names both digests), not
    // only the later walk-start compare: no child runs between them on
    // bytes nobody verified.
    assert!(
        o.stderr
            .contains("broken.ax changed after verification (sha256 "),
        "ATTACK: NH-R1 S13 a file changed during the witness run was graded\n{}",
        o.stderr
    );
    let _ = std::fs::remove_dir_all(&s.root);
}
