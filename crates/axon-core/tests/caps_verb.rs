//! R0a — `axon caps <file|-> [--fn NAME] --json` (schema `axon-caps/1`).
//!
//! A repair fence is built from what the PRISTINE code can exercise, and a
//! candidate is refused when it reaches more. That only holds if "what a fn can
//! exercise" is computed by the walker the language's own `@[contained]` check
//! uses: a second, hand-written list (`axon_os::runtime::scan_effects`) missed
//! `append_file`, `file_copy`, `file_rename` and `dir_create`, and a candidate
//! wrote a file during its checks with exit 0.
//!
//! Red before the fix: the verb did not exist (`unrecognized subcommand
//! 'caps'`, exit 2) for every test here.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn axon() -> &'static str {
    env!("CARGO_BIN_EXE_axon")
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon_caps_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(dir: &Path, name: &str, src: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, src).unwrap();
    p
}

/// `(exit, stdout, stderr)` of `axon caps <file> [--fn f] --json`.
fn caps(file: &Path, func: Option<&str>) -> (i32, String, String) {
    let mut cmd = Command::new(axon());
    cmd.arg("caps").arg(file).arg("--json");
    if let Some(f) = func {
        cmd.arg("--fn").arg(f);
    }
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn report(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("not JSON ({e}): {stdout}"))
}

fn strings(v: &serde_json::Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("`{key}` missing: {v}"))
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

const PROGRAM: &str = r#"
fn log_it(p: str, s: str) -> i64 {
    let _ = append_file(p, s)
    0
}

fn via_helper(n: i64) -> i64 {
    log_it("/tmp/axon_caps_x", "y")
    n
}

fn via_alias(n: i64) -> i64 {
    let f = |p: str, s: str| append_file(p, s)
    let _ = f("/tmp/axon_caps_y", "z")
    n
}

fn via_fn_value(n: i64) -> i64 {
    let g = log_it
    g("/tmp/axon_caps_z", "w")
}

fn moves(n: i64) -> i64 {
    let _ = file_copy("a", "b")
    let _ = file_rename("b", "c")
    let _ = dir_create("d")
    n
}

fn pure(n: i64) -> i64 {
    n * 2
}

fn runs(n: i64) -> i64 {
    let _ = exec("ls", [])
    let _ = ai_complete("hi")
    println("x")
    n
}

fn main() {
    println("{pure(2)}")
}
"#;

#[test]
fn a_helper_reaching_append_file_is_fs_write() {
    let d = tmp("helper");
    let f = write(&d, "p.ax", PROGRAM);
    let (code, out, err) = caps(&f, Some("via_helper"));
    assert_eq!(code, 0, "{err}");
    let r = report(&out);
    assert_eq!(r["schema"], "axon-caps/1");
    assert_eq!(r["fn"], "via_helper");
    assert_eq!(
        strings(&r, "capabilities"),
        ["fs:write"],
        "ATTACK: --fn via_helper hid the write its helper does: {r}"
    );
    assert_eq!(strings(&r, "effects"), ["IO"]);
    assert_eq!(r["unknown"], false);
}

#[test]
fn an_alias_or_a_fn_value_reaching_append_file_is_fs_write() {
    let d = tmp("alias");
    let f = write(&d, "p.ax", PROGRAM);
    for func in ["via_alias", "via_fn_value"] {
        let (code, out, err) = caps(&f, Some(func));
        assert_eq!(code, 0, "{func}: {err}");
        assert_eq!(
            strings(&report(&out), "capabilities"),
            ["fs:write"],
            "{func}"
        );
    }
}

/// The type checker refuses a bare builtin as a value (E0306), so such a
/// program never runs and `caps` refuses it like any type error. The walker
/// still classifies the reference, so the answer does not depend on that
/// checker rule staying in place.
#[test]
fn a_bare_builtin_alias_is_fs_write_to_the_walker_and_a_type_error_to_the_verb() {
    let src = r#"
fn sneaky(n: i64) -> i64 {
    let f = write_file
    let _ = f("/tmp/axon_caps_bare", "x")
    n
}
fn copier(n: i64) -> i64 {
    let c = file_copy
    n
}
"#;
    let program = axon_core::parse_source(src).unwrap();
    let r = axon_core::capabilities::reachable_capabilities(&program, Some("sneaky")).unwrap();
    assert_eq!(r.capabilities.iter().collect::<Vec<_>>(), ["fs:write"]);
    assert_eq!(r.effects.iter().collect::<Vec<_>>(), ["IO"]);
    let r = axon_core::capabilities::reachable_capabilities(&program, Some("copier")).unwrap();
    assert_eq!(
        r.capabilities.iter().collect::<Vec<_>>(),
        ["fs:read", "fs:write"]
    );

    let d = tmp("bare");
    let f = write(&d, "p.ax", src);
    let (code, out, err) = caps(&f, Some("sneaky"));
    assert_eq!(code, 2, "stdout: {out}");
    assert!(err.contains("E0306"), "{err}");
    assert!(out.is_empty(), "no report on a type error: {out}");
}

#[test]
fn the_multi_path_builtins_are_reported() {
    let d = tmp("moves");
    let f = write(&d, "p.ax", PROGRAM);
    let (code, out, err) = caps(&f, Some("moves"));
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        strings(&report(&out), "capabilities"),
        ["fs:read", "fs:write"]
    );
}

#[test]
fn a_pure_fn_reports_nothing() {
    let d = tmp("pure");
    let f = write(&d, "p.ax", PROGRAM);
    let (code, out, err) = caps(&f, Some("pure"));
    assert_eq!(code, 0, "{err}");
    let r = report(&out);
    assert!(strings(&r, "capabilities").is_empty(), "{r}");
    assert!(strings(&r, "effects").is_empty(), "{r}");
    assert_eq!(r["unknown"], false);
}

#[test]
fn exec_and_ai_builtins_appear() {
    let d = tmp("exec");
    let f = write(&d, "p.ax", PROGRAM);
    let (code, out, err) = caps(&f, Some("runs"));
    assert_eq!(code, 0, "{err}");
    let r = report(&out);
    assert_eq!(strings(&r, "capabilities"), ["exec", "net"]);
    // `Exec` is its own effect for the runtime gate even though the row says
    // `IO`; `ai_complete` is `AI` + `Net`.
    assert_eq!(strings(&r, "effects"), ["AI", "Exec", "IO", "Net"]);
}

#[test]
fn the_whole_program_is_the_union() {
    let d = tmp("whole");
    let f = write(&d, "p.ax", PROGRAM);
    let (code, out, err) = caps(&f, None);
    assert_eq!(code, 0, "{err}");
    let r = report(&out);
    assert_eq!(r["fn"], serde_json::Value::Null);
    assert_eq!(
        strings(&r, "capabilities"),
        ["exec", "fs:read", "fs:write", "net"]
    );
}

#[test]
fn stdin_mode_reads_the_program_text() {
    let mut child = Command::new(axon())
        .args(["caps", "-", "--fn", "via_helper", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write as _;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(PROGRAM.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let r = report(&stdout);
    assert_eq!(r["file"], "-");
    assert_eq!(strings(&r, "capabilities"), ["fs:write"]);
}

#[test]
fn a_parse_error_is_exit_2_with_the_diagnostic() {
    let d = tmp("parse");
    let f = write(&d, "p.ax", "fn main( {\n");
    let (code, out, err) = caps(&f, None);
    assert_eq!(code, 2, "stdout: {out}");
    assert!(err.contains("E0000"), "{err}");
    assert!(out.is_empty(), "{out}");
}

#[test]
fn an_unknown_fn_is_exit_2() {
    let d = tmp("nofn");
    let f = write(&d, "p.ax", PROGRAM);
    let (code, out, err) = caps(&f, Some("nope"));
    assert_eq!(code, 2, "stdout: {out}");
    assert!(err.contains("no fn named nope"), "{err}");
}

/// Imports count: a fn calling a helper from a `use`d module reaches what the
/// helper reaches, with the module found through `--base-dir`-relative
/// `AXON_PATH`.
#[test]
fn an_imported_helper_counts() {
    let d = tmp("import");
    std::fs::create_dir_all(d.join("lib")).unwrap();
    write(
        &d.join("lib"),
        "sink.ax",
        "fn sink_it(p: str) -> i64 {\n    let _ = append_file(p, \"x\")\n    0\n}\n",
    );
    let src = "mod sink\nuse sink.{sink_it}\n\nfn target(n: i64) -> i64 {\n    sink_it(\"/tmp/axon_caps_imp\")\n}\n";
    let out = Command::new(axon())
        .args(["caps", "-", "--fn", "target", "--json", "--base-dir"])
        .arg(&d)
        .env("AXON_PATH", "lib")
        .env("AXON_PATH_EXCLUSIVE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write as _;
            c.stdin.take().unwrap().write_all(src.as_bytes())?;
            c.wait_with_output()
        })
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let r = report(&String::from_utf8_lossy(&out.stdout));
    assert_eq!(strings(&r, "capabilities"), ["fs:write"]);
}

/// A callee chosen at runtime cannot be classified: `unknown` is set so a
/// consumer denies by default.
#[test]
fn a_dynamic_dispatch_target_is_unknown() {
    let d = tmp("dyn");
    let src = r#"
fn job(n: i64) -> i64 { n }
fn target(n: i64) -> i64 {
    let b = "b"
    let name = "jo{b}"
    scheduler_spawn(name, n)
    scheduler_run()
    n
}
"#;
    let f = write(&d, "p.ax", src);
    let (code, out, err) = caps(&f, Some("target"));
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        report(&out)["unknown"],
        true,
        "ATTACK: a computed dispatch target was reported as fully known: {out}"
    );
}

// ── agreement with `@[contained]` ───────────────────────────────────────────

fn within(spec: &axon_core::ast::ContainedSpec, cap: &str) -> bool {
    match cap {
        "fs:read" => !spec.fs_read.is_empty(),
        "fs:write" => !spec.fs_write.is_empty(),
        "net" => !spec.net_allow.is_empty(),
        "exec" => spec.exec_allowed,
        c => match c.strip_prefix("native:") {
            Some(m) => spec.native_grants.iter().any(|g| g == m),
            // `env` and `dstore` have no clause that can grant them.
            None => false,
        },
    }
}

fn contained_fns(
    program: &axon_core::ast::Program,
) -> Vec<(String, axon_core::ast::ContainedSpec)> {
    use axon_core::ast::Item;
    let mut out = Vec::new();
    for item in &program.items {
        if let Item::FnDef(f) = item {
            if let Some(s) = &f.contained {
                out.push((f.name.clone(), s.clone()));
            }
        }
    }
    out
}

fn examples(dir: &Path, acc: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            examples(&p, acc);
        } else if p.extension().is_some_and(|x| x == "ax") {
            acc.push(p);
        }
    }
}

/// For every `examples/**/*.ax` fn carrying `@[contained]` that `axon check`
/// accepts, `axon caps --fn` reports nothing outside its grant. A capability
/// outside the grant would mean the fence and the language disagree about
/// what the fn reaches.
#[test]
fn caps_agrees_with_contained_on_every_example() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files = Vec::new();
    examples(&root, &mut files);
    files.sort();
    let mut checked = 0;
    for file in files {
        let src = std::fs::read_to_string(&file).unwrap();
        if !src.contains("@[contained") {
            continue;
        }
        let Ok(program) = axon_core::parse_source(&src) else {
            continue;
        };
        let fns = contained_fns(&program);
        if fns.is_empty() {
            continue;
        }
        let ok = Command::new(axon())
            .arg("check")
            .arg(&file)
            .output()
            .unwrap()
            .status
            .success();
        if !ok {
            continue;
        }
        for (name, spec) in fns {
            let (code, out, err) = caps(&file, Some(&name));
            assert_eq!(code, 0, "{}: {name}: {err}", file.display());
            let r = report(&out);
            let outside: Vec<String> = strings(&r, "capabilities")
                .into_iter()
                .filter(|c| !within(&spec, c))
                .collect();
            assert!(
                outside.is_empty(),
                "{}: `{name}` passes `axon check` but reaches {outside:?} outside its grant",
                file.display()
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 10,
        "only {checked} contained fns were compared — the example set moved"
    );
}

/// The other side: a seeded violation is E1001 to `axon check` AND reported
/// outside the grant by `axon caps` (which does not refuse it — reporting it is
/// its job).
#[test]
fn a_seeded_violation_is_e1001_and_reported() {
    let d = tmp("seeded");
    let src = r#"
fn helper(p: str) -> i64 {
    let _ = append_file(p, "x")
    0
}

@[contained(fs: [read("./data/")], net: [], exec: none)]
fn reader(n: i64) -> i64 {
    let _ = read_file("./data/in.txt")
    helper("./data/out.txt")
}

fn main() { let _ = reader(1) }
"#;
    let f = write(&d, "seeded.ax", src);
    let out = Command::new(axon()).arg("check").arg(&f).output().unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(text.contains("E1001"), "{text}");

    let program = axon_core::parse_source(src).unwrap();
    let (_, spec) = contained_fns(&program).pop().unwrap();
    let (code, stdout, err) = caps(&f, Some("reader"));
    assert_eq!(code, 0, "{err}");
    let outside: Vec<String> = strings(&report(&stdout), "capabilities")
        .into_iter()
        .filter(|c| !within(&spec, c))
        .collect();
    assert_eq!(outside, ["fs:write"]);
}

// ── User code the interpreter runs WITHOUT a call to it in the fn's body ────
//
// Each route below is a place the interpreter invokes user code implicitly:
// a refinement predicate at a `let`/param/return/field coercion, a struct's
// whole-type refinement at construction, an `@[verify]` predicate at return,
// and the metric an `@[goal]` fn's call optimises. Every test first RUNS the
// program and checks the helper's file really was written (so the route is
// live, not hypothetical), then asks `axon caps --fn double`. Red before the
// fix: every one reported `"capabilities":[],"effects":[]` (Main's review,
// route c6 first), so a fence built from a pristine `double` that only
// reached `append_file` through a predicate denied nothing it needed, and a
// candidate that ADDED such a predicate passed the static excess check.
// Since checker-soundness-2 CS2-8 a refinement predicate may not reach an
// effect through a user fn, so the refinement routes are refused at check
// (E1209) and their reach is pinned through the library walker instead.

/// The helper every route reaches, and a predicate that calls it.
const LEAK: &str = r#"
fn w(x: i64) -> i64 {
    let _ = append_file("MARK", "x")
    x
}
fn leak(v: i64) -> bool {
    w(v) == v
}
"#;

const RUN_DOUBLE: &str = "\nfn main() {\n    println(to_str(double(3)))\n}\n";

/// Run `LEAK + body + main`, assert the route really writes, then return
/// `axon caps --fn double`'s report.
fn implicit_route(tag: &str, body: &str) -> serde_json::Value {
    let d = tmp(tag);
    let mark = d.join("written.W");
    let src = format!(
        "{}{body}{RUN_DOUBLE}",
        LEAK.replace("MARK", &mark.display().to_string())
    );
    let f = write(&d, "p.ax", &src);
    let run = Command::new(axon())
        .arg("run")
        .arg(&f)
        .env("XDG_CACHE_HOME", d.join("cache"))
        .output()
        .unwrap();
    assert_eq!(
        run.status.code(),
        Some(0),
        "{tag}: the route must run: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        mark.exists(),
        "{tag}: the interpreter did not run the helper — the route is not live"
    );
    let (code, out, err) = caps(&f, Some("double"));
    assert_eq!(code, 0, "{tag}: {err}");
    report(&out)
}

fn assert_reaches_the_write(tag: &str, r: &serde_json::Value) {
    assert_eq!(
        strings(r, "capabilities"),
        ["fs:write"],
        "ATTACK: {tag}: a write the interpreter runs implicitly is missing from caps: {r}"
    );
    assert_eq!(strings(r, "effects"), ["IO"], "{tag}: {r}");
}

/// checker-soundness-2 CS2-8: a refinement predicate may not reach an effect
/// through a user fn, so every refinement route through `leak` is refused at
/// check (E1209) and `axon run` writes nothing. The walker's reach through
/// the route stays pinned by the library: `reachable_capabilities` of
/// `double` (the same form as
/// `a_predicate_that_mentions_its_own_type_terminates_and_is_reached`).
fn refused_refinement_route(tag: &str, body: &str) {
    let d = tmp(tag);
    let mark = d.join("written.W");
    let src = format!(
        "{}{body}{RUN_DOUBLE}",
        LEAK.replace("MARK", &mark.display().to_string())
    );
    let f = write(&d, "p.ax", &src);
    let check = Command::new(axon()).arg("check").arg(&f).output().unwrap();
    let text = String::from_utf8_lossy(&check.stderr);
    assert!(
        check.status.code() == Some(2) && text.contains("E1209"),
        "ATTACK: {tag}: a refinement predicate reaching a write through a helper passed check \
         (rc {:?}): {text}",
        check.status.code()
    );
    let run = Command::new(axon())
        .arg("run")
        .arg(&f)
        .env("XDG_CACHE_HOME", d.join("cache"))
        .output()
        .unwrap();
    assert!(
        !mark.exists(),
        "ATTACK: {tag}: `axon run` ran a writing refinement predicate (rc {:?})",
        run.status.code()
    );
    let program = axon_core::parse_source(&src).unwrap();
    let r = axon_core::capabilities::reachable_capabilities(&program, Some("double")).unwrap();
    assert_eq!(
        r.capabilities.into_iter().collect::<Vec<_>>(),
        ["fs:write"],
        "ATTACK: {tag}: a write the interpreter runs implicitly is missing from the reach"
    );
    assert_eq!(r.effects.into_iter().collect::<Vec<_>>(), ["IO"], "{tag}");
    assert!(!r.unknown, "{tag}");
}

#[test]
fn a_refinement_predicate_at_a_let_annotation_is_reached() {
    refused_refinement_route(
        "refine_let",
        "type Lk = i64 where leak(_)\nfn double(n: i64) -> i64 {\n    let y: Lk = n\n    y * 2\n}\n",
    );
}

#[test]
fn a_refinement_predicate_at_a_helpers_parameter_is_reached() {
    refused_refinement_route(
        "refine_param",
        "type Lk = i64 where leak(_)\nfn twice(y: Lk) -> i64 {\n    y * 2\n}\n\
         fn double(n: i64) -> i64 {\n    twice(n)\n}\n",
    );
}

#[test]
fn a_refinement_predicate_on_the_return_type_is_reached() {
    refused_refinement_route(
        "refine_ret",
        "type Lk = i64 where leak(_)\nfn double(n: i64) -> Lk {\n    n * 2\n}\n",
    );
}

#[test]
fn an_inline_parameter_refinement_is_reached() {
    refused_refinement_route(
        "refine_inline",
        "fn double(n: i64 where leak(n)) -> i64 {\n    n * 2\n}\n",
    );
}

#[test]
fn a_refined_struct_field_is_checked_at_construction_and_reached() {
    refused_refinement_route(
        "refine_field",
        "type Lk = i64 where leak(_)\ntype Box = { v: Lk }\n\
         fn double(n: i64) -> i64 {\n    let b = Box { v: n }\n    b.v * 2\n}\n",
    );
}

/// checker-soundness-2 CS2-7: a variant field's refinement runs at the
/// `E::V { .. }` literal (it used to be skipped, so the route was dead), and
/// caps follows the enum's variant field types from the literal. CS2-8: the
/// predicate reaches a write through `leak`, so the route is refused at check.
#[test]
fn a_refined_variant_field_is_checked_at_construction_and_reached() {
    refused_refinement_route(
        "refine_variant",
        "type Lk = i64 where leak(_)\ntype E = A { v: Lk } | B\n\
         fn double(n: i64) -> i64 {\n    let e = E::A { v: n }\n    \
         match e {\n        E::A { v } => v * 2,\n        E::B => 0,\n    }\n}\n",
    );
}

/// checker-soundness-2 CS2-7: a lambda parameter's refinement runs when the
/// closure is called (it used to be skipped), and caps follows it. CS2-8: the
/// predicate reaches a write through `leak`, so the route is refused at check.
#[test]
fn a_refined_lambda_param_is_checked_at_call_and_reached() {
    refused_refinement_route(
        "refine_lambda",
        "type Lk = i64 where leak(_)\n\
         fn double(n: i64) -> i64 {\n    let f = |y: Lk| y * 2\n    f(n)\n}\n",
    );
}

#[test]
fn a_whole_struct_refinement_is_reached() {
    refused_refinement_route(
        "refine_struct",
        "type Pair = { lo: i64, hi: i64 } where leak(_.lo)\n\
         fn double(n: i64) -> i64 {\n    let p = Pair { lo: n, hi: n }\n    p.lo * 2\n}\n",
    );
}

/// CAP-1 (Q1 a2): a postcondition may only read the clock, so the verify
/// route no longer runs: `axon check` refuses it with E1104, `axon run`
/// writes nothing, and `axon caps` (which reports only a program that
/// checks) refuses it with the same E1104. Before the fix this route checked
/// clean, the run wrote the file, and caps reported `fs:write`.
#[test]
fn a_verify_predicate_route_is_refused_at_check() {
    let d = tmp("verify");
    let mark = d.join("written.W");
    let src = format!(
        "{}@[verify(leak(value))]\nfn double(n: i64) -> i64 {{\n    n * 2\n}}\n{RUN_DOUBLE}",
        LEAK.replace("MARK", &mark.display().to_string())
    );
    let f = write(&d, "p.ax", &src);
    let check = Command::new(axon()).arg("check").arg(&f).output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(
        check.status.code() == Some(2) && text.contains("E1104"),
        "ATTACK: verify: a verify predicate reaching a write passed check: {text}"
    );
    let run = Command::new(axon())
        .arg("run")
        .arg(&f)
        .env("XDG_CACHE_HOME", d.join("cache"))
        .output()
        .unwrap();
    assert!(
        !mark.exists(),
        "ATTACK: verify: `axon run` ran a writing verify predicate (rc {:?})",
        run.status.code()
    );
    let (code, _, err) = caps(&f, Some("double"));
    assert!(
        code == 2 && err.contains("E1104"),
        "verify: caps reported a program check refuses (rc {code}): {err}"
    );
}

#[test]
fn the_metric_of_a_goal_fn_is_reached() {
    let r = implicit_route(
        "goal",
        "@[adaptive]\nfn score(x: i64) -> i64 {\n    let _ = w(x)\n    x\n}\n\
         @[goal(metric: score, target: 5.0, max_evals: 2)]\nfn tune() -> i64 {\n    0\n}\n\
         fn double(n: i64) -> i64 {\n    let _ = tune()\n    n * 2\n}\n",
    );
    assert_reaches_the_write("goal", &r);
}

/// The walker is `@[contained]`'s own, so the same predicate is a violation
/// to `axon check`: before the fix it passed with exit 0 and the run wrote
/// the file.
#[test]
fn contained_refuses_a_write_reached_through_a_refinement_predicate() {
    let d = tmp("contained_refine");
    let src = format!(
        "{}type Lk = i64 where leak(_)\n\
         @[contained(fs: [], net: [], exec: none)]\n\
         fn double(n: i64) -> i64 {{\n    let y: Lk = n\n    y * 2\n}}\n{RUN_DOUBLE}",
        LEAK.replace("MARK", &d.join("w.W").display().to_string())
    );
    let f = write(&d, "p.ax", &src);
    let out = Command::new(axon()).arg("check").arg(&f).output().unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(text.contains("E1001"), "{text}");
}

/// A predicate whose helper annotates a binding with the predicate's own type
/// is a cycle the walk must cut (each type is walked once per path), and the
/// write behind it is still reported. Walker-level: the program never runs.
#[test]
fn a_predicate_that_mentions_its_own_type_terminates_and_is_reached() {
    let src = r#"
fn leak(v: i64) -> bool {
    let again: Lk = v
    let _ = append_file("/nonexistent/x", "x")
    again == v
}
type Lk = i64 where leak(_)
fn double(n: i64) -> i64 {
    let y: Lk = n
    y * 2
}
"#;
    let program = axon_core::parse_source(src).unwrap();
    let r = axon_core::capabilities::reachable_capabilities(&program, Some("double")).unwrap();
    assert_eq!(r.capabilities.into_iter().collect::<Vec<_>>(), ["fs:write"]);
    assert!(!r.unknown);
}
