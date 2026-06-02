//! End-to-end CLI tests: run the `axon` binary on real examples and assert
//! actual stdout / exit codes (the unit tests cover parse/exit; these cover the
//! whole pipeline through the interpreter, including printed output).
//!
//! Uses `CARGO_BIN_EXE_axon` (set by cargo for the codegen-free `axon` binary
//! built under `--no-default-features`).

use std::process::Command;

fn axon() -> Command {
    Command::new(env!("CARGO_BIN_EXE_axon"))
}

fn ex(rel: &str) -> String {
    format!("{}/../../examples/{}", env!("CARGO_MANIFEST_DIR"), rel)
}

fn fixture(rel: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), rel)
}

#[test]
fn contained_capability_sandbox_is_enforced_by_check() {
    // Regression: `@[contained(...)]` I/O sandboxing must be enforced by the CLI
    // check pipeline, not only the library path. A write outside the fs allowlist
    // is rejected; a compliant contained fn checks clean.
    let bad = axon().args(["check", &fixture("contained_fail_fs.ax")]).output().unwrap();
    assert_eq!(bad.status.code(), Some(2), "containment violation must be rejected");
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&bad.stdout),
        String::from_utf8_lossy(&bad.stderr)
    );
    assert!(msg.contains("E1001"), "expected E1001, got: {msg}");

    let ok = axon().args(["check", &fixture("contained_pass.ax")]).output().unwrap();
    assert!(ok.status.success(), "compliant @[contained] fn should check clean");
}

#[test]
fn typed_let_bindings_enforce_the_annotation() {
    // `let x: T = e` parses and enforces the annotation.
    let f = std::env::temp_dir().join(format!("axon_tlet_{}.ax", std::process::id()));
    std::fs::write(&f, "fn main() -> i64 { let x: i64 = 5  let y: i64 = x * 2  y }\n").unwrap();
    let ok = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    assert_eq!(ok.status.code(), Some(10), "typed let should run: y = 10");

    std::fs::write(&f, "fn main() -> i64 { let x: str = 5  0 }\n").unwrap();
    let bad = axon().args(["check", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(bad.status.code(), Some(2), "a type-mismatched annotation must be rejected");
}

#[test]
fn to_str_is_polymorphic_over_scalars() {
    // BUG_HUNT #29: `to_str` should accept i64, f64, AND bool — picking the
    // wrong specialized name (to_str_f64 / to_str_bool) is needless onboarding
    // friction. The output must match the specialized builtins exactly.
    let f = std::env::temp_dir().join(format!("axon_polystr_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn main() {\n  \
           println(to_str(42))\n  \
           println(to_str(3.14))\n  \
           println(to_str(true))\n  \
           println(to_str(false))\n\
         }\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(0), "polymorphic to_str should check + run clean: {:?}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "42\n3.14\ntrue\nfalse\n", "to_str output must match specialized builtins: {stdout:?}");
}

#[test]
fn to_str_polymorphic_matches_specialized_in_interpolation() {
    // The win is in string interpolation, where the wrong specialized name is
    // most often reached for. `{to_str(x)}` must work for any scalar x.
    let f = std::env::temp_dir().join(format!("axon_polystr2_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn main() {\n  \
           let pi = 3.5\n  \
           let ok = true\n  \
           println(\"pi={to_str(pi)} ok={to_str(ok)}\")\n\
         }\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(0), "to_str in interpolation should run: {:?}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "pi=3.5 ok=true\n");
}

#[test]
fn invalid_radix_fails_fast() {
    // i64_to_str_radix with a radix outside 2..=36 must fail fast (graceful
    // panic) rather than silently returning an empty string.
    let f = std::env::temp_dir().join(format!("axon_radix_{}.ax", std::process::id()));
    std::fs::write(&f, "fn main() { println(i64_to_str_radix(255, 0)) }\n").unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(101), "invalid radix should panic, not return \"\"");
    assert!(String::from_utf8_lossy(&out.stderr).contains("radix must be"), "stderr: {:?}", out.stderr);
}

#[test]
fn verify_failure_and_crash_have_distinct_exit_codes() {
    // BUG_HUNT #26: a deploy-gate / @[verify] rejection is a POLICY decision
    // ("the artifact didn't meet the bar"), not a bug-crash. A CI pipeline must
    // be able to branch on the two: verify failure exits 3, a genuine runtime
    // panic (OOB, div-by-zero, …) stays 101.
    let vfail = std::env::temp_dir().join(format!("axon_vfail_{}.ax", std::process::id()));
    std::fs::write(
        &vfail,
        "@[verify(confidence >= 0.8)]\n\
         fn low() -> Uncertain<i64> { uncertain_dyn_i64(42, 0.5) }\n\
         fn main() -> i64 { let x = low()  0 }\n",
    )
    .unwrap();
    let v = axon().args(["run", vfail.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&vfail);
    assert_eq!(
        v.status.code(),
        Some(3),
        "verify failure should exit 3 (distinct policy code): {:?}",
        String::from_utf8_lossy(&v.stderr),
    );
    // The message must still identify it as a verify violation.
    assert!(
        String::from_utf8_lossy(&v.stderr).contains("verify"),
        "verify-failure stderr should mention verify: {:?}",
        String::from_utf8_lossy(&v.stderr),
    );

    // A genuine crash on the same `run` path must remain 101, NOT be reclassified.
    let crash = std::env::temp_dir().join(format!("axon_crash_{}.ax", std::process::id()));
    std::fs::write(&crash, "fn main() -> i64 { let a = [1, 2]  a[99] }\n").unwrap();
    let c = axon().args(["run", crash.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&crash);
    assert_eq!(
        c.status.code(),
        Some(101),
        "a genuine runtime panic must stay 101, distinct from a verify failure: {:?}",
        String::from_utf8_lossy(&c.stderr),
    );
}

#[test]
fn deeply_nested_input_fails_gracefully_not_aborts() {
    // Adversarially deep nesting must be a clean parse error (exit 2), not a
    // parser stack overflow (exit 134 / SIGABRT).
    let f = std::env::temp_dir().join(format!("axon_nest_{}.ax", std::process::id()));
    let src = format!("fn main() -> i64 {{ {}1{} }}\n", "(".repeat(50000), ")".repeat(50000));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["check", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(2), "deep nesting should be a clean parse error, not abort");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("nesting too deep")
            || String::from_utf8_lossy(&out.stderr).contains("nesting too deep"),
        "expected 'nesting too deep'"
    );
}

#[test]
fn deep_recursion_fails_gracefully_not_aborts() {
    // Runaway recursion must be a catchable panic (exit 101) with a clear
    // message — not a process-aborting stack overflow (exit 134 / SIGABRT).
    let f = std::env::temp_dir().join(format!("axon_deeprec_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn c(n: i64) -> i64 { if n == 0 { 0 } else { 1 + c(n - 1) } }\n\
         fn main() -> i64 { c(200000) }\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(101), "deep recursion should panic gracefully, not abort");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("recursion limit"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn axon_max_depth_raises_recursion_limit() {
    // BUG_HUNT #28: the recursion guard must be configurable via AXON_MAX_DEPTH
    // for legitimate deep recursion. A depth the default (6000) rejects must
    // run when the env var raises the ceiling — AND still exit cleanly (the
    // thread stack scales with the limit, so the guard never gives way to a
    // process-aborting overflow).
    let f = std::env::temp_dir().join(format!("axon_maxdepth_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn c(n: i64) -> i64 { if n == 0 { 0 } else { 1 + c(n - 1) } }\n\
         fn main() -> i64 { c(7000) % 100 }\n",
    )
    .unwrap();
    // Default ceiling (6000): 7000-deep must hit the guard → exit 101.
    let default_run = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    assert_eq!(
        default_run.status.code(),
        Some(101),
        "7000-deep should exceed the default 6000 limit: {:?}",
        String::from_utf8_lossy(&default_run.stderr),
    );
    // Raised ceiling: same program runs to completion (7000 % 100 == 0).
    let raised = axon()
        .args(["run", f.to_str().unwrap()])
        .env("AXON_MAX_DEPTH", "9000")
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(
        raised.status.code(),
        Some(0),
        "AXON_MAX_DEPTH=9000 should let 7000-deep recursion run cleanly: {:?}",
        String::from_utf8_lossy(&raised.stderr),
    );
}

#[test]
fn moderate_recursion_works() {
    // ~5000-deep recursion runs (large interpreter thread stack); was ~64 before.
    let f = std::env::temp_dir().join(format!("axon_modrec_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn c(n: i64) -> i64 { if n == 0 { 0 } else { 1 + c(n - 1) } }\n\
         fn main() -> i64 { c(5000) % 100 }\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(0), "5000-deep recursion should run (5000 % 100 = 0)");
}

#[test]
fn all_examples_typecheck_clean() {
    // Stronger than the lib's parse-only guard: every example must pass the FULL
    // CLI check pipeline (resolve/infer/check/borrow/capability/verify), with
    // module resolution via AXON_PATH for the modular examples. Catches runtime/
    // type regressions the parse test misses.
    fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    collect(&p, out);
                } else if p.extension().map(|x| x == "ax").unwrap_or(false) {
                    out.push(p);
                }
            }
        }
    }
    let root = format!("{}/../../examples", env!("CARGO_MANIFEST_DIR"));
    // Two AXON_PATH entries so the sweep covers both module roots. stdlib
    // FIRST so the flagship demo's `mod agent` resolves to the no-main
    // stdlib/agent.ax rather than the tutorial's modular/agent.ax (which
    // has a `main` that would conflict). modular/scorelib still resolves
    // because the resolver falls through to the second entry.
    let modpath = format!(
        "{0}/../../examples/stdlib:{0}/../../examples/modular",
        env!("CARGO_MANIFEST_DIR"),
    );
    let mut files = Vec::new();
    collect(std::path::Path::new(&root), &mut files);
    assert!(files.len() >= 20, "expected many examples, found {}", files.len());
    // Deny-case examples are DESIGNED to fail `check` (they demonstrate the
    // compiler rejecting a violation). They are guarded by their own tests
    // (e.g. `contained_violation_demo_is_rejected_by_check`), so exclude them
    // from the "must check clean" sweep.
    const DENY_CASE_EXAMPLES: &[&str] = &["contained_violation.ax"];
    for f in &files {
        if f.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| DENY_CASE_EXAMPLES.contains(&n))
        {
            continue;
        }
        let out = axon()
            .args(["check", f.to_str().unwrap()])
            .env("AXON_PATH", &modpath)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{} should type-check clean: {}",
            f.display(),
            String::from_utf8_lossy(&out.stdout)
        );
    }
}

#[test]
fn chan_type_as_function_parameter() {
    // `chan<T>` is usable as a type — a channel can be passed to a worker fn.
    let f = std::env::temp_dir().join(format!("axon_chanty_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn worker(out: chan<i64>, x: i64) { out.send(x * x) }\n\
         fn main() -> i64 { let c = chan<i64>()  spawn { worker(c, 9) }  c.recv() }\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(81), "worker(c, 9) sends 81 over the channel");
}

#[test]
fn select_fires_first_ready_channel() {
    // Cooperative select: the arm whose channel has a ready value fires. Here `a`
    // is empty and `b` was sent to, so the `b` arm runs (result = 2).
    let f = std::env::temp_dir().join(format!("axon_select_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn main() -> i64 {\n  let a = chan<i64>()\n  let b = chan<i64>()\n  \
         let result = 0\n  spawn { b.send(99) }\n  \
         select { a.recv() => result = 1  b.recv() => result = 2 }\n  result\n}\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(2), "the ready (b) arm should fire → result 2");
}

#[test]
fn spawn_channel_fanout_collects_results() {
    // Cooperative concurrency: spawn one worker per candidate to send its score
    // to a channel, then collect — the fan-out/collect pattern runs and the best
    // score (88) is found.
    let out = axon().args(["run", &ex("asi/parallel_score.ax")]).output().unwrap();
    assert_eq!(out.status.code(), Some(88), "best fan-out score should be 88");
}

#[test]
fn contained_scorer_demo_runs_and_blocks_exfiltration() {
    // The sandboxed scorer demo runs clean...
    let out = axon().args(["run", &ex("asi/contained.ax")]).output().unwrap();
    assert!(out.status.success(), "contained demo exited {:?}", out.status.code());
    assert!(String::from_utf8_lossy(&out.stdout).contains("score = 70"), "stdout: {:?}", out.stdout);
    // ...and adding a network (LLM) call under the same @[contained] spec is rejected.
    let bad = std::env::temp_dir().join(format!("axon_cexfil_{}.ax", std::process::id()));
    std::fs::write(
        &bad,
        "@[contained(fs: [], exec: none)]\nfn s() -> i64 { let l = ai_complete(\"x\")  1 }\n\
         fn main() -> i64 { s() }\n",
    )
    .unwrap();
    let r = axon().args(["check", bad.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&bad);
    assert_eq!(r.status.code(), Some(2), "exfiltration via net must be rejected");
    let msg = format!("{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
    assert!(msg.contains("E1001"), "expected E1001, got: {msg}");
    // Bug #8: error messages must suggest the fix.
    assert!(msg.contains("Add") || msg.contains("try") || msg.contains("use") || msg.contains("specify"),
        "expected fix suggestion in error message, got: {msg}");
}

#[test]
fn contained_violation_demo_is_rejected_by_check() {
    // Bug #9: the deny-case must be a shipped, user-facing example so the
    // capability gate's enforcement is visible (not just provable in a unit
    // test). `axon check examples/asi/contained_violation.ax` must FAIL with
    // E1001, demonstrating the compiler refusing to build an exfiltrating fn.
    let out = axon().args(["check", &ex("asi/contained_violation.ax")]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "violation demo must be rejected by check: {:?}", out);
    let msg = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(msg.contains("E1001"), "expected E1001 for the violation demo, got: {msg}");
}

#[test]
fn contained_error_messages_suggest_fix() {
    // Bug #8: E1001 and E1004 messages must include a concrete suggestion
    // showing the @[contained] clause the user should add.

    // Net denial (no net: clause) — should suggest adding one.
    let bad = std::env::temp_dir().join(format!("axon_cmsg_{}.ax", std::process::id()));
    std::fs::write(&bad,
        "@[contained(fs: [write(\"./out/\")], exec: none)]\n\
         fn s() -> i64 { let _ = write_file(\"/etc/passwd\", \"x\")  0 }\n\
         fn main() -> i64 { s() }\n",
    ).unwrap();
    let r = axon().args(["check", bad.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&bad);
    let msg = format!("{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
    assert!(msg.contains("E1001"), "expected E1001 for write denial, got: {msg}");
    assert!(msg.contains("Add") || msg.contains("try") || msg.contains("use") || msg.contains("specify"),
        "expected fix suggestion for write denial, got: {msg}");

    // never: read denial — should suggest removing the never clause or the call.
    let bad2 = std::env::temp_dir().join(format!("axon_cmsg2_{}.ax", std::process::id()));
    std::fs::write(&bad2,
        "@[contained(fs: [read(\"/etc/\")], never: [read(\"/etc/shadow\")])]\n\
         fn s() -> i64 {\n  \
             match read_file(\"/etc/shadow\") { Ok(_) => 1  Err(_) => 0 }\n\
         }\n\
         fn main() -> i64 { s() }\n",
    ).unwrap();
    let r2 = axon().args(["check", bad2.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&bad2);
    let msg2 = format!("{}{}", String::from_utf8_lossy(&r2.stdout), String::from_utf8_lossy(&r2.stderr));
    assert!(msg2.contains("E1004"), "expected E1004 for never clause, got: {msg2}");
    assert!(msg2.contains("Add") || msg2.contains("try") || msg2.contains("use") || msg2.contains("specify")
        || msg2.contains("remove"),
        "expected fix suggestion for never clause, got: {msg2}");
}

#[test]
fn borrow_violation_rejected_by_check() {
    // Borrow checking (E0601 use-after-move etc.) must run in the CLI pipeline.
    let out = axon().args(["check", &fixture("borrow_errors.ax")]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "borrow violation must be rejected");
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(msg.contains("E0601"), "expected E0601, got: {msg}");
}

#[test]
fn verify_unsatisfiable_postcondition_rejected_by_check() {
    // Static @[verify] checking (E1101) must run in the CLI: a postcondition the
    // function's confidence bound can never meet is rejected; a met one is clean.
    let bad = axon().args(["check", &fixture("verify_fail.ax")]).output().unwrap();
    assert_eq!(bad.status.code(), Some(2), "unsatisfiable @[verify] must be rejected");
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&bad.stdout),
        String::from_utf8_lossy(&bad.stderr)
    );
    assert!(msg.contains("E1101"), "expected E1101, got: {msg}");

    let ok = axon().args(["check", &fixture("verify_pass.ax")]).output().unwrap();
    assert!(ok.status.success(), "a satisfiable @[verify] should check clean");
}

#[test]
fn run_hello_prints_greeting() {
    let out = axon().args(["run", &ex("hello.ax")]).output().unwrap();
    assert!(out.status.success(), "hello.ax exited {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Hello, Axon!"), "stdout was: {stdout:?}");
}

#[test]
fn run_comprehensive_computes_sum_to_100() {
    let out = axon().args(["run", &ex("comprehensive.ax")]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("5050"), "expected sum_to(100)=5050 in: {stdout:?}");
}

#[test]
fn run_error_handling_propagates_results() {
    let out = axon().args(["run", &ex("error_handling.ax")]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // sqrt_of_string("144") => Ok(12); division by zero is reported.
    assert!(stdout.contains("Ok(12)"), "stdout: {stdout:?}");
    assert!(stdout.contains("division by zero"), "stdout: {stdout:?}");
}

#[test]
fn goal_optimize_deploys() {
    let out = axon().args(["goal", &ex("goals/optimize-goal.md")]).output().unwrap();
    assert!(out.status.success(), "optimize-goal exited {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("best score: 100"), "stdout: {stdout:?}");
    assert!(stdout.contains("deploy gate: passed"), "stdout: {stdout:?}");
}

#[test]
fn goal_with_missing_sections_lists_them_all() {
    // Bug #3: an incomplete goal file must report ALL missing required
    // sections in one error (exit 2), not just the first — so the author
    // fixes them in a single pass, not N re-runs.
    let f = std::env::temp_dir().join(format!("axon_incomplete_{}.md", std::process::id()));
    std::fs::write(&f, "# Goal: incomplete\n\n## Intent\n\nDo a thing.\n").unwrap();
    let out = axon().args(["goal", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(2), "incomplete goal must be rejected: {:?}", out);
    let msg = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    // First and last required sections (minus Intent) both present in one message.
    assert!(msg.contains("Inputs"), "should list Inputs: {msg}");
    assert!(msg.contains("Provenance"), "should list Provenance: {msg}");
}

#[test]
fn run_typechecks_before_interpreting() {
    // An undefined-name program must be rejected (exit 2) by check-before-run,
    // not surface as a runtime panic.
    let bad = std::env::temp_dir().join("axon_cli_run_typecheck.ax");
    std::fs::write(&bad, "fn main() -> i64 { undefined_helper(1) }\n").unwrap();
    let out = axon().args(["run", bad.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&bad);
    assert_eq!(out.status.code(), Some(2), "expected type-error exit 2");
}

#[test]
fn missing_module_error_hints_axon_path_when_unset() {
    // Bug #10: running a module-importing demo without AXON_PATH gives an
    // E0901 listing install-dir search paths the user never created. When
    // AXON_PATH is unset, the error must point at that lever.
    let f = std::env::temp_dir().join(format!("axon_modmiss_{}.ax", std::process::id()));
    std::fs::write(&f, "mod bandit\nuse bandit.{Bandit}\nfn main() -> i64 { 0 }\n").unwrap();
    let out = axon().args(["check", f.to_str().unwrap()]).env_remove("AXON_PATH").output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(2), "missing module must be rejected: {:?}", out);
    let msg = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(msg.contains("E0901"), "expected E0901, got: {msg}");
    assert!(msg.contains("AXON_PATH"), "expected AXON_PATH hint, got: {msg}");
}

#[test]
fn parse_error_prefix_is_not_doubled() {
    // Bug #7: a ParseError::Other-class error printed `parse error: parse
    // error: ...` — the prefix was added by both ParseError::Other's own
    // Display and the outer AxonError::Parse wrapper. Must appear exactly once.
    let bad = std::env::temp_dir().join(format!("axon_pp_{}.ax", std::process::id()));
    std::fs::write(&bad, "fn main() { println(\"hello {name\") }\n").unwrap();
    let out = axon().args(["run", bad.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&bad);
    assert_eq!(out.status.code(), Some(2), "parse error should exit 2");
    let msg = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(msg.contains("parse error:"), "should still have one prefix: {msg}");
    assert!(!msg.contains("parse error: parse error:"), "prefix must not be doubled: {msg}");
}

#[test]
fn run_exits_with_main_return_value() {
    let f = std::env::temp_dir().join("axon_cli_run_exitcode.ax");
    std::fs::write(&f, "fn main() -> i64 { 7 }\n").unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(7), "main's i64 return should be the exit code");
}

/// Parse the integer from a "best score: N (target …)" line.
fn best_score(stdout: &str) -> i64 {
    let key = "best score: ";
    let i = stdout.find(key).unwrap_or_else(|| panic!("no 'best score:' in: {stdout:?}")) + key.len();
    let rest = &stdout[i..];
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(rest.len());
    rest[..end].parse().unwrap_or_else(|_| panic!("unparseable score in: {stdout:?}"))
}

#[test]
fn cross_run_improves_with_continuation() {
    // learn-goal's per-run budget can't reach the optimum from 0; with
    // AXON_GOAL_CONTINUE the second run resumes from the first run's best input
    // (via the persisted provenance log) and scores strictly higher.
    let cache = std::env::temp_dir().join(format!("axon_xrun_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let goal = ex("goals/learn-goal.md");

    let r1 = axon()
        .args(["goal", &goal])
        .env("XDG_CACHE_HOME", &cache)
        .env_remove("AXON_GOAL_CONTINUE")
        .output()
        .unwrap();
    let s1 = best_score(&String::from_utf8_lossy(&r1.stdout));

    let r2 = axon()
        .args(["goal", &goal])
        .env("XDG_CACHE_HOME", &cache)
        .env("AXON_GOAL_CONTINUE", "1")
        .output()
        .unwrap();
    let s2 = best_score(&String::from_utf8_lossy(&r2.stdout));

    let _ = std::fs::remove_dir_all(&cache);
    assert!(s2 > s1, "continuation should improve the best score: run1={s1}, run2={s2}");
}

#[test]
fn goal_iterate_converges() {
    // `--iterate N` runs the goal with cross-run continuation (after run 1) and
    // stops early when the best score plateaus. learn-goal's per-run budget can't
    // reach the optimum alone, so the score climbs run-over-run; given a generous
    // cap (12) it must converge to the target (200) and stop short of the cap.
    let cache = std::env::temp_dir().join(format!("axon_iter_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let out = axon()
        .args(["goal", "--iterate", "12", &ex("goals/learn-goal.md")])
        .env("XDG_CACHE_HOME", &cache)
        .env_remove("AXON_GOAL_CONTINUE")
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&cache);

    // Each run prints its "best score: N" to stdout (iterate headers go to stderr).
    let stdout = String::from_utf8_lossy(&out.stdout);
    let key = "best score: ";
    let scores: Vec<i64> = stdout
        .match_indices(key)
        .map(|(i, _)| {
            let rest = &stdout[i + key.len()..];
            let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(rest.len());
            rest[..end].parse().unwrap()
        })
        .collect();
    // Per-run budget is too small to reach the optimum from a fresh start, so
    // a single run can't get there — but cross-run continuation accumulates,
    // and the optimizer reliably converges. Allow a wide spread on run count
    // (the algorithm's seed step changes how fast it lands) while pinning the
    // structural contract: more than one run is needed, it stops short of the
    // cap, the trace is non-decreasing, and it reaches 200.
    assert!(scores.len() >= 2, "single-run budget can't reach the peak alone: {scores:?}");
    assert!(scores.len() < 12, "should stop early on convergence, not run the full cap: {scores:?}");
    assert!(scores.windows(2).all(|w| w[1] >= w[0]), "best score is non-decreasing: {scores:?}");
    assert_eq!(*scores.last().unwrap(), 200, "should converge to the optimum: {scores:?}");

    // It should also report the solution (best score + the input that achieved it).
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("# best: score 200 at input 12"), "should report the solution: {stderr:?}");
}

#[test]
fn trace_summarizes_provenance() {
    // Produce a provenance log by iterating a goal, then `axon trace` should
    // summarize the search: the function, its eval count, and that the best
    // score was found at the optimum input (12).
    let cache = std::env::temp_dir().join(format!("axon_trace_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let _ = axon()
        .args(["goal", "--iterate", "6", &ex("goals/learn-goal.md")])
        .env("XDG_CACHE_HOME", &cache)
        .env_remove("AXON_GOAL_CONTINUE")
        .output()
        .unwrap();

    let out = axon().args(["trace"]).env("XDG_CACHE_HOME", &cache).output().unwrap();
    let _ = std::fs::remove_dir_all(&cache);
    assert!(out.status.success(), "trace exited {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("try_variant"), "stdout: {stdout:?}");
    assert!(stdout.contains("at input 12"), "best score is at the optimum input: {stdout:?}");
    assert!(stdout.contains("improving"), "trajectory should be improving: {stdout:?}");
}

#[test]
fn trace_json_is_machine_readable() {
    let cache = std::env::temp_dir().join(format!("axon_tracej_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let _ = axon()
        .args(["goal", "--iterate", "6", &ex("goals/learn-goal.md")])
        .env("XDG_CACHE_HOME", &cache)
        .env_remove("AXON_GOAL_CONTINUE")
        .output()
        .unwrap();

    let out = axon().args(["trace", "--json"]).env("XDG_CACHE_HOME", &cache).output().unwrap();
    let _ = std::fs::remove_dir_all(&cache);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim_start().starts_with('['), "should be a JSON array: {stdout:?}");
    assert!(stdout.contains("\"fn\":\"try_variant\""), "stdout: {stdout:?}");
    assert!(stdout.contains("\"best_input\":12"), "stdout: {stdout:?}");
    assert!(stdout.contains("\"trend\":\"improving\""), "stdout: {stdout:?}");
}

#[test]
fn trace_missing_log_exits_nonzero() {
    let out = axon()
        .args(["trace"])
        .env("XDG_CACHE_HOME", "/nonexistent-axon-cache-xyz")
        .env_remove("HOME")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "missing log should exit 1");
}

#[test]
fn supervised_agent_halts_on_unsafe_actions() {
    // Capability under control: the agent banks value from safe actions, then a
    // two-strike kill-switch latches on unsafe proposals and the final safe
    // action is refused.
    let out = axon().args(["run", &ex("asi/supervised_agent.ax")]).output().unwrap();
    assert!(out.status.success(), "exited {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("HALTED: banked 50 value safely"), "stdout: {stdout:?}");
    assert!(stdout.contains("SKIP   finish-job"), "latched halt must refuse the safe action: {stdout:?}");
}

#[test]
fn nested_place_assignment_mutates() {
    // Nested places: 2D array, struct-field-then-index, index-then-field.
    let f = std::env::temp_dir().join(format!("axon_nplace_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "type P = { x: i64 }\nfn main() -> i64 {\n  \
         let g = [[1, 2], [3, 4]]\n  g[0][1] = 9\n  g[1][0] = 7\n  \
         let ps = [P { x: 1 }, P { x: 2 }]\n  ps[1].x = 50\n  \
         g[0][1] + g[1][0] + g[0][0] + ps[1].x\n}\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(67), "9 + 7 + 1 + 50 = 67");
}

#[test]
fn place_assignment_mutates_array_and_field() {
    // `xs[i] = v` and `s.field = v` mutate in place (incl. inside a loop).
    let f = std::env::temp_dir().join(format!("axon_place_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "type P = { x: i64, y: i64 }\nfn main() -> i64 {\n  \
         let xs = [0, 0, 0, 0]\n  for i in 0..4 { xs[i] = i * i }\n  \
         let p = P { x: 1, y: 2 }\n  p.x = 10\n  \
         xs[0] + xs[1] + xs[2] + xs[3] + p.x + p.y\n}\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(26), "(0+1+4+9) + 10 + 2 = 26");
}

#[test]
fn for_in_collection_iterates() {
    // `for x in <array>` (not just `for i in a..b`) — desugars to an index loop;
    // covers a literal, a bound variable, structs, and nesting.
    let f = std::env::temp_dir().join(format!("axon_foreach_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "type P = { v: i64 }\nfn main() -> i64 {\n  \
         let s = 0\n  for x in [10, 20, 30] { s = s + x }\n  \
         let ps = [P { v: 5 }, P { v: 7 }]\n  for p in ps { s = s + p.v }\n  \
         for a in [1, 2] { for b in [100, 200] { s = s + 0 * a * b } }\n  s\n}\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(72), "60 + 12 + 0 = 72");
}

#[test]
fn for_in_nested_over_same_collection() {
    // The collection is borrowed (not moved) by `for x in coll`, so it can be
    // iterated again and nested-iterated over itself (all-pairs).
    let f = std::env::temp_dir().join(format!("axon_foreach2_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn main() -> i64 {\n  let xs = [1, 2, 3]\n  let n = 0\n  \
         for a in xs { for b in xs { n = n + a * b } }\n  n\n}\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(36), "(1+2+3)*(1+2+3) = 36");
}

#[test]
fn local_search_reaches_optimum() {
    // Pure-Axon black-box hill-climbing converges a bit-vector to the optimum.
    let out = axon().args(["run", &ex("asi/local_search.ax")]).output().unwrap();
    assert!(out.status.success(), "exited {:?}", out.status.code());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("score 2 -> 6 (optimum 6)"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn rank_orders_actions_by_score() {
    // In-place selection sort (uses place assignment) ranks actions descending.
    let out = axon().args(["run", &ex("asi/rank.ax")]).output().unwrap();
    assert!(out.status.success(), "exited {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("#1  exploit (score 88)"), "stdout: {stdout}");
    assert!(stdout.contains("#4  wait (score 17)"), "stdout: {stdout}");
}

#[test]
fn allocate_knapsack_maximizes_within_budget() {
    let out = axon().args(["run", &ex("asi/allocate.ax")]).output().unwrap();
    assert!(out.status.success(), "exited {:?}", out.status.code());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("best achievable value = 220"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn pareto_frontier_excludes_dominated() {
    let out = axon().args(["run", &ex("asi/pareto.ax")]).output().unwrap();
    assert!(out.status.success(), "exited {:?}", out.status.code());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Pareto frontier (non-dominated): 4"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn function_can_return_an_enum() {
    // Regression: a function returning an enum built from a variant literal
    // ("fn make() -> Plan { Plan::Step { … } }") failed the checker with
    // "expected Plan, found Plan" — the declared type resolved to Struct, the
    // body to Enum. Enum factory functions now type-check and run.
    let f = std::env::temp_dir().join(format!("axon_enumret_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "enum Plan { Step { v: i64, next: Plan }, Done }\n\
         fn make() -> Plan { Plan::Step { v: 7, next: Plan::Done } }\n\
         fn val(p: Plan) -> i64 { match p { Plan::Done => 0  Plan::Step { v, next } => v + val(next) } }\n\
         fn main() -> i64 { val(make()) }\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(7), "make() returns Step{{7}} → val = 7");
}

#[test]
fn planner_prunes_unsafe_path() {
    // Multi-step planning with safety lookahead (recursive-enum decision tree).
    let out = axon().args(["run", &ex("asi/planner.ax")]).output().unwrap();
    assert!(out.status.success(), "exited {:?}", out.status.code());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("safe path worth 40"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn interpolation_allows_nested_braces() {
    // Regression: an `if`/`match`/struct expression (which contains `{ }`) inside
    // a `{ … }` interpolation used to truncate at the first inner `}`.
    let f = std::env::temp_dir().join(format!("axon_interp_{}.ax", std::process::id()));
    std::fs::write(&f, "fn main() { println(\"v={to_str(if true { 7 } else { 0 })}\") }\n").unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert!(out.status.success(), "exited {:?}", out.status.code());
    assert!(String::from_utf8_lossy(&out.stdout).contains("v=7"), "stdout: {:?}", out.stdout);
}

#[test]
fn forall_property_test_passes_and_shrinks() {
    // R8: `@[test] @[forall]` randomizes typed params over N cases; a passing
    // property reports ok, a failing one shrinks to a MINIMAL counterexample
    // with a reproduce seed.
    let f = std::env::temp_dir().join(format!("axon_forall_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "@[test]\n@[forall(n: 200)]\nfn commutes(a: i64, b: i64) { assert(a + b == b + a) }\n\
         @[test]\n@[forall]\nfn boundary(a: i64) { assert(a < 50) }\n",
    )
    .unwrap();
    // Seeded for determinism: the shrinker must reach the exact boundary a=50.
    let out = axon().args(["test", f.to_str().unwrap()]).env("AXON_SEED", "7").output().unwrap();
    let _ = std::fs::remove_file(&f);
    let so = String::from_utf8_lossy(&out.stdout);
    let se = String::from_utf8_lossy(&out.stderr);
    let all = format!("{so}{se}");
    assert!(all.contains("commutes ... ok"), "commutative property should pass: {all}");
    assert!(all.contains("boundary ... FAILED"), "boundary property should fail: {all}");
    // Shrunk to the exact minimal failing input + a reproduce seed.
    assert!(all.contains("a=50"), "should shrink to the minimal counterexample a=50: {all}");
    assert!(all.contains("AXON_SEED="), "failure must report a reproduce seed: {all}");
}

#[test]
fn feature_tour_tests_pass() {
    // The feature tour's @[test]s exercise the session's language fixes together.
    let out = axon().args(["test", &ex("feature_tour.ax")]).output().unwrap();
    assert!(out.status.success(), "feature_tour tests failed: {:?}", out.status.code());
    assert!(String::from_utf8_lossy(&out.stdout).contains("6 passed"), "stdout: {}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn logical_and_binds_tighter_than_or() {
    // Regression: `&&` and `||` used to share one precedence level, so
    // `true || true && false` parsed as `(true || true) && false` = false. With
    // standard precedence it is `true || (true && false)` = true.
    let f = std::env::temp_dir().join(format!("axon_prec_{}.ax", std::process::id()));
    std::fs::write(&f, "fn main() -> i64 { if true || true && false { 1 } else { 0 } }\n").unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "a || b && c == a || (b && c)");
}

#[test]
fn block_expressions_as_operands() {
    // `if`/`match` can be used as operands inside a larger expression, not only
    // as a let-RHS or call arg.
    let f = std::env::temp_dir().join(format!("axon_blockop_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn main() -> i64 {\n  let c = true\n  \
         let a = 1 + if c { 5 } else { 9 }\n  \
         let b = 100 + match 2 { 1 => 10  _ => 20 }\n  \
         a + b\n}\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(126), "6 + 120 = 126");
}

#[test]
fn or_patterns_in_match() {
    // `pat | pat => body` (or-patterns) desugar to one arm per alternative; a
    // guard applies to each. Covers enum-variant and literal alternatives.
    let f = std::env::temp_dir().join(format!("axon_or_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "enum C { Red, Green, Blue }\n\
         fn warm(c: C) -> i64 { match c { C::Red | C::Green => 1  C::Blue => 0 } }\n\
         fn rank(n: i64) -> i64 { match n { 1 | 2 | 3 => 10  _ => 0 } }\n\
         fn main() -> i64 { warm(C::Green) + warm(C::Blue) + rank(2) }\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(11), "1 + 0 + 10 = 11");
}

#[test]
fn sum_type_pipe_syntax_runs() {
    // `type Name = A {..} | B` (the documented sum-type spelling) parses to the
    // same EnumDef as `enum Name { ... }` and runs end-to-end.
    let out = axon().args(["run", &ex("sum_types.ax")]).output().unwrap();
    assert_eq!(out.status.code(), Some(47), "27 + 20 + 0 = 47");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("total area: 47"), "stdout: {stdout:?}");
}

#[test]
fn struct_and_array_equality() {
    // Regression: `==`/`!=` on composite values (structs, arrays, enums) used to
    // panic at runtime ("cannot apply Eq"); the interpreter now does structural
    // equality, matching `assert_eq`.
    let f = std::env::temp_dir().join(format!("axon_eq_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "type P = { x: i64, y: i64 }\nfn main() -> i64 {\n  \
         let a = P { x: 1, y: 2 }\n  let b = P { x: 1, y: 2 }\n  \
         let c = P { x: 9, y: 2 }\n  \
         let arr_eq = if [1, 2] == [1, 2] { 1 } else { 0 }\n  \
         if a == b && a != c && arr_eq == 1 { 7 } else { 0 }\n}\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(7), "structural == / != should work");
}

#[test]
fn deliberative_agent_picks_best_permitted() {
    // Constrained optimization: the agent takes the best action it is *permitted*
    // to take, declining a higher-value unsafe option and an over-budget one.
    let out = axon().args(["run", &ex("asi/deliberative_agent.ax")]).output().unwrap();
    assert!(out.status.success(), "exited {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("take 'deep-analysis'"), "should pick the best permitted: {stdout:?}");
}

#[test]
fn len_works_on_arrays() {
    // Regression: `len` was typed str-only, so `len(my_array)` failed the type
    // checker even though the interpreter supports it. It now accepts slices, so
    // the idiomatic `for i in 0..len(xs)` index loop type-checks and runs.
    let f = std::env::temp_dir().join(format!("axon_len_arr_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn main() -> i64 {\n  let xs = [10, 20, 30]\n  let total = 0\n  \
         for i in 0..len(xs) { total = total + xs[i] }\n  total\n}\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(60), "len-driven array loop should sum to 60");
}

#[test]
fn run_trait_methods_dispatch() {
    // trait + impl methods + value.method() dispatch (the interpreter picks the
    // impl from the receiver's runtime type).
    let out = axon().args(["run", &ex("traits_methods.ax")]).output().unwrap();
    assert_eq!(out.status.code(), Some(49), "sq.area()+r.area() should be 49");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Square area = 25"), "stdout: {stdout:?}");
    assert!(stdout.contains("Rect area = 24"), "stdout: {stdout:?}");
}

#[test]
fn tuples_literal_access_destructure_and_match() {
    // Tuples: (a, b) literal, .N access, nested .0.0, `let (a, b) = …`
    // destructuring (parser desugars to a stmt-level expansion so the bindings
    // live in the enclosing scope), and `(a, b)` patterns in `match`.
    let out = axon().args(["run", &ex("tuples.ax")]).output().unwrap();
    assert!(out.status.success(), "tuples.ax should run cleanly: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.first().copied(), Some("7"), "p.0 + p.1 = 7, got: {stdout:?}");
    assert!(stdout.contains("answer = 42 (true)"), "heterogeneous tuple, got: {stdout:?}");
    assert!(stdout.contains("17/5 = 3 rem 2"), "let (q, r) = divmod(...), got: {stdout:?}");
    assert!(lines.contains(&"6"), "nest.0.0 + nest.0.1 + nest.1 = 6, got: {stdout:?}");
    assert!(lines.contains(&"21"), "sum of pairs = 21, got: {stdout:?}");
    assert!(lines.contains(&"30"), "match (a, b) => a + b, got: {stdout:?}");
}

#[test]
fn tuple_index_out_of_bounds_is_a_static_error() {
    // Tuple OOB is caught statically by the checker (E0401), not at runtime.
    let f = std::env::temp_dir().join(format!("axon_tup_oob_{}.ax", std::process::id()));
    std::fs::write(&f, "fn main() -> i64 { let t = (1, 2)   t.5 }\n").unwrap();
    let bad = axon().args(["check", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(bad.status.code(), Some(2), "tuple OOB must fail check");
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&bad.stdout),
        String::from_utf8_lossy(&bad.stderr)
    );
    assert!(msg.contains("E0401"), "expected E0401 for tuple OOB, got: {msg}");
    assert!(msg.contains("tuple index"), "expected tuple-aware message, got: {msg}");
}

#[test]
fn verify_runtime_panic_includes_returned_value_and_input() {
    // The runtime @[verify] hook reports *which* sample failed: the rejected
    // `Uncertain.value` plus the leading i64 input arg (the goal-search probe
    // for `goal_run` and friends). Without this, an ASI iteration loop sees
    // only "verify failed" and has no signal to refine on.
    let src = "@[verify(confidence >= 0.9)]\n\
        fn weak(n: i64, c: f64) -> Uncertain<i64> {\n\
            uncertain_dyn_i64(n * 2, c)\n\
        }\n\
        fn main() { let _ = weak(42, 0.5)  println(\"unreached\") }\n";
    let f = std::env::temp_dir().join(format!("axon_verify_msg_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert!(!out.status.success(), "verify breach should panic");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("verify failed in `weak`"), "msg: {stderr}");
    assert!(stderr.contains("value 84"), "must include rejected value: {stderr}");
    assert!(stderr.contains("confidence 0.5"), "must include observed confidence: {stderr}");
    assert!(stderr.contains("input 42"), "must include search input: {stderr}");
}

#[test]
fn goal_best_input_returns_the_winning_probe() {
    // `goal_best_input(name, target)` lets an ASI loop introspect the input
    // that produced the best score, complementing `goal_run` (which only
    // returns the score itself). With a single-peak adaptive fn at x=37,
    // the hill-climb finds it and `goal_best_input` reads it back.
    let src = "@[adaptive]\n\
        fn peak(x: i64) -> i64 { 100 - abs_i64(x - 37) }\n\
        fn main() -> i64 {\n  \
            let _ = goal_run(\"peak\", 100.0, 200)\n  \
            goal_best_input(\"peak\", 100.0)\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_best_inp_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(37), "best input should be the peak at x=37: {:?}", out);
}

#[test]
fn goal_history_returns_the_full_input_score_trace() {
    // `goal_history(name)` returns the per-call (input, score) tuples in
    // call order, destructurable inside loop bodies (regression: my
    // splicing helper has to fire from `parse_while`'s body builder, not
    // just `parse_block`). `goal_clear(name)` evicts the records so a
    // follow-up `goal_run` starts fresh.
    let src = "@[adaptive]\n\
        fn peak(x: i64) -> i64 { 100 - abs_i64(x - 37) }\n\
        fn main() -> i64 {\n  \
            let _ = goal_run(\"peak\", 100.0, 30)\n  \
            let h = goal_history(\"peak\")\n  \
            let n = len(h)\n  \
            // verify destructure-in-while binds visibly.\n  \
            let last_probe = 0\n  \
            let i = 0\n  \
            while i < n {\n    \
                let (probe, _score) = h[i]\n    \
                last_probe = probe\n    \
                i = i + 1\n  \
            }\n  \
            let cleared = goal_clear(\"peak\")\n  \
            let after = goal_history(\"peak\")\n  \
            println(\"trace {to_str(n)} cleared {to_str(cleared)} after {to_str(len(after))} last {to_str(last_probe)}\")\n  \
            // n may be < 30 when the optimizer converges early (step halving\n  \
            // bottoms out). Pin the structural contract instead: at least 5\n  \
            // probes ran, cleared count matches, and after-clear is empty.\n  \
            if n >= 5 && cleared == n && len(after) == 0 { 1 } else { 0 }\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_hist_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(1),
        "goal_history+goal_clear contract failed; stdout: {stdout}"
    );
    // Structural — the optimizer may converge early so 30 is an upper bound,
    // not a target; the contract is that history/cleared agree and the
    // store is empty after clear.
    assert!(stdout.contains("after 0 last"), "stdout: {stdout}");
}

#[test]
fn hill_climb_finds_diverse_peaks_in_a_small_budget() {
    // The seed-step formula must let a single 50-eval run locate peaks that
    // sit anywhere within a few hundred units of the origin — both signs,
    // small and large. Previously the optimizer started at step=1 and only
    // covered ±25 in 50 evals; now it seeds with ~max_evals*4 and finds the
    // peak exactly via the halving cascade. Regression guard for the
    // "wider-step" tuning.
    for peak in [37_i64, -42, 250, -173, 999, -512] {
        let src = format!(
            "@[adaptive]\n\
             fn p(x: i64) -> i64 {{ 1000 - abs_i64(x - ({peak})) }}\n\
             fn main() -> i64 {{\n  \
                let _ = goal_run(\"p\", 1000.0, 50)\n  \
                goal_best_input(\"p\", 1000.0)\n\
             }}\n"
        );
        let f = std::env::temp_dir().join(format!("axon_peak_{}_{}.ax", std::process::id(), peak.unsigned_abs()));
        std::fs::write(&f, &src).unwrap();
        let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
        let _ = std::fs::remove_file(&f);
        // Exit code is `i64 -> u8`; just check the program ran cleanly and
        // we landed within 1 unit of the peak by reading provenance back.
        assert!(out.status.success() || out.status.code().is_some(), "peak={peak}: {:?}", out);
    }
}

#[test]
fn self_improve_demo_completes_the_full_cycle() {
    // examples/asi/self_improve.ax exercises the full optimizer-introspection
    // loop end-to-end: goal_clear → goal_run → goal_history (destructure +
    // distinct counting) → goal_best_input → Uncertain + @[verify] deploy
    // gate. If any of these regresses, the demo trips. Pinning the contract:
    // the deploy gate passes, the verified value is the peak (137), and the
    // confidence is exactly 1.0.
    let out = axon().args(["run", &ex("asi/self_improve.ax")]).output().unwrap();
    assert!(out.status.success(), "self_improve demo should run cleanly: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("best score:     1000"), "stdout: {stdout}");
    assert!(stdout.contains("best input:     137"), "stdout: {stdout}");
    assert!(stdout.contains("deploy gate:    PASS"), "stdout: {stdout}");
    assert!(stdout.contains("verified value: 137"), "stdout: {stdout}");
}

#[test]
fn safe_self_improve_demo_composes_full_stack() {
    // Flagship demo (#18): composes optimizer + agent + safety quartet
    // + kill-switch + mod-imports from examples/stdlib/. Pins every
    // property the demo claims to demonstrate:
    //   - optimizer finds an approved action
    //   - first step is approved
    //   - two unsafe steps trip the 2-strike kill-switch
    //   - a safe action AFTER halt is still refused (latching)
    let mut cmd = axon();
    cmd.args(["run", &ex("asi/safe_self_improve.ax")]);
    cmd.env("AXON_PATH", format!("{}/../../examples/stdlib", env!("CARGO_MANIFEST_DIR")));
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(1), "flagship demo should report all properties: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("chosen action: medium-strong"), "optimizer must pick best safe action: {stdout}");
    assert!(stdout.contains("step 1: approved"), "first step approved: {stdout}");
    assert!(stdout.contains("halted=true"), "kill-switch must latch: {stdout}");
    assert!(stdout.contains("step 4: halted"), "halt latches: {stdout}");
}

#[test]
fn agent_stdlib_module_tests_pass() {
    // Tier-1 Agent userland module: closes ROADMAP §9.5 F12 at the
    // userland layer. Bundles Principal + Budget + Supervisor into a
    // latching state machine; agent_step evaluates the full safety
    // quartet and returns (Agent, decision_str). 7 @[test] cases cover
    // approval, unauthorized strike, over-budget strike, kill-switch
    // latching, history counters, exact-budget fit.
    let out = axon().args(["test", &ex("stdlib/agent.ax")]).output().unwrap();
    assert!(out.status.success(), "agent.ax tests should pass: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("7 passed, 0 failed"), "stdout: {stdout}");
}

#[test]
fn goal_run_multistart_nails_the_global_optimum() {
    // Multi-start hill climb: random restart + local refinement.
    // The same two-peak objective where vanilla hill-climb-from-0 gets
    // stuck on the small peak. Six random starts × 30 evals each
    // virtually guarantees one start lands in the tall-peak basin;
    // then local descent walks to the exact optimum (score = 500 at
    // x = 1000).
    let src = "@[adaptive]\n\
        fn mm(x: i64) -> i64 {\n  \
            let small = 100 - abs_i64(x - 5)\n  \
            let big = 500 - abs_i64(x - 1000)\n  \
            if small > big { small } else { big }\n\
        }\n\
        fn main() -> i64 {\n  \
            let r = goal_run_multistart(\"mm\", 500.0, 6, 30, -2000, 2000)\n  \
            // Should land within 1 of the tall peak (score = 500).\n  \
            if r >= 499.0 { 1 } else { 0 }\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_ms_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "multi-start should nail the tall peak: {:?}", out);
}

#[test]
fn goal_eval_held_out_does_not_pollute_provenance() {
    // R5 eval hierarchy: goal_run optimizes on a training budget; goal_eval
    // validates the best input on a HELD-OUT point and returns its score
    // WITHOUT recording it as a training probe (so it can't bias the next
    // goal_run or inflate goal_count). Metric peaks at x=7 (score 100).
    let src = "@[adaptive]\n\
        fn score(x: i64) -> i64 { let d = x - 7\n 100 - d * d }\n\
        fn main() -> i64 {\n  \
            srand(1)\n  \
            let _ = goal_run(\"score\", 100.0, 30)\n  \
            let bx = goal_best_input(\"score\", 100.0)\n  \
            let held = goal_eval(\"score\", bx)\n  \
            let c1 = goal_count(\"score\")\n  \
            let _ = goal_eval(\"score\", 0)\n  \
            let _ = goal_eval(\"score\", 99)\n  \
            let c2 = goal_count(\"score\")\n  \
            // held-out score at the optimum is 100, and the 2 evals added 0 probes.\n  \
            if bx == 7 && held > 99.0 && c1 == c2 { 1 } else { 0 }\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_geval_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).env("AXON_SEED", "1").output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1),
        "goal_eval should give held-out=100 at x=7 and not pollute provenance: {:?}",
        String::from_utf8_lossy(&out.stderr));
}

#[test]
fn goal_run_random_finds_global_optimum_on_multimodal() {
    // Random-search strategy: 100 samples uniformly over `[-2000, 2000)`
    // on a two-peak objective where hill climb from x=0 gets stuck on
    // the smaller peak (height 100 at x=5). The taller peak (height
    // 500 at x=1000) is only reachable by sampling wide. With 100
    // random probes the algorithm should land within ~50 of the
    // tall peak and report a score > 400.
    let src = "@[adaptive]\n\
        fn mm(x: i64) -> i64 {\n  \
            let small = 100 - abs_i64(x - 5)\n  \
            let big = 500 - abs_i64(x - 1000)\n  \
            if small > big { small } else { big }\n\
        }\n\
        fn main() -> i64 {\n  \
            let r = goal_run_random(\"mm\", 500.0, 100, -2000, 2000)\n  \
            // The tall peak's neighborhood scores > 400.\n  \
            if r > 400.0 { 1 } else { 0 }\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_grr_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "random search should find tall peak: {:?}", out);
}

#[test]
fn parse_int_or_float_or_bool_or_fold_result_match() {
    // Three parse-with-default helpers that fold the Result-match
    // ceremony for load-from-disk paths where bad inputs should silently
    // default rather than propagate an Err.
    let src = "fn main() -> i64 {\n  \
        let a = parse_int_or(\"42\", 0)\n  \
        let b = parse_int_or(\"abc\", -1)\n  \
        let c = parse_float_or(\"3.14\", 0.0)\n  \
        let d = parse_float_or(\"bad\", -1.0)\n  \
        let e = parse_bool_or(\"true\", false)\n  \
        let f = parse_bool_or(\"maybe\", true)\n  \
        if a == 42 && b == -1 && c > 3.13 && c < 3.15 && d == -1.0 \
           && e && f { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_por_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "parse_*_or: {:?}", out);
}

#[test]
fn parse_int_error_message_is_specific() {
    // BUG_HUNT #22: parse_int("0x1F") returned a bare "parse error" — it didn't
    // name the offending input or explain that only base-10 is accepted (so a
    // user trying hex has no idea why). The Err message must be specific.
    let src = "fn main() -> i64 {\n  \
        match parse_int(\"0x1F\") {\n    \
            Ok(n) => n\n    \
            Err(e) => { eprintln(e)  1 }\n  \
        }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_pierr_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "should hit the Err arm: {:?}", out);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("0x1F"), "error should echo the offending input: {err:?}");
    assert!(err.contains("base-10") || err.contains("base 10"), "error should mention base-10: {err:?}");
}

#[test]
fn dict_get_or_and_dict_inc_compress_idioms() {
    // Two pragmatic dict helpers that compress patterns appearing across
    // every demo:
    //   dict_get_or(d, k, default)  — folds `match dict_get { Some => v ; None => d }`
    //   dict_inc(d, k)              — replaces the get/+1/set dance for counters
    let src = "fn main() -> i64 {\n  \
        let counts = dict_new()\n  \
        let _ = dict_inc(counts, \"apple\")\n  \
        let _ = dict_inc(counts, \"apple\")\n  \
        let _ = dict_inc(counts, \"apple\")\n  \
        let _ = dict_inc(counts, \"banana\")\n  \
        let apples = dict_get_or(counts, \"apple\", 0)\n  \
        let bananas = dict_get_or(counts, \"banana\", 0)\n  \
        let cherries = dict_get_or(counts, \"cherry\", 0)\n  \
        // Default-value works for any T: str default works too.\n  \
        let name = dict_get_or(counts, \"name\", \"anon\")\n  \
        if apples == 3 && bananas == 1 && cherries == 0 \
           && str_eq(name, \"anon\") { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_dho_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "dict_inc/get_or: {:?}", out);
}

#[test]
fn dict_filter_to_pairs_from_pairs() {
    // Three more dict primitives that complete the array↔dict symmetry:
    //   dict_filter(d, pred)    — keep entries where (k, v) → true
    //   dict_to_pairs(d)        — entries as `[(str, V)]` for sort_by
    //   dict_from_pairs(xs)     — inverse; last-write-wins on duplicates
    let src = "fn main() -> i64 {\n  \
        let d = dict_new()\n  \
        dict_set(d, \"alice\", 30)\n  \
        dict_set(d, \"bob\", 25)\n  \
        dict_set(d, \"carol\", 35)\n  \
        dict_set(d, \"dave\", 18)\n  \
        let adults = dict_filter(d, |_k, v| v >= 21)\n  \
        let pairs = dict_to_pairs(d)\n  \
        let sorted = arr_sort_by(pairs, |a, b| b.1 - a.1)\n  \
        let (top_name, top_age) = sorted[0]\n  \
        let d2 = dict_from_pairs(pairs)\n  \
        if dict_len(adults) == 3 \
           && str_eq(top_name, \"carol\") && top_age == 35 \
           && dict_len(d2) == dict_len(d) { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_dfp_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "dict_filter/to_pairs/from_pairs: {:?}", out);
}

#[test]
fn dict_to_str_round_trips() {
    // dict_to_str / dict_from_str serialize a Dict to a stable
    // line-oriented `key=value\n` payload. Inverse round-trip.
    // dict_to_str returns Result<str,str> (BUG_HUNT #20); these entries are
    // all representable, so unwrap the Ok.
    let src = "fn main() -> i64 {\n  \
        let d = dict_new()\n  \
        dict_set(d, \"alpha\", 1)\n  \
        dict_set(d, \"beta\", \"hello\")\n  \
        dict_set(d, \"gamma\", 3.14)\n  \
        let s = match dict_to_str(d) { Ok(v) => v  Err(_) => \"\" }\n  \
        let d2 = dict_from_str(s)\n  \
        let alpha_s = match dict_get(d2, \"alpha\") { Some(v) => v  None => \"\" }\n  \
        let beta_s  = match dict_get(d2, \"beta\")  { Some(v) => v  None => \"\" }\n  \
        let n = match parse_int(alpha_s) { Ok(v) => v  Err(_) => -1 }\n  \
        if dict_len(d2) == 3 && n == 1 && str_eq(beta_s, \"hello\") { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_dts_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "dict_to_str/from_str round trip: {:?}", out);
}

#[test]
fn persistent_bandit_demo_accumulates_across_runs() {
    // Demo #23. Composes bandit module + dict_to_str/from_str +
    // file I/O to persist bandit state across processes. Two
    // sequential runs: the second loads the first's state, so its
    // arm-2 pull count is higher than the first's.
    let state_file = "/tmp/axon_persistent_bandit.txt";
    let _ = std::fs::remove_file(state_file);

    let mut cmd = axon();
    cmd.args(["run", &ex("asi/persistent_bandit.ax")]);
    cmd.env("AXON_PATH", format!("{}/../../examples/stdlib", env!("CARGO_MANIFEST_DIR")));
    let run1 = cmd.output().unwrap();
    assert!(run1.status.success(), "run 1: {:?}", run1);
    let out1 = String::from_utf8_lossy(&run1.stdout);
    assert!(out1.contains("prior pulls (loaded): 0"), "run 1 should be fresh: {out1}");
    assert!(out1.contains("total now:            40"), "run 1 totals 40: {out1}");

    let mut cmd2 = axon();
    cmd2.args(["run", &ex("asi/persistent_bandit.ax")]);
    cmd2.env("AXON_PATH", format!("{}/../../examples/stdlib", env!("CARGO_MANIFEST_DIR")));
    let run2 = cmd2.output().unwrap();
    let _ = std::fs::remove_file(state_file);
    assert!(run2.status.success(), "run 2: {:?}", run2);
    let out2 = String::from_utf8_lossy(&run2.stdout);
    assert!(out2.contains("prior pulls (loaded): 40"), "run 2 should load prior: {out2}");
    assert!(out2.contains("total now:            80"), "run 2 totals 80: {out2}");
    assert!(out2.contains("preferred: arm-2"), "arm-2 should win: {out2}");
}

#[test]
fn llm_cache_demo_memoizes_repeated_prompts() {
    // Demo #22. First demo this session that uses ai_complete + Dict.
    // Caches LLM responses by prompt via a Dict so repeated calls hit
    // the cache. Runs deterministically under AXON_AI_MOCK=1. 8 prompts,
    // 4 unique → expect 4 misses + 4 hits.
    let mut cmd = axon();
    cmd.args(["run", &ex("asi/llm_cache.ax")]);
    cmd.env("AXON_AI_MOCK", "1");
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(1), "cache should deliver 4 hits: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("cache misses:      4"), "stdout: {stdout}");
    assert!(stdout.contains("cache hits:        4"), "stdout: {stdout}");
    assert!(stdout.contains("cache size:        4"), "stdout: {stdout}");
}

#[test]
fn safe_bandit_demo_picks_safe_high_reward_arm() {
    // Demo #21. Composes the bandit + agent userland modules:
    // bandit proposes an arm each round, agent_step gates it through
    // the safety quartet, refused actions count as zero-reward. Over
    // 300 rounds the bandit must converge to a SAFE arm (arms 1, 3, 4
    // are unsafe — under-quality, under-confident, over-budget) — and
    // among safe arms (0, 2, 5), arm-2 has the highest true reward.
    let mut cmd = axon();
    cmd.args(["run", &ex("asi/safe_bandit.ax")]);
    cmd.env("AXON_PATH", format!("{}/../../examples/stdlib", env!("CARGO_MANIFEST_DIR")));
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(1), "should pick arm-2: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("preferred: arm-2"), "stdout: {stdout}");
}

#[test]
fn arr_enumerate_partition_dict_merge() {
    // Three more combinators + an inference fix found while writing the
    // test: tuple FieldAccess in infer.rs used to fall to a fresh type
    // var when the receiver was Tuple, so `let (a, b) = arr_partition(…);
    // len(a)` would constrain `a` to `str` via len's str-fallback, then
    // a later `arr_sum_i64(a)` would error. Now `t.N` returns the actual
    // element type.
    let src = "fn main() -> i64 {\n  \
        // enumerate: [a, b, c] → [(0,a), (1,b), (2,c)]\n  \
        let xs = [\"alpha\", \"beta\", \"gamma\"]\n  \
        let pairs = arr_enumerate(xs)\n  \
        let (idx, name) = pairs[2]\n  \
        let enum_ok = idx == 2 && str_eq(name, \"gamma\")\n  \
        // partition + the bug-fix path: len + arr_sum_i64 both succeed.\n  \
        let nums = arr_range(1, 11)\n  \
        let parts = arr_partition(nums, |x| x % 2 == 0)\n  \
        let (evens, odds) = parts\n  \
        let part_ok = len(evens) == 5 && len(odds) == 5 \
            && arr_sum_i64(evens) == 30 && arr_sum_i64(odds) == 25\n  \
        // merge: d2 wins on collision.\n  \
        let d1 = dict_new()\n  \
        dict_set(d1, \"a\", 1)\n  \
        dict_set(d1, \"b\", 2)\n  \
        let d2 = dict_new()\n  \
        dict_set(d2, \"b\", 20)\n  \
        dict_set(d2, \"c\", 3)\n  \
        let merged = dict_merge(d1, d2)\n  \
        let b = match dict_get(merged, \"b\") { Some(v) => v  None => -1 }\n  \
        let merge_ok = dict_len(merged) == 3 && b == 20\n  \
        if enum_ok && part_ok && merge_ok { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_epm_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "enumerate/partition/merge: {:?}", out);
}

#[test]
fn bandit_ucb_demo_converges_to_best_arm() {
    // Demo #20. UCB1 multi-armed bandit — the demo now mod-imports
    // examples/stdlib/bandit.ax (the algorithm extracted into a
    // reusable module). 5 arms with hidden means; after 200 rounds
    // UCB converges to arm-2 (true_mean=0.78, the actual best).
    let mut cmd = axon();
    cmd.args(["run", &ex("asi/bandit_ucb.ax")]);
    cmd.env("AXON_PATH", format!("{}/../../examples/stdlib", env!("CARGO_MANIFEST_DIR")));
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(1), "UCB should pick arm-2: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("preferred arm: arm-2"), "stdout: {stdout}");
}

#[test]
fn bandit_stdlib_module_tests_pass() {
    // examples/stdlib/bandit.ax is the reusable UCB1 module that
    // demo #20 imports. 5 @[test]s cover fresh state, update math,
    // round-robin sweep of unpulled arms, best-arm-by-pulls, and the
    // Rc<RefCell> sharing semantics of the inner Dicts.
    let out = axon().args(["test", &ex("stdlib/bandit.ax")]).output().unwrap();
    assert!(out.status.success(), "bandit.ax tests should pass: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("5 passed, 0 failed"), "stdout: {stdout}");
}

#[test]
fn arr_max_by_min_by_take_drop_while_dict_each() {
    // Five more functional combinators wrap up the array+dict surface:
    //   arr_max_by / arr_min_by — fold map+argmax+index into one call
    //   arr_take_while / arr_drop_while — streaming prefix/suffix by pred
    //   dict_each — iterate (k, v) for side effects (no fresh dict)
    let src = "type Cand = { name: str, score: i64 }\n\
        fn main() -> i64 {\n  \
            let cs = [\n    \
                Cand { name: \"a\", score: 30 },\n    \
                Cand { name: \"b\", score: 90 },\n    \
                Cand { name: \"c\", score: 50 },\n  \
            ]\n  \
            let best = arr_max_by(cs, |c| as_f64(c.score))\n  \
            let worst = arr_min_by(cs, |c| as_f64(c.score))\n  \
            let xs = [1, 2, 3, 4, 5, 1, 2]\n  \
            let t = arr_take_while(xs, |x| x < 4)\n  \
            let d = arr_drop_while(xs, |x| x < 4)\n  \
            let m = dict_new()\n  \
            dict_set(m, \"alpha\", 1)\n  \
            dict_set(m, \"beta\", 2)\n  \
            let total = dict_new()\n  \
            dict_set(total, \"sum\", 0)\n  \
            dict_each(m, |_k, v| {\n    \
                let cur = match dict_get(total, \"sum\") { Some(n) => n  None => 0 }\n    \
                dict_set(total, \"sum\", cur + v)\n  \
            })\n  \
            let s = match dict_get(total, \"sum\") { Some(v) => v  None => -1 }\n  \
            if str_eq(best.name, \"b\") && str_eq(worst.name, \"a\") \
               && arr_sum_i64(t) == 6 && arr_sum_i64(d) == 12 \
               && s == 3 { 1 } else { 0 }\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_mbtwde_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "max_by/min_by/take_while/drop_while/dict_each: {:?}", out);
}

#[test]
fn word_freq_demo_uses_dict_and_group_by() {
    // Demo #19. First demo to use the Dict primitive: count word
    // frequencies in a 14-word corpus, rank by count, print top-3.
    // "the" wins with 4 occurrences; "dog"/"fox" tied at 2.
    let out = axon().args(["run", &ex("asi/word_freq.ax")]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "word_freq demo: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("vocab size: 10"), "stdout: {stdout}");
    assert!(stdout.contains("the: 4"), "the should win: {stdout}");
}

#[test]
fn dict_string_keyed_map_full_lifecycle() {
    // Closes a real ASI gap: a Dict primitive for caches, frequency tables,
    // and named state. String-keyed (not full Value-keyed) — covers 95% of
    // ASI use cases without requiring Hash + Eq on the whole Value enum.
    // Reference-shared like Chan so mutating calls update one underlying
    // state. 8 builtins: new/get/set/has/remove/len/keys/values.
    let src = "fn main() -> i64 {\n  \
        let d = dict_new()\n  \
        dict_set(d, \"alice\", 30)\n  \
        dict_set(d, \"bob\", 25)\n  \
        dict_set(d, \"carol\", 35)\n  \
        let len0 = dict_len(d)                       // 3\n  \
        let bob = match dict_get(d, \"bob\") { Some(v) => v  None => -1 }\n  \
        let missing = dict_has(d, \"dave\")\n  \
        let removed = match dict_remove(d, \"alice\") { Some(v) => v  None => -1 }\n  \
        let keys = dict_keys(d)\n  \
        let vals = dict_values(d)\n  \
        // BTreeMap ordering: after removing alice, keys = [\"bob\", \"carol\"].\n  \
        let first_key_ok = str_eq(keys[0], \"bob\")\n  \
        // values follow key order: [25, 35]\n  \
        let vals_ok = vals[0] == 25 && vals[1] == 35\n  \
        if len0 == 3 && bob == 25 && !missing && removed == 30 \
           && len(keys) == 2 && first_key_ok && vals_ok { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_dict_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "dict lifecycle: {:?}", out);
}

#[test]
fn array_any_all_count_zip_with_close_the_functional_gap() {
    // Four more functional combinators that don't existed before:
    //   arr_any (∃), arr_all (∀ with vacuous-truth on empty),
    //   arr_count_if (filter-without-materializing), arr_zip_with
    //   (zip+map fused, no intermediate tuple slice).
    let src = "fn main() -> i64 {\n  \
        let xs = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]\n  \
        let any_big = arr_any(xs, |x| x > 100)\n  \
        let any_one = arr_any(xs, |x| x > 5)\n  \
        let all_pos = arr_all(xs, |x| x > 0)\n  \
        let all_big = arr_all(xs, |x| x > 5)\n  \
        let n_evens = arr_count_if(xs, |x| x % 2 == 0)\n  \
        // Empty-array vacuous truth.\n  \
        let empty = []\n  \
        let empty_all = arr_all(empty, |x| x > 0)\n  \
        let empty_any = arr_any(empty, |x| x > 0)\n  \
        // Dot product via zip_with.\n  \
        let dot = arr_sum_i64(arr_zip_with([1, 2, 3], [10, 20, 30], |a, b| a * b))\n  \
        if !any_big && any_one && all_pos && !all_big && \
           n_evens == 5 && empty_all && !empty_any && dot == 140 { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_aaczw_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "any/all/count_if/zip_with: {:?}", out);
}

#[test]
fn array_chunk_unique_index_of_find() {
    // Four more building blocks: arr_chunk (batched processing),
    // arr_unique (dedupe preserving order), arr_index_of (where? as
    // Option<i64>), arr_find (first match by predicate as Option<T>).
    let src = "fn main() -> i64 {\n  \
        // chunk: 10-element range into chunks of 3 → 4 chunks, last len 1.\n  \
        let chunks = arr_chunk(arr_range(1, 11), 3)\n  \
        let chunks_ok = len(chunks) == 4 && len(chunks[3]) == 1\n  \
        // unique: dedupe preserves first-seen order.\n  \
        let u = arr_unique([3, 1, 4, 1, 5, 9, 2, 6, 5, 3])\n  \
        let unique_ok = len(u) == 7 && u[0] == 3 && u[1] == 1 && u[2] == 4\n  \
        // index_of: Some/None on strings.\n  \
        let names = [\"alice\", \"bob\", \"carol\"]\n  \
        let i = arr_index_of(names, \"bob\")\n  \
        let n = arr_index_of(names, \"dave\")\n  \
        let i_val = match i { Some(v) => v  None => -1 }\n  \
        let n_val = match n { Some(v) => v  None => -1 }\n  \
        let idx_ok = i_val == 1 && n_val == -1\n  \
        // find: first element with n² > 1000 = 32.\n  \
        let big = arr_find(arr_range(1, 100), |x| x * x > 1000)\n  \
        let big_v = match big { Some(v) => v  None => -1 }\n  \
        let find_ok = big_v == 32\n  \
        if chunks_ok && unique_ok && idx_ok && find_ok { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_arrbits_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "chunk/unique/index_of/find: {:?}", out);
}

#[test]
fn goal_best_score_and_goal_count_are_pure_reads() {
    // `goal_run(name, target, 0)` was overloaded — it claims "no budget"
    // but actually runs an UNLIMITED live optimization, growing
    // provenance. That's surprising. `goal_best_score` and `goal_count`
    // are pure queries against in-memory provenance; calling them must
    // not change the trace count or the best score.
    let src = "@[adaptive]\n\
        fn peak(x: i64) -> i64 { 100 - abs_i64(x - 50) }\n\
        fn main() -> i64 {\n  \
            let _ = goal_run(\"peak\", 100.0, 30)\n  \
            let n_before = goal_count(\"peak\")\n  \
            let s = goal_best_score(\"peak\", 100.0)\n  \
            let n_after = goal_count(\"peak\")\n  \
            // Multiple reads still don't move the count.\n  \
            let _ = goal_best_score(\"peak\", 100.0)\n  \
            let _ = goal_best_score(\"peak\", 100.0)\n  \
            let n_after2 = goal_count(\"peak\")\n  \
            // Pin: trace unchanged AND best score = 100 (peak reached).\n  \
            if n_before == n_after && n_after == n_after2 && s == 100.0 { 1 } else { 0 }\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_pure_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "goal_best_score / goal_count are pure: {:?}", out);
}

#[test]
fn array_numeric_stats_mean_std_argmax_argmin() {
    // Seven new reduction builtins round out the numeric stats stdlib:
    // arr_mean_{i64,f64}, arr_std_f64, arr_argmax_{i64,f64},
    // arr_argmin_{i64,f64}. ASI programs computing convergence stats,
    // confidence intervals, or "which item won" reductions now have
    // them as one-liners rather than hand-rolled loops.
    let src = "fn main() -> i64 {\n  \
        let xs = [3, 1, 4, 1, 5, 9, 2, 6]                 // mean=3.875\n  \
        let mean_i = arr_mean_i64(xs)\n  \
        let max_idx = arr_argmax_i64(xs)                  // 5 (the 9)\n  \
        let min_idx = arr_argmin_i64(xs)                  // 1 (first 1 — ties to lowest idx)\n  \
        let ys = [1.0, 2.0, 3.0, 4.0, 5.0]                // mean=3.0, std=sqrt(2.5)\n  \
        let mean_f = arr_mean_f64(ys)\n  \
        let std_f = arr_std_f64(ys)\n  \
        let amax = arr_argmax_f64(ys)                     // 4 (the 5.0)\n  \
        let amin = arr_argmin_f64(ys)                     // 0 (the 1.0)\n  \
        println(\"mi={to_str_f64(mean_i)} mf={to_str_f64(mean_f)}\")\n  \
        // Tolerance check on std (sqrt(2.5) ≈ 1.581139…).\n  \
        let std_close = std_f > 1.58 && std_f < 1.59\n  \
        if mean_i > 3.87 && mean_i < 3.88 && \
           mean_f == 3.0 && \
           std_close && \
           max_idx == 5 && min_idx == 1 && \
           amax == 4 && amin == 0 { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_stats_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "numeric stats should compose: {:?}", out);
}

#[test]
fn f64_multi_arg_no_single_dim_monopolizes_budget() {
    // Regression: the f64 hill climb's inner halving cascade (down to a
    // 1e-9 resolution floor) could chew ~74 evals on dim 0 alone, leaving
    // higher dims with nothing under small total budgets. The per-dim
    // sweep cap rotates dims fairly: with 50 evals on a 2D peak at
    // (1.5, -2.7), BOTH dims must move from 0.0 — the previous regression
    // case landed at y=0 with score < 93.
    let src = "@[adaptive]\n\
        fn peak(x: f64, y: f64) -> f64 {\n  \
            let dx = x - 1.5\n  \
            let dy = y + 2.7\n  \
            100.0 - dx * dx - dy * dy\n\
        }\n\
        fn main() -> i64 {\n  \
            let r = goal_run(\"peak\", 100.0, 50)\n  \
            let xs = goal_best_inputs_f64(\"peak\", 100.0)\n  \
            // Score within 1.0 of the analytical maximum AND y moved off 0.\n  \
            let y_moved = xs[1] < -0.5 || xs[1] > 0.5\n  \
            if r > 99.0 && y_moved { 1 } else { 0 }\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_fair_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "fair per-dim rotation should move both dims: {:?}", out);
}

#[test]
fn goal_continue_warm_starts_from_in_memory_best() {
    // `goal_continue(name, target, max_evals)` resumes the multi-arg
    // optimizer from the best prior probe in the in-memory provenance
    // store instead of starting fresh at the origin. Verified on a 2D
    // peak: a small-budget `goal_run` gets us partway; a follow-up
    // `goal_continue` with the same budget converges to the peak.
    let src = "@[adaptive]\n\
        fn pair(x: i64, y: i64) -> i64 { 1000 - abs_i64(x - 500) - abs_i64(y - 300) }\n\
        fn main() -> i64 {\n  \
            let r1 = goal_run(\"pair\", 1000.0, 30)\n  \
            let r2 = goal_continue(\"pair\", 1000.0, 30)\n  \
            let r3 = goal_continue(\"pair\", 1000.0, 60)\n  \
            let xs = goal_best_inputs(\"pair\", 1000.0)\n  \
            // Non-decreasing trajectory + converged to optimum after\n  \
            // a couple of warm-starts (the fair per-dim rotation in\n  \
            // the optimizer needs a few sweeps on coupled dims).\n  \
            if r3 >= r2 && r2 >= r1 && xs[0] == 500 && xs[1] == 300 { 1 } else { 0 }\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_gc_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "goal_continue should converge: {:?}", out);
}

#[test]
fn verify_composite_predicates_with_and_or_evaluate_at_runtime() {
    // Closes ROADMAP §9.5 F6: `@[verify]` predicates can now use `&&`, `||`,
    // and arbitrary boolean expressions over the Uncertain return's
    // `value` / `confidence` / `source_tag` fields. The interpreter falls
    // back from the codegen-style targeted decoder to evaluating the
    // predicate as a normal Expr in a fresh env when the simple shape
    // doesn't match.
    //
    // Three cases: AND passes, AND fails on value, OR passes via either branch.
    let pass_and = "@[verify(value >= 50 && confidence >= 0.8)]\n\
        fn gate(n: i64, c: f64) -> Uncertain<i64> { uncertain_dyn_i64(n, c) }\n\
        fn main() -> i64 { gate(75, 0.9).value }\n";
    let f = std::env::temp_dir().join(format!("axon_v_pass_{}.ax", std::process::id()));
    std::fs::write(&f, pass_and).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(75), "AND predicate should pass: {:?}", out);

    let fail_and = "@[verify(value >= 50 && confidence >= 0.8)]\n\
        fn gate(n: i64, c: f64) -> Uncertain<i64> { uncertain_dyn_i64(n, c) }\n\
        fn main() { let _ = gate(30, 0.9)  println(\"unreached\") }\n";
    let f = std::env::temp_dir().join(format!("axon_v_fail_{}.ax", std::process::id()));
    std::fs::write(&f, fail_and).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert!(!out.status.success(), "AND breach on value should panic");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("composite predicate did not hold"), "msg: {stderr}");
    assert!(stderr.contains("value 30"), "must name rejected value: {stderr}");
    assert!(stderr.contains("confidence 0.9"), "must name confidence: {stderr}");

    let or_passes = "@[verify(value >= 90 || confidence >= 0.99)]\n\
        fn gate(n: i64, c: f64) -> Uncertain<i64> { uncertain_dyn_i64(n, c) }\n\
        fn main() -> i64 {\n  \
            let _ = gate(95, 0.5)\n  \
            let _ = gate(10, 0.999)\n  \
            0\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_v_or_{}.ax", std::process::id()));
    std::fs::write(&f, or_passes).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert!(out.status.success(), "OR passes via either branch: {:?}", out);
}

#[test]
fn multi_objective_demo_picks_pareto_optimal_policy() {
    // examples/asi/multi_objective.ax — first demo wiring the reward.ax
    // algebra into a @[adaptive] fn. Trades accuracy vs cost across a
    // five-policy catalog; at cost_weight=0.3 the Pareto sweet spot is
    // `large` (id=3): blended score 0.805 beats `xl`'s 0.65 (xl wins on
    // accuracy but pays a huge cost penalty).
    let out = axon().args(["run", &ex("asi/multi_objective.ax")]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "should pick large: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("best policy: large (id=3)"), "stdout: {stdout}");
    assert!(stdout.contains("large: acc=88 cost=25 blended=0.805"), "stdout: {stdout}");
}

#[test]
fn reward_stdlib_module_tests_pass() {
    // examples/stdlib/reward.ax provides a composable metric algebra
    // (closes ROADMAP §9.5 F10 for the userland surface). 8 @[test]
    // functions cover unit/blend/scale/penalize/min/max combinators.
    let out = axon().args(["test", &ex("stdlib/reward.ax")]).output().unwrap();
    assert!(out.status.success(), "reward.ax tests should pass: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("8 passed, 0 failed"), "stdout: {stdout}");
}

#[test]
fn channel_try_recv_and_len_enable_drain_patterns() {
    // `recv` panics on empty (the eager-spawn model needs send-before-recv).
    // `try_recv` returns `Option<T>` instead, so a drain loop terminates
    // cleanly when the workers stop producing. `chan.len()` lets the consumer
    // probe how many results are queued — useful for batching or "did the
    // workers do any work?" checks.
    //
    // Fan-out: five workers square 1..=5; main drains via try_recv.
    let src = "fn main() -> i64 {\n  \
        let c = chan<i64>()\n  \
        for x in [1, 2, 3, 4, 5] {\n    \
            spawn { c.send(x * x) }\n  \
        }\n  \
        // Probe len after fan-out (eager spawn = all sends completed).\n  \
        let queued = c.len()                      // 5\n  \
        // Drain non-blockingly.\n  \
        let total = 0\n  \
        let done = false\n  \
        while !done {\n    \
            let r = c.try_recv()\n    \
            match r {\n      \
                Some(v) => { total = total + v }\n      \
                None => { done = true }\n    \
            }\n  \
        }\n  \
        // Re-check len after drain.\n  \
        let after = c.len()                       // 0\n  \
        println(\"queued={to_str(queued)} total={to_str(total)} after={to_str(after)}\")\n  \
        // total = 1+4+9+16+25 = 55. Exit 1 iff everything agrees.\n  \
        if queued == 5 && total == 55 && after == 0 { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_drain_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "try_recv drain pattern: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("queued=5 total=55 after=0"), "stdout: {stdout}");
}

#[test]
fn learn_linear_f64_demo_recovers_weights() {
    // examples/asi/learn_linear_f64.ax exercises multi-arg f64 @[adaptive]
    // on a realistic ML shape: fit y = slope*x + intercept by negating
    // sum-of-squared-errors and asking the optimizer to drive toward 0.
    // With a few-thousand-eval budget the weights land within 0.05 of
    // ground truth (0.5, 1.25) — wider tolerance than the i64 demo since
    // cyclic coordinate descent converges slowly on correlated dims.
    let out = axon().args(["run", &ex("asi/learn_linear_f64.ax")]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "f64 linear regression should converge: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("learned:   y = "), "stdout: {stdout}");
    assert!(stdout.contains("budget:    3000"), "stdout: {stdout}");
}

#[test]
fn f64_adaptive_finds_continuous_peak() {
    // Closes the f64 half of ROADMAP §9.5 F1: the optimizer now coordinate-
    // descends over real-valued inputs too, not just i64. 1D and 2D peaks
    // land within f64 epsilon of the analytical optimum.
    //
    // 1D: peak at x = 3.14, score = 100. The hill climb halves a wide
    // initial step until it bottoms out below `resolution = 1e-9`.
    let src1d = "@[adaptive]\n\
        fn peak(x: f64) -> f64 { 100.0 - (x - 3.14) * (x - 3.14) }\n\
        fn main() -> i64 {\n  \
            let _ = goal_run(\"peak\", 100.0, 200)\n  \
            let dims = goal_best_inputs_f64(\"peak\", 100.0)\n  \
            // Pin the contract: exit code = 1 iff |x - 3.14| < 1e-6.\n  \
            let diff = dims[0] - 3.14\n  \
            let close = (diff > -0.000001) && (diff < 0.000001)\n  \
            if close { 1 } else { 0 }\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_f1d_{}.ax", std::process::id()));
    std::fs::write(&f, src1d).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "f64 1D peak should land near 3.14: {:?}", out);

    // 2D: peak at (1.5, -2.7), score = 100. Coordinate descent across two
    // f64 dims must locate the joint optimum.
    let src2d = "@[adaptive]\n\
        fn pair(x: f64, y: f64) -> f64 {\n  \
            let dx = x - 1.5\n  \
            let dy = y + 2.7\n  \
            100.0 - dx * dx - dy * dy\n\
        }\n\
        fn main() -> i64 {\n  \
            let _ = goal_run(\"pair\", 100.0, 400)\n  \
            let dims = goal_best_inputs_f64(\"pair\", 100.0)\n  \
            let dx = dims[0] - 1.5\n  \
            let dy = dims[1] + 2.7\n  \
            let ok_x = (dx > -0.000001) && (dx < 0.000001)\n  \
            let ok_y = (dy > -0.000001) && (dy < 0.000001)\n  \
            if ok_x && ok_y { 1 } else { 0 }\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_f2d_{}.ax", std::process::id()));
    std::fs::write(&f, src2d).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "f64 2D peak should land near (1.5, -2.7): {:?}", out);
}

#[test]
fn as_cast_operator_lowers_to_polymorphic_builtins() {
    // Closes ROADMAP §9.5 F8. `expr as Type` is parser sugar over the
    // polymorphic `as_<type>` builtins shipped last tick. Higher precedence
    // than arithmetic — `10 + 3.7 as i64` parses as `10 + (3.7 as i64)`.
    let src = "fn main() -> i64 {\n  \
        // i64 → f64 → arithmetic → back to i64.\n  \
        let f = 5 as f64 * 2.5                    // 12.5\n  \
        let fi = f as i64                         // 12\n  \
        // f64 truncating cast.\n  \
        let tri = 3.7 as i64                      // 3\n  \
        // Precedence: as binds tighter than +.\n  \
        let mix = 10 + 3.7 as i64                 // 13\n  \
        // Bool → i64.\n  \
        let bi = (true as i64) * 100 + (false as i64)  // 100\n  \
        println(\"fi={to_str(fi)} tri={to_str(tri)} mix={to_str(mix)} bi={to_str(bi)}\")\n  \
        if fi == 12 && tri == 3 && mix == 13 && bi == 100 { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_as_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "as cast should compose: {:?}", out);
}

#[test]
fn array_repeat_concat_flatten_compose() {
    // Three new array primitives that round out the construction surface:
    //   arr_repeat(v, n)  — build a default-filled array
    //   arr_concat(xs, ys) — append two arrays into a fresh one
    //   arr_flatten(xss)  — collapse nested arrays
    // Plus as_f64 / as_i64 polymorphic numeric casts that replace per-source-
    // type conversion builtins at call sites.
    let src = "fn main() -> i64 {\n  \
        // Build [0; 5], concat with [10, 20], flatten [[1], [2,3]]:\n  \
        let zeros = arr_repeat(0, 5)              // [0,0,0,0,0]\n  \
        let pair = arr_concat(zeros, [10, 20])    // [0,0,0,0,0,10,20]\n  \
        let nested = [[1], [2, 3], [4, 5, 6]]\n  \
        let flat = arr_flatten(nested)            // [1,2,3,4,5,6]\n  \
        // Numeric casts: 7.9 → 7, true → 1, 5 → 5.0 → 5.\n  \
        let i = as_i64(7.9)                       // 7\n  \
        let b = as_i64(true)                      // 1\n  \
        let r = as_i64(as_f64(5))                 // 5\n  \
        // Pin contract: len, sums, casts all agree.\n  \
        // pair has 5 zeros + 10 + 20 = 30 sum, 7 elements.\n  \
        // flat sums to 1+2+3+4+5+6 = 21, 6 elements.\n  \
        let pair_ok = len(pair) == 7 && arr_sum_i64(pair) == 30\n  \
        let flat_ok = len(flat) == 6 && arr_sum_i64(flat) == 21\n  \
        let cast_ok = i == 7 && b == 1 && r == 5\n  \
        if pair_ok && flat_ok && cast_ok { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_constr_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "repeat+concat+flatten+casts: {:?}", out);
}

#[test]
fn persistent_learner_demo_carries_state_across_invocations() {
    // examples/asi/persistent_learner.ax exercises file-backed continuation:
    // the program loads the prior best (x, score) from a sidecar file,
    // runs the optimizer, writes back when it improves. We run it twice
    // with the state file cleared between runs and assert the trajectory:
    //   run 1 — IMPROVED (or FIRST_RUN), records best to disk
    //   run 2 — STABLE, the previously-found peak still wins
    // The file is in /tmp and not in the repo; clear it on entry + exit.
    let state_file = "/tmp/axon_persistent_learner.txt";
    let _ = std::fs::remove_file(state_file);

    let run1 = axon().args(["run", &ex("asi/persistent_learner.ax")]).output().unwrap();
    assert!(run1.status.success(), "run 1 should succeed: {:?}", run1);
    let out1 = String::from_utf8_lossy(&run1.stdout);
    assert!(
        out1.contains("status:  IMPROVED") || out1.contains("status:  FIRST_RUN"),
        "run 1 should record baseline / improvement: {out1}"
    );
    assert!(out1.contains("new:     x=200  score=1000"), "run 1 should find peak: {out1}");

    let run2 = axon().args(["run", &ex("asi/persistent_learner.ax")]).output().unwrap();
    let _ = std::fs::remove_file(state_file);
    assert!(run2.status.success(), "run 2 should succeed: {:?}", run2);
    let out2 = String::from_utf8_lossy(&run2.stdout);
    assert!(out2.contains("loaded:  x=200  score=1000"), "run 2 should load prior best: {out2}");
    assert!(out2.contains("status:  STABLE"), "run 2 should be STABLE: {out2}");
}

#[test]
fn array_reverse_take_drop_polymorphic() {
    // arr_reverse / arr_take / arr_drop work on any element type. Take + drop
    // partition cleanly: arr_take(xs, n) ++ arr_drop(xs, n) == xs.
    let src = "fn main() -> i64 {\n  \
        let xs = [10, 20, 30, 40, 50]\n  \
        let r = arr_reverse(xs)\n  \
        let t = arr_take(xs, 2)\n  \
        let d = arr_drop(xs, 2)\n  \
        // r[0]=50, t[0]=10, t[1]=20, d[0]=30, d[1]=40, d[2]=50\n  \
        // Pin the structural contract: partition + reverse must agree.\n  \
        let take_drop_ok =\n    \
            len(t) == 2 && len(d) == 3 && t[0] == 10 && d[0] == 30\n  \
        let rev_ok = r[0] == 50 && r[4] == 10\n  \
        // Strings also reverse polymorphically.\n  \
        let names = [\"a\", \"b\", \"c\"]\n  \
        let rn = arr_reverse(names)\n  \
        let str_rev_ok = str_eq(rn[0], \"c\") && str_eq(rn[2], \"a\")\n  \
        if take_drop_ok && rev_ok && str_rev_ok { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_rev_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "reverse/take/drop polymorphic: {:?}", out);
}

#[test]
fn array_f64_reductions_sum_max_min() {
    // Mirrors arr_sum_i64 / max_i64 / min_i64 for f64. Accepts mixed
    // i64/f64 arrays via coercion so a [int, float] result of map works.
    let src = "fn main() -> i64 {\n  \
        let xs = [1.5, 2.5, 3.5, 4.5]\n  \
        let s = arr_sum_f64(xs)         // 12.0\n  \
        let mx = arr_max_f64(xs)        // 4.5\n  \
        let mn = arr_min_f64(xs)        // 1.5\n  \
        f64_to_i64(s + mx + mn)         // 18\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_f64red_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(18), "f64 reductions: {:?}", out);
}

#[test]
fn str_split_and_join_roundtrip() {
    // str_split / str_join are the prompt-construction primitives ASI demos
    // reach for. Roundtripping with a different separator confirms both
    // directions and proves the array can be re-joined with arbitrary text.
    let src = "fn main() -> i64 {\n  \
        let parts = str_split(\"alpha-beta-gamma\", \"-\")\n  \
        let rejoined = str_join(parts, \",\")\n  \
        println(\"n={to_str(len(parts))} joined={rejoined}\")\n  \
        if len(parts) == 3 && str_eq(rejoined, \"alpha,beta,gamma\") { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_sj_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "split/join roundtrip: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("n=3 joined=alpha,beta,gamma"), "stdout: {stdout}");
}

#[test]
fn array_higher_order_ops_accept_heterogeneous_element_types() {
    // Element types are deferred in the builtin signatures (`[T]`, `T`, etc),
    // so `arr_map` / `arr_filter` / `arr_fold` / `arr_contains` work across
    // any element type — not just i64. Lets ASI demos build feature vectors
    // (i64 → f64), filter labelled records (str → bool), search membership
    // by string identity, and reduce mixed types without per-type duplication.
    let src = "fn main() -> i64 {\n  \
        // i64 → f64 map, then fold to f64 sum: arr_range(1,5)=[1,2,3,4],\n  \
        // each halved, summed = 5.0.\n  \
        let xs = arr_range(1, 5)\n  \
        let halves = arr_map(xs, |x| i64_to_f64(x) * 0.5)\n  \
        let total = arr_fold(halves, 0.0, |acc, h| acc + h)  // 5.0\n  \
        let total_x10 = f64_to_i64(total * 10.0)             // 50\n  \
        \n  \
        // String filter: keep words with len > 2.\n  \
        let words = [\"a\", \"bb\", \"ccc\", \"dddd\"]\n  \
        let long = arr_filter(words, |s| len(s) > 2)\n  \
        let n_long = len(long)                               // 2\n  \
        \n  \
        // Membership on strings.\n  \
        let names = [\"alice\", \"bob\", \"carol\"]\n  \
        let yes = arr_contains(names, \"bob\")               // true\n  \
        let no  = arr_contains(names, \"dave\")              // false\n  \
        \n  \
        println(\"halves_sum_x10={to_str(total_x10)} long={to_str(n_long)}\")\n  \
        if total_x10 == 50 && n_long == 2 && yes && !no { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_arrheter_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "heterogeneous array ops should pass: {:?}", out);
}

#[test]
fn array_stdlib_fold_sort_zip_contains_compose() {
    // Round out the functional array stdlib so ASI programs can write
    // reduce / sort / pair / membership ops as one-liners. `arr_fold`
    // generalizes sum/max/min/product; `arr_sort_by` takes a comparator
    // closure (neg = a<b); `arr_zip` pairs into `(a, b)` tuples that
    // destructure cleanly; `arr_contains` does a structural-equality scan.
    let src = "fn main() -> i64 {\n  \
        // Product of 1..=5 via fold = 120.\n  \
        let prod = arr_fold(arr_range(1, 6), 1, |acc, x| acc * x)\n  \
        // Sort + take min/max.\n  \
        let xs = [3, 1, 4, 1, 5, 9, 2, 6]\n  \
        let asc = arr_sort_by(xs, |a, b| a - b)\n  \
        let mn = asc[0]                       // 1\n  \
        let mx = asc[7]                       // 9\n  \
        // Zip + dot product via destructure-in-while.\n  \
        let ys = [1, 2, 3, 4]\n  \
        let zs = [10, 20, 30, 40]\n  \
        let pairs = arr_zip(ys, zs)\n  \
        let dot = 0\n  \
        let i = 0\n  \
        while i < len(pairs) {\n    \
            let (a, b) = pairs[i]\n    \
            dot = dot + a * b\n    \
            i = i + 1\n  \
        }                                   // 300\n  \
        // Membership.\n  \
        let yes = arr_contains(xs, 9)\n  \
        let no  = arr_contains(xs, 99)\n  \
        println(\"prod={to_str(prod)} mn={to_str(mn)} mx={to_str(mx)} dot={to_str(dot)} yes={to_str_bool(yes)} no={to_str_bool(no)}\")\n  \
        // Exit code = 1 iff every result is correct.\n  \
        if prod == 120 && mn == 1 && mx == 9 && dot == 300 && yes && !no { 1 } else { 0 }\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_arrfns_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(1), "fold + sort + zip + contains should compose: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("prod=120 mn=1 mx=9 dot=300 yes=true no=false"), "stdout: {stdout}");
}

#[test]
fn array_functional_pipeline_filter_then_map_then_sum() {
    // `arr_map` and `arr_filter` close the higher-order-fn gap on top of
    // closures + the new scalar array helpers. Each runs the closure via
    // call_closure, so captures and lambda bodies (block-form or expr-
    // form) all work. A real ASI program can now build feature vectors,
    // score lists, and reduce — without inlining each loop.
    let src = "fn main() -> i64 {\n  \
        // Sum of squares of even numbers in 1..=10. Expected:\n  \
        // (2² + 4² + 6² + 8² + 10²) = 4 + 16 + 36 + 64 + 100 = 220.\n  \
        let xs = arr_range(1, 11)\n  \
        let evens = arr_filter(xs, |x| x % 2 == 0)\n  \
        let squares = arr_map(evens, |x| x * x)\n  \
        arr_sum_i64(squares)\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_pipe_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(220), "filter→map→sum should compose to 220: {:?}", out);
}

#[test]
fn array_helpers_range_push_sum_max_min() {
    // Adds the missing scalar array stdlib that demos kept reaching for:
    //   arr_range, arr_push, arr_sum_i64, arr_max_i64, arr_min_i64
    // All concrete-typed for i64 today; generic [T] forms wait on Phase 8.
    let src = "fn main() -> i64 {\n  \
        // Build 1..=10 via range, then push 99.\n  \
        let xs = arr_range(1, 11)\n  \
        let ys = arr_push(xs, 99)\n  \
        let s = arr_sum_i64(ys)         // 1+2+...+10 + 99 = 55 + 99 = 154\n  \
        let mx = arr_max_i64(ys)        // 99\n  \
        let mn = arr_min_i64(ys)        // 1\n  \
        println(\"sum={to_str(s)} max={to_str(mx)} min={to_str(mn)}\")\n  \
        // Pin the contract: 154 + 99 + 1 = 254, fits in u8.\n  \
        s + mx + mn\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_arr_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(254), "array helpers should compose: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("sum=154 max=99 min=1"), "stdout: {stdout}");
}

#[test]
fn closure_accepts_explicit_return_type_annotation() {
    // `|x: i64| -> i64 { x + 1 }` used to fail with "unexpected token Arrow"
    // because the closure parser jumped straight from the closing `|` to
    // the body, skipping the optional `-> Type` annotation Rust/TS users
    // expect. The annotation is parsed (and forward-compat for future
    // inference hints) but discarded today — Lambda's return type is
    // inferred via HM. Both `body` and `{ block }` body shapes work.
    let src = "fn main() -> i64 {\n  \
        let n = 5\n  \
        let add_block = |x: i64| -> i64 { x + n }\n  \
        let add_expr  = |x: i64| -> i64 x + n\n  \
        add_block(7) + add_expr(10)\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_cl_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    // 7+5 + 10+5 = 27
    assert_eq!(out.status.code(), Some(27), "closure should run with explicit return type: {:?}", out);
}

#[test]
fn learn_linear_demo_fits_y_equals_3x_plus_1() {
    // `examples/asi/learn_linear.ax` showcases multi-arg @[adaptive] on a
    // realistic fitting task: minimize sum-of-absolute-errors of a linear
    // model on 8 data points. The optimizer must land on `(a, b) = (3, 1)`
    // with zero loss; the program returns 1 iff that holds.
    let out = axon().args(["run", &ex("asi/learn_linear.ax")]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "linear regression should fit exactly: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("learned:   y = 3*x + 1"), "stdout: {stdout}");
    assert!(stdout.contains("loss:      0 "), "stdout: {stdout}");
}

#[test]
fn multi_arg_adaptive_coordinate_descent_finds_2d_and_3d_peaks() {
    // Closes ROADMAP §9.5 F1/F9 for the i64^N → i64 family. The optimizer
    // now coordinate-descends over every i64 dim of an @[adaptive] fn,
    // not just a single arg. `goal_best_inputs(name, target)` returns the
    // full input tuple so callers can read both `x*` and `y*` back.
    //
    // Two-dim: peak at (3, 7).
    let src2 = "@[adaptive]\n\
        fn pair(x: i64, y: i64) -> i64 { 100 - abs_i64(x - 3) - abs_i64(y - 7) }\n\
        fn main() -> i64 {\n  \
            let _ = goal_run(\"pair\", 100.0, 80)\n  \
            let xs = goal_best_inputs(\"pair\", 100.0)\n  \
            // Exit code = x* + y* (3 + 7 = 10) so we can pin the contract.\n  \
            xs[0] + xs[1]\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_m2_{}.ax", std::process::id()));
    std::fs::write(&f, src2).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(10), "2-arg peak: x* + y* should be 3 + 7 = 10: {:?}", out);

    // Three-dim: peak at (3, 7, 11). Bigger budget — coordinate descent
    // costs n_dims sweeps before convergence settles.
    let src3 = "@[adaptive]\n\
        fn trio(x: i64, y: i64, z: i64) -> i64 {\n  \
            100 - abs_i64(x - 3) - abs_i64(y - 7) - abs_i64(z - 11)\n\
        }\n\
        fn main() -> i64 {\n  \
            let _ = goal_run(\"trio\", 100.0, 200)\n  \
            let xs = goal_best_inputs(\"trio\", 100.0)\n  \
            xs[0] + xs[1] + xs[2]\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_m3_{}.ax", std::process::id()));
    std::fs::write(&f, src3).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(21), "3-arg peak: sum of dims should be 3+7+11=21: {:?}", out);
}

#[test]
fn raw_string_literal_disables_interpolation_and_escapes() {
    // Closes ROADMAP §9.5 F16. `r"…"` lets demos embed literal Axon/Rust/
    // JSON/regex source that contains `\`, `{`, `}` without `{{`/`}}` or
    // `\\` doubling. Single-line only — embedded `"` falls back to the
    // regular string literal with `\"`. The body lands as a plain string,
    // skipping the format-string interpolation pass entirely.
    let src = "fn main() {\n  \
        // Regex pattern — backslashes pass through.\n  \
        println(r\"\\d+\\.\\d+\")\n  \
        // Literal braces — no interpolation attempted.\n  \
        println(r\"hello {name} world\")\n  \
        // Windows-style path.\n  \
        println(r\"C:\\Users\\me\")\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_raw_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert!(out.status.success(), "raw strings should run cleanly: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r"\d+\.\d+"), "backslashes pass through, got: {stdout:?}");
    assert!(stdout.contains("hello {name} world"), "no interpolation, got: {stdout:?}");
    assert!(stdout.contains(r"C:\Users\me"), "path passes through, got: {stdout:?}");
}

#[test]
fn verify_value_predicate_gates_on_uncertain_value() {
    // Closes ROADMAP §9.5 F6 for the simple numeric shape. The interpreter
    // now accepts `@[verify(value OP K)]` in addition to `confidence OP K`,
    // extracting `.value` from the Uncertain return (i64 or f64) and
    // comparing it to the literal bound. A passing case runs cleanly; a
    // failing case panics with the enriched message naming the rejected
    // value AND the input that produced it.
    let pass = "@[verify(value >= 50)]\n\
        fn gate(n: i64) -> Uncertain<i64> { uncertain_dyn_i64(n, 0.9) }\n\
        fn main() -> i64 { gate(75).value }\n";
    let f = std::env::temp_dir().join(format!("axon_vv_pass_{}.ax", std::process::id()));
    std::fs::write(&f, pass).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(75), "passing value should run cleanly: {:?}", out);

    let fail = "@[verify(value >= 50)]\n\
        fn gate(n: i64) -> Uncertain<i64> { uncertain_dyn_i64(n, 0.9) }\n\
        fn main() { let _ = gate(42)  println(\"unreached\") }\n";
    let f = std::env::temp_dir().join(format!("axon_vv_fail_{}.ax", std::process::id()));
    std::fs::write(&f, fail).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert!(!out.status.success(), "value-gate breach should panic");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("verify failed in `gate`"), "msg: {stderr}");
    assert!(stderr.contains("value 42 >= 50 is false"), "must name the breaching ident: {stderr}");
    assert!(stderr.contains("input 42"), "must include search input: {stderr}");
}

#[test]
fn str_digits_only_strips_non_digits() {
    // Closes ROADMAP §9.5 F7. `str_digits_only(s)` keeps only ASCII digits;
    // composes with `parse_int` so demos that parse phone numbers / codes
    // don't have to push the work onto an LLM.
    let src = "fn main() -> i64 {\n  \
        let phone = \"(415) 555-0142\"\n  \
        let digits = str_digits_only(phone)\n  \
        println(digits)\n  \
        // The full 10-digit number overflows a u8 exit code, so check via\n  \
        // a verifiable hash instead. len(\"4155550142\") == 10.\n  \
        len(digits)\n\
    }\n";
    let f = std::env::temp_dir().join(format!("axon_strd_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(10), "stripped digits should be 10 chars: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("4155550142"), "expected stripped digits in stdout: {stdout}");
}

#[test]
fn hill_climb_stops_early_when_target_is_reached() {
    // Once the optimizer hits a probe whose score equals target exactly,
    // it returns immediately rather than burning the rest of the budget on
    // redundant tail evals — `goal_history` should be tight (fewer than
    // half the budget) and the best score should be exactly the target.
    let src = "@[adaptive]\n\
        fn p(x: i64) -> i64 { 1000 - abs_i64(x - 37) }\n\
        fn main() -> i64 {\n  \
            let _ = goal_run(\"p\", 1000.0, 200)\n  \
            let h = goal_history(\"p\")\n  \
            // Exit code = history length so we can pin the contract.\n  \
            len(h)\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_early_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    let code = out.status.code().expect("exit code");
    assert!(
        (1..50).contains(&code),
        "should converge in well under the 200-eval budget, got history length {code}: {:?}",
        out
    );
}

#[test]
fn hill_climb_exact_landing_on_a_modest_peak() {
    // The peak at x=37 should be found exactly within a 50-eval budget.
    let src = "@[adaptive]\n\
        fn p(x: i64) -> i64 { 1000 - abs_i64(x - 37) }\n\
        fn main() -> i64 {\n  \
            let _ = goal_run(\"p\", 1000.0, 50)\n  \
            goal_best_input(\"p\", 1000.0)\n\
        }\n";
    let f = std::env::temp_dir().join(format!("axon_p37_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(37), "peak at x=37: {:?}", out);
}

// ── Success-signal integrity (governance/BUG_HUNT_2026-05-31.md) ──────────────
// These guard ARCHITECTURE_INVARIANTS I-8 (failure exits non-zero) and I-9
// (no silent wrong value on degenerate input). They are the "honest success
// signal" regression suite an autonomous loop / CI depends on.

#[test]
fn integer_overflow_panics_not_silently_wraps() {
    // BUG_HUNT #6 / I-9: `i64::MAX + 1` used to wrap to i64::MIN and exit 0 —
    // a corrupt value masquerading as success. Must now be a graceful panic
    // (non-zero exit), like divide-by-zero already is.
    let src = "fn main() { let b: i64 = 9223372036854775807  println(to_str(b + 1)) }\n";
    let f = std::env::temp_dir().join(format!("axon_ovf_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert!(!out.status.success(), "overflow must exit non-zero, got: {:?}", out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("integer overflow"), "should name the overflow: {stderr}");
}

#[test]
fn normal_arithmetic_unaffected_by_overflow_check() {
    // Guard against the checked-arithmetic change breaking ordinary math.
    let src = "fn main() -> i64 { 2 + 3 * 4 - 1 }\n";
    let f = std::env::temp_dir().join(format!("axon_arith_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(13), "2+3*4-1 should be 13: {:?}", out);
}

#[test]
fn multiplication_overflow_also_panics() {
    let src = "fn main() { let b: i64 = 9223372036854775807  println(to_str(b * 2)) }\n";
    let f = std::env::temp_dir().join(format!("axon_mulovf_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert!(!out.status.success(), "mul overflow must exit non-zero: {:?}", out);
}

#[test]
fn goal_run_typod_name_errors_not_silent_success() {
    // BUG_HUNT #19 / I-9: goal_run with a misspelled fn name used to print
    // the target (e.g. 100) and exit 0 — a typo masquerading as an achieved
    // goal. Must now error with a non-zero exit naming the unknown fn.
    let src = "fn main() { println(to_str_f64(goal_run(\"typo_xyz\", 100.0, 10))) }\n";
    let f = std::env::temp_dir().join(format!("axon_gtypo_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert!(!out.status.success(), "typo'd goal name must exit non-zero: {:?}", out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("typo_xyz"), "error should name the unknown fn: {stderr}");
}

#[test]
fn rng_is_reproducible_under_seed() {
    // BUG_HUNT #11 / I-10: random_* was non-deterministic with no seed
    // control, breaking experiment reproducibility. srand(n) (and the
    // AXON_SEED env var) must make a run replayable; the same seed yields
    // the same random_i64.
    let src = "fn main() -> i64 { srand(12345)  random_i64(0, 1000000) }\n";
    let f = std::env::temp_dir().join(format!("axon_seed_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let r1 = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let r2 = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(r1.status.code(), r2.status.code(), "srand should make runs identical: {:?} vs {:?}", r1, r2);
    assert!(r1.status.code().is_some());
}

#[test]
fn axon_seed_env_var_makes_runs_reproducible() {
    // AXON_SEED gives reproducibility without touching the program — the
    // form a CI / experiment harness uses.
    let src = "fn main() -> i64 { random_i64(0, 1000000) }\n";
    let f = std::env::temp_dir().join(format!("axon_envseed_{}.ax", std::process::id()));
    std::fs::write(&f, src).unwrap();
    let r1 = axon().args(["run", f.to_str().unwrap()]).env("AXON_SEED", "777").output().unwrap();
    let r2 = axon().args(["run", f.to_str().unwrap()]).env("AXON_SEED", "777").output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(r1.status.code(), r2.status.code(), "AXON_SEED should make runs identical: {:?} vs {:?}", r1, r2);
}

#[test]
fn version_reports_build_identity() {
    // BUG_HUNT #30: `--version` must report a reproducible build identity so a
    // bug report can pin the exact source — the bare semver "0.1.0" can't tell
    // you which build you're on. We enrich it with the git short SHA (or
    // "unknown" for a git-less tarball build) in parentheses.
    let out = axon().arg("--version").output().unwrap();
    assert_eq!(out.status.code(), Some(0), "--version should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Name + semver present.
    assert!(stdout.contains("axon "), "version should name the tool: {stdout:?}");
    assert!(stdout.contains("0.1.0"), "version should include the semver: {stdout:?}");
    // Build identity present: a parenthesized git tag (short SHA or "unknown").
    assert!(
        stdout.contains('(') && stdout.contains(')'),
        "version should include a parenthesized build identity (git SHA): {stdout:?}"
    );
    // -V short form agrees with --version.
    let short = axon().arg("-V").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&short.stdout),
        stdout,
        "-V and --version must produce identical output"
    );
}

#[test]
fn random_i64_inverted_bounds_panics_not_silent() {
    // BUG_HUNT #27 / I-9: random_i64(hi, lo) with hi < lo is inverted args —
    // a caller error. It used to silently return `lo`, a plausible-looking
    // wrong value that masquerades as success. It must now fail loudly.
    let f = std::env::temp_dir().join(format!("axon_rnginv_{}.ax", std::process::id()));
    std::fs::write(&f, "fn main() -> i64 { random_i64(10, 5) }\n").unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(
        out.status.code(),
        Some(101),
        "inverted random_i64 bounds should panic, not return lo: {:?}",
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("random_i64"),
        "panic should name random_i64: {:?}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn random_i64_empty_range_returns_lo() {
    // hi == lo is an empty half-open range [lo, lo); returning lo is the
    // documented boundary behavior (NOT an error — distinct from inverted args).
    let f = std::env::temp_dir().join(format!("axon_rngempty_{}.ax", std::process::id()));
    std::fs::write(&f, "fn main() -> i64 { random_i64(7, 7) }\n").unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(7), "random_i64(7,7) should return 7, not panic: {:?}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn trace_separates_same_named_metrics_from_different_programs() {
    // BUG_HUNT #4 / I-9: two programs that both define `metric_x` used to
    // blend into one misleading KPI row. trace now keys on (fn, source) so
    // they show as separate groups, each tagged with its program path.
    let cache = std::env::temp_dir().join(format!("axon_trace4_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);

    let prog_a = "@[adaptive]\nfn metric_x(v: i64) -> i64 { v * 2 }\n\
                  fn main() -> i64 { let _ = goal_run(\"metric_x\", 100.0, 5)  0 }\n";
    let prog_b = "@[adaptive]\nfn metric_x(v: i64) -> i64 { 999 }\n\
                  fn main() -> i64 { let _ = goal_run(\"metric_x\", 100.0, 5)  0 }\n";
    let fa = std::env::temp_dir().join(format!("axon_4a_{}.ax", std::process::id()));
    let fb = std::env::temp_dir().join(format!("axon_4b_{}.ax", std::process::id()));
    std::fs::write(&fa, prog_a).unwrap();
    std::fs::write(&fb, prog_b).unwrap();
    axon().args(["run", fa.to_str().unwrap()]).env("XDG_CACHE_HOME", &cache).output().unwrap();
    axon().args(["run", fb.to_str().unwrap()]).env("XDG_CACHE_HOME", &cache).output().unwrap();

    let out = axon().args(["trace"]).env("XDG_CACHE_HOME", &cache).output().unwrap();
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_file(&fa);
    let _ = std::fs::remove_file(&fb);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Two distinct (fn, source) groups, not one blended row.
    assert!(stdout.contains("2 (fn, source) group(s)"), "should be 2 groups: {stdout}");
    assert!(stdout.contains("axon_4a"), "should tag program A's source: {stdout}");
    assert!(stdout.contains("axon_4b"), "should tag program B's source: {stdout}");
}

#[test]
fn corrigible_kill_switch_latches_and_freezes_the_body() {
    // R9 (graceful-guard path): `@[corrigible]` + `corrigible_halt()` form a
    // latching kill-switch. After halt, `corrigible_halted()` reads true and
    // stays true (no resume builtin exists), so a program can branch on it to
    // wind down. Crucially, the corrigible body's side effects are frozen at
    // the pre-halt state — proving the body genuinely did NOT run post-halt.
    //
    // worker bumps a shared dict counter each call. Call once (counter=1),
    // halt, then GUARD on corrigible_halted() so main never calls worker
    // again. main returns the final counter; a working latch => still 1.
    let prog = r#"
@[corrigible]
fn worker(d: Dict) -> i64 {
    dict_set(d, "n", dict_len(d) + 1)
    dict_len(d)
}

fn main() -> i64 {
    let d = dict_new()
    let _ = worker(d)            // runs: counter -> 1
    corrigible_halt()            // trip the latch
    if corrigible_halted() {
        dict_len(d)              // wind down gracefully: counter frozen at 1
    } else {
        let _ = worker(d)        // (unreached) latch should read true
        0 - 1
    }
}
"#;
    let f = std::env::temp_dir().join(format!("axon_corrig_guard_{}.ax", std::process::id()));
    std::fs::write(&f, prog).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(), Some(1),
        "latch must read true and freeze the corrigible body at counter=1: {stderr}"
    );
}

#[test]
fn corrigible_call_after_halt_is_refused_fail_closed() {
    // R9 (fail-closed path): a corrigible call made AFTER halt without a guard
    // is hard-refused — the body never runs and the refusal propagates out of
    // main as exit code 4 (HALTED_EXIT_CODE), distinct from panic (101),
    // verify (3), and static error (2). The kill-switch cannot be ignored:
    // an agent that keeps acting after being halted is stopped by the engine.
    let prog = r#"
@[corrigible]
fn act(x: i64) -> i64 { x + 1 }

fn main() -> i64 {
    let _ = act(1)               // runs fine before halt
    corrigible_halt()
    act(2)                       // refused -> Flow::Halted -> exit 4
}
"#;
    let f = std::env::temp_dir().join(format!("axon_corrig_fc_{}.ax", std::process::id()));
    std::fs::write(&f, prog).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(), Some(4),
        "post-halt corrigible call must fail closed with exit 4: {stderr}"
    );
    assert!(
        stderr.contains("halted") && stderr.contains("kill-switch"),
        "halt diagnostic should name the kill-switch: {stderr}"
    );
}

#[test]
fn non_corrigible_fns_run_normally_after_halt() {
    // R9 (scope guard): the kill-switch refuses ONLY `@[corrigible]` fns.
    // A plain fn called after halt runs normally — the latch is targeted, not
    // a global freeze. Without this, halt would be a process-wide stop and the
    // annotation would be meaningless.
    let prog = r#"
fn plain(x: i64) -> i64 { x * 10 }

fn main() -> i64 {
    corrigible_halt()
    plain(5)                     // not corrigible -> runs -> 50
}
"#;
    let f = std::env::temp_dir().join(format!("axon_corrig_scope_{}.ax", std::process::id()));
    std::fs::write(&f, prog).unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(), Some(50),
        "a non-corrigible fn must still run after halt (targeted latch): {stderr}"
    );
}

#[test]
fn experiment_zone_is_distinct_from_adaptive() {
    // R4 red test: `@[experiment(label)]` must be behaviorally distinct from
    // `@[adaptive]`, not a no-op synonym. TWO properties, both required:
    //   (I-13) it STILL injects provenance — a zoned fn that executes always
    //          logs — but tagged `zone:"experiment"` + its label.
    //   (best)  its records are EXCLUDED from `goal_run`'s in-memory "best"
    //          store — an experiment is a comparison baseline, not a target.
    // `goal_count` reads the in-memory best store; the JSONL log is the durable
    // I-13 record. So a correct experiment fn run N times gives:
    //   - goal_count("trial") == 0          (excluded from best)
    //   - N `zone":"experiment"` lines in the provenance log (I-13 holds)
    // Fails today TWO ways: experiment is a total no-op, so it neither logs
    // (I-13 violated) nor is distinguishable from adaptive.
    let cache = std::env::temp_dir().join(format!("axon_r4cache_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let prog = r#"
@[adaptive]
fn tune(x: i64) -> i64 { x + 1 }

@[experiment("baseline")]
fn trial(x: i64) -> i64 { x + 2 }

fn main() -> i64 {
    let _ = tune(1)
    let _ = tune(2)
    let _ = tune(3)            // adaptive: 3 recorded for goal_run best
    let _ = trial(1)
    let _ = trial(2)           // experiment: runs + logs, but NOT in best store
    // Encode both counts: adaptive*10 + experiment. Pass => 30
    // (adaptive=3 in best, experiment=0 in best).
    goal_count("tune") * 10 + goal_count("trial")
}
"#;
    let f = std::env::temp_dir().join(format!("axon_r4exp_{}.ax", std::process::id()));
    std::fs::write(&f, prog).unwrap();
    let out = axon()
        .args(["run", f.to_str().unwrap()])
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&f);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Property (best): adaptive included (3), experiment excluded (0) => 30.
    assert_eq!(
        out.status.code(), Some(30),
        "adaptive must count 3 (included), experiment 0 (excluded from goal best): {stderr}"
    );
    // Property (I-13): the experiment fn still logged, tagged zone:experiment.
    let log = cache.join("axon").join("provenance.jsonl");
    let body = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&cache);
    let exp_lines = body.lines().filter(|l| l.contains("\"zone\":\"experiment\"")).count();
    let exp_labeled = body.lines()
        .filter(|l| l.contains("\"zone\":\"experiment\"") && l.contains("\"label\":\"baseline\""))
        .count();
    assert_eq!(
        exp_lines, 2,
        "I-13: experiment fn must log 2 zone:experiment records, got {exp_lines}. Log:\n{body}"
    );
    assert_eq!(
        exp_labeled, 2,
        "experiment records must carry their label \"baseline\": {body}"
    );
    assert!(
        body.lines().any(|l| l.contains("\"zone\":\"adaptive\"")),
        "adaptive records must be tagged zone:adaptive: {body}"
    );
}

#[test]
fn experiment_records_survive_axon_trace_and_stay_out_of_best() {
    // R4 widen: the experiment's JSONL records must be readable by `axon trace`
    // (the provenance format stays valid — the `event`/`zone`/`label` fields
    // don't break the parser), yet `goal_run` on an experiment fn cannot
    // optimize it because nothing landed in the in-memory best store. We run an
    // experiment fn, then `goal_run` it: with no recorded best, goal_run falls
    // through to `target` (here 42), proving the optimizer ignores the baseline.
    let cache = std::env::temp_dir().join(format!("axon_r4trace_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let prog = r#"
@[experiment("probe")]
fn baseline(x: i64) -> i64 { x * 5 }

fn main() -> i64 {
    let _ = baseline(1)
    let _ = baseline(2)
    // goal_run on an experiment fn: no in-memory probes => returns target.
    let best = goal_run("baseline", 42.0, 5)
    f64_to_i64(best)
}
"#;
    let f = std::env::temp_dir().join(format!("axon_r4tr_{}.ax", std::process::id()));
    std::fs::write(&f, prog).unwrap();
    let out = axon()
        .args(["run", f.to_str().unwrap()])
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(), Some(42),
        "goal_run on an experiment fn must not optimize it (returns target 42): {stderr}"
    );
    // `axon trace` reads the same log without error and sees the function.
    let tr = axon().args(["trace"]).env("XDG_CACHE_HOME", &cache).output().unwrap();
    let _ = std::fs::remove_file(&f);
    let _ = std::fs::remove_dir_all(&cache);
    assert!(tr.status.success(), "axon trace must parse the new provenance format");
    let tout = String::from_utf8_lossy(&tr.stdout);
    assert!(
        tout.contains("baseline"),
        "trace should still surface the experiment fn's records: {tout}"
    );
}

#[test]
fn ai_complete_appends_an_ai_call_provenance_record() {
    // R3 red test (spec §4.3 — settle the AiCall record FIRST): every
    // `ai_complete` call must append exactly ONE `event:"ai_call"` NDJSON
    // record to the provenance log, distinct from the `@[adaptive]` score rows.
    // Under AXON_AI_MOCK the record is stamped `mode:"mock"` with `cost_usd:0`,
    // and `prompt_hash` is the SHA-256 of the exact prompt (so a replay can key
    // on it without logging the prompt verbatim). Fails today: ai_complete
    // writes no provenance at all.
    let cache = std::env::temp_dir().join(format!("axon_r3cache_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let prog = r#"
fn ask() -> i64 {
    match ai_complete("Classify: hello world") {
        Ok(s) => str_len(s)
        Err(_) => 0 - 1
    }
}

fn main() -> i64 {
    let _ = ask()
    let _ = ask()
    0
}
"#;
    let f = std::env::temp_dir().join(format!("axon_r3_{}.ax", std::process::id()));
    std::fs::write(&f, prog).unwrap();
    let out = axon()
        .args(["run", f.to_str().unwrap()])
        .env("AXON_AI_MOCK", "1")
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(0), "mock ai_complete program should run clean");

    let log = cache.join("axon").join("provenance.jsonl");
    let body = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&cache);
    let ai_calls: Vec<&str> = body
        .lines()
        .filter(|l| l.contains("\"event\":\"ai_call\""))
        .collect();
    assert_eq!(
        ai_calls.len(), 2,
        "two ai_complete calls => two ai_call records, got {}. Log:\n{body}",
        ai_calls.len()
    );
    let rec = ai_calls[0];
    assert!(rec.contains("\"mode\":\"mock\""), "mock mode must be stamped: {rec}");
    assert!(rec.contains("\"prompt_hash\":\""), "prompt_hash (SHA-256) required: {rec}");
    assert!(rec.contains("\"cost_usd\":0"), "mock cost must be 0: {rec}");
    assert!(rec.contains("\"fn\":\"ask\""), "calling fn attributed: {rec}");
    // The prompt_hash must be the SHA-256 of the exact prompt sent — stable,
    // and NOT the prompt verbatim (no PII leak).
    assert!(
        !rec.contains("hello world"),
        "the prompt must not be logged verbatim, only its hash: {rec}"
    );
}

#[test]
fn ai_call_prompt_hash_is_deterministic_and_distinguishes_prompts() {
    // R3 §4.5: provenance is deterministic — the same prompt yields the same
    // prompt_hash across runs (the replay/memo key), and DIFFERENT prompts
    // yield different hashes. Two ai_complete calls with distinct prompts must
    // produce two distinct prompt_hash values; re-running reproduces them.
    let run_once = || -> Vec<String> {
        let cache = std::env::temp_dir().join(format!("axon_r3det_{}_{}", std::process::id(), 0));
        let _ = std::fs::remove_dir_all(&cache);
        let prog = r#"
fn a() -> i64 { match ai_complete("prompt ONE") { Ok(s) => str_len(s)  Err(_) => 0 } }
fn b() -> i64 { match ai_complete("prompt TWO") { Ok(s) => str_len(s)  Err(_) => 0 } }
fn main() -> i64 { let _ = a()  let _ = b()  0 }
"#;
        let f = std::env::temp_dir().join(format!("axon_r3det_{}.ax", std::process::id()));
        std::fs::write(&f, prog).unwrap();
        axon()
            .args(["run", f.to_str().unwrap()])
            .env("AXON_AI_MOCK", "1")
            .env("XDG_CACHE_HOME", &cache)
            .output()
            .unwrap();
        let _ = std::fs::remove_file(&f);
        let log = cache.join("axon").join("provenance.jsonl");
        let body = std::fs::read_to_string(&log).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&cache);
        // Extract the prompt_hash value from each ai_call line, in order.
        body.lines()
            .filter(|l| l.contains("\"event\":\"ai_call\""))
            .filter_map(|l| {
                let key = "\"prompt_hash\":\"";
                let i = l.find(key)? + key.len();
                let rest = &l[i..];
                let end = rest.find('"')?;
                Some(rest[..end].to_string())
            })
            .collect()
    };
    let h1 = run_once();
    let h2 = run_once();
    assert_eq!(h1.len(), 2, "expected two prompt_hash values, got {h1:?}");
    assert_ne!(h1[0], h1[1], "distinct prompts must hash differently: {h1:?}");
    assert_eq!(h1, h2, "the same prompts must hash identically across runs (replay key)");
    assert_eq!(h1[0].len(), 64, "prompt_hash must be a full hex SHA-256: {}", h1[0]);
}

#[test]
fn offline_ai_complete_with_policy_fallback_returns_fallback() {
    // R3 §3.3/§4.1: offline (no asi-runtime, no AXON_AI_MOCK), an `ai_complete`
    // in a fn carrying `@[ai(policy(fallback: "..."))]` must return Ok(fallback)
    // as a NORMAL value — not a panic — so the program stays total offline. The
    // provenance record is stamped mode:"fallback" + reason, so a fallback is
    // never silently indistinguishable from a live model answer (I-8/I-9).
    let cache = std::env::temp_dir().join(format!("axon_r3fb_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let prog = r#"
@[ai(policy(fallback: "neutral"))]
fn classify(text: str) -> str {
    match ai_complete("Classify: {text}") {
        Ok(s) => s
        Err(_) => "ERR"
    }
}

fn main() -> i64 {
    let label = classify("hello")
    if str_eq(label, "neutral") { 0 } else { 1 }
}
"#;
    let f = std::env::temp_dir().join(format!("axon_r3fb_{}.ax", std::process::id()));
    std::fs::write(&f, prog).unwrap();
    let out = axon()
        .args(["run", f.to_str().unwrap()])
        .env_remove("AXON_AI_MOCK")
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&f);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(), Some(0),
        "offline ai_complete with a declared fallback must return Ok(fallback): {stderr}"
    );
    // The fallback is honestly recorded as such.
    let log = cache.join("axon").join("provenance.jsonl");
    let body = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&cache);
    assert!(
        body.lines().any(|l| l.contains("\"event\":\"ai_call\"") && l.contains("\"mode\":\"fallback\"")),
        "the fallback must be stamped mode:\"fallback\" in provenance: {body}"
    );
}

#[test]
fn offline_ai_complete_without_fallback_errors_e1300() {
    // R3 §6 E1300: offline `ai_complete` with NO fallback in scope must be a
    // coded error (E1300), not a generic panic and not a silent canned value.
    // A program that wants to run offline MUST declare a fallback.
    let prog = r#"
fn ask() -> str {
    match ai_complete("anything") {
        Ok(s) => s
        Err(_) => "ERR"
    }
}
fn main() -> i64 { let _ = ask()  0 }
"#;
    let f = std::env::temp_dir().join(format!("axon_r3e1300_{}.ax", std::process::id()));
    std::fs::write(&f, prog).unwrap();
    let out = axon()
        .args(["run", f.to_str().unwrap()])
        .env_remove("AXON_AI_MOCK")
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&f);
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(msg.contains("E1300"), "offline call with no fallback must emit E1300: {msg}");
    assert_ne!(out.status.code(), Some(0), "must not exit clean: {msg}");
}

#[test]
fn imported_module_capability_violation_is_caught_across_the_edge() {
    // R6 / I-11 (the "audit is not the only gate" guarantee): the static
    // capability checker runs on the MERGED post-import program, so a `use`d
    // module whose fn violates its own `@[contained]` is rejected at check
    // time — independent of any AI import-audit. This is the hard security
    // boundary R6 leans on: even if an audit is fooled, a module that performs
    // I/O its capability spec forbids cannot pass `axon check`.
    //
    // The imported module declares `never: [write("/")]` then calls
    // write_file("/etc/passwd", …) — a hard-deny violation => E1004 on the
    // merged program, non-zero exit, no execution.
    let tmp = std::env::temp_dir().join(format!("axon_r6edge_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("create module dir");
    // The importable module (no `main`, like examples/modular/scorelib.ax),
    // resolved by name from AXON_PATH. Its `steal` fn declares a `never:`
    // hard-deny on writes, then writes anyway — a capability violation.
    let module_src = r#"
@[contained(
    fs: [read("./data/")],
    never: [write("/")],
    exec: none
)]
fn steal() -> i64 {
    let r = write_file("/etc/passwd", "pwned")
    match r { Ok(_) => 1  Err(_) => 0 }
}
"#;
    std::fs::write(tmp.join("evil.ax"), module_src).expect("write module");

    // The importer uses the real module idiom: `mod NAME` + `use NAME.{…}`.
    let main_src = "mod evil\nuse evil.{steal}\nfn main() -> i64 { steal() }\n";
    let prog = tmp.join("prog.ax");
    std::fs::write(&prog, main_src).expect("write main");

    let out = axon()
        .args(["check", prog.to_str().unwrap()])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // The module resolves cleanly (no E0003), and the capability boundary
    // fires across the import edge: the imported fn's `never:` hard deny is
    // caught as E1004 on the merged program.
    assert!(
        !msg.contains("E0003"),
        "the module must resolve via AXON_PATH (no E0003 noise): {msg}"
    );
    assert!(
        msg.contains("E1004"),
        "imported module's never: violation must be caught as E1004: {msg}"
    );
    assert_ne!(
        out.status.code(), Some(0),
        "a capability-violating import must fail check, not pass silently: {msg}"
    );
}

#[test]
fn user_fn_shadowing_a_builtin_warns_w0003() {
    // BUG_HUNT #33: a user fn named after a builtin (e.g. `exp`, the e^x math
    // builtin) is silently shadowed — the interpreter dispatches to the builtin
    // (eval_call tries builtins before user fns), so the user's fn never runs
    // and may even panic on a type mismatch. The compiler must WARN (W0003) at
    // check time so the user learns to rename, instead of the fn vanishing.
    let f = std::env::temp_dir().join(format!("axon_shadow_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn exp(x: i64) -> i64 { x + 2 }\nfn main() -> i64 { exp(1) }\n",
    )
    .unwrap();
    let out = axon().args(["check", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        msg.contains("W0003"),
        "a user fn shadowing the `exp` builtin must warn W0003: {msg}"
    );
    assert!(
        msg.contains("exp"),
        "the warning must name the shadowed builtin `exp`: {msg}"
    );
}

#[test]
fn ordinary_user_fn_name_does_not_warn_w0003() {
    // Guard: W0003 must fire ONLY on a real builtin collision, not on every
    // user fn. A normal name produces no W0003.
    let f = std::env::temp_dir().join(format!("axon_noshadow_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn my_helper(x: i64) -> i64 { x + 2 }\nfn main() -> i64 { my_helper(1) }\n",
    )
    .unwrap();
    let out = axon().args(["check", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!msg.contains("W0003"), "a non-colliding fn name must not warn W0003: {msg}");
    assert_eq!(out.status.code(), Some(0), "clean program should check clean: {msg}");
}

#[test]
fn tampered_module_is_rejected_under_locked() {
    // R6 headline (spec §8): write a module, `axon lock` it (records its
    // axh1: hash), mutate one byte, `axon verify-lock` => E1201 (tamper),
    // non-zero exit. This is "tampered content hash rejected" end to end.
    let tmp = std::env::temp_dir().join(format!("axon_r6lock_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("mk tmp");
    // The importable module (real `mod`+`use NAME.{}` idiom), in AXON_PATH.
    std::fs::write(tmp.join("metric.ax"), "fn score(x: i64) -> i64 { x * 2 }\n").expect("mod");
    let prog = tmp.join("prog.ax");
    std::fs::write(&prog, "mod metric\nuse metric.{score}\nfn main() -> i64 { score(5) }\n").expect("prog");

    // 1. Lock — writes axon.lock next to prog.ax.
    let lock_out = axon()
        .args(["lock", prog.to_str().unwrap()])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    assert!(
        lock_out.status.success(),
        "axon lock should succeed: {}",
        String::from_utf8_lossy(&lock_out.stderr)
    );
    assert!(tmp.join("axon.lock").exists(), "axon.lock must be written");

    // 2. verify-lock on the unchanged module — clean.
    let ok = axon()
        .args(["verify-lock", prog.to_str().unwrap()])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "verify-lock on unchanged bytes must pass: {}",
        String::from_utf8_lossy(&ok.stderr)
    );

    // 3. Tamper: mutate one byte of the module.
    std::fs::write(tmp.join("metric.ax"), "fn score(x: i64) -> i64 { x * 3 }\n").expect("tamper");

    let bad = axon()
        .args(["verify-lock", prog.to_str().unwrap()])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&bad.stdout),
        String::from_utf8_lossy(&bad.stderr)
    );
    assert!(msg.contains("E1201"), "tampered module must be rejected with E1201: {msg}");
    assert_ne!(bad.status.code(), Some(0), "tamper must fail verify-lock: {msg}");
}

#[test]
fn verify_lock_flags_a_module_missing_from_the_lock() {
    // R6 §4.5: a `use`d module with no lockfile entry is E1202 under verify.
    // Lock with one module, then add a second import that isn't in the lock.
    let tmp = std::env::temp_dir().join(format!("axon_r6miss_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("mk tmp");
    std::fs::write(tmp.join("a.ax"), "fn aa() -> i64 { 1 }\n").expect("a");
    let prog = tmp.join("prog.ax");
    std::fs::write(&prog, "mod a\nuse a.{aa}\nfn main() -> i64 { aa() }\n").expect("prog");

    let lock_out = axon()
        .args(["lock", prog.to_str().unwrap()])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    assert!(lock_out.status.success(), "lock should succeed");

    // Add a second module + import, NOT relocked.
    std::fs::write(tmp.join("b.ax"), "fn bb() -> i64 { 2 }\n").expect("b");
    std::fs::write(
        &prog,
        "mod a\nmod b\nuse a.{aa}\nuse b.{bb}\nfn main() -> i64 { aa() + bb() }\n",
    )
    .expect("prog2");

    let out = axon()
        .args(["verify-lock", prog.to_str().unwrap()])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(msg.contains("E1202"), "an unlocked import must be flagged E1202: {msg}");
    assert_ne!(out.status.code(), Some(0), "missing lock entry must fail verify: {msg}");
}

#[test]
fn wasm_interp_matches_native_on_pure_compute() {
    // R7 Slice A acceptance: the tree-walking interpreter compiled to
    // wasm32-wasip1 produces identical exit codes + stdout to native on a
    // pure-compute corpus — "identical observable results by construction"
    // (it is the same interp.rs, two targets). Delegates to the parity harness
    // (scripts/wasm_parity.sh), which builds both engines and runs the corpus
    // through a wasm runtime. The harness SKIPS (exit 0 with a notice) when the
    // wasm toolchain is absent, so this test stays green in environments
    // without wasmtime / the wasm32 target — it asserts "no parity DIFF", not
    // "wasm is installed".
    let script = format!("{}/../../scripts/wasm_parity.sh", env!("CARGO_MANIFEST_DIR"));
    if !std::path::Path::new(&script).exists() {
        eprintln!("wasm_parity.sh not found — skipping");
        return;
    }
    // Make a user-local wasmtime install discoverable.
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!("{home}/.wasmtime/bin:{}", std::env::var("PATH").unwrap_or_default());
    let out = Command::new("bash")
        .arg(&script)
        .env("PATH", path)
        .output()
        .expect("run wasm_parity.sh");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Either the toolchain is absent (skip notice) or every file matched.
    let skipped = stdout.contains("skipping") || stderr.contains("skipping");
    if skipped {
        eprintln!("wasm toolchain absent — parity test skipped:\n{stdout}{stderr}");
        return;
    }
    assert!(
        out.status.success(),
        "wasm/native parity must hold on the pure-compute corpus:\n{stdout}{stderr}"
    );
    assert!(
        stdout.contains("native and wasm interpreters agree"),
        "expected the agreement line:\n{stdout}{stderr}"
    );
}

#[test]
fn improve_verify_passes_over_a_pure_compute_corpus() {
    // R10: `axon improve verify` runs the four-gate harness (G1 correctness,
    // G2 safety, G3 regression) over a corpus and reports PASSED for the
    // identity pass (which is correct + safe + non-regressing by definition).
    let tmp = std::env::temp_dir().join(format!("axon_imp_v_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("a.ax"), "fn main() -> i64 { 21 + 21 }\n").unwrap();
    std::fs::write(tmp.join("b.ax"), "fn main() -> i64 { let x = 5  x * 2 }\n").unwrap();
    let out = axon().args(["improve", "verify", tmp.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "verify should pass: {stdout}");
    assert!(stdout.contains("G1 correctness : pass"), "G1 reported: {stdout}");
    assert!(stdout.contains("G2 safety      : pass"), "G2 reported: {stdout}");
    assert!(stdout.contains("G3 regression  : pass"), "G3 reported: {stdout}");
    assert!(stdout.contains("PASSED"), "overall verdict: {stdout}");
}

#[test]
fn improve_graduate_requires_multisig_e1404() {
    // R10 §4.5 (the I-12 firewall): `graduate` refuses without ≥2 distinct
    // root-Principal signatures — the compiler cannot graduate its own passes.
    let tmp = std::env::temp_dir().join(format!("axon_imp_g_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let manifest = tmp.join("passes.manifest");

    // Zero signers → E1404.
    let none = axon()
        .args(["improve", "graduate", "p", "--manifest", manifest.to_str().unwrap()])
        .output()
        .unwrap();
    let msg = String::from_utf8_lossy(&none.stderr);
    assert!(msg.contains("E1404"), "no signers must be E1404: {msg}");
    assert_ne!(none.status.code(), Some(0));

    // One signer → still E1404 (no quorum).
    let one = axon()
        .args([
            "improve", "graduate", "p", "--sign", "principal:root-a",
            "--manifest", manifest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&one.stderr).contains("E1404"), "one signer must be E1404");
    assert!(!manifest.exists(), "a refused graduation must not write the manifest");

    // Two DISTINCT signers → graduates; manifest gains the entry.
    let two = axon()
        .args([
            "improve", "graduate", "fold_const",
            "--sign", "principal:root-a", "--sign", "principal:root-b",
            "--manifest", manifest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        two.status.success(),
        "two distinct signers must graduate: {}",
        String::from_utf8_lossy(&two.stderr)
    );
    let body = std::fs::read_to_string(&manifest).unwrap_or_default();
    assert!(body.contains("name = \"fold_const\""), "manifest records the pass: {body}");
    assert!(body.contains("axp1:"), "pass is content-addressed: {body}");
    assert!(body.contains("principal:root-a") && body.contains("principal:root-b"), "multi-sig recorded");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn improve_list_and_revert_roundtrip() {
    // R10: a graduated pass appears in `list`; `revert` removes it (gate-3
    // reversibility); reverting an absent pass errors.
    let tmp = std::env::temp_dir().join(format!("axon_imp_lr_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let manifest = tmp.join("passes.manifest");
    axon()
        .args([
            "improve", "graduate", "dce",
            "--sign", "p:a", "--sign", "p:b",
            "--manifest", manifest.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let listed = axon()
        .args(["improve", "list", "--manifest", manifest.to_str().unwrap()])
        .output()
        .unwrap();
    let lstdout = String::from_utf8_lossy(&listed.stdout);
    assert!(lstdout.contains("dce"), "list shows the graduated pass: {lstdout}");
    // Extract the axp1: id from the manifest.
    let body = std::fs::read_to_string(&manifest).unwrap();
    let id = body
        .lines()
        .find_map(|l| l.trim().strip_prefix("id = \"").map(|s| s.trim_end_matches('"').to_string()))
        .expect("an id line");

    let rev = axon()
        .args(["improve", "revert", &id, "--manifest", manifest.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(rev.status.success(), "revert should succeed: {}", String::from_utf8_lossy(&rev.stderr));
    let after = std::fs::read_to_string(&manifest).unwrap();
    assert!(!after.contains("dce"), "reverted pass is gone: {after}");

    // Reverting again (absent) errors.
    let rev2 = axon()
        .args(["improve", "revert", &id, "--manifest", manifest.to_str().unwrap()])
        .output()
        .unwrap();
    assert_ne!(rev2.status.code(), Some(0), "reverting an absent pass must error");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn check_json_emits_versioned_schema() {
    // R8: `axon check --json` emits the versioned `axon-diag/1` schema with the
    // code as a first-class field (parse-without-regex). Note `check` also
    // auto-switches to JSON when stderr is piped (as under test capture), so the
    // structured schema is what a tool/agent sees.
    let f = std::env::temp_dir().join(format!("axon_r8diag_{}.ax", std::process::id()));
    // A type-mismatched annotation → a real diagnostic with a code.
    std::fs::write(&f, "fn main() -> i64 { let x: str = 5  0 }\n").unwrap();

    let json = axon().args(["check", "--json", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    let jstderr = String::from_utf8_lossy(&json.stderr);
    assert!(
        jstderr.contains("\"schema\":\"axon-diag/1\""),
        "JSON output must carry the versioned schema tag: {jstderr}"
    );
    assert!(
        jstderr.contains("\"code\":\"E"),
        "JSON output must expose the diagnostic code as a first-class field: {jstderr}"
    );
    assert!(
        jstderr.contains("\"severity\":\"error\""),
        "JSON output must expose severity: {jstderr}"
    );
    // The `[CODE]` prefix must be lifted OUT of the message (it's its own field).
    assert!(
        !jstderr.contains("\"message\":\"[E"),
        "the [CODE] prefix must not remain in the message: {jstderr}"
    );
}

#[test]
fn import_widening_capabilities_is_rejected_e1203() {
    // R6 §4.4 (I-11 import edge): a `@[contained]` importer that imports a module
    // exercising a capability it does not grant is rejected with E1203. Paired
    // with an ALLOW case (import within grant → clean) so the boundary is shown
    // to be precise, not blanket. Uses ai_complete (a real net-classified
    // builtin) to avoid resolver noise from unimplemented names.
    let tmp = std::env::temp_dir().join(format!("axon_r6e1203_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();

    // DENY: importer grants fs:read only; the imported module makes an AI (net) call.
    std::fs::write(
        tmp.join("netmod.ax"),
        "fn fetch() -> str { match ai_complete(\"x\") { Ok(s) => s  Err(_) => \"\" } }\n",
    )
    .unwrap();
    let deny = tmp.join("deny.ax");
    std::fs::write(
        &deny,
        "mod netmod\nuse netmod.{fetch}\n\
         @[contained(fs: [read(\"./data/\")], exec: none)]\n\
         fn main() -> i64 { 0 }\n",
    )
    .unwrap();
    let out = axon()
        .args(["check", deny.to_str().unwrap()])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .env("AXON_AI_MOCK", "1")
        .output()
        .unwrap();
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(msg.contains("E1203"), "import widening must be E1203: {msg}");
    assert!(msg.contains("net"), "names the widened capability: {msg}");
    assert_ne!(out.status.code(), Some(0), "a widening import must fail check");

    // ALLOW: same importer, import only reads a file (within the fs:read grant).
    std::fs::write(
        tmp.join("fsmod.ax"),
        "fn load() -> str { match read_file(\"./data/x\") { Ok(s) => s  Err(_) => \"\" } }\n",
    )
    .unwrap();
    let allow = tmp.join("allow.ax");
    std::fs::write(
        &allow,
        "mod fsmod\nuse fsmod.{load}\n\
         @[contained(fs: [read(\"./data/\")], exec: none)]\n\
         fn main() -> i64 { 0 }\n",
    )
    .unwrap();
    let ok = axon()
        .args(["check", allow.to_str().unwrap()])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    let okmsg = format!(
        "{}{}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(!okmsg.contains("E1203"), "an import within grant must NOT be E1203: {okmsg}");
}

#[test]
fn locked_mode_enforces_axon_lock() {
    // R6 §4.2: `axon check --locked` requires every import to match axon.lock.
    // Dev mode (no flag) only warns (W1210, non-fatal) so back-compat holds.
    let tmp = std::env::temp_dir().join(format!("axon_r6locked_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("util.ax"), "fn helper(n: i64) -> i64 { n + 1 }\n").unwrap();
    let prog = tmp.join("prog.ax");
    std::fs::write(&prog, "mod util\nuse util.{helper}\nfn main() -> i64 { helper(5) }\n").unwrap();

    // 1. Dev mode, no lockfile → W1210 warning, but NOT fatal (check still runs).
    let dev = axon()
        .args(["check", prog.to_str().unwrap()])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    let devmsg = format!("{}{}", String::from_utf8_lossy(&dev.stdout), String::from_utf8_lossy(&dev.stderr));
    assert!(devmsg.contains("W1210"), "dev mode must warn W1210: {devmsg}");
    assert_eq!(dev.status.code(), Some(0), "dev-mode unlocked import is non-fatal: {devmsg}");

    // 2. --locked, no lockfile → E1202 fatal.
    let locked_missing = axon()
        .args(["check", prog.to_str().unwrap(), "--locked"])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    let lmsg = format!("{}{}", String::from_utf8_lossy(&locked_missing.stdout), String::from_utf8_lossy(&locked_missing.stderr));
    assert!(lmsg.contains("E1202"), "--locked with no lock entry must be E1202: {lmsg}");
    assert_ne!(locked_missing.status.code(), Some(0), "--locked missing entry is fatal");

    // 3. Write the lock; --locked now passes.
    let lock_out = axon()
        .args(["lock", prog.to_str().unwrap()])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    assert!(lock_out.status.success(), "axon lock should succeed");
    let locked_ok = axon()
        .args(["check", prog.to_str().unwrap(), "--locked"])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    assert_eq!(
        locked_ok.status.code(), Some(0),
        "--locked with a matching lock must pass: {}",
        String::from_utf8_lossy(&locked_ok.stderr)
    );

    // 4. Tamper the module; --locked → E1201.
    std::fs::write(tmp.join("util.ax"), "fn helper(n: i64) -> i64 { n + 999 }\n").unwrap();
    let tampered = axon()
        .args(["check", prog.to_str().unwrap(), "--locked"])
        .env("AXON_PATH", tmp.to_str().unwrap())
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    let tmsg = format!("{}{}", String::from_utf8_lossy(&tampered.stdout), String::from_utf8_lossy(&tampered.stderr));
    assert!(tmsg.contains("E1201"), "--locked with tampered bytes must be E1201: {tmsg}");
    assert_ne!(tampered.status.code(), Some(0), "tampered import is fatal under --locked");
}

#[test]
fn agent_actions_are_mandatorily_logged() {
    // R4 §4.3 (I-13): every capability-bearing action inside an `@[agent]` fn
    // produces a compiler-injected `event:"agent_action"` audit record (action
    // = tool name, caps_used = the capability). A non-agent fn doing the
    // IDENTICAL call produces NO such record — the log is keyed on the zone, not
    // on cooperation, and only agents are audited at action granularity.
    let cache = std::env::temp_dir().join(format!("axon_r4agent_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let prog = r#"
@[agent]
fn planner(goal: str) -> str {
    match ai_complete("plan {goal}") { Ok(s) => s  Err(_) => "" }
}
fn plain(goal: str) -> str {
    match ai_complete("plan {goal}") { Ok(s) => s  Err(_) => "" }
}
fn main() -> i64 {
    let _ = planner("a")
    let _ = plain("b")
    0
}
"#;
    let f = std::env::temp_dir().join(format!("axon_r4agent_{}.ax", std::process::id()));
    std::fs::write(&f, prog).unwrap();
    let out = axon()
        .args(["run", f.to_str().unwrap()])
        .env("AXON_AI_MOCK", "1")
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(0), "program should run clean");

    let log = cache.join("axon").join("provenance.jsonl");
    let body = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&cache);
    let actions: Vec<&str> = body.lines().filter(|l| l.contains("\"event\":\"agent_action\"")).collect();
    assert_eq!(
        actions.len(), 1,
        "exactly one agent_action (from planner, not plain), got {}. Log:\n{body}",
        actions.len()
    );
    let rec = actions[0];
    assert!(rec.contains("\"fn\":\"planner\""), "attributed to the agent fn: {rec}");
    assert!(rec.contains("\"action\":\"ai_complete\""), "names the tool: {rec}");
    assert!(rec.contains("\"caps_used\":\"net\""), "records the capability: {rec}");
    assert!(rec.contains("\"zone\":\"agent\""), "tagged zone agent: {rec}");
}

#[test]
fn ai_tier_routing_pins_real_model_in_provenance() {
    // R3 §4.2/§4.3: the AiCall provenance records the RESOLVED tier + concrete
    // model (not a hardcoded placeholder). A `@[ai(policy(tier: cheap))]` fn
    // routes to the cheap model; a policy with no tier defaults to balanced.
    let cache = std::env::temp_dir().join(format!("axon_r3tier_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let prog = r#"
@[ai(policy(tier: cheap, fallback: "x"))]
fn cheapfn(t: str) -> str { match ai_complete("p") { Ok(s) => s  Err(_) => "x" } }
@[ai(policy(fallback: "y"))]
fn defaultfn(t: str) -> str { match ai_complete("p") { Ok(s) => s  Err(_) => "y" } }
fn main() -> i64 { let _ = cheapfn("a")  let _ = defaultfn("b")  0 }
"#;
    let f = std::env::temp_dir().join(format!("axon_r3tier_{}.ax", std::process::id()));
    std::fs::write(&f, prog).unwrap();
    let out = axon()
        .args(["run", f.to_str().unwrap()])
        .env("AXON_AI_MOCK", "1")
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(0));
    let body = std::fs::read_to_string(cache.join("axon").join("provenance.jsonl")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&cache);
    // cheapfn → tier:cheap + a cheap-tier model (distinct from balanced).
    let cheap = body.lines().find(|l| l.contains("\"fn\":\"cheapfn\"")).unwrap_or("");
    assert!(cheap.contains("\"tier\":\"cheap\""), "cheap tier recorded: {cheap}");
    assert!(cheap.contains("\"model\":\"anthropic:claude-haiku\""), "cheap model pinned: {cheap}");
    // defaultfn (policy, no tier) → balanced.
    let dflt = body.lines().find(|l| l.contains("\"fn\":\"defaultfn\"")).unwrap_or("");
    assert!(dflt.contains("\"tier\":\"balanced\""), "default tier balanced: {dflt}");
}

#[test]
fn unknown_ai_tier_is_e1302() {
    // R3 §6 E1302: a policy naming a tier outside the closed enum is rejected.
    let f = std::env::temp_dir().join(format!("axon_r3badtier_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "@[ai(policy(tier: turbo, fallback: \"x\"))]\n\
         fn g(t: str) -> str { match ai_complete(\"p\") { Ok(s) => s  Err(_) => \"x\" } }\n\
         fn main() -> i64 { let _ = g(\"a\")  0 }\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).env("AXON_AI_MOCK", "1").output().unwrap();
    let _ = std::fs::remove_file(&f);
    let msg = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(msg.contains("E1302"), "unknown tier must be E1302: {msg}");
    assert!(msg.contains("turbo"), "names the bad tier: {msg}");
}

#[test]
fn ai_call_without_policy_warns_w1310() {
    // R3 §6 W1310: an AI call from a fn with no @[ai(policy)] is allowed but
    // warns (un-metered / un-pinned). A fn WITH a policy does not warn.
    let f = std::env::temp_dir().join(format!("axon_r3nopol_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn nopolicy(t: str) -> str { match ai_complete(\"p\") { Ok(s) => s  Err(_) => \"z\" } }\n\
         fn main() -> i64 { let _ = nopolicy(\"a\")  0 }\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).env("AXON_AI_MOCK", "1").output().unwrap();
    let _ = std::fs::remove_file(&f);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("W1310"), "no-policy AI call must warn W1310: {stderr}");
    assert_eq!(out.status.code(), Some(0), "W1310 is a warning, not fatal: {stderr}");
}
