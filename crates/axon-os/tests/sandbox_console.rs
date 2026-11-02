//! The sandbox wrapper's effect tags (IG, integrator decision 2026-10-31).
//!
//! `wrap_in_sandbox` used to emit the `IO` tag only when fs_read, fs_write or
//! exec was granted, so a job that only prints was admitted with an empty
//! grant and then refused at run time ("builtin `println` requires effect `IO`
//! which is not in the active sandbox's allowed set {}", exit 8). Console
//! output and `exit` are not capabilities the admission gate grants. The
//! filesystem stays scoped by the allowlists, exec by its own `Exec` tag, and
//! an environment read by the fs_read grant (the axis admission projects
//! `env_var` onto).

#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use axon_os::grant::{Budget, ExecPolicy, Grant, Label};
use axon_os::runtime::{AxonCoreRuntime, PrincipalHandle, ProgramSource, Runtime};
use axon_os::verdict::Verdict;
use std::path::PathBuf;

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

fn grant(fs_read: &[&str], fs_write: &[&str]) -> Grant {
    Grant {
        reproducible: false,
        fs_read: fs_read.iter().map(|s| s.to_string()).collect(),
        fs_write: fs_write.iter().map(|s| s.to_string()).collect(),
        net: vec![],
        exec: ExecPolicy::None,
        max_label: Label::Internal,
        budget: Budget {
            calls: 10,
            tokens: 10,
            cost_micro: 10,
        },
    }
}

fn run(tag: &str, src: &str, g: &Grant) -> Verdict {
    let dir = std::env::temp_dir().join(format!("axon-os-console-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("out")).unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("data/in.txt"), "x").unwrap();
    let path = dir.join("j.ax");
    std::fs::write(&path, src).unwrap();
    let rt = AxonCoreRuntime::with_bin_and_timeout(axon_bin(), std::time::Duration::from_secs(60));
    let out = rt.run_sandboxed(
        &ProgramSource {
            path: &path,
            text: src,
        },
        &PrincipalHandle(0),
        g,
        &g.budget,
        1,
        None,
    );
    let _ = std::fs::remove_dir_all(&dir);
    out.verdict
}

/// [`run`], plus whether the job left `out/leak.txt` behind.
fn run_leak(tag: &str, src: &str, g: &Grant) -> (Verdict, bool) {
    let dir = std::env::temp_dir().join(format!("axon-os-console-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("out")).unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    let path = dir.join("j.ax");
    std::fs::write(&path, src).unwrap();
    let rt = AxonCoreRuntime::with_bin_and_timeout(axon_bin(), std::time::Duration::from_secs(60));
    let out = rt.run_sandboxed(
        &ProgramSource {
            path: &path,
            text: src,
        },
        &PrincipalHandle(0),
        g,
        &g.budget,
        1,
        None,
    );
    let leaked = dir.join("out/leak.txt").exists();
    let _ = std::fs::remove_dir_all(&dir);
    (out.verdict, leaked)
}

fn is_sandbox_denial(v: &Verdict) -> bool {
    matches!(v, Verdict::Denied { axis, .. } if axis == "sandbox")
}

#[test]
fn console_output_runs_under_an_empty_grant() {
    // ATTACK: println-only job refused under an empty grant
    let v = run(
        "println",
        "fn main() -> i64 {\n    println(\"hello\")\n    eprintln(\"b\")\n    0\n}\n",
        &grant(&[], &[]),
    );
    assert_eq!(
        v,
        Verdict::Completed { value: 0 },
        "ATTACK: println-only job refused under an empty grant: {v:?}"
    );
}

#[test]
fn a_job_whose_last_output_has_no_newline_still_completes() {
    // ATTACK: a job whose output ends without a newline is not completed
    let v = run(
        "print",
        "fn main() -> i64 {\n    print(\"a\")\n    0\n}\n",
        &grant(&[], &[]),
    );
    assert_eq!(
        v,
        Verdict::Completed { value: 0 },
        "ATTACK: a job whose output ends without a newline was not completed: {v:?}"
    );
}

#[test]
fn an_environment_read_needs_the_fs_read_grant() {
    // ATTACK: env_var readable without an fs_read grant
    let src = "fn main() -> i64 {\n    match env_var(\"PATH\") {\n        Ok(_) => 3\n        Err(_) => 4\n    }\n}\n";
    for g in [grant(&[], &[]), grant(&[], &["./out/"])] {
        let v = run("env-denied", src, &g);
        assert!(
            is_sandbox_denial(&v),
            "ATTACK: env_var readable without an fs_read grant ({g:?}): {v:?}"
        );
    }
    // With a read grant the environment is readable (admission's projection).
    let v = run("env-allowed", src, &grant(&["./data/"], &[]));
    assert_eq!(v, Verdict::Completed { value: 3 }, "{v:?}");
}

#[test]
fn console_access_opens_no_filesystem_or_process() {
    // ATTACK: the always-on IO tag opened the filesystem or exec
    let cases = [
        (
            "read",
            "fn main() -> i64 {\n    match read_file(\"./data/in.txt\") {\n        Ok(_) => 3\n        Err(_) => 4\n    }\n}\n",
        ),
        (
            "write",
            "fn main() -> i64 {\n    match write_file(\"./out/x.txt\", \"y\") {\n        Ok(_) => 3\n        Err(_) => 4\n    }\n}\n",
        ),
        (
            "exists",
            "fn main() -> i64 {\n    if file_exists(\"./data/in.txt\") { 3 } else { 4 }\n}\n",
        ),
        (
            "exec",
            "fn main() -> i64 {\n    match exec(\"true\", []) {\n        Ok(_) => 3\n        Err(_) => 4\n    }\n}\n",
        ),
    ];
    for (tag, src) in cases {
        let v = run(tag, src, &grant(&[], &[]));
        assert!(
            is_sandbox_denial(&v),
            "ATTACK: the always-on IO tag opened the filesystem or exec ({tag}): {v:?}"
        );
    }
}

// ---- B3-7: a module-level `let` initialiser runs inside the grant ----------

#[test]
fn a_job_module_level_let_cannot_read_outside_the_grant() {
    let src = "fn early() -> i64 {\n    let s = match read_file(\"/etc/passwd\") {\n        Ok(s) => \"LEAK \" + s,\n        Err(e) => \"refused \" + e,\n    }\n    match write_file(\"./out/leak.txt\", s) {\n        Ok(_) => 0,\n        Err(e) => 1,\n    }\n}\nlet X = early()\nfn main() -> i64 {\n    0\n}\n";
    let (v, leaked) = run_leak("b3-modlet", src, &grant(&["./data/"], &["./out/"]));
    assert!(
        is_sandbox_denial(&v) && !leaked,
        "ATTACK: a module-level let read outside the grant (leaked: {leaked}): {v:?}"
    );
}

#[test]
fn a_job_module_level_let_cannot_use_time() {
    // `Time` is never in a job's tags (`wrap_in_sandbox`).
    let src = "let T = now_ms()\nfn main() -> i64 {\n    println(\"ml t ok\")\n    0\n}\n";
    let v = run("b3-modlet-time", src, &grant(&["./data/"], &["./out/"]));
    assert!(
        is_sandbox_denial(&v),
        "ATTACK: a module-level let used an effect the grant withholds: {v:?}"
    );
}

// ---- B3-3: completion is the record's class, not a stdout token -----------

fn is_runtime_denial(v: &Verdict) -> bool {
    matches!(v, Verdict::Denied { axis, .. } if axis == "runtime")
}

/// O-GV-06: with fs_read on `/proc/`, a job read the nonce from the kernel's
/// `/proc/self/environ` and the staged wrapper (with its completion marker)
/// through `/proc/self/cmdline` + `/proc/self/root`, printed every marker it
/// found, then stopped itself with `exit(3)`: sealed `Completed { value: 3 }`.
#[test]
fn a_job_forging_completion_through_proc_is_denied() {
    let src = "fn main() -> i64 {\n\
    let env = match read_file(\"/proc/self/environ\") {\n        Ok(s) => s,\n        Err(e) => \"\",\n    }\n\
    let parts = str_split(env, \"AXON_OUTCOME_NONCE=\")\n\
    if len(parts) > 1 {\n        println(\"__axon_os_done:\" + str_slice(parts[1], 0, 32))\n    }\n\
    let cl = match read_file(\"/proc/self/cmdline\") {\n        Ok(s) => s,\n        Err(e) => \"\",\n    }\n\
    for p in str_split(cl, chr(0)) {\n\
        if str_ends_with(p, \".ax\") {\n\
            let w = match read_file(\"/proc/self/root\" + p) {\n                Ok(s) => s,\n                Err(e) => \"\",\n            }\n\
            for l in str_split(w, \"\\n\") {\n\
                if str_contains(l, \"__axon_os_done:\") {\n\
                    let q = str_split(l, \"\\\"\")\n\
                    if len(q) > 1 {\n                        println(q[1])\n                    }\n\
                }\n\
            }\n\
        }\n\
    }\n\
    exit(3)\n    0\n}\n";
    let v = run("b3-forge", src, &grant(&["./data/", "/proc/"], &["./out/"]));
    assert!(
        is_runtime_denial(&v),
        "ATTACK: a job's own exit inside its sandbox was sealed as completed: {v:?}"
    );
}

/// AUDIT T44: a job that calls `exit(n)` itself stopped before the wrapper
/// finished; that is not a completion.
#[test]
fn a_job_that_calls_exit_itself_is_not_completed() {
    let v = run(
        "b3-exit",
        "fn main() -> i64 {\n    println(\"x\")\n    exit(3)\n    0\n}\n",
        &grant(&[], &[]),
    );
    assert!(
        is_runtime_denial(&v),
        "ATTACK: a job that stopped itself was sealed as completed: {v:?}"
    );
}

/// A module-level `let` initialiser runs before `main` (and so before the
/// wrapper's `sandbox_run`); an `exit` there is not a completion either.
#[test]
fn a_job_exiting_in_a_module_level_let_is_not_completed() {
    let v = run(
        "b3-modlet-exit",
        "fn early() -> i64 {\n    println(\"early ran\")\n    exit(0)\n    0\n}\nlet X = early()\nfn main() -> i64 {\n    println(\"main ran\")\n    5\n}\n",
        &grant(&[], &[]),
    );
    assert!(
        is_runtime_denial(&v),
        "ATTACK: an exit before main was sealed as completed: {v:?}"
    );
}
