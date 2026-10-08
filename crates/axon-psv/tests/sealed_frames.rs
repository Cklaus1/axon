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
        std::fs::write(suite_dir.join(name), body).unwrap();
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
    let host = keyed_outcome(&stdout, test, &completion_key(&secret, &m));
    Seen {
        status: v.status,
        host,
        stdout,
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
