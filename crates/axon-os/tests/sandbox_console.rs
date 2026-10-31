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
    // ATTACK: completion marker lost after an unterminated print
    let v = run(
        "print",
        "fn main() -> i64 {\n    print(\"a\")\n    0\n}\n",
        &grant(&[], &[]),
    );
    assert_eq!(
        v,
        Verdict::Completed { value: 0 },
        "ATTACK: completion marker lost after an unterminated print: {v:?}"
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
