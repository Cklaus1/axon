//! C9 round 3 (amendment 46): candidate bytes cannot end, rewrite or steer
//! the operator's check from BELOW it, through the REAL runner
//! (`axon_psv::runner::run`) and the real interpreter, with the effect
//! ceiling Fabric derives for the developer grant. The host re-derives each
//! outcome under its own key, exactly as `psv::derive` does.
//!
//! * PSV-1: a sealed candidate's effect handler answered or aborted builtins
//!   the OPERATOR's closure performed (keyed pass for the wrong answer 7).
//! * PSV-1, RNG: an operator closure the candidate runs drew from the
//!   operator's stream, so the candidate chose how far it advanced.
//! * PSV-3: a `?` on a type-confused `None` ended the operator's test before
//!   its assert, and a genuine completion token was minted.

#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use axon_psv::runner::{run, RunnerConfig};
use axon_psv::*;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// The guest policy the manifest names and the runner is given (PSV-6, A87):
/// the ceiling this test used to pass directly (the runner drops `Exec`).
const POLICY: &str = r#"{"schema":"axon-vm-mmds/1","allowed_effects":["Net","AI","IO","Exec"]}"#;

fn axon() -> PathBuf {
    // The interpreter as cargo has made it current for THIS tree, never a
    // stale `target/debug/axon` (tests/script_spawn::workspace_bin).
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

/// What the guest decided, the host's keyed outcome, and the test's stdout.
struct Seen {
    status: GuestStatus,
    host: Option<bool>,
    stdout: String,
    /// The test's stderr: a CHECK-TIME refusal goes here, not to stdout.
    stderr: String,
    /// The run's temp directory, which paths in a message name.
    root: String,
}

/// Run `test` of `suite` (entry `main.ax`, plus `files` beside it) against
/// `candidate` (`sol.ax`) through the real runner.
fn check(suite: &str, files: &[(&str, &str)], candidate: &str, test: &str) -> Seen {
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let (cand, suite_dir, job, out) = (
        d.path().join("candidate"),
        d.path().join("suite"),
        d.path().join("job"),
        d.path().join("out"),
    );
    for p in [&cand, &suite_dir, &job, &out] {
        std::fs::create_dir(p).unwrap();
    }
    std::fs::write(cand.join("sol.ax"), candidate).unwrap();
    std::fs::write(suite_dir.join("main.ax"), suite).unwrap();
    for (name, body) in files {
        // `cand/<path>`: a further file of the CANDIDATE's tree.
        let p = match name.strip_prefix("cand/") {
            Some(rest) => cand.join(rest),
            None => suite_dir.join(name),
        };
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    let secret: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
    let sp = job.join("completion-secret");
    std::fs::write(&sp, secret).unwrap();
    let q = Quota::default();
    let sv = axon_workspace_recipe::tree_version_ref(&suite_dir, &q).unwrap();
    let cv = axon_workspace_recipe::tree_version_ref(&cand, &q).unwrap();
    let m = LaunchManifest {
        schema: LAUNCH_MANIFEST_SCHEMA.into(),
        operation_id: "op".into(),
        task_id: "t".into(),
        trial_id: "tr".into(),
        attempt_id: "a".into(),
        backend_profile: PROTECTED_PROFILE.into(),
        fabric_revision: "f".repeat(40),
        verifier_sha256: "d".repeat(64),
        qualification_sha256: "1".repeat(64),
        host_config_sha256: "2".repeat(64),
        launcher_sha256: "3".repeat(64),
        firecracker_sha256: "4".repeat(64),
        profile_manifest_sha256: "5".repeat(64),
        guest: GuestDigests {
            kernel_sha256: "6".repeat(64),
            rootfs_sha256: "7".repeat(64),
            axon_sha256: "8".repeat(64),
            init_sha256: "9".repeat(64),
        },
        policy_sha256: sha256_hex(POLICY.as_bytes()),
        suite: SuiteRef {
            id: "s".into(),
            version: sv.clone(),
            entry: "main.ax".into(),
            test: test.into(),
            tree_digest: sv,
            registry_sha256: "c".repeat(64),
        },
        candidate: CandidateRef {
            workspace_version: cv.clone(),
            tree_digest: cv,
        },
        completion: Completion {
            scheme: COMPLETION_SCHEME.into(),
        },
        observation_nonce: "e".repeat(32),
        authority: AuthorityRef {
            epoch: 0,
            tenant_id: "tenant-t".into(),
            task_family: "family-f".into(),
        },
        limits: Limits {
            wall_time_ms: 120_000,
            output_bytes: 1 << 20,
        },
    };
    std::fs::write(job.join("launch-manifest.json"), m.bytes()).unwrap();
    let axon = axon();
    let cfg = RunnerConfig {
        manifest: job.join("launch-manifest.json"),
        secret: sp,
        candidate: cand,
        suite: suite_dir,
        out,
        axon: axon.clone(),
        runner_exe: axon,
        expected_manifest_sha256: m.digest(),
        drop: None,
        // The developer-profile grant's ceiling as Fabric derives it (Exec is
        // then stripped by the runner).
        policy: Some(POLICY.as_bytes().to_vec()),
    };
    let v = run(&cfg);
    let stdout = std::fs::read_to_string(cfg.out.join("test-stdout")).unwrap_or_default();
    let stderr = std::fs::read_to_string(cfg.out.join("test-stderr")).unwrap_or_default();
    let host = keyed_outcome(&stdout, test, &completion_key(&secret, &m));
    Seen {
        status: v.status,
        host,
        stdout,
        stderr,
        root: d.path().display().to_string(),
    }
}

/// The operator's listener asserts the value it is handed; the second suite
/// checks it against the operator's fixture file.
const SUITE_PRINT: &str = r#"mod sol
use sol.{visit}

@[test]
fn accept() {
    visit(|v: i64| {
        println("listener saw {to_str(v)}")
        assert_eq(v, 42)
    })
}
"#;
const SUITE_FIXTURE: &str = r#"mod sol
use sol.{visit}

@[test]
fn accept() {
    visit(|v: i64| {
        let raw = match read_file("expected.txt") { Ok(s) => s  Err(e) => "" }
        let want = match parse_int(str_trim(raw)) { Ok(n) => n  Err(e) => -1 }
        assert_eq(v, want)
    })
}
"#;
const FIXTURE: &[(&str, &str)] = &[("expected.txt", "42\n")];
const GOOD: &str = "pub fn visit(cb: fn(i64) -> ()) {\n    cb(42)\n}\n";
const WRONG: &str = "pub fn visit(cb: fn(i64) -> ()) {\n    cb(7)\n}\n";
/// The review's abort variant: the arm returns without resuming at the
/// listener's `println`, skipping its assert.
const ABORT: &str = "pub fn visit(cb: fn(i64) -> ()) {\n    let _ = with handler { on IO(p) => 0 } {\n        cb(7)\n        0\n    }\n}\n";
/// The review's resume variant: the arm answers the operator's `read_file`
/// with bytes the candidate chose.
const RESUME: &str = "pub fn visit(cb: fn(i64) -> ()) {\n    with handler { on IO(p) => resume(Ok(\"7\")) } {\n        cb(7)\n    }\n}\n";

/// Controls: each suite passes the right answer and fails 7 with a keyed
/// verdict, so the attacks below meet a live check.
#[test]
fn the_listener_checks_discriminate() {
    for (suite, files) in [(SUITE_PRINT, &[][..]), (SUITE_FIXTURE, FIXTURE)] {
        let s = check(suite, files, GOOD, "accept");
        assert_eq!(
            (s.status, s.host),
            (GuestStatus::Passed, Some(true)),
            "control: {}",
            s.stdout
        );
        let s = check(suite, files, WRONG, "accept");
        assert_eq!(
            (s.status, s.host),
            (GuestStatus::Failed, Some(false)),
            "control: {}",
            s.stdout
        );
    }
}

#[test]
fn a_sealed_handler_cannot_abort_the_operators_listener() {
    let s = check(SUITE_PRINT, &[], ABORT, "accept");
    assert!(
        s.status != GuestStatus::Passed && s.host != Some(true),
        "ATTACK: the candidate's handler aborted the operator's listener and 7 got a keyed pass: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    assert_eq!(s.host, Some(false), "a keyed FAILURE: {}", s.stdout);
}

#[test]
fn a_sealed_handler_cannot_answer_the_operators_fixture_read() {
    let s = check(SUITE_FIXTURE, FIXTURE, RESUME, "accept");
    assert!(
        s.status != GuestStatus::Passed && s.host != Some(true),
        "ATTACK: the candidate's handler chose the operator's fixture bytes and 7 got a keyed pass: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    assert_eq!(s.host, Some(false), "a keyed FAILURE: {}", s.stdout);
}

/// PSV-1, RNG: the operator hands the candidate a `tick` that runs the
/// operator's random search. Under a Fabric ceiling the stream is reached by
/// `goal_run_random` (row `{AI,Net,IO}`), the route C8 steered. The draw is
/// refused, so the check fails closed; before, the tick advanced the
/// operator's stream by however many times the candidate called it.
#[test]
fn operator_code_the_candidate_runs_cannot_draw_the_operators_stream() {
    let suite = r#"mod sol
use sol.{warm}

@[adaptive]
fn probe(x: i64) -> i64 { x }

@[test]
fn accept() {
    warm(|| { let _ = goal_run_random("probe", 1000000.0, 1, 0, 1000000) })
    let found = goal_run_random("probe", 1000000.0, 1, 0, 1000000)
    println("operator challenge {to_str(found)}")
}
"#;
    let steer = "pub fn warm(tick: fn() -> ()) { tick() tick() tick() tick() tick() }\n";
    let s = check(suite, &[], steer, "accept");
    assert!(
        s.stdout.contains("operator's random stream") && s.host != Some(true),
        "ATTACK: an operator closure the candidate ran five times drew from the operator's stream: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    // Control: the operator's own draws outside any sealed call still work.
    let idle = "pub fn warm(tick: fn() -> ()) { }\n";
    let s = check(suite, &[], idle, "accept");
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Passed, Some(true)),
        "control: {}",
        s.stdout
    );
}

/// PSV-3, the review's exact candidate and suite.
const SUITE_Q: &str = r#"mod sol
use sol.{solve}

@[test]
fn t_solve() -> Result<i64, str> {
    let v = solve(21)?
    assert_eq(v, 42)
    Ok(v)
}
"#;

#[test]
fn a_test_ended_early_by_a_confused_question_mark_gets_no_token() {
    let confusing = "pub fn solve(x: i64) -> Result<i64, str> {\n    let d = dict_new()\n    let n: Option<i64> = None\n    dict_set(d, \"k\", n)\n    match dict_get(d, \"k\") {\n        Some(v) => v\n        None => Err(\"unreachable\")\n    }\n}\n";
    let s = check(SUITE_Q, &[], confusing, "t_solve");
    assert!(
        s.status != GuestStatus::Passed && s.host != Some(true),
        "ATTACK: a test that ended early at `?` got a completion token and a keyed pass: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    // Controls: an honest pass is Passed and keyed; an honest Err is no pass.
    let s = check(
        SUITE_Q,
        &[],
        "pub fn solve(x: i64) -> Result<i64, str> { Ok(x * 2) }\n",
        "t_solve",
    );
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Passed, Some(true)),
        "control: {}",
        s.stdout
    );
    let s = check(
        SUITE_Q,
        &[],
        "pub fn solve(x: i64) -> Result<i64, str> { Err(\"no\") }\n",
        "t_solve",
    );
    assert!(
        s.status != GuestStatus::Passed && s.host != Some(true),
        "control: an honest Err is not a pass: {:?} {:?}",
        s.status,
        s.host
    );
}

/// C9 round 4, PSV-1 (amendment 53, matrix A86): the candidate never
/// chooses the code that runs under the operator's judging method. The
/// review's suite and candidates, exactly: `r.ok()` is meant to run the
/// operator's `impl Judge for i64`; a `dict_get` type confusion (its value
/// type is free) handed the operator a `Fake`, and the method call — which
/// dispatches on the RUNTIME type — ran the candidate's `Fake::ok`. Keyed
/// PASS through this runner, which Fabric's `psv::derive` would sign.
const SUITE_DISPATCH: &str = r#"mod sol
use sol.{solve}

trait Judge {
    fn ok(self) -> bool
}

impl Judge for i64 {
    fn ok(self: i64) -> bool { self == 9 }
}

impl Judge for bool {
    fn ok(self: bool) -> bool { self }
}

@[test]
fn accept() {
    let r: i64 = solve(3)
    assert(r.ok())
}
"#;
/// The same suite with the receiver UNPINNED: what the operator wrote before
/// it pinned `let r: i64` (a type the candidate declared pins nothing).
const SUITE_DISPATCH_LOOSE: &str = r#"mod sol
use sol.{solve}

trait Judge {
    fn ok(self) -> bool
}

impl Judge for i64 {
    fn ok(self: i64) -> bool { self == 9 }
}

impl Judge for bool {
    fn ok(self: bool) -> bool { self }
}

@[test]
fn accept() {
    let r = solve(3)
    assert(r.ok())
}
"#;
const DISPATCH_GOOD: &str = "pub fn solve(n: i64) -> i64 { n * n }\n";
const DISPATCH_WRONG: &str = "pub fn solve(n: i64) -> i64 { n + 1 }\n";
const FAKE_OK: &str = "type Fake = { v: i64 }\ntrait Mine {\n    fn ok(self) -> bool\n}\nimpl Mine for Fake {\n    fn ok(self: Fake) -> bool { true }\n}\n";

/// The reviewer's second variant: the operator's impl is on the candidate's
/// own interface type `Point`.
const SUITE_POINT: &str = r#"mod sol
use sol.{Point, solve}

trait Judge {
    fn ok(self) -> bool
}

impl Judge for Point {
    fn ok(self: Point) -> bool { self.x * self.x == 9 && self.y == 0 }
}

@[test]
fn accept() {
    let p = solve(3)
    assert(p.ok())
}
"#;

fn refused_unkeyed(s: &Seen) -> bool {
    s.status != GuestStatus::Passed && s.host != Some(true)
}

#[test]
fn the_candidate_never_chooses_the_operators_judging_method() {
    // Controls: the suites discriminate.
    for (suite, good, wrong) in [
        (SUITE_DISPATCH, DISPATCH_GOOD.to_string(), DISPATCH_WRONG.to_string()),
        (
            SUITE_POINT,
            "pub type Point = { x: i64, y: i64 }\npub fn solve(n: i64) -> Point { Point { x: n, y: 0 } }\n"
                .to_string(),
            "pub type Point = { x: i64, y: i64 }\npub fn solve(n: i64) -> Point { Point { x: n + 1, y: 5 } }\n"
                .to_string(),
        ),
    ] {
        let s = check(suite, &[], &good, "accept");
        assert_eq!(
            (s.status, s.host),
            (GuestStatus::Passed, Some(true)),
            "control: {}",
            s.stdout
        );
        let s = check(suite, &[], &wrong, "accept");
        assert_eq!(
            (s.status, s.host),
            (GuestStatus::Failed, Some(false)),
            "control: {}",
            s.stdout
        );
    }
    let attacks: [(&str, &str, String); 4] = [
        (
            "the review's Fake from a `-> i64` fn (type confusion)",
            SUITE_DISPATCH,
            format!("{FAKE_OK}pub fn solve(n: i64) -> i64 {{\n    let d = dict_new()\n    dict_set(d, \"k\", Fake {{ v: n + 1 }})\n    match dict_get(d, \"k\") {{\n        Some(v) => v\n        None => 0\n    }}\n}}\n"),
        ),
        (
            "the review's second variant: a Fake from a `-> Point` fn",
            SUITE_POINT,
            format!("pub type Point = {{ x: i64, y: i64 }}\n{}pub fn solve(n: i64) -> Point {{\n    let d = dict_new()\n    dict_set(d, \"k\", Fake {{ v: n + 1 }})\n    match dict_get(d, \"k\") {{\n        Some(v) => v\n        None => Point {{ x: 0, y: 0 }}\n    }}\n}}\n", FAKE_OK),
        ),
        (
            "no confusion: the candidate DECLARES `-> Fake`",
            SUITE_DISPATCH_LOOSE,
            format!("{FAKE_OK}pub fn solve(n: i64) -> Fake {{ Fake {{ v: n }} }}\n"),
        ),
        (
            "a confused `true` selecting the operator's own lenient impl",
            SUITE_DISPATCH,
            "pub fn solve(n: i64) -> i64 {\n    let d = dict_new()\n    dict_set(d, \"k\", true)\n    match dict_get(d, \"k\") {\n        Some(v) => v\n        None => 0\n    }\n}\n".to_string(),
        ),
    ];
    for (why, suite, cand) in attacks {
        let s = check(suite, &[], &cand, "accept");
        assert!(
            refused_unkeyed(&s),
            "ATTACK: the candidate chose the code under the operator's judging method ({why}) and got a keyed pass: {:?} {:?} {}",
            s.status,
            s.host,
            s.stdout
        );
        assert_eq!(s.host, Some(false), "a keyed FAILURE ({why}): {}", s.stdout);
    }
}

/// C9 round 4b, PSV-1 (amendment 60, matrix A86): the review's two new
/// members of the class, through the real runner. (1) The cast took every
/// integer width for one kind while the dispatch keys on the width: a `u8`
/// laundered into a fn declared `-> i64` — and kept by the suite's own
/// `let r: i64` pin — ran the operator's lenient `impl Judge for u8`.
const SUITE_WIDTH: &str = r#"mod sol
use sol.{solve}

trait Judge {
    fn ok(self) -> bool
}

impl Judge for i64 {
    fn ok(self: i64) -> bool { self == 9 }
}

impl Judge for u8 {
    fn ok(self: u8) -> bool { true }
}

fn judge(x: i64) -> bool { x.ok() }

@[test]
fn accept() {
    let r: i64 = solve(3)
    assert(judge(r))
}

@[test]
fn accept_bare() {
    let r: i64 = solve(3)
    assert(r.ok())
}
"#;

/// (2) A non-closure crossed a declared fn type, and a call through the
/// local fell through to NAME resolution: the operator's own reference
/// `fn square` answered for the candidate. The second suite reaches the call
/// with no declared fn type in the way (a `Dict`'s values are untyped).
const SUITE_FNREF: &str = r#"mod sol
use sol.{make_square}

fn square(n: i64) -> i64 { n * n }

@[test]
fn accept() {
    let square = make_square()
    assert(square(3) == 9 && square(5) == 25)
}
"#;

const SUITE_FNTABLE: &str = r#"mod sol
use sol.{table}

fn square(n: i64) -> i64 { n * n }

@[test]
fn accept() {
    match dict_get(table(), "sq") {
        Some(square) => assert(square(3) == 9 && square(5) == 25)
        None => assert(false)
    }
}
"#;

#[test]
fn the_candidate_never_selects_the_operators_code_by_width_or_by_name() {
    let table = |v: &str| {
        format!("pub fn table() -> Dict {{\n    let d = dict_new()\n    dict_set(d, \"sq\", {v})\n    d\n}}\n")
    };
    // Controls: each suite discriminates.
    for (suite, test, good, wrong) in [
        (
            SUITE_WIDTH,
            "accept",
            DISPATCH_GOOD.to_string(),
            DISPATCH_WRONG.to_string(),
        ),
        (
            SUITE_WIDTH,
            "accept_bare",
            DISPATCH_GOOD.to_string(),
            DISPATCH_WRONG.to_string(),
        ),
        (
            SUITE_FNREF,
            "accept",
            "pub fn make_square() -> fn(i64) -> i64 { |n: i64| n * n }\n".to_string(),
            "pub fn make_square() -> fn(i64) -> i64 { |n: i64| n + 1 }\n".to_string(),
        ),
        (
            SUITE_FNTABLE,
            "accept",
            table("|n: i64| n * n"),
            table("|n: i64| n + 1"),
        ),
    ] {
        let s = check(suite, &[], &good, test);
        assert_eq!(
            (s.status, s.host),
            (GuestStatus::Passed, Some(true)),
            "control: {}",
            s.stdout
        );
        let s = check(suite, &[], &wrong, test);
        assert_eq!(
            (s.status, s.host),
            (GuestStatus::Failed, Some(false)),
            "control: {}",
            s.stdout
        );
    }
    let width = "fn narrow(n: u8) -> u8 { n }\npub fn solve(n: i64) -> i64 {\n    let d = dict_new()\n    dict_set(d, \"k\", narrow(4 as u8))\n    match dict_get(d, \"k\") {\n        Some(v) => v\n        None => 0\n    }\n}\n";
    let attacks: [(&str, &str, &str, String); 4] = [
        ("a u8 at a declared i64, pinned by `let r: i64`", SUITE_WIDTH, "accept", width.to_string()),
        ("a u8 at a declared i64, unpinned", SUITE_WIDTH, "accept_bare", width.to_string()),
        (
            "a confused 0 at a declared fn type",
            SUITE_FNREF,
            "accept",
            "pub fn make_square() -> fn(i64) -> i64 {\n    let d = dict_new()\n    dict_set(d, \"k\", 0)\n    match dict_get(d, \"k\") {\n        Some(v) => v\n        None => |n: i64| n + 1\n    }\n}\n".to_string(),
        ),
        ("a 0 in a table, called through the local `square`", SUITE_FNTABLE, "accept", table("0")),
    ];
    for (why, suite, test, cand) in attacks {
        let s = check(suite, &[], &cand, test);
        assert!(
            refused_unkeyed(&s),
            "ATTACK: the candidate selected the operator's code ({why}) and got a keyed pass: {:?} {:?} {}",
            s.status,
            s.host,
            s.stdout
        );
        assert_eq!(s.host, Some(false), "a keyed FAILURE ({why}): {}", s.stdout);
    }
}

/// C9 round 4c, PSV-1 (amendment 72): the review's four candidates through
/// the real runner. Each left a type position at a seal crossing
/// undetermined — a free `T` of a generic `Chan<T>`, a fn with no declared
/// return type, a `Wrap<T>` whose argument was erased, a free `fn(T)` the
/// candidate calls the operator's closure through — and chose the runtime
/// type there, so the operator's lenient `u8` impl gave a keyed pass. GOOD
/// stays a keyed pass and WRONG a keyed failure on every suite; every attack
/// is refused without a keyed pass.
#[test]
fn an_undetermined_type_position_never_selects_the_operators_impl() {
    const J: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for u8 {\n    fn ok(self: u8) -> bool { true }\n}\n";
    const L: &str = "fn narrow(n: i64) -> u8 { n as u8 }\nfn stash(v: u8) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";
    let suite = |uses: &str, body: &str| {
        format!("mod sol\nuse sol.{{{uses}}}\n{J}@[test]\nfn accept() {{\n{body}\n}}\n")
    };
    let launder = |some: &str, none: &str| {
        format!("match dict_get(stash(narrow(4)), \"k\") {{\n        Some(v) => {some}\n        None => {none}\n    }}")
    };
    let chan = suite(
        "fill",
        "    let c = chan<i64>()\n    fill(c)\n    let r: i64 = c.recv()\n    assert(r.ok())",
    );
    let noret = suite("solve", "    let r: i64 = solve(3)\n    assert(r.ok())");
    let wrap = suite(
        "solve, Wrap",
        "    let w = solve(Wrap { v: 3 })\n    let x: i64 = w.v\n    assert(x.ok())",
    );
    let clos = suite("apply", "    assert(apply(|x: i64| x.ok()))");
    let cases: [(&str, &String, String, String, String); 4] = [
        (
            "a generic Chan<T> (B1)",
            &chan,
            "pub fn fill(c: Chan<i64>) { c.send(9) }\n".into(),
            "pub fn fill(c: Chan<i64>) { c.send(4) }\n".into(),
            format!("{L}pub fn fill<T>(c: Chan<T>) {{\n    {}\n}}\n", launder("c.send(v)", "{}")),
        ),
        (
            "no declared return type (B2)",
            &noret,
            "pub fn solve(n: i64) -> i64 { n * n }\n".into(),
            "pub fn solve(n: i64) -> i64 { n + 1 }\n".into(),
            format!("{L}pub fn solve(n: i64) {{\n    {}\n}}\n", launder("v", "0")),
        ),
        (
            "an erased Wrap<T> argument (B3)",
            &wrap,
            "pub type Wrap<T> = { v: T }\npub fn solve(w: Wrap<i64>) -> Wrap<i64> { Wrap { v: w.v * w.v } }\n".into(),
            "pub type Wrap<T> = { v: T }\npub fn solve(w: Wrap<i64>) -> Wrap<i64> { Wrap { v: w.v + 1 } }\n".into(),
            format!(
                "{L}pub type Wrap<T> = {{ v: T }}\npub fn solve<T>(w: Wrap<T>) -> Wrap<T> {{\n    {}\n}}\n",
                launder("Wrap { v: v }", "w")
            ),
        ),
        (
            "the operator's closure through a free fn(T)",
            &clos,
            "pub fn apply(f: fn(i64) -> bool) -> bool { f(9) }\n".into(),
            "pub fn apply(f: fn(i64) -> bool) -> bool { f(4) }\n".into(),
            format!("{L}pub fn apply<T>(f: fn(T) -> bool) -> bool {{\n    {}\n}}\n", launder("f(v)", "false")),
        ),
    ];
    for (why, suite, good, wrong, attack) in cases {
        let s = check(suite, &[], &good, "accept");
        assert_eq!(
            (s.status, s.host),
            (GuestStatus::Passed, Some(true)),
            "control ({why}): {}",
            s.stdout
        );
        let s = check(suite, &[], &wrong, "accept");
        assert_eq!(
            (s.status, s.host),
            (GuestStatus::Failed, Some(false)),
            "control ({why}): {}",
            s.stdout
        );
        let s = check(suite, &[], &attack, "accept");
        assert!(
            refused_unkeyed(&s),
            "ATTACK: the candidate chose the operator's impl at an undetermined position ({why}) and got a keyed pass: {:?} {:?} {}",
            s.status,
            s.host,
            s.stdout
        );
    }
}

/// C9 round 4c, PSV-1 (amendment 72 part 2): a `Dict` carries no element
/// types, so a candidate overwrote the key the operator held with a laundered
/// `u8` and the operator's untyped `dict_get(..).ok()` ran its lenient `u8`
/// impl. The operator's dict is snapshotted when handed over; a key it held
/// may not come back with a value of another type. Through the real runner.
#[test]
fn a_dict_entry_the_operator_held_is_never_retyped_by_the_candidate() {
    const J: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for u8 {\n    fn ok(self: u8) -> bool { true }\n}\n";
    const L: &str = "fn narrow(n: i64) -> u8 { n as u8 }\nfn stash(v: u8) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";
    let suite = format!(
        "mod sol\nuse sol.{{solve}}\n{J}@[test]\nfn accept() {{\n    let d = dict_new()\n    dict_set(d, \"a\", 3)\n    solve(d)\n    match dict_get(d, \"a\") {{\n        Some(x) => {{\n            let y: i64 = x\n            assert(y.ok())\n        }}\n        None => assert(false)\n    }}\n}}\n"
    );
    let good = "pub fn solve(d: Dict) { dict_set(d, \"a\", 9) }\n".to_string();
    let wrong = "pub fn solve(d: Dict) { dict_set(d, \"a\", 4) }\n".to_string();
    let attack = format!(
        "{L}pub fn solve(d: Dict) {{\n    match dict_get(stash(narrow(4)), \"k\") {{\n        Some(v) => {{ dict_set(d, \"a\", v) }}\n        None => {{}}\n    }}\n}}\n"
    );
    let s = check(&suite, &[], &good, "accept");
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Passed, Some(true)),
        "control: {}",
        s.stdout
    );
    let s = check(&suite, &[], &wrong, "accept");
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Failed, Some(false)),
        "control: {}",
        s.stdout
    );
    let s = check(&suite, &[], &attack, "accept");
    assert!(
        refused_unkeyed(&s),
        "ATTACK: the candidate retyped the operator's dict entry to a u8 and got a keyed pass: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
}

/// C9 round 5, PSV-1 (amendment 78), through the real runner: a candidate
/// REPLACES a dict the operator held with its own carrying a laundered `u8`
/// (the round-5 BLOCKER, case c1), and fills a `None` the operator held
/// (case c4). GOOD stays a keyed pass and WRONG a keyed failure.
#[test]
fn a_replaced_or_filled_position_is_judged_by_what_the_operator_held() {
    const J: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for u8 {\n    fn ok(self: u8) -> bool { true }\n}\n";
    let suite = |body: &str| {
        format!("mod sol\nuse sol.{{solve}}\n{J}@[test]\nfn accept() {{\n{body}\n}}\n")
    };
    let replace = suite("    let inner = dict_new()\n    dict_set(inner, \"x\", 3)\n    let d = dict_new()\n    dict_set(d, \"inner\", inner)\n    solve(d)\n    match dict_get(d, \"inner\") {\n        Some(i) => match dict_get(i, \"x\") {\n            Some(v) => {\n                let y: i64 = v\n                assert(y.ok())\n            }\n            None => assert(false)\n        }\n        None => assert(false)\n    }");
    let fill = suite("    let d = dict_new()\n    dict_set(d, \"best\", Some(0))\n    solve(d)\n    match dict_get(d, \"best\") {\n        Some(o) => match o {\n            Some(v) => {\n                let y: i64 = v\n                assert(y.ok())\n            }\n            None => assert(false)\n        }\n        None => assert(false)\n    }");
    let hold = suite("    let d = dict_new()\n    dict_set(d, \"best\", None)\n    solve(d)\n    match dict_get(d, \"best\") {\n        Some(o) => match o {\n            Some(v) => {\n                let y: i64 = v\n                assert(y.ok())\n            }\n            None => assert(false)\n        }\n        None => assert(false)\n    }");
    let mk = |x: &str| {
        format!("pub fn solve(d: Dict) {{\n    let n = dict_new()\n    dict_set(n, \"x\", {x})\n    dict_set(d, \"inner\", n)\n}}\n")
    };
    let put = |x: &str| format!("pub fn solve(d: Dict) {{ dict_set(d, \"best\", Some({x})) }}\n");
    let cases: [(&str, &String, String, String, String); 2] = [
        (
            "a candidate-built dict at a held key (c1)",
            &replace,
            mk("9"),
            mk("4"),
            mk("4 as u8"),
        ),
        (
            "None filled with a u8 (c4)",
            &hold,
            put("9"),
            put("4"),
            put("4 as u8"),
        ),
    ];
    for (why, suite, good, wrong, attack) in cases {
        if why.contains("c4") {
            // The honest control for a placeholder is a TYPED one.
            let s = check(&fill, &[], &put("9"), "accept");
            assert_eq!(
                (s.status, s.host),
                (GuestStatus::Passed, Some(true)),
                "control ({why}): {}",
                s.stdout
            );
            let s = check(&fill, &[], &put("4"), "accept");
            assert_eq!(
                (s.status, s.host),
                (GuestStatus::Failed, Some(false)),
                "control ({why}): {}",
                s.stdout
            );
        } else {
            let s = check(suite, &[], &good, "accept");
            assert_eq!(
                (s.status, s.host),
                (GuestStatus::Passed, Some(true)),
                "control ({why}): {}",
                s.stdout
            );
            let s = check(suite, &[], &wrong, "accept");
            assert_eq!(
                (s.status, s.host),
                (GuestStatus::Failed, Some(false)),
                "control ({why}): {}",
                s.stdout
            );
        }
        let s = check(suite, &[], &attack, "accept");
        assert!(
            refused_unkeyed(&s),
            "ATTACK: the candidate chose a runtime type at a position the operator held ({why}) and got a keyed pass: {:?} {:?} {}",
            s.status,
            s.host,
            s.stdout
        );
    }
}

/// C9 round 6, PSV-1 (amendment 83), through the real runner: operator code
/// never dispatches an operator impl on a value whose type nothing on the
/// operator side determined (a read from a dict the candidate wrote), and a
/// candidate closure that captured the operator's dict cannot retype it
/// against a stale snapshot. A suite that PINS the read (`let y: i64 = x`)
/// keeps GOOD a keyed pass and WRONG a keyed failure; the pinned `u8` is
/// refused by the cast.
#[test]
fn operator_code_never_dispatches_on_an_untyped_read_and_a_pinned_suite_passes() {
    const J: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for u8 {\n    fn ok(self: u8) -> bool { true }\n}\n";
    let suite = |body: &str| {
        format!("mod sol\nuse sol.{{solve, make}}\n{J}@[test]\nfn accept() {{\n{body}\n}}\n")
    };
    let read = |pin: bool| {
        let r = if pin {
            "Some(x) => {\n            let y: i64 = x\n            assert(y.ok())\n        }"
        } else {
            "Some(x) => assert(x.ok())"
        };
        suite(&format!("    let out = dict_new()\n    solve(out)\n    match dict_get(out, \"result\") {{\n        {r}\n        None => assert(false)\n    }}"))
    };
    let put = |v: &str| {
        format!("pub fn make() -> i64 {{ 0 }}\npub fn solve(out: Dict) {{ dict_set(out, \"result\", {v}) }}\n")
    };
    // The pinned suite: GOOD, WRONG, and the u8 refused.
    let pinned = read(true);
    let s = check(&pinned, &[], &put("9"), "accept");
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Passed, Some(true)),
        "control: {}",
        s.stdout
    );
    let s = check(&pinned, &[], &put("4"), "accept");
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Failed, Some(false)),
        "control: {}",
        s.stdout
    );
    let s = check(&pinned, &[], &put("4 as u8"), "accept");
    assert!(
        refused_unkeyed(&s),
        "ATTACK: a u8 passed the pinned suite: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    // The unpinned suite: the dispatch is refused whatever the candidate stores.
    let loose = read(false);
    let s = check(&loose, &[], &put("4 as u8"), "accept");
    assert!(
        refused_unkeyed(&s),
        "ATTACK: operator code dispatched on the candidate's u8 and got a keyed pass: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    // The captured-dict closure against a stale snapshot (case d1).
    let stale = suite("    let d = dict_new()\n    let f = make(d)\n    dict_set(d, \"answer\", 3)\n    f()\n    match dict_get(d, \"answer\") {\n        Some(v) => {\n            let y: i64 = v\n            assert(y.ok())\n        }\n        None => assert(false)\n    }");
    let mk = |v: &str| {
        format!("pub fn solve(out: Dict) {{}}\npub fn make(d: Dict) -> fn() -> i64 {{\n    || {{\n        dict_set(d, \"answer\", {v})\n        0\n    }}\n}}\n")
    };
    let s = check(&stale, &[], &mk("9"), "accept");
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Passed, Some(true)),
        "control: {}",
        s.stdout
    );
    let s = check(&stale, &[], &mk("4"), "accept");
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Failed, Some(false)),
        "control: {}",
        s.stdout
    );
    let s = check(&stale, &[], &mk("4 as u8"), "accept");
    assert!(
        refused_unkeyed(&s),
        "ATTACK: a captured-dict closure retyped the entry: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
}

/// C9 round 7, PSV-1 (amendment 88), through the real runner: a type the
/// CANDIDATE declared (`-> u8`) does not determine the operator's receiver, a
/// local closure named like an operator fn is not that fn, and a trait-name
/// annotation pins nothing. The operator's own `let r: i64 = solve(3)` is the
/// pin: GOOD a keyed pass, WRONG a keyed failure, the candidate's u8 refused.
#[test]
fn operator_code_never_dispatches_on_a_type_the_candidate_declared() {
    const J: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for u8 {\n    fn ok(self: u8) -> bool { true }\n}\n";
    let suite = |body: &str| {
        format!("mod sol\nuse sol.{{solve}}\n{J}fn f() -> i64 {{ 5 }}\n@[test]\nfn accept() {{\n{body}\n}}\n")
    };
    let pinned = suite("    let r: i64 = solve(3)\n    assert(r.ok())");
    let s = check(
        &pinned,
        &[],
        "pub fn solve(n: i64) -> i64 { 9 }\n",
        "accept",
    );
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Passed, Some(true)),
        "control: {}",
        s.stdout
    );
    let s = check(
        &pinned,
        &[],
        "pub fn solve(n: i64) -> i64 { 4 }\n",
        "accept",
    );
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Failed, Some(false)),
        "control: {}",
        s.stdout
    );
    let s = check(
        &pinned,
        &[],
        "pub fn solve(n: i64) -> u8 { 4 as u8 }\n",
        "accept",
    );
    assert!(
        refused_unkeyed(&s),
        "ATTACK: a u8 passed the i64 pin: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    // B1: the candidate's own declaration.
    let loose = suite("    assert(solve(3).ok())");
    let s = check(
        &loose,
        &[],
        "pub fn solve(n: i64) -> u8 { 4 as u8 }\n",
        "accept",
    );
    assert!(
        refused_unkeyed(&s),
        "ATTACK: a candidate-declared u8 selected the operator's impl: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    // B2: a closure stored by the candidate, called through a local named `f`.
    let named = suite("    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"f\") {\n        Some(f) => assert(f().ok())\n        None => assert(false)\n    }");
    let s = check(
        &named,
        &[],
        "pub fn solve(d: Dict) { dict_set(d, \"f\", || 4 as u8) }\n",
        "accept",
    );
    assert!(
        refused_unkeyed(&s),
        "ATTACK: a local `f` was judged by the operator's global f: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    // B3: a trait-name annotation.
    let tr = suite("    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(v) => {\n            let y: Judge = v\n            assert(y.ok())\n        }\n        None => assert(false)\n    }");
    let s = check(
        &tr,
        &[],
        "pub fn solve(d: Dict) { dict_set(d, \"k\", 4 as u8) }\n",
        "accept",
    );
    assert!(
        refused_unkeyed(&s),
        "ATTACK: a trait annotation pinned the candidate's u8: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
}

// ---- C9 round 8, PSV1F: the features origin/main added, merged without PCI review ----

const J8: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for u8 {\n    fn ok(self: u8) -> bool { true }\n}\n";

fn passed(s: &Seen, what: &str) {
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Passed, Some(true)),
        "control {what}: {}",
        s.stdout
    );
}

/// A candidate fn that writes through `&mut` hands the operator the parameter's
/// FINAL value. It is cast against the declared parameter type at the edge back
/// (dispatch arm and width arm), whether the operator's array is annotated or
/// not, and the operator's un-re-pinned read after the call is refused.
#[test]
fn a_mut_write_through_value_is_cast_at_the_seal_edge_back_and_the_operand_is_open() {
    let fill = |v: &str| {
        format!("pub fn fill(a: &mut [i64]) {{\n    let d = dict_new()\n    dict_set(d, \"k\", {v})\n    match dict_get(d, \"k\") {{\n        Some(x) => {{ a[0] = x }}\n        None => {{ }}\n    }}\n}}\n")
    };
    let suite = |decl: &str, read: &str| {
        format!("mod sol\nuse sol.{{fill}}\n{J8}@[test]\nfn accept() {{\n    let mut a{decl} = [1, 2, 3]\n    fill(&mut a)\n{read}\n}}\n")
    };
    let pinned_read = "    let b: [i64] = a\n    assert(b[0].ok())";
    let width_read = "    let b: [i64] = a\n    assert((b[0] << 1) == 254)";
    // Dispatch arm: GOOD passes, WRONG fails, the u8 is refused.
    for (decl, read) in [("", pinned_read), (": [i64]", pinned_read)] {
        let s = check(&suite(decl, read), &[], &fill("9"), "accept");
        passed(&s, "good");
        let s = check(&suite(decl, read), &[], &fill("4"), "accept");
        assert_eq!(
            (s.status, s.host),
            (GuestStatus::Failed, Some(false)),
            "control wrong: {}",
            s.stdout
        );
        let s = check(&suite(decl, read), &[], &fill("4 as u8"), "accept");
        assert!(
            refused_unkeyed(&s),
            "ATTACK: a `&mut` u8 passed the dispatch suite ({decl:?}): {:?} {:?} {}",
            s.status,
            s.host,
            s.stdout
        );
    }
    // The annotated, un-re-pinned read (the dispatch rule cannot help: only the cast does).
    let s = check(
        &suite(": [i64]", "    assert(a[0].ok())"),
        &[],
        &fill("4 as u8"),
        "accept",
    );
    assert!(
        refused_unkeyed(&s),
        "ATTACK: a `&mut` u8 passed an ANNOTATED operator array: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    // The honest cost, stated: an un-re-pinned read after `&mut` is refused even for an honest write.
    let s = check(
        &suite("", "    assert(a[0].ok())"),
        &[],
        &fill("9"),
        "accept",
    );
    assert!(
        refused_unkeyed(&s),
        "the open-operand rule is not live: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    // Width arm: 127 << 1 == 254 passes, 255 as u8 (wraps to 254) is refused.
    let s = check(&suite("", width_read), &[], &fill("127"), "accept");
    passed(&s, "width good");
    let s = check(&suite("", width_read), &[], &fill("255 as u8"), "accept");
    assert!(
        refused_unkeyed(&s),
        "ATTACK: a `&mut` 255 as u8 wrapped in the operator's shift: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    let s = check(
        &suite("", "    assert((a[0] << 1) == 254)"),
        &[],
        &fill("255 as u8"),
        "accept",
    );
    assert!(
        refused_unkeyed(&s),
        "ATTACK: the un-re-pinned width arm accepted a u8: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
}

/// A fn named in value position by a SEALED frame is refused when it is the
/// operator's (the call edge applies at creation: the static E0004 or the
/// runtime edge, whichever fires first through the runner), while the
/// candidate's own fn values and the operator's own pass.
#[test]
fn a_sealed_frame_cannot_take_an_operator_fn_as_a_value() {
    let suite = format!("mod sol\nuse sol.{{solve}}\n{J8}fn secret() -> i64 {{ 9 }}\n@[test]\nfn accept() {{\n    assert(solve() == 9)\n}}\n");
    let own = "pub fn mine() -> i64 { 9 }\npub fn solve() -> i64 {\n    let g = mine\n    g()\n}\n";
    passed(&check(&suite, &[], own, "accept"), "own fn value");
    let wrong = own.replace("{ 9 }\npub fn solve", "{ 4 }\npub fn solve");
    let s = check(&suite, &[], &wrong, "accept");
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Failed, Some(false)),
        "control wrong: {}",
        s.stdout
    );
    let s = check(
        &suite,
        &[],
        "pub fn solve() -> i64 {\n    let g = secret\n    g()\n}\n",
        "accept",
    );
    assert!(
        refused_unkeyed(&s),
        "ATTACK: the candidate ran the operator's `secret` as a fn value: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
}

/// Rc/copy-on-write arrays: a candidate's write to its by-value array parameter
/// never changes the suite's copy, nor the state a lent closure captured.
#[test]
fn a_candidates_write_to_its_array_parameter_never_reaches_the_operators_copy() {
    let suite = "mod sol\nuse sol.{solve}\n@[test]\nfn accept() {\n    let fix = [1, 2, 3]\n    let f = || fix[0]\n    solve(fix)\n    assert_eq(fix[0], 1)\n    assert_eq(f(), 1)\n}\n";
    passed(
        &check(
            suite,
            &[],
            "pub fn solve(a: [i64]) { let mut b = a\n    b[0] = 99 }\n",
            "accept",
        ),
        "copy",
    );
    // A leak would FAIL the suite (it asserts fix[0] == 1 and f() == 1).
    let s = check(
        suite,
        &[],
        "pub fn solve(a: [i64]) { a[0] = 99 }\n",
        "accept",
    );
    passed(&s, "write to the parameter (ATTACK: a leak fails this)");
}

/// C9 round 9, PSV-1 (amendment 96): `sandbox_run(sb, "fn", arg)` declares
/// `-> i64`, but only the candidate's OWN declared return was applied to what it
/// returned (`-> u8` is satisfied by `4 as u8`), and `let r = sandbox_run(..)`
/// read as determined: `r.ok()` dispatched the lenient `u8` impl and
/// `(r << 1) == 254` wrapped at `u8`. The result is cast at the seal crossing.
#[test]
fn sandbox_run_hands_the_operator_an_i64_or_nothing() {
    let call = "    let p = principal_root(\"r\", true, true, true, 100)\n    let sb = sandbox_create(p, \"IO\")\n    let r = sandbox_run(sb, \"work\", 0)\n";
    let disp = format!(
        "mod sol\nuse sol.{{work}}\n{J8}@[test]\nfn accept() {{\n{call}    assert(r.ok())\n}}\n"
    );
    let width = format!("mod sol\nuse sol.{{work}}\n@[test]\nfn accept() {{\n{call}    assert((r << 1) == 254)\n}}\n");
    // Controls: the honest candidate passes, a wrong one fails with a keyed verdict.
    passed(
        &check(&disp, &[], "pub fn work(x: i64) -> i64 { 9 }\n", "accept"),
        "honest i64",
    );
    let s = check(&disp, &[], "pub fn work(x: i64) -> i64 { 4 }\n", "accept");
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Failed, Some(false)),
        "control wrong: {}",
        s.stdout
    );
    passed(
        &check(
            &width,
            &[],
            "pub fn work(x: i64) -> i64 { 127 }\n",
            "accept",
        ),
        "honest width",
    );
    for (what, suite, cand) in [
        (
            "dispatched the lenient u8 impl",
            &disp,
            "pub fn work(x: i64) -> u8 { 4 as u8 }\n",
        ),
        (
            "wrapped the operator's shift at u8",
            &width,
            "pub fn work(x: i64) -> u8 { 255 as u8 }\n",
        ),
    ] {
        let s = check(suite, &[], cand, "accept");
        assert!(
            refused_unkeyed(&s),
            "ATTACK: sandbox_run handed the operator a u8 the candidate chose ({what}): {:?} {:?} {}",
            s.status,
            s.host,
            s.stdout
        );
    }
}

/// C9 round 9 sweep (amendment 96): `-x` / `~x` wrap at the candidate's chosen
/// width exactly as `x + y` does and were not covered by the arithmetic arm.
#[test]
fn operator_negation_never_runs_at_a_width_the_candidate_chose() {
    let neg = "mod sol\nuse sol.{work}\n@[test]\nfn accept() {\n    let xs = arr_map([1], work)\n    assert((-xs[0]) == -4)\n}\n";
    passed(
        &check(neg, &[], "pub fn work(x: i64) -> i64 { 4 }\n", "accept"),
        "honest i64",
    );
    let wrap = "mod sol\nuse sol.{work}\n@[test]\nfn accept() {\n    let xs = arr_map([1], work)\n    assert((-xs[0]) == 252)\n}\n";
    let s = check(wrap, &[], "pub fn work(x: i64) -> i64 { 4 }\n", "accept");
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Failed, Some(false)),
        "control wrong: {}",
        s.stdout
    );
    let s = check(
        wrap,
        &[],
        "pub fn work(x: i64) -> u8 { 4 as u8 }\n",
        "accept",
    );
    assert!(
        refused_unkeyed(&s),
        "ATTACK: the operator's negation wrapped at a u8 the candidate chose: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
}

/// C9 round 9, SENTINEL (amendment 96): a candidate that calls its OWN fn value
/// with an argument runs; one that reads an operator global through an index,
/// a field or a tuple access is refused (statically here, E0004; the runtime
/// edge is the interpreter unit test's, with the static check bypassed).
#[test]
fn a_candidate_calls_its_own_fn_value_and_never_reads_an_operator_global() {
    let suite = "mod sol\nuse sol.{solve}\ntype Cfg = { k: i64 }\nlet TABLE = [9, 8]\nlet CFG = Cfg { k: 9 }\nlet PAIR = (9, 4)\n@[test]\nfn accept() {\n    assert_eq(solve(3), 4)\n}\n";
    passed(
        &check(suite, &[], "pub fn inc(n: i64) -> i64 { n + 1 }\npub fn solve(n: i64) -> i64 {\n    let g = inc\n    g(n)\n}\n", "accept"),
        "own fn value with an argument",
    );
    for (what, body) in [
        ("index", "TABLE[0] - 5"),
        ("field", "CFG.k - 5"),
        ("tuple", "PAIR.0 - 5"),
    ] {
        let s = check(
            suite,
            &[],
            &format!("pub fn solve(n: i64) -> i64 {{ {body} }}\n"),
            "accept",
        );
        assert!(
            refused_unkeyed(&s),
            "ATTACK: the candidate read an operator global through {what}: {:?} {:?} {}",
            s.status,
            s.host,
            s.stdout
        );
    }
}

// ---- C9 round 10, PSV1H (amendment 100) ----

const OVERFLOW10: &str =
    "arr_sum_i64([9223372036854775807, 1]) + 0 + (9223372036854775807 + v_one())";
const HELPER_COLLIDING10: &str = "fn helper() -> i64 {\n    let v: i64 = 9\n    println(\"p\")\n    if v.ok() { 1 } else { 0 }\n}\n";

fn arm_attack(what: &str, suite: &str, cand: &str) {
    let s = check(suite, &[], cand, "accept");
    assert!(
        refused_unkeyed(&s),
        "ATTACK: a handler arm borrowed the performer's pin verdict ({what}): {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
}

/// BLOCKER A: a handler arm (and a continuation replay) ran under the pin
/// owner of the fn that PERFORMED the effect, not the fn that INSTALLED the
/// handler. A performer holding an identical, determined site text lent its
/// verdict to the arm's undetermined one.
#[test]
fn a_handler_arm_dispatch_is_judged_by_the_fn_that_installed_it() {
    let pre = format!("mod sol\nuse sol.{{work}}\n{J8}fn v_one() -> i64 {{ 1 }}\n");
    let dispatch = |helper: &str, bind: &str| {
        format!("{pre}{helper}@[test]\nfn accept() {{\n  {bind}\n  let r = with handler {{ on IO(p) => resume(if v.ok() {{ 0 }} else {{ {OVERFLOW10} }}) }} {{ helper() }}\n  assert(r == 1)\n}}\n")
    };
    // Honest: an i64 9 passes once the test's own binding is pinned; the wrong i64 fails keyed.
    let pinned = dispatch(HELPER_COLLIDING10, "let v: i64 = work(0)");
    passed(
        &check(&pinned, &[], "pub fn work(x: i64) -> i64 { 9 }\n", "accept"),
        "honest pinned i64 9",
    );
    let ctl = check(&pinned, &[], "pub fn work(x: i64) -> i64 { 4 }\n", "accept");
    assert_eq!(
        (ctl.status, ctl.host),
        (GuestStatus::Failed, Some(false)),
        "control wrong: {}",
        ctl.stdout
    );
    // Control: with no colliding site the same attack is refused.
    let plain = dispatch(
        "fn helper() -> i64 {\n    println(\"p\")\n    1\n}\n",
        "let v = work(0)",
    );
    let s = check(
        &plain,
        &[],
        "pub fn work(x: i64) -> u8 { 4 as u8 }\n",
        "accept",
    );
    assert!(
        refused_unkeyed(&s),
        "control no-collision: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    arm_attack(
        "dispatch arm",
        &dispatch(HELPER_COLLIDING10, "let v = work(0)"),
        "pub fn work(x: i64) -> u8 { 4 as u8 }\n",
    );
}

#[test]
fn a_handler_arm_replay_is_judged_by_the_fn_that_installed_it() {
    let pre = format!("mod sol\nuse sol.{{work}}\n{J8}fn v_one() -> i64 {{ 1 }}\n");
    let general = format!("{pre}{HELPER_COLLIDING10}@[test]\nfn accept() {{\n  let v = work(0)\n  let r = with handler {{ on IO(p) => {{\n      let k = resume(0)\n      if v.ok() {{ k }} else {{ {OVERFLOW10} }}\n  }} }} {{ helper() }}\n  assert(r == 1)\n}}\n");
    arm_attack(
        "general (replay) arm",
        &general,
        "pub fn work(x: i64) -> u8 { 4 as u8 }\n",
    );
}

#[test]
fn a_handler_arm_arithmetic_is_judged_by_the_fn_that_installed_it() {
    let pre = format!("mod sol\nuse sol.{{work}}\n{J8}fn v_one() -> i64 {{ 1 }}\n");
    let width = format!("{pre}fn helper() -> i64 {{\n    let v: i64 = 255\n    println(\"p\")\n    let w = v << 1\n    w\n}}\n@[test]\nfn accept() {{\n  let v = work(0)\n  let r = with handler {{ on IO(p) => resume(if (v << 1) == 254 {{ 0 }} else {{ {OVERFLOW10} }}) }} {{ helper() }}\n  assert(r == 510)\n}}\n");
    // Control: an i64 255 does not wrap, takes the overflow branch, and FAILS.
    let ctl = check(
        &width,
        &[],
        "pub fn work(x: i64) -> i64 { 255 }\n",
        "accept",
    );
    assert!(
        refused_unkeyed(&ctl),
        "control i64 255: {:?} {:?} {}",
        ctl.status,
        ctl.host,
        ctl.stdout
    );
    arm_attack(
        "arithmetic arm",
        &width,
        "pub fn work(x: i64) -> u8 { 255 as u8 }\n",
    );
}

const SB10: &str = "    let p = principal_root(\"r\", true, true, true, 100)\n    let sb = sandbox_create(p, \"IO\")\n";

fn name_attack(what: &str, suite: &str, cand: &str) {
    let s = check(suite, &[], cand, "accept");
    assert!(
        refused_unkeyed(&s),
        "ATTACK: the candidate's string chose the operator fn {what} ran: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
}

/// BLOCKER B: a `str` the candidate returned selected which OPERATOR fn a
/// name-resolving builtin ran.
#[test]
fn sandbox_run_runs_no_function_name_the_candidate_chose() {
    let sandbox = format!("mod sol\nuse sol.{{entry}}\nfn reference(x: i64) -> i64 {{ x * 2 }}\n@[test]\nfn accept() {{\n{SB10}    let got = sandbox_run(sb, entry(), 21)\n    assert(got == reference(21))\n}}\n");
    // Honest: the suite names the candidate's fn with a literal, or builds the
    // name from literals itself.
    let lit = format!("mod sol\nuse sol.{{double}}\nfn reference(x: i64) -> i64 {{ x * 2 }}\n@[test]\nfn accept() {{\n{SB10}    let got = sandbox_run(sb, \"double\", 21)\n    assert(got == reference(21))\n}}\n");
    passed(
        &check(
            &lit,
            &[],
            "pub fn double(x: i64) -> i64 { x * 2 }\n",
            "accept",
        ),
        "literal name",
    );
    let built = format!("mod sol\nuse sol.{{double}}\nfn reference(x: i64) -> i64 {{ x * 2 }}\nfn which() -> str {{ \"dou\" + \"ble\" }}\n@[test]\nfn accept() {{\n{SB10}    let n = which()\n    let got = sandbox_run(sb, n, 21)\n    assert(got == reference(21))\n}}\n");
    passed(
        &check(
            &built,
            &[],
            "pub fn double(x: i64) -> i64 { x * 2 }\n",
            "accept",
        ),
        "operator-built name",
    );
    let wrong = check(&lit, &[], "pub fn double(x: i64) -> i64 { 0 }\n", "accept");
    assert_eq!(
        (wrong.status, wrong.host),
        (GuestStatus::Failed, Some(false)),
        "control wrong: {}",
        wrong.stdout
    );
    // The control the reviewer measured: a candidate that names its own fn is the old, wrong answer.
    let ctl = check(
        &sandbox,
        &[],
        "pub fn entry() -> str { \"double\" }\npub fn double(x: i64) -> i64 { 0 }\n",
        "accept",
    );
    assert!(
        refused_unkeyed(&ctl),
        "control: entry() -> double: {:?} {:?} {}",
        ctl.status,
        ctl.host,
        ctl.stdout
    );
    name_attack(
        "sandbox_run",
        &sandbox,
        "pub fn entry() -> str { \"reference\" }\npub fn double(x: i64) -> i64 { 0 }\n",
    );
}

#[test]
fn scheduler_spawn_runs_no_function_name_the_candidate_chose() {
    let sched = "mod sol\nuse sol.{name}\nfn grade(x: i64) -> i64 { 9 }\n@[test]\nfn accept() {\n    let id = scheduler_spawn(name(), 0)\n    let n = scheduler_run()\n    assert(scheduler_result(id) == 9)\n}\n";
    name_attack(
        "scheduler_spawn",
        sched,
        "pub fn name() -> str { \"grade\" }\n",
    );
}

#[test]
fn goal_eval_runs_no_function_name_the_candidate_chose() {
    let geval = "mod sol\nuse sol.{name}\nfn easy(x: i64) -> i64 { 100 }\nfn hard(x: i64) -> i64 { 0 }\n@[test]\nfn accept() {\n    let s = goal_eval(name(), 5)\n    assert(s > 50.0)\n}\n";
    let ctl = check(geval, &[], "pub fn name() -> str { \"hard\" }\n", "accept");
    assert!(
        refused_unkeyed(&ctl),
        "control: name() -> hard: {:?} {:?} {}",
        ctl.status,
        ctl.host,
        ctl.stdout
    );
    name_attack("goal_eval", geval, "pub fn name() -> str { \"easy\" }\n");
}

// ---- C9 round 11, PSV1T (amendment 102): the runtime taint ------------------------
//
// Through the real runner and the real interpreter. A VALUE sealed code produced
// carries a taint every operation propagates; the primitives that select operator
// code (a name given to a name-resolving builtin, an operator closure called after
// a candidate-chosen key or index, an impl dispatched on a candidate-chosen type,
// fixed-width arithmetic on a candidate-chosen width) refuse a tainted selector.
// Each attack is paired with an honest control that runs to a keyed pass, and a
// wrong answer that fails keyed.

/// The attack was refused BY THE TAINT RULE (its own message is on stdout), not by
/// some other check, and earned no keyed pass.
fn taint_attack(what: &str, suite: &str, cand: &str) -> Option<String> {
    selection_attack(what, suite, cand, true)
}

/// `by_taint`: the refusal must be the taint rule's own message. Where the
/// static pin analysis also refuses the shape (a name in a name-resolving
/// builtin), the runner leg is corroboration and either layer's message counts;
/// the interpreter unit tests judge the taint rule alone (static layer off).
/// Returns the failure, so a test can list EVERY attack that got through.
fn selection_attack(what: &str, suite: &str, cand: &str, by_taint: bool) -> Option<String> {
    let s = check(suite, &[], cand, "accept");
    if !refused_unkeyed(&s) {
        return Some(format!(
            "ATTACK: sealed code selected operator code ({what}): {:?} {:?} {}",
            s.status, s.host, s.stdout
        ));
    }
    let taint = s.stdout.contains("(runtime taint)") || s.stdout.contains("the candidate picked");
    let stat = s.stdout.contains("nothing on the operator side determined");
    if taint || (!by_taint && stat) {
        None
    } else {
        Some(format!(
            "{what}: refused, but not by the {}: {}",
            if by_taint {
                "taint rule"
            } else {
                "selection rules"
            },
            s.stdout
        ))
    }
}

fn all_refused(fails: Vec<String>) {
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

const REF11: &str = "fn reference(x: i64) -> i64 { x * 2 }\n";
const HTAB11: &str = "  let h = dict_new()\n  dict_set(h, \"double\", |x| 0)\n  dict_set(h, \"reference\", |x| x * 2)\n";
const OPS11: &str = "  let ops = [|x| 0, |x| x * 2]\n";

/// The open finding of amendment 100: an operator-built TABLE of closures
/// selected by a key or index the candidate chose. Every spelling of the choice is
/// refused; the operator naming its own row is not.
#[test]
fn an_operator_closure_the_candidate_picked_is_never_called() {
    let suite = |uses: &str, body: &str| {
        format!("mod sol\nuse sol.{{{uses}}}\n{REF11}@[test]\nfn accept() {{\n{body}\n}}\n")
    };
    // Honest controls: the operator's own row passes a right candidate and
    // fails a wrong one, keyed; a branch ON the candidate's data that runs
    // closures the operator listed is free.
    let lit = suite(
        "solve",
        &format!("{OPS11}  let f = ops[1]\n  assert(f(solve(21)) == reference(21))"),
    );
    passed(
        &check(&lit, &[], "pub fn solve(x: i64) -> i64 { x }\n", "accept"),
        "literal row",
    );
    let wrong = check(&lit, &[], "pub fn solve(x: i64) -> i64 { 0 }\n", "accept");
    assert_eq!(
        (wrong.status, wrong.host),
        (GuestStatus::Failed, Some(false)),
        "wrong: {}",
        wrong.stdout
    );
    let branch = suite("idx", &format!("{OPS11}  if idx() == 1 {{ assert(ops[1](21) == reference(21)) }} else {{ assert(false) }}"));
    passed(
        &check(&branch, &[], "pub fn idx() -> i64 { 1 }\n", "accept"),
        "statement branch",
    );
    // Attacks.
    let mut fails = Vec::new();
    for (what, uses, body, cand) in [
        ("dict_get(h, entry())", "entry", format!("{HTAB11}  match dict_get(h, entry()) {{ Some(f) => assert(f(21) == reference(21))  None => assert(false) }}"), "pub fn entry() -> str { \"reference\" }\n"),
        ("the wrong key, refused as well", "entry", format!("{HTAB11}  match dict_get(h, entry()) {{ Some(f) => assert(f(21) == reference(21))  None => assert(false) }}"), "pub fn entry() -> str { \"double\" }\n"),
        ("ops[idx()]", "idx", format!("{OPS11}  let f = ops[idx()]\n  assert(f(21) == reference(21))"), "pub fn idx() -> i64 { 1 }\n"),
        ("the index spelled as a comparison", "idx", format!("{OPS11}  let f = ops[if idx() == 1 {{ 1 }} else {{ 0 }}]\n  assert(f(21) == reference(21))"), "pub fn idx() -> i64 { 1 }\n"),
        ("the index assigned under the candidate's branch", "idx", format!("{OPS11}  let k = 0\n  if idx() == 1 {{ k = 1 }}\n  let f = ops[k]\n  assert(f(21) == reference(21))"), "pub fn idx() -> i64 { 1 }\n"),
        ("the closure chosen by an if expression", "idx", format!("{OPS11}  let f = if idx() == 1 {{ ops[1] }} else {{ ops[0] }}\n  assert(f(21) == reference(21))"), "pub fn idx() -> i64 { 1 }\n"),
        ("the closure assigned under the candidate's branch", "idx", format!("{OPS11}  let f = ops[0]\n  if idx() == 1 {{ f = ops[1] }}\n  assert(f(21) == reference(21))"), "pub fn idx() -> i64 { 1 }\n"),
        ("a closure the candidate was handed and handed back", "choose", format!("{OPS11}  let g = choose(ops[0], ops[1])\n  assert(g(21) == reference(21))"), "pub fn choose(a: fn(i64) -> i64, b: fn(i64) -> i64) -> fn(i64) -> i64 { b }\n"),
        ("a closure the candidate stored in the operator's dict", "reg", format!("{OPS11}  let d = dict_new()\n  reg(d, ops[1])\n  match dict_get(d, \"f\") {{ Some(f) => assert(f(21) == reference(21))  None => assert(false) }}"), "pub fn reg(d: Dict, f: fn(i64) -> i64) { dict_set(d, \"f\", f) }\n"),
    ] {
        fails.extend(taint_attack(what, &suite(uses, &body), cand));
    }
    all_refused(fails);
}

/// A name built from the candidate's bit is the candidate's name, however the bit
/// is spelled into it: a branch value, an assignment under the branch, a return
/// out of it, a literal computed after it, a match, a table indexed by it.
#[test]
fn a_name_built_out_of_the_candidates_bit_never_selects_an_operator_fn() {
    let dbl = "pub fn idx() -> i64 { 1 }\npub fn double(x: i64) -> i64 { 0 }\n";
    let suite = |pre: &str, body: &str| {
        format!("mod sol\nuse sol.{{idx}}\n{REF11}{pre}@[test]\nfn accept() {{\n{SB10}{body}\n}}\n")
    };
    let run = |n: &str| {
        format!("    let got = sandbox_run(sb, {n}, 21)\n    assert(got == reference(21))")
    };
    // Honest: a branch whose arms are the operator's own statements.
    let stmt = suite("", "    if idx() == 1 {\n        let got = sandbox_run(sb, \"double\", 21)\n        assert(got == 0)\n    }");
    passed(&check(&stmt, &[], dbl, "accept"), "statement-level branch");
    let mut fails = Vec::new();
    for (what, pre, body) in [
        ("an if expression", "", run("if idx() == 1 { \"reference\" } else { \"double\" }")),
        ("an assignment under the branch", "", "    let nm = \"double\"\n    if idx() == 1 { nm = \"reference\" }\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))".to_string()),
        ("a return out of the branch", "fn pick() -> str {\n    if idx() == 1 { return \"reference\" }\n    \"double\"\n}\n", run("pick()")),
        ("a match on its value", "", run("match idx() { 1 => \"reference\"  _ => \"double\" }")),
        ("a table indexed by it", "", run("[\"double\", \"reference\"][idx()]")),
        ("a literal computed after an early exit on it", "fn pick() -> str {\n    if idx() == 0 { return \"x\" }\n    \"reference\"\n}\n", run("pick()")),
    ] {
        fails.extend(selection_attack(what, &suite(pre, &body), dbl, false));
    }
    all_refused(fails);
}

/// The taint travels by every carrier a value can: a dict or channel the
/// candidate wrote, a closure's captured state, a sealed `&mut`, the scheduler,
/// a goal. A READ by the candidate of an operator dict taints nothing.
#[test]
fn taint_survives_every_carrier_a_value_can_travel_by() {
    let dbl = "pub fn double(x: i64) -> i64 { 0 }\n";
    let suite = |uses: &str, pre: &str, body: &str| {
        format!(
            "mod sol\nuse sol.{{{uses}}}\n{REF11}{pre}@[test]\nfn accept() {{\n{SB10}{body}\n}}\n"
        )
    };
    let named = |n: &str| {
        format!("    let got = sandbox_run(sb, {n}, 21)\n    assert(got == reference(21))")
    };
    // Honest: the candidate only READS the operator's dict, then the operator
    // names a literal.
    let peek = suite("peek", "", "    let d = dict_new()\n    dict_set(d, \"n\", 1)\n    let k = peek(d)\n    let got = sandbox_run(sb, \"double\", 21)\n    assert(got == 0)");
    passed(
        &check(
            &peek,
            &[],
            &format!("pub fn peek(d: Dict) -> i64 {{ dict_len(d) }}\n{dbl}"),
            "accept",
        ),
        "a sealed read",
    );
    let mut fails = Vec::new();
    for (what, uses, body, cand) in [
        ("a dict the candidate wrote", "put", "    let d = dict_new()\n    put(d)\n    let nm = match dict_get(d, \"n\") { Some(s) => s  None => \"\" }\n".to_string() + &named("nm"), "pub fn put(d: Dict) { dict_set(d, \"n\", \"reference\") }\n"),
        ("a channel the candidate wrote", "push", "    let c = chan<str>()\n    push(c)\n".to_string() + &named("c.recv()"), "pub fn push(c: Chan<str>) { c.send(\"reference\") }\n"),
        ("a &mut array the candidate wrote", "fill", "    let xs = [\"double\"]\n    fill(&mut xs)\n".to_string() + &named("xs[0]"), "pub fn fill(xs: &mut [str]) { xs[0] = \"reference\" }\n"),
        ("a closure's captured state the candidate set", "run_cb", "    let n = 0\n    let cb = |v| { if v > 0 { n = v }\n        n }\n    let r = run_cb(cb)\n    let names = [\"double\", \"reference\"]\n".to_string() + &named("names[cb(0)]"), "pub fn run_cb(f: fn(i64) -> i64) -> i64 { f(1) }\n"),
        ("the scheduler", "idx1", "    let id = scheduler_spawn(\"idx1\", 0)\n    let n = scheduler_run()\n    let names = [\"double\", \"reference\"]\n".to_string() + &named("names[scheduler_result(id)]"), "pub fn idx1(x: i64) -> i64 { 1 }\n"),
        ("sandbox_run's result", "idx1", "    let names = [\"double\", \"reference\"]\n".to_string() + &named("names[sandbox_run(sb, \"idx1\", 0)]"), "pub fn idx1(x: i64) -> i64 { 1 }\n"),
    ] {
        let cand = format!("{cand}{dbl}");
        fails.extend(selection_attack(what, &suite(uses, "", &body), &cand, false));
    }
    all_refused(fails);
}

/// Amendment 106 (round 11): a channel is ONE shared object with one taint. The
/// candidate's send, or its drain, decides how many values the operator finds
/// in it, and a count or an emptiness the candidate decided must not pick the
/// operator's closure or the NAME of the fn it runs. The honest operator's own
/// channel, used the same ways, passes.
#[test]
fn a_channel_the_candidate_touched_never_selects_operator_code() {
    let suite = |uses: &str, body: &str| {
        format!("mod sol\nuse sol.{{{uses}}}\n{REF11}@[test]\nfn accept() {{\n{body}\n}}\n")
    };
    let own = suite(
        "nop",
        &format!("{OPS11}  let c = chan<i64>()\n  c.send(7)\n  let f = ops[c.len()]\n  assert(f(21) == reference(21))"),
    );
    passed(
        &check(&own, &[], "pub fn nop() -> i64 { 0 }\n", "accept"),
        "the operator's own channel",
    );
    let relay = suite(
        "relay",
        "  let c = chan<i64>()\n  c.send(21)\n  assert(relay(c) == reference(21))",
    );
    passed(
        &check(
            &relay,
            &[],
            "pub fn relay(c: Chan<i64>) -> i64 { c.recv() * 2 }\n",
            "accept",
        ),
        "the candidate relays data and the operator compares it",
    );
    let push = "pub fn push(c: Chan<i64>) { c.send(7) }\n";
    let drain = "pub fn drain(c: Chan<i64>) { let x = c.recv() }\n";
    let drain_try = "pub fn drain(c: Chan<i64>) { let x = c.try_recv() }\n";
    let named = |pre: &str, n: &str| {
        format!("{SB10}{pre}  let nm = {n}\n  let got = sandbox_run(sb, nm, 21)\n  assert(got == reference(21))")
    };
    let mut fails = Vec::new();
    for (what, uses, body, cand) in [
        ("len after a sealed send picks a closure", "push", format!("{OPS11}  let c = chan<i64>()\n  push(c)\n  let f = ops[c.len()]\n  assert(f(21) == reference(21))"), push.to_string()),
        ("len after a sealed send picks the name", "push", named("  let c = chan<i64>()\n  push(c)\n", "if c.len() == 1 { \"reference\" } else { \"zz\" }"), format!("{push}pub fn double(x: i64) -> i64 {{ 0 }}\n")),
        ("len after a sealed drain picks a closure", "drain", format!("{OPS11}  let c = chan<i64>()\n  c.send(7)\n  drain(c)\n  let f = if c.len() == 0 {{ ops[1] }} else {{ ops[0] }}\n  assert(f(21) == reference(21))"), drain.to_string()),
        ("try_recv after a sealed recv picks a closure", "drain", format!("{OPS11}  let c = chan<i64>()\n  c.send(7)\n  drain(c)\n  let f = match c.try_recv() {{ Some(v) => ops[0]  None => ops[1] }}\n  assert(f(21) == reference(21))"), drain.to_string()),
        ("len after a sealed try_recv picks a closure", "drain", format!("{OPS11}  let c = chan<i64>()\n  c.send(7)\n  drain(c)\n  let f = if c.len() == 0 {{ ops[1] }} else {{ ops[0] }}\n  assert(f(21) == reference(21))"), drain_try.to_string()),
        ("the arm select skips because the candidate drained its channel", "drain", format!("{OPS11}  let a = chan<i64>()\n  let b = chan<i64>()\n  a.send(7)\n  b.send(7)\n  drain(a)\n  let f = select {{ a.recv() => ops[0]  b.recv() => ops[1] }}\n  assert(f(21) == reference(21))"), drain.to_string()),
        ("sends in a loop the candidate sized", "idx", format!("{OPS11}  let c = chan<i64>()\n  for i in 0..idx() {{ c.send(i) }}\n  let f = ops[c.len()]\n  assert(f(21) == reference(21))"), "pub fn idx() -> i64 { 1 }\n".to_string()),
    ] {
        fails.extend(taint_attack(what, &suite(uses, &body), &cand));
    }
    all_refused(fails);
}

/// The NAME rule of the taint, on the attacks the static name analysis lets
/// through (a branch value, a match, a return out of a branch): the refusal is
/// the runtime taint's own. The dispatch and width rules of the taint have no
/// runner leg of their own: every attack they refuse the static layer refuses
/// FIRST, so a runner row would score REFUSED_ELSEWHERE by construction (their
/// attacks and rows are the interpreter unit tests', with the static layer off).
#[test]
fn the_taint_name_rule_refuses_what_the_static_name_analysis_lets_through() {
    let dbl = "pub fn idx() -> i64 { 1 }\npub fn double(x: i64) -> i64 { 0 }\n";
    let suite = |pre: &str, body: &str| {
        format!("mod sol\nuse sol.{{idx}}\n{REF11}{pre}@[test]\nfn accept() {{\n{SB10}{body}\n}}\n")
    };
    let run = |n: &str| {
        format!("    let got = sandbox_run(sb, {n}, 21)\n    assert(got == reference(21))")
    };
    let mut fails = Vec::new();
    for (what, pre, body) in [
        ("an if expression", "", run("if idx() == 1 { \"reference\" } else { \"double\" }")),
        ("a match on its value", "", run("match idx() { 1 => \"reference\"  _ => \"double\" }")),
        ("a return out of the branch", "fn pick() -> str {\n    if idx() == 1 { return \"reference\" }\n    \"double\"\n}\n", run("pick()")),
        ("assigned in both arms", "", "    let nm = \"x\"\n    if idx() == 1 { nm = \"reference\" } else { nm = \"double\" }\n    let got = sandbox_run(sb, nm, 21)\n    assert(got == reference(21))".to_string()),
    ] {
        fails.extend(taint_attack(what, &suite(pre, &body), dbl));
    }
    all_refused(fails);
}

/// What the candidate can READ of one run: every diagnostic printed at check time
/// (test-STDERR) and every line printed at run time (test-STDOUT), with the
/// temp directory and the durations taken out. The first version of this twin read
/// stdout only, and a check-time refusal goes to stderr, so it compared two empty
/// strings and passed (round-13 SENTINEL).
fn candidate_visible(s: &Seen) -> String {
    let strip = |l: &str, key: &str| -> String {
        // Remove `"<key>":"..."` (a value without quotes inside).
        match l.find(&format!("\"{key}\":\"")) {
            Some(at) => {
                let rest = &l[at + key.len() + 4..];
                let end = rest.find('"').map_or(rest.len(), |e| e + 1);
                format!("{}{}", &l[..at], &rest[end..])
            }
            None => l.to_string(),
        }
    };
    let mut out = String::new();
    for l in s.stderr.lines().chain(s.stdout.lines()) {
        let l = l.trim();
        if l.is_empty()
            || l.starts_with("running ")
            || l.starts_with("test result")
            || l.starts_with("STDERR")
        {
            continue;
        }
        let l = strip(&strip(l, "file"), "completion").replace(&s.root, "<D>");
        // `module `sol` at <tmp>/candidate/sol.ax: parse error`
        let mut l = l;
        while let Some(at) = l.find("\"duration_ms\":") {
            let n = l[at + 14..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .count();
            l = format!("{}{}", &l[..at], &l[at + 14 + n..]);
        }
        let l = match (l.find(" at /"), l.find("/candidate/sol.ax")) {
            (Some(a), Some(b)) if a < b => format!("{} at <D>{}", &l[..a], &l[b..]),
            _ => l,
        };
        // `test accept ... ok (1.0ms)`
        let l = match l.rfind(" (") {
            Some(at) if l.ends_with("ms)") => l[..at].to_string(),
            _ => l,
        };
        out.push_str(&l);
        out.push('\n');
    }
    out
}

/// The positions of the existence-oracle twin: (position, body naming the
/// operator's, body naming the same shape of a missing name, item before `solve`
/// naming the operator's, ... the missing's). A position the sealed walk in
/// `Resolver::check_sealed` visits has a row here (`every_position_the_sealed_walk_visits_has_a_twin_row`).
fn existence_forms() -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
)> {
    vec![
        ("fn call", "ofn(1)", "zfn(1)", "", ""),
        ("fn value", "let g = ofn\n 41", "let g = zfn\n 41", "", ""),
        ("global", "OG", "ZG", "", ""),
        ("global index", "TABLE[0]", "ZTABLE[0]", "", ""),
        ("global field", "TABLE.k", "ZTABLE.k", "", ""),
        ("assign", "OG = 6\n 41", "ZG = 6\n 41", "", ""),
        ("struct literal", "let t = OT { k: 1 }\n 41", "let t = ZT { k: 1 }\n 41", "", ""),
        ("field of a struct literal", "(OT { k: 1 }).k", "(ZT { k: 1 }).k", "", ""),
        ("let annotation", "let t: OT = mk()\n 41", "let t: ZT = mk()\n 41", "", ""),
        ("own annotation", "own t: OT = mk()\n 41", "own t: ZT = mk()\n 41", "", ""),
        ("ref annotation", "ref t: OT = mk()\n 41", "ref t: ZT = mk()\n 41", "", ""),
        ("cast", "mk() as OT", "mk() as ZT", "", ""),
        ("option annotation", "let a: Option<OT> = None\n 41", "let a: Option<ZT> = None\n 41", "", ""),
        ("array annotation", "let a: [OT] = []\n 41", "let a: [ZT] = []\n 41", "", ""),
        ("fn type annotation", "let a: fn(OT) -> i64 = |x| 1\n 41", "let a: fn(ZT) -> i64 = |x| 1\n 41", "", ""),
        ("lambda parameter", "let f = |x: OT| 1\n 41", "let f = |x: ZT| 1\n 41", "", ""),
        ("param type", "41", "41", "fn pt(x: OT) -> i64 { 1 }\n", "fn pt(x: ZT) -> i64 { 1 }\n"),
        ("return type", "41", "41", "fn pt() -> OT { mk() }\n", "fn pt() -> ZT { mk() }\n"),
        ("struct field type", "41", "41", "type CT = { a: OT }\n", "type CT = { a: ZT }\n"),
        ("enum field type", "41", "41", "type CE = A { a: OT } | B\n", "type CE = A { a: ZT } | B\n"),
        ("trait method type", "41", "41", "trait CTr { fn m(self, x: OT) -> i64 }\n", "trait CTr { fn m(self, x: ZT) -> i64 }\n"),
        ("refinement base", "41", "41", "type CR = OT where _.k > 0\n", "type CR = ZT where _.k > 0\n"),
        ("refinement predicate", "41", "41", "type CP = i64 where _ > ofn(0)\n", "type CP = i64 where _ > zfn(0)\n"),
        ("verify predicate", "41", "41", "@[verify(ofn(1) > 0)]\nfn cv() -> i64 { 41 }\n", "@[verify(zfn(1) > 0)]\nfn cv() -> i64 { 41 }\n"),
        ("generic bound", "41", "41", "fn pt<T: OT>(x: T) -> i64 { 1 }\n", "fn pt<T: ZT>(x: T) -> i64 { 1 }\n"),
        ("impl for", "41", "41", "trait My { fn five(self) -> i64 }\nimpl My for OT { fn five(self: OT) -> i64 { 1 } }\n", "trait My { fn five(self) -> i64 }\nimpl My for ZT { fn five(self: ZT) -> i64 { 1 } }\n"),
        ("enum type path", "let t = OE::P\n 41", "let t = ZE::P\n 41", "", ""),
        ("enum variant literal", "let t = OE::P { }\n 41", "let t = ZE::P { }\n 41", "", ""),
        ("match pattern", "match mk() { OE::P => 41  _ => 41 }", "match mk() { ZE::P => 41  _ => 41 }", "", ""),
        ("guard", "match mk() { x if OG > 1 => 41  _ => 41 }", "match mk() { x if ZG > 1 => 41  _ => 41 }", "", ""),
        ("for bound", "for i in 0..(OG) { }\n 41", "for i in 0..(ZG) { }\n 41", "", ""),
        ("interpolation", "\"{OG}\"\n 41", "\"{ZG}\"\n 41", "", ""),
        ("trait path", "Sc::score(3)", "ZC::score(3)", "", ""),
        ("type path call", "OT::new()", "ZT::new()", "", ""),
        ("spawn body", "spawn { ofn(1) }\n 41", "spawn { zfn(1) }\n 41", "", ""),
        ("use of a module", "41", "41", "use opmod.{a}\n", "use zmod.{a}\n"),
        ("goal_run name", "let r = goal_run(\"nonadapt\", 100.0, 5)\n 41", "let r = goal_run(\"zznosuch\", 100.0, 5)\n 41", "", ""),
        ("kernel_goal_create name", "let g = kernel_goal_create(0, \"nonadapt\", 100.0)\n 41", "let g = kernel_goal_create(0, \"zznosuch\", 100.0)\n 41", "", ""),
        // Amendment 117 (PSV-1 loop findings 4, 5, 8, 9, 25, 50): positions the sealed-only check skipped.
        ("goal_run_categorical name", "let r = goal_run_categorical(\"nonadapt\", 3, 100.0, 5)\n 41", "let r = goal_run_categorical(\"zznosuch\", 3, 100.0, 5)\n 41", "", ""),
        ("goal_run_random name", "let r = goal_run_random(\"nonadapt\", 100.0, 5, 0, 10)\n 41", "let r = goal_run_random(\"zznosuch\", 100.0, 5, 0, 10)\n 41", "", ""),
        ("goal_run_multistart name", "let r = goal_run_multistart(\"nonadapt\", 100.0, 2, 3, 0, 10)\n 41", "let r = goal_run_multistart(\"zznosuch\", 100.0, 2, 3, 0, 10)\n 41", "", ""),
        ("goal_eval name", "let r = goal_eval(\"nonadapt\", 3)\n 41", "let r = goal_eval(\"zznosuch\", 3)\n 41", "", ""),
        ("struct refinement predicate", "41", "41", "type CS = { a: i64 } where _.a > ofn(0)\n", "type CS = { a: i64 } where _.a > zfn(0)\n"),
        ("dyn param", "41", "41", "fn dd(x: dyn Sc) -> i64 { 1 }\n", "fn dd(x: dyn ZC) -> i64 { 1 }\n"),
        ("dyn let", "let d: dyn Sc = mk()\n 41", "let d: dyn ZC = mk()\n 41", "", ""),
        ("dyn array", "let d: [dyn Sc] = []\n 41", "let d: [dyn ZC] = []\n 41", "", ""),
        ("dyn option", "let d: Option<dyn Sc> = None\n 41", "let d: Option<dyn ZC> = None\n 41", "", ""),
        ("dyn struct field", "41", "41", "type CD = { a: dyn Sc }\n", "type CD = { a: dyn ZC }\n"),
        ("dyn lambda parameter", "let f = |x: dyn Sc| 1\n 41", "let f = |x: dyn ZC| 1\n 41", "", ""),
        ("array element: struct literal", "let t = [OT { k: 1 }]\n 41", "let t = [ZT { k: 1 }]\n 41", "", ""),
        ("tuple element: struct literal", "let t = (OT { k: 1 }, 2)\n 41", "let t = (ZT { k: 1 }, 2)\n 41", "", ""),
        ("array element: enum path", "let t = [OE::P]\n 41", "let t = [ZE::P]\n 41", "", ""),
        ("array element: type path", "let t = [OT::new()]\n 41", "let t = [ZT::new()]\n 41", "", ""),
        ("array element: lambda", "let t = [|x: i64| OT { k: x }]\n 41", "let t = [|x: i64| ZT { k: x }]\n 41", "", ""),
        ("predicate: struct literal", "41", "41", "type RP = i64 where _ > len([OT { k: 1 }])\n", "type RP = i64 where _ > len([ZT { k: 1 }])\n"),
        ("predicate: enum path", "41", "41", "type RP = i64 where _ > len([OE::P])\n", "type RP = i64 where _ > len([ZE::P])\n"),
        ("verify predicate: struct literal", "41", "41", "@[verify(len([OT { k: 1 }]) > 0)]\nfn cv() -> i64 { 41 }\n", "@[verify(len([ZT { k: 1 }]) > 0)]\nfn cv() -> i64 { 41 }\n"),
        ("param refinement: struct literal", "41", "41", "fn pr(x: i64 where _ > len([OT { k: 1 }])) -> i64 { x }\n", "fn pr(x: i64 where _ > len([ZT { k: 1 }])) -> i64 { x }\n"),
        ("struct where: struct literal", "41", "41", "type CS2 = { a: i64 } where _.a > len([OT { k: 1 }])\n", "type CS2 = { a: i64 } where _.a > len([ZT { k: 1 }])\n"),
        ("deferred-prefix type", "41", "41", "fn pt(x: DictTable) -> i64 { 1 }\n", "fn pt(x: DictTableZ9) -> i64 { 1 }\n"),
        ("deferred-prefix struct literal", "let t = DictTable { k: 1 }\n 41", "let t = DictTableZ9 { k: 1 }\n 41", "", ""),
        ("deferred-prefix generic bound", "41", "41", "fn pt<T: DictTable>(x: T) -> i64 { 1 }\n", "fn pt<T: DictTableZ9>(x: T) -> i64 { 1 }\n"),
        // Round 15 (amendment 121): the predicate positions with a NAME that is not a fn call, a
        // top-level `let`, and the other expression shapes inside a predicate.
        ("refinement predicate: global", "41", "41", "type CP = i64 where _ > OG\n", "type CP = i64 where _ > ZG\n"),
        ("struct predicate: global", "41", "41", "type CS3 = { a: i64 } where _.a > OG\n", "type CS3 = { a: i64 } where _.a > ZG\n"),
        ("predicate: index", "41", "41", "type CP2 = i64 where _ > TABLE[0]\n", "type CP2 = i64 where _ > ZTABLE[0]\n"),
        ("predicate: field", "41", "41", "type CP6 = i64 where _ > TABLE.k\n", "type CP6 = i64 where _ > ZTABLE.k\n"),
        ("struct predicate: index", "41", "41", "type CS4 = { a: i64 } where _.a > TABLE[0]\n", "type CS4 = { a: i64 } where _.a > ZTABLE[0]\n"),
        ("predicate: interpolation", "41", "41", "type CP3 = i64 where len(\"{OG}\") > 0\n", "type CP3 = i64 where len(\"{ZG}\") > 0\n"),
        ("predicate: match guard", "41", "41", "type CP4 = i64 where match _ { x if OG > 1 => true  _ => true }\n", "type CP4 = i64 where match _ { x if ZG > 1 => true  _ => true }\n"),
        ("predicate: closure body", "41", "41", "type CP5 = i64 where len([|y: i64| OG]) > 0\n", "type CP5 = i64 where len([|y: i64| ZG]) > 0\n"),
        ("verify predicate: global", "41", "41", "@[verify(OG > 0)]\nfn cv() -> i64 { 41 }\n", "@[verify(ZG > 0)]\nfn cv() -> i64 { 41 }\n"),
        ("top-level let: global", "41", "41", "let CV = OG\n", "let CV = ZG\n"),
        ("top-level let: fn call", "41", "41", "let CV = ofn(1)\n", "let CV = zfn(1)\n"),
        ("top-level let: struct literal", "41", "41", "let CV = OT { k: 1 }\n", "let CV = ZT { k: 1 }\n"),
        ("top-level let: enum path", "41", "41", "let CV = OE::P\n", "let CV = ZE::P\n"),
        ("top-level let: array element", "41", "41", "let CV = [OT { k: 1 }]\n", "let CV = [ZT { k: 1 }]\n"),
        ("top-level let: lambda body", "41", "41", "let CV = |x: i64| OG\n", "let CV = |x: i64| ZG\n"),
        ("top-level let: match arm", "41", "41", "let CV = match 1 { 1 => OG  _ => 0 }\n", "let CV = match 1 { 1 => ZG  _ => 0 }\n"),
    ]
}

/// Where the item under test sits in the candidate's file (round 15). The
/// existence oracle held when a `fn` came first and failed when it did not: the
/// resolver located a predicate's diagnostics at the PREVIOUS statement, and the
/// parser gave an item that ends the file the dummy span `0..0`, so a diagnostic
/// about it was filed under the operator's entry file. (name, text before the
/// item under test, `pub ` or not for `solve`, text after `solve`). `mk` is a
/// helper the bodies call; it follows when it does not come first.
fn existence_placements() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    const MK: &str = "fn mk() -> i64 { 1 }\n";
    vec![
        ("first in the file (bare fn at byte 0)", "", "", MK),
        ("after a fn", MK, "pub ", ""),
        (
            "after a pub fn",
            "pub fn other() -> i64 { 2 }\n",
            "pub ",
            MK,
        ),
        ("after a type", "type Qx1 = { q: i64 }\n", "pub ", MK),
        ("after a let", "let qx2 = 1\n", "pub ", MK),
        ("after a comment", "// a comment\n", "pub ", MK),
        ("after a blank line", "\n", "pub ", MK),
        ("after a mod", "mod inner\n", "pub ", MK),
    ]
}

/// Amendment 106 / 114: a sealed caller's refusal does not say whether the operator
/// defines a name. Run through the runner with the check-time text included, the
/// refusal for an operator fn, global, type, enum, trait, struct literal, pattern,
/// annotation... and for a name nothing defines is the same text, and the same
/// ACCEPT/REFUSE.
#[test]
fn a_sealed_caller_is_refused_in_the_same_words_for_an_operator_name_and_a_missing_one() {
    let su = "mod sol\nmod opmod\nuse sol.{solve}\ntrait Sc {\n    fn score(self) -> i64\n}\nimpl Sc for i64 {\n    fn score(self: i64) -> i64 { 41 }\n}\ntype OT = { k: i64 }\ntype OE = P | Q\ntype DictTable = { k: i64 }\nlet OG = 5\nlet TABLE = [41, 42]\nfn ofn(x: i64) -> i64 { x }\nfn secret() -> i64 { 41 }\nfn nonadapt(n: i64) -> i64 { n }\n@[test]\nfn accept() {\n  assert_eq(solve(), 41)\n}\n";
    // (position, body naming the operator's, body naming the same shape of a
    // missing name, item before `solve` naming the operator's, ... the missing's)
    let forms = existence_forms();
    let swaps = [
        ("zznosuch", "nonadapt"),
        ("ZTABLE", "TABLE"),
        ("zfn", "ofn"),
        ("ZG", "OG"),
        ("ZT", "OT"),
        ("ZE", "OE"),
        ("ZC", "Sc"),
        ("zmod", "opmod"),
        ("DictTableZ9", "DictTable"),
    ];
    let run_one = |body: &str, extra: &str, before: &str, vis: &str, after: &str| {
        let cand = format!("{before}{extra}{vis}fn solve() -> i64 {{ {body} }}\n{after}");
        let s = check(su, &[], &cand, "accept");
        let mut text = candidate_visible(&s);
        for (miss, op) in swaps {
            text = text.replace(miss, op);
        }
        (refused_unkeyed(&s), s.status, s.host, text)
    };
    // Every position x every placement. The placements run side by side (one thread
    // each); a failure names both.
    let placements = existence_placements();
    let mut fails: Vec<String> = std::thread::scope(|sc| {
        let hs: Vec<_> = placements
            .iter()
            .map(|(pname, before, vis, after)| {
                let (forms, run_one) = (&forms, &run_one);
                sc.spawn(move || {
                    let mut fails = Vec::new();
                    for (pos, eb, mb, ee, me) in forms {
                        let pos = format!("[{pname}] {pos}");
                        let a = run_one(eb, ee, before, vis, after);
                        let b = run_one(mb, me, before, vis, after);
                        if (a.0, a.1, a.2) != (b.0, b.1, b.2) {
                            fails.push(format!(
                                "ATTACK: {pos}: ACCEPT/REFUSE differs: operator name {:?}/{:?} vs missing name {:?}/{:?}",
                                a.1, a.2, b.1, b.2
                            ));
                        } else if a.3 != b.3 {
                            fails.push(format!(
                                "ATTACK: {pos}: the text differs:\n  operator name: {:?}\n  missing name : {:?}",
                                a.3, b.3
                            ));
                        } else if a.3.trim().is_empty() {
                            fails.push(format!(
                                "{pos}: both texts are EMPTY, so nothing was compared"
                            ));
                        } else if !a.0 {
                            fails.push(format!(
                                "{pos}: both were ACCEPTED (a candidate may not name these)"
                            ));
                        } else if a.3.contains("which the operator's code defines") {
                            fails.push(format!(
                                "ATTACK: {pos}: the backstop's text reached the candidate: {:?}",
                                a.3
                            ));
                        }
                    }
                    fails
                })
            })
            .collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    all_refused(std::mem::take(&mut fails));
    // Honest controls: the candidate's OWN names of the same shapes pass, keyed.
    let own = "type CT = { k: i64 }\ntype CE = A | B | C { n: i64 }\ntype CPos = i64 where _ > 0\ntrait Cm {\n    fn m(self) -> i64\n    fn plus(self, x: CT) -> i64\n}\nimpl Cm for CT {\n    fn m(self: CT) -> i64 { self.k }\n    fn plus(self: CT, x: CT) -> i64 { self.k + x.k }\n}\nlet CG = 5\nfn cfn(x: i64) -> i64 { x }\nfn gm<T: Cm>(x: T) -> i64 { x.m() }\nfn pos(x: CPos) -> i64 { x }\nfn lim(n: i64 where n >= 0) -> i64 { n }\npub fn solve() -> i64 {\n  let t = CT { k: 1 }\n  let e = CE::A\n  let f = |x: CT| x.k\n  let m = match e { CE::A => cfn(CG) + f(t) + gm(t) - t.plus(t)  CE::B => 0  CE::C { n } => n }\n  let spare = 1\n  m + 36 + pos(1) - 1 + lim(0)\n}\n";
    let seen = check(su, &[], own, "accept");
    passed(&seen, "own names of every shape");
    // A sealed file's diagnostics come from ONE check (the sealed-only one): the
    // merged check's copy is dropped, so a warning is printed once.
    let once = seen.stderr.matches("unused variable `spare`").count();
    assert_eq!(
        once, 1,
        "the candidate's warning is printed {once} times: {}",
        seen.stderr
    );
    all_refused(fails);
}

/// Which twin rows cover each construct the sealed walk in `Resolver::check_sealed`
/// visits (the `Item::` arms that pick an item's expressions and types, and the
/// `Expr::` arms of the walker). The walk is the place a position is added; a
/// construct it learns that has no row here fails `every_position_the_sealed_walk_visits_has_a_twin_row`.
const WALK_ROWS: &[(&str, &[&str])] = &[
    (
        "Item::FnDef",
        &[
            "fn call",
            "param type",
            "return type",
            "generic bound",
            "verify predicate: global",
        ],
    ),
    ("Item::ImplBlock", &["impl for"]),
    (
        "Item::RefineDef",
        &[
            "refinement base",
            "refinement predicate",
            "refinement predicate: global",
        ],
    ),
    (
        "Item::TypeDef",
        &[
            "struct field type",
            "struct refinement predicate",
            "struct predicate: global",
        ],
    ),
    ("Item::EnumDef", &["enum field type"]),
    ("Item::TraitDef", &["trait method type"]),
    (
        "Item::LetDef",
        &[
            "top-level let: global",
            "top-level let: fn call",
            "top-level let: struct literal",
            "top-level let: enum path",
        ],
    ),
    ("Item::ModDecl", &["use of a module"]),
    ("Item::UseDecl", &["use of a module"]),
    ("Expr::Ident", &["global", "fn value"]),
    ("Expr::Assign", &["assign"]),
    ("Expr::StructLit", &["struct literal"]),
    ("Expr::Let", &["let annotation"]),
    ("Expr::Own", &["own annotation"]),
    ("Expr::RefBind", &["ref annotation"]),
    ("Expr::Lambda", &["lambda parameter"]),
];

/// Round 15 (amendment 121): the existence-oracle twin is a table of positions x
/// placements; a source scan fails if a construct the resolver's sealed walk
/// visits has no row in it, or a row names a position the table does not have.
#[test]
fn every_position_the_sealed_walk_visits_has_a_twin_row() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../axon-core/src/resolver.rs"
    ))
    .unwrap();
    let a = src.find("fn check_sealed(").expect("check_sealed");
    let b = src[a..].find("// ── Pass 2").expect("end of check_sealed") + a;
    let walk = &src[a..b];
    let mut seen: std::collections::BTreeSet<String> = Default::default();
    for pre in ["Item::", "Expr::"] {
        let mut rest = walk;
        while let Some(i) = rest.find(pre) {
            let tail = &rest[i + pre.len()..];
            let n = tail
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(tail.len());
            seen.insert(format!("{pre}{}", &tail[..n]));
            rest = &tail[n..];
        }
    }
    let have: std::collections::BTreeSet<String> =
        WALK_ROWS.iter().map(|(k, _)| k.to_string()).collect();
    let missing: Vec<_> = seen.difference(&have).collect();
    let stale: Vec<_> = have.difference(&seen).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "ATTACK: the sealed walk visits constructs with no twin row {missing:?} (add rows to \
         `existence_forms` and `WALK_ROWS`), or WALK_ROWS names constructs it no longer visits {stale:?}"
    );
    let names: std::collections::BTreeSet<&str> = existence_forms().iter().map(|f| f.0).collect();
    for (k, rows) in WALK_ROWS {
        assert!(!rows.is_empty(), "{k} has no rows");
        for r in *rows {
            assert!(
                names.contains(r),
                "ATTACK: {k} names the row `{r}`, which `existence_forms` lacks"
            );
        }
    }
    // The placements the oracle is held at: the item first (bare, at byte 0), after
    // each kind of item, a comment and a blank line.
    let pl: Vec<&str> = existence_placements().iter().map(|p| p.0).collect();
    for want in ["first", "fn", "type", "let", "comment", "mod"] {
        assert!(
            pl.iter().any(|p| p.contains(want)),
            "ATTACK: no placement `{want}` in the existence twin: {pl:?}"
        );
    }
}

/// Round 15 (amendment 121): the operator-typed receiver is no longer exempt when
/// the candidate PICKED the value. The registry pattern the reviewers executed
/// (`dict_get_or(d, key(), ..)` over a dict of operator structs with a strict and
/// a lenient impl) follows the candidate's key no longer: the verdict is refused
/// for both keys. The honest shapes that build an operator struct from the
/// candidate's data (`Sq { s: val() }.area()`, a factory fn, a registry the operator
/// filled and reads by its own key) pass keyed.
#[test]
fn an_operator_value_the_candidate_picked_is_refused_through_the_runner() {
    let su = |body: &str| {
        format!("mod sol\nuse sol.{{key, val, pickb}}\ntype A = {{ v: i64 }}\ntype B = {{ v: i64 }}\ntype Sq = {{ s: i64 }}\ntype Ci = {{ r: i64 }}\ntrait Rub {{\n    fn rub(self) -> bool\n}}\nimpl Rub for A {{\n    fn rub(self: A) -> bool {{ self.v == 9 }}\n}}\nimpl Rub for B {{\n    fn rub(self: B) -> bool {{ true }}\n}}\ntrait Area {{\n    fn area(self) -> i64\n}}\nimpl Area for Sq {{\n    fn area(self: Sq) -> i64 {{ self.s * self.s }}\n}}\nimpl Area for Ci {{\n    fn area(self: Ci) -> i64 {{ self.r * 3 }}\n}}\nfn mka() -> A {{ A {{ v: 1 }} }}\nfn mkb() -> B {{ B {{ v: 1 }} }}\nfn mk_sq(n: i64) -> Sq {{ Sq {{ s: n }} }}\n@[test]\nfn accept() {{\n{body}\n}}\n")
    };
    let cand = |k: &str| {
        format!("pub fn key() -> str {{ \"{k}\" }}\npub fn val() -> i64 {{ 3 }}\npub fn pickb<T>(a: fn() -> T, b: fn() -> T) -> T {{ b() }}\n")
    };
    let reg = "  let d = dict_new()\n  dict_set(d, \"a\", A { v: 1 })\n  dict_set(d, \"b\", B { v: 1 })\n";
    let mut fails = Vec::new();
    // Attacks: both keys are refused (the verdict does not follow the key).
    for (what, body) in [
        (
            "a dict of operator structs read by the candidate's key",
            format!("{reg}  let r = dict_get_or(d, key(), A {{ v: 1 }})\n  assert(r.rub())"),
        ),
        (
            "a sealed generic handing one of two operator ctors' values back",
            "  let r = pickb(mka, mkb)\n  assert(r.rub())".to_string(),
        ),
    ] {
        for k in ["a", "b"] {
            fails.extend(taint_attack(
                &format!("{what}, key {k}"),
                &su(&body),
                &cand(k),
            ));
        }
    }
    // Honest controls: the operator's own key; an operator struct built from the candidate's number.
    for (what, body) in [
        ("the operator's own key into a table of operator values", format!("{reg}  assert(dict_get_or(d, \"b\", A {{ v: 1 }}).rub())")),
        ("an operator struct holding the candidate's number", "  assert(Sq { s: val() }.area() == 9)".to_string()),
        ("a factory fn given the candidate's number", "  assert(mk_sq(val()).area() == 9)".to_string()),
        ("a registry the operator filled from the candidate's data, read by its own key", "  let reg = dict_new()\n  dict_set(reg, \"sq\", Sq { s: val() })\n  dict_set(reg, \"ci\", Ci { r: val() })\n  assert(dict_get_or(reg, \"sq\", Sq { s: 1 }).area() == 9)".to_string()),
    ] {
        let s = check(&su(&body), &[], &cand("b"), "accept");
        if (s.status, s.host) != (GuestStatus::Passed, Some(true)) {
            fails.push(format!("CONTROL REFUSED ({what}): {}", s.stdout));
        }
    }
    all_refused(fails);
}

/// Amendment 108: a STRUCTURAL COMPARISON reads the content of the dict the
/// candidate filled. `d == e`, `d != e`, `[d] == [e]`, a struct, a tuple, and
/// `arr_contains([d], e)` each chose the operator closure or the operator fn
/// NAME by the candidate's data, taint-free, until the comparison walked both
/// operands deep. The attack is the reviewer's: a WRONG answer (4) passes under
/// the lenient check or fails under the strict one depending on what the
/// candidate wrote.
#[test]
fn comparing_a_dict_the_candidate_filled_never_selects_operator_code() {
    let cand = "pub fn cand() -> i64 { 4 }\npub fn fill(d: Dict) { dict_set(d, \"k\", 1) }\n";
    let suite = |pre: &str, body: &str| {
        format!("mod sol\nuse sol.{{cand, fill}}\nfn strict(x: i64) -> bool {{ x == 9 }}\nfn lenient(x: i64) -> bool {{ true }}\nfn ref1(x: i64) -> i64 {{ x * 2 }}\nfn lax(x: i64) -> i64 {{ 0 }}\ntype W = {{ d: Dict, n: i64 }}\n{pre}@[test]\nfn accept() {{\n{body}\n}}\n")
    };
    let setup = |own: bool| {
        let fill = if own {
            "  dict_set(d, \"k\", 1)\n"
        } else {
            "  fill(d)\n"
        };
        format!("  let d = dict_new()\n{fill}  let e = dict_new()\n  dict_set(e, \"k\", 1)\n")
    };
    let pick = |cond: &str| {
        format!("  let f = if {cond} {{ lenient }} else {{ strict }}\n  assert(f(cand()))")
    };
    let conds: &[(&str, &str, &str)] = &[
        ("d == e", "", "d == e"),
        ("d != e", "", "d != e"),
        ("e == d", "", "e == d"),
        ("[d] == [e]", "", "[d] == [e]"),
        ("(d, 1) == (e, 1)", "", "(d, 1) == (e, 1)"),
        (
            "a struct holding d",
            "  let w1 = W { d: d, n: 0 }\n  let w2 = W { d: e, n: 0 }\n",
            "w1 == w2",
        ),
        ("arr_contains", "", "arr_contains([d], e)"),
    ];
    let mut fails = Vec::new();
    for (what, extra, cond) in conds {
        let attack = suite("", &format!("{}{extra}{}", setup(false), pick(cond)));
        fails.extend(taint_attack(&format!("comparison: {what}"), &attack, cand));
        // The honest control: the operator's OWN dicts compared, and the
        // candidate's wrong answer judged by a check the operator chose.
        let own = suite("", &format!("{}{extra}{}", setup(true), pick(cond)));
        let s = check(&own, &[], cand, "accept");
        if refused_unkeyed(&s)
            && (s.stdout.contains("(runtime taint)") || s.stdout.contains("the candidate picked"))
        {
            fails.push(format!(
                "CONTROL REFUSED ({what}): the operator's own dicts: {}",
                s.stdout
            ));
        }
    }
    // The same data choosing a NAME.
    let name_attack = suite(
        "",
        &format!("{}  let p = principal_root(\"r\",true,true,true,100)\n  let sb = sandbox_create(p,\"IO\")\n  let nm = if d == e {{ \"lax\" }} else {{ \"ref1\" }}\n  assert(sandbox_run(sb, nm, 21) == 42)", setup(false)),
    );
    let s = check(&name_attack, &[], cand, "accept");
    if !refused_unkeyed(&s) {
        fails.push(format!(
            "ATTACK: a name chosen by d == e was accepted: {:?} {}",
            s.status, s.stdout
        ));
    }
    all_refused(fails);
}

/// Amendment 108, existence oracle on the METHOD path: with an operator
/// `impl Sc for i64 { fn score }`, a sealed `3.score()` and a sealed
/// `3.zzscore()` are refused in the same words.
#[test]
fn a_sealed_method_call_is_refused_in_the_same_words_for_an_operator_method_and_a_missing_one() {
    let su = "mod sol\nuse sol.{solve}\ntrait Sc {\n    fn score(self) -> i64\n}\nimpl Sc for i64 {\n    fn score(self: i64) -> i64 { 41 }\n}\nimpl Sc for str {\n    fn score(self: str) -> i64 { 41 }\n}\n@[test]\nfn accept() {\n  assert_eq(solve(), 41)\n}\n";
    let text = |name: &str, form: &str| {
        let cand = format!(
            "pub fn solve() -> i64 {{ {} }}\n",
            form.replace("{N}", name)
        );
        let s = check(su, &[], &cand, "accept");
        assert!(
            refused_unkeyed(&s),
            "{name} {form}: {:?} {}",
            s.status,
            s.stdout
        );
        let line = s
            .stdout
            .split("\"message\":\"")
            .nth(1)
            .and_then(|m| m.split("\",\"").next())
            .unwrap_or("")
            .to_string();
        line.replace(name, "@")
    };
    let mut fails = Vec::new();
    for form in ["3.{N}()", "\"a\".{N}()", "3.{N}().{N}()"] {
        let (a, b) = (text("score", form), text("zzscore", form));
        if a != b {
            fails.push(format!("{form}: [score] {a:?} vs [zzscore] {b:?}"));
        }
    }
    all_refused(fails);
}

// ---- C9 round 13, PSV1W (amendment 114): operator-side control flow -------------
//
// The reviewer's attack: `let ok = (d == e) && mark(op)`, then
// `let f = TBL[1 - dict_len(op)]` picks the lenient or the strict check by whether
// the right operand RAN, which was the candidate's data (V=1 passed a wrong answer,
// V=2 failed it), because the right operand of `&&`/`||` ran under no control
// taint. The class is every construct that evaluates something conditionally, a
// number of times, or leaves early on a value (`taint::CONTROL_TABLE`); the
// interpreter tests cover each row, and these run the runner leg of the shapes
// that matter most. An attack is refused by the taint rule and earns no keyed
// pass; its control (the SAME code under a condition the operator wrote) passes.

#[test]
fn operator_side_control_flow_on_candidate_data_never_selects_operator_code() {
    let cand = "pub fn cand() -> i64 { 4 }\npub fn candr() -> Result<i64, str> { Ok(4) }\npub fn fill(d: Dict) { dict_set(d, \"k\", 1) }\npub fn drain(c: Chan<i64>) { let x = c.recv() }\npub fn crd() -> i64 {\n  if cand() == 5 {\n    match read_file(\"/nonexistent/c9r14\") { Ok(s) => 1  Err(e) => 2 }\n  } else { 0 }\n}\n";
    let pre = "fn strict(x: i64) -> bool { x == 9 }\nfn lenient(x: i64) -> bool { true }\nlet TBL = [lenient, strict]\nfn mark(op: Dict) -> bool {\n  dict_set(op, \"x\", 1)\n  true\n}\nfn bump(op: Dict, x: i64) -> i64 {\n  dict_set(op, to_str(x), 1)\n  x\n}\nfn okr() -> Result<i64, str> { Ok(4) }\nfn tq(op: Dict) -> Result<i64, str> {\n  let v = candr()?\n  let u = mark(op)\n  Ok(v)\n}\nfn tq_ok(op: Dict) -> Result<i64, str> {\n  let v = okr()?\n  let u = mark(op)\n  Ok(v)\n}\n";
    let suite = |body: &str| {
        format!("mod sol\nuse sol.{{cand, candr, fill, drain, crd}}\n{pre}@[test]\nfn accept() {{\n  let op = dict_new()\n{body}\n}}\n")
    };
    let sel = "  let f = TBL[1 - dict_len(op)]\n  assert(f(cand()))";
    let sel2 = "  let f = TBL[2 - dict_len(op)]\n  assert(f(cand()))";
    // (what, the attack body, the control body, selector)
    let dict_setup = |v: &str| {
        format!(
            "  let d = dict_new()\n  fill(d)\n  let e = dict_new()\n  dict_set(e, \"k\", {v})\n"
        )
    };
    let cases: Vec<(&str, String, String, &str)> = vec![
        (
            "&& on a dict the candidate filled (the reviewer's attack)",
            format!("{}  let ok = (d == e) && mark(op)", dict_setup("1")),
            "  let d = dict_new()\n  dict_set(d, \"k\", 1)\n  let e = dict_new()\n  dict_set(e, \"k\", 1)\n  let ok = (d == e) && mark(op)".to_string(),
            sel,
        ),
        (
            "&& on the candidate's answer",
            "  let ok = (cand() == 4) && mark(op)".to_string(),
            "  let ok = (1 == 1) && mark(op)".to_string(),
            sel,
        ),
        (
            "|| on the candidate's answer",
            "  let ok = (cand() == 5) || mark(op)".to_string(),
            "  let ok = (1 == 2) || mark(op)".to_string(),
            sel,
        ),
        (
            "&& whose right operand assigns a local that sizes a loop",
            "  let k = 0\n  let ok = (cand() == 4) && { k = 1\n    true }\n  for i in 0..k { let u = mark(op) }".to_string(),
            "  let k = 0\n  let ok = (1 == 1) && { k = 1\n    true }\n  for i in 0..k { let u = mark(op) }".to_string(),
            sel,
        ),
        (
            "&& inside a closure",
            "  let g = || (cand() == 4) && mark(op)\n  let ok = g()".to_string(),
            "  let g = || (1 == 1) && mark(op)\n  let ok = g()".to_string(),
            sel,
        ),
        (
            "&& inside an arr_any callback",
            "  let r = arr_any([1], |x| (cand() == 4) && mark(op))".to_string(),
            "  let r = arr_any([1], |x| (1 == 1) && mark(op))".to_string(),
            sel,
        ),
        (
            "an arm taken because a guard refused the earlier arm",
            "  let q = match 1 { x if cand() == 5 => 0, _ => { let u = mark(op)\n    1 } }".to_string(),
            "  let q = match 1 { x if 1 == 5 => 0, _ => { let u = mark(op)\n    1 } }".to_string(),
            sel,
        ),
        (
            "what runs after `?` on the candidate's result",
            "  let z = tq(op)".to_string(),
            "  let z = tq_ok(op)".to_string(),
            sel,
        ),
        (
            "a select arm that fired because the candidate drained the other channel",
            "  let a = chan<i64>()\n  let b = chan<i64>()\n  a.send(7)\n  b.send(7)\n  drain(a)\n  let s = select { a.recv() => 0  b.recv() => { let u = mark(op)\n    1 } }".to_string(),
            "  let a = chan<i64>()\n  let b = chan<i64>()\n  b.send(7)\n  let s = select { a.recv() => 0  b.recv() => { let u = mark(op)\n    1 } }".to_string(),
            sel,
        ),
        (
            "an arr_find stopped by the candidate's answer",
            "  let r = arr_find([1, 2, 3], |x| { let u = bump(op, x)\n    x == cand() - 2 })".to_string(),
            "  let r = arr_find([1, 2, 3], |x| { let u = bump(op, x)\n    x == 4 - 2 })".to_string(),
            sel2,
        ),
        // Amendment 117 (PSV-1 loop findings 2, 26, 3/10/18/49, 20, 17, 31/32/34).
        (
            "a match guard run because the candidate's answer matched the pattern",
            "  let q = match cand() { 4 if mark(op) => 1  _ => 0 }".to_string(),
            "  let q = match 4 { 4 if mark(op) => 1  _ => 0 }".to_string(),
            sel,
        ),
        (
            "a while condition evaluated a number of times the candidate's answer chose",
            "  let i = 0\n  let c = cand()\n  let g = || { let u = dict_inc(op, \"c\")\n    true }\n  while g() && i < c - 3 { i = i + 1 }".to_string(),
            "  let i = 0\n  let c = 4\n  let g = || { let u = dict_inc(op, \"c\")\n    true }\n  while g() && i < c - 3 { i = i + 1 }".to_string(),
            "  let f = TBL[min_i64(max_i64(dict_get_or(op, \"c\", 0) - 2, 0), 1)]\n  assert(f(cand()))",
        ),
        (
            "a parameter-free store in an arr_any callback stopped by the candidate's answer",
            "  let r = arr_any([1, 2, 3], |x| { let u = dict_inc(op, \"c\")\n    x == cand() - 2 })".to_string(),
            "  let r = arr_any([1, 2, 3], |x| { let u = dict_inc(op, \"c\")\n    x == 4 - 2 })".to_string(),
            "  let f = TBL[min_i64(max_i64(dict_get_or(op, \"c\", 0) - 2, 0), 1)]\n  assert(f(cand()))",
        ),
        (
            "an arr_sort_by comparator whose call count the candidate's answers decide",
            "  let r = arr_sort_by([3, 1, 2], |a, b| { let u = dict_inc(op, \"c\")\n    (a - b) * (cand() - 3) })".to_string(),
            "  let r = arr_sort_by([3, 1, 2], |a, b| { let u = dict_inc(op, \"c\")\n    (a - b) * (4 - 3) })".to_string(),
            "  let f = TBL[min_i64(max_i64(dict_get_or(op, \"c\", 0) - 3, 0), 1)]\n  assert(f(cand()))",
        ),
        (
            "a select arm that fired because the candidate chose which channel to look at",
            "  let a = chan<i64>()\n  let e = chan<i64>()\n  let b = chan<i64>()\n  a.send(7)\n  b.send(7)\n  let c = if cand() == 4 { a } else { e }\n  let s = select { c.recv() => { let u = mark(op)\n    1 }  b.recv() => 0 }".to_string(),
            "  let a = chan<i64>()\n  let e = chan<i64>()\n  let b = chan<i64>()\n  a.send(7)\n  b.send(7)\n  let c = if 1 == 1 { a } else { e }\n  let s = select { c.recv() => { let u = mark(op)\n    1 }  b.recv() => 0 }".to_string(),
            sel,
        ),
        (
            "an operator channel popped because the candidate's answer was 4",
            "  let ch = chan<i64>()\n  ch.send(0)\n  ch.send(1)\n  if cand() == 4 { let u = ch.recv() }\n  let k = ch.recv()".to_string(),
            "  let ch = chan<i64>()\n  ch.send(0)\n  ch.send(1)\n  if 1 == 1 { let u = ch.recv() }\n  let k = ch.recv()".to_string(),
            "  let f = TBL[1 - k]\n  assert(f(cand()))",
        ),
        (
            "what follows candidate code that could have aborted an operator with body",
            "  let r = with handler { on IO(p) => 5 } {\n    let a = crd()\n    let u = mark(op)\n    7\n  }".to_string(),
            "  let r = with handler { on IO(p) => 5 } {\n    let a = 3\n    let u = mark(op)\n    7\n  }".to_string(),
            sel,
        ),
    ];
    let mut fails = Vec::new();
    for (what, attack, control, selector) in &cases {
        fails.extend(taint_attack(
            what,
            &suite(&format!("{attack}\n{selector}")),
            cand,
        ));
        let s = check(
            &suite(&format!("{control}\n{selector}")),
            &[],
            cand,
            "accept",
        );
        if (s.status, s.host) != (GuestStatus::Passed, Some(true)) {
            fails.push(format!(
                "CONTROL ({what}): the operator's own condition was not a keyed pass: {:?} {:?} {}",
                s.status, s.host, s.stdout
            ));
        }
    }
    all_refused(fails);
}

/// Amendment 114: the sealed-only check holds the operator's `mod f` when `f` is one of the
/// CANDIDATE's modules (the suite declares the candidate's module names), so a candidate whose
/// module uses its own helper (`use f::h`) resolves, while an operator module the candidate
/// does not ship is still a module nothing declares. Found by the fabric suite
/// (`a_check_loads_no_module_from_outside_the_suite_and_the_candidate`) after the first form.
#[test]
fn a_module_the_operator_declares_for_the_candidate_resolves_the_candidates_own_helper() {
    let suite = "mod f\nmod opmod\nmod sol\nuse sol.{unused}\nuse f.{double}\n@[test]\nfn accept() {\n  assert_eq(double(21), 42)\n}\n";
    let files: &[(&str, &str)] = &[
        (
            "cand/f.ax",
            "use f::h\n\nfn double(n: i64) -> i64 { d2(n) }\n",
        ),
        ("cand/f/h.ax", "fn d2(n: i64) -> i64 { n * 2 }\n"),
    ];
    let s = check(suite, files, "pub fn unused() -> i64 { 0 }\n", "accept");
    passed(&s, "a candidate module with its own helper");
    // The operator's other module is no module of the candidate's.
    let s = check(
        suite,
        files,
        "use opmod.{a}\npub fn unused() -> i64 { 0 }\n",
        "accept",
    );
    assert!(
        refused_unkeyed(&s) && candidate_visible(&s).contains("`opmod` not found"),
        "ATTACK or wrong text: {:?} {}",
        s.status,
        candidate_visible(&s)
    );
}

/// Amendment 117 (PSV-1 loop findings 0, 1, 11, 13): `temporal_new` and
/// `temporal_is_valid` read the one process clock while classed Pure and outside
/// the Time effect, so a candidate could advance a clock the operator read back
/// through a "pure" builtin, and a run that refused `now_ms` still had a clock
/// reader. They are World, and Time: under the runner's ceiling (no Time) a
/// candidate that calls either is refused, as one that calls `now_ms` is. (A candidate
/// cannot build a `Temporal` for `temporal_is_valid` without `temporal_new`, so that
/// builtin's Time row is pinned in the interpreter unit test
/// `no_arm_of_a_pure_builtin_reads_or_advances_ambient_state`.)
#[test]
fn a_candidate_cannot_read_or_drive_the_clock_through_the_temporal_builtins() {
    let suite = "mod sol\nuse sol.{solve}\n@[test]\nfn accept() {\n  assert_eq(solve(), 42)\n}\n";
    let honest = "pub fn solve() -> i64 { 42 }\n";
    passed(&check(suite, &[], honest, "accept"), "an honest candidate");
    for (what, body) in [
        ("now_ms", "let t = now_ms()\n  42"),
        ("temporal_now", "let t = temporal_now()\n  42"),
        ("temporal_new", "let t = temporal_new(0, 1000, 0.5)\n  42"),
    ] {
        let cand = format!("pub fn solve() -> i64 {{\n  {body}\n}}\n");
        let s = check(suite, &[], &cand, "accept");
        assert!(
            refused_unkeyed(&s),
            "ATTACK: the candidate used the clock through `{what}`: {:?} {:?} {}",
            s.status,
            s.host,
            s.stdout
        );
    }
}
