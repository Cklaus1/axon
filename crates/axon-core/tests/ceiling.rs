//! ceilings (`tasks/ceilings-spec.md`): the operator's bounds on a run.
//!
//! Each test runs the `axon` binary this test run built, with every ambient
//! bound variable cleared first, so the environment of the caller never
//! decides what is bounded.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BOUND_VARS: [&str; 8] = [
    "AXON_ALLOWED_EFFECTS",
    "AXON_ALLOWED_FS_READ",
    "AXON_ALLOWED_FS_WRITE",
    "AXON_ALLOWED_NET",
    "AXON_BUDGET_TOKENS",
    "AXON_AI_MOCK",
    "AXON_ENGINE",
    "AXON_MAX_DEPTH",
];

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon_ceiling_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `axon <args..> <dir>/p.ax` with every bound variable cleared, then `vars`
/// set; the provenance cache is kept inside `dir`.
fn run_with(dir: &Path, args: &[&str], src: &str, vars: &[(&str, &str)]) -> Output {
    let prog = dir.join("p.ax");
    std::fs::write(&prog, src).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_axon"));
    cmd.args(args)
        .arg(&prog)
        .current_dir(dir)
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env_remove("AXON_CEILING")
        .env_remove("AXON_MAX_STEPS")
        .env_remove("AXON_WALL_MS");
    for v in BOUND_VARS {
        cmd.env_remove(v);
    }
    for (k, v) in vars {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn run(dir: &Path, src: &str, vars: &[(&str, &str)]) -> Output {
    run_with(dir, &["run"], src, vars)
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)
}

// ── CE-7: `goal_*` dispatch demands only `IO` ───────────────────────────────

const PURE_METRIC: &str = r#"
@[adaptive(metric: cost, target: 0.0)]
fn measured(x: i64) -> i64 {
    let d = x - 7
    d * d
}
"#;

const AI_METRIC: &str = r#"
@[adaptive(metric: cost, target: 0.0)]
fn measured(x: i64) -> i64 {
    let r = ai_complete("score this")
    let d = x - 7
    d * d
}
"#;

const GOAL_MAIN: &str = r#"
fn main() {
    let best = goal_run("measured", 0.0, 4)
    println("best=" + to_str_f64(best))
}
"#;

/// g02: the program makes an `IO`-only sandbox and calls `goal_run` in it.
const GOAL_IN_SANDBOX: &str = r#"
fn job(x: i64) -> i64 {
    let best = goal_run("measured", 0.0, 4)
    println("best=" + to_str_f64(best))
    0
}

fn main() {
    let p = principal_root("p", true, false, false, 100)
    let sb = sandbox_create(p, "IO")
    let r = sandbox_run(sb, "job", 0)
    println("r=" + to_str(r))
}
"#;

/// RED before CE-7: `goal_run` was refused (exit 8) for demanding `AI`,
/// although its metric `(x-7)^2` never calls a model.
#[test]
fn goal_run_pure_metric_runs_without_ai_grant() {
    let d = tmp("ce7_pure_ambient");
    let src = format!("{PURE_METRIC}{GOAL_MAIN}");
    let o = run(&d, &src, &[("AXON_ALLOWED_EFFECTS", "IO")]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "goal_run with a pure metric must run under AXON_ALLOWED_EFFECTS=IO: {}",
        text(&o)
    );
    assert!(text(&o).contains("best="), "{}", text(&o));
}

/// RED before CE-7: the same refusal inside a sandbox the program made itself.
#[test]
fn goal_run_pure_metric_runs_in_program_sandbox() {
    let d = tmp("ce7_pure_sandbox");
    let src = format!("{PURE_METRIC}{GOAL_IN_SANDBOX}");
    let o = run(&d, &src, &[("AXON_AI_MOCK", "1")]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "goal_run with a pure metric must run in an IO sandbox: {}",
        text(&o)
    );
    assert!(text(&o).contains("r=0"), "{}", text(&o));
}

/// Guard: a metric that DOES call a model is still refused without `AI`; the
/// refusal is at the metric's `ai_complete`.
#[test]
fn goal_run_ai_metric_still_refused_without_ai_grant() {
    let d = tmp("ce7_ai_ambient");
    let src = format!("{AI_METRIC}{GOAL_MAIN}");
    let o = run(
        &d,
        &src,
        &[("AXON_ALLOWED_EFFECTS", "IO"), ("AXON_AI_MOCK", "1")],
    );
    let t = text(&o);
    assert_eq!(
        o.status.code(),
        Some(8),
        "ATTACK: a goal_run metric reached a model without an AI grant: {t}"
    );
    assert!(
        t.contains("builtin `ai_complete` requires effect `AI`"),
        "ATTACK: a goal_run metric reached a model without an AI grant: {t}"
    );
}

/// Guard: the same inside a program-made `IO` sandbox.
#[test]
fn goal_run_ai_metric_refused_in_program_sandbox() {
    let d = tmp("ce7_ai_sandbox");
    let src = format!("{AI_METRIC}{GOAL_IN_SANDBOX}");
    let o = run(&d, &src, &[("AXON_AI_MOCK", "1")]);
    let t = text(&o);
    assert_eq!(
        o.status.code(),
        Some(8),
        "ATTACK: a sandboxed goal_run metric reached a model: {t}"
    );
    assert!(
        t.contains("builtin `ai_complete` requires effect `AI`"),
        "ATTACK: a sandboxed goal_run metric reached a model: {t}"
    );
}

// ── CE-2: `axon-ceiling/1` ─────────────────────────────────────────────────

/// A complete ceiling whose keys are each `null`-unbounded except `fs_write`
/// (`[]`), with `over` replacing (or, for a new key, adding) entries.
/// `None` in `over` drops the key.
fn ceiling_json(over: &[(&str, Option<&str>)]) -> String {
    let mut keys: Vec<(String, String)> = [
        ("schema", "\"axon-ceiling/1\""),
        ("effects", "null"),
        ("fs_read", "null"),
        ("fs_write", "[]"),
        ("net", "null"),
        ("ai_tokens", "null"),
        ("max_depth", "null"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    for (k, v) in over {
        let at = keys.iter().position(|(x, _)| x == k);
        match (at, v) {
            (Some(i), Some(v)) => keys[i].1 = v.to_string(),
            (Some(i), None) => {
                keys.remove(i);
            }
            (None, Some(v)) => keys.push((k.to_string(), v.to_string())),
            (None, None) => {}
        }
    }
    let body: Vec<String> = keys.iter().map(|(k, v)| format!("\"{k}\": {v}")).collect();
    format!("{{ {} }}\n", body.join(", "))
}

/// Write `json` to `<dir>/policy/ceiling.json` (outside the program's own
/// directory) and return its path.
fn write_ceiling(dir: &Path, json: &str) -> PathBuf {
    let p = dir.join("policy");
    std::fs::create_dir_all(&p).unwrap();
    let f = p.join("ceiling.json");
    std::fs::write(&f, json).unwrap();
    f
}

const HELLO: &str = "fn main() { println(\"hi\") }\n";

fn assert_e1007(o: &Output, what: &str, attack: &str) {
    let t = text(o);
    assert_eq!(o.status.code(), Some(2), "{attack}: {what}: {t}");
    assert!(t.contains("error[E1007]"), "{attack}: {what}: {t}");
    assert!(t.contains(what), "{attack}: {what}: {t}");
    assert!(!t.contains("hi\n"), "{attack}: program code ran: {t}");
}

#[test]
fn ceiling_missing_key_is_e1007() {
    let d = tmp("ce2_missing");
    let c = write_ceiling(&d, &ceiling_json(&[("max_depth", None)]));
    let o = run_with(&d, &["run", "--ceiling", c.to_str().unwrap()], HELLO, &[]);
    assert_e1007(
        &o,
        "missing key(s) `max_depth`",
        "ATTACK: a ceiling with a missing key was accepted",
    );
}

#[test]
fn ceiling_unknown_key_is_e1007() {
    let d = tmp("ce2_unknown");
    let c = write_ceiling(&d, &ceiling_json(&[("stpes", Some("10"))]));
    let o = run(&d, HELLO, &[("AXON_CEILING", c.to_str().unwrap())]);
    assert_e1007(
        &o,
        "unknown key `stpes`",
        "ATTACK: a misspelled ceiling key was ignored",
    );
}

#[test]
fn ceiling_fs_write_null_is_e1007() {
    let d = tmp("ce2_fswrite_null");
    let c = write_ceiling(&d, &ceiling_json(&[("fs_write", Some("null"))]));
    let o = run(&d, HELLO, &[("AXON_CEILING", c.to_str().unwrap())]);
    assert_e1007(
        &o,
        "`fs_write` must be a list",
        "ATTACK: an unbounded fs_write ceiling was accepted",
    );
}

#[test]
fn ceiling_inside_own_write_scope_is_e1007() {
    let d = tmp("ce2_self_overlap");
    let policy = d.join("policy");
    let scope = format!("[\"{}/\"]", policy.display());
    let c = write_ceiling(&d, &ceiling_json(&[("fs_write", Some(&scope))]));
    let o = run(&d, HELLO, &[("AXON_CEILING", c.to_str().unwrap())]);
    assert_e1007(
        &o,
        "lies inside its own `fs_write` scope",
        "ATTACK: a ceiling the run may rewrite was accepted",
    );
}

/// `AXON_ALLOWED_EFFECTS=IO,AI` and a ceiling of `["IO"]`: the narrower wins,
/// so an AI call is a sandbox violation (exit 8).
#[test]
fn ceiling_intersects_env_never_widens() {
    let d = tmp("ce2_intersect");
    let c = write_ceiling(
        &d,
        &ceiling_json(&[
            ("effects", Some("[\"IO\"]")),
            ("net", Some("[\"api.anthropic.com\"]")),
        ]),
    );
    let src = r#"
@[contained(net: ["api.anthropic.com"])]
fn main() {
    match ai_complete("hello") {
        Ok(s) => println("reply " + s),
        Err(e) => println("err " + e),
    }
}
"#;
    let o = run(
        &d,
        src,
        &[
            ("AXON_CEILING", c.to_str().unwrap()),
            ("AXON_ALLOWED_EFFECTS", "IO,AI"),
            ("AXON_AI_MOCK", "1"),
        ],
    );
    let t = text(&o);
    assert_eq!(
        o.status.code(),
        Some(8),
        "ATTACK: the env var widened the ceiling's effects: {t}"
    );
    assert!(
        t.contains("builtin `ai_complete` requires effect `AI`"),
        "ATTACK: the env var widened the ceiling's effects: {t}"
    );
}

#[test]
fn ceiling_digest_in_run_start() {
    use sha2::{Digest, Sha256};
    let d = tmp("ce2_digest");
    let json = ceiling_json(&[]);
    let c = write_ceiling(&d, &json);
    let o = run(&d, HELLO, &[("AXON_CEILING", c.to_str().unwrap())]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let want: String = Sha256::digest(json.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let log = std::fs::read_to_string(d.join("cache/axon/provenance.jsonl")).unwrap_or_default();
    let start = log
        .lines()
        .rev()
        .find(|l| l.contains("\"event\":\"run_start\""))
        .unwrap_or("");
    assert!(
        start.contains(&format!("\"ceiling_sha256\":\"{want}\"")),
        "ATTACK: run_start does not name the ceiling that bounded the run: {start:?}"
    );
}

/// An `ai_tokens` ceiling lowers the run token cap (E1303, exit 5).
#[test]
fn ceiling_ai_tokens_caps_the_run() {
    let d = tmp("ce2_tokens");
    let c = write_ceiling(
        &d,
        &ceiling_json(&[
            ("ai_tokens", Some("1")),
            ("net", Some("[\"api.anthropic.com\"]")),
        ]),
    );
    let src = r#"
@[contained(net: ["api.anthropic.com"])]
fn main() {
    match ai_complete("a prompt long enough to cost more than one token") {
        Ok(s) => println("reply " + s),
        Err(e) => println("err " + e),
    }
}
"#;
    let o = run(
        &d,
        src,
        &[
            ("AXON_CEILING", c.to_str().unwrap()),
            ("AXON_BUDGET_TOKENS", "100000"),
            ("AXON_AI_MOCK", "1"),
        ],
    );
    let t = text(&o);
    assert_eq!(
        o.status.code(),
        Some(5),
        "ATTACK: the ceiling's ai_tokens did not cap the run: {t}"
    );
    assert!(
        t.contains("E1303"),
        "ATTACK: the ceiling's ai_tokens did not cap the run: {t}"
    );
}

/// A `max_depth` ceiling lowers the recursion limit.
#[test]
fn ceiling_max_depth_lowers_the_limit() {
    let d = tmp("ce2_depth");
    let c = write_ceiling(&d, &ceiling_json(&[("max_depth", Some("50"))]));
    let src = r#"
fn down(n: i64) -> i64 {
    if n == 0 { 0 } else { 1 + down(n - 1) }
}
fn main() { println(to_str(down(200))) }
"#;
    let free = run(&d, src, &[]);
    assert_eq!(free.status.code(), Some(0), "{}", text(&free));
    let o = run(&d, src, &[("AXON_CEILING", c.to_str().unwrap())]);
    assert_ne!(
        o.status.code(),
        Some(0),
        "ATTACK: the ceiling's max_depth did not bound recursion: {}",
        text(&o)
    );
}

// ── CE-10: a ceiling's presence turns R45 on ────────────────────────────────

const C03_LEAK_FS: &str = r#"
fn leak() -> str { match read_file("/etc/hostname") { Ok(s) => s, Err(e) => "" } }
fn main() { println(to_str(len(leak()) > 0)) }
"#;

#[test]
fn ceiling_implies_require_contained() {
    let d = tmp("ce10_r45");
    let bare = run(&d, C03_LEAK_FS, &[]);
    assert_eq!(
        bare.status.code(),
        Some(0),
        "bare runs are unchanged: {}",
        text(&bare)
    );
    let c = write_ceiling(
        &d,
        &ceiling_json(&[("fs_read", Some("[]")), ("net", Some("[]"))]),
    );
    let o = run(&d, C03_LEAK_FS, &[("AXON_CEILING", c.to_str().unwrap())]);
    let t = text(&o);
    assert_eq!(
        o.status.code(),
        Some(2),
        "ATTACK: a ceilinged run reached an ungranted capability without R45: {t}"
    );
    assert!(
        t.contains("E1005"),
        "ATTACK: a ceilinged run reached an ungranted capability without R45: {t}"
    );
}

// -- CE-3: the AI endpoint under a ceiling (NH-C3 A) ------------------------

/// A program granted every host (`net: ["*"]`) under a ceiling whose `net` is
/// `["api.anthropic.com"]`: the AI endpoint is pinned to the ceiling, so an
/// `AXON_AI_BASE_URL` naming a local listener gets the catchable allowlist
/// `Err` and the listener sees no connection (no prompt, no key). Without the
/// ceiling the same run dials the listener - an unceilinged run may send AI
/// traffic wherever `AXON_AI_BASE_URL` points (documented). Live AI exists
/// only in `asi-runtime` builds.
#[cfg(feature = "asi-runtime")]
#[test]
fn ceiling_net_pins_the_ai_endpoint() {
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    const SRC: &str = "@[contained(net: [\"*\"])]\nfn main() {\n    match ai_complete(\"hi\") { Ok(s) => println(s), Err(e) => println(\"err=\" + e) }\n}\n";
    // Returns (connections seen, output).
    let probe = |tag: &str, ceiling: bool| -> (usize, String) {
        let d = tmp(tag);
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        // Count and drop each connection at once, so the client fails fast
        // instead of waiting out its HTTP timeout. The thread is detached.
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&seen);
        std::thread::spawn(move || {
            for s in l.incoming() {
                counter.fetch_add(1, Ordering::SeqCst);
                drop(s);
            }
        });
        let c = write_ceiling(
            &d,
            &ceiling_json(&[("net", Some("[\"api.anthropic.com\"]"))]),
        );
        let mut vars = vec![
            ("ANTHROPIC_API_KEY", "sk-dummy"),
            ("AXON_AI_BASE_URL", url.as_str()),
        ];
        let cs = c.to_str().unwrap().to_string();
        if ceiling {
            vars.push(("AXON_CEILING", cs.as_str()));
        }
        let o = run(&d, SRC, &vars);
        (seen.load(Ordering::SeqCst), text(&o))
    };
    let (bare, _) = probe("ce3_bare", false);
    assert!(
        bare > 0,
        "control: the unceilinged run dials AXON_AI_BASE_URL"
    );
    let (seen, t) = probe("ce3_ceil", true);
    assert_eq!(
        seen, 0,
        "ATTACK: a ceiling net scope left the AI endpoint unpinned: {t}"
    );
    assert!(
        t.contains("is not in this program's net allowlist"),
        "ATTACK: a ceiling net scope left the AI endpoint unpinned: {t}"
    );
}

// -- CE-4: a budget bounds its callees (NH-C4 A) ------------------------------

/// Run `src` under `AXON_AI_MOCK=1` and return (exit code, stdout+stderr).
fn run_mock(tag: &str, src: &str) -> (Option<i32>, String) {
    let d = tmp(tag);
    let o = run(&d, src, &[("AXON_AI_MOCK", "1")]);
    (o.status.code(), text(&o))
}

/// E1301 naming `fn_name` with exit 5, or panic with `attack`.
fn assert_e1301(r: (Option<i32>, String), fn_name: &str, attack: &str) {
    let (code, t) = r;
    assert_eq!(code, Some(5), "{attack}: {t}");
    assert!(
        t.contains("E1301") && t.contains(&format!("`{fn_name}` exceeded its AI budget")),
        "{attack}: {t}"
    );
}

#[test]
fn budget_covers_unbudgeted_helper() {
    const SRC: &str = "fn helper() -> i64 {\n    let _x = ai_complete(\"hidden\")\n    1\n}\n\n@[ai(policy(budget: 1))]\nfn ask() -> i64 { helper() + helper() + helper() }\n\nfn main() { println(to_str(ask())) }\n";
    assert_e1301(
        run_mock("ce4_helper", SRC),
        "ask",
        "ATTACK: an unbudgeted helper laundered AI calls past the caller's budget",
    );
}

#[test]
fn nested_budget_bounded_by_outer() {
    const SRC: &str = "@[ai(policy(budget: 5))]\nfn inner() -> i64 {\n    let _a = ai_complete(\"x\")\n    let _b = ai_complete(\"y\")\n    2\n}\n\n@[ai(policy(budget: 1))]\nfn outer() -> i64 { inner() + inner() + inner() }\n\nfn main() { println(to_str(outer())) }\n";
    assert_e1301(
        run_mock("ce4_nested", SRC),
        "outer",
        "ATTACK: a callee's larger budget widened the caller's budget",
    );
}

#[test]
fn lambda_budget_unchanged() {
    const SRC: &str = "@[ai(policy(budget: 1))]\nfn ask() -> i64 {\n    let f = |p: str| match ai_complete(p) { Ok(s) => 1, Err(e) => 0 }\n    f(\"a\") + f(\"b\") + f(\"c\")\n}\n\nfn main() { println(to_str(ask())) }\n";
    assert_e1301(
        run_mock("ce4_lambda", SRC),
        "ask",
        "ATTACK: a lambda's AI calls escaped its fn's budget",
    );
}

/// Extracts are model calls too (AF-4 routes them through the same prologue):
/// they count against a budget like `ai_complete`.
#[test]
fn budget_counts_extracts() {
    const SRC: &str = "@[ai(policy(budget: 1))]\nfn ask() -> i64 {\n    let a = match ai_extract::<i64>(\"one\") { Ok(n) => n, Err(e) => 0 }\n    let b = match ai_extract::<i64>(\"two\") { Ok(n) => n, Err(e) => 0 }\n    a + b\n}\n\nfn main() { println(to_str(ask())) }\n";
    assert_e1301(
        run_mock("ce4_extract", SRC),
        "ask",
        "ATTACK: an ai_extract call escaped the fn's AI budget",
    );
}

/// The control: a budget that covers every call in its extent runs clean.
#[test]
fn budget_within_extent_runs() {
    const SRC: &str = "fn helper() -> i64 {\n    let _x = ai_complete(\"hidden\")\n    1\n}\n\n@[ai(policy(budget: 3))]\nfn ask() -> i64 { helper() + helper() + helper() }\n\nfn main() { println(to_str(ask())) }\n";
    let (code, t) = run_mock("ce4_ok", SRC);
    assert_eq!(code, Some(0), "a budget of 3 covers 3 calls: {t}");
    assert!(t.contains('3'), "{t}");
}
