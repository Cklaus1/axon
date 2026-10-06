//! Process-control guard: Rust code must not shell out to a `kill` binary.
//!
//! `axon-intent` did, as `Command::new("kill").arg("-KILL").arg(format!("-{pid}"))`,
//! intending "SIGKILL process group <pid>". procps `kill` parses a leading-dash
//! operand as another OPTION and turns its digits into a signal number, which it
//! then uses as the target. Traced on the dev host:
//!
//! ```text
//! /usr/bin/kill -CONT -5881      -> kill(-5, SIGCONT)
//! /usr/bin/kill -CONT -12345     -> kill(-1, SIGCONT)      every process
//! /usr/bin/kill -CONT -- -5881   -> kill(-5881, SIGCONT)   what was meant
//! ```
//!
//! As root under WSL that was `kill(-1, SIGKILL)`: it killed systemd's services
//! and every terminal, twice, and was first misread as memory pressure. The fix
//! uses `libc::killpg`, where the group id is a typed integer.
//!
//! A text search is the right tool here because the defect is a CHOICE OF API,
//! visible at the call site. The guard fails on the call, not on prose, so a
//! comment that explains the bug does not trip it.
//!
//! Shell scripts are deliberately NOT covered: `kill` there is bash's BUILTIN,
//! which parses `kill -KILL "-$pgid"` correctly (verified with strace:
//! `kill(-987654, SIGCONT)`). The hazard is the external binary, which a Rust
//! `Command` always reaches.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if p.is_dir() {
            if name == "target" || name.starts_with('.') {
                continue;
            }
            rust_sources(&p, out);
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

/// Strip `//` line comments (and the rest of the line) so prose that NAMES the
/// bad call does not count as making it. String literals containing `//` are
/// not a concern for the patterns checked here.
fn code_only(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// Collapse whitespace so `Command::new( "kill" )` and `Command::new(\n"kill")`
/// written across a line are still caught when joined.
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join("")
}

const EXEMPT: &[&str] = &[
    // This file quotes the forbidden call in its own documentation and tests.
    "crates/axon-core/tests/process_control_guard.rs",
];

#[test]
fn no_rust_code_shells_out_to_a_kill_binary() {
    let root = root();
    let mut files = Vec::new();
    rust_sources(&root.join("crates"), &mut files);
    assert!(
        files.len() > 100,
        "scanned only {} Rust files — the walk is broken and this guard is \
         verifying nothing",
        files.len()
    );

    let forbidden = [
        r#"Command::new("kill")"#,
        r#"Command::new("/bin/kill")"#,
        r#"Command::new("/usr/bin/kill")"#,
        r#"Command::new("pkill")"#,
        r#"Command::new("killall")"#,
    ];
    let mut hits = Vec::new();
    for f in &files {
        let rel = f
            .strip_prefix(&root)
            .unwrap_or(f)
            .to_string_lossy()
            .to_string();
        if EXEMPT.contains(&rel.as_str()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        let code: String = text.lines().map(code_only).collect::<Vec<_>>().join("\n");
        let squashed = squash(&code);
        for pat in forbidden {
            if squashed.contains(pat) {
                hits.push(format!("{rel}: {pat}"));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "Rust code shells out to a kill binary. procps `kill` misparses a \
         negative pid (`kill -KILL -5881` became `kill(-5, SIGKILL)`, and many \
         pids became `kill(-1, SIGKILL)`: every process). Use `libc::killpg` \
         or `libc::kill` with a typed pid. If a real exception exists, add it \
         to EXEMPT with the reason.\n  {}",
        hits.join("\n  ")
    );
}

/// The guard must actually catch the construct it exists for. Checked against
/// the exact text that shipped, plus a reformatted variant, so a guard that
/// silently stopped matching would fail here rather than pass everything.
#[test]
fn the_guard_recognises_the_call_that_shipped() {
    let shipped = r#"    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(format!("-{pid}"))"#;
    let reformatted = "let _ = Command::new(\n    \"kill\"\n).arg(\"-9\");";
    let commented = r#"    // The previous `Command::new("kill")` form was misparsed."#;
    let scan = |t: &str| {
        let code: String = t.lines().map(code_only).collect::<Vec<_>>().join("\n");
        squash(&code).contains(r#"Command::new("kill")"#)
    };
    assert!(scan(shipped), "the shipped call must be caught");
    assert!(scan(reformatted), "a reformatted call must be caught");
    assert!(
        !scan(commented),
        "prose that NAMES the call must not be caught"
    );
}
