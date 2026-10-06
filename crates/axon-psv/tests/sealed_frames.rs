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
            SUITE_DISPATCH,
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
    let r = solve(3)
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
        "    let c = chan<i64>()\n    fill(c)\n    assert(c.recv().ok())",
    );
    let noret = suite("solve", "    assert(solve(3).ok())");
    let wrap = suite(
        "solve, Wrap",
        "    let w = solve(Wrap { v: 3 })\n    assert(w.v.ok())",
    );
    let clos = suite("apply", "    assert(apply(|x| x.ok()))");
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
        "mod sol\nuse sol.{{solve}}\n{J}@[test]\nfn accept() {{\n    let d = dict_new()\n    dict_set(d, \"a\", 3)\n    solve(d)\n    match dict_get(d, \"a\") {{\n        Some(x) => assert(x.ok())\n        None => assert(false)\n    }}\n}}\n"
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
    let replace = suite("    let inner = dict_new()\n    dict_set(inner, \"x\", 3)\n    let d = dict_new()\n    dict_set(d, \"inner\", inner)\n    solve(d)\n    match dict_get(d, \"inner\") {\n        Some(i) => match dict_get(i, \"x\") {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }\n        None => assert(false)\n    }");
    let fill = suite("    let d = dict_new()\n    dict_set(d, \"best\", Some(0))\n    solve(d)\n    match dict_get(d, \"best\") {\n        Some(o) => match o {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }\n        None => assert(false)\n    }");
    let hold = suite("    let d = dict_new()\n    dict_set(d, \"best\", None)\n    solve(d)\n    match dict_get(d, \"best\") {\n        Some(o) => match o {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }\n        None => assert(false)\n    }");
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
